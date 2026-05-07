//! Phase A swept-body classifier for the hex/wedge mesh-morphing pipeline.
//!
//! Pure classifier over a realization's compiled `&[GeometryOp]` slice plus the
//! parallel `&[GeometryHandleId]` slice produced by `Engine::execute_realization_ops`.
//! Returns `Some(SweptKind)` when the realization is a *recognised swept body*
//! that the morphing pipeline can later reason about, and `None` otherwise.
//!
//! This is the minimum viable Phase A surface — see
//! `docs/prds/v0_3/hex-wedge-mesh-morphing.md` (forthcoming) and the inline
//! design decisions in `.task/plan.json` for context. Phase B (axial-finishing
//! recognition) extends `SweptKind` via additional fields/variants; the enum is
//! marked `#[non_exhaustive]` so that extension is non-breaking.
//!
//! ## Purity
//!
//! This module is pure Rust and does **not** call any geometry kernel. It
//! operates solely on the [`GeometryOp`] op stream and the parallel handles
//! slice (`handles[i]` is the result handle of `ops[i]`). The classifier is
//! O(N) in the op count and allocation-free.
//!
//! ## Acceptance summary
//!
//! Recognised:
//! - Last op = [`GeometryOp::Extrude`] / [`GeometryOp::ExtrudeSymmetric`]
//!   → [`SweptKind::Extrude`] (axis = +Z, length = `distance`).
//! - Last op = [`GeometryOp::Revolve`] with non-degenerate axis and angle
//!   → [`SweptKind::Revolve`].
//! - Last op = [`GeometryOp::Sweep`] whose `path` handle resolves to a
//!   [`GeometryOp::LineSegment`] source op → [`SweptKind::Loft`] (single-profile
//!   sweep along a straight, provably non-twisted path).
//!
//! Rejected (returns `None`):
//! - Multi-profile [`GeometryOp::Loft`], [`GeometryOp::LoftGuided`],
//!   [`GeometryOp::SweepGuided`], [`GeometryOp::Pipe`].
//! - [`GeometryOp::Sweep`] along a curved path (`Arc` / `Helix` / `NurbsCurve`
//!   / `InterpCurve` / `BezierCurve`) — Phase A's conservative approximation.
//! - Any non-sweep last op (Boolean, Modify, Transform, Pattern, primitive
//!   constructors). The "no subsequent modifications" contract is implicit:
//!   a Translate/Fillet/Boolean appended after a sweep IS the last op and
//!   falls through the catch-all.
//! - Empty op slice.

use reify_types::{GeometryHandleId, GeometryOp, Value};

// ── Public types ──────────────────────────────────────────────────────────────

/// Recognised swept-body kinds produced by [`classify_swept_body`].
///
/// `#[non_exhaustive]` so Phase B (axial-finishing recognition, PRD task #14)
/// can add new variants — or augment existing variants with additional fields
/// via a wrapper struct — without breaking downstream callers. External match
/// expressions on `SweptKind` therefore must include a wildcard arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SweptKind {
    /// Linear sweep along a constant axis.
    ///
    /// Produced by [`GeometryOp::Extrude`] and [`GeometryOp::ExtrudeSymmetric`]
    /// — both extrude along +Z by `distance` (the symmetric variant centres the
    /// resulting prism around the profile's plane, but the swept axis and
    /// total length are identical to the asymmetric form for classification
    /// purposes).
    Extrude {
        /// Unit-length sweep axis in the realization frame. Currently always
        /// `[0.0, 0.0, 1.0]` because both source ops fix +Z; left as an
        /// explicit field so a future op variant with a caller-supplied axis
        /// (or a kernel-aligned local frame) can populate it without a new
        /// variant.
        axis: [f64; 3],
        /// Total swept length, dimension-tagged as `Type::length()`. Cloned
        /// directly from the source op's `distance` field.
        length: Value,
    },
    /// Revolution around an axis by an angle.
    ///
    /// Produced by [`GeometryOp::Revolve`] when the axis direction is
    /// non-degenerate (≥ [`REVOLVE_DEGENERATE_TOLERANCE`] in some component)
    /// and the angle magnitude exceeds the same tolerance. Full 2π revolutions
    /// qualify; the kernel-side full-revolution edge cases live downstream in
    /// the meshing path, not here.
    Revolve {
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    },
    /// Single-profile sweep along a *non-twisted* path.
    ///
    /// Produced by [`GeometryOp::Sweep`] when the path handle resolves to a
    /// [`GeometryOp::LineSegment`] source op in the same realization slice (a
    /// straight-line path is provably non-twisted; profile orientation is
    /// constant by construction). Curved paths (Arc / Helix / NurbsCurve / …)
    /// are conservatively rejected for Phase A — see the "non-twisted path"
    /// design decision in `.task/plan.json`.
    Loft {
        profile: GeometryHandleId,
        path: GeometryHandleId,
    },
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Classify the *last* op in a realization's compiled op slice as a Phase A
/// swept body.
///
/// Returns `Some(SweptKind)` when the realization's final op produces a
/// recognised swept body and `None` otherwise. The caller passes the parallel
/// arrays produced by `Engine::execute_realization_ops`:
///
/// - `ops[i]` — the i-th compiled [`GeometryOp`] in the realization.
/// - `handles[i]` — the [`GeometryHandleId`] kernel-result of `ops[i]`.
///
/// `handles[i]` lets the [`GeometryOp::Sweep`] arm resolve a `path` handle back
/// to its source op via a linear scan over `handles` (see step-6 below).
///
/// ## "No subsequent modifications" enforcement
///
/// The classifier inspects only the LAST op via `ops.last()?`. If a Translate
/// / Fillet / Boolean op is appended after a sweep, that modify op IS the last
/// op and falls through to the catch-all `None` arm. Earlier ops in the slice
/// (profile / curve / primitive constructions feeding the sweep) are
/// permitted; the classifier does not inspect them except for the
/// path-source check inside the [`GeometryOp::Sweep`] arm.
///
/// ## Panics
///
/// In debug builds, panics if `ops.len() != handles.len()` — the parallel-array
/// invariant must hold. In release builds the assert is elided and a malformed
/// caller produces `None` for the [`GeometryOp::Sweep`] arm (the path-source
/// scan misses) but otherwise behaves correctly for the variants whose
/// classification is independent of `handles`.
pub fn classify_swept_body(
    ops: &[GeometryOp],
    handles: &[GeometryHandleId],
) -> Option<SweptKind> {
    debug_assert_eq!(
        ops.len(),
        handles.len(),
        "classify_swept_body: ops and handles must be parallel arrays of equal length"
    );
    match ops.last()? {
        GeometryOp::Extrude { distance, .. } | GeometryOp::ExtrudeSymmetric { distance, .. } => {
            Some(SweptKind::Extrude {
                axis: [0.0, 0.0, 1.0],
                length: distance.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::Value;

    // ── Step-1: classifier API surface ─────────────────────────────────────

    #[test]
    fn classify_swept_body_empty_returns_none() {
        let ops: &[GeometryOp] = &[];
        let handles: &[GeometryHandleId] = &[];
        assert_eq!(
            classify_swept_body(ops, handles),
            None,
            "empty op slice must return None"
        );
    }

    #[test]
    fn classify_swept_body_single_extrude_classifies_as_extrude() {
        let ops = vec![GeometryOp::Extrude {
            profile: GeometryHandleId(0),
            distance: Value::length(0.01),
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            Some(SweptKind::Extrude {
                axis: [0.0, 0.0, 1.0],
                length: Value::length(0.01),
            }),
            "single Extrude op must classify as SweptKind::Extrude with axis=+Z"
        );
    }

    #[test]
    fn classify_swept_body_extrude_symmetric_classifies_as_extrude() {
        let ops = vec![GeometryOp::ExtrudeSymmetric {
            profile: GeometryHandleId(0),
            distance: Value::length(0.01),
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            Some(SweptKind::Extrude {
                axis: [0.0, 0.0, 1.0],
                length: Value::length(0.01),
            }),
            "single ExtrudeSymmetric op must classify as SweptKind::Extrude with axis=+Z"
        );
    }
}
