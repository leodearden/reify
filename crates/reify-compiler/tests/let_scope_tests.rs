//! Tests for let-binding scope resolution, especially geometry lets.

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_types::Severity;

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
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result = difference(body, hole)
}"#;
    let compiled = compile_no_errors(source);
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
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    param w: Scalar = 8mm
    param d: Scalar = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = union_all(a, b, c)
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

// ─── task-1609 step-5: nested boolean ops with let args ───

#[test]
fn nested_boolean_ops_with_let_args() {
    // combined is a boolean op result used as input to another boolean op via let.
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let a = cylinder(r, h)
    let b = cylinder(r2, h)
    let combined = difference(a, b)
    let c = sphere(r)
    let result = union(combined, c)
}"#;
    let compiled = compile_no_errors(source);
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
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let result = difference(body, cylinder(r2, h))
}"#;
    let compiled = compile_no_errors(source);
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
    // Same source as difference_with_let_bound_args.
    // Verifies the operations Vec of the `result` realization (index 2).
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let hole = cylinder(r2, h)
    let result = difference(body, hole)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    // result is the 3rd realization (index 2)
    let ops = &template.realizations[2].operations;
    assert_eq!(ops.len(), 3, "expected 3 ops, got {}", ops.len());
    assert!(
        matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[0], got {:?}",
        ops[0]
    );
    assert!(
        matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[1], got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
}

// ─── task-1709 step-2: nested boolean ops step-index assertions ───

#[test]
fn nested_boolean_ops_verify_step_indices() {
    // Same source as nested_boolean_ops_with_let_args.
    // combined=difference(a,b), result=union(combined,c).
    // result is the 5th realization (index 4).
    // Its operations Vec inlines: [Cylinder(a), Cylinder(b), Diff(0,1), Sphere(c), Union(2,3)]
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let a = cylinder(r, h)
    let b = cylinder(r2, h)
    let combined = difference(a, b)
    let c = sphere(r)
    let result = union(combined, c)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let ops = &template.realizations[4].operations;
    assert_eq!(ops.len(), 5, "expected 5 ops, got {}", ops.len());
    assert!(
        matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[0], got {:?}",
        ops[0]
    );
    assert!(
        matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[1], got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
    assert!(
        matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }),
        "expected Primitive::Sphere at ops[3], got {:?}",
        ops[3]
    );
    assert!(
        matches!(
            ops[4],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(2),
                right: GeomRef::Step(3)
            }
        ),
        "expected Boolean{{Union, Step(2), Step(3)}} at ops[4], got {:?}",
        ops[4]
    );
}

// ─── task-1709 step-3: mixed let-bound + inline op assertions ───

#[test]
fn mixed_let_and_inline_ops_verify_step_refs() {
    // Same source as mixed_let_and_inline_in_boolean_op.
    // body is let-bound; right arg is inline cylinder(r2,h).
    // result is the 2nd realization (index 1).
    let source = r#"structure S {
    param r: Scalar = 5mm
    param r2: Scalar = 3mm
    param h: Scalar = 10mm
    let body = cylinder(r, h)
    let result = difference(body, cylinder(r2, h))
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let ops = &template.realizations[1].operations;
    assert_eq!(ops.len(), 3, "expected 3 ops, got {}", ops.len());
    assert!(
        matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[0], got {:?}",
        ops[0]
    );
    assert!(
        matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[1], got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Difference, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
}

// ─── task-1709 step-4: union_all left-fold structure assertions ───

#[test]
fn union_all_ops_verify_left_fold_structure() {
    // Same source as union_all_with_let_bound_args.
    // d_geom = union_all(a, b, c) — left-fold of 3 args.
    // d_geom is the 4th realization (index 3).
    // Expected ops: [Cylinder, Sphere, Union(0,1), Box, Union(2,3)]
    let source = r#"structure S {
    param r: Scalar = 5mm
    param h: Scalar = 10mm
    param w: Scalar = 8mm
    param d: Scalar = 8mm
    let a = cylinder(r, h)
    let b = sphere(r)
    let c = box(w, h, d)
    let d_geom = union_all(a, b, c)
}"#;
    let compiled = compile_no_errors(source);
    let template = &compiled.templates[0];
    let ops = &template.realizations[3].operations;
    assert_eq!(ops.len(), 5, "expected 5 ops, got {}", ops.len());
    assert!(
        matches!(ops[0], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Cylinder, .. }),
        "expected Primitive::Cylinder at ops[0], got {:?}",
        ops[0]
    );
    assert!(
        matches!(ops[1], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Sphere, .. }),
        "expected Primitive::Sphere at ops[1], got {:?}",
        ops[1]
    );
    assert!(
        matches!(
            ops[2],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(0),
                right: GeomRef::Step(1)
            }
        ),
        "expected Boolean{{Union, Step(0), Step(1)}} at ops[2], got {:?}",
        ops[2]
    );
    assert!(
        matches!(ops[3], CompiledGeometryOp::Primitive { kind: PrimitiveKind::Box, .. }),
        "expected Primitive::Box at ops[3], got {:?}",
        ops[3]
    );
    assert!(
        matches!(
            ops[4],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(2),
                right: GeomRef::Step(3)
            }
        ),
        "expected Boolean{{Union, Step(2), Step(3)}} at ops[4], got {:?}",
        ops[4]
    );
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
