//! Eval-level e2e tests: a MONOTONE `cost_robustness_tradeoff` cost reaches the
//! zero-margin constraint BOUNDARY at λ=1 on the production `.ri` auto shape.
//!
//! `.ri`-compiled auto params carry `bounds: None`, so the solver's own default
//! box is `[1µm, 10m]`; the γ regime derives its clamp box from the constraints,
//! strict bounds included, because a value ON the boundary is PRD
//! `docs/prds/v0_6/continuous-cost-minimisation.md` §8.1's contracted λ=1
//! answer. The language spec (`cost_robustness_tradeoff`, "(2) Layer"), PRD
//! §2.4's precision note and the `examples/cost_robustness_tradeoff.ri` header
//! all state this as MEASURED at the eval layer; the first test is that
//! measurement.
//!
//! The second test records the CONSEQUENCE one layer up. `Engine::eval` never
//! runs the constraint checker — `Engine::check` does, and it is what
//! `reify check` runs — so "eval accepts the boundary silently" says nothing
//! about the checker. Measured: `check` reports the strict `t > 1mm` constraint
//! VIOLATED for the λ=1 value, because the value is exactly `1mm`.

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{ConstraintNodeId, ValueCellId};
use reify_eval::Engine;
use reify_ir::{Satisfaction, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib};

/// Mirrors `reify-constraints/tests/cost_robustness_tradeoff_blend.rs::ANCHOR_TOL_M`,
/// so both layers of the λ-anchor contract quote one epsilon.
const ANCHOR_TOL_M: f64 = 1e-5;

/// The solver-level `gamma_anchor_lambdas_are_seed_invariant_without_explicit_bounds`
/// shape — monotone `5USD · (t/1mm)` over `1mm < t < 4mm` — as `.ri` source,
/// one structure per case so each resolves independently:
///
/// - `StrictCost`: strict `auto`, λ=1 — the boundary, and the §11.6 verdict path
///   (`finalise_uniqueness` runs for a strict auto; both sides are bounded by
///   the model, so it must stay `Solved`).
/// - `FreeCost`: the same at `auto(free)`, which skips that verdict path — the
///   boundary must not depend on it.
/// - `StrictRobust`: strict `auto`, λ=0 — the centrality anchor, the 2.5mm
///   midpoint of the unit-gradient bracket.
///
/// In every structure the strict lower bound is constraint index 0.
fn monotone_tradeoff_source() -> &'static str {
    r#"structure StrictCost {
    param thickness : Length = auto
    param unit_cost : Money = 5USD

    constraint thickness > 1mm
    constraint thickness < 4mm

    minimize cost_robustness_tradeoff(unit_cost * (thickness / 1mm), 1.0)
}

structure FreeCost {
    param thickness : Length = auto(free)
    param unit_cost : Money = 5USD

    constraint thickness > 1mm
    constraint thickness < 4mm

    minimize cost_robustness_tradeoff(unit_cost * (thickness / 1mm), 1.0)
}

structure StrictRobust {
    param thickness : Length = auto
    param unit_cost : Money = 5USD

    constraint thickness > 1mm
    constraint thickness < 4mm

    minimize cost_robustness_tradeoff(unit_cost * (thickness / 1mm), 0.0)
}"#
}

fn engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(DimensionalSolver))
}

fn compiled_fixture() -> reify_compiler::CompiledModule {
    let compiled = compile_source_with_stdlib(monotone_tradeoff_source());
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "fixture should compile without errors: {compile_errors:#?}"
    );
    compiled
}

#[test]
fn monotone_tradeoff_lambda_one_resolves_the_boundary_at_eval() {
    let result = engine().eval(&compiled_fixture());

    for (entity, target_si_m) in [
        ("StrictCost", 0.001),
        ("FreeCost", 0.001),
        ("StrictRobust", 0.0025),
    ] {
        let id = ValueCellId::new(entity, "thickness");
        match result.values.get(&id) {
            Some(Value::Scalar { si_value, .. }) => assert!(
                (si_value - target_si_m).abs() < ANCHOR_TOL_M,
                "{entity}.thickness must resolve {target_si_m} m; got {si_value:.9e} m"
            ),
            other => panic!("expected a resolved Scalar for {entity}.thickness, got {other:?}"),
        }
    }

    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "every structure here is bounded on both sides by the model, so eval must \
         accept the λ=1 boundary value silently — no ConstraintNonUnique, no error of \
         any kind; got: {errors:#?}"
    );
}

/// A CHARACTERISATION test: it asserts today's behaviour, not a settled
/// contract. PRD §8.1/§8.2 sanction a λ=1 answer ON the boundary; for a STRICT
/// bound that answer is exactly the value the strict comparison rejects, so
/// `check` reports the model's own `thickness > 1mm` as violated for both λ=1
/// structures — and only for them, and only that constraint. Whether the γ
/// clamp should stop short of a strict bound, or the check should accept a
/// tradeoff-resolved boundary, is an open design question; if this test reds
/// because the verdict became `Satisfied`, that is the question being decided —
/// update this test deliberately rather than discovering the change.
#[test]
fn monotone_tradeoff_lambda_one_boundary_is_checked_violated_on_a_strict_bound() {
    let result = engine().check(&compiled_fixture());

    let verdict_of = |entity: &str, index: u32| {
        let id = ConstraintNodeId::new(entity, index);
        result
            .constraint_results
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.satisfaction)
            .unwrap_or_else(|| {
                panic!(
                    "no check verdict for {entity} constraint {index}; got: {:#?}",
                    result.constraint_results
                )
            })
    };

    for entity in ["StrictCost", "FreeCost"] {
        assert_eq!(
            verdict_of(entity, 0),
            Satisfaction::Violated,
            "{entity}: the λ=1 value is exactly 1mm, ON the strict `thickness > 1mm` bound"
        );
        assert_eq!(
            verdict_of(entity, 1),
            Satisfaction::Satisfied,
            "{entity}: the far side `thickness < 4mm` is not involved"
        );
    }
    for index in [0, 1] {
        assert_eq!(
            verdict_of("StrictRobust", index),
            Satisfaction::Satisfied,
            "StrictRobust: the λ=0 centre (2.5mm) is interior, so every constraint holds"
        );
    }
}
