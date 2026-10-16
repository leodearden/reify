// JSON-RPC 2.0 dispatcher for MCP protocol

use serde::{Deserialize, Serialize};

use crate::context::ReifyToolContext;
use crate::registry::ToolRegistry;

/// A JSON-RPC 2.0 request, or a notification when `id` is absent.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// `None` only when the member is absent; an explicit `null` is `Some(Null)`.
    #[serde(default, deserialize_with = "deserialize_present")]
    pub id: Option<serde_json::Value>,
}

fn deserialize_present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
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
/// Invalid Request — the JSON sent is not a valid Request object.
pub const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
/// Invalid params — invalid method parameter(s).
pub const INVALID_PARAMS: i32 = -32602;
/// Internal error — internal JSON-RPC error.
pub const INTERNAL_ERROR: i32 = -32603;

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

    /// Dispatch a JSON-RPC message string, returning the JSON-RPC response string.
    ///
    /// Returns `None` for a notification (a valid message without an `id`),
    /// which JSON-RPC 2.0 forbids answering.
    pub fn dispatch(&self, json_str: &str) -> Option<String> {
        let request: JsonRpcRequest = match serde_json::from_str(json_str) {
            Ok(req) => req,
            Err(_) => {
                return Some(self.error_response(
                    serde_json::Value::Null,
                    PARSE_ERROR,
                    "Parse error",
                ));
            }
        };

        // Validate jsonrpc version
        if request.jsonrpc != "2.0" {
            return Some(self.error_response(
                request.id.unwrap_or(serde_json::Value::Null),
                INVALID_REQUEST,
                "Invalid Request: jsonrpc must be '2.0'",
            ));
        }

        let id = request.id?;

        // Validate id is string or number (reject null, array, object, bool)
        if !id.is_string() && !id.is_number() {
            return Some(self.error_response(
                serde_json::Value::Null,
                INVALID_REQUEST,
                "Invalid Request: id must be a string or number",
            ));
        }

        let response = match request.method.as_str() {
            "initialize" => Self::handle_initialize(id),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, &request.params),
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: METHOD_NOT_FOUND,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            },
        };

        Some(serde_json::to_string(&response).unwrap_or_else(|e| {
            self.error_response(
                serde_json::Value::Null,
                -32603,
                &format!("Internal error: {e}"),
            )
        }))
    }

    fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
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

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        let tools = self.registry.list_tools();
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::json!({ "tools": tools })),
            error: None,
        }
    }

    fn handle_tools_call(
        &self,
        id: serde_json::Value,
        params: &serde_json::Value,
    ) -> JsonRpcResponse {
        // Validate name is a string
        let name = match params["name"].as_str() {
            Some(s) => s.to_string(),
            None => {
                return self.mcp_error_response(
                    &id,
                    "missing or invalid tool name: 'name' must be a string",
                );
            }
        };

        // Validate arguments: if present and not null, must be an object
        let arguments = match &params["arguments"] {
            v if v.is_null() => serde_json::json!({}),
            v if v.is_object() => v.clone(),
            _ => {
                return self
                    .mcp_error_response(&id, "invalid arguments: 'arguments' must be an object");
            }
        };

        match self.registry.call_tool(&name, arguments, self.context) {
            Ok(value) => {
                let text = match serde_json::to_string(&value) {
                    Ok(s) => s,
                    Err(e) => {
                        return JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: INTERNAL_ERROR,
                                message: format!("failed to serialize tool result: {e}"),
                                data: None,
                            }),
                        };
                    }
                };
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
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
                    id,
                    result: Some(serde_json::json!({
                        "content": [{"type": "text", "text": error_text}],
                        "isError": true
                    })),
                    error: None,
                }
            }
        }
    }

    /// Build an MCP-level isError content response (tool-level error, not JSON-RPC error).
    fn mcp_error_response(&self, id: &serde_json::Value, message: &str) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            result: Some(serde_json::json!({
                "content": [{"type": "text", "text": message}],
                "isError": true
            })),
            error: None,
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
        serde_json::to_string(&response).expect("JsonRpcResponse serialization")
    }
}
