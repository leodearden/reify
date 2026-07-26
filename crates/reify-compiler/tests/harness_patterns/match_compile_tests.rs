//! Match expression compilation tests.

/// Compile match with all variants covered → no error diagnostics.
#[test]
fn compile_match_exhaustive_passes() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S {
    let d = Direction.In
    let x = match d { In => 1, Out => 2, Bidi => 3 }
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_match"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
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
        reify_ir::CompiledExprKind::Match { discriminant, arms } => {
            // Discriminant should be a ValueRef to 'd'
            match &discriminant.kind {
                reify_ir::CompiledExprKind::ValueRef(id) => {
                    assert_eq!(id.member, "d");
                }
                other => panic!("expected ValueRef, got {:?}", other),
            }
            assert_eq!(arms.len(), 3, "expected 3 arms");
        }
        other => panic!("expected Match, got {:?}", other),
    }
}

/// Compile match missing variant → should emit exhaustiveness diagnostic.
#[test]
fn compile_match_missing_variant_emits_diagnostic() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S {
    let d = Direction.In
    let x = match d { In => 1, Out => 2 }
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_missing"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should have an error diagnostic mentioning missing variant or exhaustiveness
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostics for missing variant 'Bidi'"
    );
    assert!(
        errors.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("bidi") || msg.contains("exhaustive") || msg.contains("missing")
        }),
        "diagnostic should mention missing variant, got: {:?}",
        errors
    );
}

/// Compile match with wildcard → passes exhaustiveness.
#[test]
fn compile_match_wildcard_is_exhaustive() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S {
    let d = Direction.In
    let x = match d { In => 1, _ => 0 }
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_wildcard"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics with wildcard, got: {:?}",
        errors
    );
}
