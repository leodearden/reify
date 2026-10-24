use super::*;

/// Helper: count ERROR nodes in a tree-sitter tree.
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = if node.is_error() || node.is_missing() {
        1
    } else {
        0
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_errors(child);
    }
    count
}

fn parse_bracket() -> ParsedModule {
    let source = reify_test_support::bracket_source();
    parse(source, ModulePath::single("bracket"))
}

#[test]
fn ts_parse_produces_correct_structure() {
    let module = parse_bracket();
    assert!(
        module.errors.is_empty(),
        "expected no errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "Bracket");
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

    // Verify member names in order
    let names: Vec<String> = structure
        .members
        .iter()
        .map(|m| match m {
            MemberDecl::Param(p) => format!("param:{}", p.name),
            MemberDecl::Let(l) => format!("let:{}", l.name),
            MemberDecl::Constraint(_) => "constraint".into(),
            MemberDecl::ConstraintInst(ci) => format!("constraint_inst:{}", ci.name),
            MemberDecl::Sub(s) => format!("sub:{}", s.name),
            MemberDecl::Minimize(_) => "minimize".into(),
            MemberDecl::Maximize(_) => "maximize".into(),
            MemberDecl::GuardedGroup(_) => "guarded_group".into(),
            MemberDecl::AssociatedType(a) => format!("type:{}", a.name),
            MemberDecl::Port(p) => format!("port:{}", p.name),
            MemberDecl::Connect(_) => "connect".into(),
            MemberDecl::Chain(_) => "chain".into(),
            MemberDecl::MetaBlock(_) => "meta".into(),
            MemberDecl::ForallConnect(_) => "forall_connect".into(),
            MemberDecl::ForallConstraint(_) => "forall_constraint".into(),
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(_) => "match_arm_decl_group".into(),
            MemberDecl::Relate(_) => "relate".into(),
            MemberDecl::Sketch(sk) => format!("sketch:{}", sk.name),
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => format!("fn:{}", f.name),
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "param:width",
            "param:height",
            "param:thickness",
            "param:fillet_radius",
            "param:hole_diameter",
            "let:volume",
            "constraint",
            "constraint",
            "constraint",
            "let:body",
        ]
    );
}

/// Helper to get structure members from bracket parse.
fn bracket_members() -> Vec<MemberDecl> {
    let module = parse_bracket();
    match module.declarations.into_iter().next().unwrap() {
        Declaration::Structure(s) => s.members,
        _ => panic!("expected Structure"),
    }
}

#[test]
fn quantity_literal_80mm() {
    let members = bracket_members();
    let width = match &members[0] {
        MemberDecl::Param(p) => p,
        _ => panic!("expected Param"),
    };
    assert_eq!(width.name, "width");
    match &width.default.as_ref().unwrap().kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert!((value - 80.0).abs() < f64::EPSILON);
            assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
        }
        other => panic!("expected QuantityLiteral, got {:?}", other),
    }
}

#[test]
fn number_literal_4() {
    // In `constraint thickness < width / 4`, the `4` is a number literal
    let members = bracket_members();
    // constraints[1] is `constraint thickness < width / 4`
    let constraint = match &members[7] {
        MemberDecl::Constraint(c) => c,
        _ => panic!("expected Constraint"),
    };
    // expr is `thickness < width / 4`
    match &constraint.expr.kind {
        ExprKind::BinOp { right, .. } => {
            // right is `width / 4`
            match &right.kind {
                ExprKind::BinOp {
                    right: inner_right, ..
                } => match &inner_right.kind {
                    ExprKind::NumberLiteral { value: v, .. } => {
                        assert!((v - 4.0).abs() < f64::EPSILON);
                    }
                    other => panic!("expected NumberLiteral(4), got {:?}", other),
                },
                other => panic!("expected BinOp, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

#[test]
fn function_call_box() {
    let members = bracket_members();
    // Last member: `let body = box(width, height, thickness)`
    let body = match &members[9] {
        MemberDecl::Let(l) => l,
        _ => panic!("expected Let"),
    };
    assert_eq!(body.name, "body");
    match &body.value.kind {
        ExprKind::FunctionCall { name, args, .. } => {
            assert_eq!(name, "box");
            assert_eq!(args.len(), 3);
            assert!(matches!(&args[0].kind, ExprKind::Ident(n) if n == "width"));
            assert!(matches!(&args[1].kind, ExprKind::Ident(n) if n == "height"));
            assert!(matches!(&args[2].kind, ExprKind::Ident(n) if n == "thickness"));
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

#[test]
fn binary_ops_left_associative() {
    let members = bracket_members();
    // `let volume = width * height * thickness`
    let volume = match &members[5] {
        MemberDecl::Let(l) => l,
        _ => panic!("expected Let"),
    };
    assert_eq!(volume.name, "volume");
    // Should be ((width * height) * thickness)
    match &volume.value.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*");
            // right is "thickness"
            assert!(matches!(&right.kind, ExprKind::Ident(n) if n == "thickness"));
            // left is (width * height)
            match &left.kind {
                ExprKind::BinOp {
                    op: inner_op,
                    left: ll,
                    right: lr,
                } => {
                    assert_eq!(inner_op, "*");
                    assert!(matches!(&ll.kind, ExprKind::Ident(n) if n == "width"));
                    assert!(matches!(&lr.kind, ExprKind::Ident(n) if n == "height"));
                }
                other => panic!("expected inner BinOp, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

#[test]
fn comparison_with_quantity() {
    let members = bracket_members();
    // `constraint thickness > 2mm`
    let constraint = match &members[6] {
        MemberDecl::Constraint(c) => c,
        _ => panic!("expected Constraint"),
    };
    match &constraint.expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, ">");
            assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "thickness"));
            match &right.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 2.0).abs() < f64::EPSILON);
                    assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
                }
                other => panic!("expected QuantityLiteral, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

#[test]
fn spans_are_valid_and_cover_source_text() {
    let source = reify_test_support::bracket_source();
    let module = parse(source, ModulePath::single("bracket"));

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Structure spans entire source
    assert_eq!(structure.span.start, 0);
    assert_eq!(structure.span.end as usize, source.len());

    // All member spans are non-empty, within source, and contain expected keywords
    for (i, m) in structure.members.iter().enumerate() {
        let span = match m {
            MemberDecl::Param(p) => p.span,
            MemberDecl::Let(l) => l.span,
            MemberDecl::Constraint(c) => c.span,
            MemberDecl::ConstraintInst(ci) => ci.span,
            MemberDecl::Sub(s) => s.span,
            MemberDecl::Minimize(m) => m.span,
            MemberDecl::Maximize(m) => m.span,
            MemberDecl::GuardedGroup(g) => g.span,
            MemberDecl::AssociatedType(a) => a.span,
            MemberDecl::Port(p) => p.span,
            MemberDecl::Connect(c) => c.span,
            MemberDecl::Chain(c) => c.span,
            MemberDecl::MetaBlock(m) => m.span,
            MemberDecl::ForallConnect(f) => f.span,
            MemberDecl::ForallConstraint(f) => f.span,
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(g) => g.span,
            MemberDecl::Relate(r) => r.span,
            MemberDecl::Sketch(sk) => sk.span,
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => f.span,
        };
        assert!(span.start < span.end, "member {} span empty", i);
        assert!(
            (span.end as usize) <= source.len(),
            "member {} span overflows",
            i
        );

        let text = &source[span.start as usize..span.end as usize];
        match m {
            MemberDecl::Param(p) => {
                assert!(
                    text.starts_with("param"),
                    "param member {} text: {:?}",
                    i,
                    text
                );
                assert!(text.contains(&p.name), "param {} name in text", i);
            }
            MemberDecl::Let(l) => {
                assert!(text.starts_with("let"), "let member {} text: {:?}", i, text);
                assert!(text.contains(&l.name), "let {} name in text", i);
            }
            MemberDecl::Constraint(_) => {
                assert!(
                    text.starts_with("constraint"),
                    "constraint member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::Sub(s) => {
                assert!(text.starts_with("sub"), "sub member {} text: {:?}", i, text);
                assert!(text.contains(&s.name), "sub {} name in text", i);
            }
            MemberDecl::Minimize(_) => {
                assert!(
                    text.starts_with("minimize"),
                    "minimize member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::Maximize(_) => {
                assert!(
                    text.starts_with("maximize"),
                    "maximize member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::GuardedGroup(_) => {
                assert!(
                    text.starts_with("where"),
                    "guarded_group member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::AssociatedType(a) => {
                assert!(
                    text.starts_with("type"),
                    "associated_type member {} text: {:?}",
                    i,
                    text
                );
                assert!(text.contains(&a.name), "associated_type {} name in text", i);
            }
            MemberDecl::Port(p) => {
                assert!(
                    text.starts_with("port"),
                    "port member {} text: {:?}",
                    i,
                    text
                );
                assert!(text.contains(&p.name), "port {} name in text", i);
            }
            MemberDecl::Connect(_) => {
                assert!(
                    text.starts_with("connect"),
                    "connect member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::Chain(_) => {
                assert!(
                    text.starts_with("chain"),
                    "chain member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::MetaBlock(_) => {
                assert!(
                    text.starts_with("meta"),
                    "meta member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::ConstraintInst(ci) => {
                assert!(
                    text.starts_with("constraint"),
                    "constraint_inst member {} text: {:?}",
                    i,
                    text
                );
                assert!(
                    text.contains(&ci.name),
                    "constraint_inst {} name in text",
                    i
                );
            }
            MemberDecl::ForallConnect(_) => {
                assert!(
                    text.starts_with("forall"),
                    "forall_connect member {} text: {:?}",
                    i,
                    text
                );
            }
            MemberDecl::ForallConstraint(_) => {
                assert!(
                    text.starts_with("forall"),
                    "forall_constraint member {} text: {:?}",
                    i,
                    text
                );
            }
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(_) => {}
            MemberDecl::Relate(_) => {}
            MemberDecl::Sketch(_) => {}
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => {
                assert!(
                    text.starts_with("fn"),
                    "fn member {} text: {:?}",
                    i,
                    text
                );
                assert!(text.contains(&f.name), "fn {} name in text", i);
            }
        }
    }

    // Expression spans are valid
    if let MemberDecl::Param(p) = &structure.members[0] {
        let def_span = p.default.as_ref().unwrap().span;
        let def_text = &source[def_span.start as usize..def_span.end as usize];
        assert_eq!(def_text, "80mm", "width default text");

        let ty_span = p.type_expr.as_ref().unwrap().span;
        let ty_text = &source[ty_span.start as usize..ty_span.end as usize];
        assert_eq!(ty_text, "Length", "width type text");
    }
}

#[test]
fn content_hashes_computed_from_source_text() {
    let source = reify_test_support::bracket_source();
    let module = parse(source, ModulePath::single("bracket"));

    // Module content hash = hash of entire source
    assert_eq!(
        module.content_hash,
        ContentHash::of_str(source),
        "module hash"
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Structure content hash = hash of structure node's source text (not entire file)
    assert_ne!(
        structure.content_hash,
        ContentHash(0),
        "structure hash should be non-zero"
    );

    // Each member content hash = hash of its source text slice
    for (i, m) in structure.members.iter().enumerate() {
        let (span, hash) = match m {
            MemberDecl::Param(p) => (p.span, p.content_hash),
            MemberDecl::Let(l) => (l.span, l.content_hash),
            MemberDecl::Constraint(c) => (c.span, c.content_hash),
            MemberDecl::ConstraintInst(ci) => (ci.span, ci.content_hash),
            MemberDecl::Sub(s) => (s.span, s.content_hash),
            MemberDecl::Minimize(m) => (m.span, m.content_hash),
            MemberDecl::Maximize(m) => (m.span, m.content_hash),
            MemberDecl::GuardedGroup(g) => (g.span, g.content_hash),
            MemberDecl::AssociatedType(a) => (a.span, a.content_hash),
            MemberDecl::Port(p) => (p.span, p.content_hash),
            MemberDecl::Connect(c) => (c.span, c.content_hash),
            MemberDecl::Chain(c) => (c.span, c.content_hash),
            MemberDecl::MetaBlock(m) => (m.span, m.content_hash),
            MemberDecl::ForallConnect(f) => (f.span, f.content_hash),
            MemberDecl::ForallConstraint(f) => (f.span, f.content_hash),
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(g) => (g.span, g.content_hash),
            MemberDecl::Relate(r) => (r.span, r.content_hash),
            MemberDecl::Sketch(sk) => (sk.span, sk.content_hash),
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => (f.span, f.content_hash),
        };
        let text = &source[span.start as usize..span.end as usize];
        assert_eq!(
            hash,
            ContentHash::of_str(text),
            "member {} hash from source text",
            i
        );
    }

    // All param hashes should be unique
    let param_hashes: Vec<ContentHash> = structure
        .members
        .iter()
        .filter_map(|m| match m {
            MemberDecl::Param(p) => Some(p.content_hash),
            _ => None,
        })
        .collect();
    for (i, h1) in param_hashes.iter().enumerate() {
        for (j, h2) in param_hashes.iter().enumerate() {
            if i != j {
                assert_ne!(h1, h2, "params {} and {} have same hash", i, j);
            }
        }
    }
}

#[test]
fn error_recovery_partial_parse() {
    let source = r#"structure Broken {
    param width: Length = 80mm
    param !!!invalid!!!
    param height: Length = 100mm
}"#;
    let module = parse(source, ModulePath::single("broken"));

    // Should have parse errors
    assert!(
        !module.errors.is_empty(),
        "expected errors for malformed input"
    );

    // Should also have recovered declarations
    assert!(
        !module.declarations.is_empty(),
        "expected partial declarations"
    );

    if let Declaration::Structure(s) = &module.declarations[0] {
        assert_eq!(s.name, "Broken");
        // Should have at least some valid members (width and/or height)
        let valid_params: Vec<_> = s
            .members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Param(p) => Some(&p.name),
                _ => None,
            })
            .collect();
        assert!(
            !valid_params.is_empty(),
            "expected at least some valid params, got none"
        );
    } else {
        panic!("expected Structure declaration");
    }
}

#[test]
fn parse_deterministic() {
    // Parsing the same source twice produces identical output.
    let source = reify_test_support::bracket_source();
    let m1 = parse(source, ModulePath::single("bracket"));
    let m2 = parse(source, ModulePath::single("bracket"));

    assert_eq!(m1.content_hash, m2.content_hash);
    assert_eq!(m1.declarations.len(), m2.declarations.len());
    assert_eq!(m1.errors.len(), m2.errors.len());

    let s1 = match &m1.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!(),
    };
    let s2 = match &m2.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!(),
    };

    assert_eq!(s1.name, s2.name);
    assert_eq!(s1.span, s2.span);
    assert_eq!(s1.content_hash, s2.content_hash);
    assert_eq!(s1.members.len(), s2.members.len());

    for (i, (m_a, m_b)) in s1.members.iter().zip(s2.members.iter()).enumerate() {
        let (hash_a, span_a) = match m_a {
            MemberDecl::Param(p) => (p.content_hash, p.span),
            MemberDecl::Let(l) => (l.content_hash, l.span),
            MemberDecl::Constraint(c) => (c.content_hash, c.span),
            MemberDecl::ConstraintInst(ci) => (ci.content_hash, ci.span),
            MemberDecl::Sub(s) => (s.content_hash, s.span),
            MemberDecl::Minimize(m) => (m.content_hash, m.span),
            MemberDecl::Maximize(m) => (m.content_hash, m.span),
            MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
            MemberDecl::AssociatedType(a) => (a.content_hash, a.span),
            MemberDecl::Port(p) => (p.content_hash, p.span),
            MemberDecl::Connect(c) => (c.content_hash, c.span),
            MemberDecl::Chain(c) => (c.content_hash, c.span),
            MemberDecl::MetaBlock(m) => (m.content_hash, m.span),
            MemberDecl::ForallConnect(f) => (f.content_hash, f.span),
            MemberDecl::ForallConstraint(f) => (f.content_hash, f.span),
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(g) => (g.content_hash, g.span),
            MemberDecl::Relate(r) => (r.content_hash, r.span),
            MemberDecl::Sketch(sk) => (sk.content_hash, sk.span),
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => (f.content_hash, f.span),
        };
        let (hash_b, span_b) = match m_b {
            MemberDecl::Param(p) => (p.content_hash, p.span),
            MemberDecl::Let(l) => (l.content_hash, l.span),
            MemberDecl::Constraint(c) => (c.content_hash, c.span),
            MemberDecl::ConstraintInst(ci) => (ci.content_hash, ci.span),
            MemberDecl::Sub(s) => (s.content_hash, s.span),
            MemberDecl::Minimize(m) => (m.content_hash, m.span),
            MemberDecl::Maximize(m) => (m.content_hash, m.span),
            MemberDecl::GuardedGroup(g) => (g.content_hash, g.span),
            MemberDecl::AssociatedType(a) => (a.content_hash, a.span),
            MemberDecl::Port(p) => (p.content_hash, p.span),
            MemberDecl::Connect(c) => (c.content_hash, c.span),
            MemberDecl::Chain(c) => (c.content_hash, c.span),
            MemberDecl::MetaBlock(m) => (m.content_hash, m.span),
            MemberDecl::ForallConnect(f) => (f.content_hash, f.span),
            MemberDecl::ForallConstraint(f) => (f.content_hash, f.span),
            // Produced by the tree-sitter parser via lower_match_arm_decl_group (task 3564).
            MemberDecl::MatchArmDeclGroup(g) => (g.content_hash, g.span),
            MemberDecl::Relate(r) => (r.content_hash, r.span),
            MemberDecl::Sketch(sk) => (sk.content_hash, sk.span),
            // Produced by lower_function (task 3937).
            MemberDecl::Fn(f) => (f.content_hash, f.span),
        };
        assert_eq!(hash_a, hash_b, "member {} hash determinism", i);
        assert_eq!(span_a, span_b, "member {} span determinism", i);
    }
}

#[test]
fn parse_minimize_declaration() {
    let source = r#"structure S {
    param volume: Length = 100mm
    minimize volume
}"#;
    let module = parse(source, ModulePath::single("test_min"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // Should have 2 members: param + minimize
    assert_eq!(structure.members.len(), 2);

    match &structure.members[1] {
        MemberDecl::Minimize(m) => {
            assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "volume"));
        }
        other => panic!("expected Minimize, got {:?}", other),
    }
}

#[test]
fn parse_maximize_declaration() {
    let source = r#"structure S {
    param thickness: Length = 5mm
    maximize thickness
}"#;
    let module = parse(source, ModulePath::single("test_max"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 2);

    match &structure.members[1] {
        MemberDecl::Maximize(m) => {
            assert!(matches!(&m.expr.kind, ExprKind::Ident(name) if name == "thickness"));
        }
        other => panic!("expected Maximize, got {:?}", other),
    }
}

#[test]
fn parse_minimize_complex_expression() {
    let source = r#"structure S {
    param width: Length = 80mm
    param height: Length = 100mm
    minimize width * height
}"#;
    let module = parse(source, ModulePath::single("test_min_complex"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    match &structure.members[2] {
        MemberDecl::Minimize(m) => match &m.expr.kind {
            ExprKind::BinOp { op, .. } => assert_eq!(op, "*"),
            other => panic!("expected BinOp(*), got {:?}", other),
        },
        other => panic!("expected Minimize, got {:?}", other),
    }
}

#[test]
fn parse_minimize_with_other_members() {
    let source = r#"structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    let vol = w * h
    constraint w > 0mm
    minimize w
}"#;
    let module = parse(source, ModulePath::single("test_min_mixed"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // 2 params + 1 let + 1 constraint + 1 minimize = 5 members
    assert_eq!(structure.members.len(), 5);

    // Verify minimize is present alongside other members
    assert!(
        structure
            .members
            .iter()
            .any(|m| matches!(m, MemberDecl::Minimize(_))),
        "should contain a Minimize member"
    );
    assert!(
        structure
            .members
            .iter()
            .any(|m| matches!(m, MemberDecl::Constraint(_))),
        "should contain a Constraint member"
    );
}

#[test]
fn minimize_span_and_hash() {
    let source = r#"structure S {
    param x: Length = 5mm
    minimize x
}"#;
    let module = parse(source, ModulePath::single("test_min_span"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    match &structure.members[1] {
        MemberDecl::Minimize(m) => {
            // Span should cover the full "minimize x" text
            let text = &source[m.span.start as usize..m.span.end as usize];
            assert!(text.starts_with("minimize"), "span text: {:?}", text);
            assert!(
                text.contains("x"),
                "span text should contain 'x': {:?}",
                text
            );

            // Content hash should match the source text of the node
            assert_eq!(
                m.content_hash,
                ContentHash::of_str(text),
                "content_hash should match source text"
            );
        }
        other => panic!("expected Minimize, got {:?}", other),
    }
}

#[test]
fn parse_enum_declaration() {
    let source = "enum Direction { In, Out, Bidi }\nstructure S { param x: Length = 5mm }";
    let module = parse(source, ModulePath::single("test_enum"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 2);

    match &module.declarations[0] {
        Declaration::Enum(e) => {
            assert_eq!(e.name, "Direction");
            let variant_names: Vec<&str> =
                e.variants.iter().map(|v| v.name.as_str()).collect();
            assert_eq!(variant_names, vec!["In", "Out", "Bidi"]);
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

#[test]
fn parse_enum_access_expression() {
    let source = "enum Direction { In, Out, Bidi }\nstructure S { let d = Direction.In }";
    let module = parse(source, ModulePath::single("test_enum_access"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let structure = module
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Structure(s) => Some(s),
            _ => None,
        })
        .expect("expected a structure");

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "d");
    match &let_decl.value.kind {
        ExprKind::EnumAccess { type_name, variant } => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected EnumAccess, got {:?}", other),
    }
}

#[test]
fn parse_enum_missing_name_is_error() {
    let source = "enum { }";
    let module = parse(source, ModulePath::single("test_enum_err"));
    assert!(
        !module.errors.is_empty(),
        "expected parse errors for malformed enum"
    );
}

// ── parse_with_prelude_enums (task 2525) ────────────────────────────────

/// Helper: locate the first `EnumAccess` expression in a parsed module's
/// structure declarations.  Returns the matched `(type_name, variant)`
/// pair, or `None` if no `EnumAccess` is present.
///
/// Visits members in declaration order via `visit_structure_member_root_exprs`,
/// which yields `Param` defaults before `Let` values within the same structure.
/// A `Param` default carrying an `EnumAccess` will therefore be returned if it
/// appears before any `Let` with an `EnumAccess`.
fn find_first_enum_access(module: &ParsedModule) -> Option<(String, String)> {
    let mut result = None;
    crate::visit_structure_member_root_exprs(module, |expr| {
        if result.is_none()
            && let ExprKind::EnumAccess { type_name, variant } = &expr.kind
        {
            result = Some((type_name.clone(), variant.clone()));
        }
    });
    result
}

/// (a) When `parse_with_prelude_enums` is given an enum name that is NOT
/// declared in the source, `Foo.Bar` must lower to `EnumAccess { type_name: "Foo", variant: "Bar" }`
/// rather than `MemberAccess { object: Ident("Foo"), member: "Bar" }`.
/// This is the core behavior change motivated by task 2525: prelude enums
/// must participate in EnumAccess disambiguation.
#[test]
fn parse_with_prelude_enums_resolves_prelude_only_enum() {
    let source = "structure S { let v = Foo.Bar }";
    let module = parse_with_prelude_enums(
        source,
        ModulePath::single("test_prelude_enum"),
        &["Foo"],
    );
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let (type_name, variant) = find_first_enum_access(&module)
        .expect("expected at least one EnumAccess in parsed module");
    assert_eq!(type_name, "Foo");
    assert_eq!(variant, "Bar");
}

/// (b) When the same enum name appears in BOTH `prelude_enum_names` and
/// the source's own `enum_declaration`, no parse error fires (the parser
/// does not policed prelude/source name overlap), and the disambiguation
/// still resolves `Foo.Bar` to `EnumAccess`.  This pins the contract that
/// duplicate-prelude/source enum names are tolerated at parse time and
/// left to downstream name resolution to handle.
#[test]
fn parse_with_prelude_enums_dedupes_overlap_with_source_enum() {
    let source = "enum Foo { Bar, Baz }\nstructure S { let v = Foo.Bar }";
    let module = parse_with_prelude_enums(
        source,
        ModulePath::single("test_prelude_overlap"),
        &["Foo"],
    );
    assert!(
        module.errors.is_empty(),
        "parse errors should be empty even when prelude and source share an enum name: {:?}",
        module.errors
    );

    let (type_name, variant) = find_first_enum_access(&module)
        .expect("expected at least one EnumAccess in parsed module");
    assert_eq!(type_name, "Foo");
    assert_eq!(variant, "Bar");
}

/// (c) `parse_with_prelude_enums(source, path, &[])` must be
/// observationally equivalent to `parse(source, path)`.  This is a
/// regression guard that pins the empty-prelude case so the wrapper never
/// drifts away from the unparameterized `parse` behavior.
#[test]
fn parse_with_prelude_enums_empty_slice_equivalent_to_parse() {
    let source = "enum Direction { In, Out, Bidi }\nstructure S { let d = Direction.In }";
    let path = ModulePath::single("test_empty_prelude");

    let from_parse = parse(source, path.clone());
    let from_prelude = parse_with_prelude_enums(source, path, &[]);

    // Same parse-error count and same content_hash captures observational
    // equivalence at the `ParsedModule` level.
    assert_eq!(
        from_parse.errors.len(),
        from_prelude.errors.len(),
        "empty-slice prelude must produce the same parse error count as parse()"
    );
    assert_eq!(
        from_parse.content_hash, from_prelude.content_hash,
        "empty-slice prelude must produce the same content_hash as parse()"
    );
    assert_eq!(
        from_parse.declarations.len(),
        from_prelude.declarations.len(),
        "empty-slice prelude must produce the same declaration count as parse()"
    );

    // Both must locate the same `Direction.In` EnumAccess.
    let from_parse_access = find_first_enum_access(&from_parse).expect("parse() EnumAccess");
    let from_prelude_access =
        find_first_enum_access(&from_prelude).expect("parse_with_prelude_enums() EnumAccess");
    assert_eq!(from_parse_access, from_prelude_access);
}

/// (d) Regression guard for the `HashSet<&'a str>` borrow-through contract
/// (task 2558).  Pins two invariants:
///
/// 1. Functional correctness when the same `static` prelude slice is reused
///    across two consecutive `parse_with_prelude_enums` calls
///    (lifetime-mixing regression: both calls must resolve correctly without
///    interference from a prior call's internal state).
/// 2. Mixed-source resolution: in the second call a source-declared enum
///    (`SourceEnum`) and a prelude-supplied enum (`PreludeEnumB`) must BOTH
///    lower to `EnumAccess` in the same parse.
///
/// Note: the API accepts `&[&'a str]` (source-lifetime bound, task 4108);
/// non-`'static` borrowed names are accepted and covered by
/// `parse_with_prelude_enums_accepts_non_static_borrowed_names`.  The
/// no-allocation guarantee is a manual profiling check (per task description),
/// not encoded here.
#[test]
fn parse_with_prelude_enums_borrows_static_names_across_calls() {
    static PRELUDE: &[&str] = &["PreludeEnumA", "PreludeEnumB"];

    // First call — prelude-only enum (no source enum declarations).
    let source1 = "structure S1 { let v = PreludeEnumA.X }";
    let module1 = parse_with_prelude_enums(
        source1,
        ModulePath::single("test_borrow_call1"),
        PRELUDE,
    );
    assert!(
        module1.errors.is_empty(),
        "call 1 parse errors: {:?}",
        module1.errors
    );
    let (type1, variant1) =
        find_first_enum_access(&module1).expect("call 1: expected EnumAccess");
    assert_eq!(type1, "PreludeEnumA");
    assert_eq!(variant1, "X");

    // Second call — source-declared enum + prelude enum, same PRELUDE slice.
    // Both PreludeEnumB.Z and SourceEnum.Y must resolve to EnumAccess.
    let source2 =
        "enum SourceEnum { Y }\nstructure S2 { let v = PreludeEnumB.Z\n let w = SourceEnum.Y }";
    let module2 = parse_with_prelude_enums(
        source2,
        ModulePath::single("test_borrow_call2"),
        PRELUDE,
    );
    assert!(
        module2.errors.is_empty(),
        "call 2 parse errors: {:?}",
        module2.errors
    );

    // Collect all EnumAccess root-expr values from S2 via the shared visitor.
    let mut accesses: Vec<(String, String)> = Vec::new();
    crate::visit_structure_member_root_exprs(&module2, |expr| {
        if let ExprKind::EnumAccess { type_name, variant } = &expr.kind {
            accesses.push((type_name.clone(), variant.clone()));
        }
    });
    assert!(
        accesses.contains(&("PreludeEnumB".to_string(), "Z".to_string())),
        "expected PreludeEnumB.Z → EnumAccess; got: {:?}",
        accesses
    );
    assert!(
        accesses.contains(&("SourceEnum".to_string(), "Y".to_string())),
        "expected SourceEnum.Y → EnumAccess; got: {:?}",
        accesses
    );
}

/// (e) Compile-level regression guard for the lifetime relaxation (task 4108).
/// Proves that `parse_with_prelude_enums` accepts a name slice whose
/// elements are borrowed from a non-`'static` local allocation.
///
/// Under the OLD `&[&'static str]` bound this test DOES NOT COMPILE:
/// the compiler rejects `&names` because `names: Vec<&str>` borrows from
/// `owned: Vec<String>` — a local, non-`'static` value.  After step-2
/// relaxes the bound to `&[&'a str]` the test compiles and passes.
///
/// Runtime behavior (EnumAccess disambiguation for a non-`'static` name)
/// is already covered by `parse_with_prelude_enums_resolves_prelude_only_enum`
/// with the same source pattern; this test's sole new capability under test
/// is accepting a non-`'static` borrow.
#[test]
fn parse_with_prelude_enums_accepts_non_static_borrowed_names() {
    let owned: Vec<String> = vec!["Foo".to_string()];
    let names: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    let module = parse_with_prelude_enums(
        "structure S { let v = Foo.Bar }",
        ModulePath::single("test_nonstatic"),
        &names,
    );
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    let (type_name, variant) =
        find_first_enum_access(&module).expect("expected EnumAccess from non-static names");
    assert_eq!(type_name, "Foo");
    assert_eq!(variant, "Bar");
}

#[test]
fn tree_sitter_parses_bracket_source_without_errors() {
    let source = reify_test_support::bracket_source();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");

    let tree = parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    assert_eq!(root.kind(), "source_file");
    assert_eq!(
        count_errors(root),
        0,
        "Expected zero ERROR nodes, got tree:\n{}",
        root.to_sexp()
    );
}

// ── Collection literal tests ──────────────────────────

/// Helper: parse a source string wrapping an expression in a structure let,
/// and return the ExprKind of the let's value.
fn parse_let_expr(source: &str) -> ExprKind {
    let module = parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    let_decl.value.kind.clone()
}

#[test]
fn parse_list_literal_three_elements() {
    let kind = parse_let_expr("structure S { let x = [1, 2, 3] }");
    match kind {
        ExprKind::ListLiteral(elems) => {
            assert_eq!(elems.len(), 3);
            assert!(
                matches!(&elems[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&elems[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&elems[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
            );
        }
        other => panic!("expected ListLiteral, got {:?}", other),
    }
}

#[test]
fn parse_list_literal_empty() {
    let kind = parse_let_expr("structure S { let x = [] }");
    match kind {
        ExprKind::ListLiteral(elems) => {
            assert_eq!(elems.len(), 0);
        }
        other => panic!("expected ListLiteral, got {:?}", other),
    }
}

#[test]
fn parse_set_literal_three_elements() {
    let kind = parse_let_expr("structure S { let x = set{1, 2, 3} }");
    match kind {
        ExprKind::SetLiteral(elems) => {
            assert_eq!(elems.len(), 3);
            assert!(
                matches!(&elems[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&elems[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&elems[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
            );
        }
        other => panic!("expected SetLiteral, got {:?}", other),
    }
}

#[test]
fn parse_set_literal_empty() {
    let kind = parse_let_expr("structure S { let x = set{} }");
    match kind {
        ExprKind::SetLiteral(elems) => {
            assert_eq!(elems.len(), 0);
        }
        other => panic!("expected SetLiteral, got {:?}", other),
    }
}

#[test]
fn parse_map_literal_two_entries() {
    let kind = parse_let_expr(r#"structure S { let x = map{"a" => 1, "b" => 2} }"#);
    match kind {
        ExprKind::MapLiteral(entries) => {
            assert_eq!(entries.len(), 2);
            assert!(matches!(&entries[0].0.kind, ExprKind::StringLiteral(s) if s == "a"));
            assert!(
                matches!(&entries[0].1.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
            );
            assert!(matches!(&entries[1].0.kind, ExprKind::StringLiteral(s) if s == "b"));
            assert!(
                matches!(&entries[1].1.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
            );
        }
        other => panic!("expected MapLiteral, got {:?}", other),
    }
}

#[test]
fn parse_map_literal_empty() {
    let kind = parse_let_expr("structure S { let x = map{} }");
    match kind {
        ExprKind::MapLiteral(entries) => {
            assert_eq!(entries.len(), 0);
        }
        other => panic!("expected MapLiteral, got {:?}", other),
    }
}

#[test]
fn parse_index_access_number() {
    let kind = parse_let_expr("structure S { let x = items[0] }");
    match kind {
        ExprKind::IndexAccess { object, index } => {
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "items"));
            assert!(
                matches!(&index.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 0.0).abs() < f64::EPSILON)
            );
        }
        other => panic!("expected IndexAccess, got {:?}", other),
    }
}

#[test]
fn parse_index_access_string_key() {
    let kind = parse_let_expr(r#"structure S { let x = m["key"] }"#);
    match kind {
        ExprKind::IndexAccess { object, index } => {
            assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "m"));
            assert!(matches!(&index.kind, ExprKind::StringLiteral(s) if s == "key"));
        }
        other => panic!("expected IndexAccess, got {:?}", other),
    }
}

#[test]
fn parse_nested_list_literals() {
    let kind = parse_let_expr("structure S { let x = [[1, 2], [3, 4]] }");
    match kind {
        ExprKind::ListLiteral(outer) => {
            assert_eq!(outer.len(), 2);
            match &outer[0].kind {
                ExprKind::ListLiteral(inner) => {
                    assert_eq!(inner.len(), 2);
                    assert!(
                        matches!(&inner[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
                    );
                    assert!(
                        matches!(&inner[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                    );
                }
                other => panic!("expected inner ListLiteral, got {:?}", other),
            }
            match &outer[1].kind {
                ExprKind::ListLiteral(inner) => {
                    assert_eq!(inner.len(), 2);
                    assert!(
                        matches!(&inner[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
                    );
                    assert!(
                        matches!(&inner[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 4.0).abs() < f64::EPSILON)
                    );
                }
                other => panic!("expected inner ListLiteral, got {:?}", other),
            }
        }
        other => panic!("expected outer ListLiteral, got {:?}", other),
    }
}

#[test]
fn parse_collection_in_let_context() {
    let source = "structure S { let x = [1, 2, 3] }";
    let module = parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(structure.members.len(), 1);
    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "x");
    assert!(matches!(&let_decl.value.kind, ExprKind::ListLiteral(elems) if elems.len() == 3));
}

#[test]
fn parse_collections_no_regression_on_bracket() {
    let module = parse_bracket();
    assert!(
        module.errors.is_empty(),
        "expected no errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(structure.name, "Bracket");
    assert_eq!(
        structure.members.len(),
        10,
        "expected 10 members (5 params, 2 lets, 3 constraints)"
    );
    let params = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Param(_)))
        .count();
    let lets = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Let(_)))
        .count();
    let constraints = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Constraint(_)))
        .count();
    assert_eq!(params, 5, "expected 5 params");
    assert_eq!(lets, 2, "expected 2 lets");
    assert_eq!(constraints, 3, "expected 3 constraints");
}

// ── Function definition tests ─────────────────────────────────

#[test]
fn parse_simple_function_definition() {
    let source = "fn area(w: Length, h: Length) -> Length { w * h }";
    let module = parse(source, ModulePath::single("test_fn"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let f = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert_eq!(f.name, "area");
    assert!(!f.is_pub);
    assert_eq!(f.params.len(), 2);
    assert_eq!(f.params[0].name, "w");
    assert!(
        matches!(&f.params[0].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Length")
    );
    assert_eq!(f.params[1].name, "h");
    assert!(
        matches!(&f.params[1].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Length")
    );
    assert!(f.return_type.is_some());
    assert!(
        matches!(&f.return_type.as_ref().unwrap().kind, TypeExprKind::Named { name, .. } if name == "Length")
    );
    assert!(f.body.as_ref().unwrap().let_bindings.is_empty());
    assert!(matches!(&f.body.as_ref().unwrap().result_expr.kind, ExprKind::BinOp { op, .. } if op == "*"));
}

#[test]
fn parse_pub_function_with_conditional() {
    let source = "pub fn clamp(x: Real, lo: Real, hi: Real) -> Real { if x < lo then lo else if x > hi then hi else x }";
    let module = parse(source, ModulePath::single("test_pub_fn"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let f = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert!(f.is_pub);
    assert_eq!(f.name, "clamp");
    assert_eq!(f.params.len(), 3);
    assert_eq!(f.params[0].name, "x");
    assert!(
        matches!(&f.params[0].type_expr.kind, TypeExprKind::Named { name, .. } if name == "Real")
    );
    assert_eq!(f.params[1].name, "lo");
    assert_eq!(f.params[2].name, "hi");
    assert!(f.return_type.is_some());
    assert!(
        matches!(&f.return_type.as_ref().unwrap().kind, TypeExprKind::Named { name, .. } if name == "Real")
    );
    assert!(matches!(
        &f.body.as_ref().unwrap().result_expr.kind,
        ExprKind::Conditional { .. }
    ));
}

#[test]
fn parse_function_with_let_bindings() {
    let source = "fn f(x: Real) -> Real { let y = x * 2; y + 1 }";
    let module = parse(source, ModulePath::single("test_fn_let"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let f = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.body.as_ref().unwrap().let_bindings.len(), 1);
    assert_eq!(f.body.as_ref().unwrap().let_bindings[0].name, "y");
    assert!(
        matches!(&f.body.as_ref().unwrap().let_bindings[0].value.kind, ExprKind::BinOp { op, .. } if op == "*")
    );
    assert!(matches!(&f.body.as_ref().unwrap().result_expr.kind, ExprKind::BinOp { op, .. } if op == "+"));
}

#[test]
fn parse_function_with_type_parameters() {
    let source = "fn identity<T>(x: T) -> T { x }";
    let module = parse(source, ModulePath::single("test_fn_tp"));
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let f = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert_eq!(f.type_params.len(), 1);
    assert_eq!(f.type_params[0].name, "T");
    assert!(f.type_params[0].bounds.is_empty());

    // Also test with bounds
    let source2 = "fn add<T: Numeric>(a: T, b: T) -> T { a + b }";
    let module2 = parse(source2, ModulePath::single("test_fn_tp2"));
    assert!(
        module2.errors.is_empty(),
        "parse errors: {:?}",
        module2.errors
    );

    let f2 = match &module2.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert_eq!(f2.type_params.len(), 1);
    assert_eq!(f2.type_params[0].name, "T");
    assert_eq!(f2.type_params[0].bounds, vec!["Numeric"]);
}

// ── Ad-hoc selector tests ─────────────────────────────

#[test]
fn parse_ad_hoc_selector_basic() {
    let kind = parse_let_expr(r#"structure S { let x = port @ face("top") }"#);
    match kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
            assert_eq!(selector, "face");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "top"));
        }
        other => panic!("expected AdHocSelector, got {:?}", other),
    }
}

#[test]
fn parse_ad_hoc_selector_no_args() {
    let kind = parse_let_expr("structure S { let x = port @ default() }");
    match kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
            assert_eq!(selector, "default");
            assert_eq!(args.len(), 0);
        }
        other => panic!("expected AdHocSelector, got {:?}", other),
    }
}

#[test]
fn parse_ad_hoc_selector_multiple_args() {
    let kind = parse_let_expr("structure S { let x = port @ point(1, 2, 3) }");
    match kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(base.kind, ExprKind::Ident(ref n) if n == "port"));
            assert_eq!(selector, "point");
            assert_eq!(args.len(), 3);
            assert!(
                matches!(&args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 1.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&args[1].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
            );
            assert!(
                matches!(&args[2].kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 3.0).abs() < f64::EPSILON)
            );
        }
        other => panic!("expected AdHocSelector, got {:?}", other),
    }
}

// ── lower_connect_body direct tests ─────────────────────────────
//
// These tests call lower_connect_body directly, bypassing the
// check_and_lower! guard that normally preempts body-level
// diagnostics when has_error() propagates to the connect_statement.

/// Helper: parse source with tree-sitter and find the first node of a given kind.
fn find_node_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_node_by_kind(child, kind) {
            return Some(found);
        }
    }
    None
}

/// Generic helper: parse source, find the first node of `kind`, run `lower_fn`
/// on it via a fresh `Lowering`, and return collected errors.
///
/// The closure pattern avoids lifetime issues: `tree_sitter::Node` borrows from
/// `Tree`, so both must live inside the same scope — the closure receives them
/// without the caller needing to hold the `Tree`.
fn lower_node_directly<F>(source: &str, kind: &str, lower_fn: F) -> Vec<ParseError>
where
    F: FnOnce(&mut Lowering, tree_sitter::Node),
{
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "source '{}' should parse without errors — grammar regression?",
        source,
    );

    let node = find_node_by_kind(root, kind)
        .unwrap_or_else(|| panic!("no {kind} node found in parse tree"));

    let mut lowering = Lowering::new(source);
    lower_fn(&mut lowering, node);
    lowering.errors.into_inner()
}

/// Like `lower_node_directly`, but skips the clean-parse assertion.
/// Use for tests that deliberately feed malformed source to exercise
/// error-handling code paths.
fn lower_node_with_errors<F>(source: &str, kind: &str, lower_fn: F) -> Vec<ParseError>
where
    F: FnOnce(&mut Lowering, tree_sitter::Node),
{
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    let node = find_node_by_kind(root, kind)
        .unwrap_or_else(|| panic!("no {kind} node found in parse tree"));

    let mut lowering = Lowering::new(source);
    lower_fn(&mut lowering, node);
    lowering.errors.into_inner()
}

/// Helper: parse source, find the connect_body node, call lower_connect_body
/// directly (bypassing check_and_lower!), and return the errors.
fn lower_body_directly(source: &str) -> Vec<ParseError> {
    lower_node_directly(source, "connect_body", |l, n| {
        l.lower_connect_body(n);
    })
}

/// Like `lower_body_directly`, but skips the clean-parse assertion.
fn lower_body_with_errors(source: &str) -> Vec<ParseError> {
    lower_node_with_errors(source, "connect_body", |l, n| {
        l.lower_connect_body(n);
    })
}

#[test]
#[should_panic(expected = "should parse without errors")]
fn lower_node_directly_rejects_source_with_parse_errors() {
    // Deliberately broken source: `{ >= }` produces parse errors.
    // lower_node_directly should panic because root.has_error() is true.
    lower_body_directly("structure S { port a : out T  port b : in T  connect a -> b { >= } }");
}

#[test]
fn lower_connect_body_error_node_emits_diagnostic() {
    // `{ >= ) <= ( }` across two lines produces a multi-line ERROR child inside
    // connect_body. When lower_connect_body is called directly, the ERROR arm fires and
    // reports a one-line excerpt located at the ERROR's start (INV-SF-7, task #6156).
    // NOTE: we use `: BoltSet` to specify a connector_type before the brace
    // block, making `{` unambiguously the start of connect_body.  Without
    // the connector_type, the new variant_construction GLR fork (task α,
    // data-carrying-enums) keeps both a variant_construction fork and the
    // connect_body fork alive after `b {`; even though `>=` immediately
    // kills the variant_construction fork, GLR error recovery may orphan
    // `{ … }` as a member-level ERROR node rather than a connect_body,
    // causing `find_node_by_kind("connect_body")` to fail.  The connector
    // type `: BoltSet` consumes the `b :` prefix so the `{` is unambiguous.
    let source = "structure S {\n  port a : out T\n  port b : in T\n  connect a -> b : BoltSet {\n    >= )\n    <= (\n  }\n}\n";
    let errors = lower_body_with_errors(source);
    assert_eq!(errors.len(), 1, "expected one diagnostic, got: {errors:?}");
    assert_eq!(errors[0].message, "syntax error in connect body: >= )…");
    assert_eq!(errors[0].span.start as usize, source.find(">= )").unwrap());
}

#[test]
fn lower_connect_body_malformed_param_emits_diagnostic() {
    // `{ grade = }` produces a connect_param_assignment with has_error().
    // When lower_connect_body is called directly, the has_error() guard fires.
    let errors = lower_body_with_errors(
        "structure S { port a : out T  port b : in T  connect a -> b : BoltSet { grade = } }",
    );
    assert!(
        !errors.is_empty(),
        "expected body-level diagnostic for malformed param, got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("connect parameter")),
        "expected error mentioning 'connect parameter', got: {:?}",
        errors
    );
}

#[test]
fn lower_connect_body_malformed_mapping_emits_diagnostic() {
    // `{ shaft -> }` produces a port_mapping with has_error().
    // When lower_connect_body is called directly, the has_error() guard fires.
    let errors = lower_body_with_errors(
        "structure S { port a : out T  port b : in T  connect a -> b { shaft -> } }",
    );
    assert!(
        !errors.is_empty(),
        "expected body-level diagnostic for malformed mapping, got none"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("port mapping")),
        "expected error mentioning 'port mapping', got: {:?}",
        errors
    );
}

#[test]
fn lower_connect_body_extras_not_flagged() {
    // Comments are tree-sitter extras — they must NOT trigger the catch-all
    // diagnostic. The source is syntactically valid, so zero errors is the
    // correct assertion (not just "no 'unexpected' errors").
    let errors = lower_body_directly(
        "structure S { port a : out T  port b : in T  connect a -> b { /* comment */ grade = 8.8 }  }",
    );
    assert!(
        errors.is_empty(),
        "expected no errors for syntactically valid connect body with comment, got: {:?}",
        errors
    );
}

#[test]
fn lower_connect_body_anonymous_tokens_not_flagged() {
    // An empty connect body `{ }` has only anonymous tokens (braces).
    // The named-children iteration must skip them without producing errors.
    let errors = lower_body_directly(
        "structure S { port a : out T  port b : in T  connect a -> b { } }",
    );
    assert!(
        errors.is_empty(),
        "expected no errors for empty connect body (anonymous tokens only), got: {:?}",
        errors
    );
}

/// Deliberately passes a `constraint_definition` node to `lower_connect_body`
/// to exercise the catch-all branch. The constraint_definition has 3 named
/// children (identifier, param_declaration, constraint_def_predicate), none of
/// which match any connect_body arm — so the catch-all should fire for each.
#[test]
fn lower_connect_body_catch_all_emits_for_unexpected_named_children() {
    let source = "constraint def Eq { param x: Length  x > 0 }";
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "source should parse without errors — grammar regression?"
    );

    let Some(constraint_node) = find_node_by_kind(root, "constraint_definition") else {
        panic!("no constraint_definition node found in parse tree — grammar regression?");
    };

    let mut lowering = Lowering::new(source);
    lowering.lower_connect_body(constraint_node);
    let errors = lowering.errors.borrow();
    assert!(
        errors.len() >= 3,
        "expected at least 3 diagnostics (one per named child: identifier, \
         param_declaration, constraint_def_predicate), got {}: {:?}",
        errors.len(),
        errors
    );
    assert!(
        errors.iter().any(|e| e.message.contains("unexpected")),
        "expected at least one error containing 'unexpected', got: {:?}",
        errors
    );
}

// ── Port body defensive catch-all tests ────────────────────

/// Helper: parse source, find the port_body node, call lower_port_body
/// directly (bypassing check_and_lower!), and return the errors.
fn lower_port_body_directly(source: &str) -> Vec<ParseError> {
    lower_node_directly(source, "port_body", |l, n| {
        l.lower_port_body(n);
    })
}

/// Like `lower_port_body_directly`, but skips the clean-parse assertion.
fn lower_port_body_with_errors(source: &str) -> Vec<ParseError> {
    lower_node_with_errors(source, "port_body", |l, n| {
        l.lower_port_body(n);
    })
}

#[test]
fn lower_port_body_error_node_emits_diagnostic() {
    // `{ >= ) <= ( }` across two lines produces a multi-line ERROR child inside
    // port_body. When lower_port_body is called directly, the ERROR arm fires and
    // reports a one-line excerpt located at the ERROR's start (INV-SF-7, task #6156).
    let source = "structure S {\n  port a : in T {\n    >= )\n    <= (\n  }\n}\n";
    let errors = lower_port_body_with_errors(source);
    assert_eq!(errors.len(), 1, "expected one diagnostic, got: {errors:?}");
    assert_eq!(errors[0].message, "syntax error in port body: >= )…");
    assert_eq!(errors[0].span.start as usize, source.find(">= )").unwrap());
}

#[test]
fn lower_port_body_catch_all_emits_for_unexpected_named_children() {
    // Pass a constraint_definition node to lower_port_body. Its named
    // children (identifier, param_declaration, constraint_def_predicate)
    // don't match any port_body arm and should hit the catch-all.
    let source = "constraint def Eq { param x: Length  x > 0 }";
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "source should parse without errors — grammar regression?"
    );

    let Some(constraint_node) = find_node_by_kind(root, "constraint_definition") else {
        panic!("no constraint_definition node found in parse tree — grammar regression?");
    };

    let mut lowering = Lowering::new(source);
    lowering.lower_port_body(constraint_node);
    let errors = lowering.errors.borrow();
    assert!(
        errors.len() >= 2,
        "expected at least 2 diagnostics (identifier and constraint_def_predicate \
         are unexpected in port body; param_declaration is handled), got {}: {:?}",
        errors.len(),
        errors
    );
    assert!(
        errors.iter().any(|e| e.message.contains("unexpected")),
        "expected at least one error containing 'unexpected', got: {:?}",
        errors
    );
}

#[test]
fn lower_port_body_extras_not_flagged() {
    // Comments are tree-sitter extras — they must NOT trigger the catch-all
    // diagnostic. The source is syntactically valid, so zero errors is the
    // correct assertion (not just "no 'unexpected' errors").
    let errors = lower_port_body_directly(
        "structure S { port a : in T { /* comment */ param x: Length = 1 } }",
    );
    assert!(
        errors.is_empty(),
        "expected no errors for syntactically valid port body with comment, got: {:?}",
        errors
    );
}

// ── Guarded block ERROR arm ────────────────────────────────

/// A guarded block is a member list, so its `ERROR` arm takes the member-list policy
/// (`diagnose_error_node`): no source echo, located at the fault (INV-SF-7, task #6156).
///
/// Only a direct call reaches this arm. Through `parse`, `lower_member` refuses a faulty
/// `guarded_block` via `check_and_lower!` before `lower_guarded_block` ever runs.
#[test]
fn lower_guarded_block_error_node_emits_diagnostic() {
    let source = "structure S {\n  param x: Real = 1\n  where x > 0 {\n    let a = 1\n    ) (\n      ] [\n    let b = 2\n  }\n}\n";
    let errors = lower_node_with_errors(source, "guarded_block", |l, n| {
        l.lower_guarded_block(n);
    });
    assert_eq!(errors.len(), 1, "expected one diagnostic, got: {errors:?}");
    assert_eq!(errors[0].message, "syntax error in guarded block");
    assert_eq!(errors[0].span.start as usize, source.find(") (").unwrap());
}

// ── Constraint def defensive catch-all tests ───────────────

/// Helper: parse source, find the constraint_definition node, call
/// lower_constraint_def directly, and return the errors.
fn lower_constraint_def_directly(source: &str) -> Vec<ParseError> {
    lower_node_directly(source, "constraint_definition", |l, n| {
        l.lower_constraint_def(n);
    })
}

#[test]
fn lower_constraint_def_catch_all_emits_for_unexpected_named_children() {
    // Pass a structure_definition node to lower_constraint_def. Its named
    // children (sub_declaration, port_declaration, connect_declaration)
    // don't match constraint_def arms and should hit the catch-all.
    // We use structure_definition because it has a "name" field (required
    // by lower_constraint_def) and body children outside constraint scope.
    let source = "structure S { port a : in T { param x: Length = 1 }  sub b = T() }";
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "source should parse without errors — grammar regression?"
    );

    let Some(struct_node) = find_node_by_kind(root, "structure_definition") else {
        panic!("no structure_definition node found in parse tree — grammar regression?");
    };

    let mut lowering = Lowering::new(source);
    lowering.lower_constraint_def(struct_node);
    let errors = lowering.errors.borrow();
    assert!(
        errors.len() >= 2,
        "expected at least 2 diagnostics (port_declaration, sub_declaration \
         at minimum), got {}: {:?}",
        errors.len(),
        errors
    );
    assert!(
        errors.iter().any(|e| e.message.contains("unexpected")),
        "expected at least one error containing 'unexpected', got: {:?}",
        errors
    );
}

#[test]
fn lower_constraint_def_extras_not_flagged() {
    // Comments are tree-sitter extras — they must NOT trigger the catch-all
    // diagnostic. The source is syntactically valid, so zero errors is the
    // correct assertion (not just "no 'unexpected' errors").
    let errors = lower_constraint_def_directly(
        "constraint def Eq { /* comment */ param x: Length  x > 0 }",
    );
    assert!(
        errors.is_empty(),
        "expected no errors for syntactically valid constraint def with comment, got: {:?}",
        errors
    );
}

// ── Source file defensive catch-all tests ──────────────────

#[test]
fn lower_source_file_catch_all_emits_for_unexpected_named_children() {
    // Pass a structure_definition node to lower_source_file. Its named
    // children (identifier, param_declaration, port_declaration, etc.)
    // don't match any top-level declaration kind and should hit the catch-all.
    let source = "structure S { param x: Length = 1  port a : in T { param y: Length = 2 } }";
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    let struct_node = find_node_by_kind(root, "structure_definition")
        .expect("no structure_definition node found in parse tree");

    let mut lowering = Lowering::new(source);
    lowering.lower_source_file(struct_node);
    let errors = lowering.errors.borrow();
    assert!(
        !errors.is_empty(),
        "expected diagnostics for unexpected named children in source file catch-all, got none"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("unexpected")),
        "expected at least one error containing 'unexpected', got: {:?}",
        errors
    );
}

#[test]
fn lower_source_file_extras_not_flagged() {
    // Comments are tree-sitter extras — they must NOT trigger the catch-all
    // diagnostic. Verify that a source file with a block comment before a
    // valid structure produces no errors mentioning "unexpected".
    let source = "/* comment */\nstructure S { param x: Length = 1 }";
    let module = parse(source, ModulePath::single("test"));
    assert!(
        !module
            .errors
            .iter()
            .any(|e| e.message.contains("unexpected")),
        "expected no 'unexpected' errors for comment extras, got: {:?}",
        module.errors
    );
}

// ── Doc comment extraction tests ─────────────────────────

#[test]
fn doc_comment_on_structure_is_extracted() {
    let src = "/// A bracket for mounting.\nstructure Bracket {\n  param w: Length = 1\n}";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got: {other:?}"),
    };
    assert_eq!(decl.doc.as_deref(), Some("A bracket for mounting."));
}

#[test]
fn multi_line_doc_comment_joined() {
    let src = "/// Line one.\n/// Line two.\nstructure S {\n  param x: Length = 1\n}";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got: {other:?}"),
    };
    assert_eq!(decl.doc.as_deref(), Some("Line one.\nLine two."));
}

#[test]
fn no_doc_comment_yields_none() {
    let src = "structure S {\n  param x: Length = 1\n}";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got: {other:?}"),
    };
    assert!(decl.doc.is_none());
}

#[test]
fn regular_comment_not_treated_as_doc() {
    let src = "// Just a comment\nstructure S {\n  param x: Length = 1\n}";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got: {other:?}"),
    };
    assert!(
        decl.doc.is_none(),
        "regular // comment should not be a doc comment"
    );
}

#[test]
fn doc_comment_on_fn_is_extracted() {
    let src = "/// Compute area.\nfn area(w: Length, h: Length) -> Length { w * h }";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got: {other:?}"),
    };
    assert_eq!(decl.doc.as_deref(), Some("Compute area."));
}

#[test]
fn doc_comment_on_enum_is_extracted() {
    let src = "/// Direction enum.\nenum Dir { In, Out }";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Enum(e) => e,
        other => panic!("expected Enum, got: {other:?}"),
    };
    assert_eq!(decl.doc.as_deref(), Some("Direction enum."));
}

#[test]
fn doc_comment_on_trait_is_extracted() {
    let src = "/// A rigid body.\ntrait Rigid {\n  param mass: Length\n}";
    let module = parse(src, ModulePath::single("test"));
    let decl = match &module.declarations[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got: {other:?}"),
    };
    assert_eq!(decl.doc.as_deref(), Some("A rigid body."));
}

// PRD v0.6 D5: single-sided range lowering (task 3914 / ζ).
// Grammar (task 3911) names the prefix fields `op` and `bound`.
// Discriminator: two-sided has `lower`/`upper` fields; single-sided has `op`/`bound` fields.
#[test]
fn single_sided_range_gt_lower_exclusive() {
    // `>2mm` => Range { lower: Some(2mm), upper: None, lower_inclusive: false, upper_inclusive: true }
    let kind = parse_let_expr("structure S { let r = >2mm }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(upper.is_none(), "upper should be None for `>2mm`");
            assert!(!lower_inclusive, "lower should be exclusive for `>`");
            assert!(upper_inclusive, "absent upper_inclusive should be vacuous true");
            let lower_expr = lower.expect("lower should be Some for `>2mm`");
            match lower_expr.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 2.0).abs() < f64::EPSILON);
                    assert_eq!(unit, UnitExpr::Unit("mm".to_string()));
                }
                other => panic!("expected QuantityLiteral for bound, got {:?}", other),
            }
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

#[test]
fn single_sided_range_gte_lower_inclusive() {
    // `>=2mm` => Range { lower: Some(2mm), upper: None, lower_inclusive: true, upper_inclusive: true }
    let kind = parse_let_expr("structure S { let r = >=2mm }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(upper.is_none(), "upper should be None for `>=2mm`");
            assert!(lower_inclusive, "lower should be inclusive for `>=`");
            assert!(upper_inclusive, "absent upper_inclusive should be vacuous true");
            let lower_expr = lower.expect("lower should be Some for `>=2mm`");
            match lower_expr.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 2.0).abs() < f64::EPSILON);
                    assert_eq!(unit, UnitExpr::Unit("mm".to_string()));
                }
                other => panic!("expected QuantityLiteral for bound, got {:?}", other),
            }
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

#[test]
fn single_sided_range_lt_upper_exclusive() {
    // `<100MPa` => Range { lower: None, upper: Some(100MPa), lower_inclusive: true, upper_inclusive: false }
    let kind = parse_let_expr("structure S { let r = <100MPa }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower.is_none(), "lower should be None for `<100MPa`");
            assert!(lower_inclusive, "absent lower_inclusive should be vacuous true");
            assert!(!upper_inclusive, "upper should be exclusive for `<`");
            let upper_expr = upper.expect("upper should be Some for `<100MPa`");
            match upper_expr.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 100.0).abs() < f64::EPSILON);
                    assert_eq!(unit, UnitExpr::Unit("MPa".to_string()));
                }
                other => panic!("expected QuantityLiteral for bound, got {:?}", other),
            }
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

#[test]
fn single_sided_range_lte_upper_inclusive() {
    // `<=100MPa` => Range { lower: None, upper: Some(100MPa), lower_inclusive: true, upper_inclusive: true }
    let kind = parse_let_expr("structure S { let r = <=100MPa }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower.is_none(), "lower should be None for `<=100MPa`");
            assert!(lower_inclusive, "absent lower_inclusive should be vacuous true");
            assert!(upper_inclusive, "upper should be inclusive for `<=`");
            let upper_expr = upper.expect("upper should be Some for `<=100MPa`");
            match upper_expr.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 100.0).abs() < f64::EPSILON);
                    assert_eq!(unit, UnitExpr::Unit("MPa".to_string()));
                }
                other => panic!("expected QuantityLiteral for bound, got {:?}", other),
            }
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

#[test]
fn two_sided_range_inclusive_regression() {
    // `2mm..10mm` => Range { lower: Some, upper: Some, lower_inclusive: true, upper_inclusive: true }
    // Guards that the existing two-sided path is not broken by the single-sided branch.
    let kind = parse_let_expr("structure S { let r = 2mm..10mm }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower.is_some(), "lower should be Some for two-sided range");
            assert!(upper.is_some(), "upper should be Some for two-sided range");
            assert!(lower_inclusive, "lower should be inclusive for `..`");
            assert!(upper_inclusive, "upper should be inclusive for `..`");
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

#[test]
fn two_sided_range_exclusive_upper_regression() {
    // `0mm..<10mm` => Range { lower: Some, upper: Some, lower_inclusive: true, upper_inclusive: false }
    // Guards that the `..<` exclusive-upper detection loop is not broken.
    let kind = parse_let_expr("structure S { let r = 0mm..<10mm }");
    match kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower.is_some(), "lower should be Some for two-sided range");
            assert!(upper.is_some(), "upper should be Some for two-sided range");
            assert!(lower_inclusive, "lower should be inclusive for `..<`");
            assert!(!upper_inclusive, "upper should be exclusive for `..<`");
        }
        other => panic!("expected ExprKind::Range, got {:?}", other),
    }
}

// ── Unit tests for decode_string_escapes (suggestion 2 coverage) ──────────

/// An unrecognized escape sequence (e.g. `\r`, `\0`) drops the backslash
/// and keeps only the character.  This pins the "lenient" behavior
/// documented in the `decode_string_escapes` doc-comment.
///
/// Concretely: `\X` where X is not `n`, `t`, `\\`, or `"` → emit X,
/// drop the backslash.  This is data-lossy but intentional for α.
#[test]
fn decode_string_escapes_unknown_escape_drops_backslash() {
    // `\r` → 'r' (backslash dropped, character kept)
    assert_eq!(decode_string_escapes("x\\ry"), "xry");
    // `\0` → '0'
    assert_eq!(decode_string_escapes("\\0z"), "0z");
    // `\s` → 's'
    assert_eq!(decode_string_escapes("a\\sb"), "asb");
}

/// A lone `\` at the very end of a chunk (no following character) is
/// silently dropped — the `None => {}` branch in `decode_string_escapes`.
///
/// This is reachable on highly-malformed input (e.g. the external scanner
/// consumed a lone `\` at EOF inside a string literal).  Pinning the
/// behavior here prevents a silent regression if the semantics ever change.
#[test]
fn decode_string_escapes_trailing_backslash_is_silently_dropped() {
    // A chunk ending with a lone backslash: the backslash disappears.
    assert_eq!(decode_string_escapes("x\\"), "x");
    assert_eq!(decode_string_escapes("\\"), "");
}

// ── Unit test for lower_interpolated_string robustness (suggestion 3) ─────

/// Directly exercises `lower_interpolated_string` with a malformed empty
/// hole `{}` to verify the function-level robustness fix, *bypassing*
/// `check_and_lower!` (which fires at the `let_declaration` level and
/// prevents the function from being called in the full-pipeline path).
///
/// Verifies that:
/// 1. The function returns `Some(...)` — the string is NOT silently dropped.
/// 2. A diagnostic is emitted for the MISSING-expr hole.
/// 3. The surviving literal parts remain in the result.
#[test]
fn lower_interpolated_string_malformed_hole_produces_diagnostic() {
    let source = r#"structure S { let v = "x {} y" }"#;
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("Error loading Reify grammar");
    let tree = ts_parser.parse(source, None).expect("Failed to parse");
    let root = tree.root_node();

    // Find the interpolated_string node (has_error due to the empty hole).
    let interp_node = find_node_by_kind(root, "interpolated_string")
        .expect("interpolated_string node must be present in CST");
    assert!(
        interp_node.has_error(),
        "the interpolated_string node must have has_error=true for this test to be meaningful"
    );

    // Call lower_interpolated_string directly, bypassing check_and_lower!
    let lowering = Lowering::new(source);
    let result = lowering.lower_interpolated_string(interp_node);
    let errors = lowering.errors.into_inner();

    // The string must NOT be silently dropped (Some returned, not None).
    let expr = result
        .expect("lower_interpolated_string must return Some even for a malformed hole");

    // At least one diagnostic for the bad hole.
    assert!(
        !errors.is_empty(),
        "expected at least one diagnostic for MISSING-expr hole, got none"
    );

    // The surrounding literal chunks survive; the bad hole is skipped.
    match &expr.kind {
        ExprKind::InterpolatedString(parts) => {
            let literal_count = parts
                .iter()
                .filter(|p| matches!(p, StringPart::Literal(_)))
                .count();
            assert!(
                literal_count >= 2,
                "expected at least 2 surviving literal parts, got: {:?}",
                parts
            );
        }
        other => panic!("expected InterpolatedString, got {:?}", other),
    }
}

/// Robustness guard: `qualified_type_recovery_base` must return a flat,
/// bounded `Named` over the whole-node text — NOT dispatch back through
/// `lower_type_expr_node` — so that the error-recovery (`None`) arm of
/// `lower_qualified_type` does not recurse into itself and overflow the
/// stack in release builds.
///
/// The source is valid (the `qualified_type` node is real); we pass that
/// real node to `qualified_type_recovery_base` to exercise the helper on
/// a concrete CST node without needing an unreachable baseless node.
#[test]
fn qualified_type_recovery_base_is_bounded_named() {
    let source = "structure def S { param m : Coupling<Prismatic>::MotionValue }";

    // Parse with the raw tree-sitter API to get the CST directly.
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_reify::language().into())
        .expect("set_language failed");
    let tree = ts_parser
        .parse(source, None)
        .expect("parse returned None");
    let root = tree.root_node();

    // Walk the CST to locate the qualified_type node (reuses the helper
    // already defined in this test module at line 5764).
    let qnode = find_node_by_kind(root, "qualified_type")
        .expect("expected a qualified_type node in the CST");

    // Build the lowering context.
    let lowering = Lowering::new(source);

    // Call the recovery helper directly.  Asserting a flat Named over the
    // whole-node text (not a QualifiedAssoc) is the discriminating signal
    // that the helper does NOT re-dispatch through lower_type_expr_node.
    let result = lowering.qualified_type_recovery_base(qnode);

    match result.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(
                name, "Coupling<Prismatic>::MotionValue",
                "recovery base must be the raw whole-node text"
            );
            assert!(
                type_args.is_empty(),
                "recovery base must have empty type_args (no parsing), got: {:?}",
                type_args
            );
        }
        other => panic!(
            "expected TypeExprKind::Named (bounded flat recovery), got {:?}",
            other
        ),
    }
}

// The three `snippet` tests below borrow `fault_diagnosis::with_root` rather than keeping
// a second parse-and-hand-me-the-root helper here; `snippet` itself is a `Lowering` method,
// so the tests stay with it.

/// `snippet` truncates on a CHARACTER boundary, never a byte one.
///
/// INV-SF-7, task #5392. The doc on `snippet` calls this out specifically: `&s[..40]` on a
/// non-ASCII string panics mid-codepoint, and the source is UTF-8, so a string literal or
/// comment full of multi-byte characters would abort the parse instead of describing it.
#[test]
fn snippet_truncates_long_multibyte_text_on_a_character_boundary() {
    // 45 two-byte characters — comfortably past the 40-char cap, and every candidate cut
    // point past index 0 is mid-codepoint under byte slicing.
    let long = "α".repeat(45);
    let source = format!("structure S {{\n  let a = \"{long}\"\n}}\n");

    fault_diagnosis::with_root(&source, |root| {
        let node = find_node_by_kind(root, "string_literal")
            .expect("fixture must contain a string_literal");
        let lowering = Lowering::new(&source);
        let snippet = lowering.snippet(node);

        assert!(
            snippet.ends_with('…'),
            "a truncated snippet must announce the truncation, got {snippet:?}",
        );
        assert_eq!(
            snippet.chars().count(),
            41,
            "40 characters plus the ellipsis, got {snippet:?}",
        );
        assert!(
            snippet.chars().filter(|c| *c == 'α').count() >= 39,
            "the truncated prefix should be the node's own text, got {snippet:?}",
        );
    });
}

/// The other truncation branch: a first line SHORTER than the cap, followed by more lines,
/// is still marked as truncated.
#[test]
fn snippet_marks_truncation_when_only_later_lines_are_dropped() {
    let source = "structure S {\n  let a = 1\n}\n";
    fault_diagnosis::with_root(source, |root| {
        let node = find_node_by_kind(root, "structure_definition")
            .expect("fixture must contain a structure_definition");
        let lowering = Lowering::new(source);
        let snippet = lowering.snippet(node);
        assert_eq!(
            snippet, "structure S {…",
            "a multi-line node must be cut at its first newline and marked",
        );
        assert!(!snippet.contains('\n'), "snippets are always one line");
    });
}

/// A single-line node shorter than the cap is reproduced verbatim, with no ellipsis.
#[test]
fn snippet_leaves_short_single_line_text_alone() {
    let source = "structure S {\n  let a = 1\n}\n";
    fault_diagnosis::with_root(source, |root| {
        let node = find_node_by_kind(root, "let_declaration")
            .expect("fixture must contain a let_declaration");
        let lowering = Lowering::new(source);
        assert_eq!(lowering.snippet(node), "let a = 1");
    });
}
