    use super::*;
    use reify_core::Severity;
    use reify_ir::ElementOrderTag;

    fn extrude_kind() -> crate::sweep_classifier::SweptKind {
        use reify_ir::Value;
        crate::sweep_classifier::SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        }
    }

    #[test]
    fn p2_substitution_happy_path_extrude_emits_info_diagnostic() {
        let kind = extrude_kind();
        let result = p2_substitution_diagnostic(
            Some(&kind),
            false, // force_tet
            ElementOrderTag::P2,
            "B1",
        );
        let diag = result.expect("expected Some(Diagnostic) for qualifying body with P2");
        assert_eq!(
            diag.severity,
            Severity::Info,
            "diagnostic must have Info severity"
        );
        assert_eq!(
            diag.message,
            "Body B1 qualified for hex/wedge meshing; P1 hex used despite `element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet.",
            "diagnostic message must match PRD wording verbatim"
        );
    }

    /// Suppression cases: each of the three gating conditions independently
    /// disables diagnostic emission and returns `None`.
    ///
    /// (a) element_order = P1 — no substitution happening, nothing to warn about.
    /// (b) force_tet = true — hex/wedge was suppressed by the caller; PRD states
    ///     "Diagnostic is suppressed under `force_tet = true`".
    /// (c) swept_kind = None — body doesn't qualify for hex/wedge promotion.
    #[test]
    fn p2_substitution_suppression_cases_return_none() {
        let kind = extrude_kind();

        // (a) P1 element order — no substitution, no diagnostic.
        assert!(
            p2_substitution_diagnostic(Some(&kind), false, ElementOrderTag::P1, "B_P1").is_none(),
            "(a) element_order=P1 must return None"
        );

        // (b) force_tet=true — hex/wedge suppressed; diagnostic must not fire.
        assert!(
            p2_substitution_diagnostic(Some(&kind), true, ElementOrderTag::P2, "B_ForceTet")
                .is_none(),
            "(b) force_tet=true must return None"
        );

        // (c) swept_kind=None — body not hex/wedge-eligible; diagnostic must not fire.
        assert!(
            p2_substitution_diagnostic(None, false, ElementOrderTag::P2, "B_NoSweep").is_none(),
            "(c) swept_kind=None must return None"
        );
    }

    /// Variant invariance: Revolve and SweepLinear swept-body types both emit
    /// the info diagnostic when the other conditions are met.
    ///
    /// This pins that the helper does NOT gate on a specific `SweptKind` variant
    /// — any future refactor that accidentally adds a variant-specific branch
    /// (e.g. only emitting for Extrude) will break this test.
    #[test]
    fn p2_substitution_variant_invariance_revolve_and_sweep_linear_emit() {
        use std::f64::consts::FRAC_PI_2;

        let revolve_kind = crate::sweep_classifier::SweptKind::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: FRAC_PI_2,
        };

        let sweep_linear_kind = crate::sweep_classifier::SweptKind::SweepLinear {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(1),
        };

        // Compute expected message per PRD task #10 — identical wording for all
        // variants (only the body label differs). Using a closure rather than a
        // const so we can substitute the label while keeping the format string
        // in one place; any future drift in `p2_substitution_diagnostic`'s
        // wording will fail both assertions simultaneously.
        let expected_msg = |label: &str| -> String {
            format!(
                "Body {label} qualified for hex/wedge meshing; P1 hex used despite \
`element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet."
            )
        };

        // Revolve variant.
        let revolve_result = p2_substitution_diagnostic(
            Some(&revolve_kind),
            false,
            ElementOrderTag::P2,
            "RevolvedDisc",
        );
        let revolve_diag =
            revolve_result.expect("Revolve variant must emit Some(Diagnostic) with P2");
        assert_eq!(revolve_diag.severity, Severity::Info);
        assert_eq!(
            revolve_diag.message,
            expected_msg("RevolvedDisc"),
            "Revolve diagnostic must match PRD wording verbatim"
        );

        // SweepLinear variant.
        let sweep_result = p2_substitution_diagnostic(
            Some(&sweep_linear_kind),
            false,
            ElementOrderTag::P2,
            "SweptBar",
        );
        let sweep_diag =
            sweep_result.expect("SweepLinear variant must emit Some(Diagnostic) with P2");
        assert_eq!(sweep_diag.severity, Severity::Info);
        assert_eq!(
            sweep_diag.message,
            expected_msg("SweptBar"),
            "SweepLinear diagnostic must match PRD wording verbatim"
        );
    }
