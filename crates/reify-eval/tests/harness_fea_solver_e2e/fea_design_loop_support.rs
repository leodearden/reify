// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared wiring for the FEA-in-the-loop design-optimisation tests in this
//! harness — the ones that drive a real `solve_elastic_static` inside a real
//! `DimensionalSolver` cost loop.
//!
//! Carries no `#[test]` of its own: it exists so the engine construction those
//! tests need lives in ONE place rather than being copy-pasted per module. The
//! wiring is small but it is not incidental — [`fea_loop_engine`] encodes the
//! INV-FEA-1 single-bundler rule, which `scripts/check-compute-trampoline-registration.sh`
//! does NOT guard here (its SCOPE_PATHSPECS exclude `tests/`). A second hand-rolled
//! copy is therefore a copy nothing keeps in step.
//!
//! `fea_in_the_loop_producer` (task #4880) also consumes [`fea_loop_engine`] and the
//! two threshold constants below, rather than constructing its own copies (task #6607
//! finished the migration task #2930 could not — that module was outside #2930's file
//! scope).

use reify_constraints::DimensionalSolver;
use reify_eval::Engine;
use reify_test_support::MockConstraintChecker;

/// Lower interior threshold (0.1 mm SI) — two orders of magnitude above the
/// default `Length` auto-param lower bound (1 micron, `default_bounds_for` in
/// `crates/reify-constraints/src/solver.rs`), which is where an `auto` design
/// variable parks when its opposing constraint has silently become a no-op.
///
/// Shared because the anti-vacuity argument is shared: a design loop that
/// converges anywhere near the bound is not converging, it is collapsing.
pub const INTERIOR_LOWER_THRESHOLD_SI: f64 = 1e-4;

/// Upper interior threshold (1 m SI) — comfortably below the default `Length`
/// auto-param upper bound (10 m). Any physically-sane thickness for the
/// centimetre-scale brackets these tests size lands far below it.
pub const INTERIOR_UPPER_THRESHOLD_SI: f64 = 1.0;

/// An `Engine` wired for an FEA-in-the-loop design solve: the real
/// `DimensionalSolver` plus the real FEA compute trampolines.
///
/// Two things here are load-bearing and neither is obvious at the call site,
/// which is the reason this function exists rather than six inline lines:
///
///   - `register_production_compute_fns` is the SINGLE bundler (INV-FEA-1).
///     Assembling the registration from its legs instead is hazard (3) in
///     `scripts/check-compute-trampoline-registration.sh`'s header — a site that
///     never receives a leg added to the bundler later. That guard's
///     SCOPE_PATHSPECS exclude `tests/`, so nothing mechanical catches the drift
///     in this directory; keeping one call site is the mitigation.
///   - `MorphRegistration::Unavailable` matches `build_test_engine`'s posture:
///     reify-mesh-morph is a dev-only dep of reify-eval (task 4744) and the FEA
///     design loop needs only the FEA legs.
///
/// The real `DimensionalSolver` is used directly rather than
/// `SolverRegistry::production()` — see `fea_in_the_loop_producer`'s module doc
/// for why that is the narrower and more faithful seam for these tests.
pub fn fea_loop_engine() -> Engine {
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
        reason: "reify-mesh-morph is a dev-only dep of reify-eval (task 4744); the FEA \
                 design-loop tests need only the FEA legs",
    });
    engine
}
