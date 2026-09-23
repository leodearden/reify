// ---------------------------------------------------------------------------
// Hermetic tests — synthetic markdown; real repo files serve only as
// resolution targets.
// ---------------------------------------------------------------------------

#[test]
fn cited_source_paths_reads_a_slash_bearing_path_with_any_letter_led_extension() {
    let md = "Rule: `docs/prds/v0_6/doc-chunk-truth-enforcement.md`; runner: scripts/gui-test.sh.\n\
              Fixture crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri and\n\
              `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve`.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            (
                "docs/prds/v0_6/doc-chunk-truth-enforcement.md".to_string(),
                None
            ),
            ("scripts/gui-test.sh".to_string(), None),
            (
                "crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri".to_string(),
                None
            ),
            (
                "geometry_chunk_smoke.rs".to_string(),
                Some("cited_test_paths_in_the_chunk_resolve".to_string())
            ),
        ],
        "any `/`-bearing path whose last segment has a letter-led extension is a cite; a bare \
         basename still needs `.rs`/`.ri` AND a `::fn` half"
    );
}

#[test]
fn cited_source_paths_ignores_words_that_only_look_path_shaped() {
    let md = "Save it as `my_bracket.ri`; designs live under examples/, and/or the tolerancing/\n\
              subdir. OCCT's `BRepExtrema_DistShapeShape::InnerSolution()` decides it, and a\n\
              length divided as width/2.0 stays a length.\n";

    assert_eq!(
        cited_source_paths(md),
        Vec::<(String, Option<String>)>::new(),
        "a bare basename, a directory word, an extension-less slash token, a C++ cite and a \
         digit-led `.0` are prose, not file cites"
    );
}

#[test]
fn cited_source_paths_keeps_a_nested_examples_segment_whole_and_dedupes_in_document_order() {
    let md = "Worked example: `examples/tolerancing/gdt_zones.ri`.\n\
              See `docs/examples/foo.ri`, examples/tolerancing/gdt_zones.ri and examples/half_space.ri.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            ("examples/tolerancing/gdt_zones.ri".to_string(), None),
            ("docs/examples/foo.ri".to_string(), None),
            ("examples/half_space.ri".to_string(), None),
        ],
        "a nested `examples/` segment is part of its whole path, a repeated cite is listed \
         once, and the period ending a sentence is not part of the path"
    );
}

#[test]
fn audit_reports_every_dangling_cite_and_undeclared_fn_without_panicking() {
    let md = "See `docs/prds/no_such_prd.md` and\n\
              crates/reify-compiler/tests/no_such_harness.rs::anything\n\
              crates/reify-compiler/tests/harness_doc_chunks.rs::no_such_test_fn\n";

    let audit = audit_cited_paths("chunks/synthetic.md", md);

    assert_eq!(audit.violations.len(), 3, "got {:#?}", audit.violations);
    let expected_cites = [
        "docs/prds/no_such_prd.md",
        "crates/reify-compiler/tests/no_such_harness.rs",
        "harness_doc_chunks.rs::no_such_test_fn",
    ];
    for (violation, cite) in audit.violations.iter().zip(expected_cites) {
        assert!(
            violation.contains("chunks/synthetic.md") && violation.contains(cite),
            "each violation must name the chunk and the cite `{cite}`, got: {violation}"
        );
    }
}

#[test]
fn audit_passes_real_cites_and_buckets_only_rs_and_ri_files() {
    let md = "crates/reify-compiler/tests/harness_doc_chunks.rs\n\
              `examples/half_space.ri`\n\
              `docs/prds/v0_6/doc-chunk-truth-enforcement.md`\n\
              crates/reify-eval/src/geometry_ops.rs::expected_arity\n";

    let audit = audit_cited_paths("chunks/synthetic.md", md);

    assert_eq!(audit.violations, Vec::<String>::new());
    assert_eq!(audit.cites.len(), 4, "got {:#?}", audit.cites);
    assert_eq!(audit.fn_cites, 1);
    assert_eq!(
        (audit.rs_files.len(), audit.ri_files.len()),
        (2, 1),
        "a `.md` cite is counted as a cite but in NEITHER file bucket, so the chunk-local \
         `.rs`/`.ri` floors stay exact"
    );
}
