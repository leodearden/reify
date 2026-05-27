//! Forall statement-form parsing tests (task 2363).
//!
//! Covers all four body alternatives (connect, chain, constraint, constraint
//! instantiation) plus disambiguation and nesting regression scenarios.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("forall_test"));
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
    assert!(
        constraint.where_clause.is_none(),
        "where_clause should be None"
    );

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
                matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if *v == 50.0),
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
        reify_core::ContentHash(0),
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
        other => panic!(
            "expected ForallConstraintBody::Instantiation, got {:?}",
            other
        ),
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
        reify_core::ContentHash(0),
        "content_hash should be non-zero"
    );
}

// ── step-9 regression tests ───────────────────────────────────────────────────

/// step-9a: Expression-form `forall` inside `constraint` must NOT become a
/// `MemberDecl::ForallConstraint`; it must remain `MemberDecl::Constraint`
/// with an inner `ExprKind::Quantifier`.  Pins acceptance criteria 3-4:
/// disambiguation is driven by the token after `:`.
#[test]
fn parse_expression_form_unchanged() {
    let source = r#"
structure S {
    let items = [1, 2, 3]
    constraint forall x in items: x > 0
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 2, "expected exactly two members");

    // members[1] must be a plain Constraint, NOT a ForallConstraint.
    let constraint = match &members[1] {
        MemberDecl::Constraint(c) => c,
        MemberDecl::ForallConstraint(_) => {
            panic!("expression-form forall was incorrectly lowered as ForallConstraint")
        }
        other => panic!("expected Constraint, got {:?}", other),
    };

    // The expression is ExprKind::Quantifier(ForAll).
    match &constraint.expr.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate: _,
        } => {
            assert_eq!(*kind, QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(
                matches!(&collection.kind, ExprKind::Ident(n) if n == "items"),
                "expected collection Ident(items), got {:?}",
                collection.kind
            );
        }
        other => panic!("expected ExprKind::Quantifier, got {:?}", other),
    }
}

/// step-9b: `forall_statement` nested inside a guarded block must be emitted
/// as `MemberDecl::ForallConnect`, not silently dropped.  Pins that
/// `lower_member` is correctly dispatched from within `lower_members` which is
/// called by `lower_guarded_block`.
#[test]
fn parse_forall_inside_guarded_block() {
    let source = r#"
structure S {
    param needs : Bool
    where needs {
        forall v in vents: connect v.inlet -> housing.air_channel
    }
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 2, "expected param + guarded_group");

    let group = match &members[1] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected GuardedGroup, got {:?}", other),
    };

    assert_eq!(
        group.members.len(),
        1,
        "guarded group should have one member"
    );

    match &group.members[0] {
        MemberDecl::ForallConnect(d) => {
            assert_eq!(d.variable, "v");
            assert!(
                matches!(&d.collection.kind, ExprKind::Ident(n) if n == "vents"),
                "expected collection Ident(vents)"
            );
        }
        other => panic!(
            "expected ForallConnect inside guarded block, got {:?}",
            other
        ),
    }
}

/// step-1 (task 2366): Parse `forall v in vents: constraint v.mass < 50 where active`
/// → `MemberDecl::ForallConstraint` with `ForallConstraintBody::Constraint` whose
/// `where_clause` is `Some(wc)` and `wc.condition.kind == ExprKind::Ident("active")`.
/// Pins the `Some` half of the body where-clause contract that
/// `parse_forall_constraint` covers in the `None` direction (briefing item 1,
/// parser-disambiguation gap-fill).
#[test]
fn parse_forall_constraint_with_body_where_clause() {
    // NOTE: `vents` is intentionally undeclared here. The syntax-level parser
    // does not perform identifier resolution, so `vents` parses as a bare
    // `Ident` collection expression without error. The purpose of this test is
    // to pin the body where-clause `Some` shape, not to exercise declaration
    // checking — which belongs in compiler-level tests.
    let source = r#"
structure S {
    param active : Bool = true
    forall v in vents: constraint v.mass < 50 where active
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(
        members.len(),
        2,
        "expected exactly two members (param + forall)"
    );

    let decl = match &members[1] {
        MemberDecl::ForallConstraint(d) => d,
        other => panic!("expected ForallConstraint at members[1], got {:?}", other),
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

    // The body where-clause must be Some — this is the gap the existing
    // `parse_forall_constraint` test does not cover.
    let wc = constraint
        .where_clause
        .as_ref()
        .expect("expected Some(where_clause) on the body constraint, got None");

    // Condition: `active` — an Ident
    assert!(
        matches!(&wc.condition.kind, ExprKind::Ident(n) if n == "active"),
        "expected where_clause condition Ident(active), got {:?}",
        wc.condition.kind
    );
}

/// step-9c: A `forall_statement` whose collection is itself a parenthesized
/// quantifier expression must lower correctly, producing `ForallConnect` with
/// `collection.kind == ExprKind::Quantifier`.  Pins the GLR-resolution corpus
/// pin from task 2362 (nested-quantifier-collection scenario).
#[test]
fn parse_forall_with_nested_quantifier_collection() {
    let source = r#"
structure S {
    forall v in (forall x in xs: x > 0): connect v.a -> v.b
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

    // The collection should be the inner quantifier (parentheses are transparent).
    match &decl.collection.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate: _,
        } => {
            assert_eq!(*kind, QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(
                matches!(&collection.kind, ExprKind::Ident(n) if n == "xs"),
                "expected inner collection Ident(xs), got {:?}",
                collection.kind
            );
        }
        other => panic!(
            "expected collection to be ExprKind::Quantifier, got {:?}",
            other
        ),
    }

    // Body: connect v.a -> v.b
    match &decl.body {
        ForallConnectBody::Connect(c) => {
            match &c.left.expr.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert!(
                        matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                        "expected left object Ident(v)"
                    );
                    assert_eq!(member, "a");
                }
                other => panic!("expected MemberAccess for left, got {:?}", other),
            }
            match &c.right.expr.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert!(
                        matches!(object.kind, ExprKind::Ident(ref n) if n == "v"),
                        "expected right object Ident(v)"
                    );
                    assert_eq!(member, "b");
                }
                other => panic!("expected MemberAccess for right, got {:?}", other),
            }
        }
        other => panic!("expected ForallConnectBody::Connect, got {:?}", other),
    }
}
