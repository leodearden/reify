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
