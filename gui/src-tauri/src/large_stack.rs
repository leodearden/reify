//! Run engine and compiler work on OS threads with an explicit LARGE stack.
//!
//! Defense-in-depth (task 5357): deeply-nested geometry can drive
//! `reify_compiler`'s recursive compile past the ~2 MiB stack of a tokio worker
//! or a default `std` thread, overflowing it and aborting the process. Running
//! that work on a [`COMPILE_STACK_SIZE`] stack gives extra headroom on top of
//! task 5337's compiler-layer `stacker::maybe_grow` growth and recursion-depth
//! cap.
//!
//! A plain `std` thread is also strictly SAFER for the real OCCT kernel:
//! `OcctKernelHandle::execute()` uses `blocking_send`, which panics inside any
//! tokio runtime context, and a plain `std` thread is never one.
//!
//! # Two tiers, and what each one covers
//!
//! The tiers differ in the LIFETIME of the 256 MiB mapping, not in its size. A
//! 256 MiB stack is far above glibc's ~40 MiB thread-stack cache ceiling, so it
//! is never recycled: a per-call spawn pays a fresh `mmap` + guard-page
//! `mprotect` + `munmap` every time. Negligible against a full compile, pure
//! overhead per slider-drag frame or per keystroke — which is what makes the
//! persistent lanes a separate tier rather than a nicety.
//!
//! **Per-call — [`spawn_on_large_stack`].** A fresh thread per job, for work
//! that does not go through a lane: `debug_server::run_on_engine`'s debug/MCP
//! engine closures, which still bypass the evaluation queue
//! (tkt_0RV0J0HK8TK4WRS6YJVYEFP93C), and a job that [`post`] or
//! [`dispatch_async`] got back from a dead lane.
//!
//! **Persistent lanes — [`ENGINE_LANE`], [`LSP_LANE`] and [`LSP_POOL`].** A
//! lane's threads live for the process lifetime; the per-call cost becomes a
//! queue push. Most lanes run ONE consumer; [`LSP_POOL`] runs
//! [`LSP_POOL_SIZE`], which is a size on the same mechanism and not a second
//! one — see [`Lane`]'s "One consumer or N".
//!
//! * ENGINE lane — fed ONLY by [`post_to_worker`], whose sole production caller
//!   is [`crate::eval_queue::EvalQueue`]. Its roster is therefore everything that
//!   submits to that queue. Posting never waits for the job, so no submitter —
//!   least of all the GTK main thread — parks on engine work.
//! * LSP lanes — `main.rs::lsp_request` → `lsp_bridge::lsp_request_on_worker`,
//!   which fires on effectively every keystroke and cursor move.
//!   [`crate::lsp_bridge::lane_for_method`] routes each method to either the
//!   single-consumer ORDERED lane [`LSP_LANE`] (state-mutating + lifecycle
//!   methods, plus anything unrecognised) or the QUERY POOL [`LSP_POOL`] (the
//!   eight read-only queries), and both are submitted through
//!   [`dispatch_async`]. They carry a FUTURE rather than a closure: their
//!   degraded arms run in the submitting async frame, where a future can be
//!   `.await`ed but a closure with a [`tokio::runtime::Handle::block_on`] baked
//!   in would panic "Cannot start a runtime from within a runtime".
//!
//! So every engine-bearing path runs on a large stack: queued work on the ENGINE
//! lane, the debug server on per-call threads.
//!
//! # What is still NOT covered
//!
//! Four boundaries, stated as limits rather than left to be inferred. All four
//! are LSP-side: the engine surface is covered. Items 2 and 3 were OPEN at task
//! 5772 and are now bounded rather than unbounded (task 6517); they are
//! restated as the narrower limits that actually hold, not deleted, because a
//! limit that stopped being total did not stop existing. Item 4 is the reverse
//! case — a limit item 2's bound does NOT reach.
//!
//! Each item states its LIMIT and points at the one place that argues it. The
//! arguments are not repeated here: [`Lane`] owns why the lanes are split and
//! what the split gives up, [`OnAbandon`] owns the discard policy,
//! [`dispatch_async`] owns the cancellation mechanism, and
//! [`crate::lsp_bridge::lane_for_method`] owns which methods go where.
//!
//! 1. **Four LSP methods do not get the large stack.**
//!    `InProcessLsp::handle_request`'s `textDocument/definition`,
//!    `prepareRename`, `rename` and `references` arms each call
//!    `tokio::task::spawn_blocking`, so their compiler work executes on tokio's
//!    BLOCKING POOL, whose threads take the std ~2 MiB default (nothing under
//!    `gui/src-tauri` sets `thread_stack_size`). Putting `handle_request` on a
//!    lane gives the big stack only to that thread's OWN frames, so no lane —
//!    ordered or pooled — can help those four. Closing them needs a change in
//!    `crates/reify-lsp/src/server.rs`, which is outside this module and would
//!    also regress the stdio `reify lsp` CLI server (it relies on
//!    `spawn_blocking` to keep its 2-worker runtime responsive). Tracked as
//!    task #6195. NOTE this is a STACK limit only: their separate cost — holding
//!    a consumer for a workspace-wide walk while gaining nothing from it — is
//!    what item 2 now bounds, and the two were previously narrated as one.
//! 2. **Concurrency WITHIN a lane is BOUNDED, not unlimited.** LSP work runs on
//!    TWO lanes — the single-consumer ORDERED [`LSP_LANE`] and the
//!    [`LSP_POOL_SIZE`]-consumer [`LSP_POOL`] — so head-of-line blocking among
//!    queries is bounded at [`LSP_POOL_SIZE`] (the fifth simultaneous in-flight
//!    query queues) rather than total, as it was when one consumer served all
//!    of LSP. Notifications still serialize against each other, which is a
//!    REQUIREMENT rather than a residual limit. That bound is on CONSUMERS;
//!    item 4 is the tighter one it does not reach. See [`Lane`].
//! 3. **Drop-cancellation is PARTIAL, and on today's production path it is
//!    UNREACHABLE.** A job abandoned before a consumer picks it up is dropped
//!    unrun — but only on an [`OnAbandon::Discard`] destination ([`LSP_POOL`]).
//!    One already picked up runs to completion, now costing one of
//!    [`LSP_POOL_SIZE`] consumers rather than the only LSP consumer in the
//!    process. And an abandoned frontend `invoke` does not drop the command
//!    future in `tauri` 2.11.2, so the only trigger reachable in the shipped app
//!    is runtime/app teardown: a structural guarantee, not a live saving. Both
//!    limits are measured in [`dispatch_async`]'s "Drop-cancellation" section.
//! 4. **[`LSP_POOL_SIZE`] bounds CROSS-document query concurrency more tightly
//!    than SAME-document concurrency.** Item 2's bound is on CONSUMERS, not on
//!    parses, and the difference bites in the commonest case rather than an edge
//!    one — a single cursor move issues hover, `documentHighlight` and
//!    completion against ONE uri. Measured in `crates/reify-lsp/src/document.rs`:
//!    `DocumentState::parsed_module` holds a `std::sync::Mutex` across the whole
//!    `parse_with_stdlib` call, and `DocumentStore::update` replaces the entire
//!    `DocumentState` — its parse cache included — on every `didChange`. So
//!    immediately after each keystroke that document's cache is COLD, and the
//!    pool consumers that reach it concurrently serialize on a blocking lock,
//!    each holding one 256 MiB consumer while parked in it. The effective depth
//!    against a same-document burst is therefore nearer ONE parse than four-way
//!    concurrency; against DIFFERENT documents item 2's bound holds exactly.
//!    Neither a regression (before task 6517 all LSP work serialized anyway) nor
//!    unsoundness (the `Mutex` recovers poisoning), and not closable from this
//!    crate: the fix is to compute the parse OUTSIDE the lock and install it
//!    afterwards — a double parse under a race, no serialization — which is a
//!    change in `reify-lsp`, tracked as task #7272.
//!
//! # The degradation invariant
//!
//! Every submission that cannot get its large stack still RESOLVES, and no
//! degraded arm needs a resource that the condition triggering it would have
//! denied:
//!
//! * No lane (the OS refused the 256 MiB mapping): [`post`] runs the job on a
//!   spawned DEFAULT-stack thread, and [`dispatch_async`] `.await`s the future
//!   natively — neither asks for a second 256 MiB mapping, which would be
//!   refused the same way. A posted job never runs inline on its poster, which
//!   may be a tokio worker where OCCT's synchronous handle panics.
//! * Dead lane (its consumer is gone, so the queue hands the job back unrun): the
//!   job goes to [`spawn_on_large_stack`]. Its trigger is a dead lane rather than
//!   a refused mapping, so asking for a thread is not circular. If even that
//!   spawn fails, [`post`] reports `Err` to its poster and [`dispatch_async`]'s
//!   awaiter gets a loud panic.
//!
//! RE-ENTRANT submission is the one shape that would not resolve: a job that
//! WAITS on work it queued to the lane it is itself running on wedges that lane
//! and every later submitter. Only [`dispatch_async`] waits, and it rejects that
//! shape through [`assert_not_reentrant`] — the panic fires inside the running
//! job, is caught by that job's own `catch_unwind` and re-raised on its
//! submitter, so the lane survives. Posting never waits, so a job may post to
//! its own lane.
//!
//! The worst outcome anywhere in this module is therefore a loud panic or an
//! `Err` — never a silent hang, and never a nested runtime.
//!
//! ## Measured: a runtime torn down under a lane job does NOT panic the job
//!
//! Task 6517's brief predicted a shutdown edge — that a lane job outliving its
//! runtime would hit an `unwrap`/`expect` on a `spawn_blocking` `JoinHandle` in
//! `reify-lsp` and turn into a job panic. It was checked against the source and
//! is REFUTED. In `crates/reify-lsp/src/server.rs`, each of
//! `ReifyLanguageServer`'s `goto_definition`, `prepare_rename`, `rename` and
//! `references` handles its `JoinError` with `tracing::error!` plus `None`.
//! There is no `unwrap` or `expect` on any of those `JoinHandle`s — the only
//! `unwrap`-family calls in those four bodies are `goto_definition`'s and
//! `prepare_rename`'s `stdlib_path.unwrap_or_else(..)` on an
//! `Option<PathBuf>`, both INSIDE a blocking closure and neither fallible. So
//! the observed behaviour is: those arms log and answer `None`, and the
//! predicted job panic does not occur.
//!
//! Cited by SYMBOL rather than by line, deliberately: these citations are what
//! makes this section a measurement rather than a reassurance, and a line number
//! in a file this crate does not own is invalidated by any `use` added above it,
//! leaving the claim reading as verified when it is not. It is true of
//! `reify-lsp` as of task 6517, and a future change there could make it false
//! without anything here noticing.

/// Stack size for the large-stack compile thread: 256 MiB.
///
/// A thread stack is a *virtual-address reservation*, committed lazily
/// page-by-page on first touch — so 256 MiB costs only the pages actually used
/// (small RSS), not 256 MiB resident. That is ~128x the compiler worker's 2 MiB
/// default, a generous margin for pathological geometry nesting as
/// belt-and-suspenders atop task 5337's `stacker::maybe_grow` growth and
/// recursion cap. It is the single source of truth for every thread this module
/// spawns.
///
/// # Per-call cost (why high-frequency work needs a persistent lane)
///
/// A 256 MiB stack is far above glibc's thread-stack cache ceiling (~40 MiB
/// total by default), so such a stack is never recycled: every call pays a fresh
/// `mmap` + guard-page `mprotect` + `munmap` and the matching page-table
/// teardown (tens of microseconds), and churns 256 MiB of address space. That is
/// negligible next to an actual compile, but it is pure overhead on a
/// high-frequency path — hence the two tiers in the module docs.
pub const COMPILE_STACK_SIZE: usize = 256 * 1024 * 1024;

/// Thread name for [`spawn_on_large_stack`]'s per-call engine thread, and for
/// the default-stack thread [`post`] falls back to when there is no lane.
///
/// Named so panic backtraces, `RUST_BACKTRACE` dumps, `top -H` / `perf` rows and
/// debugger thread lists identify the engine work instead of reading
/// `<unnamed>` — this module relocates exactly the work most likely to crash, so
/// losing the caller's thread identity would be an observability regression.
///
/// Kept under 15 bytes: Linux `pthread_setname_np` caps names at 15 chars + NUL
/// and `std` silently ignores the failure, so a longer name would just not show
/// up in `/proc`.
pub const ENGINE_THREAD_NAME: &str = "reify-engine";
const _: () = assert!(
    ENGINE_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for the persistent ENGINE lane — [`ENGINE_LANE`]'s thread.
///
/// Distinct from the per-call name so a backtrace or profiler row says which
/// TIER the work arrived on, not just that it is large-stack work. This is the
/// thread most worth naming: it is long-lived, so unlike the per-call threads it
/// shows up in every profiler capture, `top -H` listing and debugger thread list
/// for the process's whole life. Same 15-byte budget as [`ENGINE_THREAD_NAME`].
pub const WORKER_THREAD_NAME: &str = "reify-engine-w";
const _: () = assert!(
    WORKER_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for the persistent LSP lane — [`LSP_LANE`]'s thread.
///
/// A SECOND long-lived thread earns a second name for the same reason the first
/// did, and more sharply: the two lanes exist precisely so a keystroke-frequency
/// stall and a geometry-evaluation stall are different events, and a shared name
/// would make them indistinguishable in exactly the capture where telling them
/// apart matters. Same 15-byte budget as [`ENGINE_THREAD_NAME`].
pub const LSP_WORKER_THREAD_NAME: &str = "reify-lsp-w";
const _: () = assert!(
    LSP_WORKER_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread-name PREFIX for the LSP QUERY POOL — [`LSP_POOL`]'s consumers, named
/// `reify-lsp-p0` .. `reify-lsp-p{LSP_POOL_SIZE-1}` (task 6517).
///
/// A prefix rather than a name, because a pool has more than one thread to tell
/// apart: `top -H` and profiler captures should say WHICH query consumer is
/// stalled, not merely that one is. The `-p` / `-w` distinction from
/// [`LSP_WORKER_THREAD_NAME`] is the load-bearing part — the whole point of the
/// split is that a stall on the ordered notification lane and a stall on a query
/// consumer are different events with different causes.
///
/// The budget assertion allows for a TWO-digit index rather than only the widths
/// [`LSP_POOL_SIZE`] currently reaches, so raising the pool size later cannot
/// silently overrun Linux's 15-byte `pthread_setname_np` limit — `std` ignores
/// an over-long name without erroring, so the failure would be an invisible loss
/// of thread identity rather than a build break.
pub const LSP_POOL_THREAD_PREFIX: &str = "reify-lsp-p";
const _: () = assert!(
    LSP_POOL_THREAD_PREFIX.len() + 2 <= 15,
    "the pool prefix plus a two-digit consumer index must fit Linux's 15-byte \
     pthread_setname_np limit"
);

/// How many consumers the LSP query pool runs: 4.
///
/// # Why 4, and why a fixed constant
///
/// The bound this buys is "the FIFTH simultaneous in-flight query queues", and 4
/// covers the realistic worst case: one slow workspace-wide `references` or
/// `rename` overlapping the hover / completion / documentHighlight / definition
/// traffic that a single cursor move already produces. Below 4 the common case
/// can still block on one slow query; above it, the extra consumers would idle
/// through every workload this GUI generates.
///
/// It is a FIXED constant rather than [`std::thread::available_parallelism`] on
/// purpose. The bound is then the same number on every machine — directly
/// assertable from a test, and the same number in a bug report as in the code —
/// whereas a machine-derived size would make head-of-line behaviour depend on
/// the reporter's core count.
///
/// The cost is four LAZILY-created virtual 256 MiB reservations committed
/// page-by-page (see [`COMPILE_STACK_SIZE`]), not 1 GiB resident and not
/// anything at all in a session that never issues an LSP query.
pub(crate) const LSP_POOL_SIZE: usize = 4;

/// Spawn `f` on a dedicated OS thread with a [`COMPILE_STACK_SIZE`] stack WITHOUT
/// blocking the caller, returning the [`std::thread::JoinHandle`].
///
/// The per-call tier, for work that does not go through a lane — notably
/// `debug_server::run_on_engine`, which delivers its result out-of-band via a
/// `tokio::sync::oneshot` channel. Because `f` outlives this call, it is
/// `'static` (no borrowing of caller-stack data); deliver any result through a
/// channel captured by `f`.
///
/// The `io::Result` surfaces OS thread-creation failure to the caller
/// (`Builder::spawn` returns a `Result`, whereas `thread::spawn` panics), so an
/// async caller can map it to a structured error. There is no inline fallback:
/// the closure is `'static` and the caller must not block its runtime worker.
///
/// The thread is named [`ENGINE_THREAD_NAME`] so backtraces and profiler rows
/// identify it.
pub fn spawn_on_large_stack<F>(f: F) -> std::io::Result<std::thread::JoinHandle<()>>
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name(ENGINE_THREAD_NAME.to_string())
        .stack_size(COMPILE_STACK_SIZE)
        .spawn(f)
}

// ── Persistent large-stack worker (task 5772) ────────────────────────────────

/// A type-erased unit of work queued to a persistent lane.
///
/// `'static` because the queue outlives every submitter, so a job can never
/// borrow submitter-stack data. The erased signature is `FnOnce()` regardless of
/// what the work produces: any result travels over a channel captured INSIDE the
/// job, not through this type.
///
/// `pub(crate)` so the in-crate tests can name it when building a SYNTHETIC
/// queue to provoke the `SendError` arm; it adds no public API surface.
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;

/// What a job sends back to its submitter: the computed value, or the panic
/// payload its body raised.
///
/// Carrying the payload rather than a flattened string is what lets the
/// submitter [`std::panic::resume_unwind`] the ORIGINAL panic, exactly as if
/// the work had run inline.
type JobReply<T> = Result<T, Box<dyn std::any::Any + Send>>;

/// What a DESTINATION does with a queued job whose awaiting side has already
/// gone away (task 6517).
///
/// # Why this is a property of the destination, not of `dispatch_async`
///
/// Cancel-at-the-lane was first written as a blanket rule — every job whose
/// `reply_tx` is closed is discarded, on every lane. That is wrong for a
/// destination carrying STATE-MUTATING work, and unrecoverably so: discarding a
/// queued `textDocument/didOpen` means `InProcessLsp` never learns the document
/// exists, after which `ReifyLanguageServer::did_change` takes its `didChange
/// for unknown URI` branch and returns without applying anything, and every
/// query handler — `hover`, `goto_definition`, `completion`, `document_symbol`,
/// `document_highlight`, `prepare_rename`, `rename`, `references` — returns
/// `Ok(None)` from its `documents.get(&uri)` miss arm. The file stays
/// permanently dark to hover/completion/diagnostics until it is closed and
/// reopened — a silent, unbounded loss produced by an optimisation whose entire
/// benefit is skipping work nobody is waiting for.
///
/// The parity argument that licensed the blanket rule does not survive contact
/// with that: pre-task-5772 drop-cancellation could only take effect at an
/// `.await` point AFTER the handler had begun, whereas this check drops the
/// whole notification while it is still QUEUED behind a busy consumer. That is
/// a strictly wider window for a state-mutating message, not the same one.
///
/// So the policy travels with the queue the work was routed to. Only a
/// destination that [`crate::lsp_bridge::lane_for_method`] has already
/// classified as concurrency-safe — read-only, no server-side effect — may
/// discard, and the classification is made exactly once, where the LSP semantics
/// are known.
///
/// # Why [`OnAbandon::Run`] is the DEFAULT
///
/// [`Lane::new`] and [`Lane::pool`] both declare `Run`, and the discarding
/// policy needs the distinctly-named [`Lane::cancelling_pool`]. Spelled the
/// other way round — discard by default, opt out for the ordered lane — a lane
/// added later would silently acquire the state-losing behaviour by virtue of
/// nobody having thought about it. That is the same structural-safe-direction
/// rule [`crate::lsp_bridge::lane_for_method`] applies to method
/// classification, applied to destinations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OnAbandon {
    /// RUN the job anyway. The answer is dropped (its `oneshot` receiver is
    /// already gone), but every side effect the job would have had still
    /// happens. Correct for any destination carrying state-mutating work, and
    /// the conservative default for a destination nobody has classified.
    Run,
    /// DISCARD the job unrun, dropping its future without polling it. Correct
    /// only where running it and not running it are indistinguishable to
    /// everything except the caller that stopped listening.
    Discard,
}

/// The submit end of a lane's job queue, TAGGED with the lane it feeds.
///
/// The [`std::sync::Mutex`] is defensive rather than required —
/// `mpsc::Sender<T>` is `Sync` on current `std`, but nothing here needs to
/// depend on that. The lock is held only across a `send` of an already-boxed
/// job, so no user code ever runs under it and it is never held longer than a
/// queue push.
///
/// The `lane` tag exists for the reentrancy guard, and is what makes that guard
/// PRECISE rather than blanket: it lets [`assert_not_reentrant`] distinguish
/// "submitting to the lane whose thread I am running on" (a permanent wedge for
/// a waiting submission) from "submitting to the OTHER lane" (perfectly legal: a
/// different thread, which drains independently).
///
/// `pub(crate)` for the same reason as [`Job`], and additionally because it is
/// the parameter type of the [`post`] / [`dispatch_async`] seams — the latter
/// being what `lsp_bridge` composes against.
pub(crate) struct JobSender {
    /// Which lane this queue feeds — the same `&'static str` that lane's thread
    /// publishes in [`CURRENT_LANE`].
    lane: &'static str,
    /// What this destination does with a job whose awaiting side is already
    /// gone. Copied from the [`Lane`] that built this sender, so the policy
    /// travels with the queue rather than with the submission — see
    /// [`OnAbandon`].
    on_abandon: OnAbandon,
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
}

impl JobSender {
    /// Wrap a lane's `Sender`, tagging it with that lane's name and its
    /// abandoned-job policy.
    ///
    /// `pub(crate)` so a test can build a SYNTHETIC sender (typically over an
    /// already-dropped `Receiver`) to provoke the `SendError` arms of
    /// [`post`] / [`dispatch_async`] deterministically.
    ///
    /// `on_abandon` is a required argument rather than a defaulted field
    /// precisely because a synthetic sender is how the cancel path is tested:
    /// a test that meant to exercise one policy and silently got the other
    /// would assert the wrong thing and stay green.
    pub(crate) fn new(
        lane: &'static str,
        tx: std::sync::mpsc::Sender<Job>,
        on_abandon: OnAbandon,
    ) -> Self {
        Self {
            lane,
            on_abandon,
            tx: std::sync::Mutex::new(tx),
        }
    }

    /// Push `job` onto the queue, or hand it BACK inside `SendError` when the
    /// consumer is gone.
    ///
    /// Poisoning is meaningless for a `Sender` — it holds no invariant a panic
    /// could leave broken — so the guard is recovered rather than propagated.
    fn send(&self, job: Job) -> Result<(), std::sync::mpsc::SendError<Job>> {
        self.tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(job)
    }

    /// True when the CALLING thread is this queue's own lane thread.
    fn is_own_lane_thread(&self) -> bool {
        CURRENT_LANE.with(std::cell::Cell::get) == Some(self.lane)
    }
}

thread_local! {
    /// The name of the lane whose consumer thread this is — `None` on every
    /// thread that is not a lane consumer (i.e. on every submitter).
    ///
    /// Set once by the lane's receive loop and never cleared: a lane thread does
    /// nothing but drain its own queue for the process lifetime.
    static CURRENT_LANE: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// Panic if this submission would enqueue work onto the lane whose thread is
/// making it — the one shape that wedges a lane permanently.
///
/// A job that submits to its own lane and then waits for the reply can never be
/// answered BY THE CONSUMER RUNNING IT: that consumer only returns to its
/// dequeue loop once the outer job returns, and the outer job is blocked waiting
/// for the inner one. On a single-consumer lane that is immediately fatal — the
/// lane is dead AND every future submitter in the process blocks forever too, a
/// silent, unrecoverable, process-wide hang.
///
/// # Why a size-N pool is rejected too, and not merely "when no consumer is free"
///
/// A pool with an idle consumer could in principle serve a self-submission, so
/// the blanket rule is CONSERVATIVE on purpose (task 6517). It stays blanket for
/// two reasons. First, `size` simultaneous self-submissions wedge a size-`size`
/// pool exactly as one wedges a size-1 lane, so a permissive rule's safety would
/// depend on how many callers happen to be in flight — a property no caller can
/// check, and therefore not a rule. Second, every consumer of a pool publishes
/// the SAME lane name in [`CURRENT_LANE`], which is what keeps this check a
/// thread-local compare rather than a live count of idle consumers; making it
/// conditional would mean adding shared state on the hottest submission path to
/// license a shape no caller in this crate needs.
///
/// # Why a panic here is strictly better than the hang it replaces
///
/// This check runs ON the lane thread, inside the currently-running job — so the
/// panic is caught by that job's own [`std::panic::catch_unwind`] and re-raised
/// on ITS submitter, exactly like any other job panic. The lane survives, one
/// caller sees a loud error naming the reentrancy, and the module's "never a
/// silent hang" invariant holds without exception. (An earlier revision argued a
/// guard's failure mode would poison the shared worker; that is true of a guard
/// placed on the SUBMITTING side, not of this one.)
///
/// Cross-lane submission is deliberately NOT rejected: `ENGINE_LANE` -> jobs
/// submitting to `LSP_LANE` (or the reverse) land on a different thread with its
/// own consumer, so they complete normally.
fn assert_not_reentrant(sender: &JobSender) {
    assert!(
        !sender.is_own_lane_thread(),
        "re-entrant submission to the `{}` large-stack lane: a job running ON \
         that lane submitted to it again. The consumer running the outer job \
         cannot answer the inner one — it only returns to its dequeue loop after \
         the outer job returns, and the outer job is waiting for the inner. Run \
         the inner work inline, or submit it to another lane.",
        sender.lane
    );
}

/// One persistent large-stack lane: a NAME, a SIZE, and the queue feeding it.
///
/// A lane is created lazily on first use and lives for the process. Everything
/// about the mechanism — the 256 MiB stack per consumer, the one FIFO queue, the
/// explicit `None`-when-no-consumer-started record, the never-dropped `Sender` —
/// is shared by every lane; a lane is an INSTANCE, not a variant, and its `size`
/// is one of the values that instance carries rather than a second mechanism
/// (see "One consumer or N" below).
///
/// # Why more than one lane
///
/// The alternative — one thread for all large-stack work — is a latency
/// regression, not a simplification. LSP dispatch never takes the engine mutex,
/// so it shares no state with engine work and serializing the two buys nothing;
/// but a single-consumer queue would make a `textDocument/hover` queue behind an
/// in-flight `set_parameter` geometry evaluation (hundreds of ms to seconds).
/// Since `lsp_request` fires on effectively every keystroke and cursor move,
/// that head-of-line blocking would land on the highest-frequency path in the
/// GUI, where today the two run concurrently on the tokio runtime.
///
/// A second lane costs one extra thread whose 256 MiB stack is a virtual-address
/// reservation committed page-by-page — near-zero RSS until used — and it is
/// created only if something actually submits to it.
///
/// # What the split buys, and the ONE ordering property it gives up
///
/// The lane split protects the keystroke path from ENGINE work; the ordered
/// lane / query pool split (task 6517) additionally BOUNDS its exposure to other
/// LSP work. Both halves are needed, and neither implies the other.
///
/// Task 5772 shipped a single LSP consumer that parked in
/// `Handle::block_on(fut)` for a whole request, so `lsp_request` calls ran
/// strictly one at a time — a real regression against the multi-threaded tauri
/// runtime that preceded it, where `textDocument/hover` (a brief
/// `state.read().await`, never the `eval_state` mutex `didChange` holds across
/// its diagnostics eval) genuinely ran concurrently with an in-flight
/// `didChange`. Task 6517 replaces that with two lanes: [`LSP_LANE`] keeps ONE
/// consumer for the state-mutating and lifecycle methods, and [`LSP_POOL`] runs
/// the eight read-only queries on [`LSP_POOL_SIZE`] consumers. The sharpest case
/// — a workspace-wide `references` or `rename` holding a consumer for its full
/// duration while its deep frames run on the blocking pool's ~2 MiB threads
/// (module docs item 1) — now costs one of [`LSP_POOL_SIZE`] consumers instead
/// of the only one.
///
/// A pool needed two arguments before it could be an instance of this mechanism
/// rather than a second design. Both are discharged:
///
/// * **ORDERING.** LSP notifications are order-sensitive against each other:
///   applying `didChange` N+1 before N yields text neither the client nor the
///   server ever had. So notifications keep ONE FIFO consumer — that is why
///   [`LSP_LANE`] is size 1 by requirement, not by leftover, and why
///   [`crate::lsp_bridge::lane_for_method`] defaults every UNRECOGNISED method
///   to it.
/// * **REENTRANCY.** [`assert_not_reentrant`] stays BLANKET per lane. Every
///   consumer of a pool publishes the same [`CURRENT_LANE`] name, so a pool job
///   submitting to its own pool panics exactly as a single-consumer lane's does.
///   That is conservative — a pool with an idle consumer could in principle
///   serve it — and deliberately so; see that function's docs.
///
/// The one ordering property genuinely GIVEN UP is query-versus-notification: a
/// query may now read text older than a concurrently-processing `didChange`.
/// That is STALENESS, never corruption — `reify-lsp`'s own `RwLock`/`Mutex`
/// serialise the accesses for safety — and it is exactly the pre-task-5772
/// behaviour on the multi-threaded tauri runtime.
///
/// A client that AWAITS each request before issuing the next still reads its own
/// writes, because the awaited `didChange` job has returned before the next
/// request is submitted at all (`lsp_bridge_tests`' (n)). Reify's own frontend
/// awaits only for its position-based COMMANDS. In `gui/src/editor/Editor.tsx`,
/// rename (F2) and find-uses (Shift-F12) go through `onceServerIsCurrent`, and
/// every rename request through the rename guard's `syncServer`; both call
/// `flushPendingLspChange`, which sends any still-debounced `didChange` and
/// waits for it first. Otherwise `lspClient.didChange` fires from a `setTimeout`
/// debounced by `EDITOR_DEBOUNCE_MS` that nothing sequences on, and completion,
/// hover, go-to-definition and occurrence highlights are issued by their own
/// independent CodeMirror sources on their own triggers. So the query behind a
/// DISPLAYED answer can overtake a `didChange` on the pool, and the staleness
/// above is reachable in the shipped app rather than only in a hypothetical
/// non-awaiting client. It self-corrects on the next request, which is why it
/// is disclosed here rather than fixed here. The one APPLIED answer, a `rename`
/// edit, cannot land stale: the server version-stamps it and
/// `gui/src/editor/rename.ts` refuses an edit whose stamped version disagrees
/// with the one the client last sent.
///
/// # One consumer or N: a POOL is an instance, not a variant (task 6517)
///
/// A lane carries a `size`, and everything above holds for every value of it.
/// [`Lane::new`] declares a size-1 lane; [`Lane::pool`] declares a size-N one.
/// There is no second `struct`, no `enum` arm and no second submission path:
/// [`Lane::sender`] spawns `size` consumers over a SHARED
/// `Arc<Mutex<Receiver<Job>>>`, and [`Job`], [`JobSender`], [`JobReply`], the
/// catch-inside-the-job protocol, [`post`], [`dispatch_async`],
/// [`CURRENT_LANE`] and [`assert_not_reentrant`] are reused verbatim. That is
/// the same doctrine that made a second LANE an instance rather than a second
/// design, applied one level down.
///
/// Three consequences worth stating, because none is inferable from "N
/// consumers":
///
/// * **Dequeue order is still FIFO, and jobs still run concurrently.** The FIFO
///   comes from [`std::sync::mpsc`], which delivers in send order; the mutex
///   contributes NOTHING to it and must not be read as if it did.
///   [`std::sync::Mutex`] offers no fairness or FIFO guarantee, so which blocked
///   consumer wins a freed lock is arbitrary. What the shared lock does is
///   serialise the DEQUEUE — one `recv()` at a time, so no two consumers can
///   take the same job — and each consumer RELEASES it before running the job
///   body, which is what lets `size` bodies run at once.
///
///   The consequence to state rather than gloss: on a size-N pool, the order in
///   which job BODIES start is not guaranteed to follow arrival order at all.
///   Only the size-1 [`LSP_LANE`] gives strict start-order, and it gives it
///   because there is one consumer — which is precisely why the order-sensitive
///   methods stay on it.
/// * **The uniform `Arc<Mutex<Receiver>>` costs a size-1 lane one UNCONTENDED
///   lock acquisition per job.** That is a handful of nanoseconds against a
///   channel round trip, and it is paid deliberately: special-casing size 1 to
///   the old `for job in rx` loop would mean two receive loops to keep correct,
///   which is the "one mechanism, literally" property this module trades small
///   costs to keep.
/// * **A size-N lane costs N virtual 256 MiB reservations, not N x 256 MiB
///   resident.** A thread stack is an address-space reservation committed
///   page-by-page on first touch (see [`COMPILE_STACK_SIZE`]), and the whole
///   lane — every consumer of it — is created lazily on the first
///   [`Lane::sender`] call, so a session that never submits to a pool pays
///   nothing for it.
///
/// Thread NAMES follow the same instance-not-variant rule from the outside: a
/// size-1 lane's consumer is named exactly `name`, so `reify-engine-w` and
/// `reify-lsp-w` are byte-identical to what they were before pools existed and
/// no existing profiler filter, `top -H` alert or test assertion moves. Only a
/// size-N lane suffixes an index, `{name}{i}`.
pub(crate) struct Lane {
    /// The lane thread's name, for backtraces, `top -H` and profiler rows. For a
    /// pool this is the PREFIX; consumer `i` is named `{name}{i}`.
    name: &'static str,
    /// How many consumer threads drain this lane's queue. 1 for a lane declared
    /// with [`Lane::new`]; the bound on head-of-line blocking for one declared
    /// with [`Lane::pool`].
    ///
    /// It is a FIXED constant per lane rather than a function of
    /// [`std::thread::available_parallelism`], so the bound is the same on every
    /// machine and is directly assertable from a test.
    size: usize,
    /// What this lane does with a queued job whose awaiting side has gone away.
    /// See [`OnAbandon`] for why this is a property of the DESTINATION and why
    /// [`OnAbandon::Run`] is the default.
    on_abandon: OnAbandon,
    /// How many consumers actually STARTED, as opposed to how many `size`
    /// declares. 0 until [`Lane::sender`] has run; see [`Lane::started`] for why
    /// the realised count is recorded rather than only warned about.
    started: std::sync::atomic::AtomicUsize,
    /// The lazily-created queue. `None` records that the OS REFUSED the mapping.
    queue: std::sync::OnceLock<Option<JobSender>>,
}

impl Lane {
    /// The one constructor; [`Lane::new`], [`Lane::pool`] and
    /// [`Lane::cancelling_pool`] are named façades over it that fix one argument
    /// each.
    ///
    /// Kept private and shared so "one mechanism, literally" stays true of the
    /// declaration too: there is a single place a lane's fields are assembled
    /// and a single place the size assertion lives, no matter which façade a
    /// call site names.
    ///
    /// # A zero-size lane is a COMPILE error, not a runtime one
    ///
    /// The `assert!` is reachable only in a `const` context: every call site is
    /// a `static` initialiser, so it is const-evaluated and `..(name, 0, ..)`
    /// fails the build rather than yielding a lane whose queue nobody drains —
    /// which is a silent hang, the one outcome this module promises never to
    /// produce.
    const fn declare(name: &'static str, size: usize, on_abandon: OnAbandon) -> Self {
        assert!(
            size >= 1,
            "a lane needs at least one consumer; a size-0 lane's queue would \
             never be drained, which is the silent hang this module exists to \
             rule out"
        );
        Self {
            name,
            size,
            on_abandon,
            started: std::sync::atomic::AtomicUsize::new(0),
            queue: std::sync::OnceLock::new(),
        }
    }

    /// Declare a SINGLE-consumer lane. `const` so lanes can be `static`s created
    /// at no runtime cost; the thread itself is not spawned until
    /// [`Lane::sender`] is first called.
    ///
    /// Its consumer is named exactly `name` — no index suffix — so the lanes
    /// that predate pools keep their exact thread names.
    ///
    /// [`OnAbandon::Run`], the conservative default: a single-consumer lane is
    /// the shape this module uses for ORDER-SENSITIVE, state-mutating work, and
    /// discarding such a job unrun loses state nothing recovers. See
    /// [`OnAbandon`].
    const fn new(name: &'static str) -> Self {
        Self::declare(name, 1, OnAbandon::Run)
    }

    /// Declare a lane with `size` consumers, bounding head-of-line blocking
    /// among the work routed to it at `size` rather than serializing it
    /// (task 6517).
    ///
    /// `name` is a PREFIX here: consumer `i` is named `{name}{i}`, so a caller
    /// must keep `name.len()` plus the widest index it will ever use inside
    /// Linux's 15-byte `pthread_setname_np` budget — `std` silently ignores a
    /// longer name, so an overrun would not fail loudly, it would just erase the
    /// thread's identity from `/proc`, `top -H` and every profiler capture. Each
    /// production prefix carries a `const _: () = assert!(..)` for that, beside
    /// the constant.
    ///
    /// `pub(crate)` for the same stated reason [`JobSender::new`] is: so a test
    /// can declare its OWN instance. That matters more here than it does there —
    /// proving a pool runs `size` jobs at once means PARKING `size` consumers,
    /// and doing that to a process-wide `static` would starve whichever other
    /// test in the same binary is concurrently using it.
    ///
    /// [`OnAbandon::Run`], like [`Lane::new`] — a pool is not cancellable by
    /// virtue of being a pool, because concurrency-safety is a property of the
    /// WORK routed to a destination and not of how many consumers drain it. See
    /// [`OnAbandon`], and [`Lane::cancelling_pool`] for the opt-in. A zero-size
    /// pool is a COMPILE error; see [`Lane::declare`].
    ///
    /// The `allow` is scoped to `not(test)` for the same reason [`Lane::size`]'s
    /// is: since the one production pool became a [`Lane::cancelling_pool`],
    /// this façade has no production caller, and a blanket allow would also hide
    /// the day it stopped being reachable from the tests either. It is kept
    /// rather than deleted because a NON-discarding pool is the shape any future
    /// multi-consumer lane carrying effectful work must have, and re-deriving
    /// that under pressure is how the blanket-discard defect happened once.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn pool(name: &'static str, size: usize) -> Self {
        Self::declare(name, size, OnAbandon::Run)
    }

    /// [`Lane::pool`], but declaring that a queued job whose awaiting side has
    /// gone away may be DISCARDED unrun ([`OnAbandon::Discard`]).
    ///
    /// Distinctly named, and required at the declaration, because this is the
    /// one lane property whose wrong value loses state silently ([`OnAbandon`]).
    /// A caller naming this constructor is asserting that every method routed
    /// here is read-only against server-side state — which for the one
    /// production instance ([`LSP_POOL`]) is exactly what
    /// [`crate::lsp_bridge::lane_for_method`]'s `matches!` arm decides.
    ///
    /// A POOL constructor rather than a general one because this crate has no
    /// use for a discarding size-1 lane: the shape this module gives size-1
    /// lanes is order-sensitive work, which is precisely what must not be
    /// discarded. Adding one later is a one-line `declare` call, and would then
    /// be a decision someone made rather than a default they inherited.
    pub(crate) const fn cancelling_pool(name: &'static str, size: usize) -> Self {
        Self::declare(name, size, OnAbandon::Discard)
    }

    /// How many consumers this lane DECLARES.
    ///
    /// The bound this module advertises — "head-of-line blocking among LSP
    /// queries is bounded at [`LSP_POOL_SIZE`], and notifications keep exactly
    /// one FIFO consumer" — is a property of these two numbers, and until this
    /// accessor existed neither was readable from a test. `LSP_LANE`'s 1 could
    /// be pinned behaviourally (consumers ROTATE across sequential submissions,
    /// so a second consumer shows up as a second `ThreadId`), but the pool's 4
    /// could not: observing all four names requires the shared receiver lock to
    /// be handed to a different waiter on every round trip, which is a
    /// scheduling bet rather than a guarantee, and observing them by PARKING
    /// four consumers would starve every other test sharing this process-wide
    /// `static`. So the count is exposed structurally instead.
    ///
    /// Read only by tests, and `pub(crate)` for the same stated reason
    /// [`Lane::pool`] and [`JobSender::new`] are. It is a two-word getter over a
    /// field that is already `const` at every call site, so it cannot drift from
    /// what [`Lane::sender`] spawns.
    ///
    /// The `allow` is scoped to `not(test)` rather than blanket: this getter has
    /// no production caller BY DESIGN, so a blanket allow would also hide the
    /// day it stopped being read anywhere at all.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn size(&self) -> usize {
        self.size
    }

    /// What this lane does with a queued job whose awaiting side has gone away.
    ///
    /// Exposed for the same reason [`Lane::size`] is: the module ADVERTISES that
    /// only the query pool may discard, and until this accessor existed that
    /// claim was checkable only by reading the three `static` initialisers.
    /// A lane rebuilt with the wrong façade — `LSP_LANE` declared via
    /// [`Lane::cancelling_pool`] — would start silently dropping queued
    /// `didOpen`s while every behavioural test in the binary stayed green,
    /// because the abandonment those tests need has to be manufactured.
    ///
    /// Read only by tests, with the same `not(test)`-scoped `allow` and for the
    /// same stated reason as [`Lane::size`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn on_abandon(&self) -> OnAbandon {
        self.on_abandon
    }

    /// How many consumers this lane actually STARTED — 0 until [`Lane::sender`]
    /// has run, and thereafter fixed for the process.
    ///
    /// [`Lane::size`] is what the lane DECLARES; this is what it got. They differ
    /// exactly when [`Lane::sender`] hit a partial spawn failure, which is a real
    /// state rather than a theoretical one: a pool that started 1 of 4 consumers
    /// under memory pressure serializes every LSP query again — precisely the
    /// regression task 6517 exists to prevent — while `LSP_POOL.size()` still
    /// reports 4 and every routing test stays green. Before this accessor the
    /// only evidence was one `eprintln!` on a stderr nobody reads in a packaged
    /// build.
    ///
    /// So the advertised head-of-line bound is READABLE at runtime, not merely
    /// declared. The module insists elsewhere that a limit which stopped being
    /// total did not stop existing; the same standard says the realised bound
    /// should be observable.
    ///
    /// # Why the read goes through the `OnceLock`, and why `Relaxed` then
    /// suffices
    ///
    /// The store happens inside [`std::sync::OnceLock::get_or_init`]'s closure,
    /// so the ONLY thing that synchronises a reader with it is that `OnceLock`'s
    /// acquire/release pair. A bare `Relaxed` load would therefore be
    /// well-defined but not meaningful on a thread that has never called
    /// [`Lane::sender`]: with no happens-before against another thread's
    /// `get_or_init`, it could report 0 for a lane whose consumers are already
    /// running — a wrong answer that reads exactly like the true one.
    ///
    /// So the `get()` below is the synchronisation, not a fast path. `Some`
    /// means this thread happens-after the initialiser, and the store precedes
    /// that release, so the `Relaxed` load is guaranteed to observe it and no
    /// stronger ordering here would add anything. `None` means the lane genuinely
    /// has not been created yet, and 0 is then the truth rather than a stale
    /// read. The accessor is therefore meaningful on ANY thread, which is what
    /// its callers assume.
    ///
    /// Read only by tests, with the same `not(test)`-scoped `allow` and for the
    /// same stated reason as [`Lane::size`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn started(&self) -> usize {
        if self.queue.get().is_none() {
            return 0;
        }
        self.started.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Lazily create this lane's worker, yielding its queue — or `None` if the
    /// OS refused the [`COMPILE_STACK_SIZE`] mapping.
    ///
    /// [`std::sync::OnceLock`] gives lazy creation (a session that never touches
    /// this lane never pays for the 256 MiB mapping) and exactly-once semantics.
    ///
    /// The spawn failure is recorded EXPLICITLY as `None` rather than inferred
    /// from a subsequently-dead channel: nothing documents that `Builder::spawn`
    /// drops the closure it could not run, and a leaked `Receiver` would make
    /// every `send` succeed into a queue nobody drains — i.e. a hang, the one
    /// outcome this module must never produce.
    ///
    /// Holding the `Sender` in the `OnceLock` forever is deliberate: the channel
    /// therefore never disconnects on its own, so every consumer parks on an
    /// empty queue rather than exiting.
    ///
    /// # Spawn failure with N consumers: `None` only when ZERO started
    ///
    /// The policy above generalises without changing meaning. `None` is the
    /// degrade-to-inline / degrade-to-native-await signal, and it is correct
    /// exactly when NOTHING will drain the queue. A PARTIAL spawn failure — say
    /// three of four consumers — is not that case: the queue still has a
    /// drainer, so every submission still completes on a large stack, merely
    /// with a smaller concurrency bound. Warning and continuing is therefore
    /// strictly better than discarding the consumers that did start, and it
    /// keeps the "never lose a result, never block" invariant intact under
    /// partial OS refusal.
    ///
    /// The realised count is also RECORDED, not merely warned about — see
    /// [`Lane::started`]. A pool that started 1 of 4 consumers advertises a
    /// bound it is not honouring, and a warning on stderr is not something any
    /// other part of the process (or any test) can observe.
    ///
    /// # Why the warnings stay `eprintln!` rather than `tracing::warn!`
    ///
    /// MEASURED, because the obvious improvement is a regression here.
    /// `tracing` is a dependency of this crate and `tracing::warn!` would be the
    /// idiomatic call — but `tracing` macros are no-ops unless a global
    /// subscriber is installed, and NOTHING in this workspace installs one:
    /// `tracing-subscriber` appears in no `Cargo.toml` in the repo, and no
    /// source file under `gui/src-tauri` calls `set_global_default` or any
    /// `fmt::init` equivalent. There is no "app log" for these to land in.
    /// Converting them would make a lane that failed to spawn warn NOWHERE
    /// instead of on stderr — strictly less visible, for a cosmetic gain.
    /// Revisit if and when the GUI grows a subscriber; the existing
    /// `tracing::warn!` calls elsewhere in this crate are in the same position
    /// and are not evidence to the contrary.
    pub(crate) fn sender(&'static self) -> Option<&'static JobSender> {
        self.queue
            .get_or_init(|| {
                let (tx, rx) = std::sync::mpsc::channel::<Job>();
                let name = self.name;
                let size = self.size;
                // Shared so `size` consumers can drain ONE queue. The lock is
                // taken only across a dequeue (see the loop below), never across
                // a job body.
                let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

                let mut started = 0usize;
                for index in 0..size {
                    // EXACTLY `name` for a single-consumer lane, so
                    // `reify-engine-w` / `reify-lsp-w` are byte-identical to what
                    // they were before pools existed; `{name}{index}` only when
                    // there is more than one consumer to tell apart.
                    let thread_name = if size == 1 {
                        name.to_owned()
                    } else {
                        format!("{name}{index}")
                    };
                    let rx = std::sync::Arc::clone(&rx);
                    match std::thread::Builder::new()
                        .name(thread_name)
                        .stack_size(COMPILE_STACK_SIZE)
                        .spawn(move || {
                            // Publish this thread's lane identity so
                            // `assert_not_reentrant` can tell a self-submission
                            // (a permanent wedge) from a cross-lane one (legal).
                            // EVERY consumer of a lane publishes the SAME name —
                            // that is what makes the guard work unchanged for a
                            // pool.
                            CURRENT_LANE.with(|l| l.set(Some(name)));
                            loop {
                                // Take the lock, dequeue, RELEASE it — then run
                                // the job. Releasing before the body is what
                                // lets `size` bodies run at once; see `Lane`'s
                                // "One consumer or N" for what the lock does and
                                // does not order. Poisoning is recovered rather
                                // than propagated, mirroring `JobSender::send` —
                                // and a job body cannot poison this lock anyway,
                                // because the job's own `catch_unwind` is INSIDE
                                // the job and the lock is not held while it runs.
                                let dequeued = {
                                    let guard = rx
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    guard.recv()
                                };
                                // Parks while the queue is empty. `recv` only
                                // errs when the `Sender` in the `OnceLock`
                                // drops, which never happens, so this loop lives
                                // as long as the process.
                                let Ok(job) = dequeued else { break };
                                job();
                            }
                        }) {
                        Ok(_handle) => started += 1,
                        Err(e) => {
                            eprintln!(
                                "Warning: failed to spawn {name} lane consumer \
                                 {index} of {size} ({e})"
                            );
                        }
                    }
                }

                // Record what actually started, so the realised head-of-line
                // bound is readable rather than only warned about. Ordered
                // before the two warnings and the `return` so BOTH exits — the
                // degraded `None` and the partial-pool `Some` — leave a truthful
                // count behind. See `Lane::started`.
                self.started
                    .store(started, std::sync::atomic::Ordering::Relaxed);

                if started == 0 {
                    // Nothing will drain the queue, so record the degrade
                    // signal.
                    eprintln!(
                        "Warning: failed to spawn any {name} thread; that lane's \
                         work will run on a default-size stack instead"
                    );
                    return None;
                }
                if started < size {
                    eprintln!(
                        "Warning: the {name} lane started {started} of {size} \
                         consumers; its work still runs on a large stack, with a \
                         smaller concurrency bound"
                    );
                }
                Some(JobSender::new(name, tx, self.on_abandon))
            })
            .as_ref()
    }
}

/// The ENGINE lane, fed only by [`post_to_worker`], whose sole production caller
/// is [`crate::eval_queue::EvalQueue`]. Named [`WORKER_THREAD_NAME`].
pub(crate) static ENGINE_LANE: Lane = Lane::new(WORKER_THREAD_NAME);

/// The ORDERED LSP lane: the six state-mutating and lifecycle methods plus,
/// conservatively, any method `InProcessLsp::handle_request` does not recognise.
/// Named [`LSP_WORKER_THREAD_NAME`].
///
/// WHICH six is not restated here. [`crate::lsp_bridge::lane_for_method`]'s
/// `matches!` arm is the single authoritative list, and it is the only copy
/// under test (`lsp_bridge_tests`' (j) executes every entry against the real
/// dispatcher). A prose copy beside it is a second list that nothing checks.
///
/// Separate from [`ENGINE_LANE`] so a hover never queues behind a geometry
/// evaluation — see [`Lane`]'s "Why more than one lane". SIZE 1 is a
/// correctness requirement rather than a leftover; [`Lane`]'s ORDERING
/// paragraph is why.
pub(crate) static LSP_LANE: Lane = Lane::new(LSP_WORKER_THREAD_NAME);

/// The LSP QUERY POOL: the eight read-only query methods, on [`LSP_POOL_SIZE`]
/// consumers named `{LSP_POOL_THREAD_PREFIX}{i}` (task 6517).
///
/// WHICH eight is [`crate::lsp_bridge::lane_for_method`]'s `matches!` arm — the
/// single authoritative list, and the only copy any test executes. The count is
/// repeated here because it is load-bearing against [`LSP_POOL_SIZE`]; the
/// membership is not.
///
/// This is what bounds head-of-line blocking among LSP queries instead of
/// leaving them serialized (module docs item 2), and — being declared with
/// [`Lane::cancelling_pool`] — it is also the ONE destination in the process
/// that may discard an abandoned job unrun ([`OnAbandon`]). Both properties are
/// licensed by the same fact and by nothing else: every method routed here is
/// read-only against server-side state. A method reclassified into this pool
/// inherits BOTH.
pub(crate) static LSP_POOL: Lane =
    Lane::cancelling_pool(LSP_POOL_THREAD_PREFIX, LSP_POOL_SIZE);

/// Queue `job` on the persistent ENGINE lane WITHOUT waiting for it, so the
/// calling thread is never parked on engine work. Deliver any result through a
/// channel the job captures. See [`post`] for the degraded arms.
pub fn post_to_worker(job: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    post(ENGINE_LANE.sender(), Box::new(job))
}

/// Queue `job` on `sender`'s lane without waiting for it — the lane-agnostic
/// seam behind [`post_to_worker`], shaped like [`dispatch_async`] so the
/// degraded arms are testable.
///
/// The job never runs inline on the caller, which may be a tokio worker where
/// OCCT's synchronous kernel handle panics. With no lane (the OS refused the
/// 256 MiB mapping) it runs on a spawned default-stack thread, since a second
/// 256 MiB request would be refused the same way; a job handed back by a dead
/// lane runs on [`spawn_on_large_stack`]. `Err` means no thread could be spawned
/// and the job was dropped unrun.
///
/// A panic in the job is caught and logged inside the job, so it never unwinds a
/// lane's receive loop. There is no reentrancy guard: posting never waits, so a
/// job may post to its own lane.
pub(crate) fn post(sender: Option<&JobSender>, job: Job) -> std::io::Result<()> {
    let contained: Job = Box::new(move || {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
            eprintln!(
                "Warning: a posted large-stack job panicked: {}",
                panic_payload_message(&*payload)
            );
        }
    });
    match sender {
        Some(sender) => match sender.send(contained) {
            Ok(()) => Ok(()),
            Err(std::sync::mpsc::SendError(job)) => spawn_on_large_stack(job).map(drop),
        },
        None => std::thread::Builder::new()
            .name(ENGINE_THREAD_NAME.to_string())
            .spawn(contained)
            .map(drop),
    }
}

/// The message a caught panic carried: `panic!("literal")` yields a `&str`
/// payload, a formatted `panic!` a `String`.
pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

/// Drive `fut` to completion on the persistent LSP lane WITHOUT blocking the
/// calling tokio worker, resolving to its output.
///
/// For `lsp_request` — an `async fn` Tauri command that fires on effectively
/// every keystroke and cursor move. Awaiting a
/// [`tokio::sync::oneshot`](tokio::sync::oneshot) reply RELEASES the calling
/// tokio worker while the lane thread computes, where waiting on the job would
/// pin that worker for the whole LSP round trip.
///
/// # Why this lane carries a FUTURE, not a closure
///
/// A correctness constraint rather than a style choice. The lane must be able to
/// degrade — to run the work SOMEWHERE when it is absent or its queue is dead —
/// and the degraded arms of an async submission necessarily run in the
/// submitting async frame, i.e. on a thread already inside the tauri runtime. A
/// future can simply be `.await`ed there. A closure that pre-bakes a
/// [`tokio::runtime::Handle::block_on`] — which is what an LSP job must do, see
/// [`dispatch_async`] — cannot: `block_on` from inside a runtime panics "Cannot
/// start a runtime from within a runtime". Taking the future and letting
/// [`dispatch_async`] decide how to drive it puts that decision with the code
/// that knows which frame the work will land in.
///
/// Everything else is shared with the ENGINE lane: the same [`Lane`] mechanism,
/// the same boxed [`Job`], the same catch-inside-the-job protocol. The
/// [`JobReply`] payload is what keeps a job's panic faithful to its awaiter.
///
/// This is not a new concurrency design: `debug_server::run_on_engine` already
/// bridges an async caller to a large-stack thread with exactly
/// [`spawn_on_large_stack`] + a `oneshot`. This amortises that bridge onto a
/// persistent lane instead of paying a fresh 256 MiB mapping per call.
///
/// # This is the ORDERED-lane convenience wrapper, not the production entry
///
/// Since task 6517, production LSP dispatch reaches the lanes through
/// [`crate::lsp_bridge::lane_for_method`], which routes each method to either
/// [`LSP_LANE`] (ordered) or [`LSP_POOL`] (queries). This function hard-codes
/// [`LSP_LANE`], so it is the right entry only for work that must be ordered
/// against the notification stream; several tests use it for exactly that.
///
/// It is `pub(crate)` rather than `pub` as of that same task. `lib.rs` declares
/// `pub mod large_stack`, so `pub` here meant PUBLIC API of the `reify-gui`
/// library — and since routing moved to [`crate::lsp_bridge::lane_for_method`]
/// this function has no caller in the binary at all, only in tests. Leaving it
/// `pub` would advertise the ordered lane as the LSP entry point to an outside
/// caller, which is now exactly the wrong default: an arbitrary method must be
/// ROUTED, not pinned to `LSP_LANE`. `pub(crate)` keeps every existing test
/// call site compiling (they are in this crate) while removing the misleading
/// surface — the same visibility [`post`] and [`dispatch_async`] carry, for
/// the same reason. The `not(test)` `allow` that follows is the honest record of
/// the consequence: with the visibility narrowed, "no production caller" becomes
/// a `dead_code` warning in the non-test build, and scoping the allow to
/// `not(test)` keeps the lint live for the build where the callers actually are.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn run_on_lsp_worker<Fut, T>(fut: Fut) -> T
where
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    dispatch_async(LSP_LANE.sender(), fut).await
}

/// Submit `fut` to `sender`'s lane and AWAIT its output — or, given `None`,
/// simply `.await` it here.
///
/// `pub(crate)` because turning "is there a lane?" into a parameter is what
/// makes the degraded arm reachable from a test rather than requiring a real
/// `pthread_create` failure.
///
/// # How the future is driven on the lane, and why by a `Handle`
///
/// A lane thread is a plain `std` thread with no ambient runtime, so the future
/// needs a driver. FOUR of `InProcessLsp::handle_request`'s arms
/// (`textDocument/definition`, `prepareRename`, `rename`, `references`) call
/// [`tokio::task::spawn_blocking`], whose first statement is `Handle::current()`
/// — under a bare executor such as `futures::executor::block_on` those four
/// would panic with "there is no reactor running".
/// [`tokio::runtime::Handle::block_on`] installs the runtime context via
/// `enter_runtime` and is explicitly legal from a NON-runtime thread. So the
/// handle is captured HERE, on the submitter (which is inside the tauri
/// runtime), and MOVED into the job: the "how is this future driven" policy
/// lives with the lane that drives it, not with each caller. Secondary reason
/// for `Handle` over `futures`: `futures` is not a declared `reify-gui`
/// dependency, so using it would mean adding one to get strictly worse
/// behaviour.
///
/// # Degradation policy: never lose a result, never hang an `.await`, never
/// nest a runtime
///
/// A hung or panicking future would leave the frontend's `invoke` promise
/// unresolved forever (a silently dead editor pane). Three arms, and none of
/// them needs a resource the triggering condition would deny:
///
/// * `None` lane (the OS refused the 256 MiB mapping): `.await` the future right
///   here. That is a NATIVE await, not a thread — which is the only degradation
///   that still works under the very condition that triggers it, since an OS
///   that refused a 256 MiB mapping will equally refuse a recovery thread. It is
///   also genuinely "today's behaviour": the LSP future polled on a tokio
///   worker's ~2 MiB stack, exactly what `main.rs::lsp_request` did before task
///   5772.
/// * No ambient runtime ([`tokio::runtime::Handle::try_current`] is `Err`):
///   `.await` here too. `try_current` rather than `current` so a caller polled
///   outside any runtime DEGRADES instead of panicking; with no runtime there is
///   no nesting hazard, and the four `spawn_blocking` arms would have failed
///   under any driver in that state anyway.
/// * `SendError(job)`: the queue handed the job BACK unrun. The job provably
///   contains a `Handle::block_on`, so it must NOT run in this frame — this
///   frame is inside the runtime, and `block_on` there panics "Cannot start a
///   runtime from within a runtime". Hand it to [`spawn_on_large_stack`]
///   instead: a plain `std` thread, therefore never a runtime context, with a
///   [`COMPILE_STACK_SIZE`] stack and an `io::Result` rather than an inline
///   fallback. If that spawn ALSO fails, drop the job — its `reply_tx` drops
///   with it, `reply_rx` resolves `Err(RecvError)` at once, and the loud-panic
///   arm below fires. One panic site, never a hang, and the result is preserved
///   whenever preserving it is possible at all.
///
/// And a `RecvError` is that loud panic rather than a hang: a disconnected
/// `oneshot` resolves AT ONCE, so the `.await` below can never park forever.
///
/// # Drop-cancellation: an `OnAbandon::Discard` destination checks
/// `reply_tx.is_closed()` before driving
///
/// ## First, what actually drops a `dispatch_async` future in production
///
/// MEASURED against the pinned `tauri` 2.11.2, because an earlier revision of
/// this section asserted the opposite and was wrong. An async
/// `#[tauri::command]` is resolved through `InvokeResolver::respond_async` /
/// `respond_async_serialized_inner`, and BOTH do
/// `crate::async_runtime::spawn(async move { .. })` as a statement, DISCARDING
/// the returned handle. `tauri::async_runtime::JoinHandle` is a thin enum over
/// `tokio::task::JoinHandle` with no `Drop` impl of its own
/// (`tauri::async_runtime`), and dropping a tokio `JoinHandle` DETACHES
/// the task rather than cancelling it. So the command future runs to completion
/// no matter what the webview does — a closed window, a navigated-away pane, a
/// keystroke's request superseded by the next one. That was true before task
/// 5772 as well as after it.
///
/// The consequence, stated rather than left flattering: on today's production
/// path `reply_tx.is_closed()` is reachable only when the SPAWNED TASK ITSELF is
/// dropped, i.e. at runtime/app teardown (or via an explicit
/// `JoinHandle::abort`, which nothing in this app calls). Every test that
/// exercises the check manufactures the drop with `tokio::time::timeout`,
/// because no production caller produces one. The guard is therefore a cheap
/// structural correctness property, NOT a live optimisation — and it is
/// documented as one so a later reader does not build on a benefit that is not
/// being collected.
///
/// It is kept, rather than deleted as dead weight, for two reasons: teardown IS
/// a real trigger (a queue of abandoned jobs at shutdown is work worth skipping),
/// and the check is what makes a future `select!`/timeout wrapper safe by
/// construction instead of by nobody having added one yet.
///
/// ## And what the check itself is
///
/// Six things a reader needs and cannot infer from the `is_closed` check in the
/// job body:
///
/// 1. **`reply_tx.is_closed()` IS the cancellation token.** It is true exactly
///    when the awaiting side's `dispatch_async` future was dropped, because that
///    future owns the `oneshot` receiver. So nothing has to be threaded through
///    the [`Job`] contract, through `lsp_bridge`, or through any caller: no new
///    parameter, no new type, and no new dependency. `is_closed` needs only
///    `tokio`'s `sync` feature, which this crate already declares for the
///    `oneshot` channel itself. The alternative token types were not available:
///    `tokio-util` is not a `reify-gui` dependency at all (and the workspace pin
///    it would come from selects only `rt`, not the `sync` feature that gates
///    `CancellationToken`), and neither is `futures`.
/// 2. **It can only skip work that has NOT STARTED.** It never interrupts work
///    in flight: there is no cancellation point inside
///    [`tokio::runtime::Handle::block_on`], and the four `spawn_blocking` arms
///    are uninterruptible once started in any case — dropping a `JoinHandle`
///    does not cancel a blocking task. Claiming more than "dropped from the
///    queue" would be false.
/// 3. **It applies ONLY to an [`OnAbandon::Discard`] destination — today, only
///    [`LSP_POOL`].** The blanket version of this rule was a defect. What it
///    costs, why the parity argument that licensed it does not hold, and why
///    [`OnAbandon::Run`] is the default are on [`OnAbandon`], which is where the
///    policy lives; repeating the chain here would be a second copy of it.
/// 4. **Why the fire-and-forget seam [`post`] gets no equivalent.** Nobody
///    awaits a posted job, so there is no receiver whose drop could signal
///    abandonment: a posted job always runs. The asymmetry is a property of the
///    two seams, not an omission.
/// 5. **The cancel path drops `fut` under the runtime and under a
///    `catch_unwind`.** It is the only path that disposes of `fut` WITHOUT
///    `handle.block_on`, so it is the only one that would otherwise run tokio
///    destructors on a plain `std` thread with no ambient runtime ("there is no
///    reactor running") and outside any catch. Both guards are restored
///    explicitly at the check; see the comment there for why losing a consumer
///    is the worst outcome available at this particular line. Each is pinned by
///    its own test — `large_stack_tests`' (aj) and (ak) — rather than left as a
///    justified line no failure would ever reach.
/// 6. **An `OnAbandon::Run` destination still throws the ANSWER away.** The
///    `reply_tx.send(outcome)` at the end of the job body is already
///    `let _ = ..`, so a job whose receiver has gone completes, applies its side
///    effects, and discards its result without erroring. "Run anyway" costs a
///    consumer for the duration; it does not cost a panic or a hang.
///
/// The RESIDUAL, stated rather than left to be discovered: a request abandoned
/// AFTER its consumer picked it up still runs to completion. That cost is
/// BOUNDED rather than eliminated — it occupies one of [`LSP_POOL_SIZE`] query
/// consumers instead of the only LSP consumer in the process.
pub(crate) async fn dispatch_async<Fut, T>(sender: Option<&JobSender>, fut: Fut) -> T
where
    Fut: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let Some(sender) = sender else {
        // No lane (already warned once, in the lane initialiser). Await the
        // future right here — a panic in it propagates naturally, so this arm
        // needs no forwarding of its own.
        return fut.await;
    };

    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        // Polled outside any tokio runtime, so there is nothing to hand the
        // lane thread as a driver — and equally nothing to nest. Await here.
        return fut.await;
    };

    // Rejected loudly rather than enqueued: a future submitted from a job
    // already running on this lane could only be driven after that job
    // returned (see `assert_not_reentrant`).
    assert_not_reentrant(sender);

    // Copied out before the job is boxed: `sender` is a plain borrow with no
    // `'static` bound, and `OnAbandon` is `Copy`.
    let on_abandon = sender.on_abandon;

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JobReply<T>>();
    let job: Job = Box::new(move || {
        // CANCEL AT THE LANE (task 6517), and only where the DESTINATION says
        // it is safe — see [`OnAbandon`] for why the policy travels with the
        // queue, and this function's "Drop-cancellation" section for what the
        // check is and is not.
        //
        // The two guards are the part that is not inferable from the code. This
        // is the ONE path that disposes of `fut` without `handle.block_on`, so
        // it is the only one that must restore both by hand: `handle.enter()`,
        // because a tokio resource's destructor panics "there is no reactor
        // running" on a lane consumer's plain `std` thread; and `catch_unwind`,
        // because a panicking destructor escaping here would kill a consumer of
        // this `Discard` destination (today `LSP_POOL`), silently narrowing the
        // pool and so its head-of-line bound — a loss neither `Lane::size` nor
        // `Lane::started` would show. The payload is dropped rather than re-raised because `reply_tx` is
        // already closed — by construction there is no submitter left to raise
        // it on. `AssertUnwindSafe` is sound for the same reason it is below.
        if on_abandon == OnAbandon::Discard && reply_tx.is_closed() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _enter = handle.enter();
                drop(fut);
            }));
            return;
        }
        // The catch lives INSIDE the job, so the lane's receive loop can never
        // observe an unwind and cannot be killed by user code.
        // `AssertUnwindSafe` is sound because the job OWNS its captures and is
        // consumed by this call — nothing observes them after a panic — and the
        // payload is re-raised on the submitter below rather than swallowed.
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle.block_on(fut)));
        // A dropped receiver means the awaiting task is gone; nothing to report.
        let _ = reply_tx.send(outcome);
    });

    // The queue lock is released before the `.await` below — it is a std
    // `Mutex` held only across this push, inside `JobSender::send`.
    let send_result = sender.send(job);

    if let Err(std::sync::mpsc::SendError(job)) = send_result {
        // The lane's worker is gone and the job came back unrun. It carries a
        // `Handle::block_on`, so running it in THIS frame would panic — give it
        // a plain `std` thread, which is never a runtime context.
        if let Err(e) = spawn_on_large_stack(job) {
            // Nothing left that can legally run it. Dropping the job drops its
            // `reply_tx`, so the `.await` below resolves `Err` immediately and
            // reports loudly instead of hanging.
            eprintln!(
                "Warning: a large-stack lane's queue was dead and the recovery \
                 thread could not be spawned ({e}); the submitted job cannot be \
                 run"
            );
        }
    }

    match reply_rx.await {
        Ok(Ok(value)) => value,
        // Re-raise the ORIGINAL payload on the awaiting task, exactly as if the
        // future had been awaited inline.
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        // Reached only when the job was dropped unrun — i.e. both the lane and
        // the recovery thread were unavailable. A disconnected `oneshot`
        // resolves AT ONCE, so this is a loud failure, not a hung future.
        Err(_) => panic!(
            "a large-stack lane dropped a job without answering: its oneshot \
             reply channel disconnected before a result arrived"
        ),
    }
}
