// CliToolContext — real engine-backed implementation of ReifyToolContext for CLI mode

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use reify_compiler::ValueCellKind;
use reify_mcp::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, ReifyToolContext,
    SelectionInfo, SetParamResult, SourceContent, SourceLocationInfo, ToolError, UpdateResult,
};
use reify_types::{DeterminacyState, Value};

/// Tracks the state of an open file.
struct FileEntry {
    content: String,
    dirty: bool,
}

/// Internal mutable state behind a Mutex.
struct CliState {
    engine: Option<reify_eval::Engine>,
    compiled: Option<reify_compiler::CompiledModule>,
    files: HashMap<String, FileEntry>,
    active_file: Option<String>,
    project_dir: PathBuf,
}

/// CLI-mode implementation of ReifyToolContext.
///
/// Backed by a real Engine with interior mutability via Mutex<CliState>.
pub struct CliToolContext {
    state: Mutex<CliState>,
}

impl CliToolContext {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            state: Mutex::new(CliState {
                engine: None,
                compiled: None,
                files: HashMap::new(),
                active_file: None,
                project_dir,
            }),
        }
    }

    /// Load a .ri file: read from disk, parse, compile, eval.
    pub fn load_file(&self, path: &str) -> Result<(), String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

        let module_name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        let parsed =
            reify_syntax::parse(&source, reify_types::ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        let compiled = reify_compiler::compile(&parsed);

        let checker = reify_constraints::SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        engine.eval(&compiled);

        let abs_path = std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .to_string();

        let mut state = self.state.lock().unwrap();
        state.files.insert(
            abs_path.clone(),
            FileEntry {
                content: source,
                dirty: false,
            },
        );
        state.active_file = Some(abs_path);
        state.compiled = Some(compiled);
        state.engine = Some(engine);

        Ok(())
    }
}

/// Convert a byte offset to (line, column), both 1-based.
fn byte_offset_to_line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = offset as usize;
    let mut line = 1u32;
    let mut col = 1u32;
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

/// Format a dimension as a human-readable unit string.
fn dimension_unit(ty: &reify_types::ty::Type) -> String {
    match ty {
        reify_types::ty::Type::Scalar { dimension } => format!("{}", dimension),
        _ => String::new(),
    }
}

impl ReifyToolContext for CliToolContext {
    fn get_source(&self, file_path: Option<&str>) -> Result<SourceContent, ToolError> {
        let state = self.state.lock().unwrap();
        let path = file_path
            .map(|s| s.to_string())
            .or_else(|| state.active_file.clone())
            .ok_or_else(|| ToolError::EngineError("no active file".to_string()))?;

        let entry = state
            .files
            .get(&path)
            .ok_or_else(|| ToolError::EngineError(format!("file not open: {path}")))?;

        Ok(SourceContent {
            content: entry.content.clone(),
            file_path: path,
        })
    }

    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .files
            .iter()
            .map(|(path, entry)| OpenFileInfo {
                path: path.clone(),
                language: "reify".to_string(),
                dirty: entry.dirty,
            })
            .collect())
    }

    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        let state = self.state.lock().unwrap();
        let mut result = Vec::new();

        if let Some(compiled) = &state.compiled {
            let file_path = state.active_file.clone().unwrap_or_default();
            let source = state
                .files
                .get(&file_path)
                .map(|f| f.content.as_str())
                .unwrap_or("");

            for diag in &compiled.diagnostics {
                // Use the first label's span if available, otherwise default to (1,1)
                let (line, column, end_line, end_column) =
                    if let Some(label) = diag.labels.first() {
                        let (l, c) = byte_offset_to_line_col(source, label.span.start);
                        let (el, ec) = byte_offset_to_line_col(source, label.span.end);
                        (l, c, el, ec)
                    } else {
                        (1, 1, 1, 1)
                    };
                result.push(DiagnosticInfo {
                    file_path: file_path.clone(),
                    line,
                    column,
                    end_line,
                    end_column,
                    severity: format!("{}", diag.severity),
                    message: diag.message.clone(),
                    code: None,
                });
            }
        }

        Ok(result)
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        let state = self.state.lock().unwrap();
        let snapshot = match state.engine.as_ref().and_then(|e| e.snapshot()) {
            Some(s) => s,
            None => return Ok(vec![]),
        };

        let compiled = match &state.compiled {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let mut params = Vec::new();

        // Iterate through all templates to get cell metadata (kind, type)
        for template in &compiled.templates {
            for cell_decl in &template.value_cells {
                let id = &cell_decl.id;
                let (value, determinacy) = snapshot
                    .values
                    .get(id)
                    .cloned()
                    .unwrap_or((Value::Undef, DeterminacyState::Undetermined));

                let kind_str = match cell_decl.kind {
                    ValueCellKind::Param => "Param",
                    ValueCellKind::Let => "Let",
                    ValueCellKind::Auto => "Auto",
                };

                let det_str = match determinacy {
                    DeterminacyState::Determined => "determined",
                    DeterminacyState::Undetermined => "undetermined",
                    DeterminacyState::Provisional => "provisional",
                    DeterminacyState::Auto => "auto",
                };

                params.push(ParameterInfo {
                    cell_id: format!("{}", id),
                    name: id.member.clone(),
                    value: format!("{}", value),
                    unit: dimension_unit(&cell_decl.cell_type),
                    kind: kind_str.to_string(),
                    entity_path: id.entity.clone(),
                    determinacy: det_str.to_string(),
                });
            }
        }

        Ok(params)
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        let state = self.state.lock().unwrap();
        let compiled = match &state.compiled {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        // We need to run check to get constraint satisfaction status.
        // If the engine has been initialized, use the snapshot's constraint data.
        let mut result = Vec::new();

        // Get constraint results from the engine if available
        // For now, use the compiled constraints with "unknown" status,
        // then we'll upgrade when we have check results
        for template in &compiled.templates {
            for constraint in &template.constraints {
                result.push(ConstraintInfo {
                    node_id: format!("{}", constraint.id),
                    expression: format!("{:?}", constraint.expr),
                    status: "unknown".to_string(),
                    label: constraint.label.clone(),
                    parameter_ids: vec![],
                });
            }
        }

        Ok(result)
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        let state = self.state.lock().unwrap();
        let phase = if state.engine.is_some() {
            "ready"
        } else {
            "idle"
        };
        Ok(EvalStatusInfo {
            phase: phase.to_string(),
            progress: None,
            dirty_count: 0,
        })
    }

    fn get_selection(&self) -> Result<SelectionInfo, ToolError> {
        Ok(SelectionInfo {
            selected_entity: None,
            hovered_entity: None,
        })
    }

    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        let state = self.state.lock().unwrap();
        let compiled = state
            .compiled
            .as_ref()
            .ok_or_else(|| ToolError::EngineError("no compiled module".to_string()))?;
        let file_path = state.active_file.clone().unwrap_or_default();
        let source = state
            .files
            .get(&file_path)
            .map(|f| f.content.as_str())
            .unwrap_or("");

        // Search templates for matching entity
        for template in &compiled.templates {
            if template.name == entity_path {
                // Return the span of the first value cell as a proxy for the entity
                if let Some(cell) = template.value_cells.first() {
                    let (line, column) = byte_offset_to_line_col(source, cell.span.start);
                    let (end_line, end_column) = byte_offset_to_line_col(source, cell.span.end);
                    return Ok(SourceLocationInfo {
                        file: file_path,
                        line,
                        column,
                        end_line,
                        end_column,
                    });
                }
            }

            // Also check for entity.member pattern
            for cell in &template.value_cells {
                let cell_id_str = format!("{}", cell.id);
                if cell_id_str == entity_path || cell.id.member == entity_path {
                    let (line, column) = byte_offset_to_line_col(source, cell.span.start);
                    let (end_line, end_column) = byte_offset_to_line_col(source, cell.span.end);
                    return Ok(SourceLocationInfo {
                        file: file_path,
                        line,
                        column,
                        end_line,
                        end_column,
                    });
                }
            }
        }

        Err(ToolError::EngineError(format!(
            "entity not found: {entity_path}"
        )))
    }

    fn update_source(&self, _file_path: &str, _content: &str) -> Result<UpdateResult, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn set_parameter(&self, _cell_id: &str, _value: &str) -> Result<SetParamResult, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn open_file(&self, _file_path: &str) -> Result<OpenFileInfo, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn save_file(&self, _file_path: Option<&str>) -> Result<bool, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn export(&self, _format: &str, _output_path: &str) -> Result<bool, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn focus_entity(&self, _entity_path: &str) -> Result<bool, ToolError> {
        Ok(false)
    }

    fn navigate_to_source(
        &self,
        _file: &str,
        _line: u32,
        _column: u32,
    ) -> Result<bool, ToolError> {
        Ok(false)
    }
}
