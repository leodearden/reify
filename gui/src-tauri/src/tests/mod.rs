mod claude_bridge_tests;
mod commands_tests;
mod diff_tests;
mod engine_tests;
mod lsp_bridge_tests;
mod mcp_context_tests;
mod mcp_dispatch_tests;
mod types_tests;
mod watcher_tests;

// Step 11: Module structure verification — importing all public types.
#[test]
fn public_api_types_are_accessible() {
    use crate::commands::AppState;
    use crate::engine::EngineSession;
    use crate::types::{
        ConstraintData, DiagnosticData, FileData, GuiState, MeshData, SourceLocation, ValueData,
    };

    // Verify types are Clone+Debug by using trait bounds
    fn assert_clone_debug<T: Clone + std::fmt::Debug>() {}
    assert_clone_debug::<GuiState>();
    assert_clone_debug::<MeshData>();
    assert_clone_debug::<ValueData>();
    assert_clone_debug::<ConstraintData>();
    assert_clone_debug::<SourceLocation>();
    assert_clone_debug::<FileData>();
    assert_clone_debug::<DiagnosticData>();

    // Verify AppState and EngineSession are usable as types
    let _ = std::any::type_name::<AppState>();
    let _ = std::any::type_name::<EngineSession>();
}
