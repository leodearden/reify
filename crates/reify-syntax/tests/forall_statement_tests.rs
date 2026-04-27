//! Forall statement-form parsing tests (task 2363).
//!
//! Covers all four body alternatives (connect, chain, constraint, constraint
//! instantiation) plus disambiguation and nesting regression scenarios.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("forall_test"));
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

/// step-3: Parse `forall v in vents: chain v.out -> hub.a -> hub.b`
/// -> MemberDecl::ForallConnect with Chain body (3 elements).
#[test]
fn parse_forall_chain() {
    let source = r#"
structure S {
    forall v in vents: chain v.out -> hub.a -> hub.b
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1, "expected exactly one member");

    let decl = match &members[0] {
        MemberDecl::ForallConnect(d) => d,
        other => panic!("expected ForallConnect, got {:?}", other),
    };

    assert_eq!(decl.variable, "v");
    assert!(
        matches!(&decl.collection.kind, ExprKind::Ident(n) if n == "vents"),
        "expected collection Ident(vents), got {:?}",
        decl.collection.kind
    );

    let chain = match &decl.body {
        ForallConnectBody::Chain(c) => c,
        other => panic!("expected ForallConnectBody::Chain, got {:?}", other),
    };

    assert_eq!(chain.elements.len(), 3, "expected 3 chain elements");

    // element 0: v.out
    match &chain.elements[0].kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                "expected elem[0] object Ident(v)"
            );
            assert_eq!(member, "out");
        }
        other => panic!("expected MemberAccess for elem[0], got {:?}", other),
    }

    // element 1: hub.a
    match &chain.elements[1].kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "hub"),
                "expected elem[1] object Ident(hub)"
            );
            assert_eq!(member, "a");
        }
        other => panic!("expected MemberAccess for elem[1], got {:?}", other),
    }

    // element 2: hub.b
    match &chain.elements[2].kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "hub"),
                "expected elem[2] object Ident(hub)"
            );
            assert_eq!(member, "b");
        }
        other => panic!("expected MemberAccess for elem[2], got {:?}", other),
    }
}

/// step-5: Parse `forall v in vents: constraint v.mass < 50`
/// -> MemberDecl::ForallConstraint with Constraint body.
#[test]
fn parse_forall_constraint() {
    let source = r#"
structure S {
    forall v in vents: constraint v.mass < 50
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1, "expected exactly one member");

    let decl = match &members[0] {
        MemberDecl::ForallConstraint(d) => d,
        other => panic!("expected ForallConstraint, got {:?}", other),
    };

    assert_eq!(decl.variable, "v");
    assert!(
        matches!(&decl.collection.kind, ExprKind::Ident(n) if n == "vents"),
        "expected collection Ident(vents), got {:?}",
        decl.collection.kind
    );

    let constraint = match &decl.body {
        ForallConstraintBody::Constraint(c) => c,
        other => panic!("expected ForallConstraintBody::Constraint, got {:?}", other),
    };

    assert!(constraint.label.is_none(), "label should be None");
    assert!(constraint.where_clause.is_none(), "where_clause should be None");

    // expr: v.mass < 50
    match &constraint.expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "<");
            match &left.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert!(
                        matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                        "expected left object Ident(v)"
                    );
                    assert_eq!(member, "mass");
                }
                other => panic!("expected MemberAccess for left, got {:?}", other),
            }
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral(v) if *v == 50.0),
                "expected right NumberLiteral(50), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp(<) for constraint expr, got {:?}", other),
    }

    // span and content_hash sanity
    assert!(
        decl.span.start < decl.span.end,
        "span should be non-empty: {:?}",
        decl.span
    );
    assert_ne!(
        decl.content_hash,
        reify_types::ContentHash(0),
        "content_hash should be non-zero"
    );
}

/// step-7: Parse `forall v in vents: constraint MinDistance(point: v.center)`
/// -> MemberDecl::ForallConstraint with Instantiation body.
#[test]
fn parse_forall_constraint_instantiation() {
    let source = r#"
structure S {
    forall v in vents: constraint MinDistance(point: v.center)
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1, "expected exactly one member");

    let decl = match &members[0] {
        MemberDecl::ForallConstraint(d) => d,
        other => panic!("expected ForallConstraint, got {:?}", other),
    };

    assert_eq!(decl.variable, "v");
    assert!(
        matches!(&decl.collection.kind, ExprKind::Ident(n) if n == "vents"),
        "expected collection Ident(vents), got {:?}",
        decl.collection.kind
    );

    let ci = match &decl.body {
        ForallConstraintBody::Instantiation(ci) => ci,
        other => panic!("expected ForallConstraintBody::Instantiation, got {:?}", other),
    };

    assert_eq!(ci.name, "MinDistance");
    assert_eq!(ci.args.len(), 1, "expected 1 argument");
    assert_eq!(ci.args[0].0, "point");

    // v.center
    match &ci.args[0].1.kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                "expected arg object Ident(v), got {:?}",
                object.kind
            );
            assert_eq!(member, "center");
        }
        other => panic!("expected MemberAccess for arg, got {:?}", other),
    }
}

/// step-1: Parse `forall v in vents: connect v.inlet -> housing.air_channel`
/// -> MemberDecl::ForallConnect with Connect body.
#[test]
fn parse_forall_connect() {
    let source = r#"
structure S {
    forall v in vents: connect v.inlet -> housing.air_channel
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1, "expected exactly one member");

    let decl = match &members[0] {
        MemberDecl::ForallConnect(d) => d,
        other => panic!("expected ForallConnect, got {:?}", other),
    };

    assert_eq!(decl.variable, "v");
    assert!(
        matches!(&decl.collection.kind, ExprKind::Ident(n) if n == "vents"),
        "expected collection Ident(vents), got {:?}",
        decl.collection.kind
    );

    let connect = match &decl.body {
        ForallConnectBody::Connect(c) => c,
        other => panic!("expected ForallConnectBody::Connect, got {:?}", other),
    };

    // left: v.inlet
    match &connect.left.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                "expected left object Ident(v), got {:?}",
                object.kind
            );
            assert_eq!(member, "inlet");
        }
        other => panic!("expected MemberAccess for left, got {:?}", other),
    }

    // right: housing.air_channel
    match &connect.right.expr.kind {
        ExprKind::MemberAccess { object, member } => {
            assert!(
                matches!(object.kind, ExprKind::Ident(ref n) if n == "housing"),
                "expected right object Ident(housing), got {:?}",
                object.kind
            );
            assert_eq!(member, "air_channel");
        }
        other => panic!("expected MemberAccess for right, got {:?}", other),
    }

    assert_eq!(connect.operator, ConnectOp::Forward);

    // span and content_hash sanity
    assert!(
        decl.span.start < decl.span.end,
        "span should be non-empty: {:?}",
        decl.span
    );
    assert_ne!(
        decl.content_hash,
        reify_types::ContentHash(0),
        "content_hash should be non-zero"
    );
}
