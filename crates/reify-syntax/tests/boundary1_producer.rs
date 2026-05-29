//! Boundary 1 (syntax → compiler) — Producer-side tests.
//!
//! These tests verify that the parser produces well-formed ParsedModule structures
//! that the compiler can consume. Until the Tree-sitter parser is implemented,
//! tests use the hand-built fixture from reify-test-support.

use reify_ast::*;
use reify_test_support::*;

/// Parse bracket → verify structure (1 StructureDef, 5 params, 3 constraints, 2 lets).
#[test]
fn bracket_structure() {
    let module = bracket_parsed_module();
    assert_eq!(module.declarations.len(), 1);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    assert_eq!(structure.name, "Bracket");

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
    assert_eq!(constraints.len(), 3, "expected 3 constraints");
    assert_eq!(lets.len(), 2, "expected 2 lets (volume + body)");
}

/// Error recovery: malformed input still produces partial declarations + ParseErrors.
#[test]
fn error_recovery_partial_parse() {
    let source = r#"structure Broken {
    param width: Scalar = 80mm
    param !!!invalid!!!
    param height: Scalar = 100mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("broken"));
    // Should have parse errors but also recovered declarations
    assert!(!module.errors.is_empty());
    assert!(!module.declarations.is_empty());
}

/// Content hash determinism: same source → same hashes.
#[test]
fn content_hash_determinism() {
    let m1 = bracket_parsed_module();
    let m2 = bracket_parsed_module();
    assert_eq!(m1.content_hash, m2.content_hash);
}

/// Content hash sensitivity: changed default → changed hash.
#[test]
fn content_hash_sensitivity() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Different params should have different content hashes
    let param_hashes: Vec<_> = structure
        .members
        .iter()
        .filter_map(|m| match m {
            MemberDecl::Param(p) => Some(p.content_hash),
            _ => None,
        })
        .collect();

    // All param hashes should be unique
    for (i, h1) in param_hashes.iter().enumerate() {
        for (j, h2) in param_hashes.iter().enumerate() {
            if i != j {
                assert_ne!(h1, h2, "params {} and {} have same hash", i, j);
            }
        }
    }
}

/// Quantity literal parsing: `80mm` → QuantityLiteral { value: 80.0, unit: "mm" }.
#[test]
fn quantity_literal_parsing() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // First param: width with default 80mm
    let width = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        _ => panic!("expected Param"),
    };

    assert_eq!(width.name, "width");
    match &width.default {
        Some(expr) => match &expr.kind {
            ExprKind::QuantityLiteral { value, unit } => {
                assert!((value - 80.0).abs() < f64::EPSILON);
                assert_eq!(unit, &UnitExpr::Unit("mm".to_string()));
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        },
        None => panic!("expected default value"),
    }
}

/// Operator precedence in the AST: multiplication before addition in volume computation.
#[test]
fn operator_precedence_in_ast() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Find the volume let binding
    let volume = structure
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "volume" => Some(l),
            _ => None,
        })
        .expect("volume let not found");

    // volume = width * height * thickness
    // Should be left-associative: (width * height) * thickness
    match &volume.value.kind {
        ExprKind::BinOp { op, left, .. } => {
            assert_eq!(op, "*");
            match &left.kind {
                ExprKind::BinOp { op: inner_op, .. } => {
                    assert_eq!(inner_op, "*");
                }
                other => panic!("expected inner BinOp, got {:?}", other),
            }
        }
        other => panic!("expected BinOp for volume, got {:?}", other),
    }
}

/// The bracket source text should match the fixture.
#[test]
fn bracket_source_round_trip() {
    let source = bracket_source();
    assert!(source.contains("structure Bracket"));
    assert!(source.contains("param width: Scalar = 80mm"));
    assert!(source.contains("constraint thickness > 2mm"));
    assert!(source.contains("let volume = width * height * thickness"));
    assert!(source.contains("let body = box(width, height, thickness)"));
}

/// Parse `param thickness: Scalar = auto` → ExprKind::Auto default.
#[test]
fn parse_auto_param() {
    let source = r#"structure T {
    param thickness: Scalar = auto
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 1);
    let param = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };

    assert_eq!(param.name, "thickness");
    match &param.default {
        Some(expr) => {
            assert!(
                matches!(expr.kind, ExprKind::Auto { free: false }),
                "expected ExprKind::Auto {{ free: false }}, got {:?}",
                expr.kind
            );
        }
        None => panic!("expected auto default, got None"),
    }
}

/// Parse `param x: Scalar = auto(free)` → ExprKind::Auto { free: true } default.
#[test]
fn parse_auto_free_param() {
    let source = r#"structure T {
    param x: Scalar = auto(free)
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.declarations.len(), 1);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    assert_eq!(structure.members.len(), 1);
    let param = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };

    assert_eq!(param.name, "x");
    match &param.default {
        Some(expr) => {
            assert!(
                matches!(expr.kind, ExprKind::Auto { free: true }),
                "expected ExprKind::Auto {{ free: true }}, got {:?}",
                expr.kind
            );
        }
        None => panic!("expected auto(free) default, got None"),
    }
}

/// Mixed auto and normal params coexist correctly.
#[test]
fn parse_mixed_auto_and_normal_params() {
    let source = r#"structure S {
    param x: Scalar = 5mm
    param y: Scalar = auto
    param z: Scalar
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(structure.members.len(), 3);

    // x has QuantityLiteral default
    let x = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(x.name, "x");
    assert!(matches!(
        x.default.as_ref().unwrap().kind,
        ExprKind::QuantityLiteral { .. }
    ));

    // y has Auto default
    let y = match &structure.members[1] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(y.name, "y");
    assert!(matches!(
        y.default.as_ref().unwrap().kind,
        ExprKind::Auto { free: false }
    ));

    // z has no default
    let z = match &structure.members[2] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(z.name, "z");
    assert!(z.default.is_none());
}

/// Structure with both bare `auto` and `auto(free)` params produces correct flags.
#[test]
fn parse_mixed_auto_and_auto_free() {
    let source = r#"structure S {
    param a: Scalar = auto
    param b: Scalar = auto(free)
    param c: Scalar = 5mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    assert_eq!(structure.members.len(), 3);

    // a: bare auto → free: false
    let a = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(a.name, "a");
    assert!(
        matches!(
            a.default.as_ref().unwrap().kind,
            ExprKind::Auto { free: false }
        ),
        "expected Auto {{ free: false }}, got {:?}",
        a.default.as_ref().unwrap().kind
    );

    // b: auto(free) → free: true
    let b = match &structure.members[1] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(b.name, "b");
    assert!(
        matches!(
            b.default.as_ref().unwrap().kind,
            ExprKind::Auto { free: true }
        ),
        "expected Auto {{ free: true }}, got {:?}",
        b.default.as_ref().unwrap().kind
    );

    // c: QuantityLiteral
    let c = match &structure.members[2] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(c.name, "c");
    assert!(matches!(
        c.default.as_ref().unwrap().kind,
        ExprKind::QuantityLiteral { .. }
    ));
}

/// `auto(constrained)` — unrecognized modifier — should produce parse errors.
///
/// The grammar hard-codes the literal `free` as the only accepted modifier inside
/// `auto(...)`.  This test is an intentional regression guard: if someone generalises
/// the modifier into an arbitrary identifier, this test will fail and force them to
/// decide whether that semantic change is deliberate.
///
/// The span overlap check ensures the error is attributed to the `auto(constrained)`
/// token rather than some unrelated part of the source.  A future grammar change that
/// accidentally accepts `auto(constrained)` while still producing an unrelated error
/// elsewhere (e.g. a stray `}`) would silently pass without this check.
#[test]
fn parse_auto_unrecognized_modifier_is_error() {
    let source = r#"structure T {
    param x: Scalar = auto(constrained)
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        !module.errors.is_empty(),
        "expected parse errors for unrecognized modifier 'constrained' in auto(...)"
    );
    // At least one error must overlap the `auto(constrained)` token.  Using
    // `str::find` avoids hard-coded magic byte offsets that would silently
    // become wrong if the source fixture changes.
    let token = "auto(constrained)";
    let auto_start = source
        .find(token)
        .expect("fixture must contain 'auto(constrained)'") as u32;
    let auto_end = auto_start + token.len() as u32;
    let has_overlapping_error = module
        .errors
        .iter()
        .any(|e| e.span.start < auto_end && e.span.end > auto_start);
    assert!(
        has_overlapping_error,
        "expected at least one parse error overlapping `auto(constrained)` \
         (bytes {auto_start}..{auto_end}), got: {:?}",
        module.errors,
    );
}

/// Line comment with `//` on its own line should parse without errors.
#[test]
fn parse_line_comment_double_slash() {
    let source = r#"structure S {
    // this is a comment
    param x: Scalar = 1mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
}

/// Line comment with `//` after a member (inline comment) should parse without errors.
#[test]
fn parse_line_comment_after_member() {
    let source = r#"structure S {
    param x: Scalar = 1mm // inline comment
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
}

/// Simple `/* */` block comment in a structure body should parse without errors.
#[test]
fn parse_block_comment_simple() {
    let source = r#"structure S {
    /* a comment */
    param x: Scalar = 1mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
}

/// Multi-line `/* */` block comment should parse without errors.
#[test]
fn parse_block_comment_multiline() {
    let source = r#"structure S {
    /*
     * This is a multi-line
     * block comment
     */
    param x: Scalar = 1mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
}

/// `#` is no longer a valid comment marker — should produce parse errors.
#[test]
fn parse_hash_comment_is_error() {
    let source = r#"structure S {
    # old style comment
    param x: Scalar = 1mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        !module.errors.is_empty(),
        "# should no longer be a valid comment"
    );
}

/// `///` (triple-slash doc comment) should parse fine since it starts with `//`.
#[test]
fn parse_doc_comment_triple_slash() {
    let source = r#"/// doc comment for the structure
structure S {
    /// doc comment for a param
    param x: Scalar = 1mm
}"#;
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
}

/// Comments (// and /* */) should not affect AST structure.
#[test]
fn parse_comments_preserve_ast() {
    // Parse bracket_source() normally (no comments)
    let baseline = reify_syntax::parse(bracket_source(), reify_core::ModulePath::single("test"));

    // Inject comments into bracket source
    let commented_source = bracket_source()
        .replace(
            "param width: Scalar = 80mm",
            "// width parameter\nparam width: Scalar = 80mm",
        )
        .replace("let volume", "/* volume computation */ let volume");
    let commented = reify_syntax::parse(&commented_source, reify_core::ModulePath::single("test"));

    assert!(
        commented.errors.is_empty(),
        "expected no parse errors: {:?}",
        commented.errors
    );
    assert_eq!(baseline.declarations.len(), commented.declarations.len());

    let base_s = match &baseline.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };
    let comm_s = match &commented.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };
    assert_eq!(base_s.members.len(), comm_s.members.len());
}

/// Parse bracket → all members carry non-empty spans.
#[test]
fn all_spans_valid() {
    let source = bracket_source();
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    for member in &structure.members {
        let span = match member {
            MemberDecl::Param(p) => p.span,
            MemberDecl::Let(l) => l.span,
            MemberDecl::Constraint(c) => c.span,
            MemberDecl::Sub(s) => s.span,
            MemberDecl::Minimize(m) => m.span,
            MemberDecl::Maximize(m) => m.span,
            MemberDecl::GuardedGroup(g) => g.span,
            MemberDecl::AssociatedType(a) => a.span,
            MemberDecl::Port(p) => p.span,
            MemberDecl::Connect(c) => c.span,
            MemberDecl::Chain(c) => c.span,
            MemberDecl::MetaBlock(m) => m.span,
            MemberDecl::ConstraintInst(ci) => ci.span,
            MemberDecl::ForallConnect(d) => d.span,
            MemberDecl::ForallConstraint(d) => d.span,
            // Not produced by the tree-sitter parser yet (task 2372).
            MemberDecl::MatchArmDeclGroup(g) => g.span,
        };
        assert!(span.start < span.end, "span should be non-empty");
    }
}
