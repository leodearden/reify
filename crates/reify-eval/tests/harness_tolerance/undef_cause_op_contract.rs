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
    let src = include_str!("../fixtures/undef_cause_op_contract.ri");
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

    let has_unbound_a = traced
        .iter()
        .any(|c| matches!(c, UndefCause::Unbound { param, .. } if param == &a_id));
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

    let has_unbound_a = traced
        .iter()
        .any(|c| matches!(c, UndefCause::Unbound { param, .. } if param == &a_id));
    let has_op_contract = traced
        .iter()
        .any(|c| matches!(c, UndefCause::OpContractFailed { .. }));

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
    assert_eq!(
        ids_on, ids_off,
        "cell id sets must match across capture on/off"
    );

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

// ─────────────────────────────────────────────────────────────────────────────
// A6 ORDERING FLOOR (task 5791, PRD docs/prds/v0_6/dimension-checked-readers.md)
// ─────────────────────────────────────────────────────────────────────────────
//
// BINDING ruling A6 (Leo, 2026-08-30, esc-5791-3): leaf α owns PROVING the
// "a classified cause is never relabelled `OpContractFailed`" ordering with a
// value floor; the exercised specific-cause push (`push_op_contract_failure`
// carrying a code other than `OpContractViolation`) and the `reify-expr` edit
// that produces it belong to leaf β.
//
// These two tests are therefore CHARACTERIZATION tests and are GREEN THE MOMENT
// THEY ARE WRITTEN — deliberately, not accidentally. The ordering they pin is
// already real today; nothing here manufactures a fake RED. Their job is to fail
// LATER, when β changes the push site, if that change collaterally relabels a
// cell α had already classified.
//
// MEASUREMENT behind the design — why this is a characterization floor and not a
// reachability test of the never-overwrite gate itself:
//
//   `record_op_contract_failures` (crates/reify-eval/src/engine_eval.rs:4621) has
//   the INV-SF-1 never-overwrite half as its second gate,
//   `if causes.contains_key(id) { continue; }` (:4637). That gate is NOT
//   reachable from `.ri` source:
//
//     - `AwaitingSolve` / `SolveFailed` both require `det ∈ {Auto, Provisional}`,
//       which implies `kind.is_auto()`, which implies `default_expr: None` —
//       `build_param_value_cell_decl` hard-sets it on the auto branch
//       (crates/reify-compiler/src/entity.rs:6307-6340, pinned by the
//       `build_param_value_cell_decl_auto_branch_skips_compile_default` test at
//       entity.rs:7494-7534). A `None` default_expr is skipped by
//       `record_op_contract_failures`'s THIRD gate anyway (:4646), so those two
//       causes can never reach the `contains_key` check.
//     - `Unbound` is likewise a required param with no default → same third gate.
//     - The one α cause that CAN co-occur with `Some(default_expr)` is
//       `UserUndef` (`param u: Real = undef`), and its re-eval is a
//       `Literal(Value::Undef)` — which never pushes an `OpContractFailed` into
//       the sink, so the loop falls through without inserting.
//
//   Hitting the `contains_key` gate itself would require a synthetic
//   `ValueCellDecl` injection (precedent:
//   crates/reify-eval/tests/boundary5_engine.rs:1008-1034 pushes directly onto
//   `template.value_cells`) and is deliberately out of α's remit.
//
// Both fixtures reused here are already registered in
// `DELIBERATELY_UNDEF_FIXTURES`
// (crates/reify-eval/tests/no_stale_undef_invariant_gate.rs:668-674), which is
// why they were chosen over authoring a new `.ri`: no new fixture, no new
// registry row, no run-all-classification.manifest row, no nextest partition
// entry is due.

/// Helper: load and compile the Layer-1 α-origins fixture.
///
/// Same shape as `layer1_module()` in `undef_cause_capture.rs` — duplicated
/// rather than shared because each `harness_tolerance/*.rs` file is an
/// independent `#[path]` module with no common helper module.
fn layer1_module() -> reify_compiler::CompiledModule {
    let src = include_str!("../fixtures/undef_causes_layer1.ri");
    let m = compile_source_with_stdlib(src);
    let errors = collect_errors(&m.diagnostics);
    assert!(
        errors.is_empty(),
        "undef_causes_layer1.ri should compile without errors: {errors:#?}"
    );
    m
}

/// A6 floor, positive+negative contrast in ONE eval: a cell α classified stays
/// classified, while a genuinely contract-failing sibling in the same module IS
/// `OpContractFailed`.
///
/// `OpContractDemo` gives both halves at once:
///   - `a` (`param a: Real`, no default) is `Unbound` — an α classification, and
///     it must NOT be relabelled `OpContractFailed`. This NEGATIVE half is the
///     assertion BT5 above does not make: BT5 asserts `a` IS `Unbound`, but says
///     nothing about what `a` must never become.
///   - `x` (`let x = a + sqrt(neg)`) IS `OpContractFailed` with
///     `DiagnosticCode::OpContractViolation` — the generic code that leaf β will
///     replace with a specific one.
///
/// When β changes `push_op_contract_failure` (crates/reify-expr/src/lib.rs:3262;
/// call sites :646 and :4282) to carry a classified code, `x`'s code changes and
/// this test's `x` half is expected to be retargeted by β — but the `a` half must
/// keep holding untouched. Any collateral relabelling of an α-classified cell
/// turns this red.
#[test]
fn classified_cause_is_never_relabelled_op_contract_failed() {
    let module = op_contract_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();

    let a_id = ValueCellId::new("OpContractDemo", "a");
    let x_id = ValueCellId::new("OpContractDemo", "x");

    // POSITIVE: `a` is α-classified Unbound.
    assert!(
        matches!(causes.get(&a_id), Some(UndefCause::Unbound { .. })),
        "expected Unbound for 'a', got {:?}",
        causes.get(&a_id)
    );

    // NEGATIVE (the floor BT5 does not assert): `a` must NEVER be
    // OpContractFailed. INV-SF-1's never-overwrite half, observable.
    assert!(
        !matches!(causes.get(&a_id), Some(UndefCause::OpContractFailed { .. })),
        "an α-classified cause must never be relabelled OpContractFailed; \
         'a' got {:?}",
        causes.get(&a_id)
    );

    // CONTRAST, same eval: `x` IS OpContractFailed with the generic code.
    // The two together are the floor — an ordering, not two independent facts.
    assert!(
        matches!(
            causes.get(&x_id),
            Some(UndefCause::OpContractFailed {
                code: DiagnosticCode::OpContractViolation,
                ..
            })
        ),
        "expected OpContractFailed {{ OpContractViolation }} for 'x' in the same \
         eval that leaves 'a' Unbound, got {:?}",
        causes.get(&x_id)
    );
}

/// A6 floor, second α cause family: `UserUndef` and `AwaitingSolve` survive the
/// `record_op_contract_failures` pass unrelabelled.
///
/// `UndefDemo` (`../fixtures/undef_causes_layer1.ri`) carries
/// `param u: Real = undef` (UserUndef — the ONE α cause that co-occurs with
/// `Some(default_expr)`, so it is the only one that even reaches the re-eval
/// loop's body) and `param k: Length = auto` with no solver attached
/// (AwaitingSolve — `default_expr: None`, skipped at the third gate).
///
/// Neither may become `OpContractFailed`. This is the half of the ordering the
/// `OpContractDemo` fixture cannot express: it has no auto param and no
/// undef-literal default.
#[test]
fn user_undef_and_awaiting_solve_survive_the_op_contract_pass() {
    let module = layer1_module();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.set_capture_undef_causes(true);
    engine.eval(&module);

    let causes = engine.undef_causes();

    let u_id = ValueCellId::new("UndefDemo", "u");
    let k_id = ValueCellId::new("UndefDemo", "k");

    // `u` = `param u: Real = undef` → UserUndef, and stays UserUndef.
    assert!(
        matches!(causes.get(&u_id), Some(UndefCause::UserUndef { .. })),
        "expected UserUndef for 'u', got {:?}",
        causes.get(&u_id)
    );
    assert!(
        !matches!(causes.get(&u_id), Some(UndefCause::OpContractFailed { .. })),
        "UserUndef 'u' must never be relabelled OpContractFailed by the \
         record_op_contract_failures pass; got {:?}",
        causes.get(&u_id)
    );

    // `k` = `param k: Length = auto`, no solver → AwaitingSolve, and stays so.
    assert!(
        matches!(causes.get(&k_id), Some(UndefCause::AwaitingSolve { .. })),
        "expected AwaitingSolve for 'k', got {:?}",
        causes.get(&k_id)
    );
    assert!(
        !matches!(causes.get(&k_id), Some(UndefCause::OpContractFailed { .. })),
        "AwaitingSolve 'k' must never be relabelled OpContractFailed by the \
         record_op_contract_failures pass; got {:?}",
        causes.get(&k_id)
    );
}
