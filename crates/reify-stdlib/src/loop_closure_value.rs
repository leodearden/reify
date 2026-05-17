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
        match self {
            JointValue::Scalar(x) => std::slice::from_ref(x),
            JointValue::Cyl(arr) => arr.as_slice(),
            JointValue::Planar(arr) => arr.as_slice(),
            JointValue::Sphere(arr) => arr.as_slice(),
        }
    }

    /// Construct from a flat `&[f64]` slice keyed by `kind`.  Returns
    /// `Err(FlattenError::WrongLen)` if `dofs.len() != kind.flat_len()`.
    pub fn from_slice(kind: JointKind, dofs: &[f64]) -> Result<Self, FlattenError> {
        let expected = kind.flat_len();
        if dofs.len() != expected {
            return Err(FlattenError::WrongLen {
                kind,
                expected,
                actual: dofs.len(),
            });
        }
        // Length is now guaranteed to match flat_len for each kind, so the
        // array indexing below cannot panic.
        Ok(match kind {
            JointKind::Prismatic
            | JointKind::Revolute
            | JointKind::Coupling
            | JointKind::Fixed => JointValue::Scalar(dofs[0]),
            JointKind::Cylindrical => JointValue::Cyl([dofs[0], dofs[1]]),
            JointKind::Planar => JointValue::Planar([dofs[0], dofs[1], dofs[2]]),
            JointKind::Spherical => JointValue::Sphere([dofs[0], dofs[1], dofs[2], dofs[3]]),
        })
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
    ///
    /// The 7 accepted strings mirror `crate::joints::JOINT_KINDS` exactly —
    /// if a new kind is added there, a variant must be added here too.
    pub fn from_str(s: &str) -> Option<JointKind> {
        match s {
            "prismatic" => Some(JointKind::Prismatic),
            "revolute" => Some(JointKind::Revolute),
            "coupling" => Some(JointKind::Coupling),
            "fixed" => Some(JointKind::Fixed),
            "cylindrical" => Some(JointKind::Cylindrical),
            "planar" => Some(JointKind::Planar),
            "spherical" => Some(JointKind::Spherical),
            _ => None,
        }
    }

    /// Storage width (number of f64s `JointValue` of this kind occupies in
    /// the flat buffer) — 1 for prismatic/revolute/coupling/fixed, 2 for
    /// cylindrical, 3 for planar, **4** for spherical (quaternion).
    pub fn flat_len(&self) -> usize {
        match self {
            JointKind::Prismatic
            | JointKind::Revolute
            | JointKind::Coupling
            | JointKind::Fixed => 1,
            JointKind::Cylindrical => 2,
            JointKind::Planar => 3,
            // Quaternion storage: w, x, y, z (not the manifold-DOF 3).
            JointKind::Spherical => 4,
        }
    }
}

/// Concatenate every `JointValue`'s storage (`as_f64_slice`) into a single
/// flat `Vec<f64>`.  Empty input → empty vec.
///
/// Round-trip law: `unflatten_dofs(&flatten_dofs(v), shapes) == Ok(v.to_vec())`
/// when `shapes[i]` matches each `v[i]`'s variant.
pub fn flatten_dofs(values: &[JointValue]) -> Vec<f64> {
    let total: usize = values.iter().map(|v| v.as_f64_slice().len()).sum();
    let mut out = Vec::with_capacity(total);
    for v in values {
        out.extend_from_slice(v.as_f64_slice());
    }
    out
}

/// Walk `shapes` in order and consume `kind.flat_len()` f64s from `dofs`
/// per shape via [`JointValue::from_slice`].  Returns
/// `Err(FlattenError::BufferTooShort)` on shortfall and
/// `Err(FlattenError::BufferTooLong)` if trailing f64s remain.
pub fn unflatten_dofs(dofs: &[f64], shapes: &[JointKind]) -> Result<Vec<JointValue>, FlattenError> {
    let mut out = Vec::with_capacity(shapes.len());
    let mut cursor: usize = 0;
    for (i, kind) in shapes.iter().enumerate() {
        let width = kind.flat_len();
        if cursor + width > dofs.len() {
            return Err(FlattenError::BufferTooShort {
                consumed: cursor,
                remaining_shapes: shapes.len() - i,
            });
        }
        let chunk = &dofs[cursor..cursor + width];
        // `chunk.len() == width == kind.flat_len()` so from_slice cannot fail
        // here — the WrongLen branch is unreachable by construction.
        out.push(JointValue::from_slice(*kind, chunk)?);
        cursor += width;
    }
    if cursor < dofs.len() {
        return Err(FlattenError::BufferTooLong {
            consumed: cursor,
            leftover: dofs.len() - cursor,
        });
    }
    Ok(out)
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

    // ── JointValue::as_f64_slice tests (step-2) ──────────────────────────

    #[test]
    fn as_f64_slice_scalar_yields_single_element() {
        let v = JointValue::Scalar(7.0);
        let s = v.as_f64_slice();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0], 7.0);
    }

    #[test]
    fn as_f64_slice_cyl_yields_two_elements_in_order() {
        let v = JointValue::Cyl([1.5, -2.5]);
        let s = v.as_f64_slice();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], 1.5);
        assert_eq!(s[1], -2.5);
    }

    #[test]
    fn as_f64_slice_planar_yields_three_elements_in_order() {
        let v = JointValue::Planar([0.1, 0.2, 0.3]);
        let s = v.as_f64_slice();
        assert_eq!(s.len(), 3);
        assert_eq!(s, &[0.1, 0.2, 0.3]);
    }

    #[test]
    fn as_f64_slice_sphere_yields_four_elements_in_wxyz_order() {
        // Storage length is 4 (quaternion w, x, y, z) even though dof_count = 3.
        let v = JointValue::Sphere([1.0, 0.0, 0.0, 0.0]);
        let s = v.as_f64_slice();
        assert_eq!(s.len(), 4);
        assert_eq!(s, &[1.0, 0.0, 0.0, 0.0]);
    }

    // ── JointKind::from_str / flat_len tests (step-3) ────────────────────

    #[test]
    fn joint_kind_from_str_maps_all_seven_canonical_strings() {
        // Mirrors crate::joints::JOINT_KINDS 1:1.
        assert_eq!(JointKind::from_str("prismatic"), Some(JointKind::Prismatic));
        assert_eq!(JointKind::from_str("revolute"), Some(JointKind::Revolute));
        assert_eq!(JointKind::from_str("coupling"), Some(JointKind::Coupling));
        assert_eq!(JointKind::from_str("fixed"), Some(JointKind::Fixed));
        assert_eq!(JointKind::from_str("planar"), Some(JointKind::Planar));
        assert_eq!(JointKind::from_str("spherical"), Some(JointKind::Spherical));
        assert_eq!(
            JointKind::from_str("cylindrical"),
            Some(JointKind::Cylindrical)
        );
    }

    #[test]
    fn joint_kind_from_str_unknown_returns_none() {
        assert_eq!(JointKind::from_str(""), None);
        assert_eq!(JointKind::from_str("Prismatic"), None); // case-sensitive
        assert_eq!(JointKind::from_str("ball"), None);
        assert_eq!(JointKind::from_str("hinge"), None);
    }

    #[test]
    fn joint_kind_flat_len_single_dof_kinds_are_1() {
        // Prismatic/Revolute/Coupling/Fixed all carry a single f64 payload.
        assert_eq!(JointKind::Prismatic.flat_len(), 1);
        assert_eq!(JointKind::Revolute.flat_len(), 1);
        assert_eq!(JointKind::Coupling.flat_len(), 1);
        assert_eq!(JointKind::Fixed.flat_len(), 1);
    }

    #[test]
    fn joint_kind_flat_len_cylindrical_is_2() {
        assert_eq!(JointKind::Cylindrical.flat_len(), 2);
    }

    #[test]
    fn joint_kind_flat_len_planar_is_3() {
        assert_eq!(JointKind::Planar.flat_len(), 3);
    }

    #[test]
    fn joint_kind_flat_len_spherical_is_4_not_3() {
        // STORAGE width, not manifold DOF — quaternion has 4 components.
        assert_eq!(JointKind::Spherical.flat_len(), 4);
    }

    // ── flatten_dofs tests (step-4) ──────────────────────────────────────

    #[test]
    fn flatten_dofs_empty_input_returns_empty_vec() {
        let out = flatten_dofs(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn flatten_dofs_two_scalars_concatenate_in_order() {
        // Critical for the α-pre bridge shim: scalar-joint chains feed
        // chain_transform(&[f64]) via &flatten_dofs(&[Scalar,Scalar,..]).
        let out = flatten_dofs(&[JointValue::Scalar(0.3), JointValue::Scalar(0.5)]);
        assert_eq!(out, vec![0.3, 0.5]);
    }

    #[test]
    fn flatten_dofs_mixed_variants_concatenate_with_storage_widths() {
        // 1 + 2 + 3 + 4 = 10 f64s in order; Sphere contributes 4 (storage,
        // not dof_count=3).
        let out = flatten_dofs(&[
            JointValue::Scalar(1.0),
            JointValue::Cyl([2.0, 3.0]),
            JointValue::Planar([4.0, 5.0, 6.0]),
            JointValue::Sphere([1.0, 0.0, 0.0, 0.0]),
        ]);
        assert_eq!(out.len(), 10);
        assert_eq!(
            out,
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 1.0, 0.0, 0.0, 0.0]
        );
    }

    // ── JointValue::from_slice tests (step-5) ────────────────────────────

    #[test]
    fn from_slice_prismatic_single_f64_builds_scalar() {
        let v = JointValue::from_slice(JointKind::Prismatic, &[2.5]).expect("Ok");
        assert_eq!(v, JointValue::Scalar(2.5));
    }

    #[test]
    fn from_slice_revolute_single_f64_builds_scalar() {
        let v = JointValue::from_slice(JointKind::Revolute, &[1.25]).expect("Ok");
        assert_eq!(v, JointValue::Scalar(1.25));
    }

    #[test]
    fn from_slice_coupling_single_f64_builds_scalar() {
        let v = JointValue::from_slice(JointKind::Coupling, &[0.1]).expect("Ok");
        assert_eq!(v, JointValue::Scalar(0.1));
    }

    #[test]
    fn from_slice_fixed_single_f64_builds_scalar() {
        // Fixed is a 0-DOF joint but its flat_len is still 1 (motion-value
        // slot is reserved in the chain even though the value is ignored).
        let v = JointValue::from_slice(JointKind::Fixed, &[0.0]).expect("Ok");
        assert_eq!(v, JointValue::Scalar(0.0));
    }

    #[test]
    fn from_slice_cylindrical_two_f64s_builds_cyl() {
        let v = JointValue::from_slice(JointKind::Cylindrical, &[1.0, 2.0]).expect("Ok");
        assert_eq!(v, JointValue::Cyl([1.0, 2.0]));
    }

    #[test]
    fn from_slice_planar_three_f64s_builds_planar() {
        let v = JointValue::from_slice(JointKind::Planar, &[1.0, 2.0, 3.0]).expect("Ok");
        assert_eq!(v, JointValue::Planar([1.0, 2.0, 3.0]));
    }

    #[test]
    fn from_slice_spherical_four_f64s_builds_sphere() {
        let v = JointValue::from_slice(JointKind::Spherical, &[1.0, 0.0, 0.0, 0.0]).expect("Ok");
        assert_eq!(v, JointValue::Sphere([1.0, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn from_slice_wrong_length_returns_err_without_panic() {
        // Spherical wants 4 f64s — feeding 2 must NOT panic, must return Err.
        let err = JointValue::from_slice(JointKind::Spherical, &[1.0, 2.0]).unwrap_err();
        assert!(matches!(
            err,
            FlattenError::WrongLen {
                kind: JointKind::Spherical,
                expected: 4,
                actual: 2,
            }
        ));

        // Prismatic wants 1 — feeding 3 must Err.
        let err = JointValue::from_slice(JointKind::Prismatic, &[1.0, 2.0, 3.0]).unwrap_err();
        assert!(matches!(
            err,
            FlattenError::WrongLen {
                kind: JointKind::Prismatic,
                expected: 1,
                actual: 3,
            }
        ));

        // Planar wants 3 — empty slice must Err.
        let err = JointValue::from_slice(JointKind::Planar, &[]).unwrap_err();
        assert!(matches!(
            err,
            FlattenError::WrongLen {
                kind: JointKind::Planar,
                expected: 3,
                actual: 0,
            }
        ));
    }

    // ── unflatten_dofs round-trip + error tests (step-6) ─────────────────

    #[test]
    fn unflatten_dofs_round_trips_mixed_values_through_flatten() {
        // Round-trip law: unflatten_dofs(&flatten_dofs(v), shapes) == Ok(v.to_vec()).
        // Mixed shape covering all four variants.
        let values = vec![
            JointValue::Scalar(0.25),
            JointValue::Cyl([1.0, 2.0]),
            JointValue::Planar([3.0, 4.0, 5.0]),
            JointValue::Sphere([1.0, 0.0, 0.0, 0.0]),
        ];
        let shapes = vec![
            JointKind::Prismatic,
            JointKind::Cylindrical,
            JointKind::Planar,
            JointKind::Spherical,
        ];

        let flat = flatten_dofs(&values);
        let back = unflatten_dofs(&flat, &shapes).expect("round-trip ok");
        assert_eq!(back, values);
    }

    #[test]
    fn unflatten_dofs_empty_shapes_with_empty_buffer_is_ok_empty() {
        let back = unflatten_dofs(&[], &[]).expect("empty round-trip ok");
        assert!(back.is_empty());
    }

    #[test]
    fn unflatten_dofs_buffer_too_short_returns_err() {
        // Shapes ask for 1 + 4 = 5 f64s; buffer has only 3 → too short.
        let shapes = vec![JointKind::Prismatic, JointKind::Spherical];
        let flat = vec![1.0, 0.5, 0.0];
        let err = unflatten_dofs(&flat, &shapes).unwrap_err();
        assert!(matches!(err, FlattenError::BufferTooShort { .. }));
    }

    #[test]
    fn unflatten_dofs_buffer_too_long_returns_err() {
        // Shapes ask for 1 f64; buffer has 3 → 2 trailing leftover.
        let shapes = vec![JointKind::Prismatic];
        let flat = vec![1.0, 2.0, 3.0];
        let err = unflatten_dofs(&flat, &shapes).unwrap_err();
        assert!(matches!(
            err,
            FlattenError::BufferTooLong {
                consumed: 1,
                leftover: 2,
            }
        ));
    }

    // ── JointValue::renormalize_quaternion tests (step-7) ────────────────

    #[test]
    fn renormalize_quaternion_sphere_normalizes_to_unit_norm() {
        // [0, 3, 0, 4] has L2 norm 5 → expect [0, 0.6, 0, 0.8].
        let mut v = JointValue::Sphere([0.0, 3.0, 0.0, 4.0]);
        v.renormalize_quaternion();
        match v {
            JointValue::Sphere(q) => {
                assert!((q[0] - 0.0).abs() < 1e-12);
                assert!((q[1] - 0.6).abs() < 1e-12);
                assert!((q[2] - 0.0).abs() < 1e-12);
                assert!((q[3] - 0.8).abs() < 1e-12);
                // Round-trip: post-norm should now be a unit quaternion.
                let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                assert!((n - 1.0).abs() < 1e-12);
            }
            other => panic!("expected Sphere, got {other:?}"),
        }
    }

    #[test]
    fn renormalize_quaternion_sphere_degenerate_zero_norm_resets_to_identity() {
        // [0, 0, 0, 0] has zero norm — must NOT produce NaN; must reset to
        // identity [1, 0, 0, 0] instead.
        let mut v = JointValue::Sphere([0.0, 0.0, 0.0, 0.0]);
        v.renormalize_quaternion();
        assert_eq!(v, JointValue::Sphere([1.0, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn renormalize_quaternion_scalar_is_noop() {
        let mut v = JointValue::Scalar(2.5);
        v.renormalize_quaternion();
        assert_eq!(v, JointValue::Scalar(2.5));
    }

    #[test]
    fn renormalize_quaternion_cyl_is_noop() {
        let mut v = JointValue::Cyl([0.5, 1.5]);
        v.renormalize_quaternion();
        assert_eq!(v, JointValue::Cyl([0.5, 1.5]));
    }

    #[test]
    fn renormalize_quaternion_planar_is_noop() {
        let mut v = JointValue::Planar([1.0, 2.0, 3.0]);
        v.renormalize_quaternion();
        assert_eq!(v, JointValue::Planar([1.0, 2.0, 3.0]));
    }
}
