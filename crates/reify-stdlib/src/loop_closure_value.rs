//! `JointValue` foundation + flat-`f64` unflatten helper (PRD §5.1).
//!
//! This module is the foundation of the kinematic-constraints-completion PRD.
//! It introduces the typed motion-value enum [`JointValue`] (which packs the
//! 1/2/3/4-component DOF storage for the seven joint kinds), the typed
//! shape-descriptor enum [`JointKind`], and [`unflatten_dofs`], which converts a
//! flat `&[f64]` buffer back into a `Vec<JointValue>` for the chain production
//! functions in [`crate::loop_closure`] / [`crate::loop_closure_solver`].
//!
//! Correctness invariant — storage vs DOF:
//!   * the *manifold* DOF per kind is 1 / 2 / 3 / 3.
//!   * `JointKind::flat_len()` is the *storage* width (1, 1, 1, 1, 2, 3, 4).
//!     Sphere stores 4 quaternion components but only has 3 rotational DOF, so
//!     flatten/unflatten arithmetic uses `flat_len`, never the manifold count.
//!
//! The chain production functions (`chain_transform`, `loop_residual_twist`,
//! `chain_jacobian_fd`, `solve_loop_closure`, `solve_loop_closure_with_diagnostics`)
//! take typed `&[JointValue]` directly (widened by KCC-γ); callers no longer
//! bridge through a flat `&[f64]` shim at the call boundary.

/// Per-joint motion-value carrier.
///
/// Variants store the per-kind STORAGE payload (Scalar=1 f64, Cyl=2 f64,
/// Planar=3 f64, Sphere=4 f64 = quaternion w,x,y,z).  The manifold DOF count
/// is 1/2/3/**3** — Sphere has 3 rotational DOF despite storing a 4-element
/// quaternion.
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
}

impl JointKind {
    /// Map a canonical kind string from `joints::JOINT_KINDS` to a variant.
    /// Returns `None` for any string not in that set.
    ///
    /// The 7 accepted strings mirror `crate::joints::JOINT_KINDS` exactly —
    /// if a new kind is added there, a variant must be added here too.
    ///
    /// Not an impl of `std::str::FromStr`: that trait's signature is
    /// `Result<Self, Self::Err>`, but the PRD §5.1 surface specifies an
    /// `Option<JointKind>` return so unknown-kind callers can pattern-match
    /// directly without dragging an error type through the API.
    #[allow(clippy::should_implement_trait)]
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
        // Storage length is 4 (quaternion w, x, y, z) even though the manifold DOF is 3.
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

    // ── JointKind ↔ joints::JOINT_KINDS cross-check tests (amend) ────────
    //
    // Lock the new `JointKind` enum to `crate::joints::JOINT_KINDS` — the
    // documented source of truth for joint-kind strings — so a future
    // addition to JOINT_KINDS (e.g. for a follow-up PRD) without an
    // accompanying `JointKind::from_str` arm + `flat_len` arm fails at
    // test time rather than silently mis-bridging through `unflatten_dofs`
    // (where the missing arm would emit `None` and break the chain
    // motion-value contract with no compile- or test-time signal).
    //
    // Mirrors the `is_joint_value_aligns_with_joint_kinds` /
    // `transform_at_dispatches_for_every_joint_kind` pattern that
    // already guards other JOINT_KINDS dispatch surfaces.

    #[test]
    fn joint_kind_from_str_aligns_with_joint_kinds() {
        // Iterate the canonical kind set and confirm every entry maps to
        // a `Some(JointKind::..)` variant.  The hardcoded
        // `joint_kind_from_str_maps_all_seven_canonical_strings` test
        // above only covers the 7 strings it explicitly names, so it
        // would NOT catch a new entry added to JOINT_KINDS later —
        // this assertion does.
        use crate::joints::JOINT_KINDS;
        for &kind in JOINT_KINDS {
            assert!(
                JointKind::from_str(kind).is_some(),
                "JOINT_KINDS entry '{kind}' has no matching JointKind variant — \
                 add the variant + a `from_str` arm + a `flat_len` arm, or \
                 remove '{kind}' from JOINT_KINDS."
            );
        }
    }

    #[test]
    fn joint_kind_flat_len_aligns_with_joint_kinds_arity_fixture() {
        // Single source-of-truth fixture for per-kind STORAGE width
        // (the `flat_len` driver for `unflatten_dofs`).
        // Colocated with this test so a future arity change has exactly
        // one place to update.  Iterating JOINT_KINDS catches two drift
        // modes: (1) a new JOINT_KINDS entry without a fixture row, and
        // (2) a fixture row whose width disagrees with `flat_len`.
        use crate::joints::JOINT_KINDS;
        let expected_flat_len: &[(&str, usize)] = &[
            ("prismatic", 1),
            ("revolute", 1),
            ("coupling", 1),
            ("fixed", 1),
            ("planar", 3),
            ("spherical", 4), // STORAGE: quaternion w,x,y,z (not the 3 manifold DOF).
            ("cylindrical", 2),
        ];
        for &kind in JOINT_KINDS {
            let variant = JointKind::from_str(kind).unwrap_or_else(|| {
                panic!(
                    "JOINT_KINDS entry '{kind}' has no JointKind variant — \
                     covered separately by joint_kind_from_str_aligns_with_joint_kinds"
                )
            });
            let want = expected_flat_len
                .iter()
                .find(|(k, _)| *k == kind)
                .unwrap_or_else(|| {
                    panic!(
                        "JOINT_KINDS entry '{kind}' is missing from the \
                         per-kind flat_len fixture — add ('{kind}', \
                         <storage_width>) to the `expected_flat_len` table."
                    )
                })
                .1;
            assert_eq!(
                variant.flat_len(),
                want,
                "JointKind::{variant:?}.flat_len() == {} but the per-kind \
                 fixture expects {want} for '{kind}'.",
                variant.flat_len()
            );
        }
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
    fn unflatten_dofs_reconstructs_mixed_values() {
        // unflatten_dofs(flat, shapes) reconstructs the typed values.  Mixed
        // shape covering all four variants; `flat` is the concatenation of each
        // value's storage (1 + 2 + 3 + 4 = 10 f64s).
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

        let flat = vec![0.25, 1.0, 2.0, 3.0, 4.0, 5.0, 1.0, 0.0, 0.0, 0.0];
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
}
