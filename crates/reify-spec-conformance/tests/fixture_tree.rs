//! Placement contract for the Ring-1 language-spec conformance fixture tree
//! (PRD `docs/prds/v0_6/spec-conformance-suite.md`, D2). Charter:
//! `../fixtures/README.md`.
//!
//! Pinned here: the tree exists; no loose `*.ri` sits directly at its root
//! (per-section DEPTH is leaf gamma's, #6761); at least one `*.ri` exists
//! beneath it. These tests glob and never name a fixture, so adding, renaming
//! or removing one stays inert to every tracked `.rs` source.
//!
//! Anti-vacuity (PRD D14): both directory scans are named helpers, fired
//! against a synthetic violator tree in `helper_self_tests`.

use std::path::{Path, PathBuf};

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

/// Walk `dir` recursively, appending every `*.ri` file to `out`. Silently
/// skips unreadable entries.
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

    // At beta the only resident is the `_placement-probe/` sentinel, not a
    // conformance fixture, so this pins "not an empty tree" rather than "a
    // section is populated". Leaf eta (#6765) authors the corpus; its first
    // section wave tightens this to `section_residents > 0`.
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

// ── Anti-vacuity self-tests for the directory scans (PRD D14) ───────────────

#[cfg(test)]
mod helper_self_tests {
    use super::{collect_ri, is_non_section_resident, loose_ri_at_root};
    use std::path::{Path, PathBuf};

    /// A private scratch directory that deletes itself on drop, so a failing
    /// assertion cannot leak it. Hand-rolled rather than `tempfile`: the
    /// dependency-free manifest is what keeps this crate off the occt-touching
    /// set (`src/lib.rs`, Obligation 1).
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
