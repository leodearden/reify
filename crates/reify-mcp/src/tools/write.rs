// Write tool stubs (5 tools)

use crate::registry::ToolRegistry;
use crate::types::ToolError;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(
        "reify_update_source",
        "Update the source code of a file. Triggers re-evaluation.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to update."
                },
                "content": {
                    "type": "string",
                    "description": "The new source code content."
                }
            },
            "required": ["file_path", "content"]
        }),
        |params, ctx| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("file_path is required".to_string()))?;
            let content = params["content"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("content is required".to_string()))?;

            ctx.update_source(file_path, content)?;

            let diagnostics = ctx.get_diagnostics()?;
            let filtered: Vec<_> = diagnostics
                .into_iter()
                .filter(|d| d.file_path == file_path)
                .collect();

            let diagnostics_count = filtered.len();
            let diagnostics_json = serde_json::to_value(&filtered)
                .map_err(|e| ToolError::InternalError(e.to_string()))?;

            Ok(serde_json::json!({
                "success": true,
                "diagnostics_count": diagnostics_count,
                "diagnostics": diagnostics_json,
            }))
        },
    );

    registry.register(
        "reify_set_parameter",
        "Set a parameter value by cell ID. Triggers re-evaluation.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "cell_id": {
                    "type": "string",
                    "description": "The value cell ID to set."
                },
                "value": {
                    "type": "string",
                    "description": "The new value as a string expression."
                }
            },
            "required": ["cell_id", "value"]
        }),
        |params, ctx| {
            let cell_id = params["cell_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("cell_id is required".to_string()))?;
            let value = params["value"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("value is required".to_string()))?;

            let result = ctx.set_parameter(cell_id, value)?;

            let diagnostics = ctx.get_diagnostics()?;
            let diagnostics_json = serde_json::to_value(&diagnostics)
                .map_err(|e| ToolError::InternalError(e.to_string()))?;

            Ok(serde_json::json!({
                "success": result.success,
                "new_value": result.new_value,
                "unit": result.unit,
                "diagnostics": diagnostics_json,
            }))
        },
    );

    registry.register(
        "reify_open_file",
        "Open a file from disk into the project.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to open."
                }
            },
            "required": ["file_path"]
        }),
        |params, ctx| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("file_path is required".to_string()))?;

            ctx.open_file(file_path)?;
            let source = ctx.get_source(Some(file_path))?;

            Ok(serde_json::json!({
                "success": true,
                "source": source.content,
            }))
        },
    );

    registry.register(
        "reify_save_file",
        "Save the current file to disk.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to save to. If omitted, saves the active file."
                }
            }
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_export",
        "Export the model to a file format (e.g., STEP, STL).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "description": "Export format (e.g., 'step', 'stl')."
                },
                "output_path": {
                    "type": "string",
                    "description": "Path to write the exported file."
                }
            },
            "required": ["format", "output_path"]
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );
}
