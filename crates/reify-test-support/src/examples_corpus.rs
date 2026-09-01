//! Discovery of the workspace `examples/` corpus, shared by the test suites
//! that walk it.
//!
//! Several suites in different crates need the same four things: the absolute
//! path to `<repo>/examples`, a recursive `*.ri` walk of it, the canonical
//! relative-path key form used by their skip lists and failure reports, and
//! the skip-list filter that turns "discovered" into "exercised". Before this
//! module they each carried a private copy, kept in sync by three
//! "Mirrors … update both when this changes" prose obligations. This module is
//! the single implementation those copies collapse into.
//!
//! It is the corpus-discovery sibling of [`crate::helpers::missing_paths_under`],
//! which single-sources the skip lists' dead-key check; the two together are
//! the whole shared surface of a skip-list-guarded corpus walk.
//!
//! Deliberately un-gated and dependency-free (`std::fs` / `std::path` only):
//! `crates/reify-audit` carries this crate as a NORMAL dependency, so anything
//! here that pulled a dependency closure would land in that binary's
//! production build graph (see the measured 130-vs-185-crate note in this
//! crate's `Cargo.toml`).

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// `examples_dir` names the real workspace `examples/` directory.
    ///
    /// Pins the load-bearing premise that this crate's `CARGO_MANIFEST_DIR`
    /// walk lands on the repo root: the final component is `examples`, it is a
    /// directory that actually exists on disk, and its parent is a repo root
    /// (it contains `crates/`). A crate relocation or workspace re-layout that
    /// silently changed this crate's depth would fail here.
    ///
    /// Deliberately does NOT pin a corpus file count — that is each consuming
    /// suite's own discovery-floor ratchet, not this crate's business.
    #[test]
    fn examples_dir_resolves_to_the_workspace_examples_directory() {
        let dir = super::examples_dir();

        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some("examples"),
            "expected the final component to be 'examples'; got {}",
            dir.display()
        );
        assert!(
            dir.is_dir(),
            "expected {} to be an existing directory — if this crate moved, the \
             CARGO_MANIFEST_DIR walk in examples_dir() needs updating, not this test",
            dir.display()
        );

        let repo_root = dir
            .parent()
            .expect("<repo>/examples has a parent (the repo root)");
        assert!(
            repo_root.join("crates").is_dir(),
            "expected examples_dir()'s parent ({}) to be the repo root, i.e. to contain a \
             'crates' directory",
            repo_root.display()
        );
    }

    /// `relative_to_examples_dir` produces the canonical skip-list key form for
    /// both top-level and nested paths.
    ///
    /// Pure lexical string work — the fixtures are constructed by `join`ing
    /// onto `examples_dir()` and need not exist on disk. This is the hoisted
    /// counterpart of the per-suite copies of this test.
    #[test]
    fn relative_to_examples_dir_strips_prefix_for_top_level_and_nested_paths() {
        let top_level = super::examples_dir().join("bracket.ri");
        let nested = super::examples_dir().join("fields/composed_stiffness.ri");

        assert_eq!(super::relative_to_examples_dir(&top_level), "bracket.ri");
        assert_eq!(
            super::relative_to_examples_dir(&nested),
            "fields/composed_stiffness.ri"
        );

        // The key is a plain relative path with no leading separator, so it
        // joins back onto the corpus root cleanly.
        let rel = super::relative_to_examples_dir(&nested);
        assert_eq!(
            super::examples_dir().join(&rel),
            nested,
            "expected the relative key to round-trip back onto examples_dir()"
        );
        let _ = Path::new(&rel);
    }
}
