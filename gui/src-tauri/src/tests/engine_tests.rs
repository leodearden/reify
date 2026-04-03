use std::path::Path;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, bracket_source, bracket_source_with_width};
use reify_types::ExportFormat;

use crate::engine::{EngineSession, parse_value_string};
use crate::types::DiagnosticData;

#[test]
fn engine_session_new_with_mock_kernel() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let _session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
}

#[test]
fn load_from_source_returns_gui_state_with_values() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    // Bracket has 5 params + 1 let (volume) = 6 value cells (body is geometry, not a value)
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values, got {}",
        state.values.len()
    );
}

#[test]
fn load_from_source_returns_gui_state_with_constraints() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert_eq!(state.constraints.len(), 3, "bracket has 3 constraints");
}

#[test]
fn load_from_source_width_value_is_80mm() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "80", "width should be 80mm displayed as 80");
    assert_eq!(width.unit, "mm");
}

#[test]
fn load_from_source_with_invalid_source_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let result = session.load_from_source("this is not valid reify syntax {{{}}", "bad");
    assert!(result.is_err(), "invalid source should return Err");
}

#[test]
fn set_parameter_changes_width() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "120", "width should now be 120mm");
    assert_eq!(width.unit, "mm");
}

#[test]
fn set_parameter_invalid_cell_id_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let result = session.set_parameter("Nonexistent.param", "50mm");
    assert!(result.is_err(), "invalid cell_id should return Err");
}

#[test]
fn set_parameter_constraints_still_correct() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // width = 120mm, thickness = 5mm → thickness > 2mm satisfied, thickness < 120/4=30mm satisfied
    let state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    assert_eq!(state.constraints.len(), 3);
    for c in &state.constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied",
            c.node_id
        );
    }
}

#[test]
fn load_file_returns_gui_state() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Use the examples/bracket.ri file from project root
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    let state = session.load_file(&path).expect("load_file should succeed");

    assert!(state.values.len() >= 5, "should have bracket values");
    assert_eq!(state.constraints.len(), 3, "should have 3 constraints");
}

#[test]
fn update_source_changes_width() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source("bracket.ri", &new_source)
        .expect("update_source should succeed");

    let width = state
        .values
        .iter()
        .find(|v| v.name == "width")
        .expect("should have width value");

    assert_eq!(width.value, "120", "width should be 120mm after update");
}

#[test]
fn update_source_with_invalid_source_returns_err() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let result = session.update_source("bad.ri", "this is not valid {{{}}}");
    assert!(result.is_err(), "invalid source should return Err");
}

// --- Step 11: Integration tests ---

#[test]
fn constraint_violation_roundtrip() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // Set thickness=1mm → violates "thickness > 2mm"
    let state = session
        .set_parameter("Bracket.thickness", "1mm")
        .expect("set thickness should succeed");

    let violated = state.constraints.iter().any(|c| c.status == "Violated");
    assert!(
        violated,
        "should have at least one violated constraint when thickness=1mm"
    );

    // Set back to 5mm → all satisfied again
    let state = session
        .set_parameter("Bracket.thickness", "5mm")
        .expect("set thickness back should succeed");

    for c in &state.constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied after restoring thickness",
            c.node_id
        );
    }
}

#[test]
fn get_source_location_end_to_end() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location for Bracket.width");

    assert_eq!(loc.file_path, "bracket.ri");
    // width is on line 2 of bracket_source() (line 1 = "structure Bracket {")
    assert!(
        loc.line >= 2,
        "width should be on line 2 or later, got {}",
        loc.line
    );
    assert!(loc.column >= 1, "column should be positive");
    assert!(loc.end_line >= loc.line, "end_line should be >= line");
}

#[test]
fn export_end_to_end() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.step");

    let result = session.export(ExportFormat::Step, &path);
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("exported file should be readable");
    assert!(!data.is_empty(), "exported file should not be empty");
}

// --- Step 15: Review bug regression tests ---

/// Review bug #2: source_map key inconsistency.
/// load_from_source inserts key "bracket.ri", but update_source inserts the raw path string.
/// After load_file + update_source, files should have exactly 1 entry (not 2).
#[test]
fn source_map_consistent_after_load_file_then_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    session.load_file(&path).expect("load_file should succeed");

    // Now update_source with the full path string — should normalize key, not create duplicate
    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source(path.to_str().unwrap(), &new_source)
        .expect("update_source should succeed");

    assert_eq!(
        state.files.len(),
        1,
        "should have exactly 1 file entry after load_file + update_source, got {}: {:?}",
        state.files.len(),
        state.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

/// Review bug #3: get_source_location uses non-deterministic HashMap .iter().next().
/// After load_file + update_source, get_source_location should return the correct (single) file.
#[test]
fn get_source_location_correct_after_load_file_then_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/bracket.ri");

    session.load_file(&path).expect("load_file should succeed");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source(path.to_str().unwrap(), &new_source)
        .expect("update_source should succeed");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location");

    // The file in the location should match the single file entry
    assert_eq!(state.files.len(), 1);
    assert_eq!(
        loc.file_path, state.files[0].path,
        "get_source_location file should match the single file entry"
    );
}

/// Review bug #1 regression: export should work without cloning CompiledModule.
/// This test guards the refactor in step-18 that removes the unnecessary .clone().
#[test]
fn export_no_unnecessary_clone() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.step");

    let result = session.export(ExportFormat::Step, &path);
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    // Verify output was written
    let data = std::fs::read(&path).expect("exported file should be readable");
    assert!(!data.is_empty(), "exported file should not be empty");

    // Verify engine state is still usable after export (no moved/consumed fields)
    let state = session
        .build_gui_state()
        .expect("build_gui_state after export");
    assert!(
        !state.values.is_empty(),
        "values should still be available after export"
    );
}

/// Review bug #4: [state_corruption_not_tested] + [state_inconsistency_on_error]
/// update_source() clears source_map and inserts new content BEFORE parse/compile.
/// On parse failure, old valid source is destroyed — get_source_location uses old byte offsets
/// against invalid source, and build_gui_state().files has invalid content.
/// After fix: on error, state should be completely unchanged.
#[test]
fn get_source_location_correct_after_failed_update() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // (1) Load valid source
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    // (2) Record source location for Bracket.width before failed update
    let loc_before = session
        .get_source_location("Bracket.width")
        .expect("should find source location before failed update");

    // (3) Attempt invalid update — should fail
    let result = session.update_source("bracket.ri", "this is not valid {{{}}}");
    assert!(result.is_err(), "invalid source should return Err");

    // (4) get_source_location should return the SAME line/col as before the failed update
    let loc_after = session
        .get_source_location("Bracket.width")
        .expect("should still find source location after failed update");
    assert_eq!(
        loc_before.line, loc_after.line,
        "line should be unchanged after failed update"
    );
    assert_eq!(
        loc_before.column, loc_after.column,
        "column should be unchanged after failed update"
    );
    assert_eq!(
        loc_before.file_path, loc_after.file_path,
        "file should be unchanged after failed update"
    );

    // (5) build_gui_state should still return Ok with original valid state
    let state = session
        .build_gui_state()
        .expect("build_gui_state should work after failed update");
    assert!(
        state.values.len() >= 5,
        "should still have original values after failed update, got {}",
        state.values.len()
    );
    assert_eq!(state.files.len(), 1);
    assert!(
        state.files[0].content.contains("structure Bracket"),
        "files should still contain original valid source, got: {}",
        &state.files[0].content[..50.min(state.files[0].content.len())]
    );
}

/// Review bug #3: get_source_location should use explicit key lookup, not .iter().next().
/// After load_from_source, the file should be the normalized "bracket.ri" key.
#[test]
fn get_source_location_uses_explicit_key_lookup() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find source location");

    // Should return the normalized module-name-based key
    assert_eq!(
        loc.file_path, "bracket.ri",
        "get_source_location should return normalized module-name key"
    );
}

// --- Step 21: Unit table ordering tests ---

/// Verify all supported unit suffixes parse correctly.
#[test]
fn parse_value_string_all_units_correct() {
    use reify_types::{DimensionVector, Value};

    // mm → 0.001 * value, LENGTH
    let v = parse_value_string("5mm").expect("5mm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.005).abs() < 1e-10,
                "5mm → 0.005, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5mm should be Scalar, got {:?}", v),
    }

    // cm → 0.01 * value, LENGTH
    let v = parse_value_string("5cm").expect("5cm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.05).abs() < 1e-10,
                "5cm → 0.05, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5cm should be Scalar, got {:?}", v),
    }

    // m → 1.0 * value, LENGTH
    let v = parse_value_string("5m").expect("5m should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!((si_value - 5.0).abs() < 1e-10, "5m → 5.0, got {}", si_value);
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5m should be Scalar, got {:?}", v),
    }

    // deg → PI/180 * value, ANGLE
    let v = parse_value_string("90deg").expect("90deg should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-10,
                "90deg → PI/2, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::ANGLE);
        }
        _ => panic!("90deg should be Scalar, got {:?}", v),
    }

    // rad → 1.0 * value, ANGLE
    let v = parse_value_string("1rad").expect("1rad should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.0).abs() < 1e-10,
                "1rad → 1.0, got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::ANGLE);
        }
        _ => panic!("1rad should be Scalar, got {:?}", v),
    }
}

/// Verify 'm' suffix does not shadow longer suffixes like 'cm'.
/// '100cm' must produce si_value=1.0 (not 100.0 from 'm' matching 'cm' trailing).
#[test]
fn parse_value_string_m_does_not_shadow_longer_suffixes() {
    use reify_types::{DimensionVector, Value};

    let v = parse_value_string("100cm").expect("100cm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.0).abs() < 1e-10,
                "100cm → 1.0, got {} (would be 100.0 if 'm' shadowed 'cm')",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("100cm should be Scalar, got {:?}", v),
    }
}

/// Verify unit table ordering invariant:
/// '5mm' must produce si_value 0.005 (not 5.0 from 'm' match).
/// '45deg' must produce ANGLE (ensures 3-char suffixes work correctly).
/// These tests document the ordering contract and will catch regressions.
#[test]
fn parse_value_string_unit_table_ordering_invariant() {
    use reify_types::{DimensionVector, Value};

    // '5mm' must be recognized as millimeters, not meters
    let v = parse_value_string("5mm").expect("5mm should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.005).abs() < 1e-10,
                "5mm → 0.005 (not 5.0 from 'm' match), got {}",
                si_value
            );
            assert_eq!(dimension, DimensionVector::LENGTH);
        }
        _ => panic!("5mm should be Scalar, got {:?}", v),
    }

    // '45deg' must be recognized as degrees (ANGLE), not fail or parse incorrectly
    let v = parse_value_string("45deg").expect("45deg should parse");
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            let expected = 45.0 * std::f64::consts::PI / 180.0;
            assert!(
                (si_value - expected).abs() < 1e-10,
                "45deg → {}, got {}",
                expected,
                si_value
            );
            assert_eq!(
                dimension,
                DimensionVector::ANGLE,
                "45deg should be ANGLE dimension"
            );
        }
        _ => panic!("45deg should be Scalar, got {:?}", v),
    }
}

// --- Task 132: Tessellation integration tests ---

#[test]
fn build_gui_state_includes_meshes_from_tessellation() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert!(
        !state.meshes.is_empty(),
        "build_gui_state should produce meshes when a geometry kernel is available, got empty"
    );
}

#[test]
fn build_gui_state_mesh_data_structure_matches_kernel_output() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    assert!(!state.meshes.is_empty(), "should have at least one mesh");
    let mesh = &state.meshes[0];

    // MockGeometryKernel returns: vertices = [0,0,0, 1,0,0, 0,1,0] (9 floats = 3 vertices)
    assert_eq!(
        mesh.vertices.len(),
        9,
        "expected 9 vertex floats (3 vertices × 3 coords)"
    );
    // indices = [0, 1, 2] (1 triangle)
    assert_eq!(mesh.indices.len(), 3, "expected 3 indices (1 triangle)");
    // normals = Some([0,0,1, 0,0,1, 0,0,1]) (9 floats)
    assert!(mesh.normals.is_some(), "expected normals to be present");
    assert_eq!(
        mesh.normals.as_ref().unwrap().len(),
        9,
        "expected 9 normal floats"
    );
    // entity_path should be non-empty
    assert!(
        !mesh.entity_path.is_empty(),
        "entity_path should be non-empty"
    );
}

#[test]
fn build_gui_state_no_kernel_returns_empty_meshes() {
    let checker = SimpleConstraintChecker;
    // No geometry kernel
    let mut session = EngineSession::new(Box::new(checker), None);

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed even without kernel");

    // Meshes should be empty when no kernel is available
    assert!(
        state.meshes.is_empty(),
        "expected empty meshes without geometry kernel, got {}",
        state.meshes.len()
    );

    // Values and constraints should still be populated
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values without kernel, got {}",
        state.values.len()
    );
    assert_eq!(
        state.constraints.len(),
        3,
        "expected 3 constraints without kernel"
    );
}

#[test]
fn build_gui_state_tessellation_preserves_values_and_constraints() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed");

    // Tessellation should produce meshes
    assert!(
        !state.meshes.is_empty(),
        "expected non-empty meshes with geometry kernel"
    );

    // And values/constraints should still be fully populated (tessellation doesn't interfere)
    assert!(
        state.values.len() >= 5,
        "expected at least 5 values alongside meshes, got {}",
        state.values.len()
    );
    assert_eq!(
        state.constraints.len(),
        3,
        "expected 3 constraints alongside meshes"
    );
}

#[test]
fn set_parameter_produces_updated_meshes() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let initial_state = session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    assert!(
        !initial_state.meshes.is_empty(),
        "initial state should have meshes"
    );

    // Set parameter and verify meshes are still produced
    let updated_state = session
        .set_parameter("Bracket.width", "120mm")
        .expect("set_parameter should succeed");

    assert!(
        !updated_state.meshes.is_empty(),
        "updated state should have meshes after set_parameter"
    );
}

#[test]
fn update_source_produces_meshes() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");

    let new_source = bracket_source_with_width("120mm");
    let state = session
        .update_source("bracket.ri", &new_source)
        .expect("update_source should succeed");

    assert!(
        !state.meshes.is_empty(),
        "update_source should produce meshes"
    );
}

// --- Task 827: get_diagnostics tests ---

/// Step-1 (TDD failing test): get_diagnostics() returns empty vec when no module is loaded.
/// This test fails with a compile error until EngineSession::get_diagnostics() is implemented.
#[test]
fn engine_get_diagnostics_no_module_returns_empty() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);

    let diags: Vec<DiagnosticData> = session.get_diagnostics();
    assert!(diags.is_empty(), "no module loaded → diagnostics must be empty");
}

/// Step-8 (REVIEW FIX — missing positive coverage): get_diagnostics() returns a non-empty vec
/// when the compiled module contains a warning.
///
/// Source with `port mount : NonExistentTrait` produces an "unknown port type" warning
/// (validated by crates/reify-compiler/tests/port_compile_tests.rs:101-124).
/// load_from_source() succeeds (warnings are not errors), so compiled.diagnostics stores
/// the warning. get_diagnostics() then surfaces it, exercising:
///   - the non-empty iteration path
///   - byte_offset_to_line_col span conversion
///   - file_path resolution from module_name
///   - severity Display formatting
///   - message propagation
#[test]
fn engine_get_diagnostics_returns_populated_warning() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let source = r#"structure def S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#;

    // load_from_source should succeed — warnings are not errors
    session
        .load_from_source(source, "test_warn")
        .expect("source with unknown port type should compile (warning, not error)");

    let diags: Vec<DiagnosticData> = session.get_diagnostics();

    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unknown port type, got empty"
    );

    let first = &diags[0];

    // severity must be "warning"
    assert_eq!(
        first.severity, "warning",
        "expected severity 'warning', got '{}'",
        first.severity
    );

    // message must mention the unknown port type
    assert!(
        first.message.contains("unknown port type"),
        "expected message to contain 'unknown port type', got: '{}'",
        first.message
    );
    assert!(
        first.message.contains("NonExistentTrait"),
        "expected message to mention 'NonExistentTrait', got: '{}'",
        first.message
    );

    // file_path must be derived from the module name passed to load_from_source
    assert_eq!(
        first.file_path, "test_warn.ri",
        "expected file_path 'test_warn.ri', got '{}'",
        first.file_path
    );

    // line and column must be valid 1-based values
    assert!(
        first.line >= 1,
        "expected line >= 1, got {}",
        first.line
    );
    assert!(
        first.column >= 1,
        "expected column >= 1, got {}",
        first.column
    );

    // end_line and end_column must form a coherent range
    assert!(
        first.end_line >= first.line,
        "expected end_line ({}) >= line ({})",
        first.end_line,
        first.line
    );
    assert!(
        first.end_column >= 1,
        "expected end_column >= 1, got {}",
        first.end_column
    );
}

/// Step-3: get_diagnostics() returns empty vec for bracket_source() (warning-free source).
/// Validates the method works end-to-end on a real compiled module.
#[test]
fn engine_get_diagnostics_clean_source_returns_empty() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    let diags: Vec<DiagnosticData> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "bracket source has no warnings — diagnostics must be empty, got: {:?}",
        diags
    );
}

/// Step-2: When module_name is cleared after load, get_diagnostics() falls back
/// to source_map.iter().next() and still returns the warning.
///
/// This exercises the `else` branch at engine.rs:278-283: normally module_name
/// is always set after load_from_source(), but the test helper lets us reach
/// this branch to verify the fallback key resolution works correctly.
#[test]
fn engine_get_diagnostics_module_name_none_uses_fallback() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let source = r#"structure def S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#;

    session
        .load_from_source(source, "test_warn")
        .expect("source with unknown port type should compile (warning, not error)");

    // Clear module_name to force the iter().next() fallback path
    session.clear_module_name_for_test();

    let diags: Vec<DiagnosticData> = session.get_diagnostics();

    // (a) Fallback path found the source_map entry, so diagnostics are non-empty
    assert!(
        !diags.is_empty(),
        "expected diagnostic via fallback path, got empty"
    );

    let first = &diags[0];

    // (b) file_path comes from the source_map key, not module_name
    assert_eq!(
        first.file_path, "test_warn.ri",
        "expected file_path 'test_warn.ri' via fallback, got '{}'",
        first.file_path
    );

    // (c) severity is warning
    assert_eq!(
        first.severity, "warning",
        "expected severity 'warning', got '{}'",
        first.severity
    );

    // (d) message mentions unknown port type
    assert!(
        first.message.contains("unknown port type"),
        "expected message to contain 'unknown port type', got: '{}'",
        first.message
    );
}

/// Step-3: A diagnostic with no labels gets (1,1,1,1) coordinates.
///
/// This exercises the `else` branch of `diag.labels.first()` at engine.rs:295-296.
/// The compiler always attaches labels; inject_diagnostic_for_test() lets us plant
/// a labelless diagnostic to verify the (1,1,1,1) fallback.
#[test]
fn engine_get_diagnostics_labelless_diagnostic_returns_default_span() {
    use reify_types::Diagnostic;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    // Inject a warning with no labels — this is the labelless case
    session.inject_diagnostic_for_test(Diagnostic::warning("test labelless"));

    let diags: Vec<DiagnosticData> = session.get_diagnostics();

    // (a) The injected diagnostic appears
    assert!(!diags.is_empty(), "expected injected diagnostic, got empty");

    // Find the injected one (bracket_source has none of its own)
    let injected = diags
        .iter()
        .find(|d| d.message == "test labelless")
        .expect("injected 'test labelless' diagnostic not found in results");

    // (b) All coordinates default to (1,1,1,1)
    assert_eq!(injected.line, 1, "expected line=1 for labelless, got {}", injected.line);
    assert_eq!(injected.column, 1, "expected column=1 for labelless, got {}", injected.column);
    assert_eq!(injected.end_line, 1, "expected end_line=1 for labelless, got {}", injected.end_line);
    assert_eq!(injected.end_column, 1, "expected end_column=1 for labelless, got {}", injected.end_column);

    // (c) Severity preserved
    assert_eq!(
        injected.severity, "warning",
        "expected severity 'warning', got '{}'",
        injected.severity
    );

    // (d) Message preserved
    assert_eq!(
        injected.message, "test labelless",
        "expected message 'test labelless', got '{}'",
        injected.message
    );
}

/// Step-4: After update_source with clean source, get_diagnostics() returns empty.
///
/// Verifies the update_source→get_diagnostics lifecycle contract: the compiled
/// module (and its diagnostics) are replaced on each update, so stale diagnostics
/// from a previous compilation do not persist.
#[test]
fn engine_get_diagnostics_cleared_after_update_to_clean_source() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // Load warning source — establishes a non-empty diagnostics state
    let warn_source = r#"structure def S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#;
    session
        .load_from_source(warn_source, "test_warn")
        .expect("warning source should compile");

    let diags_before = session.get_diagnostics();
    assert!(
        !diags_before.is_empty(),
        "expected diagnostics before update, got empty"
    );

    // Update the same file to clean source — diagnostics must be cleared
    session
        .update_source("test_warn.ri", bracket_source())
        .expect("bracket source should compile cleanly");

    let diags_after = session.get_diagnostics();
    assert!(
        diags_after.is_empty(),
        "expected empty diagnostics after updating to clean source, got: {:?}",
        diags_after
    );
}
