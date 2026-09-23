//! Every SIGNATURE written in a chunk's unfenced prose must be exercised, at its
//! documented arity, by a fixture that compiles clean.
//!
//! A span is a signature only when it is signature-shaped as a whole — the rule
//! is `doc_forms.rs`'s — and the prose read is `chunk_prose.rs`'s unfenced model,
//! so fence bodies (the fence gate's jurisdiction) and maintainer notes are never
//! scanned. The mirror set is this gate's own fixture together with
//! `stdlib_geometry_ops_smoke.ri`. A span shaped like a signature that is not one
//! — a prose mention, a trap example — is excused by an audited [`ProseMention`],
//! and an excuse that matches nothing is reported in its turn.
//!
//! # What is NOT established
//!
//! Arity is truly checked only where the compile layer checks it: geometry-op
//! arms, `some` and stdlib-`.ri` functions reject a wrong arity; list and field
//! helpers only WARN at an unrecognised argument shape; measurement, oracle and
//! kinematic queries, topology selectors, trailing field-op arity, `single` and
//! `mechanism` are SILENT (measured 2026-09-23). For those the pairing is a
//! doc↔fixture consistency pin that becomes a real arity pin as #7343 and the
//! builtin-signature-registry work land — deliberately not pinned executably,
//! so those merges stay in scope. Argument type and order are never checked.
//! Lambda-parameter spans and ```` ```reify-schematic ```` listings are out of
//! scope.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

use crate::chunk_prose::{code_spans, unfenced_prose};
use crate::doc_forms::{DocForm, doc_form_of_span};
use crate::fence_gate::chunk_label;

/// This gate's own fixture, repo-relative — how every violation names it.
const UNFENCED_FIXTURE: &str = "crates/reify-compiler/tests/fixtures/unfenced_signatures_smoke.ri";

/// A span in `chunk`'s prose that is signature-SHAPED but is not a signature,
/// excused from the gate for the recorded reason.
pub(crate) struct ProseMention {
    pub(crate) chunk: &'static str,
    pub(crate) span: &'static str,
    pub(crate) why: &'static str,
}

/// One signature-shaped span in a chunk's unfenced prose.
pub(crate) struct DocumentedForm {
    pub(crate) form: DocForm,
    /// 1-based line of the span's opening backtick run.
    pub(crate) line: usize,
    pub(crate) span: String,
}

/// Every signature-shaped code span in `markdown`'s unfenced prose, in document
/// order; `Err` when the prose cannot be read.
pub(crate) fn documented_unfenced_forms(markdown: &str) -> Result<Vec<DocumentedForm>, String> {
    Ok(code_spans(&unfenced_prose(markdown)?)
        .into_iter()
        .filter_map(|span| {
            doc_form_of_span(&span.text).map(|form| DocumentedForm {
                form,
                line: span.line,
                span: span.text,
            })
        })
        .collect())
}

/// Everything wrong across `chunks` (`(stem, markdown)`), one line each, sorted
/// and deduped: a documented signature no call in `calls` exercises at its
/// arity, a `not_signatures` excuse matching no span, and a chunk whose prose
/// cannot be read. Pure — no file I/O.
pub(crate) fn unfenced_signature_violations(
    chunks: &[(&str, &str)],
    calls: &[(String, usize)],
    not_signatures: &[ProseMention],
) -> Vec<String> {
    let read: Vec<(&str, Result<Vec<DocumentedForm>, String>)> = chunks
        .iter()
        .map(|(stem, markdown)| (*stem, documented_unfenced_forms(markdown)))
        .collect();
    let excused = |stem: &str, span: &str| {
        not_signatures
            .iter()
            .any(|mention| mention.chunk == stem && mention.span == span)
    };

    let unreadable = read.iter().filter_map(|(stem, forms)| {
        forms.as_ref().err().map(|error| {
            format!(
                "{}: {error} — so no signature in its prose is checked. FIX: repair the markup.",
                chunk_label(stem)
            )
        })
    });
    let unmirrored = read.iter().flat_map(|(stem, forms)| {
        forms
            .iter()
            .flatten()
            .filter(|documented| {
                !excused(stem, &documented.span) && !documented.form.is_exercised_by(calls)
            })
            .map(|documented| {
                format!(
                    "{}:{} — `{}` documents {}/{:?}, which no signature fixture calls at that \
                     arity. FIX: if the signature is right, add a call at that arity to \
                     {UNFENCED_FIXTURE} (it must compile clean); if the compiler rejects it, \
                     correct the chunk; if the span is not a signature (a prose mention or a \
                     trap example), add a ProseMention with its reason to NOT_SIGNATURES.",
                    chunk_label(stem),
                    documented.line,
                    documented.span,
                    documented.form.name,
                    documented.form.arity
                )
            })
    });
    let stale = not_signatures
        .iter()
        .filter(|mention| {
            !read.iter().any(|(stem, forms)| {
                *stem == mention.chunk
                    && forms.as_ref().map_or(true, |forms| {
                        forms
                            .iter()
                            .any(|documented| documented.span == mention.span)
                    })
            })
        })
        .map(|mention| {
            format!(
                "NOT_SIGNATURES entry `{}` for {} is STALE: no signature-shaped span there reads \
                 that any more, so it excuses nothing. It was listed because: {}. FIX: delete the \
                 entry.",
                mention.span,
                chunk_label(mention.chunk),
                mention.why
            )
        });

    let mut violations: Vec<String> = unreadable.chain(unmirrored).chain(stale).collect();
    violations.sort();
    violations.dedup();
    violations
}

/// The diagnostics that make a signature fixture's calls untrustworthy, rendered
/// one per line: any `Severity::Error`, a call to a name nothing resolves, and a
/// builtin called at an argument shape it does not recognise. Any other warning
/// never counts.
pub(crate) fn fixture_compile_violations(source: &str) -> Vec<String> {
    compile_source_with_stdlib(source)
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == Severity::Error
                || matches!(
                    diagnostic.code,
                    Some(
                        DiagnosticCode::UnresolvedFunction
                            | DiagnosticCode::BuiltinArgShapeUnrecognized
                    )
                )
        })
        .map(|diagnostic| {
            let code = diagnostic
                .code
                .as_ref()
                .map(|code| format!(" {code:?}"))
                .unwrap_or_default();
            format!("{:?}{code}: {}", diagnostic.severity, diagnostic.message)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hermetic controls — synthetic chunks and sources; no chunk or fixture file is
// read.
// ---------------------------------------------------------------------------

fn calls(forms: &[(&str, usize)]) -> Vec<(String, usize)> {
    forms
        .iter()
        .map(|(name, count)| (name.to_string(), *count))
        .collect()
}

fn mention(chunk: &'static str, span: &'static str, why: &'static str) -> ProseMention {
    ProseMention { chunk, span, why }
}

#[test]
fn a_prose_signature_no_call_exercises_is_reported_with_its_location_span_and_form() {
    let markdown = "Wrap with `some(v, w)`.\n\
                    Or with `some(v)`.\n\
                    Join with `union_all(a, b, …)`.\n";

    let violations = unfenced_signature_violations(
        &[("demo", markdown)],
        &calls(&[("some", 1), ("union_all", 3)]),
        &[],
    );

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    for needle in [
        "crates/reify-mcp/src/tools/chunks/demo.md:1",
        "some(v, w)",
        "some/Exact(2)",
    ] {
        assert!(
            violations[0].contains(needle),
            "the violation must name `{needle}`, got: {}",
            violations[0]
        );
    }
}

#[test]
fn a_signature_inside_a_fence_or_a_maintainer_note_is_not_prose() {
    let markdown = "```reify-schematic\n\
                    `some(v, w)` is the listed form\n\
                    ```\n\
                    <!-- a note quoting `some(v, w)` -->\n";

    assert_eq!(
        unfenced_signature_violations(&[("demo", markdown)], &calls(&[("some", 1)]), &[]),
        Vec::<String>::new(),
        "fence bodies are the fence gate's, and maintainer notes are not what a reader sees"
    );
}

#[test]
fn a_signature_wrapped_across_two_lines_is_read_and_reported_at_its_opening_line() {
    let markdown = "intro\n\
                    A wrapped `some(v,\n\
                    w)` span.\n";

    let violations =
        unfenced_signature_violations(&[("demo", markdown)], &calls(&[("some", 1)]), &[]);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/demo.md:2") && violations[0].contains("some/Exact(2)"),
        "got: {}",
        violations[0]
    );
}

#[test]
fn a_prose_mention_suppresses_exactly_its_span_in_its_chunk() {
    let chunks = [
        ("enums", "`f(x)` applies the lambda.\n"),
        ("fields", "`f(x)` again.\n"),
    ];

    let violations = unfenced_signature_violations(
        &chunks,
        &[],
        &[mention(
            "enums",
            "f(x)",
            "the lambda applied to the payload",
        )],
    );

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/fields.md:1"),
        "the same span in ANOTHER chunk is still reported, got: {}",
        violations[0]
    );
}

#[test]
fn a_prose_mention_matching_no_span_is_reported_as_stale_quoting_its_reason() {
    let violations = unfenced_signature_violations(
        &[("enums", "No signature here.\n")],
        &[],
        &[mention(
            "enums",
            "f(x)",
            "the lambda applied to the payload",
        )],
    );

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("STALE")
            && violations[0].contains("the lambda applied to the payload"),
        "got: {}",
        violations[0]
    );
}

#[test]
fn a_chunk_whose_prose_cannot_be_read_is_reported_not_skipped() {
    let chunks = [
        ("fenced", "prose\n```reify\nnever closed\n"),
        ("noted", "prose <!-- never closed\n"),
    ];

    let violations = unfenced_signature_violations(&chunks, &[], &[]);

    assert_eq!(violations.len(), 2, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/fenced.md") && violations[1].contains("chunks/noted.md"),
        "got {violations:#?}"
    );
}

#[test]
fn fixture_compile_violations_counts_errors_unresolved_names_and_unrecognised_arg_shapes_only() {
    let reported = [
        (
            "structure def S {\n    let o = some(1mm, 2mm)\n}\n",
            "Error",
        ),
        (
            "structure def S {\n    let x = bogus_fn(1mm)\n}\n",
            "UnresolvedFunction",
        ),
        (
            "structure def S {\n    let xs = generate(3)\n}\n",
            "BuiltinArgShapeUnrecognized",
        ),
    ];
    for (source, class) in reported {
        let violations = fixture_compile_violations(source);
        assert!(
            violations.iter().any(|violation| violation.contains(class)),
            "`{source}` must be reported as {class}, got {violations:#?}"
        );
    }

    assert_eq!(
        fixture_compile_violations("structure def S {\n    let o = some(1mm)\n}\n"),
        Vec::<String>::new(),
        "any other diagnostic — e.g. the missing-`module` warning — never counts"
    );
}
