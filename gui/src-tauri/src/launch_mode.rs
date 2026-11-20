//! The Start view's mode selection, Design / Assistant / Debug (task 9730).
//!
//! The frontend owns which mode is selected and applies the panel effects;
//! the backend's only part is Debug, which makes the debug server live.

use serde::{Deserialize, Serialize};

use crate::debug_server_slot::DebugServerSlot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Design,
    Assistant,
    Debug,
}

/// The mode just selected and the debug server's live port, if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModeState {
    pub mode: Mode,
    pub debug_port: Option<u16>,
}

/// set_mode's structured rejection, naming the stage that failed; Tauri
/// reports IPC-level failures as plain strings instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetModeError {
    /// DebugServerSlot::ensure_live could not bind the reify-debug listener.
    DebugListenerBind { message: String },
}

/// Apply `mode`: Debug makes the debug server live on `port` (idempotent);
/// Design and Assistant touch nothing here. No mode spawns a process or stops
/// the server, so `debug_port` always reports whether it is live.
pub fn set_mode_impl(
    mode: Mode,
    slot: &DebugServerSlot,
    port: u16,
) -> Result<ModeState, SetModeError> {
    if mode == Mode::Debug {
        slot.ensure_live(port)
            .map_err(|message| SetModeError::DebugListenerBind { message })?;
    }
    Ok(ModeState {
        mode,
        debug_port: slot.live_port(),
    })
}
