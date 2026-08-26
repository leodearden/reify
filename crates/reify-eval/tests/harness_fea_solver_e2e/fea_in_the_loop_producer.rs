// SPDX-License-Identifier: AGPL-3.0-or-later

//! Producer signal for task #4880 (FEA-in-the-loop): `solve_elastic_static`
//! evaluates to a REAL result (not `Value::Undef`) when an `@optimized`
//! ComputeNode is dispatched through the full `Engine` inside the
//! `DimensionalSolver` cost loop.
//!
//! # What is tested
//!
//! An inline `FeaOptimizedBracket` structure (NOT `examples/fea_bracket_minimize_mass.ri`
//! — that file is consumer task #2930's deliverable) declares:
//!   - `param thickness : Length = auto(free)` — the free design variable
//!   - `minimize thickness` — a proxy for `minimize mass(..)`: for this fixed-footprint
//!     box (`length`/`width` fixed), mass is a strictly increasing linear function of
//!     `thickness` (mass = density * length * width * thickness), so minimizing thickness
//!     is exactly equivalent to minimizing mass. This mirrors the low-level solver test
//!     (task #4880 step-5), which used the identical proxy (`minimize t`) for the same
//!     reason.
//!   - `constraint solve_elastic_static(..).max_von_mises < yield_limit` — the "where"
//!     clause: a real FEA stress constraint on the `solve_elastic_static` result.
//!     `ElasticResult.max_von_mises` is already the peak von-Mises stress
//!     (`field_max(von_mises(stress))`, solver_elastic.ri), so this is used directly
//!     rather than re-deriving `max(von_mises(result.stress))`. The FEA call is inlined
//!     directly into the constraint expression rather than bound to a top-level
//!     `let result = ...` cell — see the inline comment in `SOURCE` for why a `let`-bound
//!     `@optimized` call is unsafe here (the engine's pre-existing, task-4880-unrelated
//!     top-level ComputeNode dispatch evaluates it eagerly, before `thickness` resolves).
//!   - A plain `Pressure` literal for the yield limit rather than unwrapping
//!     `material.yield_stress : Option<Pressure>` — Reify has no `.unwrap()`/`?` and no
//!     `match some(x) {…}` precedent in the stdlib .ri files (see the identical rationale
//!     in `examples/multi_load_bracket.ri`).
//!
//! `ShellForce.Off` forces the tet/solid solver path so the auto-classification threshold
//! (thickness/extent < shell_threshold) cannot flip the body between solid and shell
//! formulations as the optimizer moves `thickness` (mirrors `examples/multi_load_bracket.ri`).
//!
//! # Why `DimensionalSolver` directly, not `SolverRegistry::production()`
//!
//! Because that is the exact seam this task's title names, and it is the narrowest
//! subject that can carry the signal: a registry in the way would add a decomposition
//! layer between the Engine and the cost loop under test.
//!
//! NOT because the registry swallows the hook — it no longer does. When this module was
//! first written, `SolverRegistry`'s `ConstraintSolver` impl overrode only
//! `solve`/`solve_ranked`, so it inherited the trait's DEFAULT
//! `solve_with_dispatch`/`solve_ranked_with_dispatch` (task #4880 step-2), which discard
//! the dispatch argument and re-enter plain `self.solve`/`self.solve_ranked` — routed
//! through the registry, the hook would never have reached `DimensionalSolver`'s cost
//! loop. That was a real gap for the CLI/GUI's `configured_eval_engine` path (which DOES
//! wire `SolverRegistry::production()`), and this task CLOSED it in steps 11/12:
//! `crates/reify-constraints/src/registry.rs` now overrides both `*_with_dispatch`
//! methods and forwards the hook to the inner solver of EVERY decomposed component.
//! `crates/reify-constraints/tests/registry_tests.rs` is where that forwarding is pinned
//! (both the `solve_with_dispatch` and `solve_ranked_with_dispatch` arms, plus the
//! no-dispatch arm staying Infeasible); this module deliberately does not duplicate it.
//!
//! Using `DimensionalSolver` directly also matches every other real-solver eval-layer
//! test in this crate
//! (`resolution.rs::e2e_minimize_through_real_solver`, `continuous_cost_min_example_e2e.rs`,
//! `robustness_floor_signal.rs` — none of them use `SolverRegistry` either) and fully
//! exercises the task's actual title: "@optimized ComputeNodes dispatch through the full
//! Engine inside the DimensionalSolver cost loop."
//!
//! # Why `auto(free)`, not strict `auto`
//!
//! Strict `auto` triggers a perturbation-based uniqueness re-solve (a second full
//! Nelder-Mead run — solver.rs `verify_uniqueness`), doubling the number of real FEA
//! solves for no additional signal here (this test is a capability probe, not a
//! uniqueness contract test). `auto(free)` skips it — same rationale as
//! `examples/continuous_cost_min.ri` and `crates/reify-eval/tests/fixtures/cost_min_robustness_floor.ri`.
//!
//! # RED (base) vs GREEN (after step-10) behaviour
//!
//! `ConstraintCostFunction::cost` (solver.rs) clamps every trial `thickness` into the
//! default `Length` auto-param bounds `[1 micron, 10 m]` (`default_bounds_for`) before
//! evaluating the constraint/objective. On RED, `solve_elastic_static` body-evals to
//! `Value::Undef` inside the cost loop (no compute-dispatch hook wired) for EVERY trial
//! `thickness` — `.max_von_mises` field access on the resulting all-`Undef`
//! `ElasticResult` is `Undef`, so `comparison_residual`'s `(lhs, rhs)` pair is
//! `(None, Some(_))` — the "can't decompose numerically" arm returns a FIXED residual
//! (`1.0`), independent of `thickness`, for every candidate including the initial seed.
//! Since the residual never drops to (or below) `FEASIBILITY_THRESHOLD` at any point —
//! confirmed empirically: `solve_core_with_sd_tolerance` reports the problem
//! `SolveResult::Infeasible` rather than `Solved` — the engine never writes a resolved
//! value into the `thickness` value cell, which is asserted here to observably stay
//! `Value::Undef` (the pre-solve placeholder). On GREEN, the dispatch hook makes
//! `.max_von_mises` a real, thickness-varying `Scalar` (stress rises sharply as
//! thickness shrinks for this cantilevered box), so the constraint becomes genuinely
//! satisfiable for large-enough thickness and violated for small thickness — a real
//! restoring force lets `solve_core` find a feasible, `Solved` point, converging near
//! the stress≈yield crossing thickness. No calibrated numeric thickness is asserted
//! (task #4880 design decision #4) — only that the resolved value is a finite Scalar
//! strictly interior to the default bounds.
//!
//! # Edit-path coverage (task #5025)
//!
//! The test above carries the FEA end-to-end proof on the COLD build path
//! (`Engine::eval`). It says nothing about the two WARM edit-path entry
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
//! trampoline rather than reusing `solve_elastic_static` above. MEASURED,
//! not assumed: a scratch prototype of the FEA form on this fixture family
//! cost 318.75 s (`edit_source`) + 304.89 s (`edit_param`) = 623.6 s, versus
//! 3.60 ms + 2.91 ms for the synthetic form, because
//! `OptimizedComputeDispatcher::dispatch` does no memoization — every
//! Nelder-Mead trial point in the cost loop re-runs the full FEA solve. The
//! FEA form would add +127% to this harness family's stated ~490 s cost on
//! every merge gate, permanently. It also converges non-uniquely (thickness
//! 1.851e-4 m on one leg of the fixture vs. 7.418e-4 m on the other), so it
//! could not carry a calibrated assertion either. The contract under test is
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

/// Inline bracket-minimize-mass fixture. Small geometry (50mm x 30mm footprint) and a
/// modest tip load (50 N) keep the per-candidate FEA solve cheap (coarse default mesh)
/// while landing the analytic stress/yield crossing point comfortably away from both the
/// default seed (~10mm, `extract_initial_point`'s fallback) and the default auto-param
/// bounds `[1 micron, 10 m]`: at `thickness = 10mm`, closed-form cantilever beam theory
/// (`sigma_max = 6*P*L/(b*h^2)`) gives ~5 MPa versus the ~310 MPa yield limit (>60x
/// margin) — comfortably feasible at the seed, so `solve_core`'s `initially_feasible`
/// fast path (a much smaller Nelder-Mead iteration budget) applies once the real
/// dispatch is wired.
const SOURCE: &str = r#"
structure FeaOptimizedBracket {
    param length : Length = 50mm
    param width  : Length = 30mm
    param thickness : Length = auto(free)

    let material = Steel_AISI_1045()
    let tip_load  = PointLoad(point: "tip", force: 50.0)
    let mount     = FixedSupport(target: "root")

    let yield_limit = 310MPa

    // The FEA call is inlined directly into the constraint expression rather than
    // bound to a top-level `let result = ...` cell. A `let`-bound `@optimized` call
    // is eagerly evaluated by the engine's own top-level ComputeNode dispatch
    // (solver_elastic.ri's "engine_eval.rs:2793-2944" mechanism, pre-existing and
    // unrelated to task #4880) as soon as its declaration is reached in the main
    // eval pass -- BEFORE the constraint solver has assigned `thickness` a resolved
    // numeric value, feeding the real trampoline an Undef dimension and panicking
    // (`extract_scalar_si`, elastic_static.rs). Inlining keeps this call embedded
    // solely inside the constraint's compiled expression tree, which only the
    // constraint solver's cost loop evaluates (via `reify_expr::eval_expr` with a
    // numeric trial `thickness` substituted on every candidate, including the
    // initial seed) -- precisely the code path task #4880 wires up.
    constraint solve_elastic_static(
        material, length, width, thickness, [tip_load], [mount],
        ElasticOptions(shell_force: ShellForce.Off)
    ).max_von_mises < yield_limit

    minimize thickness
}
"#;

/// Lower interior threshold (0.1 mm SI): comfortably above the default `Length`
/// auto-param lower bound (1 micron = 1e-6 m, `default_bounds_for` in
/// `crates/reify-constraints/src/solver.rs`) where the RED (Undef-driven) optimisation
/// parks thickness — two orders of magnitude of margin.
const INTERIOR_LOWER_THRESHOLD_SI: f64 = 1e-4;

/// Upper interior threshold (1 m SI): comfortably below the default `Length`
/// auto-param upper bound (10 m) — any physically-sane resolved bracket thickness for
/// this fixture lands far below this.
const INTERIOR_UPPER_THRESHOLD_SI: f64 = 1.0;

/// RED on base / GREEN after task #4880 step-10: `auto` thickness resolves FINITE and
/// STRICTLY INTERIOR to its bounds only when the FEA stress constraint is real and
/// binding (see module doc for the full RED/GREEN mechanics).
#[test]
fn solve_elastic_static_dispatches_real_result_inside_minimize_where_loop() {
    let compiled = compile_source_with_stdlib(SOURCE);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    // Real FEA trampolines via the SINGLE bundler `register_production_compute_fns`
    // (INV-FEA-1), not by hand-rolling its legs — hazard (3) in
    // `scripts/check-compute-trampoline-registration.sh`'s header is exactly a fourth
    // site assembling the bundle from its halves, so that a leg added to the bundler
    // later never reaches it. That guard's SCOPE_PATHSPECS exclude `tests/`, so
    // nothing would catch the drift here. `MorphRegistration::Unavailable` matches
    // `build_test_engine` (test_runner.rs) — reify-mesh-morph is a dev-only dep of
    // reify-eval and is not needed by this fixture. Plus the real `DimensionalSolver`
    // directly (see module doc for why not `SolverRegistry::production()`).
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
        reason: "reify-mesh-morph is a dev-only dep of reify-eval (task 4744); this fixture needs only the FEA/shell-extract legs",
    });

    let result = engine.eval(&compiled);

    let thickness_id = ValueCellId::new("FeaOptimizedBracket", "thickness");
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values after resolution");

    let thickness_si = match thickness_val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "expected a resolved Scalar for FeaOptimizedBracket.thickness once the FEA \
             stress constraint is real and binding; got {:?}. On the base (pre-#4880 \
             compute-dispatch-wiring) code, `solve_elastic_static` body-evals to Undef for \
             every trial thickness inside the cost loop, so the stress constraint never \
             decomposes to a satisfiable numeric residual, the auto-resolution reports \
             SolveResult::Infeasible, and `thickness` never advances past its unresolved \
             Undef placeholder — see module doc for the full RED/GREEN mechanics.",
            other
        ),
    };

    assert!(
        thickness_si.is_finite(),
        "resolved thickness must be finite, got {:?}",
        thickness_si
    );

    // Capability signal only (no calibrated numeric — task #4880 design decision #4):
    // a Solved, STRICTLY INTERIOR thickness proves the FEA stress constraint is real
    // and binding rather than a structural no-op. See module doc for the full RED
    // (parks at ~1 micron) vs GREEN (binds at an interior stress≈yield point) mechanics.
    assert!(
        thickness_si > INTERIOR_LOWER_THRESHOLD_SI && thickness_si < INTERIOR_UPPER_THRESHOLD_SI,
        "expected thickness strictly interior to its bounds ({:e} m < t < {:e} m), \
         got {:.6e} m — RED parks at the ~1 micron lower bound because the Undef-driven \
         FEA constraint contributes no thickness-dependent penalty (structural no-op)",
        INTERIOR_LOWER_THRESHOLD_SI,
        INTERIOR_UPPER_THRESHOLD_SI,
        thickness_si,
    );
}

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
/// above (INV-FEA-1 single-bundler discipline — see that test's comment for
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
