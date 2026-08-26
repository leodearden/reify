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
//! Engine construction and the interior thresholds come from the shared
//! `fea_design_loop_support` submodule — the real `DimensionalSolver` plus real
//! FEA trampolines via the SINGLE bundler `register_production_compute_fns`
//! (INV-FEA-1) rather than hand-rolled legs. They were a verbatim copy of the
//! producer's until that module was extracted; the producer's own inline copy
//! still awaits migration (see `fea_design_loop_support`'s doc).
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

use crate::fea_design_loop_support::{
    INTERIOR_LOWER_THRESHOLD_SI, INTERIOR_UPPER_THRESHOLD_SI, fea_loop_engine,
};
use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{collect_errors, compile_source_with_stdlib};

/// The shipped example under test, resolved from `CARGO_MANIFEST_DIR` so it works
/// in any worktree.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fea_bracket_minimize_mass.ri"
);

const TEMPLATE_NAME: &str = "FeaBracketMinimizeMass";

/// `Steel_AISI_1045.density` (`crates/reify-compiler/stdlib/materials_fea.ri`) —
/// one input to the closed-form mass the example's objective minimises, used
/// below to check `mass` is the quantity it claims to be rather than merely *a*
/// finite number.
///
/// The OTHER inputs — the example's `length` and `width` — are deliberately NOT
/// hardcoded here: they are read back from the eval result. Pinning them would
/// silently re-encode the example's mounting envelope, so a benign resizing of
/// the bracket would fail this ~88s test with a message about the objective
/// expression while the thing that actually drifted was a constant in this file.
/// Density is different: it is sourced from the stdlib material, not from the
/// example, so it cannot drift with the example's geometry.
const STEEL_DENSITY_SI: f64 = 7850.0;

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

    // ── Engine: real solver + real FEA trampolines ───────────────────────────
    //
    // Shared with the other FEA-in-the-loop tests rather than hand-wired here —
    // see `fea_design_loop_support::fea_loop_engine` for the INV-FEA-1
    // single-bundler rule it encodes and why nothing mechanical guards it under
    // `tests/`.
    let mut engine = fea_loop_engine();

    let result = engine.eval(&compiled);

    // Read a resolved `Scalar`'s SI magnitude out of the eval result, or panic
    // naming the cell. Used below for the example's own fixed dimensions, so the
    // mass-coherence check reads them from the design rather than restating them.
    let resolved_si = |member: &str| -> f64 {
        match result.values.get(&ValueCellId::new(TEMPLATE_NAME, member)) {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => {
                panic!("expected a resolved Scalar for {TEMPLATE_NAME}.{member}, got {other:?}")
            }
        }
    };

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
    // Checked against the SAME resolved thickness AND the example's own resolved
    // `length`/`width`, so this is a pure internal-coherence pin (the objective
    // really is that product of the design variable) — neither a calibrated
    // expectation about where the loop lands nor a covert restatement of the
    // bracket's mounting envelope.
    let length_si = resolved_si("length");
    let width_si = resolved_si("width");
    let expected_mass_si = STEEL_DENSITY_SI * length_si * width_si * thickness_si;
    let rel_err = (mass_si - expected_mass_si).abs() / expected_mass_si;
    assert!(
        rel_err < 1e-9,
        "mass ({mass_si:.9e} kg) should equal density*length*width*thickness \
         ({expected_mass_si:.9e} kg) at the resolved length {length_si:.6e} m, width \
         {width_si:.6e} m and thickness {thickness_si:.6e} m; relative error \
         {rel_err:.3e}. Every factor but the stdlib density is read back from the \
         design, so a mismatch means the objective is not the closed-form mass the \
         example documents — not that the bracket was resized."
    );
}
