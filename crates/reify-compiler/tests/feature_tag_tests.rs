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

// ─── step-3: multi-op realizations tag each op in order ──────────────────────

/// `fillet(box(10mm,20mm,30mm), 1mm)` compiles to two ops:
///   ops[0]: Primitive (box)
///   ops[1]: Modify (fillet)
/// Each must have a tag with the correct StepKind and a sequential sub_index.
#[test]
fn multi_op_realization_tags_one_per_op_with_sequential_sub_indices() {
    let compiled = compile_no_errors(
        "structure B { let s = fillet(box(10mm, 20mm, 30mm), 1mm) }",
    );
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "B")
        .expect("template B not found");

    let realization = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("s"))
        .expect("realization 's' not found");

    // Parallel-array invariant.
    assert_eq!(
        realization.feature_tags.len(),
        realization.operations.len(),
        "feature_tags.len() must equal operations.len()"
    );

    assert_eq!(
        realization.feature_tags.len(),
        2,
        "fillet(box(...)) realization must have exactly 2 feature tags, got {}",
        realization.feature_tags.len()
    );

    // ops[0]: box → Primitive
    assert_eq!(
        realization.feature_tags[0].step_kind,
        StepKind::Primitive,
        "ops[0] (box) must be StepKind::Primitive"
    );
    assert_eq!(
        realization.feature_tags[0].sub_index,
        0,
        "ops[0] must have sub_index == 0"
    );

    // ops[1]: fillet → Modify
    assert_eq!(
        realization.feature_tags[1].step_kind,
        StepKind::Modify,
        "ops[1] (fillet) must be StepKind::Modify"
    );
    assert_eq!(
        realization.feature_tags[1].sub_index,
        1,
        "ops[1] must have sub_index == 1"
    );
}

/// `union(box(10mm,20mm,30mm), sphere(5mm))` compiles to three ops:
///   ops[0]: Primitive (box)
///   ops[1]: Primitive (sphere)
///   ops[2]: Boolean (union)
/// Each must carry the correct StepKind.
#[test]
fn boolean_realization_tags_classify_op_kinds_correctly() {
    let compiled = compile_no_errors(
        "structure C { let s = union(box(10mm, 20mm, 30mm), sphere(5mm)) }",
    );
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "C")
        .expect("template C not found");

    let realization = template
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("s"))
        .expect("realization 's' not found");

    // Parallel-array invariant.
    assert_eq!(
        realization.feature_tags.len(),
        realization.operations.len(),
        "feature_tags.len() must equal operations.len()"
    );

    assert_eq!(
        realization.feature_tags.len(),
        3,
        "union(box,sphere) realization must have exactly 3 feature tags, got {}",
        realization.feature_tags.len()
    );

    assert_eq!(
        realization.feature_tags[0].step_kind,
        StepKind::Primitive,
        "ops[0] (box) must be StepKind::Primitive"
    );
    assert_eq!(realization.feature_tags[0].sub_index, 0);

    assert_eq!(
        realization.feature_tags[1].step_kind,
        StepKind::Primitive,
        "ops[1] (sphere) must be StepKind::Primitive"
    );
    assert_eq!(realization.feature_tags[1].sub_index, 1);

    assert_eq!(
        realization.feature_tags[2].step_kind,
        StepKind::Boolean,
        "ops[2] (union) must be StepKind::Boolean"
    );
    assert_eq!(realization.feature_tags[2].sub_index, 2);
}
