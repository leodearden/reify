//! Tests for let-binding scope resolution, especially geometry lets.

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PatternKind, PrimitiveKind,
    RealizationDecl, SweepKind, TopologyTemplate, TransformKind,
};
use reify_test_support::{compile_source, parse_and_compile};
use reify_core::Severity;
use reify_ir::CompiledExprKind;

// ─── Source-string constants (shared between existing and op-level tests) ─────

const SRC_DIFFERENCE_LET_BOUND: &str = r#"structure S {
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result = difference(body, hole)
}"#;

const SRC_NESTED_BOOLEAN_OPS: &str = r#"structure S {
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let a = cylinder(r, h)
    let b = cylinder(r2, h)
    let combined = difference(a, b)
    let c = sphere(r)
    let result = union(combined, c)
}"#;

const SRC_MIXED_LET_AND_INLINE: &str = r#"structure S {
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = difference(body, cylinder(r2, h))
}"#;

const SRC_UNION_ALL_LET_BOUND: &str = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    param w: Length = 8mm
    param d: Length = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = union_all(a, b, c)
}"#;

const SRC_INTERSECTION_LET_BOUND: &str = r#"structure S {
    param w: Length = 10mm
    param h: Length = 10mm
    param d: Length = 10mm
    param r: Length = 7mm
    let a = box(w, h, d)
    let b = sphere(r)
    let c = intersection(a, b)
}"#;

const SRC_INTERSECTION_ALL_LET_BOUND: &str = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    param w: Length = 8mm
    param d: Length = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = intersection_all(a, b, c)
}"#;

// ─── Op-sequence assertion helpers ────────────────────────────────────────────

/// Target-ref discriminator for `ExpectedOp` variants that carry a geometry
/// target.  `Step(n)` matches a `GeomRef::Step(n)`; `Sub(name)` matches a
/// `GeomRef::Sub(name)` (bare sibling-realization reference introduced by the
/// option-A-general sibling-let pre-check, task #4668).
#[derive(Debug)]
enum Tgt {
    Step(usize),
    Sub(&'static str),
}

/// Expected geometry op variant for `assert_op_sequence`.
#[derive(Debug)]
#[allow(dead_code)] // Curve variant kept for harness completeness; not all ops have test cases
enum ExpectedOp {
    Cylinder,
    Sphere,
    Box_,
    BoolDiff(usize, usize),
    BoolUnion(usize, usize),
    BoolIntersect(usize, usize),
    Transform(TransformKind, Tgt),
    Pattern(PatternKind, Tgt),
    Sweep(SweepKind, Vec<Tgt>),
    Modify(ModifyKind, Tgt),
    Curve(CurveKind),
}

/// Match a `GeomRef` against a `Tgt` discriminator.
fn tgt_matches(actual: &GeomRef, expected: &Tgt) -> bool {
    match (actual, expected) {
        (GeomRef::Step(s), Tgt::Step(es)) => s == es,
        (GeomRef::Sub(name), Tgt::Sub(ename)) => name.as_str() == *ename,
        _ => false,
    }
}

fn op_matches(actual: &CompiledGeometryOp, expected: &ExpectedOp) -> bool {
    match (actual, expected) {
        (
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Cylinder,
                ..
            },
            ExpectedOp::Cylinder,
        ) => true,
        (
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Sphere,
                ..
            },
            ExpectedOp::Sphere,
        ) => true,
        (
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            },
            ExpectedOp::Box_,
        ) => true,
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
        (
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Intersection,
                left: GeomRef::Step(l),
                right: GeomRef::Step(r),
            },
            ExpectedOp::BoolIntersect(el, er),
        ) => l == el && r == er,
        (
            CompiledGeometryOp::Transform { kind, target, .. },
            ExpectedOp::Transform(ek, et),
        ) => kind == ek && tgt_matches(target, et),
        (
            CompiledGeometryOp::Pattern { kind, target, .. },
            ExpectedOp::Pattern(ek, et),
        ) => kind == ek && tgt_matches(target, et),
        (CompiledGeometryOp::Sweep { kind, profiles, .. }, ExpectedOp::Sweep(ek, ep)) => {
            kind == ek
                && profiles.len() == ep.len()
                && profiles
                    .iter()
                    .zip(ep.iter())
                    .all(|(p, et)| tgt_matches(p, et))
        }
        (
            CompiledGeometryOp::Modify { kind, target, .. },
            ExpectedOp::Modify(ek, et),
        ) => kind == ek && tgt_matches(target, et),
        (CompiledGeometryOp::Curve { kind, .. }, ExpectedOp::Curve(ek)) => kind == ek,
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
    let idx = names.iter().position(|&n| n == target).unwrap_or_else(|| {
        panic!(
            "geometry let '{}' not found in names list {:?}",
            target, names
        )
    });
    &template.realizations[idx]
}

// ─── compile helpers ──────────────────────────────────────────────────────────

/// Helper: parse + compile source, assert no errors, return compiled output.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_let_scope"));
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_let_scope"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: collect all error-severity diagnostics from a compiled module.
fn error_diagnostics(compiled: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ─── step-1: geometry let should be in scope for subsequent let ───

#[test]
fn geometry_let_in_scope_for_subsequent_let() {
    // The second geometry let `pattern` references `hole` (also a geometry let).
    // This should compile without errors — `hole` must be in scope.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let pattern = circular_pattern(hole, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
    let compiled = compile_source(source);
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
    param r: Length = 5mm
    param h: Length = 10mm
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
    let compiled = compile_no_errors(SRC_INTERSECTION_LET_BOUND);
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
    param r: Length = 5mm
    param h: Length = 10mm
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
        errors.iter().any(|d| d
            .message
            .contains("argument 1 must be a geometry expression")),
        "expected 'argument 1 must be a geometry expression' error, got: {:?}",
        errors
    );
}

// ─── amend: intersection_all with let-bound args ───

#[test]
fn intersection_all_with_let_bound_args() {
    let compiled = compile_no_errors(SRC_INTERSECTION_ALL_LET_BOUND);
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
    param r: Length = 5mm
    param h: Length = 10mm
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
    let realization = realization_named(template, &["a", "b", "combined", "c", "result"], "result");
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
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
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

// ─── task-1713 step-1: intersection op-level assertions ───

#[test]
fn intersection_ops_verify_boolean_variant_and_step_refs() {
    // Verifies the operations Vec of the `c` realization.
    // Source shared with intersection_with_let_bound_args.
    let compiled = compile_no_errors(SRC_INTERSECTION_LET_BOUND);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["a", "b", "c"], "c");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Box_,
            ExpectedOp::Sphere,
            ExpectedOp::BoolIntersect(0, 1),
        ],
    );
}

// ─── task-1713 step-3: intersection_all left-fold structure assertions ───

#[test]
fn intersection_all_ops_verify_left_fold_structure() {
    // d_geom = intersection_all(a, b, c) — left-fold of 3 args.
    // Expected ops: [Cylinder, Sphere, Intersect(0,1), Box, Intersect(2,3)].
    // Source shared with intersection_all_with_let_bound_args.
    let compiled = compile_no_errors(SRC_INTERSECTION_ALL_LET_BOUND);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["a", "b", "c", "d_geom"], "d_geom");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Sphere,
            ExpectedOp::BoolIntersect(0, 1),
            ExpectedOp::Box_,
            ExpectedOp::BoolIntersect(2, 3),
        ],
    );
}

// ─── step-4: geometry let still produces realization ───

#[test]
fn geometry_let_still_produces_realization() {
    // After the scope registration fix, geometry lets must still compile to
    // RealizationDecl entries (not value cells).
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let c = cylinder(r, h)
}"#;
    let compiled = parse_and_compile(source);
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
    let compiled = parse_and_compile(source);
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
    param r: Length = 5mm
    param h: Length = 10mm
    let base = cylinder(r, h)
    let pattern = circular_pattern(base, 0, 0, 0, 0, 0, 1, 6, 360)
    let mirrored = mirror(base, 0, 0, 0, 0, 1, 0)
}"#;
    let compiled = parse_and_compile(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations, got {}",
        template.realizations.len()
    );
}

// ─── task-1715 pre-1 + step-1: translate with let-bound target emits sub-op ───

#[test]
fn translate_let_bound_target_ops() {
    // translate(hole, 1, 0, 0) where hole is a let-bound cylinder.
    // Expected: cylinder sub-op at step 0, translate op referencing step 0.
    // Currently FAILS: translate compiles hole as scalar, no sub-op emitted.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = translate(hole, 1, 0, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Transform(TransformKind::Translate, Tgt::Sub("hole")),
        ],
    );
}

// ─── task-1715 step-3: validation tests for remaining transforms ───

#[test]
fn rotate_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = rotate(hole, 0, 0, 1, 90)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Transform(TransformKind::Rotate, Tgt::Sub("hole")),
        ],
    );
}

#[test]
fn scale_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = scale(hole, 2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Transform(TransformKind::Scale, Tgt::Sub("hole")),
        ],
    );
}

#[test]
fn rotate_around_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = rotate_around(hole, 0, 0, 0, 0, 0, 1, 90)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Transform(TransformKind::RotateAround, Tgt::Sub("hole")),
        ],
    );
}

// ─── task-1715 step-4: validation tests for patterns ───

#[test]
fn circular_pattern_let_bound_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = circular_pattern(hole, 0, 0, 0, 0, 0, 1, 6, 360)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Pattern(PatternKind::Circular, Tgt::Sub("hole")),
        ],
    );
}

#[test]
fn linear_pattern_let_bound_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = linear_pattern(hole, 1, 0, 0, 3, 10)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Pattern(PatternKind::Linear, Tgt::Sub("hole")),
        ],
    );
}

#[test]
fn mirror_let_bound_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
    let result = mirror(hole, 0, 0, 0, 0, 1, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Pattern(PatternKind::Mirror, Tgt::Sub("hole")),
        ],
    );
}

// ─── task-1715 step-5: validation tests for single-profile sweeps ───

#[test]
fn extrude_let_bound_profile_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let profile = cylinder(r, h)
    let result = extrude(profile, 10)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["profile", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Extrude, vec![Tgt::Sub("profile")]),
        ],
    );
}

#[test]
fn revolve_let_bound_profile_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let profile = cylinder(r, h)
    let result = revolve(profile, 0, 0, 0, 0, 0, 1, 90)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["profile", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Revolve, vec![Tgt::Sub("profile")]),
        ],
    );
}

#[test]
fn revolve_full_let_bound_profile_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let profile = cylinder(r, h)
    let result = revolve_full(profile, 0, 0, 0, 0, 0, 1)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["profile", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Revolve, vec![Tgt::Sub("profile")]),
        ],
    );
}

// ─── task-1715 step-6: validation tests for modifiers ───

#[test]
fn shell_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = shell(body, 1)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Shell, Tgt::Sub("body")),
        ],
    );
}

#[test]
fn thicken_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = thicken(body, 1)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Thicken, Tgt::Sub("body")),
        ],
    );
}

#[test]
fn draft_let_bound_target_ops() {
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = draft(body, 5, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Draft, Tgt::Sub("body")),
        ],
    );
}

// ─── task-1823: chamfer/fillet let-bound target resolution ───

#[test]
fn chamfer_let_bound_target_ops() {
    // chamfer(body, distance): body is a let-bound cylinder.
    // Expected: [Modify(Chamfer, Sub("body"))] — sibling-let pre-check (task #4668) emits Sub.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = chamfer(body, 2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Chamfer, Tgt::Sub("body")),
        ],
    );
}

#[test]
fn fillet_let_bound_target_ops() {
    // fillet(body, radius): body is a let-bound cylinder.
    // Expected: [Modify(Fillet, Sub("body"))] — sibling-let pre-check (task #4668) emits Sub.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = fillet(body, 2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Fillet, Tgt::Sub("body")),
        ],
    );
}

// ─── task-1941: chained modifier resolution ───

#[test]
fn chamfer_chained_shell_ops() {
    // chamfer(shell(body, t), d): shell is an inline modifier; body is a sibling realization.
    // Expected: [Modify(Shell, Sub("body")), Modify(Chamfer, Step(0))]
    //   shell resolves body → Sub("body") via sibling-let pre-check (task #4668)
    //   chamfer receives Step(0) (the shell result) as its geometry target
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = chamfer(shell(body, 0.5), 2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Shell, Tgt::Sub("body")),
            ExpectedOp::Modify(ModifyKind::Chamfer, Tgt::Step(0)),
        ],
    );
}

#[test]
fn fillet_chained_shell_ops() {
    // fillet(shell(body, t), d): shell is an inline modifier; body is a sibling realization.
    // Expected: [Modify(Shell, Sub("body")), Modify(Fillet, Step(0))]
    //   shell resolves body → Sub("body") via sibling-let pre-check (task #4668)
    //   fillet receives Step(0) (the shell result) as its geometry target
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = fillet(shell(body, 0.5), 2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Shell, Tgt::Sub("body")),
            ExpectedOp::Modify(ModifyKind::Fillet, Tgt::Step(0)),
        ],
    );
}

// ─── task-1960: deeper chains & boolean-innermost modifier targets ───

#[test]
fn chamfer_fillet_shell_chained_ops() {
    // chamfer(fillet(shell(body, t), d), e): three modifier layers; body is a sibling realization.
    // Step-walk with sibling-let pre-check (task #4668):
    //   body → Sub("body") (no inline cylinder)
    //   shell wraps body → Modify(Shell, Sub("body")) at Step(0)
    //   fillet wraps shell → Modify(Fillet, Step(0)) at Step(1)
    //   chamfer wraps fillet → Modify(Chamfer, Step(1)) at Step(2)
    // Expected: [Modify(Shell, Sub("body")), Modify(Fillet, Step(0)), Modify(Chamfer, Step(1))]
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let result = chamfer(fillet(shell(body, 0.5), 2), 1)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Modify(ModifyKind::Shell, Tgt::Sub("body")),
            ExpectedOp::Modify(ModifyKind::Fillet, Tgt::Step(0)),
            ExpectedOp::Modify(ModifyKind::Chamfer, Tgt::Step(1)),
        ],
    );
}

#[test]
fn chamfer_shell_difference_chained_ops() {
    // chamfer(shell(difference(a, b), t), d): boolean-innermost modifier chain.
    // Step-walk:
    //   a → Step(0), b → Step(1)
    //   difference(a, b) dispatches to compile_boolean_op → BoolDiff(0, 1) at Step(2)
    //   shell receives Step(2) as its target (step_offset + ops.len() - 1 = 0 + 3 - 1 = 2)
    //   shell emits Modify(Shell, 2) at Step(3)
    //   chamfer receives Step(3) as its target, emits Modify(Chamfer, 3) at Step(4)
    // Expected: [Cylinder, Cylinder, BoolDiff(0, 1), Modify(Shell, 2), Modify(Chamfer, 3)]
    let source = r#"structure S {
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let a = cylinder(r, h)
    let b = cylinder(r2, h)
    let result = chamfer(shell(difference(a, b), 0.5), 1)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["a", "b", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Cylinder,
            ExpectedOp::BoolDiff(0, 1),
            ExpectedOp::Modify(ModifyKind::Shell, Tgt::Step(2)),
            ExpectedOp::Modify(ModifyKind::Chamfer, Tgt::Step(3)),
        ],
    );
}

// ─── task-1715 step-7 + step-9: sweep two geometry args; loft all-geometry profiles ───

#[test]
fn sweep_two_let_bound_geometry_args() {
    // sweep(profile, path): both args are sibling geometry realizations.
    // Expected: [Sweep(Sweep, [Sub("profile"), Sub("path")])] — sibling-let pre-check (task #4668).
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    param pitch: Length = 2mm
    let profile = cylinder(r, h)
    let path = helix(r, pitch, h)
    let result = sweep(profile, path)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["profile", "path", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Sweep, vec![Tgt::Sub("profile"), Tgt::Sub("path")]),
        ],
    );
}

#[test]
fn loft_let_bound_profiles_ops() {
    // loft(p1, p2): both profiles are sibling geometry realizations.
    // Expected: [Sweep(Loft, [Sub("p1"), Sub("p2")])] — sibling-let pre-check (task #4668).
    let source = r#"structure S {
    param r1: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let p1 = cylinder(r1, h)
    let p2 = cylinder(r2, h)
    let result = loft(p1, p2)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["p1", "p2", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Loft, vec![Tgt::Sub("p1"), Tgt::Sub("p2")]),
        ],
    );
}

// ─── task-1715 step-11: cross-category composition ───

#[test]
fn cross_category_composition_ops() {
    // difference(body, translate(hole, 1, 0, 0)):
    // body is inlined by resolve_boolean_arg (boolean path, unchanged).
    // translate(hole, ...) goes through the generic loop; hole → Sub("hole") (task #4668).
    // Expected: [Cylinder(body), Transform(Translate, Sub("hole")), BoolDiff(0,1)]
    let source = r#"structure S {
    param r: Length = 5mm
    param r2: Length = 3mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result = difference(body, translate(hole, 1, 0, 0))
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["body", "hole", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Transform(TransformKind::Translate, Tgt::Sub("hole")),
            ExpectedOp::BoolDiff(0, 1),
        ],
    );
}

// ─── task-1715 step-12: cyclic refs through transforms (behavior change: task #4668) ───

#[test]
fn cyclic_refs_through_transforms_error() {
    // Mutually-recursive geometry lets through non-boolean ops should produce a cycle error.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let a = translate(b, 1, 0, 0)
    let b = rotate(a, 0, 0, 1, 90)
}"#;
    // With the sibling-let pre-check (task #4668), a bare Ident arg that names a
    // sibling geometry realization is emitted as GeomRef::Sub — no inline recursion,
    // so the compile-time cycle detector (which relied on the visiting-set in the
    // recursive compile_geometry_call) no longer fires. Cycles between geometry
    // realizations are detected at eval time by the Kahn scheduler instead.
    //
    // After the fix: a and b each produce one realization with a Sub target.
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let a_real = realization_named(template, &["a", "b"], "a");
    assert_op_sequence(
        &a_real.operations,
        &[ExpectedOp::Transform(TransformKind::Translate, Tgt::Sub("b"))],
    );
    let b_real = realization_named(template, &["a", "b"], "b");
    assert_op_sequence(
        &b_real.operations,
        &[ExpectedOp::Transform(TransformKind::Rotate, Tgt::Sub("a"))],
    );
}

// ─── amend: inline (non-let-bound) geometry arg resolution ───

#[test]
fn translate_inline_geometry_arg_ops() {
    // translate(cylinder(r, h), 1, 0, 0): the geometry arg is inline, not let-bound.
    // The generic resolution block should still compile it as a sub-op.
    // Expected: [Cylinder, Transform(Translate, 0)]
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let result = translate(cylinder(r, h), 1, 0, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Transform(TransformKind::Translate, Tgt::Step(0)),
        ],
    );
}

// ─── amend: chained non-boolean transforms with let bindings ───

#[test]
fn chained_transforms_step_indices() {
    // let a = cylinder(r, h); let b = translate(a, 1, 0, 0); let c = rotate(b, 0, 0, 1, 90)
    // With sibling-let pre-check (task #4668): b → Sub("b") in c's rotate op; no inlining.
    // c's realization: [Rotate(Sub("b"))]
    // (b itself has [Translate(Sub("a"))]; a itself has [Cylinder])
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let a = cylinder(r, h)
    let b = translate(a, 1, 0, 0)
    let c = rotate(b, 0, 0, 1, 90)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["a", "b", "c"], "c");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Transform(TransformKind::Rotate, Tgt::Sub("b")),
        ],
    );
}

// ─── step-7: geometry let does not produce a value cell ───

#[test]
fn geometry_let_not_a_value_cell() {
    // Geometry lets should produce realizations, NOT value cells.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let hole = cylinder(r, h)
}"#;
    let compiled = parse_and_compile(source);
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

// ─── task-1708: ident-alias geometry let support ──────────────────────────────

#[test]
fn ident_alias_geometry_let_produces_realization() {
    // `alias` is a let-bound name whose init expression is an Ident that names
    // a geometry let (`body`). The compiler should recognise `alias` as a
    // geometry let too, producing 2 realizations.
    // FAILS before fix: only body gets a realization; alias is compiled as a value cell.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        2,
        "expected 2 realizations (body and alias), got {}",
        template.realizations.len()
    );
}

#[test]
fn ident_alias_in_boolean_op() {
    // alias is an ident alias of a geometry let; difference(alias, sphere(r))
    // must resolve alias through geometry_lets HashMap.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
    let result = difference(alias, sphere(r))
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (body, alias, result), got {}",
        template.realizations.len()
    );
    let result_real = realization_named(template, &["body", "alias", "result"], "result");
    assert_op_sequence(
        &result_real.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Sphere,
            ExpectedOp::BoolDiff(0, 1),
        ],
    );
}

#[test]
fn chained_ident_alias_transitive() {
    // Multi-level chain: let a = cyl; let b = a; let c = b.
    // The incremental set must capture all three as geometry lets so that
    // difference(c, sphere(r)) resolves c → b → a → cylinder.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let a = cylinder(r, h)
    let b = a
    let c = b
    let result = difference(c, sphere(r))
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        4,
        "expected 4 realizations (a, b, c, result), got {}",
        template.realizations.len()
    );
    let result_real = realization_named(template, &["a", "b", "c", "result"], "result");
    assert_op_sequence(
        &result_real.operations,
        &[
            ExpectedOp::Cylinder,
            ExpectedOp::Sphere,
            ExpectedOp::BoolDiff(0, 1),
        ],
    );
}

#[test]
fn ident_alias_not_a_value_cell() {
    // An ident alias to a geometry let must NOT appear as a value cell.
    // (Verifies the second-pass skip for ident aliases.)
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert!(
        !template
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "alias"),
        "geometry-let ident alias 'alias' should NOT be a value cell, but found one"
    );
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "body"),
        "geometry let 'body' should NOT be a value cell, but found one"
    );
}

#[test]
fn ident_alias_realization_op_sequence() {
    // The alias realization's ops must equal those of the aliased geometry let.
    // compile_geometry_call resolves the Ident through geometry_lets → cylinder expr.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let alias_real = realization_named(template, &["body", "alias"], "alias");
    assert_op_sequence(&alias_real.operations, &[ExpectedOp::Cylinder]);
}

#[test]
fn ident_alias_with_transform() {
    // Alias used as geometry arg in a non-boolean geometry function (translate).
    // translate's geometry_arg_indices is [0], so the first arg is resolved as
    // a geometry let Ident.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
    let result = translate(alias, 1, 0, 0)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (body, alias, result), got {}",
        template.realizations.len()
    );
    let result_real = realization_named(template, &["body", "alias", "result"], "result");
    assert_op_sequence(
        &result_real.operations,
        &[
            ExpectedOp::Transform(TransformKind::Translate, Tgt::Sub("alias")),
        ],
    );
}

#[test]
fn ident_alias_scope_type_is_geometry() {
    // After the fix, `alias` has Type::Geometry in scope. This means:
    //   1. `alias` is skipped in the second pass → appears in realizations, NOT value_cells.
    //   2. `let x = alias + 1` is NOT a geometry let → x IS compiled as a value cell.
    // Together these prove the first-pass type registration correctly typed `alias` as
    // Geometry. Without the fix, `alias` would be Type::dimensionless_scalar() and compiled as a value cell,
    // so realizations.len() would be 1 (only body), failing assertion (1).
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let body = cylinder(r, h)
    let alias = body
    let x = alias + 1
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // (1) Both `body` and `alias` must be realizations (not value cells).
    assert_eq!(
        template.realizations.len(),
        2,
        "expected 2 realizations (body, alias), got {} — alias must have Type::Geometry",
        template.realizations.len()
    );
    // (2) `alias` itself must NOT appear as a value cell.
    assert!(
        !template
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "alias"),
        "alias should NOT be a value cell (it has Type::Geometry)"
    );
    // (3) `x = alias + 1` is NOT a geometry let, so x must be a value cell.
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "expected value cell 'x' for let x = alias + 1"
    );
}

// ─── task-1708 amendment: guarded-group and negative-case tests ──────────────

#[test]
fn ident_alias_in_guarded_group_documents_current_behavior() {
    // Documents pre-existing limitation: geometry lets (including ident aliases)
    // inside `where {}` blocks are recognized in pass 1 (Type::Geometry, added to
    // known_geometry_lets) and skipped in pass 2 (not a value cell), but pass 3
    // only iterates top-level `structure.members` — so they are NOT compiled into
    // realizations. The alias therefore silently disappears from the output.
    //
    // This test pins the current behavior so that future changes to guarded-group
    // realization compilation have a regression signal.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    param active: Bool = true
    let body = cylinder(r, h)
    where active {
        let alias = body
    }
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // Only the top-level `body` produces a realization; `alias` inside the
    // guarded block does not (known limitation — pass 3 skips guarded members).
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization (body only — alias inside guarded block is not compiled), got {}",
        template.realizations.len()
    );
    // `alias` must NOT appear as a value cell (the pass-2 geometry-let skip applies).
    assert!(
        !template
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "alias"),
        "alias inside guarded block should NOT be a value cell"
    );
}

#[test]
fn non_geometry_ident_alias_is_a_value_cell() {
    // `let x = 5; let y = x` — `x` is not a geometry let, so `y` must NOT be
    // recognised as a geometry ident alias. Both should appear as value cells.
    let source = r#"structure S {
    let x = 5
    let y = x
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        0,
        "expected 0 realizations — neither x nor y is a geometry let"
    );
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "expected value cell 'x'"
    );
    assert!(
        template.value_cells.iter().any(|vc| vc.id.member == "y"),
        "expected value cell 'y' — non-geometry ident alias must stay a value cell"
    );
}

#[test]
fn undefined_ident_alias_produces_error() {
    // `let alias = nonexistent` — `nonexistent` is not defined anywhere. The
    // compiler should emit at least one error diagnostic (name not found).
    let source = r#"structure S {
    let alias = nonexistent
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error for undefined name 'nonexistent', got none"
    );
}

#[test]
fn cyclic_ident_alias_does_not_crash() {
    // `let a = b; let b = a` — neither name is declared before the other, so
    // the forward-pass incremental set never adds either to known_geometry_lets.
    // Both are treated as value cells referencing each other (a solver cycle).
    // The compiler must not panic or ICE; it may or may not emit an error.
    let source = r#"structure S {
    let a = b
    let b = a
}"#;
    // Must not panic.
    let compiled = compile_with_diagnostics(source);
    let template = &compiled.templates[0];
    // Neither is a geometry let — no realizations expected.
    assert_eq!(
        template.realizations.len(),
        0,
        "cyclic ident aliases must not produce realizations"
    );
}

// ─── task-1733 step-1: loft with 3+ profiles ────────────────────────────────

#[test]
fn loft_three_profiles_ops() {
    // loft(p1, p2, p3) with three sibling geometry realizations.
    // Expected: [Sweep(Loft, [Sub("p1"), Sub("p2"), Sub("p3")])] — sibling-let pre-check (task #4668).
    let source = r#"structure S {
    param r1: Length = 5mm
    param r2: Length = 3mm
    param r3: Length = 1mm
    param h: Length = 10mm
    let p1 = cylinder(r1, h)
    let p2 = cylinder(r2, h)
    let p3 = cylinder(r3, h)
    let result = loft(p1, p2, p3)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let realization = realization_named(template, &["p1", "p2", "p3", "result"], "result");
    assert_op_sequence(
        &realization.operations,
        &[
            ExpectedOp::Sweep(SweepKind::Loft, vec![Tgt::Sub("p1"), Tgt::Sub("p2"), Tgt::Sub("p3")]),
        ],
    );
}

// ─── task-1733 step-2: error-path tests for non-geometry args ───────────────

#[test]
fn sweep_non_geometry_profile_emits_error() {
    // sweep(42, helix(5, 2, 10)): arg 0 is a literal, not a geometry expression.
    // sweep() should emit a "profile … must be a geometry expression" diagnostic.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    param pitch: Length = 2mm
    let path = helix(r, pitch, h)
    let result = sweep(42, path)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("sweep()") && d.message.contains("profile")),
        "expected sweep() profile error diagnostic, got: {:?}",
        errors
    );
}

#[test]
fn sweep_non_geometry_path_emits_error() {
    // sweep(cylinder(r, h), 42): arg 1 is a literal, not a geometry expression.
    // sweep() should emit a "path … must be a geometry expression" diagnostic.
    let source = r#"structure S {
    param r: Length = 5mm
    param h: Length = 10mm
    let profile = cylinder(r, h)
    let result = sweep(profile, 42)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("sweep()") && d.message.contains("path")),
        "expected sweep() path error diagnostic, got: {:?}",
        errors
    );
}

#[test]
fn translate_non_geometry_target_uses_fallback() {
    // translate(42, 1, 0, 0): arg 0 is a literal number, not a geometry expression.
    // The geom_ref fallback silently uses GeomRef::Step(step_offset). This should
    // compile without errors (the fallback is intentional for single-geom-arg functions).
    let source = r#"structure S {
    let result = translate(42, 1, 0, 0)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    // No error expected — the geom_ref closure falls back silently.
    assert!(
        errors.is_empty(),
        "translate() with non-geometry target should not produce errors (silent fallback), got: {:?}",
        errors
    );
    // Should still produce a realization with a Transform op.
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    assert_op_sequence(
        &template.realizations[0].operations,
        &[ExpectedOp::Transform(TransformKind::Translate, Tgt::Step(0))],
    );
}

#[test]
fn loft_non_geometry_profiles_uses_fallback() {
    // loft(42, 43): both args are literal numbers, not geometry expressions.
    // loft silently falls back with GeomRef::Step offsets (matching geom_ref convention).
    // This should compile without errors.
    let source = r#"structure S {
    let result = loft(42, 43)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    // No error expected — loft silently falls back (consistent with extrude/revolve_full).
    assert!(
        errors.is_empty(),
        "loft() with non-geometry profiles should not produce errors (silent fallback), got: {:?}",
        errors
    );
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    assert_op_sequence(
        &template.realizations[0].operations,
        &[ExpectedOp::Sweep(SweepKind::Loft, vec![Tgt::Step(0), Tgt::Step(1)])],
    );
}

#[test]
fn extrude_non_geometry_target_uses_fallback() {
    // extrude(42, 10): arg 0 is a literal number, not a geometry expression.
    // extrude() uses the same geom_ref fallback path as translate() and other
    // single-geom-arg functions — it silently falls back to GeomRef::Step(step_offset).
    // This verifies the silent-fallback behavior is consistent across the category,
    // not just for transform functions.
    let source = r#"structure S {
    let result = extrude(42, 10)
}"#;
    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    // No error expected — the geom_ref closure falls back silently.
    assert!(
        errors.is_empty(),
        "extrude() with non-geometry target should not produce errors (silent fallback), got: {:?}",
        errors
    );
    // Should still produce a realization with an Extrude (Sweep) op.
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    assert_op_sequence(
        &template.realizations[0].operations,
        &[ExpectedOp::Sweep(SweepKind::Extrude, vec![Tgt::Step(0)])],
    );
}

// ── RealizationDecl.name tests ────────────────────────────────────────────────

/// Verifies that each geometry-let binding in a structure compiles to a
/// `RealizationDecl` whose `name` field is `Some(let_binding_name)`, so
/// callers can build a name→handle map for `GeomRef::Sub` resolution.
#[test]
fn realization_decl_name_matches_let_binding_name() {
    // SRC_DIFFERENCE_LET_BOUND has:
    //   let body = cylinder(r, h)
    //   let hole = cylinder(r2, h)
    //   let result = difference(body, hole)
    let compiled = compile_no_errors(SRC_DIFFERENCE_LET_BOUND);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        3,
        "expected 3 realizations (body, hole, result)"
    );

    let body_real = realization_named(template, &["body", "hole", "result"], "body");
    assert_eq!(
        body_real.name,
        Some("body".to_string()),
        "body realization should have name Some(\"body\")"
    );

    let hole_real = realization_named(template, &["body", "hole", "result"], "hole");
    assert_eq!(
        hole_real.name,
        Some("hole".to_string()),
        "hole realization should have name Some(\"hole\")"
    );

    let result_real = realization_named(template, &["body", "hole", "result"], "result");
    assert_eq!(
        result_real.name,
        Some("result".to_string()),
        "result realization should have name Some(\"result\")"
    );
}

// ── RealizationDecl.span tests ────────────────────────────────────────────────

/// Verifies that a Solid-typed param realization's `span` field is populated
/// from the originating `ParamDecl.span` — i.e., it carries meaningful byte
/// offsets that point back into the source text rather than defaulting to
/// `(0, 0)`.
#[test]
fn realization_span_populated_from_param_decl_default_span() {
    // Source is crafted so "param g: Solid" starts well past byte-offset 0.
    // The "structure Widget {\n    " prefix guarantees span.start > 0.
    let source = "structure Widget {\n    param g: Solid = cylinder(10mm, 20mm)\n}";
    let compiled = compile_source(source);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template not found");

    // The Solid param should produce exactly 1 realization.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 realization for `param g: Solid = cylinder(...)`, \
         got {}",
        template.realizations.len()
    );

    let realization = &template.realizations[0];

    assert!(
        realization.span.start > 0,
        "span.start should be > 0 (param g is not at the very beginning of source), \
         got span.start = {}",
        realization.span.start
    );
    assert!(
        realization.span.end > realization.span.start,
        "span.end should be > span.start (non-empty span), \
         got start={} end={}",
        realization.span.start,
        realization.span.end
    );
    let slice = &source[realization.span.start as usize..realization.span.end as usize];
    assert!(
        slice.contains("param g"),
        "span slice should contain \"param g\", got: {:?}",
        slice
    );
    assert!(
        slice.contains("cylinder"),
        "span slice should contain \"cylinder\", got: {:?}",
        slice
    );
}

/// Verifies that a Solid-typed param inside a guarded group (`where <cond> {
/// param g: Solid = cylinder(...) }`) produces a `RealizationDecl` whose `span`
/// is populated from the originating `ParamDecl.span`, exercising the
/// `emit_guarded_geometry_realizations` code path.  The span must point into
/// the guarded declaration rather than defaulting to `(0, 0)`.
#[test]
fn realization_span_populated_from_guarded_param_decl_span() {
    // Source is crafted so `param g` starts well past byte-offset 0.
    // The "structure W {\n    param some_cond …\n    where some_cond {\n        "
    // prefix guarantees span.start > 0.
    let source = "structure W {\n    param some_cond : Bool = true\n    where some_cond {\n        param g : Solid = cylinder(10mm, 20mm)\n    }\n}";
    let compiled = compile_source(source);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "W")
        .expect("W template not found");

    // The guarded Solid param should produce exactly 1 realization.
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 realization for guarded `param g : Solid = cylinder(...)`, \
         got {}",
        template.realizations.len()
    );

    let realization = &template.realizations[0];

    assert!(
        realization.span.start > 0,
        "span.start should be > 0 (param g is not at the beginning of source), \
         got span.start = {}",
        realization.span.start
    );
    assert!(
        realization.span.end > realization.span.start,
        "span.end should be > span.start (non-empty span), \
         got start={} end={}",
        realization.span.start,
        realization.span.end
    );
    let slice = &source[realization.span.start as usize..realization.span.end as usize];
    assert!(
        slice.contains("param g"),
        "span slice should contain \"param g\", got: {:?}",
        slice
    );
    assert!(
        slice.contains("cylinder"),
        "span slice should contain \"cylinder\", got: {:?}",
        slice
    );
}

/// Verifies that a geometry-let realization's `span` field is populated from
/// the originating `LetDecl.span` — i.e., it carries meaningful byte offsets
/// that point back into the source text rather than defaulting to `(0, 0)`.
#[test]
fn realization_span_populated_from_let_decl_span() {
    // Source is crafted so "let body = cylinder(r, h)" starts well past
    // byte-offset 0.  Three preceding lines (structure header + two params)
    // guarantee span.start > 0.
    let source = "structure S {\n    param r: Length = 5mm\n    param h: Length = 10mm\n    let body = cylinder(r, h)\n}";
    let compiled = compile_source(source);

    let template = &compiled.templates[0];
    // The first realization corresponds to `let body = cylinder(r, h)`.
    let realization = &template.realizations[0];

    assert!(
        realization.span.start > 0,
        "span.start should be > 0 (let body is not at the very beginning of source), \
         got span.start = {}",
        realization.span.start
    );
    assert!(
        realization.span.end > realization.span.start,
        "span.end should be > span.start (non-empty span), \
         got start={} end={}",
        realization.span.start,
        realization.span.end
    );
    let slice = &source[realization.span.start as usize..realization.span.end as usize];
    assert!(
        slice.contains("body"),
        "span slice should contain \"body\", got: {:?}",
        slice
    );
    assert!(
        slice.contains("cylinder"),
        "span slice should contain \"cylinder\", got: {:?}",
        slice
    );
}

// ─── task-3395 regression: if-then-else returning Solid emits clean Error ─────

/// Regression pin: a geometry let whose initializer is a geometry-typed
/// if-then-else with STRUCTURALLY-INCOMPATIBLE branches (box vs cylinder) must
/// produce a clean compile-time Error mentioning "if-then-else" and "geometry"
/// rather than crashing at eval time with the cryptic "unresolvable
/// GeomRef::Step(0)" message.
///
/// The source was repurposed from box-vs-box (which the scalar-arg hoisting pass
/// now successfully compiles) to box-vs-cylinder so the graceful-error fallback
/// path remains covered by a regression test.
///
/// The test exercises: `is_geometry_let(Conditional)` → `true`
/// (routes to compile_geometry_call) → `try_hoist_geometry_conditional` returns
/// `None` (incompatible names) → existing Error arm fires → diagnostic.
#[test]
fn conditional_returning_solid_in_let_emits_compile_error() {
    // box(length, od, od)  vs  cylinder(od, length): different name — not hoistable.
    let source = r#"structure AirBearing {
    param length: Length = 100mm
    param od: Length = 50mm
    param axis: Length = 0
    let body = if axis == 0 then box(length, od, od) else cylinder(od, length)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    // At least one Error must mention "if-then-else" and "geometry".
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("if-then-else") && d.message.contains("geometry")),
        "expected a compile-time Error containing 'if-then-else' and 'geometry', \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The Error must have at least one DiagnosticLabel — pointing at the
    // conditional expression in the source.
    let target_error = errors
        .iter()
        .find(|d| d.message.contains("if-then-else") && d.message.contains("geometry"))
        .unwrap();
    assert!(
        !target_error.labels.is_empty(),
        "the if-then-else Error must have at least one DiagnosticLabel"
    );

    // The label's span must start exactly at the `if` keyword and extend at
    // least to the end of the else-branch `cylinder(od, length)` call.
    let if_offset = source.find(" if ").expect("source must contain ' if '") + 1;
    // End: byte past the closing `)` of `cylinder(od, length)` in the else branch.
    let else_end = source
        .find("cylinder(od, length)")
        .expect("source must contain 'cylinder(od, length)'")
        + "cylinder(od, length)".len();
    assert!(
        !target_error.labels.is_empty(),
        "must have at least one label"
    );
    let label = &target_error.labels[0];
    assert_eq!(
        label.span.start as usize,
        if_offset,
        "label start must equal the byte offset of the 'if' keyword (offset {}); \
         got labels: {:?}",
        if_offset,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        label.span.end as usize,
        else_end,
        "label end must equal the byte past the closing ')' of the else-branch \
         cylinder call (offset {}); got labels: {:?}",
        else_end,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
}

// ─── task-3418 regression: match returning Solid emits clean Error ─────────

/// Regression pin: a geometry let whose initializer is a geometry-typed
/// match expression must produce a clean compile-time Error mentioning
/// "match" and "geometry" rather than crashing at eval time with the
/// cryptic "unresolvable GeomRef::Step(0)" message.
///
/// Mirrors the user scenario:
///   `let body = match axis { X => box(length, od, od), ... }`
///
/// The test exercises: `is_geometry_let(Match)` → `true`
/// (routes to compile_geometry_call) → branching-kind Error arm fires →
/// diagnostic.
#[test]
fn match_returning_solid_in_let_emits_compile_error() {
    let source = r#"enum Axis { X, Y, Z }
structure AxisBox {
    param length: Length = 100mm
    param od: Length = 50mm
    let axis = Axis.X
    let body = match axis { X => box(length, od, od), Y => box(od, length, od), Z => box(od, od, length) }
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    // At least one Error must mention "match" and "geometry".
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("match") && d.message.contains("geometry")),
        "expected a compile-time Error containing 'match' and 'geometry', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The Error must have at least one DiagnosticLabel — pointing at the
    // match expression in the source.
    let target_error = errors
        .iter()
        .find(|d| d.message.contains("match") && d.message.contains("geometry"))
        .unwrap();
    assert!(
        !target_error.labels.is_empty(),
        "the match Error must have at least one DiagnosticLabel"
    );

    // The label span must point at exactly the `match` expression — pinning
    // both byte offsets so a regression where the span bleeds past the match
    // block into the structure's closing `}` would still be caught.
    let match_expr = "match axis { X => box(length, od, od), Y => box(od, length, od), Z => box(od, od, length) }";
    let match_start = source
        .find(match_expr)
        .expect("source must contain the match expression");
    let match_end = match_start + match_expr.len();
    let label = &target_error.labels[0];
    assert_eq!(
        label.span.start as usize,
        match_start,
        "label start must equal the byte offset of the 'match' keyword (offset {}); \
         got labels: {:?}",
        match_start,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        label.span.end as usize,
        match_end,
        "label end must equal the byte past the closing '}}' of the match block \
         (offset {}); got labels: {:?}",
        match_end,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
}

// ─── task-3815: geometry-valued if-then-else hoisting ─────────────────────────

/// RED step-1 (step 1 of 6): structurally-identical box-vs-box branches must
/// hoist to a single `Primitive{Box}` op whose three scalar args are each a
/// `CompiledExprKind::Conditional`.
///
/// Uses `box(length, od, length) else box(od, length, od)` so every arg pair is
/// distinct (length vs od or od vs length) and the peephole optimisation does not
/// collapse any arg to a direct Ident — all three must be Conditionals.
///
/// This test fails today (the existing rejection error fires instead of the
/// hoist). It will pass after step-2 implements `try_hoist_geometry_conditional`.
#[test]
fn geometry_valued_if_then_else_box_lowers_to_conditional_primitive() {
    let source = r#"structure S {
    param length: Length = 100mm
    param od: Length = 50mm
    param axis: Length = 0
    let body = if axis == 0 then box(length, od, length) else box(od, length, od)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 realization (for 'body'), got {}",
        template.realizations.len()
    );

    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        1,
        "expected 1 compiled op (hoisted box), got {} ops: {:?}",
        ops.len(),
        ops
    );

    let (kind, args) = match &ops[0] {
        CompiledGeometryOp::Primitive { kind, args } => (kind, args),
        other => panic!("expected Primitive op, got {:?}", other),
    };
    assert_eq!(*kind, PrimitiveKind::Box, "expected Box primitive");
    assert_eq!(args.len(), 3, "box should have 3 named args");

    // All three args differ between branches (length vs od / od vs length), so
    // the peephole does not fire and every arg is a Conditional.
    for (name, arg) in args {
        assert!(
            matches!(&arg.kind, CompiledExprKind::Conditional { .. }),
            "box arg '{}' should be a Conditional (hoisted), got {:?}",
            name,
            arg.kind
        );
    }
}

// ─── task-3815 step-3: recursive merge cases ──────────────────────────────────

/// RED step-3a: boolean-op tree `union(box,box)` vs `union(box,box)` hoists to
/// `[Primitive{Box}, Primitive{Box}, Boolean{Union}]` where each box op has
/// `Conditional` scalar args.
#[test]
fn geometry_valued_if_then_else_union_tree_lowers_to_conditional_box_ops() {
    let source = r#"structure S {
    param a: Length = 10mm
    param b: Length = 20mm
    param c: Length = 30mm
    param d: Length = 40mm
    param axis: Length = 0
    let body = if axis == 0 then union(box(a, a, a), box(b, b, b)) else union(box(c, c, c), box(d, d, d))
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;

    // Expected: [Primitive{Box}, Primitive{Box}, Boolean{Union, Step(0), Step(1)}]
    assert_op_sequence(
        ops,
        &[
            ExpectedOp::Box_,
            ExpectedOp::Box_,
            ExpectedOp::BoolUnion(0, 1),
        ],
    );

    // Each box op must have Conditional scalar args.
    for (i, op) in ops.iter().enumerate().take(2) {
        match op {
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args,
            } => {
                for (name, arg) in args {
                    assert!(
                        matches!(&arg.kind, CompiledExprKind::Conditional { .. }),
                        "union-tree: box op[{}] arg '{}' should be Conditional, got {:?}",
                        i,
                        name,
                        arg.kind
                    );
                }
            }
            other => panic!("ops[{}]: expected Primitive{{Box}}, got {:?}", i, other),
        }
    }
}

/// RED step-3b: nested else-if chain `if a then box(p) else if b then box(q) else box(r)`
/// must hoist to a single `Primitive{Box}` whose args are nested `Conditional`s.
/// This fails under step-2 because the else-branch is itself a Conditional.
#[test]
fn geometry_valued_if_then_else_chain_lowers_to_single_conditional_primitive() {
    let source = r#"structure S {
    param p: Length = 10mm
    param q: Length = 20mm
    param r: Length = 30mm
    param axis: Int = 0
    let body = if axis == 0 then box(p, p, p) else if axis == 1 then box(q, q, q) else box(r, r, r)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors for else-if chain, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;

    // A single Primitive{Box} — the else-if chain reduces to box with nested Conditionals.
    assert_eq!(
        ops.len(),
        1,
        "expected 1 op (hoisted box), got {} ops: {:?}",
        ops.len(),
        ops
    );
    match &ops[0] {
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args,
        } => {
            assert_eq!(args.len(), 3);
            for (name, arg) in args {
                assert!(
                    matches!(&arg.kind, CompiledExprKind::Conditional { .. }),
                    "else-if chain: box arg '{}' should be Conditional, got {:?}",
                    name,
                    arg.kind
                );
            }
        }
        other => panic!("expected Primitive{{Box}}, got {:?}", other),
    }
}

/// RED step-3c: translate-wrapped `translate(box(a),tx,0,0)` vs `translate(box(b),tx,0,0)`
/// hoists to `[Primitive{Box}, Transform{Translate}]` where the box has Conditional args.
#[test]
fn geometry_valued_if_then_else_translate_wraps_conditional_box() {
    let source = r#"structure S {
    param a: Length = 10mm
    param b: Length = 20mm
    param tx: Length = 5mm
    param axis: Length = 0
    let body = if axis == 0 then translate(box(a, a, a), tx, 0, 0) else translate(box(b, b, b), tx, 0, 0)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no errors for translate-wrapped conditional, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let ops = &template.realizations[0].operations;

    // Expected: [Primitive{Box}, Transform{Translate, Step(0)}]
    assert_op_sequence(
        ops,
        &[
            ExpectedOp::Box_,
            ExpectedOp::Transform(TransformKind::Translate, Tgt::Step(0)),
        ],
    );

    // Box op must have Conditional scalar args.
    match &ops[0] {
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args,
        } => {
            for (name, arg) in args {
                assert!(
                    matches!(&arg.kind, CompiledExprKind::Conditional { .. }),
                    "translate-wrapped: box arg '{}' should be Conditional, got {:?}",
                    name,
                    arg.kind
                );
            }
        }
        other => panic!("ops[0]: expected Primitive{{Box}}, got {:?}", other),
    }
}

// ─── task-3815 step-5: enum-driven conditional + negative cases ───────────────

/// step-5 IR positive: enum-driven geometry if-then-else compiles without error.
///
/// No OCCT required. Mirrors the task acceptance repro (Note3IfSolid) at the
/// compiler IR level: a `param pick : Pick` drives a conditional between two
/// structurally-identical boxes. The hoisting pass must recognise the two
/// `box(…)` branches as compatible and produce a single `Primitive{Box}` op
/// whose three scalar args are each `CompiledExprKind::Conditional`.
#[test]
fn geometry_valued_if_then_else_enum_pick_lowers_to_conditional_primitive() {
    let source = r#"enum Pick { A, B }
structure Note3IfSolid {
    param pick : Pick = Pick.A
    let body = if pick == Pick.A then box(40mm, 40mm, 40mm) else box(80mm, 20mm, 20mm)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected exactly 1 realization (for 'body'), got {}",
        template.realizations.len()
    );

    let ops = &template.realizations[0].operations;
    assert_eq!(
        ops.len(),
        1,
        "expected 1 compiled op (hoisted box), got {} ops: {:?}",
        ops.len(),
        ops
    );

    let (kind, args) = match &ops[0] {
        CompiledGeometryOp::Primitive { kind, args } => (kind, args),
        other => panic!("expected Primitive op, got {:?}", other),
    };
    assert_eq!(*kind, PrimitiveKind::Box, "expected Box primitive");
    assert_eq!(args.len(), 3, "box should have 3 named args");

    for (name, arg) in args {
        assert!(
            matches!(&arg.kind, CompiledExprKind::Conditional { .. }),
            "box arg '{}' should be a Conditional (hoisted), got {:?}",
            name,
            arg.kind
        );
    }
}

/// step-5 negative: box vs sphere — different primitive name, should fall
/// through the hoisting pass (returns `None`) and trigger the existing
/// compile-time Error mentioning "if-then-else" and "geometry".
#[test]
fn geometry_valued_if_then_else_name_mismatch_emits_compile_error() {
    let source = r#"structure S {
    param a: Length = 10mm
    param r: Length = 5mm
    param cond: Length = 0
    let body = if cond == 0 then box(a, a, a) else sphere(r)
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    // At least one Error must mention "if-then-else" and "geometry".
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("if-then-else") && d.message.contains("geometry")),
        "expected a compile-time Error containing 'if-then-else' and 'geometry', \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The Error must have at least one DiagnosticLabel.
    let target_error = errors
        .iter()
        .find(|d| d.message.contains("if-then-else") && d.message.contains("geometry"))
        .unwrap();
    assert!(
        !target_error.labels.is_empty(),
        "the if-then-else Error must have at least one DiagnosticLabel"
    );

    // The label span must start at the `if` keyword and extend to the end of
    // the else branch.
    let if_offset = source.find(" if ").expect("source must contain ' if '") + 1;
    let else_branch = "sphere(r)";
    let else_end = source
        .find(else_branch)
        .expect("source must contain 'sphere(r)'")
        + else_branch.len();
    let label = &target_error.labels[0];
    assert_eq!(
        label.span.start as usize,
        if_offset,
        "label start must equal the byte offset of the 'if' keyword (offset {}); \
         got labels: {:?}",
        if_offset,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        label.span.end as usize,
        else_end,
        "label end must equal the byte past the closing ')' of the else-branch \
         sphere call (offset {}); got labels: {:?}",
        else_end,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
}

/// step-5 negative: ident-let branches — `if c then a else b` where `a` and `b`
/// are geometry `let` bindings (not inline calls). `try_hoist` returns `None`
/// for `Ident` branches, so the existing compile-time Error path fires and must
/// mention "if-then-else" and "geometry".
#[test]
fn geometry_valued_if_then_else_ident_let_branches_emits_compile_error() {
    let source = r#"structure S {
    param cond: Length = 0
    let a = box(10mm, 10mm, 10mm)
    let b = box(20mm, 20mm, 20mm)
    let body = if cond == 0 then a else b
}"#;

    let compiled = compile_with_diagnostics(source);
    let errors = error_diagnostics(&compiled);

    // At least one Error must mention "if-then-else" and "geometry".
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("if-then-else") && d.message.contains("geometry")),
        "expected a compile-time Error containing 'if-then-else' and 'geometry', \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The Error must have at least one DiagnosticLabel.
    let target_error = errors
        .iter()
        .find(|d| d.message.contains("if-then-else") && d.message.contains("geometry"))
        .unwrap();
    assert!(
        !target_error.labels.is_empty(),
        "the if-then-else Error must have at least one DiagnosticLabel"
    );

    // The label span must start at the `if` keyword and extend to the end of
    // the else branch.
    let if_offset = source.find(" if ").expect("source must contain ' if '") + 1;
    let else_branch = "if cond == 0 then a else b";
    let else_end = source
        .find(else_branch)
        .expect("source must contain the full if-then-else expression")
        + else_branch.len();
    let label = &target_error.labels[0];
    assert_eq!(
        label.span.start as usize,
        if_offset,
        "label start must equal the byte offset of the 'if' keyword (offset {}); \
         got labels: {:?}",
        if_offset,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        label.span.end as usize,
        else_end,
        "label end must equal the byte past the 'b' of the else branch \
         (offset {}); got labels: {:?}",
        else_end,
        target_error
            .labels
            .iter()
            .map(|l| (l.span.start, l.span.end, &l.message))
            .collect::<Vec<_>>()
    );
}

// ─── task-4668 step-1: sibling-let bare-name target resolves to Sub (RED) ─────

#[test]
fn fillet_bare_let_sibling_target_resolves_to_sub() {
    // `let b = box(...); let e = edges_at_height(b, ...); let f = fillet(b, e, 2mm)`
    //
    // Under the sibling-let pre-check (task #4668) the bare name `b` in `fillet(b, ...)`
    // should produce GeomRef::Sub("b"), NOT an inlined Box op.
    //
    // RED on baseline: f's realization is [Box, Modify(Fillet, Step(0))]
    // GREEN after fix: f's realization is [Modify(Fillet, Sub("b"))]
    let source = r#"structure S {
    let b = box(10mm, 10mm, 15mm)
    let e = edges_at_height(b, 7.5mm, 0.1mm)
    let f = fillet(b, e, 2mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // `e` (edges_at_height) is a selector let — it does NOT produce a realization.
    let f_real = realization_named(template, &["b", "f"], "f");
    assert_op_sequence(
        &f_real.operations,
        &[ExpectedOp::Modify(ModifyKind::Fillet, Tgt::Sub("b"))],
    );
}

#[test]
fn chamfer_bare_let_sibling_target_resolves_to_sub() {
    // `let b = box(...); let f = chamfer(b, 2mm)` — 2-arg form, no selector let.
    //
    // RED on baseline: f's realization is [Box, Modify(Chamfer, Step(0))].
    // GREEN after fix: f's realization is [Modify(Chamfer, Sub("b"))].
    let source = r#"structure S {
    let b = box(10mm, 10mm, 15mm)
    let f = chamfer(b, 2mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let f_real = realization_named(template, &["b", "f"], "f");
    assert_op_sequence(
        &f_real.operations,
        &[ExpectedOp::Modify(ModifyKind::Chamfer, Tgt::Sub("b"))],
    );
}

#[test]
fn fillet_bare_let_sibling_2arg_resolves_to_sub() {
    // 2-arg fillet(b, 2mm): same sibling-let check, no selector complication.
    //
    // RED on baseline: f's realization is [Box, Modify(Fillet, Step(0))].
    // GREEN after fix: f's realization is [Modify(Fillet, Sub("b"))].
    let source = r#"structure S {
    let b = box(10mm, 10mm, 15mm)
    let f = fillet(b, 2mm)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let f_real = realization_named(template, &["b", "f"], "f");
    assert_op_sequence(
        &f_real.operations,
        &[ExpectedOp::Modify(ModifyKind::Fillet, Tgt::Sub("b"))],
    );
}
