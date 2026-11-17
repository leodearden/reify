//! The interpreter the Claude sidecar needs before the host may spawn it.
//!
//! The installed sidecar is a `#!/usr/bin/env node` script. When `node` is
//! missing, the spawn still succeeds and the sidecar dies with nothing but an
//! uncoded "exited unexpectedly". So the host repeats `env`'s lookup on the PATH
//! the sidecar inherits, and refuses with a coded error when it would fail.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The program named by the shebang line build-sidecar.sh writes.
pub const SIDECAR_INTERPRETER: &str = "node";

/// Refusal code when no executable [`SIDECAR_INTERPRETER`] is on PATH.
pub const E_GUI_SIDECAR_NODE_MISSING: &str = "E_GUI_SIDECAR_NODE_MISSING";

/// The first executable [`SIDECAR_INTERPRETER`] in `path_var`'s directory
/// order, which is the file `/usr/bin/env node` would run. Empty PATH
/// components are skipped, so the interpreter never resolves against the
/// current directory.
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
