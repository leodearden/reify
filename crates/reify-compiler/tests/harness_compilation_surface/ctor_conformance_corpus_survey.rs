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
            panic!(
                "ctor_conformance_corpus_survey: cannot run `git ls-files` in {WORKSPACE_ROOT}: {e}"
            )
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
    let non_examples = corpus
        .iter()
        .filter(|p| !p.starts_with("examples/"))
        .count();
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
        assert!(
            site.field.is_some(),
            "{code:?} must yield a field, got None"
        );
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
        assert_eq!(
            site.expected, None,
            "{code:?} carries no expected/got label"
        );
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
    let site = survey_site_from_diagnostic("a.ri", "", &d)
        .expect("an unrecognised message must still yield a row — dropping it would under-size γ");
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

/// Absolute path to the stdlib directory whose FEA modules define the
/// do-not-touch partition.
const STDLIB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/stdlib");

/// The FEA stdlib module FILE STEMS, as PRD §4 D9 itself enumerates them
/// ("`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …"), reconciled against the
/// live `ls crates/reify-compiler/stdlib/`.
///
/// This small MODULE list is the survey's only reviewable knob. The def NAMES
/// are always derived by scanning these files, never hand-listed — which is
/// what keeps "zero hand-derived entries" literally true, and keeps the
/// do-not-touch partition traceable to the PRD rather than to β's judgment.
///
/// [`scan_structure_defs`] panics if any stem here has no `.ri` file, so a
/// stdlib rename cannot silently empty the partition.
const FEA_STDLIB_MODULES: &[&str] = &[
    "fea",
    "fea_multi_case",
    "fea_types",
    "materials_fea",
    "solver_buckling",
    "solver_buckling_fns",
    "solver_elastic",
];

/// The `structure def <Name>` declarations in `dir/<stem>.ri` for each `stem`.
///
/// Anchored at COLUMN 0 rather than matched as a substring, deliberately: a
/// naive scan of `stdlib/fea_multi_case.ri` harvests `already` as a def name
/// from the prose "…(its structure def already declares…" in a comment at line
/// 292. Every real declaration in the stdlib is at column 0, and a `//` line is
/// skipped outright, so both halves of that guard are cheap.
///
/// # Panics
///
/// If a listed module has no file. That is the deliberate loud failure: a
/// silently-empty FEA partition would mis-classify every v0.6-deferred site as
/// touchable, which is the single most costly error this artifact could make.
fn scan_structure_defs(
    dir: &std::path::Path,
    modules: &[&str],
) -> std::collections::BTreeSet<String> {
    const DEF_PREFIX: &str = "structure def ";
    let mut defs = std::collections::BTreeSet::new();
    for stem in modules {
        let path = dir.join(format!("{stem}.ri"));
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "ctor_conformance_corpus_survey: FEA stdlib module '{stem}' is listed in \
                 FEA_STDLIB_MODULES but {} cannot be read: {e}. A stdlib rename must not \
                 silently empty the D9 do-not-touch partition — update the list.",
                path.display()
            )
        });
        for line in source.lines() {
            // Column-0 anchor: skips comments and any nested/indented prose.
            let Some(rest) = line.strip_prefix(DEF_PREFIX) else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                defs.insert(name);
            }
        }
    }
    defs
}

/// Every structure def declared by an FEA stdlib module, scanned once.
fn fea_owned_defs() -> &'static std::collections::BTreeSet<String> {
    static DEFS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    DEFS.get_or_init(|| scan_structure_defs(std::path::Path::new(STDLIB_DIR), FEA_STDLIB_MODULES))
}

/// The D9 fix-forward class governing a site whose structure def is `def`.
///
/// Mechanizes exactly the half of D9 that IS decidable — whether the def is
/// FEA-owned, hence call-site-changes-only with field-type flips deferred to
/// v0.6. An unresolved def is its own `Unknown` bucket, never folded into
/// `NonFea`: guessing in the touchable direction is the one classification
/// error with a real cost.
fn d9_owner(def: Option<&str>, fea_defs: &std::collections::BTreeSet<String>) -> Owner {
    match def {
        None => Owner::Unknown,
        Some(name) if fea_defs.contains(name) => Owner::FeaDeferredToV06,
        Some(_) => Owner::NonFea,
    }
}

/// The neutral hint used when no (expected, found) pattern is recognised.
const NO_HINT: &str = "no mechanical hint — γ per-case judgment";

/// Every string `reify_core::Type` renders for a selector-typed field.
///
/// Read off the Display impls, NOT guessed: `SelectorKind`'s four arms render
/// `<Kind>Selector` (`crates/reify-core/src/ty.rs`), and `Type::AnySelector`
/// renders the bare `Selector`. There is no `Selector(Face)` form anywhere —
/// an earlier draft of this file matched exactly that, and so silently gave
/// NO_HINT to every real selector site including the D3 String→selector case
/// that is the PRD's headline illegality.
const SELECTOR_TYPE_RENDERINGS: &[&str] = &[
    "Selector",
    "FaceSelector",
    "EdgeSelector",
    "VertexSelector",
    "BodySelector",
];

/// Whether `ty` renders as a selector-typed field.
fn is_selector_type(ty: &str) -> bool {
    SELECTOR_TYPE_RENDERINGS.contains(&ty)
}

/// Whether `ty` renders as a coordinate pose rather than a region target.
fn is_pose_type(ty: &str) -> bool {
    ty.starts_with("Frame") || ty.starts_with("Transform") || ty.starts_with("Point")
}

/// Whether `ty` renders as a DIMENSIONED scalar (`Scalar[…]`, not bare `Real`).
fn is_dimensioned_scalar(ty: &str) -> bool {
    ty.starts_with("Scalar[")
}

/// An ADVISORY remedy hint, derived purely and deterministically from the
/// (expected, found) type pair.
///
/// This is NOT a D9 ruling. The PRD defines the split between class (1) call-
/// site bug and class (2) wrong declared field type as "per-case judgment …
/// whichever is the actual bug" and assigns it to γ; fabricating a verdict here
/// would be exactly the hand-derivation β is forbidden. Every string below
/// therefore describes what the *shape* of the mismatch suggests, and the
/// artifact's column header says "advisory".
///
/// An unrecognised pair — or one with a missing half — gets [`NO_HINT`], never
/// an invented remedy.
fn remedy_hint(expected: Option<&str>, found: Option<&str>) -> String {
    let (Some(expected), Some(found)) = (expected, found) else {
        return NO_HINT.to_owned();
    };
    if is_selector_type(expected) && found == "String" {
        // D3: implicit String → selector-typed field is newly ILLEGAL; callers
        // move to typed ctors.
        return "selector field given a string — typed ctor such as face(b, \"x_max\") \
                or vertex(b, \"tip\") is the usual replacement"
            .to_owned();
    }
    if is_selector_type(expected) && is_pose_type(found) {
        // D2 pose-vs-set: the fixed hint substring task 4833's fixtures assert.
        return "selector field given a coordinate pose — a pose locates a datum, \
                it does not name a region target"
            .to_owned();
    }
    if is_dimensioned_scalar(expected) && (found == "Real" || found == "Int") {
        // D4-6 dimensioned-scalar migration family.
        return "dimensioned scalar field given a bare number — a dimensioned \
                literal (e.g. 1m/s) is the usual replacement"
            .to_owned();
    }
    if expected == "String" && (found == "Int" || found == "Real" || found == "Bool") {
        return "string field given a non-string literal".to_owned();
    }
    NO_HINT.to_owned()
}

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
    assert!(
        !defs.is_empty(),
        "the real FEA stdlib declares structure defs"
    );
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
    let a = remedy_hint(Some("FaceSelector"), Some("String"));
    let b = remedy_hint(Some("FaceSelector"), Some("String"));
    assert_eq!(a, b, "remedy_hint must be deterministic");

    // Distinct recognised pairs map to DISTINCT fixed strings.
    let string_at_selector = remedy_hint(Some("FaceSelector"), Some("String"));
    let pose_at_selector = remedy_hint(Some("FaceSelector"), Some("Frame(3)"));
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
    assert_eq!(remedy_hint(Some("FaceSelector"), None), neutral);
    assert_eq!(remedy_hint(None, Some("String")), neutral);
    assert_ne!(
        neutral, string_at_selector,
        "the neutral string must be distinguishable from a real hint"
    );
}

#[test]
fn selector_type_renderings_match_what_reify_core_actually_displays() {
    use reify_core::Type;
    use reify_core::ty::SelectorKind;

    // Pin the table against the REAL Display impl by constructing types and
    // rendering them, rather than hand-transcribing wire forms. An earlier
    // draft matched `Selector(Face)` — a string the compiler never emits — so
    // every real selector site fell through to the neutral hint. Constructing
    // the values here means a Display rename goes RED instead of silently
    // re-emptying the selector arm.
    for kind in [
        SelectorKind::Face,
        SelectorKind::Edge,
        SelectorKind::Vertex,
        SelectorKind::Body,
    ] {
        let rendered = Type::Selector(kind).to_string();
        assert!(
            is_selector_type(&rendered),
            "Type::Selector({kind:?}) renders as {rendered:?}, which is_selector_type \
             does not recognise"
        );
    }
    let any = Type::AnySelector.to_string();
    assert!(
        is_selector_type(&any),
        "Type::AnySelector renders as {any:?}, which is_selector_type does not recognise"
    );

    // And the D3 case end-to-end: a String at a selector-typed field must get
    // the typed-ctor hint, not the neutral fallback.
    let hint = remedy_hint(Some(&any), Some("String"));
    assert_ne!(
        hint, NO_HINT,
        "the D3 String→selector case is the PRD's headline illegality; it must \
         carry a hint"
    );
    assert!(
        hint.contains("face(b"),
        "the hint names the typed-ctor replacement"
    );

    // Pose Display forms are `Frame3` / `Transform3` / `Point3<Length>`.
    for pose in [
        Type::Frame(3).to_string(),
        Type::Transform(3).to_string(),
        Type::point3(Type::length()).to_string(),
    ] {
        assert!(
            is_pose_type(&pose),
            "{pose:?} must be recognised as a coordinate pose"
        );
        assert_ne!(
            remedy_hint(Some(&any), Some(&pose)),
            NO_HINT,
            "the D2 pose-vs-set case must carry a hint for {pose:?}"
        );
    }
}

#[test]
fn remedy_hint_never_rules_between_d9_class_1_and_class_2() {
    // The PRD assigns "call-site bug vs wrong declared field type" to γ as a
    // per-case judgment. β emits an ADVISORY hint; it must not claim a verdict.
    for (e, f) in [
        (Some("FaceSelector"), Some("String")),
        (Some("FaceSelector"), Some("Frame(3)")),
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

// ─── step 9/10: end-to-end sweep over a synthetic mini-corpus ────────────────

/// The result of one pass over a corpus.
///
/// `surveyed + not_surveyed.len() == total` is an invariant: every member is
/// accounted for. The house "no silent caps" rule applies directly here — a
/// bounded sweep must state what it dropped, or the artifact reads as full
/// coverage and under-sizes γ.
#[derive(Debug, Default)]
struct SurveyRun {
    /// Every member handed in — the coverage denominator.
    total: usize,
    /// Members that reached the compile phase and contributed their sites.
    surveyed: usize,
    /// `(path, reason)` for members that contributed NO sites at all.
    /// Reasons: `read-error`, `parse-error`.
    not_surveyed: Vec<(String, String)>,
    /// `(path, reason)` for members that WERE surveyed but whose compile also
    /// produced Error-severity diagnostics — their ctor sites are collected,
    /// but coverage of that file may be partial. Reason: `compile-error`.
    ///
    /// A separate bucket from `not_surveyed` on purpose: `compile_with_stdlib`
    /// is the SINGLE-FILE path (`reify check` instead uses
    /// `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows
    /// `#cfg`-gated user imports), so every multi-module corpus member — the
    /// `examples/module_visibility/consumer.ri` class — lands here. Calling
    /// those "not surveyed" would understate coverage; calling them fully
    /// surveyed would overstate it. Naming them is the honest third option.
    partial: Vec<(String, String)>,
    /// Every ctor-conformance site found, sorted `(file, line, field)`.
    sites: Vec<SurveySite>,
}

/// Sweep `rel_paths` (resolved against `root`) and collect every
/// ctor-conformance site.
///
/// Mirrors `examples_smoke.rs`'s `ctor_conformance_one` — read →
/// `parse_with_stdlib(&source, ModulePath::single(stem))` →
/// `compile_with_stdlib` → filter `compiled.diagnostics` by
/// `is_ctor_conformance_code` — so the survey and the landed α corpus gate
/// cannot disagree about what a ctor-conformance site IS.
///
/// Two deliberate differences from that gate:
/// 1. The root widens from `examples/` to whatever corpus is handed in.
/// 2. Read and parse failures are RECORDED rather than panicked-on or silently
///    `return`ed. The non-examples corpus contains many intentionally
///    unparseable fixtures, and omitting them would inflate apparent coverage.
fn survey_corpus(root: &std::path::Path, rel_paths: &[String]) -> SurveyRun {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::{ModulePath, Severity};

    let fea = fea_owned_defs();
    let mut run = SurveyRun {
        total: rel_paths.len(),
        ..SurveyRun::default()
    };

    for rel in rel_paths {
        let path = root.join(rel);
        let Ok(source) = std::fs::read_to_string(&path) else {
            run.not_surveyed
                .push((rel.clone(), "read-error".to_owned()));
            continue;
        };
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let parsed = parse_with_stdlib(&source, ModulePath::single(&stem));
        if !parsed.errors.is_empty() {
            run.not_surveyed
                .push((rel.clone(), "parse-error".to_owned()));
            continue;
        }

        let compiled = compile_with_stdlib(&parsed);
        run.surveyed += 1;
        if compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            run.partial.push((rel.clone(), "compile-error".to_owned()));
        }
        for d in compiled
            .diagnostics
            .iter()
            .filter(|d| is_ctor_conformance_code(d.code))
        {
            let Some(mut site) = survey_site_from_diagnostic(rel, &source, d) else {
                continue;
            };
            site.owner = d9_owner(site.def.as_deref(), fea);
            run.sites.push(site);
        }
    }

    // Total order, so the artifact is byte-reproducible regardless of the order
    // members were handed in. `code` and `message` break the remaining ties so
    // two sites at the same (file, line, field) still sort deterministically.
    run.sites.sort_by(|a, b| {
        (&a.file, a.line, &a.field, &a.code, &a.message)
            .cmp(&(&b.file, b.line, &b.field, &b.code, &b.message))
    });
    run.not_surveyed.sort();
    run.partial.sort();
    run
}

/// The known-WARNING member: PRD §7 boundary-test row 2, reused verbatim from
/// `struct_ctor_field_conformance_tests.rs`'s `SOURCE_ROW2_VALUE_CELL_STRING`.
///
/// Using α's own landed fixture means the end-to-end test asserts against a
/// site shape the compiler is ALREADY proven to emit — the premise is verified
/// live on `main`, not guessed.
#[cfg(test)]
const SYNTH_WARNS: &str = "module test.row2\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: 42)\n\
     }\n";

/// The known-CLEAN member: the same source with a conforming argument.
#[cfg(test)]
const SYNTH_CLEAN: &str = "module test.clean\n\
     structure def Widget { param label : String }\n\
     structure def Root {\n\
     \x20   let x = Widget(label: \"ok\")\n\
     }\n";

/// The known-UNPARSEABLE member. The tracked corpus really does contain
/// deliberately-unparseable negative fixtures and tree-sitter parser-corpus
/// inputs, so the sweep must record them rather than panic.
#[cfg(test)]
const SYNTH_BROKEN: &str = "module test.broken\n((( this is not reify at all ]]] §§§\n";

/// Write the three synthetic members into a temp dir and return `(dir, paths)`.
#[cfg(test)]
fn synth_corpus() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, source) in [
        ("warns.ri", SYNTH_WARNS),
        ("clean.ri", SYNTH_CLEAN),
        ("broken.ri", SYNTH_BROKEN),
    ] {
        std::fs::write(dir.path().join(name), source).expect("write synthetic member");
    }
    let paths = vec![
        "broken.ri".to_owned(),
        "clean.ri".to_owned(),
        "warns.ri".to_owned(),
    ];
    (dir, paths)
}

#[test]
fn survey_corpus_finds_the_known_warning_site_with_every_column_resolved() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(
        run.sites.len(),
        1,
        "exactly one ctor-conformance site across the mini-corpus, got: {:#?}",
        run.sites
    );
    let site = &run.sites[0];
    assert_eq!(site.file, "warns.ri");
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Warning",
        "α's knob is Warning — the sweep must report what it MEASURED, never assume Error"
    );
    assert_eq!(site.field.as_deref(), Some("label"));
    assert_eq!(site.def.as_deref(), Some("Widget"));
    assert_eq!(site.expected.as_deref(), Some("String"));
    assert_eq!(site.found.as_deref(), Some("Int"));
    assert_eq!(
        site.line, 4,
        "the ctor is on line 4 of the fixture; got line {} for {:?}",
        site.line, site.message
    );
    assert_eq!(
        site.owner,
        Owner::NonFea,
        "`Widget` is not an FEA stdlib def — the sweep must CLASSIFY, not leave Unknown"
    );
}

#[test]
fn survey_corpus_records_unsurveyable_members_instead_of_dropping_them() {
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    assert_eq!(run.total, 3, "the denominator is every file handed in");
    let broken: Vec<&(String, String)> = run
        .not_surveyed
        .iter()
        .filter(|(f, _)| f == "broken.ri")
        .collect();
    assert_eq!(
        broken.len(),
        1,
        "the unparseable member must be RECORDED, not silently dropped: {:#?}",
        run.not_surveyed
    );
    assert_eq!(
        broken[0].1, "parse-error",
        "its reason must name the phase that failed"
    );

    // "No silent caps": the coverage denominator has to be visible, or γ is
    // sized against a survey that reads as full coverage but is not.
    assert_eq!(
        run.surveyed + run.not_surveyed.len(),
        run.total,
        "surveyed + not_surveyed must account for every corpus member"
    );
    assert_eq!(run.surveyed, 2, "the two parseable members are surveyed");
}

#[test]
fn survey_corpus_does_not_leak_prelude_diagnostics_into_every_file() {
    // If stdlib-prelude diagnostics were re-attributed to each swept file, the
    // artifact would inflate ~660x and be worthless. Two distinct files, each
    // yielding ONLY its own sites, is the cheap pin on that.
    let (dir, paths) = synth_corpus();
    let run = survey_corpus(dir.path(), &paths);

    let clean_sites = run.sites.iter().filter(|s| s.file == "clean.ri").count();
    assert_eq!(
        clean_sites, 0,
        "the conforming member must contribute ZERO sites; prelude leakage would \
         give it the same site count as every other file"
    );

    // Sweeping the clean member ALONE must likewise be empty.
    let solo = survey_corpus(dir.path(), &["clean.ri".to_owned()]);
    assert!(
        solo.sites.is_empty(),
        "a clean file swept alone must yield no sites, got: {:#?}",
        solo.sites
    );
    assert_eq!(solo.surveyed, 1);
    assert_eq!(solo.total, 1);
}

#[test]
fn survey_corpus_records_a_read_error_rather_than_panicking() {
    let (dir, _) = synth_corpus();
    let run = survey_corpus(dir.path(), &["no_such_file.ri".to_owned()]);
    assert_eq!(run.total, 1);
    assert_eq!(run.surveyed, 0);
    assert_eq!(
        run.not_surveyed,
        vec![("no_such_file.ri".to_owned(), "read-error".to_owned())],
        "an unreadable member is recorded with its reason, never a panic that \
         would abort a 660-file sweep"
    );
}

#[test]
fn survey_corpus_orders_sites_deterministically() {
    // Byte-reproducibility of the artifact starts here: the same corpus handed
    // in a different order must produce the same site list.
    let (dir, paths) = synth_corpus();
    let forward = survey_corpus(dir.path(), &paths);
    let mut reversed = paths.clone();
    reversed.reverse();
    let backward = survey_corpus(dir.path(), &reversed);
    assert_eq!(
        forward.sites, backward.sites,
        "site ordering must not depend on the order files are handed in"
    );
}

// ─── step 11/12: markdown rendering ─────────────────────────────────────────

/// The EXACT command that regenerates the artifact, committed inside it.
///
/// The `env` prefix is not decoration: reify's PreToolUse hook rewrites bare
/// `cargo test` invocations into condensed `PASS: N | FAIL: M` output. That is
/// harmless here — the generator WRITES the file rather than being scraped from
/// stdout — but it will confuse a reader of the run log who expects to see the
/// usual per-test lines, so the bypass is baked into the published command.
const REGEN_COMMAND: &str = "env cargo test -p reify-compiler --test harness_compilation_surface \
     -- --ignored --exact \
     ctor_conformance_corpus_survey::generate_ctor_conformance_corpus_survey";

/// Render `text` safe for a markdown table cell.
///
/// Both `|` and newlines are neutralised: either one inside a diagnostic
/// message would silently split or truncate the row, and a survey whose table
/// breaks on its most interesting entries is worse than no survey.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

/// Render an optional column: `—` when unrecoverable, never empty, never a guess.
fn opt_cell(value: Option<&String>) -> String {
    match value {
        Some(v) => cell(v),
        None => "—".to_owned(),
    }
}

/// Render the survey artifact.
///
/// Follows the house convention for a generated markdown artifact set by
/// `docs/architecture-audit/g-tool-baseline-report.md`: a
/// `**Captured:** / **Tool:** / **Design:**` header block above a
/// `## How to regenerate` section holding the literal command.
///
/// Deliberate divergence from that report: it pairs with an `#[ignore]`d
/// tolerance-based freshness test because it is a STANDING baseline. This
/// survey is a point-in-time SNAPSHOT that γ will legitimately invalidate — a
/// freshness gate would go red on every γ commit and would be driven to an
/// EMPTY artifact the moment γ reaches its stated signal, destroying the very
/// census that sized it. So the base commit SHA is stamped instead.
fn render_survey(run: &SurveyRun, base_commit: &str) -> String {
    use std::fmt::Write as _;

    let mut md = String::new();
    let site_count = run.sites.len();

    // ── header ──────────────────────────────────────────────────────────────
    md.push_str("# Struct-ctor field-type conformance — corpus survey\n\n");
    let _ = writeln!(md, "**Base commit:** `{base_commit}`");
    let _ = writeln!(
        md,
        "**Tool:** `crates/reify-compiler/tests/harness_compilation_surface/ctor_conformance_corpus_survey.rs`"
    );
    md.push_str("**Design:** `docs/prds/struct-ctor-field-type-conformance.md` (task β, §8)\n");
    let _ = writeln!(md, "**Sites:** {site_count}");
    let _ = writeln!(
        md,
        "**Corpus:** {} tracked `.ri`; {} surveyed, {} not surveyed, {} partial",
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    md.push_str(
        "\n\
        This is a point-in-time **snapshot**, not a freshness-gated golden file. γ will\n\
        legitimately invalidate it — that is the point. Its job is to enumerate and size,\n\
        once, at the base commit stamped above.\n\
        \n\
        ## Provenance\n\
        \n\
        Every row and every count here is **machine-generated — zero hand-derived\n\
        entries**. The corpus is `git ls-files -- '*.ri'`; each member is compiled with\n\
        the α(+ε) warn-stage compiler in-process (`parse_with_stdlib` →\n\
        `compile_with_stdlib`) and every diagnostic carrying one of the seven\n\
        ctor-conformance codes becomes one row. The `file:line` comes from the\n\
        diagnostic's own label span; `expected`/`found` come from the label message.\n\
        Nothing below was typed in by hand, and a column that could not be recovered\n\
        renders as `—` rather than as a guess.\n\
        \n\
        Two things to know before reading a row:\n\
        \n\
        - **`line` is the CTOR CALL-SITE line, not the offending argument's line.** α\n\
          anchors the label at the `Foo(...)` call's own span (PRD §10 Q1;\n\
          `compile_builder/entities_phase.rs`), so a multi-line ctor reports the line of\n\
          its opening `Foo(`. The offending argument is named in the `field` column and\n\
          sits within that call — e.g.\n\
          `examples/trajectory/printer_print_envelope.ri:169` is the `TOTSShaper(` line,\n\
          while `velocity_limit: 300.0` is three lines further down.\n\
        - **`def` is the identifier at that anchor,** which is a `structure def` name for\n\
          the ctor path. A few rows carry codes that reach this survey from a NON-ctor\n\
          path (selector composition, overload resolution) and are Error- rather than\n\
          Warning-severity; for those the anchor identifier can be a *function* name.\n\
          The `severity` and `message` columns disambiguate.\n\
        \n\
        The **`hint` column is ADVISORY**, derived purely from the (expected, found)\n\
        type pair. It is **not** a D9 ruling. PRD §4 D9 defines the split between class\n\
        (1) *call-site bug* and class (2) *wrong declared field type* as \"per-case\n\
        judgment … whichever is the actual bug\" and assigns it to **γ**; β does not\n\
        pre-empt it. What β does decide mechanically is the `owner` grouping below.\n\
        \n\
        ## Format (PRD §10 Q6)\n\
        \n\
        **Q6 is answered here — grouped by D9 owner class, with a flat\n\
        `(file, line, field)`-sorted table inside each group.** Grouping first by owner\n\
        makes the FEA do-not-touch partition unmissable for γ, whose actual consumption\n\
        question is *which sites may I touch*; a flat table inside each group keeps the\n\
        result directly sizable and sortable. Recorded in this artifact header rather\n\
        than by editing the PRD's §10, which the sibling α/γ/δ/ζ tasks concurrently read.\n\
        \n",
    );

    // ── site groups, FEA first ──────────────────────────────────────────────
    md.push_str("## Sites\n\n");
    if site_count == 0 {
        md.push_str(
            "**No ctor-conformance sites were found in the surveyed corpus.** This is an\n\
             explicit zero, not a truncated run — see the coverage section below for what\n\
             was and was not surveyed.\n\n",
        );
    }
    for owner in [Owner::FeaDeferredToV06, Owner::NonFea, Owner::Unknown] {
        let mut group: Vec<&SurveySite> = run.sites.iter().filter(|s| s.owner == owner).collect();
        group.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

        let _ = writeln!(md, "### {} — {} site(s)\n", owner.title(), group.len());
        match owner {
            Owner::FeaDeferredToV06 => md.push_str(
                "Per PRD §4 D9, these defs are declared in the FEA stdlib modules: γ may make\n\
                 **call-site changes ONLY**. Field-type flips remain v0.6-owned\n\
                 (`docs/prds/v0_6/fea-load-support-selector-migration.md`). **DO NOT FIX the\n\
                 declared field types here.**\n\n",
            ),
            Owner::NonFea => md.push_str(
                "D9's per-case judgment applies: fix the call site or the declared field type,\n\
                 whichever is the actual bug — γ's ruling, recorded in γ's diff.\n\n",
            ),
            Owner::Unknown => md.push_str(
                "The structure def could not be attributed mechanically — these sites come\n\
                 through the sub `=` per-arg anchor, which carries no ctor name, and the\n\
                 diagnostic prose names none either. Deliberately its own group: folding an\n\
                 unattributable site into the touchable pile is the one classification error\n\
                 with a real cost. **Triage manually before touching.**\n\n",
            ),
        }
        if group.is_empty() {
            md.push_str("_(none)_\n\n");
            continue;
        }
        md.push_str(
            "| site | def | field | expected | found | code | severity | hint (advisory) | message |\n\
             |---|---|---|---|---|---|---|---|---|\n",
        );
        for s in group {
            let _ = writeln!(
                md,
                "| `{}:{}` | {} | {} | {} | {} | `{}` | {} | {} | {} |",
                cell(&s.file),
                s.line,
                opt_cell(s.def.as_ref()),
                opt_cell(s.field.as_ref()),
                opt_cell(s.expected.as_ref()),
                opt_cell(s.found.as_ref()),
                cell(&s.code),
                cell(&s.severity),
                cell(&remedy_hint(s.expected.as_deref(), s.found.as_deref())),
                cell(&s.message),
            );
        }
        md.push('\n');
    }

    // ── coverage + limitations ──────────────────────────────────────────────
    md.push_str("## Coverage and limitations\n\n");
    let _ = writeln!(
        md,
        "Of {} tracked `.ri` members, **{} were surveyed** and **{} were not**. \
         A further **{}** were surveyed only PARTIALLY. Both are listed below rather \
         than dropped: a bounded sweep that does not state what it skipped reads as \
         full coverage and would under-size γ.\n",
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len()
    );

    md.push_str("### Not surveyed (contributed no sites)\n\n");
    if run.not_surveyed.is_empty() {
        md.push_str("_(none — every tracked member reached the compile phase)_\n\n");
    } else {
        md.push_str("| file | reason |\n|---|---|\n");
        for (file, reason) in &run.not_surveyed {
            let _ = writeln!(md, "| `{}` | `{}` |", cell(file), cell(reason));
        }
        md.push('\n');
    }

    md.push_str(
        "### Partially surveyed (sites collected, but the file also failed to compile)\n\n",
    );
    if run.partial.is_empty() {
        md.push_str("_(none)_\n\n");
    } else {
        md.push_str("| file | reason |\n|---|---|\n");
        for (file, reason) in &run.partial {
            let _ = writeln!(md, "| `{}` | `{}` |", cell(file), cell(reason));
        }
        md.push('\n');
    }

    md.push_str(
        "### Named limitations\n\
        \n\
        1. **Inline Rust-string `.ri` fixtures are not file-enumerable.** The task's\n\
           second half — the Rust test suite's inline fixtures and goldens — lives inside\n\
           `const SOURCE: &str = r#\"…\"#` literals, which `git ls-files` cannot reach and\n\
           which could only be swept by changing the compiler (out of scope for this\n\
           read-only survey). Their coverage is **transitive, and stated as such rather\n\
           than claimed**: the `--scope all --profile both` merge gate is green at the\n\
           base commit above, and the landed α/ε gates\n\
           (`no_example_emits_ctor_field_conformance_diagnostics`, the\n\
           `struct_ctor_field_conformance_tests` suite) already assert on the\n\
           ctor-conformance codes.\n\
        2. **`compile_with_stdlib` is the SINGLE-FILE path.** `reify check` instead uses\n\
           `module_dag::compile_entry_with_stdlib_cfg_checked`, which follows `#cfg`-gated\n\
           user imports and runs `SimpleConstraintChecker`. Multi-module corpus members\n\
           (the `examples/module_visibility/consumer.ri` class) therefore cannot resolve\n\
           standalone and appear above under *not surveyed* or *partially surveyed* with\n\
           their reason, rather than being silently dropped.\n\
        \n",
    );

    // ── regeneration ────────────────────────────────────────────────────────
    md.push_str("## How to regenerate\n\n```bash\n");
    let _ = writeln!(md, "{REGEN_COMMAND}");
    md.push_str("```\n\n");
    md.push_str(
        "The generator is `#[ignore]`d: it compiles the whole tracked corpus, which is\n\
        ~2.5× the `examples/` walk already documented as the most expensive thing that\n\
        test binary does, and paying that on every merge gate would fight\n\
        `docs/prds/merge-gate-compile-cost.md`. Everything the generator *decides* —\n\
        enumeration, span→line, def/field/type extraction, D9 classification and this\n\
        rendering — is unit-tested on every gate run against synthetic inputs, plus one\n\
        cheap three-file end-to-end sweep, so the pipeline cannot bit-rot between runs.\n\
        \n\
        Set `REIFY_CTOR_SURVEY_OUT` to write elsewhere (e.g. to diff a fresh run against\n\
        the committed copy without dirtying the tree).\n\
        \n\
        > The `env` prefix on the command above bypasses reify's PreToolUse hook, which\n\
        > condenses `cargo test` output. It is harmless here — the generator writes the\n\
        > file rather than being scraped from stdout — but without it a reader of the run\n\
        > log sees only a `PASS: N | FAIL: M` summary and may think the sweep did nothing.\n",
    );

    md
}

/// A `SurveyRun` assembled by hand for the renderer tests, so no corpus
/// compile is needed to exercise the artifact's whole contract.
#[cfg(test)]
fn synth_site(file: &str, line: u32, def: &str, field: &str, owner: Owner) -> SurveySite {
    SurveySite {
        file: file.to_owned(),
        line,
        def: Some(def.to_owned()),
        field: Some(field.to_owned()),
        expected: Some("FaceSelector".to_owned()),
        found: Some("String".to_owned()),
        code: "ArgTypeMismatch".to_owned(),
        severity: "Warning".to_owned(),
        message: format!(
            "argument '{field}' has type 'String' but param '{field}' requires type 'Selector(Face)'"
        ),
        owner,
    }
}

#[test]
fn render_survey_states_a_site_count_that_equals_the_rendered_rows() {
    let run = SurveyRun {
        total: 660,
        surveyed: 640,
        not_surveyed: vec![("x.ri".to_owned(), "parse-error".to_owned())],
        partial: vec![("y.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![
            synth_site("a.ri", 3, "PointLoad", "point", Owner::FeaDeferredToV06),
            synth_site("b.ri", 7, "Widget", "label", Owner::NonFea),
        ],
    };
    let md = render_survey(&run, "deadbeef");

    // The stated count is COMPUTED, never typed — that is the task's
    // "site count stated" signal, and it must equal the rows actually drawn.
    let rows = md
        .lines()
        .filter(|l| l.starts_with("| `") && l.contains(".ri:"))
        .count();
    assert_eq!(rows, 2, "two sites must draw two table rows, got:\n{md}");
    assert!(
        md.contains("**Sites:** 2"),
        "the header must state the site count; got:\n{md}"
    );
    assert!(md.contains("deadbeef"), "the base commit must be stamped");
    assert!(md.contains("660"), "the live corpus total must be stated");
    assert!(
        md.contains("640"),
        "the surveyed count must be stated so the denominator is visible"
    );
}

#[test]
fn render_survey_groups_by_d9_owner_with_fea_first_and_marked_do_not_fix() {
    let run = SurveyRun {
        total: 3,
        surveyed: 3,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![
            synth_site("z.ri", 1, "Widget", "label", Owner::NonFea),
            synth_site("a.ri", 1, "PointLoad", "point", Owner::FeaDeferredToV06),
            synth_site("m.ri", 1, "Mystery", "f", Owner::Unknown),
        ],
    };
    let md = render_survey(&run, "cafe1234");

    let fea_at = md.find("FEA").expect("FEA group heading");
    let non_fea_at = md.find("non-FEA").expect("non-FEA group heading");
    assert!(
        fea_at < non_fea_at,
        "the do-not-touch FEA partition must come FIRST — γ's first question is \
         which sites it may touch:\n{md}"
    );
    assert!(
        md.contains("DO NOT FIX"),
        "the FEA group must be explicitly labelled do-not-fix:\n{md}"
    );
    assert!(
        md.contains("unattributed"),
        "the Unknown bucket must be rendered as its own group, not folded away"
    );
}

#[test]
fn render_survey_orders_rows_deterministically_within_a_group() {
    let ordered = [
        synth_site("a.ri", 2, "W", "alpha", Owner::NonFea),
        synth_site("a.ri", 9, "W", "beta", Owner::NonFea),
        synth_site("b.ri", 1, "W", "gamma", Owner::NonFea),
    ];
    let mut shuffled = vec![ordered[2].clone(), ordered[0].clone(), ordered[1].clone()];
    // A same-(file,line) pair discriminated only by field must still sort.
    shuffled.push(synth_site("a.ri", 2, "W", "aardvark", Owner::NonFea));

    let mk = |sites: Vec<SurveySite>| SurveyRun {
        total: sites.len(),
        surveyed: sites.len(),
        not_surveyed: vec![],
        partial: vec![],
        sites,
    };
    let mut sorted = shuffled.clone();
    sorted.sort_by(|a, b| (&a.file, a.line, &a.field).cmp(&(&b.file, b.line, &b.field)));

    assert_eq!(
        render_survey(&mk(shuffled), "sha"),
        render_survey(&mk(sorted), "sha"),
        "rows must render in (file, line, field) order regardless of input order"
    );
}

#[test]
fn render_survey_escapes_pipes_and_newlines_so_a_message_cannot_break_the_table() {
    let mut site = synth_site("a.ri", 1, "W", "f", Owner::NonFea);
    site.message = "a | b\nsecond line | c".to_owned();
    site.field = Some("has|pipe".to_owned());
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, "sha");

    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    assert!(
        !row.contains("a | b"),
        "a raw pipe inside a message would split the cell: {row:?}"
    );
    assert!(
        row.contains("second line"),
        "the escaped message must still carry its full text: {row:?}"
    );
    assert!(
        !md.contains("has|pipe"),
        "a pipe inside ANY cell must be escaped, not just the message"
    );
}

#[test]
fn render_survey_writes_an_em_dash_for_every_unrecoverable_cell() {
    let site = SurveySite {
        file: "a.ri".to_owned(),
        line: 1,
        def: None,
        field: None,
        expected: None,
        found: None,
        code: "CtorArity".to_owned(),
        severity: "Warning".to_owned(),
        message: "E_CTOR_ARITY: Bar() expects at most 1 argument, got 2".to_owned(),
        owner: Owner::Unknown,
    };
    let run = SurveyRun {
        total: 1,
        surveyed: 1,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![site],
    };
    let md = render_survey(&run, "sha");
    let row = md
        .lines()
        .find(|l| l.starts_with("| `a.ri:1`"))
        .expect("the site row");
    // Check the four cells BY POSITION rather than counting em-dashes in the
    // whole row: the neutral hint string legitimately contains one too, so a
    // raw count is an imprecise proxy for what this test actually means.
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    for (idx, name) in [(2, "def"), (3, "field"), (4, "expected"), (5, "found")] {
        assert_eq!(
            cells[idx], "—",
            "the {name} cell must render as an em-dash, never an empty or invented \
             cell: {row:?}"
        );
    }
    assert!(
        !row.contains("||"),
        "no cell may be rendered empty: {row:?}"
    );
    assert!(
        cells[9].starts_with("E_CTOR_ARITY:"),
        "the raw message must still be carried verbatim: {row:?}"
    );
}

#[test]
fn render_survey_renders_the_zero_site_case_explicitly() {
    let run = SurveyRun {
        total: 660,
        surveyed: 660,
        not_surveyed: vec![],
        partial: vec![],
        sites: vec![],
    };
    let md = render_survey(&run, "sha");
    assert!(
        md.contains("**Sites:** 0"),
        "the count must still be stated"
    );
    assert!(
        md.to_lowercase().contains("no ctor-conformance"),
        "a zero-site outcome must be rendered as an explicit statement, never an \
         empty table a reader could mistake for a truncated run:\n{md}"
    );
}

#[test]
fn render_survey_carries_the_regeneration_command_and_the_coverage_section() {
    let run = SurveyRun {
        total: 4,
        surveyed: 2,
        not_surveyed: vec![
            ("bad.ri".to_owned(), "parse-error".to_owned()),
            ("gone.ri".to_owned(), "read-error".to_owned()),
        ],
        partial: vec![("multi.ri".to_owned(), "compile-error".to_owned())],
        sites: vec![synth_site("a.ri", 1, "W", "f", Owner::NonFea)],
    };
    let md = render_survey(&run, "sha");

    assert!(
        md.contains("## How to regenerate"),
        "house convention for a generated artifact (cf. \
         docs/architecture-audit/g-tool-baseline-report.md)"
    );
    assert!(
        md.contains(REGEN_COMMAND),
        "the EXACT regeneration command must appear verbatim:\n{md}"
    );

    // Coverage: every not-surveyed and partial member, with its reason.
    for (name, reason) in [
        ("bad.ri", "parse-error"),
        ("gone.ri", "read-error"),
        ("multi.ri", "compile-error"),
    ] {
        assert!(
            md.contains(name) && md.contains(reason),
            "the coverage section must list {name} with reason {reason}:\n{md}"
        );
    }
}

// ─── step 13/14: output path + the generator entry point ─────────────────────

/// Env var that redirects the generator's output to a scratch path.
const OUT_ENV: &str = "REIFY_CTOR_SURVEY_OUT";

/// The committed artifact's repo-relative path.
const ARTIFACT_REL: &str = "docs/prds/struct-ctor-field-type-conformance.survey.md";

/// Resolve the output path from an already-read override value.
///
/// A pure seam so the override can be tested without setting a process-global
/// env var, which would race every other test in this binary. An empty override
/// falls back to the default: an accidental `REIFY_CTOR_SURVEY_OUT=` must not
/// drop the artifact into the current working directory.
fn survey_output_path_for(override_value: Option<String>) -> PathBuf {
    match override_value {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => PathBuf::from(WORKSPACE_ROOT).join(ARTIFACT_REL),
    }
}

/// Where the generator writes: the committed artifact, unless
/// [`OUT_ENV`] redirects it.
///
/// Defaulting to the real location is what lets [`REGEN_COMMAND`] carry no path
/// argument — so the command committed inside the artifact cannot drift from
/// where the artifact actually lives.
fn survey_output_path() -> PathBuf {
    survey_output_path_for(std::env::var(OUT_ENV).ok())
}

/// The `git rev-parse HEAD` of the workspace, stamped into the artifact header.
fn base_commit() -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(WORKSPACE_ROOT)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| panic!("ctor_conformance_corpus_survey: cannot run git: {e}"));
    assert!(
        out.status.success(),
        "ctor_conformance_corpus_survey: `git rev-parse HEAD` exited {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// **The survey generator.** Sweeps every tracked `.ri` and writes the artifact.
///
/// `#[ignore]`d because it compiles the entire tracked corpus — ~2.5× the
/// `examples/` walk that `examples_smoke.rs` already documents as "the single
/// most expensive thing this binary does". Running it on every merge gate would
/// directly fight `docs/prds/merge-gate-compile-cost.md`.
///
/// The ignore reason is deliberately OPERATIONAL, not blocker-prose: per
/// `docs/prds/reify-audit-ptodo-detector.md` §8 (row 8, the
/// `#[ignore = "requires OCCT"]` class) an operational reason produces no PTODO
/// finding and needs no `#NNNN` cite. One is deliberately NOT written here — a
/// cite would be liveness-checked and would go orphaned the moment task #5304
/// closes.
///
/// Nothing is lost to the ignore: every DECISION this test makes lives in the
/// pure helpers above, each unit-tested on every gate run, plus one cheap
/// three-file end-to-end sweep.
#[test]
#[ignore = "corpus survey generator over all tracked .ri (660 files); run explicitly with --ignored — see docs/prds/struct-ctor-field-type-conformance.survey.md"]
fn generate_ctor_conformance_corpus_survey() {
    let corpus = tracked_ri_corpus();
    let run = survey_corpus(std::path::Path::new(WORKSPACE_ROOT), &corpus);
    let rendered = render_survey(&run, &base_commit());
    let out = survey_output_path();
    std::fs::write(&out, &rendered)
        .unwrap_or_else(|e| panic!("cannot write survey to {}: {e}", out.display()));
    println!(
        "ctor-conformance survey: {} sites across {} tracked .ri ({} surveyed, \
         {} not surveyed, {} partial) -> {}",
        run.sites.len(),
        run.total,
        run.surveyed,
        run.not_surveyed.len(),
        run.partial.len(),
        out.display()
    );
}

#[test]
fn survey_output_path_defaults_to_the_committed_artifact_location() {
    // The default must match the artifact's real location exactly, so the
    // regeneration command committed INSIDE the artifact needs no path argument
    // and cannot drift from where the file actually lives.
    //
    // `REIFY_CTOR_SURVEY_OUT` is read per call rather than memoized, but env
    // vars are process-global and this binary runs tests in parallel — so this
    // test asserts on the UNSET default without mutating the environment, and
    // the override is exercised by `survey_output_path_for` below.
    let path = survey_output_path();
    let expected = std::path::Path::new(WORKSPACE_ROOT)
        .join("docs/prds/struct-ctor-field-type-conformance.survey.md");
    assert_eq!(
        path, expected,
        "the default output path must be the committed artifact location"
    );
    let parent = path.parent().expect("the artifact path has a parent");
    assert!(
        parent.is_dir(),
        "the artifact's parent directory {} must exist",
        parent.display()
    );
}

#[test]
fn survey_output_path_honours_the_scratch_override() {
    // The override exists so the sweep can be re-run into a scratch path and
    // diffed against the committed copy WITHOUT dirtying the tree — which is
    // exactly how step 15 proves byte-for-byte reproducibility.
    assert_eq!(
        survey_output_path_for(Some("/tmp/scratch-survey.md".to_owned())),
        std::path::PathBuf::from("/tmp/scratch-survey.md"),
        "REIFY_CTOR_SURVEY_OUT must override the default"
    );
    assert_eq!(
        survey_output_path_for(None),
        survey_output_path(),
        "an unset override must fall back to the committed location"
    );
    assert_eq!(
        survey_output_path_for(Some(String::new())),
        survey_output_path(),
        "an EMPTY override must fall back too — an accidental `REIFY_CTOR_SURVEY_OUT=` \
         must not write the artifact to the current directory"
    );
}
