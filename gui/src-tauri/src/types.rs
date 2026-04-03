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

/// A diagnostic (error/warning/info) from the compiler, in GUI-native form.
///
/// Follows the pattern of [`ValueData`] and [`ConstraintData`]: defined in types.rs,
/// produced by [`crate::engine::EngineSession`], mapped to MCP types in mcp_context.rs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticData {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
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

#[cfg(test)]
mod format_value_range_tests {
    use super::*;
    use reify_types::Value;

    #[test]
    fn both_bounds_exclusive() {
        let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), false, false);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(1..10)");
        assert_eq!(unit, "");
    }

    #[test]
    fn both_bounds_inclusive() {
        let range = Value::range(Some(Value::Int(1)), Some(Value::Int(10)), true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_lower_inclusive_via_factory() {
        // Factory normalizes inclusive=false for None bound
        let range = Value::range(None, Some(Value::Int(10)), true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_lower_inclusive_via_direct_struct() {
        // Bypass factory: directly construct with inclusive=true + None lower
        let range = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::Int(10))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..10]");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_upper_inclusive_via_factory() {
        // Factory normalizes inclusive=false for None bound
        let range = Value::range(Some(Value::Int(1)), None, true, true);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn none_upper_inclusive_via_direct_struct() {
        // Bypass factory: directly construct with inclusive=true + None upper
        let range = Value::Range {
            lower: Some(Box::new(Value::Int(1))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[1..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn both_bounds_none_inclusive_normalizes_to_parentheses() {
        // Both None + both inclusive=true: defensive re-normalization must fix both brackets
        let range = Value::Range {
            lower: None,
            upper: None,
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "(-\u{221e}..+\u{221e})");
        assert_eq!(unit, "");
    }

    #[test]
    fn mixed_inclusive_exclusive() {
        // Lower inclusive, upper exclusive: half-open interval [0..5)
        let range = Value::range(Some(Value::Int(0)), Some(Value::Int(5)), true, false);
        let (formatted, unit) = format_value(&range);
        assert_eq!(formatted, "[0..5)");
        assert_eq!(unit, "");
    }

    #[test]
    fn range_unit_always_empty_even_with_scalar_bounds() {
        // Range with Scalar bounds (LENGTH dimension): unit must still be empty
        // because Range display does not propagate unit info from its bounds.
        let lower = Value::Scalar {
            si_value: 0.001,
            dimension: reify_types::DimensionVector::LENGTH,
        };
        let upper = Value::Scalar {
            si_value: 0.01,
            dimension: reify_types::DimensionVector::LENGTH,
        };
        let range = Value::range(Some(lower), Some(upper), true, false);
        let (formatted, unit) = format_value(&range);
        // Scalars inside the range are formatted individually (SI→mm conversion),
        // but the range itself carries no unit.
        assert_eq!(formatted, "[1..10)");
        assert_eq!(unit, "", "Range unit string must always be empty");
    }
}
