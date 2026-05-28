//! Rust integration tests for task 3909: `_` digit-separator support in `number_literal`.
//!
//! User-observable signal: `cargo test -p reify-syntax --test numeric_separators_grammar_tests`
//! passes (GREEN after grammar.js is patched; RED before).
//!
//! These tests assert **CST shape only** — the raw tree-sitter tree — and are
//! β-independent: they do NOT call `reify_syntax::parse` or assert f64 values
//! (lowering/stripping `_` is task β).  The β-independence criterion is that
//! `f64::from_str("1_000_000")` rejects underscores; asserting a lowered value
//! here would produce an unsatisfiable RED test.
//!
//! Coverage:
//! * **(A)** Positive CST shape — `1_000_000`, `0.000_001`, `1_000e1_0` each
//!   parse as a single `number_literal` node whose text spans the whole
//!   literal; no `unit_expr` node (defeats the unit-suffix misparse).
//! * **(B)** Regression — bare `1000` still parses as a single `number_literal`.
//! * **(C)** Regression — `5mm` still parses as a `quantity_literal` containing
//!   a `number_literal` and a `unit_expr`.
//!
//! See also: `tree-sitter-reify/test/corpus/numeric_separators.txt` for the
//! corpus-level CST documentation, runnable via `tree-sitter test`.

mod common;
use common::{find_cst_node, make_ts_parser};

// ── Assertion helpers ────────────────────────────────────────────────────────

/// Parse `structure S { let x = <lit> }` and assert:
///
/// 1. No ERROR nodes in the CST.
/// 2. The `value` field of `let_declaration` has kind `"number_literal"`.
/// 3. The `number_literal` text spans the whole `<lit>` (not just the leading
///    digit — this is the signal that `_000_000` was NOT consumed as a unit suffix).
/// 4. No `unit_expr` node anywhere in the tree (defeats the misparse).
fn assert_digit_sep_literal_is_single_number_literal(lit: &str) {
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
         with a unit-suffix starting at `_`",
        value_node.kind()
    );

    // (3) The number_literal text must span the whole literal.
    let actual_text = value_node
        .utf8_text(source.as_bytes())
        .expect("utf8_text failed");
    assert_eq!(
        actual_text, lit,
        "number_literal text must span the entire literal including `_` separators; \
         got {actual_text:?}, expected {lit:?}"
    );

    // (4) No unit_expr node — the trailing `_NNN` must NOT be consumed as a unit suffix.
    assert!(
        find_cst_node(root, "unit_expr").is_none(),
        "must not produce a `unit_expr` node for {lit:?}; \
         a unit_expr means `_NNN` was misparsed as a unit suffix"
    );
}

// ── (A) Positive CST shape — digit-separator literals ───────────────────────

/// `1_000_000` must parse as a single `number_literal` spanning all 9 characters.
///
/// With the old regex (`\d+...`), the lexer matches only `1` and the external
/// scanner's `is_unit_start()` (which includes `_`) consumes `_000_000` as a
/// unit suffix, giving `quantity_literal(1, "_000_000")`.  After the grammar
/// change this must be a single `number_literal "1_000_000"`.
#[test]
fn integer_with_digit_separators_parses_as_number_literal() {
    assert_digit_sep_literal_is_single_number_literal("1_000_000");
}

/// `0.000_001` must parse as a single `number_literal` spanning all 9 characters.
///
/// With the old regex, the lexer matches `0.000` and `_001` is a unit suffix,
/// giving `quantity_literal(0.000, "_001")`.  After the change this must be a
/// single `number_literal "0.000_001"`.
#[test]
fn decimal_with_digit_separators_parses_as_number_literal() {
    assert_digit_sep_literal_is_single_number_literal("0.000_001");
}

/// `1_000e1_0` must parse as a single `number_literal` spanning all 10 characters.
///
/// With the old regex, the lexer matches `1` and `_000e1_0` is a unit suffix.
/// After the change `\d(_?\d)*` matches the integer part `1_000` and
/// `[eE][+-]?\d(_?\d)*` matches the exponent `e1_0`, giving a full match.
#[test]
fn scientific_with_digit_separators_parses_as_number_literal() {
    assert_digit_sep_literal_is_single_number_literal("1_000e1_0");
}

// ── (B) Regression — bare integer ────────────────────────────────────────────

/// `1000` (no underscores) must still parse as a single `number_literal "1000"`.
///
/// Regression guard: `\d(_?\d)*` is equivalent to `\d+` for underscore-free
/// input — this test confirms no regression in the common case.
#[test]
fn bare_integer_still_parses_as_number_literal() {
    let source = "structure S {\n  let x = 1000\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "expected no parse error for bare integer `1000`"
    );

    let let_decl = find_cst_node(root, "let_declaration")
        .expect("expected let_declaration");
    let value_node = let_decl
        .child_by_field_name("value")
        .expect("let_declaration must have a value field");
    assert_eq!(
        value_node.kind(),
        "number_literal",
        "bare `1000` must still be a number_literal, got `{}`",
        value_node.kind()
    );
    assert_eq!(
        value_node.utf8_text(source.as_bytes()).unwrap(),
        "1000",
        "bare integer text must be `1000`"
    );
}

// ── (C) Regression — quantity literal ────────────────────────────────────────

/// `5mm` must still parse as a `quantity_literal` containing a `number_literal`
/// value and a `unit_expr` unit.
///
/// Regression guard: the grammar change must not accidentally absorb `mm` into the
/// number token or otherwise break quantity literals.
#[test]
fn quantity_literal_5mm_still_parses_as_quantity_literal() {
    let source = "structure S {\n  let w = 5mm\n}";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    let root = tree.root_node();

    assert!(
        !root.has_error(),
        "expected no parse error for quantity literal `5mm`"
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
        "`5mm` must still parse as a quantity_literal; got `{}`",
        value_node.kind()
    );

    // The unit_expr node must be present.
    assert!(
        find_cst_node(root, "unit_expr").is_some(),
        "`5mm` must still produce a unit_expr node"
    );

    // The inner number_literal value must be "5".
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
        "quantity_literal.value text must be `5`"
    );
}
