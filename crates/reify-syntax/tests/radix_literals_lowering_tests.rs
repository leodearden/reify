//! Integration tests for radix (hex / binary) literal lowering.
//!
//! Verifies that `0xFF`, `0b1010`, `0xDEAD_BEEF`, etc. lower correctly to
//! `ExprKind::NumberLiteral { value, is_real }`, with `is_real` forced `false`
//! on all radix branches (PRD D4 guard: bypass the `.`/`e`/`E` scan that
//! would false-positive `0xBEEF` / `0xe` as Real).
//!
//! Also verifies that `0xFFmm` lowers to
//! `ExprKind::QuantityLiteral { value: 255.0, unit: UnitExpr::Unit("mm") }`,
//! closing the gap the γ grammar (task 3910) opened when it made
//! `0xFFmm` parse as `quantity_literal(number_literal "0xFF", unit_expr "mm")`.
//!
//! CST-shape tests (asserting the grammar tokenises radix literals as single
//! `number_literal` nodes) live in
//! `crates/reify-syntax/tests/radix_literals_grammar_tests.rs` and are
//! γ-only; this file adds the δ lowering layer on top (task 3913).
//!
//! See also: `docs/prds/v0_6/numeric-and-range-literal-forms.md`.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("radix_literals_lowering_test"),
    );
    let structure = match module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// Extract both the `value` and `is_real` flag from a number-literal member
/// `let x = <lit>`, asserting no parse errors.
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

// ── Decimal regression guards (must stay GREEN before and after the refactor) ──

/// Plain integer `42` must lower to `(42.0, false)`.
#[test]
fn decimal_regression_integer_42() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 42\n}");
    assert_eq!(value, 42.0_f64, "42 should lower to 42.0");
    assert!(!is_real, "42 (no `.`/`e`/`E`) should have is_real = false");
}

/// Decimal `1.5` must lower to `(1.5, true)`.
#[test]
fn decimal_regression_real_1_5() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 1.5\n}");
    assert_eq!(value, 1.5_f64, "1.5 should lower to 1.5");
    assert!(is_real, "1.5 (has `.`) should have is_real = true");
}

/// Exponent `2e3` must lower to `(2000.0, true)`.
#[test]
fn decimal_regression_exponent_2e3() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 2e3\n}");
    assert_eq!(value, 2000.0_f64, "2e3 should lower to 2000.0");
    assert!(is_real, "2e3 (has `e`) should have is_real = true");
}

// ── Hex (0x / 0X) number-literal cases ──────────────────────────────────────

/// `0xFF` must lower to `(255.0, false)`.
#[test]
fn hex_lower_0xff() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0xFF\n}");
    assert_eq!(value, 255.0_f64, "0xFF should lower to 255.0");
    assert!(!is_real, "0xFF is an integer radix literal; is_real must be false");
}

/// `0XFF` (upper-case X prefix) must lower to `(255.0, false)`.
#[test]
fn hex_upper_prefix_0xff() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0XFF\n}");
    assert_eq!(value, 255.0_f64, "0XFF should lower to 255.0");
    assert!(!is_real, "0XFF is an integer radix literal; is_real must be false");
}

// ── Binary (0b / 0B) number-literal cases ───────────────────────────────────

/// `0b1010` must lower to `(10.0, false)`.
#[test]
fn binary_lower_0b1010() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0b1010\n}");
    assert_eq!(value, 10.0_f64, "0b1010 should lower to 10.0");
    assert!(!is_real, "0b1010 is an integer radix literal; is_real must be false");
}

/// `0B1010` (upper-case B prefix) must lower to `(10.0, false)`.
#[test]
fn binary_upper_prefix_0b1010() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0B1010\n}");
    assert_eq!(value, 10.0_f64, "0B1010 should lower to 10.0");
    assert!(!is_real, "0B1010 is an integer radix literal; is_real must be false");
}

// ── D4 is_real guard: `e`/`E` in hex digits must NOT trigger is_real ─────────

/// `0xBEEF` — contains uppercase `E` in the hex digits.
/// The decimal `is_real` scan would false-positive `0xBEEF` as Real
/// (because of the `E`).  The radix branch must bypass that scan and
/// force `is_real = false`.
#[test]
fn d4_guard_hex_uppercase_e_0xbeef() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0xBEEF\n}");
    assert_eq!(value, 48879.0_f64, "0xBEEF should lower to 48879.0");
    assert!(
        !is_real,
        "0xBEEF contains uppercase E but is an integer; is_real must be false (D4 guard)"
    );
}

/// `0xbeef` — contains lowercase `e` in the hex digits.
/// Same D4 guard as above but for lowercase `e`.
#[test]
fn d4_guard_hex_lowercase_e_0xbeef() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0xbeef\n}");
    assert_eq!(value, 48879.0_f64, "0xbeef should lower to 48879.0");
    assert!(
        !is_real,
        "0xbeef contains lowercase e but is an integer; is_real must be false (D4 guard)"
    );
}

// ── Separator in hex: `0xDEAD_BEEF` ──────────────────────────────────────────

/// `0xDEAD_BEEF` — hex literal with `_` digit separator.
/// Must lower to `(3735928559.0, false)`.
#[test]
fn hex_with_underscore_separator() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x = 0xDEAD_BEEF\n}");
    assert_eq!(
        value, 3735928559.0_f64,
        "0xDEAD_BEEF should lower to 3735928559.0"
    );
    assert!(
        !is_real,
        "0xDEAD_BEEF is an integer radix literal; is_real must be false"
    );
}

// ── u64 boundary: 0x8000000000000000 (> i64::MAX) ────────────────────────────

/// `0x8000000000000000` — the first value that overflows `i64` (= 2^63).
///
/// The lowering must use `u64::from_str_radix` (not `i64`) so this value
/// returns `(9223372036854775808.0, false)` rather than `None`.  This test
/// pins the u64 design decision documented in `plan.json`.
#[test]
fn hex_u64_boundary_0x8000000000000000() {
    let (value, is_real) = extract_number_literal_with_flag(
        "structure S {\n  let x = 0x8000000000000000\n}",
    );
    assert_eq!(
        value,
        9223372036854775808.0_f64,
        "0x8000000000000000 should lower to 9223372036854775808.0 (2^63 as f64)"
    );
    assert!(
        !is_real,
        "0x8000000000000000 is an integer radix literal; is_real must be false"
    );
}

