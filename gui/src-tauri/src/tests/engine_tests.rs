use std::path::Path;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{bracket_source, bracket_source_with_width, MockGeometryKernel};
use reify_types::ExportFormat;

use crate::engine::EngineSession;

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

    assert_eq!(
        state.constraints.len(),
        3,
        "bracket has 3 constraints"
    );
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
        assert_eq!(c.status, "Satisfied", "constraint {} should be satisfied", c.node_id);
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

    let state = session
        .load_file(&path)
        .expect("load_file should succeed");

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

    let violated = state
        .constraints
        .iter()
        .any(|c| c.status == "Violated");
    assert!(violated, "should have at least one violated constraint when thickness=1mm");

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

    assert_eq!(loc.file, "bracket.ri");
    // width is on line 2 of bracket_source() (line 1 = "structure Bracket {")
    assert!(loc.line >= 2, "width should be on line 2 or later, got {}", loc.line);
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

    let dir = std::env::temp_dir().join("reify-gui-test-export-e2e");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("bracket.step");

    let result = session.export(ExportFormat::Step, &path);
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("exported file should be readable");
    assert!(!data.is_empty(), "exported file should not be empty");

    // Cleanup
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}
