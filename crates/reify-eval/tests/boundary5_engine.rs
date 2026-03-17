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
