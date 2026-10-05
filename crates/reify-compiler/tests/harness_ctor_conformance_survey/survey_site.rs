//! One ctor-conformance site as the survey records it, and its extraction from
//! a compiler diagnostic.

use reify_test_support::is_ctor_conformance_code;

use crate::owner::Owner;

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

/// Where a row's `def` came from — or, when it is absent, WHY.
///
/// Recorded PER ROW so the artifact never has to *assert* a cause in prose. An
/// earlier draft of the Unknown-group blurb claimed those rows "come through the
/// sub `=` per-arg anchor" — a hand-derived cause, and the failure mode this enum
/// exists to remove: several distinct shapes reach the same unattributed group,
/// and which one a given row took is knowable only where the recovery actually
/// ran. So no prose here restates a per-row cause; the artifact's `def source`
/// column carries the machine-derived arm, one row at a time. The arm docs
/// below describe only what each arm MATCHES — never which rows are in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefOrigin {
    /// Recovered from the ctor call-site span anchor (α's expression path).
    CallSiteAnchor,
    /// Recovered from ε prose (`in call to '<Def>'` / `<Def>() expects …`).
    DiagnosticProse,
    /// The diagnostic carried no label at all, so there was no span to read.
    NoLabel,
    /// The label span points past the end of the file, or is the prelude
    /// sentinel — no source text exists at it.
    SpanOutOfRange,
    /// The label span starts inside a multi-byte codepoint; identifiers are
    /// ASCII-led, so this can never be a ctor anchor.
    SpanMidCodepoint,
    /// The span starts at something that is not an identifier — a literal, an
    /// operator, a delimiter.
    SpanNotIdentifier,
    /// An identifier was found but is not followed by `(`, so it is a plain
    /// reference rather than a call.
    IdentifierNotACall,
}

impl DefOrigin {
    /// Stable cell text for the artifact's `def source` column.
    pub(crate) fn label(self) -> &'static str {
        match self {
            DefOrigin::CallSiteAnchor => "ctor call-site anchor",
            DefOrigin::DiagnosticProse => "diagnostic prose",
            DefOrigin::NoLabel => "unrecovered: diagnostic carries no label",
            DefOrigin::SpanOutOfRange => "unrecovered: label span out of range",
            DefOrigin::SpanMidCodepoint => "unrecovered: label span mid-codepoint",
            DefOrigin::SpanNotIdentifier => "unrecovered: label span starts at a non-identifier",
            DefOrigin::IdentifierNotACall => "unrecovered: identifier not followed by `(`",
        }
    }
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
    ctor_type_name_at_with_origin(source, span).0
}

/// [`ctor_type_name_at`], plus the machine-derived [`DefOrigin`] saying how
/// recovery succeeded or why it failed.
fn ctor_type_name_at_with_origin(
    source: &str,
    span: reify_core::SourceSpan,
) -> (Option<String>, DefOrigin) {
    let start = span.start as usize;
    if start >= source.len() {
        return (None, DefOrigin::SpanOutOfRange);
    }
    // A span that starts inside a multi-byte codepoint cannot be a ctor anchor
    // (identifiers are ASCII-led), and slicing at it would panic.
    if !source.is_char_boundary(start) {
        return (None, DefOrigin::SpanMidCodepoint);
    }
    let rest = &source[start..];
    let mut chars = rest.char_indices();
    // Rust-identifier shape: first char alphabetic or `_`, then alphanumeric
    // or `_`. Reify def names are a subset of this.
    let Some((_, first)) = chars.next() else {
        return (None, DefOrigin::SpanOutOfRange);
    };
    if !(first.is_alphabetic() || first == '_') {
        return (None, DefOrigin::SpanNotIdentifier);
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
        (Some(ident.to_owned()), DefOrigin::CallSiteAnchor)
    } else {
        (None, DefOrigin::IdentifierNotACall)
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

/// One ctor-conformance site, as one row of the survey artifact.
///
/// Every field is machine-derived; nothing here is ever typed in by hand. An
/// extractor that cannot recover its column yields `None`, which renders as an
/// em-dash — never an empty cell and never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurveySite {
    /// Repo-relative forward-slash path of the swept file.
    pub(crate) file: String,
    /// 1-based line of the diagnostic's first label span (or 1 when unlabelled).
    pub(crate) line: u32,
    /// Structure def being constructed, when recoverable.
    pub(crate) def: Option<String>,
    /// How [`SurveySite::def`] was recovered — or, when it is `None`, why it
    /// could not be. Machine-derived; see [`DefOrigin`].
    pub(crate) def_origin: DefOrigin,
    /// Offending field / param name, when the wording carries one.
    pub(crate) field: Option<String>,
    /// Declared param type, from the `expected '<X>', got '<Y>'` label.
    pub(crate) expected: Option<String>,
    /// Supplied arg type, from the same label.
    pub(crate) found: Option<String>,
    /// `Debug` rendering of the `DiagnosticCode` (PascalCase).
    pub(crate) code: String,
    /// `Debug` rendering of the measured `Severity` — reported, not assumed.
    pub(crate) severity: String,
    /// The diagnostic's raw message, preserved verbatim.
    pub(crate) message: String,
    /// D9 owner class. Assigned by the corpus sweep via [`d9_owner`](crate::owner::d9_owner); the
    /// builder leaves it `Unknown`, the conservative default.
    pub(crate) owner: Owner,
    /// The 1-based line WITHIN the embedded snippet, for a row that came from
    /// an inline Rust fixture; `None` for a tracked `.ri` row, where
    /// [`SurveySite::line`] already IS the file line.
    pub(crate) snippet_line: Option<u32>,
}

/// The `emit_arg_type_mismatch` prose prefix that introduces the offending param
/// name. Also a substring of `emit_geometry_trait_violation`'s
/// `geometry argument '` and of ε's `unknown named argument '`, so one search
/// covers six of the seven codes.
pub(crate) const ARG_PREFIX: &str = "argument '";

/// The prefix used by the two non-geometry `TypeNotConformingToTrait` emitters
/// (`type 'X' does not conform to trait 'T' required by param 'f'`), which name
/// the param nowhere else.
const REQUIRED_BY_PARAM_PREFIX: &str = "required by param '";

/// The ε `CtorUnknownField` prose that names the target structure def.
const IN_CALL_TO_PREFIX: &str = "in call to '";

/// The ε `CtorArity` message prefix; the def name follows it, up to `()`.
pub(crate) const CTOR_ARITY_PREFIX: &str = "E_CTOR_ARITY: ";

/// The single-quoted token immediately following `prefix` in `haystack`.
///
/// This is the guarded quoted-token idiom from `examples_smoke.rs`'s
/// `param_name_from_ctor_diagnostic`, lifted verbatim rather than re-invented,
/// and it carries that helper's warning forward: **this is a real coupling to
/// diagnostic prose.** Every extractor built on it therefore returns `Option`,
/// and [`survey_site_from_diagnostic`] preserves the RAW message on a miss, so
/// a future wording drift degrades to a still-usable row instead of a silently
/// dropped site or a fabricated field.
///
/// An EMPTY token (`prefix` immediately followed by the closing quote) is a
/// miss, not a hit: `Some("")` names nothing, and every caller here would have
/// to re-filter it. Rejecting it at the source keeps that rule in ONE place —
/// and, more importantly, keeps the `or_else` fallback chains below live. An
/// earlier draft filtered AFTER the chain
/// (`quoted_after(A).or_else(|| quoted_after(B)).filter(non-empty)`), where a
/// `Some("")` from `A` short-circuits `or_else` and only then filters to
/// `None` — so `B` is never consulted and the "fallback" is conditionally dead.
fn quoted_after(haystack: &str, prefix: &str) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find('\'')?;
    let token = &rest[..end];
    (!token.is_empty()).then(|| token.to_owned())
}

/// The offending field / param name, across every wording the 7 codes use.
///
/// Returns `None` for `CtorArity` (whose wording names no param) and for any
/// message that has drifted out of all three known shapes.
fn field_of_message(message: &str) -> Option<String> {
    quoted_after(message, ARG_PREFIX).or_else(|| quoted_after(message, REQUIRED_BY_PARAM_PREFIX))
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
/// Each prose shape is tried ONLY for the code that emits it. An earlier draft
/// ran both prefix matches against every admitted code, which is broader than
/// the contract above and opens the one hole this survey cannot afford: any
/// future `ArgTypeMismatch` / `TypeNotConformingToTrait` wording that happened
/// to contain `in call to '<X>'` would be attributed to `<X>` as
/// [`DefOrigin::DiagnosticProse`], bypassing the call-site anchor and its
/// `IdentifierNotACall` / `SpanNotIdentifier` diagnosis — a def GUESSED from a
/// sentence rather than read off an anchor. Gating on `d.code` costs nothing
/// (the ε emitters are the only source of either prefix) and closes it.
///
/// Returns `None` — never a guess — for every anchor shape that names no def,
/// PAIRED WITH the machine-derived [`DefOrigin`] saying which shape it was, so
/// the artifact can report the cause instead of asserting one.
fn def_of_diagnostic(source: &str, d: &reify_core::Diagnostic) -> (Option<String>, DefOrigin) {
    use reify_core::diagnostics::DiagnosticCode;

    if d.code == Some(DiagnosticCode::CtorUnknownField)
        && let Some(def) = quoted_after(&d.message, IN_CALL_TO_PREFIX)
    {
        return (Some(def), DefOrigin::DiagnosticProse);
    }
    if d.code == Some(DiagnosticCode::CtorArity)
        && let Some(rest) = d.message.strip_prefix(CTOR_ARITY_PREFIX)
        && let Some(paren) = rest.find("()")
        && !rest[..paren].is_empty()
    {
        return (Some(rest[..paren].to_owned()), DefOrigin::DiagnosticProse);
    }
    match d.labels.first() {
        None => (None, DefOrigin::NoLabel),
        Some(l) => ctor_type_name_at_with_origin(source, l.span),
    }
}

/// Build one [`SurveySite`] from a diagnostic observed while sweeping `file`.
///
/// Returns `None` only when `d` is not one of the 7 ctor-conformance codes (an
/// uncoded legacy diagnostic included). A ctor-coded diagnostic ALWAYS yields a
/// row, even when every extractor misses — dropping it would silently
/// under-size γ, which is the artifact's whole purpose.
///
/// `owner` is left [`Owner::Unknown`]; the corpus sweep assigns it via
/// [`d9_owner`](crate::owner::d9_owner) once the FEA def set has been scanned.
pub(crate) fn survey_site_from_diagnostic(
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
    let (def, def_origin) = def_of_diagnostic(source, d);
    Some(SurveySite {
        file: file.to_owned(),
        line,
        def,
        def_origin,
        field: field_of_message(&d.message),
        expected,
        found,
        code: format!("{:?}", d.code.expect("filtered to Some(code) above")),
        severity: format!("{:?}", d.severity),
        message: d.message.clone(),
        owner: Owner::Unknown,
        snippet_line: None,
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
    // The severity `CTOR_FIELD_CONFORMANCE_SEVERITY` emits at today: Error since
    // δ/#5306, Warning under α. Set explicitly rather than inherited from
    // `Diagnostic::error` so these fixtures keep modelling the knob if it moves
    // again; every test here is about an EXTRACTOR, and the one that is about the
    // severity column overrides this deliberately.
    d.severity = Severity::Error;
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

    // `emit_selector_mismatch` kind-vs-kind — same `argument '` prefix. The
    // kind renderings are CONSTRUCTED from `reify_core::Type` rather than
    // transcribed: `emit_selector_mismatch` (conformance/mod.rs) interpolates
    // the `Type` Display, which is `FaceSelector`/`EdgeSelector`. An earlier
    // draft of this fixture wrote `Selector(Face)` — a string the compiler never
    // emits, and exactly the bug `is_selector_type` already shipped once — which
    // this helper's "the exact shape a given emitter produces" contract forbids.
    let face = reify_core::Type::Selector(reify_core::ty::SelectorKind::Face).to_string();
    let edge = reify_core::Type::Selector(reify_core::ty::SelectorKind::Edge).to_string();
    let d = synth(
        DiagnosticCode::SelectorKindMismatch,
        &format!(
            "argument 'face' has selector kind '{edge}' but param 'face' \
             requires selector kind '{face}'"
        ),
        Some(&format!("expected '{face}', got '{edge}'")),
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
fn an_empty_quoted_token_is_a_miss_so_the_fallback_prefix_is_still_consulted() {
    // `quoted_after` rejects an empty token at the SOURCE rather than leaving
    // each caller to re-filter, so `field_of_message`'s `or_else` chain stays a
    // real fallback. Filtering after the chain instead makes the second prefix
    // conditionally dead: `Some("")` from the first satisfies `or_else`, the
    // filter then turns it into `None`, and a recoverable field renders `—`.
    assert_eq!(
        quoted_after("argument '' has type", ARG_PREFIX),
        None,
        "an empty quoted token names nothing and must not be reported as a hit"
    );
    assert_eq!(
        field_of_message("argument '' … required by param 'part'"),
        Some("part".to_owned()),
        "an empty first token must fall THROUGH to `required by param '`, not \
         short-circuit the chain into None"
    );

    // Positive control, so the assertion above cannot pass vacuously, plus the
    // genuine no-match case.
    assert_eq!(
        field_of_message("argument 'label' has type 'Int'"),
        Some("label".to_owned())
    );
    assert_eq!(
        field_of_message("E_CTOR_ARITY: W() expects at most 1 argument, got 3"),
        None
    );

    // `def_of_diagnostic`'s ε prose path has the same latent shape and is
    // covered by the same source-level rule.
    assert_eq!(quoted_after("in call to ''; …", IN_CALL_TO_PREFIX), None);
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
fn epsilon_prose_is_only_consulted_for_the_epsilon_codes() {
    use reify_core::diagnostics::DiagnosticCode;

    // A NON-ε code whose message happens to contain the ε prose. Nothing stops
    // a future `emit_*` wording from reading "… in call to 'Bar'": the phrase is
    // ordinary English, not a reserved token. If the prose match were tried for
    // every admitted code, `Bar` would be lifted out of that sentence and
    // recorded as the def — a name GUESSED from prose, with `DiagnosticProse`
    // vouching for it, and the call-site anchor never consulted.
    let source = "let x = 42\n";
    let mut d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'label' has type 'Int' but param 'label' requires type 'String' \
         in call to 'Bar'",
        None,
    );
    d = d.with_label(reify_core::diagnostics::DiagnosticLabel::new(
        // Anchored at the `42`, i.e. a shape that names no def.
        reify_core::SourceSpan::new(8, 10),
        "expected 'String', got 'Int'",
    ));
    let site = survey_site_from_diagnostic("a.ri", source, &d).expect("site");
    assert_eq!(
        site.def, None,
        "`in call to '<X>'` must be read ONLY for CtorUnknownField; for any other \
         code the def comes from the anchor or not at all"
    );
    assert_eq!(
        site.def_origin,
        DefOrigin::SpanNotIdentifier,
        "the row must carry the anchor's own machine-derived cause, not `DiagnosticProse`"
    );

    // Same shape for the arity prefix: a non-`CtorArity` code that literally
    // starts with it still routes to the anchor.
    let d = synth(
        DiagnosticCode::CtorUnknownField,
        "E_CTOR_ARITY: Bar() expects at most 2 arguments, got 3",
        None,
    );
    let site = survey_site_from_diagnostic("a.ri", source, &d).expect("site");
    assert_eq!(
        site.def, None,
        "the `E_CTOR_ARITY: ` prefix must be read ONLY for CtorArity"
    );
    assert_eq!(site.def_origin, DefOrigin::NoLabel);
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
    use reify_core::{Severity, diagnostics::DiagnosticCode};

    let mut d = synth(
        DiagnosticCode::ArgTypeMismatch,
        "argument 'a' has type 'Int' but param 'a' requires type 'String'",
        Some("expected 'String', got 'Int'"),
    );
    // Deliberately a severity the knob does NOT emit today: feeding it the
    // current one could not tell a read-off-the-diagnostic column apart from a
    // hardcoded one, and reporting the MEASURED severity is what lets the
    // artifact record a knob flip instead of asserting one.
    d.severity = Severity::Warning;
    let site = survey_site_from_diagnostic("a.ri", "", &d).expect("site");
    // `{:?}` is used because reify-core's serde feature (which supplies the
    // PascalCase wire name) is non-default and not enabled for reify-compiler.
    // Debug renders the identical string — same choice as examples_smoke.rs.
    assert_eq!(site.code, "ArgTypeMismatch");
    assert_eq!(
        site.severity, "Warning",
        "the sweep must report the severity it MEASURED, never the one it expects"
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

// ─── synthetic sites, for the tests of the layers above ──────────────────────

/// One inline row, as `survey_inline_corpus` builds them: a HOST `.rs` file
/// and host line, plus the snippet-relative coordinate that MAKES it inline.
#[cfg(test)]
pub(crate) fn synth_inline_site(file: &str, field: Option<&str>, severity: &str) -> SurveySite {
    SurveySite {
        file: file.to_owned(),
        line: 1450,
        def: None,
        def_origin: DefOrigin::SpanNotIdentifier,
        field: field.map(str::to_owned),
        expected: Some("Scalar[m]".to_owned()),
        found: Some("Real".to_owned()),
        code: "ArgTypeMismatch".to_owned(),
        severity: severity.to_owned(),
        // The `check_param_default_conformance` wording measured live at the two
        // sites VERIFY names, with the param this row actually carries.
        message: format!(
            "argument '{p}' has type 'Real' but param '{p}' requires type 'Scalar[m]'",
            p = field.unwrap_or("z"),
        ),
        owner: Owner::Unknown,
        snippet_line: Some(2),
    }
}

/// A synthetic `.ri` [`SurveySite`] assembled by hand for the renderer and
/// disposition tests, so no corpus compile is needed to exercise the artifact's
/// whole contract.
#[cfg(test)]
pub(crate) fn synth_site(file: &str, line: u32, def: &str, field: &str, owner: Owner) -> SurveySite {
    SurveySite {
        file: file.to_owned(),
        line,
        def: Some(def.to_owned()),
        def_origin: DefOrigin::CallSiteAnchor,
        field: Some(field.to_owned()),
        expected: Some("FaceSelector".to_owned()),
        found: Some("String".to_owned()),
        code: "ArgTypeMismatch".to_owned(),
        severity: "Error".to_owned(),
        // `FaceSelector`, matching the `expected` cell above: that is the
        // rendering `reify_core::Type::Selector(SelectorKind::Face)` actually
        // Displays. `Selector(Face)` appears nowhere in real compiler output.
        message: format!(
            "argument '{field}' has type 'String' but param '{field}' requires type 'FaceSelector'"
        ),
        owner,
        snippet_line: None,
    }
}
