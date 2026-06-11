// E2e tests verifying that Gravity load flows through the elastic_static
// trampoline → solve_cantilever_fea → assemble_body_force pipeline,
// producing physically meaningful displacement fields (task 4440 β: §4.3).
//
// Tests rely on RELATIVE physical properties of linear elastostatics:
//   K·u = f,  K depends on geometry/E/ν only (not density),  f_body ∝ ρ·magnitude.
// So displacement scales linearly with both ρ and magnitude.  Ratio tolerances
// (~1e-3 relative) cover float/CG noise only, not method error.
//
// Fixture: clamped bar 1000mm×100mm×100mm, ElasticOptions(deterministic: true).
// Mesh nz=6 → nx=60, ny=1 → n_nodes = 61×2×7 = 854 → n_dofs = 2562 < 10k
// (Deterministic mode) → CG results are reproducible across solves.
//
// RED (step-4): the trampoline captures `_body_force` but does NOT yet pass it
// into `solve_cantilever_fea` (step-2 left it unused).  Gravity solves therefore
// return all-zero displacement → tests (1), (2), (3) fail on the
// "nonzero / net-negative" / "linearity" / "density-scaling" assertions.
// Test (4) ("zero magnitude → zero disp") trivially passes even before step-5.

use reify_core::{Severity, ValueCellId};
use reify_ir::{FieldSourceKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Standard gravity: `Gravity()` with default magnitude (STANDARD_GRAVITY) and
/// default direction [0,0,-1].  Deterministic mode + 1000×100×100 mm bar.
const SOURCE_GRAVITY_STD: &str = r#"
structure def GravityStd {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [Gravity()],
        [FixedSupport(target: "root")],
        ElasticOptions(deterministic: true)
    )
}
"#;

/// Double gravity magnitude: `Gravity(magnitude: 2*STANDARD_GRAVITY())`.
const SOURCE_GRAVITY_DOUBLE_MAG: &str = r#"
structure def GravityDoubleMag {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [Gravity(magnitude: 2*STANDARD_GRAVITY())],
        [FixedSupport(target: "root")],
        ElasticOptions(deterministic: true)
    )
}
"#;

/// Double density: `Steel_AISI_1045(density: 15700kg/m^3)` (2× the 7850 default).
const SOURCE_GRAVITY_DOUBLE_DENSITY: &str = r#"
structure def GravityDoubleDensity {
    let result = solve_elastic_static(
        Steel_AISI_1045(density: 15700kg/m^3), 1000mm, 100mm, 100mm,
        [Gravity()],
        [FixedSupport(target: "root")],
        ElasticOptions(deterministic: true)
    )
}
"#;

/// Zero magnitude: `Gravity(magnitude: 0*STANDARD_GRAVITY())` → zero RHS → exact-zero solve.
const SOURCE_GRAVITY_ZERO_MAG: &str = r#"
structure def GravityZeroMag {
    let result = solve_elastic_static(
        Steel_AISI_1045(), 1000mm, 100mm, 100mm,
        [Gravity(magnitude: 0*STANDARD_GRAVITY())],
        [FixedSupport(target: "root")],
        ElasticOptions(deterministic: true)
    )
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
/// a result. Returns the raw data vector (stride-3: [ux₀, uy₀, uz₀, ux₁, ...]).
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

/// Compile, eval, assert no errors, and return the `displacement` Sampled field
/// data (stride-3 [ux, uy, uz]) from `<struct_name>.result`.
fn eval_gravity_displacement(source: &str, struct_name: &str) -> Vec<f64> {
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // Assert no Error-severity diagnostics first.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for {struct_name}, got: {:?}",
        errors
    );

    let result_cell = ValueCellId::new(struct_name, "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell {struct_name}.result not found in eval result"));

    extract_sampled_field_data(result_val, "displacement")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (1) Sign/downward: `[Gravity()]` produces nonzero displacement and the
/// net sum of uz components (data[3i+2]) is strictly negative.
///
/// RED before step-5: body force is not yet applied → all-zero displacement →
/// "at least one finite non-zero sample" assertion fails.
#[test]
fn gravity_self_weight_sign_downward() {
    let data = eval_gravity_displacement(SOURCE_GRAVITY_STD, "GravityStd");

    assert!(
        !data.is_empty(),
        "displacement Sampled field must not be empty"
    );

    // At least one sample must be finite and non-zero.
    let has_nonzero = data.iter().any(|v| v.is_finite() && v.abs() > 1e-30);
    assert!(
        has_nonzero,
        "gravity solve must produce nonzero displacement; all samples are ~0 \
         (body force not applied?), max|disp| = {}",
        data.iter().fold(0.0_f64, |acc, &v| acc.max(v.abs()))
    );

    // uz components (stride-3, index 2) must sum to a negative value
    // (net downward displacement under -Z gravity).
    let uz_sum: f64 = data.chunks_exact(3).map(|c| c[2]).sum();
    assert!(
        uz_sum < 0.0,
        "sum of uz displacement components must be negative (downward gravity), got {uz_sum}"
    );
}

/// (2) Linearity: `disp(2*STANDARD_GRAVITY())` ≈ 2·disp(`STANDARD_GRAVITY()`).
///
/// Exact consequence of K·u=f with f_body ∝ magnitude; ratio tolerance 1e-3
/// covers float/CG noise only.
///
/// RED before step-5: both solves return all-zero → 2×0 = 0 but the
/// "sign_downward" check (which this test implicitly relies on) fails first.
/// When solves are all-zero, the ratio check also fails because the reference
/// displacement is zero (division-by-zero / NaN path).
#[test]
fn gravity_self_weight_linearity() {
    let disp_std = eval_gravity_displacement(SOURCE_GRAVITY_STD, "GravityStd");
    let disp_2x = eval_gravity_displacement(SOURCE_GRAVITY_DOUBLE_MAG, "GravityDoubleMag");

    assert_eq!(
        disp_std.len(),
        disp_2x.len(),
        "displacement field length must be identical across magnitude variants"
    );

    // At least one component of the reference must be non-zero (otherwise
    // we cannot check ratios).
    let ref_peak = disp_std
        .iter()
        .fold(0.0_f64, |acc, &v| acc.max(v.abs()));
    assert!(
        ref_peak > 1e-30,
        "standard-gravity displacement must be nonzero (body force not applied?), \
         max|disp_std| = {ref_peak}"
    );

    // Every component of disp_2x must be ≈ 2 × disp_std.
    // Tolerance: rtol=1e-3 relative to peak, atol=1e-15 (absolute floor).
    let rtol = 1e-3;
    let atol = ref_peak * 1e-12;
    for (i, (&d_std, &d_2x)) in disp_std.iter().zip(disp_2x.iter()).enumerate() {
        let expected = 2.0 * d_std;
        let diff = (d_2x - expected).abs();
        let tol = rtol * expected.abs().max(ref_peak) + atol;
        assert!(
            diff <= tol,
            "component {i}: disp(2×g)={d_2x}, expected ≈2×disp(g)={expected}, \
             diff={diff} > tol={tol}"
        );
    }
}

/// (3) Density-scaling: `disp(density: 15700kg/m³)` ≈ 2·disp(density: 7850kg/m³).
///
/// Exact consequence of K·u=f with f_body ∝ ρ; ratio tolerance 1e-3.
///
/// RED before step-5: same as linearity test above.
#[test]
fn gravity_self_weight_density_scaling() {
    let disp_std = eval_gravity_displacement(SOURCE_GRAVITY_STD, "GravityStd");
    let disp_2rho = eval_gravity_displacement(SOURCE_GRAVITY_DOUBLE_DENSITY, "GravityDoubleDensity");

    assert_eq!(
        disp_std.len(),
        disp_2rho.len(),
        "displacement field length must be identical across density variants"
    );

    let ref_peak = disp_std
        .iter()
        .fold(0.0_f64, |acc, &v| acc.max(v.abs()));
    assert!(
        ref_peak > 1e-30,
        "standard-gravity displacement must be nonzero (body force not applied?), \
         max|disp_std| = {ref_peak}"
    );

    let rtol = 1e-3;
    let atol = ref_peak * 1e-12;
    for (i, (&d_std, &d_2rho)) in disp_std.iter().zip(disp_2rho.iter()).enumerate() {
        let expected = 2.0 * d_std;
        let diff = (d_2rho - expected).abs();
        let tol = rtol * expected.abs().max(ref_peak) + atol;
        assert!(
            diff <= tol,
            "component {i}: disp(2ρ)={d_2rho}, expected ≈2×disp(ρ)={expected}, \
             diff={diff} > tol={tol}"
        );
    }
}

/// (4) Zero magnitude: `Gravity(magnitude: 0*STANDARD_GRAVITY())` → max|displacement| < 1e-9.
///
/// Zero RHS → exact-zero solve; this passes before step-5 (all-zero body force
/// → all-zero displacement, which satisfies the near-zero assertion).
#[test]
fn gravity_zero_magnitude_yields_zero_displacement() {
    let data = eval_gravity_displacement(SOURCE_GRAVITY_ZERO_MAG, "GravityZeroMag");

    assert!(
        !data.is_empty(),
        "displacement Sampled field must not be empty even for zero-magnitude gravity"
    );

    let max_abs = data.iter().fold(0.0_f64, |acc, &v| acc.max(v.abs()));
    assert!(
        max_abs < 1e-9,
        "zero-magnitude gravity must yield near-zero displacement, got max|disp|={max_abs}"
    );
}
