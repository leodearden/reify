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
