// Navigation tools (2 tools)

use crate::registry::ToolRegistry;
use crate::types::ToolError;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(
        "reify_focus_entity",
        "Focus an entity in the 3D viewport (zoom to fit).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "entity_path": {
                    "type": "string",
                    "description": "The entity path to focus on."
                }
            },
            "required": ["entity_path"]
        }),
        |params, ctx| {
            let entity_path = params["entity_path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("entity_path is required".to_string()))?;

            let result = ctx.focus_entity(entity_path)?;

            Ok(serde_json::json!({
                "success": result,
            }))
        },
    );

    registry.register(
        "reify_navigate_to_source",
        "Navigate the editor to the source location of an entity.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "entity_path": {
                    "type": "string",
                    "description": "The entity path to navigate to its source definition."
                }
            },
            "required": ["entity_path"]
        }),
        |params, ctx| {
            let entity_path = params["entity_path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidParams("entity_path is required".to_string()))?;

            match ctx.get_source_location(entity_path) {
                Ok(loc) => {
                    ctx.navigate_to_source(
                        &loc.file_path,
                        loc.line,
                        loc.column,
                        loc.end_line,
                        loc.end_column,
                    )?;

                    Ok(serde_json::json!({
                        "success": true,
                        "location": {
                            "file_path": loc.file_path,
                            "line": loc.line,
                            "column": loc.column,
                            "end_line": loc.end_line,
                            "end_column": loc.end_column,
                        },
                    }))
                }
                Err(_) => Ok(serde_json::json!({
                    "success": false,
                    "location": null,
                })),
            }
        },
    );
}
