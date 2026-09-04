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
//! Edit-path (`Engine::edit_param` / `Engine::edit_source`) dispatch coverage
//! for task #5025 now lives in the sibling submodule
//! `edit_path_optimized_dispatch` — moved there by task #6630 so it stays on
//! the task/merge gate, which THIS submodule's heavy atom
//! (`scripts/heavy-test-filter-lib.sh:99`) excludes. See that file's module
//! doc for the full rationale.

use reify_constraints::DimensionalSolver;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

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
