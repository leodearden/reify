//! Integration tests for `interpolated_string` CST-node lowering.
//!
//! Verifies that `"a {x} b"` and `"sum {1 + 1}"` are lowered to
//! `ExprKind::InterpolatedString(Vec<StringPart>)`, and that the plain-string
//! fast path (`"hello"` → `ExprKind::StringLiteral`) is not disturbed.
//!
//! Step-3 (RED): tests compile (StringPart + InterpolatedString exist from
//! prereq-2) but fail because `lower_interpolated_string` is not yet wired
//! into `lower_expr`.  Step-4 wires it (GREEN).
//! Step-5 (RED) adds escape-decoding assertions; step-6 makes them pass.

use reify_ast::*;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Parse `source` and return the first structure's members + errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("interp_test"));
    let structure = match module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure declaration, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// Parse `source` and extract the first `let` binding's value expression.
fn extract_let_value(source: &str) -> Expr {
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    assert!(!members.is_empty(), "no members produced — lower_interpolated_string not wired?");
    match &members[0] {
        MemberDecl::Let(l) => l.value.clone(),
        other => panic!("expected Let member, got {:?}", other),
    }
}

// ── step-3 RED tests ─────────────────────────────────────────────────────────

/// `"hello"` must still lower to `ExprKind::StringLiteral("hello")` —
/// the plain-string fast path must not be disturbed.
#[test]
fn plain_string_regression() {
    let expr = extract_let_value(r#"structure S { let v = "hello" }"#);
    match &expr.kind {
        ExprKind::StringLiteral(s) => assert_eq!(s, "hello"),
        other => panic!("expected StringLiteral(\"hello\"), got {:?}", other),
    }
}

/// `"a {x} b"` lowers to
/// `InterpolatedString([Literal("a "), Hole(Ident "x"), Literal(" b")])`.
#[test]
fn interpolated_string_simple_hole() {
    let expr = extract_let_value(r#"structure S { let v = "a {x} b" }"#);
    match &expr.kind {
        ExprKind::InterpolatedString(parts) => {
            assert_eq!(parts.len(), 3, "expected 3 parts, got {}: {:?}", parts.len(), parts);
            // Part 0: Literal "a "
            match &parts[0] {
                StringPart::Literal(s) => assert_eq!(s, "a ", "part[0] text"),
                other => panic!("expected Literal(\"a \"), got {:?}", other),
            }
            // Part 1: Hole wrapping Ident "x"
            match &parts[1] {
                StringPart::Hole(expr) => match &expr.kind {
                    ExprKind::Ident(name) => assert_eq!(name, "x"),
                    other => panic!("expected Ident(\"x\") inside Hole, got {:?}", other),
                },
                other => panic!("expected Hole, got {:?}", other),
            }
            // Part 2: Literal " b"
            match &parts[2] {
                StringPart::Literal(s) => assert_eq!(s, " b", "part[2] text"),
                other => panic!("expected Literal(\" b\"), got {:?}", other),
            }
        }
        other => panic!("expected InterpolatedString, got {:?}", other),
    }
}

/// `"sum {1 + 1}"` lowers to
/// `InterpolatedString([Literal("sum "), Hole(BinOp{"+", 1, 1})])`.
/// Verifies that holes wrap full `$._expression`, including binary expressions.
#[test]
fn interpolated_string_arithmetic_hole() {
    let expr = extract_let_value(r#"structure S { let v = "sum {1 + 1}" }"#);
    match &expr.kind {
        ExprKind::InterpolatedString(parts) => {
            assert_eq!(parts.len(), 2, "expected 2 parts, got {}: {:?}", parts.len(), parts);
            // Part 0: Literal "sum "
            match &parts[0] {
                StringPart::Literal(s) => assert_eq!(s, "sum ", "part[0] text"),
                other => panic!("expected Literal(\"sum \"), got {:?}", other),
            }
            // Part 1: Hole wrapping BinOp
            match &parts[1] {
                StringPart::Hole(expr) => match &expr.kind {
                    ExprKind::BinOp { op, left, right } => {
                        assert_eq!(op, "+");
                        match &left.kind {
                            ExprKind::NumberLiteral { value, .. } => {
                                assert_eq!(*value, 1.0_f64)
                            }
                            other => panic!("expected NumberLiteral(1) for left, got {:?}", other),
                        }
                        match &right.kind {
                            ExprKind::NumberLiteral { value, .. } => {
                                assert_eq!(*value, 1.0_f64)
                            }
                            other => {
                                panic!("expected NumberLiteral(1) for right, got {:?}", other)
                            }
                        }
                    }
                    other => panic!("expected BinOp inside Hole, got {:?}", other),
                },
                other => panic!("expected Hole, got {:?}", other),
            }
        }
        other => panic!("expected InterpolatedString, got {:?}", other),
    }
}
