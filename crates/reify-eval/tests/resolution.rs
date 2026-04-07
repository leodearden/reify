// Tests for resolution engine — wiring solver into eval pipeline.

use std::collections::HashMap;

use reify_constraints::DimensionalSolver;
use reify_eval::cache::NodeId;
use reify_eval::{ConcurrentEditResult, Engine};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver,
    MultiCallSpyConstraintSolver, SpyConstraintSolver, TopologyTemplateBuilder, binop, gt, literal,
    lt, mm, value_ref,
};
use reify_types::{
    DeterminacyState, Diagnostic, ModulePath, OptimizationObjective, SnapshotId,
    SnapshotProvenance, Type, Value, ValueCellId,
};

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

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

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
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        // constraint: thickness < 20mm
        .constraint(
            "S",
            1,
            None,
            lt(value_ref("S", "thickness"), literal(mm(20.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should be resolved to mm(5.0), not Undef
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values");
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
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let _result = engine.eval(&module);
    let snap = engine.snapshot().expect("snapshot should exist");

    // Snapshot values: thickness should be (mm(5.0), Determined)
    let (val, det) = snap
        .values
        .get(&thickness_id)
        .expect("thickness in snapshot");
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

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

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
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(SimpleConstraintChecker), None).with_solver(Box::new(solver));

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

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

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

    let solver =
        MockConstraintSolver::new_infeasible(vec![Diagnostic::error("constraints are infeasible")]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should remain Undef
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness in values");
    assert!(
        thickness_val.is_undef(),
        "expected Undef, got {:?}",
        thickness_val
    );

    // diagnostics should contain infeasible message
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible")),
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
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // thickness should remain Undef
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness in values");
    assert!(
        thickness_val.is_undef(),
        "expected Undef, got {:?}",
        thickness_val
    );

    // diagnostics should contain a warning about no progress
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("iteration limit reached")),
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
    assert!(
        x_val.is_undef(),
        "expected Undef without solver, got {:?}",
        x_val
    );

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
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Test eval() resolved_params
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(
        Box::new(MockConstraintSolver::new_solved({
            let mut m = HashMap::new();
            m.insert(thickness_id.clone(), mm(5.0));
            m
        })),
    );
    let eval_result = engine.eval(&module);
    assert_eq!(eval_result.resolved_params.len(), 1);
    assert!(eval_result.resolved_params.contains_key(&thickness_id));

    // Test check() resolved_params
    let mut engine2 = Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(
        Box::new(MockConstraintSolver::new_solved({
            let mut m = HashMap::new();
            m.insert(thickness_id.clone(), mm(5.0));
            m
        })),
    );
    let check_result = engine2.check(&module);
    assert_eq!(check_result.resolved_params.len(), 1);
    assert!(check_result.resolved_params.contains_key(&thickness_id));

    // Test build() resolved_params
    let mut engine3 = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(MockGeometryKernel::new())),
    )
    .with_solver(Box::new(MockConstraintSolver::new_solved({
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

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

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

#[test]
fn incremental_fast_path_works_after_resolution() {
    // Module: auto x, param z (default mm(1.0)), let y = x*2, let w = z*3,
    // constraint x > 2mm.  Solver returns x = mm(5.0).
    let x_id = ValueCellId::new("S", "x");
    let z_id = ValueCellId::new("S", "z");
    let y_id = ValueCellId::new("S", "y");
    let w_id = ValueCellId::new("S", "w");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param("S", "z", Type::length(), Some(literal(mm(1.0))))
        // y = x * 2
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
        // w = z * 3
        .let_binding(
            "S",
            "w",
            Type::length(),
            binop(
                reify_types::BinOp::Mul,
                value_ref("S", "z"),
                literal(Value::Real(3.0)),
            ),
        )
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(2.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold-start eval: x resolved to mm(5.0), y = 0.01, z = mm(1.0), w = 0.003
    let result = engine.eval(&module);
    let y_val = result.values.get(&y_id).expect("y in values");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected y ≈ 0.01 (mm(5.0)*2), got {:?}",
        y_val
    );
    let w_val = result.values.get(&w_id).expect("w in values");
    assert!(
        matches!(w_val, Value::Scalar { si_value, .. } if (*si_value - 0.003).abs() < 1e-10),
        "expected w ≈ 0.003 (mm(1.0)*3), got {:?}",
        w_val
    );

    // Capture snapshot version after resolution for later comparison
    let resolution_snap_version = engine.snapshot().unwrap().version;

    // Incremental edit: change z to mm(2.0)
    let result2 = engine.edit_param(z_id.clone(), mm(2.0)).unwrap();

    // w should be updated: mm(2.0) * 3 = 0.006 SI
    let w_val2 = result2.values.get(&w_id).expect("w in values after edit");
    assert!(
        matches!(w_val2, Value::Scalar { si_value, .. } if (*si_value - 0.006).abs() < 1e-10),
        "expected w ≈ 0.006 (mm(2.0)*3) after edit, got {:?}",
        w_val2
    );

    // y should be unchanged: still x*2 = mm(5.0)*2 = 0.01 SI
    let y_val2 = result2.values.get(&y_id).expect("y in values after edit");
    assert!(
        matches!(y_val2, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected y ≈ 0.01 (unchanged after edit_param(z)), got {:?}",
        y_val2
    );

    // Verify y's cache entry still has the resolution-phase basis_version,
    // confirming the fast path was usable (y was NOT in the dirty cone for z).
    let y_cache = engine
        .cache_store()
        .get(&NodeId::Value(y_id.clone()))
        .expect("y should be cached");
    assert_eq!(
        y_cache.basis_version, resolution_snap_version,
        "y's cache basis_version ({:?}) should still be the resolution version ({:?}), \
         confirming it was not re-evaluated during edit_param(z)",
        y_cache.basis_version, resolution_snap_version
    );
}

#[test]
fn objective_forwarded_to_solver_in_eval() {
    // Build a template with an explicit Minimize objective.
    // The spy captures whatever ResolutionProblem the engine passes to solve().
    // This test fails until eval() wires template.objective into the problem.
    let thickness_id = ValueCellId::new("S", "thickness");
    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let spy = SpyConstraintSolver::new_solved(solved_values);
    let captured = spy.captured_problem();

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .objective(OptimizationObjective::Minimize(value_ref("S", "thickness")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    engine.eval(&module);

    let guard = captured.lock().unwrap();
    let problem = guard
        .as_ref()
        .expect("solver should have been called during eval");
    assert!(
        matches!(&problem.objective, Some(OptimizationObjective::Minimize(_))),
        "expected Minimize objective forwarded to solver, got {:?}",
        problem.objective
    );
}

#[test]
fn objective_forwarded_in_edit_param() {
    // After eval() with a Minimize objective, edit a regular param that appears
    // in the auto-param constraint, triggering re-resolution via edit_param().
    // The spy should capture a problem with the objective still set.
    // Fails until edit_param() looks up the objective from self.objectives.
    let thickness_id = ValueCellId::new("S", "thickness");
    let limit_id = ValueCellId::new("S", "limit");

    // Solver always returns thickness = 5mm
    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let spy = SpyConstraintSolver::new_solved(solved_values);
    let captured = spy.captured_problem();

    // Template: auto thickness, param limit (default 2mm),
    // constraint thickness > limit, objective Minimize(thickness)
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .param("S", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), value_ref("S", "limit")),
        )
        .objective(OptimizationObjective::Minimize(value_ref("S", "thickness")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Initial eval: solver called with objective=Some(Minimize(...))
    engine.eval(&module);

    // Change limit to 3mm — the constraint thickness > limit becomes dirty,
    // triggering re-resolution with a new ResolutionProblem.
    let _result = engine.edit_param(limit_id.clone(), mm(3.0)).unwrap();

    // The spy should now hold the problem from the edit_param() call.
    let guard = captured.lock().unwrap();
    let problem = guard
        .as_ref()
        .expect("solver should have been called during edit_param");
    assert!(
        matches!(&problem.objective, Some(OptimizationObjective::Minimize(_))),
        "expected Minimize objective forwarded to solver in edit_param, got {:?}",
        problem.objective
    );
}

#[test]
fn objective_forwarded_in_concurrent_edit() {
    // prepare_concurrent_edit + resolve_concurrent_edit should forward the
    // template objective to the ResolutionProblem. Fails until ConcurrentEditSetup
    // carries the objective and resolve_concurrent_edit uses it.
    use std::collections::HashSet;

    let thickness_id = ValueCellId::new("S", "thickness");
    let limit_id = ValueCellId::new("S", "limit");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let spy = SpyConstraintSolver::new_solved(solved_values);
    let captured = spy.captured_problem();

    // Template: auto thickness, param limit (default 2mm),
    // constraint thickness > limit, objective Minimize(thickness)
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .param("S", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), value_ref("S", "limit")),
        )
        .objective(OptimizationObjective::Minimize(value_ref("S", "thickness")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Initial eval: solver is called, objectives are cached on the engine
    engine.eval(&module);

    // Prepare a concurrent edit: change limit to 3mm
    let setup = engine
        .prepare_concurrent_edit(limit_id.clone(), mm(3.0))
        .unwrap();

    // Build a minimal ConcurrentEditResult from the setup values
    let mut result = ConcurrentEditResult {
        values: setup.values.clone(),
        snapshot_values: setup.snapshot_values.clone(),
        node_results: Vec::new(),
        actual_eval_set: Vec::new(),
        skipped: HashSet::new(),
        resolved_params: HashMap::new(),
        diagnostics: Vec::new(),
    };

    // resolve_concurrent_edit should detect dirty constraints and call the solver
    engine.resolve_concurrent_edit(&setup, &mut result);

    // The spy should now hold the problem from resolve_concurrent_edit
    let guard = captured.lock().unwrap();
    let problem = guard
        .as_ref()
        .expect("solver should have been called during resolve_concurrent_edit");
    assert!(
        matches!(&problem.objective, Some(OptimizationObjective::Minimize(_))),
        "expected Minimize objective forwarded to solver in resolve_concurrent_edit, got {:?}",
        problem.objective
    );
}

#[test]
fn no_objective_backward_compatible() {
    // Regression guard: when a template has NO objective, eval() and edit_param()
    // should still pass objective: None to the solver (existing behavior preserved).
    let thickness_id = ValueCellId::new("S", "thickness");
    let limit_id = ValueCellId::new("S", "limit");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    let spy = SpyConstraintSolver::new_solved(solved_values);
    let captured = spy.captured_problem();

    // Template with NO objective declared
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .param("S", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), value_ref("S", "limit")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // eval() with no objective: spy should capture problem with objective=None
    engine.eval(&module);
    {
        let guard = captured.lock().unwrap();
        let problem = guard
            .as_ref()
            .expect("solver should have been called during eval");
        assert!(
            problem.objective.is_none(),
            "expected None objective for template without objective, got {:?}",
            problem.objective
        );
    }

    // edit_param() with no objective: spy should still capture objective=None
    let _result = engine.edit_param(limit_id.clone(), mm(3.0)).unwrap();
    {
        let guard = captured.lock().unwrap();
        let problem = guard
            .as_ref()
            .expect("solver should have been called during edit_param");
        assert!(
            problem.objective.is_none(),
            "expected None objective in edit_param for template without objective, got {:?}",
            problem.objective
        );
    }
}

#[test]
fn e2e_minimize_through_real_solver() {
    // End-to-end integration test: uses the real DimensionalSolver with a
    // Minimize objective. With constraint thickness < 20mm and objective
    // Minimize(thickness), the solver converges near the effective lower bound
    // (~1 micron, from DimensionalSolver's default length bounds).
    //
    // Without the objective the solver exits early (initial point 10mm is already
    // feasible), returning ~10mm. With the objective it runs Nelder-Mead and
    // minimises to near 0. Asserting thickness < 5mm proves the objective is
    // wired through and affects the solve result.
    let thickness_id = ValueCellId::new("S", "thickness");

    // Template: auto thickness, constraint thickness < 20mm, minimize thickness.
    // Single upper-bound constraint avoids floating-point boundary issues at the lower bound.
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            lt(value_ref("S", "thickness"), literal(mm(20.0))),
        )
        .objective(OptimizationObjective::Minimize(value_ref("S", "thickness")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // Solver must succeed — no diagnostics expected
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics from eval: {:?}",
        result.diagnostics
    );

    // Thickness should be in values (auto param resolved by the solver)
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values after resolution");

    let si_value = match thickness_val {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for thickness, got {:?}", other),
    };

    // The Nelder-Mead minimiser should push thickness well below the initial guess
    // of 10mm (0.01m). Any value <= 5mm (0.005m) demonstrates that the Minimize
    // objective actually influenced the result.
    let five_mm_si = 5.0 * 0.001;
    assert!(
        si_value <= five_mm_si,
        "expected thickness <= 5mm when minimizing, got {:.6}m ({:.3}mm)",
        si_value,
        si_value * 1000.0,
    );
}

#[test]
fn eval_resolves_per_template_independently() {
    // Two independent templates: Bracket (auto thickness, Minimize) and Bolt (auto diameter, Maximize).
    // eval() should call the solver once per template, each with only that template's params/constraints/objective.
    use reify_types::SolveResult;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    let mut bracket_solved = HashMap::new();
    bracket_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_solved = HashMap::new();
    bolt_solved.insert(bolt_diameter.clone(), mm(10.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved {
            values: bracket_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: bolt_solved,
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    // Bracket: auto thickness, constraint thickness > 2mm, Minimize
    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .constraint(
            "Bracket",
            0,
            None,
            gt(value_ref("Bracket", "thickness"), literal(mm(2.0))),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    // Bolt: auto diameter, constraint diameter > 5mm, Maximize
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(value_ref("Bolt", "diameter"), literal(mm(5.0))),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    let result = engine.eval(&module);

    // Verify resolved values
    let t_val = result.values.get(&bracket_thickness).expect("thickness");
    assert!(
        matches!(t_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected thickness = mm(5.0), got {:?}",
        t_val
    );
    let d_val = result.values.get(&bolt_diameter).expect("diameter");
    assert!(
        matches!(d_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected diameter = mm(10.0), got {:?}",
        d_val
    );

    // (1) Solver called exactly twice — once per template
    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        2,
        "expected 2 solver calls (one per template), got {}",
        problems.len()
    );

    // (2) Each call has auto_params from only one template
    let call0_entities: Vec<&str> = problems[0]
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();
    let call1_entities: Vec<&str> = problems[1]
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();

    // One call should be Bracket-only, the other Bolt-only
    let has_bracket_only = (call0_entities == vec!["Bracket"] && call1_entities == vec!["Bolt"])
        || (call0_entities == vec!["Bolt"] && call1_entities == vec!["Bracket"]);
    assert!(
        has_bracket_only,
        "each solver call should have params from exactly one template, got call0={:?}, call1={:?}",
        call0_entities, call1_entities
    );

    // (3) Each call's objective matches the correct template
    for problem in problems.iter() {
        let entity = problem.auto_params[0].id.entity.as_str();
        match entity {
            "Bracket" => assert!(
                matches!(&problem.objective, Some(OptimizationObjective::Minimize(_))),
                "Bracket should have Minimize objective, got {:?}",
                problem.objective
            ),
            "Bolt" => assert!(
                matches!(&problem.objective, Some(OptimizationObjective::Maximize(_))),
                "Bolt should have Maximize objective, got {:?}",
                problem.objective
            ),
            other => panic!("unexpected entity: {}", other),
        }
    }
}

#[test]
fn edit_param_resolves_per_template_not_cross_template() {
    // After eval(), edit a param that affects Bracket's constraint.
    // The re-resolution should only involve Bracket's auto params, not Bolt's.
    use reify_types::SolveResult;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_limit = ValueCellId::new("Bracket", "limit");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    // eval() results: Bracket gets thickness=5mm, Bolt gets diameter=10mm
    let mut bracket_solved = HashMap::new();
    bracket_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_solved = HashMap::new();
    bolt_solved.insert(bolt_diameter.clone(), mm(10.0));

    // edit_param() re-resolution: only Bracket should be re-solved
    let mut bracket_resolved_again = HashMap::new();
    bracket_resolved_again.insert(bracket_thickness.clone(), mm(6.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        // eval() call 1: Bracket
        SolveResult::Solved {
            values: bracket_solved,
            unique: true,
        },
        // eval() call 2: Bolt
        SolveResult::Solved {
            values: bolt_solved,
            unique: true,
        },
        // edit_param() re-resolution: Bracket only
        SolveResult::Solved {
            values: bracket_resolved_again,
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    // Bracket: auto thickness, param limit (default 2mm), constraint thickness > limit, Minimize
    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param("Bracket", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "limit"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    // Bolt: auto diameter, constraint diameter > 5mm, Maximize
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(value_ref("Bolt", "diameter"), literal(mm(5.0))),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Cold eval: two solver calls (one per template)
    engine.eval(&module);

    // Edit Bracket.limit to 3mm — triggers re-resolution of Bracket's constraints
    let edit_result = engine.edit_param(bracket_limit.clone(), mm(3.0)).unwrap();

    // (1) There should now be 3 total solver calls: 2 from eval + 1 from edit_param
    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        3,
        "expected 3 solver calls (2 from eval + 1 from edit_param), got {}",
        problems.len()
    );

    // (2) The edit_param call (index 2) should contain only Bracket's auto params
    let edit_call = &problems[2];
    let edit_entities: Vec<&str> = edit_call
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();
    assert_eq!(
        edit_entities,
        vec!["Bracket"],
        "edit_param re-resolution should only contain Bracket's auto params, got {:?}",
        edit_entities
    );

    // (3) The edit_param call should have Bracket's Minimize objective
    assert!(
        matches!(
            &edit_call.objective,
            Some(OptimizationObjective::Minimize(_))
        ),
        "edit_param should forward Bracket's Minimize objective, got {:?}",
        edit_call.objective
    );

    // (4) Bracket.thickness should be updated to the new resolved value (6mm)
    let t_val = edit_result
        .values
        .get(&bracket_thickness)
        .expect("thickness");
    assert!(
        matches!(t_val, Value::Scalar { si_value, .. } if (*si_value - 0.006).abs() < 1e-10),
        "expected thickness = mm(6.0) after re-resolution, got {:?}",
        t_val
    );
}

#[test]
fn concurrent_edit_resolves_per_template_not_cross_template() {
    // Same two-template module. After eval(), prepare_concurrent_edit on
    // Bracket.limit, then resolve_concurrent_edit. The solver call should
    // contain only Bracket's auto params.
    use reify_types::SolveResult;
    use std::collections::HashSet;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_limit = ValueCellId::new("Bracket", "limit");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    let mut bracket_solved = HashMap::new();
    bracket_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_solved = HashMap::new();
    bolt_solved.insert(bolt_diameter.clone(), mm(10.0));
    let mut bracket_resolved_again = HashMap::new();
    bracket_resolved_again.insert(bracket_thickness.clone(), mm(6.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved {
            values: bracket_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: bolt_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: bracket_resolved_again,
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param("Bracket", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "limit"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(value_ref("Bolt", "diameter"), literal(mm(5.0))),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    engine.eval(&module);

    // prepare_concurrent_edit + resolve_concurrent_edit
    let setup = engine
        .prepare_concurrent_edit(bracket_limit.clone(), mm(3.0))
        .unwrap();

    let mut result = ConcurrentEditResult {
        values: setup.values.clone(),
        snapshot_values: setup.snapshot_values.clone(),
        node_results: Vec::new(),
        actual_eval_set: Vec::new(),
        skipped: HashSet::new(),
        resolved_params: HashMap::new(),
        diagnostics: Vec::new(),
    };

    engine.resolve_concurrent_edit(&setup, &mut result);

    // (1) 3 total calls: 2 from eval + 1 from resolve_concurrent_edit
    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        3,
        "expected 3 solver calls, got {}",
        problems.len()
    );

    // (2) The resolve_concurrent_edit call (index 2) should only have Bracket's params
    let edit_call = &problems[2];
    let edit_entities: Vec<&str> = edit_call
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();
    assert_eq!(
        edit_entities,
        vec!["Bracket"],
        "resolve_concurrent_edit should only contain Bracket's auto params, got {:?}",
        edit_entities
    );

    // (3) Correct objective
    assert!(
        matches!(
            &edit_call.objective,
            Some(OptimizationObjective::Minimize(_))
        ),
        "resolve_concurrent_edit should forward Bracket's Minimize objective, got {:?}",
        edit_call.objective
    );
}

#[test]
fn edit_param_matches_eval_for_multi_template_module() {
    // Prove cold/hot path equivalence: the resolved params from edit_param
    // should match what eval() produces for a multi-template module.
    use reify_test_support::SequencedMockConstraintSolver;
    use reify_types::SolveResult;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_limit = ValueCellId::new("Bracket", "limit");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    // Deterministic per-template results
    let mut bracket_solved = HashMap::new();
    bracket_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_solved = HashMap::new();
    bolt_solved.insert(bolt_diameter.clone(), mm(10.0));

    // ── Run 1: eval() to get baseline resolved params ──
    let solver1 = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: bracket_solved.clone(),
            unique: true,
        },
        SolveResult::Solved {
            values: bolt_solved.clone(),
            unique: true,
        },
    ]);

    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param("Bracket", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "limit"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(value_ref("Bolt", "diameter"), literal(mm(5.0))),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket.clone())
        .template(bolt.clone())
        .build();

    let mut engine1 =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver1));
    let eval_result = engine1.eval(&module);

    // Record baseline resolved params from eval
    let eval_bracket_thickness = eval_result
        .values
        .get(&bracket_thickness)
        .cloned()
        .expect("Bracket.thickness should be resolved");
    let eval_bolt_diameter = eval_result
        .values
        .get(&bolt_diameter)
        .cloned()
        .expect("Bolt.diameter should be resolved");

    // ── Run 2: eval() then edit_param to trigger Bracket re-resolution ──
    // Same solver results for eval, then for re-resolution Bracket gets same result
    let solver2 = SequencedMockConstraintSolver::new(vec![
        // eval() call 1: Bracket
        SolveResult::Solved {
            values: bracket_solved.clone(),
            unique: true,
        },
        // eval() call 2: Bolt
        SolveResult::Solved {
            values: bolt_solved.clone(),
            unique: true,
        },
        // edit_param() re-resolution: Bracket returns same result
        SolveResult::Solved {
            values: bracket_solved.clone(),
            unique: true,
        },
    ]);

    let module2 = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine2 =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver2));
    engine2.eval(&module2);

    // Edit Bracket.limit to trigger Bracket's constraint re-resolution
    let edit_result = engine2.edit_param(bracket_limit.clone(), mm(3.0)).unwrap();

    // Cold/hot path equivalence: edit_param should produce the same resolved values
    let edit_bracket_thickness = edit_result
        .values
        .get(&bracket_thickness)
        .cloned()
        .expect("Bracket.thickness should be in edit_param result");

    assert_eq!(
        eval_bracket_thickness, edit_bracket_thickness,
        "Bracket.thickness should match between eval() and edit_param(): eval={:?}, edit={:?}",
        eval_bracket_thickness, edit_bracket_thickness
    );

    // Bolt should NOT have been re-solved (only Bracket was dirty), so its value
    // should still be the eval() value
    let edit_bolt_diameter = edit_result
        .values
        .get(&bolt_diameter)
        .cloned()
        .expect("Bolt.diameter should be in edit_param result");

    assert_eq!(
        eval_bolt_diameter, edit_bolt_diameter,
        "Bolt.diameter should be unchanged after editing Bracket.limit: eval={:?}, edit={:?}",
        eval_bolt_diameter, edit_bolt_diameter
    );
}

#[test]
fn scope_name_deterministic_for_multi_template() {
    // Regression test: scope_name (and thus objective lookup) must be determined
    // by entity grouping, not HashMap iteration order. Edit two different params
    // (one per template) and verify each edit_param call gets the correct objective.
    use reify_types::SolveResult;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_limit = ValueCellId::new("Bracket", "limit");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");
    let bolt_clearance = ValueCellId::new("Bolt", "clearance");

    let mut bracket_solved = HashMap::new();
    bracket_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_solved = HashMap::new();
    bolt_solved.insert(bolt_diameter.clone(), mm(10.0));

    // eval() results + 2 edit_param re-resolutions
    let spy = MultiCallSpyConstraintSolver::new(vec![
        // eval() call 1: Bracket
        SolveResult::Solved {
            values: bracket_solved.clone(),
            unique: true,
        },
        // eval() call 2: Bolt
        SolveResult::Solved {
            values: bolt_solved.clone(),
            unique: true,
        },
        // edit_param() re-resolution after editing Bracket.limit
        SolveResult::Solved {
            values: bracket_solved.clone(),
            unique: true,
        },
        // edit_param() re-resolution after editing Bolt.clearance
        SolveResult::Solved {
            values: bolt_solved.clone(),
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    // Bracket: auto thickness, param limit, constraint thickness > limit, Minimize
    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param("Bracket", "limit", Type::length(), Some(literal(mm(2.0))))
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "limit"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    // Bolt: auto diameter, param clearance, constraint diameter > clearance, Maximize
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .param("Bolt", "clearance", Type::length(), Some(literal(mm(5.0))))
        .constraint(
            "Bolt",
            0,
            None,
            gt(
                value_ref("Bolt", "diameter"),
                value_ref("Bolt", "clearance"),
            ),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    engine.eval(&module);

    // Edit 1: Bracket.limit → triggers Bracket re-resolution
    engine.edit_param(bracket_limit.clone(), mm(3.0)).unwrap();

    // Edit 2: Bolt.clearance → triggers Bolt re-resolution
    engine.edit_param(bolt_clearance.clone(), mm(6.0)).unwrap();

    let problems = captured.lock().unwrap();

    // Should have 4 total calls: 2 from eval + 1 from each edit_param
    assert_eq!(
        problems.len(),
        4,
        "expected 4 solver calls (2 eval + 2 edit), got {}",
        problems.len()
    );

    // Call index 2: Bracket edit → must have Minimize objective
    let bracket_edit = &problems[2];
    let bracket_entities: Vec<&str> = bracket_edit
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();
    assert_eq!(
        bracket_entities,
        vec!["Bracket"],
        "first edit_param should resolve Bracket, got {:?}",
        bracket_entities
    );
    assert!(
        matches!(
            &bracket_edit.objective,
            Some(OptimizationObjective::Minimize(_))
        ),
        "Bracket edit should have Minimize objective, got {:?}",
        bracket_edit.objective
    );

    // Call index 3: Bolt edit → must have Maximize objective
    let bolt_edit = &problems[3];
    let bolt_entities: Vec<&str> = bolt_edit
        .auto_params
        .iter()
        .map(|p| p.id.entity.as_str())
        .collect();
    assert_eq!(
        bolt_entities,
        vec!["Bolt"],
        "second edit_param should resolve Bolt, got {:?}",
        bolt_entities
    );
    assert!(
        matches!(
            &bolt_edit.objective,
            Some(OptimizationObjective::Maximize(_))
        ),
        "Bolt edit should have Maximize objective, got {:?}",
        bolt_edit.objective
    );
}

#[test]
fn edit_param_no_cross_group_value_contamination() {
    // When editing a param that dirties constraints in BOTH entity groups,
    // each group's solver call must receive the same pre-loop snapshot of
    // current_values — not a map contaminated by the other group's resolved values.
    use reify_types::SolveResult;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_clearance = ValueCellId::new("Bracket", "clearance");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    // eval() results
    let mut bracket_eval_solved = HashMap::new();
    bracket_eval_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_eval_solved = HashMap::new();
    bolt_eval_solved.insert(bolt_diameter.clone(), mm(10.0));

    // edit_param() re-resolution results (both groups dirty).
    // Use values DIFFERENT from eval so contamination is detectable.
    let mut edit_resolved_a = HashMap::new();
    edit_resolved_a.insert(bracket_thickness.clone(), mm(7.0));
    let mut edit_resolved_b = HashMap::new();
    edit_resolved_b.insert(bolt_diameter.clone(), mm(14.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        // eval() call 1 (either group)
        SolveResult::Solved {
            values: bracket_eval_solved,
            unique: true,
        },
        // eval() call 2 (either group)
        SolveResult::Solved {
            values: bolt_eval_solved,
            unique: true,
        },
        // edit_param() call 3 (first dirty group)
        SolveResult::Solved {
            values: edit_resolved_a,
            unique: true,
        },
        // edit_param() call 4 (second dirty group)
        SolveResult::Solved {
            values: edit_resolved_b,
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    // Bracket: auto thickness, param clearance (default 2mm), constraint thickness > clearance
    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param(
            "Bracket",
            "clearance",
            Type::length(),
            Some(literal(mm(2.0))),
        )
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "clearance"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    // Bolt: auto diameter, constraint diameter > Bracket.clearance (cross-entity ref).
    // This makes Bolt's constraint depend on Bracket.clearance, so editing clearance
    // dirties BOTH groups' constraints.
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(
                value_ref("Bolt", "diameter"),
                value_ref("Bracket", "clearance"),
            ),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Cold eval: two solver calls (one per template)
    engine.eval(&module);

    // Edit Bracket.clearance to 3mm — dirties BOTH Bracket's and Bolt's constraints
    // because both reference Bracket.clearance.
    engine
        .edit_param(bracket_clearance.clone(), mm(3.0))
        .unwrap();

    let problems = captured.lock().unwrap();

    // (1) 4 total calls: 2 from eval + 2 from edit_param (both groups dirty)
    assert_eq!(
        problems.len(),
        4,
        "expected 4 solver calls (2 from eval + 2 from edit_param), got {}",
        problems.len()
    );

    // (2) The two edit_param calls (indices 2 and 3) must have identical
    //     current_values — proving no cross-group contamination.
    //     HashMap iteration order is non-deterministic, so we don't know which
    //     group solves first. But the invariant is: both must see the same
    //     pre-loop snapshot, not a map mutated by the other group's resolve.
    let cv_a: HashMap<ValueCellId, Value> = problems[2]
        .current_values
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let cv_b: HashMap<ValueCellId, Value> = problems[3]
        .current_values
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(
        cv_a,
        cv_b,
        "cross-group contamination: edit_param solver calls received different \
         current_values maps. Call 2 auto_params={:?}, Call 3 auto_params={:?}",
        problems[2]
            .auto_params
            .iter()
            .map(|p| &p.id)
            .collect::<Vec<_>>(),
        problems[3]
            .auto_params
            .iter()
            .map(|p| &p.id)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn resolve_concurrent_edit_no_cross_group_contamination() {
    // Same invariant as edit_param_no_cross_group_value_contamination but
    // exercised through the resolve_concurrent_edit path, which uses
    // result.values instead of a local values map.
    use reify_types::SolveResult;
    use std::collections::HashSet;

    let bracket_thickness = ValueCellId::new("Bracket", "thickness");
    let bracket_clearance = ValueCellId::new("Bracket", "clearance");
    let bolt_diameter = ValueCellId::new("Bolt", "diameter");

    // eval() results
    let mut bracket_eval_solved = HashMap::new();
    bracket_eval_solved.insert(bracket_thickness.clone(), mm(5.0));
    let mut bolt_eval_solved = HashMap::new();
    bolt_eval_solved.insert(bolt_diameter.clone(), mm(10.0));

    // resolve_concurrent_edit() re-resolution results (both groups dirty).
    // Use values DIFFERENT from eval so contamination is detectable.
    let mut edit_resolved_a = HashMap::new();
    edit_resolved_a.insert(bracket_thickness.clone(), mm(7.0));
    let mut edit_resolved_b = HashMap::new();
    edit_resolved_b.insert(bolt_diameter.clone(), mm(14.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        // eval() call 1 (either group)
        SolveResult::Solved {
            values: bracket_eval_solved,
            unique: true,
        },
        // eval() call 2 (either group)
        SolveResult::Solved {
            values: bolt_eval_solved,
            unique: true,
        },
        // resolve_concurrent_edit() call 3 (first dirty group)
        SolveResult::Solved {
            values: edit_resolved_a,
            unique: true,
        },
        // resolve_concurrent_edit() call 4 (second dirty group)
        SolveResult::Solved {
            values: edit_resolved_b,
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    // Bracket: auto thickness, param clearance (default 2mm), constraint thickness > clearance
    let bracket = TopologyTemplateBuilder::new("Bracket")
        .auto_param("Bracket", "thickness", Type::length())
        .param(
            "Bracket",
            "clearance",
            Type::length(),
            Some(literal(mm(2.0))),
        )
        .constraint(
            "Bracket",
            0,
            None,
            gt(
                value_ref("Bracket", "thickness"),
                value_ref("Bracket", "clearance"),
            ),
        )
        .objective(OptimizationObjective::Minimize(value_ref(
            "Bracket",
            "thickness",
        )))
        .build();

    // Bolt: auto diameter, constraint diameter > Bracket.clearance (cross-entity ref)
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .auto_param("Bolt", "diameter", Type::length())
        .constraint(
            "Bolt",
            0,
            None,
            gt(
                value_ref("Bolt", "diameter"),
                value_ref("Bracket", "clearance"),
            ),
        )
        .objective(OptimizationObjective::Maximize(value_ref(
            "Bolt", "diameter",
        )))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bracket)
        .template(bolt)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    engine.eval(&module);

    // prepare_concurrent_edit + resolve_concurrent_edit
    let setup = engine
        .prepare_concurrent_edit(bracket_clearance.clone(), mm(3.0))
        .unwrap();

    let mut result = ConcurrentEditResult {
        values: setup.values.clone(),
        snapshot_values: setup.snapshot_values.clone(),
        node_results: Vec::new(),
        actual_eval_set: Vec::new(),
        skipped: HashSet::new(),
        resolved_params: HashMap::new(),
        diagnostics: Vec::new(),
    };

    engine.resolve_concurrent_edit(&setup, &mut result);

    let problems = captured.lock().unwrap();

    // (1) 4 total calls: 2 from eval + 2 from resolve_concurrent_edit (both groups dirty)
    assert_eq!(
        problems.len(),
        4,
        "expected 4 solver calls (2 from eval + 2 from resolve_concurrent_edit), got {}",
        problems.len()
    );

    // (2) The two resolve_concurrent_edit calls (indices 2 and 3) must have
    //     identical current_values — proving no cross-group contamination.
    let cv_a: HashMap<ValueCellId, Value> = problems[2]
        .current_values
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let cv_b: HashMap<ValueCellId, Value> = problems[3]
        .current_values
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(
        cv_a,
        cv_b,
        "cross-group contamination: resolve_concurrent_edit solver calls received different \
         current_values maps. Call 2 auto_params={:?}, Call 3 auto_params={:?}",
        problems[2]
            .auto_params
            .iter()
            .map(|p| &p.id)
            .collect::<Vec<_>>(),
        problems[3]
            .auto_params
            .iter()
            .map(|p| &p.id)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn auto_free_threads_to_solver_and_warns() {
    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    // Spy returns Solved { unique: false } to simulate a free-auto resolution
    let spy = SpyConstraintSolver::new_solved_non_unique(solved_values);
    let captured = spy.captured_problem();

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    let result = engine.eval(&module);

    // (a) The AutoParam sent to the solver should have free=true
    let problem = captured.lock().unwrap();
    let problem = problem.as_ref().expect("solver should have been called");
    assert_eq!(problem.auto_params.len(), 1);
    assert!(
        problem.auto_params[0].free,
        "expected AutoParam.free=true for auto(free) cell"
    );

    // (b) A warning diagnostic should be emitted for auto(free) with non-unique result
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resolved via auto(free)")),
        "expected warning diagnostic about auto(free) resolution, got {:?}",
        result.diagnostics
    );
}

#[test]
fn auto_free_emits_warning_diagnostic() {
    let thickness_id = ValueCellId::new("S", "thickness");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    // Mock returns Solved { unique: false } — simulating free-auto resolution
    let solver = SpyConstraintSolver::new_solved_non_unique(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // The resolved value should still be populated
    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in values");
    assert!(
        matches!(thickness_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected mm(5.0) = 0.005 SI, got {:?}",
        thickness_val
    );

    // A Warning-level diagnostic should be emitted with the param name
    let warning = result
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("resolved via auto(free)")
                && d.message.contains("thickness")
        });
    assert!(
        warning.is_some(),
        "expected warning diagnostic about 'thickness' resolved via auto(free), got {:?}",
        result.diagnostics
    );
    let warning = warning.unwrap();
    assert_eq!(
        warning.severity,
        reify_types::Severity::Warning,
        "expected Warning severity, got {:?}",
        warning.severity
    );
}

#[test]
fn strict_auto_non_unique_emits_error_diagnostic() {
    // Integration test: real DimensionalSolver with an underdetermined strict auto
    // problem. Two params with only inequality constraints (x > 10mm, y > 10mm) —
    // many valid solutions exist. With strict auto (free: false), the solver detects
    // non-uniqueness and returns Infeasible, which the eval engine surfaces as
    // an Error diagnostic.

    let template = TopologyTemplateBuilder::new("Part")
        .auto_param("Part", "width", Type::length())
        .auto_param("Part", "height", Type::length())
        .constraint(
            "Part",
            0,
            None,
            gt(value_ref("Part", "width"), literal(mm(10.0))),
        )
        .constraint(
            "Part",
            1,
            None,
            gt(value_ref("Part", "height"), literal(mm(10.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // The solver should return Infeasible due to non-uniqueness, which the eval
    // engine forwards as diagnostics. Both params should remain Undef.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("not uniquely determined")),
        "expected error diagnostic about non-unique strict auto, got {:?}",
        result.diagnostics
    );

    // The error diagnostic should have Error severity
    let error_diag = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("not uniquely determined"))
        .unwrap();
    assert_eq!(
        error_diag.severity,
        reify_types::Severity::Error,
        "expected Error severity, got {:?}",
        error_diag.severity
    );
}

// ── auto(free) test suite (task 161) ─────────────────────────────────────────

#[test]
fn auto_free_underdetermined_returns_resolved_values() {
    // Two auto(free) params with loose inequality constraints (> 10mm each).
    // Mock solver returns Solved{unique:false} with specific values.
    // Both params should appear in result.values with correct SI values
    // and in result.resolved_params.
    use reify_test_support::SequencedMockConstraintSolver;
    use reify_types::SolveResult;

    let width_id = ValueCellId::new("S", "width");
    let height_id = ValueCellId::new("S", "height");

    let mut solved_values = HashMap::new();
    solved_values.insert(width_id.clone(), mm(12.0));
    solved_values.insert(height_id.clone(), mm(15.0));

    let solver = SequencedMockConstraintSolver::new(vec![SolveResult::Solved {
        values: solved_values,
        unique: false,
    }]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "width", Type::length())
        .auto_param_free("S", "height", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(10.0))))
        .constraint("S", 1, None, gt(value_ref("S", "height"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // Both params should be resolved to the mock values
    let width_val = result.values.get(&width_id).expect("width in values");
    assert!(
        matches!(width_val, Value::Scalar { si_value, .. } if (*si_value - 0.012).abs() < 1e-10),
        "expected width = mm(12.0) = 0.012 SI, got {:?}",
        width_val
    );

    let height_val = result.values.get(&height_id).expect("height in values");
    assert!(
        matches!(height_val, Value::Scalar { si_value, .. } if (*si_value - 0.015).abs() < 1e-10),
        "expected height = mm(15.0) = 0.015 SI, got {:?}",
        height_val
    );

    // Both params should appear in resolved_params
    assert_eq!(
        result.resolved_params.len(),
        2,
        "expected 2 resolved params, got {}",
        result.resolved_params.len()
    );
    assert!(result.resolved_params.contains_key(&width_id));
    assert!(result.resolved_params.contains_key(&height_id));
}

#[test]
fn auto_free_underdetermined_real_solver() {
    // Two auto(free) params with only lower-bound constraints (> 10mm each).
    // Real DimensionalSolver. Both params should be resolved to values > 10mm,
    // no error diagnostics, and auto(free) warning diagnostics present.

    let width_id = ValueCellId::new("S", "width");
    let height_id = ValueCellId::new("S", "height");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "width", Type::length())
        .auto_param_free("S", "height", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(10.0))))
        .constraint("S", 1, None, gt(value_ref("S", "height"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // No error diagnostics
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != reify_types::Severity::Error),
        "expected no error diagnostics, got {:?}",
        result.diagnostics
    );

    // Both params resolved to > 10mm (0.01 SI)
    let width_si = match result.values.get(&width_id).expect("width in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for width, got {:?}", other),
    };
    let height_si = match result.values.get(&height_id).expect("height in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for height, got {:?}", other),
    };
    assert!(
        width_si >= 0.01,
        "expected width >= 10mm (0.01 SI), got {:.6}m",
        width_si
    );
    assert!(
        height_si >= 0.01,
        "expected height >= 10mm (0.01 SI), got {:.6}m",
        height_si
    );

    // auto(free) warning diagnostics present
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resolved via auto(free)")),
        "expected auto(free) warning diagnostics, got {:?}",
        result.diagnostics
    );
}

#[test]
fn strict_auto_infeasible_params_remain_undef() {
    // Strict auto (free:false) with underdetermined constraints.
    // MockConstraintSolver returns Infeasible with an error diagnostic.
    // Assert params remain Value::Undef and result.resolved_params is empty.

    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
        "underdetermined: constraints do not uniquely pin values",
    )]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .auto_param("S", "y", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(5.0))))
        .constraint("S", 1, None, gt(value_ref("S", "y"), literal(mm(5.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // Both params should remain Undef
    let x_val = result.values.get(&x_id).expect("x in values");
    assert!(
        x_val.is_undef(),
        "expected x = Undef after infeasible solver, got {:?}",
        x_val
    );
    let y_val = result.values.get(&y_id).expect("y in values");
    assert!(
        y_val.is_undef(),
        "expected y = Undef after infeasible solver, got {:?}",
        y_val
    );

    // resolved_params should be empty
    assert!(
        result.resolved_params.is_empty(),
        "expected empty resolved_params after Infeasible, got {:?}",
        result.resolved_params
    );

    // Error diagnostic should be present
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("underdetermined")),
        "expected error diagnostic about underdetermined, got {:?}",
        result.diagnostics
    );
}

#[test]
fn auto_free_warning_per_param_name() {
    // Three auto(free) params (x, y, z). Mock solver returns Solved{unique:false}.
    // Assert exactly 3 warning diagnostics about auto(free), each containing
    // the respective param name. Verifies per-param diagnostic granularity.

    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    let z_id = ValueCellId::new("S", "z");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(11.0));
    solved_values.insert(y_id.clone(), mm(12.0));
    solved_values.insert(z_id.clone(), mm(13.0));

    let solver = SpyConstraintSolver::new_solved_non_unique(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .auto_param_free("S", "y", Type::length())
        .auto_param_free("S", "z", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .constraint("S", 1, None, gt(value_ref("S", "y"), literal(mm(10.0))))
        .constraint("S", 2, None, gt(value_ref("S", "z"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // Exactly 3 warning diagnostics about auto(free)
    let free_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("resolved via auto(free)"))
        .collect();
    assert_eq!(
        free_warnings.len(),
        3,
        "expected exactly 3 auto(free) warnings (one per param), got {:?}",
        result.diagnostics
    );

    // Each param name must appear in at least one warning
    for name in &["x", "y", "z"] {
        assert!(
            free_warnings.iter().any(|d| d.message.contains(name)),
            "expected warning mentioning param '{}', got {:?}",
            name,
            free_warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

#[test]
fn auto_free_no_warning_when_solver_returns_unique() {
    // Single auto(free) param. Mock solver returns Solved{unique:true}.
    // Assert no warning diagnostics about 'auto(free)' are emitted.
    // The unique flag suppresses warnings even for free params.

    let x_id = ValueCellId::new("S", "x");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(20.0));

    // SpyConstraintSolver::new_solved returns unique:true
    let solver = SpyConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // No auto(free) warnings when unique=true
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("auto(free)")),
        "expected no auto(free) warnings when solver returns unique=true, got {:?}",
        result.diagnostics
    );
}

#[test]
fn mixed_free_and_strict_only_free_warned() {
    // Template with one auto(free) param (x) and one strict auto param (y).
    // Mock solver returns Solved{unique:false}.
    // Assert warning diagnostic mentions only 'x' (the free param), not 'y' (strict).
    use reify_test_support::SequencedMockConstraintSolver;
    use reify_types::SolveResult;

    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(15.0));
    solved_values.insert(y_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![SolveResult::Solved {
        values: solved_values,
        unique: false,
    }]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .auto_param("S", "y", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .constraint("S", 1, None, gt(value_ref("S", "y"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // Filter warnings about auto(free)
    let free_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("resolved via auto(free)"))
        .collect();

    // Exactly 1 warning (for x only)
    assert_eq!(
        free_warnings.len(),
        1,
        "expected exactly 1 auto(free) warning (for x only), got {:?}",
        result.diagnostics
    );

    // Warning mentions 'x'
    assert!(
        free_warnings[0].message.contains("x"),
        "expected warning to mention 'x', got '{}'",
        free_warnings[0].message
    );

    // Warning must NOT mention 'y' (strict auto params are not warned)
    assert!(
        !free_warnings[0].message.contains("`y`"),
        "warning should not mention strict auto param 'y', got '{}'",
        free_warnings[0].message
    );
}

#[test]
fn auto_free_with_tight_range_narrows_solution() {
    // auto(free) param with tight range constraints (> 49mm AND < 51mm).
    // Real DimensionalSolver. Assert resolved value is in [49mm, 51mm] SI.
    // Demonstrates explicit constraints narrowing the solution space.

    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(49.0))))
        .constraint("S", 1, None, lt(value_ref("S", "x"), literal(mm(51.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // No error diagnostics
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != reify_types::Severity::Error),
        "expected no error diagnostics, got {:?}",
        result.diagnostics
    );

    let x_si = match result.values.get(&x_id).expect("x in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for x, got {:?}", other),
    };

    // x must be in [49mm, 51mm] = [0.049, 0.051] SI
    assert!(
        x_si >= 0.049 && x_si <= 0.051,
        "expected x in [49mm, 51mm] SI, got {:.6}m ({:.3}mm)",
        x_si,
        x_si * 1000.0
    );
}

#[test]
fn auto_free_equality_constraints_pin_value() {
    // auto(free) param with constraints (> 24.9mm AND < 25.1mm), effectively
    // pinning to ~25mm. Real DimensionalSolver.
    // Assert resolved value within 0.2mm of 25mm.

    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(24.9))))
        .constraint("S", 1, None, lt(value_ref("S", "x"), literal(mm(25.1))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // No error diagnostics
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != reify_types::Severity::Error),
        "expected no error diagnostics, got {:?}",
        result.diagnostics
    );

    let x_si = match result.values.get(&x_id).expect("x in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for x, got {:?}", other),
    };

    // x should be within 0.2mm (0.0002 SI) of 25mm (0.025 SI)
    let target_si = 0.025;
    assert!(
        (x_si - target_si).abs() < 0.0002,
        "expected x within 0.2mm of 25mm, got {:.6}m ({:.3}mm)",
        x_si,
        x_si * 1000.0
    );
}

#[test]
fn auto_free_minimize_selects_smallest_feasible() {
    // auto(free) param with constraint (> 10mm) and Minimize objective.
    // Real DimensionalSolver. Assert resolved value < 15mm.
    // The minimizer drives it toward the lower bound rather than returning
    // the default large initial guess (~5000mm).
    // Warning diagnostic for non-unique also present.

    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .objective(OptimizationObjective::Minimize(value_ref("S", "x")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // No error diagnostics
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != reify_types::Severity::Error),
        "expected no error diagnostics, got {:?}",
        result.diagnostics
    );

    let x_si = match result.values.get(&x_id).expect("x in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for x, got {:?}", other),
    };

    // The minimizer should drive x toward the lower bound (10mm), well below 15mm
    let fifteen_mm_si = 0.015;
    assert!(
        x_si < fifteen_mm_si,
        "expected x < 15mm when minimizing, got {:.6}m ({:.3}mm)",
        x_si,
        x_si * 1000.0
    );

    // Warning diagnostic should still be present (free auto, non-unique result)
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("resolved via auto(free)")),
        "expected auto(free) warning diagnostic, got {:?}",
        result.diagnostics
    );
}

#[test]
fn auto_free_minimize_with_upper_bound() {
    // auto(free) param with constraints (> 10mm AND < 100mm) and Minimize objective.
    // Real DimensionalSolver. Assert resolved value < 15mm (driven to lower bound)
    // and > 10mm (satisfies lower-bound constraint).

    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .constraint("S", 1, None, lt(value_ref("S", "x"), literal(mm(100.0))))
        .objective(OptimizationObjective::Minimize(value_ref("S", "x")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&module);

    // No error diagnostics
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != reify_types::Severity::Error),
        "expected no error diagnostics, got {:?}",
        result.diagnostics
    );

    let x_si = match result.values.get(&x_id).expect("x in values") {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for x, got {:?}", other),
    };

    // x must satisfy the lower-bound constraint (>= 10mm, boundary is acceptable for
    // penalty-based solvers)
    assert!(
        x_si >= 0.01,
        "expected x >= 10mm, got {:.6}m ({:.3}mm)",
        x_si,
        x_si * 1000.0
    );

    // Minimizer should drive x to near the lower bound (< 15mm)
    assert!(
        x_si < 0.015,
        "expected x < 15mm when minimizing with bounds, got {:.6}m ({:.3}mm)",
        x_si,
        x_si * 1000.0
    );
}

#[test]
fn auto_free_snapshot_determinacy_after_resolution() {
    // auto(free) param resolved via mock solver. After eval, inspect engine.snapshot().
    // Assert the resolved param has (resolved_value, DeterminacyState::Determined)
    // in the snapshot values. Provenance should be Resolution variant.

    let x_id = ValueCellId::new("S", "x");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(30.0));

    let solver = SpyConstraintSolver::new_solved_non_unique(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(10.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let _result = engine.eval(&module);
    let snap = engine.snapshot().expect("snapshot should exist");

    // x should be (mm(30.0), Determined) in the snapshot
    let (val, det) = snap.values.get(&x_id).expect("x in snapshot");
    assert!(
        matches!(val, Value::Scalar { si_value, .. } if (*si_value - 0.030).abs() < 1e-10),
        "expected mm(30.0) = 0.030 SI in snapshot, got {:?}",
        val
    );
    assert_eq!(
        *det,
        DeterminacyState::Determined,
        "expected DeterminacyState::Determined for auto(free) resolved param"
    );

    // Provenance should be a Resolution variant
    assert!(
        matches!(snap.provenance, SnapshotProvenance::Resolution { .. }),
        "expected SnapshotProvenance::Resolution, got {:?}",
        snap.provenance
    );
}

#[test]
fn auto_free_dependent_let_binding_updated() {
    // auto(free) param (thickness) with a let binding (volume = thickness * Real(1000.0))
    // that depends on it. Mock solver resolves thickness to 5mm.
    // Assert the let binding is re-evaluated with the resolved value, not Undef.

    let thickness_id = ValueCellId::new("S", "thickness");
    let volume_id = ValueCellId::new("S", "volume");

    let mut solved_values = HashMap::new();
    solved_values.insert(thickness_id.clone(), mm(5.0));

    // SpyConstraintSolver::new_solved_non_unique → Solved{unique:false}
    let solver = SpyConstraintSolver::new_solved_non_unique(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param_free("S", "thickness", Type::length())
        // volume = thickness * 1000.0 (dimensionless multiplier)
        .let_binding(
            "S",
            "volume",
            Type::length(),
            binop(
                reify_types::BinOp::Mul,
                value_ref("S", "thickness"),
                literal(Value::Real(1000.0)),
            ),
        )
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(2.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // volume = thickness * 1000.0 = 0.005 * 1000.0 = 5.0 SI
    let volume_val = result
        .values
        .get(&volume_id)
        .expect("volume should be in values");
    assert!(
        !volume_val.is_undef(),
        "expected volume to be re-evaluated (non-Undef), got {:?}",
        volume_val
    );
    assert!(
        matches!(volume_val, Value::Scalar { si_value, .. } if (*si_value - 5.0).abs() < 1e-6),
        "expected volume = thickness(5mm) * 1000 = 5.0 SI, got {:?}",
        volume_val
    );
}
