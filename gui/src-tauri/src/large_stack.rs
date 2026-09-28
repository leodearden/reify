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
//! **Persistent lanes — [`ENGINE_LANE`] and [`LSP_LANE`].** One thread per lane
//! for the process lifetime; the per-call cost becomes a queue push.
//!
//! * ENGINE lane — fed ONLY by [`post_to_worker`], whose sole production caller
//!   is [`crate::eval_queue::EvalQueue`]. Its roster is therefore everything that
//!   submits to that queue. Posting never waits for the job, so no submitter —
//!   least of all the GTK main thread — parks on engine work.
//! * LSP lane — `main.rs::lsp_request` → `lsp_bridge::lsp_request_on_worker`,
//!   which fires on effectively every keystroke and cursor move, through
//!   [`run_on_lsp_worker`]. It carries a FUTURE rather than a closure: its
//!   degraded arms run in the submitting async frame, where a future can be
//!   `.await`ed but a closure with a [`tokio::runtime::Handle::block_on`] baked
//!   in would panic "Cannot start a runtime from within a runtime".
//!
//! So every engine-bearing path runs on a large stack: queued work on the ENGINE
//! lane, the debug server on per-call threads.
//!
//! # What is still NOT covered
//!
//! Three boundaries, stated as limits rather than left to be inferred. All
//! three are LSP-side: the engine surface is covered.
//!
//! 1. **Four LSP methods.** `InProcessLsp::handle_request`'s
//!    `textDocument/definition`, `prepareRename`, `rename` and `references` arms
//!    each call `tokio::task::spawn_blocking`, so their compiler work executes on
//!    tokio's BLOCKING POOL, whose threads take the std ~2 MiB default (nothing
//!    under `gui/src-tauri` sets `thread_stack_size`). Putting `handle_request`
//!    on a lane gives the big stack only to that thread's OWN frames, so the LSP
//!    lane cannot help those four. Closing them needs a change in
//!    `crates/reify-lsp/src/server.rs`, which is outside this module and would
//!    also regress the stdio `reify lsp` CLI server (it relies on
//!    `spawn_blocking` to keep its 2-worker runtime responsive). Tracked as
//!    task #6195.
//! 2. **Concurrency WITHIN a lane.** A lane has one consumer, so routing
//!    `lsp_request` onto [`LSP_LANE`] serializes LSP requests against each
//!    other, where the multi-threaded tauri runtime previously ran them
//!    concurrently. The split buys isolation from ENGINE work, not from other
//!    LSP work — see [`Lane`]'s "What the split does NOT buy" for what that
//!    costs and what bounding it would take. Tracked as task #6517.
//! 3. **Drop-cancellation of an LSP request.** A future handed to a lane is
//!    driven to completion by a thread that cannot be cancelled, so abandoning
//!    the awaiting side no longer stops the work — see
//!    [`crate::lsp_bridge::lsp_request_on_worker`]'s "What this COSTS".
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
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
}

impl JobSender {
    /// Wrap a lane's `Sender`, tagging it with that lane's name.
    ///
    /// `pub(crate)` so a test can build a SYNTHETIC sender (typically over an
    /// already-dropped `Receiver`) to provoke the `SendError` arms of
    /// [`post`] / [`dispatch_async`] deterministically.
    pub(crate) fn new(lane: &'static str, tx: std::sync::mpsc::Sender<Job>) -> Self {
        Self {
            lane,
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
/// A lane has a SINGLE consumer, so a job that submits to its own lane and then
/// waits for the reply can never be answered: the inner job only runs once the
/// outer one returns, and the outer one is blocked waiting for it. The thread
/// stops returning to its `for job in rx` loop, so the lane is dead AND every
/// future submitter in the process blocks forever too — a silent, unrecoverable,
/// process-wide hang.
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
         that lane submitted to it again. The lane has a single consumer, so the \
         inner job could only run after the outer one returned, and the outer one \
         is waiting for it — a permanent wedge. Run the inner work inline, or \
         submit it to the other lane.",
        sender.lane
    );
}

/// One persistent large-stack worker: a NAME plus the queue feeding it.
///
/// A lane is created lazily on first use and lives for the process. Everything
/// about the mechanism — the 256 MiB stack, the single-consumer queue, the
/// explicit `None`-on-spawn-failure record, the never-dropped `Sender` — is
/// shared by every lane; a lane is an INSTANCE, not a variant.
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
/// # What the split does NOT buy: LSP requests now serialize against EACH OTHER
///
/// Stated as a limit rather than left to be inferred from "two lanes", because
/// the argument above is about the OTHER lane's work and does not carry over.
/// A lane has a single consumer, and [`LSP_LANE`]'s consumer parks in
/// `Handle::block_on(fut)` for the whole request, so `lsp_request` calls now run
/// strictly one at a time.
///
/// That is a real change, not merely a theoretical one. Before task 5772 these
/// futures were awaited on the multi-threaded tauri runtime, and
/// `textDocument/hover` — which takes only a brief `state.read().await` and never
/// touches the `eval_state` mutex that `didChange` holds across its diagnostics
/// eval — genuinely ran concurrently with an in-flight `didChange` eval.
///
/// The sharpest case is the four `spawn_blocking` arms from the module docs'
/// "What is still NOT covered". They hand their compiler work to tokio's
/// blocking pool, but the lane thread stays parked in `block_on` for the whole
/// duration — so a workspace-wide `textDocument/references` now stalls every
/// subsequent keystroke's `didChange` and `hover` behind it, while gaining
/// nothing from the lane in exchange (its own frames on the lane's stack are
/// shallow; the deep ones are on the blocking pool's ~2 MiB threads).
///
/// So the accurate claim is: the lane split protects the keystroke path from
/// ENGINE work, not from other LSP work. Bounding the remainder means either
/// keeping the four `spawn_blocking` arms off the lane, or making [`LSP_LANE`] a
/// small fixed pool of large-stack consumers instead of a single-consumer queue.
/// Both are follow-up work rather than part of this routing: a method-keyed
/// bypass couples this module to `reify-lsp`'s internal choice of which arms
/// offload — a coupling that would rot silently if that choice changed — and a
/// pool is a different concurrency design than the one this task specified,
/// needing its own reentrancy and ordering argument (LSP notifications such as
/// `didOpen`/`didChange` are order-sensitive against later requests on the same
/// document, so a pool must not reorder them).
///
/// That deferral is TRACKED, not merely narrated: task #6517 carries both
/// candidate fixes above, and records that the choice between them should be
/// made against a measurement — no benchmark of serialized-vs-concurrent
/// keystroke latency exists yet. Cited here for the same reason the module docs
/// cite #6195: a disclosed limit with no ticket behind it is
/// indistinguishable from a limit nobody intends to close.
pub(crate) struct Lane {
    /// The lane thread's name, for backtraces, `top -H` and profiler rows.
    name: &'static str,
    /// The lazily-created queue. `None` records that the OS REFUSED the mapping.
    queue: std::sync::OnceLock<Option<JobSender>>,
}

impl Lane {
    /// Declare a lane. `const` so lanes can be `static`s created at no runtime
    /// cost; the thread itself is not spawned until [`Lane::sender`] is first
    /// called.
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            queue: std::sync::OnceLock::new(),
        }
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
    /// therefore never disconnects on its own, so the lane's `for job in rx`
    /// loop parks on an empty queue rather than exiting.
    pub(crate) fn sender(&'static self) -> Option<&'static JobSender> {
        self.queue
            .get_or_init(|| {
                let (tx, rx) = std::sync::mpsc::channel::<Job>();
                let name = self.name;
                match std::thread::Builder::new()
                    .name(name.to_string())
                    .stack_size(COMPILE_STACK_SIZE)
                    .spawn(move || {
                        // Publish this thread's lane identity so
                        // `assert_not_reentrant` can tell a self-submission (a
                        // permanent wedge) from a cross-lane one (legal).
                        CURRENT_LANE.with(|l| l.set(Some(name)));
                        // Parks while the queue is empty. `rx` only ends when
                        // the `Sender` in the `OnceLock` drops, which never
                        // happens, so this loop lives as long as the process.
                        for job in rx {
                            job();
                        }
                    }) {
                    Ok(_handle) => Some(JobSender::new(name, tx)),
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to spawn {name} thread ({e}); that \
                             lane's work will run on a default-size stack instead"
                        );
                        None
                    }
                }
            })
            .as_ref()
    }
}

/// The ENGINE lane, fed only by [`post_to_worker`], whose sole production caller
/// is [`crate::eval_queue::EvalQueue`]. Named [`WORKER_THREAD_NAME`].
pub(crate) static ENGINE_LANE: Lane = Lane::new(WORKER_THREAD_NAME);

/// The LSP lane: `lsp_request` dispatch. Named [`LSP_WORKER_THREAD_NAME`].
///
/// Separate from [`ENGINE_LANE`] so a hover never queues behind a geometry
/// evaluation — see [`Lane`]'s "Why more than one lane".
pub(crate) static LSP_LANE: Lane = Lane::new(LSP_WORKER_THREAD_NAME);

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
pub async fn run_on_lsp_worker<Fut, T>(fut: Fut) -> T
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

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JobReply<T>>();
    let job: Job = Box::new(move || {
        // The catch lives INSIDE the job, so the lane's `for job in rx` loop
        // can never observe an unwind and cannot be killed by user code.
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
