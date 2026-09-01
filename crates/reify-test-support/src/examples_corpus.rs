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

use std::path::Path;

/// Absolute path to the workspace `examples/` directory.
///
/// Resolved at compile time from `CARGO_MANIFEST_DIR` evaluated inside **this**
/// crate, which always sits at `<repo>/crates/reify-test-support/` regardless of
/// which downstream crate calls the public API — the same resolution precedent
/// [`crate::orphan_audit`]'s `resolve_script_and_root` documents (and
/// [`crate::temp_dirs::assert_no_unguarded_temp_dir_sites`] reuses), so a helper
/// here can walk a corpus owned by the workspace rather than by any one crate.
///
/// # This `concat!` is the one definition of the lexical prefix
///
/// [`relative_to_examples_dir`] strips this path as a **lexical string**
/// prefix, so the walk root and the strip prefix must come from a single
/// definition or the round-trip contract silently breaks. A caller that
/// re-spelled `concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples")` in *its*
/// crate would produce a different byte string naming the same directory —
/// `…/crates/reify-compiler/../../examples` rather than
/// `…/crates/reify-test-support/../../examples` — and every `strip_prefix`
/// against it would panic. That is why this is a function returning a `Path`
/// and not a `pub const &str`: the mis-spelling is unrepresentable. Callers
/// wanting the old string form use `examples_dir().join(…)` or
/// `examples_dir().display()`, which serve every existing use.
///
/// # Deliberately not canonicalized
///
/// The returned path retains its `..` components. Canonicalizing would resolve
/// them and no longer match the paths [`discover_ri_files`] builds by walking
/// from this same root, breaking [`relative_to_examples_dir`]'s lexical
/// `strip_prefix`. Tests that need to compare this against a *differently
/// spelled* path to the same directory must canonicalize both sides
/// themselves.
pub fn examples_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"))
}

/// Strip the [`examples_dir`] prefix from `path` and return a portable,
/// forward-slash-separated relative path string.
///
/// For example:
/// - `<examples_dir>/bracket.ri`                   → `"bracket.ri"`
/// - `<examples_dir>/fields/composed_stiffness.ri` → `"fields/composed_stiffness.ri"`
///
/// This is the canonical form used as skip-list keys and in failure reports, so
/// that same-basename files in different subdirectories stay unambiguous. It is
/// also the key form [`crate::helpers::missing_paths_under`] expects, which is
/// what lets a suite check its skip list for dead keys with a plain
/// `dir.join(rel)`.
///
/// # Panics
///
/// Panics if `path` does not begin with the **lexical** [`examples_dir`]
/// prefix. Callers must pass paths produced by [`discover_ri_files`] — i.e.
/// paths constructed by walking [`examples_dir`] without canonicalization.
/// Canonicalized paths (which resolve `..` components) will not match the
/// lexical prefix string and will panic.
pub fn relative_to_examples_dir(path: &Path) -> String {
    let rel = path.strip_prefix(examples_dir()).unwrap_or_else(|e| {
        panic!(
            "reify_test_support::examples_corpus: '{}' is not under examples_dir ({}): {}",
            path.display(),
            examples_dir().display(),
            e
        )
    });
    rel.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
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
    }
}
