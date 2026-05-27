//! Lambda expression parsing tests.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("lambda_test"));
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

/// step-1: Parse `|x| x * 2` as a lambda expression.
#[test]
fn parse_lambda_single_untyped_param() {
    let source = r#"
structure S {
    let f = |x| x * 2
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "f");

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "x");
            assert!(params[0].type_expr.is_none());
            // Body: x * 2
            match &body.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, "*");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "x"));
                    assert!(
                        matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if *v == 2.0)
                    );
                }
                other => panic!("expected BinOp(*), got {:?}", other),
            }
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// step-3: Parse `|x: Real, y: Real| x + y` — 2 typed params.
#[test]
fn parse_lambda_two_typed_params() {
    let source = r#"
structure S {
    let f = |x: Real, y: Real| x + y
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "f");

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "x");
            assert_eq!(params[0].type_expr.as_ref().unwrap().to_string(), "Real");
            assert_eq!(params[1].name, "y");
            assert_eq!(params[1].type_expr.as_ref().unwrap().to_string(), "Real");
            // Body: x + y
            match &body.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, "+");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "x"));
                    assert!(matches!(&right.kind, ExprKind::Ident(n) if n == "y"));
                }
                other => panic!("expected BinOp(+), got {:?}", other),
            }
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// step-5: Parse `|| true` (zero-param lambda).
#[test]
fn parse_lambda_zero_params() {
    let source = r#"
structure S {
    let f = || true
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 0);
            assert!(matches!(&body.kind, ExprKind::BoolLiteral(true)));
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// step-5: Parse `|x, y, z| x + y + z` (3-param untyped lambda).
#[test]
fn parse_lambda_three_params() {
    let source = r#"
structure S {
    let f = |x, y, z| x + y + z
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 3);
            assert_eq!(params[0].name, "x");
            assert_eq!(params[1].name, "y");
            assert_eq!(params[2].name, "z");
            assert!(params.iter().all(|p| p.type_expr.is_none()));
            // Body should be a nested BinOp (x + y) + z
            assert!(matches!(&body.kind, ExprKind::BinOp { op, .. } if op == "+"));
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// || coexistence: structure with both `|x| x * 2` and `a || b`, assert both
/// produce correct nodes.
#[test]
fn parse_lambda_and_logical_or_coexist() {
    let source = r#"
structure S {
    param a: Bool = true
    param b: Bool = false
    let f = |x| x * 2
    let g = a || b
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    // Find the lambda let
    let f_decl = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "f" => Some(l),
            _ => None,
        })
        .expect("should have 'f'");
    assert!(matches!(&f_decl.value.kind, ExprKind::Lambda { .. }));

    // Find the let with || (logical or)
    let g_decl = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "g" => Some(l),
            _ => None,
        })
        .expect("should have 'g'");
    match &g_decl.value.kind {
        ExprKind::BinOp { op, .. } => assert_eq!(op, "||"),
        other => panic!("expected BinOp(||), got {:?}", other),
    }
}

/// Precedence: parse `|x| x * 2 + 3`, assert body is full `Add(Mul(x, 2), 3)`.
#[test]
fn parse_lambda_body_precedence() {
    let source = r#"
structure S {
    let f = |x| x * 2 + 3
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "x");
            // Body should be Add(Mul(x, 2), 3) — not just Mul(x, 2)
            match &body.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, "+", "outer should be +, got {}", op);
                    // left should be x * 2
                    match &left.kind {
                        ExprKind::BinOp { op: inner_op, .. } => {
                            assert_eq!(inner_op, "*");
                        }
                        other => panic!("expected BinOp(*) on left, got {:?}", other),
                    }
                    // right should be 3
                    assert!(
                        matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if *v == 3.0)
                    );
                }
                other => panic!("expected BinOp(+), got {:?}", other),
            }
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}
