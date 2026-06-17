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

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("sci_notation_test"));
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
        ExprKind::NumberLiteral { value, .. } => value,
        ref other => panic!("expected NumberLiteral, got {:?}", other),
    }
}

// ── Scientific notation: positive cases ──────────────────────────────────────

#[test]
fn negative_exponent_1e_minus_6() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e-6\n}");
    assert_eq!(v, 1e-6_f64, "1e-6 should lower to 1e-6_f64");
}

#[test]
fn uppercase_e_1e_minus_6() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1E-6\n}");
    assert_eq!(v, 1E-6_f64, "1E-6 should lower to 1E-6_f64");
}

#[test]
fn positive_exponent_1e_plus_6() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e+6\n}");
    assert_eq!(v, 1e+6_f64, "1e+6 should lower to 1e+6_f64");
}

#[test]
fn bare_positive_exponent_1e6() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e6\n}");
    assert_eq!(v, 1e6_f64, "1e6 should lower to 1e6_f64");
}

#[test]
fn decimal_mantissa_1_5e2() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1.5e2\n}");
    assert_eq!(v, 1.5e2_f64, "1.5e2 should lower to 1.5e2_f64");
}

#[test]
fn large_negative_exponent_1_0e_minus_300() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1.0e-300\n}");
    assert_eq!(v, 1.0e-300_f64, "1.0e-300 should lower to 1.0e-300_f64");
}

#[test]
fn zero_exponent_1e0() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e0\n}");
    assert_eq!(v, 1e0_f64, "1e0 should lower to 1e0_f64 (== 1.0)");
}

// ── Acceptance scenario from solver_elastic.ri ───────────────────────────────

/// Acceptance scenario: `cg_tolerance: Real = 1e-6` parses without rewriting
/// to 0.000001 (the workaround noted in crates/reify-compiler/stdlib/solver_elastic.ri:33-34).
#[test]
fn cg_tolerance_acceptance_scenario() {
    let v = extract_number_literal("structure S {\n  let cg_tolerance: Real = 1e-6\n}");
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
            assert_eq!(unit, &UnitExpr::Unit("mm".to_string()), "quantity unit should be 'mm'");
        }
        other => panic!("expected QuantityLiteral, got {:?}", other),
    }
}

// ── Overflow / underflow boundary behaviour ──────────────────────────────────

/// `1e400` overflows f64: `f64::from_str("1e400")` returns `Ok(f64::INFINITY)`.
/// The current `lower_number_literal` (`.parse().ok()`) propagates `Inf` silently
/// into the type system — no parse-layer diagnostic is emitted.
///
/// This test pins the current behavior. If a future change adds Inf-rejection in
/// `lower_number_literal`, this test will fail and force explicit documentation of
/// the new policy.
// TODO(triage): silent f64 overflow → Inf is a latent issue; consider a // ptodo:allow known latent issue, no live triage task yet
// parse-layer diagnostic. This pin is intentionally a tripwire, not a blessing.
#[test]
fn overflow_1e400_lowers_to_infinity() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e400\n}");
    assert!(
        v.is_infinite() && v > 0.0,
        "1e400 should lower to f64::INFINITY (current silent-overflow behavior); got {v}"
    );
}

/// `1e-400` underflows f64: `f64::from_str("1e-400")` returns `Ok(0.0)` because
/// the value is below f64's minimum subnormal (~5e-324) and flushes to zero.
/// Like the overflow case, no parse-layer diagnostic is emitted.
///
/// This test pins the current silent-underflow behavior.
// TODO(triage): silent f64 underflow → 0.0 is a latent issue; consider a // ptodo:allow known latent issue, no live triage task yet
// parse-layer diagnostic. Tripwire, not a blessing — see overflow test above.
#[test]
fn underflow_1e_minus_400_lowers_to_zero() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e-400\n}");
    assert_eq!(
        v, 0.0_f64,
        "1e-400 should underflow to 0.0 (current silent-underflow behavior)"
    );
}

// ── Sign-present, no exponent digits ─────────────────────────────────────────

/// `1e+` (exponent sign present but no digits) is not a valid scientific-notation
/// literal. The regex `\d+(\.\d+)?([eE][+-]?\d+)?` requires `\d+` after the
/// optional sign; `1e+` fails the exponent group, so the lexer matches only `1`.
/// The `e` is then consumed by `token.immediate` as a unit suffix, giving
/// `quantity_literal(1, "e")`, and `+` becomes an orphaned operator — producing
/// an ERROR node in the CST.
///
/// This test pins the error-producing behavior so a future grammar change that
/// accidentally makes `1e+` tokenize as a valid (or silently-dropped) number
/// is caught immediately.
#[test]
fn sign_no_digits_1e_plus_produces_parse_error() {
    let (_members, errors) = parse_members("structure S {\n  let x = 1e+\n}");
    assert!(
        !errors.is_empty(),
        "1e+ (sign with no exponent digits) should produce parse errors; got empty error list"
    );
}

// ── AST records int-vs-real distinction ──────────────────────────────────────

/// Helper: extract both the `value` and `is_real` flag from a number-literal
/// member, asserting no parse errors.
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

/// `42` (bare integer token, no decimal point, no exponent) must record
/// `is_real = false`.  The `: Real` annotation is honored at compile time via
/// Int→Real widening in `conformance/checker.rs`, not by tagging the literal
/// as Real in the AST.
#[test]
fn int_literal_42_has_is_real_false() {
    let (value, is_real) = extract_number_literal_with_flag("structure S {\n  let x: Real = 42\n}");
    assert_eq!(value, 42.0_f64, "value should be 42.0");
    assert!(
        !is_real,
        "42 (no decimal, no exponent) should have is_real = false"
    );
}

/// `1.0` (whole-number decimal literal) must record `is_real = true`.
///
/// This is the core bug fix for task 3184: before this change, `1.0` was
/// silently re-typed as `Int(1)` because the AST discarded the `.` token and
/// the compiler applied a value-based heuristic (if f64 == i64, emit Int).
/// After the fix, the `is_real` flag preserves the author's intent.
#[test]
fn whole_number_real_literal_1_0_has_is_real_true() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x: Real = 1.0\n}");
    assert_eq!(value, 1.0_f64, "value should be 1.0");
    assert!(
        is_real,
        "1.0 (has decimal point) should have is_real = true"
    );
}

/// `2.5` (fractional decimal literal) must record `is_real = true`.
/// Regression check: fractional literals that were already Real continue to
/// carry `is_real = true`.
#[test]
fn fractional_literal_2_5_has_is_real_true() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x: Real = 2.5\n}");
    assert_eq!(value, 2.5_f64, "value should be 2.5");
    assert!(
        is_real,
        "2.5 (has decimal point) should have is_real = true"
    );
}

/// `1e6` (scientific notation) must record `is_real = true`.
///
/// Scientific notation is canonically a real-number literal: the exponent
/// suffix `e`/`E` signals a floating-point representation regardless of
/// whether the resulting f64 is a whole number.  Before task 3184, `1e6`
/// would silently emit `Int(1000000)` via the value-based heuristic.
#[test]
fn scientific_literal_1e6_has_is_real_true() {
    let (value, is_real) =
        extract_number_literal_with_flag("structure S {\n  let x: Real = 1e6\n}");
    assert_eq!(value, 1e6_f64, "value should be 1e6");
    assert!(
        is_real,
        "1e6 (has exponent suffix) should have is_real = true"
    );
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
            assert_eq!(
                unit,
                &UnitExpr::Unit("e".to_string()),
                "unit should be 'e' (unregistered, but parses cleanly)"
            );
        }
        other => panic!(
            "expected QuantityLiteral {{ value: 5.0, unit: \"e\" }}, got {:?}",
            other
        ),
    }
}
