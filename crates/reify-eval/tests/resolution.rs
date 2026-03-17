// Tests for resolution engine — wiring solver into eval pipeline.

use std::collections::HashMap;

use reify_eval::Engine;
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver, TopologyTemplateBuilder,
    gt, lt, literal, value_ref, mm,
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
