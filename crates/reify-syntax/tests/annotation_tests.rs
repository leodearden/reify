//! Annotation parsing tests.
//!
//! Tests for `@ident` and `@ident(args)` annotation syntax at top-level declarations.

use reify_syntax::*;

/// Helper: parse source and return the ParsedModule.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_core::ModulePath::single("annotation_test"))
}

// ── Step 1/2: bare annotation on structure ───────────────────────────────────

#[test]
fn parse_bare_annotation_on_structure() {
    let source = "@test structure S { param x: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(
        module.declarations.len(),
        1,
        "expected 1 declaration, got {:?}",
        module.declarations
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "test");
    assert!(
        s.annotations[0].args.is_empty(),
        "expected no args, got {:?}",
        s.annotations[0].args
    );
}

// ── Step 5/6: annotation with identifier arg ─────────────────────────────────

#[test]
fn parse_annotation_with_identifier_arg() {
    let source = "@category(mechanical) structure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "category");
    assert_eq!(
        s.annotations[0].args.len(),
        1,
        "expected 1 arg, got {:?}",
        s.annotations[0].args
    );
    match &s.annotations[0].args[0].kind {
        ExprKind::Ident(name) => assert_eq!(name, "mechanical"),
        other => panic!("expected Ident(\"mechanical\"), got {:?}", other),
    }
}

// ── Step 11/12: multiple annotations on one declaration ──────────────────────

#[test]
fn parse_multiple_annotations_on_structure() {
    let source = "@test\n@deprecated(\"old\")\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        2,
        "expected 2 annotations, got {:?}",
        s.annotations
    );
    // First annotation: @test with no args
    assert_eq!(s.annotations[0].name, "test");
    assert!(
        s.annotations[0].args.is_empty(),
        "expected no args for @test"
    );
    // Second annotation: @deprecated("old")
    assert_eq!(s.annotations[1].name, "deprecated");
    assert_eq!(
        s.annotations[1].args.len(),
        1,
        "expected 1 arg for @deprecated"
    );
    match &s.annotations[1].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "old"),
        other => panic!("expected StringLiteral(\"old\"), got {:?}", other),
    }
}

// ── Step 9/10: annotation with multiple args ─────────────────────────────────

#[test]
fn parse_annotation_with_multiple_args() {
    let source = r#"@config("prod", 3, true) structure S {}"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(s.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(s.annotations[0].name, "config");
    assert_eq!(
        s.annotations[0].args.len(),
        3,
        "expected 3 args, got {:?}",
        s.annotations[0].args
    );

    // First arg: StringLiteral("prod")
    match &s.annotations[0].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "prod"),
        other => panic!("expected StringLiteral(\"prod\"), got {:?}", other),
    }
    // Second arg: NumberLiteral(3.0)
    match &s.annotations[0].args[1].kind {
        ExprKind::NumberLiteral { value: n, .. } => {
            assert!((*n - 3.0).abs() < 1e-10, "expected 3.0, got {n}")
        }
        other => panic!("expected NumberLiteral(3.0), got {:?}", other),
    }
    // Third arg: BoolLiteral(true)
    match &s.annotations[0].args[2].kind {
        ExprKind::BoolLiteral(b) => assert!(*b, "expected true"),
        other => panic!("expected BoolLiteral(true), got {:?}", other),
    }
}

// ── Step 7/8: annotation with complex expression arg ─────────────────────────

#[test]
fn parse_annotation_with_complex_expression_arg() {
    let source = "@tolerance(width * 0.01) structure S { param width: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(s.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(s.annotations[0].name, "tolerance");
    assert_eq!(s.annotations[0].args.len(), 1, "expected 1 arg");

    match &s.annotations[0].args[0].kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*");
            match &left.kind {
                ExprKind::Ident(name) => assert_eq!(name, "width"),
                other => panic!("expected Ident(\"width\"), got {:?}", other),
            }
            match &right.kind {
                ExprKind::NumberLiteral { value: n, .. } => {
                    assert!((*n - 0.01).abs() < 1e-10, "expected 0.01, got {n}")
                }
                other => panic!("expected NumberLiteral(0.01), got {:?}", other),
            }
        }
        other => panic!("expected BinOp(*), got {:?}", other),
    }
}

// ── Step 3/4: annotation with string literal arg ──────────────────────────────

#[test]
fn parse_annotation_with_string_arg() {
    let source = r#"@deprecated("use NewS") structure S {}"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(
        s.annotations.len(),
        1,
        "expected 1 annotation, got {:?}",
        s.annotations
    );
    assert_eq!(s.annotations[0].name, "deprecated");
    assert_eq!(
        s.annotations[0].args.len(),
        1,
        "expected 1 arg, got {:?}",
        s.annotations[0].args
    );
    match &s.annotations[0].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "use NewS"),
        other => panic!("expected StringLiteral(\"use NewS\"), got {:?}", other),
    }
}

// ── Step 13/14: annotation on each remaining declaration type ─────────────────

#[test]
fn parse_annotation_on_function() {
    let source = "@pure fn area(w: Real, h: Real) -> Real { w * h }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let f = match &module.declarations[0] {
        Declaration::Function(f) => f,
        other => panic!("expected Function, got {:?}", other),
    };
    assert_eq!(f.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(f.annotations[0].name, "pure");
    assert!(f.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_trait() {
    let source = "@marker trait Rigid { param mass: Mass }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let t = match &module.declarations[0] {
        Declaration::Trait(t) => t,
        other => panic!("expected Trait, got {:?}", other),
    };
    assert_eq!(t.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(t.annotations[0].name, "marker");
    assert!(t.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_enum() {
    let source = "@flags enum Dir { In, Out }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let e = match &module.declarations[0] {
        Declaration::Enum(e) => e,
        other => panic!("expected Enum, got {:?}", other),
    };
    assert_eq!(e.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(e.annotations[0].name, "flags");
    assert!(e.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_import() {
    let source = "@reexport import std.math";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let i = match &module.declarations[0] {
        Declaration::Import(i) => i,
        other => panic!("expected Import, got {:?}", other),
    };
    assert_eq!(i.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(i.annotations[0].name, "reexport");
    assert!(i.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_field_def() {
    let source = "@cached field def temp : Point3 -> Scalar { source = analytical { |p| p } }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let f = match &module.declarations[0] {
        Declaration::Field(f) => f,
        other => panic!("expected Field, got {:?}", other),
    };
    assert_eq!(f.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(f.annotations[0].name, "cached");
    assert!(f.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_purpose() {
    let source = "@strict purpose mfg_ready(subject: Structure) { constraint subject.params == subject.params }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let p = match &module.declarations[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Purpose, got {:?}", other),
    };
    assert_eq!(p.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(p.annotations[0].name, "strict");
    assert!(p.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_constraint_def() {
    let source = "@builtin constraint def MinWall { x > 0 }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let c = match &module.declarations[0] {
        Declaration::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };
    assert_eq!(c.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(c.annotations[0].name, "builtin");
    assert!(c.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_unit() {
    let source = "@si unit meter : Length";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let u = match &module.declarations[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Unit, got {:?}", other),
    };
    assert_eq!(u.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(u.annotations[0].name, "si");
    assert!(u.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_occurrence() {
    let source = "@async occurrence def Heat { param rate: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let o = match &module.declarations[0] {
        Declaration::Occurrence(o) => o,
        other => panic!("expected Occurrence, got {:?}", other),
    };
    assert_eq!(o.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(o.annotations[0].name, "async");
    assert!(o.annotations[0].args.is_empty());
}

#[test]
fn parse_annotation_on_type_alias() {
    let source = "@builtin type Pressure = Force / Area";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let ta = match &module.declarations[0] {
        Declaration::TypeAlias(ta) => ta,
        other => panic!("expected TypeAlias, got {:?}", other),
    };
    assert_eq!(ta.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(ta.annotations[0].name, "builtin");
    assert!(ta.annotations[0].args.is_empty());
}

// ── Step 15/16: empty annotation arg list ────────────────────────────────────

#[test]
fn parse_annotation_with_empty_arg_list() {
    let source = "@test() structure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(s.annotations.len(), 1, "expected 1 annotation");
    assert_eq!(s.annotations[0].name, "test");
    assert!(
        s.annotations[0].args.is_empty(),
        "expected empty args for @test(), got {:?}",
        s.annotations[0].args
    );
}

// ── Step 17/18: annotations interspersed with pragmas ────────────────────────

#[test]
fn parse_pragma_then_annotation_on_structure() {
    // Pragma is module-level; annotation attaches to following structure.
    let source = "#optimize\n@test\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragma
    assert_eq!(module.pragmas.len(), 1, "expected 1 module pragma");
    assert_eq!(module.pragmas[0].name, "optimize");

    // Structure has 1 annotation
    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(s.annotations.len(), 1, "expected 1 annotation on S");
    assert_eq!(s.annotations[0].name, "test");
}

#[test]
fn parse_annotation_then_pragma_then_structure() {
    // Annotation should still attach to the structure even with a pragma between them.
    // The pragma goes to module-level; the annotation accumulates and drains into S.
    let source = "@test\n#optimize\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragma
    assert_eq!(module.pragmas.len(), 1, "expected 1 module pragma");
    assert_eq!(module.pragmas[0].name, "optimize");

    // Structure has 1 annotation
    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(s.annotations.len(), 1, "expected 1 annotation on S");
    assert_eq!(s.annotations[0].name, "test");
}

// ── Step 19/20: trailing annotation (no following declaration) ────────────────

#[test]
fn parse_trailing_annotation_is_silently_dropped() {
    let source = "structure S {}\n@orphan";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert!(
        s.annotations.is_empty(),
        "S should have no annotations, got {:?}",
        s.annotations
    );
}

// ── Step 21/22: annotation between two declarations ───────────────────────────

#[test]
fn parse_annotation_attaches_to_following_declaration() {
    let source = "structure A {}\n@middle\nstructure B {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 2, "expected 2 declarations");

    let a = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure A, got {:?}", other),
    };
    assert!(
        a.annotations.is_empty(),
        "A should have no annotations, got {:?}",
        a.annotations
    );

    let b = match &module.declarations[1] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure B, got {:?}", other),
    };
    assert_eq!(
        b.annotations.len(),
        1,
        "B should have 1 annotation, got {:?}",
        b.annotations
    );
    assert_eq!(b.annotations[0].name, "middle");
}

// ── Member-level annotations ────────────────────────────────────────────────

#[test]
fn parse_annotation_on_param_member() {
    let source = r#"structure S { @solver_hint("discrete_set", bolt_lengths) param length : Length = auto }"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // Find the param member
    let param = s
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) => Some(p),
            _ => None,
        })
        .expect("expected a Param member");

    assert_eq!(param.name, "length");
    assert_eq!(
        param.annotations.len(),
        1,
        "expected 1 annotation on param, got {:?}",
        param.annotations
    );
    assert_eq!(param.annotations[0].name, "solver_hint");
    assert_eq!(
        param.annotations[0].args.len(),
        2,
        "expected 2 args, got {:?}",
        param.annotations[0].args
    );
    // First arg: StringLiteral("discrete_set")
    match &param.annotations[0].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "discrete_set"),
        other => panic!("expected StringLiteral(\"discrete_set\"), got {:?}", other),
    }
    // Second arg: Ident("bolt_lengths")
    match &param.annotations[0].args[1].kind {
        ExprKind::Ident(name) => assert_eq!(name, "bolt_lengths"),
        other => panic!("expected Ident(\"bolt_lengths\"), got {:?}", other),
    }
}

#[test]
fn parse_annotation_on_let_member() {
    let source = r#"structure S { @solver_hint("prefer_stock", thicknesses) let t = 5mm }"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = s
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) => Some(l),
            _ => None,
        })
        .expect("expected a Let member");

    assert_eq!(let_decl.name, "t");
    assert_eq!(
        let_decl.annotations.len(),
        1,
        "expected 1 annotation on let, got {:?}",
        let_decl.annotations
    );
    assert_eq!(let_decl.annotations[0].name, "solver_hint");
    assert_eq!(let_decl.annotations[0].args.len(), 2);
    match &let_decl.annotations[0].args[0].kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "prefer_stock"),
        other => panic!("expected StringLiteral(\"prefer_stock\"), got {:?}", other),
    }
    match &let_decl.annotations[0].args[1].kind {
        ExprKind::Ident(name) => assert_eq!(name, "thicknesses"),
        other => panic!("expected Ident(\"thicknesses\"), got {:?}", other),
    }
}

#[test]
fn parse_multiple_annotations_on_param_member() {
    let source = r#"structure S { @deprecated("old") @solver_hint("discrete_set", sizes) param w : Length = auto }"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let param = s
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) => Some(p),
            _ => None,
        })
        .expect("expected a Param member");

    assert_eq!(param.name, "w");
    assert_eq!(
        param.annotations.len(),
        2,
        "expected 2 annotations on param, got {:?}",
        param.annotations
    );
    assert_eq!(param.annotations[0].name, "deprecated");
    assert_eq!(param.annotations[1].name, "solver_hint");
    assert_eq!(param.annotations[1].args.len(), 2);
}

#[test]
fn parse_member_annotation_does_not_leak() {
    // Annotation on a constraint (non-param/non-let) should be consumed
    // and NOT leak to the following param y.
    let source =
        r#"structure S { @solver_hint("discrete_set", vals) constraint x > 0 param y : Real }"#;
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    let s = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let param = s
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) => Some(p),
            _ => None,
        })
        .expect("expected a Param member");

    assert_eq!(param.name, "y");
    assert!(
        param.annotations.is_empty(),
        "param y should have no annotations (annotation was consumed by constraint), got {:?}",
        param.annotations
    );
}

// ── Step 23/24: annotation leakage on failed lowering ────────────────────────
//
// Bug: when a declaration node is encountered but its lower_* function returns
// None (e.g. function missing a name), the pending_annotations Vec is not
// consumed — the annotations "leak" and attach to the *next* successfully-
// lowered declaration instead.
//
// This test uses `fn (x: Real) -> Real { x }` (function without a name) which
// should cause lower_function() to return None.  The @leaked annotation must
// NOT carry forward to the following `structure Good {}`.
#[test]
fn annotation_does_not_leak_past_failed_lowering() {
    // A function without a name triggers lower_function() → None.
    let source = "@leaked\nfn (x: Real) -> Real { x }\nstructure Good {}";
    let module = parse_module(source);

    // Good should appear as the only declaration.
    let good_decls: Vec<_> = module
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Structure(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        good_decls.len(),
        1,
        "expected 1 structure declaration, got {:?}",
        module.declarations
    );
    assert_eq!(good_decls[0].name, "Good");

    // @leaked must NOT have attached to Good.
    assert!(
        good_decls[0].annotations.is_empty(),
        "@leaked annotation must not leak to Good; got {:?}",
        good_decls[0].annotations
    );
}
