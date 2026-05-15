//! Convention smoke tests for the `event_bus` module (GR-016 β).
//!
//! See `docs/prds/v0_3/gui-event-channel-inventory.md` §6.1/§8.1 and the
//! canonical channel inventory at `docs/gui-event-channels.md`.
//!
//! Two test halves:
//!   1. Always-on serde JSON shape snapshot (runs under `cargo test --workspace`).
//!   2. `cfg(feature = "gui")` function-pointer signature compile-check on
//!      `emit_typed` (requires `cargo test -p reify-gui --features gui`).

use serde::{Deserialize, Serialize};

/// Fixture struct used only by these tests — not a production channel type.
/// Validates the §3.4 PRD requirement: payloads must be `Serialize + Deserialize`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct ConventionSmokePayload {
    iteration: u32,
    label: String,
}

/// §6.1 byte-for-byte JSON snapshot check.
///
/// Builds a fixture, serialises it with `serde_json`, and asserts equality with
/// the frozen canonical string. Also checks the `serde_json::Value` representation.
/// Runs under standard `cargo test --workspace` (no feature flag required).
#[test]
fn convention_smoke_payload_serializes_to_canonical_shape() {
    let payload = ConventionSmokePayload {
        iteration: 7,
        label: "ok".into(),
    };

    let json_str = serde_json::to_string(&payload).unwrap();
    assert_eq!(json_str, r#"{"iteration":7,"label":"ok"}"#);

    let json_val = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        json_val,
        serde_json::json!({"iteration": 7, "label": "ok"})
    );
}

/// §8.1 signature compile-check: asserts that `emit_typed` exists with the
/// PRD §3.4-mandated signature `fn(&AppHandle, &str, &T) -> Result<(), tauri::Error>`.
///
/// Runs only under `cargo test -p reify-gui --features gui`.
/// Under standard `cargo test --workspace` (no `gui` feature) this block is
/// excluded by the `cfg` gate — the always-on serde test above covers the CI signal.
#[cfg(feature = "gui")]
#[test]
fn convention_smoke_emit_typed_signature_compiles() {
    let _: fn(
        &tauri::AppHandle,
        &str,
        &ConventionSmokePayload,
    ) -> Result<(), tauri::Error> = crate::event_bus::emit_typed::<ConventionSmokePayload>;
}
