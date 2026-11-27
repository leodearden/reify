//! Corpus-cleanliness guard: zero bare `: Scalar` type annotations and bare
//! `-> Scalar` return codomains (tasks δ + δ-completion).
//!
//! Signal: `: *Scalar([^<a-zA-Z]|$)` (annotation) or `-> Scalar([^<a-zA-Z]|$)`
//! (codomain), with pure-comment lines and `::Scalar` excluded.
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
//!   * `crates/reify-spec-conformance/fixtures/` — see the exclusion arm below.
//!
//! This test is GREEN (δ migration complete). It becomes compiler-redundant
//! once γ adds `E_BARE_SCALAR`, but protects the δ→γ window as a regression
//! guard.
//!
//! The predicate (what a bare `Scalar` is, and what it deliberately is not)
//! is `reify_test_support::bare_scalar::line_has_bare_scalar`.

use std::path::{Path, PathBuf};

use reify_test_support::bare_scalar::line_has_bare_scalar;

/// The Ring-1 language-spec conformance fixture tree, repo-relative — excluded
/// from the scan, and the home of the probe that keeps that exclusion honest.
const SPEC_CONFORMANCE_FIXTURES: &str = "crates/reify-spec-conformance/fixtures";

/// The running checkout's workspace root, canonicalized.
fn workspace_root() -> PathBuf {
    reify_test_support::workspace_root::workspace_root()
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

    // Exclude the predicate's own module for the same reason: its unit tests
    // are bare-`Scalar` literals by construction.
    let predicate_module = root.join("crates/reify-test-support/src/bare_scalar.rs");
    files.retain(|p| p != &predicate_module);

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

    // The Ring-1 conformance fixture tree holds CHARTERED must-reject fixtures
    // (PRD `docs/prds/v0_6/spec-conformance-suite.md` D2) — bare-`Scalar`
    // rejection is ITSELF a spec clause the suite must be free to test with a
    // violating fixture. Directory-level for the same reason as the arm above.
    // Charter: that tree's `README.md`. The test below keeps this arm
    // non-vacuous.
    let spec_conformance_fixtures = root.join(SPEC_CONFORMANCE_FIXTURES);
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

/// The spec-conformance exclusion arm above is non-vacuous only while a live
/// violator sits under it: the committed placement probe. If this guard is
/// retired as compiler-redundant (see the header), retire the probe with it.
#[test]
fn spec_conformance_placement_probe_is_a_live_violator() {
    let probe = workspace_root()
        .join(SPEC_CONFORMANCE_FIXTURES)
        .join("_placement-probe/placement_probe.ri");

    let content = std::fs::read_to_string(&probe)
        .unwrap_or_else(|e| panic!("placement probe {} unreadable: {e}", probe.display()));

    assert!(
        content.lines().any(line_has_bare_scalar),
        "The placement probe {} no longer trips this guard's predicate, so the \
         spec-conformance exclusion arm is vacuous. Do NOT \"fix\" the probe: \
         restore its bare annotation, or retire the arm and the probe together. \
         See {SPEC_CONFORMANCE_FIXTURES}/README.md.",
        probe.display()
    );
}
