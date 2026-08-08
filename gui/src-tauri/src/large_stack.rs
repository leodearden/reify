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
//! # Which entry points are in scope (and which deliberately are NOT)
//!
//! WRAPPED — the compile-bearing paths, i.e. the ones that run a FULL recursive
//! compile over source the user just handed us, of arbitrary nesting depth:
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
//! * `debug_server.rs::run_on_engine` → every debug/MCP engine closure, which
//!   includes the compile-bearing `open_file` / `load_fixture` tools
//!
//! NOT WRAPPED, as a deliberate scope choice for task 5357 — `set_parameter`,
//! `get_initial_state`, `export`, `get_entity_tree`, and the other projection /
//! incremental-re-eval commands. Two reasons, stated honestly:
//!
//! 1. The hazard task 5337 diagnosed is in the full compile. The commands above
//!    instead walk or project a graph a PRIOR compile already built, so they are
//!    not the diagnosed overflow site. This is not a proof that they cannot
//!    recurse deeply — a traversal of an already-deep structure can — only that
//!    they are outside the failure this task hardens.
//! 2. `set_parameter` is the highest-frequency engine entry point (it fires per
//!    slider-drag frame). A per-call [`COMPILE_STACK_SIZE`] mapping is the wrong
//!    mechanism there (see the cost note on [`COMPILE_STACK_SIZE`]); covering
//!    that path wants a persistent large-stack worker thread, not a fresh spawn
//!    per frame. That is tracked as follow-up work rather than bolted on here.
//!
//! So: "compile-bearing GUI work runs on a large stack" is the invariant this
//! module establishes — NOT "all engine-bearing GUI work".

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

/// Thread name for [`run_on_worker`]'s single PERSISTENT large-stack thread.
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
type Job = Box<dyn FnOnce() + Send + 'static>;

/// The submit end of the persistent worker's job queue.
///
/// The [`std::sync::Mutex`] is defensive rather than required —
/// `mpsc::Sender<T>` is `Sync` on current `std`, but nothing here needs to
/// depend on that. The lock is held only across a `send` of an already-boxed
/// job, so no user code ever runs under it and it is never held longer than a
/// queue push.
type JobSender = std::sync::Mutex<std::sync::mpsc::Sender<Job>>;

/// The process-wide persistent worker, created at most once.
///
/// [`std::sync::OnceLock`] gives lazy creation (a session that never touches the
/// engine never pays for the 256 MiB mapping) and exactly-once semantics.
/// `None` records that the OS REFUSED the mapping, so every call degrades to
/// inline execution instead of retrying a spawn that will keep failing.
///
/// Holding the `Sender` here forever is deliberate: the channel therefore never
/// disconnects on its own, so the worker's `for job in rx` loop parks on an
/// empty queue rather than exiting.
static WORKER: std::sync::OnceLock<Option<JobSender>> = std::sync::OnceLock::new();

/// Lazily create the persistent worker, yielding its queue — or `None` if the OS
/// refused the [`COMPILE_STACK_SIZE`] mapping.
///
/// The spawn failure is recorded EXPLICITLY as `None` rather than inferred from
/// a subsequently-dead channel: nothing documents that `Builder::spawn` drops
/// the closure it could not run, and a leaked `Receiver` would make every `send`
/// succeed into a queue nobody drains — i.e. a hang, the one outcome this module
/// must never produce.
fn worker_sender() -> Option<&'static JobSender> {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            match std::thread::Builder::new()
                .name(WORKER_THREAD_NAME.to_string())
                .stack_size(COMPILE_STACK_SIZE)
                .spawn(move || {
                    // Parks while the queue is empty. `rx` only ends when the
                    // `Sender` in `WORKER` drops, which never happens, so this
                    // loop lives as long as the process.
                    for job in rx {
                        job();
                    }
                }) {
                Ok(_handle) => Some(std::sync::Mutex::new(tx)),
                Err(e) => {
                    // Same warning shape as `run_on_large_stack`'s inline fallback.
                    eprintln!(
                        "Warning: failed to spawn {WORKER_THREAD_NAME} thread ({e}); \
                         large-stack engine work will run on the caller's \
                         default-size stack instead"
                    );
                    None
                }
            }
        })
        .as_ref()
}

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
/// # Non-reentrancy (and its corollary)
///
/// The queue has a SINGLE consumer, so a job that itself calls `run_on_worker`
/// would deadlock: the inner submission can only run after the outer job
/// returns. By the same argument, a caller must NOT already hold the engine
/// mutex — the job would block acquiring it while the caller blocks on the
/// reply. Neither is reachable from the migrated call sites (`commands::*_impl`
/// are leaves that take the engine lock themselves via `with_engine_lock`, which
/// is already non-reentrant for exactly these callers), so this is documented
/// rather than enforced: a guard's failure mode would be a hang that poisons the
/// shared worker for every other caller in the process.
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
    let Some(sender) = worker_sender() else {
        // The OS refused the mapping (already warned once, in the initialiser).
        return f();
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel::<T>();
    let job: Job = Box::new(move || {
        // A dropped receiver means the submitter is gone; nothing to report.
        let _ = reply_tx.send(f());
    });

    // Poisoning is meaningless for a `Sender` — it holds no invariant a panic
    // could leave broken — so recover the guard rather than propagating.
    let send_result = sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(job);

    if let Err(std::sync::mpsc::SendError(job)) = send_result {
        // The worker is gone. `send` returned the job unrun and the reply
        // channel is still live in this frame, so run it here: the result
        // arrives over the same channel below. Degraded, never lost.
        job();
    }

    reply_rx
        .recv()
        .expect("large-stack worker died without answering: it unwound mid-job")
}
