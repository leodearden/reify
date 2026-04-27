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
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{ExportFormat, ModulePath, Severity, StepKind};

// ─── helpers ──────────────────────────────────────────────────────────────────

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
    assert_eq!(
        realization.feature_tags[0].step_kind,
        StepKind::Primitive,
    );
    assert_eq!(realization.feature_tags[0].sub_index, 0);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(
        Box::new(checker),
        Some(Box::new(OcctKernelHandle::spawn())),
    );

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
