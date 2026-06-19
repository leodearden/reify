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

use std::collections::{HashMap, HashSet};

use reify_core::{ValueCellId, VersionId};
use reify_eval::{ConcurrentEditResult, Engine};
use reify_ir::{SolveResult, Value};
use reify_test_support::mocks::{MockConstraintChecker, SequencedMockConstraintSolver};
use reify_test_support::{make_engine, mm, parse_and_compile};

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
        .unwrap_or_else(|| {
            panic!(
                "r should be present in eval_cached values; got {} keys",
                result.eval_result.values.len()
            )
        });
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
    let r_val = result.values.get(&r_id).cloned().unwrap_or_else(|| {
        panic!(
            "r should be present in edit_param result; got {} keys",
            result.values.len()
        )
    });
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
    let r_val = result.values.get(&r_id).cloned().unwrap_or_else(|| {
        panic!(
            "r should be present in edit_source result; got {} keys",
            result.values.len()
        )
    });
    assert_eq!(
        r_val,
        Value::Bool(true),
        "edit_source: determined(x) should be Bool(true) after changing x's default; got {:?}",
        r_val
    );
}

// ── Amendment: eval_cached Param-default closure ───────────────────────────

/// `eval_cached` Param-default-closure DeterminacyPredicate regression.
///
/// The `default_or` closure inside `eval_cached`'s Param branch evaluates the
/// param's `default_expr` using an inline context that carries BOTH
/// `.with_determinacy(&snapshot_values)` AND `.with_runtime_diagnostics(&runtime_sink)`.
/// This test ensures that path stays correct: `param y : Bool = determined(x)`
/// (a param whose default is a DeterminacyPredicate) must return `Bool(true)`
/// when `x` is a param with a concrete default (thus `Determined`).
///
/// Regression guard for the eval_cached Param-default-closure site in engine_eval.rs.
#[test]
fn eval_cached_param_default_resolves_determinacy_predicate() {
    let module = parse_and_compile(
        r#"
        structure S {
            param x : Length = 10mm
            param y : Bool = determined(x)
        }
        "#,
    );
    let mut engine = make_engine();

    let result = engine.eval_cached(&module, VersionId(1));

    let y_id = ValueCellId::new("S", "y");
    let y_val = result
        .eval_result
        .values
        .get(&y_id)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "y should be present in eval_cached values; got {} keys",
                result.eval_result.values.len()
            )
        });
    assert_eq!(
        y_val,
        Value::Bool(true),
        "eval_cached Param-default: param y = determined(x) should be Bool(true) \
         for param x with default 10mm; got {:?}",
        y_val
    );
}

// ── Amendment: concurrent wave-2 ──────────────────────────────────────────

/// Concurrent wave-2 DeterminacyPredicate regression.
///
/// Module: `param a : Length = 3mm`, `param x : Length = auto`,
/// `let r = determined(x)`, `constraint x > a`.
///
/// After cold eval (solver call 1 → x = mm(5.0), `determined(x)` = `Bool(true)`),
/// change `a → mm(8.0)` via `prepare_concurrent_edit`.  `resolve_concurrent_edit`
/// re-runs the solver (call 2 → x = mm(20.0)) in wave-1 and then re-evaluates
/// `r` in wave-2 via `cell_eval_ctx`.  Because `x` is still `Determined` after
/// re-solve, `result.values["r"]` must remain `Bool(true)`.
///
/// Regression guard for the concurrent wave-2 cell-eval site in concurrent.rs.
#[test]
fn concurrent_wave2_resolves_determinacy_predicate() {
    let x_id = ValueCellId::new("S", "x");
    let a_id = ValueCellId::new("S", "a");
    let r_id = ValueCellId::new("S", "r");

    // Solver: call 1 → x = mm(5.0); call 2 (after prepare) → x = mm(20.0).
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    // Module with auto x, let r = determined(x), constraint x > a.
    // The constraint forces the solver to resolve x so x becomes Determined.
    let module = parse_and_compile(
        r#"
        structure S {
            param a : Length = 3mm
            param x : Length = auto
            let r = determined(x)
            constraint x > a
        }
        "#,
    );

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver call 1 → x = mm(5.0); r = Bool(true) (x is Determined).
    let _cold = engine.eval(&module);

    // Prepare concurrent edit: change a from mm(3.0) → mm(8.0).
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), mm(8.0))
        .expect("prepare_concurrent_edit should succeed");

    // Minimal ConcurrentEditResult seeded from setup (no pre-computed node results).
    let mut result = ConcurrentEditResult {
        values: setup.values.clone(),
        snapshot_values: setup.snapshot_values.clone(),
        node_results: vec![],
        actual_eval_set: setup.eval_set.clone(),
        skipped: HashSet::new(),
        resolved_params: HashMap::new(),
        diagnostics: Vec::new(),
    };

    // resolve_concurrent_edit:
    //   wave-1 — constraint dirty → solver call 2 → x = mm(20.0), still Determined
    //   wave-2 — r depends on x → re-evaluated via cell_eval_ctx → Bool(true)
    engine.resolve_concurrent_edit(&setup, &mut result);

    let r_val = result.values.get(&r_id).cloned().unwrap_or_else(|| {
        panic!(
            "r should be present in ConcurrentEditResult values after resolve; \
                 got {} keys",
            result.values.len()
        )
    });
    assert_eq!(
        r_val,
        Value::Bool(true),
        "concurrent wave-2: determined(x) should be Bool(true) after x is re-solved; \
         got {:?}",
        r_val
    );
}
