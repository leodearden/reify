//! Static-vs-runtime Mul/Div parity test (task compiler-type-hygiene β3).
//!
//! Ties together the two independently-authored Mul/Div truth tables so
//! they cannot silently drift:
//! - β1 (runtime): `crates/reify-expr/tests/mul_div_runtime_truth_table.rs`
//!   — `eval_mul`/`eval_div` pinned through public `eval_expr`.
//! - β2 (static): `infer_mul_div_result` (`type_compat.rs:1601`) — the
//!   `Some(T)`=accept / `None`=reject partition that `infer_binop_type`'s
//!   Mul/Div arm delegates to.
//!
//! Cites `docs/prds/v0_6/compiler-type-hygiene.md` §7.2 (the parity
//! contract, lines 72-77) and §3 decision 4 (the exemption ledger, line
//! 34). Establishes **INV-COMP-3** (`docs/invariants.md` row 38): this
//! change flips its Status column `proposed` → `enforced(test)` in the
//! same commit as the enforcing test (INV-META-1).
//!
//! ## Contract (PRD §7.2)
//!
//! For op ∈ {Mul, Div} and every operand-kind pair that has a runtime
//! `Value` representation:
//! - runtime non-Undef ⇒ `infer_mul_div_result` returns `Some(T)` whose
//!   kind — and, for Scalar/Complex results, dimension — matches the
//!   runtime result's value kind ("dimension-correct for scalar algebra").
//! - runtime structurally-Undef (excluding DATA-DRIVEN-Undef — div-by-zero,
//!   non-finite quaternion — a runtime VALUE question, not a static TYPE
//!   question) ⇒ `infer_mul_div_result` returns `None`.
//! - Divergences are allowed ONLY via the commented `EXEMPTION_LEDGER`
//!   (§3 decision 4, added in step-7/8); an unledgered divergence panics.
//!
//! ## Access to the static table
//!
//! `infer_mul_div_result` is `pub(crate)` in `reify-compiler`, unreachable
//! from an integration test (a separate external crate). This test reaches
//! it through the `test-support`-gated
//! `reify_compiler::__infer_mul_div_result_for_parity_test` shim (mirrors
//! `__validate_annotations_for_parity_test`, `lib.rs:127-140`); the
//! self-pull dev-dependency that activates `feature = "test-support"` for
//! this integration test target already exists (`Cargo.toml:24-30`).
//!
//! ## Reuse note
//!
//! The runtime driver (`eval_binop`) and representative `Value`
//! constructors below are re-authored from β1's
//! `mul_div_runtime_truth_table.rs:49-121` — sibling integration-test
//! crates cannot import each other's `tests/` modules, so this is
//! reuse-by-pattern (identical shape, not a shared import).

#![cfg(feature = "test-support")]
// Helper constructors below are added ahead of the rows that consume them
// (TDD steps land the full row batches in step-3/step-5); until then some
// are unused.
#![allow(dead_code)]

use reify_core::{DimensionVector, Type};
use reify_expr::{eval_expr, EvalContext};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

// ── Runtime driver (re-authored from β1, mul_div_runtime_truth_table.rs:49-55) ──

/// Evaluate `lv OP rv` through the public `eval_expr` path. Every
/// literal/binop node is annotated with a dummy `Type::dimensionless_scalar()`
/// — the evaluator dispatches on the `Value` variant at runtime, never on
/// the declared `Type`, so the dummy annotation is valid regardless of the
/// operands' real shape/dimension.
fn eval_binop(op: BinOp, lv: Value, rv: Value) -> Value {
    let left = CompiledExpr::literal(lv, Type::dimensionless_scalar());
    let right = CompiledExpr::literal(rv, Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(op, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    eval_expr(&expr, &EvalContext::simple(&values))
}

// ── Representative Value constructors (re-authored from β1) ────────────────

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

/// Build a `Value::Matrix` — the user-facing matrix literal variant,
/// DISTINCT from a rank-2 `Value::Tensor` (and from the distinct static
/// `Type::Matrix { m, n, quantity }` variant, which also has no runtime
/// Mul/Div arm — PRD §10 Open-Question-2).
fn matrix2(rows: Vec<Vec<Value>>) -> Value {
    Value::Matrix(rows)
}

/// Identity quaternion (no rotation).
fn orient() -> Value {
    Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

/// Build a `Value::Transform` with the given rotation and a LENGTH
/// translation vector.
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

// ── Smoke test (step-1 RED / step-2 GREEN) ──────────────────────────────────

/// Smoke row proving the harness wiring end-to-end: `Int × Int` is the
/// simplest INTENTIONAL row on both sides — runtime `Value::Int(42)`,
/// static `Some(Type::Int)`. Fails to COMPILE until step-2 adds the
/// `__infer_mul_div_result_for_parity_test` shim (unresolved
/// import/function — the shim does not exist yet).
#[test]
fn smoke_int_times_int_parity() {
    let runtime = eval_binop(BinOp::Mul, Value::Int(6), Value::Int(7));
    assert_eq!(runtime, Value::Int(42));

    let static_result = reify_compiler::__infer_mul_div_result_for_parity_test(
        BinOp::Mul,
        &Type::Int,
        &Type::Int,
    );
    assert_eq!(static_result, Some(Type::Int));
}
