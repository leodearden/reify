use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{bracket_source, MockGeometryKernel};

use crate::commands::AppState;
use crate::engine::EngineSession;

fn make_loaded_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    session
}

#[test]
fn app_state_constructible() {
    let session = make_loaded_session();
    let _state = AppState {
        engine: Arc::new(Mutex::new(session)),
    };
}

#[test]
fn save_and_open_file_roundtrip() {
    use crate::commands::{open_file_impl, save_file_impl};

    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));

    let dir = std::env::temp_dir().join("reify-gui-test");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("test_roundtrip.ri");

    // Save
    save_file_impl(path.to_str().unwrap(), bracket_source()).expect("save should succeed");

    // Open
    let file_data = open_file_impl(path.to_str().unwrap()).expect("open should succeed");
    assert_eq!(file_data.path, path.to_str().unwrap());
    assert!(file_data.content.contains("structure Bracket"));

    // Cleanup
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn constraint_violation_set_thickness_1mm() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));

    let state = {
        let mut session = engine.lock().unwrap();
        session
            .set_parameter("Bracket.thickness", "1mm")
            .expect("set thickness should succeed")
    };

    // thickness=1mm violates "thickness > 2mm"
    let thickness_gt_constraint = state
        .constraints
        .iter()
        .find(|c| c.status == "Violated");

    assert!(
        thickness_gt_constraint.is_some(),
        "should have at least one violated constraint when thickness=1mm"
    );
}

#[test]
fn get_source_location_for_width() {
    let session = make_loaded_session();
    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find width source location");

    assert_eq!(loc.file, "bracket.ri");
    assert!(loc.line >= 1, "line should be positive");
    assert!(loc.column >= 1, "column should be positive");
}

#[test]
fn export_writes_file() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = std::env::temp_dir().join("reify-gui-test-export");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("test.step");

    let result = session.export(reify_types::ExportFormat::Step, &path);
    // MockGeometryKernel writes MOCK_EXPORT_DATA
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("should read exported file");
    assert!(data.len() > 0, "exported file should not be empty");

    // Cleanup
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}
