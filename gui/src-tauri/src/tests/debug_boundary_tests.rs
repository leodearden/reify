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

/// Build a real `GuiState` headlessly from arbitrary source: a mock-kernel
/// `EngineSession`, so the payload assertions below run against a state with
/// genuinely populated `values`/`meshes`/`constraints` rather than a hand-rolled
/// struct literal (which could agree with a serializer that had drifted).
///
/// The `source` parameter is what lets one test build TWO states that differ by
/// a single parameter and compare them — the constraint-flip test at the end of
/// this file — without a second construction site drifting from this one.
///
/// `gui`-gated with its consumers below: `debug_server` is itself behind
/// `#[cfg(feature = "gui")]`, so these ride verify.sh's `--features gui`
/// TEST-EXECUTION pass.
#[cfg(feature = "gui")]
fn boundary_gui_state_from(source: &str) -> crate::types::GuiState {
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;

    let mut session = crate::engine::EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    session
        .load_from_source(source, "bracket")
        .expect("load_from_source should succeed")
}

/// The default fixture state — `bracket_source()` through
/// [`boundary_gui_state_from`].
#[cfg(feature = "gui")]
fn boundary_gui_state() -> crate::types::GuiState {
    boundary_gui_state_from(reify_test_support::bracket_source())
}

/// Push one write-tool payload across a real `DebugTransport` and hand back what
/// came out the far side, having pinned the key set on the way.
///
/// The key-set assertion lives HERE rather than at each call site, and that is
/// the reason this is a helper at all: "a write-tool push carries exactly
/// `guiState` + `file`" is ONE claim about the payload shape, so it is asserted
/// in one place. Two copies would drift the moment the shape changes, and
/// `apply_gui_state`'s handler must never have to guess which extra fields a
/// write tool decided to attach.
///
/// The retry loop around `resolve` is the same spawn/insert race every transport
/// test in this file runs into: `create_request` has not inserted the pending
/// entry yet when the spawned task first fires. The id is deterministically 1 on
/// a fresh transport.
#[cfg(feature = "gui")]
async fn round_trip_write_tool_payload(payload: &serde_json::Value) -> serde_json::Value {
    let raw = serde_json::to_string(payload).expect("payload must serialize");

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

    received
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

    let received = round_trip_write_tool_payload(&payload).await;

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


/// The write tools' push REPLY, across the same seam as the push itself: the
/// bridge answers an unregistered command (or a handler that threw) with an
/// in-band `{error: string}` object, and `DebugTransport` delivers it as a
/// perfectly well-formed `Ok(Value)` — which is why
/// `error_envelope_passes_through_transport` above is a PASSTHROUGH contract
/// rather than a failure one.
///
/// So the discrimination has to happen above the transport, and this pins it
/// there end to end: refusal payload → transport → `frontend_ok` → `Err`. The
/// consequence it guards is specific to the δ write tools — the seam has
/// already refreshed the delta baseline to S1 before the push goes out, so a
/// refused push that reads as success leaves `last_state` ahead of the
/// frontend (bug #7) while the AI client is told the write landed
/// (task #5097 δ, review finding).
#[cfg(feature = "gui")]
#[tokio::test]
async fn frontend_error_envelope_is_refused_after_the_transport() {
    let transport = Arc::new(DebugTransport::new());
    let t = transport.clone();
    tokio::spawn(async move {
        loop {
            if t.resolve(1, r#"{"error":"unknown command: apply_gui_state"}"#.to_string())
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
    let reply: serde_json::Value = serde_json::from_str(&raw).expect("reply must be JSON");

    // The transport itself is content-blind: this is an `Ok` value, and a
    // handler that ignored it would answer `success: true`.
    assert_eq!(reply["error"], "unknown command: apply_gui_state");

    let err = crate::debug_server::frontend_ok(reply, "apply_gui_state")
        .expect_err("an {error} reply must be refused, not read as a landed push");
    assert_eq!(
        err,
        "apply_gui_state push refused by the frontend: unknown command: apply_gui_state"
    );

    // A real `apply_gui_state` reply — the frontend handler returns an empty
    // object — survives the same path untouched.
    let ok = crate::debug_server::frontend_ok(serde_json::json!({}), "apply_gui_state")
        .expect("a successful push must not be refused");
    assert_eq!(ok, serde_json::json!({}));
}

/// CONSTRAINT-STATUS SYNC across the write-tool payload — the headless half of
/// task 5098's printer_v01 rail-lengthening gate, in miniature.
///
/// The live gate drives `reify_set_parameter` at a real printer and watches a
/// pin flip Satisfied → Violated → Satisfied. That needs a webview and OCCT, so
/// it can never run in CI. What CAN run in CI is the claim underneath it: that a
/// constraint whose STATUS changed because a parameter changed survives the
/// write-tool payload intact — `GuiState.constraints` is a
/// `diffed keyed(key=node_id)` field on the same delta choke-point as `values`
/// and `meshes`, and the sibling test above already pins the `values` half.
///
/// `bracket_source_with_width("20mm")` supplies the flip for free: the fixture's
/// `constraint thickness < width / 4` reads 5 < 20 at the default width and
/// 5 < 5 once narrowed, so ONE parameter edit moves exactly one constraint. No
/// GUI, no live engine, no printer.
#[cfg(feature = "gui")]
#[tokio::test]
async fn write_tool_payload_carries_a_flipped_constraint_status() {
    use reify_ir::Satisfaction;
    use reify_test_support::{bracket_source, bracket_source_with_width};

    let narrowed_source = bracket_source_with_width("20mm");
    let before = boundary_gui_state_from(bracket_source());
    let after = boundary_gui_state_from(&narrowed_source);

    // Without this the flip search below is vacuous: two empty lists agree, and
    // "no constraint changed" would read as a pass.
    assert!(
        !before.constraints.is_empty() && !after.constraints.is_empty(),
        "both fixtures must carry constraints, or this test proves nothing"
    );

    // The flip is found by comparing the SAME node_id across the two states —
    // the key `diff_gui_state` itself matches on.
    let prior: std::collections::HashMap<&str, &str> = before
        .constraints
        .iter()
        .map(|c| (c.node_id.as_str(), c.status.as_str()))
        .collect();
    let flipped: Vec<&crate::types::ConstraintData> = after
        .constraints
        .iter()
        .filter(|c| prior.get(c.node_id.as_str()).copied() != Some(c.status.as_str()))
        .collect();
    assert_eq!(
        flipped.len(),
        1,
        "narrowing width must move exactly one constraint; moved: {:?}",
        flipped
            .iter()
            .map(|c| (&c.node_id, &c.status))
            .collect::<Vec<_>>()
    );
    let flipped = flipped[0];
    // Never a hand-written `"satisfied"`: `types.rs` makes
    // `engine::satisfaction_token` the sole permitted producer of this token,
    // and a literal here is what let this gate ship asserting the PascalCase
    // ENUM VARIANT names instead of the lower-case wire tokens.
    assert_eq!(
        prior.get(flipped.node_id.as_str()).copied(),
        Some(crate::engine::satisfaction_token(Satisfaction::Satisfied)),
        "the flipped constraint must have been satisfied before the edit"
    );
    assert_eq!(
        flipped.status,
        crate::engine::satisfaction_token(Satisfaction::Violated)
    );

    // The selector the live gate uses: a pin named by the cells it is about
    // instead of by its positional `node_id`. `parameter_ids` is
    // `collect_value_refs(expr)`, an INSTANCE PATH rooted at the declaring
    // entity — `{Entity}.{member}` for a same-entity `self.x` reference, but
    // `{Entity}.{sub}.{member}` for a cross-sub `self.s.x` one. Only the former
    // is also a valid `reify_set_parameter` `cell_id`: that surface resolves
    // against `compiled.templates[].value_cells`, which is keyed by TYPE.
    // `bracket` is a flat single-structure fixture, so the two namespaces
    // coincide HERE and this assertion must not be read as a general rule —
    // the composed case is derived in `railLengtheningGate.mjs`'s
    // `PIN_RAIL_SPAN_CELL` docblock and tabulated in docs/debug-mcp-contract.md.
    assert!(
        flipped.parameter_ids.contains(&"Bracket.width".to_string()),
        "the flipped constraint must name the edited cell; parameter_ids: {:?}",
        flipped.parameter_ids
    );

    // ── The payload round trip: serialize → DebugTransport → deserialize ──
    let payload = crate::debug_server::write_tool_frontend_payload(
        &after,
        Some(("/tmp/bracket.ri", narrowed_source.as_str())),
    )
    .expect("write_tool_frontend_payload must return Ok");

    // The helper re-asserts the key set: carrying a constraint flip must not
    // have added a channel of its own.
    let received = round_trip_write_tool_payload(&payload).await;

    let round_tripped: crate::types::GuiState =
        serde_json::from_value(received["guiState"].clone())
            .expect("GuiState round-trip must succeed");
    let delivered = round_tripped
        .constraints
        .iter()
        .find(|c| c.node_id == flipped.node_id)
        .expect("the flipped constraint must still be in the delivered payload");
    assert_eq!(
        (
            &delivered.node_id,
            &delivered.status,
            &delivered.parameter_ids
        ),
        (&flipped.node_id, &flipped.status, &flipped.parameter_ids),
        "node_id / status / parameter_ids must survive the transport byte-identical"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// PRD §7 B4 — **no stale baseline** (INV-GUI-2, gui-state-sync survey bug #7) —
// is NOT pinned here, and looking for it in this file is the mistake this note
// exists to prevent.
//
// It is the one B-row of task 5098's ζ gate that the LIVE driver cannot see,
// and that is by design rather than a gap in the debug surface: §6.2 caveat (i)
// — restated on `write_on_engine_and_refresh_baseline` itself — has the debug
// path DISCARD the `StateDelta` and push the full `GuiState` instead, so no
// debug tool can return a delta. The invariant is therefore only observable
// where `compute_delta` and `last_state` both are, and it is already pinned
// there, driving the real production seam:
// `debug_server::tests::write_tools::write_helper_refreshes_the_delta_baseline`
// calls `write_on_engine_and_refresh_baseline` on a tempdir-backed engine and
// asserts the baseline moved to S1 AND that a second diff against S1 emits no
// events at all — which is exactly "no spurious over-reporting".
//
// A copy here that reached `compute_delta` directly instead of the seam would
// be strictly weaker: `compute_delta` advances the baseline unconditionally
// (`diff.rs`), so the silence it produces holds by construction, and the bug-#7
// shape — a write wrapper that mutates the engine and forgets to refresh —
// would leave such a test green.
// ─────────────────────────────────────────────────────────────────────────────
