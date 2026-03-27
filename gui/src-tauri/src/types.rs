// IPC types for GUI ↔ Engine communication

use serde::{Deserialize, Serialize};

use reify_types::{DeterminacyState, Value};

/// Full GUI state snapshot sent to the frontend after each operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuiState {
    pub meshes: Vec<MeshData>,
    pub values: Vec<ValueData>,
    pub constraints: Vec<ConstraintData>,
    pub files: Vec<FileData>,
}

/// Tessellated mesh for 3D display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub entity_path: String,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<f32>>,
}

/// A value cell (param, let, or auto) for the property editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueData {
    pub cell_id: String,
    pub name: String,
    pub value: String,
    pub unit: String,
    pub determinacy: String,
    pub entity_path: String,
    pub kind: String,
}

/// A constraint with its check status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintData {
    pub node_id: String,
    pub expression: String,
    pub status: String,
    pub label: Option<String>,
    pub parameter_ids: Vec<String>,
}

/// Source location reference (for click-to-source navigation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// A source file in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileData {
    pub path: String,
    pub content: String,
}

/// Current phase of the evaluation engine (mirrors frontend EvaluationStatus interface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationStatus {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

/// Format a Value for GUI display, returning (formatted_value, unit_string).
///
/// Delegates to [`Value::format_display_pair()`] — the canonical implementation
/// lives on Value itself so that adding a new variant only requires editing value.rs.
pub fn format_value(v: &Value) -> (String, String) {
    v.format_display_pair()
}

/// Format a DeterminacyState as a string.
pub fn format_determinacy(d: DeterminacyState) -> String {
    match d {
        DeterminacyState::Determined => "determined".to_string(),
        DeterminacyState::Undetermined => "undetermined".to_string(),
        DeterminacyState::Provisional => "provisional".to_string(),
        DeterminacyState::Auto => "auto".to_string(),
    }
}

