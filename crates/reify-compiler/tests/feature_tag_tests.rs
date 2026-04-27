//! Tests for feature-tag IR derivation (task 2323).
//!
//! TDD structure:
//!   step-1: single-primitive box realization carries one FeatureTag
//!   step-3: multi-op realization has one tag per op; boolean ops classified
//!   step-9: parallel-array invariant held for all representative inputs

use reify_compiler::compile_with_stdlib;
use reify_types::{FeatureTag, SourceSpan, StepKind};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_feature_tag"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:#?}",
        errors
    );
    compiled
}

// ─── step-1: box realization carries single Primitive feature tag ─────────────

/// A `structure A { let body = box(10mm, 20mm, 30mm) }` compiles to a single
/// realization with exactly one op (box = Primitive). The `feature_tags` field
/// must be present, have length 1 (parallel-array invariant), the tag at index 0
/// must have `step_kind == StepKind::Primitive`, `sub_index == 0`, and
/// `source_span == realization.span`.
#[test]
fn box_realization_carries_single_primitive_feature_tag() {
    let compiled = compile_no_errors(
        "structure A { let body = box(10mm, 20mm, 30mm) }",
    );
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

    // Parallel-array invariant: same length as operations.
    assert_eq!(
        realization.feature_tags.len(),
        realization.operations.len(),
        "feature_tags.len() must equal operations.len()"
    );

    // Single-op realization.
    assert_eq!(
        realization.feature_tags.len(),
        1,
        "box realization must have exactly 1 feature tag"
    );

    let tag: &FeatureTag = &realization.feature_tags[0];
    assert_eq!(
        tag.step_kind,
        StepKind::Primitive,
        "box op must be classified as StepKind::Primitive"
    );
    assert_eq!(tag.sub_index, 0, "first op must have sub_index == 0");
    assert_eq!(
        tag.source_span, realization.span,
        "tag source_span must equal the realization's span"
    );
}
