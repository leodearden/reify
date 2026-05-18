//! Featherstone 6D spatial-vector primitives.
//!
//! Implements the spatial-vector core from `docs/prds/v0_3/rigid-body-dynamics.md`
//! §10 Phase 1 (RBD-γ), consumed by RBD-δ (motion subspace) and RBD-ε (RNEA).
//! All math is pure-Rust `f64` numerics — no Reify-level `Value` dispatch and
//! no heavyweight linalg dependency (triple-nested-loop multiply on `[f64; N]`
//! is plenty fast for the small mechanism sizes targeted in v0.3).
//!
//! # Conventions (Featherstone, *Rigid Body Dynamics Algorithms*, 2008)
//!
//! * **Spatial-vector ordering** (§2.4 motion-vector convention): angular
//!   first, linear second — `[ω_x, ω_y, ω_z, v_x, v_y, v_z]`. The PRD §5.1
//!   inline literal `[ω; v]` matches. Spatial *force* vectors reuse the same
//!   storage but interpret `[0..3]` as torque τ and `[3..6]` as force F.
//! * **Matrix storage**: 6×6 transforms / inertias are row-major `[f64; 36]`.
//! * **Quaternions**: `(w, x, y, z)` unit-quat ordering, scalar first, matching
//!   `reify_types::Value::Orientation`.

/// A 6D spatial vector in Featherstone motion-vector ordering
/// `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear second).
///
/// Used for both spatial *motion* vectors (velocity, acceleration) and spatial
/// *force* vectors (where `[0..3]` is torque τ and `[3..6]` is force F); the
/// interpretation is fixed by the operator, not the storage.
///
/// `PartialEq` is bit-wise on the underlying `f64`s — numerical comparisons in
/// tests use an entrywise tolerance helper, never derived equality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialVector6([f64; 6]);

impl SpatialVector6 {
    /// The zero spatial vector (six zeros).
    pub fn zero() -> Self {
        SpatialVector6([0.0; 6])
    }

    /// Construct from a raw `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` array.
    pub fn from_array(a: [f64; 6]) -> Self {
        SpatialVector6(a)
    }

    /// Construct from separate angular and linear 3-vectors.
    pub fn from_angular_linear(angular: [f64; 3], linear: [f64; 3]) -> Self {
        SpatialVector6([
            angular[0], angular[1], angular[2], linear[0], linear[1], linear[2],
        ])
    }

    /// The raw `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` storage.
    pub fn as_array(&self) -> [f64; 6] {
        self.0
    }

    /// The angular part `[ω_x, ω_y, ω_z]` (indices `0..3`).
    pub fn angular(&self) -> [f64; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// The linear part `[v_x, v_y, v_z]` (indices `3..6`).
    pub fn linear(&self) -> [f64; 3] {
        [self.0[3], self.0[4], self.0[5]]
    }
}
