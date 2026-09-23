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
