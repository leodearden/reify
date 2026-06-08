//! Regression tests for warm/edit DeterminacyPredicate cells returning `Undef`
//! instead of `Bool`.
//!
//! Root cause: the five bare eval sites (eval_cached Let branch, eval_cached
//! Param-default closure, edit_param Let main loop, edit_source Let main loop,
//! concurrent wave-2) omit `.with_determinacy(snapshot_values)` from the
//! EvalContext they build.  Any `determined(x)` / `undetermined(x)` / etc.
//! evaluated through those sites collapses to `Value::Undef` because the
//! `DeterminacyPredicate` eval arm returns `Undef` when no determinacy map is
//! present.
//!
//! These tests use a plain NON-guard readable `let r = determined(x)` so they
//! traverse the bare main-loop site (not the guard-re-elaboration phase that
//! already rescues guard cells with `.with_determinacy`).
//!
//! Task 4356: cell_eval_ctx determinacy unification.

use reify_core::{ValueCellId, VersionId};
use reify_ir::Value;
use reify_test_support::{make_engine, parse_and_compile};

/// Source shared across all three warm tests:
///   param x  : Length = 10mm
///   let  r   = determined(x)
/// Cold eval returns Bool(true) because x has a default → Determined.
/// Warm/incremental paths must also return Bool(true) after task-4356 fix.
const SRC_V1: &str = r#"
    structure S {
        param x : Length = 10mm
        let r = determined(x)
    }
"#;

/// Source v2 for edit_source: x's default changed to 20mm so r ends up in the
/// dirty cone and is re-evaluated through the warm site.
const SRC_V2: &str = r#"
    structure S {
        param x : Length = 20mm
        let r = determined(x)
    }
"#;

// ── Step 1: eval_cached warm site ─────────────────────────────────────────────

/// `eval_cached` warm-path DeterminacyPredicate.
///
/// RED today: the Let branch in eval_cached uses a bare
/// `eval_ctx_with_meta(...)` (no `.with_determinacy`), so `determined(x)`
/// evaluates to `Value::Undef` instead of `Bool(true)`.
///
/// GREEN after step-2: cell_eval_ctx threads `.with_determinacy(snapshot_values)`.
#[test]
fn eval_cached_resolves_determinacy_predicate() {
    let module = parse_and_compile(SRC_V1);
    let mut engine = make_engine();

    let result = engine.eval_cached(&module, VersionId(1));

    let r_id = ValueCellId::new("S", "r");
    let r_val = result
        .eval_result
        .values
        .get(&r_id)
        .cloned()
        .unwrap_or_else(|| panic!("r should be present in eval_cached values; got {} keys",
                                  result.eval_result.values.len()));
    assert_eq!(
        r_val,
        Value::Bool(true),
        "eval_cached: determined(x) should be Bool(true) for param x with default 10mm; got {:?}",
        r_val
    );
}

// ── Step 3: edit_param warm site ──────────────────────────────────────────────

/// `edit_param` warm-path DeterminacyPredicate.
///
/// RED today: the edit_param top-level Let main-loop site uses bare
/// `eval_ctx_with_meta(...).with_runtime_diagnostics(&runtime_sink)` (no
/// `.with_determinacy`), so after editing x, r evaluates to `Value::Undef`.
///
/// GREEN after step-4: cell_eval_ctx threads `.with_determinacy(new_snapshot.values)`.
#[test]
fn edit_param_resolves_determinacy_predicate() {
    let module = parse_and_compile(SRC_V1);
    let mut engine = make_engine();

    // Cold eval — r should be Bool(true) from the cold path.
    engine.eval(&module);

    // Edit x (determined(x) registers a read of x, so x→r is a real edge).
    let x_id = ValueCellId::new("S", "x");
    let result = engine
        .edit_param(x_id, Value::length(0.02))
        .expect("edit_param should succeed");

    let r_id = ValueCellId::new("S", "r");
    let r_val = result
        .values
        .get(&r_id)
        .cloned()
        .unwrap_or_else(|| panic!("r should be present in edit_param result; got {} keys",
                                  result.values.len()));
    assert_eq!(
        r_val,
        Value::Bool(true),
        "edit_param: determined(x) should be Bool(true) after editing x; got {:?}",
        r_val
    );
}

// ── Step 5: edit_source warm site ─────────────────────────────────────────────

/// `edit_source` warm-path DeterminacyPredicate.
///
/// RED today: the edit_source top-level Let main-loop site uses the same bare
/// ctx as edit_param, so r evaluates to `Value::Undef` when x's default
/// changes and forces r into the dirty cone.
///
/// GREEN after step-6: cell_eval_ctx threads `.with_determinacy(new_snapshot.values)`.
#[test]
fn edit_source_resolves_determinacy_predicate() {
    let module_v1 = parse_and_compile(SRC_V1);
    let module_v2 = parse_and_compile(SRC_V2);
    let mut engine = make_engine();

    // Cold eval on v1.
    engine.eval(&module_v1);

    // Edit source: x's default changes from 10mm → 20mm.
    // r depends on x, so r is in the dirty cone and re-evaluates.
    let result = engine
        .edit_source(&module_v2)
        .expect("edit_source should succeed");

    let r_id = ValueCellId::new("S", "r");
    let r_val = result
        .values
        .get(&r_id)
        .cloned()
        .unwrap_or_else(|| panic!("r should be present in edit_source result; got {} keys",
                                  result.values.len()));
    assert_eq!(
        r_val,
        Value::Bool(true),
        "edit_source: determined(x) should be Bool(true) after changing x's default; got {:?}",
        r_val
    );
}
