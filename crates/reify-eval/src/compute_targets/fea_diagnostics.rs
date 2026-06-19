// crates/reify-eval/src/compute_targets/fea_diagnostics.rs
//
// Maps a solver-side `FeaFailure` to a `reify_core::Diagnostic`.
//
// This module lives in reify-eval because:
//   - reify-solver-elastic has no reify-core dependency (neutral types only).
//   - reify-eval depends on BOTH reify-solver-elastic AND reify-core.
//   - The mapping is therefore the natural glue layer here.
//
// All TODAY callers pass `span = None` (per the Leo-ratified relaxed scope
// 2026-05-30, esc-2929-40 option B).  The `span: Option<SourceSpan>` parameter
// is kept for future-proofing: the label is only attached when `span` is `Some`.

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};
use reify_solver_elastic::FeaFailure;

/// Map a `FeaFailure` to a `reify_core::Diagnostic`.
///
/// - `message` is taken verbatim from `failure.message()`.
/// - `severity` is `Severity::Error` when `failure.is_error()`, else `Severity::Warning`.
/// - `code` is set to the corresponding `DiagnosticCode::Fea*` variant.
/// - A `DiagnosticLabel` is appended only when `span` is `Some`; all current
///   callers pass `None` (per esc-2929-40 relaxed scope).
pub fn fea_diagnostic_to_core(failure: &FeaFailure, span: Option<SourceSpan>) -> Diagnostic {
    let message = failure.message();
    let code = match failure {
        FeaFailure::UnderConstrained { .. } => DiagnosticCode::FeaUnderConstrained,
        FeaFailure::SingularStiffness { .. } => DiagnosticCode::FeaSingularStiffness,
        FeaFailure::NonConvergence { .. } => DiagnosticCode::FeaNonConvergence,
        FeaFailure::NoLoads => DiagnosticCode::FeaNoLoads,
        FeaFailure::LoadOnInterior { .. } => DiagnosticCode::FeaLoadOnInterior,
        FeaFailure::SelectorNoMatch { .. } => DiagnosticCode::FeaSelectorNoMatch,
        FeaFailure::ThinBody { .. } => DiagnosticCode::FeaThinBody,
    };

    let mut diag = if failure.is_error() {
        Diagnostic::error(message.clone())
    } else {
        Diagnostic::warning(message.clone())
    }
    .with_code(code);

    if let Some(s) = span {
        diag = diag.with_label(DiagnosticLabel::new(s, message));
    }

    diag
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use reify_core::{DiagnosticCode, Severity, SourceSpan};
    use reify_solver_elastic::FeaFailure;

    use super::fea_diagnostic_to_core;

    // ── DiagnosticCode mapping ─────────────────────────────────────────────

    #[test]
    fn no_loads_maps_to_fea_no_loads_code() {
        let f = FeaFailure::NoLoads;
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaNoLoads));
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn under_constrained_maps_to_fea_under_constrained_code() {
        let f = FeaFailure::UnderConstrained { support_count: 0 };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaUnderConstrained));
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn thin_body_maps_to_fea_thin_body_code() {
        let f = FeaFailure::ThinBody {
            aspect_ratio: 100.0,
        };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaThinBody));
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn non_convergence_maps_to_fea_non_convergence_code() {
        let f = FeaFailure::NonConvergence {
            iterations: 2000,
            max_iter: 2000,
            final_residual: Some(1e-3),
        };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaNonConvergence));
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn singular_stiffness_maps_to_fea_singular_stiffness_code() {
        let f = FeaFailure::SingularStiffness { element_id: 5 };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaSingularStiffness));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn load_on_interior_maps_to_fea_load_on_interior_code() {
        let f = FeaFailure::LoadOnInterior {
            selector: "mid".to_string(),
        };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaLoadOnInterior));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, f.message());
    }

    #[test]
    fn selector_no_match_maps_to_fea_selector_no_match_code() {
        let f = FeaFailure::SelectorNoMatch {
            selector: "oops".to_string(),
            nearest: None,
        };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(d.code, Some(DiagnosticCode::FeaSelectorNoMatch));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, f.message());
    }

    // ── Span handling ──────────────────────────────────────────────────────

    #[test]
    fn span_none_produces_no_labels() {
        let f = FeaFailure::NoLoads;
        let d = fea_diagnostic_to_core(&f, None);
        assert!(
            d.labels.is_empty(),
            "span=None must produce no labels, got: {:?}",
            d.labels
        );
    }

    #[test]
    fn span_some_produces_exactly_one_label_with_matching_span() {
        let f = FeaFailure::NoLoads;
        let span = SourceSpan::new(3, 10);
        let d = fea_diagnostic_to_core(&f, Some(span));
        assert_eq!(
            d.labels.len(),
            1,
            "span=Some must produce exactly one label, got {:?}",
            d.labels.len()
        );
        assert_eq!(
            d.labels[0].span, span,
            "label span must match the passed SourceSpan"
        );
    }

    // ── Severity cross-check for all advisory variants ────────────────────

    #[test]
    fn all_advisory_variants_produce_warning() {
        let advisories: Vec<FeaFailure> = vec![
            FeaFailure::NoLoads,
            FeaFailure::UnderConstrained { support_count: 0 },
            FeaFailure::ThinBody {
                aspect_ratio: 100.0,
            },
            FeaFailure::NonConvergence {
                iterations: 10,
                max_iter: 2000,
                final_residual: None,
            },
        ];
        for f in &advisories {
            let d = fea_diagnostic_to_core(f, None);
            assert_eq!(
                d.severity,
                Severity::Warning,
                "expected Warning for {:?}, got {:?}",
                f,
                d.severity
            );
        }
    }

    #[test]
    fn all_error_variants_produce_error() {
        let errors: Vec<FeaFailure> = vec![
            FeaFailure::SingularStiffness { element_id: 0 },
            FeaFailure::LoadOnInterior {
                selector: "x".to_string(),
            },
            FeaFailure::SelectorNoMatch {
                selector: "x".to_string(),
                nearest: None,
            },
        ];
        for f in &errors {
            let d = fea_diagnostic_to_core(f, None);
            assert_eq!(
                d.severity,
                Severity::Error,
                "expected Error for {:?}, got {:?}",
                f,
                d.severity
            );
        }
    }

    // ── Message passthrough ────────────────────────────────────────────────

    #[test]
    fn message_equals_failure_message() {
        let f = FeaFailure::ThinBody { aspect_ratio: 42.5 };
        let d = fea_diagnostic_to_core(&f, None);
        assert_eq!(
            d.message,
            f.message(),
            "fea_diagnostic_to_core must use failure.message() verbatim"
        );
    }
}
