//! `JointValue` foundation + flat-`f64` bridge helpers (PRD §5.1).
//!
//! This module is the α-pre foundation of the kinematic-constraints-completion
//! PRD.  It introduces the typed motion-value enum [`JointValue`] (which packs
//! the 1/2/3/4-component DOF storage for the seven joint kinds), the typed
//! shape-descriptor enum [`JointKind`], and the [`flatten_dofs`] /
//! [`unflatten_dofs`] bridge that converts a `&[JointValue]` to a flat
//! `Vec<f64>` (and back) for compatibility with the `&[f64]`-typed chain
//! production functions in [`crate::loop_closure`] /
//! [`crate::loop_closure_solver`].
//!
//! Correctness invariant — storage vs DOF:
//!   * `JointValue::dof_count()` is the *manifold* DOF (1, 2, 3, 3).
//!   * `JointKind::flat_len()` is the *storage* width (1, 1, 1, 1, 2, 3, 4).
//!     Sphere stores 4 quaternion components but only has 3 rotational DOF, so
//!     flatten/unflatten arithmetic uses `flat_len`, **never** `dof_count`.
//!     This makes `unflatten_dofs(flatten_dofs(v), shapes) == v` for any
//!     `v: &[JointValue]` whose shapes match.
//!
//! Production signatures (`chain_transform`, `loop_residual_twist`,
//! `chain_jacobian_fd`, `solve_loop_closure`, `solve_loop_closure_with_diagnostics`)
//! intentionally remain `&[f64]` in α-pre — PRD task γ widens those signatures.
//! Chain tests bridge via `&flatten_dofs(&vals)` at the call boundary.

/// Per-joint motion-value carrier.
///
/// Variants store the per-kind STORAGE payload (Scalar=1 f64, Cyl=2 f64,
/// Planar=3 f64, Sphere=4 f64 = quaternion w,x,y,z).  The manifold DOF count
/// (returned by [`Self::dof_count`]) is 1/2/3/**3** — Sphere has 3 rotational
/// DOF despite storing a 4-element quaternion.
#[derive(Clone, Debug, PartialEq)]
pub enum JointValue {
    /// 1-DOF, 1 f64 — prismatic, revolute, coupling, fixed.
    Scalar(f64),
    /// 2-DOF, 2 f64 — cylindrical (translation along axis, rotation about axis).
    Cyl([f64; 2]),
    /// 3-DOF, 3 f64 — planar (tx, ty, theta).
    Planar([f64; 3]),
    /// 3-DOF (manifold), 4 f64 (storage) — spherical, quaternion [w, x, y, z].
    Sphere([f64; 4]),
}

/// Shape descriptor for a joint, mirroring `joints::JOINT_KINDS` 1:1.
///
/// Used by [`JointValue::from_slice`] and [`unflatten_dofs`] to drive the
/// per-kind storage-width consumption from a flat `&[f64]` buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    Prismatic,
    Revolute,
    Coupling,
    Fixed,
    Cylindrical,
    Planar,
    Spherical,
}

/// Failure modes for the fallible bridge constructors
/// [`JointValue::from_slice`] and [`unflatten_dofs`].
#[derive(Clone, Debug, PartialEq)]
pub enum FlattenError {
    /// `dofs.len()` did not match `kind.flat_len()` in `from_slice`.
    WrongLen {
        kind: JointKind,
        expected: usize,
        actual: usize,
    },
    /// `unflatten_dofs` reached the end of `dofs` before consuming all `shapes`.
    BufferTooShort {
        consumed: usize,
        remaining_shapes: usize,
    },
    /// `unflatten_dofs` consumed all `shapes` but `dofs` had leftover trailing f64s.
    BufferTooLong {
        consumed: usize,
        leftover: usize,
    },
}

impl std::fmt::Display for FlattenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlattenError::WrongLen {
                kind,
                expected,
                actual,
            } => write!(
                f,
                "JointValue::from_slice: kind {kind:?} expects {expected} f64s, got {actual}"
            ),
            FlattenError::BufferTooShort {
                consumed,
                remaining_shapes,
            } => write!(
                f,
                "unflatten_dofs: buffer too short — consumed {consumed} f64s with {remaining_shapes} shapes still to fill"
            ),
            FlattenError::BufferTooLong { consumed, leftover } => write!(
                f,
                "unflatten_dofs: buffer too long — consumed {consumed} f64s, {leftover} trailing f64s left over"
            ),
        }
    }
}

impl std::error::Error for FlattenError {}

impl JointValue {
    /// Manifold DOF for this value (1 / 2 / 3 / 3).  Does **not** drive
    /// flatten/unflatten arithmetic — use [`JointKind::flat_len`] for that.
    pub fn dof_count(&self) -> usize {
        match self {
            JointValue::Scalar(_) => 1,
            JointValue::Cyl(_) => 2,
            JointValue::Planar(_) => 3,
            // Sphere stores 4 quaternion components but only has 3 manifold DOF.
            JointValue::Sphere(_) => 3,
        }
    }

    /// Borrow the underlying storage as a contiguous slice of f64s.  Length
    /// matches `JointKind::flat_len` for the corresponding kind (1, 2, 3, or 4).
    pub fn as_f64_slice(&self) -> &[f64] {
        unimplemented!("step-2-impl")
    }

    /// Construct from a flat `&[f64]` slice keyed by `kind`.  Returns
    /// `Err(FlattenError::WrongLen)` if `dofs.len() != kind.flat_len()`.
    pub fn from_slice(kind: JointKind, dofs: &[f64]) -> Result<Self, FlattenError> {
        let _ = (kind, dofs);
        unimplemented!("step-5-impl")
    }

    /// Project Sphere back onto the unit-quaternion manifold (L2 normalize);
    /// no-op for Scalar / Cyl / Planar.  Resets a degenerate (near-zero-norm)
    /// quaternion to identity `[1, 0, 0, 0]` rather than producing NaN.
    pub fn renormalize_quaternion(&mut self) {
        unimplemented!("step-7-impl")
    }
}

impl JointKind {
    /// Map a canonical kind string from `joints::JOINT_KINDS` to a variant.
    /// Returns `None` for any string not in that set.
    pub fn from_str(s: &str) -> Option<JointKind> {
        let _ = s;
        unimplemented!("step-3-impl")
    }

    /// Storage width (number of f64s `JointValue` of this kind occupies in
    /// the flat buffer) — 1 for prismatic/revolute/coupling/fixed, 2 for
    /// cylindrical, 3 for planar, **4** for spherical (quaternion).
    pub fn flat_len(&self) -> usize {
        unimplemented!("step-3-impl")
    }
}

/// Concatenate every `JointValue`'s storage (`as_f64_slice`) into a single
/// flat `Vec<f64>`.  Empty input → empty vec.
///
/// Round-trip law: `unflatten_dofs(&flatten_dofs(v), shapes) == Ok(v.to_vec())`
/// when `shapes[i]` matches each `v[i]`'s variant.
pub fn flatten_dofs(values: &[JointValue]) -> Vec<f64> {
    let _ = values;
    unimplemented!("step-4-impl")
}

/// Walk `shapes` in order and consume `kind.flat_len()` f64s from `dofs`
/// per shape via [`JointValue::from_slice`].  Returns
/// `Err(FlattenError::BufferTooShort)` on shortfall and
/// `Err(FlattenError::BufferTooLong)` if trailing f64s remain.
pub fn unflatten_dofs(dofs: &[f64], shapes: &[JointKind]) -> Result<Vec<JointValue>, FlattenError> {
    let _ = (dofs, shapes);
    unimplemented!("step-6-impl")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── JointValue::dof_count tests (step-1) ─────────────────────────────

    #[test]
    fn dof_count_scalar_is_1() {
        assert_eq!(JointValue::Scalar(0.0).dof_count(), 1);
    }

    #[test]
    fn dof_count_cyl_is_2() {
        assert_eq!(JointValue::Cyl([0.0, 0.0]).dof_count(), 2);
    }

    #[test]
    fn dof_count_planar_is_3() {
        assert_eq!(JointValue::Planar([0.0, 0.0, 0.0]).dof_count(), 3);
    }

    #[test]
    fn dof_count_sphere_is_3_not_4() {
        // PRD §5.1 comment: dof_count is 1|2|3|3 (manifold DOF).  Sphere
        // STORES 4 quaternion components but only has 3 rotational DOF.
        assert_eq!(JointValue::Sphere([1.0, 0.0, 0.0, 0.0]).dof_count(), 3);
    }
}
