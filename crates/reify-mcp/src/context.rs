// ReifyToolContext — trait abstracting engine access for MCP tools

use crate::types::*;

/// Trait that abstracts engine access for MCP tool handlers.
///
/// Implementors bridge between the engine state and MCP-specific types.
/// The trait is object-safe, Send, and Sync so it can be shared across
/// async tasks and stored behind `Arc<dyn ReifyToolContext>`.
pub trait ReifyToolContext: Send + Sync {
    /// Get source code for a file (or the active file if path is None).
    fn get_source(&self, file_path: Option<&str>) -> Result<SourceContent, ToolError>;

    /// List all open files.
    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError>;

    /// Get all diagnostics (errors/warnings).
    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError>;

    /// Get all parameters (value cells).
    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError>;

    /// Get all constraints.
    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError>;

    /// Get the current evaluation engine status.
    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError>;

    /// Get the current viewport selection.
    fn get_selection(&self) -> Result<SelectionInfo, ToolError>;

    /// Get the source location of an entity.
    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError>;

    /// Update source code for a file.
    fn update_source(&self, file_path: &str, content: &str) -> Result<UpdateResult, ToolError>;

    /// Set a parameter value.
    fn set_parameter(&self, cell_id: &str, value: &str) -> Result<SetParamResult, ToolError>;

    /// Open a file from disk.
    fn open_file(&self, file_path: &str) -> Result<OpenFileInfo, ToolError>;

    /// Save the current file.
    fn save_file(&self, file_path: Option<&str>) -> Result<bool, ToolError>;

    /// Export to a given format.
    fn export(&self, format: &str, output_path: &str) -> Result<bool, ToolError>;

    /// Focus an entity in the viewport.
    fn focus_entity(&self, entity_path: &str) -> Result<bool, ToolError>;

    /// Navigate to a source location.
    fn navigate_to_source(
        &self,
        file: &str,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Result<bool, ToolError>;
}

/// Mock implementation of ReifyToolContext for testing.
///
/// All fields are public so tests can configure specific data.
/// Use struct update syntax: `MockToolContext { diagnostics: vec![...], ..Default::default() }`.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct MockToolContext {
    pub source: SourceContent,
    pub open_files: Vec<OpenFileInfo>,
    pub diagnostics: Vec<DiagnosticInfo>,
    pub parameters: Vec<ParameterInfo>,
    pub constraints: Vec<ConstraintInfo>,
    pub eval_status: EvalStatusInfo,
    pub selection: SelectionInfo,
    pub source_locations: std::collections::HashMap<String, SourceLocationInfo>,
    // Optional error overrides for write/nav methods
    pub update_source_error: Option<ToolError>,
    pub set_param_error: Option<ToolError>,
    pub open_file_error: Option<ToolError>,
    pub save_file_error: Option<ToolError>,
    pub export_error: Option<ToolError>,
    pub focus_entity_error: Option<ToolError>,
    pub navigate_error: Option<ToolError>,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for MockToolContext {
    fn default() -> Self {
        Self {
            source: SourceContent {
                content: String::new(),
                file_path: String::new(),
            },
            open_files: vec![],
            diagnostics: vec![],
            parameters: vec![],
            constraints: vec![],
            eval_status: EvalStatusInfo {
                phase: "idle".to_string(),
                progress: None,
                dirty_count: 0,
            },
            selection: SelectionInfo::default(),
            source_locations: std::collections::HashMap::new(),
            update_source_error: None,
            set_param_error: None,
            open_file_error: None,
            save_file_error: None,
            export_error: None,
            focus_entity_error: None,
            navigate_error: None,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ReifyToolContext for MockToolContext {
    fn get_source(&self, _file_path: Option<&str>) -> Result<SourceContent, ToolError> {
        Ok(self.source.clone())
    }

    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError> {
        Ok(self.open_files.clone())
    }

    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        Ok(self.diagnostics.clone())
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        Ok(self.parameters.clone())
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        Ok(self.constraints.clone())
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        Ok(self.eval_status.clone())
    }

    fn get_selection(&self) -> Result<SelectionInfo, ToolError> {
        Ok(self.selection.clone())
    }

    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        self.source_locations
            .get(entity_path)
            .cloned()
            .ok_or_else(|| ToolError::EngineError(format!("entity not found: {entity_path}")))
    }

    fn update_source(&self, _file_path: &str, _content: &str) -> Result<UpdateResult, ToolError> {
        if let Some(err) = &self.update_source_error {
            return Err(err.clone());
        }
        Ok(UpdateResult {
            success: true,
            diagnostics_count: 0,
        })
    }

    fn set_parameter(&self, _cell_id: &str, _value: &str) -> Result<SetParamResult, ToolError> {
        if let Some(err) = &self.set_param_error {
            return Err(err.clone());
        }
        Ok(SetParamResult {
            success: true,
            new_value: String::new(),
            unit: String::new(),
        })
    }

    fn open_file(&self, file_path: &str) -> Result<OpenFileInfo, ToolError> {
        if let Some(err) = &self.open_file_error {
            return Err(err.clone());
        }
        Ok(OpenFileInfo {
            path: file_path.to_string(),
            language: "reify".to_string(),
            dirty: false,
        })
    }

    fn save_file(&self, _file_path: Option<&str>) -> Result<bool, ToolError> {
        if let Some(err) = &self.save_file_error {
            return Err(err.clone());
        }
        Ok(true)
    }

    fn export(&self, _format: &str, _output_path: &str) -> Result<bool, ToolError> {
        if let Some(err) = &self.export_error {
            return Err(err.clone());
        }
        Ok(true)
    }

    fn focus_entity(&self, _entity_path: &str) -> Result<bool, ToolError> {
        if let Some(err) = &self.focus_entity_error {
            return Err(err.clone());
        }
        Ok(true)
    }

    fn navigate_to_source(
        &self,
        _file: &str,
        _line: u32,
        _column: u32,
        _end_line: u32,
        _end_column: u32,
    ) -> Result<bool, ToolError> {
        if let Some(err) = &self.navigate_error {
            return Err(err.clone());
        }
        Ok(true)
    }
}
