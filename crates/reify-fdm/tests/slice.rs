// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the PrusaSlicer subprocess core (`reify_fdm::slice`,
//! task η).
//!
//! Every test here is GREEN-able WITHOUT a live PrusaSlicer (which is not on
//! PATH in CI): PATH discovery uses a synthetic `$PATH` string + a fake
//! executable in a tempdir; subprocess spawn/cancel/reap uses injected stub
//! binaries (`sh -c …`); G-code→Toolpath reuses ζ's parser on the committed
//! fixture; determinism is asserted by parsing the committed fixture twice.

use std::path::PathBuf;

use reify_fdm::slice::discover_slicer;

/// The canonical PrusaSlicer binary names probed on `$PATH`, in priority order.
const CANDIDATES: &[&str] = &[
    "prusa-slicer",
    "prusa-slicer-console",
    "PrusaSlicer",
    "prusaslicer",
];

/// Write a file named `name` under `dir`, marking it executable on unix, and
/// return its path.
fn write_exe(dir: &std::path::Path, name: &str, executable: bool) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("write fake exe");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("set perms on fake exe");
    }
    path
}

// ── step-1: discover_slicer ────────────────────────────────────────────────────

/// A fake `prusa-slicer` executable on the synthetic PATH is discovered, and the
/// returned path is exactly the fake exe.
#[test]
fn discovers_executable_candidate_on_synthetic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = write_exe(dir.path(), "prusa-slicer", true);

    // PATH = a noise dir that has no candidate, then the dir holding the fake exe.
    let other = tempfile::tempdir().expect("tempdir2");
    let path_var = format!(
        "{}:{}",
        other.path().display(),
        dir.path().display()
    );

    let found = discover_slicer(&path_var, CANDIDATES);
    assert_eq!(
        found.as_deref(),
        Some(exe.as_path()),
        "discover_slicer must find the executable prusa-slicer on the synthetic PATH"
    );
}

/// An empty PATH yields no slicer (the W_FDM_SLICER_UNAVAILABLE trigger).
#[test]
fn empty_path_yields_none() {
    assert_eq!(
        discover_slicer("", CANDIDATES),
        None,
        "an empty PATH must yield no slicer"
    );
}

/// A PATH whose dirs contain no candidate yields None (the absent-slicer case).
#[test]
fn path_without_candidate_yields_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A same-dir file with a NON-candidate name must not be matched.
    write_exe(dir.path(), "some-other-tool", true);
    let path_var = format!("{}", dir.path().display());

    assert_eq!(
        discover_slicer(&path_var, CANDIDATES),
        None,
        "a PATH with no candidate-named executable must yield None"
    );
}

/// A file with the right name but NO executable bit is not matched (unix only —
/// on non-unix discovery falls back to existence).
#[cfg(unix)]
#[test]
fn non_executable_candidate_is_not_matched() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_exe(dir.path(), "prusa-slicer", false); // exists but not +x
    let path_var = format!("{}", dir.path().display());

    assert_eq!(
        discover_slicer(&path_var, CANDIDATES),
        None,
        "a non-executable file named prusa-slicer must NOT be matched"
    );
}
