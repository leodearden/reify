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
