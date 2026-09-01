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
//! (b) `example_lambda_sweep_boundary_blend_centre`: reads the SHIPPED
//!     `examples/cost_robustness_tradeoff.ri` from disk and evals its λ sweep
//!     (λ=1.0, λ=0.5, λ=0.0 over one shared Money cost and one shared constraint
//!     set) plus the file's objectiveless `CentralityReference` control. Asserts
//!     zero error-severity diagnostics, every λ strictly inside the feasible
//!     region, and — per task #5715 — a REAL, SEED-INDEPENDENT λ signal: the λ=1
//!     anchor reaches the cost's closed-form interior optimum, the λ=0 anchor
//!     reaches the Chebyshev centre (cross-checked against `CentralityReference`,
//!     which the engine resolves through its own synthesised centrality default),
//!     and the three λ are strictly ordered with a minimum pairwise separation —
//!     with every one of them provably distinct from the solver's
//!     constraint-derived seed.
//!
//!     Both halves of that separation take a deliberate lever in the example.
//!     The λ=1 half needs a cost with a reachable INTERIOR optimum; the λ=0 half
//!     needs an ASYMMETRIC constraint set (`thickness * 2 < 30mm` alongside the
//!     `1mm`/`25mm` box), because for a plain two-sided box the derived seed
//!     midpoint IS the Chebyshev centre by construction and no choice of numbers
//!     could tell a real λ=0 solve apart from a seed fallback.
//!
//!     This replaces the #5618/#5715 characterization pin (`spread < 1e-9`),
//!     which recorded the degenerate state in which both anchors had collapsed
//!     onto that seed. The complementary zero-margin-BOUNDARY form of the λ=1
//!     invariant (monotone cost, explicit tight `AutoParam` bounds) stays
//!     verified at the solver level in
//!     `reify-constraints/tests/cost_robustness_tradeoff_blend.rs`.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Path to the shipped example, resolved relative to this crate's manifest directory
/// (mirrors `continuous_cost_min_example_e2e.rs::EXAMPLE_PATH`).
const EXAMPLE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/cost_robustness_tradeoff.ri");

/// Absolute tolerance (metres) for the λ-anchor convergence checks in test (b).
/// Mirrors `reify-constraints/tests/cost_robustness_tradeoff_blend.rs::ANCHOR_TOL_M`
/// so both layers of this contract quote one epsilon.
///
/// This is an ACHIEVABILITY bound, not a tuned value: every target it is compared
/// against is derived in closed form at its assertion site, and the measured
/// convergence errors on this problem are 4.6e-11 m (the λ=1 cost anchor) and
/// < 1e-17 m (the λ=0 centrality anchor) — six-plus orders of margin. It is also
/// far below every separation the test cares about (the smallest is ~2.2 mm), so
/// it cannot launder a collapsed sweep into a pass.
const ANCHOR_TOL_M: f64 = 1e-5;

/// Minimum pairwise separation (metres) required between consecutive λ results in
/// test (b). A FLOOR, not a target: the measured separations are 2.188 mm
/// (λ=1 → λ=0.5) and 3.145 mm (λ=0.5 → λ=0), i.e. both exceed it by more than 2x.
///
/// The point of a floor rather than a bare `<` is that a future degenerate
/// re-collapse of the two anchors — the exact #5715 failure this test replaces —
/// fails loudly instead of squeaking past on float noise.
const LAMBDA_SEPARATION_M: f64 = 1e-3;

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
/// across three structures sharing one Money cost and one constraint set, plus a
/// fourth objectiveless `CentralityReference` control carrying that same param and
/// the same constraints.
///
/// A REAL λ signal, seed-independent at this layer (task #5715). BOTH anchors are
/// pinned to independently-derived targets that are distinct from each other AND
/// from the solver's constraint-derived seed:
///
///   * λ=1 → the cost's closed-form interior optimum (5mm);
///   * λ=0 → the Chebyshev centre of the three-half-space feasible region
///     (31/3 mm), cross-checked against `CentralityReference`, which the engine
///     resolves through its own synthesised `Maximize(min_slack)` default;
///   * λ=0.5 → strictly between, with a minimum pairwise separation of
///     `LAMBDA_SEPARATION_M`.
///
/// This replaces the #5715 characterization pin (`spread < 1e-9`), which recorded
/// the degenerate state described in that task: with a MONOTONE cost the λ=1
/// optimum sits past a strict `>` boundary the floor-free anchor cannot reach
/// (the constraint penalty has zero slope at its own root, and `.ri`-compiled
/// autos always get `bounds: None` — `engine_eval.rs::build_auto_param_list`), so
/// `solve_core_with_sd_tolerance`'s drift fallback reported THE SEED; and for a
/// plain two-sided box that seed midpoint IS the Chebyshev centre, i.e. the λ=0
/// target, by construction. Both anchors therefore collapsed onto one point. The
/// example now breaks that second coincidence with an ASYMMETRIC third constraint
/// (`thickness * 2 < 30mm`), whose gradient differs from the box's, so the
/// Chebyshev centre and the bounding-box midpoint are no longer the same point.
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
    // The objectiveless control: same auto param, same three constraints, NO
    // `minimize`, so the engine resolves it through `build_centrality_objective`'s
    // synthesised `Maximize(min_slack)` default.
    let t_centrality = thickness_si("CentralityReference");

    // ── every result resolves strictly INSIDE the feasible region (1mm, 15mm) ──
    // Layer-appropriate and genuinely load-bearing: the example compiles, solves,
    // and every λ lands on a finite value satisfying ALL THREE user constraints.
    // The upper bound is 15mm, not 25mm: `thickness * 2 < 30mm` binds first.
    for (label, t) in [
        ("λ=1.0", t_pure_cost),
        ("λ=0.5", t_blend),
        ("λ=0.0", t_robust),
        ("CentralityReference", t_centrality),
    ] {
        assert!(
            t.is_finite() && t > 0.001 && t < 0.015,
            "{label} thickness should be finite and strictly inside (1mm, 15mm), got {:.6e} m",
            t
        );
    }

    // ── λ=1 ANCHOR: the cost's CLOSED-FORM interior optimum ────────────────────
    //
    // The example's cost is `unit_cost * (thickness / 1mm + 25mm / thickness)`,
    // i.e. `5USD · (a·t + b/t)` with `a = 1/(1mm)` and `b = 25mm`. Then
    //     d/dt (a·t + b/t) = a − b/t² = 0  →  t* = sqrt(b/a) = sqrt(25mm · 1mm) = 5mm,
    // and it is the UNIQUE global minimum on `t > 0` since the second derivative
    // `2b/t³` is strictly positive there.
    //
    // Why this is reachable where a monotone cost's optimum is not: 5mm is
    // STRICTLY INTERIOR to the feasible region, so the constraint penalty is
    // identically zero in a neighbourhood of it. There is no zero-slope-at-the-root
    // interaction with the penalty, no infeasible drift, and hence no
    // `solve_core_with_sd_tolerance` seed fallback — the very failure mode that
    // made the old monotone-cost sweep flat (task #5715, γ #4791).
    //
    // 5mm is NOT the solver's constraint-derived seed (13mm, measured) and NOT the
    // robustness/Chebyshev centre. This assertion can therefore only pass if the
    // λ=1 anchor genuinely optimised the cost.
    assert!(
        (t_pure_cost - 0.005).abs() < ANCHOR_TOL_M,
        "λ=1.0 must reach the cost's closed-form interior optimum \
         sqrt(25mm · 1mm) = 5mm (floor-free pure cost-minimisation); got {:.6e} m. \
         Landing on ~1.3e-2 m means the anchor fell back to the constraint-derived \
         seed — see task #5715.",
        t_pure_cost,
    );

    // ── λ=0 ANCHOR: a GENUINE Chebyshev centre, cross-checked and seed-free ────
    //
    // (i) ENGINE-COMPUTED reference. `CentralityReference` carries the same auto
    // param and the same three constraints with NO objective at all, so the
    // engine resolves it through `build_centrality_objective`'s synthesised
    // `Maximize(min_slack)` default. PRD §8.1 requires λ=0 to reduce to exactly
    // that objective, so the two must agree. This is the in-example analogue of
    // the solver-level sibling's independent `build_centrality_objective` solve:
    // a target the ENGINE computes, not a lone hardcoded constant.
    assert!(
        (t_robust - t_centrality).abs() < ANCHOR_TOL_M,
        "λ=0.0 must reduce to the engine's own centrality default (PRD §8.1); got \
         t(λ=0)={:.6e} m vs objectiveless CentralityReference={:.6e} m",
        t_robust,
        t_centrality,
    );

    // (ii) CLOSED FORM. The slacks are `t − 1mm`, `25mm − t` and `30mm − 2t`.
    // Maximising their minimum is a 1-D Chebyshev problem: the binding pair is
    // `t − 1mm` (slope +1) against `30mm − 2t` (slope −2), so
    //     t − 1 = 30 − 2t  →  t = 31/3 mm ≈ 10.3333mm,
    // at which the remaining slack `25mm − t = 14.667mm` comfortably exceeds the
    // binding value `9.333mm` and is therefore non-binding.
    assert!(
        (t_robust - 31.0 / 3.0 * 0.001).abs() < ANCHOR_TOL_M,
        "λ=0.0 must reach the closed-form Chebyshev centre 31/3 mm ≈ 1.03333e-2 m \
         of the three-half-space region; got {:.6e} m",
        t_robust,
    );

    // (iii) SEED-INDEPENDENCE TRIPWIRE — the assertion this whole task exists to
    // make possible (#5715). The λ=0 result must NOT be the solver's
    // constraint-derived box midpoint.
    //
    // Where 13mm comes from: `extract_initial_point` (solver.rs) arm 3 derives a
    // box from the constraints, and `derive_from_side` recognises only the shapes
    // `p OP far`, `p − k OP far` and `k − p OP far`. `thickness * 2 < 30mm` has a
    // `Mul` on the near side, so it contributes a centrality slack but does NOT
    // tighten the derived box — the box stays `[1mm, 25mm]` and its midpoint is
    // 13mm (measured directly with a monotone-cost drift-fallback probe).
    //
    // This is not a hidden coupling to that gap: if `derive_from_side` is ever
    // widened to recognise scaled shapes, the derived box becomes `[1mm, 15mm]`
    // and the seed moves to 8mm — still more than LAMBDA_SEPARATION_M away from
    // all three λ values (3.0mm / 0.81mm / 2.33mm), so the assertion keeps
    // meaning the same thing.
    assert!(
        (t_robust - 0.013).abs() > LAMBDA_SEPARATION_M,
        "λ=0.0 must be distinguishable from the constraint-derived seed midpoint \
         (13mm): landing there would make a real centrality solve and a drift \
         fallback indistinguishable, which is exactly the #5715 defect. got \
         {:.6e} m",
        t_robust,
    );

    // ── λ ORDERING with a minimum pairwise SEPARATION ──────────────────────────
    //
    // The direct inversion of the #5715 characterization pin this replaces
    // (`spread < 1e-9`, "all three λ coincide at this layer"). Increasing λ weights
    // the cost anchor, decreasing λ weights the robustness anchor, so the sweep
    // must move monotonically from the cost optimum up towards the robust centre.
    //
    // A separation FLOOR rather than a bare `<` is the load-bearing choice: it
    // makes a degenerate re-collapse of the two anchors fail loudly instead of
    // squeaking past on float noise.
    assert!(
        t_blend - t_pure_cost > LAMBDA_SEPARATION_M,
        "λ=0.5 must sit at least {:.3e} m ABOVE λ=1.0 (the blend pulls away from the \
         pure-cost optimum towards the robust centre); got t(λ=1)={:.6e}, \
         t(λ=0.5)={:.6e}, separation {:.6e} m",
        LAMBDA_SEPARATION_M,
        t_pure_cost,
        t_blend,
        t_blend - t_pure_cost,
    );
    assert!(
        t_robust - t_blend > LAMBDA_SEPARATION_M,
        "λ=0.0 must sit at least {:.3e} m ABOVE λ=0.5 (pure robustness goes all the \
         way to the centre); got t(λ=0.5)={:.6e}, t(λ=0)={:.6e}, separation {:.6e} m",
        LAMBDA_SEPARATION_M,
        t_blend,
        t_robust,
        t_robust - t_blend,
    );
}
