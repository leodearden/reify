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
//! (a) step-07 (RED until step-08) `tradeoff_scope_suppresses_floor_diagnostic_sibling_does_not`:
//!     two sibling top-level structures — one `minimize cost_robustness_tradeoff(<money>, 0.5)`,
//!     one plain `minimize <money>` — each with its OWN objective (so F-inherit
//!     inheritance, #4824, cannot confound which scope the diagnostic belongs to).
//!     Only the plain scope qualifies for `RobustnessFloorApplied`; the tradeoff
//!     scope must NOT, since it replaces the floor with its own two-anchor blend.
//!     Fails today because `scope_qualifies_for_robustness_floor` keys only on
//!     Money + inequality and does not yet check `cost_robustness_lambda`.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

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
