//! Every SIGNATURE listed in a gated chunk's ```` ```reify-schematic ```` blocks
//! must be exercised, at its documented arity, by a call in a signature fixture —
//! one of `signature_fixtures.rs`'s, each held there to compiling clean. The
//! fenced twin of `unfenced_signature_gate.rs`.
//!
//! A listing is read line by line: `doc_forms::listing_signature_spans` cuts the
//! candidate spans, and `doc_forms::doc_form_of_span` reads each by the same rule
//! the prose gate uses. A signature listing is notation, not prose, so every span
//! in it is meant as a signature: one the rule cannot read is a violation, never
//! skipped. Only fences tagged EXACTLY `reify-schematic` are read — a `reify`
//! fence is the fence gate's to compile, and a `reify-fragment` is syntax, not a
//! listing.
//!
//! # What is NOT established
//!
//! The limits of `unfenced_signature_gate.rs` apply unchanged: arity is a real pin
//! only where the compile layer checks it, and argument type and order are never
//! checked. Only [`LISTING_GATED_CHUNKS`] are read. The other chunks' listings
//! also hold type, trait and grammar schematics, which this reading would report
//! as unreadable; admitting them is #7803's.

use std::collections::BTreeSet;

use crate::chunk_io::{chunk_label, read_chunk_file, report};
use crate::chunk_markdown::parse_fences;
use crate::doc_forms::{Arity, DocForm, call_forms, doc_form_of_span, listing_signature_spans};
use crate::signature_fixtures::{SIGNATURE_FIXTURES, read_fixture};

/// The info string of the fences read as signature listings.
const LISTING_TAG: &str = "reify-schematic";

/// The fixture a listed signature's mirroring call belongs in, repo-relative.
const SCHEMATIC_LISTING_SIGNATURES_FIXTURE: &str =
    "crates/reify-compiler/tests/fixtures/schematic_listing_signatures_smoke.ri";

/// The chunks, by stem, whose ```` ```reify-schematic ```` listings are all
/// signature listings.
const LISTING_GATED_CHUNKS: &[&str] = &["geometry"];

/// One span cut from a listing line.
struct ListedSpan {
    /// 1-based chunk line the span sits on.
    line: usize,
    span: String,
    /// What the span declares; `None` when it is not signature-shaped.
    form: Option<DocForm>,
}

/// Every span in `markdown`'s ```` ```reify-schematic ```` listings, in document
/// order; `Err` when the chunk's fences cannot be parsed.
fn listed_spans(markdown: &str) -> Result<Vec<ListedSpan>, String> {
    let fences = parse_fences(markdown)?;
    Ok(fences
        .iter()
        .filter(|fence| fence.tag.as_deref() == Some(LISTING_TAG))
        .flat_map(|fence| {
            fence
                .body
                .lines()
                .enumerate()
                .flat_map(move |(offset, text)| {
                    listing_signature_spans(text)
                        .into_iter()
                        .map(move |span| ListedSpan {
                            line: fence.open_line + 1 + offset,
                            form: doc_form_of_span(&span),
                            span,
                        })
                })
        })
        .collect())
}

/// What is wrong with `listed`, a span of the chunk named `stem`, against
/// `calls`; `None` when a call exercises it.
fn listing_violation(stem: &str, listed: &ListedSpan, calls: &[(String, usize)]) -> Option<String> {
    let location = format!("{}:{}", chunk_label(stem), listed.line);
    match &listed.form {
        None => Some(format!(
            "{location} — `{}` is an unreadable listing signature: every span in a \
             `{LISTING_TAG}` listing is read as a signature, and this one is not \
             signature-shaped. FIX: write it in the notation doc_forms reads — lowercase \
             snake_case metavariables, `label: metavar` for a named argument, U+2026 `…` (never \
             ASCII `...`) for a variadic tail — or move it out of the listing if it is not a \
             signature.",
            listed.span
        )),
        Some(form) if !form.is_exercised_by(calls) => Some(format!(
            "{location} — `{}` lists {}/{:?}, which no signature fixture calls at that arity. \
             FIX: if the signature is right, add a call at that arity to \
             {SCHEMATIC_LISTING_SIGNATURES_FIXTURE} (it must compile clean); if the compiler \
             rejects it, correct the listing.",
            listed.span, form.name, form.arity
        )),
        Some(_) => None,
    }
}

/// Everything wrong across `chunks` (`(stem, markdown)`), one line each, sorted
/// and deduped: a listed span that is not signature-shaped, a listed signature no
/// call in `calls` exercises at its arity, and a chunk whose fences cannot be
/// parsed. Pure — no file I/O.
fn schematic_listing_violations(chunks: &[(&str, &str)], calls: &[(String, usize)]) -> Vec<String> {
    let mut violations: Vec<String> = chunks
        .iter()
        .flat_map(|(stem, markdown)| match listed_spans(markdown) {
            Err(error) => vec![format!(
                "{}: {error} — so no signature in its listings is checked. FIX: repair the \
                 markup.",
                chunk_label(stem)
            )],
            Ok(spans) => spans
                .iter()
                .filter_map(|listed| listing_violation(stem, listed, calls))
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

/// Anti-vacuity floor: the distinct signature forms the gate reads across the
/// gated listings. EXACT live value — re-measure it by the protocol stated once
/// next to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const MINIMUM_LISTING_FORMS: usize = 1;

/// Forms each listing block must yield, so a block the reading stops reaching is
/// named: the Primitives, 2D-profile / Prelude, GD&T-zone and Free-form listings.
const SENTINELS: &[(&str, &str, Arity)] = &[
    ("geometry", "half_space", Arity::Exact(6)),
    ("geometry", "polygon", Arity::AtLeast(6)),
    ("geometry", "zone_annulus", Arity::Exact(4)),
    ("geometry", "nurbs_surface", Arity::Exact(6)),
    ("geometry", "isosurface", Arity::Exact(3)),
];

/// Every signature in a gated chunk's listings is exercised, at its documented
/// arity, by a call in a compile-verified fixture. Scope and limits: this
/// module's doc.
#[test]
fn every_listing_signature_in_a_gated_chunk_is_exercised_by_a_compiling_fixture() {
    let corpus: Vec<(&str, String)> = LISTING_GATED_CHUNKS
        .iter()
        .map(|stem| (*stem, read_chunk_file(stem)))
        .collect();
    let chunks: Vec<(&str, &str)> = corpus
        .iter()
        .map(|(stem, markdown)| (*stem, markdown.as_str()))
        .collect();
    let calls: Vec<(String, usize)> = SIGNATURE_FIXTURES
        .iter()
        .flat_map(|path| call_forms(&read_fixture(path), path))
        .collect();

    let listed: Vec<(&str, DocForm)> = chunks
        .iter()
        .flat_map(|(stem, markdown)| {
            listed_spans(markdown)
                .unwrap_or_default()
                .into_iter()
                .filter_map(move |listed| listed.form.map(|form| (*stem, form)))
        })
        .collect();
    let distinct: BTreeSet<&DocForm> = listed.iter().map(|(_, form)| form).collect();
    assert!(
        distinct.len() >= MINIMUM_LISTING_FORMS,
        "the listing scan read only {} distinct signature form(s) across the gated chunks, \
         expected at least {MINIMUM_LISTING_FORMS} — either the reading regressed and the verdict \
         below passes vacuously, or signatures were removed and MINIMUM_LISTING_FORMS must come \
         down in the same diff",
        distinct.len()
    );
    for (stem, name, arity) in SENTINELS {
        assert!(
            listed.iter().any(|(listed_stem, form)| {
                listed_stem == stem && form.name == *name && form.arity == *arity
            }),
            "{} lists {name}/{arity:?}, but the listing scan did not read it — the reading no \
             longer reaches that listing (or the signature was rewritten: re-pick the sentinel)",
            chunk_label(stem)
        );
    }

    report(
        "signatures in the MCP language-reference chunks' reify-schematic listings that no \
         compile-verified fixture exercises at their documented arity",
        &schematic_listing_violations(&chunks, &calls),
    );
}

// ---------------------------------------------------------------------------
// Hermetic controls — synthetic chunks and calls; no chunk or fixture file is
// read.
// ---------------------------------------------------------------------------

fn calls(forms: &[(&str, usize)]) -> Vec<(String, usize)> {
    forms
        .iter()
        .map(|(name, count)| (name.to_string(), *count))
        .collect()
}

#[test]
fn a_listed_signature_a_fixture_calls_at_its_arity_is_silent() {
    let markdown = "```reify-schematic\n\
                    box(width, depth, height)   -> Solid\n\
                    isosurface(grid, iso: level)\n\
                    ```\n";

    assert_eq!(
        schematic_listing_violations(
            &[("demo", markdown)],
            &calls(&[("box", 3), ("isosurface", 2)])
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_listed_signature_no_call_exercises_is_reported_with_its_location_span_and_form() {
    let markdown = "intro\n\
                    ```reify-schematic\n\
                    sphere(radius)\n\
                    torus(major_radius, minor_radius)   -> Solid\n\
                    ```\n";

    let violations = schematic_listing_violations(
        &[("demo", markdown)],
        &calls(&[("sphere", 1), ("torus", 3)]),
    );

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    let demo_line = format!("{}:4", chunk_label("demo"));
    for needle in [
        demo_line.as_str(),
        "torus(major_radius, minor_radius)",
        "torus/Exact(2)",
        SCHEMATIC_LISTING_SIGNATURES_FIXTURE,
    ] {
        assert!(
            violations[0].contains(needle),
            "the violation must name `{needle}`, got: {}",
            violations[0]
        );
    }
}

#[test]
fn an_ascii_elision_in_a_listing_is_reported_unreadable() {
    let markdown = "```reify-schematic\n\
                    polygon(x1, y1, x2, y2, ...)\n\
                    ```\n";

    let violations = schematic_listing_violations(&[("demo", markdown)], &calls(&[("polygon", 6)]));

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/demo.md:2")
            && violations[0].contains("unreadable")
            && violations[0].contains("U+2026"),
        "a listed span doc_forms cannot read is a violation naming the notation fix, got: {}",
        violations[0]
    );
}

#[test]
fn only_fences_tagged_exactly_reify_schematic_are_read_as_listings() {
    let markdown = "```reify-fragment\n\
                    sphere(radius)\n\
                    ```\n\
                    ```reify\n\
                    structure def S { let t = torus(5mm, 1mm) }\n\
                    ```\n\
                    Prose naming `cone(a, b, c)`.\n\
                    ```reify-schematic\n\
                    wedge(width, depth, height, top_width)\n\
                    ```\n";

    let violations = schematic_listing_violations(&[("demo", markdown)], &[]);

    assert_eq!(
        violations.len(),
        1,
        "only the reify-schematic listing's `wedge` is read, got {violations:#?}"
    );
    assert!(
        violations[0].contains("chunks/demo.md:9") && violations[0].contains("wedge/Exact(4)"),
        "got: {}",
        violations[0]
    );
}

#[test]
fn a_qualified_name_in_a_listing_is_never_read() {
    let markdown = "```reify-schematic\n\
                    Orientation.from_quaternion(w, x, y, z)\n\
                    from_axis_angle(axis, angle)\n\
                    ```\n";

    let violations = schematic_listing_violations(&[("demo", markdown)], &[]);

    assert_eq!(
        violations.len(),
        1,
        "the `.`-qualified name is skipped while its unqualified neighbour is read, got \
         {violations:#?}"
    );
    assert!(
        violations[0].contains("chunks/demo.md:3"),
        "got: {}",
        violations[0]
    );
}

#[test]
fn a_chunk_whose_fences_cannot_be_parsed_is_reported_not_skipped() {
    let violations =
        schematic_listing_violations(&[("demo", "```reify-schematic\nsphere(radius)\n")], &[]);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/demo.md") && violations[0].contains("repair the markup"),
        "got: {}",
        violations[0]
    );
}
