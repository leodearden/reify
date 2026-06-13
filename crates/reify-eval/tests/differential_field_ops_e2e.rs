//! End-to-end integration gate for differential field ops (PRD task θ,
//! docs/prds/v0_6/differential-field-operators.md).
//!
//! This is the G2-bearing integration gate for the whole differential-field
//! batch (tasks α–η). Its distinct value over the per-leaf tests is proving
//! Phase-1 (FEA producer-consumer dataflow) and Phase-2 (generic FD
//! construction-gap closure) coexist and evaluate correctly in ONE runnable
//! artifact through the full parse→compile→eval→ComputeNode pipeline.
//!
//! RED: fails to COMPILE until step-2 creates
//!   `examples/differential_field_ops.ri`  (include_str! compile error).
//! GREEN: after step-2 the test binary compiles and all assertions pass.

use reify_core::{Severity, Type, ValueCellId};
use reify_ir::{FieldSourceKind, Satisfaction, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load the combined differential-field-ops example.
///
/// Uses `include_str!` so the test binary carries the source at compile time
/// and is always in sync with the user-facing example file (single-source-of-truth
/// pattern, mirroring solve_elastic_static_e2e.rs ↔ fea_cantilever_smoke.ri).
fn diff_field_ops_source() -> &'static str {
    include_str!("../../../examples/differential_field_ops.ri")
}

/// Extract a named field from an ElasticResult value (StructureInstance or Map).
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract the `SampledField.data` vec from a named `Value::Field{Sampled}` in
/// an ElasticResult value.  Panics if the field is absent, not a Sampled field,
/// or the lambda is not `Value::SampledField`.
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

// ── integration gate ──────────────────────────────────────────────────────────

/// Full end-to-end integration gate for the differential-field-ops batch.
///
/// Asserts (in order):
///   (a) No Error-severity diagnostics — parse + compile + eval are clean.
///   (b) A ComputeNode with `target == "solver::elastic_static"` exists in the
///       snapshot graph (Phase-1 FEA path lowered via `@optimized`).
///   (c) All `.ri` `constraint` statements evaluate to `Satisfaction::Satisfied`
///       via `engine.check`.
///   (d) PHASE 1 — `result.divergence` is a `Value::Field{source:Sampled}`
///       with `codomain_type == Type::dimensionless_scalar()` and all-finite
///       data; the exact cross-field trace identity
///         div[k] = (1−2ν)/E · tr(σ)[k]
///       holds to rel-tol 1e-6 (proven-GREEN on the same cantilever fixture in
///       α/solve_elastic_static_e2e.rs:814-875; reused verbatim here).
///       Also asserts `DifferentialFieldOps.g_mag` is finite and > 0 (γ
///       magnitude signal, non-trivial under load) and < 1 (small-strain bound).
///   (e) PHASE 2 — `DifferentialFieldOps.lap_max` ≈ 2.0 within 1e-9
///       (max of laplacian(f) where f(x)=x²; exact on quadratics).
///       `DifferentialFieldOps.grad_max` ≈ 3.0 within 1e-9
///       (max of gradient(g) where g(x)=3x+2; exact on linears).
///       Proven by δ's laplacian_1d_quadratic_exact (1e-12 on 5-node grid).
#[test]
fn differential_field_ops_integration_gate() {
    let source = diff_field_ops_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (a) No Error-severity diagnostics ────────────────────────────────────
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

    // ── (b) ComputeNode with target == "solver::elastic_static" ──────────────
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::elastic_static");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"solver::elastic_static\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // ── (c) All constraint_results == Satisfied ───────────────────────────────
    // Re-evaluate via engine.check to obtain constraint satisfaction results.
    let check_result = engine.check(&compiled);
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }

    // ── (d) Phase 1 — divergence field contract + trace identity ─────────────

    let result_cell = ValueCellId::new("DifferentialFieldOps", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.result not found"));

    // Divergence must be Value::Field{source:Sampled, codomain:Real}
    let div_val = extract_field(result_val, "divergence")
        .unwrap_or_else(|| panic!("field 'divergence' not found in DifferentialFieldOps.result"));

    let (div_domain, div_codomain) = match &div_val {
        Value::Field { domain_type, codomain_type, source, .. } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "divergence source must be Sampled, got: {:?}",
                source
            );
            (domain_type.clone(), codomain_type.clone())
        }
        other => panic!(
            "DifferentialFieldOps.result.divergence must be Value::Field, got: {:?}",
            other
        ),
    };
    assert_eq!(
        div_domain,
        Type::point3(Type::length()),
        "divergence domain must be Point3<Length>"
    );
    assert_eq!(
        div_codomain,
        Type::dimensionless_scalar(),
        "divergence codomain must be Real (dimensionless_scalar)"
    );

    // All data finite
    let div_data = extract_sampled_field_data(result_val, "divergence");
    for (k, &d) in div_data.iter().enumerate() {
        assert!(d.is_finite(), "divergence data[{}] = {} is not finite", k, d);
    }

    let disp_data = extract_sampled_field_data(result_val, "displacement");
    let n_grid_nodes = disp_data.len() / 3;
    assert_eq!(
        div_data.len(),
        n_grid_nodes,
        "divergence data.len() = {} but displacement grid has {} nodes",
        div_data.len(),
        n_grid_nodes
    );

    // ── Exact cross-field trace identity: div[k] = (1−2ν)/E · tr(σ)[k] ──────
    //
    // Steel_AISI_1045: E = 205e9 Pa, ν = 0.29.
    // Both fields recovered and resampled with the same linear weights →
    // identity holds to floating-point accumulation (rel-tol 1e-6 generous).
    // Proven GREEN on the same cantilever fixture in solve_elastic_static_e2e.rs:814-875.
    // Reused verbatim here as the Phase-1 engineering-quantity signal (PRD §θ).
    let e_pa = 205e9_f64;
    let nu = 0.29_f64;
    let factor = (1.0 - 2.0 * nu) / e_pa;

    let stress_data = extract_sampled_field_data(result_val, "stress");
    assert_eq!(
        stress_data.len(),
        n_grid_nodes * 9,
        "stress data must have 9 components per grid node"
    );

    let mut max_div = 0.0_f64;
    let mut max_tr_sigma = 0.0_f64;
    for k in 0..n_grid_nodes {
        let tr_sigma = stress_data[9 * k] + stress_data[9 * k + 4] + stress_data[9 * k + 8];
        let expected_div = factor * tr_sigma;
        let got_div = div_data[k];
        let scale = expected_div.abs().max(1e-18);
        assert!(
            (got_div - expected_div).abs() < 1e-6 * scale,
            "trace identity violated at k={}: div={:e}, (1-2ν)/E·tr(σ)={:e}, rel-err={:e}",
            k, got_div, expected_div,
            (got_div - expected_div).abs() / scale,
        );
        if got_div.abs() > max_div { max_div = got_div.abs(); }
        if tr_sigma.abs() > max_tr_sigma { max_tr_sigma = tr_sigma.abs(); }
    }
    assert!(
        max_div > 1e-12,
        "max|div| = {:e} is effectively zero — no divergence signal",
        max_div
    );

    // ── g_mag = max(result.gradient): finite, >0 under load, <1 small-strain ─
    let g_mag_cell = ValueCellId::new("DifferentialFieldOps", "g_mag");
    let g_mag_val = eval_result
        .values
        .get(&g_mag_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.g_mag not found"));
    let g_mag = g_mag_val
        .as_f64()
        .unwrap_or_else(|| panic!("g_mag must be numeric, got: {:?}", g_mag_val));
    assert!(
        g_mag.is_finite() && g_mag > 0.0,
        "g_mag = max(result.gradient) must be finite and > 0 under non-zero load; got {}",
        g_mag
    );
    assert!(
        g_mag < 1.0,
        "g_mag = {} must be < 1.0 (small-strain engineering bound)",
        g_mag
    );

    // ── (e) Phase 2 — exact polynomial fixture assertions ────────────────────
    //
    // laplacian_1d_quadratic_exact (sampled_fd.rs) proves max(laplacian(x²)) = 2.0
    // to 1e-12 on a 5-node Regular1D grid with spacing 1.0; tolerance here is 1e-9.
    // gradient_1d_affine_exact proves the first-difference is exact on linears;
    // max(gradient(3x+2)) = 3.0 to 1e-12; tolerance here is 1e-9.

    let lap_max_cell = ValueCellId::new("DifferentialFieldOps", "lap_max");
    let lap_max_val = eval_result
        .values
        .get(&lap_max_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.lap_max not found"));
    let lap_max = lap_max_val
        .as_f64()
        .unwrap_or_else(|| panic!("lap_max must be numeric, got: {:?}", lap_max_val));
    assert!(
        (lap_max - 2.0).abs() < 1e-9,
        "lap_max = max(laplacian(quadratic)) = {} expected ≈ 2.0 (exact on quadratics, tol=1e-9)",
        lap_max
    );

    let grad_max_cell = ValueCellId::new("DifferentialFieldOps", "grad_max");
    let grad_max_val = eval_result
        .values
        .get(&grad_max_cell)
        .unwrap_or_else(|| panic!("cell DifferentialFieldOps.grad_max not found"));
    let grad_max = grad_max_val
        .as_f64()
        .unwrap_or_else(|| panic!("grad_max must be numeric, got: {:?}", grad_max_val));
    assert!(
        (grad_max - 3.0).abs() < 1e-9,
        "grad_max = max(gradient(linear)) = {} expected ≈ 3.0 (exact on linears, tol=1e-9)",
        grad_max
    );
}
