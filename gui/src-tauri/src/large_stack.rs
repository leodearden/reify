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

/// Stack size for the large-stack compile thread: 256 MiB.
///
/// A thread stack is a *virtual-address reservation*, committed lazily
/// page-by-page on first touch — so 256 MiB costs only the pages actually used
/// (small RSS), not 256 MiB resident. That is ~128x the compiler worker's 2 MiB
/// default, a generous margin for pathological geometry nesting as
/// belt-and-suspenders atop task 5337's `stacker::maybe_grow` growth and
/// recursion cap. It is the single source of truth for both helpers below.
pub const COMPILE_STACK_SIZE: usize = 256 * 1024 * 1024;

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
pub fn run_on_large_stack<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(COMPILE_STACK_SIZE)
            .spawn_scoped(scope, f)
            .expect("failed to spawn large-stack thread")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
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
/// thread-creation failure to the caller instead of panicking (`Builder::spawn`
/// returns a `Result`, whereas `thread::spawn` panics), so an async caller can
/// map it to a structured error.
pub fn spawn_on_large_stack<F>(f: F) -> std::io::Result<std::thread::JoinHandle<()>>
where
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .stack_size(COMPILE_STACK_SIZE)
        .spawn(f)
}
