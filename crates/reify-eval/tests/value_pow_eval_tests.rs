//! Runtime evaluation tests for the value-level `^` operator (task 3805).
//!
//! These are **characterization tests** that lock the runtime contract delivered
//! by the pre-existing `eval_pow` in `reify-expr/src/lib.rs:2663`.  No
//! `reify-expr` source change is required — the grammar (step-2) + typing
//! (steps 5 & 7) feed `BinOp{op:"^"}` through the existing Pow dispatch, and
//! these tests confirm the end-to-end result.
//!
//! # RED → GREEN
//!
//! The tests are GREEN on arrival once steps 2 and 5 have landed, because the
//! grammar emits the `^` node, the compiler assigns the correct `Scalar<Q^n>`
//! type, and `eval_pow` already handles `Scalar^Int`, `Int^Int`, and
//! `Real^Real`.  There are no source changes needed in `reify-expr`.
//!
//! # Numeric bounds
//!
//! All SI-value comparisons use epsilon `1e-9` (same as money_arithmetic_eval).
//! - `5mm ^ 2`:  SI(5mm) = 0.005 m;  0.005^2 = 2.5e-5 m² (exact in f64)
//! - `5mm ^ -2`: 1 / (0.005^2) = 40000.0 m⁻² (exact in f64)
//! - `2.0 ^ 3.0`: 8.0 (exact)
//! - `2 ^ 3`: 8 (exact integer)

use reify_test_support::eval_source;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;

const EPSILON: f64 = 1e-9;

// ─── test 1: `5mm ^ 2 → Scalar{AREA, 2.5e-5}` ───────────────────────────────

/// At runtime, `5mm ^ 2` must evaluate to
/// `Value::Scalar { si_value ≈ 2.5e-5, dimension: AREA }`.
///
/// `mm` is a built-in unit: 1 mm = 0.001 m, so 5 mm = 0.005 m (SI).
/// 0.005^2 = 2.5e-5 m² = 2.5e-5 (SI for AREA).
///
/// GREEN on arrival (no reify-expr change needed): `eval_pow` raises the
/// `si_value` via `powi(n)` and scales the `DimensionVector` via `pow(i8)`.
#[test]
fn pow_5mm_2_evaluates_to_area() {
    let source = "structure S { param p : Area = 5mm ^ 2 }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 2.5e-5).abs() < EPSILON,
                "expected si_value 2.5e-5 (0.005^2), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::AREA,
                "expected AREA dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── test 2: `2.0 ^ 3.0 → Real(8.0)` ────────────────────────────────────────

/// At runtime, `2.0 ^ 3.0` must evaluate to `Value::Real(8.0)`.
///
/// Dimensionless Real^Real path in `eval_pow`.
#[test]
fn pow_real_real_evaluates_to_8() {
    let source = "structure S { let p = 2.0 ^ 3.0 }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Real(v) => {
            assert!(
                (*v - 8.0).abs() < EPSILON,
                "expected 8.0, got {}",
                v
            );
        }
        other => panic!("expected Value::Real(8.0), got {:?}", other),
    }
}

// ─── test 3: `2 ^ 3 → Int(8)` ────────────────────────────────────────────────

/// At runtime, `2 ^ 3` must evaluate to `Value::Int(8)`.
///
/// Int^Int path in `eval_pow`.
#[test]
fn pow_int_int_evaluates_to_8() {
    let source = "structure S { let p = 2 ^ 3 }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Int(n) => {
            assert_eq!(*n, 8, "expected 8, got {}", n);
        }
        other => panic!("expected Value::Int(8), got {:?}", other),
    }
}

// ─── test 4: `5mm ^ -2 → Scalar{LENGTH^-2, 40000.0}` ────────────────────────

/// At runtime, `5mm ^ -2` must evaluate to
/// `Value::Scalar { si_value ≈ 40000.0, dimension: LENGTH.pow(-2) }`.
///
/// SI(5mm) = 0.005 m; 0.005^(-2) = 1/2.5e-5 = 40000.0 m⁻².
///
/// The negative exponent parses as `UnOp{"-", NumberLiteral{2, false}}`
/// (since `^` binds tighter than unary `-`), and the compiler extracts
/// `n = -2` from that shape.
#[test]
fn pow_5mm_neg2_evaluates_to_inv_length_sq() {
    let source = "structure S { param p : Real = 5mm ^ -2 }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 40000.0).abs() < EPSILON,
                "expected si_value 40000.0 (1/0.005^2), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH.pow(-2),
                "expected LENGTH^-2 dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}
