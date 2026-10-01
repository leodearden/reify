//! Eval-level e2e tests: a MONOTONE `cost_robustness_tradeoff` cost reaches the
//! constraint BOUNDARY at λ=1 on the production `.ri` auto shape.
//!
//! `.ri`-compiled auto params carry `bounds: None`, so the solver's own default
//! box is `[1µm, 10m]`; the γ regime derives its clamp box from the constraints,
//! because the boundary is PRD `docs/prds/v0_6/continuous-cost-minimisation.md`
//! §8.1's contracted λ=1 answer. For a STRICT bound that infimum is not
//! attained, so the box stops one representable value inside it (task #7883).
//! The language spec (`cost_robustness_tradeoff`, "(2) Layer"), PRD §2.4's
//! precision note and the `examples/cost_robustness_tradeoff.ri` header all
//! state this as MEASURED at the eval layer; the first test is that
//! measurement.
//!
//! The second test pins the CONSEQUENCE one layer up. `Engine::eval` never
//! runs the constraint checker — `Engine::check` does, and it is what
//! `reify check` runs — so "eval accepts the boundary silently" says nothing
//! about the checker. The checker compares exactly, so it must ACCEPT the λ=1
//! value: the model's own strict constraints hold there.

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
/// - `StrictCostFalling`: strict `auto`, λ=1, over the DECREASING cost
///   `5USD · (1mm/t)` — the mirror case, whose boundary is the strict UPPER
///   bound.
///
/// In every structure the strict lower bound is constraint index 0 and the
/// strict upper bound is constraint index 1.
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
}

structure StrictCostFalling {
    param thickness : Length = auto
    param unit_cost : Money = 5USD

    constraint thickness > 1mm
    constraint thickness < 4mm

    minimize cost_robustness_tradeoff(unit_cost * (1mm / thickness), 1.0)
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

    let thickness_of =
        |entity: &str| match result.values.get(&ValueCellId::new(entity, "thickness")) {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("expected a resolved Scalar for {entity}.thickness, got {other:?}"),
        };

    for (entity, target_si_m) in [
        ("StrictCost", 0.001),
        ("FreeCost", 0.001),
        ("StrictRobust", 0.0025),
        ("StrictCostFalling", 0.004),
    ] {
        let si_value = thickness_of(entity);
        assert!(
            (si_value - target_si_m).abs() < ANCHOR_TOL_M,
            "{entity}.thickness must resolve {target_si_m} m; got {si_value:.9e} m"
        );
    }

    for entity in ["StrictCost", "FreeCost"] {
        let si_value = thickness_of(entity);
        assert!(
            si_value > 0.001,
            "{entity}: the λ=1 boundary value must lie INSIDE the strict `thickness > 1mm`, \
             not on it; got {si_value:.17e} m"
        );
    }
    let si_value = thickness_of("StrictCostFalling");
    assert!(
        si_value < 0.004,
        "StrictCostFalling: the λ=1 boundary value must lie INSIDE the strict \
         `thickness < 4mm`, not on it; got {si_value:.17e} m"
    );

    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "every structure here is bounded on both sides by the model, so eval must \
         accept the λ=1 boundary value silently — no ConstraintNonUnique, no error of \
         any kind; got: {errors:#?}"
    );
}

/// The settled contract (task #7883, option (a)): the γ clamp box stops one
/// representable value inside a STRICT bound, so the λ=1 boundary answer PRD
/// §8.1/§8.2 sanction satisfies the model's own strict comparison. `check`
/// compares exactly, so every constraint of every structure — both λ=1
/// boundaries, lower and upper, and the λ=0 interior centre — is `Satisfied`,
/// and the check raises no error.
#[test]
fn monotone_tradeoff_lambda_one_boundary_value_passes_check() {
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

    for entity in [
        "StrictCost",
        "FreeCost",
        "StrictRobust",
        "StrictCostFalling",
    ] {
        for (index, constraint) in [(0, "thickness > 1mm"), (1, "thickness < 4mm")] {
            assert_eq!(
                verdict_of(entity, index),
                Satisfaction::Satisfied,
                "{entity}: the resolved thickness must satisfy the model's own strict \
                 `{constraint}` exactly — `reify check` compares with a bare f64 comparison"
            );
        }
    }

    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "check must accept every λ=1 boundary value with no error; got: {errors:#?}"
    );
}
