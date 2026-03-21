//! Tests for let-binding scope resolution, especially geometry lets.

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
