//! Lowering characterization tests for the value-level `^` binary operator.
//!
//! These tests lock the contract that `lower_binary_expr` in `ts_parser.rs` requires
//! **NO source change** to handle `^` — the generic path reads the `op` field as a
//! String and emits `ExprKind::BinOp { op: "^", left, right }` automatically once
//! the grammar emits a `binary_expression` with op `^`.
//!
//! # RED → GREEN via grammar only (step-2)
//!
//! Before step-2 (grammar change): these tests were RED because the parser emitted
//! an ERROR node for any value-level `^` (the grammar only had `token.immediate('^')`
//! inside `unit_expr`, not in `binary_expression`), so `reify_syntax::parse` produced
//! a module with error diagnostics rather than a well-formed AST.
//!
//! After step-2: the grammar emits `binary_expression { left, op: "^", right }`, the
//! existing `lower_binary_expr` picks up `op = "^"` generically, and these tests pass
//! with **zero edits to `reify-syntax` source**. That is the contract being locked.
//!
//! `5mm ^ 2` (spaces around `^`) is a `binary_expression` with a `quantity_literal`
//! on the left (value 5.0, unit `mm`) and an integer `number_literal` on the right.
//! This is distinct from `5mm^2` (no spaces), which is a single `quantity_literal`
//! with a `unit_expr` pow arm (committed in the unit_expr corpus as `Pow(mm, 2)`).

use reify_ast::*;

/// Parse `structure S { param p : T = <expr> }` and return the `ExprKind` of
/// the first parameter's default expression.  Mirrors the helper in
/// `unit_expr_lowering_tests.rs`.
fn parse_param_default_kind(source: &str) -> ExprKind {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("value_pow_lowering_test"),
    );
    let structure = match module.declarations.into_iter().next() {
        Some(Declaration::Structure(s)) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    match structure.members.into_iter().next() {
        Some(MemberDecl::Param(p)) => p
            .default
            .expect("param should have a default")
            .kind,
        Some(other) => panic!("expected Param member, got {:?}", other),
        None => panic!("structure has no members"),
    }
}

// ── Value-level ^ lowering ────────────────────────────────────────────────────

/// `5mm ^ 2` lowers to `BinOp { op: "^", left: QuantityLiteral(5.0, Unit("mm")),
/// right: NumberLiteral { value: 2.0, is_real: false } }`.
///
/// PRD §4.3 row: `Scalar<Q> ^ n → Scalar<Q^n>`.  The `^` operator with spaces
/// is a value-level binary_expression; the left operand `5mm` is a quantity_literal,
/// the right is an integer literal (is_real: false).
#[test]
fn quantity_literal_pow_integer_lowers_to_binop() {
    let kind = parse_param_default_kind("structure S { param p : Area = 5mm ^ 2 }");
    match kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "^", "op must be \"^\"");
            // Left: QuantityLiteral(5.0, Unit("mm"))
            match left.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!(
                        (value - 5.0).abs() < f64::EPSILON,
                        "expected value 5.0, got {}",
                        value
                    );
                    assert_eq!(
                        unit,
                        UnitExpr::Unit("mm".to_string()),
                        "unit should be Unit(\"mm\")"
                    );
                }
                other => panic!("expected QuantityLiteral on left, got {:?}", other),
            }
            // Right: NumberLiteral { value: 2.0, is_real: false }
            match right.kind {
                ExprKind::NumberLiteral { value, is_real } => {
                    assert!(
                        (value - 2.0).abs() < f64::EPSILON,
                        "expected value 2.0, got {}",
                        value
                    );
                    assert!(!is_real, "exponent 2 should be integer (is_real: false)");
                }
                other => panic!("expected NumberLiteral on right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

/// `2 ^ 3 ^ 2` lowers right-associated:
/// `BinOp { "^", NumberLiteral(2), BinOp { "^", NumberLiteral(3), NumberLiteral(2) } }`.
///
/// PRD §3.3 / spec §2.7: `^` is right-associative, so `2^3^2 = 2^(3^2)`.
/// This characterization test locks that the grammar's `prec.right(8, ...)` rule
/// produces right-nested BinOp trees, matching the standard mathematical convention.
#[test]
fn right_associativity_2_pow_3_pow_2() {
    let kind = parse_param_default_kind("structure S { param p : Int = 2 ^ 3 ^ 2 }");
    match kind {
        ExprKind::BinOp { op, left, right } => {
            // Outer: 2 ^ (...)
            assert_eq!(op, "^");
            match left.kind {
                ExprKind::NumberLiteral { value, is_real } => {
                    assert!((value - 2.0).abs() < f64::EPSILON, "outer left should be 2");
                    assert!(!is_real);
                }
                other => panic!("outer left should be NumberLiteral(2), got {:?}", other),
            }
            // Inner: 3 ^ 2
            match right.kind {
                ExprKind::BinOp {
                    op: inner_op,
                    left: inner_left,
                    right: inner_right,
                } => {
                    assert_eq!(inner_op, "^");
                    match inner_left.kind {
                        ExprKind::NumberLiteral { value, is_real } => {
                            assert!(
                                (value - 3.0).abs() < f64::EPSILON,
                                "inner left should be 3"
                            );
                            assert!(!is_real);
                        }
                        other => panic!("inner left should be NumberLiteral(3), got {:?}", other),
                    }
                    match inner_right.kind {
                        ExprKind::NumberLiteral { value, is_real } => {
                            assert!(
                                (value - 2.0).abs() < f64::EPSILON,
                                "inner right should be 2"
                            );
                            assert!(!is_real);
                        }
                        other => {
                            panic!("inner right should be NumberLiteral(2), got {:?}", other)
                        }
                    }
                }
                other => panic!("outer right should be BinOp{{^}}, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}
