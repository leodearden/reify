//! Run compile-bearing work on a dedicated OS thread with an explicit LARGE
//! stack.
//!
//! Defense-in-depth (task 5357): the GUI's synchronous compile entry points run
//! on tokio worker threads, which have the default ~2 MiB stack. Deeply-nested
//! geometry can drive `reify_compiler`'s recursive compile past that, overflowing
//! the worker stack and aborting the process. Routing the compile onto a thread
//! with a generous stack gives extra headroom on top of task 5337's
//! compiler-layer `stacker::maybe_grow` growth and recursion-depth cap.
//!
//! Relocating the compile off the tokio worker onto a plain `std` thread is also
//! strictly SAFER for the real OCCT kernel: `OcctKernelHandle::execute()` uses
//! `blocking_send`, which panics inside any tokio runtime context. A plain `std`
//! thread is never a tokio context — the same reason `debug_server::run_on_engine`
//! already spawns a `std` thread for engine work.
//!
//! # Three tiers, and what each one covers
//!
//! The tiers differ in the LIFETIME of the 256 MiB mapping, not in its size. A
//! 256 MiB stack is far above glibc's ~40 MiB thread-stack cache ceiling, so it
//! is never recycled: a per-call spawn pays a fresh `mmap` + guard-page
//! `mprotect` + `munmap` every time. Negligible against a full compile, pure
//! overhead per slider-drag frame or per keystroke — which is what makes the
//! third tier a separate thing rather than a nicety.
//!
//! **1. Per-call, SCOPED — [`run_on_large_stack`].** For the compile-bearing
//! paths: a FULL recursive compile over source the user just handed us, of
//! arbitrary nesting depth. Its scoped thread lets the closure BORROW
//! caller-stack data, so these sites need no `Arc` clone.
//!
//! * `main.rs::open_file_engine` → `commands::open_file_engine_impl`
//! * `main.rs::update_source` (the frontend-invoked Tauri command) →
//!   `commands::reload_for_watch_impl`
//! * `main.rs::create_watcher`'s `FileEvent::Changed` callback →
//!   `commands::reload_for_watch_impl`. This is the on-disk watch-reload path,
//!   a DIFFERENT entry point from `update_source` above and the
//!   highest-frequency full recompile; it also runs on the `FileWatcher`'s own
//!   `std::thread::spawn` worker (default ~2 MiB stack), so it needs the
//!   wrapper for the same reason a tokio worker does.
//!
//! **2. Per-call, FIRE-AND-FORGET — [`spawn_on_large_stack`].** For an async
//! caller that must not block its runtime worker on a join.
//!
//! * `debug_server.rs::run_on_engine` → every debug/MCP engine closure, which
//!   includes the compile-bearing `open_file` / `load_fixture` tools
//!
//! **3. PERSISTENT lanes — [`run_on_worker`] (engine) and
//! [`run_on_lsp_worker`] (LSP).** For high-frequency work, where a per-call
//! mapping is the wrong mechanism. One thread per lane for the process lifetime;
//! the per-call cost becomes a queue push and a channel round trip. The `'static`
//! bound is the price (see [`run_on_worker`]).
//!
//! The two lanes differ in their JOB TYPE, not only in their name. `ENGINE_LANE`
//! takes a BLOCKING closure `FnOnce() -> T`, submitted via [`run_on_worker`] /
//! [`dispatch`]; `LSP_LANE` takes a `Future`, submitted via
//! [`run_on_lsp_worker`] / [`dispatch_async`]. That split is a correctness
//! constraint rather than a style choice: when a lane is absent its work must
//! still run somewhere, and an async submission's fallback frame is inside the
//! tokio runtime — a future can be `.await`ed there, whereas a closure with a
//! [`tokio::runtime::Handle::block_on`] already baked in cannot, because
//! `block_on` from inside a runtime panics "Cannot start a runtime from within a
//! runtime". Taking the future and letting the lane decide how to drive it is
//! what keeps the degraded arm legal.
//!
//! * ENGINE lane — the fourteen projection / incremental-re-eval Tauri commands:
//!   `set_parameter` (per slider-drag frame), `get_initial_state`,
//!   `sync_observed_demand`, `sync_demand`, `export`, `get_source_location`,
//!   `get_entity_tree`, `get_entity_identity_map`, `get_mechanism_descriptors`,
//!   `get_def_preview`, `get_containing_definition`,
//!   `get_entity_at_source_location`, `get_active_fea_case`,
//!   `set_active_fea_case`.
//! * LSP lane — `main.rs::lsp_request` → `lsp_bridge::lsp_request_on_worker`,
//!   which fires on effectively every keystroke and cursor move.
//!
//! # What is still NOT covered
//!
//! Two boundaries, stated as limits rather than left to be inferred:
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
//! 2. **`main.rs::mcp_tool_call`** remains unrouted; it is task 5466's scope, and
//!    joins the ENGINE lane as a lane choice rather than a redesign.
//! 3. **Concurrency WITHIN a lane.** A lane has one consumer, so routing
//!    `lsp_request` onto [`LSP_LANE`] serializes LSP requests against each
//!    other, where the multi-threaded tauri runtime previously ran them
//!    concurrently. The split buys isolation from ENGINE work, not from other
//!    LSP work — see [`Lane`]'s "What the split does NOT buy" for what that
//!    costs and what bounding it would take.
//!
//! So the invariant this module establishes is: "compile-bearing and
//! high-frequency engine work, plus the inline LSP dispatch arms, run on a large
//! stack" — NOT "all engine-bearing GUI work", and NOT "all of LSP".
//!
//! # The degradation invariant, across all three tiers
//!
//! Every tier can fail to get its large stack, and every degradation arm here
//! RESOLVES — no arm needs a resource that the condition triggering it would
//! have denied, so "never lose a result, never block, never nest a runtime" is
//! true of the async lane too and not only of the two tiers that predate it.
//!
//! * The mapping-refusal arms take no new resource at all. [`run_on_large_stack`]
//!   and [`dispatch`] run their closure INLINE in the submitting frame, which is
//!   legal on any thread because those closures are runtime-agnostic by stated
//!   precondition (see [`dispatch`]); [`dispatch_async`]'s `None` arm and its
//!   no-ambient-runtime arm `.await` the future natively. That matters because
//!   an OS that has just refused a 256 MiB mapping will equally refuse a
//!   recovery thread — any thread-based fallback would be circular.
//! * The ONE arm that does ask for a resource is [`dispatch_async`]'s
//!   `SendError` recovery: the job it gets handed back carries a `Handle::block_on`
//!   and so must not run in the submitting async frame, which leaves
//!   [`spawn_on_large_stack`] — a plain `std` thread, never a runtime context.
//!   Its trigger is a DEAD LANE rather than a refused mapping, so asking for a
//!   thread is not circular there; and if even that spawn fails the job is
//!   dropped, its reply channel resolves `Err` at once, and the loud-panic arm
//!   fires.
//!
//! The one failure mode that would NOT have resolved is RE-ENTRANT submission —
//! a job submitting to the lane it is itself running on, which wedges that lane
//! and every later submitter in the process. It is a caller error rather than a
//! degradation arm, and it is rejected by [`assert_not_reentrant`] instead of
//! being left to hang: the panic fires inside the running job, is caught by that
//! job's own `catch_unwind`, and is re-raised on its submitter, so the lane
//! survives. See [`run_on_worker`]'s reentrancy section.
//!
//! The worst outcome anywhere in this module is therefore a loud panic — never a
//! silent hang, and never a nested runtime.

/// Stack size for the large-stack compile thread: 256 MiB.
///
/// A thread stack is a *virtual-address reservation*, committed lazily
/// page-by-page on first touch — so 256 MiB costs only the pages actually used
/// (small RSS), not 256 MiB resident. That is ~128x the compiler worker's 2 MiB
/// default, a generous margin for pathological geometry nesting as
/// belt-and-suspenders atop task 5337's `stacker::maybe_grow` growth and
/// recursion cap. It is the single source of truth for both helpers below.
///
/// # Per-call cost (why this is for compile-bearing paths only)
///
/// A 256 MiB stack is far above glibc's thread-stack cache ceiling (~40 MiB
/// total by default), so such a stack is never recycled: every call pays a fresh
/// `mmap` + guard-page `mprotect` + `munmap` and the matching page-table
/// teardown (tens of microseconds), and churns 256 MiB of address space. That is
/// negligible next to an actual compile, but it is pure overhead on a
/// high-frequency path — hence the scope note in the module docs.
pub const COMPILE_STACK_SIZE: usize = 256 * 1024 * 1024;

/// Thread name for [`run_on_large_stack`]'s blocking compile thread.
///
/// Named so panic backtraces, `RUST_BACKTRACE` dumps, `top -H` / `perf` rows and
/// debugger thread lists identify the compile instead of reading `<unnamed>` —
/// this module relocates exactly the work most likely to crash, so losing the
/// caller's thread identity would be an observability regression.
///
/// Kept under 15 bytes: Linux `pthread_setname_np` caps names at 15 chars + NUL
/// and `std` silently ignores the failure, so a longer name would just not show
/// up in `/proc`.
pub const COMPILE_THREAD_NAME: &str = "reify-compile";
const _: () = assert!(
    COMPILE_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for [`spawn_on_large_stack`]'s fire-and-forget engine thread.
///
/// Distinct from [`COMPILE_THREAD_NAME`] so a backtrace or profiler row
/// immediately says whether the work arrived via a Tauri command or via the
/// debug/MCP server. Same 15-byte budget as [`COMPILE_THREAD_NAME`].
pub const ENGINE_THREAD_NAME: &str = "reify-engine";
const _: () = assert!(
    ENGINE_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Thread name for the persistent ENGINE lane — [`run_on_worker`]'s thread.
///
/// Distinct from both per-call names so a backtrace or profiler row says which
/// TIER the work arrived on, not just that it is large-stack work. This is the
/// thread most worth naming: it is long-lived, so unlike the per-call threads it
/// shows up in every profiler capture, `top -H` listing and debugger thread list
/// for the process's whole life. Same 15-byte budget as [`COMPILE_THREAD_NAME`].
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
/// apart matters. Same 15-byte budget as [`COMPILE_THREAD_NAME`].
pub const LSP_WORKER_THREAD_NAME: &str = "reify-lsp-w";
const _: () = assert!(
    LSP_WORKER_THREAD_NAME.len() <= 15,
    "thread name must fit Linux's 15-byte pthread_setname_np limit"
);

/// Run `f` to completion on a dedicated OS thread with a [`COMPILE_STACK_SIZE`]
/// stack, BLOCKING the caller until it returns, and hand back its value.
///
/// This is the variant for the synchronous Tauri commands (`open_file_engine`,
/// `update_source`), which must produce the `GuiState` result inline. It uses a
/// *scoped* thread ([`std::thread::scope`] + [`std::thread::Builder::spawn_scoped`]),
/// so `f` may BORROW caller-stack data (e.g. `&state.engine`, `&path`) with no
/// `'static` bound and no `Arc` clone — the scope guarantees the thread joins
/// before this function returns, keeping the borrows valid.
///
/// Panic semantics are faithful: if `f` panics, the panic is re-raised on the
/// caller via [`std::panic::resume_unwind`] (preserving the original payload),
/// exactly as if `f` had run inline.
///
/// # Spawn-failure policy: fall back to running `f` INLINE
///
/// Requesting a 256 MiB stack makes `EAGAIN`/`ENOMEM` from `pthread_create`
/// measurably likelier than the default-stack spawns this replaced (a
/// restrictive `RLIMIT_AS`, `vm.overcommit_memory=2`, or a container memory cap
/// can all refuse the mapping). If the spawn fails, this helper logs a warning
/// and runs `f` on the caller's own stack, so the worst case is exactly the
/// pre-task-5357 behaviour (a compile on the default stack) — never a lost
/// result.
///
/// This is a DELIBERATE asymmetry with [`spawn_on_large_stack`], which surfaces
/// the `io::Error` instead. The reason is the caller shape, not inconsistency:
/// this helper is a drop-in wrapper around a call the Tauri commands used to make
/// inline, so "just make the call" is a strictly available, strictly better
/// fallback — whereas turning the failure into an `Err` would convert a
/// hardening change into a new user-visible failure mode, and panicking here
/// would unwind a Tauri command thread and leave the frontend's `invoke` promise
/// unresolved (a silently hung GUI). `spawn_on_large_stack`'s async caller has no
/// such inline option: its closure is `'static` and must not block the runtime
/// worker, so a structured `Err` is the best it can do.
pub fn run_on_large_stack<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    // `spawn_scoped` CONSUMES the closure, so park it in an `Option` the worker
    // takes from. When the spawn fails the worker never runs, `f` is therefore
    // still in the slot, and the inline fallback below can recover and call it.
    let mut slot = Some(f);
    let mut out: Option<T> = None;

    let spawn_err = std::thread::scope(|scope| {
        let worker = || {
            let f = slot
                .take()
                .expect("large-stack worker runs at most once, so `f` is present");
            out = Some(f());
        };
        match std::thread::Builder::new()
            .name(COMPILE_THREAD_NAME.to_string())
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, worker)
        {
            Ok(handle) => {
                if let Err(payload) = handle.join() {
                    // Re-raise the ORIGINAL panic payload on the caller, so
                    // behaviour is indistinguishable from running `f` inline.
                    std::panic::resume_unwind(payload);
                }
                None
            }
            Err(e) => Some(e),
        }
    });

    if let Some(e) = spawn_err {
        // The OS refused the thread. Degrade to the pre-hardening behaviour
        // rather than failing the command (see "Spawn-failure policy" above).
        eprintln!(
            "Warning: failed to spawn {COMPILE_THREAD_NAME} thread ({e}); \
             running on the caller's default-size stack instead"
        );
        let f = slot
            .take()
            .expect("`f` is untouched when the spawn itself failed");
        return f();
    }

    out.expect("the large-stack worker ran to completion, so it produced a value")
}

/// Spawn `f` on a dedicated OS thread with a [`COMPILE_STACK_SIZE`] stack WITHOUT
/// blocking the caller, returning the [`std::thread::JoinHandle`].
///
/// This is the fire-and-forget variant for async callers that must NOT block
/// their runtime worker on a join — notably `debug_server::run_on_engine`, which
/// delivers its result out-of-band via a `tokio::sync::oneshot` channel. Because
/// `f` outlives this call, it is `'static` (no borrowing of caller-stack data);
/// deliver any result through a channel captured by `f`.
///
/// Unlike [`run_on_large_stack`], the returned `io::Result` surfaces OS
/// thread-creation failure to the caller instead of falling back to an inline
/// call (`Builder::spawn` returns a `Result`, whereas `thread::spawn` panics), so
/// an async caller can map it to a structured error. There is no inline fallback
/// available here: the closure is `'static` and the caller must not block its
/// runtime worker. See `run_on_large_stack`'s "Spawn-failure policy" for why the
/// two helpers differ on purpose.
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

/// A type-erased unit of work queued to the persistent worker.
///
/// `'static` because the queue outlives every submitter, so a job can never
/// borrow submitter-stack data. The erased signature is `FnOnce()` regardless of
/// the caller's `T`: the per-call result travels back over a reply channel
/// captured INSIDE the job, not through this type.
///
/// `pub(crate)` so the in-crate tests can name it when building a SYNTHETIC
/// queue to provoke the `SendError` arm; it adds no public API surface.
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;

/// What a job sends back to its submitter: the computed value, or the panic
/// payload its body raised.
///
/// Carrying the payload rather than a flattened string is what lets the
/// submitter [`std::panic::resume_unwind`] the ORIGINAL panic, keeping
/// [`run_on_worker`]'s semantics identical to [`run_on_large_stack`]'s.
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
/// "submitting to the lane whose thread I am running on" (a permanent wedge —
/// see [`run_on_worker`]'s reentrancy section) from "submitting to the OTHER
/// lane" (perfectly legal: a different thread, which drains independently).
///
/// `pub(crate)` for the same reason as [`Job`], and additionally because it is
/// the parameter type of the [`dispatch`] / [`dispatch_async`] seams that
/// `lsp_bridge` composes against.
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
    /// [`dispatch`] / [`dispatch_async`] deterministically.
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
/// needing its own reentrancy and ordering argument.
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
                        // Same warning shape as `run_on_large_stack`'s inline
                        // fallback.
                        eprintln!(
                            "Warning: failed to spawn {name} thread ({e}); that \
                             lane's work will run on the caller's default-size \
                             stack instead"
                        );
                        None
                    }
                }
            })
            .as_ref()
    }
}

/// The ENGINE lane: the projection / incremental-re-eval Tauri commands, fed by
/// [`run_on_worker`]. Named [`WORKER_THREAD_NAME`].
pub(crate) static ENGINE_LANE: Lane = Lane::new(WORKER_THREAD_NAME);

/// The LSP lane: `lsp_request` dispatch. Named [`LSP_WORKER_THREAD_NAME`].
///
/// Separate from [`ENGINE_LANE`] so a hover never queues behind a geometry
/// evaluation — see [`Lane`]'s "Why more than one lane".
pub(crate) static LSP_LANE: Lane = Lane::new(LSP_WORKER_THREAD_NAME);

/// Run `f` to completion on the process-wide PERSISTENT large-stack thread,
/// BLOCKING the caller until it returns, and hand back its value.
///
/// This is the tier for HIGH-FREQUENCY engine work — the projection and
/// incremental-re-eval commands (`set_parameter` fires per slider-drag frame).
/// [`run_on_large_stack`] would give the same stack, but a fresh one per call:
/// 256 MiB is far above glibc's ~40 MiB thread-stack cache ceiling, so that
/// mapping is never recycled and every call pays a full `mmap` + guard-page
/// `mprotect` + `munmap` (see the cost note on [`COMPILE_STACK_SIZE`]). This
/// helper amortises all of it into ONE mapping for the process lifetime; the
/// per-call cost becomes a queue push and a channel round-trip.
///
/// The worker is a never-joined daemon: it parks on an empty queue and exits
/// with the process.
///
/// # Why `'static` (the API price of persistence)
///
/// [`run_on_large_stack`] uses a SCOPED thread, so its closure may borrow
/// caller-stack data. A persistent worker cannot: the job outlives the frame
/// that submitted it as far as the type system can see, so `f` and its result
/// must be `'static`. In practice that costs one `Arc::clone` per call at the
/// migrated sites — an atomic increment, set against the 256 MiB mapping this
/// exists to eliminate. Keeping the borrow API would need the
/// `crossbeam`/`rayon` trick of `unsafe`-transmuting the boxed job's lifetime,
/// which is not a trade worth making for this.
///
/// # Non-reentrancy: one half ENFORCED, one half documented
///
/// The queue has a SINGLE consumer, so a job that itself calls `run_on_worker`
/// cannot be answered: the inner submission only runs once the outer job
/// returns, and the outer job is waiting for it. Left unguarded that is the
/// module's worst possible outcome — the lane thread never returns to its
/// `for job in rx` loop, so the lane is dead AND every future submitter in the
/// process blocks forever in `recv()` too: a silent, unrecoverable, process-wide
/// hang.
///
/// It is therefore CHECKED. [`dispatch`] and [`dispatch_async`] call
/// [`assert_not_reentrant`], which panics when the submitting thread is the
/// target lane's own thread. The check runs inside the running job, so its panic
/// is caught by that job's `catch_unwind` and re-raised on ITS submitter: one
/// loud error, and the lane survives. The check is per-lane, so an ENGINE job
/// submitting to the LSP lane (or the reverse) is unaffected — a different
/// thread with its own consumer. None of this is reachable from the fourteen
/// migrated call sites (`commands::*_impl` are leaves); the guard is there
/// because the lane is SHARED and grows new callers — `main.rs::mcp_tool_call`
/// is already named as a future one (task 5466).
///
/// The COROLLARY is not checkable and stays a documented precondition: a caller
/// must not already hold the engine mutex, or the job would block acquiring it
/// while the caller blocks on the reply. That deadlock involves no lane identity
/// this module can observe. It is likewise unreachable from the migrated sites,
/// which take the engine lock themselves via `with_engine_lock`.
///
/// # Panic isolation (the one behavioural difference this tier requires)
///
/// Panic semantics are faithful — a panicking job re-raises its ORIGINAL payload
/// on ITS submitter via [`std::panic::resume_unwind`], just as
/// [`run_on_large_stack`] does — but the MECHANISM has to differ. Each job body
/// runs under [`std::panic::catch_unwind`] INSIDE the job, so the worker's
/// receive loop never observes an unwind and cannot be killed by user code.
///
/// The per-call helpers need none of this: there, an unwinding thread is the
/// thread that was about to be joined anyway, so its death costs exactly one
/// call. A shared worker's death would cost every FUTURE call in the process —
/// one poisoned job would silently downgrade the whole GUI to inline execution
/// on ~2 MiB tokio worker stacks, which is the hazard this module exists to
/// remove.
///
/// # Degradation policy: never lose a result, never block
///
/// Mirrors [`run_on_large_stack`]'s spawn-failure policy. If the OS refuses the
/// 256 MiB mapping, `f` runs INLINE on the caller's stack, so the worst case is
/// exactly the pre-task-5357 behaviour. If the queue is dead, `send` HANDS THE
/// JOB BACK and it likewise runs inline. And `recv` on a disconnected channel
/// returns immediately, so no path here can block on a queue nobody drains.
pub fn run_on_worker<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    dispatch(ENGINE_LANE.sender(), f)
}

/// Submit `f` to `sender`'s lane and block for its result — or, given `None`,
/// run `f` INLINE on the caller's own stack.
///
/// This is [`run_on_worker`] with its "is there a worker?" question turned into
/// a parameter, which does double duty. It makes the DEGRADED arm reachable from
/// a test — task 5357 documented the same inline-fallback policy for
/// [`run_on_large_stack`] but could not exercise it, because provoking a
/// `pthread_create` failure from a unit test is not possible, so passing `None`
/// here tests the seam instead of the OS. And it makes the submission logic
/// LANE-AGNOSTIC: a second lane is a second `Option<&JobSender>` argument, not a
/// second code path, which is what keeps "one worker design" literally true.
///
/// `pub(crate)` is deliberate and sufficient: the tests are an in-crate
/// `#[cfg(test)] mod tests`, so the seam adds no public API surface. Callers
/// outside this module want [`run_on_worker`], which supplies the engine lane.
///
/// # Precondition: `f` must be runtime-agnostic
///
/// Both degraded arms below run `f` in the SUBMITTING frame — on whatever thread
/// called in, which may or may not be inside a tokio runtime. So `f` must be
/// legal on either: no [`tokio::runtime::Handle::block_on`], no `Runtime::new`,
/// nothing that panics when a runtime is already entered. That holds for the
/// fourteen engine-lane call sites — plain sync `commands::*_impl` calls made
/// from a non-async `#[tauri::command] fn`, which Tauri runs as
/// `ExecutionContext::Blocking` on its own thread — and it is stated here as a
/// precondition rather than left as an accident of who happens to call it.
///
/// The async lane cannot honour the same precondition: an LSP future needs a
/// driver, and the only one that works is a `Handle`. That is why
/// [`dispatch_async`] takes a FUTURE and drives it itself, instead of taking a
/// closure with a `block_on` already baked in.
pub(crate) fn dispatch<F, T>(sender: Option<&JobSender>, f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let Some(sender) = sender else {
        // No lane (the OS refused the mapping; already warned once, in the
        // initialiser). Run inline — a panic here propagates naturally, so this
        // arm needs no forwarding of its own.
        return f();
    };

    // Rejected loudly rather than enqueued: submitting to the lane this thread
    // IS would wedge it forever (see `assert_not_reentrant`).
    assert_not_reentrant(sender);

    let (reply_tx, reply_rx) = std::sync::mpsc::channel::<JobReply<T>>();
    let job: Job = Box::new(move || {
        // The catch lives INSIDE the job, so the worker's `for job in rx` loop
        // can never observe an unwind and therefore cannot be killed by user
        // code. `AssertUnwindSafe` is sound here because the job OWNS its
        // captures and is consumed by this call — nothing observes them after a
        // panic — and the payload is re-raised on the submitter below rather
        // than swallowed.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        // A dropped receiver means the submitter is gone; nothing to report.
        let _ = reply_tx.send(outcome);
    });

    let send_result = sender.send(job);

    if let Err(std::sync::mpsc::SendError(job)) = send_result {
        // The lane's worker is gone. `send` returned the job unrun and the reply
        // channel is still live in this frame, so run it here: the result
        // arrives over the same channel below. Degraded, never lost.
        job();
    }

    match reply_rx.recv() {
        Ok(Ok(value)) => value,
        // Re-raise the ORIGINAL payload on the submitter, exactly as
        // `run_on_large_stack` does after joining its scoped thread.
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        // Unreachable while the job catches its own unwind: the reply channel
        // can only disconnect if the job was dropped unrun. `recv` on a
        // disconnected channel returns AT ONCE, so this is a loud failure, not
        // a block. Deliberately lane-agnostic: `dispatch` is shared by every
        // lane, and naming one of them here would misreport the other.
        Err(_) => panic!(
            "a large-stack lane dropped a job without answering: its reply \
             channel disconnected before a result arrived"
        ),
    }
}

/// Drive `fut` to completion on the persistent LSP lane WITHOUT blocking the
/// calling tokio worker, resolving to its output.
///
/// The async sibling of [`run_on_worker`], for `lsp_request` — an `async fn`
/// Tauri command that fires on effectively every keystroke and cursor move.
/// [`run_on_worker`] would park its caller in `mpsc::recv()`; on the tauri
/// runtime that pins a worker for the whole LSP round trip, which is precisely
/// what an async command must not do. Awaiting a
/// [`tokio::sync::oneshot`](tokio::sync::oneshot) reply instead RELEASES the
/// worker while the lane thread computes.
///
/// # Why this lane carries a FUTURE, not a closure
///
/// The blocking lane takes `FnOnce() -> T`; this one takes
/// `Future<Output = T>`, and the difference is a correctness constraint rather
/// than a style choice. Both lanes must be able to degrade — to run the work
/// SOMEWHERE when the lane is absent or its queue is dead — and the degraded
/// arms of an async submission necessarily run in the submitting async frame,
/// i.e. on a thread already inside the tauri runtime. A future can simply be
/// `.await`ed there. A closure that pre-bakes a
/// [`tokio::runtime::Handle::block_on`] — which is what an LSP job must do, see
/// [`dispatch_async`] — cannot: `block_on` from inside a runtime panics "Cannot
/// start a runtime from within a runtime". Taking the future and letting
/// [`dispatch_async`] decide how to drive it puts that decision with the code
/// that knows which frame the work will land in.
///
/// Everything else is shared with the blocking seam: the same [`LSP_LANE`], the
/// same boxed [`Job`], the same catch-inside-the-job protocol and [`JobReply`]
/// payload, the same panic fidelity. Only the reply channel differs.
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
/// The async counterpart of [`dispatch`], and `pub(crate)` for the same reason:
/// turning "is there a lane?" into a parameter is what makes the degraded arm
/// reachable from a test rather than requiring a real `pthread_create` failure.
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
/// This matters more here than on the blocking seam — a blocking submitter that
/// degrades merely runs slower, whereas a hung or panicking future would leave
/// the frontend's `invoke` promise unresolved forever (a silently dead editor
/// pane). Three arms, and none of them needs a resource the triggering condition
/// would deny:
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

    // Same guard as the blocking seam: a future submitted from a job already
    // running on this lane could only be driven after that job returned.
    assert_not_reentrant(sender);

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<JobReply<T>>();
    let job: Job = Box::new(move || {
        // Identical to `dispatch`'s job body apart from the driver: the catch
        // lives INSIDE the job, so the lane's `for job in rx` loop can never
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
        // Re-raise the ORIGINAL payload on the awaiting task, so panic semantics
        // are identical to every other tier's.
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
