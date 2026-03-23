//! MCP (Model Context Protocol) server for Reify.
//!
//! Provides a JSON-RPC 2.0 based MCP server with 16 tools for reading model state,
//! writing source/parameters, navigation, and language reference. Supports dual-mode
//! transport: in-process (via `McpServer::handle_message`) and stdio stream
//! (via `McpServer::run_stdio`).

pub mod context;
pub mod jsonrpc;
pub mod registry;
pub mod tools;
pub mod transport;
pub mod types;

// Re-export key types for convenient access
pub use context::ReifyToolContext;
pub use registry::ToolRegistry;
pub use tools::register_all_tools;
pub use transport::McpServer;
pub use types::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, SelectionInfo,
    SetParamResult, SourceContent, SourceLocationInfo, ToolError, ToolInfo, UpdateResult,
};

#[cfg(any(test, feature = "test-support"))]
pub use context::MockToolContext;
