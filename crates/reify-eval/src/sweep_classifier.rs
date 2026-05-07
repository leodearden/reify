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

use std::collections::HashMap;

use reify_types::{GeometryHandleId, GeometryOp, Value};

/// Tolerance for treating a [`GeometryOp::Revolve`]'s axis or angle as
/// degenerate.
///
/// A revolve is rejected when:
/// - every component of `axis_dir` has magnitude `< REVOLVE_DEGENERATE_TOLERANCE`
///   (zero-length axis vector), or
/// - `angle_rad.abs() < REVOLVE_DEGENERATE_TOLERANCE` (no rotation).
///
/// `1e-12` matches the project's general geometric-tolerance convention: tight
/// enough to catch genuine zero-vector / zero-angle degenerates without
/// rejecting legitimate near-axis-aligned values.
const REVOLVE_DEGENERATE_TOLERANCE: f64 = 1e-12;

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

/// Runtime table mapping geometry handle ids to their Phase A swept-body
/// classification.
///
/// Populated by `Engine::execute_realization_ops` after a successful
/// realization completes (keyed by the realization's final handle — i.e. the
/// last entry in `step_handles[handle_start..]`). Cleared and repopulated on
/// every `build()` / `build_snapshot()` / `tessellate_realizations()` /
/// `tessellate_snapshot()` call (per-build, not per-realization). Mirrors the
/// `FeatureTagTable` / `TopologyAttributeTable` shape — same four-method API
/// (`record` / `lookup` / `len` / `is_empty`) and the same last-write-wins
/// semantics on duplicate-id `record` calls.
#[derive(Debug, Default)]
pub struct SweptKindTable {
    entries: HashMap<GeometryHandleId, SweptKind>,
}

impl SweptKindTable {
    /// Record that geometry handle `id` is the realization-final handle of a
    /// recognised swept body of `kind`.
    ///
    /// Overwrites any prior entry for the same id (last-write-wins, matching
    /// `FeatureTagTable::record` and `TopologyAttributeTable::record`). Phase A
    /// callers (the engine post-realization wiring) should never produce
    /// duplicate keys because each successful realization writes its own
    /// distinct final handle, but the contract is recorded here for symmetry.
    pub fn record(&mut self, id: GeometryHandleId, kind: SweptKind) {
        self.entries.insert(id, kind);
    }

    /// Look up the swept-body kind for a given handle, if any.
    pub fn lookup(&self, id: GeometryHandleId) -> Option<&SweptKind> {
        self.entries.get(&id)
    }

    /// Number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
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
        GeometryOp::Revolve {
            axis_origin,
            axis_dir,
            angle_rad,
            ..
        } => {
            // Reject zero-length axis vector and zero-angle revolves; any
            // other angle (including the full 2π case) qualifies.
            if axis_dir
                .iter()
                .all(|c| c.abs() < REVOLVE_DEGENERATE_TOLERANCE)
                || angle_rad.abs() < REVOLVE_DEGENERATE_TOLERANCE
            {
                None
            } else {
                Some(SweptKind::Revolve {
                    axis_origin: *axis_origin,
                    axis_dir: *axis_dir,
                    angle_rad: *angle_rad,
                })
            }
        }
        GeometryOp::Sweep { profile, path } => {
            // Resolve `path` back to its source op via the parallel handles
            // slice. We use an explicit linear scan (no HashMap) because the
            // classifier runs once per realization and N is small (the op
            // count is bounded by the realization size); allocation-free is
            // worth the O(N) factor.
            let path_source = handles
                .iter()
                .position(|h| h == path)
                .and_then(|i| ops.get(i));
            match path_source {
                Some(GeometryOp::LineSegment { .. }) => Some(SweptKind::Loft {
                    profile: *profile,
                    path: *path,
                }),
                // Curved paths (Arc / Helix / NurbsCurve / InterpCurve /
                // BezierCurve) and unresolvable handles fall through to None.
                _ => None,
            }
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

    // ── Step-3: Revolve happy paths and degenerate-axis/angle rejection ────

    #[test]
    fn classify_swept_body_revolve_partial_angle_classifies() {
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            Some(SweptKind::Revolve {
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            }),
            "partial-angle revolve with non-degenerate axis must classify as SweptKind::Revolve"
        );
    }

    #[test]
    fn classify_swept_body_revolve_full_2pi_classifies() {
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [1.0, 2.0, 3.0],
            axis_dir: [0.0, 1.0, 0.0],
            angle_rad: 2.0 * std::f64::consts::PI,
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            Some(SweptKind::Revolve {
                axis_origin: [1.0, 2.0, 3.0],
                axis_dir: [0.0, 1.0, 0.0],
                angle_rad: 2.0 * std::f64::consts::PI,
            }),
            "full 2π revolve must still classify as SweptKind::Revolve (kernel handles full-revolution edge cases downstream)"
        );
    }

    #[test]
    fn classify_swept_body_revolve_degenerate_axis_returns_none() {
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "revolve with all-zero axis_dir must be rejected (degenerate axis)"
        );
    }

    #[test]
    fn classify_swept_body_revolve_degenerate_angle_returns_none() {
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: 0.0,
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "revolve with zero angle_rad must be rejected (degenerate angle)"
        );
    }

    // ── Step-5: Sweep / Loft path-source resolution and rejection ─────────

    #[test]
    fn classify_swept_body_sweep_with_line_segment_path_classifies_as_loft() {
        // Two-op slice: op[0] is a LineSegment path constructor whose result
        // handle is GeometryHandleId(1); op[1] is a Sweep that consumes that
        // path handle. The classifier must trace path → LineSegment via the
        // parallel handles slice and accept the sweep as a non-twisted Loft.
        let ops = vec![
            GeometryOp::LineSegment {
                x1: 0.0,
                y1: 0.0,
                z1: 0.0,
                x2: 0.0,
                y2: 0.0,
                z2: 0.01,
            },
            GeometryOp::Sweep {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            Some(SweptKind::Loft {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            }),
            "Sweep along a LineSegment-source path must classify as SweptKind::Loft"
        );
    }

    #[test]
    fn classify_swept_body_sweep_with_arc_path_returns_none() {
        // Same shape but the path handle resolves to an Arc constructor.
        // Phase A conservatively rejects any curved path source.
        let ops = vec![
            GeometryOp::Arc {
                center: [0.0, 0.0, 0.0],
                radius: 0.005,
                start_angle: 0.0,
                end_angle: std::f64::consts::PI,
                axis: [0.0, 0.0, 1.0],
            },
            GeometryOp::Sweep {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "Sweep along an Arc-source path must be rejected (Phase A: only LineSegment paths qualify)"
        );
    }

    #[test]
    fn classify_swept_body_geometry_op_loft_multi_profile_returns_none() {
        // GeometryOp::Loft is multi-profile by construction; explicitly
        // rejected for Phase A even though SweptKind::Loft exists (the latter
        // names single-profile Sweep-along-line-segment, per the design
        // decision in .task/plan.json).
        let ops = vec![GeometryOp::Loft {
            profiles: vec![GeometryHandleId(0), GeometryHandleId(1)],
        }];
        let handles = vec![GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "GeometryOp::Loft (multi-profile) must be rejected for Phase A"
        );
    }

    // ── Step-7: "no subsequent modifications" contract ────────────────────
    // These tests pin the implicit contract that any post-sweep modify op
    // (Translate / Fillet / Boolean / …) sits on top of the sweep as the
    // *last* op, and the top-level `match ops.last()?` returns None for it.

    #[test]
    fn classify_swept_body_extrude_followed_by_translate_returns_none() {
        let ops = vec![
            GeometryOp::Extrude {
                profile: GeometryHandleId(0),
                distance: Value::length(0.01),
            },
            GeometryOp::Translate {
                target: GeometryHandleId(1),
                dx: 0.01,
                dy: 0.0,
                dz: 0.0,
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "Extrude followed by Translate is no longer a recognised swept body (last op is Translate)"
        );
    }

    #[test]
    fn classify_swept_body_extrude_followed_by_fillet_returns_none() {
        let ops = vec![
            GeometryOp::Extrude {
                profile: GeometryHandleId(0),
                distance: Value::length(0.01),
            },
            GeometryOp::Fillet {
                target: GeometryHandleId(1),
                radius: Value::length(0.001),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "Extrude followed by Fillet is no longer a recognised swept body (last op is Fillet)"
        );
    }

    #[test]
    fn classify_swept_body_revolve_followed_by_union_returns_none() {
        let ops = vec![
            GeometryOp::Revolve {
                profile: GeometryHandleId(0),
                axis_origin: [0.0, 0.0, 0.0],
                axis_dir: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            },
            GeometryOp::Union {
                left: GeometryHandleId(1),
                right: GeometryHandleId(0),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "Revolve followed by Union is no longer a recognised swept body (last op is Union)"
        );
    }

    // ── Step-9: SweptKindTable record / lookup / len / is_empty ───────────

    #[test]
    fn swept_kind_table_new_is_empty() {
        let table = SweptKindTable::default();
        assert!(table.is_empty(), "default-constructed table must be empty");
        assert_eq!(table.len(), 0, "default-constructed table must have len() == 0");
    }

    #[test]
    fn swept_kind_table_record_and_lookup_round_trips() {
        let mut table = SweptKindTable::default();
        let kind = SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        };
        table.record(GeometryHandleId(7), kind.clone());
        assert_eq!(table.len(), 1, "table must have len() == 1 after one record");
        assert!(!table.is_empty(), "table must not be empty after one record");
        assert_eq!(
            table.lookup(GeometryHandleId(7)),
            Some(&kind),
            "lookup must round-trip the recorded kind"
        );
    }

    #[test]
    fn swept_kind_table_lookup_unknown_returns_none() {
        let table = SweptKindTable::default();
        assert_eq!(
            table.lookup(GeometryHandleId(99)),
            None,
            "lookup of an unrecorded id must return None"
        );

        // And on a populated table — a different id must still miss.
        let mut populated = SweptKindTable::default();
        populated.record(
            GeometryHandleId(1),
            SweptKind::Extrude {
                axis: [0.0, 0.0, 1.0],
                length: Value::length(0.005),
            },
        );
        assert_eq!(
            populated.lookup(GeometryHandleId(2)),
            None,
            "lookup of an unrecorded id on a populated table must return None"
        );
    }

    #[test]
    fn swept_kind_table_record_overwrites_existing() {
        let mut table = SweptKindTable::default();
        let id = GeometryHandleId(3);
        let first = SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.005),
        };
        let second = SweptKind::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        };
        table.record(id, first);
        table.record(id, second.clone());
        assert_eq!(table.len(), 1, "second record at the same id must not grow len()");
        assert_eq!(
            table.lookup(id),
            Some(&second),
            "second record must overwrite the first (last-write-wins)"
        );
    }
}
