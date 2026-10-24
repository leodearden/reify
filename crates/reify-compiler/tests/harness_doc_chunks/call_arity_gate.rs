//! Every call form any chunk writes as copyable code passes the compiler's
//! arity gate.

use std::collections::BTreeSet;

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

use crate::arity_gate_probe::{ArityRejection, arity_rejections};
use crate::chunk_io::{all_chunks, chunk_label, report};
use crate::written_calls::WrittenCall;

/// One chunk's written call forms, or why its markup could not be read.
struct ChunkCalls<'a> {
    stem: &'a str,
    calls: Result<Vec<WrittenCall>, String>,
}

/// Every chunk of `chunks` (`(stem, markdown)`) read for its written calls.
fn read_chunks<'a>(_chunks: &[(&'a str, &str)]) -> Vec<ChunkCalls<'a>> {
    Vec::new()
}

/// The distinct `(name, arity)` forms written across `read`.
fn written_forms(_read: &[ChunkCalls]) -> BTreeSet<(String, usize)> {
    BTreeSet::new()
}

/// Every violation in `read`, given the forms the compiler `rejections` name.
fn violations(_read: &[ChunkCalls], _rejections: &[ArityRejection]) -> Vec<String> {
    Vec::new()
}

/// Everything wrong across `chunks` (`(stem, markdown)`), one line each, sorted
/// and deduped. Pure — no file I/O.
pub(crate) fn call_arity_violations(chunks: &[(&str, &str)]) -> Vec<String> {
    let read = read_chunks(chunks);
    let forms: Vec<(String, usize)> = written_forms(&read).into_iter().collect();
    violations(&read, &arity_rejections(&forms))
}

/// The violations of the one chunk `demo` holding `markdown`.
fn demo_violations(markdown: &str) -> Vec<String> {
    call_arity_violations(&[("demo", markdown)])
}

/// The functions.md-shaped `## Overloading` listing #6890 shipped, its
/// illustrative overload pair named `name`. The 3-parameter head is on line 6.
fn overloading_listing(name: &str) -> String {
    format!(
        "# Functions\n\
         \n\
         ## Overloading\n\
         \n\
         ```reify-schematic\n\
         fn {name}<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G {{ ... }}\n\
         fn {name}<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G {{ ... }}\n\
         ```\n"
    )
}

#[test]
fn the_6890_overload_listing_on_a_builtin_name_is_reported_at_its_three_parameter_head() {
    let violations = demo_violations(&overloading_listing("rotate"));
    assert_eq!(
        violations.len(),
        1,
        "the listing writes rotate/3, which the builtin rejects, and rotate/2, which it accepts \
         — exactly one violation; got {violations:#?}"
    );
    for expected in [
        format!("{}:6 ", chunk_label("demo")),
        "rotate/3".to_string(),
        "rotate() expects 2 or 5 arguments, got 3".to_string(),
    ] {
        assert!(
            violations[0].contains(&expected),
            "the violation must name `{expected}` — the location, the form and the compiler's \
             own message; got {}",
            violations[0]
        );
    }

    assert_eq!(
        demo_violations(&overloading_listing("align")),
        Vec::<String>::new(),
        "renamed off the builtin's name (functions.md's fix), the same listing is clean"
    );
}

#[test]
fn a_call_is_read_from_copyable_fences_and_prose_but_never_a_rejection_fence_or_a_note() {
    for (markdown, reported, why) in [
        (
            "```reify-fragment\nlet b = box(w, d)\n```\n",
            true,
            "a ```reify-fragment body is copied as accepted Reify",
        ),
        (
            "```reify-invalid\nlet b = box(w, d)\n```\n",
            false,
            "a ```reify-invalid body documents a form the compiler refuses",
        ),
        (
            "Write `box(w, d)` for a slab.\n",
            true,
            "a prose code span is copied as accepted Reify",
        ),
        (
            "<!-- maintainer: `box(w, d)` -->\n",
            false,
            "a maintainer note is never rendered",
        ),
    ] {
        let violations = demo_violations(markdown);
        assert_eq!(
            violations.len(),
            usize::from(reported),
            "{why}; got {violations:#?}"
        );
        if reported {
            assert!(
                violations[0].contains("box/2"),
                "the violation names the form; got {}",
                violations[0]
            );
        }
    }
}

#[test]
fn every_written_location_of_a_rejected_form_is_reported() {
    let violations =
        demo_violations("```reify-fragment\nlet a = box(w, d)\nlet b = box(x, y)\n```\n");
    assert_eq!(
        violations.len(),
        2,
        "two lines writing box/2 are two violations — every location is named, not one per \
         form; got {violations:#?}"
    );
    for line in [2, 3] {
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with(&format!("{}:{line} ", chunk_label("demo")))),
            "line {line} must be named; got {violations:#?}"
        );
    }
}

#[test]
fn a_correct_form_whose_fillers_draw_a_type_error_is_not_an_arity_violation() {
    let control = compile_source_with_stdlib(
        "module call_arity_gate_control\n\
         \n\
         structure def FillerControl {\n\
         \x20   let v = rotate(1mm, 1mm, 1mm, 1mm, 1mm)\n\
         }\n",
    );
    assert!(
        control
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && !d.message.starts_with("rotate() expects")),
        "control: `1mm` fillers must draw a NON-arity error on rotate/5 (its angle slot expects \
         an Angle), or the case below proves nothing about the narrowed predicate; got {:#?}",
        control.diagnostics
    );

    assert_eq!(
        demo_violations("```reify-fragment\nlet r = rotate(geo, ax, ay, az, angle)\n```\n"),
        Vec::<String>::new(),
        "rotate/5 is an accepted arity: only an argument-count rejection is a violation, never \
         the type error its fillers draw"
    );
}

#[test]
fn an_unreadable_chunk_is_reported_never_skipped() {
    for (markdown, why) in [
        (
            "```reify-fragment\nlet b = box(w, d)\n",
            "an unterminated fence",
        ),
        ("<!-- an unterminated note\n", "an unterminated note"),
    ] {
        let violations = demo_violations(markdown);
        assert_eq!(
            violations.len(),
            1,
            "{why} is one violation; got {violations:#?}"
        );
        assert!(
            violations[0].starts_with(&chunk_label("demo"))
                && violations[0].contains("repair the markup"),
            "{why} names its chunk and the fix; got {}",
            violations[0]
        );
    }
}

/// The exact number of distinct `(name, arity)` forms the live chunks write —
/// the anti-vacuity floor on the scan. Re-measured by the protocol stated once
/// beside `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const MINIMUM_WRITTEN_CALL_FORMS: usize = 166;

/// `(stem, name, documented arity, rejected arity)`: a form the chunk writes,
/// each from a different source kind, whose name the compiler arity-gates — so
/// the live gate is proven to reach every source kind AND to discriminate.
const SENTINELS: &[(&str, &str, usize, usize)] = &[
    ("constraints", "determined", 1, 2),
    ("parameters", "constrained", 1, 2),
    ("purposes", "determined", 1, 2),
    ("stdlib", "rotate", 5, 3),
];

/// THE GATE over the live corpus.
#[test]
fn every_call_form_a_chunk_writes_passes_the_compiler_arity_gate() {
    let chunks = all_chunks("the call-arity gate");
    let chunks: Vec<(&str, &str)> = chunks
        .iter()
        .map(|(stem, markdown)| (stem.as_str(), markdown.as_str()))
        .collect();
    let read = read_chunks(&chunks);
    let forms = written_forms(&read);
    assert!(
        forms.len() >= MINIMUM_WRITTEN_CALL_FORMS,
        "the chunks write only {} distinct call form(s), floor {MINIMUM_WRITTEN_CALL_FORMS} — \
         the call-arity gate would be vacuous",
        forms.len()
    );

    for (stem, name, documented, _) in SENTINELS {
        let wrote = read.iter().any(|chunk| {
            chunk.stem == *stem
                && chunk
                    .calls
                    .iter()
                    .flatten()
                    .any(|call| call.name == *name && call.arity == *documented)
        });
        assert!(
            wrote,
            "sentinel: {} must write {name}/{documented} — re-pick a gated form from the same \
             chunk and source kind if it moved",
            chunk_label(stem)
        );
    }

    let mut probed = forms.clone();
    probed.extend(
        SENTINELS
            .iter()
            .map(|(_, name, _, rejected)| (name.to_string(), *rejected)),
    );
    let probed: Vec<(String, usize)> = probed.into_iter().collect();
    let rejections = arity_rejections(&probed);
    for (_, name, _, rejected) in SENTINELS {
        assert!(
            rejections
                .iter()
                .any(|r| r.name == *name && r.arity == *rejected),
            "sentinel: the compiler must reject {name}/{rejected} on arity, or this gate no \
             longer discriminates on live content; rejections: {rejections:#?}"
        );
    }

    report(
        "every call form a chunk writes passes the compiler's arity gate",
        &violations(&read, &rejections),
    );
}
