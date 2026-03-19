//! Boundary 5 (cli → eval) — Engine facade tests.
//!
//! These tests verify the Engine API works correctly with mock implementations.

use reify_test_support::*;
use reify_types::Satisfaction;

/// Full pipeline with mocks: compile → evaluate → expected ValueMap.
#[test]

fn full_pipeline_with_mocks() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.eval(&module);
    assert!(!result.values.is_empty());
}

/// Build with mock geometry kernel → produces output.
#[test]

fn build_with_mock_kernel() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, reify_types::ExportFormat::Step);
    assert!(result.geometry_output.is_some());
}

/// Auto param evaluates to (Undef, DeterminacyState::Auto) in snapshot.
#[test]
fn eval_auto_param_undef_auto() {
    use reify_types::{CompiledExpr, DeterminacyState, ModulePath, Type, ValueCellId};

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param("S", "y", Type::length(), Some(CompiledExpr::literal(mm(5.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // x should be Undef in the values map
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef, got {:?}", x_val);

    // y should be evaluated to 0.005 (5mm in SI)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    assert!(!y_val.is_undef(), "normal param y should not be Undef");

    // Check snapshot determinacy
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (x_snap_val, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert!(x_snap_val.is_undef(), "snapshot x should be Undef");
    assert_eq!(*x_det, DeterminacyState::Auto, "snapshot x determinacy should be Auto");

    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(*y_det, DeterminacyState::Determined, "snapshot y determinacy should be Determined");
}

/// eval_cached: auto param gets (Undef, Auto), and override applies Determined.
#[test]
fn eval_cached_auto_param() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId, VersionId};

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param("S", "y", Type::length(), Some(CompiledExpr::literal(mm(5.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval_cached: auto param should be (Undef, Auto)
    let result = engine.eval_cached(&module, VersionId(1));
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.eval_result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef on cold start");

    // Now set an override for the auto param and re-evaluate
    engine.set_param_and_invalidate(&x_id, mm(10.0));
    let result2 = engine.eval_cached(&module, VersionId(2));
    let x_val2 = result2.eval_result.values.get(&x_id).expect("x should be in values");
    assert!(!x_val2.is_undef(), "auto param x should have override value");
}

/// Constraint on auto param → Indeterminate (Undef propagates).
#[test]
fn constraint_on_auto_param_indeterminate() {
    use reify_types::{ModulePath, Type, ValueCellId};

    // Build module with auto param x and constraint x > 5mm
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(5.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    // x should be Undef
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef");

    // Constraint should be Indeterminate (Undef propagation)
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        reify_types::Satisfaction::Indeterminate,
        "constraint on auto param should be Indeterminate"
    );
}

/// End-to-end: parse → compile → eval → check with auto param.
#[test]
fn e2e_parse_compile_eval_auto_param() {
    use reify_compiler::ValueCellKind;
    use reify_types::{DeterminacyState, ModulePath, Satisfaction, ValueCellId};

    let source = r#"structure S {
    param x : Scalar = auto
    param y : Scalar = 5mm
    let z = y * 2
    constraint x > 2mm
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("e2e_auto"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    // Check that x is ValueCellKind::Auto
    let template = &compiled.templates[0];
    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id == ValueCellId::new("S", "x"))
        .expect("x cell not found");
    assert_eq!(x_cell.kind, ValueCellKind::Auto);

    // Evaluate and check
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // x should be Undef
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef, got {:?}", x_val);

    // y should be ~0.005 SI (5mm)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    let y_f64 = y_val.as_f64().expect("y should be a number");
    assert!((y_f64 - 0.005).abs() < 1e-10, "y should be 0.005 SI, got {}", y_f64);

    // z = y * 2 ≈ 0.01 SI
    let z_id = ValueCellId::new("S", "z");
    let z_val = result.values.get(&z_id).expect("z should be in values");
    let z_f64 = z_val.as_f64().expect("z should be a number");
    assert!((z_f64 - 0.01).abs() < 1e-10, "z should be 0.01 SI, got {}", z_f64);

    // Constraint on x should be Indeterminate
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Indeterminate,
        "constraint on auto param x should be Indeterminate"
    );

    // Check snapshot determinacy
    let snapshot = engine.snapshot().expect("snapshot should exist");
    let (_, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert_eq!(*x_det, DeterminacyState::Auto);

    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(*y_det, DeterminacyState::Determined);
}

/// Engine with predetermined constraint results → reports violations.
#[test]

fn engine_reports_violations() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new()
        .with_result(cnid("Bracket", 0), Satisfaction::Violated);
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    let violated: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|c| c.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(!violated.is_empty(), "should report violations");
}

/// Engine is not initialized before eval() is called.
#[test]
fn engine_is_not_initialized_before_eval() {
    let checker = MockConstraintChecker::new();
    let engine = reify_eval::Engine::new(Box::new(checker), None);
    assert!(!engine.is_initialized());
}

/// Engine is initialized after eval() is called.
#[test]
fn engine_is_initialized_after_eval() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&module);
    assert!(engine.is_initialized());
}

/// edit_param before eval() returns Err(EngineError::NotInitialized).
#[test]
fn edit_param_before_eval_returns_error() {
    use reify_types::ValueCellId;

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.edit_param(ValueCellId::new("S", "x"), mm(10.0));
    assert!(result.is_err(), "edit_param before eval should return Err");
    let err = result.unwrap_err();
    assert!(
        matches!(err, reify_eval::EngineError::NotInitialized),
        "error should be NotInitialized, got {:?}",
        err,
    );
}

/// After eval(), eval_state is available as a single atomic unit containing
/// snapshot, reverse_index, and trace_map.
#[test]
fn eval_state_available_atomically_after_eval() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId};

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "w", Type::length(), Some(CompiledExpr::literal(mm(10.0), Type::length())))
        .param("S", "h", Type::length(), Some(CompiledExpr::literal(mm(20.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&module);

    // eval_state should be available
    assert!(engine.is_initialized());
    let state = engine.eval_state().expect("eval_state should be Some after eval()");

    // snapshot should contain the values
    let w_id = ValueCellId::new("S", "w");
    let h_id = ValueCellId::new("S", "h");
    assert!(state.snapshot.values.get(&w_id).is_some(), "snapshot should contain w");
    assert!(state.snapshot.values.get(&h_id).is_some(), "snapshot should contain h");

    // reverse_index should be populated (has entries for value cells)
    // trace_map should be populated
    assert!(!state.trace_map.is_empty(), "trace_map should be populated after eval");

    // snapshot() accessor should also work and return the same snapshot
    let snap = engine.snapshot().expect("snapshot should be Some");
    assert_eq!(snap.id, state.snapshot.id);
}

/// Let bindings with forward references are evaluated correctly,
/// including after auto-resolution phase. Serves as regression test
/// for the let-binding evaluation helper extraction.
#[test]
fn let_binding_evaluation_produces_same_results_with_helper() {
    use reify_types::{BinOp, CompiledExpr, ModulePath, Type, ValueCellId};

    // Build module: param p = 3, let a = b + 1, let b = p * 2
    // Forward ref: a references b which is declared after a.
    // Expected: b = 3*2 = 6, a = 6+1 = 7
    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S", "p", Type::Real,
            Some(CompiledExpr::literal(reify_types::Value::Real(3.0), Type::Real)),
        )
        .let_binding(
            "S", "a", Type::Real,
            binop(
                BinOp::Add,
                value_ref("S", "b"),
                CompiledExpr::literal(reify_types::Value::Real(1.0), Type::Real),
            ),
        )
        .let_binding(
            "S", "b", Type::Real,
            binop(
                BinOp::Mul,
                value_ref("S", "p"),
                CompiledExpr::literal(reify_types::Value::Real(2.0), Type::Real),
            ),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Verify let binding values
    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let p_id = ValueCellId::new("S", "p");

    let p_val = result.values.get(&p_id).expect("p should be in values");
    assert_eq!(p_val.as_f64().unwrap(), 3.0, "p should be 3.0");

    let b_val = result.values.get(&b_id).expect("b should be in values");
    assert_eq!(b_val.as_f64().unwrap(), 6.0, "b should be p*2 = 6.0");

    let a_val = result.values.get(&a_id).expect("a should be in values");
    assert_eq!(a_val.as_f64().unwrap(), 7.0, "a should be b+1 = 7.0");

    // Verify values also correct after edit_param
    let result2 = engine.edit_param(p_id.clone(), reify_types::Value::Real(5.0)).unwrap();

    let p_val2 = result2.values.get(&p_id).expect("p should be in values");
    assert_eq!(p_val2.as_f64().unwrap(), 5.0, "p should be 5.0 after edit");

    // Note: edit_param only re-evaluates dirty nodes in the eval set.
    // Let bindings b and a should be re-evaluated since they depend on p.
    // b = 5*2 = 10, a = 10+1 = 11
    let b_val2 = result2.values.get(&b_id).expect("b should be in values");
    assert_eq!(b_val2.as_f64().unwrap(), 10.0, "b should be p*2 = 10.0 after edit");

    let a_val2 = result2.values.get(&a_id).expect("a should be in values");
    assert_eq!(a_val2.as_f64().unwrap(), 11.0, "a should be b+1 = 11.0 after edit");
}
