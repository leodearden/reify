//! The synthetic `debug_server.rs` corpus `debug_write_tool_routing_tests`
//! runs against: one fixture per defect or compliance SHAPE, each pinning a
//! claim the INV-GUI-2 checker makes about real source it cannot host.
//!
//! Split from the checker rather than inlined beside it because the two vary
//! for different reasons — a fixture is added when a new SHAPE of Rust shows
//! up, the scanner changes when a new PROPERTY is asserted — and because
//! together they made one file too large to read. The cost is that a fixture
//! no longer sits beside the check it constrains; each doc comment therefore
//! names the test that consumes it, so neither file has to be read to
//! understand the other.

/// A synthetic `debug_server.rs` excerpt whose `reify_set_parameter` handler
/// mutates the engine directly and pushes to the frontend without ever
/// reaching a `*_and_refresh_baseline` seam. This is the defect INV-GUI-2
/// exists to catch; the checker must flag it.
///
/// Consumed by
/// `bypassing_fixture_is_flagged`,
/// `the_break_glass_knob_downgrades_a_real_bypass_to_a_warn`.
pub(super) const BYPASSING_SOURCE: &str = r#"
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
/// comment and a line comment — while its body bypasses them entirely. Not
/// hypothetical: the real `handle_reify_set_parameter`'s doc comment literally
/// names its seam, so a checker over raw text greens any handler that merely
/// mentions one.
///
/// Consumed by `seam_named_only_in_a_comment_does_not_count`.
pub(super) const COMMENT_ONLY_MENTION_SOURCE: &str = r#"
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

/// A fixture reaching the same hole as [`COMMENT_ONLY_MENTION_SOURCE`]
/// through STRING LITERALS instead of prose, in both directions at once:
///
/// - `handle_reify_save_file` names a seam only inside a `tracing` message
///   while bypassing it in code — a false GREEN, the direction this module
///   must never have. A span name, an error string or a `json!` field naming
///   the seam being removed is an ordinary edit.
/// - `handle_reify_export` routes correctly and merely mentions `.emit(` in a
///   log string — a false RED, merely noisy, but fixed by the same blanking.
///
/// Consumed by `seam_named_only_in_a_string_does_not_count`.
pub(super) const STRING_ONLY_MENTION_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_save_file" => handle_reify_save_file(state, params).await,
        "reify_export" => handle_reify_export(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}

async fn handle_reify_save_file(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let path = reify_save_file_params(&params)?;
    tracing::debug!("bypassing write_on_engine_and_refresh_baseline for speed");
    let gs = run_on_engine(&state.engine, move |s| s.save_to(&path)).await?;
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_save_file_envelope(&path))
}

async fn handle_reify_export(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let (format, output_path) = reify_export_params(&params)?;
    let gs = write_on_engine_and_refresh_baseline(&state.engine, &state.last_state, move |s| {
        s.export(&format, &output_path)
    })
    .await?;
    tracing::debug!("the frontend push replaced a state.app.emit( call here");
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_export_envelope(&output_path))
}
"#;

/// Both compliant shapes the real file uses, side by side:
///
/// - `reify_set_parameter` names its seam directly in the handler body — the
///   shape four of the five write tools take.
/// - `reify_open_file` reaches the SAME refresh ONE HOP away
///   (`handle_reify_open_file` → `open_path_into_engine` →
///   `open_source_into_engine_and_refresh_baseline`). This is the one stated
///   exception δ documented for θ, and the only shape that forces the checker
///   to trace a delegation.
///
/// Consumed by
/// `compliant_fixtures_in_both_seam_shapes_are_clean`,
/// `the_break_glass_knob_downgrades_a_real_bypass_to_a_warn`.
pub(super) const COMPLIANT_SOURCE: &str = r#"
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
/// its own — the private emission path point (b) on
/// `write_on_engine_and_refresh_baseline` forbids in as many words: "Do NOT
/// add a second emit path here or in any caller".
///
/// FORWARD-LOOKING, like `DELEGATED_PRIVATE_EMIT_SOURCE` below: both shapes
/// it writes are unrealizable in `debug_server.rs` today — `emit_delta` is
/// private to the `reify-gui` BINARY, and `state.app` presumes an `AppHandle`
/// field `DebugServerState` (`engine`, `selection`, `debug_bridge`,
/// `last_state`) does not have. Deliberately NOT rewritten to a realizable
/// shape: no emit shape at all is realizable until that field exists, so a
/// rewrite would pin fiction, whereas this pins the grammar for the day
/// someone adds it and writes the direct call. `EVENT_BUS_PRIVATE_EMIT_SOURCE`
/// carries the arm that CAN fire against today's library.
///
/// Consumed by `no_write_tool_handler_emits_privately`.
pub(super) const PRIVATE_EMIT_SOURCE: &str = r#"
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

/// A handler that is itself spotless and delegates everything to a helper
/// that emits privately — the REAL `handle_reify_open_file` shape, which does
/// nothing but call `open_path_into_engine`.
///
/// So while `reaches_a_seam` followed that hop and `emits_privately` did not,
/// the one tool the whole delegation machinery exists for had its entire
/// emission behaviour outside the sweep: an `app.emit(…)` added to
/// `open_path_into_engine` left the gate green.
///
/// The HOP is what this fixture pins, and that is live today. The `.emit(`
/// shape it hops to is forward-looking for the reason given on
/// `PRIVATE_EMIT_SOURCE` above.
///
/// Consumed by `a_private_emit_in_the_delegated_helper_is_flagged`.
pub(super) const DELEGATED_PRIVATE_EMIT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_open_file" => handle_reify_open_file(state, params).await,
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
    let delta = crate::diff::compute_delta(&state.last_state, &gui_state);
    state.app.emit("state-delta", &delta).ok();
    let frontend = push_gui_state(&state.debug_bridge, &gui_state, None).await?;
    Ok((frontend, gui_state.source.clone()))
}
"#;

/// A handler that routes through its seam CORRECTLY and then emits a second
/// time through `crate::event_bus::emit_typed` — the library's own sanctioned
/// emission wrapper, whose header steers callers to it as "a stable API
/// surface that lets emitter call sites stay unchanged".
///
/// This is the shape a future author is MOST likely to write, and the one the
/// first private-emit grammar could not see: `emit_typed` yields none of the
/// identifiers that grammar matched, and `::emit_typed(` is not `.emit(`.
/// Unlike the two fixtures above it is realizable against the real library
/// today — `emit_typed` is a `pub fn` of `reify_gui`, while `emit_delta` is
/// private to the `reify-gui` BINARY.
///
/// Consumed by `a_private_emit_through_the_event_bus_wrapper_is_flagged`.
pub(super) const EVENT_BUS_PRIVATE_EMIT_SOURCE: &str = r#"
async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "reify_set_parameter" => handle_reify_set_parameter(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
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
    let delta = crate::diff::compute_delta(&state.last_state, &gs);
    crate::event_bus::emit_typed(&state.app, "state-delta", &delta).ok();
    push_gui_state(&state.debug_bridge, &gs, None).await?;
    Ok(reify_set_parameter_envelope(None, None, vec![]))
}
"#;

/// A handler that looks perfectly compliant — it calls a
/// `*_and_refresh_baseline` fn — where that fn never reaches `compute_delta`.
/// Without this check the whole gate would rest on a NAME rather than on
/// behaviour, and a seam whose name lies would green it falsely.
///
/// Consumed by `every_refresh_baseline_seam_actually_refreshes`.
pub(super) const LYING_SEAM_SOURCE: &str = r#"
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
/// catch-all the live `dispatch_tool` also has, so it is dispatched but never
/// seen by the arm scan.
///
/// No widening of the arm parser can reach this shape: there is no arm to
/// read. Only a second, independent enumeration of the tool set catches it.
///
/// Consumed by `advertised_tools_must_all_appear_in_the_dispatch_scan`.
pub(super) const ADVERTISED_BUT_UNDISPATCHED_SOURCE: &str = r#"
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

/// The three shapes of ordinary Rust the review's probe confirmed the original
/// per-line arm parser DROPPED — wrapped, block and or-pattern — each with a
/// matching `ToolDef` entry so the fixture exercises the registry cross-check
/// alongside the scan.
///
/// That cross-check already fails these CLOSED, so widening the parser is
/// false-positive reduction, not a soundness fix. Worth doing anyway: a gate
/// that reds on ordinary Rust is one the next author weakens to get past.
///
/// Consumed by `ordinary_arm_shapes_are_all_enumerated`.
pub(super) const ARM_SHAPES_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_wrapped",
            description: "Handler on the following line.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_blocked",
            description: "Handler inside a block arm.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_first",
            description: "Shares one handler with reify_second.",
            input_schema: json!({ "type": "object" }),
        },
        ToolDef {
            name: "reify_second",
            description: "Shares one handler with reify_first.",
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
        "reify_wrapped" =>
            handle_reify_wrapped(state, params).await,
        "reify_blocked" => { handle_reify_blocked(state, params).await }
        "reify_first" | "reify_second" => handle_shared(state, params).await,
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}
"#;

/// A tool handled INLINE, with no handler fn anywhere in the file.
///
/// The trap: `Ok` IS an identifier followed by `(`, so a first-call-expression
/// rule resolves the handler to `Ok` and then reports `NoBaselineRefresh` —
/// telling the reader the handler skips the seam when the truth is that the
/// checker never found a handler at all. Those two warrant different fixes, so
/// they get different names.
///
/// Consumed by `an_arm_with_no_resolvable_handler_is_reported`.
pub(super) const INLINE_ARM_SOURCE: &str = r#"
fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "reify_inline",
            description: "Answered inline, with no handler fn.",
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
        "reify_inline" => Ok(Value::Null),
        _ => state.debug_bridge.query_frontend(name, params).await,
    }
}
"#;
