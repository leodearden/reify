//! Every concrete Reify IDIOM written in a gated chunk's unfenced prose must
//! compile, its free identifiers left unbound as metavariables.

use crate::chunk_io::chunk_label;

/// One prose idiom: a call-shaped, unelided, non-signature code span.
#[derive(Debug, PartialEq)]
struct ProseIdiom {
    /// 1-based chunk line of the span's opening backtick run.
    line: usize,
    span: String,
}

fn prose_idioms(_markdown: &str) -> Result<Vec<ProseIdiom>, String> {
    todo!("#8050 read the prose idioms")
}

fn idiom_violations(_span: &str) -> Vec<String> {
    todo!("#8050 compile one idiom")
}

fn prose_idiom_violations(_chunks: &[(&str, &str)]) -> Vec<String> {
    todo!("#8050 report every chunk's stale idioms")
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
        (
            "snapshot(m, [])",
            "an empty-list WARNING never counts",
        ),
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
            "Error",
            "bare zeros are not lengths, so the type check rejects them",
        ),
        ("box(1mm, 2mm)", "Error", "box takes three arguments"),
        (
            "helixx(1mm, 2mm, 3mm)",
            "UnresolvedFunction",
            "a misspelled builtin is only a WARNING, and it still discredits the idiom",
        ),
    ] {
        let violations = idiom_violations(span);
        assert!(
            violations.iter().any(|violation| violation.contains(needle)),
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
    for needle in [
        location.as_str(),
        "`translate(cylinder(r, h), 0, 0, -h/2)`",
    ] {
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
