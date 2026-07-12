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

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
use reify_expr::{eval_expr, EvalContext};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::compile_source;

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

/// 90-degree rotation about the Z axis (re-authored from β1's
/// `transform_eval_tests.rs`-derived helper).
fn rotation_90z() -> Value {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    Value::Orientation {
        w: s,
        x: 0.0,
        y: 0.0,
        z: s,
    }
}

// ── Representative Type constructors (mirror type_compat.rs's own unit-test
//    helpers, e.g. `time_ty`/`area_ty` at type_compat.rs:4489,4924) ─────────

fn time_ty() -> Type {
    Type::Scalar {
        dimension: DimensionVector::TIME,
    }
}

fn area_ty() -> Type {
    Type::Scalar {
        dimension: DimensionVector::AREA,
    }
}

// ── Kind(+dimension) mapping (step-3 RED stub / step-4 GREEN impl) ─────────

/// Kind(+dimension) mapping between a runtime `Value` and a static `Type`
/// (PRD §7.2 "dimension-correct for scalar algebra"; design decision 3).
///
/// There is no `Type::Real` — a `Real` literal statically types as
/// `Type::Scalar{DIMENSIONLESS}`, and the runtime additionally collapses any
/// dimensionless `Scalar` product/quotient to `Value::Real`
/// (`Value::from_real_scalar`, value.rs:1557). So `Value::Real` is treated as
/// equivalent to `Type::Scalar{DIMENSIONLESS}` here. `Complex` results are
/// checked the same way on their inner quantity. Aggregate results
/// (`Vector`/`Point`/`Tensor`) recurse this same check over each component
/// against the aggregate's quantity slot (mirroring the static table's own
/// aggregate-scale recursion through `infer_mul_div_result`). `Transform`
/// only needs kind agreement — it has no quantity slot to check.
fn value_kind_matches_type(runtime: &Value, static_ty: &Type) -> bool {
    match (runtime, static_ty) {
        (Value::Int(_), Type::Int) => true,
        (Value::Real(_), Type::Scalar { dimension }) => dimension.is_dimensionless(),
        (Value::Scalar { dimension: rd, .. }, Type::Scalar { dimension: sd }) => rd == sd,
        (Value::Complex { dimension: rd, .. }, Type::Complex(sq)) => match sq.as_ref() {
            Type::Scalar { dimension: sd } => rd == sd,
            _ => false,
        },
        (Value::Vector(items), Type::Vector { quantity, .. }) => items
            .iter()
            .all(|item| value_kind_matches_type(item, quantity)),
        (Value::Point(items), Type::Point { quantity, .. }) => items
            .iter()
            .all(|item| value_kind_matches_type(item, quantity)),
        (Value::Tensor(items), Type::Tensor { quantity, .. }) => items
            .iter()
            .all(|item| value_kind_matches_type(item, quantity)),
        (Value::Transform { .. }, Type::Transform(_)) => true,
        _ => false,
    }
}

// ── Positive-parity assertion engine (step-3 RED / step-8 GREEN ledger) ────

/// Positive-parity assertion (PRD §7.2, first clause): runtime non-Undef ⇒
/// static `Some(T)` whose kind matches the runtime result via
/// `value_kind_matches_type`. `label` names the row for panic attribution.
fn assert_accepts(op: BinOp, lv: Value, rv: Value, lt: Type, rt: Type, label: &str) {
    let runtime = eval_binop(op, lv.clone(), rv.clone());
    assert!(
        !runtime.is_undef(),
        "{label}: expected non-Undef runtime result for {op:?}({lv:?}, {rv:?}), got Undef"
    );
    let static_result = reify_compiler::__infer_mul_div_result_for_parity_test(op, &lt, &rt);
    let static_ty = match static_result {
        Some(t) => t,
        None => panic!(
            "{label}: expected static Some(T) for {op:?}({lt:?}, {rt:?}) \
             (runtime={runtime:?}), got None"
        ),
    };
    assert!(
        value_kind_matches_type(&runtime, &static_ty),
        "{label}: runtime result {runtime:?} does not match static type {static_ty:?} \
         for {op:?}({lt:?}, {rt:?})"
    );
}

// ── Positive-parity batch (step-3 RED / step-4 GREEN) ───────────────────────
//
// Mirrors every INTENTIONAL row of β1's runtime truth table for BOTH Mul and
// Div (mul_div_runtime_truth_table.rs), EXCLUDING the Int/Int non-divisible
// Div row — that row is a genuine static/runtime DIVERGENCE (the runtime
// widens to Real; the static table intentionally stays Int) and is handled
// by the EXEMPTION_LEDGER in step-7/8, not here. The divisible Int/Int Div
// row below (row 27) IS a clean-agreement row and stays in this batch.

struct PositiveRow {
    label: &'static str,
    op: BinOp,
    lv: Value,
    rv: Value,
    lt: Type,
    rt: Type,
}

#[rustfmt::skip]
fn positive_rows() -> Vec<PositiveRow> {
    use BinOp::{Div, Mul};
    use DimensionVector as D;

    vec![
        // ── Mul: numeric + Scalar core ──────────────────────────────────
        PositiveRow { label: "Mul Int*Int",              op: Mul, lv: Value::Int(6),                 rv: Value::Int(7),                 lt: Type::Int,                     rt: Type::Int },
        PositiveRow { label: "Mul Real*Real",             op: Mul, lv: Value::Real(2.5),               rv: Value::Real(4.0),               lt: Type::dimensionless_scalar(),  rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Mul Int*Real",              op: Mul, lv: Value::Int(3),                  rv: Value::Real(2.5),               lt: Type::Int,                     rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Mul Real*Int",              op: Mul, lv: Value::Real(2.5),               rv: Value::Int(3),                  lt: Type::dimensionless_scalar(),  rt: Type::Int },
        PositiveRow { label: "Mul Scalar[L]*Scalar[L]->Area", op: Mul, lv: Value::length(3.0),          rv: Value::length(4.0),             lt: Type::length(),                rt: Type::length() },
        PositiveRow {
            label: "Mul Scalar[1/L]*Scalar[L]->collapses Real", op: Mul,
            lv: sc(2.0, D::DIMENSIONLESS.div(&D::LENGTH)), rv: Value::length(4.0),
            lt: Type::Scalar { dimension: D::DIMENSIONLESS.div(&D::LENGTH) }, rt: Type::length(),
        },
        PositiveRow { label: "Mul Scalar[L]*Int preserves dim",  op: Mul, lv: Value::length(5.0), rv: Value::Int(3),    lt: Type::length(), rt: Type::Int },
        PositiveRow { label: "Mul Int*Scalar[L] preserves dim",  op: Mul, lv: Value::Int(3),       rv: Value::length(5.0), lt: Type::Int,   rt: Type::length() },
        PositiveRow { label: "Mul Scalar[L]*Real preserves dim", op: Mul, lv: Value::length(5.0), rv: Value::Real(2.0), lt: Type::length(), rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Mul Real*Scalar[L] preserves dim", op: Mul, lv: Value::Real(2.0),   rv: Value::length(5.0), lt: Type::dimensionless_scalar(), rt: Type::length() },

        // ── Mul: Complex arms ────────────────────────────────────────────
        PositiveRow { label: "Mul Complex[L]*Complex[T] dims multiply", op: Mul, lv: cx(2.0, 3.0, D::LENGTH), rv: cx(5.0, 7.0, D::TIME), lt: Type::complex(Type::length()), rt: Type::complex(time_ty()) },
        PositiveRow { label: "Mul Complex[L]*Scalar[T] dims multiply",  op: Mul, lv: cx(2.0, 3.0, D::LENGTH), rv: sc(5.0, D::TIME),      lt: Type::complex(Type::length()), rt: time_ty() },
        PositiveRow { label: "Mul Scalar[T]*Complex[L] dims multiply",  op: Mul, lv: sc(5.0, D::TIME),        rv: cx(2.0, 3.0, D::LENGTH), lt: time_ty(),                    rt: Type::complex(Type::length()) },
        PositiveRow { label: "Mul Complex[L]*Int preserves dim",        op: Mul, lv: cx(2.0, 3.0, D::LENGTH), rv: Value::Int(4),          lt: Type::complex(Type::length()), rt: Type::Int },
        PositiveRow { label: "Mul Int*Complex[L] preserves dim",        op: Mul, lv: Value::Int(4),           rv: cx(2.0, 3.0, D::LENGTH), lt: Type::Int,                   rt: Type::complex(Type::length()) },
        PositiveRow { label: "Mul Complex[L]*Real preserves dim",       op: Mul, lv: cx(2.0, 3.0, D::LENGTH), rv: Value::Real(1.5),       lt: Type::complex(Type::length()), rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Mul Real*Complex[L] preserves dim",       op: Mul, lv: Value::Real(1.5),        rv: cx(2.0, 3.0, D::LENGTH), lt: Type::dimensionless_scalar(), rt: Type::complex(Type::length()) },

        // ── Mul: aggregate-scale + Transform arms ────────────────────────
        PositiveRow { label: "Mul Tensor*Int scaled", op: Mul, lv: tensor1(vec![Value::Int(1), Value::Int(2), Value::Int(3)]), rv: Value::Int(2), lt: Type::tensor(1, 3, Type::Int), rt: Type::Int },
        PositiveRow { label: "Mul Int*Tensor scaled", op: Mul, lv: Value::Int(2), rv: tensor1(vec![Value::Int(1), Value::Int(2), Value::Int(3)]), lt: Type::Int, rt: Type::tensor(1, 3, Type::Int) },
        PositiveRow { label: "Mul Vector[L]*Int scaled", op: Mul, lv: vec3(D::LENGTH, 1.0, 2.0, 3.0), rv: Value::Int(2), lt: Type::vec3(Type::length()), rt: Type::Int },
        PositiveRow { label: "Mul Int*Vector[L] scaled", op: Mul, lv: Value::Int(2), rv: vec3(D::LENGTH, 1.0, 2.0, 3.0), lt: Type::Int, rt: Type::vec3(Type::length()) },
        PositiveRow { label: "Mul Point[L]*Int scaled",  op: Mul, lv: pt3(D::LENGTH, 1.0, 2.0, 3.0),  rv: Value::Int(2), lt: Type::point3(Type::length()), rt: Type::Int },
        PositiveRow { label: "Mul Int*Point[L] scaled",  op: Mul, lv: Value::Int(2), rv: pt3(D::LENGTH, 1.0, 2.0, 3.0), lt: Type::Int, rt: Type::point3(Type::length()) },
        PositiveRow { label: "Mul Transform*Vector[L]->Vector[L] (identity)", op: Mul, lv: xform(orient(), 0.0, 0.0, 0.0), rv: vec3(D::LENGTH, 1.0, 2.0, 3.0), lt: Type::transform(3), rt: Type::vec3(Type::length()) },
        PositiveRow { label: "Mul Transform*Point[L]->Point[L]",             op: Mul, lv: xform(rotation_90z(), 10.0, 20.0, 30.0), rv: pt3(D::LENGTH, 1.0, 0.0, 0.0), lt: Type::transform(3), rt: Type::point3(Type::length()) },
        PositiveRow { label: "Mul Transform*Transform->Transform (row-9)",   op: Mul, lv: xform(rotation_90z(), 10.0, 0.0, 0.0), rv: xform(orient(), 1.0, 0.0, 0.0), lt: Type::transform(3), rt: Type::transform(3) },

        // ── Div: numeric + Scalar core ───────────────────────────────────
        // NOTE: Int/Int here is the DIVISIBLE case (clean agreement, both
        // Int). The non-divisible case is the exemption-ledger row, added
        // in step-7/8 (NOT here).
        PositiveRow { label: "Div Int/Int (divisible)",   op: Div, lv: Value::Int(6),   rv: Value::Int(3),   lt: Type::Int, rt: Type::Int },
        PositiveRow { label: "Div Real/Real",              op: Div, lv: Value::Real(10.0), rv: Value::Real(4.0), lt: Type::dimensionless_scalar(), rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Div Int/Real",                op: Div, lv: Value::Int(9),   rv: Value::Real(2.0), lt: Type::Int, rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Div Real/Int",                op: Div, lv: Value::Real(9.0), rv: Value::Int(2),   lt: Type::dimensionless_scalar(), rt: Type::Int },
        PositiveRow { label: "Div Scalar[Area]/Scalar[L]->Scalar[L]", op: Div, lv: sc(12.0, D::AREA), rv: Value::length(4.0), lt: area_ty(), rt: Type::length() },
        PositiveRow { label: "Div Scalar[L]/Scalar[L]->collapses Real", op: Div, lv: Value::length(12.0), rv: Value::length(4.0), lt: Type::length(), rt: Type::length() },
        PositiveRow { label: "Div Scalar[L]/Int preserves dim",  op: Div, lv: Value::length(15.0), rv: Value::Int(3), lt: Type::length(), rt: Type::Int },
        PositiveRow { label: "Div Scalar[L]/Real preserves dim", op: Div, lv: Value::length(15.0), rv: Value::Real(2.0), lt: Type::length(), rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Div Real/Scalar[T] reciprocal dim", op: Div, lv: Value::Real(8.0), rv: sc(4.0, D::TIME), lt: Type::dimensionless_scalar(), rt: time_ty() },
        PositiveRow { label: "Div Int/Scalar[T] reciprocal dim",  op: Div, lv: Value::Int(8),    rv: sc(4.0, D::TIME), lt: Type::Int, rt: time_ty() },

        // ── Div: Complex + aggregate arms ─────────────────────────────────
        PositiveRow { label: "Div Complex[Area]/Complex[L] dims divide", op: Div, lv: cx(10.0, 5.0, D::AREA), rv: cx(3.0, 4.0, D::LENGTH), lt: Type::complex(area_ty()), rt: Type::complex(Type::length()) },
        PositiveRow { label: "Div Complex[Area]/Scalar[L] dims divide",  op: Div, lv: cx(10.0, 6.0, D::AREA), rv: sc(2.0, D::LENGTH),      lt: Type::complex(area_ty()), rt: Type::length() },
        PositiveRow { label: "Div Complex[L]/Int preserves dim",         op: Div, lv: cx(8.0, 6.0, D::LENGTH), rv: Value::Int(2),          lt: Type::complex(Type::length()), rt: Type::Int },
        PositiveRow { label: "Div Complex[L]/Real preserves dim",        op: Div, lv: cx(9.0, 6.0, D::LENGTH), rv: Value::Real(3.0),       lt: Type::complex(Type::length()), rt: Type::dimensionless_scalar() },
        PositiveRow { label: "Div Tensor/Int scaled", op: Div, lv: tensor1(vec![Value::Int(4), Value::Int(6), Value::Int(8)]), rv: Value::Int(2), lt: Type::tensor(1, 3, Type::Int), rt: Type::Int },
        PositiveRow { label: "Div Vector[L]/Int scaled", op: Div, lv: vec3(D::LENGTH, 8.0, 10.0, 12.0), rv: Value::Int(2), lt: Type::vec3(Type::length()), rt: Type::Int },
        PositiveRow { label: "Div Point[L]/Int scaled",  op: Div, lv: pt3(D::LENGTH, 8.0, 10.0, 12.0),  rv: Value::Int(2), lt: Type::point3(Type::length()), rt: Type::Int },
    ]
}

/// Positive-parity batch: every row above must AGREE (runtime non-Undef ⇒
/// static `Some(T)` whose kind/dimension matches). Panics via `todo!()`
/// inside `value_kind_matches_type` until step-4 implements it (RED).
#[test]
fn positive_parity_batch() {
    for row in positive_rows() {
        assert_accepts(row.op, row.lv, row.rv, row.lt, row.rt, row.label);
    }
}

// ── Rejection-parity assertion engine (step-5 RED / step-6 GREEN) ──────────

/// Rejection-parity assertion (PRD §7.2, second clause): runtime
/// structurally-Undef ⇒ static `None` (`infer_mul_div_result` rejects the
/// same pairing). `label` names the row for panic attribution.
fn assert_rejects(op: BinOp, lv: Value, rv: Value, lt: Type, rt: Type, label: &str) {
    let runtime = eval_binop(op, lv.clone(), rv.clone());
    assert!(
        runtime.is_undef(),
        "{label}: expected structurally-Undef runtime result for {op:?}({lv:?}, {rv:?}), \
         got {runtime:?}"
    );
    let static_result = reify_compiler::__infer_mul_div_result_for_parity_test(op, &lt, &rt);
    assert!(
        static_result.is_none(),
        "{label}: expected static None (reject) for {op:?}({lt:?}, {rt:?}) \
         (runtime={runtime:?}), got {static_result:?}"
    );
}

// ── Rejection-parity batch (step-5 RED / step-6 GREEN) ──────────────────────
//
// Mirrors β1's STRUCTURAL-Undef set (mul_div_runtime_truth_table.rs) with
// NON-degenerate operands (no zero divisor — DATA-DRIVEN-Undef div-by-zero
// rows are a separate, EXCLUDED carve-out added in step-7/8, not here).
// Excludes the `degenerate-NOT-intentional` Tensor×Vector row (PRD decision
// 5) — that row is NOT Undef at runtime, so it is not a rejection-parity
// candidate at all. Type-side shapes mirror type_compat.rs's own
// `infer_mul_div_result_*_is_none` unit tests where one exists; the
// ORDER-SENSITIVE Vector×Transform row is asserted here (Undef), with its
// mirror Transform×Vector already covered as a POSITIVE row above (accepted)
// — together the two rows check both orders independently, per PRD §7.2.

struct RejectionRow {
    label: &'static str,
    op: BinOp,
    lv: Value,
    rv: Value,
    lt: Type,
    rt: Type,
}

#[rustfmt::skip]
fn rejection_rows() -> Vec<RejectionRow> {
    use BinOp::{Div, Mul};
    use DimensionVector as D;

    vec![
        // ── Mul: aggregate × aggregate (kind-level, no scale arm) ─────────
        RejectionRow { label: "Mul Vector[L]*Vector[L] undef", op: Mul, lv: vec3(D::LENGTH, 1.0, 2.0, 3.0), rv: vec3(D::LENGTH, 4.0, 5.0, 6.0), lt: Type::vec3(Type::length()), rt: Type::vec3(Type::length()) },
        RejectionRow { label: "Mul Tensor*Tensor undef",        op: Mul, lv: tensor1(vec![Value::Int(1), Value::Int(2)]), rv: tensor1(vec![Value::Int(3), Value::Int(4)]), lt: Type::tensor(1, 3, Type::length()), rt: Type::tensor(1, 3, Type::length()) },
        RejectionRow { label: "Mul Point[L]*Point[L] undef",    op: Mul, lv: pt3(D::LENGTH, 1.0, 2.0, 3.0), rv: pt3(D::LENGTH, 4.0, 5.0, 6.0), lt: Type::point3(Type::length()), rt: Type::point3(Type::length()) },

        // ── Mul: Matrix in either position (PRD §10 Open-Question-2) ──────
        RejectionRow { label: "Mul Matrix*Int undef",       op: Mul, lv: matrix2(vec![vec![Value::Int(1), Value::Int(2)], vec![Value::Int(3), Value::Int(4)]]), rv: Value::Int(2), lt: Type::matrix(2, 2, Type::length()), rt: Type::Int },
        RejectionRow { label: "Mul Matrix*Scalar[L] undef", op: Mul, lv: matrix2(vec![vec![Value::Int(1), Value::Int(2)], vec![Value::Int(3), Value::Int(4)]]), rv: Value::length(2.0), lt: Type::matrix(2, 2, Type::length()), rt: Type::length() },
        RejectionRow { label: "Mul Matrix*Real undef",      op: Mul, lv: matrix2(vec![vec![Value::Int(1), Value::Int(2)], vec![Value::Int(3), Value::Int(4)]]), rv: Value::Real(2.0), lt: Type::matrix(2, 2, Type::length()), rt: Type::dimensionless_scalar() },
        RejectionRow { label: "Mul Int*Matrix undef",       op: Mul, lv: Value::Int(2), rv: matrix2(vec![vec![Value::Int(1), Value::Int(2)], vec![Value::Int(3), Value::Int(4)]]), lt: Type::Int, rt: Type::matrix(2, 2, Type::length()) },

        // ── Mul: no runtime arm at all (List/String/Bool) ─────────────────
        RejectionRow { label: "Mul List*Int undef",   op: Mul, lv: Value::List(vec![Value::Int(1), Value::Int(2)]), rv: Value::Int(2), lt: Type::List(Box::new(Type::Int)), rt: Type::Int },
        RejectionRow { label: "Mul String*Int undef", op: Mul, lv: Value::String("abc".to_string()), rv: Value::Int(2), lt: Type::String, rt: Type::Int },
        RejectionRow { label: "Mul Bool*Int undef",   op: Mul, lv: Value::Bool(true), rv: Value::Int(2), lt: Type::Bool, rt: Type::Int },

        // ── Mul: ORDER-SENSITIVE Vector×Transform (reverse of the positive
        //    "Transform*Vector[L]->Vector[L]" row above) ────────────────────
        RejectionRow { label: "Mul Vector[L]*Transform undef (order-sensitive)", op: Mul, lv: vec3(D::LENGTH, 1.0, 0.0, 0.0), rv: xform(rotation_90z(), 100.0, 200.0, 300.0), lt: Type::vec3(Type::length()), rt: Type::transform(3) },

        // ── Div: non-commutative reversals (Scalar/Real numerator over an
        //    aggregate denominator — no reverse-scale arm exists) ──────────
        RejectionRow { label: "Div Scalar[L]/Vector[L] undef (non-commutative)", op: Div, lv: Value::length(10.0), rv: vec3(D::LENGTH, 1.0, 2.0, 3.0), lt: Type::length(), rt: Type::vec3(Type::length()) },
        RejectionRow { label: "Div Real/Vector[L] undef",                       op: Div, lv: Value::Real(10.0),   rv: vec3(D::LENGTH, 1.0, 2.0, 3.0), lt: Type::dimensionless_scalar(), rt: Type::vec3(Type::length()) },

        // ── Div: no runtime arm at all (Matrix/List/String/Bool) ──────────
        RejectionRow { label: "Div Matrix/Int undef", op: Div, lv: matrix2(vec![vec![Value::Int(1), Value::Int(2)], vec![Value::Int(3), Value::Int(4)]]), rv: Value::Int(2), lt: Type::matrix(2, 2, Type::length()), rt: Type::Int },
        RejectionRow { label: "Div List/Int undef",   op: Div, lv: Value::List(vec![Value::Int(1), Value::Int(2)]), rv: Value::Int(2), lt: Type::List(Box::new(Type::Int)), rt: Type::Int },
        RejectionRow { label: "Div String/Int undef", op: Div, lv: Value::String("abc".to_string()), rv: Value::Int(2), lt: Type::String, rt: Type::Int },
        RejectionRow { label: "Div Bool/Int undef",   op: Div, lv: Value::Bool(true), rv: Value::Int(2), lt: Type::Bool, rt: Type::Int },
    ]
}

/// Rejection-parity batch: every row above must AGREE (runtime
/// structurally-Undef ⇒ static `None`). Panics via `todo!()` inside
/// `assert_rejects` until step-6 implements it (RED).
#[test]
fn rejection_parity_batch() {
    for row in rejection_rows() {
        assert_rejects(row.op, row.lv, row.rv, row.lt, row.rt, row.label);
    }
}

// ── None → ArithOperandKind anchor (step-5) ─────────────────────────────────

/// Ties the abstract `None` reject-partition to the OBSERVABLE diagnostic
/// named in the PRD contract (§7.2): a representative structural-Undef pair
/// (`Vector3<Length> × Vector3<Length>`, the first row of the batch above)
/// must actually surface `DiagnosticCode::ArithOperandKind` when compiled
/// through the real pipeline — belt-and-suspenders coverage for the `None`
/// partition without re-testing β2's full guard suite
/// (`mul_div_operand_guard_tests.rs`, whose `compile_source` +
/// filter-`Severity::Error` pattern this reuses; design decision 4).
#[test]
fn none_partition_anchors_to_arith_operand_kind_diagnostic() {
    let source = r#"
structure def P {
    param a : Vector3<Length>
    let v = a * a
}
"#;
    let module = compile_source(source);
    let flagged = module.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.code == Some(DiagnosticCode::ArithOperandKind)
    });
    assert!(
        flagged,
        "`a * a` (Vector3<Length> × Vector3<Length>) must yield \
         DiagnosticCode::ArithOperandKind; got diagnostics: {:?}",
        module.diagnostics
    );
}

// ── Exemption ledger (step-7 RED / step-8 GREEN; PRD §3 decision 4) ────────

/// Documented, intentional static/runtime divergences (PRD §3 decision 4):
/// "every exemption row requires a comment; an uncommented divergence
/// fails." Keyed by `(op, left static Type, right static Type)` — exact
/// `Type` equality, not just discriminant, so a future DIMENSIONED exemption
/// would need its own row.
///
/// Seeded with exactly the one known exemption:
///
/// - `Div(Int, Int)`: the RUNTIME widens `Int|Real` by divisibility
///   (`reify-expr/src/lib.rs:4640-4648` — non-divisible `7 % 2 != 0` ⇒
///   `Value::Real(3.5)`, pinned by β1's `div_int_int_nondivisible_yields_real`),
///   but the STATIC table intentionally stays `Some(Type::Int)` (pinned by
///   β2's `infer_mul_div_result_int_div_int_yields_int_exemption`,
///   type_compat.rs:4524-4531) — changing integer-division statics is a
///   language-semantics decision, out of scope for this task (PRD §4
///   "Integer-division static-type change").
const EXEMPTION_LEDGER: &[(BinOp, Type, Type)] = &[(BinOp::Div, Type::Int, Type::Int)];

/// Ledger membership check: is `(op, lt, rt)` a documented exemption?
fn is_ledgered_divergence(op: BinOp, lt: &Type, rt: &Type) -> bool {
    EXEMPTION_LEDGER
        .iter()
        .any(|(ledgered_op, ledgered_lt, ledgered_rt)| {
            *ledgered_op == op && ledgered_lt == lt && ledgered_rt == rt
        })
}

/// Top-level parity classification (PRD §7.2 + §3 decision 4): both sides
/// "accept" (runtime non-Undef, static `Some`) and the runtime result's kind
/// AGREES with the static type's kind ⇒ pass (clean parity, same contract as
/// `assert_accepts`). If the kinds DIVERGE, the row passes ONLY when
/// `(op, lt, rt)` is a documented `EXEMPTION_LEDGER` entry — otherwise it
/// panics naming the row (the "uncommented divergence fails" guarantee, PRD
/// §8).
///
/// TEETH (PRD decision 4's "an uncommented divergence fails" — proven by
/// temporary mutation, not a committed failing assertion; both recipes below
/// were manually verified while authoring this fn, then reverted):
/// - Comment out the `(BinOp::Div, Type::Int, Type::Int)` row in
///   `EXEMPTION_LEDGER` above and re-run
///   `cargo test -p reify-compiler --test mul_div_static_runtime_parity` —
///   `int_int_nondivisible_division_is_a_ledgered_divergence` goes RED at
///   its own `is_ledgered_divergence` assertion ("Div(Int, Int) must be
///   present in EXEMPTION_LEDGER"), proving the ledger lookup is
///   load-bearing rather than vacuously true.
/// - To see THIS fn's own panic fire (rather than that earlier test-side
///   check), call it directly on the same row with the ledger emptied:
///   panics "UNLEDGERED divergence — runtime Real(3.5) (kind mismatch) vs
///   static Int for Div(Int, Int); add a commented EXEMPTION_LEDGER row
///   ... or fix the mismatch", prefixed with the row's `label`.
///
fn assert_parity_or_ledgered_divergence(
    op: BinOp,
    lv: Value,
    rv: Value,
    lt: Type,
    rt: Type,
    label: &str,
) {
    let runtime = eval_binop(op, lv.clone(), rv.clone());
    let static_result = reify_compiler::__infer_mul_div_result_for_parity_test(op, &lt, &rt);
    match (runtime.is_undef(), &static_result) {
        // Both sides "accept": clean parity if kinds agree, else the
        // divergence must be a documented exemption.
        (false, Some(static_ty)) => {
            if !value_kind_matches_type(&runtime, static_ty) {
                assert!(
                    is_ledgered_divergence(op, &lt, &rt),
                    "{label}: UNLEDGERED divergence — runtime {runtime:?} (kind mismatch) \
                     vs static {static_ty:?} for {op:?}({lt:?}, {rt:?}); add a commented \
                     EXEMPTION_LEDGER row (PRD §3 decision 4) or fix the mismatch"
                );
            }
        }
        // Both sides "reject": clean parity (same contract as `assert_rejects`).
        (true, None) => {}
        // Accept/reject disagreement — not part of this task's known
        // exemption inventory (β1/β2 were written against the same §2
        // inventory, so no such row is expected to reach this engine).
        (runtime_is_undef, _) => panic!(
            "{label}: unclassified accept/reject divergence (runtime Undef={runtime_is_undef}, \
             static accepted={}) for {op:?}({lt:?}, {rt:?}) — not a documented exemption",
            static_result.is_some()
        ),
    }
}

/// (a) The seeded exemption (PRD §3 decision 4): `Int(7) / Int(2)`
/// (non-divisible) — runtime widens to `Value::Real(3.5)`, static stays
/// `Some(Type::Int)`. First confirms this IS a genuine kind divergence
/// (guards against the ledger accidentally "covering" a row that was never
/// divergent — if `value_kind_matches_type` ever starts agreeing here, this
/// assertion catches the ledger going stale), and that `(Div, Int, Int)` is
/// ledgered; then confirms the unified engine classifies the row
/// `diverges AND ledgered ⇒ OK` (does not panic).
#[test]
fn int_int_nondivisible_division_is_a_ledgered_divergence() {
    let runtime = eval_binop(BinOp::Div, Value::Int(7), Value::Int(2));
    assert!(
        !runtime.is_undef(),
        "expected non-Undef runtime result for Div(Int(7), Int(2)), got Undef"
    );
    let static_result = reify_compiler::__infer_mul_div_result_for_parity_test(
        BinOp::Div,
        &Type::Int,
        &Type::Int,
    );
    let static_ty = static_result.expect("expected static Some(Type::Int) for Div(Int, Int)");
    assert!(
        !value_kind_matches_type(&runtime, &static_ty),
        "expected a GENUINE kind divergence (runtime {runtime:?} vs static {static_ty:?}) — \
         if this now agrees, the EXEMPTION_LEDGER row is stale and should be removed"
    );
    assert!(
        is_ledgered_divergence(BinOp::Div, &Type::Int, &Type::Int),
        "Div(Int, Int) must be present in EXEMPTION_LEDGER"
    );

    // The unified engine must classify this row as OK (no panic).
    assert_parity_or_ledgered_divergence(
        BinOp::Div,
        Value::Int(7),
        Value::Int(2),
        Type::Int,
        Type::Int,
        "Div Int(7)/Int(2) non-divisible — ledgered exemption",
    );
}

// ── DATA-DRIVEN-Undef exclusion (step-7; PRD §7.2 carve-out) ────────────────

/// (c) `Int/0` and `Scalar/0` are DATA-DRIVEN-Undef (β1's
/// `div_int_by_zero_is_undef` / `div_scalar_by_zero_is_undef`): the
/// DIVISOR'S VALUE is zero, not the operand KINDS — `Int/Int` and
/// `Scalar[L]/Int` are otherwise intentional, dimension-preserving arms
/// (pinned in the positive batch above). PRD §7.2 excludes value-dependent
/// Undef from the structural-reject rule, so these rows are deliberately
/// ABSENT from `rejection_rows()` — this test documents why: the static
/// side does NOT reject `(Int, Int)`/`(Scalar[L], Int)` (it accepts, same as
/// the positive-batch rows), so asserting `infer_mul_div_result == None` for
/// them would be WRONG, not merely redundant.
#[test]
fn data_driven_undef_rows_are_excluded_from_structural_reject_rule() {
    // Int / 0 — value-dependent Undef.
    let int_by_zero = eval_binop(BinOp::Div, Value::Int(5), Value::Int(0));
    assert!(
        int_by_zero.is_undef(),
        "expected Int(5)/Int(0) to be runtime Undef (data-driven)"
    );
    assert_eq!(
        reify_compiler::__infer_mul_div_result_for_parity_test(BinOp::Div, &Type::Int, &Type::Int),
        Some(Type::Int),
        "Int/Int stays statically Some(Type::Int) regardless of the runtime divisor's value — \
         Int/0's Undef-ness is data-driven, not a static-typing rejection"
    );

    // Scalar[L] / 0 — same shape, dimensioned Scalar numerator.
    let scalar_by_zero = eval_binop(BinOp::Div, Value::length(5.0), Value::Int(0));
    assert!(
        scalar_by_zero.is_undef(),
        "expected Scalar[L](5.0)/Int(0) to be runtime Undef (data-driven)"
    );
    assert_eq!(
        reify_compiler::__infer_mul_div_result_for_parity_test(
            BinOp::Div,
            &Type::length(),
            &Type::Int
        ),
        Some(Type::length()),
        "Scalar[L]/Int stays statically Some(Scalar[L]) regardless of the runtime divisor's \
         value — Scalar/0's Undef-ness is data-driven, not a static-typing rejection"
    );
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
