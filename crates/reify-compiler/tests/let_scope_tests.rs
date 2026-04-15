//! Tests for let-binding scope resolution, especially geometry lets.

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind, RealizationDecl,
                     TopologyTemplate};
use reify_types::Severity;

// ─── Source-string constants (shared between existing and op-level tests) ─────

const SRC_DIFFERENCE_LET_BOUND: &str = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result = difference(body, hole)
}"#;

const SRC_NESTED_BOOLEAN_OPS: &str = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let a = cylinder(r, h)
    let b = cylinder(r2, h)
    let combined = difference(a, b)
    let c = sphere(r)
    let result = union(combined, c)
}"#;

const SRC_MIXED_LET_AND_INLINE: &str = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let result = difference(body, cylinder(r2, h))
}"#;

const SRC_UNION_ALL_LET_BOUND: &str = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    param w: Scalar = 8mm
    param d: Scalar = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = union_all(a, b, c)
}"#;

// ─── Op-sequence assertion helpers ────────────────────────────────────────────

/// Expected geometry op variant for `assert_op_sequence`.
#[derive(Debug)]
enum ExpectedOp {
    Cylinder,
    Sphere,
    Box_,
    BoolDiff(usize, usize),
    BoolUnion(usize, usize),
}

fn op_matches(actual: &CompiledGeometryOp, expected: &ExpectedOp) -> bool {
    match (actual, expected) {
        (CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }, ExpectedOp::Cylinder) => true,
        (CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }, ExpectedOp::Sphere) => true,
        (CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }, ExpectedOp::Box_) => true,
        (
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(l),
                right: GeomRef::Step(r),
            },
            ExpectedOp::BoolDiff(el, er),
        ) => l == el && r == er,
        (
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(l),
                right: GeomRef::Step(r),
            },
            ExpectedOp::BoolUnion(el, er),
        ) => l == el && r == er,
        _ => false,
    }
}

/// Assert that `ops` matches `expected` element-by-element, providing clear
/// failure messages that identify which position mismatched.
fn assert_op_sequence(ops: &[CompiledGeometryOp], expected: &[ExpectedOp]) {
    assert_eq!(
        ops.len(),
        expected.len(),
        "expected {} ops, got {}",
        expected.len(),
        ops.len()
    );
    for (i, (actual, exp)) in ops.iter().zip(expected.iter()).enumerate() {
        assert!(
            op_matches(actual, exp),
            "ops[{}]: expected {:?}, got {:?}",
            i,
            exp,
            actual
        );
    }
}

/// Find a realization by its position in the ordered list of geometry-let
/// names. `names` should list every geometry let in source order; `target` is
/// the name to look up.  This is more self-documenting than a raw index and is
/// resilient to minor reordering when `names` is updated alongside the source.
fn realization_named<'a>(
    template: &'a TopologyTemplate,
    names: &[&str],
    target: &str,
) -> &'a RealizationDecl {
    assert_eq!(
        names.len(),
        template.realizations.len(),
        "names count ({}) does not match realizations count ({})",
        names.len(),
        template.realizations.len()
    );
    let idx = names
        .iter()
        .position(|&n| n == target)
        .unwrap_or_else(|| panic!("geometry let '{}' not found in names list {:?}", target, names));
    assert!(
        idx < template.realizations.len(),
        "realization index {} for '{}' is out of bounds (len={})",
        idx,
        target,
        template.realizations.len()
    );
    &template.realizations[idx]
}

// ─── compile helpers ──────────────────────────────────────────────────────────

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_let_scope"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );
    compiled
}

/// Helper: parse + compile source, return compiled output (may have errors).
fn compile_with_diagnostics(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_let_scope"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ─── step-1: geometry let should be in scope for subsequent let ───

#[test]
fn geometry_let_in_scope_for_subsequent_let() {
    // The second geometry let `pattern` references `hole` (also a geometry let).
    // This should compile without errors — `hole` must be in scope.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let hole = cylinder(r, h)
    let pattern = circular_pattern(hole, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "geometry let 'hole' should be in scope for subsequent let 'pattern', but got errors: {:?}",
        errors
    );
}

// ─── task-1609 step-1: difference with let-bound args ───

#[test]
fn difference_with_let_bound_args() {
    // Both args to difference() are let-bound geometry variables (Idents).
    // The compiler must resolve these to their initializer expressions.
    let compiled = compile_no_errors(SRC_DIFFERENCE_LET_BOUND);
    let template = &compiled.templates[0];
    // body, hole, result → 3 realizations
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (body, hole, result), got {}",
        template.realizations.len()
    );
}

// ─── task-1609 step-2: union with let-bound args ───

#[test]
fn union_with_let_bound_args() {
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = union(a, b)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (a, b, c), got {}",
        template.realizations.len()
    );
}

// ─── task-1609 step-3: intersection with let-bound args ───

#[test]
fn intersection_with_let_bound_args() {
    let source = r#"structure S {
    param w: Scalar = 10mm
    param h: Scalar = 10mm
    param d: Scalar = 10mm
    param r: Scalar = 7mm
    let a = box(w, h, d)
    let b = sphere(r)
    let c = intersection(a, b)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (a, b, c), got {}",
        template.realizations.len()
    );
}

// ─── task-1609 step-4: union_all with let-bound args ───

#[test]
fn union_all_with_let_bound_args() {
    let compiled = compile_no_errors(SRC_UNION_ALL_LET_BOUND);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        4,
        "expected 4 realizations (a, b, c, d_geom), got {}",
        template.realizations.len()
    );
}

// ─── task-1609 step-5: nested boolean ops with let args ───

#[test]
fn nested_boolean_ops_with_let_args() {
    // combined is a boolean op result used as input to another boolean op via let.
    let compiled = compile_no_errors(SRC_NESTED_BOOLEAN_OPS);
    let template = &compiled.templates[0];
    // a, b, combined, c, result → 5 realizations
    assert_eq!(
        template.realizations.len(),
        5,
        "expected 5 realizations (a, b, combined, c, result), got {}",
        template.realizations.len()
    );
}

// ─── task-1609 step-10: non-geometry let in boolean op errors ───

#[test]
fn non_geometry_let_in_boolean_op_errors() {
    // A non-geometry let (scalar) used as a boolean op argument should error.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let x = 5
    let b = cylinder(r, h)
    let c = difference(x, b)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic when non-geometry let used in boolean op"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("argument 1 must be a geometry expression")),
        "expected 'argument 1 must be a geometry expression' error, got: {:?}",
        errors
    );
}

// ─── amend: intersection_all with let-bound args ───

#[test]
fn intersection_all_with_let_bound_args() {
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    param w: Scalar = 8mm
    param d: Scalar = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = intersection_all(a, b, c)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        4,
        "expected 4 realizations (a, b, c, d_geom), got {}",
        template.realizations.len()
    );
}

// ─── amend: mixed let-bound and inline args in boolean op ───

#[test]
fn mixed_let_and_inline_in_boolean_op() {
    // One arg is a let-bound Ident, the other is an inline geometry call.
    let compiled = compile_no_errors(SRC_MIXED_LET_AND_INLINE);
    let template = &compiled.templates[0];
    // body, result → 2 realizations
    assert_eq!(
        template.realizations.len(),
        2,
        "expected 2 realizations (body, result), got {}",
        template.realizations.len()
    );
}

// ─── amend: cyclic geometry let references should error ───

#[test]
fn cyclic_geometry_let_references_error() {
    // Mutually-recursive geometry lets should produce a cycle error, not stack overflow.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let a = difference(b, cylinder(r, h))
    let b = difference(a, cylinder(r, h))
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostics for cyclic geometry lets"
    );
    assert!(
        errors.iter().any(|d| d.message.contains("cyclic")),
        "expected cyclic reference error, got: {:?}",
        errors
    );
}

// ─── task-1709 step-1: difference op-level assertions ───

#[test]
fn difference_ops_verify_boolean_variant_and_step_refs() {
    // Verifies the operations Vec of the `result` realization.
    // Source shared with difference_with_let_bound_args.
    let compiled = compile_no_errors(SRC_DIFFERENCE_LET_BOUND);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Cylinder,
            ExpectedOp::BoolDiff(0, 1),
        ],
    );
}

// ─── task-1709 step-2: nested boolean ops step-index assertions ───

#[test]
fn nested_boolean_ops_verify_step_indices() {
    // combined=difference(a,b), result=union(combined,c).
    // result inlines: [Cylinder(a), Cylinder(b), Diff(0,1), Sphere(c), Union(2,3)].
    // Source shared with nested_boolean_ops_with_let_args.
    let compiled = compile_no_errors(SRC_NESTED_BOOLEAN_OPS);
    let template = &compiled.templates[0];
    let realization =
        realization_named(template, &["a", "b", "combined", "c", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Cylinder,
            ExpectedOp::BoolDiff(0, 1),
            ExpectedOp::Sphere,
            ExpectedOp::BoolUnion(2, 3),
        ],
    );
}

// ─── task-1709 step-3: mixed let-bound + inline op assertions ───

#[test]
fn mixed_let_and_inline_ops_verify_step_refs() {
    // body is let-bound; right arg is inline cylinder(r2,h).
    // Source shared with mixed_let_and_inline_in_boolean_op.
    let compiled = compile_no_errors(SRC_MIXED_LET_AND_INLINE);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Cylinder,
            ExpectedOp::BoolDiff(0, 1),
        ],
    );
}

// ─── task-1709 step-4: union_all left-fold structure assertions ───

#[test]
fn union_all_ops_verify_left_fold_structure() {
    // d_geom = union_all(a, b, c) — left-fold of 3 args.
    // Expected ops: [Cylinder, Sphere, Union(0,1), Box, Union(2,3)].
    // Source shared with union_all_with_let_bound_args.
    let compiled = compile_no_errors(SRC_UNION_ALL_LET_BOUND);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["a", "b", "c", "d_geom"], "d_geom");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Sphere,
            ExpectedOp::BoolUnion(0, 1),
            ExpectedOp::Box_,
            ExpectedOp::BoolUnion(2, 3),
        ],
    );
}

// ─── task-1709 amend: shared let-bound operand step indices ───

#[test]
fn shared_let_operand_step_indices_correct() {
    // `body` (a let-bound cylinder) is used as the left operand of two different
    // boolean ops: `result1 = difference(body, hole)` and `result2 = union(body, addon)`.
    // Each realization inlines `body` independently, so step indices in both
    // realizations start from 0 — verifying that the compiler does not emit
    // a shared GeomRef::Step across realizations.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result1 = difference(body, hole)
    let addon = sphere(r)
    let result2 = union(body, addon)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // 5 geometry lets → 5 realizations
    assert_eq!(
        template.realizations.len(),
        5,
        "expected 5 realizations (body, hole, result1, addon, result2)"
    );

    let names = ["body", "hole", "result1", "addon", "result2"];

    // result1: difference(body, hole) → [Cylinder, Cylinder, Diff(0,1)]
    let r1 = realization_named(template, &names, "result1");
    assert_op_sequence(
        &r1.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Cylinder,
            ExpectedOp::BoolDiff(0, 1),
        ],
    );

    // result2: union(body, addon) → [Cylinder, Sphere, Union(0,1)]
    // Step(0) here refers to body inlined fresh into this realization,
    // NOT to any step from result1's ops.
    let r2 = realization_named(template, &names, "result2");
    assert_op_sequence(
        &r2.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Sphere,
            ExpectedOp::BoolUnion(0, 1),
        ],
    );
}

// ─── task-1712 step-1: realization_named panics on names/realizations count mismatch ───

#[test]
#[should_panic(expected = "names count")]
fn realization_named_panics_on_count_mismatch() {
    // SRC_DIFFERENCE_LET_BOUND produces 3 realizations: body, hole, result.
    // Passing only &["body"] (1 element) with target "body" would silently
    // succeed without the guard: idx=0 is in bounds, returning realizations[0]
    // even though the mapping is wrong. The names-count guard must catch this.
    let compiled = compile_no_errors(SRC_DIFFERENCE_LET_BOUND);
    let template = &compiled.templates[0];
    // Deliberate mismatch: 1 name vs 3 realizations
    let _ = realization_named(template, &["body"], "body");
}

// ─── step-4: geometry let still produces realization ───

#[test]
fn geometry_let_still_produces_realization() {
    // After the scope registration fix, geometry lets must still compile to
    // RealizationDecl entries (not value cells).
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let c = cylinder(r, h)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for cylinder call, got {}",
        template.realizations.len()
    );
    assert!(
        matches!(
            &template.realizations[0].operations[0],
            CompiledGeometryOp::Primitive { .. }
        ),
        "expected Primitive geometry op, got {:?}",
        template.realizations[0].operations[0]
    );
}

// ─── step-5: non-geometry let-to-let reference still works ───

#[test]
fn non_geometry_let_to_let_reference_still_works() {
    // Non-geometry lets referencing other non-geometry lets should still work.
    let source = r#"structure S {
    let x = 5
    let y = x + 1
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // Both should be value cells, not realizations
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "should have 'x' value cell"
    );
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "y"),
        "should have 'y' value cell"
    );
}

// ─── step-6: multiple geometry lets all produce realizations ───

#[test]
fn multiple_geometry_lets_all_produce_realizations() {
    // Multiple chained geometry lets should all produce realizations and no errors.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let base = cylinder(r, h)
    let pattern = circular_pattern(base, 0, 0, 0, 0, 0, 1, 6, 360)
    let mirrored = mirror(base, 0, 0, 0, 0, 1, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations, got {}",
        template.realizations.len()
    );
}

// ─── step-7: geometry let does not produce a value cell ───

#[test]
fn geometry_let_not_a_value_cell() {
    // Geometry lets should produce realizations, NOT value cells.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    let hole = cylinder(r, h)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // 'hole' should NOT appear as a value cell
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "hole"),
        "geometry let 'hole' should NOT be a value cell, but found one"
    );
    // It should be a realization
    assert_eq!(
        template.realizations.len(),
        1,
        "geometry let 'hole' should produce exactly 1 realization"
    );
}
