//! Integration tests for Money-dimension currency-mass arithmetic (task 2379).
//!
//! Each test source begins with `pub unit USD : Money` so the inline
//! declaration seeds the unit registry of that test compilation.  This keeps
//! the tests order-independent of the stdlib's `pub unit USD : Money`
//! (task 2378, commit 94b2fac20) and matches the convention established by
//! `crates/reify-compiler/tests/money_force_diagnostic_tests.rs`.
//!
//! Underlying impl wired by deps 57 (Money slot 9), 208 (unit registry),
//! 209 (user-defined units with cross-unit factor expressions), and 2378
//! (`unit USD : Money` instances).

mod common;

use common::{UNIT_EPSILON, expect_binop, expect_scalar};
use reify_test_support::{compile_source, errors_only};
use reify_types::{BinOp, DimensionVector, Type};

// ─── test 1: 25USD/1kg compound dimension via inline-decl path ───────────────

/// `pub unit USD : Money` declared inline + `25USD/1kg` in a `Money/Mass` param.
/// Verify the param's `cell_type` carries `MONEY/MASS`, the `default_expr` is
/// a `Div` BinOp whose `result_type` also carries `MONEY/MASS`, and the
/// operands resolve to `(25.0, MONEY)` and `(1.0, MASS)`.
///
/// Distinct from
/// `money_units_tests.rs::stdlib_USD_per_kg_compound_resolves_to_money_per_mass`
/// (which uses `compile_with_stdlib_helper`) — this test exercises the
/// user-decl seed-into-registry path via `compile_source`, not the stdlib
/// prelude.
#[test]
fn compound_money_per_mass_via_inline_user_unit_decl() {
    let source = "pub unit USD : Money\n\
                  type CostPerMass = Money / Mass\n\
                  structure def S { param p : CostPerMass = 25USD/1kg }";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p cell not found");

    let expected_dim = DimensionVector::MONEY.div(&DimensionVector::MASS);
    assert_eq!(
        cell.cell_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "cell_type wrong"
    );

    let expr = cell.default_expr.as_ref().expect("p has no default_expr");
    assert_eq!(
        expr.result_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "Div result_type wrong"
    );

    let (op, left, right) = expect_binop(expr);
    assert_eq!(*op, BinOp::Div, "expected Div op");
    let (lsi, ldim) = expect_scalar(left);
    let (rsi, rdim) = expect_scalar(right);
    assert!((lsi - 25.0).abs() < UNIT_EPSILON, "left si_value {} ≠ 25.0", lsi);
    assert_eq!(ldim, DimensionVector::MONEY);
    assert!((rsi - 1.0).abs() < UNIT_EPSILON, "right si_value {} ≠ 1.0", rsi);
    assert_eq!(rdim, DimensionVector::MASS);
}

// ─── test 2: cancellation `(25USD/1kg) * 2kg → MONEY` ────────────────────────

/// `(25USD/1kg) * 2kg` should compile with the OUTER BinOp's `result_type`
/// carrying the bare `MONEY` dimension — `MASS` in the compound divisor
/// cancels with the `2kg` multiplicand, so slot 1 (MASS) returns to 0 while
/// slot 9 (MONEY) stays 1.  Structural assertions: outer BinOp is `Mul`,
/// inner-left is `Div` with `result_type` `MONEY/MASS`, inner-right is the
/// `2kg` literal with `MASS` dim.  Marquee test for the task's
/// `25USD/kg * 2kg = 50USD` example (canonical Reify form: `(25USD/1kg) * 2kg`).
#[test]
fn money_per_mass_times_mass_cancels_to_money_at_compile_time() {
    let source = "pub unit USD : Money\n\
                  structure def S { param p : Money = (25USD/1kg) * 2kg }";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let template = module.templates.iter().find(|t| t.name == "S").unwrap();
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .unwrap();
    assert_eq!(
        cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "after cancellation cell_type should be Scalar{{MONEY}}, got {:?}",
        cell.cell_type
    );

    let expr = cell.default_expr.as_ref().unwrap();
    assert_eq!(
        expr.result_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "outer BinOp result_type should be Scalar{{MONEY}}"
    );

    let (op, left, right) = expect_binop(expr);
    assert_eq!(*op, BinOp::Mul, "outer op should be Mul");

    let expected_compound = DimensionVector::MONEY.div(&DimensionVector::MASS);
    assert_eq!(
        left.result_type,
        Type::Scalar {
            dimension: expected_compound
        },
        "inner-left Div result_type should be MONEY/MASS"
    );
    let (left_op, _, _) = expect_binop(left);
    assert_eq!(*left_op, BinOp::Div, "inner-left op should be Div");

    let (rsi, rdim) = expect_scalar(right);
    assert!((rsi - 2.0).abs() < UNIT_EPSILON, "right si_value {} ≠ 2.0", rsi);
    assert_eq!(rdim, DimensionVector::MASS);
}

// ─── test 3: user-defined GBP via 1.25USD factor ─────────────────────────────

/// `unit GBP : Money = 1.25USD` should compile and register a `CompiledUnit`
/// with `dimension = MONEY`, `factor ≈ 1.25` (pulled from the cross-unit
/// QuantityLiteral against USD's factor of 1.0), and `offset = None`.
/// Distinct from `unit_registry_tests.rs::evaluate_const_quantity_literal_cross_ref`
/// (which exercises Length/mm cross-ref) — this test locks the same machinery
/// for the Money dimension.
#[test]
fn user_defined_gbp_registers_with_money_dim_and_factor_125() {
    let source = "pub unit USD : Money\nunit GBP : Money = 1.25USD";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let gbp = module
        .units
        .iter()
        .find(|u| u.name == "GBP")
        .expect("unit 'GBP' not found in compiled module");
    assert_eq!(
        gbp.dimension,
        DimensionVector::MONEY,
        "GBP dimension should be MONEY, got {:?}",
        gbp.dimension
    );
    assert!(
        (gbp.factor - 1.25).abs() < 1e-12,
        "GBP factor should be ≈ 1.25, got {}",
        gbp.factor
    );
    assert!(
        gbp.offset.is_none(),
        "GBP offset should be None, got {:?}",
        gbp.offset
    );
}

// ─── test 4: 5GBP literal resolves via user factor (1.25USD = 1.25 SI) ───────

/// After declaring `unit GBP : Money = 1.25USD`, the quantity literal `5GBP`
/// in a `Money` param should compile to a `Literal(Scalar { si_value: 6.25,
/// MONEY })` — locking the registry-first lookup at `expr.rs:264..267` and
/// the user-defined factor flow.
#[test]
fn gbp_quantity_literal_resolves_via_user_factor() {
    let source = "pub unit USD : Money\n\
                  unit GBP : Money = 1.25USD\n\
                  structure def S { param x : Money = 5GBP }";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let template = module.templates.iter().find(|t| t.name == "S").unwrap();
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x cell not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    let (si, dim) = expect_scalar(expr);
    assert!(
        (si - 6.25).abs() < UNIT_EPSILON,
        "5GBP si_value should be 6.25 (5 * 1.25), got {}",
        si
    );
    assert_eq!(dim, DimensionVector::MONEY, "5GBP dimension should be MONEY");
}

// ─── test 5: cross-currency `5GBP + 5USD` compiles with MONEY dim ────────────

/// `5GBP + 5USD` is dimensionally compatible (both MONEY) and should compile
/// with the outer BinOp's `result_type == Scalar{MONEY}`, locking the
/// contract that "different currencies of the same dimension" type-check at
/// compile time.  The actual SI-summed value (11.25) is verified separately
/// in the eval-side tests; this test focuses on compile-time dimension
/// propagation.
#[test]
fn cross_currency_addition_compiles_with_money_dim() {
    let source = "pub unit USD : Money\n\
                  unit GBP : Money = 1.25USD\n\
                  structure def S { param p : Money = 5GBP + 5USD }";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let template = module.templates.iter().find(|t| t.name == "S").unwrap();
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .unwrap();
    assert_eq!(
        cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "cell_type should be Scalar{{MONEY}}"
    );

    let expr = cell.default_expr.as_ref().unwrap();
    assert_eq!(
        expr.result_type,
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "BinOp result_type should be Scalar{{MONEY}}"
    );
    let (op, _, _) = expect_binop(expr);
    assert_eq!(*op, BinOp::Add, "outer op should be Add");
}
