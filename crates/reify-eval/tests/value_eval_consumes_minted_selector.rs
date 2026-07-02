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
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, Engine, RealizationReadHandle};
use reify_ir::{OpaqueState, Value};
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

// ═══════════════════════════════════════════════════════════════════════════
// R3e (task #4907): same-pass consumer re-eval after in-walk selector mint.
//
// #4900 (R3d, above) fixed the case where a consumer reads a minted selector
// LATER in topo order than the mint. This residual gap (root-caused under
// esc-4655-120) covers a consumer that is ALSO downstream of an `@optimized`
// compute-node dispatch in the SAME template walk: `peak = peak_deviation_at(track,
// loc)` where `track` is an @optimized compute node and `loc` is the R3d
// in-walk-minted selector. `peak_deviation_at` is called directly (not via the
// `peak_deviation` `.ri` wrapper, whose declared `location : LocationId` (=
// `Real`) parameter type would reject a `Selector` argument at compile time) —
// `peak_deviation_at` has NO `.ri` declaration, so it resolves through the
// undeclared-intrinsic path (`NoUserFunctions` → `FunctionCall` →
// `eval_builtin`, see trajectory.ri's "delegate-to-undeclared-name" section)
// with no static arg-type check. `peak_deviation_at` (reify-stdlib
// trampoline.rs) never inspects `track`'s content — a resolved Selector `loc`
// always yields `Real(0.0)` — so the ONLY way `peak` can be `Value::Undef` is
// a stale pre-mint read of `loc` that was never re-evaluated.
// ═══════════════════════════════════════════════════════════════════════════

/// R3e fixture: `R3eWidget` mirrors `WIDGET_SRC` (named `body` param + `loc`
/// selector) but adds an `@optimized` compute-node `track` (registered via
/// `r3e_track_fn`, modeled on `compute_dispatch_registry.rs`'s `identity_fn`)
/// and a same-pass consumer `peak = peak_deviation_at(track, loc)` (the
/// undeclared intrinsic, called directly — see the module-level comment above
/// for why the `.ri`-declared `peak_deviation` wrapper can't be used here).
///
/// `r3e_track_test`'s inline fallback body is `seed` (bare passthrough) —
/// NOT `EndEffectorTrack()` (the no-arg ctor, `trajectory_fns.ri`), which
/// body-inlines to `Value::Undef` (confirmed empirically: `eval_cached` has
/// no `@optimized` ComputeNode-dispatch branch at all — see the module-level
/// comment — so it ALWAYS body-inlines this fn regardless of registration,
/// and an Undef `track` would short-circuit `peak` via strict undef-
/// propagation for a reason unrelated to R3e). `compile_function` applies no
/// body-vs-return-type check (same precedent as `evaluate_profile_at` et
/// al.), so `seed : Real` type-checks fine against the declared
/// `-> EndEffectorTrack`. `Engine::eval` / `engine_edit` register
/// `r3e_track_fn` for `"test::r3e_track"` and dispatch through the
/// ComputeNode path instead, which ALSO returns `seed`'s value — so both
/// paths agree, isolating the regression purely to `loc`'s stale pre-mint
/// read.
const R3E_SRC: &str = r#"
@optimized("test::r3e_track")
fn r3e_track_test(seed: Real) -> EndEffectorTrack {
    seed
}

structure def R3eWidget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let track = r3e_track_test(1.0)
    let loc = faces_by_normal(body, dir, tol)
    let peak = peak_deviation_at(track, loc)
}"#;

/// Trampoline for `"test::r3e_track"` — returns its first value input (a
/// non-Undef `Real`) so `track` is never itself `Undef`, isolating the
/// regression to `loc`'s stale pre-mint read. Modeled verbatim on
/// `compute_dispatch_registry.rs`'s `identity_fn`.
fn r3e_track_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: value_inputs.first().cloned().unwrap_or(Value::Undef),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// `Engine::eval` (kernel-free, no build) must yield a non-Undef value for
/// `R3eWidget.peak` — a same-pass consumer of BOTH the `@optimized` compute
/// node `track` and the in-walk-minted selector `loc`.
///
/// **RED** until the R3e post-mint re-eval pass is wired into
/// `evaluate_params_and_lets_unified`: `peak` reads a stale pre-mint `Undef`
/// snapshot of `loc` and is never re-evaluated after `loc`'s in-walk mint
/// fires.
#[test]
fn value_eval_template_consumer_reads_minted_selector_finite_eval() {
    let compiled = compile_source_with_stdlib(R3E_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3e_track", r3e_track_fn as ComputeFn);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("R3eWidget", "peak");
    let value = result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3eWidget.peak must NOT be Value::Undef after Engine::eval — \
         the same-pass consumer of an in-walk-minted selector must be \
         re-evaluated after the mint fires; got: {value:?}"
    );
}

/// `Engine::eval_cached` (kernel-free, incremental path) must yield a
/// non-Undef value for `R3eWidget.peak`.
///
/// **RED** until the R3e post-mint re-eval pass is ALSO wired into
/// `eval_cached`'s own topo walk — a distinct implementation from
/// `evaluate_params_and_lets_unified` (see #4900's equivalent step-3/step-4
/// split), so step-2's fix does not cover this call site.
#[test]
fn value_eval_template_consumer_reads_minted_selector_finite_eval_cached() {
    let compiled = compile_source_with_stdlib(R3E_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3e_track", r3e_track_fn as ComputeFn);
    let result = engine.eval_cached(&compiled, VersionId(1));

    let cell_id = ValueCellId::new("R3eWidget", "peak");
    let value = result.eval_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3eWidget.peak must NOT be Value::Undef after Engine::eval_cached — \
         the same-pass consumer of an in-walk-minted selector must be \
         re-evaluated after the mint fires; got: {value:?}"
    );
}

/// `engine_edit` (incremental re-eval after param edit) must yield a
/// non-Undef value for `R3eWidget.peak`.
///
/// **RED** until the R3e post-mint re-eval pass is ALSO wired into
/// `engine_edit.rs`'s reeval walk — the 3rd mandated call-site, distinct from
/// both `evaluate_params_and_lets_unified` and `eval_cached` (it walks
/// `new_snapshot.graph.value_cells`, not a `CompiledModule`'s
/// `TopologyTemplate`, since `edit_param` has no access to the compiled
/// module).
#[test]
fn value_eval_template_consumer_reads_minted_selector_finite_after_edit() {
    let compiled = compile_source_with_stdlib(R3E_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3e_track", r3e_track_fn as ComputeFn);
    // Establish baseline.
    engine.eval(&compiled);

    // Edit a param to trigger incremental re-eval.
    let width_id = ValueCellId::new("R3eWidget", "width");
    let edit_result = engine
        .edit_param(width_id, Value::length(0.012))
        .expect("edit_param must succeed after eval");

    let cell_id = ValueCellId::new("R3eWidget", "peak");
    let value = edit_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3eWidget.peak must NOT be Value::Undef after engine_edit — \
         the same-pass consumer of an in-walk-minted selector must be \
         re-evaluated after the mint fires; got: {value:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// R3f (task #4946): post-walk mint → consumer re-eval for geometry-LET-backed
// selector targets (the R3e/#4907 residual).
//
// R3e (above) rescues a same-pass consumer via the R3d IN-WALK mint retry
// (`minted_in_walk`), which only fires for a selector whose TARGET has a
// value cell — e.g. a PARAM `body` — because it resolves the target's
// GeometryHandle through `Engine::mint_symbolic_geometry_handle_for_cell`,
// which looks up a NAMED REALIZATION'S value cell.
//
// A geometry LET (`let loc_box = box(...)`) lowers to a realization with NO
// value cell, so the in-walk retry can never resolve it: `loc =
// faces_by_normal(loc_box, dir, tol)` stays `Value::Undef` for the entire
// in-walk pass and NEVER enters `minted_in_walk`. `loc_box`'s GeometryHandle
// — and therefore `loc` itself — is resolved ONLY by the two POST-WALK mints
// (`mint_symbolic_geometry_handles_into_values` then
// `mint_symbolic_topology_selectors_into_values`), which run AFTER the whole
// per-template walk (and R3e's in-walk-only re-eval) completes. Without a
// consumer re-eval hook keyed on THEIR flip, `peak = peak_deviation_at(track,
// loc)` stays stuck at a stale pre-mint Undef.
//
// The fix reuses the exact R3e re-eval machinery
// (`re_eval_consumers_of_in_walk_mints[_from_graph]`), keyed on the post-walk
// mints' own flipped (Undef→non-Undef) set instead of the in-walk
// `minted_in_walk` set — see that function's doc comment.
// ═══════════════════════════════════════════════════════════════════════════

/// R3f fixture: `R3fWidget` mirrors `R3E_SRC` (same `@optimized` `track` +
/// `peak_deviation_at(track, loc)` consumer shape) but the selector TARGET is
/// a geometry LET (`loc_box`), not a named PARAM (`body`) — the single
/// structural difference the root-cause isolates. `loc_box` has no value
/// cell, so its `GeometryHandle` — and thus `loc` — can only be resolved by
/// the POST-WALK mints, never the R3d in-walk retry.
const R3F_SRC: &str = r#"
@optimized("test::r3f_track")
fn r3f_track_test(seed: Real) -> EndEffectorTrack {
    seed
}

structure def R3fWidget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    let loc_box = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let track = r3f_track_test(1.0)
    let loc = faces_by_normal(loc_box, dir, tol)
    let peak = peak_deviation_at(track, loc)
}"#;

/// `Engine::eval` (kernel-free, no build) must yield a non-Undef value for
/// `R3fWidget.peak` — a same-pass consumer of BOTH the `@optimized` compute
/// node `track` and the geometry-LET-backed selector `loc`, whose target
/// `loc_box` resolves only in the post-walk handle-mint.
///
/// **RED** until the post-walk mints' flipped set feeds
/// `re_eval_consumers_of_in_walk_mints` in `eval()`: `peak` reads a stale
/// pre-mint `Undef` snapshot of `loc` and is never re-evaluated, since `loc`
/// never enters the R3e in-walk-only `minted_in_walk` set.
#[test]
fn value_eval_geometry_let_consumer_reads_minted_selector_finite_eval() {
    let compiled = compile_source_with_stdlib(R3F_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3f_track", r3e_track_fn as ComputeFn);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("R3fWidget", "peak");
    let value = result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3fWidget.peak must NOT be Value::Undef after Engine::eval — \
         a consumer of a geometry-LET-backed selector must be re-evaluated \
         after the post-walk mint resolves it; got: {value:?}"
    );
}

/// `Engine::eval_cached` (kernel-free, incremental path) must yield a
/// non-Undef value for `R3fWidget.peak`.
///
/// **RED** until the post-walk mints' flipped set ALSO feeds
/// `re_eval_consumers_of_in_walk_mints` in `eval_cached`'s own post-walk hook
/// — a distinct call site from `eval()`'s.
#[test]
fn value_eval_geometry_let_consumer_reads_minted_selector_finite_eval_cached() {
    let compiled = compile_source_with_stdlib(R3F_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3f_track", r3e_track_fn as ComputeFn);
    let result = engine.eval_cached(&compiled, VersionId(1));

    let cell_id = ValueCellId::new("R3fWidget", "peak");
    let value = result.eval_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3fWidget.peak must NOT be Value::Undef after Engine::eval_cached — \
         a consumer of a geometry-LET-backed selector must be re-evaluated \
         after the post-walk mint resolves it; got: {value:?}"
    );
}

/// `engine_edit` (incremental re-eval via `edit_source`) must yield a
/// non-Undef value for `R3fWidget.peak`.
///
/// **RED** until the post-walk mints' flipped set ALSO feeds
/// `re_eval_consumers_of_in_walk_mints_from_graph` in `edit_source`'s own
/// post-walk hook — the 3rd mandated call-site. Uses `edit_source`, NOT
/// `edit_param`: `edit_param` has no `module` argument and therefore cannot
/// mint a geometry-LET's `GeometryHandle` at all (that mint iterates
/// `module.templates.realizations`).
#[test]
fn value_eval_geometry_let_consumer_reads_minted_selector_finite_after_source_edit() {
    let compiled = compile_source_with_stdlib(R3F_SRC);
    assert_no_compile_errors(&compiled);

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_compute_fn("test::r3f_track", r3e_track_fn as ComputeFn);
    // Establish baseline (edit_source's precondition).
    engine.eval(&compiled);

    let edit_result = engine
        .edit_source(&compiled)
        .expect("edit_source must succeed after eval");

    let cell_id = ValueCellId::new("R3fWidget", "peak");
    let value = edit_result.values.get_or_undef(&cell_id);
    assert!(
        !matches!(value, Value::Undef),
        "R3fWidget.peak must NOT be Value::Undef after engine_edit (edit_source) — \
         a consumer of a geometry-LET-backed selector must be re-evaluated \
         after the post-walk mint resolves it; got: {value:?}"
    );
}
