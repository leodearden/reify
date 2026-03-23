// ToolRegistry — maps tool names to handlers

use crate::context::ReifyToolContext;
use crate::types::{ToolError, ToolInfo};

/// Handler function type for MCP tools.
type ToolHandler =
    Box<dyn Fn(serde_json::Value, &dyn ReifyToolContext) -> Result<serde_json::Value, ToolError> + Send + Sync>;

/// An entry in the tool registry.
struct ToolEntry {
    info: ToolInfo,
    handler: ToolHandler,
}

/// Registry that maps tool names to their metadata and handlers.
pub struct ToolRegistry {
    tools: Vec<ToolEntry>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool with its name, description, input schema, and handler.
    pub fn register(
        &mut self,
        name: &str,
        description: &str,
        input_schema: serde_json::Value,
        handler: impl Fn(serde_json::Value, &dyn ReifyToolContext) -> Result<serde_json::Value, ToolError>
            + Send
            + Sync
            + 'static,
    ) {
        if self.tools.iter().any(|e| e.info.name == name) {
            panic!("duplicate tool registration: '{name}'");
        }
        self.tools.push(ToolEntry {
            info: ToolInfo {
                name: name.to_string(),
                description: description.to_string(),
                input_schema,
            },
            handler: Box::new(handler),
        });
    }

    /// List all registered tools.
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools.iter().map(|entry| entry.info.clone()).collect()
    }

    /// Call a tool by name with the given params and context.
    pub fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
        context: &dyn ReifyToolContext,
    ) -> Result<serde_json::Value, ToolError> {
        let entry = self
            .tools
            .iter()
            .find(|e| e.info.name == name)
            .ok_or_else(|| ToolError::InvalidParams(format!("unknown tool: {name}")))?;

        (entry.handler)(params, context)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
