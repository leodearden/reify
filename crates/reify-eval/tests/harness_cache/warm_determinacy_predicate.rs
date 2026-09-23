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
//! The main-loop tests below use a plain NON-guard readable `let r =
//! determined(x)` so they traverse the bare main-loop site (not the
//! guard-re-elaboration phase that already rescues guard cells with
//! `.with_determinacy`). A further site sits downstream of the main loop:
//! edit_source's post-solve second propagation wave, reached only through a
//! solver-resolved `auto` param — covered separately below (task #7114).
//!
//! Task 4356: cell_eval_ctx determinacy unification.

use reify_constraints::DimensionalSolver;
use reify_core::{ValueCellId, VersionId};
use reify_ir::Value;
use reify_test_support::{make_engine, make_simple_engine, parse_and_compile};

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

// ── Solver wave-2: edit_param vs edit_source (task #7114) ────────────────────

/// Pre-edit solver fixture: `base = 3mm` ⇒ `x == 5mm`. `ready`/`gated` read
/// only `x`, never `base`, so they sit outside `base`'s dirty cone — the
/// post-solve second propagation wave is the only phase that re-evaluates
/// them on either edit surface.
const SOLVER_WAVE2_BASE3_SRC: &str = r#"
    structure S {
        param base : Length = 3mm
        param x : Length = auto
        constraint x == base + 2mm
        let ready = determined(x)
        let gated = if determined(x) then x else 0mm
    }
"#;

/// Same structure/cell IDs as [`SOLVER_WAVE2_BASE3_SRC`]; only `base`'s
/// default changes, 3mm → 7mm, so the solver re-resolves `x` to 9mm.
const SOLVER_WAVE2_BASE7_SRC: &str = r#"
    structure S {
        param base : Length = 7mm
        param x : Length = auto
        constraint x == base + 2mm
        let ready = determined(x)
        let gated = if determined(x) then x else 0mm
    }
"#;

/// Absolute tolerance for `gated`'s expected 9mm (0.009 m) — a Nelder-Mead
/// solver-search output, not an exact literal. Same rationale as
/// `MOVED_AUTO_TOL` in engine_edit.rs's `edit_param_back_props_moved_auto`:
/// the measured error is ~5.6e-16 m on all three paths below, ~9 orders of
/// magnitude inside this bound, while the stale pre-edit value (5mm) is
/// 4e-3 m away and `Undef` fails the pattern outright.
const SOLVER_TOL_M: f64 = 1e-6;

/// A `SimpleConstraintChecker` + `DimensionalSolver` engine, no geometry
/// kernel — used by all three legs (cold / edit_param / edit_source) below.
fn solver_engine() -> reify_eval::Engine {
    make_simple_engine().with_solver(Box::new(DimensionalSolver))
}

/// A solver-driven `determined(x)` / `if determined(x) then x else 0mm` must
/// resolve identically whether `x` was re-resolved via `edit_param`,
/// `edit_source`, or a cold `eval()` of the post-edit source. `ready`/`gated`
/// are outside `base`'s dirty cone (see [`SOLVER_WAVE2_BASE3_SRC`]), so the
/// post-solve second propagation wave is the only phase that re-evaluates
/// them — this pins that wave's context on both edit surfaces, not just the
/// main walk covered above.
///
/// RED today: edit_source's second propagation wave omits
/// `.with_determinacy`, so `ready`/`gated` evaluate to `Undef` there while
/// `edit_param` and cold both resolve to `Bool(true)` / 9mm.
#[test]
fn solver_wave2_resolves_determinacy_predicate_on_both_edit_surfaces() {
    let pre = parse_and_compile(SOLVER_WAVE2_BASE3_SRC);
    let post = parse_and_compile(SOLVER_WAVE2_BASE7_SRC);

    let base_id = ValueCellId::new("S", "base");
    let x_id = ValueCellId::new("S", "x");
    let ready_id = ValueCellId::new("S", "ready");
    let gated_id = ValueCellId::new("S", "gated");

    let mut param_engine = solver_engine();
    param_engine.eval(&pre);
    let edit_param_result = param_engine
        .edit_param(base_id, Value::length(0.007))
        .expect("edit_param should succeed");

    let mut source_engine = solver_engine();
    source_engine.eval(&pre);
    let edit_source_result = source_engine
        .edit_source(&post)
        .expect("edit_source should succeed");

    let mut cold_engine = solver_engine();
    let cold_result = cold_engine.eval(&post);

    // GUARD: both edits must actually reach the solver, or the fixture
    // proves nothing about wave2 — a miss means base's edit stopped dirtying
    // the constraint, not that #7114's determinacy bug is fixed.
    assert!(
        edit_param_result.resolved_params.contains_key(&x_id),
        "edit_param: x should be in resolved_params (solver must re-resolve x)"
    );
    assert!(
        edit_source_result.resolved_params.contains_key(&x_id),
        "edit_source: x should be in resolved_params (solver must re-resolve x)"
    );

    for (label, result) in [
        ("cold", &cold_result),
        ("edit_param", &edit_param_result),
        ("edit_source", &edit_source_result),
    ] {
        assert_eq!(
            result.values.get(&ready_id),
            Some(&Value::Bool(true)),
            "{label}: ready = determined(x) should be Bool(true); got {:?}",
            result.values.get(&ready_id)
        );

        match result.values.get(&gated_id) {
            Some(Value::Scalar { si_value, .. }) => {
                assert!(
                    (si_value - 0.009).abs() < SOLVER_TOL_M,
                    "{label}: gated should be within {:.0e}m of 9mm (0.009m); \
                     got {si_value}m",
                    SOLVER_TOL_M
                );
            }
            other => panic!("{label}: gated should be a Length Scalar (~9mm); got {other:?}"),
        }
    }
}
