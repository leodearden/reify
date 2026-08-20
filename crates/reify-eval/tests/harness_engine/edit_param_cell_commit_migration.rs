//! Task δ (#5056) step-7: end-to-end done-criteria fixtures for migrating
//! `engine_edit.rs`'s `edit_param` eval-and-commit transaction copies onto
//! the α primitive (`commit_cell_result`, `crate::cell_commit`) + the β
//! required-args free-fn (`cell_eval_ctx`, `crate::cell_eval_ctx`).
//!
//! Two named done-criteria, both RED on base — GREEN once step-8 migrates
//! `edit_param`'s remaining eval-ctx constructions onto the free-fn,
//! threading determinacy + runtime_sink + containment identically to the
//! cold `eval()` path:
//!
//! - Fixture A (`warm_edit_commits_identical_determinacy_to_cold_eval`, B2):
//!   edit-path re-eval must commit an IDENTICAL `(value, DeterminacyState)`
//!   pair to what a cold `eval()` of the same final state commits, for a
//!   cell whose expr is a `DeterminacyPredicate` reached through the wave2
//!   solver-reseed ctx (site "1624" in the task's site inventory), which
//!   currently omits `.with_determinacy`.
//! - Fixture B (`edit_path_reeval_surfaces_dropped_field_oob_warning`): a
//!   guarded-group ACTIVE member that samples a field out of bounds,
//!   reached through the BARE ctx site (U1, post-wave2 active-member
//!   reseed) that wires no runtime sink at all — the `W_FIELD_OUT_OF_BOUNDS`
//!   warning must surface in `edit_param(...).diagnostics`, not be silently
//!   dropped.
//!
//! Fixture C (`warm_collection_grow_commits_identical_determinacy_to_cold_eval`,
//! reviewer amend round 2 — test-coverage gap): extends the same
//! identical-to-cold-eval pattern as Fixture A to the collection-resize-child
//! (U2) and grown-instance-reseed (U4) ctx sites, which step-8 also migrated
//! onto the required-args free-fn but which had no dedicated behavioral
//! fixture proving the newly-threaded determinacy (read from the
//! partially-updated `new_snapshot.values` map, mutated within the same
//! reseed loop) doesn't diverge from cold eval on a grown collection member.
//! Already GREEN (step-8 landed before this fixture was added) — a
//! characterization test pinning the convergence, not a new RED/GREEN pair.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{compile_source, make_simple_engine, parse_and_compile_with_stdlib};

// ─────────────────────────────────────────────────────────────────────────
// Fixture A — warm edit commits identical (value, determinacy) to cold eval.
// ─────────────────────────────────────────────────────────────────────────

/// `x` is a solver-resolved `auto` param (`constraint x == 10mm`); `y =
/// determined(x)` is a `DeterminacyPredicate` over `x`. Cold `eval()`
/// resolves `x` (via the solver, since it's dirty on a fresh start) and then
/// evaluates `y` through `engine_eval.rs`'s main unified pass — which DOES
/// carry a determinacy map — so cold `y` is `Bool(true)`.
///
/// Editing `x` to exactly its already-resolved value (`10mm`) re-triggers
/// the SAME solver-resolved-auto flow inside `edit_param`: the `Solved` arm
/// fires (T4, early-exit — no search needed) and then the wave2 schedule
/// re-evaluates dependents of `x`, including `y` (the "1624" site). That
/// site's ctx is sink-only (`eval_ctx_with_meta(..).with_runtime_diagnostics(..)`,
/// no `.with_determinacy`), so `determined(x)` resolves to `Value::Undef`
/// there — even though `x` itself is unchanged and still `Determined`.
const SRC: &str = r#"structure WarmDetPredCommit {
    param x : Length = auto
    constraint x == 10mm
    let y = determined(x)
}"#;

/// Fixture A (done-criterion 1, "B2"): warm `edit_param` must commit the
/// IDENTICAL `(value, DeterminacyState)` pair a cold `eval()` of the same
/// final state commits, for a cell reached only via the edit path's wave2
/// solver-reseed ctx.
///
/// RED on base: cold eval's `y` is `(Bool(true), Determined)`. Re-affirming
/// `x` at its already-solved value via `edit_param` should be a value-level
/// no-op for every dependent, but today it corrupts `y` to
/// `(Value::Undef, Determined)` — the VALUE diverges from the cold
/// reference purely because the wave2 reseed ctx omits `.with_determinacy`,
/// even though `commit_cell_result`'s `UnconditionalDetermined` rule still
/// stamps `Determined` on both paths regardless. GREEN once step-8 threads
/// `.with_determinacy(&new_snapshot.values)` into that ctx (via the
/// `cell_eval_ctx` free-fn's required determinacy arg), converging the
/// VALUE too.
#[test]
fn warm_edit_commits_identical_determinacy_to_cold_eval() {
    let x_id = ValueCellId::new("WarmDetPredCommit", "x");
    let y_id = ValueCellId::new("WarmDetPredCommit", "y");

    // Cold reference: a FRESH engine's eval() resolves x via the solver and
    // evaluates y through the main unified pass — ground truth.
    let cold_compiled = compile_source(SRC);
    let mut cold_engine = Engine::new(Box::new(SimpleConstraintChecker), None)
        .with_solver(Box::new(DimensionalSolver));
    let cold_result = cold_engine.eval(&cold_compiled);
    let cold_val = cold_result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("cold eval: y must be present"));
    let cold_snapshot = cold_engine
        .snapshot()
        .expect("cold engine must have a snapshot after eval");
    let (_, cold_det) = cold_snapshot
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("cold snapshot: y must be present"));
    assert_eq!(
        *cold_val,
        Value::Bool(true),
        "precondition: cold eval's y = determined(x) must be Bool(true) \
         (x resolves to a concrete value via the solver on cold start)"
    );

    // Warm: a SEPARATE engine, cold-eval'd identically, then edit_param
    // re-affirms x at its already-solved value (10mm) — triggering the
    // solver's early-exit (T4) and the wave2 dependent reseed of y (the
    // "1624" site under test).
    let warm_compiled = compile_source(SRC);
    let mut warm_engine =
        Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(DimensionalSolver));
    warm_engine.eval(&warm_compiled);
    let warm_result = warm_engine
        .edit_param(x_id, Value::length(0.01))
        .expect("edit_param(x, 10mm) must succeed after a cold eval");
    let warm_val = warm_result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("warm edit_param: y must be present"));
    let warm_snapshot = warm_engine
        .snapshot()
        .expect("warm engine must have a snapshot after edit_param");
    let (_, warm_det) = warm_snapshot
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("warm snapshot: y must be present"));

    assert_eq!(
        warm_val, cold_val,
        "warm edit_param's y value must match cold eval's \
         (determined(x) must resolve identically on both paths); \
         warm={warm_val:?}, cold={cold_val:?}"
    );
    assert_eq!(
        *warm_det, *cold_det,
        "warm edit_param's y determinacy must match cold eval's; \
         warm={warm_det:?}, cold={cold_det:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture B — edit-path reseed surfaces a previously-dropped runtime warning.
// ─────────────────────────────────────────────────────────────────────────

/// A guarded group whose ACTIVE member samples a field `f` out of its
/// `[0.0m, 1.0m]` bounds (query point `5.0m`). `cond` starts `false` (the
/// member is inactive — never evaluated, so no warning at cold-eval time);
/// `edit_param(cond, true)` activates it via the post-wave2 active-member
/// reseed (U1), which today uses a BARE ctx (no `.with_runtime_diagnostics`
/// at all), so any diagnostic `sample()` pushes never reaches
/// `EvalResult::diagnostics`.
const OOB_GUARD_SRC: &str = r#"
field def f : Real -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(1.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [0.0, 1.0] } }

structure S {
    param cond : Bool = false
    where cond {
        let oob = sample(f, 5.0m)
    }
}
"#;

/// Fixture B (done-criterion 2): the `W_FIELD_OUT_OF_BOUNDS` warning that
/// `sample(f, 5.0m)` emits (query point `5.0m` is outside the field's
/// `[0.0m, 1.0m]` bounds) must surface in `edit_param(...).diagnostics`.
///
/// RED on base: `oob`'s default_expr is evaluated through the bare ctx at
/// the post-wave2 active-member reseed site (U1) — no runtime sink is
/// wired there, so the warning `sample()` pushes onto its `EvalContext` is
/// discarded, never reaching the diagnostics drain (engine_edit.rs's
/// `runtime_sink.borrow_mut()` append near the end of `edit_param`). GREEN
/// once step-8 threads `.with_runtime_diagnostics(&runtime_sink)` into that
/// ctx (via the `cell_eval_ctx` free-fn's required sink arg).
#[test]
fn edit_path_reeval_surfaces_dropped_field_oob_warning() {
    let compiled = parse_and_compile_with_stdlib(OOB_GUARD_SRC);
    let mut engine = make_simple_engine();

    // Cold eval: cond=false, oob is an inactive guarded member (Undef,
    // never evaluated) — no OOB warning yet.
    engine.eval(&compiled);

    let cond_id = ValueCellId::new("S", "cond");
    let result = engine
        .edit_param(cond_id, Value::Bool(true))
        .expect("edit_param(cond, true) must succeed");

    let oob_warnings: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.code == Some(DiagnosticCode::FieldOutOfBounds)
        })
        .collect();
    assert_eq!(
        oob_warnings.len(),
        1,
        "edit_param must surface the W_FIELD_OUT_OF_BOUNDS warning from the \
         newly-activated guarded member's sample() call, got diagnostics: {:?}",
        result.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Fixture C — collection-resize / grown-instance reseed determinacy parity.
// ─────────────────────────────────────────────────────────────────────────

/// `Bolt.is_diameter_determined = determined(diameter)` is a
/// `DeterminacyPredicate` over a plain, always-`Determined` sibling param.
/// Per `reify_expr::eval_expr`'s `DeterminacyPredicate` arm, `determined(_)`
/// resolves to `Value::Undef` whenever its `EvalContext` was built WITHOUT
/// `.with_determinacy` — regardless of whether the referenced cell is
/// genuinely determined — which makes it a precise probe for the *capability*
/// threaded into a `cell_eval_ctx` call, not merely the value it produces.
///
/// `S.bolts[i].diameter` is committed by the collection-resize-child ctx
/// (U2, `engine_edit.rs`'s collection create-loop) and then, since a fresh
/// engine's demand defaults to full-scope (every node demanded), immediately
/// re-processed by the grown-instance reseed ctx (U4, the post-resize
/// `grown_seed` pass) — U4 runs after U2 in the same `edit_param` call and is
/// therefore the final writer for `S.bolts[i].is_diameter_determined`. Both
/// sites build their `EvalContext` via the identical `cell_eval_ctx(&values,
/// &functions, &self.meta_map, &new_snapshot.values, &runtime_sink, self)`
/// free-fn call (post step-8), so this fixture's observed final state is
/// authoritative for U4's threading and, via the shared call shape,
/// characterizes U2's identically-built ctx too.
const COLLECTION_GROW_SRC_N2: &str = r#"structure Bolt {
    param diameter : Length = 10mm
    let is_diameter_determined = determined(diameter)
}
structure S {
    param n : Int = 2
    sub bolts : List<Bolt>
    constraint bolts.count == n
}"#;

/// Same module, but starting (and cold-eval'd) at `n = 4` directly — the
/// warm fixture's target final state, used as the cold reference.
const COLLECTION_GROW_SRC_N4: &str = r#"structure Bolt {
    param diameter : Length = 10mm
    let is_diameter_determined = determined(diameter)
}
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}"#;

/// Fixture C (test-coverage amendment, reviewer round 2): pins that growing a
/// collection via `edit_param` commits the SAME `(value, determinacy)` a cold
/// `eval()` of the equivalent final state commits, for a grown member's
/// `DeterminacyPredicate` cell — closing the coverage gap flagged for the
/// collection-resize-child (U2) and grown-instance-reseed (U4) sites, which
/// (unlike Fixture A's wave2-reseed site) had no dedicated fixture proving
/// their newly-threaded determinacy capability doesn't diverge from cold eval.
///
/// Before step-8, both U2 and U4 built their ctx via
/// `eval_ctx_with_meta(..).with_runtime_diagnostics(..)` only (sink-only, no
/// `.with_determinacy`), so `is_diameter_determined` would have committed
/// `Value::Undef` instead of `Bool(true)` for a grown instance — silently
/// diverging from cold eval. GREEN today (step-8 already migrated both
/// sites onto the free-fn); this is a characterization test guarding that
/// convergence, not a new RED/GREEN pair.
#[test]
fn warm_collection_grow_commits_identical_determinacy_to_cold_eval() {
    // bolts[3] only exists post-grow (n=2 -> n=4), so it necessarily routes
    // through U2 (creation) and U4 (grown-instance reseed) on the warm path.
    let target_id = ValueCellId::new("S.bolts[3]", "is_diameter_determined");

    // Cold reference: a FRESH engine's eval() of the N=4 module directly —
    // bolts[3] is created and evaluated entirely within engine_eval.rs's
    // main pass. Ground truth.
    let cold_compiled = compile_source(COLLECTION_GROW_SRC_N4);
    let mut cold_engine = make_simple_engine();
    let cold_result = cold_engine.eval(&cold_compiled);
    let cold_val = cold_result
        .values
        .get(&target_id)
        .unwrap_or_else(|| panic!("cold eval: bolts[3].is_diameter_determined must be present"));
    let cold_snapshot = cold_engine
        .snapshot()
        .expect("cold engine must have a snapshot after eval");
    let (_, cold_det) = cold_snapshot
        .values
        .get(&target_id)
        .unwrap_or_else(|| panic!("cold snapshot: bolts[3].is_diameter_determined must be present"));
    assert_eq!(
        *cold_val,
        Value::Bool(true),
        "precondition: cold eval's is_diameter_determined must be Bool(true) \
         (diameter is a plain Determined param)"
    );

    // Warm: a SEPARATE engine evaluated at n=2, then edit_param grows n to 4
    // — creating bolts[2] and bolts[3] via the collection-resize-child ctx
    // (U2) and re-processing them via the grown-instance reseed ctx (U4).
    let warm_compiled = compile_source(COLLECTION_GROW_SRC_N2);
    let mut warm_engine = make_simple_engine();
    warm_engine.eval(&warm_compiled);
    let n_id = ValueCellId::new("S", "n");
    let warm_result = warm_engine
        .edit_param(n_id, Value::Int(4))
        .expect("edit_param(n, 4) must succeed after a cold eval at n=2");
    let warm_val = warm_result
        .values
        .get(&target_id)
        .unwrap_or_else(|| panic!("warm edit_param: bolts[3].is_diameter_determined must be present"));
    let warm_snapshot = warm_engine
        .snapshot()
        .expect("warm engine must have a snapshot after edit_param");
    let (_, warm_det) = warm_snapshot
        .values
        .get(&target_id)
        .unwrap_or_else(|| panic!("warm snapshot: bolts[3].is_diameter_determined must be present"));

    assert_eq!(
        warm_val, cold_val,
        "warm edit_param's grown bolts[3].is_diameter_determined value must match cold \
         eval's; warm={warm_val:?}, cold={cold_val:?}"
    );
    assert_eq!(
        *warm_det, *cold_det,
        "warm edit_param's grown bolts[3].is_diameter_determined determinacy must match \
         cold eval's; warm={warm_det:?}, cold={cold_det:?}"
    );
}
