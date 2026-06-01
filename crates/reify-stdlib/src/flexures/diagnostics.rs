//! Flexure PRB-constructor diagnostic classifier.
//!
//! `flexure_diagnose(name, args, result)` is the flexure analogue of
//! `stackup::diagnose` / `fea::diagnose`: a pure post-call classifier invoked
//! from reify-expr's `FunctionCall` arm (task 3871). Unlike the stackup/fea
//! helpers — which return a single `Diagnostic` only on a `Value::Undef` result
//! — `flexure_diagnose` runs on BOTH the success and the `Undef` path and
//! returns a `Vec<Diagnostic>`, because one PRB ctor call can surface several
//! signals at once (PRD `docs/prds/v0_3/compliant-joints.md` §1 / §5.3):
//!   • `W_FlexureYielding`            (Warning) — cached `at_yield == true`
//!   • `W_FlexurePrbOutOfRange`       (Warning) — declared range exceeds ±5°
//!   • `E_FlexureGeometryInvalid`     (Error)   — degenerate geometry on Undef
//!   • `W_FlexureFatigueCheckMissing` (Info)    — standing advisory, every call
//!
//! The once-per-session dedup of the Info fatigue advisory is the emission
//! layer's responsibility (reify-expr, step-10), not this classifier's.

use reify_core::Diagnostic;
use reify_ir::Value;

/// Classify the diagnostics a PRB flexure constructor call should surface.
///
/// step-7 RED scaffolding: returns an empty `Vec` so the step-7 unit tests
/// compile and fail on their assertions; the real classification logic lands in
/// step-8 (GREEN).
// `#[allow(dead_code)]` is transient: until step-10 wires the call into
// reify-expr's `FunctionCall` arm and re-exports it from `lib.rs`
// (`pub use flexures::flexure_diagnose`), this fn is reachable only from the
// `#[cfg(test)]` unit tests, so the non-test lib build would flag it dead.
// Removed in step-10 when the `pub use` makes it live.
#[allow(dead_code)]
pub(crate) fn flexure_diagnose(name: &str, args: &[Value], result: &Value) -> Vec<Diagnostic> {
    let _ = (name, args, result);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::flexure_diagnose;
    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::Value;

    /// First diagnostic in `diags` carrying `code` (panics with the observed
    /// code set if none match).
    fn find(diags: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
        diags
            .iter()
            .find(|d| d.code == Some(code))
            .unwrap_or_else(|| {
                let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
                panic!("expected a diagnostic with code {code:?}; got codes {codes:?}")
            })
    }

    fn has_code(diags: &[Diagnostic], code: DiagnosticCode) -> bool {
        diags.iter().any(|d| d.code == Some(code))
    }

    fn count_code(diags: &[Diagnostic], code: DiagnosticCode) -> usize {
        diags.iter().filter(|d| d.code == Some(code)).count()
    }

    /// Yielding-cantilever call args: t=0.05 mm, L=2 mm, w=5 mm, steel, neutral
    /// 0°, declared ±10° (σ(10°) ≈ 447 MPa > 310 MPa yield, and 10° > the ±5°
    /// PRB bound), mirroring examples/flexures/yield_warning.ri.
    fn yielding_args() -> Vec<Value> {
        let ten_deg = 10.0_f64 * std::f64::consts::PI / 180.0;
        vec![
            Value::length(0.002),
            Value::length(0.005),
            Value::length(0.00005),
            steel(),
            origin(),
            axis_y(),
            Value::angle(0.0),
            Value::angle(ten_deg),
        ]
    }

    #[test]
    fn flexure_diagnose_yielding_cantilever_emits_yielding_and_prb_out_of_range() {
        let args = yielding_args();
        let result = crate::eval_builtin("prb_cantilever_beam", &args);
        assert!(!result.is_undef(), "yielding cantilever is still a valid joint");
        let diags = flexure_diagnose("prb_cantilever_beam", &args, &result);

        // (a) W_FlexureYielding (Warning): the message reports the surface
        // stress, the material yield, the safety factor (= yield / max_stress),
        // and a suggested narrower operating range.
        let yielding = find(&diags, DiagnosticCode::FlexureYielding);
        assert_eq!(
            yielding.severity,
            Severity::Warning,
            "FlexureYielding is a Warning"
        );
        let m = yielding.message.to_lowercase();
        assert!(m.contains("stress"), "yielding message reports stress: {}", yielding.message);
        assert!(m.contains("yield"), "yielding message reports yield: {}", yielding.message);
        assert!(
            m.contains("safety factor"),
            "yielding message reports the safety factor: {}",
            yielding.message
        );
        assert!(
            m.contains("narrow"),
            "yielding message suggests a narrower range: {}",
            yielding.message
        );

        // (b) W_FlexurePrbOutOfRange (Warning): cites the ±5° PRB bound and the
        // bookmarked nonlinear-FEA escalation path.
        let oor = find(&diags, DiagnosticCode::FlexurePrbOutOfRange);
        assert_eq!(
            oor.severity,
            Severity::Warning,
            "FlexurePrbOutOfRange is a Warning"
        );
        assert!(oor.message.contains("5°"), "out-of-range message cites the 5° bound: {}", oor.message);
        assert!(
            oor.message.to_lowercase().contains("nonlinear") && oor.message.to_lowercase().contains("fea"),
            "out-of-range message cites nonlinear FEA: {}",
            oor.message
        );
        assert!(
            oor.message.contains("compliant-joints"),
            "out-of-range message bookmarks the PRD path: {}",
            oor.message
        );

        // (d) W_FlexureFatigueCheckMissing (Info) accompanies any prb_* call.
        let fatigue = find(&diags, DiagnosticCode::FlexureFatigueCheckMissing);
        assert_eq!(
            fatigue.severity,
            Severity::Info,
            "FlexureFatigueCheckMissing is Info (advisory)"
        );
        assert!(
            fatigue.message.to_lowercase().contains("fatigue"),
            "fatigue advisory mentions fatigue: {}",
            fatigue.message
        );
    }

    #[test]
    fn flexure_diagnose_geometry_invalid_only_for_degenerate_geometry() {
        // (c.1) Degenerate geometry: thickness (3 mm) ≥ length (2 mm) ⇒ Undef ⇒
        // E_FlexureGeometryInvalid (Error).
        let degenerate = vec![
            Value::length(0.002), // L = 2 mm
            Value::length(0.005),
            Value::length(0.003), // t = 3 mm ≥ L (degenerate)
            steel(),
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &degenerate);
        assert!(result.is_undef(), "degenerate geometry returns Undef");
        let diags = flexure_diagnose("prb_cantilever_beam", &degenerate, &result);
        let geo = find(&diags, DiagnosticCode::FlexureGeometryInvalid);
        assert_eq!(
            geo.severity,
            Severity::Error,
            "FlexureGeometryInvalid is an Error"
        );
        assert!(
            geo.message.to_lowercase().contains("geometry"),
            "geometry-invalid message describes the degeneracy: {}",
            geo.message
        );

        // (c.2) Non-geometry Undef: VALID geometry (t=0.5 mm < L=20 mm) but a
        // bad material ⇒ Undef. classify_geometry_invalid must NOT fire.
        let bad_material = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            Value::Real(1.0), // not a material StructureInstance
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &bad_material);
        assert!(result.is_undef(), "bad material returns Undef");
        let diags = flexure_diagnose("prb_cantilever_beam", &bad_material, &result);
        assert!(
            !has_code(&diags, DiagnosticCode::FlexureGeometryInvalid),
            "valid geometry + bad material does NOT emit FlexureGeometryInvalid"
        );
    }

    #[test]
    fn flexure_diagnose_valid_call_emits_fatigue_info() {
        // Plain safe 6-arg cantilever (no declared range): not yielding, in
        // range (auto ±5° cap), so only the standing fatigue advisory fires.
        let args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &args);
        assert!(!result.is_undef());
        let diags = flexure_diagnose("prb_cantilever_beam", &args, &result);

        // (d) Exactly one Info fatigue advisory per call.
        assert_eq!(
            count_code(&diags, DiagnosticCode::FlexureFatigueCheckMissing),
            1,
            "valid prb_* call emits one fatigue Info"
        );
        // A safe, in-range call emits neither yielding nor out-of-range.
        assert!(
            !has_code(&diags, DiagnosticCode::FlexureYielding),
            "safe geometry does not yield"
        );
        assert!(
            !has_code(&diags, DiagnosticCode::FlexurePrbOutOfRange),
            "auto ±5° range is within the PRB bound"
        );
    }

    #[test]
    fn flexure_diagnose_non_flexure_name_is_empty() {
        // (e) A non-flexure builtin name yields no diagnostics, regardless of
        // the result value.
        assert!(flexure_diagnose("box", &[], &Value::Undef).is_empty());
        assert!(flexure_diagnose("stackup_rss", &[], &Value::Undef).is_empty());

        // Even when handed a real joint Map under a non-flexure name, the
        // classifier short-circuits to empty (it dispatches on the name).
        let joint = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(0.02),
                Value::length(0.005),
                Value::length(0.0005),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        assert!(
            flexure_diagnose("not_a_flexure", &[], &joint).is_empty(),
            "non-flexure name short-circuits to empty even with a joint result"
        );
    }
}
