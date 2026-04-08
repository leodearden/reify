use std::path::Path;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, bracket_source, bracket_source_with_width};
use reify_types::ExportFormat;

use reify_mcp::{DiagnosticInfo, SourceLocationInfo};

use crate::engine::{EngineSession, parse_value_string};

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
fn get_source_location_returns_source_location_info() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let loc: SourceLocationInfo = session
        .get_source_location("Bracket.width")
        .expect("should find source location for Bracket.width");

    assert_eq!(loc.file_path, "bracket.ri");
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

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "no module loaded → diagnostics must be empty"
    );
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

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();

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
    assert!(first.line >= 1, "expected line >= 1, got {}", first.line);
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

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();
    assert!(
        diags.is_empty(),
        "bracket source has no warnings — diagnostics must be empty, got: {:?}",
        diags
    );
}

// --- Task 836: resolve_source pinning tests ---

/// get_source_location returns None when no module is loaded.
/// Documents the early-return (`let compiled = self.compiled.as_ref()?`)
/// that fires before resolve_source is reached.
#[test]
fn get_source_location_returns_none_without_module() {
    let checker = SimpleConstraintChecker;
    let session = EngineSession::new(Box::new(checker), None);

    let loc = session.get_source_location("Bracket.width");
    assert!(
        loc.is_none(),
        "get_source_location should return None when no module is loaded"
    );
}

/// get_diagnostics and get_source_location return the same file key.
///
/// After load_from_source with a warning-producing source, both methods must resolve
/// the file key through the same "{module_name}.ri" derivation via resolve_source.
#[test]
fn diagnostics_and_source_location_agree_on_file_key() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let source = r#"structure S {
    param width : Length = 80mm
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#;

    session
        .load_from_source(source, "testmod")
        .expect("source with unknown port type should compile (warning, not error)");

    let diags = session.get_diagnostics();
    assert!(
        !diags.is_empty(),
        "expected at least one diagnostic for unknown port type"
    );
    assert_eq!(
        diags[0].file_path, "testmod.ri",
        "get_diagnostics file_path"
    );

    let loc = session
        .get_source_location("S.width")
        .expect("should find source location for S.width");
    assert_eq!(loc.file_path, "testmod.ri", "get_source_location file_path");
}

/// get_diagnostics uses the updated module name key after update_source.
///
/// After load_from_source("initial") then update_source("updated.ri", ...),
/// get_diagnostics must resolve the new key "updated.ri", not "initial.ri".
#[test]
fn diagnostics_file_key_consistent_after_update_source() {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);

    let warning_source = r#"structure S {
    port mount : NonExistentTrait {
        param d : Length = 5mm
    }
}"#;

    session
        .load_from_source(warning_source, "initial")
        .expect("initial load should succeed");

    let diags_before = session.get_diagnostics();
    assert!(
        !diags_before.is_empty(),
        "should have diagnostics after initial load"
    );
    assert_eq!(
        diags_before[0].file_path, "initial.ri",
        "before update: file_path should be 'initial.ri'"
    );

    session
        .update_source("updated.ri", warning_source)
        .expect("update_source should succeed");

    let diags_after = session.get_diagnostics();
    assert!(
        !diags_after.is_empty(),
        "should still have diagnostics after update_source"
    );
    assert_eq!(
        diags_after[0].file_path, "updated.ri",
        "after update_source, file_path should be 'updated.ri'"
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

    let diags: Vec<DiagnosticInfo> = session.get_diagnostics();

    // (a) The injected diagnostic appears
    assert!(!diags.is_empty(), "expected injected diagnostic, got empty");

    // Find the injected one (bracket_source has none of its own)
    let injected = diags
        .iter()
        .find(|d| d.message == "test labelless")
        .expect("injected 'test labelless' diagnostic not found in results");

    // (b) All coordinates default to (1,1,1,1)
    assert_eq!(
        injected.line, 1,
        "expected line=1 for labelless, got {}",
        injected.line
    );
    assert_eq!(
        injected.column, 1,
        "expected column=1 for labelless, got {}",
        injected.column
    );
    assert_eq!(
        injected.end_line, 1,
        "expected end_line=1 for labelless, got {}",
        injected.end_line
    );
    assert_eq!(
        injected.end_column, 1,
        "expected end_column=1 for labelless, got {}",
        injected.end_column
    );

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

// --- Task 837: build_line_offsets unit tests ---

/// build_line_offsets returns empty vec for empty string.
#[test]
fn build_line_offsets_empty_string() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("");
    assert_eq!(offsets, Vec::<usize>::new());
}

/// build_line_offsets returns empty vec for a single-line string (no '\n').
#[test]
fn build_line_offsets_single_line() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("hello world");
    assert_eq!(offsets, Vec::<usize>::new());
}

/// build_line_offsets returns correct byte positions of '\n' for a multi-line string.
///
/// "abc\ndef\nghi"
///  0123 4567 8910
/// '\n' at byte 3 and byte 7.
#[test]
fn build_line_offsets_multi_line() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("abc\ndef\nghi");
    assert_eq!(offsets, vec![3, 7]);
}

/// build_line_offsets handles a trailing newline (last char is '\n').
///
/// "abc\ndef\n"
///  0123 4567 8
/// '\n' at byte 3 and byte 7.
#[test]
fn build_line_offsets_trailing_newline() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("abc\ndef\n");
    assert_eq!(offsets, vec![3, 7]);
}

/// build_line_offsets handles a string that is only newlines.
///
/// "\n\n\n" → '\n' at bytes 0, 1, 2.
#[test]
fn build_line_offsets_only_newlines() {
    use crate::engine::build_line_offsets;
    let offsets = build_line_offsets("\n\n\n");
    assert_eq!(offsets, vec![0, 1, 2]);
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

// --- byte_offset_to_line_col edge-case tests ---

#[test]
fn byte_offset_to_line_col_basic_conversion() {
    use crate::engine::byte_offset_to_line_col;

    let source = "abc\ndef";
    // offset 0 → start of first line → (1, 1)
    assert_eq!(byte_offset_to_line_col(source, 0), (1, 1));
    // offset 3 → just before the '\n' → (1, 4) (col after 'a','b','c')
    assert_eq!(byte_offset_to_line_col(source, 3), (1, 4));
    // offset 4 → first char of second line → (2, 1)
    assert_eq!(byte_offset_to_line_col(source, 4), (2, 1));
    // offset 6 → last char 'f' → (2, 3)
    assert_eq!(byte_offset_to_line_col(source, 6), (2, 3));
}

#[test]
fn byte_offset_to_line_col_empty_source() {
    use crate::engine::byte_offset_to_line_col;

    // Empty source: offset 0 → initial position (1, 1)
    assert_eq!(byte_offset_to_line_col("", 0), (1, 1));
}

#[test]
fn byte_offset_to_line_col_offset_beyond_len() {
    use crate::engine::byte_offset_to_line_col;

    // Source "ab" (len=2). Offset 100 far exceeds the source length.
    // The loop exhausts all chars without hitting the break, leaving the
    // position after the last char: column incremented for 'a' and 'b' → (1, 3).
    assert_eq!(byte_offset_to_line_col("ab", 100), (1, 3));
}

#[test]
fn byte_offset_to_line_col_empty_span_identical_coords() {
    use crate::engine::byte_offset_to_line_col;

    // When a diagnostic span has start == end (empty span), both calls to
    // byte_offset_to_line_col with the same offset must return identical coords.
    // offset 6 in "hello\nworld" (after '\n') → start of second line → (2, 1).
    let source = "hello\nworld";
    let offset = 6; // 'w' is the first char of "world"
    let start_coord = byte_offset_to_line_col(source, offset);
    let end_coord = byte_offset_to_line_col(source, offset);
    assert_eq!(
        start_coord, end_coord,
        "empty span: identical offsets must produce identical coords"
    );
    assert_eq!(start_coord, (2, 1));
}

#[test]
fn byte_offset_to_line_col_multibyte_chars() {
    use crate::engine::byte_offset_to_line_col;

    // Source: "αβ\nγ"
    // α = U+03B1, 2 bytes (UTF-8: 0xCE 0xB1), byte offset 0
    // β = U+03B2, 2 bytes (UTF-8: 0xCE 0xB2), byte offset 2
    // \n              ,  byte offset 4
    // γ = U+03B3, 2 bytes (UTF-8: 0xCE 0xB3), byte offset 5
    //
    // Columns must be codepoint-based (1, 2, 3), not byte-based (1, 3, 5).
    let source = "αβ\nγ";
    assert_eq!(source.len(), 7, "sanity-check byte length");

    // offset 0 → 'α' (codepoint 1 on line 1) → (1, 1)
    assert_eq!(byte_offset_to_line_col(source, 0), (1, 1));
    // offset 2 → 'β' (codepoint 2 on line 1) → (1, 2)
    assert_eq!(byte_offset_to_line_col(source, 2), (1, 2));
    // offset 4 → '\n' (codepoint 3 on line 1) → (1, 3)
    assert_eq!(byte_offset_to_line_col(source, 4), (1, 3));
    // offset 5 → 'γ' (first codepoint on line 2) → (2, 1)
    assert_eq!(byte_offset_to_line_col(source, 5), (2, 1));
}

#[test]
fn byte_offset_to_line_col_at_source_len() {
    use crate::engine::byte_offset_to_line_col;

    // Source "abc\ndef" has byte length 7.
    // offset == source.len() is the EOF position, one past the last char 'f'.
    // The loop iterates all chars (indices 0-6, all < 7) exhausting them,
    // so: 'a'→col2, 'b'→col3, 'c'→col4, '\n'→line2,col1, 'd'→col2, 'e'→col3, 'f'→col4
    // Then the loop ends and we return (2, 4).
    let source = "abc\ndef";
    assert_eq!(source.len(), 7, "sanity-check byte length");
    assert_eq!(byte_offset_to_line_col(source, 7), (2, 4));
}

// --- Task 837: offset_to_line_col_fast unit tests ---

/// offset_to_line_col_fast returns (1,1) for offset 0 on any source.
#[test]
fn offset_to_line_col_fast_offset_zero() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
}

/// offset_to_line_col_fast cross-validates with byte_offset_to_line_col
/// for every byte offset in a multi-line string.
#[test]
fn offset_to_line_col_fast_matches_original_every_offset() {
    use crate::engine::{build_line_offsets, byte_offset_to_line_col, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let line_offsets = build_line_offsets(source);
    for offset in 0..source.len() {
        let expected = byte_offset_to_line_col(source, offset);
        let actual = offset_to_line_col_fast(source, &line_offsets, offset);
        assert_eq!(
            actual, expected,
            "mismatch at offset {}: fast={:?} original={:?}",
            offset, actual, expected
        );
    }
}

/// offset_to_line_col_fast returns correct values at specific key offsets.
///
/// "abc\ndef\nghi" — '\n' at bytes 3 and 7.
/// offset 3  → (1,4) — the '\n' itself is still on line 1
/// offset 4  → (2,1) — first char of line 2
/// offset 7  → (2,4) — the second '\n'
/// offset 8  → (3,1) — first char of line 3
/// offset 10 → (3,3) — last char 'i'
#[test]
fn offset_to_line_col_fast_key_positions() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "abc\ndef\nghi";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 3), (1, 4)); // '\n'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 4), (2, 1)); // 'd'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 7), (2, 4)); // '\n'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 8), (3, 1)); // 'g'
    assert_eq!(offset_to_line_col_fast(source, &offsets, 10), (3, 3)); // 'i'
}

/// offset_to_line_col_fast works on empty source (no newlines).
#[test]
fn offset_to_line_col_fast_empty_source() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
}

/// offset_to_line_col_fast works on single-line source (no newlines).
#[test]
fn offset_to_line_col_fast_single_line() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "hello";
    let offsets = build_line_offsets(source);
    assert_eq!(offset_to_line_col_fast(source, &offsets, 0), (1, 1));
    assert_eq!(offset_to_line_col_fast(source, &offsets, 4), (1, 5));
}

/// offset_to_line_col_fast agrees with byte_offset_to_line_col at source.len()
/// (one-past-end / EOF position, the highest offset a compiler span can produce).
///
/// For offsets strictly beyond source.len() the two implementations diverge —
/// the original stops iterating at the last source char while the fast version
/// extrapolates the column — but that case never occurs in production because
/// diagnostic spans are always within source bounds.
#[test]
fn offset_to_line_col_fast_at_eof_offset() {
    use crate::engine::{build_line_offsets, byte_offset_to_line_col, offset_to_line_col_fast};
    let source = "abc\ndef";
    let line_offsets = build_line_offsets(source);
    // source.len() is the EOF position — both implementations must agree here.
    let eof = source.len();
    let expected = byte_offset_to_line_col(source, eof);
    let actual = offset_to_line_col_fast(source, &line_offsets, eof);
    assert_eq!(actual, expected, "EOF offset: fast={:?} original={:?}", actual, expected);
}

// --- Task 837: step-7 stress / multi-diagnostic tests ---

/// get_diagnostics with multiple injected diagnostics at various byte offsets
/// produces line/col values matching byte_offset_to_line_col for each span.
///
/// This is the primary end-to-end regression for the optimized path: we inject
/// three warnings with labels at byte positions we compute from bracket_source,
/// then verify get_diagnostics returns the same line/col as the O(M) reference.
#[test]
fn get_diagnostics_multi_diagnostic_stress_matches_reference() {
    use reify_types::{Diagnostic, DiagnosticLabel, SourceSpan};
    use crate::engine::byte_offset_to_line_col;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    let source = bracket_source();
    session
        .load_from_source(source, "bracket")
        .expect("bracket source should compile cleanly");

    // Pick three byte offsets that land at recognisable tokens across
    // different lines, using `find` so the test stays robust to whitespace.
    let offset_a = source.find("width").expect("'width' not in bracket_source") as u32;
    let offset_b = source.find("height").expect("'height' not in bracket_source") as u32;
    let offset_c = source.find("thickness").expect("'thickness' not in bracket_source") as u32;

    let diag_a = Diagnostic::warning("stress-a")
        .with_label(DiagnosticLabel::new(
            SourceSpan::new(offset_a, offset_a + 5),
            "label a",
        ));
    let diag_b = Diagnostic::warning("stress-b")
        .with_label(DiagnosticLabel::new(
            SourceSpan::new(offset_b, offset_b + 6),
            "label b",
        ));
    let diag_c = Diagnostic::warning("stress-c")
        .with_label(DiagnosticLabel::new(
            SourceSpan::new(offset_c, offset_c + 9),
            "label c",
        ));

    session.inject_diagnostic_for_test(diag_a);
    session.inject_diagnostic_for_test(diag_b);
    session.inject_diagnostic_for_test(diag_c);

    let diags = session.get_diagnostics();

    // Find each injected diagnostic and verify its span against the reference.
    for (msg, start, end) in [
        ("stress-a", offset_a as usize, (offset_a + 5) as usize),
        ("stress-b", offset_b as usize, (offset_b + 6) as usize),
        ("stress-c", offset_c as usize, (offset_c + 9) as usize),
    ] {
        let d = diags
            .iter()
            .find(|d| d.message == msg)
            .unwrap_or_else(|| panic!("diagnostic '{}' not found", msg));

        let (exp_line, exp_col) = byte_offset_to_line_col(source, start);
        let (exp_end_line, exp_end_col) = byte_offset_to_line_col(source, end);

        assert_eq!(
            d.line, exp_line as u32,
            "{}: line mismatch (got {}, expected {})",
            msg, d.line, exp_line
        );
        assert_eq!(
            d.column, exp_col as u32,
            "{}: column mismatch (got {}, expected {})",
            msg, d.column, exp_col
        );
        assert_eq!(
            d.end_line, exp_end_line as u32,
            "{}: end_line mismatch (got {}, expected {})",
            msg, d.end_line, exp_end_line
        );
        assert_eq!(
            d.end_column, exp_end_col as u32,
            "{}: end_column mismatch (got {}, expected {})",
            msg, d.end_column, exp_end_col
        );
    }
}

/// The labelless (1,1,1,1) fallback is unaffected by the optimization.
/// Delegates to the existing test — this is just a marker asserting step-7
/// coverage of the labelless path specifically.
#[test]
fn get_diagnostics_labelless_fallback_unchanged_after_optimization() {
    use reify_types::Diagnostic;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    session
        .load_from_source(bracket_source(), "bracket")
        .expect("bracket source should compile cleanly");

    session.inject_diagnostic_for_test(Diagnostic::warning("no-label-stress"));

    let diags = session.get_diagnostics();
    let d = diags
        .iter()
        .find(|d| d.message == "no-label-stress")
        .expect("injected 'no-label-stress' not found");

    assert_eq!((d.line, d.column, d.end_line, d.end_column), (1, 1, 1, 1));
}

// --- Task 837 step-9: multibyte UTF-8 cross-validation tests ---

/// offset_to_line_col_fast must match byte_offset_to_line_col for every
/// char-boundary offset in a string containing 2-byte UTF-8 sequences.
///
/// "héllo\nwörld": 'é' (U+00E9) = 2 bytes; 'ö' (U+00F6) = 2 bytes.
/// The old byte-arithmetic implementation computes `offset - newline_pos` which
/// gives byte distance, not codepoint count.  The new implementation must
/// compute `source[line_start..offset].chars().count() + 1`.
///
/// Specific regression anchor:
///   byte offset 3 = the first 'l' after 'é'.
///   codepoint column = 3 (h=1, é=2, l=3) — NOT 4 (which byte distance gives).
#[test]
fn offset_to_line_col_fast_matches_original_multibyte_utf8() {
    use crate::engine::{build_line_offsets, byte_offset_to_line_col, offset_to_line_col_fast};
    let source = "héllo\nwörld";
    let line_offsets = build_line_offsets(source);
    // Iterate only char-boundary offsets.
    for (byte_idx, _ch) in source.char_indices() {
        let expected = byte_offset_to_line_col(source, byte_idx);
        let actual = offset_to_line_col_fast(source, &line_offsets, byte_idx);
        assert_eq!(
            actual, expected,
            "2-byte UTF-8: mismatch at byte offset {} (char '{}'): fast={:?} original={:?}",
            byte_idx, _ch, actual, expected
        );
    }
    // Also check the EOF position (one past last byte).
    let eof = source.len();
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, eof),
        byte_offset_to_line_col(source, eof),
        "2-byte UTF-8: mismatch at EOF offset {}",
        eof
    );
}

/// Targeted assertion: byte offset 3 in "héllo\nwörld" must give column 3
/// (codepoints h=1, é=2, l=3), NOT column 4 (byte distance from start).
#[test]
fn offset_to_line_col_fast_two_byte_char_column_is_codepoint_not_byte() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};
    let source = "héllo\nwörld";
    // 'é' occupies bytes 1..=2; the 'l' following it starts at byte 3.
    let line_offsets = build_line_offsets(source);
    // col should be 3 (h,é,l = 3 codepoints), not 4 (byte distance 3 → +1=4).
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 3),
        (1, 3),
        "byte 3 ('l' after 'é') should have codepoint column 3, not byte-based 4"
    );
    // 'r' on line 2: 'ö' at bytes 8..=9, so 'r' at byte 10.
    // Codepoints on line 2 before 'r': w=1, ö=2  → 'r' = col 3.
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 10),
        (2, 3),
        "byte 10 ('r' after 'ö') should have codepoint column 3, not byte-based 4"
    );
}

/// offset_to_line_col_fast matches byte_offset_to_line_col for every
/// char-boundary offset in a string containing 3-byte CJK UTF-8 sequences.
///
/// "ab\n你好world": '你' (U+4F60) = 3 bytes; '好' (U+597D) = 3 bytes.
/// 'w' is the 3rd codepoint on line 2 (you=1, hao=2, w=3).
/// Old byte arithmetic would give col = (9 - 2) = 7, which is wrong.
#[test]
fn offset_to_line_col_fast_matches_original_cjk_utf8() {
    use crate::engine::{build_line_offsets, byte_offset_to_line_col, offset_to_line_col_fast};
    let source = "ab\n\u{4F60}\u{597D}world";
    let line_offsets = build_line_offsets(source);
    for (byte_idx, _ch) in source.char_indices() {
        let expected = byte_offset_to_line_col(source, byte_idx);
        let actual = offset_to_line_col_fast(source, &line_offsets, byte_idx);
        assert_eq!(
            actual, expected,
            "CJK UTF-8: mismatch at byte offset {} (char '{}'): fast={:?} original={:?}",
            byte_idx, _ch, actual, expected
        );
    }
    // EOF check.
    let eof = source.len();
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, eof),
        byte_offset_to_line_col(source, eof),
        "CJK UTF-8: mismatch at EOF offset {}",
        eof
    );
    // Targeted: 'w' at byte 9 should be (2, 3), not byte-arithmetic (2, 7).
    assert_eq!(
        offset_to_line_col_fast(source, &line_offsets, 9),
        (2, 3),
        "byte 9 ('w' after two 3-byte CJK chars) should have codepoint column 3"
    );
}

/// offset_to_line_col_fast does not panic on non-char-boundary byte offsets;
/// it snaps backward to the nearest valid boundary instead.
#[test]
fn offset_to_line_col_fast_non_char_boundary_no_panic() {
    use crate::engine::{build_line_offsets, offset_to_line_col_fast};

    // "é" is 2 bytes (0xC3 0xA9), so byte 1 is mid-char.
    let source = "é";
    let line_offsets = build_line_offsets(source);
    // Byte 1 is not a char boundary — should not panic, should snap back to 0.
    let (line, col) = offset_to_line_col_fast(source, &line_offsets, 1);
    assert_eq!(line, 1);
    assert_eq!(col, 1, "non-boundary offset should snap back to start");

    // Multi-line with CJK: "日\nA" — '日' is 3 bytes; byte 2 is mid-char.
    let source2 = "日\nA";
    let offsets2 = build_line_offsets(source2);
    let (l, c) = offset_to_line_col_fast(source2, &offsets2, 2);
    assert_eq!(l, 1);
    assert_eq!(c, 1, "mid-CJK offset should snap back to start of char");
}
