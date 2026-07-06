//! Runtime Mul/Div truth-table inventory (INV-COMP-3 producer).
//!
//! Pins the *runtime* half of the static-vs-runtime Mul/Div truth table —
//! `eval_mul` (`crates/reify-expr/src/lib.rs:4354-4629`) and `eval_div`
//! (`:4631-4780`) — as table-driven characterization tests, driven
//! exclusively through the public `eval_expr` path. Neither private function
//! is called directly; the lib.rs `#[cfg(test)]` helpers (`lit`/`mm_val`)
//! are invisible to an integration test, and the public path is exactly
//! what β3's parity test will use, so pinning through it makes this suite a
//! faithful reference.
//!
//! Cites `docs/prds/v0_6/compiler-type-hygiene.md` §2 (the Mul/Div
//! inventory), §7.2 (the truth-table contract), and design decisions 2
//! (characterization-first: RED = PRD-inventory assertion, GREEN = pin the
//! OBSERVED output) and 5 (degenerate `scale_components` shapes are
//! NOT-intentional). β2 (the static `infer_binop_type` fix) is written
//! against this frozen inventory; β3 (the runtime/static parity test) cites
//! it directly.
//!
//! **Characterization-only: this file makes NO production changes.** Every
//! row is reconciled to the OBSERVED runtime output — GREEN is reached with
//! zero edits to `eval_mul`/`eval_div`.
//!
//! Row classification tags used throughout, one per pinned row:
//! - `INTENTIONAL` — a deliberate arm in `eval_mul`/`eval_div`.
//! - `STRUCTURAL-Undef` — kind-level `_ => Undef` fallthrough (lib.rs:4627,
//!   4778); β3's parity test REJECTS these statically.
//! - `DATA-DRIVEN-Undef` — value-dependent Undef (e.g. divide-by-zero,
//!   lib.rs:4633-4637); β3 EXCLUDES these from the parity comparison.
//! - `degenerate-NOT-intentional` — a current `scale_components` shape that
//!   is not a deliberate design (PRD decision 5 / survey H1-P7b).
//! - `Matrix-diagnostic` — resolves PRD §10 Open-Question-2: `Value::Matrix`
//!   has no arm in either function, so it is a statically-diagnostic bucket.

use reify_core::{DimensionVector, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

// ── Driver ───────────────────────────────────────────────────────────────────

/// Evaluate `lv OP rv` through the public `eval_expr` path — mirrors
/// `transform_eval_tests.rs:56-68` / `tensor_matrix_arithmetic_tests.rs:34-42`.
///
/// Every literal/binop node is annotated with the same dummy
/// `Type::dimensionless_scalar()`. The evaluator dispatches on the `Value`
/// variant at runtime, never on the declared `Type`
/// (`tensor_matrix_arithmetic_tests.rs:14-17,31-36`), so the dummy is valid
/// regardless of the operands' real shape/dimension.
fn eval_binop(op: BinOp, lv: Value, rv: Value) -> Value {
    let left = CompiledExpr::literal(lv, Type::dimensionless_scalar());
    let right = CompiledExpr::literal(rv, Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(op, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

// ── Value constructors ───────────────────────────────────────────────────────

/// Build a dimensioned scalar: `Value::Scalar { si_value, dimension }`.
fn sc(v: f64, dim: DimensionVector) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: dim,
    }
}

/// Build a dimensioned complex value: `Value::Complex { re, im, dimension }`.
fn cx(re: f64, im: f64, dim: DimensionVector) -> Value {
    Value::Complex {
        re,
        im,
        dimension: dim,
    }
}

/// Build a 3-component `Value::Vector` of dimensioned scalars.
fn vec3(dim: DimensionVector, a: f64, b: f64, c: f64) -> Value {
    Value::Vector(vec![sc(a, dim), sc(b, dim), sc(c, dim)])
}

/// Build a 3-component `Value::Point` of dimensioned scalars.
fn pt3(dim: DimensionVector, a: f64, b: f64, c: f64) -> Value {
    Value::Point(vec![sc(a, dim), sc(b, dim), sc(c, dim)])
}

/// Build a rank-1 `Value::Tensor` from raw elements.
fn tensor1(elems: Vec<Value>) -> Value {
    Value::Tensor(elems)
}

/// Build a `Value::Matrix` — the user-facing matrix literal variant
/// (`value.rs:1207`), DISTINCT from a rank-2 `Value::Tensor`
/// (`tensor_matrix_arithmetic_tests.rs`'s `mat()` builds the latter).
/// Neither `eval_mul` nor `eval_div` has a match arm for `Value::Matrix`,
/// which is exactly what step-13's Open-Question-2 rows pin.
fn matrix2(rows: Vec<Vec<Value>>) -> Value {
    Value::Matrix(rows)
}

/// Identity quaternion (no rotation) — mirrors `transform_eval_tests.rs:11-18`.
fn orient() -> Value {
    Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

/// Build a `Value::Transform` with the given rotation and a LENGTH
/// translation vector — mirrors `transform_eval_tests.rs:44-53`.
fn xform(rotation: Value, tx: f64, ty: f64, tz: f64) -> Value {
    Value::Transform {
        rotation: Box::new(rotation),
        translation: Box::new(Value::Vector(vec![
            Value::length(tx),
            Value::length(ty),
            Value::length(tz),
        ])),
    }
}

// ── MUL: numeric + Scalar core arms ─────────────────────────────────────────

/// INTENTIONAL (lib.rs:4356): `Int × Int → Int`, exact integer product —
/// observed output matches the pre-execution prediction exactly.
#[test]
fn mul_int_int_yields_int() {
    let result = eval_binop(BinOp::Mul, Value::Int(6), Value::Int(7));
    assert_eq!(result, Value::Int(42));
}

/// INTENTIONAL (lib.rs:4357): `Real × Real → Real`.
#[test]
fn mul_real_real_yields_real() {
    let result = eval_binop(BinOp::Mul, Value::Real(2.5), Value::Real(4.0));
    match result {
        Value::Real(v) => assert!((v - 10.0).abs() < 1e-12, "v = {v}, expected ~10.0"),
        other => panic!("expected Real, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4358): `Int × Real → Real` — the dimensionless Int
/// widens to `f64` and multiplies directly.
#[test]
fn mul_int_real_yields_real() {
    let result = eval_binop(BinOp::Mul, Value::Int(3), Value::Real(2.5));
    match result {
        Value::Real(v) => assert!((v - 7.5).abs() < 1e-12, "v = {v}, expected ~7.5"),
        other => panic!("expected Real, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4358): `Real × Int → Real` — commutative counterpart
/// of `mul_int_real_yields_real`; both orders are the same match arm
/// (`(Int, Real) | (Real, Int)`).
#[test]
fn mul_real_int_yields_real() {
    let result = eval_binop(BinOp::Mul, Value::Real(2.5), Value::Int(3));
    match result {
        Value::Real(v) => assert!((v - 7.5).abs() < 1e-12, "v = {v}, expected ~7.5"),
        other => panic!("expected Real, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4378-4388): `Scalar × Scalar` multiplies `si_value`
/// and combines dimensions via `DimensionVector::mul` (add exponents) through
/// `Value::from_real_scalar` — `LENGTH.mul(&LENGTH) == AREA`, so the
/// non-cancelling case stays a `Scalar` (contrast with the cancelling case
/// below, which collapses to `Real`).
#[test]
fn mul_scalar_length_times_scalar_length_yields_scalar_area() {
    let result = eval_binop(BinOp::Mul, Value::length(3.0), Value::length(4.0));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.mul(&DimensionVector::LENGTH)
            );
            assert_eq!(dimension, DimensionVector::AREA);
            assert!(
                (si_value - 12.0).abs() < 1e-12,
                "si_value = {si_value}, expected ~12.0"
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4378-4388, `Value::from_real_scalar` at
/// value.rs:1557): same `Scalar × Scalar` arm as
/// `mul_scalar_length_times_scalar_length_yields_scalar_area`, but with
/// cancelling dimensions: `(1/LENGTH).mul(&LENGTH) == DIMENSIONLESS`, so
/// `Value::from_real_scalar` collapses the product to `Value::Real` —
/// CONFIRMED observed output is `Value::Real`, NOT
/// `Value::Scalar { dimension: DIMENSIONLESS, .. }`.
#[test]
fn mul_scalar_inverse_length_times_scalar_length_collapses_to_real() {
    let inverse_length = sc(
        2.0,
        DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH),
    );
    let result = eval_binop(BinOp::Mul, inverse_length, Value::length(4.0));
    match result {
        Value::Real(v) => assert!((v - 8.0).abs() < 1e-12, "v = {v}, expected ~8.0"),
        other => panic!(
            "expected Real (from_real_scalar dimensionless collapse), got {:?}",
            other
        ),
    }
}

/// INTENTIONAL (lib.rs:4389-4403): `Scalar × Int` scales `si_value` by the
/// dimensionless Int and preserves the Scalar's dimension unchanged (no
/// `DimensionVector` arithmetic — the Int contributes no dimension).
#[test]
fn mul_scalar_times_int_preserves_dimension() {
    let result = eval_binop(BinOp::Mul, Value::length(5.0), Value::Int(3));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!(
                (si_value - 15.0).abs() < 1e-12,
                "si_value = {si_value}, expected ~15.0"
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4389-4403): commutative counterpart of
/// `mul_scalar_times_int_preserves_dimension` — `(Scalar, Int) | (Int, Scalar)`
/// is a single match arm, so both operand orders share one code path and
/// produce the identical preserved-dimension result.
#[test]
fn mul_int_times_scalar_preserves_dimension() {
    let result = eval_binop(BinOp::Mul, Value::Int(3), Value::length(5.0));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!(
                (si_value - 15.0).abs() < 1e-12,
                "si_value = {si_value}, expected ~15.0"
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4404-4417): `Scalar × Real` scales `si_value` by the
/// dimensionless Real and preserves the Scalar's dimension unchanged.
#[test]
fn mul_scalar_times_real_preserves_dimension() {
    let result = eval_binop(BinOp::Mul, Value::length(5.0), Value::Real(2.0));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!(
                (si_value - 10.0).abs() < 1e-12,
                "si_value = {si_value}, expected ~10.0"
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4404-4417): commutative counterpart of
/// `mul_scalar_times_real_preserves_dimension` — `(Scalar, Real) | (Real, Scalar)`
/// is a single match arm, so both operand orders share one code path and
/// produce the identical preserved-dimension result.
#[test]
fn mul_real_times_scalar_preserves_dimension() {
    let result = eval_binop(BinOp::Mul, Value::Real(2.0), Value::length(5.0));
    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!(
                (si_value - 10.0).abs() < 1e-12,
                "si_value = {si_value}, expected ~10.0"
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

// ── MUL: Complex arms (commutative) ─────────────────────────────────────────

/// INTENTIONAL (lib.rs:4361-4377): `Complex × Complex` computes
/// `(ac-bd) + (ad+bc)i` and MULTIPLIES dimensions via `DimensionVector::mul`
/// (add exponents) — `LENGTH.mul(&TIME)`, not preserved from either operand.
#[test]
fn mul_complex_length_times_complex_time_yields_multiplied_dims() {
    // (2+3i){LENGTH} * (5+7i){TIME}: re = 2*5 - 3*7 = -11, im = 2*7 + 3*5 = 29.
    let a = cx(2.0, 3.0, DimensionVector::LENGTH);
    let b = cx(5.0, 7.0, DimensionVector::TIME);
    let result = eval_binop(BinOp::Mul, a, b);
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.mul(&DimensionVector::TIME)
            );
            assert!((re - -11.0).abs() < 1e-12, "re = {re}, expected ~-11.0");
            assert!((im - 29.0).abs() < 1e-12, "im = {im}, expected ~29.0");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4418-4444): `Complex × Scalar` scales `re`/`im` by the
/// Scalar's `si_value` and MULTIPLIES dimensions (`cd.mul(sd)`) — like
/// Complex×Complex, dimensions combine rather than being preserved from the
/// Complex operand alone.
#[test]
fn mul_complex_times_scalar_dims_multiply() {
    // (2+3i){LENGTH} * 5{TIME}: re = 2*5 = 10, im = 3*5 = 15.
    let a = cx(2.0, 3.0, DimensionVector::LENGTH);
    let b = sc(5.0, DimensionVector::TIME);
    let result = eval_binop(BinOp::Mul, a, b);
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.mul(&DimensionVector::TIME)
            );
            assert!((re - 10.0).abs() < 1e-12, "re = {re}, expected ~10.0");
            assert!((im - 15.0).abs() < 1e-12, "im = {im}, expected ~15.0");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4418-4444): commutative counterpart of
/// `mul_complex_times_scalar_dims_multiply` — `(Complex, Scalar) | (Scalar, Complex)`
/// is a single match arm binding `cd`/`sd` to whichever operand holds them,
/// so both orders produce the identical dims-multiplied result.
#[test]
fn mul_scalar_times_complex_dims_multiply() {
    let a = sc(5.0, DimensionVector::TIME);
    let b = cx(2.0, 3.0, DimensionVector::LENGTH);
    let result = eval_binop(BinOp::Mul, a, b);
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.mul(&DimensionVector::TIME)
            );
            assert!((re - 10.0).abs() < 1e-12, "re = {re}, expected ~10.0");
            assert!((im - 15.0).abs() < 1e-12, "im = {im}, expected ~15.0");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4445-4451): `Complex × Int` scales `re`/`im` by the
/// dimensionless Int and PRESERVES the Complex's dimension unchanged —
/// contrast with Complex×Complex/Complex×Scalar, where dimensions multiply.
#[test]
fn mul_complex_times_int_preserves_dimension() {
    let a = cx(2.0, 3.0, DimensionVector::LENGTH);
    let result = eval_binop(BinOp::Mul, a, Value::Int(4));
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!((re - 8.0).abs() < 1e-12, "re = {re}, expected ~8.0");
            assert!((im - 12.0).abs() < 1e-12, "im = {im}, expected ~12.0");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4445-4451): commutative counterpart of
/// `mul_complex_times_int_preserves_dimension` — `(Complex, Int) | (Int, Complex)`
/// is a single match arm, so both orders share one code path.
#[test]
fn mul_int_times_complex_preserves_dimension() {
    let b = cx(2.0, 3.0, DimensionVector::LENGTH);
    let result = eval_binop(BinOp::Mul, Value::Int(4), b);
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!((re - 8.0).abs() < 1e-12, "re = {re}, expected ~8.0");
            assert!((im - 12.0).abs() < 1e-12, "im = {im}, expected ~12.0");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4452-4458): `Complex × Real` scales `re`/`im` by the
/// dimensionless Real and PRESERVES the Complex's dimension unchanged.
#[test]
fn mul_complex_times_real_preserves_dimension() {
    let a = cx(2.0, 3.0, DimensionVector::LENGTH);
    let result = eval_binop(BinOp::Mul, a, Value::Real(1.5));
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!((re - 3.0).abs() < 1e-12, "re = {re}, expected ~3.0");
            assert!((im - 4.5).abs() < 1e-12, "im = {im}, expected ~4.5");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4452-4458): commutative counterpart of
/// `mul_complex_times_real_preserves_dimension` — `(Complex, Real) | (Real, Complex)`
/// is a single match arm, so both orders share one code path.
#[test]
fn mul_real_times_complex_preserves_dimension() {
    let b = cx(2.0, 3.0, DimensionVector::LENGTH);
    let result = eval_binop(BinOp::Mul, Value::Real(1.5), b);
    match result {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH);
            assert!((re - 3.0).abs() < 1e-12, "re = {re}, expected ~3.0");
            assert!((im - 4.5).abs() < 1e-12, "im = {im}, expected ~4.5");
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

// ── MUL: aggregate-scale + Transform arms ───────────────────────────────────

/// 90-degree rotation about the Z axis — mirrors `transform_eval_tests.rs:20-29`.
fn rotation_90z() -> Value {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    }
}

/// INTENTIONAL (lib.rs:4459-4464): `Tensor × scalar-like` scales each
/// component via `scale_components(.., eval_mul, Value::Tensor)` — the guard
/// only excludes `Value::Tensor` itself, so Int/Real/Scalar all qualify as
/// "scalar-like" here.
#[test]
fn mul_tensor_times_int_yields_scaled_tensor() {
    let t = tensor1(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = eval_binop(BinOp::Mul, t, Value::Int(2));
    match result {
        Value::Tensor(items) => {
            let vals: Vec<i64> = items
                .iter()
                .map(|v| match v {
                    Value::Int(i) => *i,
                    other => panic!("expected Int element, got {:?}", other),
                })
                .collect();
            assert_eq!(vals, vec![2, 4, 6]);
        }
        other => panic!("expected Tensor, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4459-4464): commutative counterpart of
/// `mul_tensor_times_int_yields_scaled_tensor` — `(Tensor, scalar) | (scalar, Tensor)`
/// is a single match arm (guarded only by "not `Tensor`"), so both operand
/// orders share one code path.
#[test]
fn mul_int_times_tensor_yields_scaled_tensor() {
    let t = tensor1(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = eval_binop(BinOp::Mul, Value::Int(2), t);
    match result {
        Value::Tensor(items) => {
            let vals: Vec<i64> = items
                .iter()
                .map(|v| match v {
                    Value::Int(i) => *i,
                    other => panic!("expected Int element, got {:?}", other),
                })
                .collect();
            assert_eq!(vals, vec![2, 4, 6]);
        }
        other => panic!("expected Tensor, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4465-4473): `Vector × scalar-like` scales each
/// `Scalar` component via `scale_components`, preserving each component's
/// dimension — the guard excludes `Vector | Point | Tensor | Transform`, so
/// Int/Real/Scalar all qualify.
#[test]
fn mul_vector_times_int_yields_scaled_vector() {
    let v = vec3(DimensionVector::LENGTH, 1.0, 2.0, 3.0);
    let result = eval_binop(BinOp::Mul, v, Value::Int(2));
    match result {
        Value::Vector(items) => {
            assert_eq!(items.len(), 3);
            for (item, expected) in items.iter().zip([2.0, 4.0, 6.0]) {
                match item {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(*dimension, DimensionVector::LENGTH);
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "si_value = {si_value}, expected ~{expected}"
                        );
                    }
                    other => panic!("expected Scalar element, got {:?}", other),
                }
            }
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4465-4473): commutative counterpart of
/// `mul_vector_times_int_yields_scaled_vector` — `(Vector, scalar) | (scalar, Vector)`
/// is a single match arm, so both operand orders share one code path.
#[test]
fn mul_int_times_vector_yields_scaled_vector() {
    let v = vec3(DimensionVector::LENGTH, 1.0, 2.0, 3.0);
    let result = eval_binop(BinOp::Mul, Value::Int(2), v);
    match result {
        Value::Vector(items) => {
            assert_eq!(items.len(), 3);
            for (item, expected) in items.iter().zip([2.0, 4.0, 6.0]) {
                match item {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(*dimension, DimensionVector::LENGTH);
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "si_value = {si_value}, expected ~{expected}"
                        );
                    }
                    other => panic!("expected Scalar element, got {:?}", other),
                }
            }
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4474-4484): `Point × scalar-like` scales each `Scalar`
/// component via `scale_components`, same shape as the Vector arm. Source
/// comment flags this as "a pragmatic deviation from strict affine rules,
/// needed for weighted interpolation and barycentric coordinates".
#[test]
fn mul_point_times_int_yields_scaled_point() {
    let p = pt3(DimensionVector::LENGTH, 1.0, 2.0, 3.0);
    let result = eval_binop(BinOp::Mul, p, Value::Int(2));
    match result {
        Value::Point(items) => {
            assert_eq!(items.len(), 3);
            for (item, expected) in items.iter().zip([2.0, 4.0, 6.0]) {
                match item {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(*dimension, DimensionVector::LENGTH);
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "si_value = {si_value}, expected ~{expected}"
                        );
                    }
                    other => panic!("expected Scalar element, got {:?}", other),
                }
            }
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4474-4484): commutative counterpart of
/// `mul_point_times_int_yields_scaled_point` — `(Point, scalar) | (scalar, Point)`
/// is a single match arm, so both operand orders share one code path.
#[test]
fn mul_int_times_point_yields_scaled_point() {
    let p = pt3(DimensionVector::LENGTH, 1.0, 2.0, 3.0);
    let result = eval_binop(BinOp::Mul, Value::Int(2), p);
    match result {
        Value::Point(items) => {
            assert_eq!(items.len(), 3);
            for (item, expected) in items.iter().zip([2.0, 4.0, 6.0]) {
                match item {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(*dimension, DimensionVector::LENGTH);
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "si_value = {si_value}, expected ~{expected}"
                        );
                    }
                    other => panic!("expected Scalar element, got {:?}", other),
                }
            }
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4485-4505): `Transform × Vector` applies rotation
/// ONLY (translation is deliberately ignored for vectors — a vector is a
/// free/direction quantity, not a position). Shape is `Vector → Vector`.
/// Identity transform * vector returns the same vector exactly (identity
/// quaternion rotation is exact under IEEE 754 — no rounding).
#[test]
fn mul_identity_transform_times_vector_yields_same_vector() {
    let v = vec3(DimensionVector::LENGTH, 1.0, 2.0, 3.0);
    let identity = xform(orient(), 0.0, 0.0, 0.0);
    let result = eval_binop(BinOp::Mul, identity, v.clone());
    assert_eq!(result, v);
}

/// INTENTIONAL (lib.rs:4485-4505): same arm as
/// `mul_identity_transform_times_vector_yields_same_vector`, exercised with a
/// non-trivial rotation. 90-degree Z rotation applied to (1,0,0) yields
/// (0,1,0); the transform's large translation is ignored (Transform * Vector
/// is rotation-only — confirms translation is NOT consulted).
#[test]
fn mul_transform_90z_times_vector_rotates_ignoring_translation() {
    let v = vec3(DimensionVector::LENGTH, 1.0, 0.0, 0.0);
    let t = xform(rotation_90z(), 100.0, 200.0, 300.0);
    let result = eval_binop(BinOp::Mul, t, v);
    match result {
        Value::Vector(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!((x - 0.0).abs() < 1e-10, "x = {x}, expected ~0");
            assert!((y - 1.0).abs() < 1e-10, "y = {y}, expected ~1");
            assert!((z - 0.0).abs() < 1e-10, "z = {z}, expected ~0");
        }
        other => panic!("expected Vector, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4506-4544): `Transform × Point` applies rotation THEN
/// ADDS translation — contrast with Transform×Vector, where translation is
/// ignored. Shape is `Point → Point`. 90-degree Z rotation + (10,20,30)
/// translation applied to point (1,0,0): rotate(1,0,0) -> (0,1,0), then +
/// (10,20,30) = (10,21,30).
#[test]
fn mul_transform_90z_times_point_rotates_and_translates() {
    let p = pt3(DimensionVector::LENGTH, 1.0, 0.0, 0.0);
    let t = xform(rotation_90z(), 10.0, 20.0, 30.0);
    let result = eval_binop(BinOp::Mul, t, p);
    match result {
        Value::Point(ref items) if items.len() == 3 => {
            let x = items[0].as_f64().unwrap();
            let y = items[1].as_f64().unwrap();
            let z = items[2].as_f64().unwrap();
            assert!((x - 10.0).abs() < 1e-10, "x = {x}, expected ~10");
            assert!((y - 21.0).abs() < 1e-10, "y = {y}, expected ~21");
            assert!((z - 30.0).abs() < 1e-10, "z = {z}, expected ~30");
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

/// INTENTIONAL (lib.rs:4545-4626): `Transform × Transform` composes rotations
/// (quaternion product, normalized) and translations
/// (`R1 * t2 + t1`) — shape is `Transform → Transform`. Compose
/// (R1,t1)*(R2,t2) = (R1*R2, R1*t2+t1). R1 = 90Z, t1 = (10,0,0), R2 =
/// identity, t2 = (1,0,0). Result rotation = 90Z, result translation =
/// 90Z*(1,0,0) + (10,0,0) = (0,1,0) + (10,0,0) = (10,1,0).
#[test]
fn mul_transform_times_transform_composes() {
    let t1 = xform(rotation_90z(), 10.0, 0.0, 0.0);
    let t2 = xform(orient(), 1.0, 0.0, 0.0);
    let result = eval_binop(BinOp::Mul, t1, t2);
    match result {
        Value::Transform {
            rotation,
            translation,
        } => {
            let s = std::f64::consts::FRAC_1_SQRT_2;
            match rotation.as_ref() {
                Value::Orientation { w, x, y, z } => {
                    // Quaternion sign ambiguity: q and -q represent the same rotation.
                    let pos_ok = (w - s).abs() < 1e-10
                        && x.abs() < 1e-10
                        && y.abs() < 1e-10
                        && (z - s).abs() < 1e-10;
                    let neg_ok = (w + s).abs() < 1e-10
                        && x.abs() < 1e-10
                        && y.abs() < 1e-10
                        && (z + s).abs() < 1e-10;
                    assert!(
                        pos_ok || neg_ok,
                        "rotation ({w},{x},{y},{z}) != expected 90Z"
                    );
                }
                other => panic!("expected Orientation, got {:?}", other),
            }
            match translation.as_ref() {
                Value::Vector(items) if items.len() == 3 => {
                    let x = items[0].as_f64().unwrap();
                    let y = items[1].as_f64().unwrap();
                    let z = items[2].as_f64().unwrap();
                    assert!((x - 10.0).abs() < 1e-10, "x = {x}, expected ~10");
                    assert!((y - 1.0).abs() < 1e-10, "y = {y}, expected ~1");
                    assert!((z - 0.0).abs() < 1e-10, "z = {z}, expected ~0");
                }
                other => panic!("expected Vector translation, got {:?}", other),
            }
        }
        other => panic!("expected Transform, got {:?}", other),
    }
}
