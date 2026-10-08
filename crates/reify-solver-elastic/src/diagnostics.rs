// crates/reify-solver-elastic/src/diagnostics.rs
//
// Neutral FEA failure classification — NO reify-core imports allowed.
// (This crate depends on reify-ir / reify-kernel-gmsh / faer / inventory,
// NOT on reify-core, so Diagnostic / DiagnosticCode / Severity / SourceSpan
// must NOT appear here.)
//
// The mapping from FeaFailure → reify_core::Diagnostic lives in
// reify-eval/src/compute_targets/fea_diagnostics.rs.

use std::fmt;

use crate::result::tet_signed_volume_p1;

/// The 6 rigid-body degrees of freedom of a connected 3D elastic continuum.
///
/// These are the exact rigid-body null-space modes: 3 translations (X/Y/Z axis)
/// and 3 axis rotations (X/Y/Z axis).  A fully-unsupported body has exactly these
/// 6 zero-stiffness modes — a textbook identity that needs no eigensolver.
///
/// Neutral type — no serde, no reify-core references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DofDirection {
    /// Translation along the X axis.
    TranslationX,
    /// Translation along the Y axis.
    TranslationY,
    /// Translation along the Z axis.
    TranslationZ,
    /// Rotation about the X axis.
    RotationX,
    /// Rotation about the Y axis.
    RotationY,
    /// Rotation about the Z axis.
    RotationZ,
}

impl DofDirection {
    /// Returns the canonical 6-mode rigid-body null space as a fixed-size array
    /// `[TranslationX, TranslationY, TranslationZ, RotationX, RotationY, RotationZ]`.
    ///
    /// This is the exact rigid-body null space of a connected 3D elastic continuum:
    /// 3 rigid translations + 3 rigid axis rotations.  The enumeration is a textbook
    /// identity and requires no eigensolver or null-space analysis.
    ///
    /// The fixed-arity return type avoids a per-call heap allocation.  Call `.into()`
    /// on the result to get a `Vec<DofDirection>` where one is required (e.g. when
    /// constructing [`FeaDiagnosticDetail::Unconstrained`]).
    pub fn all_rigid_body_modes() -> [DofDirection; 6] {
        [
            DofDirection::TranslationX,
            DofDirection::TranslationY,
            DofDirection::TranslationZ,
            DofDirection::RotationX,
            DofDirection::RotationY,
            DofDirection::RotationZ,
        ]
    }
}

/// Identifies a mesh element by its position index.
///
/// A transparent newtype over `usize` — neutral type, no serde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub usize);

/// Typed structured overlay payload for an FEA diagnostic variant.
///
/// Carries the geometry needed by the GUI overlay to render:
/// - `Unconstrained` — rigid-body-mode arrows (which DOF directions are unconstrained)
/// - `ProblemElements` — outline highlights around degenerate elements
/// - `UnresolvedSelector` — ghost selector path for unmatched selectors
///
/// Neutral enum — no serde, no reify-core references.
/// Rust↔TS IPC serialization is consumer task 2966's responsibility.
///
/// An existing `FeaFailure` produces its optional structured detail via
/// `FeaFailure::structured_detail(&self) -> Option<FeaDiagnosticDetail>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeaDiagnosticDetail {
    /// The body is under-constrained: lists the unconstrained rigid-body DOF directions.
    ///
    /// For a fully-unsupported body this is always all 6 rigid-body modes
    /// (see `DofDirection::all_rigid_body_modes`).
    Unconstrained { rigid_body_modes: Vec<DofDirection> },
    /// One or more mesh elements are degenerate / problematic.
    ProblemElements { ids: Vec<ElementId> },
    /// A selector string matched no geometry nodes.
    UnresolvedSelector { selector_path: String },
}

/// The small fixed set of well-known FEA failure modes, with actionable messages.
///
/// Neutral type — no reify-core references.  The `message()` and `is_error()`
/// methods encode the triage-table text and severity hints; the conversion to a
/// full `reify_core::Diagnostic` happens in `reify-eval`'s `fea_diagnostic_to_core`.
#[derive(Debug)]
pub enum FeaFailure {
    /// Root face auto-clamp model has no user-specified supports.
    UnderConstrained { support_count: usize },
    /// The mesh's worst tet fails the [`MIN_TET_SHAPE_QUALITY`] gate, so its
    /// stiffness matrix is numerically singular.
    SingularStiffness(DegenerateTet),
    /// CG solver reached max iterations without converging.
    NonConvergence {
        iterations: usize,
        max_iter: usize,
        final_residual: Option<f64>,
    },
    /// No loads were specified (all-zero applied force).
    NoLoads,
    /// A load was applied to an interior node (not a boundary selector).
    LoadOnInterior { selector: String },
    /// A selector matched no geometry nodes.
    SelectorNoMatch {
        selector: String,
        nearest: Option<String>,
    },
    /// Body bounding-box aspect ratio exceeds the thin-body threshold.
    ThinBody { aspect_ratio: f64 },
}

impl FeaFailure {
    /// Human-readable actionable message for this failure mode.
    ///
    /// Text follows the triage table in the FEA diagnostics PRD.
    pub fn message(&self) -> String {
        match self {
            FeaFailure::UnderConstrained { support_count } => format!(
                "FEA model has insufficient supports ({support_count} specified); \
                 the root face is auto-clamped but results may not reflect design intent. \
                 Add a FixedSupport or PinnedSupport to constrain the structure."
            ),
            FeaFailure::SingularStiffness(d) => format!(
                "stiffness matrix is singular: {d}. Refine the mesh or check the geometry \
                 for collapsed or inverted elements."
            ),
            FeaFailure::NonConvergence {
                iterations,
                max_iter,
                final_residual,
            } => {
                let res_str = final_residual
                    .map(|r| format!(", final residual {r:.3e}"))
                    .unwrap_or_default();
                format!(
                    "CG solver did not converge after {iterations}/{max_iter} iterations{res_str}. \
                     Consider increasing ElasticOptions max_iter or checking boundary conditions."
                )
            }
            FeaFailure::NoLoads => {
                "No loads applied to the FEA model. \
                 Add at least one PointLoad or PressureLoad to produce a non-trivial result."
                    .to_string()
            }
            FeaFailure::LoadOnInterior { selector } => format!(
                "Load selector '{selector}' targets an interior node, not a boundary face. \
                 Use a face selector (x_min, x_max, y_min, y_max, z_min, z_max) or 'tip'."
            ),
            FeaFailure::SelectorNoMatch { selector, nearest } => {
                let hint = nearest
                    .as_deref()
                    .map(|n| format!(" Did you mean '{n}'?"))
                    .unwrap_or_default();
                format!(
                    "Selector '{selector}' did not match any geometry nodes.{hint}"
                )
            }
            FeaFailure::ThinBody { aspect_ratio } => format!(
                "Body aspect ratio {aspect_ratio:.1} is very thin; \
                 P1 solid elements perform poorly for thin bodies (shells PRD, task P2). \
                 Consider using shell elements via ElasticOptions(shell_force: ShellForce.On) \
                 or increasing element_order."
            ),
        }
    }

    /// Returns the optional typed structured overlay payload for this failure.
    ///
    /// The three geometric variants carry data needed by the GUI overlay:
    /// - `UnderConstrained` → [`FeaDiagnosticDetail::Unconstrained`] with the full
    ///   6-DOF rigid-body null space (see [`DofDirection::all_rigid_body_modes`]).
    ///   A fully-unsupported body always has exactly all 6 free-body modes; partial-
    ///   constraint mode-subset analysis (needing a K null-space solver) is out of scope.
    /// - `SingularStiffness(d)` → [`FeaDiagnosticDetail::ProblemElements`]
    ///   containing `[ElementId(d.element_id)]` — the degenerate element to highlight.
    /// - `SelectorNoMatch { selector, .. }` → [`FeaDiagnosticDetail::UnresolvedSelector`]
    ///   with `selector_path = selector.clone()`.
    ///
    /// The four non-geometric variants (`NoLoads`, `NonConvergence`, `ThinBody`,
    /// `LoadOnInterior`) return `None` — they convey no geometry for overlay rendering.
    pub fn structured_detail(&self) -> Option<FeaDiagnosticDetail> {
        match self {
            FeaFailure::UnderConstrained { support_count } => {
                // A fully-unsupported connected 3D body has exactly the 6-DOF rigid-body
                // null space (3 translations + 3 axis rotations) — a textbook identity.
                // The production solve path only ever flags support_count==0, so the full
                // 6-mode set is always the correct payload.
                //
                // Assert the invariant loudly in debug/test builds: if a future caller
                // constructs UnderConstrained{support_count>0} the overlay would silently
                // emit a physically wrong 6-mode payload.  Partial-constraint null-space
                // analysis (mode subsets for support_count>0) is out of scope — task 4090.
                debug_assert_eq!(
                    *support_count, 0,
                    "structured_detail: UnderConstrained{{support_count={support_count}}} > 0; \
                     only support_count==0 is expected from the production solve path — \
                     partial-constraint mode-subset analysis is out of scope (task #4090)"
                );
                Some(FeaDiagnosticDetail::Unconstrained {
                    rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
                })
            }
            FeaFailure::SingularStiffness(d) => {
                // Re-wrap the degenerate element into ProblemElements for outline rendering.
                Some(FeaDiagnosticDetail::ProblemElements {
                    ids: vec![ElementId(d.element_id)],
                })
            }
            FeaFailure::SelectorNoMatch { selector, .. } => {
                // Re-wrap the selector string for ghost-selector rendering.
                Some(FeaDiagnosticDetail::UnresolvedSelector {
                    selector_path: selector.clone(),
                })
            }
            // Non-geometric variants — no overlay geometry to render.
            FeaFailure::NoLoads
            | FeaFailure::NonConvergence { .. }
            | FeaFailure::ThinBody { .. }
            | FeaFailure::LoadOnInterior { .. } => None,
        }
    }

    /// Returns `true` if this failure mode represents an unrecoverable error
    /// (should map to `Severity::Error`), `false` for advisory warnings.
    pub fn is_error(&self) -> bool {
        matches!(
            self,
            FeaFailure::SingularStiffness(_)
                | FeaFailure::LoadOnInterior { .. }
                | FeaFailure::SelectorNoMatch { .. }
        )
    }
}

/// Emit a `ThinBody` advisory if `max_dim / min_dim > threshold`.
///
/// Returns `Some(FeaFailure::ThinBody { aspect_ratio })` when the body's
/// bounding-box aspect ratio exceeds `threshold`; `None` otherwise.
///
/// `threshold ≈ 10` is the recommended value (P1 solid elements are unreliable
/// when the thinnest dimension is < 1/10 of the largest).
pub fn thin_body_advisory(
    length: f64,
    width: f64,
    height: f64,
    threshold: f64,
) -> Option<FeaFailure> {
    let max_dim = length.max(width).max(height);
    let min_dim = length.min(width).min(height);
    if min_dim <= 0.0 {
        return None;
    }
    let ratio = max_dim / min_dim;
    if ratio > threshold {
        Some(FeaFailure::ThinBody { aspect_ratio: ratio })
    } else {
        None
    }
}

/// Classify convergence outcome.
///
/// Returns `Some(FeaFailure::NonConvergence{..})` when `!converged`;
/// `None` when the solver converged.
pub fn classify_convergence(
    converged: bool,
    iterations: usize,
    max_iter: usize,
    residual: Option<f64>,
) -> Option<FeaFailure> {
    if converged {
        None
    } else {
        Some(FeaFailure::NonConvergence {
            iterations,
            max_iter,
            final_residual: residual,
        })
    }
}

/// The smallest oriented [`tet_shape_quality`] a P1 tet may have. q is 1 for a regular
/// tet at any size, and a P1 tet's stiffness entries scale as E·ℓ/q, so below 1e-8 one
/// element swamps more f64 digits than the 1e-6 CG tolerance leaves room for.
pub const MIN_TET_SHAPE_QUALITY: f64 = 1e-8;

/// Signed volume-length shape quality `q = 6√2·V/ℓ_rms³` of a P1 tet, where `V` is its
/// signed volume and `ℓ_rms` the RMS of its 6 edge lengths.
///
/// `q` is 1 for a regular tet, independent of scale, tends to 0 for slivers, needles,
/// caps and flat tets, and changes sign with the node ordering. Coincident points give
/// NaN.
pub fn tet_shape_quality(phys: &[[f64; 3]; 4]) -> f64 {
    const EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    let squared_length =
        |(a, b): (usize, usize)| -> f64 { (0..3).map(|i| (phys[b][i] - phys[a][i]).powi(2)).sum() };
    let mean_squared_edge = EDGES.into_iter().map(squared_length).sum::<f64>() / 6.0;
    let rms_edge_cubed = mean_squared_edge * mean_squared_edge.sqrt();
    6.0 * std::f64::consts::SQRT_2 * tet_signed_volume_p1(phys) / rms_edge_cubed
}

/// A tet that fails the [`MIN_TET_SHAPE_QUALITY`] gate, as found by
/// [`find_degenerate_tet`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DegenerateTet {
    /// Index of the element in the mesh's tet list.
    pub element_id: usize,
    /// Its [`tet_shape_quality`] ORIENTED against the mesh: negative means the tet is
    /// inverted relative to the rest of the mesh, NaN means non-finite geometry.
    pub quality: f64,
}

impl fmt::Display for DegenerateTet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shape = if self.quality < 0.0 {
            "inverted relative to the rest of the mesh"
        } else {
            "a sliver, needle or flat tet"
        };
        write!(
            f,
            "element {} has oriented shape quality q = 6√2·V/ℓ_rms³ = {:e}, failing the \
             minimum q ≥ {MIN_TET_SHAPE_QUALITY:e} (it is {shape})",
            self.element_id, self.quality
        )
    }
}

/// The worst tet of a P1 mesh if it fails the [`MIN_TET_SHAPE_QUALITY`] gate, else `None`.
///
/// Orientation is relative to the mesh: each tet's [`tet_shape_quality`] is signed by the
/// mesh's total signed volume, so a consistently mirrored mesh passes, matching the
/// orientation-agnostic `|det J|` of assembly. A NaN quality fails closed. Every index in
/// `tets` must be in range for `coords`, as for assembly.
pub fn find_degenerate_tet(coords: &[[f64; 3]], tets: &[[usize; 4]]) -> Option<DegenerateTet> {
    let nodes = |tet: &[usize; 4]| tet.map(|n| coords[n]);
    let total_signed_volume: f64 = tets.iter().map(|t| tet_signed_volume_p1(&nodes(t))).sum();
    let orientation = if total_signed_volume >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let mut worst: Option<DegenerateTet> = None;
    for (element_id, tet) in tets.iter().enumerate() {
        let candidate = DegenerateTet {
            element_id,
            quality: orientation * tet_shape_quality(&nodes(tet)),
        };
        if candidate.quality.is_nan() {
            return Some(candidate);
        }
        if worst.is_none_or(|w| candidate.quality < w.quality) {
            worst = Some(candidate);
        }
    }
    worst.filter(|w| w.quality < MIN_TET_SHAPE_QUALITY)
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── message() substrings ──────────────────────────────────────────────────

    #[test]
    fn under_constrained_message_contains_key_phrase() {
        let f = FeaFailure::UnderConstrained { support_count: 0 };
        assert!(
            f.message().contains("insufficient supports"),
            "UnderConstrained message must contain 'insufficient supports', got: {}",
            f.message()
        );
    }

    #[test]
    fn no_loads_message_contains_key_phrase() {
        let f = FeaFailure::NoLoads;
        assert!(
            f.message().contains("No loads"),
            "NoLoads message must contain 'No loads', got: {}",
            f.message()
        );
    }

    #[test]
    fn non_convergence_message_contains_key_phrase() {
        let f = FeaFailure::NonConvergence {
            iterations: 2000,
            max_iter: 2000,
            final_residual: Some(1.5e-3),
        };
        assert!(
            f.message().contains("did not converge"),
            "NonConvergence message must contain 'did not converge', got: {}",
            f.message()
        );
    }

    #[test]
    fn thin_body_message_contains_key_phrase() {
        let f = FeaFailure::ThinBody { aspect_ratio: 100.0 };
        assert!(
            f.message().contains("thin"),
            "ThinBody message must contain 'thin', got: {}",
            f.message()
        );
    }

    #[test]
    fn singular_stiffness_message_names_the_element_metric_and_threshold() {
        let d = DegenerateTet {
            element_id: 7,
            quality: 0.0,
        };
        let message = FeaFailure::SingularStiffness(d).message();
        assert!(
            message.contains(&d.to_string()),
            "message must embed DegenerateTet's Display, got: {message}"
        );
        assert!(
            message.contains(&format!("{MIN_TET_SHAPE_QUALITY:e}")),
            "message must name the threshold, got: {message}"
        );
        assert!(
            message.contains('7'),
            "message must name the element, got: {message}"
        );
    }

    #[test]
    fn singular_stiffness_message_says_inverted_for_negative_quality() {
        let message = FeaFailure::SingularStiffness(DegenerateTet {
            element_id: 7,
            quality: -0.5,
        })
        .message();
        assert!(
            message.contains("inverted"),
            "inverted-tet message must say so, got: {message}"
        );
    }

    #[test]
    fn load_on_interior_message_contains_key_phrase() {
        let f = FeaFailure::LoadOnInterior {
            selector: "mid".to_string(),
        };
        assert!(
            f.message().contains("interior"),
            "LoadOnInterior message must contain 'interior', got: {}",
            f.message()
        );
    }

    #[test]
    fn selector_no_match_message_contains_key_phrase() {
        let f = FeaFailure::SelectorNoMatch {
            selector: "oops".to_string(),
            nearest: None,
        };
        assert!(
            f.message().contains("did not match"),
            "SelectorNoMatch message must contain 'did not match', got: {}",
            f.message()
        );
    }

    // ── is_error() ────────────────────────────────────────────────────────────

    #[test]
    fn singular_stiffness_is_error() {
        let d = DegenerateTet {
            element_id: 7,
            quality: 0.0,
        };
        assert!(FeaFailure::SingularStiffness(d).is_error());
    }

    #[test]
    fn load_on_interior_is_error() {
        assert!(FeaFailure::LoadOnInterior {
            selector: "x".to_string()
        }
        .is_error());
    }

    #[test]
    fn selector_no_match_is_error() {
        assert!(FeaFailure::SelectorNoMatch {
            selector: "x".to_string(),
            nearest: None
        }
        .is_error());
    }

    #[test]
    fn advisory_variants_are_not_errors() {
        assert!(!FeaFailure::UnderConstrained { support_count: 0 }.is_error());
        assert!(!FeaFailure::NonConvergence {
            iterations: 1,
            max_iter: 2000,
            final_residual: None
        }
        .is_error());
        assert!(!FeaFailure::NoLoads.is_error());
        assert!(!FeaFailure::ThinBody { aspect_ratio: 100.0 }.is_error());
    }

    // ── thin_body_advisory ────────────────────────────────────────────────────

    #[test]
    fn thin_body_advisory_fires_when_ratio_exceeds_threshold() {
        // 1.0 / 0.01 = 100 >> threshold 10.
        let result = thin_body_advisory(1.0, 1.0, 0.01, 10.0);
        match result {
            Some(FeaFailure::ThinBody { aspect_ratio }) => {
                assert!(
                    (aspect_ratio - 100.0).abs() < 0.01,
                    "expected aspect_ratio≈100, got {aspect_ratio}"
                );
            }
            other => panic!("expected Some(ThinBody), got {:?}", other),
        }
    }

    #[test]
    fn thin_body_advisory_silent_when_ratio_at_or_below_threshold() {
        // 1.0 / 1.0 = 1.0 <= threshold 10.
        assert!(
            thin_body_advisory(1.0, 1.0, 1.0, 10.0).is_none(),
            "cubic body (ratio=1) must not trigger thin-body advisory"
        );
    }

    #[test]
    fn thin_body_advisory_silent_exactly_at_threshold() {
        // max/min = 10.0 — exactly at threshold, NOT strictly exceeding.
        let result = thin_body_advisory(1.0, 1.0, 0.1, 10.0);
        assert!(
            result.is_none(),
            "ratio exactly at threshold must not fire advisory (must be strictly >), got {:?}",
            result
        );
    }

    // ── classify_convergence ──────────────────────────────────────────────────

    #[test]
    fn classify_convergence_non_converged_returns_failure() {
        let result = classify_convergence(false, 2000, 2000, Some(1.5e-3));
        assert!(
            matches!(result, Some(FeaFailure::NonConvergence { .. })),
            "non-converged solver must yield NonConvergence failure, got {:?}",
            result
        );
    }

    #[test]
    fn classify_convergence_converged_returns_none() {
        let result = classify_convergence(true, 42, 2000, Some(1e-8));
        assert!(
            result.is_none(),
            "converged solver must yield None, got {:?}",
            result
        );
    }

    #[test]
    fn classify_convergence_preserves_fields() {
        match classify_convergence(false, 1500, 2000, Some(2.5e-4)) {
            Some(FeaFailure::NonConvergence {
                iterations,
                max_iter,
                final_residual: Some(r),
            }) => {
                assert_eq!(iterations, 1500);
                assert_eq!(max_iter, 2000);
                assert!((r - 2.5e-4).abs() < 1e-10);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    // ── tet shape quality / find_degenerate_tet ───────────────────────────────

    /// A `reps`-hex grid over a `dims` box, each hex split into the 6
    /// positively oriented Freudenthal tets around the c0→c6 diagonal (the
    /// split `elastic_static`'s box builder uses).
    fn freudenthal_box(dims: [f64; 3], reps: [usize; 3]) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let [rx, ry, rz] = reps;
        let (nx1, ny1, nz1) = (rx + 1, ry + 1, rz + 1);
        let node = |ix: usize, iy: usize, iz: usize| iz * ny1 * nx1 + iy * nx1 + ix;
        let mut coords = Vec::with_capacity(nx1 * ny1 * nz1);
        for iz in 0..nz1 {
            for iy in 0..ny1 {
                for ix in 0..nx1 {
                    coords.push([
                        ix as f64 * dims[0] / rx as f64,
                        iy as f64 * dims[1] / ry as f64,
                        iz as f64 * dims[2] / rz as f64,
                    ]);
                }
            }
        }
        let mut tets = Vec::with_capacity(rx * ry * rz * 6);
        for hz in 0..rz {
            for hy in 0..ry {
                for hx in 0..rx {
                    let c = [
                        node(hx, hy, hz),
                        node(hx + 1, hy, hz),
                        node(hx + 1, hy + 1, hz),
                        node(hx, hy + 1, hz),
                        node(hx, hy, hz + 1),
                        node(hx + 1, hy, hz + 1),
                        node(hx + 1, hy + 1, hz + 1),
                        node(hx, hy + 1, hz + 1),
                    ];
                    tets.extend([
                        [c[0], c[1], c[2], c[6]],
                        [c[0], c[2], c[3], c[6]],
                        [c[0], c[5], c[1], c[6]],
                        [c[0], c[3], c[7], c[6]],
                        [c[0], c[4], c[5], c[6]],
                        [c[0], c[7], c[4], c[6]],
                    ]);
                }
            }
        }
        (coords, tets)
    }

    fn scaled(coords: &[[f64; 3]], s: f64) -> Vec<[f64; 3]> {
        coords.iter().map(|p| p.map(|x| x * s)).collect()
    }

    fn tet_nodes(coords: &[[f64; 3]], tet: [usize; 4]) -> [[f64; 3]; 4] {
        tet.map(|n| coords[n])
    }

    #[test]
    fn tet_shape_quality_is_one_for_a_regular_tet() {
        let regular = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5, 3.0_f64.sqrt() / 2.0, 0.0],
            [0.5, 3.0_f64.sqrt() / 6.0, (2.0_f64 / 3.0).sqrt()],
        ];
        let q = tet_shape_quality(&regular);
        assert!((q - 1.0).abs() < 1e-12, "regular tet quality = {q}");
    }

    #[test]
    fn tet_shape_quality_of_the_cube_freudenthal_tet() {
        let tet = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
        ];
        let expected = 2.0_f64.sqrt() / (10.0_f64 / 6.0).powf(1.5);
        let q = tet_shape_quality(&tet);
        assert!(
            (q - expected).abs() < 1e-12,
            "Freudenthal tet quality = {q}, expected {expected}"
        );
        assert!((q - 0.6573).abs() < 1e-4, "Freudenthal tet quality = {q}");
    }

    #[test]
    fn tet_shape_quality_is_signed_by_orientation() {
        let tet = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
        ];
        let swapped = [tet[1], tet[0], tet[2], tet[3]];
        assert_eq!(tet_shape_quality(&swapped), -tet_shape_quality(&tet));
    }

    #[test]
    fn tet_shape_quality_of_coplanar_points_is_zero() {
        let flat = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        assert_eq!(tet_shape_quality(&flat), 0.0);
    }

    #[test]
    fn well_shaped_mesh_passes_the_degenerate_tet_gate_at_any_scale() {
        let (unit_coords, tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        for s in [1e-6, 1e-4, 1e-3, 1.0, 1e3] {
            let coords = scaled(&unit_coords, s);
            assert_eq!(
                find_degenerate_tet(&coords, &tets),
                None,
                "well-shaped box mesh scaled by {s} must pass the gate"
            );
            for &tet in &tets {
                let q_unit = tet_shape_quality(&tet_nodes(&unit_coords, tet));
                let q_scaled = tet_shape_quality(&tet_nodes(&coords, tet));
                assert!(
                    ((q_scaled - q_unit) / q_unit).abs() < 1e-9,
                    "quality must be scale-invariant: {q_scaled} at scale {s} vs {q_unit} at 1"
                );
            }
        }
    }

    #[test]
    fn sub_tenth_mm_box_mesh_would_have_failed_the_old_absolute_volume_threshold() {
        let (unit_coords, tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        let coords = scaled(&unit_coords, 1e-4);
        for &tet in &tets {
            let volume = crate::result::tet_volume_p1(&tet_nodes(&coords, tet));
            assert!(
                volume < 1e-12,
                "premise: every tet volume < 1e-12, got {volume}"
            );
        }
    }

    #[test]
    fn find_degenerate_tet_flags_an_appended_flat_tet() {
        let (coords, mut tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        // Nodes 0, 1, 4, 3 are hex (0,0,0)'s z = 0 bottom face.
        tets.push([0, 1, 4, 3]);
        let flat_id = tets.len() - 1;
        let d = find_degenerate_tet(&coords, &tets).expect("flat tet must be flagged");
        assert_eq!(d.element_id, flat_id);
        assert!(
            d.quality < MIN_TET_SHAPE_QUALITY,
            "flat tet quality = {}",
            d.quality
        );
    }

    #[test]
    fn find_degenerate_tet_flags_a_near_flat_sliver_at_any_scale() {
        let sliver = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 1e-12],
        ];
        for s in [1e-4, 1.0, 1e3] {
            let coords = scaled(&sliver, s);
            let d = find_degenerate_tet(&coords, &[[0, 1, 2, 3]])
                .unwrap_or_else(|| panic!("sliver scaled by {s} must be flagged"));
            assert_eq!(d.element_id, 0);
            assert!(
                d.quality > 0.0 && d.quality < MIN_TET_SHAPE_QUALITY,
                "sliver (not inverted) quality at scale {s} = {}",
                d.quality
            );
        }
    }

    #[test]
    fn find_degenerate_tet_flags_a_tet_inverted_against_the_mesh() {
        let (coords, mut tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        let k = 13;
        tets[k].swap(0, 1);
        let d = find_degenerate_tet(&coords, &tets).expect("inverted tet must be flagged");
        assert_eq!(d.element_id, k);
        assert!(d.quality < 0.0, "inverted tet quality = {}", d.quality);
    }

    #[test]
    fn find_degenerate_tet_accepts_a_consistently_mirrored_mesh() {
        let (coords, mut tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        for tet in &mut tets {
            tet.swap(0, 1);
        }
        assert_eq!(find_degenerate_tet(&coords, &tets), None);
    }

    #[test]
    fn find_degenerate_tet_flags_a_non_finite_coordinate() {
        let (mut coords, tets) = freudenthal_box([1.0, 0.5, 0.25], [2, 2, 2]);
        let poisoned = coords.len() - 1;
        coords[poisoned][2] = f64::NAN;
        let d = find_degenerate_tet(&coords, &tets).expect("NaN coordinate must be flagged");
        assert!(d.quality.is_nan(), "quality = {}", d.quality);
        assert!(
            tets[d.element_id].contains(&poisoned),
            "flagged element {} must reference the NaN node {poisoned}",
            d.element_id
        );
    }

    #[test]
    fn find_degenerate_tet_on_an_empty_mesh_is_none() {
        assert_eq!(find_degenerate_tet(&[], &[]), None);
    }

    // ── DofDirection ──────────────────────────────────────────────────────────

    #[test]
    fn dof_direction_all_rigid_body_modes_has_exactly_six() {
        let modes = DofDirection::all_rigid_body_modes();
        assert_eq!(
            modes.len(),
            6,
            "rigid-body null space of a connected 3D continuum must have exactly 6 DOFs"
        );
    }

    #[test]
    fn dof_direction_all_rigid_body_modes_canonical_order() {
        let modes = DofDirection::all_rigid_body_modes();
        assert_eq!(
            modes,
            [
                DofDirection::TranslationX,
                DofDirection::TranslationY,
                DofDirection::TranslationZ,
                DofDirection::RotationX,
                DofDirection::RotationY,
                DofDirection::RotationZ,
            ],
            "all_rigid_body_modes must return the 6 modes in canonical order"
        );
    }

    // ── ElementId ─────────────────────────────────────────────────────────────

    #[test]
    fn element_id_inner_value_accessible() {
        let id = ElementId(7);
        assert_eq!(id.0, 7, "ElementId(7).0 must equal 7");
    }

    #[test]
    fn element_id_eq_and_copy() {
        let a = ElementId(3);
        let b = a; // Copy
        assert_eq!(a, b, "ElementId must implement Copy + PartialEq");
    }

    // ── FeaDiagnosticDetail ───────────────────────────────────────────────────

    #[test]
    fn fea_diagnostic_detail_problem_elements_roundtrip() {
        let detail = FeaDiagnosticDetail::ProblemElements {
            ids: vec![ElementId(3), ElementId(5)],
        };
        let expected = FeaDiagnosticDetail::ProblemElements {
            ids: vec![ElementId(3), ElementId(5)],
        };
        assert_eq!(detail, expected, "ProblemElements must round-trip via PartialEq");
    }

    #[test]
    fn fea_diagnostic_detail_unconstrained_eq_self() {
        let detail = FeaDiagnosticDetail::Unconstrained {
            rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
        };
        assert_eq!(
            detail,
            FeaDiagnosticDetail::Unconstrained {
                rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
            },
            "Unconstrained must compare equal to itself"
        );
    }

    #[test]
    fn fea_diagnostic_detail_unresolved_selector_eq_self() {
        let detail = FeaDiagnosticDetail::UnresolvedSelector {
            selector_path: "top".to_string(),
        };
        assert_eq!(
            detail,
            FeaDiagnosticDetail::UnresolvedSelector {
                selector_path: "top".to_string(),
            },
            "UnresolvedSelector must compare equal to itself"
        );
    }

    // ── FeaFailure::structured_detail ─────────────────────────────────────────

    #[test]
    fn structured_detail_under_constrained_yields_all_six_modes() {
        // HEADLINE SIGNAL: unconstrained body → Unconstrained{all 6 rigid-body modes}.
        let f = FeaFailure::UnderConstrained { support_count: 0 };
        assert_eq!(
            f.structured_detail(),
            Some(FeaDiagnosticDetail::Unconstrained {
                rigid_body_modes: DofDirection::all_rigid_body_modes().into(),
            }),
            "UnderConstrained must map to Unconstrained with all 6 rigid-body DOF directions"
        );
    }

    #[test]
    fn structured_detail_singular_stiffness_yields_problem_elements() {
        let f = FeaFailure::SingularStiffness(DegenerateTet {
            element_id: 7,
            quality: 0.0,
        });
        assert_eq!(
            f.structured_detail(),
            Some(FeaDiagnosticDetail::ProblemElements {
                ids: vec![ElementId(7)],
            }),
            "SingularStiffness(element 7) must map to ProblemElements{{ids:[ElementId(7)]}}"
        );
    }

    #[test]
    fn structured_detail_selector_no_match_yields_unresolved_selector() {
        let f = FeaFailure::SelectorNoMatch {
            selector: "oops".to_string(),
            nearest: None,
        };
        assert_eq!(
            f.structured_detail(),
            Some(FeaDiagnosticDetail::UnresolvedSelector {
                selector_path: "oops".to_string(),
            }),
            "SelectorNoMatch must map to UnresolvedSelector with the selector string"
        );
    }

    #[test]
    fn structured_detail_no_loads_is_none() {
        assert_eq!(
            FeaFailure::NoLoads.structured_detail(),
            None,
            "NoLoads has no overlay geometry → None"
        );
    }

    #[test]
    fn structured_detail_non_convergence_is_none() {
        let f = FeaFailure::NonConvergence {
            iterations: 2000,
            max_iter: 2000,
            final_residual: None,
        };
        assert_eq!(
            f.structured_detail(),
            None,
            "NonConvergence has no overlay geometry → None"
        );
    }

    #[test]
    fn structured_detail_thin_body_is_none() {
        assert_eq!(
            FeaFailure::ThinBody { aspect_ratio: 50.0 }.structured_detail(),
            None,
            "ThinBody has no overlay geometry → None"
        );
    }

    #[test]
    fn structured_detail_load_on_interior_is_none() {
        assert_eq!(
            FeaFailure::LoadOnInterior {
                selector: "mid".to_string(),
            }
            .structured_detail(),
            None,
            "LoadOnInterior has no overlay geometry → None"
        );
    }
}
