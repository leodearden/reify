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
//!   [`GeometryOp::LineSegment`] source op → [`SweptKind::SweepLinear`] (single-profile
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

use reify_ir::{GeometryHandleId, GeometryOp, Value};

/// Tolerance for treating a [`GeometryOp::Revolve`]'s axis or angle as
/// degenerate.
///
/// A revolve is rejected when:
/// - the Euclidean norm of `axis_dir` is `< REVOLVE_DEGENERATE_TOLERANCE`
///   (zero-length axis vector), or
/// - `angle_rad.abs() < REVOLVE_DEGENERATE_TOLERANCE` (no rotation).
///
/// `1e-12` matches the project's general geometric-tolerance convention: tight
/// enough to catch genuine zero-vector / zero-angle degenerates without
/// rejecting legitimate near-axis-aligned values. The norm test (rather than
/// componentwise) means tiny-but-nonzero axes like `[1e-11, 0.0, 0.0]` (norm
/// ~1e-11, effectively zero) are correctly rejected as degenerate even though
/// one component nominally exceeds the tolerance.
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
    /// the meshing path, not here. The stored `axis_dir` is unit-length — see
    /// the per-field doc.
    Revolve {
        /// Point on the rotation axis in the realization frame, propagated
        /// verbatim from `GeometryOp::Revolve.axis_origin`.
        axis_origin: [f64; 3],
        /// Unit-length axis direction in the realization frame. The classifier
        /// normalises `GeometryOp::Revolve.axis_dir` here (the source op only
        /// guarantees a non-degenerate norm; producers are not required to pass
        /// a unit vector) so downstream consumers — mesh morphing and rotation
        /// math — can rely on this invariant. Mirrors `SweptKind::Extrude.axis`.
        axis_dir: [f64; 3],
        /// Signed rotation angle in radians, propagated verbatim from
        /// `GeometryOp::Revolve.angle_rad`. The classifier rejects
        /// `|angle_rad| < REVOLVE_DEGENERATE_TOLERANCE`.
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
    SweepLinear {
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
pub fn classify_swept_body(ops: &[GeometryOp], handles: &[GeometryHandleId]) -> Option<SweptKind> {
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
            // other angle (including the full 2π case) qualifies. Use the
            // Euclidean norm (rather than a per-component check) so a tiny
            // axis like `[1e-11, 0.0, 0.0]` — norm ~1e-11, geometrically a
            // zero vector — is correctly rejected even though one component
            // nominally exceeds `REVOLVE_DEGENERATE_TOLERANCE`.
            let axis_norm_sq: f64 = axis_dir.iter().map(|c| c * c).sum();
            if axis_norm_sq < REVOLVE_DEGENERATE_TOLERANCE * REVOLVE_DEGENERATE_TOLERANCE
                || angle_rad.abs() < REVOLVE_DEGENERATE_TOLERANCE
            {
                None
            } else {
                // Normalise axis_dir here (inside the post-degeneracy branch)
                // to give parity with `SweptKind::Extrude.axis`'s unit-length
                // invariant, so downstream mesh-morphing rotation math can rely
                // on unit-length across both variants. Computing sqrt only here
                // (not before the guard) statically guarantees we never
                // sqrt(0): axis_norm_sq ≥ tol² > 0 at this point.
                let axis_norm = axis_norm_sq.sqrt();
                Some(SweptKind::Revolve {
                    axis_origin: *axis_origin,
                    axis_dir: [
                        axis_dir[0] / axis_norm,
                        axis_dir[1] / axis_norm,
                        axis_dir[2] / axis_norm,
                    ],
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
                Some(GeometryOp::LineSegment { .. }) => Some(SweptKind::SweepLinear {
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

/// Translate a [`SweptKind`] classification into the kernel-facing
/// [`reify_solver_elastic::SweepParams`] shape required by
/// [`reify_solver_elastic::sweep_2d_mesh_to_3d`].
///
/// # Parameters
///
/// - `kind`: the swept-body classification produced by [`classify_swept_body`].
/// - `ops`: the parallel compiled-op slice from the realization
///   (`Engine::execute_realization_ops`).
/// - `handles`: the parallel handle-id slice from the same realization.
///
/// `ops` and `handles` are only required for the [`SweptKind::SweepLinear`]
/// arm, which must re-resolve the `path` handle's source
/// [`GeometryOp::LineSegment`] to derive `axis` and `length`.  Pass empty
/// slices for [`SweptKind::Extrude`] and [`SweptKind::Revolve`] — those arms
/// ignore them.
///
/// # Returns
///
/// `Some(SweepParams)` when the conversion succeeds.  Returns `None` only for
/// the [`SweptKind::SweepLinear`] arm when the `path` handle cannot be
/// resolved in `handles` (cross-realization handle, or a malformed
/// ops/handles pair) — a condition the classifier already rejects at
/// classification time, so production callers will rarely see `None`.
pub fn swept_kind_to_sweep_params(
    kind: &SweptKind,
    ops: &[GeometryOp],
    handles: &[GeometryHandleId],
) -> Option<reify_solver_elastic::SweepParams> {
    use reify_solver_elastic::SweepParams;
    match kind {
        // Step-16: Extrude — forward axis verbatim, extract length as f64.
        SweptKind::Extrude { axis, length } => length.as_f64().map(|len| SweepParams::Extrude {
            axis: *axis,
            length: len,
        }),
        // Step-18: Revolve — forward fields; rename angle_rad → angle.
        // SweepParams::Revolve.angle must be > 0 (validate_sweep_inputs rejects
        // angle <= 0.0 with DegenerateMagnitude).  SweptKind::Revolve.angle_rad
        // is signed — the classifier only rejects |angle_rad| < tolerance, so
        // negative values can pass through.  Normalise: when angle_rad < 0,
        // negate axis_dir (reversing the rotation direction) and use abs(angle_rad)
        // so the physical rotation is unchanged with a positive-angle representation.
        SweptKind::Revolve {
            axis_origin,
            axis_dir,
            angle_rad,
        } => {
            let (axis_dir_out, angle_out) = if *angle_rad < 0.0 {
                ([-axis_dir[0], -axis_dir[1], -axis_dir[2]], -*angle_rad)
            } else {
                (*axis_dir, *angle_rad)
            };
            Some(SweepParams::Revolve {
                axis_origin: *axis_origin,
                axis_dir: axis_dir_out,
                angle: angle_out,
            })
        }
        // Step-20: SweepLinear — re-resolve path handle to its LineSegment source.
        SweptKind::SweepLinear { path, .. } => {
            let source_op = handles
                .iter()
                .position(|h| h == path)
                .and_then(|i| ops.get(i))?;
            match source_op {
                GeometryOp::LineSegment {
                    x1,
                    y1,
                    z1,
                    x2,
                    y2,
                    z2,
                } => {
                    let axis = [x2 - x1, y2 - y1, z2 - z1];
                    let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                    Some(SweepParams::SweepLinear { axis, length })
                }
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::Value;

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

    #[test]
    fn classify_swept_body_revolve_normalizes_non_unit_axis_dir() {
        // Why the 3-4-5 triple: `axis_dir = [3.0, 0.0, 4.0]` has Euclidean norm
        // exactly 5.  After normalisation the expected components are
        // [3/5, 0/5, 4/5] = [0.6, 0.0, 0.8] — a clean rational triple that
        // exercises all three components nontrivially (no zero cancellation on
        // axis[0] or axis[2]).
        //
        // Why tolerance assertions rather than
        //   `assert_eq!(Some(SweptKind::Revolve { axis_dir: [0.6, 0.0, 0.8], … }))`
        // 0.6 and 0.8 are not exactly representable in IEEE-754 f64; while
        // `3.0_f64 / 5.0` and the literal `0.6_f64` round to the same nearest
        // double on every target we support, depending on bit-equality is fragile
        // and obscures the intent.  Componentwise tolerance assertions pin the
        // actual semantic invariant — unit-length axis pointing in the right
        // direction — and read as documentation.
        //
        // Contract parity: `SweptKind::Extrude.axis` is documented "Unit-length
        // sweep axis in the realization frame".  `SweptKind::Revolve.axis_dir`
        // should honour the same invariant so downstream mesh-morphing rotation
        // math can rely on it across both variants without per-variant length
        // checks.
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [1.0, 2.0, 3.0],
            axis_dir: [3.0, 0.0, 4.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        let result = classify_swept_body(&ops, &handles);
        match result {
            Some(SweptKind::Revolve {
                axis_origin,
                axis_dir,
                angle_rad,
            }) => {
                // axis_origin and angle_rad must be propagated verbatim.
                assert_eq!(
                    axis_origin,
                    [1.0, 2.0, 3.0],
                    "axis_origin must be propagated verbatim"
                );
                // `angle_rad` is stored without modification — assert bit-exact
                // equality rather than tolerance, since no arithmetic is applied.
                assert_eq!(
                    angle_rad,
                    std::f64::consts::FRAC_PI_2,
                    "angle_rad must be propagated verbatim"
                );
                // axis_dir must be unit-length after normalisation.
                let norm: f64 = axis_dir.iter().map(|c| c * c).sum::<f64>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-12,
                    "normalised axis_dir must be unit-length; got norm={norm}"
                );
                // Componentwise: expected [3/5, 0/5, 4/5] = [0.6, 0.0, 0.8].
                assert!(
                    (axis_dir[0] - 0.6).abs() < 1e-12,
                    "axis_dir[0] must be ~0.6; got {}",
                    axis_dir[0]
                );
                assert!(
                    axis_dir[1].abs() < 1e-12,
                    "axis_dir[1] must be ~0.0; got {}",
                    axis_dir[1]
                );
                assert!(
                    (axis_dir[2] - 0.8).abs() < 1e-12,
                    "axis_dir[2] must be ~0.8; got {}",
                    axis_dir[2]
                );
            }
            other => panic!("expected Some(SweptKind::Revolve {{ ... }}) but got {other:?}"),
        }
    }

    #[test]
    fn classify_swept_body_revolve_normalizes_negative_component_axis_dir() {
        // Sign-preservation check: `axis_dir = [-3.0, 0.0, -4.0]` has the same
        // Euclidean norm as [3.0, 0.0, 4.0] (norm = 5), so the normalised result
        // must be [-0.6, 0.0, -0.8].  This confirms the normalisation divides each
        // component by the scalar norm rather than clamping or taking an absolute
        // value — a rotation axis with reversed sign points in the opposite direction,
        // which is semantically different and must not be silently flipped.
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [1.0, 2.0, 3.0],
            axis_dir: [-3.0, 0.0, -4.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        let result = classify_swept_body(&ops, &handles);
        match result {
            Some(SweptKind::Revolve {
                axis_origin,
                axis_dir,
                angle_rad,
            }) => {
                assert_eq!(
                    axis_origin,
                    [1.0, 2.0, 3.0],
                    "axis_origin must be propagated verbatim"
                );
                assert_eq!(
                    angle_rad,
                    std::f64::consts::FRAC_PI_2,
                    "angle_rad must be propagated verbatim"
                );
                let norm: f64 = axis_dir.iter().map(|c| c * c).sum::<f64>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-12,
                    "normalised axis_dir must be unit-length; got norm={norm}"
                );
                // Expected: [-3/5, 0/5, -4/5] = [-0.6, 0.0, -0.8].
                assert!(
                    (axis_dir[0] - (-0.6)).abs() < 1e-12,
                    "axis_dir[0] must be ~-0.6; got {}",
                    axis_dir[0]
                );
                assert!(
                    axis_dir[1].abs() < 1e-12,
                    "axis_dir[1] must be ~0.0; got {}",
                    axis_dir[1]
                );
                assert!(
                    (axis_dir[2] - (-0.8)).abs() < 1e-12,
                    "axis_dir[2] must be ~-0.8; got {}",
                    axis_dir[2]
                );
            }
            other => panic!("expected Some(SweptKind::Revolve {{ ... }}) but got {other:?}"),
        }
    }

    #[test]
    fn classify_swept_body_revolve_preserves_already_unit_axis_dir() {
        // Idempotence check: when `axis_dir` is already unit-length (norm = 1.0
        // exactly), the normalisation step `v / sqrt(v·v)` is IEEE-754 bit-exact
        // idempotent for standard-unit vectors such as [0.0, 0.0, 1.0].
        // This test explicitly exercises the normalisation code path with a
        // unit-length input and verifies that the output is still unit-length
        // to 1e-12 and matches the input componentwise.  It ensures a future
        // refactor of the normalisation logic cannot accidentally perturb
        // already-normalised axes.
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::PI,
        }];
        let handles = vec![GeometryHandleId(1)];
        let result = classify_swept_body(&ops, &handles);
        match result {
            Some(SweptKind::Revolve {
                axis_origin,
                axis_dir,
                angle_rad,
            }) => {
                assert_eq!(
                    axis_origin,
                    [0.0, 0.0, 0.0],
                    "axis_origin must be propagated verbatim"
                );
                assert_eq!(
                    angle_rad,
                    std::f64::consts::PI,
                    "angle_rad must be propagated verbatim"
                );
                let norm: f64 = axis_dir.iter().map(|c| c * c).sum::<f64>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-12,
                    "already-unit axis_dir must remain unit-length; got norm={norm}"
                );
                assert!(
                    axis_dir[0].abs() < 1e-12,
                    "axis_dir[0] must remain ~0.0; got {}",
                    axis_dir[0]
                );
                assert!(
                    axis_dir[1].abs() < 1e-12,
                    "axis_dir[1] must remain ~0.0; got {}",
                    axis_dir[1]
                );
                assert!(
                    (axis_dir[2] - 1.0).abs() < 1e-12,
                    "axis_dir[2] must remain ~1.0; got {}",
                    axis_dir[2]
                );
            }
            other => panic!("expected Some(SweptKind::Revolve {{ ... }}) but got {other:?}"),
        }
    }

    #[test]
    fn classify_swept_body_revolve_just_above_tolerance_classifies() {
        // Boundary regression guard: axis_dir = [tol*(1+1e-3), 0, 0] → norm_sq ≈ 1.002·tol²
        // > tol², so the op must classify as Revolve with normalised axis [1, 0, 0].
        // `is_finite()` pins the "never sqrt(0)" invariant against refactors that move sqrt
        // above the guard; FRAC_PI_2 isolates the axis-threshold check from the angle guard.
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [REVOLVE_DEGENERATE_TOLERANCE * (1.0 + 1e-3), 0.0, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        let result = classify_swept_body(&ops, &handles);
        match result {
            Some(SweptKind::Revolve {
                axis_origin,
                axis_dir,
                angle_rad,
            }) => {
                assert_eq!(
                    axis_origin,
                    [0.0, 0.0, 0.0],
                    "axis_origin must be propagated verbatim"
                );
                assert_eq!(
                    angle_rad,
                    std::f64::consts::FRAC_PI_2,
                    "angle_rad must be propagated verbatim"
                );
                assert!(
                    axis_dir.iter().all(|c| c.is_finite()),
                    "all axis_dir components must be finite; got {axis_dir:?}"
                );
                let norm: f64 = axis_dir.iter().map(|c| c * c).sum::<f64>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-12,
                    "normalised axis_dir must be unit-length; got norm={norm}"
                );
                assert!(
                    (axis_dir[0] - 1.0).abs() < 1e-12,
                    "axis_dir[0] must be ~1.0; got {}",
                    axis_dir[0]
                );
                assert!(
                    axis_dir[1].abs() < 1e-12,
                    "axis_dir[1] must be ~0.0; got {}",
                    axis_dir[1]
                );
                assert!(
                    axis_dir[2].abs() < 1e-12,
                    "axis_dir[2] must be ~0.0; got {}",
                    axis_dir[2]
                );
            }
            other => panic!(
                "expected Some(SweptKind::Revolve {{ ... }}) for axis_dir just above \
                 REVOLVE_DEGENERATE_TOLERANCE, but got {other:?}"
            ),
        }
    }

    #[test]
    fn classify_swept_body_revolve_just_below_tolerance_returns_none() {
        // Boundary complement: axis_dir = [tol*(1-1e-3), 0, 0] → norm_sq ≈ 0.998·tol² < tol²,
        // so the degeneracy guard fires and classify_swept_body must return None.  Distinct
        // from the all-zero-axis test — pins the threshold edge against guard-loosening
        // mutations (e.g. `<` → `<=`, dropped guard).  FRAC_PI_2 isolates the axis guard from
        // the angle guard.
        let ops = vec![GeometryOp::Revolve {
            profile: GeometryHandleId(0),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [REVOLVE_DEGENERATE_TOLERANCE * (1.0 - 1e-3), 0.0, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "axis_dir norm just below REVOLVE_DEGENERATE_TOLERANCE must be rejected as degenerate"
        );
    }

    // ── Step-5: Sweep path-source resolution; multi-profile Loft/LoftGuided rejection ──

    #[test]
    fn classify_swept_body_sweep_with_line_segment_path_classifies_as_sweep_linear() {
        // Two-op slice: op[0] is a LineSegment path constructor whose result
        // handle is GeometryHandleId(1); op[1] is a Sweep that consumes that
        // path handle. The classifier must trace path → LineSegment via the
        // parallel handles slice and accept the sweep as SweepLinear.
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
            Some(SweptKind::SweepLinear {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            }),
            "Sweep along a LineSegment-source path must classify as SweptKind::SweepLinear"
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
        // rejected for Phase A even though SweptKind::SweepLinear exists (the
        // latter names single-profile Sweep-along-line-segment, per the design
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

    // ── Amendment: explicitly-rejected sweep variants ─────────────────────
    // The module docs list `SweepGuided`, `LoftGuided`, and `Pipe` as Phase A
    // rejects. They currently fall through the catch-all `_ => None` arm, which
    // is correct, but a future refactor that splits the match could silently
    // start accepting them. These tests pin the rejection so any such regression
    // is caught.

    #[test]
    fn classify_swept_body_sweep_guided_returns_none() {
        let ops = vec![GeometryOp::SweepGuided {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(1),
            guide: GeometryHandleId(2),
        }];
        let handles = vec![GeometryHandleId(3)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "GeometryOp::SweepGuided (auxiliary-guide twist potential) must be rejected for Phase A"
        );
    }

    #[test]
    fn classify_swept_body_loft_guided_returns_none() {
        let ops = vec![GeometryOp::LoftGuided {
            profiles: vec![GeometryHandleId(0), GeometryHandleId(1)],
            guides: vec![GeometryHandleId(2)],
        }];
        let handles = vec![GeometryHandleId(3)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "GeometryOp::LoftGuided (multi-profile + guide) must be rejected for Phase A"
        );
    }

    #[test]
    fn classify_swept_body_pipe_returns_none() {
        let ops = vec![GeometryOp::Pipe {
            path: GeometryHandleId(0),
            radius: Value::length(0.005),
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "GeometryOp::Pipe (non-canonical sweep) must be rejected for Phase A"
        );
    }

    // ── Amendment: cross-realization path source is unresolvable ──────────
    // The Sweep arm resolves a `path` handle by linear-scanning the parallel
    // `handles` slice for the *current* realization only. If the path was
    // produced in a different realization (e.g. via `named_steps` lookup) and
    // its handle isn't present in this slice, the position lookup misses and
    // the classifier returns `None` — even if the producing op was a
    // `LineSegment` and would otherwise qualify. This test pins that
    // documented limitation; if a future revision passes a richer handle→op
    // map into the classifier and accepts cross-realization paths, this test
    // is the right place to update.

    #[test]
    fn classify_swept_body_sweep_with_cross_realization_path_returns_none() {
        // The Sweep references handle id 99, which was produced in a *prior*
        // realization and is NOT present in this realization's `handles`
        // slice. Even though the Sweep is otherwise well-formed and a
        // hypothetical LineSegment producer would qualify, the path-source
        // scan misses → `None`.
        let ops = vec![GeometryOp::Sweep {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(99),
        }];
        let handles = vec![GeometryHandleId(1)];
        assert_eq!(
            classify_swept_body(&ops, &handles),
            None,
            "Sweep with a path handle produced outside this realization slice must return None (cross-realization path resolution is out of scope for Phase A)"
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
                edges: vec![],
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

    // ── Steps 15-20: swept_kind_to_sweep_params ───────────────────────────

    #[test]
    fn swept_kind_to_sweep_params_extrude_converts_with_axis_z_and_length() {
        let kind = SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        };
        let result = swept_kind_to_sweep_params(&kind, &[], &[]);
        match result {
            Some(reify_solver_elastic::SweepParams::Extrude { axis, length }) => {
                assert_eq!(axis, [0.0, 0.0, 1.0], "axis must be forwarded verbatim");
                assert!(
                    (length - 0.01).abs() < 1e-12,
                    "length must be 0.01; got {length}"
                );
            }
            other => panic!("expected Some(SweepParams::Extrude {{ ... }}), got {other:?}"),
        }
    }

    #[test]
    fn swept_kind_to_sweep_params_revolve_forwards_origin_dir_and_angle() {
        let kind = SweptKind::Revolve {
            axis_origin: [1.0, 2.0, 3.0],
            axis_dir: [0.0, 1.0, 0.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        };
        let result = swept_kind_to_sweep_params(&kind, &[], &[]);
        match result {
            Some(reify_solver_elastic::SweepParams::Revolve {
                axis_origin,
                axis_dir,
                angle,
            }) => {
                assert!(
                    (axis_origin[0] - 1.0).abs() < 1e-12
                        && (axis_origin[1] - 2.0).abs() < 1e-12
                        && (axis_origin[2] - 3.0).abs() < 1e-12,
                    "axis_origin must match; got {axis_origin:?}"
                );
                assert!(
                    axis_dir[0].abs() < 1e-12
                        && (axis_dir[1] - 1.0).abs() < 1e-12
                        && axis_dir[2].abs() < 1e-12,
                    "axis_dir must match; got {axis_dir:?}"
                );
                assert!(
                    (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
                    "angle must be FRAC_PI_2; got {angle}"
                );
            }
            other => panic!("expected Some(SweepParams::Revolve {{ ... }}), got {other:?}"),
        }
    }

    #[test]
    fn swept_kind_to_sweep_params_sweep_linear_resolvable_and_unresolvable() {
        // Subcase A: resolvable — ops[0] is LineSegment with path handle id 1
        let ops = vec![
            GeometryOp::LineSegment {
                x1: 0.0,
                y1: 0.0,
                z1: 0.0,
                x2: 0.0,
                y2: 0.0,
                z2: 0.5,
            },
            GeometryOp::Sweep {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        let kind = SweptKind::SweepLinear {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(1),
        };
        let result_a = swept_kind_to_sweep_params(&kind, &ops, &handles);
        match result_a {
            Some(reify_solver_elastic::SweepParams::SweepLinear { axis, length }) => {
                // axis = endpoint diff [0-0, 0-0, 0.5-0] = [0, 0, 0.5], length = 0.5
                assert!(
                    axis[0].abs() < 1e-12 && axis[1].abs() < 1e-12 && (axis[2] - 0.5).abs() < 1e-12,
                    "axis must be [0,0,0.5]; got {axis:?}"
                );
                assert!(
                    (length - 0.5).abs() < 1e-12,
                    "length must be 0.5; got {length}"
                );
            }
            other => panic!(
                "subcase A: expected Some(SweepParams::SweepLinear {{ ... }}), got {other:?}"
            ),
        }

        // Subcase B: unresolvable — path handle 99 not in handles slice
        let kind_b = SweptKind::SweepLinear {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(99),
        };
        let result_b = swept_kind_to_sweep_params(&kind_b, &ops, &handles);
        assert!(
            result_b.is_none(),
            "unresolvable path handle must return None; got {result_b:?}"
        );
    }

    // ── Amendment: additional swept_kind_to_sweep_params edge cases ──────

    /// Negative `angle_rad` on a Revolve: the converter must emit a positive
    /// `angle` in `SweepParams` (which validate_sweep_inputs requires > 0) and
    /// flip `axis_dir` to preserve the rotation direction.
    #[test]
    fn swept_kind_to_sweep_params_revolve_negative_angle_normalises_to_positive() {
        let kind = SweptKind::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: -std::f64::consts::FRAC_PI_2,
        };
        let result = swept_kind_to_sweep_params(&kind, &[], &[]);
        match result {
            Some(reify_solver_elastic::SweepParams::Revolve {
                axis_origin,
                axis_dir,
                angle,
            }) => {
                // angle must be positive (abs of input)
                assert!(
                    (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
                    "angle must be |angle_rad|; got {angle}"
                );
                // axis_dir must be negated to preserve rotation direction
                assert!(
                    axis_dir[0].abs() < 1e-12
                        && axis_dir[1].abs() < 1e-12
                        && (axis_dir[2] + 1.0).abs() < 1e-12,
                    "axis_dir must be flipped to [0,0,-1]; got {axis_dir:?}"
                );
                // axis_origin forwarded unchanged
                assert!(
                    axis_origin[0].abs() < 1e-12
                        && axis_origin[1].abs() < 1e-12
                        && axis_origin[2].abs() < 1e-12,
                    "axis_origin must be [0,0,0]; got {axis_origin:?}"
                );
            }
            other => panic!("expected Some(SweepParams::Revolve {{ ... }}), got {other:?}"),
        }
    }

    /// Zero-length `LineSegment` (both endpoints equal): the converter still
    /// returns `Some(SweepParams::SweepLinear { axis: [0,0,0], length: 0.0 })`.
    /// Validation (DegenerateMagnitude) is the kernel's responsibility, not the
    /// converter's — document that contract here.
    #[test]
    fn swept_kind_to_sweep_params_sweep_linear_zero_length_line_returns_some() {
        let ops = vec![
            GeometryOp::LineSegment {
                x1: 0.0,
                y1: 0.0,
                z1: 0.0,
                x2: 0.0,
                y2: 0.0,
                z2: 0.0, // degenerate: same point
            },
            GeometryOp::Sweep {
                profile: GeometryHandleId(0),
                path: GeometryHandleId(1),
            },
        ];
        let handles = vec![GeometryHandleId(1), GeometryHandleId(2)];
        let kind = SweptKind::SweepLinear {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(1),
        };
        let result = swept_kind_to_sweep_params(&kind, &ops, &handles);
        match result {
            Some(reify_solver_elastic::SweepParams::SweepLinear { axis, length }) => {
                // axis = [0,0,0], length = 0 — degenerate but converter returns Some;
                // kernel's validate_sweep_inputs will reject with DegenerateMagnitude.
                assert!(
                    axis[0].abs() < 1e-12 && axis[1].abs() < 1e-12 && axis[2].abs() < 1e-12,
                    "axis must be [0,0,0]; got {axis:?}"
                );
                assert!(length.abs() < 1e-12, "length must be 0.0; got {length}");
            }
            other => {
                panic!("expected Some(SweepParams::SweepLinear {{ ... }}), got {other:?}")
            }
        }
    }

    // ── Step-9: SweptKindTable record / lookup / len / is_empty ───────────

    #[test]
    fn swept_kind_table_new_is_empty() {
        let table = SweptKindTable::default();
        assert!(table.is_empty(), "default-constructed table must be empty");
        assert_eq!(
            table.len(),
            0,
            "default-constructed table must have len() == 0"
        );
    }

    #[test]
    fn swept_kind_table_record_and_lookup_round_trips() {
        let mut table = SweptKindTable::default();
        let kind = SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        };
        table.record(GeometryHandleId(7), kind.clone());
        assert_eq!(
            table.len(),
            1,
            "table must have len() == 1 after one record"
        );
        assert!(
            !table.is_empty(),
            "table must not be empty after one record"
        );
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
        assert_eq!(
            table.len(),
            1,
            "second record at the same id must not grow len()"
        );
        assert_eq!(
            table.lookup(id),
            Some(&second),
            "second record must overwrite the first (last-write-wins)"
        );
    }
}
