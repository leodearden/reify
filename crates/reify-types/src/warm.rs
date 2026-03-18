use std::any::Any;

/// Type-erased container for warm-start state.
///
/// Wraps a `Box<dyn Any + Send>` with an estimated size hint, allowing
/// evaluators to preserve opaque solver/kernel state across re-evaluations
/// without the evaluation engine knowing the concrete type.
pub struct OpaqueState {
    inner: Box<dyn Any + Send>,
    estimated_size: usize,
}

/// Trait for types that can produce and consume warm-start state.
///
/// Implementors stash solver/kernel state into an `OpaqueState` after
/// evaluation and restore it before re-evaluation for faster convergence.
/// The warm state is best-effort: if the state is the wrong type or stale,
/// the implementor can silently ignore it.
pub trait WarmStartable {
    /// Produce warm-start state from the current evaluation result.
    /// Returns `None` if this type has no warm state to donate.
    fn warm_state(&self) -> Option<OpaqueState>;

    /// Consume warm-start state before re-evaluation.
    /// Silently ignores state that is the wrong type or stale.
    fn with_warm_state(&mut self, state: OpaqueState);
}
