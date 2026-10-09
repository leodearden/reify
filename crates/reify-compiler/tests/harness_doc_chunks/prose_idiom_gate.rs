//! Every concrete Reify IDIOM written in a gated chunk's unfenced prose must
//! compile: it is a call a designer will copy, so it is held to the criterion a
//! signature fixture's calls are.
//!
//! A PROSE IDIOM is a code span of a chunk's unfenced prose that
//!
//! 1. opens call-shaped — `doc_forms::span_callee` reads a callee;
//! 2. is no signature — `doc_forms::doc_form_of_span` reads none, so every span
//!    has at most one owner and signatures stay `unfenced_signature_gate.rs`'s;
//! 3. elides nothing — no ASCII `...` and no U+2026 `…`, since an elided call
//!    is a schematic mention rather than an idiom.
//!
//! The prose read is `chunk_prose.rs`'s unfenced model, so fence bodies (the
//! fence gate's jurisdiction) and maintainer notes are never scanned.
//!
//! An idiom must parse as ONE Reify expression, bound as the sole `let` of a
//! `structure def`, and compile with no diagnostic that
//! `signature_fixtures::discredits_calls` counts, `DiagnosticCode::UnresolvedName`
//! excepted. An idiom's free identifiers are metavariables, unbound by design.
//! The compiler poisons each one, so it raises no follow-on diagnostic, while a
//! literal argument of the wrong type, a wrong arity or a misspelled callee
//! beside it is still reported.
//!
//! # What is NOT established
//!
//! A slot whose argument is a metavariable is never type-checked: the poisoned
//! name stands in for any type. An expression that does not OPEN with a call
//! (e.g. `2*corner_r < min(width, depth)`) is not compiled. Only
//! [`IDIOM_GATED_CHUNKS`] are read: units.md quotes deliberately-rejected forms in
//! its prose, so admitting it needs a notion of an idiom documented as rejected
//! first.

use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
use reify_core::{DiagnosticCode, ModulePath};

use crate::chunk_io::{chunk_label, read_chunk_file, report};
use crate::chunk_prose::{code_spans, unfenced_prose};
use crate::doc_forms::{doc_form_of_span, span_callee};
use crate::module_compile::ModuleCompile;
use crate::signature_fixtures::{discredits_calls, rendered_diagnostic};

/// What marks a span as eliding part of its argument list.
const ELISIONS: &[&str] = &["...", "…"];

/// One prose idiom: a call-shaped, unelided, non-signature code span.
#[derive(Debug, PartialEq)]
struct ProseIdiom {
    /// 1-based chunk line of the span's opening backtick run.
    line: usize,
    span: String,
}

/// Is the trimmed code span `span` a prose idiom (see the module doc)?
fn is_prose_idiom(span: &str) -> bool {
    span_callee(span).is_some()
        && doc_form_of_span(span).is_none()
        && !ELISIONS.iter().any(|elision| span.contains(elision))
}

/// Every prose idiom in `markdown`'s unfenced prose, trimmed, in document
/// order; `Err` when the prose cannot be read.
fn prose_idioms(markdown: &str) -> Result<Vec<ProseIdiom>, String> {
    Ok(code_spans(&unfenced_prose(markdown)?)
        .into_iter()
        .filter_map(|span| {
            let text = span.text.trim();
            is_prose_idiom(text).then(|| ProseIdiom {
                line: span.line,
                span: text.to_string(),
            })
        })
        .collect())
}

/// What the compiler holds against the idiom `span`, one line each; empty when
/// it compiles clean, its metavariables aside.
///
/// A span that does not parse is ONE line carrying the parser's messages: any
/// compile-layer diagnostic downstream of a broken AST describes the wreckage,
/// not the idiom.
fn idiom_violations(span: &str) -> Vec<String> {
    let source = format!("structure def ProseIdiom {{\n    let idiom = {span}\n}}\n");
    let parsed = parse_with_stdlib(&source, ModulePath::single("prose_idiom"));
    if !parsed.errors.is_empty() {
        let messages: Vec<&str> = parsed.errors.iter().map(|e| e.message.as_str()).collect();
        return vec![format!(
            "does not parse as a Reify expression: {}",
            messages.join("; ")
        )];
    }
    compile_with_stdlib(&parsed)
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            discredits_calls(diagnostic)
                && !matches!(diagnostic.code, Some(DiagnosticCode::UnresolvedName))
        })
        .map(rendered_diagnostic)
        .collect()
}

/// The report for `idiom`, an idiom of the chunk named `stem`; `None` when it
/// compiles clean.
fn idiom_violation(stem: &str, idiom: &ProseIdiom) -> Option<String> {
    let diagnostics = idiom_violations(&idiom.span);
    (!diagnostics.is_empty()).then(|| {
        format!(
            "{}:{} — `{}` is an idiom the compiler does not accept:\n{}\nFIX: correct the idiom \
             to a form the compiler accepts (and its mirror in docs/reify-stdlib-reference.md, if \
             it has one); a placeholder argument list is written `…`, which makes the span a \
             mention rather than an idiom.",
            chunk_label(stem),
            idiom.line,
            idiom.span,
            ModuleCompile::rendered(&diagnostics)
        )
    })
}

/// Everything wrong across `chunks` (`(stem, markdown)`), one report each,
/// sorted and deduped: an idiom the compiler does not accept, and a chunk whose
/// prose cannot be read. Pure — no file I/O.
fn prose_idiom_violations(chunks: &[(&str, &str)]) -> Vec<String> {
    let mut violations: Vec<String> = chunks
        .iter()
        .flat_map(|(stem, markdown)| match prose_idioms(markdown) {
            Err(error) => vec![format!(
                "{}: {error} — so no idiom in its prose is checked. FIX: repair the markup.",
                chunk_label(stem)
            )],
            Ok(idioms) => idioms
                .iter()
                .filter_map(|idiom| idiom_violation(stem, idiom))
                .collect(),
        })
        .collect();
    violations.sort();
    violations.dedup();
    violations
}

// ---------------------------------------------------------------------------
// The real-corpus gate
// ---------------------------------------------------------------------------

/// The chunks, by stem, whose prose idioms are gated.
const IDIOM_GATED_CHUNKS: &[&str] = &["geometry"];

/// The callee of an idiom each chunk's prose must yield. The geometry one sits in
/// the Topology Selectors prose at the END of geometry.md, so reading it proves
/// the extraction survived every fence and note above it.
const SENTINELS: &[(&str, &str)] = &[("geometry", "adjacent_faces")];

/// Every prose idiom in a gated chunk compiles, its metavariables left unbound.
/// Scope and limits: this module's doc.
#[test]
fn every_prose_idiom_in_a_gated_chunk_compiles() {
    let corpus: Vec<(&str, String)> = IDIOM_GATED_CHUNKS
        .iter()
        .map(|stem| (*stem, read_chunk_file(stem)))
        .collect();
    let chunks: Vec<(&str, &str)> = corpus
        .iter()
        .map(|(stem, markdown)| (*stem, markdown.as_str()))
        .collect();

    for (stem, callee) in SENTINELS {
        let (_, markdown) = chunks
            .iter()
            .find(|(gated, _)| gated == stem)
            .unwrap_or_else(|| panic!("sentinel chunk `{stem}` must be in IDIOM_GATED_CHUNKS"));
        let idioms = prose_idioms(markdown)
            .unwrap_or_else(|error| panic!("{}: {error} — repair the markup", chunk_label(stem)));
        assert!(
            idioms
                .iter()
                .any(|idiom| span_callee(&idiom.span) == Some(*callee)),
            "{} has a `{callee}(…)` idiom in its prose, but the scan did not read it — the \
             extraction no longer reaches it (or the idiom was rewritten: re-pick the sentinel)",
            chunk_label(stem)
        );
    }

    report(
        "idioms in the MCP language-reference chunks' prose that the compiler does not accept",
        &prose_idiom_violations(&chunks),
    );
}

// ---------------------------------------------------------------------------
// Hermetic controls — synthetic chunks and spans; no chunk file is read.
// ---------------------------------------------------------------------------

#[test]
fn prose_idioms_are_the_call_shaped_unelided_non_signature_spans_of_unfenced_prose() {
    let markdown = r#"# Demo

Centre it with `translate(cylinder(r, h), 0mm, 0mm, -h/2)`.

| Use | Idiom |
|---|---|
| overlap | `intersects(box(10mm, 10mm, 10mm), bracket)` |

Signatures are not idioms: `fillet(solid, edges, radius)`, `volume(solid) -> Scalar<Volume>`.
Elided calls are mentions: `translate(...)`, `body(m, "n", …)`.
Nor is a span that does not open with a call: `let a = faces(b)`, `2*r < min(a, b)`,
`Selector(Face)`, `Engine::new(x)`.

```reify
// `sphere(1mm)` quoted in a fence
structure def S {
    let b = sphere(1mm)
}
```

<!-- a maintainer note quoting `cone(1mm, 2mm, 3mm)` -->
"#;

    let read: Vec<(usize, String)> = prose_idioms(markdown)
        .expect("the synthetic chunk's prose is readable")
        .into_iter()
        .map(|idiom| (idiom.line, idiom.span))
        .collect();

    assert_eq!(
        read,
        vec![
            (3, "translate(cylinder(r, h), 0mm, 0mm, -h/2)".to_string()),
            (7, "intersects(box(10mm, 10mm, 10mm), bracket)".to_string()),
        ],
        "only the call-shaped, unelided, non-signature spans of unfenced prose are idioms — a \
         table row's included, a fence's or a maintainer note's never"
    );
}

#[test]
fn an_idiom_whose_only_unresolved_names_are_its_metavariables_compiles_clean() {
    for (span, why) in [
        (
            "translate(cylinder(r, h), 0mm, 0mm, -h/2)",
            "a nested call over metavariables",
        ),
        (
            "adjacent_faces(body, single(faces_by_normal(body, up, 1deg)))",
            "a selector chain over metavariables",
        ),
        (
            "flat_map(snaps, |s| [min_clearance(s, a, b)])",
            "a lambda whose body names metavariables",
        ),
        ("distance(a, b) > tol", "a comparison opened by a call"),
        ("snapshot(m, [])", "an empty-list WARNING never counts"),
    ] {
        assert_eq!(
            idiom_violations(span),
            Vec::<String>::new(),
            "`{span}` must compile clean, its unbound metavariables aside: {why}"
        );
    }
}

#[test]
fn a_stale_idiom_is_reported_whichever_layer_rejects_it() {
    for (span, needle, why) in [
        (
            "translate(z=-height/2)",
            "does not parse",
            "translate has no keyword arguments, so the PARSER rejects it",
        ),
        (
            "translate(cylinder(r, h), 0, 0, -h/2)",
            "ArgTypeMismatch",
            "bare zeros are not lengths, so the type check rejects them",
        ),
        (
            "box(1mm, 2mm)",
            "box() expects 3 arguments, got 2",
            "box takes three arguments, and the arity check (which carries no code) says so",
        ),
        (
            "helixx(1mm, 2mm, 3mm)",
            "UnresolvedFunction",
            "a misspelled builtin is only a WARNING, and it still discredits the idiom",
        ),
    ] {
        let violations = idiom_violations(span);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(needle)),
            "`{span}` must be reported naming `{needle}`: {why}; got {violations:#?}"
        );
    }
}

#[test]
fn prose_idiom_violations_names_chunk_line_and_span_and_clears_once_the_idiom_is_repaired() {
    let stale = "Intro.\n\nCentre it with `translate(cylinder(r, h), 0, 0, -h/2)`.\n";

    let violations = prose_idiom_violations(&[("demo", stale)]);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    let location = format!("{}:3", chunk_label("demo"));
    for needle in [location.as_str(), "`translate(cylinder(r, h), 0, 0, -h/2)`"] {
        assert!(
            violations[0].contains(needle),
            "the violation must name `{needle}`, got: {}",
            violations[0]
        );
    }

    let repaired = stale.replace("0, 0,", "0mm, 0mm,");
    assert_eq!(
        prose_idiom_violations(&[("demo", &repaired)]),
        Vec::<String>::new(),
        "the same chunk with its idiom repaired is clean"
    );
}

#[test]
fn a_chunk_whose_prose_cannot_be_read_is_reported_not_skipped() {
    let chunks = [
        ("fenced", "prose\n```reify\nnever closed\n"),
        ("noted", "prose <!-- never closed\n"),
    ];

    let violations = prose_idiom_violations(&chunks);

    assert_eq!(violations.len(), 2, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/fenced.md") && violations[1].contains("chunks/noted.md"),
        "got {violations:#?}"
    );
}
