//! FEA-solver warm-state shape and OpaqueState conversion (PRD task #14).
//!
//! Per PRD `docs/prds/v0_3/structural-analysis-fea.md` task #14, the
//! Jacobi-CG warm state is "just `u_0` — preconditioner is trivial to
//! recompute" (full direct-solve symbolic-factorization caching is
//! explicitly out of scope for v0.3). This module owns the engine-
//! integration surface (`OpaqueState` conversion) so the pure-numerics
//! `solver.rs` stays free of `reify-types` dependency surface.

/// Warm-start state for the Jacobi-preconditioned CG solver.
///
/// Carries the displacement vector `u` from a previous solve so it can be
/// passed as the CG initial guess `u_0` on the next solve. The Jacobi
/// preconditioner itself is trivially recomputed from `K`, so it is not
/// part of the state.
#[derive(Debug, Clone, PartialEq)]
pub struct CgWarmState {
    /// Displacement vector from the previous solve, used as `u_0` on the
    /// next call.
    pub u: Vec<f64>,
}

impl CgWarmState {
    /// Construct a warm state from a displacement vector.
    pub fn from_displacement(u: Vec<f64>) -> Self {
        Self { u }
    }

    /// Estimated heap-payload size in bytes for `WarmStatePool` budget
    /// enforcement. Counts only the `Vec<f64>` payload (`u.len() *
    /// size_of::<f64>()`); the constant ~24-byte `Vec` heap header and the
    /// struct's own stack overhead are negligible relative to typical FEA
    /// solution vectors (10⁴ – 10⁶ DOFs).
    pub fn estimated_size_bytes(&self) -> usize {
        self.u.len() * std::mem::size_of::<f64>()
    }

    /// Wrap this warm state in an `OpaqueState` for storage in
    /// `WarmStatePool`. The size hint is `estimated_size_bytes()`.
    pub fn into_opaque_state(self) -> reify_types::OpaqueState {
        let bytes = self.estimated_size_bytes();
        reify_types::OpaqueState::new(self, bytes)
    }

    /// Attempt to recover a `CgWarmState` from an `OpaqueState`. Returns
    /// `None` if the inner type is not `CgWarmState` (the caller should
    /// silently treat that as a cold start, per the `WarmStartable`
    /// best-effort contract).
    pub fn from_opaque_state(state: reify_types::OpaqueState) -> Option<Self> {
        state.downcast::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CgWarmState::from_displacement(u)` → `into_opaque_state()` →
    /// `from_opaque_state()` round-trips the displacement vector unchanged.
    /// Also pins the `estimated_size_bytes` formula
    /// (`u.len() * size_of::<f64>()`).
    #[test]
    fn cg_warm_state_round_trips_through_opaque_state() {
        let u = vec![1.0_f64, 2.0, 3.0];
        let ws = CgWarmState::from_displacement(u.clone());
        let opaque = ws.into_opaque_state();
        let restored = CgWarmState::from_opaque_state(opaque).expect("downcast");
        assert_eq!(restored.u, u);

        assert_eq!(
            CgWarmState::from_displacement(vec![0.0_f64; 5]).estimated_size_bytes(),
            5 * std::mem::size_of::<f64>(),
        );
    }
}
