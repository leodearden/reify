// JSON-RPC 2.0 dispatcher for MCP protocol

use serde::{Deserialize, Serialize};

use crate::context::ReifyToolContext;
use crate::registry::ToolRegistry;

const JSONRPC_VERSION: &str = "2.0";

/// A JSON-RPC 2.0 request, or a notification when `id` is absent.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// `None` only when the member is absent; an explicit `null` is `Some(Null)`.
    #[serde(default, deserialize_with = "present_even_if_null")]
    pub id: Option<serde_json::Value>,
}

fn present_even_if_null<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(deserializer).map(Some)
}

/// A JSON-RPC 2.0 response, carrying exactly one of `result` or `error`.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// A success response carrying `result`.
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: Some(result),
            error: None,
        }
    }

    /// An error response carrying a JSON-RPC error object.
    pub fn err(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
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
    /// which JSON-RPC 2.0 forbids answering. A notification is acknowledged
    /// without effect: no method handler runs for it, `tools/call` included.
    pub fn dispatch(&self, json_str: &str) -> Option<String> {
        self.respond(json_str).map(|response| encode(&response))
    }

    fn respond(&self, json_str: &str) -> Option<JsonRpcResponse> {
        let Ok(request) = serde_json::from_str::<JsonRpcRequest>(json_str) else {
            return Some(JsonRpcResponse::err(
                serde_json::Value::Null,
                PARSE_ERROR,
                "Parse error",
            ));
        };

        if request.jsonrpc != JSONRPC_VERSION {
            return Some(JsonRpcResponse::err(
                request.id.unwrap_or(serde_json::Value::Null),
                INVALID_REQUEST,
                format!("Invalid Request: jsonrpc must be '{JSONRPC_VERSION}'"),
            ));
        }

        let id = request.id?;

        // Validate id is string or number (reject null, array, object, bool)
        if !id.is_string() && !id.is_number() {
            return Some(JsonRpcResponse::err(
                serde_json::Value::Null,
                INVALID_REQUEST,
                "Invalid Request: id must be a string or number",
            ));
        }

        Some(match request.method.as_str() {
            "initialize" => Self::handle_initialize(id),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, &request.params),
            method => {
                JsonRpcResponse::err(id, METHOD_NOT_FOUND, format!("Method not found: {method}"))
            }
        })
    }

    fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
        JsonRpcResponse::ok(
            id,
            serde_json::json!({
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
            }),
        )
    }

    fn handle_tools_list(&self, id: serde_json::Value) -> JsonRpcResponse {
        let tools = self.registry.list_tools();
        JsonRpcResponse::ok(id, serde_json::json!({ "tools": tools }))
    }

    fn handle_tools_call(
        &self,
        id: serde_json::Value,
        params: &serde_json::Value,
    ) -> JsonRpcResponse {
        let Some(name) = params["name"].as_str() else {
            return JsonRpcResponse::ok(
                id,
                tool_result(
                    "missing or invalid tool name: 'name' must be a string",
                    true,
                ),
            );
        };

        // Validate arguments: if present and not null, must be an object
        let arguments = match &params["arguments"] {
            v if v.is_null() => serde_json::json!({}),
            v if v.is_object() => v.clone(),
            _ => {
                return JsonRpcResponse::ok(
                    id,
                    tool_result("invalid arguments: 'arguments' must be an object", true),
                );
            }
        };

        match self.registry.call_tool(name, arguments, self.context) {
            Ok(value) => match serde_json::to_string(&value) {
                Ok(text) => JsonRpcResponse::ok(id, tool_result(text, false)),
                Err(e) => JsonRpcResponse::err(
                    id,
                    INTERNAL_ERROR,
                    format!("failed to serialize tool result: {e}"),
                ),
            },
            Err(tool_err) => JsonRpcResponse::ok(id, tool_result(tool_err.to_string(), true)),
        }
    }
}

/// The MCP `tools/call` result: one text content block and the tool-level
/// `isError` flag (a tool failure is reported here, not as a JSON-RPC error).
fn tool_result(text: impl Into<String>, is_error: bool) -> serde_json::Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text.into()}],
        "isError": is_error
    })
}

fn encode(response: &JsonRpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|e| {
        let fallback = JsonRpcResponse::err(
            serde_json::Value::Null,
            INTERNAL_ERROR,
            format!("Internal error: {e}"),
        );
        serde_json::to_string(&fallback).expect("JsonRpcResponse serialization")
    })
}
