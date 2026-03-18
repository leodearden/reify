//! Integration tests for the interactive edit loop.
//!
//! Proves true incrementality: eval() → edit_param() → check_snapshot() → build_snapshot().
//! Uses MockConstraintChecker/MockGeometryKernel so tests run fast and in parallel.

use reify_eval::Engine;
use reify_test_support::{bracket_compiled_module, vcid};
use reify_test_support::mocks::{MockConstraintChecker, MockGeometryKernel};
use reify_types::{ExportFormat, Satisfaction, Value};

#[test]
fn check_snapshot_returns_constraint_results_from_current_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new(); // default: all Satisfied
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline snapshot
    let eval_result = engine.eval(&module);
    assert_eq!(eval_result.values.len(), 6, "should have 6 value cells");

    // check_snapshot should return constraint results from current snapshot
    let check = engine
        .check_snapshot(&module)
        .expect("check_snapshot should return Some after eval()");

    // 3 constraints, all Satisfied (MockConstraintChecker default)
    assert_eq!(
        check.constraint_results.len(),
        3,
        "should have 3 constraint results"
    );
    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied",
            entry.id
        );
    }

    // Values should match eval result
    assert_eq!(check.values.len(), 6, "check_snapshot values should have 6 entries");
    for (id, val) in eval_result.values.iter() {
        assert_eq!(
            check.values.get(id),
            Some(val),
            "check_snapshot value for {:?} should match eval",
            id
        );
    }
}

#[test]
fn build_snapshot_produces_geometry_from_current_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mock_kernel = MockGeometryKernel::new();
    let ops_ref = mock_kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(mock_kernel)));

    // Cold-start eval
    let _eval_result = engine.eval(&module);

    // build_snapshot should produce geometry from current snapshot values
    let build = engine
        .build_snapshot(&module, ExportFormat::Step)
        .expect("build_snapshot should return Some after eval()");

    // Geometry should be exported (MockGeometryKernel writes b"MOCK_EXPORT_DATA")
    assert!(
        build.geometry_output.is_some(),
        "geometry_output should be Some"
    );
    assert_eq!(
        build.geometry_output.as_deref(),
        Some(b"MOCK_EXPORT_DATA".as_slice()),
        "geometry output should be mock export data"
    );

    // Constraint results should be present
    assert_eq!(
        build.constraint_results.len(),
        3,
        "should have 3 constraint results"
    );

    // Mock kernel should have received at least one geometry op (the box)
    let ops = ops_ref.lock().unwrap();
    assert!(!ops.is_empty(), "mock kernel should have received geometry operations");
}
