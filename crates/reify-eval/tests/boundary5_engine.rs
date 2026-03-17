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
