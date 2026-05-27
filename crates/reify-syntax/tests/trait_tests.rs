//! Trait declaration tests.
//!
//! Tests for `trait Name { ... }` declarations with refinements, type parameters,
//! associated types, pub visibility, and structure trait bounds.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("trait_test"));
    (module.declarations, module.errors)
}

/// Helper: unwrap a Named type_expr returning (name, type_args).
fn as_named(te: &TypeExpr) -> (&str, &[TypeExpr]) {
    match &te.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected TypeExprKind::Named, got {:?}", other),
    }
}

// ── Step 1: basic trait ────────────────────────────────────────────

#[test]
fn parse_basic_trait() {
    let (decls, errors) = parse_decls("trait Rigid { param mass : Mass }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Rigid");
    assert!(!trait_decl.is_pub);
    assert!(trait_decl.refinements.is_empty());
    assert!(trait_decl.type_params.is_empty());
    assert_eq!(trait_decl.members.len(), 1);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => {
            assert_eq!(p.name, "mass");
            assert_eq!(p.type_expr.as_ref().unwrap().to_string(), "Mass");
        }
        other => panic!("expected Param, got {:?}", other),
    }
}

// ── Step 3: trait with refinement ──────────────────────────────────

#[test]
fn parse_trait_with_refinement() {
    let (decls, errors) = parse_decls("trait Fastener : Rigid { param thread_pitch : Length }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Fastener");
    let refinement_names: Vec<&str> = trait_decl
        .refinements
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(refinement_names, vec!["Rigid"]);
    assert_eq!(trait_decl.members.len(), 1);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "thread_pitch"),
        other => panic!("expected Param, got {:?}", other),
    }
}

// ── Step 5: multiple refinements ──────────────────────────────────

#[test]
fn parse_trait_multiple_refinements() {
    let (decls, errors) = parse_decls("trait A : B + C { }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "A");
    let refinement_names: Vec<&str> = trait_decl
        .refinements
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(refinement_names, vec!["B", "C"]);
    assert!(trait_decl.members.is_empty());
}

// ── Step 7: various members ───────────────────────────────────────

#[test]
fn parse_trait_various_members() {
    let source = "trait Full {\n  param mass : Mass\n  let density = mass / volume\n  constraint mass > 0\n  sub inner = Component()\n}";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 4);

    match &trait_decl.members[0] {
        MemberDecl::Param(p) => assert_eq!(p.name, "mass"),
        other => panic!("expected Param, got {:?}", other),
    }
    match &trait_decl.members[1] {
        MemberDecl::Let(l) => assert_eq!(l.name, "density"),
        other => panic!("expected Let, got {:?}", other),
    }
    assert!(matches!(&trait_decl.members[2], MemberDecl::Constraint(_)));
    match &trait_decl.members[3] {
        MemberDecl::Sub(s) => assert_eq!(s.name, "inner"),
        other => panic!("expected Sub, got {:?}", other),
    }
}

// ── Step 9: associated types ──────────────────────────────────────

#[test]
fn parse_trait_associated_type() {
    let (decls, errors) = parse_decls("trait WithType { type Material = Steel }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 1);
    match &trait_decl.members[0] {
        MemberDecl::AssociatedType(a) => {
            assert_eq!(a.name, "Material");
            assert!(a.default_type.is_some());
            assert_eq!(a.default_type.as_ref().unwrap().to_string(), "Steel");
        }
        other => panic!("expected AssociatedType, got {:?}", other),
    }
}

#[test]
fn parse_trait_associated_type_no_default() {
    let (decls, errors) = parse_decls("trait Bare { type Output }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.members.len(), 1);
    match &trait_decl.members[0] {
        MemberDecl::AssociatedType(a) => {
            assert_eq!(a.name, "Output");
            assert!(a.default_type.is_none());
        }
        other => panic!("expected AssociatedType, got {:?}", other),
    }
}

// ── Step 11: pub trait ────────────────────────────────────────────

#[test]
fn parse_pub_trait() {
    let (decls, errors) = parse_decls("pub trait Visible { param color : Scalar }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert!(trait_decl.is_pub);
    assert_eq!(trait_decl.name, "Visible");
    assert_eq!(trait_decl.members.len(), 1);
}

// ── Step 13: structure with trait bounds ───────────────────────────

#[test]
fn parse_structure_with_trait_bounds() {
    let (decls, errors) =
        parse_decls("structure def Bolt : Fastener + Rigid { param length : Length = 20mm }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "Bolt");
    let bound_names: Vec<&str> = structure
        .trait_bounds
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(bound_names, vec!["Fastener", "Rigid"]);
    assert!(!structure.is_pub);
    assert!(structure.type_params.is_empty());
    assert_eq!(structure.members.len(), 1);
}

// ── Step 15: type parameters ──────────────────────────────────────

#[test]
fn parse_trait_with_type_params() {
    let (decls, errors) = parse_decls("trait Container<T: Rigid> { param capacity : Scalar }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.name, "Container");
    assert_eq!(trait_decl.type_params.len(), 1);
    assert_eq!(trait_decl.type_params[0].name, "T");
    assert_eq!(trait_decl.type_params[0].bounds, vec!["Rigid"]);
}

#[test]
fn parse_trait_with_multi_type_params() {
    let (decls, errors) = parse_decls("trait Pair<A: Rigid, B> { }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.type_params.len(), 2);
    assert_eq!(trait_decl.type_params[0].name, "A");
    assert_eq!(trait_decl.type_params[0].bounds, vec!["Rigid"]);
    assert_eq!(trait_decl.type_params[1].name, "B");
    assert!(trait_decl.type_params[1].bounds.is_empty());
}

// ── Step 17: backward compatibility ───────────────────────────────

#[test]
fn backward_compat_bracket_source() {
    let source = reify_test_support::bracket_source();
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "Bracket");
    assert!(!structure.is_pub);
    assert!(structure.type_params.is_empty());
    assert!(structure.trait_bounds.is_empty());
    assert_eq!(structure.members.len(), 10);
}

#[test]
fn backward_compat_no_def_keyword() {
    let (decls, errors) = parse_decls("structure S { param x : Scalar = 5mm }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.name, "S");
    assert_eq!(structure.members.len(), 1);
}

// ── span tests for refinements ────────────────────────────────────

#[test]
fn parse_trait_refinement_has_span() {
    let source = "trait Fastener : Rigid { param thread_pitch : Length }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    let rigid_start = source.find("Rigid").unwrap();
    assert_eq!(trait_decl.refinements.len(), 1);
    assert_eq!(trait_decl.refinements[0].name, "Rigid");
    assert_eq!(
        trait_decl.refinements[0].span,
        reify_core::SourceSpan::new(rigid_start as u32, (rigid_start + 5) as u32),
        "span should cover exactly the 'Rigid' token"
    );
}

#[test]
fn parse_trait_multiple_refinements_have_distinct_spans() {
    let source = "trait A : B + C { }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let trait_decl = match &decls[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };

    assert_eq!(trait_decl.refinements.len(), 2);

    let b_start = source.find('B').unwrap();
    assert_eq!(trait_decl.refinements[0].name, "B");
    assert_eq!(
        trait_decl.refinements[0].span,
        reify_core::SourceSpan::new(b_start as u32, (b_start + 1) as u32),
        "span for 'B' should cover exactly 1 byte"
    );

    let c_start = source.find('C').unwrap();
    assert_eq!(trait_decl.refinements[1].name, "C");
    assert_eq!(
        trait_decl.refinements[1].span,
        reify_core::SourceSpan::new(c_start as u32, (c_start + 1) as u32),
        "span for 'C' should cover exactly 1 byte"
    );
}

// ── pre-2: type_expr with type args ────────────────────────────────

#[test]
fn parse_type_expr_with_type_args() {
    // param whose type annotation is a parameterized type: Box<T>
    let (decls, errors) = parse_decls("structure def S { param contents : Box<Bolt> }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let param = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };

    let te = param
        .type_expr
        .as_ref()
        .expect("type_expr should be present");
    let (name, type_args) = as_named(te);
    assert_eq!(name, "Box");
    assert_eq!(type_args.len(), 1);
    let (arg0_name, arg0_args) = as_named(&type_args[0]);
    assert_eq!(arg0_name, "Bolt");
    assert!(arg0_args.is_empty());
}

#[test]
fn parse_type_expr_nested_type_args() {
    // Nested parameterized types: Container<Box<T>>
    let (decls, errors) = parse_decls("structure def S { param x : Container<Box<T>> }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let param = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };

    let te = param.type_expr.as_ref().unwrap();
    let (name, type_args) = as_named(te);
    assert_eq!(name, "Container");
    assert_eq!(type_args.len(), 1);
    let (arg0_name, arg0_args) = as_named(&type_args[0]);
    assert_eq!(arg0_name, "Box");
    assert_eq!(arg0_args.len(), 1);
    let (arg0_0_name, _) = as_named(&arg0_args[0]);
    assert_eq!(arg0_0_name, "T");
}

// ── pre-2: type_parameter with default ─────────────────────────────

#[test]
fn parse_type_param_with_default() {
    let (decls, errors) =
        parse_decls("structure def Box<T: Rigid = Steel> { param w : Length = 10mm }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.type_params.len(), 1);
    let tp = &structure.type_params[0];
    assert_eq!(tp.name, "T");
    assert_eq!(tp.bounds, vec!["Rigid"]);
    let default = tp.default.as_ref().expect("default type should be present");
    let (default_name, default_args) = as_named(default);
    assert_eq!(default_name, "Steel");
    assert!(default_args.is_empty());
}

// ── pre-2: sub_declaration with type args ──────────────────────────

#[test]
fn parse_sub_with_type_args() {
    let (decls, errors) = parse_decls("structure def Asm { sub part = Box<Bolt>() }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let sub = match &structure.members[0] {
        MemberDecl::Sub(s) => s,
        other => panic!("expected Sub, got {:?}", other),
    };

    assert_eq!(sub.name, "part");
    assert_eq!(sub.structure_name, "Box");
    assert_eq!(sub.type_args.len(), 1);
    let (arg0_name, _) = as_named(&sub.type_args[0]);
    assert_eq!(arg0_name, "Bolt");
}

// ── pre-2: trait_bound_list with type args ─────────────────────────

#[test]
fn parse_trait_bound_with_type_args() {
    // Structure conforming to a parameterized trait bound
    let (decls, errors) =
        parse_decls("structure def Crate : Container<Bolt> { param count : Int = 5 }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.trait_bounds.len(), 1);
    assert_eq!(structure.trait_bounds[0].name, "Container");
    assert_eq!(structure.trait_bounds[0].type_args.len(), 1);
    let (arg0_name, _) = as_named(&structure.trait_bounds[0].type_args[0]);
    assert_eq!(arg0_name, "Bolt");
}
