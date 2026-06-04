use std::sync::Arc;

use serde_json::json;
use tauri::test::mock_app;

use crate::debug::DebugBridge;

/// Round-trip: a value emitted via query_frontend and resolved back by the
/// caller's resolve() arrives intact through the id-keyed oneshot.
///
/// RED until DebugBridge is generic over R: Runtime (step-2): DebugBridge::new
/// currently pins AppHandle<Wry>, but mock_app().handle() yields
/// AppHandle<MockRuntime>.
#[tokio::test]
async fn round_trip_value_survives_transport() {
    let app = mock_app();
    let bridge = Arc::new(DebugBridge::new(app.handle().clone()));

    let bridge_c = bridge.clone();
    tokio::spawn(async move {
        // Retry until query_frontend has inserted the pending entry (avoids
        // the insert/spawn race; id is deterministically 1 on a fresh bridge).
        loop {
            if bridge_c
                .resolve(1, r#"{"devicePixelRatio":2.0}"#.to_string())
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    let result = bridge
        .query_frontend("get_window_state", json!({}))
        .await
        .unwrap();
    assert_eq!(result["devicePixelRatio"], 2.0_f64);
}

/// Error-envelope passthrough: an {error:...} JSON payload resolved into the
/// transport arrives verbatim at the query_frontend caller — the Rust seam
/// does not unwrap or transform the envelope.
///
/// RED until DebugBridge is generic over R: Runtime (step-2).
#[tokio::test]
async fn error_envelope_passes_through_transport() {
    let app = mock_app();
    let bridge = Arc::new(DebugBridge::new(app.handle().clone()));

    let bridge_c = bridge.clone();
    tokio::spawn(async move {
        loop {
            if bridge_c
                .resolve(1, r#"{"error":"boom"}"#.to_string())
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    let result = bridge
        .query_frontend("get_window_state", json!({}))
        .await
        .unwrap();
    assert_eq!(result["error"], "boom");
}

/// resolve() with an id that has no pending entry returns Err containing
/// "no pending request".
#[test]
fn resolve_unknown_id_returns_err() {
    let app = mock_app();
    let bridge = DebugBridge::new(app.handle().clone());
    let err = bridge.resolve(999, "{}".to_string()).unwrap_err();
    assert!(err.contains("no pending request"), "unexpected err: {err}");
}
