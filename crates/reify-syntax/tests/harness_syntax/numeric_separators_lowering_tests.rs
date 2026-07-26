//! Integration tests for `_` digit-separator lowering in number literals.
//!
//! Verifies that `1_000_000`, `0.000_001`, `1_000e1_0`, etc. lower correctly
//! to `ExprKind::NumberLiteral { value, is_real }`, and that `1_000mm` lowers
//! correctly to `ExprKind::QuantityLiteral { value, unit }`.
//!
//! These tests pin the end-to-end AST lowering contract: grammar.js
//! (task 3909 / α) already emits a single `number_literal` CST node for
//! `_`-bearing literals; the remaining gap is that `lower_number_literal`
//! and `lower_quantity_literal` must strip `_` before calling
//! `f64::from_str` (which rejects `_` in raw form).
//!
//! CST-shape tests (asserting the grammar tokenises `_`-bearing literals as
//! single `number_literal` nodes) live in
//! `crates/reify-syntax/tests/numeric_separators_grammar_tests.rs` and are
//! α-only; this file adds the β lowering layer on top.
//!
//! See also: `docs/prds/v0_6/numeric-and-range-literal-forms.md`.

use reify_ast::*;

/// Helper: parse source and return the `ParsedModule`.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_core::ModulePath::single("numeric_sep_lowering_test"))
}

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("numeric_sep_lowering_test"),
    );
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

/// Extract both the `value` and `is_real` flag from a number-literal
/// member `let x: Real = <lit>`, asserting no parse errors.
fn extract_number_literal_with_flag(source: &str) -> (f64, bool) {
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match let_decl.value.kind {
        ExprKind::NumberLiteral { value, is_real } => (value, is_real),
        ref other => panic!(
            "expected NumberLiteral {{ value, is_real }}, got {:?}",
            other
        ),
    }
}

// ── Number-literal `_` separator cases ──────────────────────────────────────

/// `1_000_000` (integer with `_` separators) must lower to
/// `{ value: 1_000_000.0, is_real: false }`.
#[test]
fn integer_with_underscores_1_000_000() {
    let (value, is_real) = extract_number_literal_with_flag(
        "structure S {\n  let x: Real = 1_000_000\n}",
    );
    assert_eq!(value, 1_000_000.0_f64, "1_000_000 should lower to 1000000.0");
    assert!(
        !is_real,
        "1_000_000 (no decimal point, no exponent) should have is_real = false"
    );
}

/// `0.000_001` (decimal with `_` separator in fractional part) must lower to
/// `{ value: 0.000_001 (≡ 1e-6), is_real: true }`.
#[test]
fn decimal_with_underscore_0_000_001() {
    let (value, is_real) = extract_number_literal_with_flag(
        "structure S {\n  let x: Real = 0.000_001\n}",
    );
    assert_eq!(value, 0.000_001_f64, "0.000_001 should lower to 1e-6");
    assert!(
        is_real,
        "0.000_001 (has decimal point) should have is_real = true"
    );
}

/// `1_000e1_0` (integer `_` mantissa + exponent `_` separator) must lower to
/// `{ value: 1e13, is_real: true }`.
#[test]
fn mixed_underscore_1_000e1_0() {
    let (value, is_real) = extract_number_literal_with_flag(
        "structure S {\n  let x: Real = 1_000e1_0\n}",
    );
    assert_eq!(value, 1e13_f64, "1_000e1_0 should lower to 1e13 (1000 * 10^10)");
    assert!(
        is_real,
        "1_000e1_0 (has exponent suffix) should have is_real = true"
    );
}

// ── Regression: plain integers continue to work ──────────────────────────────

/// `1000` (plain integer, no `_`) must still lower correctly to
/// `{ value: 1000.0, is_real: false }`.
#[test]
fn regression_plain_integer_1000() {
    let (value, is_real) = extract_number_literal_with_flag(
        "structure S {\n  let x: Real = 1000\n}",
    );
    assert_eq!(value, 1000.0_f64, "1000 should lower to 1000.0");
    assert!(
        !is_real,
        "1000 (no decimal point, no exponent) should have is_real = false"
    );
}

// ── Quantity-literal `_` separator cases ─────────────────────────────────────

/// `1_000mm` (quantity with `_` separator in numeric value) must lower to
/// `ExprKind::QuantityLiteral { value: 1000.0, unit: UnitExpr::Unit("mm") }`.
///
/// Grammar: `quantity_literal = field('value', number_literal) + _unit_expr_start
/// + field('unit', unit_expr)`, so `1_000mm` produces
/// `quantity_literal(value="1_000", unit=mm)`.  The value child text "1_000"
/// must have `_` stripped before `f64::from_str` is called.
#[test]
fn quantity_literal_1_000mm() {
    let (members, errors) = parse_members("structure S {\n  let len = 1_000mm\n}");
    assert!(errors.is_empty(), "unexpected parse errors: {:?}", errors);
    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    match &let_decl.value.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert_eq!(*value, 1000.0_f64, "1_000mm quantity value should be 1000.0");
            assert_eq!(
                unit,
                &UnitExpr::Unit("mm".to_string()),
                "1_000mm unit should be 'mm'"
            );
        }
        other => panic!("expected QuantityLiteral, got {:?}", other),
    }
}

// ── Pragma `_` separator cases ───────────────────────────────────────────────

/// `#config(level=1_000)` must lower the pragma number value to
/// `PragmaValue::Number(1000.0)`.  This exercises the `lower_pragma_value`
/// `"number_literal"` arm, which previously called raw `text.parse::<f64>()`
/// and silently returned `None` for `_`-bearing literals.
#[test]
fn pragma_number_with_underscores() {
    let source = "#config(level=1_000)\nstructure S {}";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "unexpected parse errors: {:?}", module.errors);
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");
    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "config");
    assert_eq!(pragma.args.len(), 1, "expected 1 arg, got {:?}", pragma.args);
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "level");
            assert_eq!(*value, PragmaValue::Number(1000.0), "1_000 should lower to 1000.0");
        }
        other => panic!("expected KeyValue('level', Number(1000.0)), got {:?}", other),
    }
}

/// `#config(min_wall=1_000mm)` must lower the pragma quantity value to
/// `PragmaValue::Quantity { value: 1000.0, unit: "mm" }`.  This exercises the
/// `lower_pragma_value` `"quantity_literal"` arm, which previously called raw
/// `self.node_text(value_node).parse().ok()?` and silently returned `None` for
/// `_`-bearing numeric values inside quantity literals.
#[test]
fn pragma_quantity_with_underscores() {
    let source = "#config(min_wall=1_000mm)\nstructure S {}";
    let module = parse_module(source);
    assert!(module.errors.is_empty(), "unexpected parse errors: {:?}", module.errors);
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");
    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "config");
    assert_eq!(pragma.args.len(), 1, "expected 1 arg, got {:?}", pragma.args);
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "min_wall");
            assert_eq!(
                *value,
                PragmaValue::Quantity { value: 1000.0, unit: "mm".to_string() },
                "1_000mm should lower to Quantity {{ value: 1000.0, unit: \"mm\" }}"
            );
        }
        other => panic!(
            "expected KeyValue('min_wall', Quantity {{ value: 1000.0, unit: 'mm' }}), got {:?}",
            other
        ),
    }
}
