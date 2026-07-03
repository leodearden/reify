use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cooperative-cancellation handle for an in-flight ComputeNode dispatch.
///
/// A thin wrapper around `Arc<AtomicBool>`. Cloning shares the same
/// underlying flag, so cancelling via any clone propagates to all holders
/// (including graph snapshots taken mid-dispatch). See
/// `docs/prds/v0_3/compute-node-contract.md` §2 for the full contract.
///
/// Both `cancel()` and `is_cancelled()` use `Ordering::Relaxed`: the flag is
/// a one-shot monotonic signal (false → true; never resets within a handle's
/// lifetime). There is no other memory operation whose ordering needs to be
/// enforced relative to this flag, so stronger orderings buy nothing.
#[derive(Debug, Clone)]
pub struct CancellationHandle {
    inner: Arc<AtomicBool>,
}

#[allow(clippy::new_without_default)] // Default intentionally omitted: keeps API minimal and leaves room to swap inner to a non-Default-able primitive (e.g. tokio_util::sync::CancellationToken) — see compute-node-contract.md §2
impl CancellationHandle {
    /// Create a new, non-cancelled handle.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation. All clones of this handle will observe the change.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::Relaxed);
    }

    /// Returns `true` if `cancel()` has been called on this handle or any clone.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::Relaxed)
    }
}
