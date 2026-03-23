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
        |_params, _ctx| Err(ToolError::NotImplemented),
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
        |_params, _ctx| Err(ToolError::NotImplemented),
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
        |_params, _ctx| Err(ToolError::NotImplemented),
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
