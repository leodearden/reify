//! Quantifier compilation tests.

use reify_types::CompiledExprKind;

/// step-3: Compile `forall x in [1,2,3]: x > 0` -> CompiledExprKind::Quantifier
/// with ForAll kind, verify collection and predicate sub-expressions are compiled.
#[test]
fn compile_forall_basic() {
    let source = r#"
structure S {
    constraint forall x in [1, 2, 3]: x > 0
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_quant"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no error diagnostics, got: {:?}", errors);

    let template = &compiled.templates[0];
    // The constraint should have an expression
    assert!(!template.constraints.is_empty(), "should have at least one constraint");

    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::Quantifier {
            kind,
            variable,
            variable_id,
            collection,
            predicate,
        } => {
            assert_eq!(*kind, reify_types::QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(variable_id.entity.starts_with("$quant"), "variable entity should start with $quant");
            assert_eq!(variable_id.member, "x");
            // collection should be a ListLiteral
            assert!(matches!(&collection.kind, CompiledExprKind::ListLiteral(_)));
            // predicate should be a BinOp(Gt)
            match &predicate.kind {
                CompiledExprKind::BinOp { op, .. } => {
                    assert_eq!(*op, reify_types::BinOp::Gt);
                }
                other => panic!("expected BinOp(Gt), got {:?}", other),
            }
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}

/// step-3: Compile `exists x in [1,2,3]: x == 2` -> CompiledExprKind::Quantifier(Exists)
#[test]
fn compile_exists_basic() {
    let source = r#"
structure S {
    let found = exists x in [1, 2, 3]: x == 2
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_quant2"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no error diagnostics, got: {:?}", errors);

    let template = &compiled.templates[0];
    let found_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "found")
        .expect("should have 'found' value cell");

    let expr = found_cell.default_expr.as_ref().expect("let should have expr");
    match &expr.kind {
        CompiledExprKind::Quantifier {
            kind,
            variable,
            ..
        } => {
            assert_eq!(*kind, reify_types::QuantifierKind::Exists);
            assert_eq!(variable, "x");
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}
