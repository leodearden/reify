// step-7 RED (task 4264): e2e tests for PressureLoad flowing through the
// elastic_static trampoline → solve_cantilever_fea → ElasticResult pipeline.
//
// Both tests are RED until step-8 wires `extract_pressure_loads` + passes the
// pressure specs into `solve_cantilever_fea` inside the trampoline.
//
// Before step-8:
//   - The trampoline passes `&[]` (empty pressures) to `solve_cantilever_fea`.
//   - PressureLoad is never assembled into the force vector.
//   - Result: max_von_mises == 0, displacement is all-zero.
//   - Both tests assert finite > 0 / at least one non-zero sample → FAIL.
//
// After step-8:
//   - `extract_pressure_loads` reads PressureLoad specs from `value_inputs[4]`.
//   - The spec is passed to `solve_cantilever_fea`, which calls
//     `assemble_box_face_pressures` → apply_traction_load.
//   - max_von_mises is finite > 0; displacement has non-zero samples → PASS.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Fixture sources ───────────────────────────────────────────────────────────

/// Single-case pressure smoke fixture (user-facing file; include_str! pins it).
fn pressure_smoke_source() -> &'static str {
    include_str!("../../../examples/fea_pressure_smoke.ri")
}

/// Multi-case fixture: one LoadCase with a PressureLoad (mirrors the
/// TWO_CASE_SOURCE pattern from multi_case_compute_node.rs but uses
/// PressureLoad instead of PointLoad so the multi_case delegation path
/// is also exercised).
const PRESSURE_MULTI_CASE_SOURCE: &str = r#"
structure def PressureMultiCaseFixture {
    let ci  = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc1 = LoadCase(
        name:     "pressure_case",
        loads:    [PressureLoad(magnitude: 1.0e6, face: "x_max", direction: "normal")],
        supports: [FixedSupport(target: "root")],
    )
    let result = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1], ElasticOptions())
}
"#;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract a named field from a `Value::StructureInstance` or `Value::Map`.
fn extract_field(val: &Value, field: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract the `SampledField.data` vec from a named `Value::Field{Sampled}` in
/// a result. Returns the raw data vector for non-triviality assertions.
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

// ── step-7(a): Single-case e2e ────────────────────────────────────────────────
//
// Compile `examples/fea_pressure_smoke.ri` (include_str!), eval, assert:
//   - No Error-severity diagnostics.
//   - result.converged == Bool(true).
//   - result.max_von_mises is Value::Scalar{dimension=PRESSURE, si_value finite>0}.
//   - result.displacement Sampled field has ≥1 finite non-zero sample.
//
// RED until step-8: trampoline passes &[] → PressureLoad ignored →
//   max_von_mises == 0, displacement all-zero.

/// Single-case pressure smoke: `fea_pressure_smoke.ri` produces non-trivial fields.
#[test]
fn e2e_pressure_box_produces_nontrivial_fields() {
    let source = pressure_smoke_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (1) No Error diagnostics ─────────────────────────────────────────────
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

    // ── (2) Extract result cell ───────────────────────────────────────────────
    let result_cell = ValueCellId::new("FeaPressureSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaPressureSmoke.result not found in eval result"));

    // ── (3) converged == Bool(true) ───────────────────────────────────────────
    let converged = extract_field(result_val, "converged").unwrap_or_else(|| {
        panic!("could not extract 'converged' from result: {result_val:?}")
    });
    assert_eq!(
        converged,
        Value::Bool(true),
        "expected converged == Bool(true), got: {converged:?}"
    );

    // ── (4) max_von_mises is Scalar(PRESSURE), finite > 0 ────────────────────
    let mvm = extract_field(result_val, "max_von_mises")
        .unwrap_or_else(|| panic!("max_von_mises field missing from result: {result_val:?}"));
    let si_value = match &mvm {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "max_von_mises dimension must be PRESSURE, got: {dimension:?}"
            );
            *si_value
        }
        other => panic!("expected max_von_mises to be Value::Scalar, got: {other:?}"),
    };
    assert!(
        si_value.is_finite() && si_value > 0.0,
        "max_von_mises must be finite > 0 (non-trivial solve), got: {si_value}"
    );

    // ── (5) displacement Sampled field has ≥1 finite non-zero sample ──────────
    //
    // Before step-8: all samples are 0 because no pressure force is assembled.
    // After step-8: at least some nodes have non-zero axial displacement.
    let disp_data = extract_sampled_field_data(result_val, "displacement");
    assert!(
        !disp_data.is_empty(),
        "displacement Sampled field data must not be empty"
    );
    let has_nonzero = disp_data
        .iter()
        .any(|v| v.is_finite() && v.abs() > 1e-30);
    assert!(
        has_nonzero,
        "displacement field must have at least one finite non-zero sample \
         (all-zero indicates no load was applied); max abs = {}",
        disp_data
            .iter()
            .fold(0.0_f64, |acc, &v| acc.max(v.abs()))
    );
}

// ── step-7(b): Multi-case e2e ─────────────────────────────────────────────────
//
// Inline `solve_load_cases` source with one LoadCase carrying a PressureLoad.
// Assert that the per-case ElasticResult in the `cases` Map has max_von_mises
// finite > 0 — verifying PressureLoad is picked up through the multi_case
// delegation path (multi_case.rs → solve_elastic_static_trampoline).
//
// RED until step-8: trampoline passes &[] → PressureLoad ignored →
//   max_von_mises == 0.

/// Multi-case via delegation: PressureLoad in a LoadCase produces non-trivial stress.
#[allow(clippy::mutable_key_type)] // Value contains AtomicBool; map is read-only after construction
#[test]
fn e2e_pressure_flows_through_multi_case() {
    let compiled = parse_and_compile_with_stdlib(PRESSURE_MULTI_CASE_SOURCE);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (1) No Error diagnostics ─────────────────────────────────────────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );

    // ── (2) Extract cases map ─────────────────────────────────────────────────
    let result_cell = ValueCellId::new("PressureMultiCaseFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell PressureMultiCaseFixture.result not found"));

    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_load_cases result must be Value::Map, got: {:?}",
            std::mem::discriminant(other)
        ),
    };
    assert_eq!(
        cases_map.len(),
        1,
        "cases map must have exactly 1 entry; got {} entries: {:?}",
        cases_map.len(),
        cases_map.keys().collect::<Vec<_>>()
    );

    // ── (3) Per-case: max_von_mises finite > 0 ───────────────────────────────
    let case_val = cases_map
        .get(&Value::String("pressure_case".to_string()))
        .unwrap_or_else(|| {
            panic!(
                "cases map must contain \"pressure_case\"; got: {:?}",
                cases_map.keys().collect::<Vec<_>>()
            )
        });

    let mvm = extract_field(case_val, "max_von_mises")
        .unwrap_or_else(|| panic!("max_von_mises missing from pressure_case: {case_val:?}"));
    let si_value = match &mvm {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "pressure_case max_von_mises must be Value::Scalar, got: {other:?}"
        ),
    };
    assert!(
        si_value.is_finite() && si_value > 0.0,
        "pressure_case max_von_mises must be finite > 0 (PressureLoad must flow \
         through multi_case delegation), got: {si_value}"
    );
}
