//! R3d dependency-ordered selector/handle resolution tests (task #4900).
//!
//! Pins the TIMING/ORDERING axis fix: value-eval consumer cells that reference
//! a topology selector (`loc`) must see a resolved `Value::Selector` at READ
//! time — not `Value::Undef` — through `Engine::eval`, `eval_cached`, and
//! `engine_edit`.
//!
//! ## TDD arc
//!
//! **Step-1 (RED):** `value_eval_consumer_reads_minted_selector_finite_eval` —
//! asserts that `Widget.n_top` (a downstream consumer reading `loc`) is NOT
//! `Value::Undef` after `Engine::eval`.  FAILS until step-2 wires the
//! dependency-ordered interleave into `evaluate_params_and_lets_unified`.
//!
//! **Step-3 (RED):** `value_eval_consumer_reads_minted_selector_finite_eval_cached` —
//! same fixture via `eval_cached`.  FAILS until step-4.
//!
//! **Step-5 (RED):** `value_eval_consumer_reads_minted_selector_finite_after_edit` —
//! same fixture via `engine_edit`.  FAILS until step-6.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::VersionId;
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::compile_source_with_stdlib;

/// Fixture: Widget with a NAMED `body` param (Solid = box) + let-bound dir/tol
/// + topology selector `loc = faces_by_normal(body, dir, tol)` + downstream
///   consumer `n_top = loc`.
///
/// Using a NAMED `body` param exercises BOTH relocations transitively:
/// `body`'s GeometryHandle must be minted in-walk BEFORE `loc` can resolve
/// its target from `values`, which must happen BEFORE `n_top` reads `loc`.
const WIDGET_SRC: &str = r#"structure def Widget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let loc = faces_by_normal(body, dir, tol)
    let n_top = loc
}"#;

fn assert_no_compile_errors(compiled: &reify_compiler::CompiledModule) {
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no compile-time errors; got: {:#?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `Engine::eval` (kernel-free, no build) must yield a non-Undef value for
/// `Widget.n_top` — the downstream consumer of `Widget.loc`.
///
/// **RED** until step-2 wires the dependency-ordered interleave: currently
/// `loc` is minted AFTER the topo walk, so `n_top` reads `Value::Undef` and
/// is never re-evaluated.
#[test]
fn value_eval_consumer_reads_minted_selector_finite_eval() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("Widget", "n_top");
    let value = result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "Widget.n_top must NOT be Value::Undef after Engine::eval — \
         the in-walk mint must resolve `loc` before `n_top` reads it; \
         got: {value:?}"
    );
}

/// `Engine::eval_cached` (kernel-free, incremental path) must yield a
/// non-Undef value for `Widget.n_top`.
///
/// **RED** until step-4 wires the interleave into `eval_cached`'s own topo
/// walk (distinct from `evaluate_params_and_lets_unified`).
#[test]
fn value_eval_consumer_reads_minted_selector_finite_eval_cached() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval_cached(&compiled, VersionId(1));

    let cell_id = ValueCellId::new("Widget", "n_top");
    let value = result.eval_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "Widget.n_top must NOT be Value::Undef after Engine::eval_cached — \
         the in-walk mint must resolve `loc` before `n_top` reads it; \
         got: {value:?}"
    );
}

/// `engine_edit` (incremental re-eval after param edit) must yield a non-Undef
/// value for `Widget.n_top`.
///
/// **RED** until step-6 wires the interleave into the engine_edit reeval walk.
#[test]
fn value_eval_consumer_reads_minted_selector_finite_after_edit() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    // Establish baseline.
    engine.eval(&compiled);

    // Edit a param to trigger incremental re-eval.
    let width_id = ValueCellId::new("Widget", "width");
    let edit_result = engine
        .edit_param(width_id, Value::length(0.012))
        .expect("edit_param must succeed after eval");

    let cell_id = ValueCellId::new("Widget", "n_top");
    let value = edit_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "Widget.n_top must NOT be Value::Undef after engine_edit — \
         the in-walk mint must resolve `loc` before `n_top` reads it; \
         got: {value:?}"
    );
}
