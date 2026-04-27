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
