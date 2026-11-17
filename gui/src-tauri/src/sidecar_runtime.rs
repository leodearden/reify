//! The interpreter the Claude sidecar needs before the host may spawn it.
//!
//! The installed sidecar is a `#!/usr/bin/env node` script. When `node` is
//! missing, the spawn still succeeds and the sidecar dies with nothing but an
//! uncoded "exited unexpectedly". So the host looks `node` up on the PATH the
//! sidecar inherits, much as `env` does, and refuses with a coded error before
//! spawning when none is found.

use std::ffi::OsStr;
use std::future::Future;
use std::path::{Path, PathBuf};

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

/// The first [`SIDECAR_INTERPRETER`] in `path_var`'s directory order that is a
/// regular file with an execute bit set. This approximates the lookup
/// `/usr/bin/env node` makes, with two differences. Any execute bit counts,
/// even one the current user does not hold, and `env` would then exit 126.
/// Empty PATH components are skipped on purpose, where `env` would resolve
/// them against the current directory.
pub fn require_sidecar_interpreter(path_var: Option<&OsStr>) -> Result<PathBuf, String> {
    path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(SIDECAR_INTERPRETER))
        .find(|candidate| is_executable_file(candidate))
        .ok_or_else(|| {
            let searched = path_var.map_or_else(|| "(unset)".into(), OsStr::to_string_lossy);
            format!(
                "{E_GUI_SIDECAR_NODE_MISSING}: Assistant mode runs reify-sidecar under Node.js, \
                 but no executable `{SIDECAR_INTERPRETER}` was found on PATH={searched}. \
                 Install Node.js 18 or newer (Ubuntu: `sudo apt install nodejs`) and send \
                 the message again."
            )
        })
}

/// `true` iff `path` is an existing regular file with an execute bit set.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// `true` iff `path` is an existing regular file (non-unix: no execute bit).
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
