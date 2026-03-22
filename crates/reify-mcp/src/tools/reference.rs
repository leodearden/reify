// Reference tool stubs (1 tool)

use crate::registry::ToolRegistry;
use crate::types::ToolError;

pub fn register(registry: &mut ToolRegistry) {
    registry.register(
        "reify_language_reference",
        "Look up Reify language reference documentation for a topic.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "The language feature or topic to look up (e.g., 'param', 'constraint', 'sketch')."
                }
            }
        }),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );
}
