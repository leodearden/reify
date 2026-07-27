//! Integration tests for the PDOCCOVER registry↔chunk name-drift detector
//! (`pdoccover::check`).
//!
//! Two test families:
//!
//! - **Hermetic fixture trees** — a `tempfile` tempdir as `project_root` with
//!   synthetic `units.rs` / `chunks/*.md` / `pdoccover-baseline.txt` files, a
//!   `MockGitOps::set_ls_files`, an in-memory rusqlite and a
//!   `MockJCodemunchOps` (the `tests/pdssentinel.rs` harness). Only the file
//!   *list* is mocked; `check()` reads real content from disk. Every
//!   disposition assertion — including the `offset_surface` fabrication
//!   fixture — lives here, never against the real tree, so a sibling task
//!   editing chunk content cannot flip these RED.
//!
//! - **Brittle-parse floor guards** — run the pure scanners over the REAL
//!   `crates/reify-compiler/src/units.rs` and `crates/reify-mcp/src/tools/
//!   chunks/*.md` and assert conservative floors. Their job is to fail RED
//!   when a source refactor breaks extraction, instead of letting PDOCCOVER
//!   silently pass clean on an empty census. They freeze no exact count.

mod common;

use std::path::{Path, PathBuf};

/// Write `content` to relative `path` inside `root`, creating parent dirs.
#[allow(dead_code)]
fn write_file(root: &Path, path: &str, content: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("create_dir_all");
    }
    std::fs::write(&full, content).expect("write_file");
}

/// Repo root, resolved from `CARGO_MANIFEST_DIR` (= `crates/reify-audit`).
#[allow(dead_code)]
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}
