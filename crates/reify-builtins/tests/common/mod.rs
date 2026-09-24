//! Shared test helpers for `reify-builtins`' integration-test binaries.
//!
//! Include in a test binary with `mod common;` at the top of the file.
//! Helpers are `pub` so they are visible after `use common::{...}`.
//!
//! Integration tests are separate binaries and cannot import one another, so
//! anything two of them need lives here rather than being copy-pasted.

/// Resolves the manifest directory to use when locating this crate's files at
/// test time.
///
/// Prefers the runtime `CARGO_MANIFEST_DIR` (correct for whatever worktree is
/// actually running the test) over the compile-time `env!()` bake, which goes
/// stale when a seeded warm-lane `target/` is reused from a since-deleted
/// worktree (`CARGO_MANIFEST_DIR` is not part of cargo's fingerprint, so a
/// content-identical rebuild is never triggered). See esc-4906-57.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn resolve_manifest_dir(runtime: Result<String, std::env::VarError>) -> String {
    runtime.unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string())
}

/// This crate's manifest directory (`…/crates/reify-builtins`), warm-lane-safe.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn manifest_dir() -> String {
    resolve_manifest_dir(std::env::var("CARGO_MANIFEST_DIR"))
}

/// The workspace root — [`manifest_dir`] walked up two levels
/// (`…/crates/reify-builtins` → `…/crates` → `…`).
///
/// Warm-lane-safe for the same reason [`manifest_dir`] is: it is derived from
/// the runtime value, never from the compile-time bake of the SEEDING lane's
/// directory.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(&manifest_dir())
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| {
            panic!(
                "manifest_dir() = {:?} has fewer than two ancestors — expected \
                 <workspace>/crates/reify-builtins",
                manifest_dir()
            )
        })
        .to_path_buf()
}
