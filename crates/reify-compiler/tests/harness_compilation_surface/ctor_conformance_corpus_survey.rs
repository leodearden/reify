//! Struct-ctor field-type conformance — corpus survey generator (task #5304).
//!
//! PRD `docs/prds/struct-ctor-field-type-conformance.md`, task β (§8): run the
//! α(+ε) warn-stage compiler over **all tracked `.ri`** and commit
//! `docs/prds/struct-ctor-field-type-conformance.survey.md` — every warning
//! site, classified per D9, with the regeneration command. The signal is
//! "mechanized, not a hand audit": every row and every count in that artifact
//! is produced by the code in this module, with zero hand-derived entries.
//!
//! # Why this lives HERE and not in a new `tests/*.rs` binary
//!
//! `tests/infra/test_harness_kloc_cap.sh` rule (b) flags any NEW standalone
//! top-level `crates/reify-compiler/tests/*.rs` as `reason=unsanctioned-standalone`
//! unless a grandfather-baseline row is added — an explicitly-discouraged
//! "conscious baseline edit" whose whole point is to stop new test binaries
//! silently re-accreting against the merge-gate link count
//! (`docs/prds/merge-gate-compile-cost.md` §5 C1). Folding the generator into
//! this already-consolidated unit adds no link at all, and the unit is
//! thematically exact — "what you hand the compiler … examples". The sibling
//! `examples_smoke.rs` already runs the identical parse→compile→filter pipeline
//! over `examples/`; β widens the root to the whole tracked corpus.
//!
//! # Why the expensive walk is `#[ignore]`d and the decisions are not
//!
//! Compiling the ~261 `examples/` files is documented as "the single most
//! expensive thing this binary does" (`examples_smoke.rs`); 660 tracked files
//! is ~2.5× that, and paying it on every merge gate would directly fight the
//! merge-gate-compile-cost PRD. So the full corpus walk is ONE `#[ignore]`d
//! generator, run on demand — while everything it *decides* (corpus
//! enumeration, span→line, ctor-name recovery, field/expected/found
//! extraction, D9 classification, markdown rendering) is factored into pure
//! helpers that ARE gate-resident and unit-tested here against synthetic
//! inputs, plus one cheap end-to-end sweep over a 3-file synthetic corpus.
//! The pipeline is therefore regression-guarded on every gate run at near-zero
//! cost, without the walk itself ever running there.

use std::path::PathBuf;

/// Absolute path to the workspace root, resolved at compile time from this
/// crate's manifest directory (two levels up).
///
/// Same rooting idiom as `examples_smoke.rs`'s `EXAMPLES_DIR`, pointed one
/// level higher: β's whole point is that the sweep is NOT examples-scoped.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

// ─── step 1/2: corpus enumeration ────────────────────────────────────────────

/// Every TRACKED `.ri` file in the repository, as repo-relative
/// forward-slash paths, sorted and deduplicated.
///
/// Shells out to `git ls-files -z -- '*.ri'` at the workspace root rather than
/// walking the filesystem, for three reasons:
///
/// 1. The task defines the corpus as "all **tracked** `.ri`", and both the PRD
///    and the capability manifest cite `git ls-files '*.ri'` as the enumerating
///    command — so the survey's denominator is identical to the one the PRD
///    gate reasons about.
/// 2. A filesystem walk would have to exclude `target/` and every other
///    gitignored tree by hand, and would drift from that definition; a
///    build-artifact `.ri` could silently enter the survey.
/// 3. `-z` / NUL splitting means a path containing a space or a newline cannot
///    corrupt the list.
///
/// Panics if git is unavailable or exits non-zero. A silently-empty corpus
/// would render a falsely-clean survey, which is the one failure mode this
/// artifact must never have.
fn tracked_ri_corpus() -> Vec<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(WORKSPACE_ROOT)
        .args(["ls-files", "-z", "--", "*.ri"])
        .output()
        .unwrap_or_else(|e| {
            panic!("ctor_conformance_corpus_survey: cannot run `git ls-files` in {WORKSPACE_ROOT}: {e}")
        });
    assert!(
        out.status.success(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '*.ri'` in {WORKSPACE_ROOT} \
         exited {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );

    let stdout = String::from_utf8(out.stdout)
        .expect("ctor_conformance_corpus_survey: `git ls-files` emitted non-UTF-8 paths");
    let mut paths: Vec<String> = stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    assert!(
        !paths.is_empty(),
        "ctor_conformance_corpus_survey: `git ls-files -z -- '*.ri'` returned nothing in \
         {WORKSPACE_ROOT} — a silently-empty corpus would render a falsely-clean survey"
    );
    paths
}

#[test]
fn tracked_ri_corpus_is_non_empty_and_covers_the_whole_tracked_tree() {
    let corpus = tracked_ri_corpus();
    // A FLOOR, never an exact count: 660 measured at plan time and the corpus
    // legitimately grows. An exact assertion would go red on every new `.ri`.
    assert!(
        corpus.len() >= 600,
        "tracked .ri corpus must have >= 600 entries (660 measured 2026-08-27), got {}",
        corpus.len()
    );
}

#[test]
fn tracked_ri_corpus_entries_all_end_in_dot_ri() {
    let corpus = tracked_ri_corpus();
    let bad: Vec<&String> = corpus.iter().filter(|p| !p.ends_with(".ri")).collect();
    assert!(
        bad.is_empty(),
        "every corpus entry must end in '.ri', got {} that do not: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn tracked_ri_corpus_is_sorted_and_deduplicated() {
    // Determinism: the artifact must be byte-reproducible, which requires the
    // enumeration itself to be a total order with no repeats.
    let corpus = tracked_ri_corpus();
    let mut expected = corpus.clone();
    expected.sort();
    expected.dedup();
    assert_eq!(
        corpus, expected,
        "tracked_ri_corpus must return a sorted, deduplicated list"
    );
}

#[test]
fn tracked_ri_corpus_entries_all_resolve_to_existing_files() {
    let root = PathBuf::from(WORKSPACE_ROOT);
    let corpus = tracked_ri_corpus();
    let missing: Vec<&String> = corpus
        .iter()
        .filter(|rel| !root.join(rel).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "every corpus entry must resolve to an existing file under the workspace \
         root, got {} that do not: {:?}",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
}

#[test]
fn tracked_ri_corpus_reaches_outside_examples() {
    // The landed `discover_ri_files()` walk is rooted at `examples/` and would
    // miss ~399 of the 660 tracked files. Widening the root IS β.
    let corpus = tracked_ri_corpus();
    assert!(
        corpus
            .iter()
            .any(|p| p.starts_with("crates/reify-compiler/stdlib/")),
        "corpus must include stdlib members (β is not examples-scoped); \
         first 5 entries: {:?}",
        &corpus[..corpus.len().min(5)]
    );
    assert!(
        corpus.iter().any(|p| p.starts_with("examples/")),
        "corpus must still include the examples/ tree"
    );
    let non_examples = corpus.iter().filter(|p| !p.starts_with("examples/")).count();
    assert!(
        non_examples >= 300,
        "the non-examples half is the point of β (399 measured at plan time), got {non_examples}"
    );
}

#[test]
fn tracked_ri_corpus_paths_are_repo_relative_forward_slash() {
    let corpus = tracked_ri_corpus();
    for p in &corpus {
        assert!(
            !p.starts_with('/') && !p.starts_with("./") && !p.contains('\\'),
            "corpus entries must be repo-relative forward-slash paths, got {p:?}"
        );
    }
}

// ─── step 3/4: source-position helpers ───────────────────────────────────────

#[test]
fn line_of_span_is_one_based_and_multibyte_correct() {
    use reify_core::SourceSpan;

    let source = "alpha\nbeta\ngamma\n";
    assert_eq!(
        line_of_span(source, SourceSpan::empty(0)),
        1,
        "offset 0 must be line 1 (1-based, not 0-based)"
    );
    let third = source.find("gamma").expect("fixture has 'gamma'") as u32;
    assert_eq!(
        line_of_span(source, SourceSpan::new(third, third + 5)),
        3,
        "an offset on the third line must be line 3"
    );

    // A multi-byte prefix must not shift the line: `byte_offset_to_line_col`
    // counts codepoints for COLUMNS but newlines for LINES, so a non-ASCII
    // prefix on line 1 leaves an offset on line 2 reporting 2.
    let wide = "π·m·s^-1\nsecond line\n";
    let second = wide.find("second").expect("fixture has 'second'") as u32;
    assert_eq!(
        line_of_span(wide, SourceSpan::new(second, second + 6)),
        2,
        "a multi-byte prefix must not shift the reported line"
    );
}

#[test]
fn line_of_span_clamps_past_eof_instead_of_panicking() {
    use reify_core::SourceSpan;

    // A synthetic / fallback span must never abort a 660-file sweep. Both the
    // plain past-EOF case and the PRELUDE sentinel are exercised: the sentinel
    // is `SourceSpan::empty(u32::MAX)`, which `byte_offset_to_line_col` maps to
    // (1, 1) but which a naive `offset <= len` debug_assert would trip on.
    let source = "one\ntwo\n";
    assert_eq!(
        line_of_span(source, SourceSpan::empty(9_999)),
        2,
        "an offset past EOF must clamp to the last line, not panic"
    );
    assert_eq!(
        line_of_span("", SourceSpan::empty(0)),
        1,
        "an empty source must still report line 1"
    );
    let prelude = line_of_span(source, SourceSpan::prelude());
    assert_eq!(
        prelude, 1,
        "the prelude sentinel must degrade to line 1, not panic or report a wild line"
    );
}

#[test]
fn ctor_type_name_at_recovers_the_def_from_the_call_site_anchor() {
    use reify_core::SourceSpan;

    // α anchors the expression-path label at the ctor call-site's OWN span
    // (compile_builder/entities_phase.rs: `ctor_span.unwrap_or(representative_span)`),
    // so `source[span.start..]` begins with `Widget(` and the leading
    // identifier IS the def name.
    let source = "structure def Root {\n    let x = Widget(label: 42)\n}\n";
    let at_ctor = source.find("Widget(").expect("fixture has 'Widget('") as u32;
    assert_eq!(
        ctor_type_name_at(source, SourceSpan::new(at_ctor, at_ctor + 6)),
        Some("Widget".to_owned()),
        "a span starting at the ctor identifier must recover the def name"
    );
}

#[test]
fn ctor_type_name_at_returns_none_rather_than_guessing() {
    use reify_core::SourceSpan;

    let source = "structure def Root {\n    let x = Widget(label: 42)\n}\n";

    // The sub `=` path anchors PER-ARG (entity.rs `PendingBoundCheck`), so the
    // span starts mid-argument. Recovery must yield None — recorded as `—` in
    // the artifact — never a guessed def name.
    let at_arg = source.find("42").expect("fixture has '42'") as u32;
    assert_eq!(
        ctor_type_name_at(source, SourceSpan::new(at_arg, at_arg + 2)),
        None,
        "a span starting mid-argument must not be mistaken for a ctor anchor"
    );

    // An identifier not followed by `(` is a plain reference, not a ctor.
    let plain = "let y = someBinding + 1\n";
    let at_ident = plain.find("someBinding").expect("fixture has ident") as u32;
    assert_eq!(
        ctor_type_name_at(plain, SourceSpan::new(at_ident, at_ident + 11)),
        None,
        "an identifier not followed by '(' is not a ctor call"
    );

    // Whitespace between the identifier and `(` is still a call.
    let spaced = "let z = Gadget (a: 1)\n";
    let at_g = spaced.find("Gadget").expect("fixture has 'Gadget'") as u32;
    assert_eq!(
        ctor_type_name_at(spaced, SourceSpan::new(at_g, at_g + 6)),
        Some("Gadget".to_owned()),
        "whitespace before '(' must not defeat recovery"
    );

    // Out-of-range and empty-source spans must degrade to None, not panic.
    assert_eq!(ctor_type_name_at(source, SourceSpan::empty(9_999)), None);
    assert_eq!(ctor_type_name_at("", SourceSpan::empty(0)), None);
    assert_eq!(ctor_type_name_at(source, SourceSpan::prelude()), None);
}
