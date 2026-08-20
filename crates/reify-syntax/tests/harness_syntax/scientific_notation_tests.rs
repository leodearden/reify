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
//
// Policy (task #4681, Leo 2026-06-23): out-of-range numeric literals are
// REJECTED at the parse layer with a diagnostic — no silent coercion to
// Inf (overflow) or 0.0 (underflow).

/// `1e400` overflows f64 (10^400 ≫ f64::MAX ≈ 1.7977e308): the parse layer
/// must reject it with a diagnostic rather than silently propagating +Inf.
#[test]
fn overflow_1e400_is_rejected() {
    let (_members, errors) =
        parse_members("structure S {\n  let x: Real = 1e400\n}");
    assert!(
        !errors.is_empty(),
        "1e400 should produce a parse error (overflow → Inf rejected); got empty error list"
    );
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("overflow") || m.contains("out of range") || m.contains("1e400")
        }),
        "an error should mention overflow or the literal; got: {:?}",
        errors
    );
}

/// `1e-400` underflows f64 (10^-400 ≪ min subnormal ≈ 4.9e-324): the parse
/// layer must reject it with a diagnostic rather than silently flushing to 0.0.
#[test]
fn underflow_1e_minus_400_is_rejected() {
    let (_members, errors) =
        parse_members("structure S {\n  let x: Real = 1e-400\n}");
    assert!(
        !errors.is_empty(),
        "1e-400 should produce a parse error (underflow → 0.0 rejected); got empty error list"
    );
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("underflow") || m.contains("out of range") || m.contains("1e-400")
        }),
        "an error should mention underflow or the literal; got: {:?}",
        errors
    );
}

// ── Boundary-acceptance regressions ──────────────────────────────────────────

/// `1e308` is finite (10^308 < f64::MAX ≈ 1.7977e308): must parse without error.
#[test]
fn boundary_1e308_is_accepted() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e308\n}");
    assert!(v.is_finite(), "1e308 should parse to a finite value; got {v}");
    assert!(v < f64::MAX, "1e308 should be less than f64::MAX; got {v}");
}

/// Genuine zero literals have an all-zero significand and must NOT be rejected.
#[test]
fn genuine_zeros_accepted_no_errors() {
    for src in &[
        "structure S {\n  let x: Real = 0\n}",
        "structure S {\n  let x: Real = 0.0\n}",
        "structure S {\n  let x: Real = 0e10\n}",
    ] {
        let v = extract_number_literal(src);
        assert_eq!(v, 0.0_f64, "genuine zero literal should lower to 0.0; src={src}");
    }
}

/// `1e-310` is a nonzero subnormal (> min subnormal ≈ 4.9e-324): must parse
/// without error and produce a positive (nonzero) value.
#[test]
fn nonzero_subnormal_1e_minus_310_is_accepted() {
    let v = extract_number_literal("structure S {\n  let x: Real = 1e-310\n}");
    assert!(
        v > 0.0,
        "1e-310 (nonzero subnormal) should parse to a positive value; got {v}"
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

// ── Quantity-literal range rejection (step-3) ────────────────────────────────
//
// Grammar note: `1e400mm` tokenizes as quantity_literal(number_literal "1e400",
// unit_expr "mm") because the exponent regex matches `1e400` and then
// token.immediate consumes the `mm` as a unit suffix.

/// `1e400mm` — overflow in a quantity literal: must be rejected with a diagnostic.
#[test]
fn quantity_overflow_1e400mm_is_rejected() {
    let (_members, errors) =
        parse_members("structure S {\n  let x = 1e400mm\n}");
    assert!(
        !errors.is_empty(),
        "1e400mm should produce a parse error (overflow → Inf rejected); got empty error list"
    );
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("overflow") || m.contains("out of range") || m.contains("1e400")
        }),
        "an error should mention overflow or the literal; got: {:?}",
        errors
    );
}

/// `1e-400mm` — underflow in a quantity literal: must be rejected with a diagnostic.
#[test]
fn quantity_underflow_1e_minus_400mm_is_rejected() {
    let (_members, errors) =
        parse_members("structure S {\n  let x = 1e-400mm\n}");
    assert!(
        !errors.is_empty(),
        "1e-400mm should produce a parse error (underflow → 0.0 rejected); got empty error list"
    );
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("underflow") || m.contains("out of range") || m.contains("1e-400")
        }),
        "an error should mention underflow or the literal; got: {:?}",
        errors
    );
}

/// `5mm` — already-accepted quantity literal must still work after the range check.
#[test]
fn quantity_5mm_still_accepted() {
    let (members, errors) = parse_members("structure S {\n  let x = 5mm\n}");
    assert!(errors.is_empty(), "5mm should parse without errors; got: {:?}", errors);
    match &members[0] {
        MemberDecl::Let(l) => match &l.value.kind {
            ExprKind::QuantityLiteral { value, .. } => {
                assert_eq!(*value, 5.0_f64, "5mm value should be 5.0");
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        },
        other => panic!("expected Let, got {:?}", other),
    }
}

/// `1e308mm` — finite quantity literal must parse without error.
#[test]
fn quantity_1e308mm_is_accepted() {
    let (members, errors) = parse_members("structure S {\n  let x = 1e308mm\n}");
    assert!(errors.is_empty(), "1e308mm should parse without errors; got: {:?}", errors);
    match &members[0] {
        MemberDecl::Let(l) => match &l.value.kind {
            ExprKind::QuantityLiteral { value, .. } => {
                assert!(value.is_finite(), "1e308mm value should be finite; got {value}");
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        },
        other => panic!("expected Let, got {:?}", other),
    }
}

// ── Radix-literal overflow (hex/binary) ──────────────────────────────────────
//
// Hex literals that exceed u64::MAX are accumulated as f64 directly (see
// `parse_number_literal_text`'s `parse_radix` closure).  A hex literal with
// enough digits (>256 F's, since 16^256 ≈ 2^1024 ≈ f64::MAX) overflows the
// accumulation and reaches `classify_number_range` as +Inf.  This path is
// distinct from the decimal `f64::from_str` path and is exercised here.

/// A 300-digit hex literal (16^300 >> f64::MAX) accumulates to +Inf during
/// the radix f64-accumulation path and must be rejected with an overflow error.
#[test]
fn hex_overflow_300_f_digits_is_rejected() {
    let hex_literal = format!("0x{}", "F".repeat(300));
    let src = format!("structure S {{\n  let x: Real = {hex_literal}\n}}");
    let (_members, errors) = parse_members(&src);
    assert!(
        !errors.is_empty(),
        "a 300-digit hex literal should produce an overflow parse error; got empty error list"
    );
    assert!(
        errors.iter().any(|e| {
            let m = e.message.to_lowercase();
            m.contains("overflow") || m.contains("out of range")
        }),
        "an error should mention overflow or out of range; got: {:?}",
        errors
    );
}

/// `0xFFFFFFFFFFFFFFFF` (u64::MAX ≈ 1.84e19) is well within f64::MAX: must
/// parse without error and produce a finite value.
#[test]
fn hex_u64_max_is_accepted() {
    let v = extract_number_literal("structure S {\n  let x: Real = 0xFFFFFFFFFFFFFFFF\n}");
    assert!(
        v.is_finite(),
        "0xFFFFFFFFFFFFFFFF (u64::MAX) should parse to a finite value; got {v}"
    );
    assert!(v > 0.0, "0xFFFFFFFFFFFFFFFF should be positive; got {v}");
}
