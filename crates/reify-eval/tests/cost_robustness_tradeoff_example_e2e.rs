//! Eval-level e2e tests for task 4791 (cost-min γ): the
//! `minimize cost_robustness_tradeoff(<money-expr>, λ)` special form.
//!
//! PRD `docs/prds/v0_6/continuous-cost-minimisation.md` §2.4/§8.1: the tradeoff
//! form REPLACES the α robustness floor (#4789) rather than composing with it.
//! Import set and compile→eval→assert skeleton mirror
//! `crates/reify-eval/tests/robustness_floor_signal.rs`.
//!
//! # Tests
//!
//! (a) step-07/step-08 (GREEN) `tradeoff_scope_suppresses_floor_diagnostic_sibling_does_not`:
//!     two sibling top-level structures — one `minimize cost_robustness_tradeoff(<money>, 0.5)`,
//!     one plain `minimize <money>` — each with its OWN objective (so F-inherit
//!     inheritance, #4824, cannot confound which scope the diagnostic belongs to).
//!     Only the plain scope qualifies for `RobustnessFloorApplied`; the tradeoff
//!     scope must NOT, since it replaces the floor with its own two-anchor blend.
//!
//! (b) step-09/step-10 (GREEN) `example_lambda_sweep_boundary_blend_centre`: reads
//!     the SHIPPED `examples/cost_robustness_tradeoff.ri` from disk and evals its
//!     three structures (λ=1.0, λ=0.5, λ=0.0 over the same Money cost and
//!     two-sided `1mm < thickness < 25mm` box, Chebyshev centre 13mm). Asserts
//!     zero error-severity diagnostics, every λ strictly inside the box, and —
//!     per task #5715 — that all three λ COINCIDE at this layer. It formerly
//!     asserted a strict ordering; that assertion rested on the pre-#5618 fixed
//!     seed and never proved λ drives the blend. The real λ contract is verified
//!     seed-independently at the solver level in
//!     `cost_robustness_tradeoff_blend.rs` with explicit tight `AutoParam` bounds.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Path to the shipped example, resolved relative to this crate's manifest directory
/// (mirrors `continuous_cost_min_example_e2e.rs::EXAMPLE_PATH`).
const EXAMPLE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/cost_robustness_tradeoff.ri");

/// Two sibling top-level structures sharing the same shape (one `Length = auto(free)`
/// param, one `Money` unit cost, one `>` inequality) so the ONLY variable between
/// them is the objective form — isolating the floor-suppression behaviour under
/// test. `auto(free)` sidesteps the perturbation-based uniqueness re-solve (which
/// is not tradeoff-aware — out of scope for γ, see solver.rs::verify_uniqueness),
/// mirroring `cost_min_robustness_floor.ri`'s use of `auto(free)` for the same
/// reason.
fn tradeoff_vs_plain_source() -> &'static str {
    r#"structure TradeoffScope {
    param t: Length = auto(free)
    param c: Money = 5USD

    constraint t > 1mm

    minimize cost_robustness_tradeoff(c * (t / 1mm), 0.5)
}

structure PlainMoneyScope {
    param t: Length = auto(free)
    param c: Money = 5USD

    constraint t > 1mm

    minimize c * (t / 1mm)
}"#
}

/// (a) HEADLINE: `cost_robustness_tradeoff` REPLACES the α robustness floor.
///
/// `TradeoffScope` must NOT emit `RobustnessFloorApplied` (its own two-anchor
/// blend supersedes the floor); the sibling `PlainMoneyScope` — identical shape,
/// ordinary `minimize` — still does, proving the suppression is keyed on the
/// `cost_robustness_lambda` marker and not some accidental byproduct (e.g. a
/// blanket suppression, or the fixture failing to qualify at all).
#[test]
fn tradeoff_scope_suppresses_floor_diagnostic_sibling_does_not() {
    let compiled = compile_source_with_stdlib(tradeoff_vs_plain_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // Both scopes must actually resolve (Solved) — a spurious Infeasible/NoProgress
    // would make the diagnostic assertions below vacuous.
    for (entity, cell) in [("TradeoffScope", "t"), ("PlainMoneyScope", "t")] {
        let id = ValueCellId::new(entity, cell);
        match result.values.get(&id) {
            Some(Value::Scalar { si_value, .. }) => assert!(
                *si_value > 0.001,
                "{entity}.{cell} should resolve strictly above the 1mm boundary; got {:.6} m",
                si_value
            ),
            other => panic!("expected Scalar for {entity}.{cell}, got {:?}", other),
        }
    }

    // PRIMARY assertion: exactly one RobustnessFloorApplied (Info), and it names
    // the plain scope, NOT the tradeoff scope.
    let floor_applied: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorApplied))
        .collect();
    assert_eq!(
        floor_applied.len(),
        1,
        "expected exactly one RobustnessFloorApplied diagnostic (PlainMoneyScope only, \
         TradeoffScope's own blend replaces the floor); got {}: {:#?}",
        floor_applied.len(),
        result.diagnostics,
    );
    assert!(
        floor_applied[0].message.contains("PlainMoneyScope"),
        "the sole RobustnessFloorApplied diagnostic should name PlainMoneyScope, got: {:?}",
        floor_applied[0].message,
    );
    assert!(
        !floor_applied[0].message.contains("TradeoffScope"),
        "RobustnessFloorApplied must not name TradeoffScope (its own tradeoff blend \
         replaces the floor), got: {:?}",
        floor_applied[0].message,
    );
}

/// (b) the shipped `examples/cost_robustness_tradeoff.ri` sweeps λ=1.0 → 0.5 → 0.0
/// across three structures sharing the same Money cost and two-sided
/// `1mm < thickness < 25mm` box.
///
/// NOT a λ signal — see the characterization pin below and task #5715. Both the
/// λ=1 and λ=0 anchors land on the box midpoint at this layer, so the sweep is
/// observably flat here; the λ contract is proven in
/// `reify-constraints/tests/cost_robustness_tradeoff_blend.rs`.
///
/// Reads the example from disk (not a fixture copy) — mirrors
/// `continuous_cost_min_example_e2e.rs`'s disk-path convention; compile-level
/// regressions are caught first by the bulk gate `examples_smoke.rs`, so this
/// test's compile check is a fast-fail precondition for the eval assertions below.
#[test]
fn example_lambda_sweep_boundary_blend_centre() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).unwrap_or_else(|e| {
        panic!(
            "Could not read {}: {} — run step-10 to create the example file",
            EXAMPLE_PATH, e
        )
    });

    // ── compile ────────────────────────────────────────────────────────────────
    let compiled = compile_source_with_stdlib(&src);
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "examples/cost_robustness_tradeoff.ri should compile without errors: {:#?}",
        compile_errors
    );

    // ── eval ───────────────────────────────────────────────────────────────────
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval of examples/cost_robustness_tradeoff.ri should produce no error-severity \
         diagnostics: {:#?}",
        eval_errors
    );

    let thickness_si = |entity: &str| -> f64 {
        let id = ValueCellId::new(entity, "thickness");
        match result.values.get(&id) {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("expected Scalar for {entity}.thickness, got {:?}", other),
        }
    };

    let t_pure_cost = thickness_si("TradeoffPureCost"); // λ=1.0
    let t_blend = thickness_si("TradeoffBlend"); // λ=0.5
    let t_robust = thickness_si("TradeoffRobust"); // λ=0.0

    // ── all three λ resolve strictly INSIDE the open box (1mm, 25mm) ───────────
    // Layer-appropriate and genuinely load-bearing: the example compiles, solves,
    // and every λ lands on a finite value satisfying both user constraints.
    for (label, t) in [("λ=1.0", t_pure_cost), ("λ=0.5", t_blend), ("λ=0.0", t_robust)] {
        assert!(
            t.is_finite() && t > 0.001 && t < 0.025,
            "{label} thickness should be finite and strictly inside (1mm, 25mm), got {:.6e} m",
            t
        );
    }

    // ── CHARACTERIZATION PIN (task #5715): all three λ coincide at this layer ──
    //
    // This test formerly asserted the strict ordering t(λ=1) < t(λ=0.5) < t(λ=0),
    // calling it "the reliable, solver-independent signal at this layer". It was
    // neither reliable nor solver-independent, and it never proved λ drives the
    // blend. Two compounding facts, BOTH structural:
    //
    //  1. `.ri`-compiled autos always get `bounds: None` (engine_eval.rs), so the
    //     floor-free λ=1 cost anchor cannot reach the strict `>` boundary — the
    //     constraint penalty has zero slope at its own root — and
    //     `solve_core_with_sd_tolerance`'s drift-fallback returns THE SEED. The
    //     example's header comment always said so: the λ=1 value was never 1mm.
    //  2. For a two-sided box `lo < t < hi` the constraint-derived seed midpoint
    //     `(lo+hi)/2` IS the Chebyshev centre — the λ=0 target — BY CONSTRUCTION.
    //     No choice of numbers separates them.
    //
    // So λ=1 returns the seed midpoint and λ=0 returns the Chebyshev centre, and
    // those are one point (13mm here). The old ordering passed only because the
    // PRE-#5618 seed was a FIXED 10mm that happened to sit below 13mm; task #5618
    // derived the seed from the constraint box, the two collapsed, and the
    // coincidence that had been carrying the assertion disappeared. The blend is
    // not broken — its two anchors genuinely coincide at this layer.
    //
    // The λ contract itself is verified rigorously and seed-independently at the
    // SOLVER level in `reify-constraints/tests/cost_robustness_tradeoff_blend.rs`
    // (λ=1 → true 1mm boundary, λ=0 → 2.5mm centre, λ=0.5 strictly between), which
    // deliberately picks bounds whose midpoint matches NEITHER target so a pass
    // "can only come from the solver actually reaching the target, never from a
    // seed/target coincidence". Those 3 tests are green. Nothing is uncovered here.
    //
    // Pinned as equality rather than deleted so this stays a tripwire: when #5715
    // gives the λ dial a real `.ri`-layer signal, this assertion FAILS and whoever
    // lands it restores an ordering check that actually means something.
    let spread = t_pure_cost.max(t_blend).max(t_robust)
        - t_pure_cost.min(t_blend).min(t_robust);
    assert!(
        spread < 1e-9,
        "#5715 pins all three λ as coincident at the .ri layer (both anchors land on \
         the 13mm box midpoint). A nonzero spread means λ became observable here — \
         good news: replace this pin with a real ordering assertion. got t(λ=1)={:.6e}, \
         t(λ=0.5)={:.6e}, t(λ=0)={:.6e}",
        t_pure_cost,
        t_blend,
        t_robust,
    );
}
