//! Integration tests for the incremental evaluator pipeline.
//!
//! These tests verify that Engine's incremental evaluation (edit_param)
//! produces correct results, proper provenance, partial re-evaluation,
//! early cutoff, and freshness transitions.

use reify_eval::Engine;
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{Value, ValueCellId};

/// Canary backward-compatibility test: verifies that cold-start eval()
/// produces the correct values for the bracket fixture.
/// This test must pass BEFORE and AFTER the Engine refactoring.
#[test]
fn cold_start_eval_produces_correct_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let result = engine.eval(&module);

    let e = "Bracket";

    // 5 params
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.08)),
        "width should be 80mm = 0.08m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "height")),
        Some(&Value::length(0.10)),
        "height should be 100mm = 0.10m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "thickness")),
        Some(&Value::length(0.005)),
        "thickness should be 5mm = 0.005m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "fillet_radius")),
        Some(&Value::length(0.003)),
        "fillet_radius should be 3mm = 0.003m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "hole_diameter")),
        Some(&Value::length(0.006)),
        "hole_diameter should be 6mm = 0.006m"
    );

    // 1 let binding: volume = width * height * thickness
    // = 0.08 * 0.10 * 0.005 = 0.00004 = 4e-5
    let volume = result.values.get(&ValueCellId::new(e, "volume"));
    assert!(volume.is_some(), "volume should exist");
    let vol_f64 = volume.unwrap().as_f64().expect("volume should be numeric");
    assert!(
        (vol_f64 - 4e-5).abs() < 1e-10,
        "volume should be ~4e-5 m³, got {}",
        vol_f64
    );

    // Total: 6 values
    assert_eq!(result.values.len(), 6, "should have exactly 6 values");
    assert!(result.diagnostics.is_empty(), "no diagnostics expected");
}
