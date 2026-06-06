//! End-to-end integration tests for `fn solve_elastic_static` @optimized →
//! ComputeNode → trampoline pipeline (PRD §8 task η,
//! docs/prds/v0_3/compute-node-contract.md).
//!
//! Steps:
//!   step-3/4  — API surface pin + module skeleton
//!   step-5/6  — ComputeNode-insertion assertion + smoke .ri
//!   step-7/8  — cantilever stress magnitude assertion + real FEA impl
//!   step-9/10 — cache-hit assertion + doc comments

use std::sync::atomic::{AtomicU32, Ordering};

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{FieldSourceKind, OpaqueState, SampledGridKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile the cantilever smoke fixture.
///
/// Uses `include_str!` so the test binary carries the source at compile time
/// and is always in sync with the user-facing example file. This is the
/// "single source of truth" design decision documented in the plan.
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

/// Extract `result.max_von_mises` from an ElasticResult value.
///
/// Handles both `Value::StructureInstance(data)` (preferred path after step-8)
/// and `Value::Map(m)` (temporary fallback documented in plan step-8).
/// Returns `None` if the value doesn't match either shape.
fn extract_max_von_mises(result: &Value) -> Option<Value> {
    match result {
        // PersistentMap::get takes &K (= &String), not &str — use owned key.
        Value::StructureInstance(data) => data.fields.get(&"max_von_mises".to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String("max_von_mises".to_string())).cloned(),
        _ => None,
    }
}

/// Extract a named field from an ElasticResult value.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        // PersistentMap::get takes &K (= &String), not &str — use owned key.
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract the `SampledField.data` vec from a named `Value::Field{Sampled}` in a result.
///
/// Panics if the field is absent, not `Value::Field{Sampled}`, or the lambda is not
/// `Value::SampledField`.  Used in step-9 determinism/cache guard assertions.
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

// ── step-3: RED — API surface pin ────────────────────────────────────────────
//
// Compile-time test: coerce
//   `reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline`
// to `ComputeFn` to pin the cross-crate signature. No runtime assertion —
// compile success is the signal. Expected to fail until step-4 creates the
// `compute_targets` module.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn =
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline;
}

/// Step-3: `register_compute_fns` installs the trampoline under the correct key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, asserts
/// `engine.compute_dispatch("solver::elastic_static").is_some()`.
///
/// Expected to fail until step-4 creates the `compute_targets` module.
#[test]
fn register_compute_fns_installs_solver_elastic_static() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("solver::elastic_static").is_some(),
        "register_compute_fns must install a trampoline under 'solver::elastic_static'"
    );
}

// ── step-5: RED — ComputeNode-insertion assertion ─────────────────────────────
//
// Mirrors the recipe at crates/reify-eval/tests/compute_dispatch_registry.rs:175-223.
// Three observable signals:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "solver::elastic_static" exists in the graph
//   (c) the result cell has a non-Undef value (StructureInstance or Map)
//
// Expected to fail (compile error) because examples/fea_cantilever_smoke.ri
// does not yet exist — step-6 creates it.

/// End-to-end smoke: cantilever .ri lowers to a ComputeNode (not body-inlined)
/// and the result cell is a non-Undef StructureInstance or Map.
#[test]
fn e2e_cantilever_smoke_lowers_to_compute_node() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
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

    // (b) A ComputeNode with target == "solver::elastic_static" must be in the graph
    //     (confirming @optimized lowering fired, not body-inlined).
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

    // (c) The result cell must hold a non-Undef value (StructureInstance or Map).
    //     Step-6 upgrades the skeleton trampoline to return a placeholder ElasticResult.
    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );
}

// ── step-7: RED — cantilever stress magnitude assertion ───────────────────────
//
// Analytical reference (Euler–Bernoulli, rectangular cross-section):
//   σ_max = 6 · P · L / (b · h²)
//         = 6 × 1000 × 1.0 / (0.1 × 0.01)
//         = 6 000 000 Pa  (6 MPa)
//
// Tolerance: ±50% — documented method-error budget for a coarse P1-tet mesh.
// P1 tets are stiffer than reality, so the FEA underestimates by 20–50%.
// Design decision 2 in the plan documents this threshold as the achievability
// basis, not a guessed tolerance.
//
// Expected to fail (assertion error) until step-8 implements the real FEA solve,
// because the placeholder trampoline returns max_von_mises = 0 Pa.

/// Cantilever max von Mises within ±50% of the analytical 6 MPa reference.
#[test]
fn e2e_cantilever_max_von_mises_within_tolerance() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // No Error diagnostics — clean solve required before asserting on values.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics before stress assertion, got: {:?}",
        errors
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval result"));

    // ── (a) max_von_mises ────────────────────────────────────────────────────
    //
    // Extract max_von_mises via the helper that handles both StructureInstance
    // and Map (the two ElasticResult shapes documented in plan step-8).
    let mvm = extract_max_von_mises(result_val).unwrap_or_else(|| {
        panic!(
            "could not extract max_von_mises from result: {:?}",
            result_val
        )
    });

    // The value must be Scalar with dimension == PRESSURE.
    let (si_value, dimension) = match &mvm {
        Value::Scalar {
            si_value,
            dimension,
        } => (*si_value, *dimension),
        other => panic!(
            "expected max_von_mises to be Value::Scalar {{ ... }}, got: {:?}",
            other
        ),
    };
    assert_eq!(
        dimension,
        DimensionVector::PRESSURE,
        "expected max_von_mises dimension == DimensionVector::PRESSURE, got: {:?}",
        dimension
    );

    // Analytical reference σ_max = 6PL/(bh²) = 6×1000×1.0/(0.1×0.01) = 6e6 Pa.
    // Tolerance: ±50% of analytical (3 MPa ≤ σ ≤ 9 MPa).
    let analytical_sigma: f64 = 6.0 * 1000.0 * 1.0 / (0.1 * 0.1 * 0.1); // 6e6 Pa
    let lo = analytical_sigma * 0.5; // 3e6 Pa  (P1 stiffness underestimate floor)
    let hi = analytical_sigma * 1.5; // 9e6 Pa  (stress concentration head-room)
    assert!(
        si_value.is_finite(),
        "max_von_mises must be finite, got: {}",
        si_value
    );
    assert!(
        si_value > 0.0,
        "max_von_mises must be positive, got: {}",
        si_value
    );
    assert!(
        si_value >= lo && si_value <= hi,
        "max_von_mises = {:.3e} Pa is outside ±50% of analytical {:.3e} Pa \
         (expected [{:.3e}, {:.3e}])",
        si_value,
        analytical_sigma,
        lo,
        hi
    );

    // ── (b) converged ────────────────────────────────────────────────────────
    let converged = extract_field(result_val, "converged").unwrap_or_else(|| {
        panic!(
            "could not extract 'converged' field from result: {:?}",
            result_val
        )
    });
    assert_eq!(
        converged,
        Value::Bool(true),
        "expected result.converged == Bool(true), got: {:?}",
        converged
    );

    // ── (c) iterations ───────────────────────────────────────────────────────
    let iterations = extract_field(result_val, "iterations").unwrap_or_else(|| {
        panic!(
            "could not extract 'iterations' field from result: {:?}",
            result_val
        )
    });
    match &iterations {
        Value::Int(n) => {
            assert!(*n >= 0, "expected iterations >= 0, got: {}", n);
        }
        other => panic!("expected iterations to be Value::Int, got: {:?}", other),
    }
}

// ── step-9: RED — cache-hit assertion ────────────────────────────────────────
//
// Verifies that the second eval() of the same compiled module does NOT
// re-dispatch the trampoline. The significance_filter opt-in
// (significance_filter.rs:76) plus the cache machinery should prevent
// re-dispatch when inputs haven't changed.
//
// `counting_wrapper` is a module-level fn (required by the ComputeFn type alias,
// which is a plain fn-pointer and cannot be a boxed closure). DISPATCH_COUNT is
// a module-level AtomicU32 shared across all invocations.
//
// Expected: DISPATCH_COUNT == 1 after two eval() calls.
// If the second eval() re-dispatches (DISPATCH_COUNT == 2), the test fails —
// that would expose either a missing significance-filter opt-in OR a
// cache-key non-determinism in the trampoline output (see step-10 for the fix).

/// Dispatch counter incremented by `counting_wrapper` on every trampoline call.
/// Module-level static so it is callable as a plain `ComputeFn` fn-pointer.
static DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper: increments `DISPATCH_COUNT` then calls through to
/// the production trampoline.  Installed via `engine.register_compute_fn`
/// (bypasses `register_compute_fns` to avoid the panic-on-double-registration).
fn counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Cache-hit: second eval() of the same compiled module must NOT re-dispatch.
///
/// Step-9 RED: asserts `DISPATCH_COUNT == 1` after two sequential `engine.eval()`
/// calls on the same `CompiledModule`.  Fails if the second eval re-dispatches
/// the trampoline (DISPATCH_COUNT would be 2).
#[test]
fn e2e_cantilever_second_eval_hits_cache() {
    // Reset counter for test isolation (guards against the test being re-run in
    // the same process without reinitialising the static).
    DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    // Register the counting wrapper directly (bypasses register_compute_fns so
    // the engine holds exactly one registration for "solver::elastic_static").
    engine.register_compute_fn("solver::elastic_static", counting_wrapper as ComputeFn);

    // ── First eval: trampoline must be dispatched once (cold start) ───────────
    let eval1 = engine.eval(&compiled);
    let errors1: Vec<_> = eval1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "first eval must have no Error diagnostics, got: {:?}",
        errors1
    );
    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch the trampoline exactly once"
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result1 = eval1
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("first eval: cell FeaCantileverSmoke.result not found"));

    // ── Second eval: cache hit — must NOT re-dispatch ─────────────────────────
    let eval2 = engine.eval(&compiled);
    let errors2: Vec<_> = eval2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "second eval must have no Error diagnostics, got: {:?}",
        errors2
    );

    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must hit the cache and NOT re-dispatch the trampoline \
         (DISPATCH_COUNT must stay at 1); if this fails, investigate \
         significance-filter opt-in or cache-key determinism — see step-10"
    );

    // ── Both evals must produce the same max_von_mises ────────────────────────
    let result2 = eval2
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("second eval: cell FeaCantileverSmoke.result not found"));
    let mvm1 = extract_max_von_mises(&result1)
        .unwrap_or_else(|| panic!("first eval: could not extract max_von_mises"));
    let mvm2 = extract_max_von_mises(&result2)
        .unwrap_or_else(|| panic!("second eval: could not extract max_von_mises"));
    assert_eq!(
        mvm1, mvm2,
        "both evals must produce bit-identical max_von_mises \
         (deterministic trampoline contract)"
    );

    // ── (step-9/α) Both evals: displacement SampledField.data must be bit-identical ──
    //
    // The §8-η Final-gate (engine_eval.rs) short-circuits re-dispatch on the
    // second eval, so result2 is the cached value from the first eval.
    // Bit-identical data confirms the trampoline is deterministic AND that the
    // populated Sampled fields do not break the cache-hit contract.
    let disp1_data = extract_sampled_field_data(&result1, "displacement");
    let disp2_data = extract_sampled_field_data(&result2, "displacement");
    assert_eq!(
        disp1_data.len(),
        disp2_data.len(),
        "displacement data length differs between eval1 ({}) and eval2 ({})",
        disp1_data.len(),
        disp2_data.len()
    );
    for (i, (v1, v2)) in disp1_data.iter().zip(disp2_data.iter()).enumerate() {
        assert_eq!(
            v1.to_bits(),
            v2.to_bits(),
            "displacement data[{}] not bit-identical across evals: {:e} vs {:e}",
            i,
            v1,
            v2
        );
    }

    // ── (step-9/α) Both evals: stress SampledField.data must be bit-identical ────
    let stress1_data = extract_sampled_field_data(&result1, "stress");
    let stress2_data = extract_sampled_field_data(&result2, "stress");
    assert_eq!(
        stress1_data.len(),
        stress2_data.len(),
        "stress data length differs between eval1 ({}) and eval2 ({})",
        stress1_data.len(),
        stress2_data.len()
    );
    for (i, (v1, v2)) in stress1_data.iter().zip(stress2_data.iter()).enumerate() {
        assert_eq!(
            v1.to_bits(),
            v2.to_bits(),
            "stress data[{}] not bit-identical across evals: {:e} vs {:e}",
            i,
            v1,
            v2
        );
    }
}

// ── step-5: RED — tet trampoline I-1/I-3 ─────────────────────────────────────
//
// Confirms that the tet-path trampoline result has:
//   (a) `shell_channels == Undef` (I-3 honest absence — no through-thickness
//       data for solid elements)
//   (b) `stress == Undef` (I-1 non-breaking — the flat stress field is
//       unchanged from pre-task baseline)
//
// RED until step-6 adds ("shell_channels", Value::Undef) to the fields map
// in `elastic_static.rs:195-207`.  Before that, `extract_field(result,
// "shell_channels")` returns `None` (key absent), not `Some(Undef)`.

/// I-3 (tet): `result.shell_channels` must be `Value::Undef` — honest
/// absence signal for solid elements (no through-thickness data).
///
/// RED until step-6 emits the `shell_channels = Undef` key.
#[test]
fn tet_trampoline_shell_channels_is_undef() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found"));

    assert_eq!(
        extract_field(result_val, "shell_channels"),
        Some(Value::Undef),
        "tet result.shell_channels must be Undef (I-3 honest absence); \
         got: {:?} — step-6 adds the shell_channels=Undef key to the trampoline",
        extract_field(result_val, "shell_channels")
    );
}

// ── step-5/α: RED — B1/B2: displacement+stress as Regular3D Sampled Fields ───
//
// B1: result.displacement and result.stress are now Value::Field{Sampled,Regular3D},
//     NOT Value::Undef.  tet_trampoline_stress_is_undef (added by sibling #4067/δ)
//     is intentionally removed here because α's purpose is precisely to populate
//     those fields.
//
// NOTE: tet_trampoline_stress_is_undef was removed (its Undef-premise is voided
// by α).  tet_trampoline_shell_channels_is_undef is KEPT (shell_channels stays
// Undef under α — only displacement and stress are populated).

/// B1/B2 — displacement and stress are populated Sampled Regular3D Fields.
///
/// B1 assertions (shape/metadata):
/// - `displacement` is `Value::Field { source: Sampled, kind: Regular3D,
///   domain: Point3<Length>, codomain: Vector3<Length> }` with
///   `data.len() == grid_count × 3`; ALL samples finite.
/// - `stress` is `Value::Field { source: Sampled, kind: Regular3D,
///   domain: Point3<Length>, codomain: Tensor<2,3,Pressure> }` with
///   `data.len() == grid_count × 9`; ALL samples finite.
/// - Both fields share identical SampledField grid metadata.
/// - `frame == Undef`; `shell_channels == Undef`.
///
/// B2 assertions (von Mises consistency ratio):
/// - `field_max_vm ≤ max_von_mises × (1+1e-6)` (provable upper bound —
///   Kronecker-δ coincidence + convexity of vM + convex nodal averaging).
/// - `field_max_vm ≥ 0.5 × max_von_mises` (recovery-quality floor).
///
/// RED until step-6 populates displacement/stress in the trampoline.
#[test]
fn e2e_cantilever_b1_b2_displacement_stress_fields() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No Error diagnostics.
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

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found"));

    // ── B1: displacement ──────────────────────────────────────────────────────
    let disp_val = extract_field(result_val, "displacement")
        .unwrap_or_else(|| panic!("displacement field missing from result"));

    let (disp_domain, disp_codomain, disp_sf) = match &disp_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "displacement source must be Sampled, got: {:?}",
                source
            );
            let sf = match lambda.as_ref() {
                Value::SampledField(sf) => sf.clone(),
                other => panic!("displacement lambda must be SampledField, got: {:?}", other),
            };
            (domain_type.clone(), codomain_type.clone(), sf)
        }
        other => panic!("expected displacement to be Value::Field, got: {:?}", other),
    };
    assert_eq!(
        disp_sf.kind,
        SampledGridKind::Regular3D,
        "displacement SampledField.kind must be Regular3D"
    );
    assert_eq!(
        disp_domain,
        Type::point3(Type::length()),
        "displacement domain_type mismatch"
    );
    assert_eq!(
        disp_codomain,
        Type::vec3(Type::length()),
        "displacement codomain_type mismatch"
    );

    // ── B1: stress ────────────────────────────────────────────────────────────
    let stress_val = extract_field(result_val, "stress")
        .unwrap_or_else(|| panic!("stress field missing from result"));

    let (stress_codomain, stress_sf) = match &stress_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => {
            assert!(
                matches!(source, FieldSourceKind::Sampled),
                "stress source must be Sampled, got: {:?}",
                source
            );
            assert_eq!(
                *domain_type,
                Type::point3(Type::length()),
                "stress domain_type mismatch"
            );
            let sf = match lambda.as_ref() {
                Value::SampledField(sf) => sf.clone(),
                other => panic!("stress lambda must be SampledField, got: {:?}", other),
            };
            (codomain_type.clone(), sf)
        }
        other => panic!("expected stress to be Value::Field, got: {:?}", other),
    };
    assert_eq!(
        stress_sf.kind,
        SampledGridKind::Regular3D,
        "stress SampledField.kind must be Regular3D"
    );
    assert_eq!(
        stress_codomain,
        Type::tensor(
            2,
            3,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE
            }
        ),
        "stress codomain_type mismatch"
    );

    // ── B1: grid counts ───────────────────────────────────────────────────────
    // Cantilever fixture: length=1.0m, height=0.1m, nz=6
    //   nx = round(1.0/0.1 × 6) = round(60) = 60
    //   ny = 1
    //   grid_count = (nx+1)×(ny+1)×(nz+1) = 61×2×7 = 854
    let nz: usize = 6;
    let ny: usize = 1;
    let nx: usize = ((1.0_f64 / 0.1_f64 * nz as f64).round() as usize).max(1);
    let grid_count = (nx + 1) * (ny + 1) * (nz + 1);

    assert_eq!(
        disp_sf.data.len(),
        grid_count * 3,
        "displacement data.len() must be grid_count({})×3={}, got {}",
        grid_count,
        grid_count * 3,
        disp_sf.data.len()
    );
    assert_eq!(
        stress_sf.data.len(),
        grid_count * 9,
        "stress data.len() must be grid_count({})×9={}, got {}",
        grid_count,
        grid_count * 9,
        stress_sf.data.len()
    );

    // ── B1: disp + stress share identical grid metadata ───────────────────────
    assert_eq!(
        disp_sf.bounds_min, stress_sf.bounds_min,
        "grid bounds_min mismatch between disp and stress"
    );
    assert_eq!(
        disp_sf.bounds_max, stress_sf.bounds_max,
        "grid bounds_max mismatch between disp and stress"
    );
    assert_eq!(
        disp_sf.spacing, stress_sf.spacing,
        "grid spacing mismatch between disp and stress"
    );
    assert_eq!(
        disp_sf.axis_grids.len(),
        stress_sf.axis_grids.len(),
        "axis_grids count mismatch between disp and stress"
    );
    for (i, (ag_d, ag_s)) in disp_sf
        .axis_grids
        .iter()
        .zip(stress_sf.axis_grids.iter())
        .enumerate()
    {
        assert_eq!(
            ag_d, ag_s,
            "axis_grids[{}] mismatch between disp and stress",
            i
        );
    }

    // ── B1: ALL grid samples finite (prismatic box → every point inside solid) ──
    assert!(
        disp_sf.data.iter().all(|v| v.is_finite()),
        "displacement field has non-finite values; first non-finite index: {:?}",
        disp_sf.data.iter().position(|v| !v.is_finite())
    );
    assert!(
        stress_sf.data.iter().all(|v| v.is_finite()),
        "stress field has non-finite values; first non-finite index: {:?}",
        stress_sf.data.iter().position(|v| !v.is_finite())
    );

    // ── B1: frame and shell_channels remain Undef ─────────────────────────────
    assert_eq!(
        extract_field(result_val, "frame"),
        Some(Value::Undef),
        "result.frame must remain Undef (tet/solid: no per-element local frame)"
    );
    assert_eq!(
        extract_field(result_val, "shell_channels"),
        Some(Value::Undef),
        "result.shell_channels must remain Undef (solid elements have no through-thickness data)"
    );

    // ── B2: field-max von Mises consistency ratio ─────────────────────────────
    // Inline the standard vM formula (compute_von_mises_3x3 is pub(crate) in
    // reify-stdlib, unreachable from this test crate).
    // Layout: row-major [s00,s01,s02, s10,s11,s12, s20,s21,s22].
    // σ_VM = sqrt(((s00-s11)²+(s11-s22)²+(s22-s00)²)/2 + 3·(s01²+s12²+s02²))
    let mut field_max_vm: f64 = 0.0;
    for chunk in stress_sf.data.chunks_exact(9) {
        // Skip any window containing NaN (out-of-solid sentinel).
        if chunk.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let (s00, s11, s22) = (chunk[0], chunk[4], chunk[8]);
        let (s01, s12, s02) = (chunk[1], chunk[5], chunk[2]);
        let vm = f64::sqrt(
            0.5 * ((s00 - s11).powi(2) + (s11 - s22).powi(2) + (s22 - s00).powi(2))
                + 3.0 * (s01.powi(2) + s12.powi(2) + s02.powi(2)),
        );
        if vm > field_max_vm {
            field_max_vm = vm;
        }
    }

    let max_von_mises = match extract_max_von_mises(result_val) {
        Some(Value::Scalar { si_value, .. }) => si_value,
        other => panic!("expected Scalar max_von_mises, got: {:?}", other),
    };

    assert!(
        field_max_vm.is_finite() && field_max_vm > 0.0,
        "field-max von Mises must be positive finite, got {:.3e}",
        field_max_vm
    );
    // Upper bound: provable via Kronecker-δ + convexity of vM + convex nodal avg.
    assert!(
        field_max_vm <= max_von_mises * (1.0 + 1e-6),
        "field-max vM {:.3e} Pa exceeds element-max {:.3e} Pa × (1+1e-6) — upper bound violated",
        field_max_vm,
        max_von_mises
    );
    // Lower bound: recovery-quality floor (0.5× is conservative; peak node ≳0.7× in practice).
    assert!(
        field_max_vm >= 0.5 * max_von_mises,
        "field-max vM {:.3e} Pa < 0.5 × element-max {:.3e} Pa — recovery quality floor violated",
        field_max_vm,
        max_von_mises
    );
}

// ── step-11: deterministic ElasticOptions accepted end-to-end (task 2926) ──────
//
// Compiles + evals an inline `.ri` calling `solve_elastic_static(...)` with
// `ElasticOptions(deterministic: true)` on the L=1m, b=h=0.1m, P=1000N cantilever
// (same material/geometry as the smoke fixture). Proves the new `deterministic`
// ElasticOptions field flows through compile → ComputeNode → trampoline cleanly.
//
// Achievability basis: the cantilever mesh (nz=6, nx=60 ⇒ ~854 nodes ⇒ 2562
// DOFs < PARALLEL_DOF_THRESHOLD=10_000) resolves to Deterministic even with
// deterministic:true, so the σ_max band is the SAME validated computation as
// `e2e_cantilever_max_von_mises_within_tolerance`.
//
// On a fresh checkout (no `deterministic` field on ElasticOptions) compiling the
// `.ri` would emit an unknown-ctor-param Error; GREENed by steps 4 + 10.
//
// Scope note: this test covers the *plumbing* (field accepted → ComputeNode →
// trampoline) on a sub-threshold mesh, where deterministic:true resolves to the
// same Deterministic modes as the default. The *behavioral* guarantee — that the
// resolver drives a bit-stable deterministic solve and a tolerance-equivalent
// parallel solve for a >PARALLEL_DOF_THRESHOLD problem — is verified at the
// solver layer by
// `reify_solver_elastic::solver::tests::resolve_execution_modes_drives_bit_stable_and_equivalent_solves`
// (a fast small-system test rather than a slow large-mesh e2e).

/// Inline cantilever source opting into deterministic execution via
/// `ElasticOptions(deterministic: true)`. Mirrors the smoke fixture's
/// material/geometry/loads/supports plus the ConstitutiveLawInput / LoadCase
/// coercion workarounds (see `examples/fea_cantilever_smoke.ri`).
const CANTILEVER_DETERMINISTIC_SRC: &str = r#"
structure FeaCantileverDeterministic {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")
    let lc = LoadCase(name: "cantilever", loads: [tip_load], supports: [mount])
    let ci = ConstitutiveLawInput(law: material)

    let result = solve_elastic_static(
        ci.law, length, width, height, lc.loads, lc.supports,
        ElasticOptions(deterministic: true)
    )
}
"#;

/// Cantilever solved with `ElasticOptions(deterministic: true)`: clean compile,
/// ComputeNode lowering, converged solve, and σ_max within ±50% of 6 MPa.
#[test]
fn e2e_cantilever_deterministic_option_within_tolerance() {
    let compiled = parse_and_compile_with_stdlib(CANTILEVER_DETERMINISTIC_SRC);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (a) No Error-severity diagnostics — proves `deterministic:` is an
    //     accepted ElasticOptions field end-to-end (compile + eval). ───────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics with ElasticOptions(deterministic: true), got: {:?}",
        errors
    );

    // ── (b) A ComputeNode with target == "solver::elastic_static" must exist. ──
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
        "expected a ComputeNode with target==\"solver::elastic_static\" in the graph"
    );

    // ── (c) converged == true AND max_von_mises ∈ [3e6, 9e6] Pa. ───────────────
    //
    // deterministic:true keeps the same Deterministic solve (mesh < 10K DOFs),
    // so the validated ±50%-of-6 MPa band still holds.
    let result_cell = ValueCellId::new("FeaCantileverDeterministic", "result");
    let result_val = eval_result.values.get(&result_cell).unwrap_or_else(|| {
        panic!("cell FeaCantileverDeterministic.result not found in eval result")
    });

    let converged = extract_field(result_val, "converged")
        .unwrap_or_else(|| panic!("could not extract 'converged' from result: {:?}", result_val));
    assert_eq!(
        converged,
        Value::Bool(true),
        "expected result.converged == Bool(true) under deterministic solve, got: {:?}",
        converged
    );

    let mvm = extract_max_von_mises(result_val).unwrap_or_else(|| {
        panic!("could not extract max_von_mises from result: {:?}", result_val)
    });
    let si_value = match &mvm {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "expected max_von_mises dimension == PRESSURE, got: {:?}",
                dimension
            );
            *si_value
        }
        other => panic!("expected max_von_mises to be Value::Scalar, got: {:?}", other),
    };

    // Analytical σ_max = 6PL/(bh²) = 6e6 Pa; ±50% band = [3e6, 9e6].
    let analytical_sigma: f64 = 6.0 * 1000.0 * 1.0 / (0.1 * 0.1 * 0.1);
    let lo = analytical_sigma * 0.5;
    let hi = analytical_sigma * 1.5;
    assert!(
        si_value.is_finite() && si_value > 0.0,
        "max_von_mises must be positive finite, got: {}",
        si_value
    );
    assert!(
        si_value >= lo && si_value <= hi,
        "max_von_mises = {:.3e} Pa outside ±50% of analytical {:.3e} Pa (expected [{:.3e}, {:.3e}])",
        si_value,
        analytical_sigma,
        lo,
        hi
    );
}
