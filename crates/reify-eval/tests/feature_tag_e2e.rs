//! End-to-end tests for the feature-tag runtime table (task 2323).
//!
//! Tests are gated on `reify_kernel_occt::OCCT_AVAILABLE` (same guard used in
//! `topology_filtered_selectors.rs`) and exercise the full pipeline:
//!   parse → compile_with_stdlib → Engine::build (with real OCCT kernel)
//!   → Engine::feature_tag_table()
//!
//! step-5: Engine::build records at least one FeatureTag for a box realization.
//! step-7: edges_at_height_with_tags returns the same edges as edges_at_height
//!         and records per-edge FeatureTags in the supplied table.

use reify_compiler::compile_with_stdlib;
use reify_eval::topology_selectors;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    ExportFormat, FeatureTag, FeatureTagTable, GeometryOp, ModulePath, Severity, SourceSpan,
    StepKind, Value,
};

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Build a 10×10×10 mm box via a direct OcctKernelHandle and return the
/// (kernel, box_id) pair. Matches the `box_handle(10.0, 10.0, 10.0)` pattern
/// in `topology_filtered_selectors.rs`.
fn box_10mm() -> (OcctKernelHandle, reify_types::GeometryHandleId) {
    let kernel = OcctKernelHandle::spawn();
    let id = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10e-3),
            height: Value::Real(10e-3),
            depth: Value::Real(10e-3),
        })
        .expect("Box creation should succeed")
        .id;
    (kernel, id)
}

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test_feature_tag_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

// ─── step-5: Engine::build records FeatureTag entries for box realization ─────

/// After `Engine::build()` on a single-box realization, the engine's
/// `feature_tag_table()` must be non-empty. Since the box realization has
/// exactly one geometry op (Primitive/Box), the table must have exactly one
/// entry. The compiled IR already establishes that entry's tag is
/// `StepKind::Primitive` with `sub_index == 0` (verified in
/// `feature_tag_tests.rs`); here we confirm the runtime table is populated.
///
/// Fails to compile until step-6 wires `feature_tag_table: FeatureTagTable`
/// onto `Engine` and exposes the `feature_tag_table()` accessor.
#[test]
fn engine_build_records_top_level_feature_tag_for_box_realization() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let compiled = compile_no_errors("structure A { let body = box(10mm, 10mm, 10mm) }");

    // Precondition: compiled IR has exactly 1 op with the expected tag.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "A")
        .expect("template A not found");
    let realization = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("body"))
        .expect("realization 'body' not found");
    assert_eq!(
        realization.feature_tags.len(),
        1,
        "box realization must have exactly 1 feature tag in the compiled IR"
    );
    assert_eq!(realization.feature_tags[0].step_kind, StepKind::Primitive,);
    assert_eq!(realization.feature_tags[0].sub_index, 0);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine =
        reify_eval::Engine::new(Box::new(checker), Some(Box::new(OcctKernelHandle::spawn())));

    let build_result = engine.build(&compiled, ExportFormat::Step);

    // Geometry must have been produced (no errors).
    let geom_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:#?}",
        geom_errors
    );
    assert!(
        build_result.geometry_output.is_some(),
        "expected geometry output (STEP bytes) for a box realization"
    );

    // After build(), the engine's feature_tag_table must have recorded exactly
    // 1 entry: one per executed geometry operation (the single box primitive).
    let table = engine.feature_tag_table();
    assert_eq!(
        table.len(),
        1,
        "feature_tag_table must have exactly 1 entry after building a single-op box realization, got {}",
        table.len()
    );
}

// ─── step-7: edges_at_height_with_tags records per-edge tags ─────────────────

/// `edges_at_height_with_tags` must:
/// (a) return the same `Vec<GeometryHandleId>` as `edges_at_height` for the
///     same input (same filtered edges, same canonical order);
/// (b) record a `FeatureTag` for each filtered edge in the supplied table;
/// (c) each recorded tag's `step_kind` and `source_span` must match the
///     parent tag's;
/// (d) the recorded `sub_index` values must be unique across the filtered edges.
///
/// Will fail to compile until step-8 adds `edges_at_height_with_tags` to
/// `topology_selectors`.
#[test]
fn edges_at_height_with_tags_returns_same_edges_as_baseline_and_records_per_edge_tags() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // Build a 10×10×10 mm box.  The box is centered at the origin by the OCCT
    // kernel, so z extends from −5e-3 to +5e-3.  The top face at z = +5e-3
    // has exactly 4 edges.
    let (mut kernel, box_id) = box_10mm();

    // Synthesise a parent tag that simulates the box-primitive realization.
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(0, 0),
        step_kind: StepKind::Primitive,
        sub_index: 0,
    };

    // --- baseline: edges_at_height (existing, unmodified) ----
    let baseline = topology_selectors::edges_at_height(&mut kernel, box_id, 5e-3, 1e-6)
        .expect("edges_at_height on a valid box should succeed");

    assert_eq!(
        baseline.len(),
        4,
        "baseline edges_at_height(z=+5e-3, tol=1e-6) on a 10x10x10 mm box must return 4 top edges, got {}",
        baseline.len()
    );

    // --- tagged variant: edges_at_height_with_tags (new) ----
    let mut table = FeatureTagTable::default();
    let tagged = topology_selectors::edges_at_height_with_tags(
        &mut kernel,
        &mut table,
        box_id,
        parent_tag,
        5e-3,
        1e-6,
    )
    .expect("edges_at_height_with_tags on a valid box should succeed");

    // (a) Same number of edges returned (same filter predicate applies).
    //
    // Note: we cannot compare the raw GeometryHandleId values directly because
    // `extract_edges` allocates fresh kernel handles on each call; the second
    // invocation (inside `edges_at_height_with_tags`) produces new IDs even for
    // the same parent shape.  Count equality is the correct proxy for "same edges
    // selected by the same predicate."
    assert_eq!(
        tagged.len(),
        baseline.len(),
        "edges_at_height_with_tags must return the same number of edges as edges_at_height"
    );

    // (b) Every filtered edge has a recorded FeatureTag.
    for id in &tagged {
        assert!(
            table.lookup(*id).is_some(),
            "filtered edge {:?} must have a FeatureTag recorded in the table",
            id
        );
    }

    // (c) Each recorded tag's step_kind and source_span match the parent's.
    for id in &tagged {
        let tag = table.lookup(*id).unwrap();
        assert_eq!(
            tag.step_kind, parent_tag.step_kind,
            "recorded tag.step_kind for edge {:?} must match the parent's {:?}",
            id, parent_tag.step_kind
        );
        assert_eq!(
            tag.source_span, parent_tag.source_span,
            "recorded tag.source_span for edge {:?} must match the parent's {:?}",
            id, parent_tag.source_span
        );
    }

    // (d) The recorded sub_indices are unique among the filtered edges.
    let mut sub_indices: Vec<u32> = tagged
        .iter()
        .map(|id| table.lookup(*id).unwrap().sub_index)
        .collect();
    let original_len = sub_indices.len();
    sub_indices.sort_unstable();
    sub_indices.dedup();
    assert_eq!(
        sub_indices.len(),
        original_len,
        "sub_index values must be unique across all filtered edges"
    );
}
