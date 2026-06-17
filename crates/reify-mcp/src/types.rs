// MCP response types and core protocol types

use serde::{Deserialize, Serialize};

// Re-export presentation types that live in reify-types so the engine layer
// can import them without depending on the MCP adapter crate.
pub use reify_core::{DiagnosticInfo, SourceLocationInfo};

/// Error types returned by MCP tool handlers.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

// --- MCP response types (JSON-oriented, decoupled from GUI types) ---

/// Source content returned by get_source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceContent {
    pub content: String,
    pub file_path: String,
}

/// Information about an open file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenFileInfo {
    pub path: String,
    pub language: String,
    pub dirty: bool,
}

/// A parameter (value cell) in the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintInfo {
    pub node_id: String,
    pub expression: String,
    pub status: String,
    pub label: Option<String>,
    pub parameter_ids: Vec<String>,
}

/// Current evaluation engine status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalStatusInfo {
    pub phase: String,
    pub progress: Option<f64>,
    pub dirty_count: u32,
}

/// Current selection in the viewport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SelectionInfo {
    pub selected_entity: Option<String>,
    /// Full multi-selection list; empty when nothing is selected.
    /// The `#[serde(default)]` ensures backward-compat deserialization
    /// from clients that send the old shape without this field.
    #[serde(default)]
    pub selected_entities: Vec<String>,
    pub hovered_entity: Option<String>,
}

/// Result of an update_source operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateResult {
    pub success: bool,
    pub diagnostics_count: u32,
}

/// Result of a set_parameter operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetParamResult {
    pub success: bool,
    pub new_value: String,
    pub unit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ParameterInfo::reason (undef-cause surface, §4.4 ε) ---

    #[test]
    fn parameter_info_reason_some_serializes_as_string() {
        let p = ParameterInfo {
            cell_id: "c1".to_string(),
            name: "outer_d".to_string(),
            value: "undef".to_string(),
            unit: "mm".to_string(),
            kind: "Param".to_string(),
            entity_path: "box/outer_d".to_string(),
            determinacy: "undetermined".to_string(),
            reason: Some("outer_d unbound".to_string()),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["reason"], "outer_d unbound");
    }

    #[test]
    fn parameter_info_reason_none_serializes_as_null() {
        let p = ParameterInfo {
            cell_id: "c1".to_string(),
            name: "width".to_string(),
            value: "10".to_string(),
            unit: "mm".to_string(),
            kind: "Param".to_string(),
            entity_path: "box/width".to_string(),
            determinacy: "determined".to_string(),
            reason: None,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert!(v["reason"].is_null());
    }

    #[test]
    fn parameter_info_reason_backward_compat_no_key_deserializes_to_none() {
        // Older payload without "reason" key must deserialize cleanly (serde(default)).
        let json = serde_json::json!({
            "cell_id": "c1",
            "name": "width",
            "value": "10",
            "unit": "mm",
            "kind": "Param",
            "entity_path": "box/width",
            "determinacy": "determined"
        });
        let p: ParameterInfo = serde_json::from_value(json).unwrap();
        assert_eq!(p.reason, None);
    }

    #[test]
    fn selection_info_default_has_none_fields() {
        let info = SelectionInfo::default();
        assert_eq!(info.selected_entity, None);
        assert_eq!(info.hovered_entity, None);
    }

    #[test]
    fn source_location_info_serializes_with_file_path_key() {
        let loc = SourceLocationInfo {
            file_path: "bracket.ri".to_string(),
            line: 3,
            column: 4,
            end_line: 3,
            end_column: 30,
        };
        let v = serde_json::to_value(&loc).unwrap();
        assert_eq!(v["file_path"], "bracket.ri");
        assert!(v.get("file").is_none(), "should not serialize as 'file'");
    }
}
