//! Integration tests for task γ (3422): per-Engine compute dispatch registry
//! and @optimized→ComputeNode lowering wire.
//!
//! Tests are grouped by step:
//!   step-3/4: trampoline-invocation contract via dispatch helper
//!   step-5/6: end-to-end @optimized→ComputeNode lowering (fixture eval)
//!   step-7/8: unregistered target fallback diagnostic
//!   step-9/10: public seam API-surface pin

use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::{
    CancellationHandle, ComputeDispatchRegistry, ComputeFn, ComputeOutcome, RealizationReadHandle,
    RealizedContent,
};
use reify_ir::{OpaqueState, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

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
        structured_detail: vec![],
    };

    // ComputeOutcome::Cancelled
    let _cancelled = ComputeOutcome::Cancelled;

    // ComputeOutcome::Failed
    let _failed = ComputeOutcome::Failed {
        diagnostics: vec![],
        structured_detail: vec![],
    };

    // ComputeDispatchRegistry is constructible
    let _registry = ComputeDispatchRegistry::new();

    // RealizationReadHandle is constructible via the public constructor
    let _handle = RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("test", 0),
        reify_core::ContentHash(0),
        None,
    );

    // CancellationHandle: cancel() and is_cancelled()
    let ch = CancellationHandle::new();
    ch.cancel();
    let _cancelled_flag: bool = ch.is_cancelled();

    // reify_eval::RealizedContent is re-exported and constructible (α seam pin).
    // Compile success is the signal — no prose assertions per seam-pin convention.
    let _rc: RealizedContent = RealizedContent::SurfaceMesh(std::sync::Arc::new(reify_ir::Mesh {
        vertices: vec![],
        indices: vec![],
        normals: None,
    }));
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
        result: value_inputs.first().cloned().unwrap_or(Value::Undef),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
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
        structured_detail: vec![],
    }
}

// ── e2e: zero-arg @optimized call drives trampoline with empty arg slice ─────
//
// This exercises the actual engine path that the empty-slice guard in
// identity_fn protects: a zero-argument @optimized call produces an empty
// arg_values vector when the engine evaluates the call. The trampoline
// receives &[] as its `value_inputs` parameter (which is the evaluated
// arg_values, not the ComputeNodeData.value_inputs graph field). The guard
// `value_inputs.first().cloned().unwrap_or(Value::Undef)` prevents a panic;
// the result written to the cell is Value::Undef.

/// e2e: a zero-argument @optimized call invokes the trampoline with an empty
/// arg_values slice, triggering the empty-slice guard in identity_fn and
/// writing Value::Undef to the output cell.
#[test]
fn e2e_optimized_zero_arg_call_invokes_trampoline_with_empty_inputs() {
    let source = r#"
        @optimized("test::identity")
        fn zero_arg_compute() -> Int {
            42
        }

        structure ZeroArgFixture {
            let result = zero_arg_compute()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // The trampoline received &[] (zero args → empty arg_values) and returned
    // Value::Undef via the empty-slice guard. The function body literal `42`
    // is NOT returned because for a registered trampoline the engine uses the
    // trampoline's ComputeOutcome directly (no body-inlining fallback).
    let result_cell = ValueCellId::new("ZeroArgFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell ZeroArgFixture.result not found in eval result"));
    assert_eq!(
        *result_val,
        Value::Undef,
        "expected ZeroArgFixture.result == Value::Undef (empty-slice guard fired) \
         for zero-arg @optimized call, got {:?}",
        result_val
    );
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
        .dispatch_compute_node(
            "test::identity",
            std::slice::from_ref(&input),
            &[],
            &Value::Undef,
            None,
        )
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

/// Task 5311 — the two assertions the HARD-site contract ADDS, kept as a
/// separate sibling of
/// `dispatch_compute_node_unregistered_target_returns_error_diagnostic` above,
/// which the RULING in `docs/prds/v0_6/check-diagnostic-truthfulness.md` D4
/// marks UNCHANGED and which is therefore not touched.
///
/// HONEST SCOPE — the INPUT is not what is new here. `make_simple_engine()`'s
/// compute registry is empty in the older test too, so "still `Severity::Error`
/// on an entirely empty registry" — the input that DOWNGRADES the two SOFT
/// sites in `engine_eval.rs` — is already pinned there, and stays pinned there.
/// What this test adds is only what that lock cannot absorb without being
/// edited: the [`DiagnosticCode`], and the deliberate ABSENCE of the
/// `(falling back to body-inlining)` clause. The severity is re-asserted on the
/// code-selected entry, in one line, so "coded AND still Error" is stated
/// somewhere as a single fact.
///
/// If a later sweep ever "tidies up" by applying the empty-registry predicate
/// uniformly across all four sites, BOTH tests turn red — which is the point of
/// keeping them adjacent.
///
/// Why the predicate is deliberately not applied at the HARD sites, argued on
/// the merits: `hard_no_trampoline_diagnostic`'s rustdoc in
/// `crates/reify-eval/src/engine_compute.rs`. Not restated here — it was
/// previously triplicated across that constructor, this docblock and an
/// assertion message.
#[test]
fn dispatch_compute_node_unregistered_target_is_error_and_coded_even_on_an_empty_registry() {
    let engine = make_simple_engine();
    assert!(
        engine.compute_dispatch("nonexistent::target").is_none(),
        "precondition: the target must be unregistered",
    );

    let diags = engine
        .dispatch_compute_node(
            "nonexistent::target",
            &[Value::Int(1)],
            &[],
            &Value::Undef,
            None,
        )
        .expect_err("expected Err for unregistered target");

    let diag = diags
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::NoRegisteredComputeTrampoline))
        .unwrap_or_else(|| {
            panic!(
                "the HARD form carries the SAME code as the SOFT form — the code \
                 names the cause, independently of severity, and downstream \
                 tooling matches on it rather than on the prose; got: {diags:?}"
            )
        });
    assert_eq!(
        diag.severity,
        Severity::Error,
        "the HARD sites do NOT apply the empty-registry downgrade — see \
         hard_no_trampoline_diagnostic's rustdoc; got: {diag:?}"
    );
    assert!(
        !diag.message.contains("falling back to body-inlining"),
        "the fallback clause is deliberately omitted at the HARD sites: \
         body-inlining is the eval-loop caller's behaviour, not this helper's, \
         and direct callers do not inline; got: {diag:?}"
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

    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic from Failed"
    );
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
// registered, it must emit a diagnostic naming the target, then fall back to
// body-inlining so the cell still evaluates correctly (no ComputeNode inserted).
// Task 5311 conditioned that diagnostic's SEVERITY at this SOFT site on whether
// the engine's compute registry is entirely empty (Warning) or not (Error); the
// pair of tests below covers both arms. The diagnostic's identity is carried by
// `DiagnosticCode::NoRegisteredComputeTrampoline`, which is the same in both.

/// Test: @optimized fn with unregistered target emits a diagnostic naming the
/// target, body-inlines (cell value == input), and inserts no ComputeNode.
///
/// SUPERSEDED at task 5311 on the SEVERITY assertion only — the four
/// behavioural assertions below are unchanged. This test builds
/// `make_simple_engine()`, whose compute registry is entirely EMPTY, and evals
/// through the SOFT emission site
/// (`engine_eval.rs::evaluate_params_and_lets_unified`). Per the 2026-09-01
/// RULING in `docs/prds/v0_6/check-diagnostic-truthfulness.md` D4, that site
/// now conditions severity on `compute_registry.fns.is_empty()`: an empty
/// registry means "this driver registered no trampolines at all", which is a
/// posture, not a defect, so the diagnostic is a `Severity::Warning`.
///
/// The old `Severity::Error` assertion pinned CURRENT BEHAVIOUR, not a
/// contract: the row it cited — `docs/prds/v0_3/compute-node-contract.md`:189 —
/// asks for a NAMED diagnostic and `Freshness::Failed`, and says NOTHING about
/// severity. What IS contractual is the CAUSE, so this test now also asserts
/// `DiagnosticCode::NoRegisteredComputeTrampoline`, which is PRESERVED across
/// the severity flip precisely so downstream tooling can match the cause
/// independently of severity.
///
/// `e2e_unregistered_optimized_target_on_a_nonempty_registry_stays_an_error`
/// below is this test's mandatory twin: without it, relaxing the severity here
/// would be a silent weakening rather than a supersession.
#[test]
fn e2e_unregistered_optimized_target_emits_diagnostic_and_inlines() {
    // Use compute_identity.ri but register NO trampoline for "test::identity".
    let source = compute_identity_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    // Deliberately no register_compute_fn — "test::identity" is unregistered,
    // AND the registry is entirely empty, which is what selects Warning.
    let eval_result = engine.eval(&compiled);

    // (a) Must emit at least one Warning diagnostic naming the unknown target.
    let warning_diags: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        !warning_diags.is_empty(),
        "expected Warning diagnostic for unregistered @optimized target on an \
         empty compute registry, got diagnostics: {:?}",
        eval_result.diagnostics
    );
    let target_named = warning_diags
        .iter()
        .any(|d| d.message.contains("test::identity"));
    assert!(
        target_named,
        "expected at least one Warning diagnostic to name \"test::identity\", \
         got: {:?}",
        warning_diags
    );

    // (a2) The CAUSE is carried by the code, which survives the severity flip.
    let coded = warning_diags
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NoRegisteredComputeTrampoline));
    assert!(
        coded,
        "expected the fallback diagnostic to carry \
         DiagnosticCode::NoRegisteredComputeTrampoline — downstream tooling \
         matches the code, not the severity or the prose, got: {:?}",
        warning_diags
    );

    // (a3) NEGATIVE: no Error-severity diagnostic naming the target may remain.
    // A `contains`-style assertion alone would pass if BOTH were emitted.
    let stray_error = eval_result
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error && d.message.contains("test::identity"));
    assert!(
        stray_error.is_none(),
        "an empty compute registry must produce ONLY the Warning form; a \
         surviving Error naming \"test::identity\" is the loud/silent mismatch \
         task 5311 closes, got: {:?}",
        stray_error
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

/// Task 5311 — the MANDATORY TWIN of
/// `e2e_unregistered_optimized_target_emits_diagnostic_and_inlines` above.
///
/// Same fixture, same four behavioural assertions, one difference: an unrelated
/// trampoline is registered FIRST, so `compute_registry.fns.is_empty()` is
/// false while `"test::identity"` itself stays unregistered. That is exactly
/// the `reify eval` / `reify build` posture — both call
/// `register_compute_trampolines`, whose production bundle registers 19
/// targets — and it must keep producing `Severity::Error`, because a driver
/// that registered SOME trampolines and is still missing THIS one is a genuine
/// defect rather than a declared posture.
///
/// Without this test, the severity relaxation in the sibling above would be
/// indistinguishable from an unconditional downgrade, and review must reject it
/// as a silent weakening. The `DiagnosticCode` assertion is identical in both,
/// which is the point: the code identifies the cause; the severity reports how
/// much the caller's posture makes it matter.
#[test]
fn e2e_unregistered_optimized_target_on_a_nonempty_registry_stays_an_error() {
    let source = compute_identity_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    // A target that exists nowhere else in the workspace: `register_compute_fn`
    // panics on a duplicate target, so a fresh name is required here.
    engine.register_compute_fn("test::registry_nonempty_probe", identity_fn as ComputeFn);
    // "test::identity" remains UNregistered — only the registry's emptiness changed.
    let eval_result = engine.eval(&compiled);

    // (a) Error, not Warning, because the registry is non-empty.
    let error_diags: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "a non-empty compute registry missing THIS target is a defect, not a \
         posture, so the diagnostic must stay Severity::Error, got: {:?}",
        eval_result.diagnostics
    );
    let target_named = error_diags
        .iter()
        .any(|d| d.message.contains("test::identity"));
    assert!(
        target_named,
        "expected at least one Error diagnostic to name \"test::identity\", \
         got: {:?}",
        error_diags
    );

    // (a2) Same code as the Warning form — the cause is severity-independent.
    let coded = error_diags
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::NoRegisteredComputeTrampoline));
    assert!(
        coded,
        "expected DiagnosticCode::NoRegisteredComputeTrampoline on the Error \
         form too, got: {:?}",
        error_diags
    );

    // (b) Body still inlines — severity does not change the fallback behaviour.
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

    // (c) Still no ComputeNode inserted for the unregistered target.
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

// ── step-5: e2e regression-pin — non-ValueRef arg leaves value_inputs empty ──
// CHARACTERIZATION TEST — intentionally GREEN on first write.
//
// γ contract: the shallow walk in engine_eval.rs that populates
// `ComputeNodeData::value_inputs` collects ONLY direct `ValueRef(cell)` args.
// A BinOp (or any non-ValueRef sub-expression), even one that transitively
// references a param cell, contributes NO entries.
//
// The trampoline is invoked with `arg_values` (the *evaluated* argument
// values), NOT with `value_inputs`. So a call `identity_compute_test(2 + input)`
// still evaluates correctly (2 + 40 = 42 via arg_values) even though
// `value_inputs` is empty.
//
// This pin guards the γ contract against P3.2's planned transitive-dependency
// walk: if P3.2 changes the shallow walk to include transitive refs,
// `data.value_inputs.is_empty()` here will turn RED and alert the reviewer
// that the γ/P3.2 boundary has shifted.

/// Regression-pin (step-5): a non-ValueRef arg (`2 + input`) evaluates to the
/// correct value via `arg_values` (Int(42)) while leaving `value_inputs` EMPTY
/// in the ComputeNode — the γ shallow-walk contract.
#[test]
fn e2e_optimized_non_valueref_arg_yields_empty_value_inputs() {
    // Inline fixture: the @optimized call takes a BinOp arg `2 + input`
    // (param input = 40), so the result is 42 but `value_inputs` is empty.
    let source = r#"
        @optimized("test::identity")
        fn identity_compute_test(x: Int) -> Int {
            x
        }

        structure NonValueRefFixture {
            param input: Int = 40
            let result = identity_compute_test(2 + input)
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) The trampoline evaluated the BinOp argument correctly via arg_values:
    //     2 + 40 == 42.
    let result_cell = ValueCellId::new("NonValueRefFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell NonValueRefFixture.result not found in eval result"));
    assert_eq!(
        *result_val,
        Value::Int(42),
        "expected NonValueRefFixture.result == Int(42) (2+40 via arg_values), got {:?}",
        result_val
    );

    // (b) The ComputeNode's value_inputs field is EMPTY — the γ shallow walk
    //     only captures direct ValueRef args; the BinOp `2 + input` is NOT a
    //     ValueRef, so input is NOT included even transitively.
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
    assert!(
        data.value_inputs.is_empty(),
        "expected value_inputs to be empty for non-ValueRef arg (γ shallow-walk contract), \
         got: {:?}",
        data.value_inputs
    );
}

// ── task #6662: @optimized dispatch at sub-INSTANCE scope ────────────────────
//
// Defect: `@optimized` → ComputeNode lowering happens ONLY at template scope
// (`engine_eval.rs::evaluate_params_and_lets_unified` /
// `evaluate_let_bindings`, both keyed on `for template in &module.templates`).
// Instance-scope cells — the ones a `sub` produces, keyed
// `ValueCellId::new("Parent.sub", member)` — are elaborated in
// `unfold.rs` through `cell_eval_ctx`, which carries no compute dispatch, so
// `reify_expr::try_compute_dispatch` returns None and the `.ri` function BODY
// runs instead. For every solver stdlib target the body is a bare sentinel
// constructor, so an instantiated sub silently gets an empty shell where the
// template got the real solved value.
//
// The trampoline/body discriminator below is the same trick the zero-arg test
// above uses: the trampoline returns 777, the `.ri` body returns 42, so the
// observed value names which path ran.

/// Trampoline returning a fixed sentinel (777) that can never be confused with
/// the `.ri` body literal (42) — the instance-vs-template discriminator.
fn const777_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::Int(777),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// RED (#6662 S1): an `@optimized` cell reached through a `sub` instantiation
/// must carry the template's DISPATCHED value, not the body-inlined fallback.
///
/// `Outer` instantiates `Inner` with no constructor overrides, so the instance
/// cell's inputs are value-identical to the template's and the two scopes must
/// agree. The template-scope assertion is kept deliberately: it is already
/// green today, and pinning it keeps the test honest about which scope changed.
#[test]
fn instance_scope_optimized_cell_equals_template_dispatched_value() {
    let source = r#"
        @optimized("test::const777")
        fn zero_arg_777() -> Int {
            42
        }

        structure Inner {
            let result = zero_arg_777()
        }

        structure Outer {
            sub inner = Inner()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::const777", const777_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) Template scope — already green; dispatch fires in `module.templates`.
    let template_cell = ValueCellId::new("Inner", "result");
    assert_eq!(
        eval_result.values.get(&template_cell),
        Some(&Value::Int(777)),
        "template-scope Inner.result must be the dispatched 777, got {:?}",
        eval_result.values.get(&template_cell)
    );

    // (b) Instance scope — the defect. Int(42) here means the `.ri` function
    //     BODY was inlined instead of the registered trampoline's result.
    let instance_cell = ValueCellId::new("Outer.inner", "result");
    let instance_val = eval_result.values.get(&instance_cell);
    assert_ne!(
        instance_val,
        Some(&Value::Int(42)),
        "Outer.inner.result is the body-inline sentinel Int(42): @optimized \
         dispatch did not reach instance scope (unfold.rs elaborates instance \
         cells without compute dispatch, so try_compute_dispatch returned None \
         and the .ri body ran)"
    );
    assert_eq!(
        instance_val,
        Some(&Value::Int(777)),
        "instance-scope Outer.inner.result must equal the template's dispatched \
         value Int(777) (no ctor overrides ⇒ inputs are value-identical), got {:?}",
        instance_val
    );
}

/// RED (#6662 S3): the fix must reach NESTED instance scope, and phase 1.5's
/// scratch overlay must agree with what phase 2 commits.
///
/// `elaborate_child_instance_nested`'s `Phase15Node::Let` arm computes a
/// SCRATCH let value into the running `overlay` that nested subs' constructor
/// args are pre-evaluated against, and its comment pins the invariant "the
/// scratch value equals the value phase 2 will commit". Fixing only phase 2
/// BREAKS that invariant for `@optimized` cells: phase 2 would commit 777 while
/// the overlay still held the body-inline 42 — a new, quieter instance of the
/// very defect being fixed. `Top.mid.sink.seed` is where a stale scratch value
/// surfaces and nowhere else.
#[test]
fn nested_sub_instance_optimized_cell_and_phase15_scratch_agree() {
    let source = r#"
        @optimized("test::const777")
        fn zero_arg_777() -> Int {
            42
        }

        structure Leaf {
            param seed : Int = 0
            let result = zero_arg_777()
        }

        structure Mid {
            sub leaf = Leaf()
            let echoed = self.leaf.result
            sub sink = Leaf(seed: self.leaf.result)
        }

        structure Top {
            sub mid = Mid()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::const777", const777_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) Two-level nesting: reaches phase 1.5's recursion, not just the
    //     top-level plain-sub loop.
    let leaf_cell = ValueCellId::new("Top.mid.leaf", "result");
    assert_eq!(
        eval_result.values.get(&leaf_cell),
        Some(&Value::Int(777)),
        "nested instance Top.mid.leaf.result must be the dispatched 777 \
         (Int(42) = body-inlined), got {:?}",
        eval_result.values.get(&leaf_cell)
    );

    // (b) The middle template's own let reads the nested sub's member through
    //     the projection BFS.
    let echoed_cell = ValueCellId::new("Top.mid", "echoed");
    assert_eq!(
        eval_result.values.get(&echoed_cell),
        Some(&Value::Int(777)),
        "Top.mid.echoed must read the nested sub's dispatched value, got {:?}",
        eval_result.values.get(&echoed_cell)
    );

    // (c) PHASE-1.5 PARITY: `sink`'s ctor arg is pre-evaluated against the
    //     phase-1.5 overlay, so a stale scratch 42 surfaces HERE.
    let sink_seed_cell = ValueCellId::new("Top.mid.sink", "seed");
    assert_eq!(
        eval_result.values.get(&sink_seed_cell),
        Some(&Value::Int(777)),
        "Top.mid.sink.seed must be 777: the nested sub's ctor arg is evaluated \
         against phase 1.5's scratch overlay, so Int(42) here means the overlay \
         still holds the body-inlined value while phase 2 commits the dispatched \
         one — the \"scratch value equals the value phase 2 will commit\" \
         invariant is broken. Got {:?}",
        eval_result.values.get(&sink_seed_cell)
    );
}

/// RED (#6662 S3): phase 1.5's SCRATCH let arm must resolve `@optimized` the
/// same way phase 2 does.
///
/// Discriminating shape: the phase-1.5 `Phase15Node::Let` node must ITSELF be
/// the `@optimized` call. (In the nested test above the phase-1.5 let is
/// `self.leaf.result`, a member projection over already-collapsed nested-sub
/// values — it never reaches the `@optimized` arm, which is why that test went
/// green on the phase-2 fix alone.)
///
/// Here `Mid.computed` IS the call, and `sub sink`'s ctor arg reads it. Phase
/// 1.5 evaluates the scratch value into the overlay that ctor args are
/// pre-evaluated against, while phase 2 separately commits the authoritative
/// cell — so a phase-2-only fix makes the two disagree: `Top.mid.computed` is
/// 777 but `Top.mid.sink.seed` is the stale body-inlined 42.
#[test]
fn phase15_scratch_let_optimized_value_matches_phase2_commit() {
    let source = r#"
        @optimized("test::const777")
        fn zero_arg_777() -> Int {
            42
        }

        structure Leaf2 {
            param seed : Int = 0
        }

        structure Mid2 {
            let computed = zero_arg_777()
            sub sink = Leaf2(seed: computed)
        }

        structure Top2 {
            sub mid = Mid2()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::const777", const777_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) Phase 2's authoritative commit — green since the phase-2 wiring.
    let computed_cell = ValueCellId::new("Top2.mid", "computed");
    assert_eq!(
        eval_result.values.get(&computed_cell),
        Some(&Value::Int(777)),
        "Top2.mid.computed must be the dispatched 777, got {:?}",
        eval_result.values.get(&computed_cell)
    );

    // (b) PHASE-1.5 PARITY — the genuine RED. `sink`'s ctor arg is
    //     pre-evaluated against the phase-1.5 overlay, so Int(42) here means
    //     the overlay holds the body-inlined value while phase 2 commits the
    //     dispatched one, breaking the arm's own documented invariant that
    //     "the scratch value equals the value phase 2 will commit".
    let sink_seed_cell = ValueCellId::new("Top2.mid.sink", "seed");
    assert_eq!(
        eval_result.values.get(&sink_seed_cell),
        Some(&Value::Int(777)),
        "Top2.mid.sink.seed must equal the phase-2 commit of Top2.mid.computed \
         (777); Int(42) is phase 1.5's stale body-inlined scratch value. Got {:?}",
        eval_result.values.get(&sink_seed_cell)
    );
}

/// RED (#6662 S3): a param whose DEFAULT is an `@optimized` call must carry the
/// dispatched value at instance scope too.
///
/// `elaborate_child_params_only`'s `default_expr` branch is the third
/// instance-scope eval site. Only the default arm is in scope: an EXPLICIT ctor
/// arg is by construction an instance-specific input, and it is evaluated in
/// the PARENT's scope, so the helper's read comparison would be against the
/// wrong map there.
#[test]
fn instance_scope_optimized_param_default_equals_template_dispatched_value() {
    let source = r#"
        @optimized("test::const777")
        fn zero_arg_777() -> Int {
            42
        }

        structure DefaultedParam {
            param p : Int = zero_arg_777()
        }

        structure OuterDp {
            sub dp = DefaultedParam()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::const777", const777_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // CHARACTERIZATION, measured on this branch: template scope does NOT lower
    // an `@optimized` call in a PARAM DEFAULT — only let-cells reach the
    // ComputeNode lowering — so `DefaultedParam.p` is the body-inlined 42, not
    // 777. That template-scope gap is a separate defect, filed as a follow-up;
    // closing it here would be a template-scope change, which this task's scope
    // (instance cells inheriting the template's dispatched value) excludes.
    //
    // Pinning the template value explicitly is what keeps the cross-scope
    // assertion below honest: without it, `instance == template` would pass
    // trivially in the world where BOTH scopes body-inline. When the
    // template-scope param gap is closed, THIS assertion turns RED first and
    // names exactly what changed — and the instance-scope assertion below
    // should then follow it to 777 in lockstep, with no production change
    // needed here, because the reuse helper copies whatever the template holds.
    let template_cell = ValueCellId::new("DefaultedParam", "p");
    assert_eq!(
        eval_result.values.get(&template_cell),
        Some(&Value::Int(42)),
        "characterization: template-scope DefaultedParam.p is the body-inlined \
         42 today (param defaults are not lowered to ComputeNodes). If this is \
         now 777, the template-scope param-default gap has been closed — update \
         the instance assertion below to match. Got {:?}",
        eval_result.values.get(&template_cell)
    );

    // The contract this task owns: whatever the template resolved to, the
    // uninstantiated-arg instance cell must AGREE with it. Agreement holds in
    // both worlds — it is the reuse helper's invariant, not a value pin.
    let instance_cell = ValueCellId::new("OuterDp.dp", "p");
    assert_eq!(
        eval_result.values.get(&instance_cell),
        eval_result.values.get(&template_cell),
        "instance-scope OuterDp.dp.p must equal template-scope DefaultedParam.p \
         (no ctor override ⇒ inputs are value-identical); got instance {:?} vs \
         template {:?}",
        eval_result.values.get(&instance_cell),
        eval_result.values.get(&template_cell)
    );
}

/// Doubling trampoline — makes the reuse gate's INPUT sensitivity observable:
/// the dispatched result depends on the argument, so a blanket copy of the
/// template's value is distinguishable from a correct per-instance answer.
fn double_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let out = match value_inputs.first() {
        Some(Value::Int(n)) => Value::Int(n * 2),
        _ => Value::Undef,
    };
    ComputeOutcome::Completed {
        result: out,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// RED (#6662 S5): the reuse gate must DECLINE when the instance's inputs
/// differ from the template's, and must say so out loud.
///
/// This locks the two ways the reuse helper could be wrong:
///   - copying the template value even when the instance's inputs differ
///     (silently WRONG — strictly worse than the bug being fixed);
///   - declining SILENTLY (today's bug, merely relocated).
///
/// `Outer3.a` takes the default (inputs equal ⇒ reuse fires); `Outer3.b`
/// overrides `x` (inputs differ ⇒ the helper must not copy).
#[test]
fn instance_scope_optimized_cell_declines_reuse_when_ctor_args_differ() {
    let source = r#"
        @optimized("test::double")
        fn dbl(x : Int) -> Int {
            0
        }

        structure Inner2 {
            param x : Int = 3
            let r = dbl(x)
        }

        structure Outer3 {
            sub a = Inner2()
            sub b = Inner2(x: 10)
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::double", double_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    // (a) Template scope dispatched: 2 * 3 == 6.
    let template_cell = ValueCellId::new("Inner2", "r");
    assert_eq!(
        eval_result.values.get(&template_cell),
        Some(&Value::Int(6)),
        "template-scope Inner2.r must be the dispatched 2*3 == 6, got {:?}",
        eval_result.values.get(&template_cell)
    );

    // (b) No ctor override ⇒ inputs equal ⇒ reuse fires.
    let a_cell = ValueCellId::new("Outer3.a", "r");
    assert_eq!(
        eval_result.values.get(&a_cell),
        Some(&Value::Int(6)),
        "Outer3.a.r must reuse the template's dispatched 6 (x defaults to 3, so \
         the instance's inputs are value-identical), got {:?}",
        eval_result.values.get(&a_cell)
    );

    // (c) Ctor override ⇒ inputs DIFFER (x = 10 vs 3) ⇒ the helper must NOT
    //     copy 6. Pin the exact body-inline sentinel (`dbl`'s body is `0`), not
    //     merely `!= Int(6)`: a weaker assertion would let a future blanket-copy
    //     regression through as long as it produced something else.
    let b_cell = ValueCellId::new("Outer3.b", "r");
    assert_eq!(
        eval_result.values.get(&b_cell),
        Some(&Value::Int(0)),
        "Outer3.b.r must be the body-inline sentinel Int(0): x is overridden to \
         10, so the template's 6 is NOT this instance's answer and must not be \
         copied. (The correct per-instance answer, 20, needs genuine \
         per-instance dispatch — #6592.) Got {:?}",
        eval_result.values.get(&b_cell)
    );

    // (d) The decline must be LOUD — exactly one Warning, naming both the
    //     @optimized target and the scoped cell that declined, and none for the
    //     instance that reused successfully.
    let warnings: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("test::double"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly ONE Warning naming \"test::double\" (the declining \
         cell Outer3.b.r). Phase 1.5's scratch arm and phase 2's committing arm \
         both see the same cell, so more than one means the diagnostic is \
         emitted from a non-authoritative site too. Got: {:?}",
        eval_result.diagnostics
    );
    assert!(
        warnings[0].message.contains("Outer3.b"),
        "the Warning must name the scoped cell that declined (Outer3.b), got: {:?}",
        warnings[0]
    );
    assert!(
        !warnings[0].message.contains("Outer3.a"),
        "Outer3.a reused successfully and must NOT be named in a decline \
         warning, got: {:?}",
        warnings[0]
    );
}

/// Guard for the `reify check` path (#6662 S5/S6): when NO trampoline is
/// registered, template scope body-inlines too, so instance and template agree
/// and the reuse gate fires SILENTLY. `reify check` deliberately registers no
/// trampolines, so a diagnostic here would fire on every `check` of every
/// fixture with an `@optimized` call in an instantiated structure.
#[test]
fn instance_scope_optimized_unregistered_target_reuses_silently() {
    let source = r#"
        @optimized("test::never_registered")
        fn unregistered_call() -> Int {
            42
        }

        structure InnerU {
            let result = unregistered_call()
        }

        structure OuterU {
            sub inner = InnerU()
        }
    "#;
    let compiled = parse_and_compile_with_stdlib(source);

    // NO register_compute_fn — this is the `reify check` shape.
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // Both scopes body-inline, and they AGREE — which is correct.
    let template_cell = ValueCellId::new("InnerU", "result");
    let instance_cell = ValueCellId::new("OuterU.inner", "result");
    assert_eq!(
        eval_result.values.get(&template_cell),
        Some(&Value::Int(42)),
        "template-scope InnerU.result body-inlines to 42 with no trampoline registered"
    );
    assert_eq!(
        eval_result.values.get(&instance_cell),
        Some(&Value::Int(42)),
        "instance-scope OuterU.inner.result must agree with the template's 42"
    );

    // The reuse gate must NOT add a decline warning here: inputs compare equal
    // and the template cell exists, so this is `Reuse`, not `Unreusable`. The
    // unregistered-target diagnostic that template scope already emits is the
    // one and only report of this condition.
    let decline_warnings: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .filter(|d| d.message.contains("OuterU.inner"))
        .collect();
    assert!(
        decline_warnings.is_empty(),
        "an unregistered @optimized target must not produce an instance-scope \
         decline warning (template and instance agree; `reify check` registers \
         no trampolines and would otherwise warn on every fixture). Got: {:?}",
        decline_warnings
    );
}
