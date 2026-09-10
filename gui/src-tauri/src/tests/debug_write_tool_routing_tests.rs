//! INV-GUI-2 (`docs/invariants.md`) structural gate for the AI/MCP entry
//! point: every `reify_*` write tool reaches the delta baseline through one
//! of the two shared `*_and_refresh_baseline` seams, never through a private
//! emit of its own.
//!
//! The claim this mechanizes is stated in prose in two places and is NOT
//! restated here — see `gui/src-tauri/src/debug_server.rs` point (a) on
//! `write_on_engine_and_refresh_baseline`, and the "Two seams, ONE stated
//! exception" section of `docs/debug-mcp-contract.md`.

/// One way a `reify_*` write tool can break INV-GUI-2.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum BypassKind {
    /// The handler never reaches a `*_and_refresh_baseline` seam, so the
    /// delta baseline silently goes stale after its write.
    NoBaselineRefresh,
}

/// A single INV-GUI-2 violation, reported structurally so callers assert on
/// values rather than on a rendered message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Bypass {
    tool: String,
    handler: String,
    kind: BypassKind,
}

/// Parses the `"reify_<tool>" => <handler>(` dispatch arms out of `source`,
/// returning `(tool, handler)` in source order.
///
/// The write-tool set is ENUMERATED here rather than hardcoded, so a sixth
/// `reify_*` tool is picked up automatically and must route or go red — which
/// is the gap INV-GUI-2 exists to close. There is deliberately no read-only
/// exemption set: a future read-only `reify_*` tool reds until someone
/// classifies it consciously.
fn dispatch_arms(source: &str) -> Vec<(String, String)> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("\"reify_")?;
            let (tool_suffix, rest) = rest.split_once('"')?;
            let rest = rest.trim_start().strip_prefix("=>")?;
            let handler = rest.trim_start();
            let end = handler.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
            if handler[end..].starts_with('(') && end > 0 {
                Some((format!("reify_{tool_suffix}"), handler[..end].to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Parses the fn name out of a (trimmed) line that looks like a fn signature,
/// stripping leading `pub`/`pub(...)`/`async`/`unsafe` modifiers first.
/// Returns `None` if the line isn't a fn signature. Modelled on
/// `crates/reify-eval/tests/version_id_discipline_gate.rs`'s `parse_fn_name`;
/// the name scan stops at the first non-identifier char, so a generic
/// signature (`fn write_on_engine_and_refresh_baseline<F>(`) parses too.
fn parse_fn_name(trimmed: &str) -> Option<&str> {
    let mut rest = trimmed;
    while !rest.starts_with("fn ") {
        rest = if let Some(r) = rest.strip_prefix("pub(") {
            r[r.find(')')? + 1..].trim_start()
        } else if let Some(r) = rest.strip_prefix("pub ") {
            r.trim_start()
        } else if let Some(r) = rest.strip_prefix("async ") {
            r.trim_start()
        } else if let Some(r) = rest.strip_prefix("unsafe ") {
            r.trim_start()
        } else {
            return None;
        };
    }
    let rest = rest["fn ".len()..].trim_start();
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    (end > 0).then(|| &rest[..end])
}

/// Returns the source slice of the TOP-LEVEL fn named `name`, from its
/// signature line through the closing brace.
///
/// The terminator is the next line that is exactly `}` in column 0. That is
/// sound for `debug_server.rs`, which indents every nested block, so no inner
/// brace can reach column 0; a top-level fn's own closing brace is the first
/// that does. A file violating that convention truncates the body early,
/// which loses seam matches and therefore false-POSITIVES — red, never
/// silently green.
fn fn_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let mut start: Option<usize> = None;
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        match start {
            None => {
                if parse_fn_name(line.trim_end()) == Some(name) {
                    start = Some(offset);
                }
            }
            Some(begin) => {
                if line.trim_end() == "}" {
                    return Some(&source[begin..offset + line.len()]);
                }
            }
        }
        offset += line.len();
    }
    None
}

/// True when `body` names any identifier ending in `_and_refresh_baseline`
/// other than `own_name` (a fn's own signature line is part of its body).
fn names_a_seam(body: &str, own_name: &str) -> bool {
    identifiers(body).any(|id| id.ends_with("_and_refresh_baseline") && id != own_name)
}

/// Every maximal `[A-Za-z0-9_]+` run in `text`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

/// Every INV-GUI-2 violation in `source`, one per (write tool, defect).
fn write_tool_bypasses(source: &str) -> Vec<Bypass> {
    dispatch_arms(source)
        .into_iter()
        .filter(|(_, handler)| {
            !fn_body(source, handler).is_some_and(|body| names_a_seam(body, handler))
        })
        .map(|(tool, handler)| Bypass {
            tool,
            handler,
            kind: BypassKind::NoBaselineRefresh,
        })
        .collect()
}

/// A synthetic `debug_server.rs` excerpt whose `reify_set_parameter` handler
/// mutates the engine directly and pushes to the frontend without ever
/// reaching a `*_and_refresh_baseline` seam. This is the defect INV-GUI-2
/// exists to catch; the checker must flag it.
const BYPASSING_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "engine_state" => handle_engine_state(state).await,
        "reify_set_parameter" => handle_reify_set_parameter(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_set_parameter(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let (cell_id, value) = reify_set_parameter_params(&params)?;
    let gs = run_on_engine(&state.engine, move |s| {
        s.apply_param_to_source_str(&cell_id, &value)
    })
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_set_parameter_envelope(None, None, vec![]))
}
"#;

/// A fixture whose `handle_reify_export` NAMES both seams in prose — a doc
/// comment and a line comment — while its body bypasses them entirely.
///
/// Not hypothetical: the real `handle_reify_set_parameter`'s doc comment
/// literally contains "1. `reify_set_parameter_on_engine_and_refresh_baseline`
/// — splices the …". A checker that greps raw text greens any handler that
/// merely mentions a seam while bypassing it in code.
const COMMENT_ONLY_MENTION_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_export" => handle_reify_export(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

/// Export the current model.
///
/// Flow:
///  1. `reify_export_on_engine_and_refresh_baseline` — writes the file and
///     refreshes the delta baseline.
///  2. push the rebuilt `GuiState` to the frontend.
async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    // routes through write_on_engine_and_refresh_baseline
    let gs = run_on_engine(&state.engine, move |s| s.export(&format, &output_path)).await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}
"#;

#[test]
fn bypassing_fixture_is_flagged() {
    assert_eq!(
        write_tool_bypasses(BYPASSING_SOURCE),
        vec![Bypass {
            tool: "reify_set_parameter".to_string(),
            handler: "handle_reify_set_parameter".to_string(),
            kind: BypassKind::NoBaselineRefresh,
        }],
    );
}

#[test]
fn seam_named_only_in_a_comment_does_not_count() {
    assert_eq!(
        write_tool_bypasses(COMMENT_ONLY_MENTION_SOURCE),
        vec![Bypass {
            tool: "reify_export".to_string(),
            handler: "handle_reify_export".to_string(),
            kind: BypassKind::NoBaselineRefresh,
        }],
    );
}
