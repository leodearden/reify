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
//!     the previous contract passes trivially on an empty tree). At beta the
//!     ONLY resident is the placement-probe sentinel, so the sentinel is what
//!     satisfies it — deliberately, and spelled out in that test's own message;
//!   * the placement-probe sentinel is present and is still a live violator of
//!     the corpus-cleanliness guard it exists to keep honest;
//!   * that guard still EXISTS and still registers this tree — without which
//!     the sentinel would be standing watch over nothing.
//!
//! Anti-vacuity (PRD D14: seeded-fire self-tests are mandatory for gates this
//! PRD adds). A scan written inline in a test body can only ever be observed
//! passing, so both directory scans are factored into named helpers and fired
//! against a synthetic violator tree in `helper_self_tests`; the mirrored
//! predicate is fired against the guard's own discriminating cases in
//! `mirror_predicate_tests`.
//!
//! EDITING THIS FILE: it lives under `crates/**/*.rs` and is deliberately NOT
//! covered by any exclusion arm of `corpus_no_bare_scalar.rs` (only this
//! crate's `fixtures/` tree is), so a literal bare `Scalar` annotation written
//! on a non-comment line here would red that guard for real. The unit cases
//! below therefore splice the keyword in at runtime (`with_scalar`) rather than
//! spelling it out — that is not obfuscation, it is the only way this file can
//! carry violating examples without itself becoming a violation.
//!
//! The sentinel test is the ONE documented exception to the
//! never-name-a-basename rule above. `_placement-probe/` is not a spec section
//! (the leading `_` marks it as such) and its resident is not a conformance
//! fixture: it is the sentinel that stops the registered exclusion arm in
//! `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` from silently
//! going vacuous, so its identity is precisely the thing that must be pinned.
//! Naming this one basename is safe: the basename-derived coupling machinery
//! (`_RUST_COUPLED_RI_FIXTURES` in `scripts/verify.sh`, and PG-DRIFT's
//! derivation in `tests/infra/test_verify_scope.sh`) is hard-scoped to
//! `tests/prd-gate/fixtures/`, so a spec-conformance basename can never key
//! into it.

use std::path::{Path, PathBuf};

/// Repo-relative path of the corpus-cleanliness guard this tree is registered
/// with. Spelled once; used by the sentinel and registration tests.
const CORPUS_GUARD_REL: &str = "crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs";

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
// Sentinel: the placement probe must stay a LIVE violator of the corpus guard.
//
// `corpus_no_bare_scalar.rs` carries a registered exclusion arm for this whole
// tree, and that arm's rationale asserts it "can never silently go vacuous"
// because a permanently committed live violator sits under it. Without a test,
// migrating the probe's bare annotation to `Length` — a wholly plausible
// drive-by cleanup — would leave every test in the repo green while the
// exclusion quietly stopped excluding anything.
//
// TWO tests hold that up, in two different crates, on purpose:
//   * `spec_conformance_placement_probe_is_a_live_violator` in the GUARD ITSELF
//     runs the REAL predicate over the probe — zero drift surface, and it is
//     the authoritative one;
//   * the mirrored check below is the fast-feedback copy, and is what still
//     fires under a `.ri`-only scope narrowing that never builds `reify-cli`.
// ---------------------------------------------------------------------------

/// Mirror of `line_has_bare_scalar` in
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
/// documented in this crate's `src/lib.rs`.
///
/// DRIFT, STATED HONESTLY — this copy is safe in ONE direction only:
///   * mirror STRICTER than the guard (or the probe migrated away from a bare
///     annotation) → this test reds while the guard stays green. Safe: a human
///     is sent to look, and the message says what to look at.
///   * mirror LOOSER than the guard, or the guard's predicate later narrowed by
///     a new carve-out, or the guard DELETED outright (its own header
///     anticipates becoming compiler-redundant once gamma adds `E_BARE_SCALAR`)
///     → this test would stay green over a probe that no longer guards
///     anything. That direction is NOT covered by this function, and is why two
///     further things exist: `mirror_predicate_tests` below fires the guard's
///     own discriminating cases at this copy so a looser mirror reds here, and
///     `corpus_guard_still_registers_this_tree` reds if the guard file or its
///     registration of this tree disappears.
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
             `{CORPUS_GUARD_REL}`. \
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

    // `spec_conformance_fixtures` is the exclusion arm's own binding, and is
    // the sharp needle here: the bare `reify-spec-conformance` path literal
    // would survive the arm's deletion (the guard's own sentinel test names the
    // probe path too), whereas the binding would not.
    for needle in [
        "line_has_bare_scalar",
        "spec_conformance_fixtures",
        "reify-spec-conformance",
    ] {
        assert!(
            source.contains(needle),
            "The corpus-cleanliness guard {} no longer mentions `{needle}`, so \
             it has most likely lost its registered exclusion arm for this \
             tree (or renamed the predicate this crate mirrors).\n\n\
             If the arm was deliberately re-pathed, update this pin and \
             `crates/reify-spec-conformance/fixtures/README.md` to match. If it \
             was removed, the guard should now be RED on \
             `_placement-probe/placement_probe.ri` — fix that, do not silence \
             this. See docs/prds/v0_6/spec-conformance-suite.md D2.",
            guard.display()
        );
    }
}

// ── Anti-vacuity self-tests for the directory scans (PRD D14) ───────────────

#[cfg(test)]
mod helper_self_tests {
    use super::{collect_ri, is_non_section_resident, loose_ri_at_root};
    use std::path::{Path, PathBuf};

    /// A private scratch directory, created empty. Dependency-free on purpose:
    /// this crate's empty `[dependencies]`/`[dev-dependencies]` is load-bearing
    /// (it is what keeps it off the occt-touching set — see `src/lib.rs`), so
    /// reaching for `tempfile` here would be a real cost, not a convenience.
    fn scratch(name: &str) -> PathBuf {
        let base = option_env!("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!(
            "fixture_tree_selftest_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
        dir
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
        let dir = scratch("loose");
        touch(&dir.join("x.ri"));
        touch(&dir.join("sub").join("y.ri"));
        touch(&dir.join("README.md"));

        let loose = loose_ri_at_root(&dir);

        assert_eq!(
            loose,
            vec![dir.join("x.ri")],
            "loose_ri_at_root must report exactly the root-level `.ri`; got {loose:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// …and must report NOTHING on a clean tree, so the production assertion's
    /// green is a real green rather than a scan that never finds anything.
    #[test]
    fn loose_ri_at_root_is_silent_on_a_clean_tree() {
        let dir = scratch("clean");
        touch(&dir.join("sub").join("y.ri"));
        touch(&dir.join("README.md"));

        assert!(
            loose_ri_at_root(&dir).is_empty(),
            "loose_ri_at_root must ignore nested `.ri` files and non-`.ri` files"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SEEDED FIRE for the non-vacuity scan: it must recurse, and must pick up
    /// only `.ri` files.
    #[test]
    fn collect_ri_recurses_and_filters_by_extension() {
        let dir = scratch("collect");
        touch(&dir.join("a").join("b").join("deep.ri"));
        touch(&dir.join("a").join("notes.md"));

        let mut found: Vec<PathBuf> = Vec::new();
        collect_ri(&dir, &mut found);

        assert_eq!(
            found,
            vec![dir.join("a").join("b").join("deep.ri")],
            "collect_ri must recurse and match only `.ri`; got {found:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
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

// ── Unit cases for the MIRRORED predicate ──────────────────────────────────
//
// Ported from `predicate_tests` in the guard, restricted to the cases that
// DISCRIMINATE: a mirror that is looser than the guard fails one of these
// rather than silently keeping the sentinel green over a probe the real guard
// would no longer flag.
//
// The keyword is spliced in at runtime by `with_scalar` because this file is
// itself scanned by the guard being mirrored (see the module doc) — writing
// `: Scalar` literally on these non-comment lines would make this file a real
// corpus violation.

#[cfg(test)]
mod mirror_predicate_tests {
    use super::line_is_bare_scalar;

    const SCALAR_KW: &str = "Scalar";

    /// Join `parts` with the bare keyword, e.g.
    /// `with_scalar(&["param w: ", " = 10mm"])`.
    fn with_scalar(parts: &[&str]) -> String {
        parts.join(SCALAR_KW)
    }

    // ── must MATCH (the mirror may not be looser than the guard) ────────────

    #[test]
    fn detects_bare_annotation_with_space() {
        assert!(line_is_bare_scalar(&with_scalar(&[
            "    param width: ",
            " = 10mm"
        ])));
    }

    #[test]
    fn detects_bare_annotation_without_space() {
        assert!(line_is_bare_scalar(&with_scalar(&[
            "    param width:",
            " = 10mm"
        ])));
    }

    #[test]
    fn detects_bare_annotation_at_end_of_line() {
        assert!(line_is_bare_scalar(&with_scalar(&["    fn foo(x: ", ""])));
    }

    #[test]
    fn detects_bare_return_codomain() {
        assert!(line_is_bare_scalar(&with_scalar(&[
            "    fn area(w: Length) -> ",
            ""
        ])));
    }

    #[test]
    fn detects_bare_return_codomain_with_brace() {
        // The Debug carve-out is annotation-only; the codomain arm is untouched.
        assert!(line_is_bare_scalar(&with_scalar(&[
            "    field def temp : Point3 -> ",
            " {"
        ])));
    }

    #[test]
    fn detects_dsl_structure_supertype_with_brace() {
        // `structure def X` is not a single identifier, so the Debug carve-out
        // must not swallow this DSL shape.
        assert!(line_is_bare_scalar(&with_scalar(&[
            "structure def OddRule : ",
            " {"
        ])));
    }

    #[test]
    fn detects_bare_annotation_after_a_url_double_slash() {
        // `://` in a string literal must not be mistaken for a comment start.
        assert!(line_is_bare_scalar(&with_scalar(&[
            r#"    let _u = "https://x.com"; let src = "param x: "#,
            r#"";"#
        ])));
    }

    // ── must NOT match (the mirror may not be stricter, either) ─────────────

    #[test]
    fn excludes_pure_comment_line() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    // param x: ",
            " = 10mm"
        ])));
    }

    #[test]
    fn excludes_rust_enum_path() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    let t = Type::",
            " { dimension: LENGTH };"
        ])));
    }

    #[test]
    fn excludes_parameterized_form() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    param x: ",
            "<Length> = 10mm"
        ])));
    }

    #[test]
    fn excludes_keyword_followed_by_a_letter() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    param xs: ",
            "s = 1"
        ])));
    }

    #[test]
    fn excludes_rust_debug_struct_field_opener() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "        width: ",
            " {"
        ])));
    }

    #[test]
    fn excludes_rust_debug_struct_field_opener_underscore_ident() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "        outer_r: ",
            " {"
        ])));
    }

    #[test]
    fn excludes_trailing_comment_mentioning_the_bare_form() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    param width: Length = 10mm // was: ",
            ""
        ])));
    }

    #[test]
    fn excludes_trailing_comment_mentioning_the_bare_codomain() {
        assert!(!line_is_bare_scalar(&with_scalar(&[
            "    field def t : Point3 -> Length {} // was -> ",
            ""
        ])));
    }
}
