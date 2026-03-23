// Read tools (8 tools)

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
        |params, ctx| {
            let file_path = params["file_path"].as_str();
            let source = ctx.get_source(file_path)?;
            serde_json::to_value(source).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_open_files",
        "List all currently open files in the project.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, ctx| {
            let files = ctx.get_open_files()?;
            serde_json::to_value(files).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_diagnostics",
        "Get all diagnostics (errors and warnings) from the engine.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Filter diagnostics by file path."
                },
                "severity": {
                    "type": "string",
                    "description": "Filter diagnostics by severity (e.g. 'error', 'warning')."
                }
            }
        }),
        |params, ctx| {
            let diagnostics = ctx.get_diagnostics()?;
            let file_path_filter = params["file_path"].as_str();
            let severity_filter = params["severity"].as_str();

            let filtered: Vec<_> = diagnostics
                .into_iter()
                .filter(|d| {
                    if let Some(fp) = file_path_filter
                        && d.file_path != fp
                    {
                        return false;
                    }
                    if let Some(sev) = severity_filter
                        && !d.severity.eq_ignore_ascii_case(sev)
                    {
                        return false;
                    }
                    true
                })
                .collect();

            serde_json::to_value(filtered).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_parameters",
        "Get all parameters (value cells) in the current model.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "entity_path": {
                    "type": "string",
                    "description": "Filter parameters by entity path prefix."
                }
            }
        }),
        |params, ctx| {
            let parameters = ctx.get_parameters()?;
            let entity_path_filter = params["entity_path"].as_str();

            let filtered: Vec<_> = parameters
                .into_iter()
                .filter(|p| {
                    if let Some(prefix) = entity_path_filter
                        && !p.entity_path.starts_with(prefix)
                    {
                        return false;
                    }
                    true
                })
                .collect();

            serde_json::to_value(filtered).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_constraints",
        "Get all constraints and their satisfaction status.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Filter constraints by status (e.g. 'satisfied', 'violated')."
                }
            }
        }),
        |params, ctx| {
            let constraints = ctx.get_constraints()?;
            let status_filter = params["status"].as_str();

            let filtered: Vec<_> = constraints
                .into_iter()
                .filter(|c| {
                    if let Some(status) = status_filter
                        && c.status != status
                    {
                        return false;
                    }
                    true
                })
                .collect();

            serde_json::to_value(filtered).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_eval_status",
        "Get the current evaluation engine status (phase, progress, dirty count).",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, ctx| {
            let status = ctx.get_eval_status()?;
            serde_json::to_value(status).map_err(|e| ToolError::InternalError(e.to_string()))
        },
    );

    registry.register(
        "reify_get_selection",
        "Get the current viewport selection (selected entity and cells).",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        |_params, ctx| {
            let selection = ctx.get_selection()?;
            serde_json::to_value(selection).map_err(|e| ToolError::InternalError(e.to_string()))
        },
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
        |params, ctx| {
            let entity_path = params["entity_path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("entity_path is required".to_string()))?;

            match ctx.get_source_location(entity_path) {
                Ok(location) => serde_json::to_value(location)
                    .map_err(|e| ToolError::InternalError(e.to_string())),
                Err(_) => Ok(serde_json::Value::Null),
            }
        },
    );
}
