//! An `@optimized` call inlined in a `constraint` reaches a definite verdict on
//! the engine's check surfaces (task 10033, INV-SF-4).
//!
//! The flagship example's yield constraint (`examples/fea_bracket_minimize_mass.ri`)
//! inlines `solve_elastic_static(..)` in its predicate. The solver's cost loop
//! dispatches that call to the compute registry (#4880), but the post-solve
//! checker body-evaluated it to `Undef`, so the verdict stayed `Indeterminate`.
//! A synthetic O(1) trampoline stands in for the FEA solve here: the wiring under
//! test is target-agnostic, and only the synthetic form is cheap enough for the gate.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::ConstraintNodeId;
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, ConstraintCheckEntry, Engine,
    RealizationReadHandle, SolverPosture,
};
use reify_ir::{OpaqueState, Satisfaction, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib};

const SOURCE: &str = r#"
structure def CheckProbeReading {
    param level : Length
}

// The RED mechanism: `CheckProbeReading()` leaves its no-default `level` Undef,
// so without a compute dispatch the call body-evaluates to Undef. DO NOT replace
// this body with a real expression (e.g. `span * 0.5`): the unhooked checker
// would then decide the constraints and the test would pass vacuously.
@optimized("test::check_half_span")
fn check_half_span(span : Length) -> Length {
    CheckProbeReading().level
}

structure CheckDispatchProbe {
    param span : Length = 50mm
    constraint check_half_span(span) > 10mm
    constraint check_half_span(span) > 30mm
}
"#;

/// `span / 2`, standing in for `solve_elastic_static`.
fn check_half_span_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let result = match value_inputs.first() {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => Value::Scalar {
            si_value: si_value * 0.5,
            dimension: *dimension,
        },
        _ => Value::Undef,
    };
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
        structured_detail: Vec::new(),
    }
}

fn compile_probe() -> CompiledModule {
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile cleanly: {errors:#?}"
    );
    compiled
}

fn probe_engine() -> Engine {
    let mut engine = Engine::new(
        SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        None,
    );
    engine.register_compute_fn("test::check_half_span", check_half_span_fn as ComputeFn);
    engine
}

/// 25mm > 10mm holds and 25mm > 30mm does not; an `Indeterminate` here is the
/// pre-fix symptom (the checker body-evaluated the `@optimized` call).
fn assert_probe_verdicts(entries: &[ConstraintCheckEntry], surface: &str) {
    for (index, expected) in [(0, Satisfaction::Satisfied), (1, Satisfaction::Violated)] {
        let id = ConstraintNodeId::new("CheckDispatchProbe", index);
        let entry = entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{surface}: no verdict for {id}; got {entries:?}"));
        assert_eq!(
            (entry.satisfaction, &entry.indeterminate_reason),
            (expected, &None),
            "{surface}: {id} must be decided from the dispatched trampoline result, \
             not body-eval Undef"
        );
    }
}

#[test]
fn engine_check_dispatches_optimized_calls_inside_constraints() {
    let compiled = compile_probe();
    let mut engine = probe_engine();

    let result = engine.check(&compiled);

    assert_probe_verdicts(&result.constraint_results, "Engine::check");
}

#[test]
fn recheck_with_values_dispatches_optimized_calls_inside_constraints() {
    let compiled = compile_probe();
    let mut engine = probe_engine();
    let evaluated = engine.eval(&compiled);

    let (entries, _diagnostics) = engine
        .check_constraints_with_values(&evaluated.values)
        .expect("eval initialised the snapshot");

    assert_probe_verdicts(&entries, "Engine::check_constraints_with_values");
}
