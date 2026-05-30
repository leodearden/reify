//! Rust integration tests for task 3910: hex (0x/0X) and binary (0b/0B) integer forms
//! in `number_literal`.
//!
//! User-observable signal: `cargo test -p reify-syntax --test radix_literals_grammar_tests`
//! passes (GREEN after grammar.js + scanner.c are patched; RED before).
//!
//! These tests assert **CST shape only** — the raw tree-sitter tree — and are
//! δ-independent: they do NOT call `reify_syntax::parse` or assert f64/Int values
//! (radix-aware lowering is task δ).  The δ-independence criterion is that
//! `f64::from_str("0xFF")` rejects hex today; asserting a lowered value here
//! would produce an unsatisfiable RED test.
//!
//! Coverage:
//! * **(A)** Positive CST shape — `0xFF`, `0XFF`, `0b1010`, `0B1010`, `0xDEAD_BEEF`
//!   each parse as a single `number_literal` node whose text spans the whole literal;
//!   no `unit_expr` node (defeats the quantity-literal misparse).
//! * **(B)** Quantity suffix — `0xFFmm` is a `quantity_literal` containing a
//!   `number_literal "0xFF"` and a `unit_expr "mm"`.
//! * **(C)** Regression — `0`, `255`, `0.5` each still parse as a single
//!   `number_literal` with exact text; `5mm` still parses as `quantity_literal`.
//!
//! See also: `tree-sitter-reify/test/corpus/radix_literals.txt` for the
//! corpus-level CST documentation, runnable via `tree-sitter test`.

mod common;
use common::{find_cst_node, make_ts_parser};

// ── Assertion helpers ────────────────────────────────────────────────────────

/// Parse `structure S { let x = <lit> }` and assert:
///
/// 1. No ERROR nodes in the CST.
/// 2. The `value` field of `let_declaration` has kind `"number_literal"`.
/// 3. The `number_literal` text spans the whole `<lit>` (not just the leading
///    `0` — this is the signal that `xFF`/`b1010` was NOT consumed as a unit suffix).
/// 4. No `unit_expr` node anywhere in the tree (defeats the misparse).
fn assert_radix_literal_is_single_number_literal(lit: &str) {
    let source = format!("structure S {{\n  let x = {lit}\n}}");
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    // (1) No ERROR nodes.
    assert!(
        !root.has_error(),
        "expected no parse error for {lit:?}; got errors in: {source:?}"
    );

    // (2) The let_declaration value field must be a number_literal.
    let let_decl = find_cst_node(root, "let_declaration")
        .expect("expected a let_declaration node in the CST");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a `value` field");
    assert_eq!(
        value_node.kind(),
        "number_literal",
        "let_declaration.value must be `number_literal` for {lit:?}, \
         got `{}`; this indicates the literal was misparsed as quantity_literal \
         with a unit-suffix starting at `x`/`b`",
        value_node.kind()
    );

    // (3) The number_literal text must span the whole literal.
    let actual_text = value_node
        .utf8_text(source.as_bytes())
        .expect("utf8_text failed");
    assert_eq!(
        actual_text, lit,
        "number_literal text must span the entire literal including the radix prefix; \
         got {actual_text:?}, expected {lit:?}"
    );

    // (4) No unit_expr node — the hex/binary digits must NOT be consumed as a unit suffix.
    assert!(
        find_cst_node(root, "unit_expr").is_none(),
        "must not produce a `unit_expr` node for {lit:?}; \
         a unit_expr means the radix digits were misparsed as a unit suffix"
    );
}

// ── (A) Positive CST shape — radix literals ──────────────────────────────────

/// `0xFF` must parse as a single `number_literal` spanning all 4 characters.
///
/// Without the external scanner, `0xFF` → `quantity_literal(number_literal "0",
/// unit_expr "xFF")` because `x` is a unit-start character.
#[test]
fn hex_lowercase_prefix_parses_as_number_literal() {
    assert_radix_literal_is_single_number_literal("0xFF");
}

/// `0XFF` (uppercase prefix) must parse as a single `number_literal`.
#[test]
fn hex_uppercase_prefix_parses_as_number_literal() {
    assert_radix_literal_is_single_number_literal("0XFF");
}

/// `0b1010` must parse as a single `number_literal` spanning all 6 characters.
///
/// Without the external scanner, `0b1010` → `quantity_literal(number_literal "0",
/// unit_expr "b1010")` because `b` is a unit-start character.
#[test]
fn bin_lowercase_prefix_parses_as_number_literal() {
    assert_radix_literal_is_single_number_literal("0b1010");
}

/// `0B1010` (uppercase prefix) must parse as a single `number_literal`.
#[test]
fn bin_uppercase_prefix_parses_as_number_literal() {
    assert_radix_literal_is_single_number_literal("0B1010");
}

/// `0xDEAD_BEEF` (hex with `_` separators) must parse as a single `number_literal`.
///
/// Tests that the external scanner respects `_`-separated hex groups and does not
/// stop at the `_`, nor absorb a trailing `_` into the token.
#[test]
fn hex_with_digit_separators_parses_as_number_literal() {
    assert_radix_literal_is_single_number_literal("0xDEAD_BEEF");
}

// ── (B) Quantity suffix — radix literal followed by a unit ───────────────────

/// `0xFFmm` must parse as a `quantity_literal` with inner `number_literal "0xFF"`
/// and `unit_expr "mm"`.
///
/// The external scanner must stop consuming at the first non-hex character (`m`),
/// leaving the unit machinery to pick up `mm` via the normal `unit_expr` path.
#[test]
fn hex_with_unit_suffix_parses_as_quantity_literal() {
    let lit = "0xFFmm";
    let source = format!("structure S {{\n  let x = {lit}\n}}");
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    // No error nodes.
    assert!(
        !root.has_error(),
        "expected no parse error for {lit:?}; got errors in: {source:?}"
    );

    // The let_declaration value must be a quantity_literal.
    let let_decl = find_cst_node(root, "let_declaration")
        .expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "quantity_literal",
        "`0xFFmm` must parse as a quantity_literal; got `{}`",
        value_node.kind()
    );

    // The inner number_literal value must be "0xFF".
    let inner_number = value_node
        .child_by_field_name("value")
        .expect("quantity_literal must have a value field");
    assert_eq!(
        inner_number.kind(),
        "number_literal",
        "quantity_literal.value must be a number_literal; got `{}`",
        inner_number.kind()
    );
    assert_eq!(
        inner_number.utf8_text(source.as_bytes()).unwrap(),
        "0xFF",
        "quantity_literal inner number_literal text must be `0xFF`"
    );

    // The unit_expr must be present and its text must be "mm".
    let unit_node = find_cst_node(root, "unit_expr")
        .expect("`0xFFmm` must produce a unit_expr node");
    assert_eq!(
        unit_node.utf8_text(source.as_bytes()).unwrap(),
        "mm",
        "unit_expr text must be `mm`"
    );
}

// ── (C) Regression guards ─────────────────────────────────────────────────────

/// `0` (bare zero) must still parse as a single `number_literal "0"`.
///
/// Regression guard: the radix scanner must return false and let the decimal DFA
/// handle bare `0` (0 is a valid decimal literal; no radix prefix follows).
#[test]
fn bare_zero_still_parses_as_number_literal() {
    let source = "structure S {\n  let x = 0\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(!root.has_error(), "expected no parse error for bare `0`");

    let let_decl = find_cst_node(root, "let_declaration").expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "number_literal",
        "bare `0` must still be a number_literal, got `{}`",
        value_node.kind()
    );
    assert_eq!(
        value_node.utf8_text(source.as_bytes()).unwrap(),
        "0",
        "bare zero text must be `0`"
    );
    assert!(
        find_cst_node(root, "unit_expr").is_none(),
        "bare `0` must not produce a unit_expr"
    );
}

/// `255` (bare integer) must still parse as a single `number_literal "255"`.
#[test]
fn bare_integer_still_parses_as_number_literal() {
    let source = "structure S {\n  let x = 255\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(!root.has_error(), "expected no parse error for `255`");

    let let_decl = find_cst_node(root, "let_declaration").expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "number_literal",
        "bare `255` must still be a number_literal, got `{}`",
        value_node.kind()
    );
    assert_eq!(
        value_node.utf8_text(source.as_bytes()).unwrap(),
        "255",
        "integer text must be `255`"
    );
}

/// `0.5` (decimal fraction) must still parse as a single `number_literal "0.5"`.
///
/// Regression guard: the radix scanner sees `0` then `.` — `.` is neither `x`/`X`
/// nor `b`/`B`, so it must return false and let the decimal DFA match `0.5`.
#[test]
fn decimal_fraction_still_parses_as_number_literal() {
    let source = "structure S {\n  let x = 0.5\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(!root.has_error(), "expected no parse error for `0.5`");

    let let_decl = find_cst_node(root, "let_declaration").expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "number_literal",
        "`0.5` must still be a number_literal, got `{}`",
        value_node.kind()
    );
    assert_eq!(
        value_node.utf8_text(source.as_bytes()).unwrap(),
        "0.5",
        "decimal text must be `0.5`"
    );
    assert!(
        find_cst_node(root, "unit_expr").is_none(),
        "`0.5` must not produce a unit_expr"
    );
}

/// `5mm` must still parse as a `quantity_literal` containing a `number_literal "5"`
/// and a `unit_expr`.
///
/// Regression guard: the radix scanner must not interfere with the existing
/// quantity-literal machinery for non-zero-prefixed numerics.
#[test]
fn quantity_literal_5mm_still_parses_as_quantity_literal() {
    let source = "structure S {\n  let w = 5mm\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(!root.has_error(), "expected no parse error for `5mm`");

    let let_decl = find_cst_node(root, "let_declaration").expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "quantity_literal",
        "`5mm` must still parse as a quantity_literal; got `{}`",
        value_node.kind()
    );

    assert!(
        find_cst_node(root, "unit_expr").is_some(),
        "`5mm` must still produce a unit_expr node"
    );

    let inner_number = value_node
        .child_by_field_name("value")
        .expect("quantity_literal must have a value field");
    assert_eq!(
        inner_number.kind(),
        "number_literal",
        "quantity_literal.value must be a number_literal"
    );
    assert_eq!(
        inner_number.utf8_text(source.as_bytes()).unwrap(),
        "5",
        "quantity_literal inner number text must be `5`"
    );
}
