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
use reify_test_support::make_simple_engine;
use reify_types::{OpaqueState, Severity, Value};

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
