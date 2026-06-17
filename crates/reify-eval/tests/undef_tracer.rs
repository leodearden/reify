//! Integration tests for task 4322 (PRD undef-self-describing β): undef tracer.
//!
//! Tests the `Engine::trace_undef_causes` wrapper against REAL `Engine::eval` runs
//! (capture ON).  Signal assertions cover BT2, BT3, and B3; guard assertions cover
//! capture-OFF and no-snapshot cases.
//!
//! Reuses the reify_test_support harness from `tests/undef_cause_capture.rs`.

use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::UndefCause;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

// ── fixture helpers ───────────────────────────────────────────────────────────

fn layer1_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_causes_layer1.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_causes_layer1.ri should compile without errors: {errors:#?}"
    );
    m
}

fn undef_trace_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_trace.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_trace.ri should compile without errors: {errors:#?}"
    );
    m
}

// ── BT2: two-root propagated let ─────────────────────────────────────────────

/// BT2: trace(c) where c = a+b and a/b are Unbound → 2 causes, both Unbound,
/// order-stable (a before b, sorted by ValueCellId ascending).
#[test]
fn bt2_two_root_propagated() {
    let module = layer1_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let c_id = ValueCellId::new("UndefDemo", "c");
    let a_id = ValueCellId::new("UndefDemo", "a");
    let b_id = ValueCellId::new("UndefDemo", "b");

    let result = engine.trace_undef_causes(&c_id);

    assert_eq!(result.len(), 2, "BT2: expected 2 causes for c (a and b Unbound), got {:?}", result);
    assert!(
        result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &a_id)),
        "BT2: result must contain Unbound(a): {:?}",
        result
    );
    assert!(
        result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &b_id)),
        "BT2: result must contain Unbound(b): {:?}",
        result
    );

    // Order-stability: a < b (sorted by ValueCellId ascending).
    let params: Vec<&ValueCellId> = result
        .iter()
        .filter_map(|r| if let UndefCause::Unbound { param, .. } = r { Some(param) } else { None })
        .collect();
    assert_eq!(params.len(), 2, "BT2: expected 2 Unbound params");
    assert!(
        params[0] < params[1],
        "BT2: result must be sorted by ValueCellId ascending, got {:?}",
        params
    );
}

// ── BT3: chain collapse ───────────────────────────────────────────────────────

/// BT3: trace(z) where z=y=x and x is Unbound → [Unbound x] only (y/z propagated).
#[test]
fn bt3_chain_collapse() {
    let module = undef_trace_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let z_id = ValueCellId::new("UndefTrace", "z");
    let x_id = ValueCellId::new("UndefTrace", "x");

    let result = engine.trace_undef_causes(&z_id);

    assert_eq!(result.len(), 1, "BT3: expected [Unbound x], got {:?}", result);
    assert!(
        matches!(&result[0], UndefCause::Unbound { param, .. } if param == &x_id),
        "BT3: expected Unbound(x), got {:?}",
        result
    );
}

// ── B3: multi-root mixed causes ───────────────────────────────────────────────

/// B3: trace(r) where r = a+b+u, a/b Unbound, u UserUndef →
/// [Unbound a, Unbound b, UserUndef u] (3 causes, sorted by cell id ascending).
#[test]
fn b3_multi_root_mixed_causes() {
    let module = undef_trace_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let r_id = ValueCellId::new("UndefTrace", "r");
    let a_id = ValueCellId::new("UndefTrace", "a");
    let b_id = ValueCellId::new("UndefTrace", "b");

    let result = engine.trace_undef_causes(&r_id);

    assert_eq!(result.len(), 3, "B3: expected 3 causes for r, got {:?}", result);

    // All three roots must be present.
    assert!(
        result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &a_id)),
        "B3: must contain Unbound(a): {:?}",
        result
    );
    assert!(
        result.iter().any(|r| matches!(r, UndefCause::Unbound { param, .. } if param == &b_id)),
        "B3: must contain Unbound(b): {:?}",
        result
    );
    assert!(
        result.iter().any(|r| matches!(r, UndefCause::UserUndef { .. })),
        "B3: must contain UserUndef (u): {:?}",
        result
    );

    // Order-stability: sorted by originating ValueCellId ascending.
    // For this fixture: UndefTrace::a < UndefTrace::b < UndefTrace::u.
    // The first two results should be Unbound (a then b) and the last UserUndef (u).
    assert!(
        matches!(&result[0], UndefCause::Unbound { param, .. } if param == &a_id),
        "B3: first cause must be Unbound(a), got {:?}",
        result
    );
    assert!(
        matches!(&result[1], UndefCause::Unbound { param, .. } if param == &b_id),
        "B3: second cause must be Unbound(b), got {:?}",
        result
    );
    assert!(
        matches!(&result[2], UndefCause::UserUndef { .. }),
        "B3: third cause must be UserUndef(u), got {:?}",
        result
    );
}

// ── guard: capture-OFF → empty trace ─────────────────────────────────────────

/// capture-OFF engine: trace returns [] (no origins recorded in side-map).
#[test]
fn capture_off_returns_empty() {
    let module = layer1_module();

    // Engine with capture OFF (default).
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.eval(&module);

    let c_id = ValueCellId::new("UndefDemo", "c");
    let result = engine.trace_undef_causes(&c_id);
    assert!(result.is_empty(), "capture-OFF must yield empty trace: {:?}", result);
}

// ── guard: no-snapshot (no eval called) → empty trace ────────────────────────

/// No eval called → no snapshot → trace returns [].
#[test]
fn no_snapshot_returns_empty() {
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    // eval() never called — no snapshot present.
    let c_id = ValueCellId::new("UndefDemo", "c");
    let result = engine.trace_undef_causes(&c_id);
    assert!(result.is_empty(), "no-snapshot engine must yield empty trace: {:?}", result);
}
