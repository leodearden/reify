//! Corpus-cleanliness guard: zero bare `: Scalar` type annotations and bare
//! `-> Scalar` return codomains (tasks δ + δ-completion).
//!
//! Walks:
//!   * `examples/**/*.ri`          — design example files
//!   * `crates/**/*.ri`            — standalone fixture .ri files
//!   * `crates/**/*.rs`            — inline .ri fixtures in Rust sources
//!     (comment/doc-prose lines are excluded — see predicate)
//!   * `gui/src-tauri/**/*.rs`     — GUI inline DSL test sources
//!   * `gui/test/**/*.ri`          — GUI fixture files
//!
//! Excluded from scan (parse-only, pin literal "Scalar", never type-resolve):
//!   * `crates/reify-syntax/tests/`
//!   * `crates/reify-ast/tests/`
//!
//! Excluded from scan (conformance corpus, must-reject fixtures are chartered):
//!   * `crates/reify-spec-conformance/fixtures/` — see the exclusion arm below
//!     and, for the charter and the sentinel arrangement that keeps that arm
//!     non-vacuous, `crates/reify-spec-conformance/fixtures/README.md`.
//!
//! The predicate itself and every carve-out's rationale (`::Scalar` enum paths,
//! `Scalar<…>`, `Scalars`, pure-comment lines, the `{:#?}` Debug struct-field
//! opener) live on the items in `bare_scalar_predicate.rs`, which this file and
//! `crates/reify-spec-conformance/tests/fixture_tree.rs` both include so there
//! is exactly one copy. `predicate_tests` at the bottom of THIS file are that
//! predicate's unit tests — they live here because this file is self-excluded
//! from the scan and so may spell violating examples out literally.
//!
//! This test is GREEN (δ migration complete). It becomes compiler-redundant
//! once γ adds `E_BARE_SCALAR`, but protects the δ→γ window as a regression
//! guard.

use std::path::{Path, PathBuf};

/// The detection predicate, single-sourced. Shared by `#[path]` inclusion and
/// never by a Cargo dependency edge — see that file's header for why.
#[path = "bare_scalar_predicate.rs"]
mod bare_scalar_predicate;

use bare_scalar_predicate::line_has_bare_scalar;

/// Resolve the workspace root from CARGO_MANIFEST_DIR.
///
/// `reify-cli` lives at `<root>/crates/reify-cli`, so the workspace root is
/// two levels up.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root must be accessible")
}

/// Walk `dir` recursively, appending every file whose extension equals `ext`
/// to `out`.  Silently skips unreadable entries.
fn collect_files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

#[test]
fn corpus_has_zero_bare_scalar() {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();

    // A. examples/**/*.ri — design example files
    collect_files(&root.join("examples"), "ri", &mut files);

    // B + C + D. crates/**/*.ri (fixture .ri files) + crates/**/*.rs (inline fixtures, doc-prose)
    collect_files(&root.join("crates"), "ri", &mut files);
    collect_files(&root.join("crates"), "rs", &mut files);

    // E. gui/src-tauri/**/*.rs — all GUI src-tauri Rust sources (production + inline-DSL
    //    tests). The recursive walk intentionally covers production files (engine.rs,
    //    types.rs, …) as well as the inline-DSL test fixtures; the wider coverage is
    //    beneficial — any bare `Scalar` reaching type resolution from GUI code is caught
    //    here too. (δ-completion: δ's guard never scanned this tree.)
    collect_files(&root.join("gui").join("src-tauri"), "rs", &mut files);

    // F. GUI fixture .ri files
    let gui_fixtures = root.join("gui").join("test");
    collect_files(&gui_fixtures, "ri", &mut files);

    // Deduplicate: the crates/ walk can't overlap with examples/ or gui/, but
    // sort + dedup keeps the list tidy.
    files.sort();
    files.dedup();

    // Exclude this guard-test file itself — it contains `: Scalar` and `-> Scalar`
    // in its own comments, strings, and unit-test literals.  Scanning it would create
    // self-referential false positives that prevent the test from ever going GREEN.
    files.retain(|p| p.file_name().and_then(|f| f.to_str()) != Some("corpus_no_bare_scalar.rs"));

    // Exclude parse-only test directories — they pin the LITERAL PARSED name
    // "Scalar" (never reach type resolution, can never be E_BARE_SCALAR violators).
    //   * crates/reify-syntax/tests/ — field_tests.rs:30,64 assert codomain_type.to_string()=="Scalar"
    //   * crates/reify-ast/tests/   — api_surface.rs:70 asserts name=="Scalar"
    //
    // Accepted blind spot: the exclusion is directory-level, not file-level.
    // A new test file added to either directory would also be excluded from the
    // scan.  This is intentional: every test in reify-syntax/tests/ and
    // reify-ast/tests/ is parse-only by design — none reach type resolution,
    // so none can ever be E_BARE_SCALAR (γ) violators.  The carve-out matches
    // the invariant the directories enforce, not just the two current files.
    let syntax_tests = root.join("crates").join("reify-syntax").join("tests");
    let ast_tests = root.join("crates").join("reify-ast").join("tests");
    files.retain(|p| !p.starts_with(&syntax_tests) && !p.starts_with(&ast_tests));

    // MARKER: spec-conformance-fixtures-exclusion-arm
    //
    // That token is a deliberate machine-read contract, not decoration:
    // `corpus_guard_still_registers_this_tree` in
    // `crates/reify-spec-conformance/tests/fixture_tree.rs` greps THIS FILE for
    // it, so the spec-conformance crate can tell "the arm is still here" from
    // "the guard was retired out from under my sentinel". Keep the token
    // adjacent to the `retain` below; if the arm is ever retired, delete the
    // token in the SAME change so that test reds loudly instead of passing over
    // a tree nobody excludes any more. Pinning a token rather than the local
    // binding or the predicate's name is what makes both sides rename-proof.
    //
    // The arm itself: the Ring-1 language-spec conformance fixture tree holds
    // CHARTERED must-reject fixtures (PRD `docs/prds/v0_6/spec-conformance-suite.md`
    // D2, leaf beta #6759) — bare-`Scalar` rejection is ITSELF a spec clause the
    // conformance suite must be free to test with a violating fixture. The
    // charter, and the sentinel arrangement that keeps this arm from going
    // vacuous, are in `crates/reify-spec-conformance/fixtures/README.md`; the
    // sentinel's guard-side half is the test below.
    //
    // Accepted blind spot: the exclusion is directory-level, not file-level.
    // Any future fixture added under that tree is also excluded from the scan.
    // This is intentional and matches the arm above: the invariant the directory
    // enforces — a conformance fixture may violate any spec clause on purpose —
    // holds for every future resident, not just today's files.
    let spec_conformance_fixtures = root
        .join("crates")
        .join("reify-spec-conformance")
        .join("fixtures");
    files.retain(|p| !p.starts_with(&spec_conformance_fixtures));

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = path.strip_prefix(&root).unwrap_or(path);
        for (line_idx, line) in content.lines().enumerate() {
            if line_has_bare_scalar(line) {
                violations.push(format!(
                    "{}:{}: {}",
                    rel.display(),
                    line_idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} bare `Scalar` annotation(s) or codomain(s). \
         Migrate each `: Scalar` -> `: Length` and `-> Scalar` -> `-> Length`:\n\n{}",
        violations.len(),
        violations.join("\n")
    );
}

// ── Sentinel for the spec-conformance exclusion arm ──────────────────

/// Guard-side half of the placement-probe sentinel: the exclusion arm above is
/// non-vacuous only while a live violator sits under it, and this makes that a
/// checked property rather than a comment. Full rationale (and the other half,
/// in `crates/reify-spec-conformance/tests/fixture_tree.rs`) lives in
/// `crates/reify-spec-conformance/fixtures/README.md`.
///
/// If this guard is ever retired as compiler-redundant (see the header: once
/// γ adds `E_BARE_SCALAR`), retire the probe and both sentinels in the same
/// change — do not leave a sentinel standing watch over nothing.
#[test]
fn spec_conformance_placement_probe_is_a_live_violator() {
    let probe = workspace_root()
        .join("crates/reify-spec-conformance/fixtures/_placement-probe/placement_probe.ri");

    let content = std::fs::read_to_string(&probe).unwrap_or_else(|e| {
        panic!(
            "The spec-conformance placement probe {} is missing or unreadable ({e}). \
             It is the live violator that keeps this test's `reify-spec-conformance/fixtures` \
             exclusion arm from being vacuous. Restore it, or retire the arm in the same change. \
             See crates/reify-spec-conformance/fixtures/README.md.",
            probe.display()
        )
    });

    assert!(
        content.lines().any(line_has_bare_scalar),
        "The spec-conformance placement probe {} no longer trips this guard's predicate, \
         so the `reify-spec-conformance/fixtures` exclusion arm above now excludes nothing \
         detectable: it would stay green even if deleted.\n\n\
         Do NOT \"fix\" the probe — its bare annotation is deliberate. Restore it, or retire \
         the exclusion arm (and the probe) in the same change. \
         See crates/reify-spec-conformance/fixtures/README.md.",
        probe.display()
    );
}

// ── Unit tests for the detection predicate ─────────────────────────────────
//
// These are the ONLY unit tests for `bare_scalar_predicate.rs`; the
// spec-conformance crate includes the same source and so needs no copy of them.
// They live in THIS file because it is the one self-excluded from the scan
// above, so its literals may spell `: Scalar` / `-> Scalar` out in full.

#[cfg(test)]
mod predicate_tests {
    use super::line_has_bare_scalar;

    // Should match (violations) — annotation cases
    #[test]
    fn detects_bare_scalar_with_space() {
        assert!(line_has_bare_scalar("    param width: Scalar = 10mm"));
    }

    #[test]
    fn detects_bare_scalar_no_space() {
        assert!(line_has_bare_scalar("    param width:Scalar = 10mm"));
    }

    #[test]
    fn detects_bare_scalar_at_end_of_line() {
        assert!(line_has_bare_scalar("    fn foo(x: Scalar"));
    }

    #[test]
    fn detects_bare_scalar_followed_by_comma() {
        assert!(line_has_bare_scalar("    fn foo(x: Scalar, y: Scalar)"));
    }

    #[test]
    fn detects_bare_scalar_followed_by_paren() {
        assert!(line_has_bare_scalar(
            "    fn area(w: Scalar, h: Scalar) -> Scalar"
        ));
    }

    #[test]
    fn detects_bare_scalar_in_inline_ri_string() {
        assert!(line_has_bare_scalar(
            r#"    let src = "param w: Scalar = 50mm";"#
        ));
    }

    // Should match (violations) — codomain cases
    #[test]
    fn detects_return_scalar() {
        // `-> Scalar` IS a bare return codomain (δ-completion migrates it)
        assert!(line_has_bare_scalar("    fn area(w: Length) -> Scalar"));
    }

    #[test]
    fn detects_return_scalar_with_brace() {
        assert!(line_has_bare_scalar(
            "    field def temp : Point3 -> Scalar { 1.0m }"
        ));
    }

    #[test]
    fn detects_return_scalar_at_end_of_line() {
        assert!(line_has_bare_scalar("    fn foo() -> Scalar"));
    }

    // Should NOT match (correctly excluded)
    #[test]
    fn excludes_double_colon_scalar() {
        assert!(!line_has_bare_scalar(
            "    let t = Type::Scalar { dimension: LENGTH };"
        ));
    }

    #[test]
    fn excludes_value_double_colon_scalar() {
        assert!(!line_has_bare_scalar("    Value::Scalar(v)"));
    }

    #[test]
    fn excludes_scalar_with_angle_bracket() {
        assert!(!line_has_bare_scalar("    param x: Scalar<Length> = 10mm"));
    }

    #[test]
    fn excludes_return_scalar_parameterized() {
        // `-> Scalar<Q>` is NOT bare — parameterized, not a migration target
        assert!(!line_has_bare_scalar("    fn foo() -> Scalar<Length>"));
    }

    #[test]
    fn excludes_scalar_followed_by_letter() {
        assert!(!line_has_bare_scalar("    // Scalars and tensors"));
    }

    #[test]
    fn excludes_comment_only_double_colon() {
        assert!(!line_has_bare_scalar("    // see Type::Scalar for details"));
    }

    #[test]
    fn excludes_comment_line_with_return_scalar() {
        // Pure comment lines are skipped entirely
        assert!(!line_has_bare_scalar(
            "    // field def area(w: Length) -> Scalar"
        ));
    }

    #[test]
    fn excludes_comment_line_with_annotation_scalar() {
        // Pure comment lines are skipped even for annotation form
        assert!(!line_has_bare_scalar("    // param x: Scalar = 10mm"));
    }

    // Trailing-comment stripping — non-comment lines whose trailing `// ...`
    // mentions a bare Scalar must NOT be flagged (the code itself is migrated).
    #[test]
    fn excludes_trailing_comment_with_return_scalar() {
        assert!(!line_has_bare_scalar(
            "    field def t : Point3 -> Length {} // was -> Scalar"
        ));
    }

    #[test]
    fn excludes_trailing_comment_with_annotation_scalar() {
        assert!(!line_has_bare_scalar(
            "    param width: Length = 10mm // was: Scalar"
        ));
    }

    // Rust `{:#?}` Debug goldens — `#[derive(Debug)]` renders the
    // `Value::Scalar` enum variant unqualified, so a snapshot of a
    // LENGTH-dimensioned IR field reads as `<field>: Scalar {`.
    #[test]
    fn excludes_rust_debug_scalar_struct_field() {
        assert!(!line_has_bare_scalar("        width: Scalar {"));
    }

    #[test]
    fn excludes_rust_debug_scalar_struct_field_underscore_ident() {
        assert!(!line_has_bare_scalar("        outer_r: Scalar {"));
    }

    // …and the carve-out must stay narrow: everything below is still a
    // violation.
    #[test]
    fn detects_bare_scalar_annotation_despite_trailing_brace_rule() {
        // A DSL param annotation has more than an identifier before the `:`
        // and does not end in ` {` — unaffected by the Debug carve-out.
        assert!(line_has_bare_scalar("    param width: Scalar = 10mm"));
    }

    #[test]
    fn detects_dsl_structure_supertype_scalar_with_brace() {
        // `structure def X : Scalar {` is the one DSL shape that is `: <Type> {`;
        // `structure def X` is not a single identifier, so it still matches.
        assert!(line_has_bare_scalar("structure def OddRule : Scalar {"));
    }

    #[test]
    fn detects_return_scalar_with_brace_is_not_debug_excluded() {
        // The codomain arm is untouched by the Debug carve-out.
        assert!(line_has_bare_scalar(
            "    field def temp : Point3 -> Scalar {"
        ));
    }

    #[test]
    fn preserves_scalar_before_url_double_slash() {
        // `://` in a string literal must not be mistaken for a comment start;
        // bare Scalar appearing later on the same line must still be detected.
        assert!(line_has_bare_scalar(
            r#"    let _u = "https://x.com"; let src = "param x: Scalar";"#
        ));
    }
}
