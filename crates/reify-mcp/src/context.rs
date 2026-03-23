// ReifyToolContext — trait abstracting engine access for MCP tools

use crate::types::*;

/// Trait that abstracts engine access for MCP tool handlers.
///
/// Implementors bridge between the engine state and MCP-specific types.
/// The trait is object-safe, Send, and Sync so it can be shared across
/// async tasks and stored behind `Arc<dyn ReifyToolContext>`.
pub trait ReifyToolContext: Send + Sync {
    /// Get source code for a file (or the active file if path is None).
    fn get_source(&self, file_path: Option<&str>) -> Result<String, ToolError>;

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
    ) -> Result<bool, ToolError>;
}

/// Mock implementation of ReifyToolContext for testing.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct MockToolContext;

#[cfg(any(test, feature = "test-support"))]
impl ReifyToolContext for MockToolContext {
    fn get_source(&self, _file_path: Option<&str>) -> Result<String, ToolError> {
        Ok(String::new())
    }

    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        Ok(EvalStatusInfo {
            phase: "idle".to_string(),
            progress: None,
            dirty_count: 0,
        })
    }

    fn get_selection(&self) -> Result<SelectionInfo, ToolError> {
        Ok(SelectionInfo {
            entity_path: None,
            cell_ids: vec![],
        })
    }

    fn get_source_location(&self, _entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        Ok(SourceLocationInfo {
            file: String::new(),
            line: 0,
            column: 0,
            end_line: 0,
            end_column: 0,
        })
    }

    fn update_source(&self, _file_path: &str, _content: &str) -> Result<UpdateResult, ToolError> {
        Ok(UpdateResult {
            success: true,
            diagnostics_count: 0,
        })
    }

    fn set_parameter(&self, _cell_id: &str, _value: &str) -> Result<SetParamResult, ToolError> {
        Ok(SetParamResult {
            success: true,
            new_value: String::new(),
            unit: String::new(),
        })
    }

    fn open_file(&self, file_path: &str) -> Result<OpenFileInfo, ToolError> {
        Ok(OpenFileInfo {
            path: file_path.to_string(),
            language: "reify".to_string(),
        })
    }

    fn save_file(&self, _file_path: Option<&str>) -> Result<bool, ToolError> {
        Ok(true)
    }

    fn export(&self, _format: &str, _output_path: &str) -> Result<bool, ToolError> {
        Ok(true)
    }

    fn focus_entity(&self, _entity_path: &str) -> Result<bool, ToolError> {
        Ok(true)
    }

    fn navigate_to_source(
        &self,
        _file: &str,
        _line: u32,
        _column: u32,
    ) -> Result<bool, ToolError> {
        Ok(true)
    }
}
