// Integration tests for the resolve_order-wired eval() resolution loop (task #4822, β).
//
// These tests exercise the EVAL path at engine level — verifying that after
// the resolve_order wiring (step-6), `eval()` solves scopes in dependency
// order so a later-declared scope that reads an earlier-declared scope's auto
// cell actually sees the solved value (INV-1 / BT3).
//
// Strategy: use MultiCallSpyConstraintSolver (reify-test-support) to capture
// the ResolutionProblem presented to the solver on each call, in call order.
// Assert the auto_params and current_values of each captured problem directly.

use std::collections::HashMap;

use reify_core::{ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::SolveResult;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MultiCallSpyConstraintSolver,
    TopologyTemplateBuilder, gt, literal, mm, value_ref,
};

// ---------------------------------------------------------------------------
// step-5 case (a): cross-scope value flow (INV-1 / BT3)
//
// Module in source order [Later, Leaf] where Later reads Leaf.k.
// After step-6 wiring, eval() should call the solver for Leaf first, then
// Later — and Later's ResolutionProblem.current_values must contain Leaf.k
// with the solved value from Leaf's solve call.
// ---------------------------------------------------------------------------

/// Verify that eval() solves scopes in resolve_order order (Leaf before Later)
/// and that Later's solver problem sees Leaf's SOLVED value in current_values.
///
/// RED until step-6 wires resolve_order into eval() — before that, eval()
/// walks source order [Later, Leaf], so Later is solved first and does NOT see
/// Leaf's solved value.
#[test]
fn eval_solves_leaf_first_and_later_sees_solved_leaf_value() {
    let leaf_k = ValueCellId::new("Leaf", "k");
    let later_y = ValueCellId::new("Later", "y");

    // Source order: [Later=0, Leaf=1]
    // Later reads Leaf.k — so Leaf must be solved first (resolve_order returns [1, 0]).
    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        // Later.y > Leaf.k  (cross-scope dependency)
        .constraint(
            "Later",
            0,
            None,
            gt(value_ref("Leaf", "k"), literal(mm(0.0))),
        )
        .build();

    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint("Leaf", 0, None, gt(value_ref("Leaf", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(later)
        .template(leaf)
        .build();

    // Solver: call-0 solves Leaf.k = 5mm, call-1 solves Later.y = 10mm.
    let leaf_k_solved = {
        let mut m = HashMap::new();
        m.insert(leaf_k.clone(), mm(5.0));
        SolveResult::Solved {
            values: m,
            unique: true,
        }
    };
    let later_y_solved = {
        let mut m = HashMap::new();
        m.insert(later_y.clone(), mm(10.0));
        SolveResult::Solved {
            values: m,
            unique: true,
        }
    };

    let spy = MultiCallSpyConstraintSolver::new(vec![leaf_k_solved, later_y_solved]);
    let captured = spy.captured_problems();
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(spy));

    let _result = engine.eval(&module);

    let problems = captured.lock().unwrap();

    // Exactly 2 solve calls — one per scope.
    assert_eq!(
        problems.len(),
        2,
        "expected exactly 2 solve calls (one per scope); got {}",
        problems.len()
    );

    // The FIRST solve call must be Leaf's (auto_params contains Leaf.k).
    let first = &problems[0];
    let first_is_leaf = first.auto_params.iter().any(|ap| ap.id == leaf_k);
    assert!(
        first_is_leaf,
        "first solve call must be Leaf's (Leaf.k in auto_params); got auto_params: {:?}",
        first.auto_params.iter().map(|ap| &ap.id).collect::<Vec<_>>()
    );

    // The SECOND solve call must be Later's.
    let second = &problems[1];
    let second_is_later = second.auto_params.iter().any(|ap| ap.id == later_y);
    assert!(
        second_is_later,
        "second solve call must be Later's (Later.y in auto_params); got auto_params: {:?}",
        second.auto_params.iter().map(|ap| &ap.id).collect::<Vec<_>>()
    );

    // Later's problem must contain Leaf.k in current_values (Later sees Leaf's solved value).
    // We check presence, not the exact value — presence is sufficient to verify that
    // Leaf was solved first and its result was propagated into Later's problem context.
    let later_sees_leaf_k = second.current_values.get(&leaf_k).is_some();
    assert!(
        later_sees_leaf_k,
        "Later's ResolutionProblem.current_values must contain Leaf.k \
         (Later sees Leaf's solved value); current_values keys: {:?}",
        second
            .current_values
            .iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-5 case (b): back-compat — uncoupled module solved in source order (INV-2)
// ---------------------------------------------------------------------------

/// Two-scope module with NO cross-scope reads: solve order must equal source
/// order [X=0, Y=1] (INV-2 back-compat identity).
#[test]
fn eval_uncoupled_module_solved_in_source_order() {
    let x_a = ValueCellId::new("X", "a");
    let y_b = ValueCellId::new("Y", "b");

    let x = TopologyTemplateBuilder::new("X")
        .auto_param("X", "a", Type::length())
        .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
        .build();

    let y = TopologyTemplateBuilder::new("Y")
        .auto_param("Y", "b", Type::length())
        .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(x)
        .template(y)
        .build();

    let x_solved = {
        let mut m = HashMap::new();
        m.insert(x_a.clone(), mm(1.0));
        SolveResult::Solved { values: m, unique: true }
    };
    let y_solved = {
        let mut m = HashMap::new();
        m.insert(y_b.clone(), mm(2.0));
        SolveResult::Solved { values: m, unique: true }
    };

    let spy = MultiCallSpyConstraintSolver::new(vec![x_solved, y_solved]);
    let captured = spy.captured_problems();
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(spy));

    let _result = engine.eval(&module);

    let problems = captured.lock().unwrap();
    assert_eq!(problems.len(), 2);

    // First call is X's, second is Y's — source order preserved.
    let first_is_x = problems[0].auto_params.iter().any(|ap| ap.id == x_a);
    let second_is_y = problems[1].auto_params.iter().any(|ap| ap.id == y_b);
    assert!(
        first_is_x,
        "first call must be X's (INV-2 source order); got: {:?}",
        problems[0].auto_params.iter().map(|ap| &ap.id).collect::<Vec<_>>()
    );
    assert!(
        second_is_y,
        "second call must be Y's (INV-2 source order); got: {:?}",
        problems[1].auto_params.iter().map(|ap| &ap.id).collect::<Vec<_>>()
    );
}
