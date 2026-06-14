//! Boundary 5 (cli → eval) — Engine facade tests.
//!
//! These tests verify the Engine API works correctly with mock implementations.

use reify_ir::Satisfaction;
use reify_test_support::*;

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
    let result = engine.build(&module, reify_ir::ExportFormat::Step);
    assert!(result.geometry_output.is_some());
}

/// Auto param evaluates to (Undef, DeterminacyState::Auto) in snapshot.
#[test]
fn eval_auto_param_undef_auto() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::{CompiledExpr, DeterminacyState};

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param(
            "S",
            "y",
            Type::length(),
            Some(CompiledExpr::literal(mm(5.0), Type::length())),
        )
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
    assert!(
        x_val.is_undef(),
        "auto param x should be Undef, got {:?}",
        x_val
    );

    // y should be evaluated to 0.005 (5mm in SI)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    assert!(!y_val.is_undef(), "normal param y should not be Undef");

    // Check snapshot determinacy
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (x_snap_val, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert!(x_snap_val.is_undef(), "snapshot x should be Undef");
    assert_eq!(
        *x_det,
        DeterminacyState::Auto,
        "snapshot x determinacy should be Auto"
    );

    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(
        *y_det,
        DeterminacyState::Determined,
        "snapshot y determinacy should be Determined"
    );
}

/// eval_cached: auto param gets (Undef, Auto), and override applies Determined.
#[test]
fn eval_cached_auto_param() {
    use reify_core::{ModulePath, Type, ValueCellId, VersionId};
    use reify_ir::CompiledExpr;

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param(
            "S",
            "y",
            Type::length(),
            Some(CompiledExpr::literal(mm(5.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval_cached: auto param should be (Undef, Auto)
    let result = engine.eval_cached(&module, VersionId(1));
    let x_id = ValueCellId::new("S", "x");
    let x_val = result
        .eval_result
        .values
        .get(&x_id)
        .expect("x should be in values");
    assert!(
        x_val.is_undef(),
        "auto param x should be Undef on cold start"
    );

    // Now set an override for the auto param and re-evaluate
    engine.set_param_and_invalidate(&x_id, mm(10.0));
    let result2 = engine.eval_cached(&module, VersionId(2));
    let x_val2 = result2
        .eval_result
        .values
        .get(&x_id)
        .expect("x should be in values");
    assert!(
        !x_val2.is_undef(),
        "auto param x should have override value"
    );
}

/// Constraint on auto param → Indeterminate (Undef propagates).
#[test]
fn constraint_on_auto_param_indeterminate() {
    use reify_core::{ModulePath, Type, ValueCellId};

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
        reify_ir::Satisfaction::Indeterminate,
        "constraint on auto param should be Indeterminate"
    );
}

/// End-to-end: parse → compile → eval → check with auto param.
#[test]
fn e2e_parse_compile_eval_auto_param() {
    use reify_core::{ModulePath, ValueCellId};
    use reify_ir::{DeterminacyState, Satisfaction};

    let source = r#"structure S {
    param x : Length = auto
    param y : Length = 5mm
    let z = y * 2
    constraint x > 2mm
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("e2e_auto"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    // Check that x is ValueCellKind::Auto
    let template = &compiled.templates[0];
    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id == ValueCellId::new("S", "x"))
        .expect("x cell not found");
    assert!(x_cell.kind.is_auto());

    // Evaluate and check
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // x should be Undef
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(
        x_val.is_undef(),
        "auto param x should be Undef, got {:?}",
        x_val
    );

    // y should be ~0.005 SI (5mm)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    let y_f64 = y_val.as_f64().expect("y should be a number");
    assert!(
        (y_f64 - 0.005).abs() < 1e-10,
        "y should be 0.005 SI, got {}",
        y_f64
    );

    // z = y * 2 ≈ 0.01 SI
    let z_id = ValueCellId::new("S", "z");
    let z_val = result.values.get(&z_id).expect("z should be in values");
    let z_f64 = z_val.as_f64().expect("z should be a number");
    assert!(
        (z_f64 - 0.01).abs() < 1e-10,
        "z should be 0.01 SI, got {}",
        z_f64
    );

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
    let checker =
        MockConstraintChecker::new().with_result(cnid("Bracket", 0), Satisfaction::Violated);
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
    use reify_core::ValueCellId;

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
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::CompiledExpr;

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "w",
            Type::length(),
            Some(CompiledExpr::literal(mm(10.0), Type::length())),
        )
        .param(
            "S",
            "h",
            Type::length(),
            Some(CompiledExpr::literal(mm(20.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&module);

    // eval_state should be available
    assert!(engine.is_initialized());
    let state = engine
        .eval_state()
        .expect("eval_state should be Some after eval()");

    // snapshot should contain the values
    let w_id = ValueCellId::new("S", "w");
    let h_id = ValueCellId::new("S", "h");
    assert!(
        state.snapshot.values.get(&w_id).is_some(),
        "snapshot should contain w"
    );
    assert!(
        state.snapshot.values.get(&h_id).is_some(),
        "snapshot should contain h"
    );

    // reverse_index should be populated (has entries for value cells)
    // trace_map should be populated
    assert!(
        !state.trace_map.is_empty(),
        "trace_map should be populated after eval"
    );

    // snapshot() accessor should also work and return the same snapshot
    let snap = engine.snapshot().expect("snapshot should be Some");
    assert_eq!(snap.id, state.snapshot.id);
}

/// Let bindings with forward references are evaluated correctly,
/// including after auto-resolution phase. Serves as regression test
/// for the let-binding evaluation helper extraction.
#[test]
fn let_binding_evaluation_produces_same_results_with_helper() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr};

    // Build module: param p = 3, let a = b + 1, let b = p * 2
    // Forward ref: a references b which is declared after a.
    // Expected: b = 3*2 = 6, a = 6+1 = 7
    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "p",
            Type::dimensionless_scalar(),
            Some(CompiledExpr::literal(
                reify_ir::Value::Real(3.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(
            "S",
            "a",
            Type::dimensionless_scalar(),
            binop(
                BinOp::Add,
                value_ref("S", "b"),
                CompiledExpr::literal(reify_ir::Value::Real(1.0), Type::dimensionless_scalar()),
            ),
        )
        .let_binding(
            "S",
            "b",
            Type::dimensionless_scalar(),
            binop(
                BinOp::Mul,
                value_ref("S", "p"),
                CompiledExpr::literal(reify_ir::Value::Real(2.0), Type::dimensionless_scalar()),
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
    let result2 = engine
        .edit_param(p_id.clone(), reify_ir::Value::Real(5.0))
        .unwrap();

    let p_val2 = result2.values.get(&p_id).expect("p should be in values");
    assert_eq!(p_val2.as_f64().unwrap(), 5.0, "p should be 5.0 after edit");

    // Note: edit_param only re-evaluates dirty nodes in the eval set.
    // Let bindings b and a should be re-evaluated since they depend on p.
    // b = 5*2 = 10, a = 10+1 = 11
    let b_val2 = result2.values.get(&b_id).expect("b should be in values");
    assert_eq!(
        b_val2.as_f64().unwrap(),
        10.0,
        "b should be p*2 = 10.0 after edit"
    );

    let a_val2 = result2.values.get(&a_id).expect("a should be in values");
    assert_eq!(
        a_val2.as_f64().unwrap(),
        11.0,
        "a should be b+1 = 11.0 after edit"
    );
}

/// Sub-component param values appear in the eval result with scoped IDs.
#[test]
fn sub_component_params_appear_in_eval_result() {
    use reify_core::ValueCellId;

    let module = parent_child_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.height should be width * 0.5 = 80mm * 0.5 = 40mm = 0.04 SI
    let scoped_id = ValueCellId::new("Parent.rib", "height");
    let val = result
        .values
        .get(&scoped_id)
        .expect("Parent.rib.height should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.04).abs() < 1e-10,
        "Parent.rib.height should be ~0.04 SI (40mm), got {}",
        f
    );
}

/// Sub-component child let-bindings are evaluated and appear in the result.
#[test]
fn sub_component_child_lets_evaluated() {
    use reify_core::ValueCellId;

    let module = parent_child_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.half_h should be height / 2 = 40mm / 2 = 20mm = 0.02 SI
    let scoped_id = ValueCellId::new("Parent.rib", "half_h");
    let val = result
        .values
        .get(&scoped_id)
        .expect("Parent.rib.half_h should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.02).abs() < 1e-10,
        "Parent.rib.half_h should be ~0.02 SI (20mm), got {}",
        f
    );
}

/// Sub-component with no args falls back to child's default param value.
#[test]
fn sub_component_default_param_when_no_arg() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::CompiledExpr;

    // Child: param height = 10mm
    let child_template = TopologyTemplateBuilder::new("Child")
        .param(
            "Child",
            "height",
            Type::length(),
            Some(CompiledExpr::literal(mm(10.0), Type::length())),
        )
        .build();

    // Parent: param width = 80mm, sub rib = Child() — no args
    let parent_template = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "width",
            Type::length(),
            Some(CompiledExpr::literal(mm(80.0), Type::length())),
        )
        .sub_component("rib", "Child", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(child_template)
        .template(parent_template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.height should use Child's default: 10mm = 0.01 SI
    let scoped_id = ValueCellId::new("Parent.rib", "height");
    let val = result
        .values
        .get(&scoped_id)
        .expect("Parent.rib.height should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.01).abs() < 1e-10,
        "Parent.rib.height should be ~0.01 SI (10mm default), got {}",
        f
    );
}

/// Sub-component referencing a missing structure is skipped gracefully.
#[test]
fn sub_component_missing_structure_skipped_gracefully() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::CompiledExpr;

    // Parent template with sub referencing nonexistent "NonExistent"
    let parent_template = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "width",
            Type::length(),
            Some(CompiledExpr::literal(mm(80.0), Type::length())),
        )
        .sub_component("thing", "NonExistent", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent_template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Should not panic; Parent's own param should be evaluated correctly
    let width_id = ValueCellId::new("Parent", "width");
    let val = result
        .values
        .get(&width_id)
        .expect("Parent.width should be in values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.08).abs() < 1e-10,
        "Parent.width should be ~0.08 SI, got {}",
        f
    );

    // No scoped entries for "Parent.thing" should exist
    let has_scoped = result
        .values
        .iter()
        .any(|(id, _)| format!("{}", id).contains("Parent.thing"));
    assert!(
        !has_scoped,
        "no Parent.thing entries should exist when structure is missing"
    );
}

/// Regression test: edit_param's second propagation wave (lib.rs ~line 1864)
/// must not panic after solver resolution. The `eval_state.as_ref().unwrap()`
/// is exercised through the positive path (eval_state is always Some when the
/// early guard at edit_param entry passes).
///
/// Setup: param `a`, auto `x`, let `y = x * 2`, constraint `x > a`.
/// Sequenced solver: 1st call (eval) → x=mm(5), 2nd call (edit_param) → x=mm(20).
/// After edit_param(a, mm(8)): y must be re-evaluated via the second wave
/// to reflect the new x value (y = 0.02 * 2 = 0.04).
#[test]
fn edit_param_second_wave_no_panic_after_solver_resolution() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::{BinOp, SolveResult, Value};
    use std::collections::HashMap;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // First call (cold eval): solver returns x = mm(5.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    // Second call (edit_param): solver returns x = mm(20.0)
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(solver));

    // Cold eval: solver returns x=mm(5.0)=0.005 SI, y = 0.005 * 2 = 0.01
    let result = engine.eval(&module);
    let y_val = result.values.get(&y_id).expect("y should be in values");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "y should be ~0.01 after cold eval, got {:?}",
        y_val,
    );

    // edit_param(a, mm(8.0)) → solver re-resolves x to mm(20.0)=0.02 SI.
    // Second propagation wave re-evaluates y = 0.02 * 2 = 0.04.
    // This exercises the eval_state unwrap through the positive path.
    let result2 = engine
        .edit_param(a_id.clone(), mm(8.0))
        .expect("edit_param should succeed (eval_state populated by eval())");

    // x should have the new resolved value
    let x_val2 = result2.values.get(&x_id).expect("x should be in values");
    assert!(
        matches!(x_val2, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "x should be mm(20.0)=0.02 SI after re-resolution, got {:?}",
        x_val2,
    );

    // y should be re-evaluated via second wave: y = 0.02 * 2 = 0.04
    let y_val2 = result2.values.get(&y_id).expect("y should be in values");
    assert!(
        matches!(y_val2, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "y should be ~0.04 after second wave re-evaluation, got {:?}",
        y_val2,
    );
}

/// Engine-level verification: stdlib functions evaluate correctly in let-bindings.
#[test]
fn engine_eval_stdlib_function_in_let() {
    use reify_core::{ModulePath, ValueCellId};

    let source = r#"structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    let diag = sqrt(w * w + h * h)
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("stdlib_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // diag = sqrt(0.08^2 + 0.1^2) = sqrt(0.0064 + 0.01) = sqrt(0.0164) ≈ 0.128062
    let diag_id = ValueCellId::new("S", "diag");
    let val = result
        .values
        .get(&diag_id)
        .expect("S.diag should be in eval result");
    let f = val.as_f64().expect("should be numeric");
    let expected = (0.08_f64.powi(2) + 0.1_f64.powi(2)).sqrt();
    assert!(
        (f - expected).abs() < 1e-10,
        "S.diag should be ~{} (sqrt of w²+h²), got {}",
        expected,
        f
    );
}

/// Engine-level verification: imports are transparent to evaluation.
#[test]
fn engine_eval_with_import() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_ir::CompiledExpr;

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "x",
            Type::length(),
            Some(CompiledExpr::literal(mm(50.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .import("std/math")
        .template(template)
        .build();

    // Verify imports are stored
    assert_eq!(module.imports.len(), 1);
    assert_eq!(module.imports[0].path, "std/math");

    // Evaluate — imports should not affect evaluation
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    let x_id = ValueCellId::new("S", "x");
    let val = result.values.get(&x_id).expect("S.x should be in values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.05).abs() < 1e-10,
        "S.x should be ~0.05 SI (50mm), got {}",
        f
    );
}

/// Comprehensive E2E: all three features (import, stdlib, sub-component) through Engine.
#[test]
fn e2e_all_three_features_through_engine() {
    use reify_core::{ModulePath, ValueCellId};

    let source = r#"import std.math

structure Child {
    param size: Length = 10mm
    let half = size / 2
}

structure Parent {
    param w: Length = 80mm
    let diag = sqrt(w * w)
    sub part = Child(size: w / 2)
    constraint diag > 0mm
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("e2e_all_three"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // (a) Module has 1 import
    assert_eq!(compiled.imports.len(), 1);
    assert_eq!(compiled.imports[0].path, "std.math");

    // Evaluate
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // (b) Parent values
    let w_id = ValueCellId::new("Parent", "w");
    let w_val = result.values.get(&w_id).expect("Parent.w should exist");
    let w_f = w_val.as_f64().expect("numeric");
    assert!(
        (w_f - 0.08).abs() < 1e-10,
        "Parent.w should be ~0.08, got {}",
        w_f
    );

    let diag_id = ValueCellId::new("Parent", "diag");
    let diag_val = result
        .values
        .get(&diag_id)
        .expect("Parent.diag should exist");
    let diag_f = diag_val.as_f64().expect("numeric");
    // sqrt(0.08^2) = 0.08
    assert!(
        (diag_f - 0.08).abs() < 1e-10,
        "Parent.diag should be ~0.08, got {}",
        diag_f
    );

    // (c) Sub-component param: Parent.part.size = w / 2 = 0.04
    let size_id = ValueCellId::new("Parent.part", "size");
    let size_val = result
        .values
        .get(&size_id)
        .expect("Parent.part.size should exist");
    let size_f = size_val.as_f64().expect("numeric");
    assert!(
        (size_f - 0.04).abs() < 1e-10,
        "Parent.part.size should be ~0.04, got {}",
        size_f
    );

    // (d) Sub-component let: Parent.part.half = size / 2 = 0.02
    let half_id = ValueCellId::new("Parent.part", "half");
    let half_val = result
        .values
        .get(&half_id)
        .expect("Parent.part.half should exist");
    let half_f = half_val.as_f64().expect("numeric");
    assert!(
        (half_f - 0.02).abs() < 1e-10,
        "Parent.part.half should be ~0.02, got {}",
        half_f
    );

    // (e) Constraint check
    let (constraint_results, _check_diags) = engine
        .check_constraints_with_values(&result.values)
        .unwrap();
    assert!(
        !constraint_results.is_empty(),
        "should have at least one constraint result"
    );

    // (f) No error-level diagnostics (only import warning is allowed)
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "no error diagnostics expected, got {:?}",
        errors
    );
}

/// Root-level cyclic let-bindings should emit an error diagnostic.
///
/// This tests the `evaluate_let_bindings` code path (root-level, non-recursive
/// template with no sub-components), NOT the `elaborate_child_lets_only` path
/// tested by `cyclic_let_bindings_emit_diagnostic` in recursive_unfold.rs.
#[test]
fn root_level_cyclic_let_bindings_emit_diagnostic() {
    use reify_core::{ModulePath, Severity, Type, ValueCellId};
    use reify_ir::{BinOp, Value};
    use reify_test_support::builders::{binop, literal, value_ref_typed};

    // Template S with cyclic lets: let a = b + 1, let b = a + 1
    // No sub-components, not recursive — exercises the root-level path.
    let a_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Int),
        literal(Value::Int(1)),
    );
    let b_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "a", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Int, a_expr)
        .let_binding("S", "b", Type::Int, b_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Cyclic bindings should be absent or Undef (Kahn's algorithm omits them).
    let a_val = result.values.get(&ValueCellId::new("S", "a"));
    assert!(
        a_val.is_none() || a_val == Some(&Value::Undef),
        "S.a should be absent or Undef (circular dependency), got {:?}",
        a_val,
    );
    let b_val = result.values.get(&ValueCellId::new("S", "b"));
    assert!(
        b_val.is_none() || b_val == Some(&Value::Undef),
        "S.b should be absent or Undef (circular dependency), got {:?}",
        b_val,
    );

    // An error diagnostic should be emitted about the circular dependency.
    let has_cycle_error = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && (d.message.contains("circular") || d.message.contains("cycle"))
            && d.message.contains("a")
            && d.message.contains("b")
    });
    assert!(
        has_cycle_error,
        "Expected an error diagnostic about circular let-binding dependency naming 'a' and 'b', \
         got: {:?}",
        result.diagnostics
    );
}

/// Non-cyclic forward-referencing lets must NOT trigger the cycle-detection diagnostic.
///
/// Template S: let a = b + 1, let b = 3. Forward reference (a → b) but no cycle.
/// Expected: b = 3, a = 4, no error diagnostics.
#[test]
fn root_level_non_cyclic_lets_no_false_positive() {
    use reify_core::{ModulePath, Severity, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, Value};
    use reify_test_support::builders::{binop, literal, value_ref_typed};

    let a_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Int),
        literal(Value::Int(1)),
    );
    let b_expr = CompiledExpr::literal(Value::Int(3), Type::Int);

    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Int, a_expr)
        .let_binding("S", "b", Type::Int, b_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // No error diagnostics about circular dependencies.
    let cycle_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && (d.message.contains("circular") || d.message.contains("cycle"))
        })
        .collect();
    assert!(
        cycle_errors.is_empty(),
        "No circular-dependency errors expected for non-cyclic lets, got: {:?}",
        cycle_errors
    );

    // Values should be correctly computed.
    let b_val = result
        .values
        .get(&ValueCellId::new("S", "b"))
        .expect("S.b should exist");
    assert_eq!(*b_val, Value::Int(3), "b should be 3");

    let a_val = result
        .values
        .get(&ValueCellId::new("S", "a"))
        .expect("S.a should exist");
    assert_eq!(*a_val, Value::Int(4), "a should be b+1 = 4");
}

/// A Let-kind cell with `default_expr = None` must be silently skipped by
/// `evaluate_let_bindings` — no panic, no spurious result.
///
/// Characterization test pinning the None-default_expr branch so that any
/// future edit that silently drops the kind-check or filter_map None-handling
/// will cause a test failure (or panic).
///
/// `TopologyTemplateBuilder::let_binding` always requires an expr, so we
/// inject the rare Let/None shape by pushing directly onto `template.value_cells`
/// after building (the field is public).
#[test]
fn evaluate_let_bindings_skips_let_cell_without_default_expr() {
    use reify_compiler::{ValueCellDecl, ValueCellKind, Visibility};
    use reify_core::{ModulePath, SourceSpan, Type, ValueCellId};
    use reify_ir::Value;

    // Build a template with one normal let-binding (the "good" cell).
    let mut template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "good", Type::Int, literal(Value::Int(7)))
        .build();

    // Inject a Let cell with no default_expr — the rare Let/None shape.
    // This must be filtered out by evaluate_let_bindings without panicking.
    template.value_cells.push(ValueCellDecl {
        id: ValueCellId::new("S", "bad"),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: Type::Int,
        default_expr: None,
        solver_hints: vec![],
        span: SourceSpan::new(0, 0),
    });

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    // Must not panic.
    let result = engine.eval(&module);

    // The good let-binding should evaluate normally.
    let good_val = result.values.get(&ValueCellId::new("S", "good"));
    assert_eq!(
        good_val,
        Some(&Value::Int(7)),
        "S.good should evaluate to Int(7), got {:?}",
        good_val,
    );

    // The Let cell with no default_expr must be absent from result.values.
    // The engine only writes to `values` for cells it evaluates; a Let cell
    // with `default_expr = None` is filtered out by `evaluate_let_bindings`
    // before the evaluation loop, so it is never inserted — the value is
    // absent, not Undef.  Asserting exact absence (not an OR) locks this in
    // as a characterization test: if a future change silently writes stale
    // data for unevaluated Let cells, this assertion will catch it.
    let bad_val = result.values.get(&ValueCellId::new("S", "bad"));
    assert!(
        bad_val.is_none(),
        "S.bad (Let with no default_expr) must be absent from result.values, got {:?}",
        bad_val,
    );
}

/// Characterization test: the cache entry for a let binding carries the full
/// static dependency set extracted from its expression.
///
/// Template S: `let a = 1`, `let b = 2`, `let c = a + b`, `let d = c + 1`.
/// After eval:
/// - `S.c.dependency_trace.reads` == {S.a, S.b}
/// - `S.d.dependency_trace.reads` == {S.c}  (chain: topologically later)
/// - `S.a.dependency_trace.reads` == {}      (literal)
///
/// The chain case (`d` depends on `c`) exercises the topologically-later path
/// where a cached-vs-recomputed trace lookup diverges if the wrong key or a
/// default value is used — making this stronger than a flat fan-in alone.
#[test]
fn evaluate_let_bindings_cache_records_dependency_trace() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_eval::cache::NodeId;
    use reify_ir::{BinOp, Value};
    use reify_test_support::builders::{binop, literal};

    // let a = 1        (no reads — literal)
    // let b = 2        (no reads — literal)
    // let c = a + b    (reads S.a and S.b)
    // let d = c + 1    (reads S.c — chain dependency)
    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Int, literal(Value::Int(1)))
        .let_binding("S", "b", Type::Int, literal(Value::Int(2)))
        .let_binding(
            "S",
            "c",
            Type::Int,
            binop(
                BinOp::Add,
                reify_test_support::builders::value_ref_typed("S", "a", Type::Int),
                reify_test_support::builders::value_ref_typed("S", "b", Type::Int),
            ),
        )
        .let_binding(
            "S",
            "d",
            Type::Int,
            binop(
                BinOp::Add,
                reify_test_support::builders::value_ref_typed("S", "c", Type::Int),
                literal(Value::Int(1)),
            ),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&module);

    let cache = engine.cache_store();

    // S.c should have reads = {S.a, S.b}
    let node_c = NodeId::Value(ValueCellId::new("S", "c"));
    let cache_c = cache
        .get(&node_c)
        .expect("S.c should be in cache after eval");
    let mut reads_c = cache_c.dependency_trace.reads.clone();
    reads_c.sort();
    let mut expected_c = vec![ValueCellId::new("S", "a"), ValueCellId::new("S", "b")];
    expected_c.sort();
    assert_eq!(
        reads_c, expected_c,
        "S.c dependency_trace.reads should be [S.a, S.b] (sorted), got {:?}",
        cache_c.dependency_trace.reads,
    );

    // S.d should have reads = [S.c] (chain: topologically later node)
    let node_d = NodeId::Value(ValueCellId::new("S", "d"));
    let cache_d = cache
        .get(&node_d)
        .expect("S.d should be in cache after eval");
    let mut reads_d = cache_d.dependency_trace.reads.clone();
    reads_d.sort();
    let mut expected_d = vec![ValueCellId::new("S", "c")];
    expected_d.sort();
    assert_eq!(
        reads_d, expected_d,
        "S.d dependency_trace.reads should be [S.c] (sorted), got {:?}",
        cache_d.dependency_trace.reads,
    );

    // S.a should have empty reads (it's a literal)
    let node_a = NodeId::Value(ValueCellId::new("S", "a"));
    let cache_a = cache
        .get(&node_a)
        .expect("S.a should be in cache after eval");
    assert!(
        cache_a.dependency_trace.reads.is_empty(),
        "S.a dependency_trace.reads should be empty (literal), got {:?}",
        cache_a.dependency_trace.reads,
    );
}

/// Regression test: evaluate_let_bindings must preserve duplicate reads when the same
/// cell appears more than once in a let-binding expression.
///
/// Template:
///   let a = 1       (literal, no reads)
///   let c = a + a   (BinOp; both operands are ValueRef(S.a) — two reads of the same cell)
///
/// `cache_c.dependency_trace.reads` must contain S.a *twice*, not once.  A HashSet
/// comparison would silently pass even if the implementation deduplicates; sorted-Vec
/// equality locks in the multiplicity.  This mirrors the deps.rs-level tests
/// `extract_dependency_trace_preserves_duplicate_reads_for_same_cell_in_binop` but
/// exercises the full evaluate_let_bindings → cache handoff path.
#[test]
fn evaluate_let_bindings_cache_preserves_duplicate_reads_for_same_cell() {
    use reify_core::{ModulePath, Type, ValueCellId};
    use reify_eval::cache::NodeId;
    use reify_ir::{BinOp, Value};
    use reify_test_support::builders::{binop, literal, value_ref_typed};

    // let a = 1        (no reads — literal)
    // let c = a + a    (two reads of S.a — same cell referenced twice)
    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Int, literal(Value::Int(1)))
        .let_binding(
            "S",
            "c",
            Type::Int,
            binop(
                BinOp::Add,
                value_ref_typed("S", "a", Type::Int),
                value_ref_typed("S", "a", Type::Int),
            ),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&module);

    let cache = engine.cache_store();

    let node_c = NodeId::Value(ValueCellId::new("S", "c"));
    let cache_c = cache
        .get(&node_c)
        .expect("S.c should be in cache after eval");

    // Multiplicity check: two reads, not one.
    assert_eq!(
        cache_c.dependency_trace.reads.len(),
        2,
        "S.c dependency_trace.reads should have length 2 (S.a appears twice), got {:?}",
        cache_c.dependency_trace.reads,
    );

    // Content check: both reads refer to S.a.
    let mut reads_c = cache_c.dependency_trace.reads.clone();
    reads_c.sort();
    let mut expected = vec![ValueCellId::new("S", "a"), ValueCellId::new("S", "a")];
    expected.sort();
    assert_eq!(
        reads_c, expected,
        "S.c dependency_trace.reads should be [S.a, S.a] (sorted), got {:?}",
        cache_c.dependency_trace.reads,
    );
}
