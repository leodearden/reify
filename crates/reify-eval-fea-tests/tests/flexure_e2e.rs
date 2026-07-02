//! Producer-side end-to-end tests for PRB flexure constructors
//! (Compliant-Joints PRD §10.1 / §1 CI gate).
//!
//! - `cantilever_beam_prb_runs_end_to_end`: compiles and evals
//!   `examples/flexures/cantilever_beam_prb.ri` (PRD §10.1 row 1), checks
//!   Error-clean and spring_rate within 1% of Howell k_θ = 2.65·E·I/L.
//! - `notch_hinge_circular_prb_runs_end_to_end`: compiles and evals
//!   `examples/flexures/notch_hinge_circular_prb.ri` (PRD §10.1 row 2), checks
//!   Error-clean and spring_rate within 2% of Paros-Weisbord
//!   k_θ = 2·E·b·t^2.5/(9π·r^0.5).

#![allow(clippy::mutable_key_type)]

use reify_core::{DiagnosticCode, Severity, ValueCellId};
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

/// The yielding-cantilever worked-example source (t=0.05mm, L=2mm, forced ±10°
/// declared range so σ(10°) ≈ 447MPa > 310MPa yield AND ±10° > ±5° PRB bound).
fn yield_warning_source() -> &'static str {
    include_str!("../../../examples/flexures/yield_warning.ri")
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

    // The example must be Error-clean. λ adds a standing once-per-session Info
    // W_FlexureFatigueCheckMissing advisory to every PRB ctor, so the example is
    // no longer strictly diagnostic-free — but it emits no Error severity.
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
    let flexure = eval_result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("CantileverBeamPrb.pivot_flexure cell missing from eval result"));
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
/// Error-clean, and checks the §10.1 row-3 producer signal:
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

    // The example must be Error-clean. λ adds a standing once-per-session Info
    // W_FlexureFatigueCheckMissing advisory to every PRB ctor, so the example is
    // no longer strictly diagnostic-free — but it emits no Error severity.
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
/// Error-clean, and checks the §10.1 row-4 producer signal:
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

    // §10.1 row 4: transverse_stiffness == (single-stage k_transverse)/2,
    // and ratio transverse/spring = (L/t)² (preserved across series composition).
    let k_transverse_expected = 2.0 * e * (width * thickness) / length; // (4·E·b·t/L)/2
    let transverse_si = match map_get(flexure, "transverse_stiffness") {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - k_transverse_expected).abs() / k_transverse_expected;
            assert!(
                rel < 0.01,
                "transverse_stiffness {si_value} within 1% of {k_transverse_expected} (rel {rel})"
            );
            *si_value
        }
        other => panic!("expected transverse_stiffness Scalar, got {other:?}"),
    };
    let ratio = transverse_si / spring_rate_si;
    assert!(
        ratio >= 1000.0,
        "transverse/spring ratio {ratio} ≥ 1000 (§10.1 row 4: ratio preserved)"
    );

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

/// step-9 (RED→GREEN): the yielding-cantilever fixture surfaces the §5.3
/// operating-stress diagnostics end-to-end.
///
/// `examples/flexures/yield_warning.ri` forces a ±10° declared operating range
/// on a short, thin cantilever (t=0.05mm, L=2mm), so σ(10°) ≈ 447 MPa exceeds
/// the 310 MPa steel yield AND the ±10° range exceeds the ±5° PRB bound. The
/// eval pipeline must therefore emit `W_FlexureYielding` + `W_FlexurePrbOutOfRange`
/// (both Warning) and exactly one `W_FlexureFatigueCheckMissing` (Info), while
/// the ctor still returns a valid revolute joint whose cached compliance has
/// `at_yield == true`.
///
/// RED until reify-expr's `FunctionCall` arm calls `flexure_diagnose` on the
/// builtin result and pushes the diagnostics into the eval sink (step-10): today
/// the constructor populates the cached record (so the joint / at_yield
/// assertions already hold) but no flexure diagnostic ever reaches
/// `eval_result.diagnostics`.
#[test]
fn yield_warning_surfaces_flexure_diagnostics_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(yield_warning_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let with_code = |code: DiagnosticCode| -> Vec<_> {
        eval_result
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(code))
            .collect()
    };

    // W_FlexureYielding (Warning): peak surface stress at the declared ±10°
    // endpoint exceeds the material yield.
    let yielding = with_code(DiagnosticCode::FlexureYielding);
    assert_eq!(
        yielding.len(),
        1,
        "exactly one W_FlexureYielding; got diagnostics {:?}",
        eval_result.diagnostics
    );
    assert_eq!(
        yielding[0].severity,
        Severity::Warning,
        "FlexureYielding is a Warning"
    );

    // W_FlexurePrbOutOfRange (Warning): the forced ±10° range exceeds the ±5°
    // pseudo-rigid-body small-deflection bound.
    let oor = with_code(DiagnosticCode::FlexurePrbOutOfRange);
    assert_eq!(
        oor.len(),
        1,
        "exactly one W_FlexurePrbOutOfRange; got diagnostics {:?}",
        eval_result.diagnostics
    );
    assert_eq!(
        oor[0].severity,
        Severity::Warning,
        "FlexurePrbOutOfRange is a Warning"
    );

    // Exactly ONE W_FlexureFatigueCheckMissing (Info) — once per eval session.
    let fatigue = with_code(DiagnosticCode::FlexureFatigueCheckMissing);
    assert_eq!(
        fatigue.len(),
        1,
        "exactly one Info W_FlexureFatigueCheckMissing; got diagnostics {:?}",
        eval_result.diagnostics
    );
    assert_eq!(
        fatigue[0].severity,
        Severity::Info,
        "FlexureFatigueCheckMissing is Info (advisory)"
    );

    // The flexure cell is still a valid revolute joint (not Undef) whose cached
    // compliance record has at_yield == true.
    let id = ValueCellId::new("YieldWarning", "pivot_flexure");
    let flexure = eval_result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("YieldWarning.pivot_flexure cell missing from eval result"));
    assert_eq!(
        map_get(flexure, "kind"),
        Some(&Value::String("revolute".to_string())),
        "pivot_flexure is a valid revolute joint (not Undef); got {flexure:?}"
    );
    match map_get(flexure, "__flexure_compliance") {
        Some(Value::StructureInstance(d)) => {
            assert_eq!(
                d.type_name, "FlexureCompliance",
                "__flexure_compliance is a FlexureCompliance record"
            );
            assert_eq!(
                d.fields.get(&"at_yield".to_string()),
                Some(&Value::Bool(true)),
                "the forced ±10° declared range drives at_yield true"
            );
        }
        other => panic!("expected __flexure_compliance StructureInstance, got {other:?}"),
    }
}

/// The compliance-accessor probe source: builds the yielding cantilever joint
/// (same geometry / forced ±10° range as `yield_warning.ri`) and binds
/// `fc = flexure_compliance(pivot_flexure)` so the test can read the record the
/// PRD §4.2 accessor surfaces.
fn compliance_accessor_source() -> &'static str {
    r#"
structure def ComplianceAccessorProbe {
    let steel = Steel_AISI_1045()
    let pivot_flexure = prb_cantilever_beam(
        2mm, 5mm, 0.05mm, steel, point3(0mm, 0mm, 0mm), vec3(0, 1, 0), 0deg, 10deg)
    let fc = flexure_compliance(pivot_flexure)
}
"#
}

/// step-11 (RED→GREEN): the `flexure_compliance(joint)` accessor (PRD §4.2)
/// returns the POPULATED cached record, not the β sentinel-zero stub.
///
/// The yielding cantilever (t=0.05mm, L=2mm, forced ±10°) caches a
/// `FlexureCompliance` with `at_yield == true` and `max_stress ≈ 447 MPa`. The
/// accessor must surface those populated values — NOT the stub defaults
/// (`at_yield == false`, `max_stress == 0 Pa`) that `FlexureCompliance()`
/// returns today.
///
/// RED until step-12 adds the `__flexure_compliance_get` intrinsic and rewires
/// the accessor body from `FlexureCompliance()` to `__flexure_compliance_get(joint)`.
#[test]
fn flexure_compliance_accessor_returns_populated_record() {
    let compiled = parse_and_compile_with_stdlib(compliance_accessor_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // No Error diagnostics — the §5.3 yield signal is a Warning, and the
    // accessor itself emits nothing (`flexure_compliance` /
    // `__flexure_compliance_get` are not PRB ctors).
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

    // `fc` is the FlexureCompliance record the accessor returned.
    let id = ValueCellId::new("ComplianceAccessorProbe", "fc");
    let fc = eval_result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("ComplianceAccessorProbe.fc cell missing from eval result"));
    let fields = match fc {
        Value::StructureInstance(d) => {
            assert_eq!(
                d.type_name, "FlexureCompliance",
                "accessor returns a FlexureCompliance record"
            );
            &d.fields
        }
        other => panic!("expected fc to be a FlexureCompliance StructureInstance, got {other:?}"),
    };

    // POPULATED at_yield (stub default is false).
    assert_eq!(
        fields.get(&"at_yield".to_string()),
        Some(&Value::Bool(true)),
        "accessor surfaces the populated at_yield=true, not the stub default false"
    );

    // POPULATED max_stress ≈ σ(10°) = E·(t/2)·θ/L ≈ 447 MPa (stub default 0 Pa).
    let theta = 10.0_f64.to_radians();
    let e = 205e9_f64; // Steel_AISI_1045
    let t = 0.05e-3_f64;
    let length = 2e-3_f64;
    let sigma_expected = e * (t / 2.0) * theta / length;
    match fields.get(&"max_stress".to_string()) {
        Some(Value::Scalar { si_value, .. }) => {
            let rel = (si_value - sigma_expected).abs() / sigma_expected;
            assert!(
                rel < 0.01,
                "max_stress {si_value} within 1% of analytic σ(10°) {sigma_expected} \
                 (rel {rel}); NOT the stub 0 Pa"
            );
        }
        other => panic!("expected max_stress Scalar, got {other:?}"),
    }
}

/// step-21 (RED→GREEN): the integration consistency gate — "every PRB primitive
/// emits consistent diagnostics + populates the cache".
///
/// Every existing SAFE worked example (no forced range) must, now that λ wires
/// compliance population into all 13 PRB ctors:
///  (a) eval with NO Error-severity diagnostics, and
///  (b) expose a populated `__flexure_compliance` on its joint cell whose
///      `at_yield == false` — the auto validity range IS the safe envelope, so
///      operating at it is not "yielding".
///
/// RED until step-22 aligns `at_yield` with the strict `max_stress > yield`
/// semantics (margin < 0). The yield-capped families (notch) and the fixed-guided
/// compound stages (parallelogram / double) sit EXACTLY at yield at their auto
/// endpoint — max_stress == yield by construction — so the inclusive `>=` form
/// reports at_yield=true for these safe examples, tripping (b).
fn assert_example_safe_populated(source: &str, struct_name: &str, cell_name: &str) {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{struct_name}.{cell_name}: expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) The joint cell carries a populated FlexureCompliance with at_yield=false.
    let id = ValueCellId::new(struct_name, cell_name);
    let flexure = eval_result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{struct_name}.{cell_name} cell missing from eval result"));
    match map_get(flexure, "__flexure_compliance") {
        Some(Value::StructureInstance(d)) => {
            assert_eq!(
                d.type_name, "FlexureCompliance",
                "{struct_name}.{cell_name}: __flexure_compliance is a FlexureCompliance record"
            );
            assert_eq!(
                d.fields.get(&"at_yield".to_string()),
                Some(&Value::Bool(false)),
                "{struct_name}.{cell_name}: safe geometry (no forced range) ⇒ at_yield false"
            );
        }
        other => panic!(
            "{struct_name}.{cell_name}: expected populated __flexure_compliance, got {other:?}"
        ),
    }
}

#[test]
fn existing_flexure_examples_populate_compliance_and_stay_safe() {
    assert_example_safe_populated(cantilever_source(), "CantileverBeamPrb", "pivot_flexure");
    assert_example_safe_populated(notch_source(), "NotchHingeCircularPrb", "pivot_flexure");
    assert_example_safe_populated(
        parallelogram_source(),
        "ParallelogramStagePrb",
        "stage_flexure",
    );
    assert_example_safe_populated(
        double_parallelogram_source(),
        "DoubleParallelogramPrb",
        "stage_flexure",
    );
}

/// A single eval session constructing several distinct, ALL-SAFE PRB flexures
/// (cantilever revolute, parallelogram prismatic, living-hinge revolute) — to
/// prove the once-per-session diagnostics dedup holds ACROSS different ctors.
fn multi_ctor_session_source() -> &'static str {
    r#"
structure def MultiFlexureSession {
    let steel = Steel_AISI_1045()
    let a = prb_cantilever_beam(20mm, 5mm, 0.5mm, steel, point3(0mm, 0mm, 0mm), vec3(0, 1, 0))
    let b = prb_parallelogram_flexure(20mm, 5mm, 0.5mm, 10mm, steel, vec3(1, 0, 0), point3(0mm, 0mm, 0mm))
    let c = prb_living_hinge(20mm, 5mm, 0.5mm, steel, point3(0mm, 0mm, 0mm), vec3(0, 1, 0))
}
"#
}

/// step-21 (RED→GREEN): the once-per-session dedup holds across MULTIPLE PRB
/// ctors, and all-safe-geometry flexures emit no §5.3 yielding warning.
///
/// RED until step-22: the parallelogram stage sits exactly at yield at its auto
/// endpoint, so under the inclusive `>=` at_yield form it spuriously emits a
/// `W_FlexureYielding` — tripping the "no yielding for safe geometry" assertion.
#[test]
fn multi_ctor_session_surfaces_exactly_one_fatigue_info() {
    let compiled = parse_and_compile_with_stdlib(multi_ctor_session_source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Exactly ONE Info W_FlexureFatigueCheckMissing across THREE PRB ctor calls
    // (the standing advisory is deduped once per eval session, step-10).
    let fatigue: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FlexureFatigueCheckMissing))
        .collect();
    assert_eq!(
        fatigue.len(),
        1,
        "exactly one fatigue Info across 3 ctors (dedup holds across ctors); got {:?}",
        eval_result.diagnostics
    );

    // All three are safe geometry at their auto range ⇒ NO W_FlexureYielding.
    let yielding: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::FlexureYielding))
        .collect();
    assert!(
        yielding.is_empty(),
        "safe auto-range flexures emit no W_FlexureYielding; got {:?}",
        yielding
    );

    // And no Error-severity diagnostics at all.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "no Error diagnostics; got {:?}", errors);
}
