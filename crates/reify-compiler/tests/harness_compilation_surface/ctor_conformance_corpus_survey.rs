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

/// 1-based line of `span`'s START offset within `source`.
///
/// Delegates to `reify_core::byte_offset_to_line_col` rather than hand-rolling
/// newline counting — that helper is already multi-byte-correct and carries its
/// own round-trip tests, and it short-circuits the prelude sentinel to `(1, 1)`
/// in both debug and release builds.
///
/// The one thing added here is the OUT-OF-RANGE CLAMP: `byte_offset_to_line_col`
/// carries a `debug_assert!(offset <= source.len())`, so a synthetic or stale
/// span would abort a debug-profile sweep of 660 files.
///
/// The clamp bounds the resulting LINE, not merely the offset, and that
/// distinction is load-bearing for the artifact. Clamping the offset alone to
/// `source.len()` reports line 3 for a two-line file that ends in a newline —
/// `byte_offset_to_line_col` counts the phantom empty line after the trailing
/// `\n`. Every row in the survey is a `file:line` a human will open, so a line
/// number past the end of the file is a dangling pointer. The postcondition is
/// therefore `1 <= result <= source.lines().count().max(1)`: every emitted line
/// resolves to a real line of the swept file.
///
/// The prelude sentinel is deliberately NOT offset-clamped: it is passed
/// through so the callee's own `(1, 1)` short-circuit applies.
fn line_of_span(source: &str, span: reify_core::SourceSpan) -> u32 {
    let raw = span.start as usize;
    let offset = if raw == reify_core::SourceSpan::PRELUDE_SENTINEL_OFFSET {
        raw
    } else {
        raw.min(source.len())
    };
    let line = reify_core::byte_offset_to_line_col(source, offset).0 as u32;
    // `lines()` does not yield a trailing empty line for a source ending in
    // `\n`, which is exactly the bound wanted here. `.max(1)` keeps the empty
    // source reporting line 1 rather than 0.
    let last_line = source.lines().count().max(1) as u32;
    line.clamp(1, last_line)
}

/// The structure-def name at `span`'s start, when `span` anchors a ctor call.
///
/// Takes the leading Rust-identifier-shaped run at `span.start` and returns it
/// ONLY when the next non-whitespace byte is `(`. That is exactly α's
/// expression-path anchor shape — `compile_builder/entities_phase.rs` sets the
/// label span to `ctor_span.unwrap_or(representative_span)`, the offending
/// `Foo(...)` call's own span — so recovery is exact there.
///
/// Returns `None`, never a guess, for every other shape: the sub `=` path's
/// per-arg anchor (`entity.rs` `PendingBoundCheck`, which starts mid-argument),
/// a plain identifier reference, an out-of-range or prelude-sentinel span, and
/// a `representative_span` fallback of `SourceSpan::empty(0)`. A `None` renders
/// as `—` in the artifact; the def is then named in prose by the two ε codes or
/// left unattributed, which is the honest outcome.
fn ctor_type_name_at(source: &str, span: reify_core::SourceSpan) -> Option<String> {
    let start = span.start as usize;
    if start >= source.len() {
        return None;
    }
    // A span that starts inside a multi-byte codepoint cannot be a ctor anchor
    // (identifiers are ASCII-led), and slicing at it would panic.
    if !source.is_char_boundary(start) {
        return None;
    }
    let rest = &source[start..];
    let mut chars = rest.char_indices();
    // Rust-identifier shape: first char alphabetic or `_`, then alphanumeric
    // or `_`. Reify def names are a subset of this.
    let (_, first) = chars.next()?;
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    let ident_end = chars
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let ident = &rest[..ident_end];
    // Whitespace between the identifier and `(` is legal and must not defeat
    // recovery; anything else means this is not a call.
    let after = rest[ident_end..].trim_start();
    if after.starts_with('(') {
        Some(ident.to_owned())
    } else {
        None
    }
}

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

// ─── step 5/6: diagnostic field extraction ───────────────────────────────────

/// True when `code` is one of the diagnostic codes emitted by the struct-ctor
/// field-conformance surface (tasks 5302 / 5303 / 4584 / 4598 / 4622 / 4444).
///
/// Deliberately duplicated from the identically-named helpers in
/// `examples_smoke.rs` and `struct_ctor_field_conformance_tests.rs`, following
/// the rule those files' own headers state: integration tests are separate
/// binaries and cannot share a private helper without a support-crate hop, and
/// the set is small enough that duplication is cheaper than the indirection.
/// The survey is a third consumer under that same rule.
fn is_ctor_conformance_code(code: Option<reify_core::diagnostics::DiagnosticCode>) -> bool {
    use reify_core::diagnostics::DiagnosticCode;
    matches!(
        code,
        Some(
            DiagnosticCode::ArgTypeMismatch
                | DiagnosticCode::SelectorKindMismatch
                | DiagnosticCode::TypeNotConformingToTrait
                | DiagnosticCode::TypeNotConformingToStructureRef
                | DiagnosticCode::TypeNotConformingToVector
                | DiagnosticCode::CtorUnknownField
                | DiagnosticCode::CtorArity
        )
    )
}

/// Which D9 fix-forward rule governs a site — the load-bearing, mechanizable
/// half of D9 and the artifact's primary grouping key.
///
/// This does NOT encode D9's split between class (1) call-site bug and class
/// (2) wrong declared field type. The PRD defines that as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; β must not fabricate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    /// The site's structure def is declared in an FEA stdlib module. Per D9,
    /// γ may make CALL-SITE changes only — field-type flips stay v0.6-owned.
    FeaDeferredToV06,
    /// The def is resolved and is not FEA-owned: D9's per-case judgment applies.
    NonFea,
    /// The def could not be attributed (the sub `=` per-arg anchor carries no
    /// ctor name, and the diagnostic prose names none). Deliberately its own
    /// bucket: silently defaulting an unattributable site into the touchable
    /// pile would be the one classification error with a real cost.
    Unknown,
}

impl Owner {
    /// Stable section title for the rendered artifact.
    fn title(self) -> &'static str {
        match self {
            Owner::FeaDeferredToV06 => "FEA — deferred to v0.6 (DO NOT FIX HERE)",
            Owner::NonFea => "non-FEA — γ per-case judgment",
            Owner::Unknown => "unattributed def — needs manual triage",
        }
    }
}

/// One ctor-conformance warning site, as one row of the survey artifact.
///
/// Every field is machine-derived; nothing here is ever typed in by hand. An
/// extractor that cannot recover its column yields `None`, which renders as an
/// em-dash — never an empty cell and never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SurveySite {
    /// Repo-relative forward-slash path of the swept file.
    file: String,
    /// 1-based line of the diagnostic's first label span (or 1 when unlabelled).
    line: u32,
    /// Structure def being constructed, when recoverable.
    def: Option<String>,
    /// Offending field / param name, when the wording carries one.
    field: Option<String>,
    /// Declared param type, from the `expected '<X>', got '<Y>'` label.
    expected: Option<String>,
    /// Supplied arg type, from the same label.
    found: Option<String>,
    /// `Debug` rendering of the `DiagnosticCode` (PascalCase).
    code: String,
    /// `Debug` rendering of the measured `Severity` — reported, not assumed.
    severity: String,
    /// The diagnostic's raw message, preserved verbatim.
    message: String,
    /// D9 owner class. Assigned by the corpus sweep via [`d9_owner`]; the
    /// builder leaves it `Unknown`, the conservative default.
    owner: Owner,
}

/// The `emit_arg_type_mismatch` prose prefix that introduces the offending param
/// name. Also a substring of `emit_geometry_trait_violation`'s
/// `geometry argument '` and of ε's `unknown named argument '`, so one search
/// covers six of the seven codes.
const ARG_PREFIX: &str = "argument '";

/// The prefix used by the two non-geometry `TypeNotConformingToTrait` emitters
/// (`type 'X' does not conform to trait 'T' required by param 'f'`), which name
/// the param nowhere else.
const REQUIRED_BY_PARAM_PREFIX: &str = "required by param '";

/// The ε `CtorUnknownField` prose that names the target structure def.
const IN_CALL_TO_PREFIX: &str = "in call to '";

/// The ε `CtorArity` message prefix; the def name follows it, up to `()`.
const CTOR_ARITY_PREFIX: &str = "E_CTOR_ARITY: ";

/// The single-quoted token immediately following `prefix` in `haystack`.
///
/// This is the guarded quoted-token idiom from `examples_smoke.rs`'s
/// `param_name_from_ctor_diagnostic`, lifted verbatim rather than re-invented,
/// and it carries that helper's warning forward: **this is a real coupling to
/// diagnostic prose.** Every extractor built on it therefore returns `Option`,
/// and [`survey_site_from_diagnostic`] preserves the RAW message on a miss, so
/// a future wording drift degrades to a still-usable row instead of a silently
/// dropped site or a fabricated field.
fn quoted_after(haystack: &str, prefix: &str) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

/// The offending field / param name, across every wording the 7 codes use.
///
/// Returns `None` for `CtorArity` (whose wording names no param) and for any
/// message that has drifted out of all three known shapes.
fn field_of_message(message: &str) -> Option<String> {
    quoted_after(message, ARG_PREFIX)
        .or_else(|| quoted_after(message, REQUIRED_BY_PARAM_PREFIX))
        .filter(|s| !s.is_empty())
}

/// `(expected, found)` from a `expected '<X>', got '<Y>'` LABEL.
///
/// The label is preferred over the prose main message because it is exactly
/// that shape (`conformance/mod.rs` `emit_arg_type_mismatch` and its three
/// siblings), whereas the message interleaves the param name twice and may
/// carry the D4-6 dimensioned-scalar migration hint after a `;`. The two ε
/// codes carry no such label and correctly yield `(None, None)`.
fn expected_found_of_labels(d: &reify_core::Diagnostic) -> (Option<String>, Option<String>) {
    for label in &d.labels {
        let (Some(expected), Some(found)) = (
            quoted_after(&label.message, "expected '"),
            quoted_after(&label.message, "got '"),
        ) else {
            continue;
        };
        return (Some(expected), Some(found));
    }
    (None, None)
}

/// The structure def being constructed, when recoverable.
///
/// Three sources, in order of reliability:
/// 1. The ε `CtorUnknownField` prose `in call to '<Def>'`.
/// 2. The ε `CtorArity` prose `<Def>() expects at most …`.
/// 3. The call-site span anchor — α anchors the expression-path label at the
///    ctor's own span, so `source[span.start..]` begins with `Def(`.
///
/// Returns `None` — never a guess — for the sub `=` per-arg anchor, which
/// starts mid-argument and names no def anywhere.
fn def_of_diagnostic(source: &str, d: &reify_core::Diagnostic) -> Option<String> {
    if let Some(def) = quoted_after(&d.message, IN_CALL_TO_PREFIX) {
        return Some(def);
    }
    if let Some(rest) = d.message.strip_prefix(CTOR_ARITY_PREFIX)
        && let Some(paren) = rest.find("()")
        && !rest[..paren].is_empty()
    {
        return Some(rest[..paren].to_owned());
    }
    d.labels
        .first()
        .and_then(|l| ctor_type_name_at(source, l.span))
}

/// Build one [`SurveySite`] from a diagnostic observed while sweeping `file`.
///
/// Returns `None` only when `d` is not one of the 7 ctor-conformance codes (an
/// uncoded legacy diagnostic included). A ctor-coded diagnostic ALWAYS yields a
/// row, even when every extractor misses — dropping it would silently
/// under-size γ, which is the artifact's whole purpose.
///
/// `owner` is left [`Owner::Unknown`]; the corpus sweep assigns it via
/// [`d9_owner`] once the FEA def set has been scanned.
fn survey_site_from_diagnostic(
    file: &str,
    source: &str,
    d: &reify_core::Diagnostic,
) -> Option<SurveySite> {
    if !is_ctor_conformance_code(d.code) {
        return None;
    }
    let (expected, found) = expected_found_of_labels(d);
    let line = d
        .labels
        .first()
        .map(|l| line_of_span(source, l.span))
        .unwrap_or(1);
    Some(SurveySite {
        file: file.to_owned(),
        line,
        def: def_of_diagnostic(source, d),
        field: field_of_message(&d.message),
        expected,
        found,
        code: format!("{:?}", d.code.expect("filtered to Some(code) above")),
        severity: format!("{:?}", d.severity),
        message: d.message.clone(),
        owner: Owner::Unknown,
    })
}

/// Build a synthetic diagnostic in the exact shape a given emitter produces, so
/// the extractor tests need no compilation at all.
#[cfg(test)]
fn synth(
    code: reify_core::diagnostics::DiagnosticCode,
    message: &str,
    label: Option<&str>,
) -> reify_core::Diagnostic {
    use reify_core::{Severity, diagnostics::DiagnosticLabel};
    let mut d = reify_core::Diagnostic::error(message).with_code(code);
    // α's knob: ctor field conformance is Warning severity, not Error.
    d.severity = Severity::Warning;
    if let Some(l) = label {
        d = d.with_label(DiagnosticLabel::new(reify_core::SourceSpan::empty(0), l));
    }
    d
}

#[test]
fn survey_site_extracts_field_from_the_argument_prose_prefix() {
    use reify_core::diagnostics::DiagnosticCode;

    // `emit_arg_type_mismatch` (conformance/mod.rs) — the dominant shape.
    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'label' has type 'Int' but param 'label' requires type 'String'",
        Some("expected 'String', got 'Int'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("ctor-coded diag yields a site");
    assert_eq!(site.field.as_deref(), Some("label"));

    // `emit_selector_mismatch` kind-vs-kind — same `argument '` prefix.
    let d = synth(
        DiagnosticCode::SelectorKindMismatch,
        "argument 'face' has selector kind 'Selector(Edge)' but param 'face' \
         requires selector kind 'Selector(Face)'",
        Some("expected 'Selector(Face)', got 'Selector(Edge)'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("face"));

    // `emit_geometry_trait_violation` — the prefix is `geometry argument '`,
    // which still CONTAINS `argument '`, so the same extractor recovers it.
    let d = synth(
        DiagnosticCode::TypeNotConformingToTrait,
        "geometry argument 'target' does not conform to trait 'Solid'",
        Some("geometry argument 'target' is not Solid"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("target"));

    // The OTHER `TypeNotConformingToTrait` shape names the param via a
    // different prefix entirely: `required by param '<name>'`.
    let d = synth(
        DiagnosticCode::TypeNotConformingToTrait,
        "type 'Bolt' does not conform to trait 'Fastener' required by param 'part'",
        Some("type 'Bolt' does not conform to trait 'Fastener'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(
        site.field.as_deref(),
        Some("part"),
        "the `required by param '` shape must also yield a field"
    );

    // `emit_structure_ref_mismatch` / `emit_vector_mismatch`.
    for (code, msg) in [
        (
            DiagnosticCode::TypeNotConformingToStructureRef,
            "argument 'part' has type 'Int' but param 'part' requires structure type 'Part'",
        ),
        (
            DiagnosticCode::TypeNotConformingToVector,
            "argument 'axis' has type 'Real' but param 'axis' requires vector type 'Vector3<Length>'",
        ),
    ] {
        let d = synth(code, msg, Some("expected 'X', got 'Y'"));
        let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
        assert!(site.field.is_some(), "{code:?} must yield a field, got None");
    }
}

#[test]
fn survey_site_extracts_field_and_def_from_the_epsilon_codes() {
    use reify_core::diagnostics::DiagnosticCode;

    // ε `CtorUnknownField` (expr.rs): names BOTH the field and the def.
    let d = synth(
        DiagnosticCode::CtorUnknownField,
        "E_CTOR_UNKNOWN_FIELD: unknown named argument 'wgt' in call to 'Bar'; \
         'Bar' has no parameter with that name",
        Some("unknown named argument"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.field.as_deref(), Some("wgt"));
    assert_eq!(
        site.def.as_deref(),
        Some("Bar"),
        "the `in call to '<Def>'` prose must supply the def"
    );

    // ε `CtorArity` names the def but NO param — its wording carries none.
    let d = synth(
        DiagnosticCode::CtorArity,
        "E_CTOR_ARITY: Bar() expects at most 2 arguments, got 3",
        Some("wrong number of arguments"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(
        site.field, None,
        "CtorArity names no param — inventing one would be a fabricated row"
    );
    assert_eq!(site.def.as_deref(), Some("Bar"));
}

#[test]
fn survey_site_prefers_the_label_for_expected_and_found() {
    use reify_core::diagnostics::DiagnosticCode;

    // The LABEL is more structured than the prose main message: it is exactly
    // `expected '<X>', got '<Y>'` (conformance/mod.rs emit_arg_type_mismatch),
    // whereas the message interleaves the param name twice and may carry the
    // D4-6 dimensioned-scalar migration hint after a `;`.
    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'velocity_limit' has type 'Real' but param 'velocity_limit' requires \
         type 'Scalar[m·s^-1]'; pass a dimensioned literal such as 1m/s",
        Some("expected 'Scalar[m·s^-1]', got 'Real'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    assert_eq!(site.expected.as_deref(), Some("Scalar[m·s^-1]"));
    assert_eq!(site.found.as_deref(), Some("Real"));
    assert_eq!(site.field.as_deref(), Some("velocity_limit"));

    // The two ε codes carry no `expected '…', got '…'` label at all.
    for (code, msg, label) in [
        (
            DiagnosticCode::CtorUnknownField,
            "E_CTOR_UNKNOWN_FIELD: unknown named argument 'w' in call to 'Bar'; \
             'Bar' has no parameter with that name",
            "unknown named argument",
        ),
        (
            DiagnosticCode::CtorArity,
            "E_CTOR_ARITY: Bar() expects at most 1 argument, got 2",
            "wrong number of arguments",
        ),
    ] {
        let d = synth(code, msg, Some(label));
        let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
        assert_eq!(site.expected, None, "{code:?} carries no expected/got label");
        assert_eq!(site.found, None, "{code:?} carries no expected/got label");
    }
}

#[test]
fn survey_site_renders_code_and_severity_in_pascal_case_debug_form() {
    use reify_core::diagnostics::DiagnosticCode;

    let d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'a' has type 'Int' but param 'a' requires type 'String'",
        Some("expected 'String', got 'Int'"),
    );
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    // `{:?}` is used because reify-core's serde feature (which supplies the
    // PascalCase wire name) is non-default and not enabled for reify-compiler.
    // Debug renders the identical string — same choice as examples_smoke.rs.
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Warning",
        "α's knob is Warning; the sweep must report what it measured, not assume Error"
    );
}

#[test]
fn survey_site_degrades_to_none_fields_and_keeps_the_raw_message() {
    use reify_core::diagnostics::DiagnosticCode;

    // Extraction is a REAL coupling to diagnostic prose (the warning carried
    // forward from examples_smoke.rs's param_name_from_ctor_diagnostic). A
    // future wording drift must degrade to a still-usable row — never a
    // silently dropped site, and never a fabricated field.
    let drifted = "the wording of this diagnostic drifted entirely";
    let d = synth(DiagnosticCode::ArgTypeMismatch, drifted, None);
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect(
        "an unrecognised message must still yield a row — dropping it would under-size γ",
    );
    assert_eq!(site.field, None);
    assert_eq!(site.expected, None);
    assert_eq!(site.found, None);
    assert_eq!(site.def, None);
    assert_eq!(
        site.message, drifted,
        "the RAW message must survive verbatim so a drifted row stays usable"
    );
}

#[test]
fn survey_site_rejects_non_ctor_conformance_diagnostics() {
    use reify_core::diagnostics::DiagnosticCode;

    // A code outside the 7-variant set is not a survey site.
    let d = synth(
        DiagnosticCode::TraitNotImplemented,
        "type 'Bolt' does not implement trait 'Fastener'",
        None,
    );
    assert!(
        survey_site_from_diagnostic("a.ri", "", &d).is_none(),
        "only the 7 ctor-conformance codes may enter the survey"
    );

    // An uncoded (legacy) diagnostic is likewise not a site.
    let mut uncoded = reify_core::Diagnostic::error("argument 'a' has type 'Int'");
    uncoded.severity = reify_core::Severity::Warning;
    assert!(survey_site_from_diagnostic("a.ri", "", &uncoded).is_none());
}

#[test]
fn survey_site_carries_file_and_resolved_line() {
    use reify_core::{Severity, diagnostics::DiagnosticCode, diagnostics::DiagnosticLabel};

    let source = "module test.x\nstructure def Root {\n    let q = Widget(label: 42)\n}\n";
    let at_ctor = source.find("Widget(").expect("fixture") as u32;
    let mut d = reify_core::Diagnostic::error(
        "argument 'label' has type 'Int' but param 'label' requires type 'String'",
    )
    .with_code(DiagnosticCode::ArgTypeMismatch)
    .with_label(DiagnosticLabel::new(
        reify_core::SourceSpan::new(at_ctor, at_ctor + 6),
        "expected 'String', got 'Int'",
    ));
    d.severity = Severity::Warning;

    let site = survey_site_from_diagnostic("examples/x.ri", source, &d).expect("site");
    assert_eq!(site.file, "examples/x.ri");
    assert_eq!(site.line, 3, "the ctor is on line 3 of the fixture");
    assert_eq!(
        site.def.as_deref(),
        Some("Widget"),
        "the call-site anchor must recover the def name for the expression path"
    );
    // Not yet classified: `d9_owner` is applied by the corpus sweep, and the
    // conservative default is the unattributable bucket, never `NonFea`.
    assert_eq!(site.owner, Owner::Unknown);
}

// ─── step 7/8: D9 owner classification ───────────────────────────────────────

#[test]
fn scan_structure_defs_reads_only_the_listed_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\nstructure def Alpha { param a : Real }\nstructure def Beta { }\n",
    )
    .expect("write fea.ri");
    std::fs::write(
        dir.path().join("joints.ri"),
        "module std.joints\nstructure def Gamma { }\n",
    )
    .expect("write joints.ri");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert_eq!(
        defs.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Alpha", "Beta"],
        "only the LISTED module's defs may enter the FEA partition"
    );
    assert!(
        !defs.contains("Gamma"),
        "an unlisted module's defs must not be classified as FEA-owned"
    );
}

#[test]
fn scan_structure_defs_ignores_structure_def_prose_inside_comments() {
    // Measured, not hypothetical: `stdlib/fea_multi_case.ri` line 292 contains
    // the comment "// is a strict relaxation for PointLoad (its structure def
    // already declares". A naive substring scan harvests `already` as a def
    // name and would mis-classify any site whose def is literally named that.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("fea.ri"),
        "module std.fea\n\
         // its structure def already declares point and force\n\
         structure def Real1 { }\n\
             structure def Indented { }\n",
    )
    .expect("write");

    let defs = scan_structure_defs(dir.path(), &["fea"]);
    assert!(
        !defs.contains("already"),
        "prose inside a comment must not enter the def set, got {defs:?}"
    );
    assert!(defs.contains("Real1"), "a real column-0 def must be found");
}

#[test]
#[should_panic(expected = "fea_nonexistent")]
fn scan_structure_defs_panics_when_a_listed_module_is_missing() {
    // A stdlib rename must not silently EMPTY the do-not-touch partition and
    // mis-classify every deferred site as touchable. Fail loud instead.
    let dir = tempfile::tempdir().expect("tempdir");
    let _ = scan_structure_defs(dir.path(), &["fea_nonexistent"]);
}

#[test]
fn fea_owned_defs_scans_the_real_stdlib() {
    let defs = fea_owned_defs();
    assert!(!defs.is_empty(), "the real FEA stdlib declares structure defs");
    for expected in ["PointLoad", "FixedSupport", "LoadCase", "PressureLoad"] {
        assert!(
            defs.contains(expected),
            "'{expected}' is declared in stdlib/fea_multi_case.ri and must be FEA-owned; got {} defs",
            defs.len()
        );
    }
    assert!(
        !defs.contains("already"),
        "the comment-prose false positive must not reach the real scan either"
    );
}

#[test]
fn d9_owner_classifies_fea_non_fea_and_unattributed() {
    let fea = fea_owned_defs();

    assert_eq!(
        d9_owner(Some("PointLoad"), fea),
        Owner::FeaDeferredToV06,
        "D9: FEA defs are call-site-only; field-type flips stay v0.6-owned"
    );
    assert_eq!(
        d9_owner(Some("NotAnFeaStructureDefAnywhere"), fea),
        Owner::NonFea,
        "a def that is not FEA-owned falls under D9's per-case judgment"
    );
    assert_eq!(
        d9_owner(None, fea),
        Owner::Unknown,
        "an unattributable site (the sub `=` per-arg anchor) must NEVER silently \
         default into the touchable bucket"
    );
}

#[test]
fn remedy_hint_is_a_pure_deterministic_function_of_the_type_pair() {
    // Same input -> same output, no I/O, no ordering dependence.
    let a = remedy_hint(Some("Selector(Face)"), Some("String"));
    let b = remedy_hint(Some("Selector(Face)"), Some("String"));
    assert_eq!(a, b, "remedy_hint must be deterministic");

    // Distinct recognised pairs map to DISTINCT fixed strings.
    let string_at_selector = remedy_hint(Some("Selector(Face)"), Some("String"));
    let pose_at_selector = remedy_hint(Some("Selector(Face)"), Some("Frame(3)"));
    let bare_at_dimensioned = remedy_hint(Some("Scalar[m·s^-1]"), Some("Real"));
    assert_ne!(string_at_selector, pose_at_selector);
    assert_ne!(string_at_selector, bare_at_dimensioned);
    assert_ne!(pose_at_selector, bare_at_dimensioned);
    for h in [&string_at_selector, &pose_at_selector, &bare_at_dimensioned] {
        assert!(!h.is_empty(), "a recognised pair must produce a hint");
    }

    // An unrecognised pair, and a pair with a missing half, get a NEUTRAL
    // string — never an invented remedy.
    let neutral = remedy_hint(None, None);
    assert_eq!(remedy_hint(Some("Widget"), Some("Gadget")), neutral);
    assert_eq!(remedy_hint(Some("Selector(Face)"), None), neutral);
    assert_eq!(remedy_hint(None, Some("String")), neutral);
    assert_ne!(
        neutral, string_at_selector,
        "the neutral string must be distinguishable from a real hint"
    );
}

#[test]
fn remedy_hint_never_rules_between_d9_class_1_and_class_2() {
    // The PRD assigns "call-site bug vs wrong declared field type" to γ as a
    // per-case judgment. β emits an ADVISORY hint; it must not claim a verdict.
    for (e, f) in [
        (Some("Selector(Face)"), Some("String")),
        (Some("Selector(Face)"), Some("Frame(3)")),
        (Some("Scalar[m·s^-1]"), Some("Real")),
        (Some("String"), Some("Int")),
        (None, None),
    ] {
        let h = remedy_hint(e, f);
        assert!(
            !h.contains("must ") && !h.contains("the bug is"),
            "the hint is advisory, not a ruling; got {h:?} for ({e:?}, {f:?})"
        );
    }
}
