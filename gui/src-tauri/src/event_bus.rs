//! Convention helpers for GUI event channels — see
//! `docs/prds/v0_3/gui-event-channel-inventory.md` §3.4 and the canonical
//! channel inventory at `docs/gui-event-channels.md` (GR-016 task β).
//!
//! `emit_typed` is intentionally a single-line wrapper around
//! `tauri::AppHandle::emit`. Its value is:
//!   (a) a single grep-able call site for future telemetry / debug-build
//!       assertions / channel-name validation against the inventory;
//!   (b) a stable API surface that lets emitter call sites stay unchanged
//!       when Phase 5 lint enforcement lands (PRD §9 task μ).
//!
//! Hand-call to `app.emit()` remains permitted for variant-shaped payloads
//! (LSP diagnostics, MCP-routed events) — see §3.4 final paragraph.

use serde::Serialize;
use tauri::Emitter;

/// Emit `payload` on the given Tauri event `channel`.
///
/// Thin typed wrapper over `tauri::AppHandle::emit`. `T: Serialize` ensures
/// the payload type satisfies the PRD §3.2 cross-process serialization
/// requirement (no closures, no `&'static`-only references, etc.).
pub fn emit_typed<T: Serialize>(
    app: &tauri::AppHandle,
    channel: &str,
    payload: &T,
) -> Result<(), tauri::Error> {
    app.emit(channel, payload)
}
