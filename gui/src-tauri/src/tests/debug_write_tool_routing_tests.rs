//! INV-GUI-2 (`docs/invariants.md`) structural gate for the AI/MCP entry
//! point: every `reify_*` write tool reaches the delta baseline through one
//! of the two shared `*_and_refresh_baseline` seams, never through a private
//! emit of its own.
//!
//! The claim this mechanizes is stated in prose in two places and is NOT
//! restated here — see `gui/src-tauri/src/debug_server.rs` point (a) on
//! `write_on_engine_and_refresh_baseline`, and the "Two seams, ONE stated
//! exception" section of `docs/debug-mcp-contract.md`.

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
