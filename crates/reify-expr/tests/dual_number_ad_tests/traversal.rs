//! The core `CompiledExpr` traversal.
//!
//! The subject is `dual_eval::eval_dual`, which walks a real
//! `CompiledExpr` and returns a `DualValue`.  The single most important
//! property in this whole task is asserted first, and re-asserted over every
//! tree in the corpus:
//!
//!     THE PRIMAL INVARIANT — eval_dual(e).value == eval_expr(e)
//!
//! The dual evaluator computes *only tangents*.  Every primal it returns comes
//! from the existing implementation, so reify's value semantics (dimension
//! algebra, strict-Undef propagation, sanitize, the Invariant V
//! `Scalar{DIMENSIONLESS} → Real` collapse, `Point + Point` rejection) have
//! exactly ONE implementation.  If this invariant ever fails, a second
//! evaluator has been forked, which is the thing this task is forbidden to do.

use reify_core::{DimensionVector, Type, ValueCellId};
use reify_expr::{BranchRecord, EvalContext, Seeds, Tangent, eval_dual, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal, neg, value_ref_typed};

use crate::support::{ENT, assert_row_close, cell};

fn dim_len() -> DimensionVector {
    DimensionVector::LENGTH
}

fn scalar(v: f64, dim: DimensionVector) -> Value {
    Value::Scalar { si_value: v, dimension: dim }
}

/// The shared fixture: three seeded cells (`a`, `b` are `Scalar{LENGTH}`;
/// `c` is a bare `Real`) plus one *unseeded* cell `u`.
///
/// `c` being a `Value::Real` rather than a `Value::Scalar` is deliberate: reify
/// has THREE numeric scalar variants (`Int`, `Real`, `Scalar{si_value,
/// dimension}`) and `Value::from_real_scalar` collapses a dimensionless
/// `Scalar` to `Real` (Invariant V).  A dual evaluator that pattern-matches on
/// `Value::Scalar` alone would silently drop the tangent of every dimensionless
/// intermediate.  Joining via `Value::as_f64()` is the only correct route.
fn fixture() -> (ValueMap, Vec<ValueCellId>) {
    let mut values = ValueMap::new();
    values.insert(cell("a"), scalar(3.0, dim_len()));
    values.insert(cell("b"), scalar(4.0, dim_len()));
    values.insert(cell("c"), Value::Real(2.5));
    values.insert(cell("u"), scalar(7.0, dim_len()));
    (values, vec![cell("a"), cell("b"), cell("c")])
}

fn ref_a() -> CompiledExpr {
    value_ref_typed(ENT, "a", Type::Scalar { dimension: dim_len() })
}
fn ref_b() -> CompiledExpr {
    value_ref_typed(ENT, "b", Type::Scalar { dimension: dim_len() })
}
fn ref_c() -> CompiledExpr {
    // A bare `Real` is typed as a DIMENSIONLESS Scalar — reify has no `Type::Real`.
    value_ref_typed(ENT, "c", Type::Scalar { dimension: DimensionVector::DIMENSIONLESS })
}
fn ref_u() -> CompiledExpr {
    value_ref_typed(ENT, "u", Type::Scalar { dimension: dim_len() })
}

/// The whole step-3 corpus, as `(label, expr)`.  Every entry is run through the
/// primal invariant; the interesting ones additionally get an analytic-tangent
/// assertion below.
fn corpus() -> Vec<(&'static str, CompiledExpr)> {
    vec![
        ("literal_real", literal(Value::Real(5.0))),
        ("literal_scalar", literal(scalar(2.0, dim_len()))),
        ("ref_seeded_a", ref_a()),
        ("ref_seeded_c", ref_c()),
        ("ref_unseeded_u", ref_u()),
        ("neg_a", neg(ref_a())),
        ("add", binop(BinOp::Add, ref_a(), ref_b())),
        ("sub", binop(BinOp::Sub, ref_a(), ref_b())),
        ("mul", binop(BinOp::Mul, ref_a(), ref_b())),
        ("div", binop(BinOp::Div, ref_a(), ref_b())),
        ("mul_real_scalar", binop(BinOp::Mul, ref_c(), ref_a())),
        ("mul_scalar_int_literal", binop(BinOp::Mul, ref_a(), literal(Value::Int(3)))),
        ("pow_scalar_int2", binop(BinOp::Pow, ref_a(), literal(Value::Int(2)))),
        ("pow_scalar_int0", binop(BinOp::Pow, ref_a(), literal(Value::Int(0)))),
        ("pow_real_real", binop(BinOp::Pow, ref_c(), literal(Value::Real(3.0)))),
        ("mod_real", binop(BinOp::Mod, ref_c(), literal(Value::Real(1.0)))),
        // --- Undef cliffs ---------------------------------------------------
        ("undef_add_dim_mismatch", binop(BinOp::Add, ref_a(), ref_c())),
        ("undef_div_by_zero", binop(BinOp::Div, ref_a(), literal(scalar(0.0, dim_len())))),
        ("undef_pow_scalar_real", binop(BinOp::Pow, ref_a(), literal(Value::Real(2.0)))),
        ("undef_mod_scalar", binop(BinOp::Mod, ref_a(), ref_b())),
        // --- deep seed-free subtree ----------------------------------------
        (
            "seed_free_deep",
            binop(
                BinOp::Mul,
                binop(BinOp::Add, ref_u(), literal(scalar(2.0, dim_len()))),
                binop(BinOp::Sub, ref_u(), literal(scalar(1.0, dim_len()))),
            ),
        ),
    ]
}

/// Evaluate one corpus entry and return `(primal, tangent)`.
fn dual_of(expr: &CompiledExpr) -> (Value, Tangent) {
    let (values, seed_cells) = fixture();
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dv = eval_dual(expr, &ctx, &seeds, &mut record);
    (dv.value, dv.tangent)
}

/// Materialise a tangent as a concrete row, failing loudly if it is `None`.
fn row_of(expr: &CompiledExpr, label: &str) -> Vec<f64> {
    let (_, t) = dual_of(expr);
    t.materialize(3).unwrap_or_else(|| panic!("{label}: expected a differentiable tangent, got None"))
}

// ---------------------------------------------------------------------------
// (1) THE PRIMAL INVARIANT
// ---------------------------------------------------------------------------

#[test]
fn dual_primal_matches_eval_expr_across_the_whole_corpus() {
    let (values, seed_cells) = fixture();
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&seed_cells);
    for (label, expr) in corpus() {
        let mut record = BranchRecord::new();
        let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
        let plain = eval_expr(&expr, &ctx);
        assert_eq!(
            dual.value, plain,
            "{label}: eval_dual must reuse the existing value semantics verbatim \
             (dual={:?}, eval_expr={:?})",
            dual.value, plain
        );
    }
}

#[test]
fn every_undef_cliff_in_the_corpus_really_is_undef_so_the_invariant_has_teeth() {
    // Guards the test above from silently degenerating: if a future change made
    // these expressions produce a finite value, the Undef half of the primal
    // invariant would stop being exercised and nobody would notice.
    for label in
        ["undef_add_dim_mismatch", "undef_div_by_zero", "undef_pow_scalar_real", "undef_mod_scalar"]
    {
        let expr = corpus().into_iter().find(|(l, _)| *l == label).unwrap().1;
        let (values, _) = fixture();
        let ctx = EvalContext::simple(&values);
        assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "{label} must evaluate to Undef");
    }
}

// ---------------------------------------------------------------------------
// (2) Leaves: seeded ValueRef, unseeded ValueRef, Literal
// ---------------------------------------------------------------------------

#[test]
fn a_seeded_value_ref_yields_the_basis_tangent_for_its_column() {
    for (member, j) in [("a", 0usize), ("b", 1), ("c", 2)] {
        let dim = if member == "c" { DimensionVector::DIMENSIONLESS } else { dim_len() };
        let ty = Type::Scalar { dimension: dim };
        let e = value_ref_typed(ENT, member, ty);
        let row = row_of(&e, member);
        let expected: Vec<f64> = (0..3).map(|k| if k == j { 1.0 } else { 0.0 }).collect();
        assert_eq!(row, expected, "seeded ref `{member}` must be column {j}");
    }
}

#[test]
fn an_unseeded_value_ref_and_a_literal_yield_tangent_zero_not_a_zero_row() {
    // `Tangent::Zero` is a *provable* statement ("this subtree contains no
    // seed"), which is materially different from a `Scalar` row that happens to
    // be all zeros — the former lets the evaluator skip the whole subtree.
    let (_, t_ref) = dual_of(&ref_u());
    assert_eq!(t_ref, Tangent::Zero, "an unseeded ValueRef is provably seed-independent");

    let (_, t_lit) = dual_of(&literal(Value::Real(5.0)));
    assert_eq!(t_lit, Tangent::Zero, "a literal is provably seed-independent");
}

// ---------------------------------------------------------------------------
// (3) Analytic tangents for each arithmetic operation
// ---------------------------------------------------------------------------

#[test]
fn negation_tangent_is_the_negated_row() {
    assert_row_close(&row_of(&neg(ref_a()), "neg_a"), &[-1.0, 0.0, 0.0], "neg_a");
}

#[test]
fn add_and_sub_tangents_are_componentwise() {
    assert_row_close(&row_of(&binop(BinOp::Add, ref_a(), ref_b()), "add"), &[1.0, 1.0, 0.0], "add");
    assert_row_close(&row_of(&binop(BinOp::Sub, ref_a(), ref_b()), "sub"), &[1.0, -1.0, 0.0], "sub");
}

#[test]
fn mul_tangent_is_the_product_rule_over_the_si_values() {
    // a = 3 m, b = 4 m  →  ∂(ab)/∂a = b = 4, ∂(ab)/∂b = a = 3
    assert_row_close(&row_of(&binop(BinOp::Mul, ref_a(), ref_b()), "mul"), &[4.0, 3.0, 0.0], "mul");
}

#[test]
fn div_tangent_is_the_quotient_rule_over_the_si_values() {
    // ∂(a/b)/∂a = 1/b = 0.25 ; ∂(a/b)/∂b = -a/b² = -3/16
    assert_row_close(
        &row_of(&binop(BinOp::Div, ref_a(), ref_b()), "div"),
        &[0.25, -3.0 / 16.0, 0.0],
        "div",
    );
}

#[test]
fn tangents_join_through_real_int_and_scalar_mixes_via_as_f64() {
    // c (Real 2.5) * a (Scalar 3 m) → ∂/∂a = c = 2.5, ∂/∂c = a = 3.0
    assert_row_close(
        &row_of(&binop(BinOp::Mul, ref_c(), ref_a()), "mul_real_scalar"),
        &[2.5, 0.0, 3.0],
        "mul_real_scalar",
    );
    // a * Int(3) → ∂/∂a = 3
    assert_row_close(
        &row_of(&binop(BinOp::Mul, ref_a(), literal(Value::Int(3))), "mul_scalar_int"),
        &[3.0, 0.0, 0.0],
        "mul_scalar_int",
    );
}

#[test]
fn pow_tangent_with_a_constant_integer_exponent_is_the_power_rule() {
    // ∂(a²)/∂a = 2a = 6
    assert_row_close(
        &row_of(&binop(BinOp::Pow, ref_a(), literal(Value::Int(2))), "pow2"),
        &[6.0, 0.0, 0.0],
        "pow2",
    );
    // ∂(c³)/∂c = 3c² = 18.75
    assert_row_close(
        &row_of(&binop(BinOp::Pow, ref_c(), literal(Value::Real(3.0))), "pow_real"),
        &[0.0, 0.0, 3.0 * 2.5 * 2.5],
        "pow_real",
    );
}

#[test]
fn pow_with_a_zero_exponent_collapses_to_an_exactly_zero_tangent() {
    let e = binop(BinOp::Pow, ref_a(), literal(Value::Int(0)));
    let (value, t) = dual_of(&e);
    // Invariant V: dimension.pow(0) == DIMENSIONLESS, so the primal collapses
    // from Scalar to Real.
    assert_eq!(value, Value::Real(1.0), "a⁰ collapses to a dimensionless Real");
    assert_eq!(t.materialize(3).unwrap(), vec![0.0, 0.0, 0.0], "d(a⁰)/da is exactly zero");
}

#[test]
fn mod_tangent_is_one_in_the_dividend_almost_everywhere() {
    // c % 1.0 with c = 2.5 → 0.5 ; ∂/∂c = 1 away from the wrap points
    assert_row_close(
        &row_of(&binop(BinOp::Mod, ref_c(), literal(Value::Real(1.0))), "mod"),
        &[0.0, 0.0, 1.0],
        "mod",
    );
}

// ---------------------------------------------------------------------------
// (4) The result's DIMENSION is whatever the primal helper produced
// ---------------------------------------------------------------------------

#[test]
fn dual_evaluation_preserves_the_dimension_algebra_of_the_primal_helpers() {
    let (mul_v, _) = dual_of(&binop(BinOp::Mul, ref_a(), ref_b()));
    assert_eq!(
        mul_v,
        Value::Scalar { si_value: 12.0, dimension: dim_len().mul(&dim_len()) },
        "Scalar·Scalar multiplies values and ADDS dimension exponents"
    );

    let (div_v, _) = dual_of(&binop(BinOp::Div, ref_a(), ref_b()));
    assert_eq!(
        div_v,
        Value::Real(0.75),
        "L/L cancels, and Invariant V collapses Scalar{{DIMENSIONLESS}} → Real"
    );

    let (pow_v, _) = dual_of(&binop(BinOp::Pow, ref_a(), literal(Value::Int(2))));
    assert_eq!(
        pow_v,
        Value::Scalar { si_value: 9.0, dimension: dim_len().pow(2) },
        "Scalar^Int raises the value and multiplies dimension exponents"
    );
}

// ---------------------------------------------------------------------------
// (5) A seed-free subtree is Tangent::Zero regardless of depth
// ---------------------------------------------------------------------------

#[test]
fn a_subtree_with_no_seeded_cell_is_tangent_zero_at_any_depth() {
    let (_, t) = dual_of(&corpus().into_iter().find(|(l, _)| *l == "seed_free_deep").unwrap().1);
    assert_eq!(t, Tangent::Zero, "a deep seed-free subtree short-circuits to Tangent::Zero");

    // And nesting it under a seeded node still leaves the seeded columns right:
    // (u+2)(u−1) is a constant 54 m², so d/da [a · (u+2)(u−1)] = 54.
    let seed_free = corpus().into_iter().find(|(l, _)| *l == "seed_free_deep").unwrap().1;
    let mixed = binop(BinOp::Mul, ref_a(), seed_free);
    assert_row_close(&row_of(&mixed, "mixed"), &[9.0 * 6.0, 0.0, 0.0], "mixed");
}

// ---------------------------------------------------------------------------
// (6) The Undef cliff: a non-finite primal never carries a bogus derivative
// ---------------------------------------------------------------------------

#[test]
fn an_undef_primal_yields_tangent_none_never_a_plausible_looking_zero() {
    for label in
        ["undef_add_dim_mismatch", "undef_div_by_zero", "undef_pow_scalar_real", "undef_mod_scalar"]
    {
        let expr = corpus().into_iter().find(|(l, _)| *l == label).unwrap().1;
        let (value, t) = dual_of(&expr);
        assert_eq!(value, Value::Undef, "{label} primal");
        assert_eq!(
            t,
            Tangent::None,
            "{label}: an Undef primal has no derivative — reporting Tangent::Zero here would \
             tell the solver the residual does not move with the variable, which is a false claim"
        );
    }
}
