mod claude_bridge_tests;
mod kernel_status_tests;
mod commands_tests;
mod diff_tests;
mod engine_tests;
mod lsp_bridge_tests;
mod mcp_context_tests;
mod mcp_dispatch_tests;
mod mechanism_descriptors_tests;
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

    // Verify full IPC contract (Serialize + DeserializeOwned + Clone + Debug + PartialEq)
    assert_ipc_contract::<GuiState>();
    assert_ipc_contract::<MeshData>();
    assert_ipc_contract::<ValueData>();
    assert_ipc_contract::<ConstraintData>();
    assert_ipc_contract::<SourceLocationInfo>();
    assert_ipc_contract::<FileData>();
    // DiagnosticInfo is the MCP canonical replacement for the removed GUI-local type
    assert_ipc_contract::<DiagnosticInfo>();

    // Verify AppState and EngineSession are usable as types
    let _ = std::any::type_name::<AppState>();
    let _ = std::any::type_name::<EngineSession>();
}
