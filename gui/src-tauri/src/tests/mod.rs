mod claude_bridge_tests;
mod commands_tests;
mod diff_tests;
mod engine_lock_tests;
mod engine_tests;
mod event_bus_tests;
mod kernel_status_tests;
mod lsp_bridge_tests;
mod mcp_context_tests;
mod mcp_dispatch_tests;
mod types_tests;
mod watcher_tests;

use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::engine::EngineSession;

/// Shared engine fixture for tests across this crate's test modules.
///
/// Builds a real [`EngineSession`] backed by a [`MockGeometryKernel`] with
/// a known-good source file pre-loaded, wrapped in an `Arc<Mutex<…>>` ready
/// for use with [`crate::engine_lock::with_engine_lock`] and related helpers.
pub(crate) fn make_test_engine() -> Arc<Mutex<EngineSession>> {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");
    Arc::new(Mutex::new(session))
}

/// Compile-time assertion that a type satisfies the full GUI IPC contract:
/// serializable, deserializable (owned), cloneable, debuggable, and comparable.
fn assert_ipc_contract<
    T: serde::Serialize + serde::de::DeserializeOwned + Clone + std::fmt::Debug + PartialEq,
>() {
}

// Step 11: Module structure verification — importing all public types.
#[test]
fn public_api_types_are_accessible() {
    use crate::commands::AppState;
    use crate::engine::EngineSession;
    use crate::types::{
        ConstraintData, FileData, GuiState, JointBinding, JointDescriptor, MechanismDescriptor,
        MeshData, ValueData,
    };
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
    // Mechanism descriptor types introduced in task 2536
    assert_ipc_contract::<MechanismDescriptor>();
    assert_ipc_contract::<JointDescriptor>();
    // JointBinding enum introduced in task 3783
    assert_ipc_contract::<JointBinding>();

    // Verify AppState and EngineSession are usable as types
    let _ = std::any::type_name::<AppState>();
    let _ = std::any::type_name::<EngineSession>();
}
