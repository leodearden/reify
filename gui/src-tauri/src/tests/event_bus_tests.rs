//! Convention smoke test for the `event_bus` module (GR-016 β).
//!
//! See `docs/prds/v0_3/gui-event-channel-inventory.md` §6.1/§8.1 and the
//! canonical channel inventory at `docs/gui-event-channels.md`.
//!
//! §8.1 signature compile-check: asserts `emit_typed` exists with the
//! PRD §3.4-mandated signature. Runs only under
//! `cargo test -p reify-gui --features gui`.

/// §8.1 signature compile-check: asserts that `emit_typed` exists with the
/// PRD §3.4-mandated signature `fn(&AppHandle, &str, &T) -> Result<(), tauri::Error>`.
///
/// Uses `String` as a representative `T: Serialize` type — any `Serialize` type
/// suffices to pin the signature; the important assertion is the function's
/// arity and return type, not the concrete payload shape.
///
/// Runs only under `cargo test -p reify-gui --features gui`.
/// Under standard `cargo test --workspace` (no `gui` feature) this block is
/// excluded by the `cfg` gate — the always-on serde shape pin is omitted per
/// reviewer guidance: it was pinning serde_json's behavior on a local fixture
/// struct (no production caller), not the event-bus contract itself.
#[cfg(feature = "gui")]
#[test]
fn convention_smoke_emit_typed_signature_compiles() {
    let _: fn(
        &tauri::AppHandle,
        &str,
        &String,
    ) -> Result<(), tauri::Error> = crate::event_bus::emit_typed::<String>;
}
