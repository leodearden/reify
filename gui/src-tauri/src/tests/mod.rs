mod claude_bridge_tests;
mod commands_tests;
mod diff_tests;
mod engine_tests;
mod lsp_bridge_tests;
mod mcp_context_tests;
mod mcp_dispatch_tests;
mod types_tests;
mod watcher_tests;

/// Compile-time assertion that a type satisfies the full GUI IPC contract:
/// serializable, deserializable (owned), cloneable, debuggable, and comparable.
fn assert_ipc_contract<T: serde::Serialize + serde::de::DeserializeOwned + Clone + std::fmt::Debug + PartialEq>() {
}

// Step 11: Module structure verification — importing all public types.
#[test]
fn public_api_types_are_accessible() {
    use crate::commands::AppState;
    use crate::engine::EngineSession;
    use crate::types::{ConstraintData, FileData, GuiState, MeshData, ValueData};
    use reify_mcp::{DiagnosticInfo, SourceLocationInfo};

    // Verify types are Clone+Debug by using trait bounds
    fn assert_clone_debug<T: Clone + std::fmt::Debug>() {}
    assert_clone_debug::<GuiState>();
    assert_clone_debug::<MeshData>();
    assert_clone_debug::<ValueData>();
    assert_clone_debug::<ConstraintData>();
    assert_clone_debug::<SourceLocationInfo>();
    assert_clone_debug::<FileData>();
    // DiagnosticInfo is the MCP canonical replacement for the removed GUI-local type
    assert_clone_debug::<DiagnosticInfo>();

    // Verify full IPC contract (Serialize + DeserializeOwned + Clone + Debug + PartialEq)
    assert_ipc_contract::<GuiState>();
    assert_ipc_contract::<MeshData>();
    assert_ipc_contract::<ValueData>();
    assert_ipc_contract::<ConstraintData>();
    assert_ipc_contract::<SourceLocationInfo>();
    assert_ipc_contract::<FileData>();
    assert_ipc_contract::<DiagnosticInfo>();

    // Verify AppState and EngineSession are usable as types
    let _ = std::any::type_name::<AppState>();
    let _ = std::any::type_name::<EngineSession>();
}
