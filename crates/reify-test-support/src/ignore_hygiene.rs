use std::path::{Path, PathBuf};

/// Scan `source` for `#[ignore = "..."]` reason strings that contain a stale
/// transient-plan-doc pointer (e.g. a `plan step-N` breadcrumb). Returns one
/// human-readable violation string per offender. Empty Vec means clean.
///
/// The marker and needle are assembled at runtime so this source file does not
/// contain the literal substrings and does not self-trigger when scanned.
pub fn find_stale_plan_pointers_in_source(_source: &str) -> Vec<String> {
    unimplemented!()
}

/// Recursively walk `workspace_root` collecting every `.rs` file whose path
/// contains a directory component named `tests`. Skips `target`, `.git`,
/// `.worktrees`, and any directory whose name starts with `.`.
///
/// Uses `std::fs::read_dir` with an explicit stack (no recursion, no external
/// `walkdir` dep) — matching the existing convention in `reify-kernel-occt/build.rs`.
///
/// # Panics
///
/// Does not panic — I/O errors on individual directories are silently skipped.
pub fn walk_test_rs_files(_workspace_root: &Path) -> Vec<PathBuf> {
    unimplemented!()
}
