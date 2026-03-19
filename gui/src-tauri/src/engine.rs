// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::HashMap;
use std::path::PathBuf;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_eval::{CheckResult, Engine};
use reify_types::{
    ConstraintChecker, DeterminacyState, DimensionVector, GeometryKernel, ModulePath, Satisfaction,
    Severity, Value, ValueCellId,
};

use crate::types::{
    format_determinacy, format_value, ConstraintData, FileData, GuiState, MeshData, ValueData,
};

/// Session wrapping an Engine with its compiled module and source text.
///
/// Provides higher-level operations for the GUI: load, update, set parameter, export.
pub struct EngineSession {
    engine: Engine,
    compiled: Option<CompiledModule>,
    source_map: HashMap<String, String>,
    file_path: Option<PathBuf>,
    last_check: Option<CheckResult>,
}

impl EngineSession {
    /// Create a new EngineSession with the given constraint checker and optional geometry kernel.
    pub fn new(
        checker: Box<dyn ConstraintChecker>,
        kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self {
            engine: Engine::new(checker, kernel),
            compiled: None,
            source_map: HashMap::new(),
            file_path: None,
            last_check: None,
        }
    }

    /// Load source code, parse, compile, evaluate, and return full GUI state.
    pub fn load_from_source(&mut self, source: &str, module_name: &str) -> Result<GuiState, String> {
        // Parse
        let parsed = reify_syntax::parse(source, ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        // Compile
        let compiled = reify_compiler::compile(&parsed);

        // Check for compile errors
        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
        if has_errors {
            let msgs: Vec<String> = compiled
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect();
            return Err(format!("Compile errors: {}", msgs.join("; ")));
        }

        // Store source
        self.source_map
            .insert(format!("{}.ri", module_name), source.to_string());

        // Evaluate + check constraints
        let check_result = self.engine.check(&compiled);

        self.compiled = Some(compiled);
        self.last_check = Some(check_result);

        self.build_gui_state()
    }

    /// Set a parameter value by cell ID string and value string.
    ///
    /// `cell_id_str` is "Entity.member" (e.g., "Bracket.width").
    /// `value_str` is a quantity literal (e.g., "120mm"), plain number, or boolean.
    pub fn set_parameter(
        &mut self,
        cell_id_str: &str,
        value_str: &str,
    ) -> Result<GuiState, String> {
        let cell_id = parse_cell_id(cell_id_str)?;
        let value = parse_value_string(value_str)?;

        // Validate cell exists in compiled module
        let compiled = self
            .compiled
            .as_ref()
            .ok_or_else(|| "No module loaded".to_string())?;
        let cell_exists = compiled
            .templates
            .iter()
            .any(|t| t.value_cells.iter().any(|vc| vc.id == cell_id));
        if !cell_exists {
            return Err(format!("Unknown parameter '{}'", cell_id_str));
        }

        let check_result = self
            .engine
            .edit_check(cell_id, value)
            .map_err(|e| format!("Engine error: {}", e))?;

        self.last_check = Some(check_result);
        self.build_gui_state()
    }

    /// Build the full GUI state from the current engine state.
    pub fn build_gui_state(&self) -> Result<GuiState, String> {
        let compiled = self
            .compiled
            .as_ref()
            .ok_or_else(|| "No module loaded".to_string())?;

        let check = self
            .last_check
            .as_ref()
            .ok_or_else(|| "No check result available".to_string())?;

        // Build values
        let mut values = Vec::new();
        for template in &compiled.templates {
            for cell in &template.value_cells {
                let val = check.values.get_or_undef(&cell.id);
                let (formatted_value, unit) = format_value(&val);

                let determinacy = match &val {
                    reify_types::Value::Undef => {
                        if cell.kind == ValueCellKind::Auto {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        }
                    }
                    _ => DeterminacyState::Determined,
                };

                let kind = match cell.kind {
                    ValueCellKind::Param => "Param",
                    ValueCellKind::Let => "Let",
                    ValueCellKind::Auto => "Auto",
                };

                values.push(ValueData {
                    cell_id: cell.id.to_string(),
                    name: cell.id.member.clone(),
                    value: formatted_value,
                    unit,
                    determinacy: format_determinacy(determinacy),
                    entity_path: cell.id.entity.clone(),
                    kind: kind.to_string(),
                });
            }
        }

        // Build constraints
        let mut constraints = Vec::new();
        for entry in &check.constraint_results {
            let status = match entry.satisfaction {
                Satisfaction::Satisfied => "Satisfied",
                Satisfaction::Violated => "Violated",
                Satisfaction::Indeterminate => "Indeterminate",
            };

            constraints.push(ConstraintData {
                node_id: entry.id.to_string(),
                expression: String::new(), // TODO: reconstruct from compiled expr
                status: status.to_string(),
                label: entry.label.clone(),
                parameter_ids: vec![],
            });
        }

        // Build meshes (from tessellation of realizations)
        let meshes = Vec::new();
        // TODO: tessellate realizations when geometry kernel is available

        // Build files
        let files: Vec<FileData> = self
            .source_map
            .iter()
            .map(|(path, content)| FileData {
                path: path.clone(),
                content: content.clone(),
            })
            .collect();

        Ok(GuiState {
            meshes,
            values,
            constraints,
            files,
        })
    }
}

/// Parse a "Entity.member" string into a ValueCellId.
fn parse_cell_id(s: &str) -> Result<ValueCellId, String> {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid cell ID '{}': expected 'Entity.member' format",
            s
        ));
    }
    Ok(ValueCellId::new(parts[0], parts[1]))
}

/// Parse a value string into a Value.
///
/// Supported formats:
/// - Quantity literals: "80mm", "100cm", "1.5m", "90deg", "1.57rad"
/// - Plain numbers: "5.0" → Real, "5" → Int
/// - Booleans: "true", "false"
pub fn parse_value_string(s: &str) -> Result<Value, String> {
    let s = s.trim();

    // Booleans
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }

    // Try quantity literals (number + unit suffix)
    let unit_table: &[(&str, f64, DimensionVector)] = &[
        ("mm", 0.001, DimensionVector::LENGTH),
        ("cm", 0.01, DimensionVector::LENGTH),
        ("m", 1.0, DimensionVector::LENGTH),
        ("deg", std::f64::consts::PI / 180.0, DimensionVector::ANGLE),
        ("rad", 1.0, DimensionVector::ANGLE),
    ];

    // Try units from longest suffix to shortest to avoid "m" matching before "mm"/"cm"
    for &(unit, scale, dimension) in unit_table {
        if let Some(num_str) = s.strip_suffix(unit) {
            let num_str = num_str.trim();
            if let Ok(v) = num_str.parse::<f64>() {
                return Ok(Value::Scalar {
                    si_value: v * scale,
                    dimension,
                });
            }
        }
    }

    // Plain integer
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Value::Int(i));
    }

    // Plain float
    if let Ok(f) = s.parse::<f64>() {
        return Ok(Value::Real(f));
    }

    Err(format!("Cannot parse value '{}'", s))
}
