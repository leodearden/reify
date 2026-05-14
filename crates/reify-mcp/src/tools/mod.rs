// Tool registration

pub mod language_chunks;
pub mod navigation;
pub mod read;
pub mod reference;
pub mod write;

use crate::registry::ToolRegistry;

/// Register all 16 MCP tools in the given registry.
pub fn register_all_tools(registry: &mut ToolRegistry) {
    read::register(registry);
    write::register(registry);
    navigation::register(registry);
    reference::register(registry);
}
