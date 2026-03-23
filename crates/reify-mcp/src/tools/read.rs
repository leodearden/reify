// Read tool stubs (8 tools)

use crate::registry::ToolRegistry;
use crate::types::ToolError;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(
        "reify_get_source",
        "Get the source code of a file. Returns the full text content.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file. If omitted, returns the active file's source."
                }
            }
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_open_files",
        "List all currently open files in the project.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_diagnostics",
        "Get all diagnostics (errors and warnings) from the engine.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_parameters",
        "Get all parameters (value cells) in the current model.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_constraints",
        "Get all constraints and their satisfaction status.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_eval_status",
        "Get the current evaluation engine status (phase, progress, dirty count).",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_selection",
        "Get the current viewport selection (selected entity and cells).",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_get_source_location",
        "Get the source code location of a named entity.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "entity_path": {
                    "type": "string",
                    "description": "The entity path to look up."
                }
            },
            "required": ["entity_path"]
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );
}
