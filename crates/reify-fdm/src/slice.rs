// SPDX-License-Identifier: AGPL-3.0-or-later

//! PrusaSlicer subprocess invocation core (task η).
//!
//! See `docs/prds/v0_5/fdm-as-printed-fea.md` task η (slice 2). This module is
//! the pure-ish subprocess half of the `fdm::slice` ComputeNode: it discovers a
//! PrusaSlicer binary on `$PATH`, composes a deterministic settings/CLI profile,
//! runs the slicer **as a subprocess** (never FFI — AGPL boundary, PRD DD#4)
//! with cooperative SIGTERM→SIGKILL cancellation, and parses the resulting
//! G-code into a [`crate::Toolpath`] (delegating to ζ's
//! [`crate::parse_prusaslicer_gcode`]).
//!
//! The cancellation signal is a `Fn() -> bool` closure, NOT a
//! `reify_eval::CancellationHandle`: reify-fdm must not depend on reify-eval
//! (the reverse dependency edge). The eval-side trampoline
//! (`reify-eval/src/compute_targets/fdm_slice.rs`) supplies
//! `|| cancellation.is_cancelled()`.
//
// The implementation is built incrementally across task η steps 1–12.

use std::path::{Path, PathBuf};

/// The canonical PrusaSlicer binary names probed on `$PATH`, in priority order.
///
/// Covers the GUI binary (`prusa-slicer`), the headless console binary
/// (`prusa-slicer-console`, the Windows/AppImage CLI name), and the
/// CamelCase / lowercase spellings shipped by various distributions /
/// AppImages. The eval-side trampoline passes this slice to
/// [`discover_slicer`]; tests pass their own subset.
pub const DEFAULT_SLICER_NAMES: &[&str] = &[
    "prusa-slicer",
    "prusa-slicer-console",
    "PrusaSlicer",
    "prusaslicer",
];

/// Discover a PrusaSlicer binary on the `$PATH`-style `path_var` string.
///
/// Splits `path_var` on the platform path separator (`:` on unix, `;` on
/// Windows) via [`std::env::split_paths`], then for each non-empty directory
/// tries each `candidates` name in order, returning the first `dir/name` that
/// exists and is an executable regular file. Returns `None` when no candidate is
/// found — the W_FDM_SLICER_UNAVAILABLE trigger (PRD open Q4).
///
/// Iteration is **directory-major**: every candidate is tried in `$PATH`
/// directory order, so an earlier `$PATH` entry wins over a later one
/// regardless of candidate priority (standard `$PATH` lookup semantics).
///
/// On unix "executable" means a regular file with at least one of the three
/// execute bits set (`mode & 0o111 != 0`); on other platforms it means any
/// existing regular file (the execute bit has no unix-style meaning there).
pub fn discover_slicer(path_var: &str, candidates: &[&str]) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            // An empty `$PATH` component (e.g. `""` or a `::`) conventionally
            // means "the current directory"; we deliberately skip it so a bare
            // candidate name can never be resolved relative to the CWD.
            continue;
        }
        for &name in candidates {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// `true` iff `path` is an existing regular file that is executable.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

/// `true` iff `path` is an existing regular file (non-unix: no execute bit).
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
