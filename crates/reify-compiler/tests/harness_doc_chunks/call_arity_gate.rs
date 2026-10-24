//! Every call form any chunk writes as copyable code passes the compiler's
//! arity gate.
//!
//! A reader copies a documented call form — out of prose, a fence or a
//! signature listing — and the compiler resolves its bare name. When a builtin
//! of that name rejects the written arity, the served documentation asserts a
//! call the compiler refuses (task #6890's `rotate(geometry, axis, angle)`).
//! What is read as copyable is `written_calls.rs`'s `COPYABLE_FENCE_TAGS` plus
//! unfenced prose code spans; whether a builtin rejects a form is
//! `arity_gate_probe.rs`'s, asked once for the whole corpus.
//!
//! # What is NOT established
//!
//! - **Argument types and order.** Only the argument COUNT is checked.
//! - **Anything about names with no arity gate.** Topology selectors,
//!   measurement queries and stdlib-`.ri` functions draw no argument-count
//!   message, so they contribute nothing — and nor does a name that resolves to
//!   nothing at all.
//! - **An elided call.** `box(...)` has no arity to check.
//! - **A fence whose tag is not listed** as copyable: it is unread until listed.

use std::collections::BTreeSet;

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

use crate::arity_gate_probe::{ArityRejection, arity_rejections};
use crate::chunk_io::{all_chunks, chunk_label, report};
use crate::written_calls::{WrittenCall, chunk_written_calls};

/// One chunk's written call forms, or why its markup could not be read.
struct ChunkCalls<'a> {
    stem: &'a str,
    calls: Result<Vec<WrittenCall>, String>,
}

/// Every chunk of `chunks` (`(stem, markdown)`) read for its written calls.
fn read_chunks<'a>(chunks: &[(&'a str, &str)]) -> Vec<ChunkCalls<'a>> {
    chunks
        .iter()
        .map(|(stem, markdown)| ChunkCalls {
            stem,
            calls: chunk_written_calls(markdown),
        })
        .collect()
}

/// The distinct `(name, arity)` forms written across `read`.
fn written_forms(read: &[ChunkCalls]) -> BTreeSet<(String, usize)> {
    read.iter()
        .flat_map(|chunk| chunk.calls.iter().flatten())
        .map(|call| (call.name.clone(), call.arity))
        .collect()
}

/// Every violation in `read`, given the forms the compiler `rejections` name:
/// one line per written LOCATION of a rejected form, and one per chunk whose
/// markup cannot be read. Sorted and deduped.
fn violations(read: &[ChunkCalls], rejections: &[ArityRejection]) -> Vec<String> {
    let unreadable = read.iter().filter_map(|chunk| {
        chunk.calls.as_ref().err().map(|error| {
            format!(
                "{}: {error} — so no call form in it is checked. FIX: repair the markup.",
                chunk_label(chunk.stem)
            )
        })
    });
    let rejected = read.iter().flat_map(|chunk| {
        chunk.calls.iter().flatten().flat_map(move |call| {
            rejections
                .iter()
                .filter(|r| r.name == call.name && r.arity == call.arity)
                .map(move |rejection| rejected_call_violation(chunk.stem, call, rejection))
        })
    });

    let mut violations: Vec<String> = unreadable.chain(rejected).collect();
    violations.sort();
    violations.dedup();
    violations
}

/// The violation line for `call`, written in chunk `stem`, whose form the
/// compiler rejects as `rejection` says. A wrapped call's text is shown on one
/// line.
fn rejected_call_violation(stem: &str, call: &WrittenCall, rejection: &ArityRejection) -> String {
    format!(
        "{}:{} — `{}` writes {}/{}, which the compiler's arity gate rejects: {}. FIX: correct \
         the documented call to an arity the compiler accepts (and its mirror in \
         docs/reify-stdlib-reference.md, if it has one); if it illustrates a user-declared \
         function, rename it off the builtin's name (functions.md's `align`, task #6890); a form \
         documented AS rejected belongs in a ```reify-invalid fence.",
        chunk_label(stem),
        call.line,
        call.text.split_whitespace().collect::<Vec<_>>().join(" "),
        call.name,
        call.arity,
        rejection.message
    )
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

/// `(stem, name, documented arity, rejected arity)`: a form the chunk writes
/// whose name the compiler rejects at another arity — so the live gate is
/// proven, on live content, to reach each source kind AND to discriminate,
/// rather than being green because nothing it reads is arity-gated.
///
/// One per source kind, each in a chunk that writes the form in that kind
/// only: a ```` ```reify-fragment ```` (constraints), a
/// ```` ```reify-schematic ```` listing (parameters), a ```` ```reify ```` fence
/// (purposes) and a prose code span (stdlib).
const SENTINELS: &[(&str, &str, usize, usize)] = &[
    ("constraints", "determined", 1, 2),
    ("parameters", "constrained", 1, 2),
    ("purposes", "determined", 1, 2),
    ("stdlib", "rotate", 5, 3),
];

/// THE GATE over the live corpus. The sentinels' rejected forms join the
/// written forms in the SAME probe, so the gate stays one compile.
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
