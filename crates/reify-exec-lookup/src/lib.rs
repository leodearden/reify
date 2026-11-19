//! The workspace's single PATH lookup for host executables.
//!
//! [`find_executable_on_path`] tries each PATH directory in order and, within
//! a directory, each name in order: an earlier directory wins over an earlier
//! name. An unset PATH finds nothing.
//!
//! Empty PATH components are skipped on purpose. `execvp` and `env` resolve
//! them against the current directory; this lookup never does.
//!
//! A candidate is executable when it is a regular file (symlinks followed)
//! with at least one execute bit set on unix, and any regular file elsewhere.

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
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
