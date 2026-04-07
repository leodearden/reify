// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_eval::{CheckResult, Engine};
use reify_types::{
    ConstraintChecker, DeterminacyState, DimensionVector, ExportFormat, GeometryKernel, ModulePath,
    Satisfaction, Severity, Value, ValueCellId,
};

use reify_mcp::{DiagnosticInfo, SourceLocationInfo};

use crate::types::{
    ConstraintData, FileData, GuiState, MeshData, ValueData, format_determinacy, format_value,
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
    module_name: Option<String>,
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
            module_name: None,
        }
    }

    /// Load source code, parse, compile, evaluate, and return full GUI state.
    pub fn load_from_source(
        &mut self,
        source: &str,
        module_name: &str,
    ) -> Result<GuiState, String> {
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

        // Store source with normalized key; clear stale entries
        self.source_map.clear();
        self.source_map
            .insert(format!("{}.ri", module_name), source.to_string());
        self.module_name = Some(module_name.to_string());

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
    ///
    /// On error (parse or compile failure), the session state is left completely unchanged —
    /// source_map, module_name, compiled, and last_check all retain their previous values.
    pub fn update_source(&mut self, path: &str, content: &str) -> Result<GuiState, String> {
        let module_name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        // Re-parse and re-compile from scratch (topology may have changed)
        // All state mutation is deferred until after successful parse+compile
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

        // Parse+compile succeeded — now atomically update all state
        let normalized_key = format!("{}.ri", module_name);
        self.source_map.clear();
        self.source_map.insert(normalized_key, content.to_string());
        self.module_name = Some(module_name.to_string());

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
            .ok_or_else(|| "No module loaded".to_string())?;

        let result = self.engine.build(compiled, format);

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

    /// Resolve the canonical source key and text for the currently loaded module.
    ///
    /// Returns `Some((key, source_text))` where `key` is `"{module_name}.ri"` and
    /// `source_text` is the stored source for that key.  Returns `None` when the
    /// key is not present in `source_map` (should not happen in practice once a
    /// module is loaded, but guards against any inconsistency).
    ///
    /// # Caller precondition
    ///
    /// Callers **must** verify `self.compiled.is_some()` before calling this
    /// method.  `load_from_source` and `update_source` always set `module_name`
    /// atomically with `compiled` (they fail before mutating state on parse/compile
    /// errors), so `compiled.is_some()` implies `module_name.is_some()`.  The
    /// `None` arm of the `match` below is therefore unreachable under that
    /// precondition.
    fn resolve_source(&self) -> Option<(String, &str)> {
        match self.module_name {
            Some(ref name) => {
                let key = format!("{}.ri", name);
                let src = self.source_map.get(&key)?;
                Some((key, src.as_str()))
            }
            None => {
                // compiled.is_some() implies module_name.is_some() — this branch
                // is dead whenever callers gate on compiled being present.
                unreachable!(
                    "resolve_source called with module_name = None; \
                     callers must verify compiled.is_some() first"
                )
            }
        }
    }

    /// Look up source location for an entity path (e.g., "Bracket.width").
    pub fn get_source_location(&self, entity_path: &str) -> Option<SourceLocationInfo> {
        let compiled = self.compiled.as_ref()?;
        let cell_id = parse_cell_id(entity_path).ok()?;

        // Find the span for this cell
        let span = compiled.templates.iter().find_map(|t| {
            t.value_cells
                .iter()
                .find(|vc| vc.id == cell_id)
                .map(|vc| vc.span)
        })?;

        // Resolve the source file key and text via the shared helper.
        let (file, source) = self.resolve_source()?;

        let (line, col) = byte_offset_to_line_col(source, span.start as usize);
        let (end_line, end_col) = byte_offset_to_line_col(source, span.end as usize);

        Some(SourceLocationInfo {
            file_path: file.clone(),
            line: line as u32,
            column: col as u32,
            end_line: end_line as u32,
            end_column: end_col as u32,
        })
    }

    /// Return diagnostics (warnings, info) from the most recently compiled module.
    ///
    /// If no module is loaded, returns an empty vec. Because
    /// [`load_from_source`] and [`update_source`] return `Err` before storing
    /// a module that has compile errors, only warnings and info-level
    /// diagnostics survive here — compile errors are surfaced as `Err` results
    /// from those methods.
    ///
    /// Delegates source key resolution to [`resolve_source`].
    pub fn get_diagnostics(&self) -> Vec<DiagnosticInfo> {
        let compiled = match self.compiled.as_ref() {
            Some(c) => c,
            None => return Vec::new(),
        };

        // Resolve file_path and source text via the shared helper.
        let (file_path, source) = match self.resolve_source() {
            Some(pair) => pair,
            None => return Vec::new(),
        };

        compiled
            .diagnostics
            .iter()
            .map(|diag| {
                // Use the first label's span if available; otherwise default to (1,1,1,1).
                let (line, column, end_line, end_column) = if let Some(label) = diag.labels.first()
                {
                    let (l, c) = byte_offset_to_line_col(source, label.span.start as usize);
                    let (el, ec) = byte_offset_to_line_col(source, label.span.end as usize);
                    (l as u32, c as u32, el as u32, ec as u32)
                } else {
                    (1, 1, 1, 1)
                };

                DiagnosticInfo {
                    file_path: file_path.clone(),
                    line,
                    column,
                    end_line,
                    end_column,
                    severity: format!("{}", diag.severity),
                    message: diag.message.clone(),
                    code: None,
                }
            })
            .collect()
    }

    /// Build the full GUI state from the current engine state.
    pub fn build_gui_state(&mut self) -> Result<GuiState, String> {
        let (compiled, check) = match (self.compiled.as_ref(), self.last_check.as_ref()) {
            (Some(c), Some(k)) => (c, k),
            _ => {
                return Ok(GuiState {
                    meshes: Vec::new(),
                    values: Vec::new(),
                    constraints: Vec::new(),
                    files: Vec::new(),
                });
            }
        };

        // Build values
        let mut values = Vec::new();
        for template in &compiled.templates {
            for cell in &template.value_cells {
                let val = check.values.get_or_undef(&cell.id);
                let (formatted_value, unit) = format_value(&val);

                let determinacy = match &val {
                    reify_types::Value::Undef => {
                        if cell.kind.is_auto() {
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
                    ValueCellKind::Auto { .. } => "Auto",
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

        // Build constraints — cross-reference compiled constraints for expressions
        let mut constraints = Vec::new();
        for entry in &check.constraint_results {
            let status = match entry.satisfaction {
                Satisfaction::Satisfied => "Satisfied",
                Satisfaction::Violated => "Violated",
                Satisfaction::Indeterminate => "Indeterminate",
            };

            // Look up the compiled constraint for expression text and parameter refs
            let (expression, parameter_ids) = compiled
                .templates
                .iter()
                .find_map(|t| {
                    t.constraints.iter().find(|c| c.id == entry.id).map(|c| {
                        let expr_str = format_expr(&c.expr);
                        let param_ids = collect_value_refs(&c.expr);
                        (expr_str, param_ids)
                    })
                })
                .unwrap_or_default();

            constraints.push(ConstraintData {
                node_id: entry.id.to_string(),
                expression,
                status: status.to_string(),
                label: entry.label.clone(),
                parameter_ids,
            });
        }

        // Build meshes (from tessellation of realizations)
        let meshes = match self.engine.tessellate_snapshot(compiled) {
            Some(result) => {
                for diag in &result.diagnostics {
                    eprintln!("[tessellation] {:?}: {}", diag.severity, diag.message);
                }
                result
                    .meshes
                    .into_iter()
                    .map(|(entity_path, mesh)| MeshData {
                        entity_path,
                        vertices: mesh.vertices,
                        indices: mesh.indices,
                        normals: mesh.normals,
                    })
                    .collect()
            }
            None => Vec::new(),
        };

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

/// Test helpers — compiled out of production binaries.
#[cfg(test)]
impl EngineSession {
    /// Inject a diagnostic directly into the compiled module's diagnostics vec,
    /// enabling tests to exercise the `diag.labels.first() == None` fallback path
    /// without needing the compiler to produce such a diagnostic.
    ///
    /// # Panics
    /// Panics if no module is currently loaded (`self.compiled` is `None`).
    pub(crate) fn inject_diagnostic_for_test(&mut self, diag: reify_types::Diagnostic) {
        self.compiled
            .as_mut()
            .expect("inject_diagnostic_for_test: no compiled module loaded")
            .diagnostics
            .push(diag);
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
    // Units ordered by descending suffix length — longest match first.
    // debug_assert! enforces this invariant.
    let unit_table: &[(&str, f64, DimensionVector)] = &[
        ("deg", std::f64::consts::PI / 180.0, DimensionVector::ANGLE),
        ("rad", 1.0, DimensionVector::ANGLE),
        ("mm", 0.001, DimensionVector::LENGTH),
        ("cm", 0.01, DimensionVector::LENGTH),
        ("m", 1.0, DimensionVector::LENGTH),
    ];
    debug_assert!(
        unit_table.windows(2).all(|w| w[0].0.len() >= w[1].0.len()),
        "unit_table must be sorted by descending suffix length"
    );
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

/// Format a compiled expression as a human-readable string.
fn format_expr(expr: &reify_types::CompiledExpr) -> String {
    use reify_types::CompiledExprKind;

    match &expr.kind {
        CompiledExprKind::Literal(v) => {
            let (val, unit) = crate::types::format_value(v);
            if unit.is_empty() {
                val
            } else {
                format!("{}{}", val, unit)
            }
        }
        CompiledExprKind::ValueRef(id) => id.member.clone(),
        CompiledExprKind::BinOp { op, left, right } => {
            let op_str = match op {
                reify_types::BinOp::Add => "+",
                reify_types::BinOp::Sub => "-",
                reify_types::BinOp::Mul => "*",
                reify_types::BinOp::Div => "/",
                reify_types::BinOp::Mod => "%",
                reify_types::BinOp::Pow => "**",
                reify_types::BinOp::Eq => "==",
                reify_types::BinOp::Ne => "!=",
                reify_types::BinOp::Lt => "<",
                reify_types::BinOp::Le => "<=",
                reify_types::BinOp::Gt => ">",
                reify_types::BinOp::Ge => ">=",
                reify_types::BinOp::And => "&&",
                reify_types::BinOp::Or => "||",
            };
            format!("{} {} {}", format_expr(left), op_str, format_expr(right))
        }
        CompiledExprKind::UnOp { op, operand } => {
            let op_str = match op {
                reify_types::UnOp::Neg => "-",
                reify_types::UnOp::Not => "!",
            };
            format!("{}{}", op_str, format_expr(operand))
        }
        CompiledExprKind::FunctionCall { function, args } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("{}({})", function.name, arg_strs.join(", "))
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            format!(
                "if {} then {} else {}",
                format_expr(condition),
                format_expr(then_branch),
                format_expr(else_branch)
            )
        }
        CompiledExprKind::Match { discriminant, arms } => {
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|arm| format!("{} => {}", arm.patterns.join(" | "), format_expr(&arm.body)))
                .collect();
            format!(
                "match {} {{ {} }}",
                format_expr(discriminant),
                arm_strs.join(", ")
            )
        }
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("{}({})", function_name, arg_strs.join(", "))
        }
        CompiledExprKind::Lambda { .. } => "<lambda>".to_string(),
        CompiledExprKind::ListLiteral(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(format_expr).collect();
            format!("[{}]", elem_strs.join(", "))
        }
        CompiledExprKind::SetLiteral(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(format_expr).collect();
            format!("set{{{}}}", elem_strs.join(", "))
        }
        CompiledExprKind::MapLiteral(entries) => {
            let entry_strs: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{} => {}", format_expr(k), format_expr(v)))
                .collect();
            format!("map{{{}}}", entry_strs.join(", "))
        }
        CompiledExprKind::IndexAccess { object, index } => {
            format!("{}[{}]", format_expr(object), format_expr(index))
        }
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            if args.is_empty() {
                format!("{}.{}", format_expr(object), method)
            } else {
                let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
                format!(
                    "{}.{}({})",
                    format_expr(object),
                    method,
                    arg_strs.join(", ")
                )
            }
        }
        CompiledExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
            ..
        } => {
            let keyword = match kind {
                reify_types::QuantifierKind::ForAll => "forall",
                reify_types::QuantifierKind::Exists => "exists",
            };
            format!(
                "{} {} in {}: {}",
                keyword,
                variable,
                format_expr(collection),
                format_expr(predicate)
            )
        }
        CompiledExprKind::OptionSome(inner) => format!("some({})", format_expr(inner)),
        CompiledExprKind::OptionNone => "none".to_string(),
        CompiledExprKind::MetaAccess { entity, key } => format!("{}.meta.{}", entity, key),
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            let fn_name = match kind {
                reify_types::DeterminacyPredicateKind::Determined => "determined",
                reify_types::DeterminacyPredicateKind::Undetermined => "undetermined",
                reify_types::DeterminacyPredicateKind::Constrained => "constrained",
                reify_types::DeterminacyPredicateKind::PartiallyDetermined => {
                    "partially_determined"
                }
            };
            format!("{}({})", fn_name, cell.member)
        }
        CompiledExprKind::RangeConstructor {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => match (lower, upper) {
            (Some(lo), Some(hi)) => {
                let op = if *upper_inclusive { ".." } else { "..<" };
                format!("{}{}{}", format_expr(lo), op, format_expr(hi))
            }
            (Some(bound), None) => {
                let op = if *lower_inclusive { ">=" } else { ">" };
                format!("{}{}", op, format_expr(bound))
            }
            (None, Some(bound)) => {
                let op = if *upper_inclusive { "<=" } else { "<" };
                format!("{}{}", op, format_expr(bound))
            }
            (None, None) => "..".to_string(),
        },
    }
}

/// Collect all ValueCellId references from a compiled expression.
fn collect_value_refs(expr: &reify_types::CompiledExpr) -> Vec<String> {
    let mut refs: Vec<String> = expr
        .collect_value_refs()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

/// Pre-compute byte positions of all `\n` characters in `source` in O(M).
///
/// Returns a sorted `Vec<usize>` of the byte offset of each newline.
/// Pass this to [`offset_to_line_col_fast`] to binary-search for line/col
/// in O(log M) instead of the O(M) scan done by [`byte_offset_to_line_col`].
pub(crate) fn build_line_offsets(source: &str) -> Vec<usize> {
    source
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| if b == b'\n' { Some(i) } else { None })
        .collect()
}

/// Binary-search for the (line, column) of `offset` using a pre-built newline table.
///
/// `line_offsets` must be the result of [`build_line_offsets`] for the same source.
/// Both line and column are 1-based. Runs in O(log M) vs O(M) for the naive scan.
pub(crate) fn offset_to_line_col_fast(line_offsets: &[usize], offset: usize) -> (usize, usize) {
    // Count newlines that appear *strictly before* `offset`.
    let line_idx = line_offsets.partition_point(|&nl| nl < offset);
    let line = line_idx + 1;
    let col = if line_idx == 0 {
        offset + 1
    } else {
        offset - line_offsets[line_idx - 1]
    };
    (line, col)
}

/// Convert a byte offset in source text to (line, column), both 1-based.
pub(crate) fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
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
