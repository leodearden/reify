// TauriToolContext — bridges MCP tool context to EngineSession for Tauri GUI

use std::sync::{Arc, Mutex, RwLock};

use reify_mcp::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, ReifyToolContext,
    SelectionInfo, SetParamResult, SourceContent, SourceLocationInfo, ToolError, UpdateResult,
};

use crate::engine::EngineSession;

/// Event emitter callback type for navigation events (focus_entity, navigate_to_source).
type EventEmitter = Box<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// Bridges the MCP tool context trait to an EngineSession for use in the Tauri GUI.
///
/// Holds an `Arc<Mutex<EngineSession>>` and delegates each `ReifyToolContext` method
/// to the appropriate `EngineSession` method after acquiring the lock. Converts between
/// GUI types (`ValueData`, `ConstraintData`) and MCP types (`ParameterInfo`, `ConstraintInfo`).
///
/// An optional event emitter callback is used for navigation tools (`focus_entity`,
/// `navigate_to_source`), keeping the struct testable without a Tauri runtime.
///
/// Selection state is read from a shared `Arc<RwLock<SelectionInfo>>` that the frontend
/// updates via the `update_selection` Tauri command.
pub struct TauriToolContext {
    engine: Arc<Mutex<EngineSession>>,
    event_emitter: Option<EventEmitter>,
    selection: Arc<RwLock<SelectionInfo>>,
}

/// Builder for [`TauriToolContext`].
///
/// Use [`TauriToolContext::builder`] to create a builder, then chain
/// `.with_selection(...)` and/or `.with_event_emitter(...)` before calling `.build()`.
pub struct TauriToolContextBuilder {
    engine: Arc<Mutex<EngineSession>>,
    event_emitter: Option<EventEmitter>,
    selection: Option<Arc<RwLock<SelectionInfo>>>,
}

impl TauriToolContextBuilder {
    /// Set a shared selection state for the context.
    ///
    /// If not called, `build()` creates a fresh unshared `Arc<RwLock<SelectionInfo>>`
    /// with empty fields.
    pub fn with_selection(mut self, selection: Arc<RwLock<SelectionInfo>>) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Set an event emitter for navigation events (`focus_entity`, `navigate_to_source`).
    ///
    /// The closure is boxed into an [`EventEmitter`] during `build()`.
    pub fn with_event_emitter(
        mut self,
        emitter: impl Fn(&str, serde_json::Value) + Send + Sync + 'static,
    ) -> Self {
        self.event_emitter = Some(Box::new(emitter));
        self
    }

    /// Finalize the builder and create a [`TauriToolContext`].
    ///
    /// If no selection was provided, creates a fresh unshared `Arc<RwLock<SelectionInfo>>`
    /// with empty fields (not connected to the frontend).
    pub fn build(self) -> TauriToolContext {
        let selection = self
            .selection
            .unwrap_or_else(|| Arc::new(RwLock::new(SelectionInfo::default())));
        TauriToolContext {
            engine: self.engine,
            event_emitter: self.event_emitter,
            selection,
        }
    }
}

impl TauriToolContext {
    /// Create a [`TauriToolContextBuilder`] with the given engine.
    pub fn builder(engine: Arc<Mutex<EngineSession>>) -> TauriToolContextBuilder {
        TauriToolContextBuilder {
            engine,
            event_emitter: None,
            selection: None,
        }
    }
}

impl ReifyToolContext for TauriToolContext {
    fn get_source(&self, _file_path: Option<&str>) -> Result<SourceContent, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session.build_gui_state().map_err(ToolError::EngineError)?;

        // Return the first file's content (single-file model for now)
        if let Some(file) = gui_state.files.first() {
            Ok(SourceContent {
                content: file.content.clone(),
                file_path: file.path.clone(),
            })
        } else {
            Err(ToolError::EngineError("No source loaded".to_string()))
        }
    }

    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session.build_gui_state().map_err(ToolError::EngineError)?;

        Ok(gui_state
            .files
            .iter()
            .map(|f| OpenFileInfo {
                path: f.path.clone(),
                language: "reify".to_string(),
                dirty: false,
            })
            .collect())
    }

    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        let session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;

        Ok(session.get_diagnostics())
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session.build_gui_state().map_err(ToolError::EngineError)?;

        Ok(gui_state
            .values
            .iter()
            .map(|v| ParameterInfo {
                cell_id: v.cell_id.clone(),
                name: v.name.clone(),
                value: v.value.clone(),
                unit: v.unit.clone(),
                kind: v.kind.clone(),
                entity_path: v.entity_path.clone(),
                determinacy: v.determinacy.clone(),
                reason: None, // wired in step-8
            })
            .collect())
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session.build_gui_state().map_err(ToolError::EngineError)?;

        Ok(gui_state
            .constraints
            .iter()
            .map(|c| ConstraintInfo {
                node_id: c.node_id.clone(),
                expression: c.expression.clone(),
                status: c.status.clone(),
                label: c.label.clone(),
                parameter_ids: c.parameter_ids.clone(),
            })
            .collect())
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        Ok(EvalStatusInfo {
            phase: "idle".to_string(),
            progress: None,
            dirty_count: 0,
        })
    }

    fn get_selection(&self) -> Result<SelectionInfo, ToolError> {
        let sel = self
            .selection
            .read()
            .map_err(|e| ToolError::InternalError(format!("Selection lock poisoned: {}", e)))?;
        Ok(sel.clone())
    }

    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        let session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;

        session
            .get_source_location(entity_path)
            .ok_or_else(|| ToolError::EngineError(format!("entity not found: {}", entity_path)))
    }

    fn update_source(&self, file_path: &str, content: &str) -> Result<UpdateResult, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        session
            .update_source(file_path, content)
            .map(|_| UpdateResult {
                success: true,
                diagnostics_count: 0,
            })
            .map_err(ToolError::EngineError)
    }

    fn set_parameter(&self, cell_id: &str, value: &str) -> Result<SetParamResult, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session
            .set_parameter(cell_id, value)
            .map_err(ToolError::EngineError)?;

        // Find the updated parameter in the returned GuiState
        let param = gui_state
            .values
            .iter()
            .find(|v| v.cell_id == cell_id)
            .ok_or_else(|| {
                ToolError::EngineError(format!("parameter '{}' not found in result", cell_id))
            })?;

        Ok(SetParamResult {
            success: true,
            new_value: param.value.clone(),
            unit: param.unit.clone(),
        })
    }

    fn open_file(&self, file_path: &str) -> Result<OpenFileInfo, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        session
            .load_file(std::path::Path::new(file_path))
            .map_err(ToolError::EngineError)?;

        Ok(OpenFileInfo {
            path: file_path.to_string(),
            language: "reify".to_string(),
            dirty: false,
        })
    }

    fn save_file(&self, file_path: Option<&str>) -> Result<bool, ToolError> {
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        let gui_state = session.build_gui_state().map_err(ToolError::EngineError)?;

        // Get the first file's content (single-file model)
        let file = gui_state
            .files
            .first()
            .ok_or_else(|| ToolError::EngineError("No source loaded".to_string()))?;

        let path = file_path.unwrap_or(&file.path);
        std::fs::write(path, &file.content)
            .map_err(|e| ToolError::EngineError(format!("Error writing {}: {}", path, e)))?;
        Ok(true)
    }

    fn export(&self, format: &str, output_path: &str) -> Result<bool, ToolError> {
        let export_format = match format {
            "step" | "stp" => reify_ir::ExportFormat::Step,
            "stl" => reify_ir::ExportFormat::Stl,
            _ => {
                return Err(ToolError::InvalidParams(format!(
                    "Unknown export format: {}",
                    format
                )));
            }
        };
        let mut session = self
            .engine
            .lock()
            .map_err(|e| ToolError::InternalError(format!("Lock error: {}", e)))?;
        session
            .export(export_format, std::path::Path::new(output_path))
            .map_err(ToolError::EngineError)?;
        Ok(true)
    }

    fn focus_entity(&self, entity_path: &str) -> Result<bool, ToolError> {
        if let Some(ref emitter) = self.event_emitter {
            emitter("focus-entity", serde_json::json!(entity_path));
        }
        Ok(true)
    }

    fn navigate_to_source(
        &self,
        file: &str,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Result<bool, ToolError> {
        if let Some(ref emitter) = self.event_emitter {
            emitter(
                "navigate-to-source",
                serde_json::json!({
                    "file": file,
                    "line": line,
                    "column": column,
                    "end_line": end_line,
                    "end_column": end_column,
                }),
            );
        }
        Ok(true)
    }
}

/// Dispatch an MCP tool call by name, using the given context.
///
/// Creates a fresh `ToolRegistry`, registers all tools, then dispatches.
/// Returns the tool's JSON result or a String error.
pub fn mcp_tool_call_impl(
    name: &str,
    params: serde_json::Value,
    context: &dyn ReifyToolContext,
) -> Result<serde_json::Value, String> {
    let mut registry = reify_mcp::ToolRegistry::new();
    reify_mcp::register_all_tools(&mut registry);
    registry
        .call_tool(name, params, context)
        .map_err(|e| e.to_string())
}
