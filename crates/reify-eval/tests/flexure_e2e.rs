//! Producer-side end-to-end tests for PRB flexure constructors
//! (Compliant-Joints PRD §10.1 / §1 CI gate).
//!
//! - `cantilever_beam_prb_runs_end_to_end`: compiles and evals
//!   `examples/flexures/cantilever_beam_prb.ri` (PRD §10.1 row 1), checks
//!   diagnostic-clean and spring_rate within 1% of Howell k_θ = 2.65·E·I/L.
//! - `notch_hinge_circular_prb_runs_end_to_end`: compiles and evals
//!   `examples/flexures/notch_hinge_circular_prb.ri` (PRD §10.1 row 2), checks
//!   diagnostic-clean and spring_rate within 2% of Paros-Weisbord
//!   k_θ = 2·E·b·t^2.5/(9π·r^0.5).

#![allow(clippy::mutable_key_type)]

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// The cantilever worked-example source (L=20mm, b=5mm, h=0.5mm, Steel_AISI_1045 E=205GPa).
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/flexures/cantilever_beam_prb.ri")
}

/// The notch worked-example source (r=1mm, t=0.2mm, b=5mm, Steel_AISI_1045 E=205GPa).
fn notch_source() -> &'static str {
    include_str!("../../../examples/flexures/notch_hinge_circular_prb.ri")
}

/// The parallelogram-stage worked-example source
/// (L=20mm, b=5mm, t=0.5mm, blade_spacing=10mm, Steel_AISI_1045 E=205GPa).
fn parallelogram_source() -> &'static str {
    include_str!("../../../examples/flexures/parallelogram_stage.ri")
}

/// The double-parallelogram worked-example source
/// (same geometry as parallelogram; two stages in mirror-symmetric series).
fn double_parallelogram_source() -> &'static str {
    include_str!("../../../examples/flexures/double_parallelogram.ri")
}

/// Look up `key` in a `Value::Map`, returning `None` for any other variant.
fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Map(m) => m.get(&Value::String(key.to_string())),
        _ => None,
    }
}

#[test]
fn cantilever_beam_prb_runs_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(cantilever_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // The example must be diagnostic-clean — γ emits no diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // The pivot_flexure cell is a revolute joint Map.
    let id = ValueCellId::new("CantileverBeamPrb", "pivot_flexure");
    let flexure = eval_result.values.get(&id).unwrap_or_else(|| {
        panic!("CantileverBeamPrb.pivot_flexure cell missing from eval result")
    });
    assert_eq!(
        map_get(flexure, "kind"),
        Some(&Value::String("revolute".to_string())),
        "pivot_flexure presents as a revolute joint; got {flexure:?}"
    );

    // spring_rate within 1% of analytic k_θ = 2.65·E·I/L (closed-form Howell §5.1).
    let length = 0.02_f64;
    let width = 0.005_f64;
    let thickness = 0.0005_f64;
    let e = 205e9_f64; // Steel_AISI_1045
    let i = width * thickness.powi(3) / 12.0;
    let k_expected = 2.65 * e * i / length;
    match map_get(flexure, "spring_rate") {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - k_expected).abs() / k_expected;
            assert!(
                rel < 0.01,
                "spring_rate {si_value} within 1% of analytic {k_expected} (rel {rel})"
            );
        }
        other => panic!("expected spring_rate Scalar, got {other:?}"),
    }
}

/// step-13: RED→GREEN — parallelogram stage end-to-end (§10.1 row 3).
///
/// Compiles and evals `examples/flexures/parallelogram_stage.ri`, asserts
/// diagnostic-clean, and checks the §10.1 row-3 producer signal:
/// - spring_rate within 1% of 48·E·I/L³
/// - transverse_stiffness / spring_rate ≥ 1000 (stiffness ratio)
/// - parasitic_error < L/1000
#[test]
fn parallelogram_stage_runs_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(parallelogram_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Diagnostic-clean — η emits no Error diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // The stage_flexure cell is a prismatic joint Map.
    let id = ValueCellId::new("ParallelogramStagePrb", "stage_flexure");
    let flexure = eval_result.values.get(&id).unwrap_or_else(|| {
        panic!("ParallelogramStagePrb.stage_flexure cell missing from eval result")
    });
    assert_eq!(
        map_get(flexure, "kind"),
        Some(&Value::String("prismatic".to_string())),
        "stage_flexure presents as a prismatic joint; got {flexure:?}"
    );

    // Analytic fixture values: L=20mm, b=5mm, t=0.5mm, E=205GPa (Steel_AISI_1045).
    let length = 0.02_f64;
    let width = 0.005_f64;
    let thickness = 0.0005_f64;
    let e = 205e9_f64;
    let i = width * thickness.powi(3) / 12.0;

    // spring_rate within 1% of k_stage = 48·E·I/L³ (four fixed-guided blades, γ=12).
    let k_expected = 48.0 * e * i / length.powi(3);
    let spring_rate_si = match map_get(flexure, "spring_rate") {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - k_expected).abs() / k_expected;
            assert!(
                rel < 0.01,
                "spring_rate {si_value} within 1% of analytic {k_expected} (rel {rel})"
            );
            *si_value
        }
        other => panic!("expected spring_rate Scalar, got {other:?}"),
    };

    // §10.1 row 3: transverse_stiffness / spring_rate ≥ 1000.
    let transverse_si = match map_get(flexure, "transverse_stiffness") {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected transverse_stiffness Scalar, got {other:?}"),
    };
    let ratio = transverse_si / spring_rate_si;
    assert!(
        ratio >= 1000.0,
        "transverse/spring ratio {ratio} ≥ 1000 (§10.1 row 3)"
    );

    // §10.1 row 3: parasitic_error is Option(Some(Length)) with si_value < L/1000.
    match map_get(flexure, "parasitic_error") {
        Some(Value::Option(Some(inner))) => match inner.as_ref() {
            Value::Scalar { si_value, .. } => {
                assert!(
                    *si_value < length / 1000.0,
                    "parasitic_error {si_value} < L/1000 = {} (§10.1 row 3)",
                    length / 1000.0
                );
            }
            other => panic!("parasitic_error inner: expected Scalar, got {other:?}"),
        },
        other => panic!("expected parasitic_error Option(Some(Scalar)), got {other:?}"),
    }
}

#[test]
fn notch_hinge_circular_prb_runs_end_to_end() {
    use std::f64::consts::PI;

    let compiled = parse_and_compile_with_stdlib(notch_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // The example must be diagnostic-clean — δ emits no diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // The pivot_flexure cell is a revolute joint Map.
    let id = ValueCellId::new("NotchHingeCircularPrb", "pivot_flexure");
    let flexure = eval_result.values.get(&id).unwrap_or_else(|| {
        panic!("NotchHingeCircularPrb.pivot_flexure cell missing from eval result")
    });
    assert_eq!(
        map_get(flexure, "kind"),
        Some(&Value::String("revolute".to_string())),
        "pivot_flexure presents as a revolute joint; got {flexure:?}"
    );

    // spring_rate within 2% of Paros-Weisbord k_θ = 2·E·b·t^2.5/(9π·r^0.5)
    // (PRD §10.1 row 2: r=1mm, t=0.2mm, b=5mm, Steel_AISI_1045 E=205GPa).
    let r = 1e-3_f64;
    let t = 2e-4_f64;
    let b = 5e-3_f64;
    let e = 205e9_f64;
    let k_expected = 2.0 * e * b * t.powf(2.5) / (9.0 * PI * r.sqrt());
    match map_get(flexure, "spring_rate") {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - k_expected).abs() / k_expected;
            assert!(
                rel < 0.02,
                "spring_rate {si_value} within 2% of Paros-Weisbord {k_expected} (rel {rel})"
            );
        }
        other => panic!("expected spring_rate Scalar, got {other:?}"),
    }
}

/// step-14: RED→GREEN — double-parallelogram end-to-end (§10.1 row 4).
///
/// Compiles and evals `examples/flexures/double_parallelogram.ri`, asserts
/// diagnostic-clean, and checks the §10.1 row-4 producer signal:
/// - spring_rate within 1% of 24·E·I/L³ (series-halved)
/// - parasitic_error < L/100000 (mirror-cancellation, 4+ orders better than single)
#[test]
fn double_parallelogram_runs_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(double_parallelogram_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Diagnostic-clean — η emits no Error diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // The stage_flexure cell is a prismatic joint Map.
    let id = ValueCellId::new("DoubleParallelogramPrb", "stage_flexure");
    let flexure = eval_result.values.get(&id).unwrap_or_else(|| {
        panic!("DoubleParallelogramPrb.stage_flexure cell missing from eval result")
    });
    assert_eq!(
        map_get(flexure, "kind"),
        Some(&Value::String("prismatic".to_string())),
        "stage_flexure presents as a prismatic joint; got {flexure:?}"
    );

    // Analytic fixture values: L=20mm, b=5mm, t=0.5mm, E=205GPa (Steel_AISI_1045).
    let length = 0.02_f64;
    let width = 0.005_f64;
    let thickness = 0.0005_f64;
    let e = 205e9_f64;
    let i = width * thickness.powi(3) / 12.0;

    // spring_rate within 1% of k_stage/2 = 24·E·I/L³ (two stages in series).
    let k_expected = 24.0 * e * i / length.powi(3);
    match map_get(flexure, "spring_rate") {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - k_expected).abs() / k_expected;
            assert!(
                rel < 0.01,
                "spring_rate {si_value} within 1% of analytic {k_expected} (rel {rel})"
            );
        }
        other => panic!("expected spring_rate Scalar, got {other:?}"),
    }

    // §10.1 row 4: parasitic_error is Option(Some(Length)) with si_value < L/100000
    // (mirror-cancellation residual, approximately 4 orders better than single stage).
    match map_get(flexure, "parasitic_error") {
        Some(Value::Option(Some(inner))) => match inner.as_ref() {
            Value::Scalar { si_value, .. } => {
                assert!(
                    *si_value < length / 100_000.0,
                    "double parasitic {si_value} < L/100000 = {} (§10.1 row 4)",
                    length / 100_000.0
                );
            }
            other => panic!("parasitic_error inner: expected Scalar, got {other:?}"),
        },
        other => panic!("expected parasitic_error Option(Some(Scalar)), got {other:?}"),
    }
}
