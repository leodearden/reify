//! Quantifier compilation tests.

use reify_ir::CompiledExprKind;

/// step-3: Compile `forall x in [1,2,3]: x > 0` -> CompiledExprKind::Quantifier
/// with ForAll kind, verify collection and predicate sub-expressions are compiled.
#[test]
fn compile_forall_basic() {
    let source = r#"
structure S {
    constraint forall x in [1, 2, 3]: x > 0
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_quant"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
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

    let template = &compiled.templates[0];
    // The constraint should have an expression
    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::Quantifier {
            kind,
            variable,
            variable_id,
            collection,
            predicate,
        } => {
            assert_eq!(*kind, reify_ast::QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(
                variable_id.entity.starts_with("$quant"),
                "variable entity should start with $quant"
            );
            assert_eq!(variable_id.member, "x");
            // collection should be a ListLiteral
            assert!(matches!(&collection.kind, CompiledExprKind::ListLiteral(_)));
            // predicate should be a BinOp(Gt)
            match &predicate.kind {
                CompiledExprKind::BinOp { op, .. } => {
                    assert_eq!(*op, reify_ir::BinOp::Gt);
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_quant2"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
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

    let template = &compiled.templates[0];
    let found_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "found")
        .expect("should have 'found' value cell");

    let expr = found_cell
        .default_expr
        .as_ref()
        .expect("let should have expr");
    match &expr.kind {
        CompiledExprKind::Quantifier { kind, variable, .. } => {
            assert_eq!(*kind, reify_ast::QuantifierKind::Exists);
            assert_eq!(variable, "x");
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}

/// step-13: Compile a lambda wrapping a quantifier, e.g. `|items| forall x in [items]: x > 0`,
/// and assert the lambda's `captures` vec does NOT contain the quantifier's bound variable `x`.
/// This exposes a bug in `collect_body_refs_inner` where the Quantifier arm blindly recurses
/// into the predicate without filtering out `variable_id`.
///
/// Note: `[items]` wraps the untyped lambda param (which defaults to Real) in a List literal,
/// producing a `List<Real>` collection that satisfies the quantifier type-check introduced in
/// task-2066. The capture-analysis behavior under test is unchanged: `items` is still a lambda
/// param (not a capture) and `x` is still a quantifier bound variable (not a capture).
#[test]
fn lambda_containing_forall_has_correct_captures() {
    let source = r#"
structure S {
    let checker = |items| forall x in [items]: x > 0
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_quant_cap"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
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

    let template = &compiled.templates[0];
    let checker_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "checker")
        .expect("should have 'checker' value cell");

    let expr = checker_cell
        .default_expr
        .as_ref()
        .expect("let should have expr");
    match &expr.kind {
        CompiledExprKind::Lambda { captures, .. } => {
            // The captures should NOT contain the quantifier's bound variable `x`
            // (whose entity starts with "$quant").
            for cap in captures {
                assert!(
                    !cap.entity.starts_with("$quant"),
                    "lambda captures should not include quantifier bound variable, but found: {:?}",
                    cap
                );
            }
            // captures should be empty since `items` is a lambda param (not a capture)
            // and `x` is a quantifier bound variable (should be excluded).
            // `[items]` references `items` as the list element — `items` is still a param,
            // not an external capture.
            assert!(
                captures.is_empty(),
                "expected no captures (items is a lambda param, x is a quantifier var), got: {:?}",
                captures
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}
