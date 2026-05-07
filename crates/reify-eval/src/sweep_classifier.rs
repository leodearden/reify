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

use reify_types::{GeometryHandleId, GeometryOp};

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
