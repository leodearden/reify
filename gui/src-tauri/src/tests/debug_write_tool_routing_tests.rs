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

/// True when the top-level fn `name` reaches a `*_and_refresh_baseline` seam
/// either directly or through exactly ONE delegation hop.
///
/// The depth cap is deliberate, not a shortcut: one hop is exactly what
/// `reify_open_file` needs (`handle_reify_open_file` → `open_path_into_engine`
/// → `open_source_into_engine_and_refresh_baseline`), and an uncapped walk
/// would be a call-graph analyzer — far more machinery than the claim
/// warrants. The cap's failure direction is the safe one: a chain deeper than
/// one hop false-POSITIVES, so a human looks, and it can never false-GREEN.
fn reaches_a_seam(code: &str, name: &str) -> bool {
    let Some(body) = fn_body(code, name) else {
        return false;
    };
    names_a_seam(body, name)
        || identifiers(body)
            .filter(|callee| *callee != name)
            .any(|callee| fn_body(code, callee).is_some_and(|hop| names_a_seam(hop, callee)))
}

/// Every maximal `[A-Za-z0-9_]+` run in `text`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
}

/// Blanks every `//`/`///`/`//!` line comment and `/* … */` block comment,
/// replacing each comment span with an equal number of ASCII spaces so byte
/// offsets, line count and line lengths all survive unchanged — `fn_body`'s
/// column-0 `}` sentinel therefore still means the same thing.
///
/// Stripping must be genuinely correct rather than merely conservative,
/// because the two checks it feeds fail in OPPOSITE directions: leaving a
/// comment in place false-GREENS the seam check (prose naming a seam reads as
/// routing), while blanking too much false-GREENS the private-emit check.
///
/// It is a scanner, not a Rust lexer: it tracks `"` string literals — the
/// only literal in this corpus that can contain a `//` (`"http://…"`) — with
/// `\` escapes, across line boundaries. Raw strings (`r"…"`, `r#"…"#`) and
/// the pathological `'"'` char literal, neither of which occurs in
/// `debug_server.rs`, are not special-cased.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut in_string = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if in_string {
            match rest[0] {
                b'\\' => {
                    // Copy the backslash and the char it escapes together, so
                    // an escaped quote (`\"`) cannot close the string.
                    let width = 1 + source[i + 1..].chars().next().map_or(0, char::len_utf8);
                    out.push_str(&source[i..i + width]);
                    i += width;
                    continue;
                }
                b'"' => in_string = false,
                _ => {}
            }
        } else if rest.starts_with(b"//") {
            let end = source[i..].find('\n').map_or(source.len(), |n| i + n);
            out.push_str(&" ".repeat(end - i));
            i = end;
            continue;
        } else if rest.starts_with(b"/*") {
            let end = source[i..]
                .find("*/")
                .map_or(source.len(), |n| i + n + "*/".len());
            for c in source[i..end].chars() {
                out.push(if c == '\n' { '\n' } else { ' ' });
                for _ in 1..c.len_utf8() {
                    out.push(' ');
                }
            }
            i = end;
            continue;
        } else if rest[0] == b'"' {
            in_string = true;
        }
        let width = source[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&source[i..i + width]);
        i += width;
    }
    out
}

/// Every INV-GUI-2 violation in `source`, one per (write tool, defect).
fn write_tool_bypasses(source: &str) -> Vec<Bypass> {
    // Stripped ONCE here, at the single entry point, so every helper below
    // sees code-only text and none can independently forget to.
    let code = strip_comments(source);
    dispatch_arms(&code)
        .into_iter()
        .filter(|(_, handler)| !reaches_a_seam(&code, handler))
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

/// Both compliant shapes the real file uses, side by side:
///
/// - `reify_set_parameter` names its seam directly in the handler body — the
///   shape four of the five write tools take.
/// - `reify_open_file` reaches the SAME refresh ONE HOP away, through
///   `open_path_into_engine` (debug_server.rs:1701 → :1525 → :1640). This is
///   the one stated exception δ documented for θ, and the only shape that
///   forces the checker to trace a delegation.
const COMPLIANT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_open_file" => handle_reify_open_file(state, params).await,
        "reify_set_parameter" => handle_reify_set_parameter(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_open_file(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let raw_path = open_file_path_param(&params)?;
    let (frontend, content) = open_path_into_engine(state, &raw_path).await?;
    frontend_ok(frontend, "open_file")?;
    Ok(reify_open_file_envelope(&content))
}

async fn open_path_into_engine(
    state: &DebugServerState,
    raw_path: &str,
) -> Result<(Value, String), String> {
    let path = canonicalize_open_path(raw_path)?;
    let gui_state =
        open_source_into_engine_and_refresh_baseline(&state.engine, &state.last_state, &path)
            .await?;
    let frontend = push_gui_state(&state.debug_bridge, &gui_state, None).await?;
    Ok((frontend, gui_state.source.clone()))
}

async fn handle_reify_set_parameter(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let (cell_id, value) = reify_set_parameter_params(&params)?;
    let gs = reify_set_parameter_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &cell_id,
        &value,
    )
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_set_parameter_envelope(None, None, vec![]))
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

#[test]
fn compliant_fixtures_in_both_seam_shapes_are_clean() {
    assert_eq!(write_tool_bypasses(COMPLIANT_SOURCE), vec![]);
}

/// THE gate: the mechanized corpus sweep against the real `debug_server.rs`.
#[test]
fn every_debug_write_tool_routes_through_the_delta_choke_point() {
    let source = debug_server_source();

    // NON-VACUITY FLOOR — checked before the real assertion so a moved file
    // or a parser that silently stopped matching reds instead of passing
    // vacuously (same shape as `every_test_module_file_is_declared`'s floor).
    // It also fail-closes the split debug_server.rs weighed at :1984-2000: if
    // the write-tool cluster moves to its own module the arms vanish here,
    // this fires, and someone must re-point the checker.
    let arms = dispatch_arms(&strip_comments(&source));
    assert!(
        arms.len() >= 5,
        "dispatch-arm scan of debug_server.rs found only {} `reify_*` tool(s) (expected >= 5) — \
         the checker may be reading the wrong file, the arm parser may have stopped matching, \
         or the write tools may have moved to another module",
        arms.len()
    );

    let bypasses = write_tool_bypasses(&source);
    if bypasses.is_empty() {
        return;
    }

    let report = bypasses
        .iter()
        .map(|b| {
            format!(
                "  {} -> {} ({:?}) in gui/src-tauri/src/debug_server.rs",
                b.tool, b.handler, b.kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    if routing_check_bypassed() {
        eprintln!("[REIFY_INV_GUI_2_BYPASS] DOWNGRADED to warn:\n{report}");
        return;
    }

    panic!(
        "INV-GUI-2: {} `reify_*` write tool(s) do not reach the delta baseline through one of \
         the two shared `*_and_refresh_baseline` seams:\n{report}\n\n\
         (set REIFY_INV_GUI_2_BYPASS=1 to downgrade this to a warning as a break-glass \
         escape hatch)",
        bypasses.len()
    );
}
