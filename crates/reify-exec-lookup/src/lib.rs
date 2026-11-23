//! The workspace's single PATH lookup for host executables.
//!
//! [`find_executable_on_path`] tries each PATH directory in order and, within
//! a directory, each name in order: an earlier directory wins over an earlier
//! name.
//!
//! It differs from `execvp`, and so from `env`, in two deliberate ways:
//! - Empty PATH components are skipped. `execvp` resolves them against the
//!   current directory; this lookup never does.
//! - An unset PATH finds nothing. `execvp` falls back to a default search
//!   path (glibc: `/bin:/usr/bin`).
//!
//! A candidate is executable when it is a regular file (symlinks followed)
//! that, on unix, `access(2)` with `X_OK` accepts. `access` checks the real
//! user and group IDs where `execve` checks the effective ones, so the two
//! agree unless the process runs setuid or setgid. Elsewhere any regular file
//! counts.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The first `dir/name` on `path_var` that is executable, per the crate-level
/// policy.
pub fn find_executable_on_path(names: &[&str], path_var: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
