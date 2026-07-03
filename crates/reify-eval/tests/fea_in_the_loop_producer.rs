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
//!   - `constraint result.max_von_mises < yield_limit` — the "where" clause: a real FEA
//!     stress constraint on the `solve_elastic_static` result. `ElasticResult.max_von_mises`
//!     is already the peak von-Mises stress (`field_max(von_mises(stress))`,
//!     solver_elastic.ri), so this is used directly rather than re-deriving
//!     `max(von_mises(result.stress))`.
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
//! `SolverRegistry`'s `ConstraintSolver` impl (`crates/reify-constraints/src/registry.rs`,
//! out of this task's file scope) overrides only `solve`/`solve_ranked` — it does NOT
//! override `solve_with_dispatch`/`solve_ranked_with_dispatch`, so it inherits the trait's
//! DEFAULT implementation (task #4880 step-2), which discards the dispatch argument and
//! re-calls plain `self.solve`/`self.solve_ranked`. Routed through the registry, the
//! compute-dispatch hook would never reach `DimensionalSolver`'s cost loop, silently
//! defeating this test's signal. This is a real gap for the CLI/GUI's
//! `configured_eval_engine` path (which DOES wire `SolverRegistry::production()`) —
//! reported via `escalate_info` as a dependency for a follow-up task — but this task's
//! own file scope does not include `registry.rs`. Using `DimensionalSolver` directly
//! matches every other real-solver eval-layer test in this crate
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
//! `Value::Undef` inside the cost loop (no compute-dispatch hook wired), so
//! `result.max_von_mises` is `Undef` and `comparison_violation`'s `(lhs, rhs)` pair is
//! `(None, Some(_))` — the "can't decompose numerically" arm returns a FIXED penalty
//! (`1.0`) independent of `thickness`. With no thickness-dependent restoring force,
//! `minimize thickness` free-falls to the lower clamp bound (~1 micron) — NOT interior.
//! On GREEN, the dispatch hook makes `result.max_von_mises` a real, thickness-varying
//! `Scalar` (stress rises sharply as thickness shrinks for this cantilevered box), so
//! `comparison_violation` becomes a real quadratic penalty that grows steeply once
//! stress approaches/exceeds the yield limit, creating a genuine restoring force. Given
//! `PENALTY_WEIGHT = 1e6`, the optimizer converges tightly to the feasible-side boundary
//! — an interior thickness where stress is approximately at (not exceeding) the yield
//! limit. No calibrated numeric thickness is asserted (task #4880 design decision #4) —
//! only that the resolved value is finite and strictly interior to the default bounds.

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

    let result = solve_elastic_static(
        material, length, width, thickness, [tip_load], [mount],
        ElasticOptions(shell_force: ShellForce.Off)
    )

    let yield_limit = 310MPa

    constraint result.max_von_mises < yield_limit

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

    // Real FEA trampolines — same registration pair as `build_test_engine`
    // (test_runner.rs:109-112) — plus the real `DimensionalSolver` directly (see module
    // doc for why not `SolverRegistry::production()`).
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    reify_eval::register_shell_extract_compute_fns(&mut engine);

    let result = engine.eval(&compiled);

    let thickness_id = ValueCellId::new("FeaOptimizedBracket", "thickness");
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values after resolution");

    let thickness_si = match thickness_val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "expected Scalar for FeaOptimizedBracket.thickness, got {:?}",
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
