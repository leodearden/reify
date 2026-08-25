// SPDX-License-Identifier: AGPL-3.0-or-later

//! Leaf signal for task #2930: `examples/fea_bracket_minimize_mass.ri` — the
//! FEA-in-the-loop design example — actually CONVERGES. Its `auto` thickness is
//! resolved to a finite, physically-meaningful value by a real Nelder-Mead search
//! whose every candidate runs a real `solve_elastic_static` solve.
//!
//! # Why this is not a duplicate of `fea_in_the_loop_producer`
//!
//! That module (task #4880) proves the CAPABILITY on an inline fixture:
//! `solve_elastic_static` evaluates to a real result rather than `Value::Undef`
//! when an `@optimized` ComputeNode is dispatched through the full `Engine` inside
//! the `DimensionalSolver` cost loop. This module's SUBJECT is the shipped example
//! file itself, read from disk — which is exactly what makes it 2930's deliverable
//! signal rather than a second copy of 4880's. If the example drifts (a renamed
//! face, a load that stops resolving, a constraint that stops compiling), the
//! producer test stays green and only this one goes red.
//!
//! Engine construction is reused verbatim from the producer: the real
//! `DimensionalSolver`, plus real FEA trampolines via the SINGLE bundler
//! `register_production_compute_fns` (INV-FEA-1) rather than hand-rolled legs.
//!
//! # THE LOAD-BEARING ASSERTION, and its RED/GREEN mechanic
//!
//! `assert_interior` below is the whole point of this module, and it is not an
//! arbitrary range check. Read it as an ANTI-VACUITY guard.
//!
//! `ConstraintCostFunction::cost` clamps every trial thickness into the default
//! `Length` auto-param bounds `[1 micron, 10 m]` (`default_bounds_for`). The
//! example's objective, `minimize mass`, is strictly increasing in thickness. So
//! the ONLY thing standing between the optimiser and the 1-micron lower bound is
//! the von-Mises stress constraint pushing back. If that constraint is real and
//! binding, thickness settles at the stress≈yield crossing — an interior point. If
//! it is a no-op in ANY of the several ways it can silently become one, the
//! optimiser is unopposed and thickness parks at ~1e-6 m, failing this assertion.
//!
//! The silent-no-op failure modes this therefore catches, all of which compile
//! clean and several of which produce a "solved" result:
//!
//!   - the pressure load is DROPPED because its `face` does not resolve to one of
//!     the six named box faces → zero stress → constraint vacuously satisfied at
//!     any thickness. Two distinct ways to land here, both observed: naming a
//!     descriptive face like `"top"`/`"mount"` that `box_face_plane` does not
//!     know, or using the typed `face(body, "z_max")` selector spelling, which
//!     mints `Value::Undef` when one of `body`'s box dims is an unresolved `auto`
//!     param (measured: the example then parks at 2.88e-6 m — see the example's
//!     own comment and follow-up ticket tkt_0RSVYHGPGEYD2QV9MTGTXPQ86S). The drop
//!     does emit a `tracing::warn!`, but no test installs a subscriber, so in
//!     practice it is silent and only this assertion catches it;
//!   - the stress predicate was written as a `minimize … where …` guard, whose
//!     `where_clause` the compiler's Minimize lowering arm never reads → the
//!     structure compiles with ZERO constraints;
//!   - `solve_elastic_static` evaluates to `Value::Undef` inside the cost loop
//!     (the pre-#4880 state) → the comparison cannot decompose numerically and
//!     contributes a FIXED residual independent of thickness.
//!
//! A test that only asserted "thickness is a finite Scalar" would pass in the
//! first two cases. Interiority is what makes it falsifiable.
//!
//! # Runtime
//!
//! MEASURED 66s / 93s / 105s across three runs on a contended 32-core host — a
//! real Nelder-Mead-over-FEA loop. Cheaper than the producer's ~490s but still
//! ~30x this harness's 2.25s per-test mean, so it is evicted from the merge gate
//! by its own test-scoped atom in `scripts/heavy-test-filter-lib.sh`.

use reify_constraints::DimensionalSolver;
use reify_core::{Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// The shipped example under test, resolved from `CARGO_MANIFEST_DIR` so it works
/// in any worktree.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fea_bracket_minimize_mass.ri"
);

const TEMPLATE_NAME: &str = "FeaBracketMinimizeMass";

/// Lower interior threshold (0.1 mm SI) — two orders of magnitude above the
/// default `Length` auto-param lower bound (1 micron, `default_bounds_for` in
/// `crates/reify-constraints/src/solver.rs`) where an unopposed `minimize mass`
/// parks. Same value as the producer test's, for the same reason.
const INTERIOR_LOWER_THRESHOLD_SI: f64 = 1e-4;

/// Upper interior threshold (1 m SI) — comfortably below the default 10 m upper
/// bound. Any physically-sane thickness for a 50 mm x 30 mm bracket is far below.
const INTERIOR_UPPER_THRESHOLD_SI: f64 = 1.0;

/// `Steel_AISI_1045.density` (`crates/reify-compiler/stdlib/materials_fea.ri`),
/// and the example's fixed footprint — the inputs to the closed-form mass the
/// example's objective minimises. Used to check `mass` is the quantity it claims
/// to be, not merely *a* finite number.
const STEEL_DENSITY_SI: f64 = 7850.0;
const LENGTH_SI: f64 = 0.050;
const WIDTH_SI: f64 = 0.030;

#[test]
fn fea_bracket_minimize_mass_example_converges_to_an_interior_thickness() {
    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("failed to read examples/fea_bracket_minimize_mass.ri");

    // ── (a) Cheap precondition: the example compiles clean ───────────────────
    //
    // Checked here as well as in the compile-surface test so that a compile
    // regression fails FAST and legibly, rather than surfacing ~400s later as a
    // confusing unresolved-thickness panic.
    let compiled = compile_source_with_stdlib(&src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/fea_bracket_minimize_mass.ri should compile without Error \
         diagnostics: {:#?}",
        errors
    );
    debug_assert_eq!(
        errors.len(),
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    );

    // ── Engine: real solver + real FEA trampolines ───────────────────────────
    //
    // `register_production_compute_fns` is the SINGLE bundler (INV-FEA-1); never
    // assemble it from its legs, which is hazard (3) in
    // `scripts/check-compute-trampoline-registration.sh`'s header — that guard's
    // SCOPE_PATHSPECS exclude `tests/`, so nothing would catch the drift here.
    // `MorphRegistration::Unavailable` matches `build_test_engine`: reify-mesh-morph
    // is a dev-only dep of reify-eval and this example needs only the FEA legs.
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    engine.register_production_compute_fns(reify_eval::MorphRegistration::Unavailable {
        reason: "reify-mesh-morph is a dev-only dep of reify-eval (task 4744); this example needs only the FEA legs",
    });

    let result = engine.eval(&compiled);

    // Reported so a future failure of this ~100s test can be diagnosed from the
    // captured output alone, without a re-run under --no-capture.
    eprintln!(
        "fea_bracket_minimize_mass: thickness = {:?}, mass = {:?}",
        result.values.get(&ValueCellId::new(TEMPLATE_NAME, "thickness")),
        result.values.get(&ValueCellId::new(TEMPLATE_NAME, "mass")),
    );

    // ── (b) `thickness` resolved to a Scalar, not the Undef placeholder ──────

    let thickness_id = ValueCellId::new(TEMPLATE_NAME, "thickness");
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values after resolution");

    let thickness_si = match thickness_val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "expected a resolved Scalar for {TEMPLATE_NAME}.thickness, got {other:?}. \
             An Undef here means the auto-resolution reported Infeasible and never \
             wrote a value: the stress constraint never decomposed into a satisfiable \
             numeric residual. See the module doc for the failure modes."
        ),
    };

    // ── (c) Finite ───────────────────────────────────────────────────────────

    assert!(
        thickness_si.is_finite(),
        "resolved thickness must be finite, got {thickness_si:?}"
    );

    // ── (d) THE LOAD-BEARING ASSERTION: strictly interior ────────────────────
    //
    // See the module doc for the full RED/GREEN mechanic. Do NOT widen these
    // thresholds to make a failure go away — a value at the lower bound is the
    // signal that the stress constraint has become a no-op, which is precisely
    // what this example exists to demonstrate is NOT the case.
    assert!(
        thickness_si > INTERIOR_LOWER_THRESHOLD_SI && thickness_si < INTERIOR_UPPER_THRESHOLD_SI,
        "expected the design loop to converge STRICTLY INTERIOR to the default auto \
         bounds ({INTERIOR_LOWER_THRESHOLD_SI:e} m < t < {INTERIOR_UPPER_THRESHOLD_SI:e} m), \
         got {thickness_si:.6e} m. At the ~1e-6 m lower bound this means `minimize mass` \
         ran UNOPPOSED — the von-Mises stress constraint contributed no \
         thickness-dependent penalty. Diagnose which no-op it is (dropped pressure \
         load / discarded objective guard / Undef FEA result — see the module doc); \
         do NOT widen this range."
    );

    // ── (e) `mass` is a real evaluated quantity, not Undef ───────────────────
    //
    // Guards the trap that the objective could be a well-typed expression that
    // evaluates to nothing — e.g. if it were reached for via the `Physical` trait's
    // derived `mass = volume(geometry) * material.density`, which is Phase-1
    // typecheck-only and evaluates to `Value::Undef`. An Undef objective leaves the
    // optimiser with nothing to descend, so pinning it here keeps assertion (d)
    // honest about WHY it converged.
    let mass_id = ValueCellId::new(TEMPLATE_NAME, "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .expect("mass should be in values after eval — it is the compiled objective");

    let mass_si = match mass_val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!(
            "expected a resolved Scalar for {TEMPLATE_NAME}.mass (the minimize \
             objective), got {other:?}. An Undef objective would leave the optimiser \
             with no gradient to descend."
        ),
    };
    assert!(
        mass_si.is_finite() && mass_si > 0.0,
        "mass must be finite and positive, got {mass_si:?}"
    );

    // Consistency with the closed-form the example declares:
    //   mass = material.density * length * width * thickness
    // Checked against the SAME resolved thickness, so this is an internal-coherence
    // pin (the objective really is that product of the design variable), not a
    // calibrated numeric expectation about where the loop lands.
    let expected_mass_si = STEEL_DENSITY_SI * LENGTH_SI * WIDTH_SI * thickness_si;
    let rel_err = (mass_si - expected_mass_si).abs() / expected_mass_si;
    assert!(
        rel_err < 1e-9,
        "mass ({mass_si:.9e} kg) should equal density*length*width*thickness \
         ({expected_mass_si:.9e} kg) at the resolved thickness {thickness_si:.6e} m; \
         relative error {rel_err:.3e}. A mismatch means the objective is not the \
         closed-form mass the example documents."
    );
}
