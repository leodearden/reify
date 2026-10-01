//! The one MCP streamable-HTTP response-body decode, shared by
//! [`FusedMemoryClient`](crate::fused_memory_client::FusedMemoryClient),
//! [`JcodemunchClient`](crate::jcodemunch_client::JcodemunchClient) and the
//! live-serve test harnesses.
//!
//! Pure: it takes the response's `Content-Type` and its already-read body and
//! does no I/O. Envelope policy — the 202 short-circuit, reading the body, the
//! JSON-RPC `error` check — stays with the caller.

use std::fmt;

use serde_json::Value;

/// Why an MCP response body could not be decoded into a JSON-RPC message.
#[derive(Debug)]
pub enum BodyDecodeError {
    /// An SSE-framed body carried no `data:` line at all.
    SseNoDataLine { body: String },
    /// An SSE-framed body's `data:` payload is not valid JSON.
    SseDataParse {
        source: serde_json::Error,
        body: String,
    },
    /// A non-SSE body is not valid JSON.
    JsonBodyParse {
        source: serde_json::Error,
        body: String,
    },
}

impl fmt::Display for BodyDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SseNoDataLine { body } => write!(f, "no SSE data line in response: {body}"),
            Self::SseDataParse { source, body } => {
                write!(f, "SSE data parse: {source}; body={body}")
            }
            Self::JsonBodyParse { source, body } => write!(f, "body parse: {source}; body={body}"),
        }
    }
}

impl std::error::Error for BodyDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SseNoDataLine { .. } => None,
            Self::SseDataParse { source, .. } | Self::JsonBodyParse { source, .. } => Some(source),
        }
    }
}

/// Decode one MCP streamable-HTTP response body into its JSON-RPC message,
/// choosing SSE or bare-JSON framing from `content_type`.
pub fn decode_body(content_type: &str, body: &str) -> Result<Value, BodyDecodeError> {
    if content_type.contains("text/event-stream") {
        let data = body
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .ok_or_else(|| BodyDecodeError::SseNoDataLine {
                body: body.to_owned(),
            })?;
        serde_json::from_str(data.trim()).map_err(|source| BodyDecodeError::SseDataParse {
            source,
            body: body.to_owned(),
        })
    } else if body.is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_str(body).map_err(|source| BodyDecodeError::JsonBodyParse {
            source,
            body: body.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SSE: &str = "text/event-stream";
    const JSON: &str = "application/json";

    #[test]
    fn sse_frame_decodes_its_data_line_past_the_event_line() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(
            decode_body(SSE, body).unwrap(),
            json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})
        );
    }

    #[test]
    fn sse_content_type_with_parameters_still_routes_to_sse() {
        let body = "event: message\ndata: {\"a\":1}\n\n";
        assert_eq!(
            decode_body("text/event-stream; charset=utf-8", body).unwrap(),
            json!({"a": 1})
        );
    }

    #[test]
    fn sse_data_line_without_a_space_after_the_colon_decodes() {
        assert_eq!(
            decode_body(SSE, "data:{\"a\":1}\n").unwrap(),
            json!({"a": 1})
        );
    }

    #[test]
    fn sse_malformed_data_payload_is_refused() {
        let err = decode_body(SSE, "event: message\ndata: {not json\n\n").unwrap_err();
        assert!(
            matches!(err, BodyDecodeError::SseDataParse { .. }),
            "got {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.starts_with("SSE data parse:"), "got {msg:?}");
        assert!(msg.contains("body=event: message"), "got {msg:?}");
    }

    #[test]
    fn sse_frame_with_no_data_line_is_refused() {
        let err = decode_body(SSE, "event: message\n\n").unwrap_err();
        assert!(
            matches!(err, BodyDecodeError::SseNoDataLine { .. }),
            "got {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.starts_with("no SSE data line in response:"),
            "got {msg:?}"
        );
    }

    #[test]
    fn empty_sse_body_is_refused_not_null() {
        let err = decode_body(SSE, "").unwrap_err();
        assert!(
            matches!(err, BodyDecodeError::SseNoDataLine { .. }),
            "got {err:?}"
        );
    }

    // Deliberately pins today's first-line-wins behaviour; spec-conformant
    // multi-line `data:` accumulation is #6288, which replaces this test.
    #[test]
    fn sse_first_data_line_wins_until_task_6288() {
        let body = "data: {\"first\":1}\ndata: {\"second\":2}\n\n";
        assert_eq!(decode_body(SSE, body).unwrap(), json!({"first": 1}));
    }

    #[test]
    fn json_body_decodes_verbatim() {
        let body = "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"tools\":[]}}";
        assert_eq!(
            decode_body(JSON, body).unwrap(),
            json!({"jsonrpc": "2.0", "id": 7, "result": {"tools": []}})
        );
    }

    #[test]
    fn empty_non_sse_body_decodes_to_null() {
        assert_eq!(decode_body(JSON, "").unwrap(), Value::Null);
        assert_eq!(decode_body("", "").unwrap(), Value::Null);
    }

    #[test]
    fn whitespace_only_non_sse_body_is_refused_not_null() {
        let err = decode_body(JSON, " \n").unwrap_err();
        assert!(
            matches!(err, BodyDecodeError::JsonBodyParse { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn malformed_json_body_is_refused() {
        let err = decode_body(JSON, "{not json").unwrap_err();
        assert!(
            matches!(err, BodyDecodeError::JsonBodyParse { .. }),
            "got {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.starts_with("body parse:"), "got {msg:?}");
    }

    #[test]
    fn json_rpc_error_envelope_is_decoded_not_refused() {
        let envelope = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32000, "message": "x"}
        });
        let json_framed = envelope.to_string();
        let sse_framed = format!("event: message\ndata: {envelope}\n\n");
        for (content_type, body) in [(JSON, json_framed.as_str()), (SSE, sse_framed.as_str())] {
            let decoded = decode_body(content_type, body)
                .unwrap_or_else(|e| panic!("{content_type} framing refused the envelope: {e}"));
            assert_eq!(decoded, envelope, "{content_type} framing");
        }
    }
}
