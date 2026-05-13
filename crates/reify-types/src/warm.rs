use std::any::Any;
use std::fmt;

/// Type-erased container for warm-start state.
///
/// Wraps a `Box<dyn Any + Send + Sync>` with an estimated size hint, allowing
/// evaluators to preserve opaque solver/kernel state across re-evaluations
/// without the evaluation engine knowing the concrete type.
///
/// The inner bound is `Send + Sync` (not just `Send`) because
/// `ComputeNodeData` holds `Option<OpaqueState>` and worker threads may
/// observe that slot through a shared `&` snapshot of
/// `EvaluationGraph::compute_nodes` — a `PersistentMap` whose `Sync` impl
/// requires `V: Sync`. Out-of-tree producers carrying a `!Sync` payload
/// (e.g. `Cell`, `RefCell`, `Rc`, non-`Sync` FFI handles) will no longer
/// compile against this type. See
/// `docs/prds/v0_3/compute-node-infrastructure.md`.
pub struct OpaqueState {
    inner: Box<dyn Any + Send + Sync>,
    estimated_size: usize,
}

impl fmt::Debug for OpaqueState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpaqueState")
            .field("estimated_size", &self.estimated_size)
            .finish_non_exhaustive()
    }
}

impl OpaqueState {
    /// Create a new `OpaqueState` wrapping the given value.
    ///
    /// `estimated_size_bytes` is a caller-provided hint of how much memory
    /// the value occupies (including heap allocations). Used by
    /// `WarmStatePool` for budget enforcement.
    ///
    /// `T: Sync` is required in addition to `Send`; see the struct-level
    /// documentation for the cross-thread rationale.
    pub fn new<T: Any + Send + Sync>(value: T, estimated_size_bytes: usize) -> Self {
        Self {
            inner: Box::new(value),
            estimated_size: estimated_size_bytes,
        }
    }

    /// Return the estimated size in bytes provided at construction.
    pub fn estimated_size_bytes(&self) -> usize {
        self.estimated_size
    }

    /// Consume the container and attempt to downcast to the concrete type.
    /// Returns `None` if the inner type does not match `T`.
    pub fn downcast<T: Any>(self) -> Option<T> {
        self.inner.downcast::<T>().ok().map(|b| *b)
    }

    /// Borrow the inner value as `&T` if the type matches.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_state_roundtrip_i32() {
        let state = OpaqueState::new(42i32, 4);
        let value = state.downcast::<i32>();
        assert_eq!(value, Some(42i32));
    }

    #[test]
    fn opaque_state_estimated_size_bytes() {
        let state = OpaqueState::new(String::from("hello"), 128);
        assert_eq!(state.estimated_size_bytes(), 128);
    }

    #[test]
    fn opaque_state_wrong_type_downcast_returns_none() {
        let state = OpaqueState::new(42i32, 4);
        let value = state.downcast::<String>();
        assert_eq!(value, None);
    }

    #[test]
    fn opaque_state_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        // Both bounds are intentional: worker threads share EvaluationGraph
        // snapshots via `&`, so the `Option<OpaqueState>` slot inside
        // `ComputeNodeData` must be `Sync`-safe, not just `Send`-safe.
        assert_send_sync::<OpaqueState>();
    }

    #[test]
    fn opaque_state_downcast_ref_success() {
        let state = OpaqueState::new(42i32, 4);
        let value_ref = state.downcast_ref::<i32>();
        assert_eq!(value_ref, Some(&42i32));
    }

    #[test]
    fn opaque_state_downcast_ref_wrong_type_returns_none() {
        let state = OpaqueState::new(42i32, 4);
        let value_ref = state.downcast_ref::<String>();
        assert_eq!(value_ref, None);
    }

    // --- WarmStartable trait tests ---

    struct MockWarmStartable {
        stored_i32: Option<i32>,
    }

    impl MockWarmStartable {
        fn new() -> Self {
            Self { stored_i32: None }
        }
    }

    impl WarmStartable for MockWarmStartable {
        fn warm_state(&self) -> Option<OpaqueState> {
            self.stored_i32.map(|v| OpaqueState::new(v, 4)) // i32 is 4 bytes
        }

        fn with_warm_state(&mut self, state: OpaqueState) {
            if let Some(v) = state.downcast::<i32>() {
                self.stored_i32 = Some(v);
            }
            // Silently ignore non-i32 values
        }
    }

    #[test]
    fn warm_startable_roundtrip_returns_correct_state() {
        let mut mock = MockWarmStartable::new();
        let state = OpaqueState::new(123i32, 8);

        mock.with_warm_state(state);
        let retrieved = mock.warm_state();

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(123i32));
    }

    #[test]
    fn warm_startable_with_warm_state_wrong_type_silently_ignores() {
        let mut mock = MockWarmStartable::new();

        // Store a String (wrong type) - should be silently ignored
        let wrong_state = OpaqueState::new(String::from("hello"), 10);
        mock.with_warm_state(wrong_state);

        // The mock only stores i32 values, so String should be ignored
        assert!(mock.warm_state().is_none());
    }

    #[test]
    fn warm_startable_warm_state_returns_none_when_no_state() {
        let mock = MockWarmStartable::new();
        assert!(mock.warm_state().is_none());
    }
}
