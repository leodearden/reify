// SPDX-License-Identifier: AGPL-3.0-or-later

//! Edit-path dispatch coverage for task #5025: `Engine::edit_param` and
//! `Engine::edit_source` each construct their OWN `OptimizedComputeDispatcher`
//! and pass it into their OWN call to the solver, so each WARM entry point
//! needs its own regression guard.
//!
//! # Why this is a separate submodule (task #6630)
//!
//! These two guards used to live in the sibling `fea_in_the_loop_producer`
//! submodule, beside the cold-path producer test they complement. That is
//! where they belong topically — but it cost them their gate coverage.
//!
//! The 7th atom of the heavy filterset (`scripts/heavy-test-filter-lib.sh:99`)
//! is `package(reify-eval) & binary(harness_fea_solver_e2e) &
//! test(/^fea_in_the_loop_producer::/)`. A test-scoped atom is SUBMODULE-
//! granular, not test-granular: it evicts EVERY test in that stem off the
//! task/merge gate (`verify.sh:740`'s `-E "not ($heavy)"`, under
//! `REIFY_GATE_EXCLUDE_HEAVY=1`) and onto the asynchronous offline lane
//! (`verify.sh:751`). That is correct for the ~490 s producer the atom was
//! written for, and wrong for these two ~1.9 s guards, which merely shared its
//! file. Measured before the move: `cargo nextest list -E "not ($heavy)"`
//! matched ZERO of them, so a regression in either warm edit path would have
//! surfaced only asynchronously, long after the change that broke it landed.
//!
//! This stem is deliberately OUTSIDE that atom, so the two guards run on every
//! task and merge gate. `tests/infra/test_heavy_filter_atoms.sh` Assertion G is
//! the standing guard on that property: re-homing either test into any
//! heavy-gated stem — or deleting or renaming it — fails there, so the coverage
//! hole #6630 closed cannot silently re-open.
//!
//! # Edit-path coverage (task #5025)
//!
//! The sibling `fea_in_the_loop_producer` submodule carries the FEA
//! end-to-end proof on the COLD build path (`Engine::eval`). It says nothing
//! about the two WARM edit-path entry
//! points, `Engine::edit_param` (`engine_edit.rs:1466` dispatcher
//! construction / `:1546` `solve_with_dispatch(&problem, Some(&dispatcher))`)
//! and `Engine::edit_source` (`:3628` / `:3708`, the same pair) — each
//! builds its OWN `OptimizedComputeDispatcher` and passes it into its OWN
//! call to the solver, independently of the cold path and of each other.
//! The two tests below (`edit_source_dispatches_optimized_compute_into_solver_cost_loop`,
//! `edit_param_dispatches_optimized_compute_into_solver_cost_loop`) each pin
//! one of those two call sites, so a regression names which site broke.
//!
//! They use a synthetic O(1) `@optimized("test::edit_path_half_span")`
//! trampoline rather than reusing `solve_elastic_static` as that sibling
//! does. MEASURED, not assumed: a scratch prototype of the FEA form on this
//! fixture family cost 318.75 s (`edit_source`) + 304.89 s (`edit_param`) =
//! 623.6 s, versus 3.60 ms + 2.91 ms for the synthetic form, because
//! `OptimizedComputeDispatcher::dispatch` does no memoization — every
//! Nelder-Mead trial point in the cost loop re-runs the full FEA solve.
//!
//! That price decides the LANE, which is the whole point of this file. A
//! 623.6 s pair could only ever have been paid on the asynchronous offline
//! heavy lane (`verify.sh:751`) — nothing at that cost sits on a gate. The
//! synthetic form instead costs, end-to-end per test, 0.542 s / 1.037 s /
//! 1.554 s (`edit_param`) and 0.566 s / 1.200 s / 1.864 s (`edit_source`)
//! across three runs on a contended 32-core host (2026-09-04, task #6630).
//! The spread is host contention, not variance in the work; at every point in
//! it these sit under the enclosing 233-test binary's ~2.25 s per-test mean,
//! i.e. cheaper than the average test they run beside. That is what makes
//! these two guards affordable ON the task/merge gate, and is precisely why
//! task #6630 re-homed them to this stem.
//!
//! The FEA form also converges non-uniquely (thickness 1.851e-4 m on one leg
//! of the fixture vs. 7.418e-4 m on the other), so it could not carry a
//! calibrated assertion either. The contract under test is
//! the target-agnostic `Some(&dispatcher)` argument — `OptimizedComputeDispatcher`
//! is literally `fns: registry.fns.clone()` — so a synthetic target pins the
//! same wiring the FEA path would.
//!
//! `Engine::edit_check` is deliberately NOT covered by a third test: it
//! delegates to `edit_param` first (`engine_edit.rs:4249`) and forwards
//! `resolved_params` at `:4295`, so it inherits this wiring transitively.

use reify_constraints::DimensionalSolver;
use reify_core::{Severity, ValueCellId};
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, Engine, RealizationReadHandle};
use reify_ir::{OpaqueState, Value};
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib, mm};

// ═══════════════════════════════════════════════════════════════════════════════
// Edit-path coverage (task #5025) — see the module doc's "Edit-path coverage"
// section for the full rationale (why a synthetic trampoline, why
// `resolved_params` is the primary RED discriminator, why `edit_check` is
// not separately tested).
// ═══════════════════════════════════════════════════════════════════════════════

/// Shared prelude for the edit-path fixture pair: the `EditProbeReading`
/// contract structure, its RED-mechanism comment, the `@optimized`
/// trampoline declaration, and the start of `EditPathOptimizedProbe` through
/// its `param span : Length = 50mm` line. [`edit_path_source`] appends
/// either an empty tail (BEFORE) or [`EDIT_PATH_THICKNESS_TAIL`] (AFTER)
/// plus the closing brace, so the "BEFORE and AFTER are byte-identical
/// through `param span`" claim is structural (one shared `const`) rather
/// than a copy-pasted-and-hopefully-kept-in-sync claim across two `const`
/// literals (task #5025 amendment).
const EDIT_PATH_PRELUDE: &str = r#"
structure def EditProbeReading {
    param level : Length
}

// This inline contract body is the RED mechanism this whole fixture pair
// exists to exercise: `EditProbeReading()` default-constructs a `structure
// def` whose `level` param has NO default, so it evaluates to `Undef`.
// Without a registered compute-dispatch hook, `try_compute_dispatch`
// (crates/reify-expr/src/lib.rs:1990-1992) returns `None` for the call
// below, the call falls through to ordinary body-eval, and the whole
// expression evaluates to Undef -- mirroring
// `crates/reify-compiler/stdlib/solver_elastic.ri:734-745`'s
// `{ ElasticResult() }` discipline one-for-one. DO NOT replace this body
// with a real expression (e.g. `span * 0.5`): that would make the RED path
// produce a satisfiable value and silently destroy the test.
@optimized("test::edit_path_half_span")
fn edit_path_half_span(span : Length) -> Length {
    EditProbeReading().level
}

structure EditPathOptimizedProbe {
    param span : Length = 50mm
"#;

/// Tail [`edit_path_source`] appends after [`EDIT_PATH_PRELUDE`] to build
/// the AFTER fixture: the `thickness` auto param and the constraint that
/// dispatches `edit_path_half_span` through the solver cost loop. The
/// BEFORE fixture appends no tail at all (`edit_path_source("")`).
///
/// BEFORE deliberately does NOT declare `thickness`. An Auto cell's
/// `content_hash` is `id_hash.combine(None)` — a pure function of the cell
/// id — so `edit_source`'s generic carry-over branch (`engine_edit.rs`
/// L3033-3037) restores a cell's PRIOR value across any edit that does not
/// rename or remove it. Had `thickness` already existed in BEFORE, that
/// carry-over would mask the RED signal in `values` on the `edit_source` leg
/// (see `edit_source_dispatches_optimized_compute_into_solver_cost_loop`
/// below). Because it is instead a brand-new cell in the AFTER graph, it is
/// seeded `(Undef, DeterminacyState::Auto)` by `Snapshot::from_compiled_module`,
/// so `values` genuinely reads `Undef` on RED too — belt and braces
/// alongside the `resolved_params` primary assertion.
const EDIT_PATH_THICKNESS_TAIL: &str = r#"    param thickness : Length = auto
    // The @optimized call is inlined directly into the constraint
    // expression, NOT bound to a top-level `let` -- same hazard already
    // documented on `SOURCE` above: a `let`-bound `@optimized` call is
    // evaluated eagerly by the engine's own top-level ComputeNode dispatch,
    // before the solver has assigned `thickness` a resolved numeric value.
    constraint thickness == edit_path_half_span(span)
"#;

/// Builds one of the two edit-path fixture sources: `edit_path_source("")`
/// is BEFORE, `edit_path_source(EDIT_PATH_THICKNESS_TAIL)` is AFTER. Both
/// share [`EDIT_PATH_PRELUDE`] verbatim and close with the same `}`, so the
/// only possible diff between the two rendered sources is `tail` itself —
/// the byte-identity claim is structural, not aspirational.
fn edit_path_source(tail: &str) -> String {
    let mut source = String::from(EDIT_PATH_PRELUDE);
    source.push_str(tail);
    source.push_str("}\n");
    source
}

/// Compiles both edit-path fixture sources and asserts each compiles
/// without errors, returning `(before, after)`. Shared by both edit-path
/// tests so a fixture-breaking grammar/stdlib change is reported at its own
/// compile step instead of misdirecting a downstream assertion failure to
/// blame the wrong leg (task #5025 amendment: `edit_param_dispatches_optimized_compute_into_solver_cost_loop`
/// previously compiled both sources without checking diagnostics on either).
fn compile_edit_path_sources() -> (
    reify_compiler::CompiledModule,
    reify_compiler::CompiledModule,
) {
    let before = compile_source_with_stdlib(&edit_path_source(""));
    let before_errors = collect_errors(&before.diagnostics);
    assert!(
        before_errors.is_empty(),
        "edit_path_source(\"\") (BEFORE) should compile without errors: {:#?}",
        before_errors
    );

    let after = compile_source_with_stdlib(&edit_path_source(EDIT_PATH_THICKNESS_TAIL));
    let after_errors = collect_errors(&after.diagnostics);
    assert!(
        after_errors.is_empty(),
        "edit_path_source(EDIT_PATH_THICKNESS_TAIL) (AFTER) should compile without errors: {:#?}",
        after_errors
    );

    (before, after)
}

/// O(1) `@optimized` trampoline for the edit-path regression tests: returns
/// `span / 2` as a same-dimension `Value::Scalar`, standing in for
/// `solve_elastic_static` (see the module doc's "Edit-path coverage"
/// section for why). Falls back to `Value::Undef` for any non-Scalar input,
/// which should not occur given the fixture's types.
fn edit_path_half_span_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let result = match value_inputs.first() {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => Value::Scalar {
            si_value: si_value * 0.5,
            dimension: *dimension,
        },
        _ => Value::Undef,
    };
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
        structured_detail: Vec::new(),
    }
}

/// Shared engine constructor for the edit-path regression tests: the same
/// `Engine::new` + `with_solver` + `register_production_compute_fns`
/// discipline as `solve_elastic_static_dispatches_real_result_inside_minimize_where_loop`
/// in the sibling `fea_in_the_loop_producer` submodule (INV-FEA-1
/// single-bundler discipline — see that test's comment for
/// why `register_production_compute_fns` is called even though this fixture
/// needs none of its legs), plus registration of the synthetic
/// `test::edit_path_half_span` target. `scripts/check-compute-trampoline-registration.sh`
/// excludes `tests/` dirs by path, so this test-local registration does not
/// trip that guard, and the target name collides with no existing `test::`
/// target under `compute_targets/`.
fn build_edit_path_engine() -> Engine {
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
        reason: "reify-mesh-morph is a dev-only dep of reify-eval (task 4744); this fixture \
                 uses only the synthetic test:: dispatch target, no morph/FEA legs at all",
    });
    engine.register_compute_fn(
        "test::edit_path_half_span",
        edit_path_half_span_fn as ComputeFn,
    );
    engine
}

/// Asserts the shared edit-path contract for one leg (`edit_source` or
/// `edit_param`): `resolved_params[thickness_id]` is present and a
/// `Value::Scalar`, `values[thickness_id]` is the identical, non-`Undef`,
/// finite Scalar, its SI value matches `expected_si` within a derived 1e-9 m
/// tolerance, and the edit produced no `Severity::Error` diagnostics. Shared
/// by both edit-path tests (task #5025 amendment) to collapse what was a
/// ~55-line copy-pasted assertion tail into one call each.
///
/// `path_label` names the `Engine` method under test (`"edit_source"` or
/// `"edit_param"`) and doubles as the symbol cited in the PRIMARY panic
/// message below (`Engine::{path_label}`) -- deliberately a symbol, not a
/// source line number, so the message stays accurate across future edits to
/// `engine_edit.rs`. The call sites are still cited, with line numbers, in
/// the module doc and in each test's own doc comment, where drift is
/// cosmetic rather than actively misleading a debugger at panic time.
///
/// # Why the PRIMARY assertion is on `resolved_params`, not just `values`
///
/// This is the single most likely thing a future reader "simplifies" away
/// and silently defeats the guard. `edit_param`'s `SolveResult::Infeasible`
/// arm (engine_edit.rs ~L1593) and `edit_source`'s (~L3751) only do
/// `diagnostics.extend(solver_diags)` -- neither writes anything back nor
/// resets anything. `values` is cloned wholesale from the prior snapshot
/// (edit_param L1010-1015); the main eval loop skips auto cells (gated on
/// `default_expr.is_some()`, and auto cells compile with `default_expr:
/// None`); and `deactivate_if_not_auto` is explicitly auto-safe. So on RED
/// the `thickness` cell would still hold whatever value the LAST successful
/// solve wrote to it -- a `values`-only "is it non-Undef" assertion could
/// PASS on RED by construction. `resolved_params` is a fresh per-call
/// `HashMap` written only in the `Solved` arm and plumbed into the returned
/// `EvalResult` (L2655 for edit_param, L4236 for edit_source), so its
/// absence is an unambiguous "this call's solve produced nothing". (The
/// positive `expected_si` equality below is a second, independent
/// discriminator for the same reason.)
fn assert_edit_path_thickness_resolved(
    result: &reify_eval::EvalResult,
    thickness_id: &ValueCellId,
    expected_si: f64,
    path_label: &str,
) {
    // PRIMARY assertion -- the one that goes RED.
    let resolved = result.resolved_params.get(thickness_id).unwrap_or_else(|| {
        panic!(
            "thickness missing from resolved_params after {path_label} -- the edit-path \
                 solve returned Infeasible because the @optimized node body-evaluated to Undef, \
                 i.e. the Some(&dispatcher) argument to solve_with_dispatch in \
                 Engine::{path_label} was dropped"
        )
    });
    assert!(
        matches!(resolved, Value::Scalar { .. }),
        "expected resolved_params[thickness] to be a Value::Scalar after {path_label}, got {:?}",
        resolved
    );

    // SECONDARY assertions, all on `values` -- the task's literal acceptance
    // signal ("non-Undef after the edit") is framed in terms of `values`.
    let thickness_value = result
        .values
        .get(thickness_id)
        .unwrap_or_else(|| panic!("thickness must be in values after {path_label}'s solve"));
    assert_eq!(
        thickness_value, resolved,
        "values[thickness] must be the same Scalar {path_label}'s solve wrote to resolved_params",
    );
    assert!(
        !matches!(thickness_value, Value::Undef),
        "thickness must not be Undef after {path_label}'s solve (task #5025 acceptance signal), \
         got {:?}",
        thickness_value
    );
    let thickness_si = match thickness_value {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "expected values[thickness] to be a resolved Scalar after {path_label}, got {:?}",
            other
        ),
    };
    assert!(
        thickness_si.is_finite(),
        "resolved thickness must be finite after {path_label}, got {:?}",
        thickness_si
    );
    // Tolerance basis (derived, not tuned): `solve_core` only returns
    // `SolveResult::Solved` when `final_max_residual <= FEASIBILITY_THRESHOLD`
    // (crates/reify-constraints/src/solver.rs:20 = 1e-12, gated at :1989);
    // the measured absolute error on this exact fixture shape was ~5.5e-16 m.
    // 1e-9 m sits 6 orders above the measured error and 7 orders below the
    // smallest expected-value signal (15mm, the edit_param leg).
    assert!(
        (thickness_si - expected_si).abs() < 1e-9,
        "expected thickness == {expected_si:.6e} m after {path_label}, got {thickness_si:.6e} m",
    );

    // No Severity::Error diagnostics on GREEN (RED emits exactly one:
    // "constraints could not be satisfied (max absolute residual: 1.00e0)").
    let error_diagnostics: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diagnostics.is_empty(),
        "expected no Severity::Error diagnostics after a successful {path_label} solve, got {:#?}",
        error_diagnostics
    );
}

/// task #5025: `Engine::edit_source` dispatches `@optimized` ComputeNodes
/// through the real `OptimizedComputeDispatcher` inside the `DimensionalSolver`
/// cost loop, exactly as the cold `eval()` path does. Pins `engine_edit.rs:3628`
/// (dispatcher construction) / `:3708` (`solve_with_dispatch(&problem, Some(&dispatcher))`).
///
/// See [`assert_edit_path_thickness_resolved`]'s doc for the full RED/GREEN
/// assertion rationale (why `resolved_params`, not just `values`).
#[test]
fn edit_source_dispatches_optimized_compute_into_solver_cost_loop() {
    let (before, after) = compile_edit_path_sources();

    let mut engine = build_edit_path_engine();
    let thickness_id = ValueCellId::new("EditPathOptimizedProbe", "thickness");

    // Precondition: BEFORE never declares `thickness`, so the cold eval must
    // not produce an entry for it -- the AFTER cell is brand new.
    let cold = engine.eval(&before);
    assert!(
        cold.values.get(&thickness_id).is_none(),
        "thickness must not exist before the edit_source swap introduces it \
         (the BEFORE fixture never declares it); got {:?}",
        cold.values.get(&thickness_id)
    );

    let edited = engine
        .edit_source(&after)
        .expect("edit_source must succeed after a cold eval");

    // span/2 for the fixture's unedited `span = 50mm`.
    assert_edit_path_thickness_resolved(&edited, &thickness_id, 0.025, "edit_source");
}

/// task #5025: `Engine::edit_param` dispatches `@optimized` ComputeNodes
/// through the real `OptimizedComputeDispatcher` inside the `DimensionalSolver`
/// cost loop. Pins `engine_edit.rs:1466` (dispatcher construction) / `:1546`
/// (`solve_with_dispatch(&problem, Some(&dispatcher))`) -- the OTHER call
/// site from `edit_source_dispatches_optimized_compute_into_solver_cost_loop`
/// above; the RED probe for this test cross-checks that the two sites are
/// independently guarded (removing one leaves the other test green).
///
/// See [`assert_edit_path_thickness_resolved`]'s doc for the full RED/GREEN
/// assertion rationale (why `resolved_params`, not just `values`).
#[test]
fn edit_param_dispatches_optimized_compute_into_solver_cost_loop() {
    let (before, after) = compile_edit_path_sources();

    let mut engine = build_edit_path_engine();
    let thickness_id = ValueCellId::new("EditPathOptimizedProbe", "thickness");
    let span_id = ValueCellId::new("EditPathOptimizedProbe", "span");

    // Cheap baseline: edit_param requires a prior eval() to establish the
    // eval_state/demand registry it needs.
    engine.eval(&before);

    // SETUP ONLY: introduce the `thickness` auto cell via edit_source, as
    // edit_source_dispatches_optimized_compute_into_solver_cost_loop does.
    // One precondition only -- a failure here means THAT test's leg has
    // regressed and this test's own result is meaningless until it is
    // fixed; this test does not re-assert that leg's full contract.
    let setup = engine
        .edit_source(&after)
        .expect("edit_source setup must succeed after a cold eval");
    assert!(
        setup.resolved_params.contains_key(&thickness_id),
        "setup edit_source did not resolve thickness -- the edit_source leg (see \
         edit_source_dispatches_optimized_compute_into_solver_cost_loop) has regressed; this \
         test's result is meaningless until that is fixed"
    );

    // Editing `span` (not some unrelated param) matters: `constraint
    // thickness == edit_path_half_span(span)` reads BOTH `thickness` (so
    // the constraint is in `filtered_constraints` for the auto-param group)
    // AND `span` (so editing `span` puts the constraint in the dirty cone
    // and forces a re-solve). Editing a param the constraint does not read
    // would skip the group entirely, never invoke the solver, and silently
    // stop testing anything.
    let edited = engine
        .edit_param(span_id, mm(30.0))
        .expect("edit_param must succeed after a cold eval");

    // span/2 for the edited `span = 30mm`.
    assert_edit_path_thickness_resolved(&edited, &thickness_id, 0.015, "edit_param");
}
