//! Tests for let-binding scope resolution, especially geometry lets.

use reify_compiler::CompiledGeometryOp;
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
