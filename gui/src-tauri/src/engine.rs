// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_eval::{CheckResult, Engine};
use reify_types::{
    ConstraintChecker, DeterminacyState, DimensionVector, ExportFormat, GeometryKernel, ModulePath,
    Satisfaction, Severity, Value, ValueCellId,
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

    /// Load a .ri file from disk.
    pub fn load_file(&mut self, path: &Path) -> Result<GuiState, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("Error reading {}: {}", path.display(), e))?;

        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        self.file_path = Some(path.to_path_buf());
        self.load_from_source(&source, module_name)
    }

    /// Update source code and re-evaluate from scratch.
    ///
    /// Source changes can alter topology, so we create a fresh parse/compile/eval cycle.
    /// The existing engine state (snapshot, caches) is reused where possible via check().
    pub fn update_source(&mut self, path: &str, content: &str) -> Result<GuiState, String> {
        let module_name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        // Store updated source
        self.source_map.insert(path.to_string(), content.to_string());

        // Re-parse and re-compile from scratch (topology may have changed)
        let parsed = reify_syntax::parse(content, ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        let compiled = reify_compiler::compile(&parsed);

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

        let check_result = self.engine.check(&compiled);

        self.compiled = Some(compiled);
        self.last_check = Some(check_result);

        self.build_gui_state()
    }

    /// Export geometry to a file.
    pub fn export(&mut self, format: ExportFormat, path: &Path) -> Result<(), String> {
        let compiled = self
            .compiled
            .as_ref()
            .ok_or_else(|| "No module loaded".to_string())?
            .clone();

        let result = self.engine.build(&compiled, format);

        for diag in &result.diagnostics {
            if diag.severity == Severity::Error {
                return Err(format!("Build error: {}", diag.message));
            }
        }

        match result.geometry_output {
            Some(data) => {
                std::fs::write(path, &data)
                    .map_err(|e| format!("Error writing {}: {}", path.display(), e))?;
                Ok(())
            }
            None => Err("No geometry output produced".to_string()),
        }
    }

    /// Look up source location for an entity path (e.g., "Bracket.width").
    pub fn get_source_location(&self, entity_path: &str) -> Option<crate::types::SourceLocation> {
        let compiled = self.compiled.as_ref()?;
        let cell_id = parse_cell_id(entity_path).ok()?;

        // Find the span for this cell
        let span = compiled.templates.iter().find_map(|t| {
            t.value_cells
                .iter()
                .find(|vc| vc.id == cell_id)
                .map(|vc| vc.span)
        })?;

        // Convert byte offset to line/column using stored source
        // Find the source file that contains this span
        let (file, source) = self.source_map.iter().next()?;

        let (line, col) = byte_offset_to_line_col(source, span.start as usize);
        let (end_line, end_col) = byte_offset_to_line_col(source, span.end as usize);

        Some(crate::types::SourceLocation {
            file: file.clone(),
            line: line as u32,
            column: col as u32,
            end_line: end_line as u32,
            end_column: end_col as u32,
        })
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

/// Convert a byte offset in source text to (line, column), both 1-based.
fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
