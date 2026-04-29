#![allow(clippy::mutable_key_type)]

pub mod claude_bridge;
pub mod commands;
#[cfg(feature = "gui")]
pub mod debug;
pub mod kernel_status;
#[cfg(feature = "gui")]
pub mod debug_server;
pub mod diff;
pub mod engine;
pub mod lsp_bridge;
pub mod mcp_context;
pub mod types;
pub mod watcher;

#[cfg(test)]
mod tests;
