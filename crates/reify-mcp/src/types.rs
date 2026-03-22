// MCP response types and core protocol types

use serde::{Deserialize, Serialize};

/// Error types returned by MCP tool handlers.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("tool not implemented")]
    NotImplemented,

    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("internal error: {0}")]
    InternalError(String),

    #[error("engine error: {0}")]
    EngineError(String),
}

/// Metadata about a registered tool, returned by tools/list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

// --- MCP response types (JSON-oriented, decoupled from GUI types) ---

/// Information about an open file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFileInfo {
    pub path: String,
    pub language: String,
}

/// A diagnostic (error/warning) from the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
}

/// A parameter (value cell) in the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub cell_id: String,
    pub name: String,
    pub value: String,
    pub unit: String,
    pub kind: String,
    pub entity_path: String,
    pub determinacy: String,
}

/// A constraint in the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintInfo {
    pub node_id: String,
    pub expression: String,
    pub status: String,
    pub label: Option<String>,
    pub parameter_ids: Vec<String>,
}

/// Current evaluation engine status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalStatusInfo {
    pub phase: String,
    pub progress: Option<f64>,
    pub dirty_count: u32,
}

/// Current selection in the viewport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionInfo {
    pub entity_path: Option<String>,
    pub cell_ids: Vec<String>,
}

/// A source location reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocationInfo {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// Result of an update_source operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub success: bool,
    pub diagnostics_count: u32,
}

/// Result of a set_parameter operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetParamResult {
    pub success: bool,
    pub new_value: String,
    pub unit: String,
}
