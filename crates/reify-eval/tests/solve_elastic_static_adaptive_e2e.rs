//! End-to-end integration tests for the task-4902 A2 adaptive-refinement loop
//! threaded through `fn solve_elastic_static` @optimized → ComputeNode →
//! trampoline (PRD `docs/prds/v0_4/a-posteriori-error-estimation.md` Task
//! decomposition #2, dependency task 2997/3000).
//!
//! # Scope — loop-threading + budget enforcement + convergence, NOT adaptivity
//!
//! These tests demonstrate that the `ConvergenceStatus` / budget-enforcement
//! loop is correctly THREADED end-to-end through the compiled `.ri` →
//! ComputeNode → trampoline pipeline, and that the loop correctly ENFORCES
//! its budget knobs (a tight `max_refinement_iterations: 0` terminates with
//! `NotConverged`). They do **NOT** demonstrate adaptivity: v1 uses
//! mesh-free UNIFORM grid-resolution refinement (RATIFIED esc-4902-83
//! option D) — `CantileverAdaptiveProblem::refine` ignores the Dörfler-marked
//! element set and doubles the synthetic grid resolution instead of
//! performing a real gmsh size-field remesh (deferred to follow-up task
//! 4909). See `crates/reify-eval/src/compute_targets/elastic_static.rs`
//! (`CantileverAdaptiveProblem`) for the full v1-scope rationale.

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract a named field from an ElasticResult value.
///
/// Mirrors `solve_elastic_static_e2e.rs`'s `extract_field` helper: handles
/// both `Value::StructureInstance(data)` (the standard shape) and
/// `Value::Map(m)` (temporary fallback documented in that file's plan
/// step-8).
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Evaluate `source` (a self-contained `.ri` structure named `structure_name`
/// binding a `result` cell), assert no Error-severity diagnostics, and return
/// the `result` cell's `Value`.
fn eval_result_cell(source: &str, structure_name: &str) -> Value {
    let compiled = parse_and_compile_with_stdlib(source);

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
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    let result_cell = ValueCellId::new(structure_name, "result");
    eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell {structure_name}.result not found in eval result"))
        .clone()
}

/// Unpack a `Value::Enum` into `(type_name, variant, payload)`, panicking
/// with a descriptive message if `v` is not an `Enum`.
fn expect_enum(v: &Value, context: &str) -> (String, String, Vec<(String, Value)>) {
    match v {
        Value::Enum { type_name, variant, payload } => {
            (type_name.clone(), variant.clone(), payload.clone())
        }
        other => panic!("{context}: expected Value::Enum, got {other:?}"),
    }
}

/// Unpack a `Value::Option` into `Option<Value>` (unboxed), panicking if `v`
/// is not an `Option`.
fn expect_option(v: &Value, context: &str) -> Option<Value> {
    match v {
        Value::Option(inner) => inner.as_deref().cloned(),
        other => panic!("{context}: expected Value::Option, got {other:?}"),
    }
}

/// Unpack a `Value::Real` into `f64`, panicking if `v` is not a `Real`.
fn expect_real(v: &Value, context: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        other => panic!("{context}: expected Value::Real, got {other:?}"),
    }
}

// ── fixtures ──────────────────────────────────────────────────────────────────
//
// All three cases share the standard cantilever fixture (Steel_AISI_1045,
// L=1000mm × W=100mm × H=100mm, 1000 N tip load, fixed root) — the same
// fixture used by `solve_elastic_static_e2e.rs`'s deterministic-option test.
// Only `ElasticOptions(...)` differs per case.

/// (a) ADAPTIVE CONVERGED: a loose `target_accuracy: 0.9` is met on the
/// FIRST solve (the coarse cantilever's measured η_global ≈ 0.295 — see
/// `CantileverAdaptiveProblem::solve_and_estimate`'s in-crate unit test,
/// which serves as the achievability basis — comfortably clears 0.9 with
/// margin, and 0.9 satisfies the DSL constraint `target_accuracy < 1`).
const CANTILEVER_ADAPTIVE_CONVERGED_SRC: &str = r#"
structure FeaCantileverAdaptiveConverged {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    let result = solve_elastic_static(
        material, length, width, height, [tip_load], [mount],
        ElasticOptions(adaptive: true, target_accuracy: 0.9)
    )
}
"#;

/// (b) ADAPTIVE BUDGET-CAPPED NOTCONVERGED: a tight `target_accuracy: 0.001`
/// (far below the coarse solve's η_global ≈ 0.295) combined with
/// `max_refinement_iterations: 0` forces the loop to terminate on its FIRST
/// solve without meeting the target — `run_adaptive_refinement` calls
/// `solve_and_estimate` before any budget check, so `MaxIterations` fires
/// with a populated (> 0.001) global error already recorded.
const CANTILEVER_ADAPTIVE_BUDGET_CAPPED_SRC: &str = r#"
structure FeaCantileverAdaptiveBudgetCapped {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    let result = solve_elastic_static(
        material, length, width, height, [tip_load], [mount],
        ElasticOptions(adaptive: true, target_accuracy: 0.001, max_refinement_iterations: 0)
    )
}
"#;

/// (c) NON-ADAPTIVE CONTROL: a bare `ElasticOptions()` (`adaptive` defaults
/// to `false`) must stay byte-identical to the pre-4902 trivial defaults.
const CANTILEVER_ADAPTIVE_CONTROL_SRC: &str = r#"
structure FeaCantileverAdaptiveControl {
    param length : Length = 1000mm
    param width  : Length = 100mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    let result = solve_elastic_static(
        material, length, width, height, [tip_load], [mount],
        ElasticOptions()
    )
}
"#;

// ── step-15: RED until step-16 wires the adaptive branch into the tet path ────

/// (a) ADAPTIVE CONVERGED.
///
/// RED: the trampoline does not yet call `extract_adaptive_params` /
/// `run_adaptive_refinement`, so `convergence_status` is still the trivial
/// non-adaptive `Converged { final_indicator: 0.0 }` default and
/// `global_relative_energy_error` is still `none` — both assertions below
/// fail until step-16.
#[test]
fn e2e_adaptive_converged_threads_real_status_and_global_error() {
    let result = eval_result_cell(CANTILEVER_ADAPTIVE_CONVERGED_SRC, "FeaCantileverAdaptiveConverged");

    let convergence_status = extract_field(&result, "convergence_status")
        .expect("ElasticResult must carry convergence_status");
    let (type_name, variant, payload) = expect_enum(&convergence_status, "convergence_status");
    assert_eq!(type_name, "ConvergenceStatus");
    assert_eq!(
        variant, "Converged",
        "target_accuracy=0.9 must converge on the first (coarse) solve"
    );
    assert_eq!(payload.len(), 1, "Converged payload must carry final_indicator");
    let final_indicator = expect_real(&payload[0].1, "convergence_status.Converged.final_indicator");
    assert!(
        final_indicator.is_finite() && (0.0..=0.9).contains(&final_indicator),
        "final_indicator must be finite and in [0, 0.9], got {final_indicator}"
    );

    let global_error_field = extract_field(&result, "global_relative_energy_error")
        .expect("ElasticResult must carry global_relative_energy_error");
    let global_error = expect_option(&global_error_field, "global_relative_energy_error")
        .unwrap_or_else(|| panic!("global_relative_energy_error must be Some on the adaptive path"));
    assert_eq!(
        expect_real(&global_error, "global_relative_energy_error"),
        final_indicator,
        "global_relative_energy_error must equal the Converged final_indicator"
    );

    let error_indicator_field =
        extract_field(&result, "error_indicator").expect("ElasticResult must carry error_indicator");
    assert_eq!(
        expect_option(&error_indicator_field, "error_indicator"),
        None,
        "error_indicator stays none in v1 (task 4910 deferral)"
    );
}

/// (b) ADAPTIVE BUDGET-CAPPED NOTCONVERGED.
///
/// RED: same underlying gap as (a) — the trampoline still emits the trivial
/// non-adaptive defaults regardless of `adaptive`/budget knobs.
#[test]
fn e2e_adaptive_budget_capped_reports_notconverged_max_iterations() {
    let result = eval_result_cell(
        CANTILEVER_ADAPTIVE_BUDGET_CAPPED_SRC,
        "FeaCantileverAdaptiveBudgetCapped",
    );

    let convergence_status = extract_field(&result, "convergence_status")
        .expect("ElasticResult must carry convergence_status");
    let (type_name, variant, payload) = expect_enum(&convergence_status, "convergence_status");
    assert_eq!(type_name, "ConvergenceStatus");
    assert_eq!(
        variant, "NotConverged",
        "target_accuracy=0.001 with max_refinement_iterations=0 must NOT converge on the \
         coarse first solve"
    );
    assert_eq!(payload.len(), 1, "NotConverged payload must carry reason");
    let (reason_type, reason_variant, reason_payload) =
        expect_enum(&payload[0].1, "convergence_status.NotConverged.reason");
    assert_eq!(reason_type, "BudgetReason");
    assert_eq!(
        reason_variant, "MaxIterations",
        "max_refinement_iterations=0 must terminate via the MaxIterations budget reason"
    );
    assert!(reason_payload.is_empty(), "BudgetReason is a bare enum (no payload)");

    let global_error_field = extract_field(&result, "global_relative_energy_error")
        .expect("ElasticResult must carry global_relative_energy_error");
    let global_error = expect_option(&global_error_field, "global_relative_energy_error")
        .unwrap_or_else(|| {
            panic!("global_relative_energy_error must be Some even on a budget-capped outcome")
        });
    let global_error = expect_real(&global_error, "global_relative_energy_error");
    assert!(
        global_error.is_finite() && global_error > 0.001,
        "global_relative_energy_error must be finite and > target_accuracy (0.001), got \
         {global_error}"
    );

    let error_indicator_field =
        extract_field(&result, "error_indicator").expect("ElasticResult must carry error_indicator");
    assert_eq!(
        expect_option(&error_indicator_field, "error_indicator"),
        None,
        "error_indicator stays none in v1 (task 4910 deferral)"
    );
}

/// (c) NON-ADAPTIVE CONTROL — must stay byte-identical to the pre-4902
/// trivial defaults (`aposteriori_nonadaptive_default_fields`).
///
/// This case is NOT expected to be RED before step-16 (the non-adaptive path
/// is untouched), but is included here so the three cases live together and
/// so a future regression in the non-adaptive path is caught by the same
/// file.
#[test]
fn e2e_non_adaptive_control_stays_trivial_default() {
    let result = eval_result_cell(CANTILEVER_ADAPTIVE_CONTROL_SRC, "FeaCantileverAdaptiveControl");

    let convergence_status = extract_field(&result, "convergence_status")
        .expect("ElasticResult must carry convergence_status");
    assert_eq!(
        convergence_status,
        Value::Enum {
            type_name: "ConvergenceStatus".to_string(),
            variant: "Converged".to_string(),
            payload: vec![("final_indicator".to_string(), Value::Real(0.0))],
        },
        "non-adaptive control must report the trivial Converged {{ final_indicator: 0.0 }}"
    );

    let error_indicator_field =
        extract_field(&result, "error_indicator").expect("ElasticResult must carry error_indicator");
    assert_eq!(expect_option(&error_indicator_field, "error_indicator"), None);

    let global_error_field = extract_field(&result, "global_relative_energy_error")
        .expect("ElasticResult must carry global_relative_energy_error");
    assert_eq!(
        expect_option(&global_error_field, "global_relative_energy_error"),
        None,
        "non-adaptive control must keep global_relative_energy_error == none"
    );
}
