//! Qualified access expression tests.
//!
//! Tests for `TypeName::ident` (qualified trait access) and
//! `expr.(TypeName::ident)` (instance-level qualified trait access).

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("qualified_access_test"));
    (module.declarations, module.errors)
}

// ── Step 1: basic qualified access ────────────────────────────────────────
// ── Step 3: chained qualified access ──────────────────────────────────────

#[test]
fn parse_chained_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = A::B::c }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: QualifiedAccess { qualifier: QualifiedAccess { qualifier: Ident("A"), member: "B" }, member: "c" }
    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert_eq!(member, "c");
            match &qualifier.kind {
                ExprKind::QualifiedAccess { qualifier: inner_qualifier, member: inner_member } => {
                    assert_eq!(inner_member, "B");
                    match &inner_qualifier.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "A"),
                        other => panic!("expected Ident inner qualifier, got {:?}", other),
                    }
                }
                other => panic!("expected inner QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected outer QualifiedAccess, got {:?}", other),
    }
}

// ── Step 1: basic qualified access ────────────────────────────────

#[test]
fn parse_basic_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = Rigid::mass }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert_eq!(member, "mass");
            match &qualifier.kind {
                ExprKind::Ident(name) => assert_eq!(name, "Rigid"),
                other => panic!("expected Ident qualifier, got {:?}", other),
            }
        }
        other => panic!("expected QualifiedAccess, got {:?}", other),
    }
}

// ── Step 5: qualified access in binary expression ──────────────────────────

#[test]
fn parse_qualified_access_in_binary_expr() {
    let (decls, errors) = parse_decls("structure S { let x = Rigid::mass + 1 }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: BinOp { op: "+", left: QualifiedAccess(Ident("Rigid"), "mass"), right: NumberLiteral(1.0) }
    match &let_decl.value.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "+");
            match &left.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert_eq!(member, "mass");
                    match &qualifier.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "Rigid"),
                        other => panic!("expected Ident qualifier, got {:?}", other),
                    }
                }
                other => panic!("expected QualifiedAccess left operand, got {:?}", other),
            }
            match &right.kind {
                ExprKind::NumberLiteral(v) => assert!((v - 1.0).abs() < f64::EPSILON),
                other => panic!("expected NumberLiteral right operand, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

// ── Step 7: basic instance qualified access ────────────────────────────────

#[test]
fn parse_basic_instance_qualified_access() {
    let (decls, errors) = parse_decls("structure S { let x = motor.(Driveable::torque) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: InstanceQualifiedAccess { object: Ident("motor"), qualified: QualifiedAccess(Ident("Driveable"), "torque") }
    match &let_decl.value.kind {
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "motor"),
                other => panic!("expected Ident object, got {:?}", other),
            }
            match &qualified.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert_eq!(member, "torque");
                    match &qualifier.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "Driveable"),
                        other => panic!("expected Ident qualifier, got {:?}", other),
                    }
                }
                other => panic!("expected QualifiedAccess qualified, got {:?}", other),
            }
        }
        other => panic!("expected InstanceQualifiedAccess, got {:?}", other),
    }
}

// ── Step 9: instance qualified with member chain ───────────────────────────

#[test]
fn parse_instance_qualified_with_member_chain() {
    let (decls, errors) = parse_decls("structure S { let x = self.motor.(Driveable::torque) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: InstanceQualifiedAccess {
    //   object: MemberAccess { object: Ident("self"), member: "motor" },
    //   qualified: QualifiedAccess(Ident("Driveable"), "torque")
    // }
    match &let_decl.value.kind {
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            match &object.kind {
                ExprKind::MemberAccess { object: inner_obj, member } => {
                    assert_eq!(member, "motor");
                    match &inner_obj.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "self"),
                        other => panic!("expected Ident inner object, got {:?}", other),
                    }
                }
                other => panic!("expected MemberAccess object, got {:?}", other),
            }
            match &qualified.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert_eq!(member, "torque");
                    match &qualifier.kind {
                        ExprKind::Ident(name) => assert_eq!(name, "Driveable"),
                        other => panic!("expected Ident qualifier, got {:?}", other),
                    }
                }
                other => panic!("expected QualifiedAccess qualified, got {:?}", other),
            }
        }
        other => panic!("expected InstanceQualifiedAccess, got {:?}", other),
    }
}

// ── Step 11: instance qualified with chained path ─────────────────────────

#[test]
fn parse_instance_qualified_with_chained_path() {
    let (decls, errors) = parse_decls("structure S { let x = obj.(A::B::c) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Expected: InstanceQualifiedAccess {
    //   object: Ident("obj"),
    //   qualified: QualifiedAccess {
    //     qualifier: QualifiedAccess { qualifier: Ident("A"), member: "B" },
    //     member: "c"
    //   }
    // }
    match &let_decl.value.kind {
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            match &object.kind {
                ExprKind::Ident(name) => assert_eq!(name, "obj"),
                other => panic!("expected Ident object, got {:?}", other),
            }
            match &qualified.kind {
                ExprKind::QualifiedAccess { qualifier: outer_q, member: outer_m } => {
                    assert_eq!(outer_m, "c");
                    match &outer_q.kind {
                        ExprKind::QualifiedAccess { qualifier: inner_q, member: inner_m } => {
                            assert_eq!(inner_m, "B");
                            match &inner_q.kind {
                                ExprKind::Ident(name) => assert_eq!(name, "A"),
                                other => panic!("expected Ident inner qualifier, got {:?}", other),
                            }
                        }
                        other => panic!("expected inner QualifiedAccess, got {:?}", other),
                    }
                }
                other => panic!("expected outer QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!("expected InstanceQualifiedAccess, got {:?}", other),
    }
}

// ── Step 13: distinct AST nodes ───────────────────────────────────────────

#[test]
fn parse_distinct_ast_nodes() {
    let (decls, errors) = parse_decls(
        "structure S { let a = Rigid::mass  let b = obj.(Rigid::mass) }"
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let s = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(s.members.len(), 2, "expected 2 members, got {}", s.members.len());

    // First member: `let a = Rigid::mass` → QualifiedAccess
    let let_a = match &s.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let for 'a', got {:?}", other),
    };
    assert!(
        matches!(&let_a.value.kind, ExprKind::QualifiedAccess { .. }),
        "expected QualifiedAccess for 'a', got {:?}", let_a.value.kind,
    );

    // Second member: `let b = obj.(Rigid::mass)` → InstanceQualifiedAccess
    let let_b = match &s.members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let for 'b', got {:?}", other),
    };
    assert!(
        matches!(&let_b.value.kind, ExprKind::InstanceQualifiedAccess { .. }),
        "expected InstanceQualifiedAccess for 'b', got {:?}", let_b.value.kind,
    );
}
