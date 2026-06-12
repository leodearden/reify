// η integration gate — typed FEA authoring e2e (task 4445).
//
// Locks the whole typed FEA surface (α–ζ) together at the eval layer:
//   - examples/multi_load_bracket.ri  — migrated to typed PointLoad/Gravity/FixedSupport (ε/4443)
//   - examples/fea_cantilever_smoke.ri — direct-pass material, no ConstitutiveLawInput (γ/4441 + δ/4442)
//   - examples/fea_multi_case_smoke.ri — mixed List<Load> fixture + live β gravity solve (η/4445)
//
// Complements the COMPILER-layer multi_load_bracket_example_tests.rs;
// this file is eval-layer (runtime behavior, not source-text shape).
//
// PRD reference: docs/prds/v0_6/typed-fea-authoring-surface.md §8 η.

use reify_core::{Severity, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_field(val: &Value, field: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

fn extract_sampled_field_data(result: &Value, field: &str) -> Vec<f64> {
    let field_val = extract_field(result, field)
        .unwrap_or_else(|| panic!("field '{}' not found in result", field));
    match &field_val {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "field '{}' source must be Sampled, got: {:?}",
                field,
                source
            );
            match lambda.as_ref() {
                Value::SampledField(sf) => sf.data.clone(),
                other => panic!(
                    "field '{}' lambda must be Value::SampledField, got: {:?}",
                    field, other
                ),
            }
        }
        other => panic!("field '{}' must be Value::Field, got: {:?}", field, other),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (1) `examples/multi_load_bracket.ri` compiles error-free and its
/// LoadCase cells carry typed StructureInstance loads/supports.
///
/// Scope: compile-clean + typed-cell inspection only. Does NOT assert global
/// eval-no-Error, since envelope_von_mises/worst_case/linear_combine run on the
/// MultiCaseResult STUB (live multi-case solve = task 3009, out of scope).
#[test]
fn multi_load_bracket_evals_to_typed_loadcases() {
    let src = include_str!("../../../examples/multi_load_bracket.ri");
    let compiled = parse_and_compile_with_stdlib(src);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "examples/multi_load_bracket.ri must compile without errors; got: {:?}",
        errors
    );

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // operating → LoadCase with loads:[PointLoad], supports:[FixedSupport]
    let operating = eval_result
        .values
        .get(&ValueCellId::new("MultiLoadBracket", "operating"))
        .unwrap_or_else(|| panic!("MultiLoadBracket.operating cell missing"));
    assert_loadcase_loads_type(operating, 0, "PointLoad", "operating.loads[0]");
    assert_loadcase_supports_type(operating, 0, "FixedSupport", "operating.supports[0]");

    // overload → LoadCase with loads:[PointLoad], supports:[FixedSupport]
    let overload = eval_result
        .values
        .get(&ValueCellId::new("MultiLoadBracket", "overload"))
        .unwrap_or_else(|| panic!("MultiLoadBracket.overload cell missing"));
    assert_loadcase_loads_type(overload, 0, "PointLoad", "overload.loads[0]");
    assert_loadcase_supports_type(overload, 0, "FixedSupport", "overload.supports[0]");

    // transport → LoadCase with loads:[Gravity], supports:[FixedSupport]
    let transport = eval_result
        .values
        .get(&ValueCellId::new("MultiLoadBracket", "transport"))
        .unwrap_or_else(|| panic!("MultiLoadBracket.transport cell missing"));
    assert_loadcase_loads_type(transport, 0, "Gravity", "transport.loads[0]");
    assert_loadcase_supports_type(transport, 0, "FixedSupport", "transport.supports[0]");
}

fn assert_loadcase_loads_type(case: &Value, index: usize, expected_type: &str, label: &str) {
    match extract_field(case, "loads") {
        Some(Value::List(items)) => match &items[index] {
            Value::StructureInstance(si) => assert_eq!(
                si.type_name, expected_type,
                "{label} must be StructureInstance{{type_name={expected_type:?}}}; got {:?}",
                si.type_name
            ),
            other => panic!("{label} must be Value::StructureInstance; got {other:?}"),
        },
        other => panic!("LoadCase.loads must be Value::List; got {other:?}"),
    }
}

fn assert_loadcase_supports_type(case: &Value, index: usize, expected_type: &str, label: &str) {
    match extract_field(case, "supports") {
        Some(Value::List(items)) => match &items[index] {
            Value::StructureInstance(si) => assert_eq!(
                si.type_name, expected_type,
                "{label} must be StructureInstance{{type_name={expected_type:?}}}; got {:?}",
                si.type_name
            ),
            other => panic!("{label} must be Value::StructureInstance; got {other:?}"),
        },
        other => panic!("LoadCase.supports must be Value::List; got {other:?}"),
    }
}

/// (2) `examples/fea_cantilever_smoke.ri` compiles error-free, live-solves, and
/// the result is a converged ElasticResult (proves direct-pass material +
/// typed PointLoad/FixedSupport run end-to-end, δ/4442 + γ/4441).
#[test]
fn fea_cantilever_smoke_live_solves_direct_material() {
    let src = include_str!("../../../examples/fea_cantilever_smoke.ri");
    let compiled = parse_and_compile_with_stdlib(src);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fea_cantilever_smoke.ri must eval without errors; got: {:?}",
        errors
    );

    let result_val = eval_result
        .values
        .get(&ValueCellId::new("FeaCantileverSmoke", "result"))
        .unwrap_or_else(|| panic!("FeaCantileverSmoke.result cell missing"));

    let converged = extract_field(result_val, "converged")
        .unwrap_or_else(|| panic!("could not extract 'converged' field from result"));
    assert_eq!(
        converged,
        Value::Bool(true),
        "FeaCantileverSmoke.result.converged must be Bool(true); got: {converged:?}"
    );
}

/// (3) `examples/fea_multi_case_smoke.ri` compiles error-free and its `bundle`
/// cell is a LoadCase with a MIXED `List<Load>` ([PointLoad, Gravity]) and
/// `List<Support>` ([FixedSupport]) — proves ζ/4444 tightening accepts both
/// Load subtypes in one tightened list.
///
/// RED state: this test's `include_str!` references the fixture that does not
/// exist yet → the binary fails to compile, making all three tests pending.
/// Both (1) and (2) go green once the binary compiles in step-2.
#[test]
fn fea_multi_case_fixture_typechecks_mixed_loadcase() {
    let src = include_str!("../../../examples/fea_multi_case_smoke.ri");
    let compiled = parse_and_compile_with_stdlib(src);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "examples/fea_multi_case_smoke.ri must compile without errors; got: {:?}",
        errors
    );

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    let bundle = eval_result
        .values
        .get(&ValueCellId::new("FeaMultiCaseSmoke", "bundle"))
        .unwrap_or_else(|| panic!("FeaMultiCaseSmoke.bundle cell missing"));

    // bundle must be a LoadCase
    match bundle {
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "LoadCase",
            "FeaMultiCaseSmoke.bundle must have type_name=\"LoadCase\"; got {:?}",
            data.type_name
        ),
        other => panic!("expected Value::StructureInstance for bundle; got {other:?}"),
    }

    // loads = [PointLoad, Gravity]  — mixed tightened List<Load>
    match extract_field(bundle, "loads") {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                2,
                "bundle.loads must have 2 elements (PointLoad + Gravity); got {:?}",
                items
            );
            match &items[0] {
                Value::StructureInstance(si) => assert_eq!(
                    si.type_name, "PointLoad",
                    "bundle.loads[0] must be PointLoad; got {:?}",
                    si.type_name
                ),
                other => panic!("bundle.loads[0] must be StructureInstance; got {other:?}"),
            }
            match &items[1] {
                Value::StructureInstance(si) => assert_eq!(
                    si.type_name, "Gravity",
                    "bundle.loads[1] must be Gravity; got {:?}",
                    si.type_name
                ),
                other => panic!("bundle.loads[1] must be StructureInstance; got {other:?}"),
            }
        }
        other => panic!("bundle.loads must be Value::List; got {other:?}"),
    }

    // supports = [FixedSupport]
    match extract_field(bundle, "supports") {
        Some(Value::List(items)) => {
            assert_eq!(
                items.len(),
                1,
                "bundle.supports must have 1 element; got {:?}",
                items
            );
            match &items[0] {
                Value::StructureInstance(si) => assert_eq!(
                    si.type_name, "FixedSupport",
                    "bundle.supports[0] must be FixedSupport; got {:?}",
                    si.type_name
                ),
                other => panic!("bundle.supports[0] must be StructureInstance; got {other:?}"),
            }
        }
        other => panic!("bundle.supports must be Value::List; got {other:?}"),
    }
}

/// (4) `examples/fea_multi_case_smoke.ri` fixture gravity solve produces a
/// nonzero, net-downward displacement field (β/4440 integration signal:
/// sign + nonzero only — linearity/density/zero stay in gravity_self_weight_e2e.rs).
///
/// RED state (step-3): fixture has no `self_weight` binding yet → cell lookup
/// panics at runtime. Goes green in step-4 when the binding is added.
#[test]
fn fea_multi_case_fixture_gravity_self_weight_downward() {
    let src = include_str!("../../../examples/fea_multi_case_smoke.ri");
    let compiled = parse_and_compile_with_stdlib(src);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "fea_multi_case_smoke.ri must eval without errors for self_weight solve; got: {:?}",
        errors
    );

    let result_val = eval_result
        .values
        .get(&ValueCellId::new("FeaMultiCaseSmoke", "self_weight"))
        .unwrap_or_else(|| {
            panic!(
                "FeaMultiCaseSmoke.self_weight cell missing — fixture needs `let self_weight = \
                 solve_elastic_static(...)` binding (step-4)"
            )
        });

    // Must be a converged ElasticResult
    let converged = extract_field(result_val, "converged")
        .unwrap_or_else(|| panic!("could not extract 'converged' field from self_weight result"));
    assert_eq!(
        converged,
        Value::Bool(true),
        "FeaMultiCaseSmoke.self_weight.converged must be Bool(true); got: {converged:?}"
    );

    let data = extract_sampled_field_data(result_val, "displacement");

    assert!(
        !data.is_empty(),
        "self_weight displacement Sampled field must not be empty"
    );

    let has_nonzero = data.iter().any(|v| v.is_finite() && v.abs() > 1e-30);
    assert!(
        has_nonzero,
        "self_weight displacement must be nonzero (body force not applied?), \
         max|disp| = {}",
        data.iter().fold(0.0_f64, |acc, &v| acc.max(v.abs()))
    );

    // uz components (stride-3, index 2) must sum negative (net downward under -Z gravity).
    let uz_sum: f64 = data.chunks_exact(3).map(|c| c[2]).sum();
    assert!(
        uz_sum < 0.0,
        "sum of uz displacement components must be negative (downward -Z gravity), got {uz_sum}"
    );
}
