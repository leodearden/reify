//! Guard (where) clause tests.
//!
//! Tests for per-declaration postfix guards (`param x : Length where cond`)
//! and block-level guards (`where cond { ...members... } else { ...members... }`).

use reify_ast::*;

/// Helper: parse source and return the first structure's members.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("guard_test"));
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

// ── Step 1: param and let with where clause ──────────────────────

/// Parse `param x : Scalar where needs_cooling` — expects ParamDecl with WhereClause.
#[test]
fn parse_param_with_where_clause() {
    let source = r#"structure S {
    param x : Scalar where needs_cooling
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let param = match &members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(param.name, "x");

    let wc = param.where_clause.as_ref().expect("expected where_clause");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "needs_cooling"),
        other => panic!("expected Ident('needs_cooling'), got {:?}", other),
    }
}

/// Parse `let y = x * 2 where active` — expects LetDecl with WhereClause.
#[test]
fn parse_let_with_where_clause() {
    let source = r#"structure S {
    param x : Scalar = 5mm
    let y = x * 2 where active
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 2);

    let let_decl = match &members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "y");

    let wc = let_decl
        .where_clause
        .as_ref()
        .expect("expected where_clause");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "active"),
        other => panic!("expected Ident('active'), got {:?}", other),
    }
}

// ── Step 3: constraint and sub with where clause ─────────────────

/// Parse `constraint thickness > 2mm where active` — expects ConstraintDecl with WhereClause.
#[test]
fn parse_constraint_with_where_clause() {
    let source = r#"structure S {
    param thickness : Scalar = 5mm
    constraint thickness > 2mm where active
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 2);

    let constraint = match &members[1] {
        MemberDecl::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    let wc = constraint
        .where_clause
        .as_ref()
        .expect("expected where_clause");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "active"),
        other => panic!("expected Ident('active'), got {:?}", other),
    }
}

/// Parse `sub hole = Hole(diameter: 6mm) where needs_holes` — expects SubDecl with WhereClause.
#[test]
fn parse_sub_with_where_clause() {
    let source = r#"structure S {
    sub hole = Hole(diameter: 6mm) where needs_holes
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let sub = match &members[0] {
        MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };
    assert_eq!(sub.name, "hole");

    let wc = sub.where_clause.as_ref().expect("expected where_clause");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "needs_holes"),
        other => panic!("expected Ident('needs_holes'), got {:?}", other),
    }
}

// ── Step 5: guarded block (basic) ────────────────────────────────

/// Parse `where needs_cooling { param fan_size : Length = 50mm }`.
#[test]
fn parse_guarded_block_basic() {
    let source = r#"structure S {
    where needs_cooling {
        param fan_size : Scalar = 50mm
    }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let group = match &members[0] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected GuardedGroup, got {:?}", other),
    };

    match &group.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "needs_cooling"),
        other => panic!("expected Ident('needs_cooling'), got {:?}", other),
    }
    assert_eq!(group.members.len(), 1);
    assert!(group.else_members.is_empty());

    match &group.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "fan_size"),
        other => panic!("expected Param 'fan_size', got {:?}", other),
    }
}

// ── Step 7: guarded block with else ──────────────────────────────

/// Parse `where cond { param a : Real = 1mm } else { param b : Real = 2mm }`.
#[test]
fn parse_guarded_block_with_else() {
    let source = r#"structure S {
    where cond {
        param a : Scalar = 1mm
    } else {
        param b : Scalar = 2mm
    }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let group = match &members[0] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected GuardedGroup, got {:?}", other),
    };

    match &group.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "cond"),
        other => panic!("expected Ident('cond'), got {:?}", other),
    }

    assert_eq!(group.members.len(), 1);
    match &group.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "a"),
        other => panic!("expected Param 'a', got {:?}", other),
    }

    assert_eq!(group.else_members.len(), 1);
    match &group.else_members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "b"),
        other => panic!("expected Param 'b', got {:?}", other),
    }
}

// ── Step 9: nested guards ────────────────────────────────────────

/// Parse `where a { where b { param x : Real } }` — nested GuardedGroupDecl.
#[test]
fn parse_nested_guards() {
    let source = r#"structure S {
    where a {
        where b {
            param x : Scalar = 1mm
        }
    }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let outer = match &members[0] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected outer GuardedGroup, got {:?}", other),
    };
    match &outer.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "a"),
        other => panic!("expected Ident('a'), got {:?}", other),
    }

    assert_eq!(outer.members.len(), 1);
    let inner = match &outer.members[0] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected inner GuardedGroup, got {:?}", other),
    };
    match &inner.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "b"),
        other => panic!("expected Ident('b'), got {:?}", other),
    }

    assert_eq!(inner.members.len(), 1);
    match &inner.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "x"),
        other => panic!("expected Param 'x', got {:?}", other),
    }
}

// ── Step 11: complex guard expressions ───────────────────────────

/// Parse param with complex where condition: `x > 5mm && active`.
#[test]
fn parse_complex_guard_expression() {
    let source = r#"structure S {
    param fan : Scalar = 50mm where x > 5mm && active
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let param = match &members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };

    let wc = param.where_clause.as_ref().expect("expected where_clause");
    // Should be BinOp(&&, BinOp(>, Ident(x), QuantityLiteral(5mm)), Ident(active))
    match &wc.condition.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "&&");
            // left: x > 5mm
            match &left.kind {
                ExprKind::BinOp { op, .. } => assert_eq!(op, ">"),
                other => panic!("expected BinOp(>), got {:?}", other),
            }
            // right: active
            match &right.kind {
                ExprKind::Ident(name) => assert_eq!(name, "active"),
                other => panic!("expected Ident('active'), got {:?}", other),
            }
        }
        other => panic!("expected BinOp(&&), got {:?}", other),
    }
}

/// Parse block guard with complex expression.
#[test]
fn parse_guarded_block_complex_expression() {
    let source = r#"structure S {
    where needs_cooling || override_flag {
        param fan : Scalar = 50mm
    }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let group = match &members[0] {
        MemberDecl::GuardedGroup(g) => g,
        other => panic!("expected GuardedGroup, got {:?}", other),
    };

    match &group.condition.kind {
        ExprKind::BinOp { op, .. } => assert_eq!(op, "||"),
        other => panic!("expected BinOp(||), got {:?}", other),
    }
}

// ── Step 13: backward compatibility ──────────────────────────────

/// Bracket source with no guards: all where_clause fields are None, member counts unchanged.
#[test]
fn bracket_backward_compat_no_guards() {
    let source = reify_test_support::bracket_source();
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // Member count unchanged
    assert_eq!(structure.members.len(), 10);

    let params: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Param(_)))
        .collect();
    let lets: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Let(_)))
        .collect();
    let constraints: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Constraint(_)))
        .collect();

    assert_eq!(params.len(), 5, "expected 5 params");
    assert_eq!(lets.len(), 2, "expected 2 lets");
    assert_eq!(constraints.len(), 3, "expected 3 constraints");

    // All where_clause fields are None
    for (i, member) in structure.members.iter().enumerate() {
        match member {
            MemberDecl::Param(p) => {
                assert!(
                    p.where_clause.is_none(),
                    "param {} ({}) should have no where_clause",
                    i,
                    p.name
                );
            }
            MemberDecl::Let(l) => {
                assert!(
                    l.where_clause.is_none(),
                    "let {} ({}) should have no where_clause",
                    i,
                    l.name
                );
            }
            MemberDecl::Constraint(c) => {
                assert!(
                    c.where_clause.is_none(),
                    "constraint {} should have no where_clause",
                    i
                );
            }
            _ => {}
        }
    }

    // Content hashes unchanged (source didn't change)
    let module2 = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    let s2 = match &module2.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!(),
    };

    assert_eq!(structure.content_hash, s2.content_hash);
    for (m1, m2) in structure.members.iter().zip(s2.members.iter()) {
        let h1 = match m1 {
            MemberDecl::Param(p) => p.content_hash,
            MemberDecl::Let(l) => l.content_hash,
            MemberDecl::Constraint(c) => c.content_hash,
            MemberDecl::ConstraintInst(ci) => ci.content_hash,
            MemberDecl::Sub(s) => s.content_hash,
            MemberDecl::Minimize(m) => m.content_hash,
            MemberDecl::Maximize(m) => m.content_hash,
            MemberDecl::GuardedGroup(g) => g.content_hash,
            MemberDecl::AssociatedType(a) => a.content_hash,
            MemberDecl::Port(p) => p.content_hash,
            MemberDecl::Connect(c) => c.content_hash,
            MemberDecl::Chain(c) => c.content_hash,
            MemberDecl::MetaBlock(m) => m.content_hash,
            MemberDecl::ForallConnect(f) => f.content_hash,
            MemberDecl::ForallConstraint(f) => f.content_hash,
            // Not produced by the tree-sitter parser yet (task 2372).
            MemberDecl::MatchArmDeclGroup(g) => g.content_hash,
            // Produced by lower_function (task 3937); fn members have a content_hash.
            MemberDecl::Fn(f) => f.content_hash,
        };
        let h2 = match m2 {
            MemberDecl::Param(p) => p.content_hash,
            MemberDecl::Let(l) => l.content_hash,
            MemberDecl::Constraint(c) => c.content_hash,
            MemberDecl::ConstraintInst(ci) => ci.content_hash,
            MemberDecl::Sub(s) => s.content_hash,
            MemberDecl::Minimize(m) => m.content_hash,
            MemberDecl::Maximize(m) => m.content_hash,
            MemberDecl::GuardedGroup(g) => g.content_hash,
            MemberDecl::AssociatedType(a) => a.content_hash,
            MemberDecl::Port(p) => p.content_hash,
            MemberDecl::Connect(c) => c.content_hash,
            MemberDecl::Chain(c) => c.content_hash,
            MemberDecl::MetaBlock(m) => m.content_hash,
            MemberDecl::ForallConnect(f) => f.content_hash,
            MemberDecl::ForallConstraint(f) => f.content_hash,
            // Not produced by the tree-sitter parser yet (task 2372).
            MemberDecl::MatchArmDeclGroup(g) => g.content_hash,
            // Produced by lower_function (task 3937); fn members have a content_hash.
            MemberDecl::Fn(f) => f.content_hash,
        };
        assert_eq!(h1, h2);
    }
}
