//! Integration tests for task 4321 (PRD undef-self-describing α):
//! per-cell UndefCause origin capture via the post-eval classification pass.
//!
//! Signal assertions (real Engine::eval path, not synthetic snapshot construction):
//! - A2: each Layer-1 cluster records the correct UndefCause variant.
//! - A3: a purely-propagated undef (let c = a+b, inputs unbound) records None.
//! - A1/BT8: capture on vs. off leaves (Value, DeterminacyState) and
//!   content-hash per cell byte-identical (transparency lock).

use reify_core::{Diagnostic, ValueCellId};
use reify_eval::Engine;
use reify_ir::UndefCause;
use reify_test_support::{
    MockConstraintChecker, MockConstraintSolver, collect_errors, compile_source_with_stdlib,
};

// ── Helper: load and compile the Layer-1 fixture ─────────────────────────────

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

fn solve_failed_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_cause_solve_failed.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_cause_solve_failed.ri should compile without errors: {errors:#?}"
    );
    m
}

// ── A2: Layer-1 origins — non-solver path ────────────────────────────────────

/// Evaluating UndefDemo with capture ON records:
///   - "a" and "b"  → Unbound
///   - "c"          → None (propagated, A3)
///   - "u"          → UserUndef
///   - "k"          → AwaitingSolve (no solver attached)
///
/// And a second engine with capture OFF leaves undef_causes() empty.
#[test]
fn layer1_non_solver_origins_recorded() {
    let module = layer1_module();

    // Engine with capture ON.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();

    // "a" and "b" are unbound required params → Unbound.
    let a_id = ValueCellId::new("UndefDemo", "a");
    let b_id = ValueCellId::new("UndefDemo", "b");
    assert!(
        matches!(causes.get(&a_id), Some(UndefCause::Unbound { .. })),
        "expected Unbound for 'a', got {:?}",
        causes.get(&a_id)
    );
    assert!(
        matches!(causes.get(&b_id), Some(UndefCause::Unbound { .. })),
        "expected Unbound for 'b', got {:?}",
        causes.get(&b_id)
    );

    // Field-level check on 'a': param must equal the cell id, span must be
    // non-empty (i.e. the classifier captured a real source location).
    if let Some(UndefCause::Unbound { param, span }) = causes.get(&a_id) {
        assert_eq!(param, &a_id, "Unbound.param must equal the cell's ValueCellId");
        assert!(!span.is_empty(), "Unbound.span must be non-empty (real source location)");
    }

    // "c" is a propagated let (c = a + b; both inputs undef) → None (A3).
    let c_id = ValueCellId::new("UndefDemo", "c");
    assert!(
        causes.get(&c_id).is_none(),
        "propagated let 'c' must record None, got {:?}",
        causes.get(&c_id)
    );

    // "u" is `param u: Real = undef` → UserUndef.
    let u_id = ValueCellId::new("UndefDemo", "u");
    assert!(
        matches!(causes.get(&u_id), Some(UndefCause::UserUndef { .. })),
        "expected UserUndef for 'u', got {:?}",
        causes.get(&u_id)
    );

    // "k" is `param k: Length = auto`, no solver → AwaitingSolve.
    let k_id = ValueCellId::new("UndefDemo", "k");
    assert!(
        matches!(causes.get(&k_id), Some(UndefCause::AwaitingSolve { .. })),
        "expected AwaitingSolve for 'k', got {:?}",
        causes.get(&k_id)
    );

    // Engine with capture OFF leaves undef_causes() empty.
    let mut engine_off = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_off.eval(&module);
    assert!(
        engine_off.undef_causes().is_empty(),
        "capture OFF engine must return empty undef_causes"
    );
}

// ── A2: SolveFailed — infeasible solver path (step-5 / step-6) ───────────────

/// Evaluating SolveFail with MockConstraintSolver::new_infeasible records
/// SolveFailed (not AwaitingSolve) for the auto param "x".
#[test]
fn solve_failed_origin_recorded() {
    let module = solve_failed_module();

    let solver = MockConstraintSolver::new_infeasible(vec![
        Diagnostic::error("constraints are infeasible"),
    ]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();
    let x_id = ValueCellId::new("SolveFail", "x");

    assert!(
        matches!(causes.get(&x_id), Some(UndefCause::SolveFailed { .. })),
        "expected SolveFailed for 'x' after infeasible solve, got {:?}",
        causes.get(&x_id)
    );

    // Verify the detail string is non-empty / coarse.
    if let Some(UndefCause::SolveFailed { detail }) = causes.get(&x_id) {
        assert!(!detail.is_empty(), "SolveFailed.detail must not be empty");
    }
}

// ── A2: SolveFailed — no-progress solver path ────────────────────────────────

/// Evaluating SolveFail with MockConstraintSolver::new_no_progress records
/// SolveFailed (not AwaitingSolve) for the auto param "x", with a detail
/// string beginning "no progress:".
#[test]
fn no_progress_solve_failed_origin_recorded() {
    let module = solve_failed_module();

    let solver = MockConstraintSolver::new_no_progress("iteration limit reached");

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();
    let x_id = ValueCellId::new("SolveFail", "x");

    assert!(
        matches!(causes.get(&x_id), Some(UndefCause::SolveFailed { .. })),
        "expected SolveFailed for 'x' after no-progress solve, got {:?}",
        causes.get(&x_id)
    );

    // The detail string must begin "no progress:" to distinguish from Infeasible.
    if let Some(UndefCause::SolveFailed { detail }) = causes.get(&x_id) {
        assert!(
            detail.starts_with("no progress:"),
            "NoProgress detail must start with 'no progress:', got: {detail:?}"
        );
    }
}

// ── A1/BT8: transparency lock (step-7) ───────────────────────────────────────

/// Capture ON vs. OFF leaves (Value, DeterminacyState) and content-hash
/// per cell byte-identical.  Also asserts capture is not a silent no-op.
#[test]
fn capture_is_byte_transparent() {
    let module = layer1_module();

    // Run with capture OFF (default).
    let mut engine_off = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_off.eval(&module);
    let snap_off = engine_off.snapshot().expect("snapshot present after eval");

    // Run with capture ON.
    let mut engine_on = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_on.set_capture_undef_causes(true);
    engine_on.eval(&module);
    let snap_on = engine_on.snapshot().expect("snapshot present after eval");

    // (1) Same set of cell ids.
    let ids_off: std::collections::BTreeSet<_> = snap_off.values.keys().cloned().collect();
    let ids_on: std::collections::BTreeSet<_> = snap_on.values.keys().cloned().collect();
    assert_eq!(ids_off, ids_on, "cell id sets must match across capture on/off");

    // (2) & (3) Per-cell (Value, DeterminacyState) and content-hash are equal.
    for id in &ids_off {
        let (val_off, det_off) = snap_off.values.get(id).unwrap();
        let (val_on, det_on) = snap_on.values.get(id).unwrap();
        assert_eq!(
            (val_off, det_off),
            (val_on, det_on),
            "cell {id}: (Value,DeterminacyState) must be identical across capture on/off"
        );
        assert_eq!(
            val_off.content_hash(),
            val_on.content_hash(),
            "cell {id}: content_hash must be identical across capture on/off"
        );
    }

    // (4) EvalResult diagnostics are equal (checked via a second eval).
    // `Diagnostic` does not implement `PartialEq`, so compare via Debug-format
    // strings — this captures all fields (level, message, code, labels) and
    // is sufficient for a transparency regression gate.
    let mut engine_off2 = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result_off = engine_off2.eval(&module);
    let mut engine_on2 = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_on2.set_capture_undef_causes(true);
    let result_on = engine_on2.eval(&module);
    assert_eq!(
        format!("{:?}", result_off.diagnostics),
        format!("{:?}", result_on.diagnostics),
        "EvalResult.diagnostics must be identical across capture on/off"
    );

    // Capture ON is not a silent no-op.
    assert!(
        !engine_on2.undef_causes().is_empty(),
        "capture ON engine must have non-empty undef_causes after eval"
    );
    assert!(
        engine_off2.undef_causes().is_empty(),
        "capture OFF engine must have empty undef_causes after eval"
    );
}
