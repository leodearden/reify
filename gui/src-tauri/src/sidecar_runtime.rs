//! The interpreter the Claude sidecar needs before the host may spawn it.
//!
//! The installed sidecar is a `#!/usr/bin/env node` script. When `node` is
//! missing, the spawn still succeeds and the sidecar dies with nothing but an
//! uncoded "exited unexpectedly". So the host looks `node` up on the PATH the
//! sidecar inherits, much as `env` does, and refuses with a coded error before
//! spawning when none is found.

use std::ffi::OsStr;
use std::future::Future;
use std::path::PathBuf;

/// The program named by the shebang line build-sidecar.sh writes.
pub const SIDECAR_INTERPRETER: &str = "node";

/// Refusal code when no executable [`SIDECAR_INTERPRETER`] is on PATH.
pub const E_GUI_SIDECAR_NODE_MISSING: &str = "E_GUI_SIDECAR_NODE_MISSING";

/// Awaits `spawn` only when [`require_sidecar_interpreter`] finds the
/// interpreter on `path_var`. Otherwise returns its refusal, and `spawn` is
/// never polled.
pub async fn spawn_if_interpreter_found<T>(
    path_var: Option<&OsStr>,
    spawn: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    require_sidecar_interpreter(path_var)?;
    spawn.await
}

/// The [`SIDECAR_INTERPRETER`] that `/usr/bin/env node` would run, found with
/// [`reify_exec_lookup::find_executable_on_path`], whose doc states the lookup
/// policy and how it differs from `env`.
pub fn require_sidecar_interpreter(path_var: Option<&OsStr>) -> Result<PathBuf, String> {
    reify_exec_lookup::find_executable_on_path(&[SIDECAR_INTERPRETER], path_var).ok_or_else(|| {
        let searched = path_var.map_or_else(|| "(unset)".into(), OsStr::to_string_lossy);
        format!(
            "{E_GUI_SIDECAR_NODE_MISSING}: Assistant mode runs reify-sidecar under Node.js, \
             but no executable `{SIDECAR_INTERPRETER}` was found on PATH={searched}. \
             Install Node.js 18 or newer (Ubuntu: `sudo apt install nodejs`) and send \
             the message again."
        )
    })
}
