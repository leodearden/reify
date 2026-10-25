//! What counts as a completed MCP `initialize`: the one acceptance rule both
//! [`FusedMemoryClient`](crate::fused_memory_client::FusedMemoryClient) and
//! [`JcodemunchClient`](crate::jcodemunch_client::JcodemunchClient) apply to
//! the decoded `initialize` response before treating a server as a live seam.
//!
//! The rule checks the SHAPE of an MCP 2024-11-05 `InitializeResult`: a
//! JSON-RPC 2.0 envelope whose `result` carries a string `protocolVersion`,
//! an object `capabilities`, and an object `serverInfo` with a string `name`.
//! It checks nothing about identity. Which server answered is the live
//! capstone's question, and a hermetic mock must pass.
//!
//! Pure: it takes the already-decoded response and does no I/O. A 202 or an
//! empty body decodes to `Value::Null` (see [`crate::mcp_wire::decode_body`]),
//! which is [`InitializeRejection::NoBody`] here.

use std::fmt;

use serde_json::Value;

/// The JSON type an `InitializeResult` field must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonKind {
    String,
    Object,
}

impl JsonKind {
    fn admits(self, value: &Value) -> bool {
        match self {
            JsonKind::String => value.is_string(),
            JsonKind::Object => value.is_object(),
        }
    }
}

impl fmt::Display for JsonKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonKind::String => f.write_str("a string"),
            JsonKind::Object => f.write_str("an object"),
        }
    }
}

/// Why a decoded `initialize` response is not a completed MCP handshake: one
/// variant per broken invariant, each carrying the offending value (`None`
/// when the member is absent).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InitializeRejection {
    /// No body at all: `Value::Null`, which is what a 202 or an empty body
    /// decodes to.
    NoBody,
    /// `jsonrpc` is absent or not `"2.0"`.
    NotJsonRpc2 { found: Option<Value> },
    /// `result` is absent or not an object.
    NoResultObject { got: Option<Value> },
    /// A required `InitializeResult` member is absent or of the wrong type.
    /// `field` is its path under `result`.
    MissingField {
        field: &'static str,
        expected: JsonKind,
        got: Option<Value>,
    },
}

/// Renders a member's value for a rejection message.
struct Found<'a>(&'a Option<Value>);

impl fmt::Display for Found<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => f.write_str("absent"),
            Some(value) => write!(f, "{value}"),
        }
    }
}

impl fmt::Display for InitializeRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("initialize: ")?;
        match self {
            Self::NoBody => f.write_str(
                "the response has no body (a 202 or an empty body), so it carries \
                 no InitializeResult",
            )?,
            Self::NotJsonRpc2 { found } => {
                write!(f, "`jsonrpc` must be \"2.0\" (found: {})", Found(found))?
            }
            Self::NoResultObject { got } => {
                write!(f, "`result` must be an object (found: {})", Found(got))?
            }
            Self::MissingField {
                field,
                expected,
                got,
            } => write!(
                f,
                "`result.{field}` must be {expected} (found: {})",
                Found(got)
            )?,
        }
        f.write_str(" — not a live MCP seam")
    }
}

impl std::error::Error for InitializeRejection {}

/// `Ok` iff `response` is a completed MCP handshake (see the module doc).
pub(crate) fn check_initialize_response(response: &Value) -> Result<(), InitializeRejection> {
    if response.is_null() {
        return Err(InitializeRejection::NoBody);
    }
    let jsonrpc = response.get("jsonrpc");
    if jsonrpc.and_then(Value::as_str) != Some("2.0") {
        return Err(InitializeRejection::NotJsonRpc2 {
            found: jsonrpc.cloned(),
        });
    }
    let result = match response.get("result") {
        Some(result) if result.is_object() => result,
        other => {
            return Err(InitializeRejection::NoResultObject {
                got: other.cloned(),
            });
        }
    };
    require(
        result,
        "protocolVersion",
        "protocolVersion",
        JsonKind::String,
    )?;
    require(result, "capabilities", "capabilities", JsonKind::Object)?;
    let server_info = require(result, "serverInfo", "serverInfo", JsonKind::Object)?;
    require(server_info, "name", "serverInfo.name", JsonKind::String)?;
    Ok(())
}

/// `parent[key]`, if present and of kind `expected`; otherwise the
/// [`InitializeRejection::MissingField`] naming it by its `field` path.
fn require<'a>(
    parent: &'a Value,
    key: &str,
    field: &'static str,
    expected: JsonKind,
) -> Result<&'a Value, InitializeRejection> {
    match parent.get(key) {
        Some(value) if expected.admits(value) => Ok(value),
        got => Err(InitializeRejection::MissingField {
            field,
            expected,
            got: got.cloned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The `initialize` reply a live fused-memory serve sent on 2026-10-07
    /// (localhost:8002), minus its `instructions` prose. jcodemunch runs the
    /// same Python MCP SDK `InitializeResult` model.
    fn live_fused_memory_reply() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "experimental": {},
                    "prompts": {"listChanged": false},
                    "resources": {"subscribe": false, "listChanged": false},
                    "tools": {"listChanged": false}
                },
                "serverInfo": {"name": "Fused Memory", "version": "1.27.0"}
            }
        })
    }

    #[test]
    fn a_live_servers_initialize_result_is_accepted() {
        assert_eq!(
            check_initialize_response(&live_fused_memory_reply()),
            Ok(())
        );
    }

    #[test]
    fn null_is_no_body() {
        assert_eq!(
            check_initialize_response(&Value::Null),
            Err(InitializeRejection::NoBody)
        );
    }

    #[test]
    fn an_absent_or_wrong_jsonrpc_version_is_not_json_rpc_2() {
        assert_eq!(
            check_initialize_response(&json!({})),
            Err(InitializeRejection::NotJsonRpc2 { found: None })
        );
        let mut reply = live_fused_memory_reply();
        reply["jsonrpc"] = json!("1.0");
        assert_eq!(
            check_initialize_response(&reply),
            Err(InitializeRejection::NotJsonRpc2 {
                found: Some(json!("1.0"))
            })
        );
    }

    #[test]
    fn an_absent_or_non_object_result_is_no_result_object() {
        assert_eq!(
            check_initialize_response(&json!({"jsonrpc": "2.0", "id": 1})),
            Err(InitializeRejection::NoResultObject { got: None })
        );
        assert_eq!(
            check_initialize_response(&json!({"jsonrpc": "2.0", "id": 1, "result": "ok"})),
            Err(InitializeRejection::NoResultObject {
                got: Some(json!("ok"))
            })
        );
    }

    /// Each required member, removed or mistyped in turn from an otherwise
    /// live reply, is reported by its own path.
    #[test]
    fn each_required_member_is_a_missing_field_by_its_own_path() {
        struct Case {
            parent: &'static str,
            key: &'static str,
            field: &'static str,
            replacement: Option<Value>,
            expected: JsonKind,
        }
        let case = |parent, key, field, replacement, expected| Case {
            parent,
            key,
            field,
            replacement,
            expected,
        };
        let cases = [
            case(
                "/result",
                "protocolVersion",
                "protocolVersion",
                None,
                JsonKind::String,
            ),
            case(
                "/result",
                "protocolVersion",
                "protocolVersion",
                Some(json!(20241105)),
                JsonKind::String,
            ),
            case(
                "/result",
                "capabilities",
                "capabilities",
                None,
                JsonKind::Object,
            ),
            case(
                "/result",
                "capabilities",
                "capabilities",
                Some(json!([])),
                JsonKind::Object,
            ),
            case(
                "/result",
                "serverInfo",
                "serverInfo",
                None,
                JsonKind::Object,
            ),
            case(
                "/result",
                "serverInfo",
                "serverInfo",
                Some(json!("jcodemunch")),
                JsonKind::Object,
            ),
            case(
                "/result/serverInfo",
                "name",
                "serverInfo.name",
                None,
                JsonKind::String,
            ),
            case(
                "/result/serverInfo",
                "name",
                "serverInfo.name",
                Some(Value::Null),
                JsonKind::String,
            ),
        ];
        for c in cases {
            let mut reply = live_fused_memory_reply();
            let parent = reply
                .pointer_mut(c.parent)
                .and_then(Value::as_object_mut)
                .expect("parent is an object in the live reply");
            match &c.replacement {
                None => parent.remove(c.key),
                Some(value) => parent.insert(c.key.to_string(), value.clone()),
            };
            assert_eq!(
                check_initialize_response(&reply),
                Err(InitializeRejection::MissingField {
                    field: c.field,
                    expected: c.expected,
                    got: c.replacement.clone(),
                }),
                "{}/{} replaced by {:?}",
                c.parent,
                c.key,
                c.replacement
            );
        }
    }

    /// The message names the handshake leg, the broken invariant and the
    /// offending value, so a breadcrumb is actionable on its own.
    #[test]
    fn the_message_names_the_leg_the_invariant_and_the_value() {
        let message = InitializeRejection::MissingField {
            field: "serverInfo",
            expected: JsonKind::Object,
            got: Some(json!("jcodemunch")),
        }
        .to_string();
        assert_eq!(
            message,
            "initialize: `result.serverInfo` must be an object (found: \"jcodemunch\") \
             — not a live MCP seam"
        );
        assert!(
            InitializeRejection::NotJsonRpc2 { found: None }
                .to_string()
                .contains("(found: absent)")
        );
    }
}
