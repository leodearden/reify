//! Integration tests for the interactive edit loop.
//!
//! Proves true incrementality: eval() → edit_param() → check_snapshot() → build_snapshot().
//! Uses MockConstraintChecker/MockGeometryKernel so tests run fast and in parallel.

use reify_eval::Engine;
use reify_test_support::{bracket_compiled_module, cnid, vcid};
use reify_test_support::mocks::{MockConstraintChecker, MockGeometryKernel};
use reify_constraints::SimpleConstraintChecker;
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

#[test]
fn edit_param_then_check_snapshot_reflects_updated_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    let _eval_result = engine.eval(&module);

    // Edit width: 80mm → 100mm (0.1m)
    let _edit_result = engine.edit_param(vcid("Bracket", "width"), Value::length(0.1));

    // check_snapshot should reflect updated values
    let check = engine
        .check_snapshot(&module)
        .expect("check_snapshot should return Some after edit_param()");

    // Verify width was updated to 0.1m
    let width_val = check.values.get(&vcid("Bracket", "width")).expect("width should exist");
    assert_eq!(*width_val, Value::length(0.1), "width should be 0.1m (100mm)");

    // Verify volume was recomputed: 0.1m * 0.1m * 0.005m = 5e-5 m³
    let volume_val = check.values.get(&vcid("Bracket", "volume")).expect("volume should exist");
    let expected_volume = 0.1 * 0.1 * 0.005; // 5e-5 m³
    match volume_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (*si_value - expected_volume).abs() < 1e-12,
                "volume should be {expected_volume}, got {si_value}"
            );
        }
        other => panic!("volume should be Scalar, got {:?}", other),
    }

    // Unchanged params should be preserved
    let height_val = check.values.get(&vcid("Bracket", "height")).expect("height should exist");
    assert_eq!(*height_val, Value::length(0.1), "height should still be 100mm = 0.1m");

    let thickness_val = check.values.get(&vcid("Bracket", "thickness")).expect("thickness should exist");
    assert_eq!(*thickness_val, Value::length(0.005), "thickness should still be 5mm = 0.005m");

    // All 3 constraints still Satisfied (width 100mm doesn't violate any constraint)
    assert_eq!(check.constraint_results.len(), 3);
    for entry in &check.constraint_results {
        assert_eq!(entry.satisfaction, Satisfaction::Satisfied);
    }
}

#[test]
fn edit_param_constraint_violation_detected() {
    let module = bracket_compiled_module();
    // Use SimpleConstraintChecker to actually evaluate constraint expressions
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval — all constraints satisfied with defaults
    let _eval_result = engine.eval(&module);

    // Edit thickness: 5mm → 1mm (0.001m) — violates `thickness > 2mm`
    let _edit_result = engine.edit_param(vcid("Bracket", "thickness"), Value::length(0.001));

    // check_snapshot should detect the violation
    let check = engine
        .check_snapshot(&module)
        .expect("check_snapshot should return Some");

    // Verify thickness was updated
    let thickness_val = check.values.get(&vcid("Bracket", "thickness")).expect("thickness");
    assert_eq!(*thickness_val, Value::length(0.001), "thickness should be 0.001m (1mm)");

    // Constraint 0: thickness > 2mm — should be VIOLATED (1mm < 2mm)
    let c0 = check.constraint_results.iter().find(|e| e.id == cnid("Bracket", 0))
        .expect("constraint 0 should exist");
    assert_eq!(
        c0.satisfaction,
        Satisfaction::Violated,
        "thickness > 2mm should be violated when thickness=1mm"
    );

    // Constraint 1: thickness < width/4 — should be Satisfied (1mm < 80mm/4 = 20mm)
    let c1 = check.constraint_results.iter().find(|e| e.id == cnid("Bracket", 1))
        .expect("constraint 1 should exist");
    assert_eq!(
        c1.satisfaction,
        Satisfaction::Satisfied,
        "thickness < width/4 should be satisfied when thickness=1mm"
    );
}
