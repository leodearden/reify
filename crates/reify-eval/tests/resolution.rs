// Tests for resolution engine — wiring solver into eval pipeline.

use std::collections::HashMap;

use reify_eval::cache::NodeId;
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

#[test]
fn resolve_multiple_auto_params() {
    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");

    let mut solved_values = HashMap::new();
    solved_values.insert(a_id.clone(), mm(5.0));
    solved_values.insert(b_id.clone(), mm(10.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "a", Type::length())
        .auto_param("S", "b", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "a"), literal(mm(1.0))))
        .constraint("S", 1, None, gt(value_ref("S", "b"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);

    let a_val = result.values.get(&a_id).expect("a should be in values");
    assert!(
        matches!(a_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected a=mm(5.0), got {:?}",
        a_val
    );

    let b_val = result.values.get(&b_id).expect("b should be in values");
    assert!(
        matches!(b_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected b=mm(10.0), got {:?}",
        b_val
    );

    assert_eq!(result.resolved_params.len(), 2);
    assert!(result.resolved_params.contains_key(&a_id));
    assert!(result.resolved_params.contains_key(&b_id));
}

#[test]
fn solver_infeasible_produces_diagnostics() {
    use reify_types::Diagnostic;

    let thickness_id = ValueCellId::new("S", "thickness");

    let solver = MockConstraintSolver::new_infeasible(vec![
        Diagnostic::error("constraints are infeasible"),
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should remain Undef
    let thickness_val = result.values.get(&thickness_id).expect("thickness in values");
    assert!(thickness_val.is_undef(), "expected Undef, got {:?}", thickness_val);

    // diagnostics should contain infeasible message
    assert!(
        result.diagnostics.iter().any(|d| d.message.contains("infeasible")),
        "expected infeasible diagnostic, got {:?}",
        result.diagnostics
    );

    // resolved_params should be empty
    assert!(result.resolved_params.is_empty());
}

#[test]
fn solver_no_progress_produces_warning() {
    let thickness_id = ValueCellId::new("S", "thickness");

    let solver = MockConstraintSolver::new_no_progress("iteration limit reached");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should remain Undef
    let thickness_val = result.values.get(&thickness_id).expect("thickness in values");
    assert!(thickness_val.is_undef(), "expected Undef, got {:?}", thickness_val);

    // diagnostics should contain a warning about no progress
    assert!(
        result.diagnostics.iter().any(|d| d.message.contains("iteration limit reached")),
        "expected no-progress warning, got {:?}",
        result.diagnostics
    );

    // resolved_params should be empty
    assert!(result.resolved_params.is_empty());
}

#[test]
fn no_solver_backward_compatible() {
    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Engine WITHOUT with_solver() — solver is None
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

    let result = engine.eval(&module);

    // x should be Undef with Auto determinacy in snapshot
    let x_val = result.values.get(&x_id).expect("x in values");
    assert!(x_val.is_undef(), "expected Undef without solver, got {:?}", x_val);

    let snap = engine.snapshot().expect("snapshot should exist");
    let (val, det) = snap.values.get(&x_id).expect("x in snapshot");
    assert!(val.is_undef());
    assert_eq!(*det, DeterminacyState::Auto);

    // No diagnostics
    assert!(result.diagnostics.is_empty());

    // No resolved params
    assert!(result.resolved_params.is_empty());
}

#[test]
fn eval_result_tracks_resolved_params() {
    use reify_test_support::MockGeometryKernel;
    use reify_types::ExportFormat;

    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "thickness"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Test eval() resolved_params
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(MockConstraintSolver::new_solved({
            let mut m = HashMap::new();
            m.insert(thickness_id.clone(), mm(5.0));
            m
        })));
    let eval_result = engine.eval(&module);
    assert_eq!(eval_result.resolved_params.len(), 1);
    assert!(eval_result.resolved_params.contains_key(&thickness_id));

    // Test check() resolved_params
    let mut engine2 = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(MockConstraintSolver::new_solved({
            let mut m = HashMap::new();
            m.insert(thickness_id.clone(), mm(5.0));
            m
        })));
    let check_result = engine2.check(&module);
    assert_eq!(check_result.resolved_params.len(), 1);
    assert!(check_result.resolved_params.contains_key(&thickness_id));

    // Test build() resolved_params
    let mut engine3 = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    ).with_solver(Box::new(MockConstraintSolver::new_solved({
        let mut m = HashMap::new();
        m.insert(thickness_id.clone(), mm(5.0));
        m
    })));
    let build_result = engine3.build(&module, ExportFormat::Step);
    assert_eq!(build_result.resolved_params.len(), 1);
    assert!(build_result.resolved_params.contains_key(&thickness_id));
}

#[test]
fn resolution_cache_version_matches_snapshot() {
    // Build module with auto param x, let binding y = x * 2, constraint x > 2mm
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
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
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    let _result = engine.eval(&module);

    // Get snapshot version after resolution
    let snap = engine.snapshot().expect("snapshot should exist");
    let snap_version = snap.version;

    // Get cache entry for auto param x
    let x_cache = engine
        .cache_store()
        .get(&NodeId::Value(x_id.clone()))
        .expect("x should be cached");

    // Get cache entry for let binding y
    let y_cache = engine
        .cache_store()
        .get(&NodeId::Value(y_id.clone()))
        .expect("y should be cached");

    // Both cache entries' basis_version must match the snapshot's version.
    // This is the invariant that try_fast_path relies on: if basis_version
    // doesn't match snapshot.version, subsequent edit_param() calls will
    // never hit the fast path for resolution-phase entries, forcing full
    // dependency-trace evaluation even for unaffected nodes.
    assert_eq!(
        x_cache.basis_version, snap_version,
        "auto param x cache basis_version ({:?}) should match snapshot version ({:?})",
        x_cache.basis_version, snap_version
    );
    assert_eq!(
        y_cache.basis_version, snap_version,
        "let binding y cache basis_version ({:?}) should match snapshot version ({:?})",
        y_cache.basis_version, snap_version
    );
}
