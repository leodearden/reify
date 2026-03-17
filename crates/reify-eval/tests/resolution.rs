// Tests for resolution engine — wiring solver into eval pipeline.

use std::collections::HashMap;

use reify_eval::Engine;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver, TopologyTemplateBuilder,
    binop, gt, lt, literal, value_ref, mm,
};
use reify_types::{DeterminacyState, ModulePath, SnapshotId, SnapshotProvenance, Type, Value, ValueCellId};

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

#[test]
fn resolve_single_auto_param() {
    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        // constraint: thickness > 2mm
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        // constraint: thickness < 20mm
        .constraint("S", 1, None, lt(value_ref("S", "thickness"), literal(mm(20.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should be resolved to mm(5.0), not Undef
    let thickness_val = result.values.get(&thickness_id).expect("thickness should be in values");
    // mm(5.0) = 0.005 SI
    assert!(
        matches!(thickness_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected mm(5.0) = 0.005 SI, got {:?}",
        thickness_val
    );
}

#[test]
fn resolved_param_determinacy_and_provenance() {
    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let _result = engine.eval(&module);
    let snap = engine.snapshot().expect("snapshot should exist");

    // Snapshot values: thickness should be (mm(5.0), Determined)
    let (val, det) = snap.values.get(&thickness_id).expect("thickness in snapshot");
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected mm(5.0) in snapshot, got {:?}",
        val
    );
    assert_eq!(*det, DeterminacyState::Determined);

    // Provenance should be Resolution with scope "S"
    let mut resolved_set = std::collections::HashSet::new();
    resolved_set.insert(thickness_id.clone());
    assert_eq!(
        snap.provenance,
        SnapshotProvenance::Resolution {
            scope: "S".to_string(),
            resolved: resolved_set,
            parent: SnapshotId(0),
        }
    );
}

#[test]
fn let_binding_re_evaluated_after_resolution() {
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(10.0)); // 0.01 m SI

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        // y = x * 2.0
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(
                reify_types::BinOp::Mul,
                value_ref("S", "x"),
                literal(Value::Real(2.0)),
            ),
        )
        // constraint: x > 2mm
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // y = x * 2.0 = 0.01 * 2.0 = 0.02 m SI
    let y_val = result.values.get(&y_id).expect("y should be in values");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected y ≈ 0.02 SI (10mm * 2), got {:?}",
        y_val
    );
}

#[test]
fn check_reports_satisfied_after_resolution() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_types::Satisfaction;

    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        // constraint: thickness > 2mm
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None)
        .with_solver(Box::new(solver));

    let result = engine.check(&module);

    // After resolution, thickness=5mm > 2mm → Satisfied
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "constraint should be satisfied after resolution, got {:?}",
        result.constraint_results[0].satisfaction
    );
}
