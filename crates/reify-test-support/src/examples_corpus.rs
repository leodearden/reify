//! Discovery of the workspace `examples/` corpus, shared by the test suites
//! that walk it.
//!
//! Four pieces, used together: [`examples_dir`], the absolute corpus root;
//! [`discover_ri_files`], a sorted recursive `*.ri` walk; [`relative_to`] /
//! [`relative_to_examples_dir`], the forward-slash relative key that skip lists
//! and failure reports use; and [`filter_skipped_under`] / [`filter_skipped`],
//! which turn "discovered" into "exercised". The skip lists' dead-key check is
//! the sibling [`crate::skip_sets::missing_paths_under`].
//!
//! Every function that pairs a walk root with a strip prefix takes that root
//! once, so the two are always the same spelling; the [`relative_to_examples_dir`]
//! and [`filter_skipped`] wrappers bind it to [`examples_dir`].
//!
//! Dependency-free (`std` only): `crates/reify-audit` carries this crate as a
//! NORMAL dependency, so anything pulled in here would land in that binary's
//! production build graph (see this crate's `Cargo.toml`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Absolute path to the workspace `examples/` directory.
///
/// Resolved at compile time from `CARGO_MANIFEST_DIR` evaluated inside **this**
/// crate, which sits at `<repo>/crates/reify-test-support/` whichever crate
/// calls it — the resolution precedent [`crate::orphan_audit`]'s
/// `resolve_script_and_root` documents.
///
/// # The one definition of the lexical prefix
///
/// [`relative_to_examples_dir`] strips this path as a **lexical** prefix, so the
/// walk root and the strip prefix must both come from this one definition. A
/// caller re-spelling the `concat!` in its own crate gets a different byte
/// string for the same directory (`…/crates/reify-compiler/../../examples`), and
/// every `strip_prefix` against it panics. That is why this returns a `Path`
/// rather than exposing a `pub const &str` to copy.
///
/// # Deliberately not canonicalized
///
/// The returned path keeps its `..` components, so it lexically prefixes every
/// path [`discover_ri_files`] builds from it. Canonicalizing one side alone
/// breaks that pairing; a test comparing this against a differently spelled
/// path to the same directory must canonicalize both sides.
pub fn examples_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"))
}

/// Strip the `root` prefix from `path` and return a portable,
/// forward-slash-separated relative path string.
///
/// With `root` = [`examples_dir`]:
/// - `<root>/bracket.ri`                   → `"bracket.ri"`
/// - `<root>/fields/composed_stiffness.ri` → `"fields/composed_stiffness.ri"`
///
/// This is the skip-list key and failure-report form, so same-basename files in
/// different subdirectories stay distinct, and it is the key form
/// [`crate::skip_sets::missing_paths_under`] expects (`root.join(rel)`).
///
/// `root` is a parameter so that the walk root and the stripped prefix can be
/// the same value over a hermetic fixture tree or a corpus sub-root such as
/// `examples/best_practices`. Whole-corpus callers use
/// [`relative_to_examples_dir`].
///
/// # Panics
///
/// If `path` does not begin with the **lexical** `root` prefix. Pass paths
/// produced by `discover_ri_files(root)`; canonicalizing one side alone
/// resolves `..` components and breaks the pairing.
pub fn relative_to(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or_else(|e| {
        panic!(
            "reify_test_support::examples_corpus: '{}' is not under the corpus root '{}': {}",
            path.display(),
            root.display(),
            e
        )
    });
    rel.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// [`relative_to`] bound to [`examples_dir`]: the workspace-corpus key form.
///
/// Binding the root here, rather than having each call site spell
/// `examples_dir()`, is what keeps every whole-corpus caller's strip prefix
/// identical to its walk root.
///
/// # Panics
///
/// Via [`relative_to`], if `path` was neither produced by
/// `discover_ri_files(examples_dir())` nor built by joining onto
/// `examples_dir()`.
pub fn relative_to_examples_dir(path: &Path) -> String {
    relative_to(examples_dir(), path)
}

/// Return all `*.ri` files under `dir` (recursively), sorted by full path.
///
/// Whole-corpus callers pass [`examples_dir`]; the root is a parameter so the
/// walk is testable against a fixture tree.
///
/// # Contracts callers may rely on
///
/// - **Sorted by full path.** Suites print discovered files in failure messages
///   and ratchet on their count, so the order must not depend on `read_dir`.
/// - **Uncanonicalized**, built by joining onto `dir`, so each path is a valid
///   input to [`relative_to`] with that same `dir`.
/// - **Only `.ri` FILES match.** A directory whose name ends in `.ri` is
///   recursed into, never returned.
/// - **Symlinks are never descended into.** Recursion is decided by the entry's
///   own `file_type`, which does not follow links, so a symlinked directory
///   cycle cannot make the walk revisit the tree it is walking. A symlink is
///   classified by its own name, so a symlinked `foo.ri` is returned like any
///   other `.ri` file; a symlinked directory is walked only if passed as `dir`.
///
/// # Panics
///
/// If any directory, directory entry, or entry file type cannot be read, naming
/// the path and the io error. A walk that silently skipped an unreadable
/// directory would make every guard built on it vacuous.
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
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "reify_test_support::examples_corpus: cannot read an entry of '{}': {}",
                dir.display(),
                e
            )
        });
        let path = entry.path();
        // Not `path.is_dir()`: that follows symlinks (see the symlink contract).
        let file_type = entry.file_type().unwrap_or_else(|e| {
            panic!(
                "reify_test_support::examples_corpus: cannot read file type of '{}': {}",
                path.display(),
                e
            )
        });
        if file_type.is_dir() {
            collect_ri_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ri") {
            out.push(path);
        }
    }
}

/// The subset of `paths` whose [`relative_to`]-against-`root` key is not in
/// `skip_keys`, each paired with that key.
///
/// The single definition of a suite's skip-list-filtered "exercised" set: its
/// corpus-walking tests and its discovery-floor ratchets all call this, so they
/// cannot disagree about what was exercised.
///
/// # Contracts callers may rely on
///
/// - **Input order is preserved**, so a [`discover_ri_files`] result stays
///   path-sorted.
/// - **The relative key is computed once, here**, and returned, so callers
///   cannot compute it differently.
/// - **A skip key naming no path in `paths` is silently inert.** Detecting such
///   a dead key is [`crate::skip_sets::missing_paths_under`]'s job.
/// - **`root` must be the root `paths` were walked from** (see [`relative_to`]).
///   Whole-corpus callers use [`filter_skipped`].
///
/// # Skip lists of any arity
///
/// Skip lists carry per-entry metadata of differing shape, so this takes a
/// plain iterator of keys and each caller projects its own tuple away at the
/// call boundary — `SKIP_SET.iter().map(|(name, _)| *name)` — which keeps every
/// skip list private to its own crate. `skip_keys` has its own lifetime so that
/// a borrow of a `'static` SKIP_SET need not be tied to the `paths` borrow.
///
/// # Panics
///
/// Via [`relative_to`], if any path is not lexically rooted under `root`.
pub fn filter_skipped_under<'p, 'k>(
    root: &Path,
    paths: &'p [PathBuf],
    skip_keys: impl IntoIterator<Item = &'k str>,
) -> Vec<(&'p PathBuf, String)> {
    let skip: HashSet<&str> = skip_keys.into_iter().collect();
    paths
        .iter()
        .filter_map(|p| {
            let rel = relative_to(root, p);
            if skip.contains(rel.as_str()) {
                None
            } else {
                Some((p, rel))
            }
        })
        .collect()
}

/// [`filter_skipped_under`] bound to [`examples_dir`]: the workspace-corpus
/// "exercised" set, with all of [`filter_skipped_under`]'s contracts.
///
/// # Panics
///
/// Via [`filter_skipped_under`], if any path is not lexically rooted under
/// [`examples_dir`].
pub fn filter_skipped<'p, 'k>(
    paths: &'p [PathBuf],
    skip_keys: impl IntoIterator<Item = &'k str>,
) -> Vec<(&'p PathBuf, String)> {
    filter_skipped_under(examples_dir(), paths, skip_keys)
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
    /// onto `examples_dir()` and need not exist on disk.
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

    /// Over the LIVE corpus, every path `discover_ri_files(examples_dir())`
    /// returns is accepted by `relative_to_examples_dir`, and its key joins back
    /// onto `examples_dir()` to reproduce the path.
    ///
    /// The fixture tests below pin the walk and the key against trees this
    /// suite controls; this pins that the real corpus root and the real walk
    /// still pair lexically — a walker that started canonicalizing, or a root
    /// spelled two ways, would panic or fail the join here. The non-empty check
    /// keeps the loop from passing vacuously.
    #[test]
    fn relative_to_examples_dir_round_trips_every_discovered_corpus_path() {
        let paths = super::discover_ri_files(super::examples_dir());
        assert!(
            !paths.is_empty(),
            "expected the live corpus at {} to hold at least one .ri file",
            super::examples_dir().display()
        );

        for path in paths {
            let rel = super::relative_to_examples_dir(&path);
            assert_eq!(
                super::examples_dir().join(&rel),
                path,
                "round-trip failed: examples_dir().join({rel:?}) != original {path:?}"
            );
        }
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

    /// discover_ri_files: a symlinked `.ri` file is collected, a symlinked
    /// directory is never descended into.
    ///
    /// `sub/loop -> ..` is a directory cycle: a link-following walk
    /// (`path.is_dir()`) re-enters the root through it, returning
    /// `sub/loop/real.ri`, `sub/loop/sub/loop/…` and so on until the OS refuses
    /// the path. `link.ri -> real.ri` must still come back like any `.ri` file.
    #[cfg(unix)]
    #[test]
    fn discover_ri_files_collects_symlinked_files_but_never_descends_symlinked_dirs() {
        let guard = crate::temp_dirs::prefixed_tempdir(EXAMPLES_CORPUS_TEMPDIR_PREFIX);
        let dir = guard.path();

        touch_under(dir, "real.ri");
        touch_under(dir, "sub/inner.ri");
        std::os::unix::fs::symlink("real.ri", dir.join("link.ri"))
            .unwrap_or_else(|e| panic!("symlink link.ri -> real.ri: {e}"));
        std::os::unix::fs::symlink("..", dir.join("sub/loop"))
            .unwrap_or_else(|e| panic!("symlink sub/loop -> ..: {e}"));

        let found = super::discover_ri_files(dir);

        assert_eq!(
            found,
            vec![
                dir.join("link.ri"),
                dir.join("real.ri"),
                dir.join("sub/inner.ri"),
            ],
            "expected the symlinked `link.ri` to be collected and the `sub/loop -> ..` cycle \
             never descended into; got {found:?}"
        );
    }

    /// discover_ri_files: an unreadable root fails LOUDLY.
    ///
    /// The documented contract is that a walk which cannot read a directory
    /// panics rather than yielding nothing, because "walked nothing" and
    /// "corpus is empty" are indistinguishable downstream and would make every
    /// guard built on this vacuously pass. Nothing else in the suite exercises
    /// it, so a future refactor swapping the `unwrap_or_else(panic)` for a
    /// `.unwrap_or_default()` or `filter_map(Result::ok)` would go green while
    /// silently hollowing out every corpus gate in the workspace.
    #[test]
    #[should_panic(expected = "cannot read directory")]
    fn discover_ri_files_panics_on_an_unreadable_directory() {
        let guard = crate::temp_dirs::prefixed_tempdir(EXAMPLES_CORPUS_TEMPDIR_PREFIX);

        // A path under a real temp dir that was never created: `read_dir` fails
        // with ENOENT, which must surface as a panic naming the directory.
        let _ = super::discover_ri_files(&guard.path().join("does-not-exist"));
    }

    /// relative_to_examples_dir: an off-corpus path fails LOUDLY.
    ///
    /// The other half of the "never silently degrade" pair above. A path not
    /// lexically rooted under `examples_dir()` has no relative key, and the
    /// contract is to panic naming both paths rather than to invent one — a
    /// silently-wrong key would miss its skip-list entry and be reported under
    /// a name that joins back onto nothing.
    #[test]
    #[should_panic(expected = "is not under the corpus root")]
    fn relative_to_examples_dir_panics_on_a_path_outside_the_corpus() {
        let _ = super::relative_to_examples_dir(std::path::Path::new("/tmp/elsewhere/x.ri"));
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

    /// A corpus root that is deliberately NOT `examples_dir()`.
    ///
    /// Filtering is pure lexical key comparison, so these fixtures need not
    /// exist on disk — but rooting them somewhere other than the live corpus is
    /// what proves `filter_skipped_under`/`relative_to` really are
    /// root-parameterized rather than quietly pinned to the workspace corpus.
    fn fixture_root() -> &'static std::path::Path {
        std::path::Path::new("/fixture-corpus")
    }

    /// filter_skipped_under: exactly the skipped keys are dropped, the rest come
    /// back in input order paired with their relative key.
    ///
    /// Rooted at [`fixture_root`], not the live corpus. Paths are ordered
    /// skipped/kept/skipped/kept so neither a short-circuit on the first skip
    /// nor an off-by-one can pass, and the keys mix a top-level entry with
    /// nested forward-slash ones (the real skip-list key shape).
    ///
    /// Also pins that a skip key naming a path NOT in `paths` is silently inert:
    /// it filters nothing and is not an error. Detecting such a dead key is
    /// [`crate::skip_sets::missing_paths_under`]'s job, not this one — duplicating
    /// it here would give two guards one contract.
    #[test]
    fn filter_skipped_drops_exactly_the_skipped_keys_and_preserves_order() {
        let paths = vec![
            fixture_root().join("auto/skipped_top.ri"),
            fixture_root().join("kept_top.ri"),
            fixture_root().join("fields/skipped_nested.ri"),
            fixture_root().join("fields/kept_nested.ri"),
        ];

        let kept = super::filter_skipped_under(
            fixture_root(),
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

    /// filter_skipped_under: the same logical skip list projected out of a
    /// 2-tuple and out of a 3-tuple yields identical results.
    ///
    /// This is the behaviour that lets ONE implementation serve skip lists of
    /// differing arity in different crates — reify-compiler's
    /// `&[(&str, &str)]` and reify-eval's `&[(&str, SkipKind, &str)]` — while
    /// each stays private to its own crate. It mirrors the "# Arity is the
    /// caller's problem" contract of `skip_sets.rs`'s `missing_paths_under`.
    #[test]
    fn filter_skipped_is_agnostic_to_the_callers_skip_set_arity() {
        let paths = vec![
            fixture_root().join("skipped.ri"),
            fixture_root().join("auto/kept.ri"),
        ];

        // Stand-ins for the two real SKIP_SET shapes; the `u8` stands in for
        // reify-eval's `SkipKind`.
        let two_tuple: &[(&str, &str)] = &[("skipped.ri", "why it is skipped")];
        let three_tuple: &[(&str, u8, &str)] = &[("skipped.ri", 7, "why it is skipped")];

        let from_two =
            super::filter_skipped_under(fixture_root(), &paths, two_tuple.iter().map(|(k, _)| *k));
        let from_three = super::filter_skipped_under(
            fixture_root(),
            &paths,
            three_tuple.iter().map(|(k, _, _)| *k),
        );

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
    ///
    /// Deliberately the one test on the `examples_dir()`-bound WRAPPER rather
    /// than on `filter_skipped_under`: it is what pins that the wrapper passes
    /// the workspace corpus root through, which the fixture-rooted tests above
    /// cannot see.
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

    /// The walk and the filter compose over one fixture tree, end to end.
    ///
    /// This is the composition the root parameters exist for: a
    /// `discover_ri_files(root)` result skip-filtered against that same `root`,
    /// on a tree whose exact contents this test controls.
    #[test]
    fn discovered_paths_can_be_skip_filtered_against_the_tree_they_were_walked_from() {
        let guard = crate::temp_dirs::prefixed_tempdir(EXAMPLES_CORPUS_TEMPDIR_PREFIX);
        let root = guard.path();

        touch_under(root, "kept.ri");
        touch_under(root, "sub/skipped.ri");
        touch_under(root, "sub/kept_nested.ri");

        let discovered = super::discover_ri_files(root);
        let exercised = super::filter_skipped_under(root, &discovered, ["sub/skipped.ri"]);

        let keys: Vec<&str> = exercised.iter().map(|(_, rel)| rel.as_str()).collect();
        assert_eq!(
            keys,
            vec!["kept.ri", "sub/kept_nested.ri"],
            "expected the walk's own output to feed straight into the filter over the same \
             root, with `sub/skipped.ri` dropped by its relative key; got {keys:?}"
        );
        assert!(
            exercised.iter().all(|(path, _)| path.starts_with(root)),
            "expected the returned paths to be the walked ones, still rooted at {}",
            root.display()
        );
    }
}
