//! Placement contract for the Ring-1 language-spec conformance fixture tree
//! (PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2; leaf beta #6759).
//!
//! The charter for this tree — why must-reject fixtures are chartered residents,
//! what `fixtures/_*/` means, and the full sentinel arrangement these tests
//! implement — is normative in `../fixtures/README.md`. Only what a reader of
//! THIS file needs in order to edit it correctly is repeated below.
//!
//! What is pinned here, and nothing more:
//!   * the tree exists and is a directory;
//!   * no loose `*.ri` sits directly at its root — strictly weaker than D2's
//!     one-directory-per-section shape, because section-directory naming and
//!     therefore per-section DEPTH belong to leaf gamma (#6761);
//!   * at least one `*.ri` exists somewhere beneath (non-vacuity);
//!   * the placement-probe sentinel is present and is still a live violator of
//!     the corpus-cleanliness guard it exists to keep honest;
//!   * that guard still carries its registered exclusion arm for this tree.
//!
//! These tests glob and deliberately never name a *conformance fixture's*
//! basename, so adding, renaming or removing a fixture stays inert to every
//! tracked `.rs` source. `_placement-probe/placement_probe.ri` is the ONE
//! documented exception: it is a sentinel rather than a conformance fixture, so
//! its identity is precisely the thing that must be pinned.
//!
//! Anti-vacuity (PRD D14): a scan written inline in a test body can only ever be
//! observed passing, so both directory scans are factored into named helpers and
//! fired against a synthetic violator tree in `helper_self_tests`.
//!
//! EDITING THIS FILE: it lives under `crates/**/*.rs` and is NOT covered by any
//! exclusion arm of `corpus_no_bare_scalar.rs` (only this crate's `fixtures/`
//! tree is), so a literal bare `Scalar` annotation on a non-comment line here
//! would red that guard for real. That guard's own unit tests are the place for
//! violating literals — its file is self-excluded from the scan.

use std::path::{Path, PathBuf};

/// The corpus guard's detection predicate, included from its single source so
/// this crate re-runs the REAL predicate rather than a mirror of it.
///
/// By `#[path]` and never by a Cargo dependency edge: a `reify-cli` dependency
/// would make this crate occt-touching (see `src/lib.rs`). Source inclusion adds
/// no edge, so `cargo tree -p reify-spec-conformance -e normal,dev` is unchanged
/// — while still leaving the sentinel below able to fire under a `.ri`-only
/// scope narrowing that never builds `reify-cli`.
#[path = "../../reify-cli/tests/harness_cli/bare_scalar_predicate.rs"]
mod bare_scalar_predicate;

use bare_scalar_predicate::line_has_bare_scalar;

/// Repo-relative path of the corpus-cleanliness guard this tree is registered
/// with. Spelled once; used by the sentinel and registration tests.
const CORPUS_GUARD_REL: &str = "crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs";

/// The token the guard carries beside its exclusion arm for this tree, as an
/// explicit machine-read contract between the two files.
///
/// Pinning the token rather than the arm's local binding or the predicate's name
/// is deliberate: both of those are private implementation detail of another
/// crate's test, and a pure rename there would red this test with a message
/// claiming the arm was lost. The token exists to be grepped and is documented
/// as such on the guard side.
const CORPUS_GUARD_MARKER: &str = "MARKER: spec-conformance-fixtures-exclusion-arm";

/// Resolve the workspace root from `CARGO_MANIFEST_DIR`.
///
/// This crate lives at `<root>/crates/reify-spec-conformance`, so the workspace
/// root is two levels up — the same idiom as the guard's own `workspace_root()`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must be accessible")
}

/// Resolve the fixture tree root and assert it exists, returning it.
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

/// Every `*.ri` sitting DIRECTLY at `dir` (never recursing), sorted.
///
/// Factored out of [`no_loose_ri_at_fixture_tree_root`] so the scan can be
/// fired at a synthetic violator tree — an inline scan is only ever observable
/// passing, which is the exact vacuity D14 forbids.
fn loose_ri_at_root(dir: &Path) -> Vec<PathBuf> {
    let rd = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("fixture tree {} must be readable: {e}", dir.display()));

    let mut loose: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() && path.extension().and_then(|e| e.to_str()) == Some("ri") {
            loose.push(path);
        }
    }
    loose.sort();
    loose
}

/// Is `path` a resident of a `fixtures/_*/` NON-SECTION directory?
///
/// The leading-underscore convention is the crate's (see `src/lib.rs`): a
/// directory directly under `fixtures/` whose name starts with `_` is not a
/// spec section, and leaf gamma's manifest generator must skip it.
fn is_non_section_resident(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|rel| rel.components().next())
        .and_then(|c| c.as_os_str().to_str())
        .is_some_and(|first| first.starts_with('_'))
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
    let loose = loose_ri_at_root(&root);

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
    found.sort();

    let section_residents = found
        .iter()
        .filter(|p| !is_non_section_resident(&root, p))
        .count();

    // WHAT THIS ASSERTS AT BETA, HONESTLY: `found` is non-empty today because
    // of `_placement-probe/placement_probe.ri` and nothing else. That resident
    // is a sentinel, NOT a conformance fixture — so this test currently
    // establishes only that the no-loose-`.ri`-at-root contract above has SOME
    // resident to be non-vacuous over, not that any spec section is populated.
    // Authoring the corpus is leaf eta's (#6765), not beta's, so asserting on
    // `section_residents` here would red for a reason nobody could fix at beta.
    // The count is computed anyway, and reported below, so that when eta lands
    // its first real section wave the tightening is a one-line change:
    // assert on `section_residents > 0` instead of `!found.is_empty()`.
    assert!(
        !found.is_empty(),
        "The fixture tree {} contains no `*.ri` files anywhere beneath it. \
         Without at least one resident, the no-loose-`.ri`-at-root contract \
         passes vacuously on an empty tree and stops pinning anything.\n\n\
         At leaf beta this is satisfied by the `_placement-probe/` sentinel \
         alone (section residents outside `fixtures/_*/`: {section_residents}) \
         — beta authors no corpus. If you are here because the probe was \
         deleted, restore it (see `fixtures/README.md`); if you are landing \
         real fixtures, add them under a per-section subdirectory \
         (`crates/reify-spec-conformance/fixtures/<section>/<name>.ri`). \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        root.display()
    );
}

// ---------------------------------------------------------------------------
// Sentinel: the placement probe must stay a LIVE violator of the corpus guard,
// and that guard must still register this tree. Rationale: `fixtures/README.md`.
//
// This crate-local half runs the guard's predicate from its single source, so
// it carries no drift surface; its distinct value over the guard-side half is
// that it still fires under a `.ri`-only scope narrowing that never builds
// `reify-cli`.
// ---------------------------------------------------------------------------

#[test]
fn placement_probe_sentinel_still_violates_the_corpus_guard() {
    let probe = placement_probe();

    let content = std::fs::read_to_string(&probe).unwrap_or_else(|e| {
        panic!(
            "The placement-probe sentinel {} must exist and be readable ({e}).\n\n\
             It is the permanently committed live violator sitting under the \
             registered exclusion arm for this tree in \
             `{CORPUS_GUARD_REL}`. \
             Delete it and that arm excludes a directory with nothing left in it \
             to exclude — vacuous, and no other test in the repo would notice. \
             Restore the probe, or retire the exclusion arm in the same change. \
             See crates/reify-spec-conformance/fixtures/README.md.",
            probe.display()
        )
    });

    assert!(
        content.lines().any(line_has_bare_scalar),
        "The placement-probe sentinel {} no longer carries a bare `Scalar` \
         annotation on a non-comment line, so it no longer trips the \
         corpus-cleanliness predicate in `{CORPUS_GUARD_REL}`.\n\n\
         That guard's registered exclusion arm for this tree is now vacuous: it \
         would stay green even if the arm were deleted, so the arm's stated \
         rationale (\"can never silently go vacuous\") no longer holds.\n\n\
         Do NOT \"fix\" the probe — its bare annotation is deliberate. Restore \
         it, or retire the exclusion arm in the same change. \
         See crates/reify-spec-conformance/fixtures/README.md.",
        probe.display()
    );
}

#[test]
fn corpus_guard_still_registers_this_tree() {
    let guard = workspace_root().join(CORPUS_GUARD_REL);

    let source = std::fs::read_to_string(&guard).unwrap_or_else(|e| {
        panic!(
            "The corpus-cleanliness guard {} is gone or unreadable ({e}).\n\n\
             It is the ONE repo-wide walker that reaches `crates/**/*.ri`, and \
             the whole purpose of this crate's `_placement-probe/` sentinel is \
             to keep that guard's registered exclusion arm for this tree \
             honest. With the guard retired, the probe guards nothing and \
             every test here would happily stay green over it.\n\n\
             The guard's own header anticipates this: it \"becomes \
             compiler-redundant once gamma adds E_BARE_SCALAR\". If that has \
             happened, retire the probe and these two sentinel tests in the \
             SAME change (and drop the exclusion note from \
             crates/reify-spec-conformance/fixtures/README.md) — do not leave \
             a sentinel standing watch over nothing.",
            guard.display()
        )
    });

    assert!(
        source.contains(CORPUS_GUARD_MARKER),
        "The corpus-cleanliness guard {} no longer carries the token \
         `{CORPUS_GUARD_MARKER}`, which it documents as the machine-read \
         contract marking its registered exclusion arm for this tree.\n\n\
         Either the arm was removed — in which case that guard should now be \
         RED on `_placement-probe/placement_probe.ri`; fix that, do not silence \
         this — or the token was renamed without updating this pin. The token \
         is deliberately not a source identifier precisely so that a refactor \
         of the guard cannot red this test by accident: if you moved it, keep \
         the token adjacent to the arm and update `CORPUS_GUARD_MARKER` here \
         and `crates/reify-spec-conformance/fixtures/README.md` to match. \
         See docs/prds/v0_6/spec-conformance-suite.md D2.",
        guard.display()
    );
}

// ── Anti-vacuity self-tests for the directory scans (PRD D14) ───────────────

#[cfg(test)]
mod helper_self_tests {
    use super::{collect_ri, is_non_section_resident, loose_ri_at_root};
    use std::path::{Path, PathBuf};

    /// A private scratch directory that deletes itself on drop.
    ///
    /// RAII rather than a trailing `remove_dir_all` after the assertion: a
    /// failing assertion unwinds past a trailing cleanup and leaks the tree
    /// under `CARGO_TARGET_TMPDIR`, which is exactly the accretion the warm-lane
    /// disk guards exist to catch — and a red test is when it would happen.
    ///
    /// Dependency-free on purpose: this crate's empty `[dependencies]` /
    /// `[dev-dependencies]` is load-bearing (it is what keeps it off the
    /// occt-touching set — see `src/lib.rs`), so reaching for `tempfile` here
    /// would be a real cost, not a convenience.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let base = option_env!("CARGO_TARGET_TMPDIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
            let dir = base.join(format!(
                "fixture_tree_selftest_{name}_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn join(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("scratch subdir must be creatable");
        }
        std::fs::write(path, "// scratch\n").expect("scratch file must be writable");
    }

    /// SEEDED FIRE: the root scan must report a loose `x.ri`, and must report
    /// ONLY it — not the nested `sub/y.ri` (it does not recurse) and not the
    /// non-`.ri` `README.md`. A typo'd extension check, a wrong `read_dir`
    /// target or an inverted `is_dir` test all red here.
    #[test]
    fn loose_ri_at_root_fires_on_a_seeded_violator() {
        let dir = Scratch::new("loose");
        touch(&dir.join("x.ri"));
        touch(&dir.join("sub").join("y.ri"));
        touch(&dir.join("README.md"));

        let loose = loose_ri_at_root(dir.path());

        assert_eq!(
            loose,
            vec![dir.join("x.ri")],
            "loose_ri_at_root must report exactly the root-level `.ri`; got {loose:?}"
        );
    }

    /// …and must report NOTHING on a clean tree, so the production assertion's
    /// green is a real green rather than a scan that never finds anything.
    #[test]
    fn loose_ri_at_root_is_silent_on_a_clean_tree() {
        let dir = Scratch::new("clean");
        touch(&dir.join("sub").join("y.ri"));
        touch(&dir.join("README.md"));

        assert!(
            loose_ri_at_root(dir.path()).is_empty(),
            "loose_ri_at_root must ignore nested `.ri` files and non-`.ri` files"
        );
    }

    /// SEEDED FIRE for the non-vacuity scan: it must recurse, and must pick up
    /// only `.ri` files.
    #[test]
    fn collect_ri_recurses_and_filters_by_extension() {
        let dir = Scratch::new("collect");
        touch(&dir.join("a").join("b").join("deep.ri"));
        touch(&dir.join("a").join("notes.md"));

        let mut found: Vec<PathBuf> = Vec::new();
        collect_ri(dir.path(), &mut found);

        assert_eq!(
            found,
            vec![dir.join("a").join("b").join("deep.ri")],
            "collect_ri must recurse and match only `.ri`; got {found:?}"
        );
    }

    /// The `fixtures/_*/` non-section rule keys on the FIRST component under
    /// the root only — a nested `_`-prefixed directory inside a real section is
    /// still a section resident.
    #[test]
    fn non_section_rule_keys_on_the_first_component_only() {
        let root = Path::new("/tmp/fixtures");
        assert!(is_non_section_resident(
            root,
            &root.join("_placement-probe").join("p.ri")
        ));
        assert!(!is_non_section_resident(
            root,
            &root.join("s09_2").join("case.ri")
        ));
        assert!(!is_non_section_resident(
            root,
            &root.join("s09_2").join("_scratch").join("case.ri")
        ));
    }
}
