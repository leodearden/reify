//! Quantifier expression parsing tests.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("quantifier_test"));
    let structure = match &module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// step-1: Parse `forall x in list: x > 0` -> Quantifier(ForAll)
#[test]
fn parse_forall_expression() {
    let source = r#"
structure S {
    let items = [1, 2, 3]
    constraint forall x in items: x > 0
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let constraint = match &members[1] {
        MemberDecl::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    match &constraint.expr.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
            ..
        } => {
            assert_eq!(*kind, QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(matches!(&collection.kind, ExprKind::Ident(n) if n == "items"));
            match &predicate.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, ">");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "x"));
                    assert!(
                        matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if *v == 0.0)
                    );
                }
                other => panic!("expected BinOp(>), got {:?}", other),
            }
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}

/// Parse `forall x in items: x > 0` and verify the parser populates
/// `variable_span` as the narrow binder identifier span (just the `x`),
/// not the full quantifier expression span.
///
/// This test intentionally fails to compile until `ExprKind::Quantifier` gains
/// the `variable_span: SourceSpan` field (step-2) — the canonical RED for a
/// field-addition in this codebase.
#[test]
fn parse_quantifier_populates_variable_span() {
    let source = r#"
structure S {
    let items = [1, 2, 3]
    constraint forall x in items: x > 0
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let constraint = match &members[1] {
        MemberDecl::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    match &constraint.expr.kind {
        ExprKind::Quantifier {
            variable,
            variable_span,
            ..
        } => {
            assert_eq!(variable, "x");
            // Locate `x` by finding "x in" rather than "forall x" + offset
            // arithmetic, so the assertion is robust to any whitespace between
            // `forall` and `x` (the parser is correct regardless of spacing).
            let off = source.find("x in").unwrap();
            assert_eq!(
                variable_span.start,
                off as u32,
                "variable_span.start must point at the binder `x`"
            );
            assert_eq!(
                variable_span.end,
                (off + 1) as u32,
                "variable_span.end must be one byte past the binder `x`"
            );
            // The variable span must be strictly narrower than the whole-expression span.
            assert!(
                variable_span.end - variable_span.start
                    < constraint.expr.span.end - constraint.expr.span.start,
                "variable_span must be strictly narrower than the full expression span"
            );
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}

/// step-1: Parse `exists x in set: x == target` -> Quantifier(Exists)
#[test]
fn parse_exists_expression() {
    let source = r#"
structure S {
    let target = 5
    let items = [1, 2, 3, 5]
    let found = exists x in items: x == target
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[2] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "found");

    match &let_decl.value.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
            ..
        } => {
            assert_eq!(*kind, QuantifierKind::Exists);
            assert_eq!(variable, "x");
            assert!(matches!(&collection.kind, ExprKind::Ident(n) if n == "items"));
            match &predicate.kind {
                ExprKind::BinOp { op, .. } => {
                    assert_eq!(op, "==");
                }
                other => panic!("expected BinOp(==), got {:?}", other),
            }
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}
