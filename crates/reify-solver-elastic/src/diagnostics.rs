// crates/reify-solver-elastic/src/diagnostics.rs
//
// Neutral FEA failure classification — NO reify-core imports allowed.
// (This crate depends on reify-ir / reify-kernel-gmsh / faer / inventory,
// NOT on reify-core, so Diagnostic / DiagnosticCode / Severity / SourceSpan
// must NOT appear here.)
//
// Implementation added in step-2.  This file contains only the #[cfg(test)]
// unit tests so step-1 (RED) compiles to a "no items" error.

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
}
