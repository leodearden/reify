//! Contracts for Engine-side test-instrumentation counters (task 2276).
//!
//! Each test pins the wiring of one counter: starts at 0 after the `eval()`
//! or `eval_cached()` entry point resets it, reaches ≥ 1 when the targeted
//! emitter path fires.
//!
//! Independent of the LSP cluster that uses these counters for structural
//! sanity checks — this file anchors the contract at the source of truth
//! (Engine itself) so the counters remain locked even if the LSP cluster
//! changes shape.

use reify_core::{DimensionVector, ModulePath, ValueCellId, VersionId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;

// ─── param_override_type_kind counter ────────────────────────────────────────

/// Counter is 0 after `eval()` with no override; ≥ 1 after `eval()` with a
/// TypeKindMismatch override (Bool on a Scalar param).
#[test]
fn param_override_type_kind_rejection_counter_increments() {
    let compiled = parse_and_compile_with_stdlib("structure S { param width: Length = 100mm }");
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

    // Warm the engine state; no override yet.
    let _ = engine.eval(&compiled);

    // Counter resets to 0 at eval() entry — no override means nothing fires.
    assert_eq!(
        engine.last_param_override_type_kind_rejections(),
        0,
        "no override installed — counter must be 0 after initial eval"
    );

    // Install a TypeKindMismatch override (Bool on a Scalar param).
    engine.set_param_and_invalidate(&ValueCellId::new("S", "width"), Value::Bool(true));

    // Run eval again — triggers the type-kind rejection path.
    let _ = engine.eval(&compiled);

    // Assert exactly 1 (not just ≥ 1) so a cumulative-count regression (counter not
    // resetting at eval() entry) would surface as 2+ on a repeated triggering eval.
    assert_eq!(
        engine.last_param_override_type_kind_rejections(),
        1,
        "type-kind mismatch rejection must have incremented the counter to exactly 1; \
         if > 1, counter may not be resetting at eval() entry"
    );
}

// ─── param_override_dimension counter ────────────────────────────────────────

/// Counter is 0 after `eval()` with no override; ≥ 1 after `eval()` with a
/// ScalarDimensionMismatch override (mass dimension on a length-typed param).
#[test]
fn param_override_dimension_rejection_counter_increments() {
    let compiled = parse_and_compile_with_stdlib("structure S { param width: Length = 100mm }");
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

    // Warm the engine state; no override yet.
    let _ = engine.eval(&compiled);

    // Counter resets to 0 at eval() entry — no override means nothing fires.
    assert_eq!(
        engine.last_param_override_dimension_rejections(),
        0,
        "no override installed — counter must be 0 after initial eval"
    );

    // Install a ScalarDimensionMismatch override (mass dimension on a length-typed param).
    engine.set_param_and_invalidate(
        &ValueCellId::new("S", "width"),
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        },
    );

    // Run eval again — triggers the dimension rejection path.
    let _ = engine.eval(&compiled);

    // Assert exactly 1 (not just ≥ 1) so a cumulative-count regression (counter not
    // resetting at eval() entry) would surface as 2+ on a repeated triggering eval.
    assert_eq!(
        engine.last_param_override_dimension_rejections(),
        1,
        "dimension mismatch rejection must have incremented the counter to exactly 1; \
         if > 1, counter may not be resetting at eval() entry"
    );
}

// ─── sub_component_unknown_structure counter ──────────────────────────────────

/// Counter is 0 after `eval()` with a valid source; ≥ 1 after `eval()` on a
/// source with an unknown structure reference. Also covers the `eval_cached()`
/// writer site: each call resets and re-counts independently.
#[test]
fn sub_component_unknown_structure_counter_increments_in_eval_and_eval_cached() {
    let compiled: reify_compiler::CompiledModule = {
        // Does not compile_with_stdlib-assert-no-errors because the unknown
        // structure reference is intentionally an eval-time error, not a
        // compile-time error.
        let parsed = reify_syntax::parse(
            "structure S { sub x = Unknown() }",
            ModulePath::single("test"),
        );
        reify_compiler::compile_with_stdlib(&parsed)
    };

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

    // eval() path — writer site at engine_eval.rs (first eval pass).
    let _ = engine.eval(&compiled);
    // Assert exactly 1 (not just ≥ 1): each call resets the counter at entry, so exactly one
    // unknown-structure reference means exactly one increment per call. If the counter became
    // cumulative (forgot to reset), the second call would show 2 and fail.
    assert_eq!(
        engine.last_sub_component_unknown_structure_errors(),
        1,
        "eval() sub-component unknown-structure path must have incremented the counter to \
         exactly 1; if > 1, counter may not be resetting at eval() entry"
    );

    // eval_cached() path — writer site at engine_eval.rs (eval_cached pass).
    // Each eval_cached() call resets its own counter independently (reset-at-entry).
    let version = VersionId(999);
    let _ = engine.eval_cached(&compiled, version);
    assert_eq!(
        engine.last_sub_component_unknown_structure_errors(),
        1,
        "eval_cached() sub-component unknown-structure path must have incremented the counter \
         to exactly 1; if > 1, counter may not be resetting at eval_cached() entry"
    );
}
