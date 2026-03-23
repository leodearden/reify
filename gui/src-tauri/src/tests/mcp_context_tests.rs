use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{bracket_source, MockGeometryKernel};

use crate::engine::EngineSession;
use crate::mcp_context::TauriToolContext;
use reify_mcp::ReifyToolContext;

fn make_loaded_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    session
}

fn make_tauri_context() -> TauriToolContext {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    TauriToolContext::new(engine)
}

// --- Read method tests ---

#[test]
fn get_source_returns_bracket_content() {
    let ctx = make_tauri_context();
    let source = ctx.get_source(None).expect("get_source should succeed");
    assert!(
        source.content.contains("structure Bracket"),
        "source should contain bracket structure"
    );
    assert_eq!(source.file_path, "bracket.ri");
}

#[test]
fn get_open_files_returns_bracket_file() {
    let ctx = make_tauri_context();
    let files = ctx.get_open_files().expect("get_open_files should succeed");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "bracket.ri");
    assert_eq!(files[0].language, "reify");
    assert_eq!(files[0].dirty, false);
}

#[test]
fn get_parameters_returns_bracket_params() {
    let ctx = make_tauri_context();
    let params = ctx.get_parameters().expect("get_parameters should succeed");
    assert!(!params.is_empty(), "should have parameters");

    // Check that expected params exist
    let width = params.iter().find(|p| p.name == "width");
    assert!(width.is_some(), "should have width parameter");
    let width = width.unwrap();
    assert_eq!(width.cell_id, "Bracket.width");
    assert_eq!(width.value, "80");
    assert_eq!(width.unit, "mm");
    assert_eq!(width.kind, "Param");
    assert_eq!(width.determinacy, "determined");

    let height = params.iter().find(|p| p.name == "height");
    assert!(height.is_some(), "should have height parameter");
    let height = height.unwrap();
    assert_eq!(height.cell_id, "Bracket.height");
    assert_eq!(height.value, "100");
    assert_eq!(height.unit, "mm");

    let thickness = params.iter().find(|p| p.name == "thickness");
    assert!(thickness.is_some(), "should have thickness parameter");
    let thickness = thickness.unwrap();
    assert_eq!(thickness.cell_id, "Bracket.thickness");
    assert_eq!(thickness.value, "5");
    assert_eq!(thickness.unit, "mm");
}

#[test]
fn get_constraints_returns_satisfied() {
    let ctx = make_tauri_context();
    let constraints = ctx
        .get_constraints()
        .expect("get_constraints should succeed");
    assert!(!constraints.is_empty(), "should have constraints");

    // All bracket constraints should be satisfied at default values
    for c in &constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied",
            c.node_id
        );
    }
}

#[test]
fn get_eval_status_returns_idle() {
    let ctx = make_tauri_context();
    let status = ctx
        .get_eval_status()
        .expect("get_eval_status should succeed");
    assert_eq!(status.phase, "idle");
    assert_eq!(status.dirty_count, 0);
}

#[test]
fn get_selection_returns_empty() {
    let ctx = make_tauri_context();
    let selection = ctx.get_selection().expect("get_selection should succeed");
    assert!(selection.selected_entity.is_none());
    assert!(selection.hovered_entity.is_none());
}

#[test]
fn get_source_location_for_width() {
    let ctx = make_tauri_context();
    let loc = ctx
        .get_source_location("Bracket.width")
        .expect("get_source_location should succeed for Bracket.width");
    assert_eq!(loc.file, "bracket.ri");
    assert!(loc.line >= 1, "line should be positive");
}

#[test]
fn get_source_location_nonexistent_returns_error() {
    let ctx = make_tauri_context();
    let result = ctx.get_source_location("Nonexistent.param");
    assert!(result.is_err(), "should return error for nonexistent entity");
}

#[test]
fn get_diagnostics_returns_empty() {
    let ctx = make_tauri_context();
    let diags = ctx
        .get_diagnostics()
        .expect("get_diagnostics should succeed");
    assert!(diags.is_empty(), "diagnostics should be empty initially");
}
