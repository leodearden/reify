//! Integration tests for scientific notation in number literals.
//!
//! Verifies that `1e-6`, `2.5E10`, `3.14e+0`, etc. lower correctly to
//! `ExprKind::NumberLiteral(f64)`, and that existing quantity literals
//! (`5mm`, `80mm`, `1deg`) continue to work unchanged.
//!
//! These tests pin the end-to-end contract: grammar.js emits a single
//! `number_literal` CST node, and `lower_number_literal` passes its text
//! to `f64::from_str`, which accepts scientific notation per the standard.
//!
//! See also: `tree-sitter-reify/test/corpus/scientific_notation.txt` for
//! the CST-level corpus tests.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("sci_notation_test"));
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

/// Extract the f64 value from a `let x: Real = <expr>` member, asserting no errors.
fn extract_number_literal(source: &str) -> f64 {
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match let_decl.value.kind {
        ExprKind::NumberLiteral(v) => v,
        ref other => panic!("expected NumberLiteral, got {:?}", other),
    }
}

// ── Scientific notation: positive cases ──────────────────────────────────────

#[test]
fn negative_exponent_1e_minus_6() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1e-6\n}",
    );
    assert_eq!(v, 1e-6_f64, "1e-6 should lower to 1e-6_f64");
}

#[test]
fn uppercase_e_1e_minus_6() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1E-6\n}",
    );
    assert_eq!(v, 1E-6_f64, "1E-6 should lower to 1E-6_f64");
}

#[test]
fn positive_exponent_1e_plus_6() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1e+6\n}",
    );
    assert_eq!(v, 1e+6_f64, "1e+6 should lower to 1e+6_f64");
}

#[test]
fn bare_positive_exponent_1e6() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1e6\n}",
    );
    assert_eq!(v, 1e6_f64, "1e6 should lower to 1e6_f64");
}

#[test]
fn decimal_mantissa_1_5e2() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1.5e2\n}",
    );
    assert_eq!(v, 1.5e2_f64, "1.5e2 should lower to 1.5e2_f64");
}

#[test]
fn large_negative_exponent_1_0e_minus_300() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1.0e-300\n}",
    );
    assert_eq!(v, 1.0e-300_f64, "1.0e-300 should lower to 1.0e-300_f64");
}

#[test]
fn zero_exponent_1e0() {
    let v = extract_number_literal(
        "structure S {\n  let x: Real = 1e0\n}",
    );
    assert_eq!(v, 1e0_f64, "1e0 should lower to 1e0_f64 (== 1.0)");
}

// ── Acceptance scenario from solver_elastic.ri ───────────────────────────────

/// Acceptance scenario: `cg_tolerance: Real = 1e-6` parses without rewriting
/// to 0.000001 (the workaround noted in crates/reify-compiler/stdlib/solver_elastic.ri:33-34).
#[test]
fn cg_tolerance_acceptance_scenario() {
    let v = extract_number_literal(
        "structure S {\n  let cg_tolerance: Real = 1e-6\n}",
    );
    assert_eq!(
        v, 1e-6_f64,
        "cg_tolerance = 1e-6 should lower to f64 value 1e-6"
    );
}

// ── Preserved quantity literals ───────────────────────────────────────────────

/// Existing `5mm` quantity literal must still parse unchanged after the grammar change.
#[test]
fn preserved_quantity_literal_5mm() {
    let (members, errors) = parse_members("structure S {\n  let w = 5mm\n}");
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match &let_decl.value.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert_eq!(*value, 5.0_f64, "quantity value should be 5.0");
            assert_eq!(unit, "mm", "quantity unit should be 'mm'");
        }
        other => panic!("expected QuantityLiteral, got {:?}", other),
    }
}

// ── Unit-suffix fallback disambiguation ─────────────────────────────────────

/// `5e` (no digits after 'e') falls back to quantity_literal(5, "e") at the
/// parse layer — no error nodes. The unit 'e' is unregistered, so a
/// type-resolution diagnostic will fire later, but the parser succeeds cleanly.
///
/// This pins the well-defined fallback behaviour described in the plan's
/// disambiguation note: the exponent regex requires \d+ after the optional
/// sign, so `5e` fails the exponent match, the regex matches only `5`, and
/// tree-sitter's `token.immediate` in `quantity_literal` consumes `e` as the
/// unit suffix.
#[test]
fn disambiguation_5e_parses_as_quantity_literal() {
    let (members, errors) = parse_members("structure S {\n  let x = 5e\n}");
    assert!(
        errors.is_empty(),
        "parse layer should succeed for '5e' (unit-suffix fallback); got: {:?}",
        errors
    );
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match &let_decl.value.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert_eq!(*value, 5.0_f64, "quantity value should be 5.0");
            assert_eq!(unit, "e", "unit should be 'e' (unregistered, but parses cleanly)");
        }
        other => panic!("expected QuantityLiteral {{ value: 5.0, unit: \"e\" }}, got {:?}", other),
    }
}
