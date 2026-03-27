// Dual-mode transport: in-process handle_message() + async stream mode

use std::sync::Arc;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::context::ReifyToolContext;
use crate::jsonrpc::McpDispatcher;
use crate::registry::ToolRegistry;
use crate::tools::register_all_tools;

/// MCP server combining the tool registry, context, and transport.
pub struct McpServer {
    registry: ToolRegistry,
    context: Arc<dyn ReifyToolContext>,
}

impl McpServer {
    /// Create a new MCP server with the given context.
    ///
    /// Automatically registers all 16 tools.
    pub fn new(context: Arc<dyn ReifyToolContext>) -> Self {
        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);
        Self { registry, context }
    }

    /// Handle a single JSON-RPC message (in-process mode).
    ///
    /// Takes a JSON string, dispatches it, and returns a JSON string response.
    pub fn handle_message(&self, json: &str) -> String {
        let dispatcher = McpDispatcher::new(&self.registry, self.context.as_ref());
        dispatcher.dispatch(json)
    }

    /// Run the MCP server on stdin/stdout (CLI mode).
    ///
    /// Reads newline-delimited JSON from stdin, writes responses to stdout.
    pub async fn run_stdio(&self) {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let reader = tokio::io::BufReader::new(stdin);
        if let Err(e) = self.run_on_streams(reader, stdout).await {
            eprintln!("MCP transport error: {e}");
        }
    }

    /// Run the MCP server on arbitrary async streams (for testing and embedding).
    ///
    /// Reads newline-delimited JSON from reader, writes responses to writer.
    /// Returns `Ok(())` on graceful shutdown (EOF), `Err` on I/O failure.
    pub async fn run_on_streams<R, W>(&self, reader: R, mut writer: W) -> Result<(), std::io::Error>
    where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut lines = reader.lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let response = self.handle_message(trimmed);
                    let output = format!("{response}\n");
                    if let Err(e) = writer.write_all(output.as_bytes()).await {
                        eprintln!("MCP write error: {e}");
                        return Err(e);
                    }
                    if let Err(e) = writer.flush().await {
                        eprintln!("MCP flush error: {e}");
                        return Err(e);
                    }
                }
                Ok(None) => {
                    // Clean EOF — graceful shutdown
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("MCP read error: {e}");
                    return Err(e);
                }
            }
        }
    }
}
