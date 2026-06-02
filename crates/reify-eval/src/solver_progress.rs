//! Thread-local dispatch context for solver-progress emission (task #4079).
//!
//! `run_compute_dispatch` installs a [`SolveDispatchContext`] around each
//! synchronous trampoline call via [`install_solve_dispatch_context`].  The
//! elastic-static trampoline reads the context via
//! [`current_solve_dispatch_context`] to emit per-CG-iteration progress and
//! to poll for cancellation — without changing the fixed `ComputeFn`
//! fn-pointer signature.
//!
//! # Thread safety
//! The context lives in a `thread_local!` and is therefore per-thread; it is
//! only accessed by the thread executing the synchronous dispatch.

use std::sync::Arc;

use crate::graph::CancellationHandle;

// ── Public types ─────────────────────────────────────────────────────────────

/// Per-iteration update emitted by the CG solver trampoline.
pub struct SolverProgressUpdate {
    /// Short identifier for the solver kind, e.g. `"cg"`.
    pub solver_kind: &'static str,
    /// 1-indexed iteration number.
    pub iter: u32,
    /// Residual norm at this iteration.
    pub residual: f64,
}

/// Sink for per-iteration solver-progress events.
///
/// Object-safe: each method takes `&self` (shared ref).  Implementors must be
/// `Send + Sync` so that the `Arc<dyn SolverProgressSink>` can be shared
/// across threads (the eval engine may be used from multiple threads).
pub trait SolverProgressSink: Send + Sync {
    fn on_iteration(&self, update: &SolverProgressUpdate);
}

// ── Thread-local context ─────────────────────────────────────────────────────

struct SolveDispatchContext {
    progress_sink: Option<Arc<dyn SolverProgressSink>>,
    cancel: Option<CancellationHandle>,
}

thread_local! {
    static SOLVE_DISPATCH_CONTEXT: std::cell::RefCell<Option<SolveDispatchContext>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard that clears the thread-local slot on drop.
pub struct SolveDispatchContextGuard {
    _private: (),
}

impl Drop for SolveDispatchContextGuard {
    fn drop(&mut self) {
        SOLVE_DISPATCH_CONTEXT.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

/// Install a solver-dispatch context for the current thread.  Returns a guard
/// that clears the slot when dropped (even on panic/early-return).
///
/// Callers must hold the guard alive for the duration of the trampoline call.
pub fn install_solve_dispatch_context(
    sink: Option<Arc<dyn SolverProgressSink>>,
    cancel: Option<CancellationHandle>,
) -> SolveDispatchContextGuard {
    SOLVE_DISPATCH_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(SolveDispatchContext {
            progress_sink: sink,
            cancel,
        });
    });
    SolveDispatchContextGuard { _private: () }
}

/// Snapshot of the currently installed dispatch context.
///
/// Returns `None` when no context is installed (outside a dispatch window).
/// Inside a dispatch window returns `Some((sink, cancel))` where either
/// component may itself be `None` if not set.
pub fn current_solve_dispatch_context(
) -> Option<(Option<Arc<dyn SolverProgressSink>>, Option<CancellationHandle>)> {
    SOLVE_DISPATCH_CONTEXT.with(|cell| {
        cell.borrow().as_ref().map(|ctx| {
            (
                ctx.progress_sink.clone(),
                ctx.cancel.clone(),
            )
        })
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::graph::CancellationHandle;

    use super::{
        SolverProgressSink, SolverProgressUpdate, current_solve_dispatch_context,
        install_solve_dispatch_context,
    };

    /// Recording sink: stores (iter, residual) tuples via a shared Arc<Mutex>.
    struct RecordingSink {
        records: Arc<Mutex<Vec<(u32, f64)>>>,
    }

    impl SolverProgressSink for RecordingSink {
        fn on_iteration(&self, update: &SolverProgressUpdate) {
            self.records
                .lock()
                .unwrap()
                .push((update.iter, update.residual));
        }
    }

    /// Outside any guard, `current_solve_dispatch_context` returns `None`.
    #[test]
    fn context_is_none_outside_guard() {
        let ctx = current_solve_dispatch_context();
        assert!(
            ctx.is_none(),
            "context must be None outside an installed guard"
        );
    }

    /// Inside a guard that installs a recording sink + non-cancelled handle,
    /// `current_solve_dispatch_context` returns `Some` with both components.
    /// After the guard is dropped, it returns `None` again.
    #[test]
    fn context_visible_inside_guard_and_cleared_on_drop() {
        let records: Arc<Mutex<Vec<(u32, f64)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<dyn SolverProgressSink> = Arc::new(RecordingSink {
            records: Arc::clone(&records),
        });
        let handle = CancellationHandle::new();

        // Before guard: None.
        assert!(current_solve_dispatch_context().is_none());

        {
            let _guard =
                install_solve_dispatch_context(Some(Arc::clone(&sink)), Some(handle.clone()));

            // Inside guard: Some, both components present.
            let ctx = current_solve_dispatch_context();
            assert!(ctx.is_some(), "context must be Some inside guard");

            let (got_sink, got_cancel) = ctx.unwrap();
            assert!(got_sink.is_some(), "sink component must be Some");
            assert!(got_cancel.is_some(), "cancel component must be Some");

            // Sink is reachable: emit an update via the snapshot.
            got_sink.unwrap().on_iteration(&SolverProgressUpdate {
                solver_kind: "cg",
                iter: 1,
                residual: 0.5,
            });

            // Cancel handle is not cancelled yet.
            assert!(
                !got_cancel.unwrap().is_cancelled(),
                "cancel handle must not be cancelled"
            );
        } // guard drops here

        // After guard: None again.
        assert!(
            current_solve_dispatch_context().is_none(),
            "context must be None after guard is dropped"
        );

        // Verify the emit was actually recorded via the shared Arc.
        let locked = records.lock().unwrap();
        assert_eq!(locked.len(), 1, "expected 1 recorded update");
        assert_eq!(locked[0], (1, 0.5));
    }
}
