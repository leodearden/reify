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
//! * `main.rs::update_source` → `commands::reload_for_watch_impl` (watch reload)
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
