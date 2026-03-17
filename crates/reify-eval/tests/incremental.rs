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

/// After cold-start eval, edit width from 80mm to 100mm.
/// Verify updated values: width=100mm, volume recomputed, others unchanged.
#[test]
fn edit_param_returns_updated_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    let initial = engine.eval(&module);
    let e = "Bracket";

    // Edit width from 80mm (0.08m) to 100mm (0.1m)
    let width_id = ValueCellId::new(e, "width");
    let result = engine.edit_param(width_id, Value::length(0.1));

    // Width should be updated
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.1)),
        "width should be 100mm = 0.1m after edit"
    );

    // Volume should be recomputed: 0.1 * 0.1 * 0.005 = 5e-5
    let volume = result.values.get(&ValueCellId::new(e, "volume"));
    assert!(volume.is_some(), "volume should exist");
    let vol_f64 = volume.unwrap().as_f64().expect("volume should be numeric");
    assert!(
        (vol_f64 - 5e-5).abs() < 1e-10,
        "volume should be ~5e-5 m³ after width edit, got {}",
        vol_f64
    );

    // Other params unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "height")),
        initial.values.get(&ValueCellId::new(e, "height")),
        "height should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "thickness")),
        initial.values.get(&ValueCellId::new(e, "thickness")),
        "thickness should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "fillet_radius")),
        initial.values.get(&ValueCellId::new(e, "fillet_radius")),
        "fillet_radius should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "hole_diameter")),
        initial.values.get(&ValueCellId::new(e, "hole_diameter")),
        "hole_diameter should be unchanged"
    );
}
