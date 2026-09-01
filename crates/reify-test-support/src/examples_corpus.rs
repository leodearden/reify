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

use std::path::{Path, PathBuf};

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

/// Return all `*.ri` files under `dir` (recursively), sorted by full path.
///
/// Call sites walking the workspace corpus pass [`examples_dir`]; the root is a
/// parameter rather than baked in so this is unit-testable against a fixture
/// tree instead of only against the live corpus (and to match
/// [`crate::helpers::missing_paths_under`], which likewise takes its directory
/// explicitly).
///
/// # Contracts callers may rely on
///
/// - **The result is sorted by full path.** Suites report discovered files in
///   failure messages and ratchet on their count, so a `read_dir`-order walk
///   would make those messages reorder between runs on the same corpus. The
///   sort is a determinism contract, not an incidental tidy-up.
/// - **Paths are returned uncanonicalized**, built by joining onto `dir`. That
///   is what keeps them valid inputs to [`relative_to_examples_dir`], whose
///   `strip_prefix` is lexical.
/// - **Only `.ri` file EXTENSIONS match.** A *directory* whose name ends in
///   `.ri` is recursed into, never pushed.
///
/// # Panics
///
/// Panics if any directory in the tree cannot be read, naming the directory and
/// the io error. A corpus walk that silently degraded to walking nothing would
/// turn every guard built on it vacuous, so an unreadable directory must fail
/// loudly.
pub fn discover_ri_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_ri_files(dir, &mut paths);
    paths.sort();
    paths
}

/// Recursively collect `*.ri` files under `dir` into `out`.
///
/// Private: the sort in [`discover_ri_files`] is part of the public contract,
/// and exposing the unsorted accumulator would let a caller opt out of it.
fn collect_ri_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "reify_test_support::examples_corpus: cannot read directory '{}': {}",
            dir.display(),
            e
        )
    });
    for entry in entries {
        let entry = entry.expect("IO error reading examples dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
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
    // ─── discover_ri_files contract ───────────────────────────────────────

    /// Prefix for every temp dir these `discover_ri_files` tests create, so
    /// SIGKILL debris under `/tmp` stays attributable to this suite (see
    /// `temp_dirs::prefixed_tempdir`'s "Names stay attributable" section).
    const EXAMPLES_CORPUS_TEMPDIR_PREFIX: &str = "reify-examples-corpus-";

    /// Materialise a fixture file at `dir.join(rel)`, creating any parent
    /// directories the forward-slash-separated `rel` implies.
    fn touch_under(dir: &std::path::Path, rel: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create parent dirs for fixture {rel:?}: {e}"));
        }
        std::fs::write(&path, "// fixture\n")
            .unwrap_or_else(|e| panic!("write fixture {rel:?}: {e}"));
    }

    /// discover_ri_files: over one fixture tree, exactly the `.ri` FILES at any
    /// depth come back, sorted by full path.
    ///
    /// One case carries the whole contract on purpose. The tree contains a
    /// top-level `.ri`, a nested `sub/deep/*.ri` (so a non-recursing walk
    /// fails), a non-`.ri` sibling (`notes.md`), and a `.ri`-suffixed
    /// DIRECTORY — which must be recursed into and never itself pushed, the
    /// one case where "ends in .ri" and "is a `.ri` file" diverge. Fixtures are
    /// created in an order that differs from their sorted order, and the
    /// comparison is made WITHOUT re-sorting the result, so an
    /// order-preserving-but-unsorted implementation cannot pass.
    #[test]
    fn discover_ri_files_returns_only_nested_ri_files_sorted_by_path() {
        let guard = crate::temp_dirs::prefixed_tempdir(EXAMPLES_CORPUS_TEMPDIR_PREFIX);
        let dir = guard.path();

        // Creation order is deliberately not sorted order.
        touch_under(dir, "zebra.ri");
        touch_under(dir, "sub/deep/nested.ri");
        touch_under(dir, "alpha.ri");
        touch_under(dir, "notes.md");
        touch_under(dir, "sub/notes.md");
        // A DIRECTORY whose name ends in `.ri`, holding a real `.ri` file.
        touch_under(dir, "looks_like_a_file.ri/inner.ri");

        let found = super::discover_ri_files(dir);

        assert_eq!(
            found,
            vec![
                dir.join("alpha.ri"),
                dir.join("looks_like_a_file.ri/inner.ri"),
                dir.join("sub/deep/nested.ri"),
                dir.join("zebra.ri"),
            ],
            "expected exactly the four `.ri` FILES at any depth, sorted by full path and \
             compared without re-sorting: `notes.md` and `sub/notes.md` must be filtered out \
             by extension, the `.ri`-named DIRECTORY must be recursed into rather than \
             pushed, and a walk that did not recurse would miss `sub/deep/nested.ri`; \
             got {found:?}"
        );
    }

    /// discover_ri_files: an empty directory yields an empty `Vec` rather than
    /// panicking, so a suite whose corpus is momentarily empty fails on its own
    /// discovery floor with a countable number, not on a walk panic.
    #[test]
    fn discover_ri_files_on_an_empty_directory_yields_an_empty_vec() {
        let guard = crate::temp_dirs::prefixed_tempdir(EXAMPLES_CORPUS_TEMPDIR_PREFIX);

        let found = super::discover_ri_files(guard.path());

        assert!(
            found.is_empty(),
            "expected an empty directory to yield an empty Vec; got {found:?}"
        );
    }
    // ─── filter_skipped contract ──────────────────────────────────────────

    /// filter_skipped: exactly the skipped keys are dropped, the rest come back
    /// in input order paired with their relative key.
    ///
    /// Inputs are constructed lexically under `examples_dir()` and need not
    /// exist on disk — filtering is pure key comparison. Paths are ordered
    /// skipped/kept/skipped/kept so neither a short-circuit on the first skip
    /// nor an off-by-one can pass, and the keys mix a top-level entry with
    /// nested forward-slash ones (the real skip-list key shape).
    ///
    /// Also pins that a skip key naming a path NOT in `paths` is silently inert:
    /// it filters nothing and is not an error. Detecting such a dead key is
    /// [`crate::helpers::missing_paths_under`]'s job, not this one — duplicating
    /// it here would give two guards one contract.
    #[test]
    fn filter_skipped_drops_exactly_the_skipped_keys_and_preserves_order() {
        let paths = vec![
            super::examples_dir().join("auto/skipped_top.ri"),
            super::examples_dir().join("kept_top.ri"),
            super::examples_dir().join("fields/skipped_nested.ri"),
            super::examples_dir().join("fields/kept_nested.ri"),
        ];

        let kept = super::filter_skipped(
            &paths,
            [
                "auto/skipped_top.ri",
                "fields/skipped_nested.ri",
                "never/appears/in/paths.ri",
            ],
        );

        assert_eq!(
            kept,
            vec![
                (&paths[1], "kept_top.ri".to_string()),
                (&paths[3], "fields/kept_nested.ri".to_string()),
            ],
            "expected exactly the two unskipped paths, in input order, each paired with its \
             precomputed relative key — and the dead skip key 'never/appears/in/paths.ri' to \
             be silently inert; got {kept:?}"
        );
    }

    /// filter_skipped: the same logical skip list projected out of a 2-tuple and
    /// out of a 3-tuple yields identical results.
    ///
    /// This is the behaviour that lets ONE implementation serve skip lists of
    /// differing arity in different crates — reify-compiler's
    /// `&[(&str, &str)]` and reify-eval's `&[(&str, SkipKind, &str)]` — while
    /// each stays private to its own crate. It mirrors
    /// `missing_paths_under`'s "# Arity is the caller's problem" contract
    /// (helpers.rs:62-69) and its arity-projection test.
    #[test]
    fn filter_skipped_is_agnostic_to_the_callers_skip_set_arity() {
        let paths = vec![
            super::examples_dir().join("skipped.ri"),
            super::examples_dir().join("auto/kept.ri"),
        ];

        // Stand-ins for the two real SKIP_SET shapes; the `u8` stands in for
        // reify-eval's `SkipKind`.
        let two_tuple: &[(&str, &str)] = &[("skipped.ri", "why it is skipped")];
        let three_tuple: &[(&str, u8, &str)] = &[("skipped.ri", 7, "why it is skipped")];

        let from_two = super::filter_skipped(&paths, two_tuple.iter().map(|(k, _)| *k));
        let from_three = super::filter_skipped(&paths, three_tuple.iter().map(|(k, _, _)| *k));

        assert_eq!(
            from_two,
            vec![(&paths[1], "auto/kept.ri".to_string())],
            "expected the 2-tuple projection to drop exactly the skipped path; got {from_two:?}"
        );
        assert_eq!(
            from_two, from_three,
            "expected the same logical skip list to produce identical results whether the \
             caller projects it out of a 2-tuple or a 3-tuple — that agnosticism is what lets \
             one implementation serve both crates' SKIP_SETs; got {from_two:?} vs {from_three:?}"
        );
    }

    /// filter_skipped: an empty skip iterator keeps every input path, so a suite
    /// with no skips still gets the (path, key) pairing this returns rather than
    /// having to special-case the empty list.
    #[test]
    fn filter_skipped_with_an_empty_skip_list_keeps_every_path() {
        let paths = vec![
            super::examples_dir().join("a.ri"),
            super::examples_dir().join("nested/b.ri"),
        ];
        let empty: [&str; 0] = [];

        let kept = super::filter_skipped(&paths, empty);

        assert_eq!(
            kept,
            vec![
                (&paths[0], "a.ri".to_string()),
                (&paths[1], "nested/b.ri".to_string()),
            ],
            "expected an empty skip list to keep every path, still paired with its relative \
             key; got {kept:?}"
        );
    }
}
