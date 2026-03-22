// Navigation tool stubs (2 tools)

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
        |_params, _ctx| Err(ToolError::NotImplemented),
    );

    registry.register(
        "reify_navigate_to_source",
        "Navigate the editor to a specific source location.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "description": "File path to navigate to."
                },
                "line": {
                    "type": "integer",
                    "description": "Line number (1-based)."
                },
                "column": {
                    "type": "integer",
                    "description": "Column number (1-based)."
                }
            },
            "required": ["file", "line", "column"]
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );
}
