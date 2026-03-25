// IPC types for GUI ↔ Engine communication

use serde::{Deserialize, Serialize};

use reify_types::{DeterminacyState, DimensionVector, Value};

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
/// Examples:
/// - `Scalar{0.08, LENGTH}` → `("80", "mm")`
/// - `Scalar{1.5708, ANGLE}` → `("90", "deg")`
/// - `Int(5)` → `("5", "")`
/// - `Real(3.14)` → `("3.14", "")`
/// - `Bool(true)` → `("true", "")`
/// - `Undef` → `("undefined", "")`
pub fn format_value(v: &Value) -> (String, String) {
    match v {
        Value::Scalar { si_value, dimension } => {
            let (display_value, unit) = convert_si_to_display(*si_value, *dimension);
            // Format nicely: avoid trailing zeros for whole numbers
            let formatted = format_number(display_value);
            (formatted, unit.to_string())
        }
        Value::Int(i) => (i.to_string(), String::new()),
        Value::Real(r) => (format_number(*r), String::new()),
        Value::Bool(b) => (b.to_string(), String::new()),
        Value::String(s) => (s.clone(), String::new()),
        Value::Enum { variant, .. } => (variant.clone(), String::new()),
        Value::List(items) => {
            let strs: Vec<String> = items.iter().map(|v| format_value(v).0).collect();
            (format!("[{}]", strs.join(", ")), String::new())
        }
        Value::Set(items) => {
            let strs: Vec<String> = items.iter().map(|v| format_value(v).0).collect();
            (format!("set{{{}}}", strs.join(", ")), String::new())
        }
        Value::Map(entries) => {
            let strs: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{} => {}", format_value(k).0, format_value(v).0))
                .collect();
            (format!("map{{{}}}", strs.join(", ")), String::new())
        }
        Value::Option(opt) => match opt {
            Some(inner) => format_value(inner),
            None => ("none".to_string(), String::new()),
        },
        Value::Lambda { .. } => ("<lambda>".to_string(), String::new()),
        Value::Field { domain_type, codomain_type, source, .. } => {
            (format!("Field<{}, {}>({:?})", domain_type, codomain_type, source), String::new())
        }
        Value::Tensor(items) => {
            let strs: Vec<String> = items.iter().map(|v| format_value(v).0).collect();
            (format!("[{}]", strs.join(", ")), String::new())
        }
        Value::Complex { re, im, dimension } => {
            let (display_re, unit) = convert_si_to_display(*re, *dimension);
            let (display_im, _) = convert_si_to_display(*im, *dimension);
            let formatted = format!("{} + {}i", format_number(display_re), format_number(display_im));
            (formatted, unit.to_string())
        }
        Value::Undef => ("undefined".to_string(), String::new()),
    }
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

/// Convert an SI value to a human-readable display value with unit string.
fn convert_si_to_display(si_value: f64, dimension: DimensionVector) -> (f64, &'static str) {
    if dimension == DimensionVector::LENGTH {
        // SI meters → millimeters
        (si_value * 1000.0, "mm")
    } else if dimension == DimensionVector::ANGLE {
        // SI radians → degrees
        (si_value * 180.0 / std::f64::consts::PI, "deg")
    } else if dimension == DimensionVector::AREA {
        // SI m² → mm²
        (si_value * 1e6, "mm²")
    } else if dimension == DimensionVector::VOLUME {
        // SI m³ → mm³
        (si_value * 1e9, "mm³")
    } else if dimension.is_dimensionless() {
        (si_value, "")
    } else {
        // Unknown dimension — show raw SI value
        (si_value, "SI")
    }
}

/// Format a floating-point number nicely (no trailing zeros for whole numbers).
fn format_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}
