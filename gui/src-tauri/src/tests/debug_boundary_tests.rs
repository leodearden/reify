use std::sync::Arc;

use crate::debug::DebugTransport;

/// Round-trip: a value delivered via resolve() arrives intact through the
/// id-keyed oneshot — the core transport contract.
///
/// Uses DebugTransport directly so no Tauri runtime (real or mock) is needed.
/// DebugBridge::query_frontend delegates to this same transport seam; the
/// emit/receive wiring that wraps it is exercised by the frontend boundary
/// tests (debugContract.test.ts).
#[tokio::test]
async fn round_trip_value_survives_transport() {
    let transport = Arc::new(DebugTransport::new());
    let t = transport.clone();

    tokio::spawn(async move {
        // Retry until create_request() has inserted the pending entry
        // (avoids the spawn/insert race; id is deterministically 1 on a
        // fresh transport).
        loop {
            if t.resolve(1, r#"{"devicePixelRatio":2.0}"#.to_string())
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    let (id, rx) = transport.create_request().unwrap();
    assert_eq!(id, 1, "first id on a fresh transport should be 1");

    let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .expect("resolve timed out")
        .expect("channel dropped");

    let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(result["devicePixelRatio"], 2.0_f64);
}

/// Error-envelope passthrough: an {error:...} JSON payload resolved into the
/// transport arrives verbatim — the seam does not unwrap or transform the
/// envelope.
#[tokio::test]
async fn error_envelope_passes_through_transport() {
    let transport = Arc::new(DebugTransport::new());
    let t = transport.clone();

    tokio::spawn(async move {
        loop {
            if t.resolve(1, r#"{"error":"boom"}"#.to_string()).is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    let (id, rx) = transport.create_request().unwrap();
    assert_eq!(id, 1);

    let raw = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .expect("resolve timed out")
        .expect("channel dropped");

    let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(result["error"], "boom");
}

/// resolve() with an id that has no pending entry returns Err containing
/// "no pending request".
#[test]
fn resolve_unknown_id_returns_err() {
    let transport = DebugTransport::new();
    let err = transport.resolve(999, "{}".to_string()).unwrap_err();
    assert!(err.contains("no pending request"), "unexpected err: {err}");
}

// ─────────────────────────────────────────────────────────────────────────────
// task 5097 δ — the `apply_gui_state` push shape the reify-* write tools use
// ─────────────────────────────────────────────────────────────────────────────

/// Build a real `GuiState` headlessly: a mock-kernel `EngineSession` loaded
/// from `bracket_source()`, so the payload assertions below run against a
/// state with genuinely populated `values`/`meshes` rather than a hand-rolled
/// struct literal (which could agree with a serializer that had drifted).
///
/// `gui`-gated with its two consumers below: `debug_server` is itself behind
/// `#[cfg(feature = "gui")]`, so these ride verify.sh's `--features gui`
/// TEST-EXECUTION pass.
#[cfg(feature = "gui")]
fn boundary_gui_state() -> crate::types::GuiState {
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::{MockGeometryKernel, bracket_source};

    let mut session = crate::engine::EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("load_from_source should succeed")
}

/// The δ write tools push their rebuilt `GuiState` — and, for
/// `reify_update_source`, the new editor buffer — across the SAME
/// `DebugTransport` seam every other debug tool uses. Pinned end to end here
/// (serialize → transport → deserialize) rather than on the serializer alone,
/// because the failure this guards against is a payload that is well-formed
/// in-process and lossy at the wire.
#[cfg(feature = "gui")]
#[tokio::test]
async fn write_tool_frontend_payload_survives_the_transport() {
    let gui_state = boundary_gui_state();
    assert!(
        !gui_state.values.is_empty(),
        "the fixture must carry values, or the round-trip below proves nothing"
    );

    let payload = crate::debug_server::write_tool_frontend_payload(
        &gui_state,
        Some(("/tmp/part.ri", "// new text")),
    )
    .expect("write_tool_frontend_payload must return Ok");

    let raw = serde_json::to_string(&payload).expect("payload must serialize");

    let transport = Arc::new(DebugTransport::new());
    let t = transport.clone();
    tokio::spawn(async move {
        loop {
            if t.resolve(1, raw.clone()).is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    let (id, rx) = transport.create_request().unwrap();
    assert_eq!(id, 1, "first id on a fresh transport should be 1");
    let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .expect("resolve timed out")
        .expect("channel dropped");
    let received: serde_json::Value =
        serde_json::from_str(&received).expect("received payload must be JSON");

    // EXACTLY these keys: `apply_gui_state`'s handler must not have to guess
    // which extra fields a write tool decided to attach.
    let mut keys: Vec<&str> = received
        .as_object()
        .expect("payload must be a JSON object")
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["file", "guiState"],
        "the write-tool payload must carry exactly guiState + file"
    );

    // The editor-sync half round-trips verbatim — the AI path writes the
    // engine's in-memory buffer, so nothing else will reconcile the editor.
    assert_eq!(received["file"]["path"].as_str(), Some("/tmp/part.ri"));
    assert_eq!(received["file"]["content"].as_str(), Some("// new text"));

    // The GuiState half survives serde intact across the wire.
    let round_tripped: crate::types::GuiState =
        serde_json::from_value(received["guiState"].clone())
            .expect("GuiState round-trip must succeed");
    assert_eq!(
        round_tripped
            .values
            .iter()
            .map(|v| (v.cell_id.clone(), v.value.clone(), v.unit.clone()))
            .collect::<Vec<_>>(),
        gui_state
            .values
            .iter()
            .map(|v| (v.cell_id.clone(), v.value.clone(), v.unit.clone()))
            .collect::<Vec<_>>(),
        "guiState.values must survive the transport byte-identical"
    );
}

/// With no file to sync, the payload must be EXACTLY what the existing
/// `apply_gui_state` senders already produce — no `file` key present at all,
/// not a `null` one. The frontend handler distinguishes the two, and the
/// pure-state write tools (`reify_save_file`, `reify_export`) plus the
/// re-expressed `fea_case_frontend_payload` all ride this arm.
#[cfg(feature = "gui")]
#[tokio::test]
async fn write_tool_frontend_payload_omits_file_when_absent() {
    let gui_state = boundary_gui_state();

    let payload = crate::debug_server::write_tool_frontend_payload(&gui_state, None)
        .expect("write_tool_frontend_payload must return Ok");

    let obj = payload.as_object().expect("payload must be a JSON object");
    assert_eq!(
        obj.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
        vec!["guiState"],
        "with no file, the payload must carry guiState and nothing else"
    );
    assert!(
        !obj.contains_key("file"),
        "an absent file must be an ABSENT key, never a null one"
    );
}
