//! INV-GUI-2 (`docs/invariants.md`) structural gate for the AI/MCP entry
//! point: every `reify_*` write tool reaches the delta baseline through one
//! of the two shared `*_and_refresh_baseline` seams, never through a private
//! emit of its own.
//!
//! The claim this mechanizes is stated in prose in two places and is NOT
//! restated here — see `gui/src-tauri/src/debug_server.rs:1867-1908`, point
//! (a) on `write_on_engine_and_refresh_baseline`, and the "Two seams, ONE
//! stated exception" section of `docs/debug-mcp-contract.md`. Note the shape
//! of the claim: "one of the two shared seams", NOT "all five route through
//! `write_on_engine_and_refresh_baseline`" — `reify_open_file` reaches the
//! same refresh through `open_source_into_engine_and_refresh_baseline`, for
//! the #5193 lock-ordering reason point (d) gives.
//!
//! This reads `debug_server.rs` as TEXT and links nothing, so unlike
//! `debug_boundary_tests` — which builds a real `EngineSession` — it carries
//! NO `#[cfg(feature = "gui")]` gate. That is deliberate: it runs in the
//! DEFAULT test pass rather than only in `verify.sh`'s conditionally-emitted
//! `--features gui` arm, so the architecture gate cannot be silently skipped
//! on a run that does not touch the GUI crate — exactly the run during which
//! someone might add a bypassing tool elsewhere.
//!
//! POSTURE: ships default-ASSERT. The task called for contract → warn-mode
//! corpus sweep → enforce; the sweep was performed at plan time (5/5 routed,
//! clean) and is re-performed mechanically by
//! `every_debug_write_tool_routes_through_the_delta_choke_point` on every
//! run, so a warn-only default would emit no signal on a green tree and defer
//! the leaf indefinitely. `REIFY_INV_GUI_2_BYPASS=1` is the break-glass
//! escape hatch, mirroring `REIFY_MAIN_GATE_BYPASS` (CLAUDE.md), and it is
//! genuinely live: it lets someone mid-refactor land a legitimate third seam
//! by setting a knob instead of deleting the guard. A
//! `REIFY_INV_GUI_2_ENFORCE` alias is deliberately NOT provided — against a
//! default-assert shipped state it would be a no-op, and an unreachable knob
//! is worse than an absent one. Same posture convention as
//! `crates/reify-eval/tests/harness_cache/snapshot_cache_divergence_gate.rs`.

/// One way a `reify_*` write tool can break INV-GUI-2.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum BypassKind {
    /// The handler never reaches a `*_and_refresh_baseline` seam, so the
    /// delta baseline silently goes stale after its write.
    NoBaselineRefresh,
    /// The handler emits state on its own, alongside the shared seam —
    /// the second emission path `debug_server.rs:1888` forbids.
    PrivateEmit,
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

/// True when `body` emits state on its own rather than leaving emission to
/// the shared seam — an `emit_delta` identifier, or any `.emit(` call.
fn emits_privately(body: &str) -> bool {
    identifiers(body).any(|id| id == "emit_delta") || body.contains(".emit(")
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

/// Names every top-level fn in `code` whose name ends `_and_refresh_baseline`,
/// in source order. `code` must already be comment-stripped.
fn seam_fns(code: &str) -> Vec<&str> {
    code.lines()
        .filter_map(|line| parse_fn_name(line.trim_end()))
        .filter(|name| name.ends_with("_and_refresh_baseline"))
        .collect()
}

/// Names the `*_and_refresh_baseline` fns that never actually refresh the
/// baseline, sorted.
///
/// Without this, [`write_tool_bypasses`] would rest on a NAME: any fn called
/// `…_and_refresh_baseline` would satisfy it, so a seam whose name lies would
/// green the gate falsely. A seam is accepted if its body reaches
/// `compute_delta` directly, or — one hop, same cap and same fail-closed
/// direction as [`reaches_a_seam`] — if it delegates to another
/// `*_and_refresh_baseline` fn in the same source that does.
fn unrefreshing_seams(source: &str) -> Vec<String> {
    let code = strip_comments(source);
    let refreshes = |name: &str| {
        fn_body(&code, name).is_some_and(|body| identifiers(body).any(|id| id == "compute_delta"))
    };
    let mut liars: Vec<String> = seam_fns(&code)
        .into_iter()
        .filter(|name| {
            !refreshes(name)
                && !fn_body(&code, name).is_some_and(|body| {
                    identifiers(body).any(|id| {
                        id != *name && id.ends_with("_and_refresh_baseline") && refreshes(id)
                    })
                })
        })
        .map(str::to_string)
        .collect();
    liars.sort();
    liars
}

/// Every INV-GUI-2 violation in `source`, one per (write tool, defect).
fn write_tool_bypasses(source: &str) -> Vec<Bypass> {
    // Stripped ONCE here, at the single entry point, so every helper below
    // sees code-only text and none can independently forget to.
    let code = strip_comments(source);
    let mut bypasses: Vec<Bypass> = dispatch_arms(&code)
        .into_iter()
        .flat_map(|(tool, handler)| {
            let body = fn_body(&code, &handler).unwrap_or("");
            // The two defects are INDEPENDENT: a handler can route correctly
            // and still emit privately, and collapsing them would hide one.
            let kinds = [
                (!reaches_a_seam(&code, &handler)).then_some(BypassKind::NoBaselineRefresh),
                emits_privately(body).then_some(BypassKind::PrivateEmit),
            ];
            kinds.into_iter().flatten().map(move |kind| Bypass {
                tool: tool.clone(),
                handler: handler.clone(),
                kind,
            })
        })
        .collect();
    // Sorted so failure output is stable across runs; `Bypass`'s derived Ord
    // is (tool, handler, kind).
    bypasses.sort();
    bypasses
}

/// Reads the real `debug_server.rs` this gate is asserted against.
///
/// Located via [`super::gui_crate_manifest_dir`], which prefers the RUNTIME
/// `CARGO_MANIFEST_DIR` over the compile-time `env!()` bake — the bake goes
/// stale when a seeded warm-lane `target/` is reused from a since-deleted
/// worktree (esc-4906-57).
fn debug_server_source() -> String {
    let path = std::path::Path::new(&super::gui_crate_manifest_dir()).join("src/debug_server.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} for the INV-GUI-2 gate: {e}",
            path.display()
        )
    })
}

/// Break-glass: `REIFY_INV_GUI_2_BYPASS=1` downgrades the corpus assertion to
/// a warn (prints offenders, does not fail), mirroring `REIFY_MAIN_GATE_BYPASS`.
/// See the file header for the ENFORCE/BYPASS symmetry rationale.
fn routing_check_bypassed() -> bool {
    std::env::var("REIFY_INV_GUI_2_BYPASS").is_ok_and(|v| v == "1")
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

/// A handler that DOES route through a seam but then emits a second time on
/// its own — the private emission path `debug_server.rs:1888` forbids in as
/// many words: "Do NOT add a second emit path here or in any caller".
const PRIVATE_EMIT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_update_source" => handle_reify_update_source(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_update_source(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    let source = reify_update_source_params(&params)?;
    let gs = reify_update_source_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &source,
    )
    .await?;
    let delta = crate::diff::compute_delta(&state.last_state, &gs);
    emit_delta(&state.app, &delta);
    state.app.emit("state-delta", &delta).ok();
    Ok(reify_update_source_envelope(&gs))
}
"#;

/// A handler that looks perfectly compliant — it calls a
/// `*_and_refresh_baseline` fn — where that fn never reaches `compute_delta`.
/// Without this check the whole gate would rest on a NAME rather than on
/// behaviour, and a seam whose name lies would green it falsely.
const LYING_SEAM_SOURCE: &str = r#"
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

async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    let gs = reify_export_on_engine_and_refresh_baseline(
        &state.engine,
        &state.last_state,
        &format,
        &output_path,
    )
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}

pub async fn reify_export_on_engine_and_refresh_baseline(
    engine: &Arc<Mutex<EngineSession>>,
    last_state: &std::sync::Mutex<Option<crate::types::GuiState>>,
    format: &str,
    output_path: &str,
) -> Result<crate::types::GuiState, String> {
    let format = format.to_owned();
    let output_path = output_path.to_owned();
    let _ = last_state;
    run_on_engine(engine, move |s| {
        let fmt = crate::commands::parse_export_format(&format)?;
        s.export(fmt, std::path::Path::new(&output_path))?;
        s.build_gui_state()
    })
    .await
}
"#;

/// A fixture whose `ToolDef` registry advertises BOTH `reify_alpha` and
/// `reify_beta` while `dispatch_tool` carries a literal arm for `reify_alpha`
/// only — `reify_beta` falls through to the `_ =>` frontend-delegation
/// catch-all, the shape the live `dispatch_tool` has at
/// `debug_server.rs:1329`, so it is dispatched but never seen by the arm scan.
///
/// No widening of the arm parser can reach this shape: there is no arm to
/// read. Only a second, independent enumeration of the tool set catches it.
const ADVERTISED_BUT_UNDISPATCHED_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_alpha",
            description: "Set alpha.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_beta",
            description: "Set beta.",
            input_schema: json!({ "type": "object" }),
        },
    ]
}

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_alpha" => handle_reify_alpha(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_alpha(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let value = reify_alpha_params(&params)?;
    let gs = write_on_engine_and_refresh_baseline(&state.engine, &state.last_state, move |s| {
        s.set_alpha(&value)
    })
    .await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_alpha_envelope(&gs))
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
    // It also fail-closes the split debug_server.rs weighed under "WHY THE
    // CLUSTER LIVES IN THIS FILE" (:1987-2006): if
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

#[test]
fn no_write_tool_handler_emits_privately() {
    assert_eq!(
        write_tool_bypasses(PRIVATE_EMIT_SOURCE),
        vec![Bypass {
            tool: "reify_update_source".to_string(),
            handler: "handle_reify_update_source".to_string(),
            kind: BypassKind::PrivateEmit,
        }],
    );

    // The real file is clean, and is only clean because step-4 strips
    // comments: its ONLY textual `emit_delta` occurrences are the doc
    // comments at :1612 and :1888, so a checker reading raw text would
    // false-positive right here.
    let private_emits: Vec<Bypass> = write_tool_bypasses(&debug_server_source())
        .into_iter()
        .filter(|b| b.kind == BypassKind::PrivateEmit)
        .collect();
    assert_eq!(private_emits, vec![]);
}

#[test]
fn every_refresh_baseline_seam_actually_refreshes() {
    assert_eq!(
        unrefreshing_seams(LYING_SEAM_SOURCE),
        vec!["reify_export_on_engine_and_refresh_baseline".to_string()],
    );

    let source = debug_server_source();

    // NON-VACUITY FLOOR — a renamed seam family or a broken fn-signature
    // parser must red here, not pass by finding nothing to check. Seven seams
    // exist today: write_on_engine_*, open_source_into_engine_* and
    // set_fea_case_on_engine_* reach compute_delta directly, and the four
    // reify_*_on_engine_* reach it via write_on_engine_and_refresh_baseline.
    let code = strip_comments(&source);
    let seams = seam_fns(&code);
    assert!(
        seams.len() >= 3,
        "seam scan of debug_server.rs found only {} `*_and_refresh_baseline` fn(s) \
         (expected >= 3) — the seam family may have been renamed, or the fn-signature \
         parser may have stopped matching: {seams:?}",
        seams.len()
    );

    assert_eq!(
        unrefreshing_seams(&source),
        Vec::<String>::new(),
        "a fn named `*_and_refresh_baseline` never reaches `crate::diff::compute_delta`, \
         so INV-GUI-2's routing check would be resting on that name rather than on behaviour"
    );
}

/// The soundness net for the arm scan: a tool the scan CANNOT SEE must red as
/// a set difference, never vanish. The registry is an independent textual
/// enumeration of the same set, so whatever the reason an arm is unreadable —
/// including reasons nobody has thought of — the tool still shows up here.
#[test]
fn advertised_tools_must_all_appear_in_the_dispatch_scan() {
    assert_eq!(
        unenumerated_tools(ADVERTISED_BUT_UNDISPATCHED_SOURCE),
        vec!["reify_beta".to_string()],
    );
}
