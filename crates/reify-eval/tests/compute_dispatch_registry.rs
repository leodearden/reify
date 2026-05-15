//! Integration tests for task γ (3422): per-Engine compute dispatch registry
//! and @optimized→ComputeNode lowering wire.
//!
//! Tests are grouped by step:
//!   step-3/4: trampoline-invocation contract via dispatch helper
//!   step-5/6: end-to-end @optimized→ComputeNode lowering (fixture eval)
//!   step-7/8: unregistered target fallback diagnostic
//!   step-9/10: public seam API-surface pin

use reify_eval::{
    CancellationHandle, ComputeDispatchRegistry, ComputeFn, ComputeOutcome, RealizationReadHandle,
};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{OpaqueState, Severity, Value, ValueCellId};

// ── Identity trampoline used by multiple tests ────────────────────────────────

fn identity_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: value_inputs[0].clone(),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

fn failing_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Failed {
        diagnostics: vec![reify_types::Diagnostic::error("test trampoline failed")],
    }
}

// ── step-3: RED — dispatch helper contract ───────────────────────────────────

/// Test: dispatch helper with registered identity trampoline returns the input
/// value as the result (maps ComputeOutcome::Completed → Ok(value)).
#[test]
fn dispatch_compute_node_registered_identity_returns_input_value() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let input = Value::Int(99);
    let (result_value, diagnostics) = engine
        .dispatch_compute_node("test::identity", &[input.clone()], &[], &Value::Undef, None)
        .expect("expected Ok for registered trampoline");

    assert_eq!(
        result_value, input,
        "expected result == input from identity trampoline"
    );
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics from identity trampoline, got: {:?}",
        diagnostics
    );
}

/// Test: dispatch helper with unregistered target returns an Err variant whose
/// diagnostic message names the unknown target.
#[test]
fn dispatch_compute_node_unregistered_target_returns_error_diagnostic() {
    let engine = make_simple_engine();

    let diags = engine
        .dispatch_compute_node(
            "nonexistent::target",
            &[Value::Int(1)],
            &[],
            &Value::Undef,
            None,
        )
        .expect_err("expected Err for unregistered target");

    assert!(!diags.is_empty(), "expected at least one diagnostic");
    let error_diag = diags.iter().find(|d| d.severity == Severity::Error);
    assert!(
        error_diag.is_some(),
        "expected Error-severity diagnostic, got: {:?}",
        diags
    );
    assert!(
        error_diag.unwrap().message.contains("nonexistent::target"),
        "expected diagnostic to name the unknown target, got: {:?}",
        error_diag
    );
}

/// Test: dispatch helper propagates Error diagnostics from a Failed trampoline.
#[test]
fn dispatch_compute_node_failed_outcome_surfaces_diagnostics() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::failing", failing_fn as ComputeFn);

    let diags = engine
        .dispatch_compute_node("test::failing", &[Value::Int(1)], &[], &Value::Undef, None)
        .expect_err("expected Err for Failed trampoline");

    assert!(!diags.is_empty(), "expected at least one diagnostic from Failed");
    let error_diag = diags.iter().find(|d| d.severity == Severity::Error);
    assert!(
        error_diag.is_some(),
        "expected Error-severity diagnostic from Failed outcome, got: {:?}",
        diags
    );
}

// ── step-5: RED — end-to-end @optimized→ComputeNode lowering ─────────────────
// PRD §8 γ observable signal:
//   (a) the observable cell's value == the call argument (42 → 42)
//   (b) the engine graph contains a ComputeNode with target=="test::identity"
//       (no inlining occurred)

/// Load the fixture source (compute_identity.ri inlined as a &str so the test
/// is self-contained and doesn't depend on the fixture file path at test time).
fn compute_identity_source() -> &'static str {
    include_str!("fixtures/compute_identity.ri")
}

/// End-to-end test: @optimized fn lowers to ComputeNode when trampoline is
/// registered, and the observable cell value equals the call argument.
#[test]
fn e2e_optimized_fn_lowers_to_compute_node_and_evaluates() {
    let source = compute_identity_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) The observable cell `IdentityFixture.result` must equal the input 42.
    let result_cell = ValueCellId::new("IdentityFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell IdentityFixture.result not found in eval result"));
    assert_eq!(
        *result_val,
        Value::Int(42),
        "expected IdentityFixture.result == Int(42) (trampoline identity), got {:?}",
        result_val
    );

    // (b) The evaluation graph must contain a ComputeNode whose target is
    //     "test::identity" (confirming the trampoline path, not inlining).
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "test::identity");
    assert!(
        compute_node.is_some(),
        "expected a ComputeNode with target==\"test::identity\" in the graph, \
         found compute nodes: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| &d.target)
            .collect::<Vec<_>>()
    );
}

// ── step-7: RED — unregistered @optimized target fallback diagnostic ──────────
// PRD §7.2: when the engine encounters an @optimized fn call whose target is not
// registered, it must emit an Error diagnostic naming the target, then fall back
// to body-inlining so the cell still evaluates correctly (no ComputeNode inserted).

/// Test: @optimized fn with unregistered target emits an Error diagnostic naming
/// the target, body-inlines (cell value == input), and inserts no ComputeNode.
#[test]
fn e2e_unregistered_optimized_target_emits_diagnostic_and_inlines() {
    // Use compute_identity.ri but register NO trampoline for "test::identity".
    let source = compute_identity_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    // Deliberately no register_compute_fn — "test::identity" is unregistered.
    let eval_result = engine.eval(&compiled);

    // (a) Must emit at least one Error diagnostic naming the unknown target.
    let error_diags: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected Error diagnostic for unregistered @optimized target, \
         got diagnostics: {:?}",
        eval_result.diagnostics
    );
    let target_named = error_diags.iter().any(|d| d.message.contains("test::identity"));
    assert!(
        target_named,
        "expected at least one Error diagnostic to name \"test::identity\", \
         got: {:?}",
        error_diags
    );

    // (b) Body inlines: cell value still equals the input (42).
    let result_cell = ValueCellId::new("IdentityFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell IdentityFixture.result not found in eval result"));
    assert_eq!(
        *result_val,
        Value::Int(42),
        "expected IdentityFixture.result == Int(42) (inline fallback), got {:?}",
        result_val
    );

    // (c) No ComputeNode inserted for the unregistered target.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let rogue_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "test::identity");
    assert!(
        rogue_node.is_none(),
        "expected no ComputeNode for unregistered target, found: {:?}",
        rogue_node.map(|(id, _)| id)
    );
}
