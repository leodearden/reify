#![allow(clippy::mutable_key_type)]
//! Integration tests for task 4088: `solve_load_cases` @optimized →
//! `"solver::multi_case"` ComputeNode lowering + compute-result-reuse
//! verification.
//!
//! Two observable signals from the §8-η plan contract:
//!
//! - **(a) TWO-CASE SOLVE → POPULATED MultiCaseResult** — after @optimized
//!   lowering fires, each case in the returned `Value::Map{"cases"→Map}` is a
//!   real `Value::StructureInstance("ElasticResult")` with populated
//!   `displacement` (Sampled Regular3D Field, codomain vec3<Length>,
//!   all-finite) and `stress` (codomain Tensor<2,3,Pressure>) fields; a
//!   ComputeNode with target `"solver::multi_case"` appears in the graph; and
//!   per-case independence holds (2× tip load ⇒ 2× stress, i.e.
//!   `overload.max_von_mises > operating.max_von_mises`).
//!
//! - **(b) SECOND EVAL REUSES COMPUTE RESULT** — a counting wrapper registered
//!   for `"solver::multi_case"` is dispatched exactly once across two
//!   `engine.eval()` calls; the §8-η Final-gate (engine_eval.rs) short-circuits
//!   the 2nd eval.  Explicitly does NOT assert `realization_entries` (re-homed
//!   to task 4152, esc-4088-231 decision A).
//!
//! Both tests are **RED** until step-2 adds:
//!   1. `@optimized("solver::multi_case")` to `solve_load_cases` in
//!      `fea_multi_case.ri`;
//!   2. `crates/reify-eval/src/compute_targets/multi_case.rs` with
//!      `pub fn solve_multi_case_trampoline(...)`;
//!   3. Registration in `compute_targets::register_compute_fns`.
//!
//! Mirrors the scaffold from `solve_elastic_static_e2e.rs` and
//! `modal_compute_node.rs`.

use std::sync::atomic::{AtomicU32, Ordering};

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_eval::graph::CancellationHandle;
use reify_eval::{ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{FieldSourceKind, OpaqueState, SampledGridKind, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Fixture source ────────────────────────────────────────────────────────────
//
// Two LoadCases differing only in tip force (1000 N vs 2000 N).  The 2× ratio
// is the per-case independence assertion: linear elasticity ⇒ 2× load ⇒ 2×
// stress, so overload.max_von_mises > operating.max_von_mises must hold.
//
// Dimensions match the cantilever smoke fixture so the same grid-count formula
// applies: length=1000mm, width=100mm, height=100mm.

const TWO_CASE_SOURCE: &str = r#"
structure def MultiCaseSolveFixture {
    let ci        = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc1       = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let lc2       = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "tip", force: 2000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1, lc2], ElasticOptions())
}
"#;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract a named field from a `Value::StructureInstance` or `Value::Map`.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract `max_von_mises` as `f64` (SI value) from an ElasticResult value.
/// Panics if the field is absent or not a Scalar.
fn extract_max_von_mises_f64(result: &Value, case_name: &str) -> f64 {
    let mvm = extract_field(result, "max_von_mises").unwrap_or_else(|| {
        panic!("case \"{case_name}\": max_von_mises field missing from ElasticResult")
    });
    match mvm {
        Value::Scalar { si_value, .. } => si_value,
        other => {
            panic!("case \"{case_name}\": max_von_mises must be Value::Scalar, got: {other:?}")
        }
    }
}

// ── Grid-count formula (matches fea_cantilever_smoke.ri / elastic_static.rs) ─
//
// For a prismatic cantilever: nz=6, ny=1, nx=round(length/height × nz).max(1).
// For the fixture: length=1.0m, height=0.1m ⇒ nx=60, ny=1, nz=6.
// grid_count = (nx+1)×(ny+1)×(nz+1) = 61×2×7 = 854.
const NZ: usize = 6;
const NY: usize = 1;

fn grid_count_for_fixture() -> usize {
    let nx = ((1.0_f64 / 0.1_f64 * NZ as f64).round() as usize).max(1);
    (nx + 1) * (NY + 1) * (NZ + 1)
}

// ─────────────────────────────────────────────────────────────────────────────
// (a) TWO-CASE SOLVE → POPULATED MultiCaseResult
// ─────────────────────────────────────────────────────────────────────────────

/// End-to-end: `solve_load_cases` with 2 cases lowers to a
/// `"solver::multi_case"` ComputeNode and returns a `MultiCaseResult`-shaped
/// `Value::Map` where each per-case is a fully-populated
/// `Value::StructureInstance("ElasticResult")` with real Sampled-Field
/// `displacement` and `stress`.
///
/// # RED until step-2
///
/// Before step-2:
///   - `solve_load_cases` has no `@optimized` annotation → no ComputeNode with
///     `"solver::multi_case"` is created (the graph-node assertion fails).
///   - The per-case values come from `eval_solve_load_cases` via
///     `invoke_solve_elastic_static` (contract-body evaluation), which returns
///     an empty `Value::StructureInstance("ElasticResult")` stub without
///     `displacement` / `stress` fields (those field-presence assertions fail).
///   - Additionally, `reify_eval::compute_targets::multi_case` does not exist,
///     so this file fails to **compile** until step-2 creates the module.
#[test]
fn two_case_solve_returns_populated_multi_case_result() {
    let compiled = parse_and_compile_with_stdlib(TWO_CASE_SOURCE);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (1) No Error-severity diagnostics ────────────────────────────────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );

    // ── (2) A ComputeNode with target "solver::multi_case" must exist ─────────
    //
    // Confirms @optimized lowering fired (engine_eval.rs §8-η path), NOT
    // body-inline / reify-expr interceptor fallback.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_multi_case_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::multi_case");
    assert!(
        has_multi_case_node,
        "expected a ComputeNode with target==\"solver::multi_case\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // ── (3) Result is Value::Map{"cases" -> Map} with exactly 2 entries ───────
    let result_cell = ValueCellId::new("MultiCaseSolveFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell MultiCaseSolveFixture.result not found in eval result"));

    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_load_cases result must be Value::Map (not {:?})",
            std::mem::discriminant(other)
        ),
    };
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries; got {} entries: {:?}",
        cases_map.len(),
        cases_map.keys().collect::<Vec<_>>()
    );

    // ── (4) Per-case shape: StructureInstance("ElasticResult") ───────────────
    for case_name in ["operating", "overload"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "cases map must contain \"{}\" key; got: {:?}",
                    case_name,
                    cases_map.keys().collect::<Vec<_>>()
                )
            });

        match case_val {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "ElasticResult",
                    "case \"{case_name}\" must be StructureInstance(\"ElasticResult\"), \
                     got type_name = \"{}\"",
                    data.type_name
                );
            }
            other => panic!(
                "case \"{case_name}\" must be Value::StructureInstance(\"ElasticResult\"), \
                 got: {other:?}"
            ),
        }

        let grid_count = grid_count_for_fixture();

        // ── (4a) displacement: Sampled Regular3D, domain point3<Length>,
        //         codomain vec3<Length>, all-finite, len == grid_count*3 ─────
        let disp_val = extract_field(case_val, "displacement").unwrap_or_else(|| {
            panic!("case \"{case_name}\": displacement field missing from ElasticResult")
        });
        let (disp_domain, disp_codomain, disp_sf) = match &disp_val {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda,
            } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "case \"{case_name}\": displacement source must be Sampled, got: {source:?}"
                );
                let sf = match lambda.as_ref() {
                    Value::SampledField(sf) => sf.clone(),
                    other => panic!(
                        "case \"{case_name}\": displacement lambda must be SampledField, \
                         got: {other:?}"
                    ),
                };
                (domain_type.clone(), codomain_type.clone(), sf)
            }
            other => panic!(
                "case \"{case_name}\": expected displacement to be Value::Field, got: {other:?}"
            ),
        };
        assert_eq!(
            disp_sf.kind,
            SampledGridKind::Regular3D,
            "case \"{case_name}\": displacement SampledField.kind must be Regular3D"
        );
        assert_eq!(
            disp_domain,
            Type::point3(Type::length()),
            "case \"{case_name}\": displacement domain_type mismatch"
        );
        assert_eq!(
            disp_codomain,
            Type::vec3(Type::length()),
            "case \"{case_name}\": displacement codomain_type mismatch"
        );
        assert_eq!(
            disp_sf.data.len(),
            grid_count * 3,
            "case \"{case_name}\": displacement data.len() must be grid_count({grid_count})*3={}, \
             got {}",
            grid_count * 3,
            disp_sf.data.len()
        );
        assert!(
            disp_sf.data.iter().all(|v| v.is_finite()),
            "case \"{case_name}\": displacement field has non-finite values; \
             first non-finite index: {:?}",
            disp_sf.data.iter().position(|v| !v.is_finite())
        );

        // ── (4b) stress: Sampled Regular3D, codomain Tensor<2,3,Pressure>,
        //         len == grid_count*9, finite where inside solid ─────────────
        let stress_val = extract_field(case_val, "stress").unwrap_or_else(|| {
            panic!("case \"{case_name}\": stress field missing from ElasticResult")
        });
        let (stress_domain, stress_codomain, stress_sf) = match &stress_val {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda,
            } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "case \"{case_name}\": stress source must be Sampled, got: {source:?}"
                );
                let sf = match lambda.as_ref() {
                    Value::SampledField(sf) => sf.clone(),
                    other => panic!(
                        "case \"{case_name}\": stress lambda must be SampledField, \
                         got: {other:?}"
                    ),
                };
                (domain_type.clone(), codomain_type.clone(), sf)
            }
            other => {
                panic!("case \"{case_name}\": expected stress to be Value::Field, got: {other:?}")
            }
        };
        assert_eq!(
            stress_sf.kind,
            SampledGridKind::Regular3D,
            "case \"{case_name}\": stress SampledField.kind must be Regular3D"
        );
        assert_eq!(
            stress_domain,
            Type::point3(Type::length()),
            "case \"{case_name}\": stress domain_type mismatch"
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
            "case \"{case_name}\": stress codomain_type mismatch"
        );
        assert_eq!(
            stress_sf.data.len(),
            grid_count * 9,
            "case \"{case_name}\": stress data.len() must be grid_count({grid_count})*9={}, \
             got {}",
            grid_count * 9,
            stress_sf.data.len()
        );
        // Prismatic box — all grid points lie inside the solid, so all finite.
        assert!(
            stress_sf.data.iter().all(|v| v.is_finite()),
            "case \"{case_name}\": stress field has non-finite values (prismatic box: \
             all grid points are inside the solid); \
             first non-finite index: {:?}",
            stress_sf.data.iter().position(|v| !v.is_finite())
        );
    }

    // ── (5) Per-case independence: overload.max_von_mises > operating.max_von_mises
    //
    // Linear elasticity identity: 2× tip load ⇒ 2× stress throughout the
    // body.  The "overload" case (2000 N) must therefore produce strictly
    // higher peak von-Mises stress than the "operating" case (1000 N).
    let op_val = cases_map
        .get(&Value::String("operating".to_string()))
        .expect("cases map must contain \"operating\"");
    let ov_val = cases_map
        .get(&Value::String("overload".to_string()))
        .expect("cases map must contain \"overload\"");

    let op_mvm = extract_max_von_mises_f64(op_val, "operating");
    let ov_mvm = extract_max_von_mises_f64(ov_val, "overload");

    assert!(
        op_mvm > 0.0,
        "operating.max_von_mises must be positive (real FEA stress), got {op_mvm}"
    );
    assert!(
        ov_mvm > op_mvm,
        "overload.max_von_mises ({ov_mvm:.3e} Pa) must be strictly greater than \
         operating.max_von_mises ({op_mvm:.3e} Pa) — \
         linear elasticity: 2× tip load ⇒ 2× stress"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (b) SECOND EVAL REUSES COMPUTE RESULT (§8-η Final-gate)
// ─────────────────────────────────────────────────────────────────────────────
//
// A counting wrapper delegates to `solve_multi_case_trampoline` and increments
// `MC_DISPATCH_COUNT` on every call.  Two sequential `engine.eval()` calls on
// the same `CompiledModule` must dispatch the trampoline exactly once — the
// §8-η Final-gate (engine_eval.rs:3284-3324) short-circuits the 2nd eval.
//
// NOTE: `realization_entries` is NOT asserted here.  The false-premise that
// `CacheStats.realization_entries` exists has been confirmed; that RED is
// re-homed to task 4152 (esc-4088-231 decision A).  The DISPATCH_COUNT==1
// assertion is the proven, directly-observable compute-result reuse signal.

/// Dispatch counter incremented by `mc_counting_wrapper` on every
/// multi_case trampoline call.  Module-level static so it is callable as a
/// plain `ComputeFn` fn-pointer.
static MC_DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper: increments `MC_DISPATCH_COUNT` then delegates to the
/// production `solve_multi_case_trampoline`.
///
/// Registered for `"solver::multi_case"` on a fresh engine (bypasses
/// `register_compute_fns` to avoid the panic-on-double-registration).
fn mc_counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    MC_DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::multi_case::solve_multi_case_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Cache-reuse: second `engine.eval()` on the same module must NOT
/// re-dispatch the `"solver::multi_case"` trampoline — the §8-η Final-gate
/// serves the cached result.
///
/// # RED until step-2
///
/// Before step-2:
///   - `reify_eval::compute_targets::multi_case` does not exist → compile error.
///   - Even if the module existed, `solve_load_cases` has no `@optimized`
///     annotation → no dispatch ever fires → `MC_DISPATCH_COUNT` stays 0,
///     failing the `== 1` assertion.
#[test]
fn multi_case_second_eval_reuses_compute_result() {
    // Reset for test isolation.
    MC_DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let compiled = parse_and_compile_with_stdlib(TWO_CASE_SOURCE);

    let mut engine = make_simple_engine();
    // Register only what TWO_CASE_SOURCE actually exercises:
    //   - "solver::elastic_static"  for the per-case sub-solves
    //   - "solver::multi_case"      as the counting wrapper
    // Registering only these two keeps the test minimal and intention-revealing;
    // it does not drift with register_compute_fns internals and avoids
    // double-registration panics.
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline as ComputeFn,
    );
    engine.register_compute_fn("solver::multi_case", mc_counting_wrapper as ComputeFn);

    // ── First eval: trampoline dispatched once (cold start) ───────────────────
    let eval1 = engine.eval(&compiled);
    let errors1: Vec<_> = eval1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "first eval must have no Error diagnostics, got: {errors1:?}"
    );
    assert_eq!(
        MC_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch the multi_case trampoline exactly once"
    );

    // ── Second eval: Final-gate hit — must NOT re-dispatch ───────────────────
    let eval2 = engine.eval(&compiled);
    let errors2: Vec<_> = eval2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "second eval must have no Error diagnostics, got: {errors2:?}"
    );

    // NOTE: realization_entries is NOT asserted here.
    // CacheStats has no such field (false premise, esc-4088-231 decision A);
    // that RED is re-homed to task 4152.  DISPATCH_COUNT==1 is the
    // directly-observable compute-result reuse signal.
    assert_eq!(
        MC_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must reuse the cached ComputeNode result (§8-η Final-gate) \
         and NOT re-dispatch the multi_case trampoline; \
         if this fails, check the Final-gate path in engine_eval.rs:3284-3324"
    );
}
