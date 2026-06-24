// crates/reify-solver-elastic/src/diagnostics.rs
//
// Neutral FEA failure classification — NO reify-core imports allowed.
// (This crate depends on reify-ir / reify-kernel-gmsh / faer / inventory,
// NOT on reify-core, so Diagnostic / DiagnosticCode / Severity / SourceSpan
// must NOT appear here.)
//
// The mapping from FeaFailure → reify_core::Diagnostic lives in
// reify-eval/src/compute_targets/fea_diagnostics.rs.

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
    /// Returns the canonical 6-mode rigid-body null space in order:
    /// `[TranslationX, TranslationY, TranslationZ, RotationX, RotationY, RotationZ]`.
    ///
    /// This is the exact rigid-body null space of a connected 3D elastic continuum:
    /// 3 rigid translations + 3 rigid axis rotations.  The enumeration is a textbook
    /// identity and requires no eigensolver or null-space analysis.
    pub fn all_rigid_body_modes() -> Vec<DofDirection> {
        vec![
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
    /// One or more elements have near-zero volume (degenerate mesh).
    SingularStiffness { element_id: usize },
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
            FeaFailure::SingularStiffness { element_id } => format!(
                "stiffness matrix is singular: element {element_id} has near-zero volume \
                 (degenerate mesh). Refine the mesh or check geometry for collapsed elements."
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
    /// - `SingularStiffness { element_id }` → [`FeaDiagnosticDetail::ProblemElements`]
    ///   containing `[ElementId(element_id)]` — the degenerate element to highlight.
    /// - `SelectorNoMatch { selector, .. }` → [`FeaDiagnosticDetail::UnresolvedSelector`]
    ///   with `selector_path = selector.clone()`.
    ///
    /// The four non-geometric variants (`NoLoads`, `NonConvergence`, `ThinBody`,
    /// `LoadOnInterior`) return `None` — they convey no geometry for overlay rendering.
    pub fn structured_detail(&self) -> Option<FeaDiagnosticDetail> {
        match self {
            FeaFailure::UnderConstrained { .. } => {
                // A fully-unsupported connected 3D body has exactly the 6-DOF rigid-body
                // null space (3 translations + 3 axis rotations) — a textbook identity.
                // The production solve path only ever flags support_count==0, so the full
                // 6-mode set is always the correct payload.
                Some(FeaDiagnosticDetail::Unconstrained {
                    rigid_body_modes: DofDirection::all_rigid_body_modes(),
                })
            }
            FeaFailure::SingularStiffness { element_id } => {
                // Re-wrap the existing element_id into ProblemElements for outline rendering.
                Some(FeaDiagnosticDetail::ProblemElements {
                    ids: vec![ElementId(*element_id)],
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
            FeaFailure::SingularStiffness { .. }
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

/// Classify a degenerate element.
///
/// Returns `Some(FeaFailure::SingularStiffness { element_id })` when
/// `min_tet_volume < eps`; `None` otherwise.
pub fn classify_degenerate(
    min_tet_volume: f64,
    eps: f64,
    element_id: usize,
) -> Option<FeaFailure> {
    if min_tet_volume < eps {
        Some(FeaFailure::SingularStiffness { element_id })
    } else {
        None
    }
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
    fn singular_stiffness_message_contains_key_phrase() {
        let f = FeaFailure::SingularStiffness { element_id: 7 };
        assert!(
            f.message().contains("near-zero volume"),
            "SingularStiffness message must contain 'near-zero volume', got: {}",
            f.message()
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
        assert!(FeaFailure::SingularStiffness { element_id: 0 }.is_error());
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

    // ── classify_degenerate ───────────────────────────────────────────────────

    #[test]
    fn classify_degenerate_tiny_volume_returns_failure() {
        let result = classify_degenerate(1e-15, 1e-12, 3);
        assert!(
            matches!(result, Some(FeaFailure::SingularStiffness { element_id: 3 })),
            "tiny tet volume must yield SingularStiffness{{element_id:3}}, got {:?}",
            result
        );
    }

    #[test]
    fn classify_degenerate_normal_volume_returns_none() {
        let result = classify_degenerate(1.0, 1e-12, 3);
        assert!(
            result.is_none(),
            "normal tet volume must yield None, got {:?}",
            result
        );
    }

    #[test]
    fn classify_degenerate_at_eps_returns_none() {
        // volume == eps is NOT strictly less than eps → None.
        let result = classify_degenerate(1e-12, 1e-12, 0);
        assert!(
            result.is_none(),
            "volume exactly at eps must yield None (must be strictly <), got {:?}",
            result
        );
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
            vec![
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
            rigid_body_modes: DofDirection::all_rigid_body_modes(),
        };
        assert_eq!(
            detail,
            FeaDiagnosticDetail::Unconstrained {
                rigid_body_modes: DofDirection::all_rigid_body_modes(),
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
                rigid_body_modes: DofDirection::all_rigid_body_modes(),
            }),
            "UnderConstrained must map to Unconstrained with all 6 rigid-body DOF directions"
        );
    }

    #[test]
    fn structured_detail_singular_stiffness_yields_problem_elements() {
        let f = FeaFailure::SingularStiffness { element_id: 4 };
        assert_eq!(
            f.structured_detail(),
            Some(FeaDiagnosticDetail::ProblemElements {
                ids: vec![ElementId(4)],
            }),
            "SingularStiffness{{element_id:4}} must map to ProblemElements{{ids:[ElementId(4)]}}"
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
