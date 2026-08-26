//! Placement contract for the Ring-1 language-spec conformance fixture tree
//! (PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2; leaf beta #6759).
//!
//! D2 mandates `crates/reify-spec-conformance/fixtures/<section>/<name>.ri` —
//! per-section subdirectories, never a flat pile at the tree root. These tests
//! pin the STRUCTURE of the tree, not its contents: they glob and deliberately
//! never name any individual fixture's basename, so adding, renaming or
//! removing a fixture stays inert to every tracked `.rs` source (which is what
//! keeps a future basename-derived coupling set from ever keying on this tree).

use std::path::{Path, PathBuf};

/// The fixture tree root, resolved from this crate's manifest directory.
///
/// Mirrors the `env!("CARGO_MANIFEST_DIR")` idiom used by
/// `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`'s
/// `workspace_root()`, but resolves the crate-local tree directly rather than
/// walking up to the workspace root.
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Walk `dir` recursively, appending every `*.ri` file to `out`.
///
/// Silently skips unreadable entries — mirrors `collect_files` in
/// `corpus_no_bare_scalar.rs`.
fn collect_ri(dir: &Path, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ri(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

#[test]
fn fixture_tree_exists() {
    let root = fixtures_root();
    assert!(
        root.is_dir(),
        "D2 requires the language-spec conformance fixture tree at \
         `crates/reify-spec-conformance/fixtures/` (resolved here as {}), \
         but it does not exist or is not a directory. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );
}

#[test]
fn no_loose_ri_at_fixture_tree_root() {
    let root = fixtures_root();
    assert!(
        root.is_dir(),
        "D2 requires the language-spec conformance fixture tree at \
         `crates/reify-spec-conformance/fixtures/` (resolved here as {}), \
         but it does not exist or is not a directory. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );

    let rd = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("fixture tree {} must be readable: {e}", root.display()));

    let mut loose: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() && path.extension().and_then(|e| e.to_str()) == Some("ri") {
            loose.push(path);
        }
    }
    loose.sort();

    assert!(
        loose.is_empty(),
        "Found {} loose `*.ri` file(s) directly at the fixture-tree root:\n\n{}\n\n\
         D2 requires every conformance fixture to live in a per-section \
         subdirectory — `crates/reify-spec-conformance/fixtures/<section>/<name>.ri`, \
         never flat at `fixtures/`. Move each file into its section \
         subdirectory. Non-`.ri` files at the root (e.g. README.md) are fine — \
         the tree documents itself. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        loose.len(),
        loose
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn fixture_tree_is_not_vacuous() {
    let root = fixtures_root();
    assert!(
        root.is_dir(),
        "D2 requires the language-spec conformance fixture tree at \
         `crates/reify-spec-conformance/fixtures/` (resolved here as {}), \
         but it does not exist or is not a directory. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );

    let mut found: Vec<PathBuf> = Vec::new();
    collect_ri(&root, &mut found);

    assert!(
        !found.is_empty(),
        "The fixture tree {} contains no `*.ri` files anywhere beneath it. \
         Without at least one resident, the no-loose-`.ri`-at-root contract \
         passes vacuously on an empty tree and stops pinning anything. \
         Add a fixture under a per-section subdirectory \
         (`crates/reify-spec-conformance/fixtures/<section>/<name>.ri`). \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );
}
