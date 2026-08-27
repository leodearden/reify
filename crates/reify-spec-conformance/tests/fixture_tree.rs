//! Placement contract for the Ring-1 language-spec conformance fixture tree
//! (PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2; leaf beta #6759).
//!
//! D2 mandates `crates/reify-spec-conformance/fixtures/<section>/<name>.ri` —
//! per-section subdirectories, never a flat pile at the tree root. These tests
//! pin the STRUCTURE of the tree, not its contents: they glob and deliberately
//! never name a *conformance fixture's* basename, so adding, renaming or
//! removing a fixture stays inert to every tracked `.rs` source (which is what
//! keeps a future basename-derived coupling set from ever keying on this tree).
//!
//! What is pinned at beta, stated precisely — the messages below claim no more:
//!   * the tree exists and is a directory;
//!   * **no loose `*.ri` sits directly at its root**. That is strictly weaker
//!     than D2's one-directory-per-section shape: a fixture nested arbitrarily
//!     deep (`fixtures/a/b/c.ri`) passes here today. Pinning the per-section
//!     DEPTH belongs to leaf gamma (#6761), which decides section-directory
//!     naming and writes the manifest generator that has to parse it — pinning
//!     a depth before that decision exists would just be guessing at it;
//!   * at least one `*.ri` exists somewhere beneath (non-vacuity, without which
//!     the previous contract passes trivially on an empty tree);
//!   * the placement-probe sentinel is present and is still a live violator of
//!     the corpus-cleanliness guard it exists to keep honest.
//!
//! That last test is the ONE documented exception to the never-name-a-basename
//! rule above. `_placement-probe/` is not a spec section (the leading `_` marks
//! it as such) and its resident is not a conformance fixture: it is the sentinel
//! that stops the registered exclusion arm in
//! `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` from silently
//! going vacuous, so its identity is precisely the thing that must be pinned.
//! Naming this one basename is safe: the basename-derived coupling machinery
//! (`_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`, and PG-DRIFT's
//! derivation in `tests/infra/test_verify_scope.sh`) is hard-scoped to
//! `tests/prd-gate/fixtures/`, so a spec-conformance basename can never key
//! into it.

use std::path::{Path, PathBuf};

/// Resolve the fixture tree root and assert it exists, returning it.
///
/// Mirrors the `env!("CARGO_MANIFEST_DIR")` idiom used by
/// `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`'s
/// `workspace_root()`, but resolves the crate-local tree directly rather than
/// walking up to the workspace root.
///
/// Every test here needs the same precondition with the same remedy, so it
/// lives once: the D2 citation and the path spelling have exactly one home.
fn require_fixtures_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    assert!(
        root.is_dir(),
        "D2 requires the language-spec conformance fixture tree at \
         `crates/reify-spec-conformance/fixtures/` (resolved here as {}), \
         but it does not exist or is not a directory. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );
    root
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

/// The placement-probe sentinel — see the module doc for why this one basename
/// is spelled out while conformance fixtures deliberately are not.
fn placement_probe() -> PathBuf {
    require_fixtures_root()
        .join("_placement-probe")
        .join("placement_probe.ri")
}

#[test]
fn fixture_tree_exists() {
    let _ = require_fixtures_root();
}

#[test]
fn no_loose_ri_at_fixture_tree_root() {
    let root = require_fixtures_root();

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
         Every conformance fixture must live in a subdirectory of \
         `crates/reify-spec-conformance/fixtures/`, never flat at the root — \
         D2's shape is `fixtures/<section>/<name>.ri`. Move each file into a \
         subdirectory. Non-`.ri` files at the root (e.g. README.md) are fine: \
         the tree documents itself.\n\n\
         Scope of this check: it enforces only \"not directly at the root\". \
         Nesting depth below the root is NOT checked here, because \
         section-directory naming is deferred to leaf gamma (#6761); gamma's \
         manifest generator is what pins the per-section depth. \
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
    let root = require_fixtures_root();

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

// ---------------------------------------------------------------------------
// Sentinel: the placement probe must stay a LIVE violator of the corpus guard.
//
// `corpus_no_bare_scalar.rs` carries a registered exclusion arm for this whole
// tree, and that arm's rationale asserts it "can never silently go vacuous"
// because a permanently committed live violator sits under it. Nothing but the
// test below actually holds that up. Without it, migrating the probe's bare
// annotation to `Length` — a wholly plausible drive-by cleanup — leaves every
// test in the repo green while the exclusion quietly stops excluding anything.
// ---------------------------------------------------------------------------

/// Byte-for-byte mirror of `line_has_bare_scalar` in
/// `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`, including its
/// two helpers (`strip_trailing_line_comment`, `is_rust_debug_scalar_field`).
///
/// A re-implementation rather than a substring match, on purpose: the property
/// worth pinning is "the guard's predicate still fires on this file", and only
/// the predicate's own shape expresses that. A substring test would pass on a
/// pure-comment mention, which the guard skips.
///
/// It is duplicated rather than shared because `reify-spec-conformance` has
/// empty `[dependencies]` at beta; depending on `reify-cli` to reach the helper
/// would make this crate occt-touching and drag in the hand-synced pair
/// documented in this crate's `src/lib.rs`. If the two ever disagree, this test
/// fails loudly (the guard would be green while the sentinel reds), which is the
/// safe direction for a drift to break in.
fn line_is_bare_scalar(line: &str) -> bool {
    const SCALAR: &str = "Scalar";

    // Pure comment lines are skipped by the guard, so they cannot keep the
    // exclusion arm alive either.
    if line.trim_start().starts_with("//") {
        return false;
    }
    let line = strip_trailing_line_comment(line);

    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find(SCALAR) {
        let abs = search_start + rel;
        let after_ok = match line[abs + SCALAR.len()..].chars().next() {
            None => true,
            Some(c) => c != '<' && !c.is_ascii_alphabetic(),
        };
        if after_ok && !is_rust_debug_scalar_field(line, abs) {
            let before_trimmed = line[..abs].trim_end_matches(' ');
            // (a) bare annotation: a single `:` introducer, never `::`.
            if before_trimmed.ends_with(':') && !before_trimmed.ends_with("::") {
                return true;
            }
            // (b) bare return codomain.
            if before_trimmed.ends_with("->") {
                return true;
            }
        }
        search_start = abs + SCALAR.len();
    }
    false
}

/// Mirror of the guard's `strip_trailing_line_comment` (preserves `://`).
fn strip_trailing_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'/' && (i == 0 || bytes[i - 1] != b':') {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Mirror of the guard's `is_rust_debug_scalar_field` (whole-line
/// `<indent><ident>: Scalar {` pretty-`Debug` struct-variant opener).
fn is_rust_debug_scalar_field(line: &str, abs: usize) -> bool {
    if line[abs + "Scalar".len()..].trim_end() != " {" {
        return false;
    }
    let Some(head) = line[..abs].strip_suffix(": ") else {
        return false;
    };
    let ident = head.trim_start_matches([' ', '\t']);
    !ident.is_empty()
        && ident.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[test]
fn placement_probe_sentinel_still_violates_the_corpus_guard() {
    let probe = placement_probe();

    let content = std::fs::read_to_string(&probe).unwrap_or_else(|e| {
        panic!(
            "The placement-probe sentinel {} must exist and be readable ({e}).\n\n\
             It is the permanently committed live violator sitting under the \
             registered exclusion arm for this tree in \
             `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`. \
             Delete it and that arm excludes a directory with nothing left in it \
             to exclude — vacuous, and no other test in the repo would notice. \
             Restore the probe, or retire the exclusion arm in the same change. \
             See crates/reify-spec-conformance/fixtures/README.md.",
            probe.display()
        )
    });

    assert!(
        content.lines().any(line_is_bare_scalar),
        "The placement-probe sentinel {} no longer carries a bare `Scalar` \
         annotation on a non-comment line, so it no longer trips the \
         corpus-cleanliness predicate in \
         `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`.\n\n\
         That guard's registered exclusion arm for this tree is now vacuous: it \
         would stay green even if the arm were deleted, so the arm's stated \
         rationale (\"can never silently go vacuous\") no longer holds.\n\n\
         Do NOT \"fix\" the probe — its bare annotation is deliberate. Restore \
         it, or retire the exclusion arm in the same change. \
         See crates/reify-spec-conformance/fixtures/README.md.",
        probe.display()
    );
}
