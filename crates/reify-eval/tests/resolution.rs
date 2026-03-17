// Tests for resolution engine — wiring solver into eval pipeline.

use std::collections::HashMap;

use reify_eval::Engine;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver, TopologyTemplateBuilder,
    mm,
};
use reify_types::{ModulePath, Type, Value, ValueCellId};

#[test]
fn engine_with_solver_accepts_solver() {
    let mut solved_values = HashMap::new();
    solved_values.insert(ValueCellId::new("S", "x"), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);
    // No panic, x is in values (may still be Undef until resolution phase is added)
    assert!(result.values.get(&ValueCellId::new("S", "x")).is_some());
}
