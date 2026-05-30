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
use reify_core::{Severity, ValueCellId};
use reify_ir::{OpaqueState, Value};

// ── step-9: RED — public seam API-surface pin ─────────────────────────────────
// Compile-time test that coerces a concrete fn to `reify_eval::ComputeFn`,
// constructs all `ComputeOutcome` variants, names `ComputeDispatchRegistry`,
// and exercises the re-exported `reify_eval::CancellationHandle` API
// (cancel()/is_cancelled()). Pinning the cross-crate seam shape that later
// slices and downstream PRDs (buckling-eigensolver, shell-extract-engine-bridge)
// depend on. No prose assertions — compile success is the signal.

#[allow(dead_code)]
fn _seam_pin_api_surface() {
    // ComputeFn is a plain fn-pointer type
    let _f: ComputeFn = identity_fn;

    // ComputeOutcome::Completed
    let _completed = ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    };

    // ComputeOutcome::Cancelled
    let _cancelled = ComputeOutcome::Cancelled;

    // ComputeOutcome::Failed
    let _failed = ComputeOutcome::Failed {
        diagnostics: vec![],
    };

    // ComputeDispatchRegistry is constructible
    let _registry = ComputeDispatchRegistry::new();

    // RealizationReadHandle is constructible
    let _handle = RealizationReadHandle {
        node_id: reify_core::RealizationNodeId::new("test", 0),
    };

    // CancellationHandle: cancel() and is_cancelled()
    let ch = CancellationHandle::new();
    ch.cancel();
    let _cancelled_flag: bool = ch.is_cancelled();
}

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
        diagnostics: vec![reify_core::Diagnostic::error("test trampoline failed")],
    }
}

// ── guard: identity_fn empty value_inputs ────────────────────────────────────

/// RED (step-3): `identity_fn` called with an empty `value_inputs` slice must
/// return `ComputeOutcome::Completed { result: Value::Undef }` instead of
/// panicking. Mirrors the engine_compute.rs unit test for the integration-file
/// copy of the same helper.
#[test]
fn identity_fn_empty_value_inputs_returns_undef_without_panic() {
    let result = identity_fn(&[], &[], &Value::Undef, None, &CancellationHandle::new());
    match result {
        ComputeOutcome::Completed { result: Value::Undef, .. } => {}
        other => panic!(
            "expected ComputeOutcome::Completed {{ result: Value::Undef }}, got {:?}",
            other
        ),
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
        .dispatch_compute_node("test::identity", std::slice::from_ref(&input), &[], &Value::Undef, None)
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

// ── step-11: RED — ComputeNodeId index-collision regression ───────────────────
// Review feedback #1 (engine_eval.rs:2806-2809): the lowering hardcoded
// `ComputeNodeId::new(cell_id.entity.as_str(), 0)`, so two `@optimized` calls
// in the same entity would collide on the `PersistentMap<ComputeNodeId, _>`
// key, with the second `insert_compute_node` silently overwriting the first.
//
// This test pins the contract that each per-entity ComputeNode receives a
// distinct `index`, surviving PersistentMap insertion as separate entries.

/// Two-call inline fixture: an entity with TWO `@optimized("test::identity")`
/// calls — the engine must insert TWO distinct ComputeNodes (not overwrite
/// one with the other).
#[test]
fn e2e_two_optimized_calls_in_same_entity_yield_distinct_compute_nodes() {
    let source = r#"
        @optimized("test::identity")
        fn identity_compute_test(x: Int) -> Int {
            x
        }

        structure TwoCallsFixture {
            param input1: Int = 7
            param input2: Int = 9
            let result1 = identity_compute_test(input1)
            let result2 = identity_compute_test(input2)
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();

    // (a) Both ComputeNodes survived insertion: the filter count == 2 means
    //     neither got overwritten by a colliding ComputeNodeId.
    let identity_nodes: Vec<_> = snapshot
        .graph
        .compute_nodes
        .iter()
        .filter(|(_, d)| d.target == "test::identity")
        .collect();
    assert_eq!(
        identity_nodes.len(),
        2,
        "expected 2 ComputeNodes for 2 @optimized calls in the same entity, \
         found {} (this indicates ComputeNodeId index collision): {:?}",
        identity_nodes.len(),
        identity_nodes
            .iter()
            .map(|(id, _)| (*id).clone())
            .collect::<Vec<_>>()
    );

    // (b) The two inserted ComputeNodeIds have distinct `index` values.
    use std::collections::HashSet;
    let indices: HashSet<u32> = identity_nodes.iter().map(|(id, _)| id.index).collect();
    assert_eq!(
        indices.len(),
        2,
        "expected 2 distinct ComputeNodeId indices, got {:?} (collision)",
        indices
    );

    // (c) Both observable cells evaluate to their respective inputs.
    let r1 = eval_result
        .values
        .get(&ValueCellId::new("TwoCallsFixture", "result1"))
        .expect("TwoCallsFixture.result1 not found");
    let r2 = eval_result
        .values
        .get(&ValueCellId::new("TwoCallsFixture", "result2"))
        .expect("TwoCallsFixture.result2 not found");
    assert_eq!(*r1, Value::Int(7), "result1 should be Int(7), got {:?}", r1);
    assert_eq!(*r2, Value::Int(9), "result2 should be Int(9), got {:?}", r2);
}

// ── step-13: RED — value_inputs self-loop regression ─────────────────────────
// Review feedback #2 (engine_eval.rs:2811, 2819): the lowering sets
// `value_inputs: vec![cell_id.clone()]`, which is the OUTPUT cell — that's a
// graph self-loop. Per graph.rs ComputeNodeData doc, `value_inputs` is the
// "Inputs (drive cache key in P3.2)" field and must reference the actual
// argument cells whose values feed the trampoline, not the output cell.
//
// This test pins the contract that `value_inputs` excludes the output cell
// and includes the direct ValueRef argument cell.

/// Test: the inserted ComputeNode has correct `value_inputs` (input cell,
/// not the output cell), preserving `output_value_cells` as the output.
#[test]
fn e2e_compute_node_value_inputs_does_not_include_output_cell() {
    let source = compute_identity_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let _eval_result = engine.eval(&compiled);

    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();

    let (_id, data) = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "test::identity")
        .expect("expected a ComputeNode with target == \"test::identity\"");

    let input_cell = ValueCellId::new("IdentityFixture", "input");
    let result_cell = ValueCellId::new("IdentityFixture", "result");

    // (a) value_inputs MUST NOT contain the output cell — that's a self-loop.
    assert!(
        !data.value_inputs.contains(&result_cell),
        "value_inputs must not contain the output cell (self-loop bug). \
         Got value_inputs: {:?}",
        data.value_inputs
    );

    // (b) value_inputs MUST equal the direct argument cell list (just `input`).
    assert_eq!(
        data.value_inputs,
        vec![input_cell.clone()],
        "value_inputs should be [IdentityFixture.input], got {:?}",
        data.value_inputs
    );

    // (c) output_value_cells is unchanged (still the result cell).
    assert_eq!(
        data.output_value_cells,
        vec![result_cell],
        "output_value_cells should be [IdentityFixture.result], got {:?}",
        data.output_value_cells
    );
}

// ── amend: registered Failed trampoline does NOT silently body-inline ────────
// Review feedback (suggestion 1, engine_eval.rs:2888-2893): before this
// amendment, when a registered compute trampoline returned
// ComputeOutcome::Failed (or Cancelled), the lowering surfaced the Error
// diagnostics but then fell through to body-inlining — and (assuming the body
// succeeded) the cell ended up with a perfectly valid Determined value. From
// the user's perspective the structure 'evaluated' successfully despite a hard
// Error diagnostic claiming the @optimized target failed.
//
// This regression test pins the contract that Failed/Cancelled propagate
// through to the cell: the diagnostics are surfaced AND the cell is NOT
// rescued by body-inline. Distinct from the unregistered-target case (PRD §9
// Q1), where fallback IS the documented behaviour.

/// Test: a registered trampoline that returns Failed surfaces the diagnostics
/// and the observable cell is NOT silently rescued via body-inlining.
#[test]
fn e2e_registered_failed_trampoline_does_not_silently_body_inline() {
    // Inline fixture: `@optimized("test::failing")` with body `x` (the same
    // inline-fallback shape as the identity fixture). Registers `failing_fn`
    // for "test::failing" so the trampoline is present but always Failed.
    let source = r#"
        @optimized("test::failing")
        fn failing_compute_test(x: Int) -> Int {
            x
        }

        structure FailingFixture {
            param input: Int = 42
            let result = failing_compute_test(input)
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::failing", failing_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) The failing trampoline's diagnostics are surfaced.
    let error_diags: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic from the Failed trampoline, \
         got diagnostics: {:?}",
        eval_result.diagnostics
    );
    let failing_named = error_diags
        .iter()
        .any(|d| d.message.contains("test trampoline failed"));
    assert!(
        failing_named,
        "expected the trampoline's own \"test trampoline failed\" diagnostic to \
         be surfaced, got: {:?}",
        error_diags
    );

    // (b) Body-inlining did NOT occur — the cell is NOT a Determined Int(42).
    //     The §9.1-mirroring Failed handler does not write to `values`, so the
    //     cell is absent from the result map (matching the panic-boundary
    //     precedent at engine_eval.rs ~L2929-2965). The KEY assertion is
    //     "NOT Int(42)" — that distinguishes Failed from the body-inline
    //     rescue this amendment removed.
    let result_cell = ValueCellId::new("FailingFixture", "result");
    let inlined = eval_result.values.get(&result_cell) == Some(&Value::Int(42));
    assert!(
        !inlined,
        "expected the cell to NOT be silently body-inlined to Int(42); got {:?}",
        eval_result.values.get(&result_cell)
    );
}
