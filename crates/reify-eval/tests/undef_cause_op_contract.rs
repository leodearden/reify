//! Integration tests for task 4323 (PRD undef-self-describing γ):
//! op/builtin contract-failure reason sink — `record_op_contract_failures`.
//!
//! Signal assertions (real Engine::eval path, not synthetic snapshot construction):
//! - BT5: `x = a + sqrt(neg)` with `a` unbound ⇒ tracer returns BOTH
//!   `a:Unbound` AND a sqrt-domain `OpContractFailed` via the side-map.
//! - BT6: `y = sqrt(a)` with `a` unbound ⇒ tracer returns ONLY `a:Unbound`;
//!   NO false `OpContractFailed` (undef-arg short-circuit fires before builtin).
//! - Determined control: `ok = sqrt(neg + 5.0)` ⇒ `undef_causes().get(ok)` is None.
//! - G3/transparency: capture OFF ⇒ byte-identical (Value, DeterminacyState) per cell
//!   AND empty undef_causes().
//!
//! RED until step-6 adds `record_op_contract_failures` to engine_eval.rs.

use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::UndefCause;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

// ── Helper: load and compile the γ fixture ────────────────────────────────────

fn op_contract_module() -> reify_compiler::CompiledModule {
    let src = include_str!("fixtures/undef_cause_op_contract.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_cause_op_contract.ri should compile without errors: {errors:#?}"
    );
    m
}

// ── BT5: x records OpContractFailed AND tracer collects both causes ────────────

/// Cell `x = a + sqrt(neg)` with `a` unbound and `neg` determined negative:
///
/// 1. The side-map records `OpContractFailed { code: OpContractViolation }` for `x`
///    (γ's `record_op_contract_failures` re-evals `x`'s expr with a sink; the
///    determined-input sqrt domain failure is the genuine cause).
/// 2. `trace_undef_causes(x)` returns BOTH a `UndefCause::Unbound { param: a }` (via
///    the dep-walk from `a`'s side-map entry) AND the `OpContractFailed` from `x`'s
///    own entry — the tracer walks cell `x` itself first, then its dep edges.
///
/// RED: step-6 (`record_op_contract_failures`) is not yet wired — `x` lacks an
/// `OpContractFailed` entry in the side-map.
#[test]
fn bt5_x_has_op_contract_failed_in_side_map_and_both_causes_in_tracer() {
    let module = op_contract_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();

    let a_id = ValueCellId::new("OpContractDemo", "a");
    let x_id = ValueCellId::new("OpContractDemo", "x");

    // Side-map: `a` must be recorded as Unbound (α).
    assert!(
        matches!(causes.get(&a_id), Some(UndefCause::Unbound { .. })),
        "expected Unbound for 'a', got {:?}",
        causes.get(&a_id)
    );

    // Side-map: `x` must be recorded as OpContractFailed (γ) with the
    // OpContractViolation diagnostic code.
    assert!(
        matches!(
            causes.get(&x_id),
            Some(UndefCause::OpContractFailed {
                code: DiagnosticCode::OpContractViolation,
                ..
            })
        ),
        "expected OpContractFailed {{ OpContractViolation }} for 'x', got {:?}",
        causes.get(&x_id)
    );

    // Tracer: walking from `x` must return BOTH a:Unbound AND an OpContractFailed.
    let traced = engine.trace_undef_causes(&x_id);

    let has_unbound_a = traced.iter().any(|c| {
        matches!(c, UndefCause::Unbound { param, .. } if param == &a_id)
    });
    let has_op_contract = traced.iter().any(|c| {
        matches!(
            c,
            UndefCause::OpContractFailed {
                code: DiagnosticCode::OpContractViolation,
                ..
            }
        )
    });

    assert!(
        has_unbound_a,
        "trace_undef_causes(x) must contain Unbound {{ param: a }}, got {traced:?}"
    );
    assert!(
        has_op_contract,
        "trace_undef_causes(x) must contain OpContractFailed {{ OpContractViolation }}, got {traced:?}"
    );
}

// ── BT6: y has NO false OpContractFailed ─────────────────────────────────────

/// Cell `y = sqrt(a)` with `a` unbound:
///
/// The strict undef-arg short-circuit (lib.rs:242) fires BEFORE `eval_builtin` is
/// called, so no `OpContractFailed` is ever pushed — the no-false-attribution
/// guarantee falls out of the existing short-circuit structure.
///
/// 1. `undef_causes().get(y)` must be `None` (γ's re-eval short-circuits too).
/// 2. `trace_undef_causes(y)` must contain ONLY `a:Unbound` and NO `OpContractFailed`.
#[test]
fn bt6_y_has_no_false_op_contract_failed() {
    let module = op_contract_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();

    let a_id = ValueCellId::new("OpContractDemo", "a");
    let y_id = ValueCellId::new("OpContractDemo", "y");

    // Side-map: `y` must have NO entry (purely propagated — re-eval hits the
    // undef-arg short-circuit before any OpContractFailed can be pushed).
    assert!(
        causes.get(&y_id).is_none(),
        "y must have no side-map entry (purely propagated via undef-arg short-circuit), got {:?}",
        causes.get(&y_id)
    );

    // Tracer: must contain only a's Unbound, no OpContractFailed.
    let traced = engine.trace_undef_causes(&y_id);

    let has_unbound_a = traced.iter().any(|c| {
        matches!(c, UndefCause::Unbound { param, .. } if param == &a_id)
    });
    let has_op_contract = traced.iter().any(|c| {
        matches!(c, UndefCause::OpContractFailed { .. })
    });

    assert!(
        has_unbound_a,
        "trace_undef_causes(y) must contain Unbound {{ param: a }}, got {traced:?}"
    );
    assert!(
        !has_op_contract,
        "trace_undef_causes(y) must NOT contain any OpContractFailed (BT6), got {traced:?}"
    );
}

// ── Determined control: ok has no cause ───────────────────────────────────────

/// Cell `ok = sqrt(neg + 5.0)` evaluates to `Real(2.0)` (determined).
/// The side-map must not record any cause for it.
#[test]
fn determined_control_ok_has_no_cause() {
    let module = op_contract_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();
    let ok_id = ValueCellId::new("OpContractDemo", "ok");

    assert!(
        causes.get(&ok_id).is_none(),
        "determined cell 'ok' must have no cause, got {:?}",
        causes.get(&ok_id)
    );
}

// ── G3/Transparency: capture OFF ⇒ byte-identical per cell ───────────────────

/// A second engine with capture OFF produces byte-identical (Value, DeterminacyState)
/// for every cell, and `undef_causes()` is empty.
///
/// This asserts A1/G3 structurally: the re-eval pass is read-only on snapshot.values
/// and the push-sites are no-ops when no sink is attached.
#[test]
fn g3_transparency_capture_off_is_byte_identical() {
    let module = op_contract_module();

    // Engine with capture ON.
    let mut engine_on = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_on.set_capture_undef_causes(true);
    engine_on.eval(&module);
    let snap_on = engine_on.snapshot().expect("snapshot present after eval");

    // Engine with capture OFF.
    let mut engine_off = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine_off.eval(&module);
    let snap_off = engine_off.snapshot().expect("snapshot present after eval");

    // Same set of cell ids.
    let ids_on: std::collections::BTreeSet<_> = snap_on.values.keys().cloned().collect();
    let ids_off: std::collections::BTreeSet<_> = snap_off.values.keys().cloned().collect();
    assert_eq!(ids_on, ids_off, "cell id sets must match across capture on/off");

    // Per-cell (Value, DeterminacyState) must be byte-identical.
    for id in &ids_on {
        let (val_on, det_on) = snap_on.values.get(id).unwrap();
        let (val_off, det_off) = snap_off.values.get(id).unwrap();
        assert_eq!(
            (val_on, det_on),
            (val_off, det_off),
            "cell {id}: (Value,DeterminacyState) must be identical across capture on/off"
        );
    }

    // Capture OFF must have empty undef_causes.
    assert!(
        engine_off.undef_causes().is_empty(),
        "capture OFF engine must return empty undef_causes"
    );

    // Capture ON must have non-empty undef_causes (not a silent no-op).
    assert!(
        !engine_on.undef_causes().is_empty(),
        "capture ON engine must have non-empty undef_causes after eval"
    );
}
