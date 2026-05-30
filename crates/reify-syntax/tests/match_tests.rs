//! Match expression parsing tests.

use reify_ast::{MatchPattern, *};

/// Helper: parse source and return the first structure's members.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("match_test"));
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

/// Parse `match d { In => 1, Out => 2, Bidi => 3 }` with an enum Direction declaration.
/// Extract the let's value expr, assert it is ExprKind::Match with 3 arms.
#[test]
fn parse_match_three_arms() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S {
    param d : Scalar
    let x = match d { In => 1, Out => 2, Bidi => 3 }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "x");

    match &let_decl.value.kind {
        ExprKind::Match { discriminant, arms } => {
            // Discriminant should be Ident("d")
            match &discriminant.kind {
                ExprKind::Ident(name) => assert_eq!(name, "d"),
                other => panic!("expected Ident('d'), got {:?}", other),
            }

            // 3 arms
            assert_eq!(arms.len(), 3, "expected 3 arms, got {}", arms.len());

            // Arm 0: In => 1
            assert_eq!(arms[0].patterns, vec!["In"]);
            match &arms[0].body.kind {
                ExprKind::NumberLiteral { value: v, .. } => assert_eq!(*v, 1.0),
                other => panic!("expected NumberLiteral(1), got {:?}", other),
            }

            // Arm 1: Out => 2
            assert_eq!(arms[1].patterns, vec!["Out"]);
            match &arms[1].body.kind {
                ExprKind::NumberLiteral { value: v, .. } => assert_eq!(*v, 2.0),
                other => panic!("expected NumberLiteral(2), got {:?}", other),
            }

            // Arm 2: Bidi => 3
            assert_eq!(arms[2].patterns, vec!["Bidi"]);
            match &arms[2].body.kind {
                ExprKind::NumberLiteral { value: v, .. } => assert_eq!(*v, 3.0),
                other => panic!("expected NumberLiteral(3), got {:?}", other),
            }
        }
        other => panic!("expected Match, got {:?}", other),
    }
}

/// Parse match with multi-variant pattern: `Socket | Button => "recessed"`.
#[test]
fn parse_match_multi_variant_arm() {
    let source = r#"structure S {
    let x = match d { Socket | Button => "recessed", Slider => "raised" }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 2, "expected 2 arms");

            // First arm: Socket | Button => "recessed"
            assert_eq!(arms[0].patterns, vec!["Socket", "Button"]);
            match &arms[0].body.kind {
                ExprKind::StringLiteral(s) => assert_eq!(s, "recessed"),
                other => panic!("expected StringLiteral('recessed'), got {:?}", other),
            }

            // Second arm: Slider => "raised"
            assert_eq!(arms[1].patterns, vec!["Slider"]);
            match &arms[1].body.kind {
                ExprKind::StringLiteral(s) => assert_eq!(s, "raised"),
                other => panic!("expected StringLiteral('raised'), got {:?}", other),
            }
        }
        other => panic!("expected Match, got {:?}", other),
    }
}

/// Parse match with wildcard pattern: `_ => 0`.
#[test]
fn parse_match_wildcard_arm() {
    let source = r#"structure S {
    let x = match d { In => 1, _ => 0 }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 2, "expected 2 arms");

            // First arm: In => 1
            assert_eq!(arms[0].patterns, vec!["In"]);

            // Second arm: _ => 0 (wildcard)
            assert_eq!(arms[1].patterns, vec!["_"]);
            match &arms[1].body.kind {
                ExprKind::NumberLiteral { value: v, .. } => assert_eq!(*v, 0.0),
                other => panic!("expected NumberLiteral(0), got {:?}", other),
            }
        }
        other => panic!("expected Match, got {:?}", other),
    }
}

// ── Task 3938: named-field payload-binding lowering test (step-3 RED) ────────

/// Parse a match expression with three arms:
///   arm0 — bare variant:           `Point => 0mm`
///   arm1 — one-field bind:         `Circle { radius: r } => r`
///   arm2 — two-field bind:         `Rect { width: w, height: h } => w`
///
/// Asserts that `lower_match_arm` produces structured `MatchPattern` values:
///   arm0 → [MatchPattern::Variant("Point")]
///   arm1 → [MatchPattern::VariantBind { name: "Circle", binders: [("radius", "r")] }]
///   arm2 → [MatchPattern::VariantBind { name: "Rect",   binders: [("width","w"),("height","h")] }]
///
/// RED until step-4 lands (reify_ast::MatchPattern doesn't exist yet and
/// MatchArm.patterns is still Vec<String>).
#[test]
fn parse_match_named_field_binding() {
    let source = r#"structure S {
    let area = match outline {
        Point => 0mm,
        Circle { radius: r } => r,
        Rect { width: w, height: h } => w
    }
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 3, "expected 3 arms, got {}", arms.len());

            // arm0: bare variant Point
            assert_eq!(
                arms[0].patterns,
                vec![MatchPattern::Variant("Point".into())],
                "arm0 patterns mismatch"
            );

            // arm1: single named-field Circle { radius: r }
            assert_eq!(
                arms[1].patterns,
                vec![MatchPattern::VariantBind {
                    name: "Circle".into(),
                    binders: vec![("radius".into(), "r".into())],
                }],
                "arm1 patterns mismatch"
            );

            // arm2: two-field Rect { width: w, height: h }
            assert_eq!(
                arms[2].patterns,
                vec![MatchPattern::VariantBind {
                    name: "Rect".into(),
                    binders: vec![
                        ("width".into(), "w".into()),
                        ("height".into(), "h".into()),
                    ],
                }],
                "arm2 patterns mismatch"
            );
        }
        other => panic!("expected Match, got {:?}", other),
    }
}
