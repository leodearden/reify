// JSON-RPC 2.0 dispatcher for MCP protocol

use serde::{Deserialize, Serialize};

use crate::context::ReifyToolContext;
use crate::registry::ToolRegistry;

/// A JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: serde_json::Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// Standard JSON-RPC error codes
const PARSE_ERROR: i32 = -32700;
const METHOD_NOT_FOUND: i32 = -32601;

/// MCP protocol dispatcher that routes JSON-RPC requests to the tool registry.
pub struct McpDispatcher<'a> {
    registry: &'a ToolRegistry,
    context: &'a dyn ReifyToolContext,
}

impl<'a> McpDispatcher<'a> {
    /// Create a new dispatcher with the given registry and context.
    pub fn new(registry: &'a ToolRegistry, context: &'a dyn ReifyToolContext) -> Self {
        Self { registry, context }
    }

    /// Dispatch a JSON-RPC request string, returning a JSON-RPC response string.
    pub fn dispatch(&self, json_str: &str) -> String {
        let request: JsonRpcRequest = match serde_json::from_str(json_str) {
            Ok(req) => req,
            Err(_) => {
                return self.error_response(serde_json::Value::Null, PARSE_ERROR, "Parse error");
            }
        };

        let response = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request),
            "tools/list" => self.handle_tools_list(&request),
            "tools/call" => self.handle_tools_call(&request),
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: METHOD_NOT_FOUND,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            },
        };

        serde_json::to_string(&response).unwrap_or_else(|e| {
            self.error_response(
                serde_json::Value::Null,
                -32603,
                &format!("Internal error: {e}"),
            )
        })
    }

    fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "reify-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }
    }

    fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let tools = self.registry.list_tools();
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({ "tools": tools })),
            error: None,
        }
    }

    fn handle_tools_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        let name = request.params["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let arguments = request.params["arguments"].clone();

        match self.registry.call_tool(&name, arguments, self.context) {
            Ok(value) => {
                let text = serde_json::to_string(&value).unwrap_or_default();
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({
                        "content": [{"type": "text", "text": text}],
                        "isError": false
                    })),
                    error: None,
                }
            }
            Err(tool_err) => {
                let error_text = tool_err.to_string();
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(serde_json::json!({
                        "content": [{"type": "text", "text": error_text}],
                        "isError": true
                    })),
                    error: None,
                }
            }
        }
    }

    fn error_response(&self, id: serde_json::Value, code: i32, message: &str) -> String {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        };
        serde_json::to_string(&response).unwrap_or_default()
    }
}
