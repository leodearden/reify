//! Match expression compilation tests.

/// Compile match with all variants covered → no error diagnostics.
#[test]
fn compile_match_exhaustive_passes() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S {
    param d : Scalar
    let x = match d { In => 1, Out => 2, Bidi => 3 }
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_match"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected (the placeholder emits an error)
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    // Verify the compiled template has the match expression in the let's default_expr
    let template = &compiled.templates[0];
    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");

    let x_expr = x_cell.default_expr.as_ref().expect("let should have expr");
    match &x_expr.kind {
        reify_types::CompiledExprKind::Match { discriminant, arms } => {
            // Discriminant should be a ValueRef to 'd'
            match &discriminant.kind {
                reify_types::CompiledExprKind::ValueRef(id) => {
                    assert_eq!(id.member, "d");
                }
                other => panic!("expected ValueRef, got {:?}", other),
            }
            assert_eq!(arms.len(), 3, "expected 3 arms");
        }
        other => panic!("expected Match, got {:?}", other),
    }
}
