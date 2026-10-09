//! Every SIGNATURE written in a chunk's unfenced prose must be exercised, at its
//! documented arity, by a call in a signature fixture — one of
//! `signature_fixtures.rs`'s, each held there to compiling clean.
//!
//! A span is a signature only when it is signature-shaped as a whole — the rule
//! is `doc_forms.rs`'s — and the prose read is `chunk_prose.rs`'s unfenced model,
//! so fence bodies (the fence gate's jurisdiction) and maintainer notes are never
//! scanned. A span shaped like a signature that is not one — a prose mention, a
//! trap example — is excused by an audited [`ProseMention`], and an excuse that
//! matches nothing is reported in its turn.
//!
//! # What is NOT established
//!
//! Arity is truly checked only where the compile layer checks it: geometry-op
//! arms, `some` and stdlib-`.ri` functions reject a wrong arity; list and field
//! helpers only WARN at an unrecognised argument shape; measurement, oracle and
//! kinematic queries, topology selectors, trailing field-op arity, `single` and
//! `mechanism` are SILENT. For those the pairing is a doc↔fixture consistency
//! pin, which becomes a real arity pin as #7343 and the builtin-signature-registry
//! work land. Argument type and order are never checked. Lambda-parameter spans
//! are out of scope; ```` ```reify-schematic ```` listings are the fenced twin
//! `schematic_listing_gate.rs`'s.

use std::collections::BTreeSet;

use crate::chunk_io::{all_chunks, chunk_label, report};
use crate::doc_forms::{Arity, DocForm, DocumentedForm, call_forms, documented_unfenced_forms};
use crate::signature_fixtures::{SIGNATURE_FIXTURES, UNFENCED_SIGNATURES_FIXTURE, read_fixture};

/// A span in `chunk`'s prose that is signature-SHAPED but is not a signature,
/// excused from the gate for the recorded reason.
pub(crate) struct ProseMention {
    pub(crate) chunk: &'static str,
    pub(crate) span: &'static str,
    pub(crate) why: &'static str,
}

impl ProseMention {
    fn excuses(&self, stem: &str, span: &str) -> bool {
        self.chunk == stem && self.span == span
    }
}

/// The signature-shaped spans in the live chunks that are not signatures —
/// each triaged by hand, and each reported STALE the moment it excuses nothing.
const NOT_SIGNATURES: &[ProseMention] = &[
    ProseMention {
        chunk: "enums",
        span: "f(x)",
        why: "the map_or lambda applied to the payload — a metavariable in the combinator \
              table, not a builtin",
    },
    ProseMention {
        chunk: "topology",
        span: "single()",
        why: "names the function in prose — `single` takes one selector argument",
    },
    ProseMention {
        chunk: "geometry",
        span: "min_clearance(a, b)",
        why: "trap 3 documents this 2-arg form as UNSUPPORTED",
    },
];

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
            .any(|mention| mention.excuses(stem, span))
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
                     {UNFENCED_SIGNATURES_FIXTURE} (it must compile clean); if the compiler \
                     rejects it, correct the chunk; if the span is not a signature (a prose \
                     mention or a trap example), add a ProseMention with its reason to \
                     NOT_SIGNATURES.",
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
            !read.iter().any(|(stem, forms)| match forms {
                // Reported as unreadable; whether it still excuses anything is unknowable.
                Err(_) => *stem == mention.chunk,
                Ok(forms) => forms
                    .iter()
                    .any(|documented| mention.excuses(stem, &documented.span)),
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

// ---------------------------------------------------------------------------
// The real-corpus gate
// ---------------------------------------------------------------------------

/// Anti-vacuity floor: the distinct documented forms the gate reads across the
/// corpus, excused spans left out. EXACT live value — re-measure it by the
/// protocol stated once next to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const MINIMUM_UNFENCED_FORMS: usize = 119;

/// One form per chunk family that prose extraction must reach. The geometry
/// and topology ones each sit after every fence and maintainer note in their
/// chunk — trap 5's `min_clearance(s, id, id)` closes geometry.md, and
/// `faces_by_normal` is a row of topology.md's selector table — so reading them
/// proves the extraction survived everything above them.
const SENTINELS: &[(&str, &str, Arity)] = &[
    ("collections", "generate", Arity::Exact(2)),
    ("enums", "unwrap_or", Arity::Exact(2)),
    ("fields", "constant_field", Arity::Exact(1)),
    ("stdlib", "rotate", Arity::Exact(5)),
    ("geometry", "min_clearance", Arity::Exact(3)),
    ("measurement", "max_deviation", Arity::Exact(2)),
    ("topology", "faces_by_normal", Arity::Exact(3)),
];

/// Every signature in any chunk's unfenced prose is exercised, at its documented
/// arity, by a call in a compile-verified fixture. Scope and limits: this
/// module's doc.
#[test]
fn every_unfenced_signature_is_exercised_by_a_compiling_fixture() {
    let corpus = all_chunks("the unfenced-signature gate");
    let chunks: Vec<(&str, &str)> = corpus
        .iter()
        .map(|(stem, markdown)| (stem.as_str(), markdown.as_str()))
        .collect();
    let calls: Vec<(String, usize)> = SIGNATURE_FIXTURES
        .iter()
        .flat_map(|path| call_forms(&read_fixture(path), path))
        .collect();

    let documented: Vec<(&str, DocumentedForm)> = chunks
        .iter()
        .flat_map(|(stem, markdown)| {
            documented_unfenced_forms(markdown)
                .unwrap_or_default()
                .into_iter()
                .map(move |documented| (*stem, documented))
        })
        .collect();
    let distinct: BTreeSet<&DocForm> = documented
        .iter()
        .filter(|(stem, documented)| {
            !NOT_SIGNATURES
                .iter()
                .any(|mention| mention.excuses(stem, &documented.span))
        })
        .map(|(_, documented)| &documented.form)
        .collect();
    assert!(
        distinct.len() >= MINIMUM_UNFENCED_FORMS,
        "the prose scan read only {} distinct documented form(s) across the chunks, expected at \
         least {MINIMUM_UNFENCED_FORMS} — either the extraction regressed and the verdict below \
         passes vacuously, or signatures were removed and MINIMUM_UNFENCED_FORMS must come down \
         in the same diff",
        distinct.len()
    );
    for (stem, name, arity) in SENTINELS {
        assert!(
            documented.iter().any(|(documented_stem, documented)| {
                documented_stem == stem
                    && documented.form.name == *name
                    && documented.form.arity == *arity
            }),
            "{} documents {name}/{arity:?} in its prose, but the scan did not read it — the \
             extraction no longer reaches that chunk (or the signature was rewritten: re-pick \
             the sentinel)",
            chunk_label(stem)
        );
    }

    report(
        "signatures in the MCP language-reference chunks' prose that no compile-verified fixture \
         exercises at their documented arity",
        &unfenced_signature_violations(&chunks, &calls, NOT_SIGNATURES),
    );
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
    let demo_line = format!("{}:1", chunk_label("demo"));
    for needle in [demo_line.as_str(), "some(v, w)", "some/Exact(2)"] {
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
