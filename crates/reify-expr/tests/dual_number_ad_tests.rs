//! Forward-mode dual-number automatic differentiation — task #6672
//! (solver-unification ε).
//!
//! Design reference: `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.7 ("Derivatives — three sources").  This file is a *separate test
//! binary*, so it exercises only the *published public surface* of
//! `reify_expr::dual` / `reify_expr::dual_eval`, which is exactly the surface
//! the downstream consumers see: η (#6675, the Gauss-Newton/LM step and its
//! `‖Jᵀr‖` certificate), μ (#6680, the reduced gradient) and λ (#6679, kink
//! chatter detection).
//!
//! This first block pins the `Dual` *scalar* arithmetic contract with no
//! `CompiledExpr` in sight: if these rules are wrong, every Jacobian built on
//! top of them is wrong in a way no integration test can localise.

use reify_expr::dual::Dual;

/// Tolerance for the hand-computed algebraic identities below.  These are
/// closed-form comparisons of two f64 expression *orderings*, not
/// finite-difference comparisons, so the only error present is last-place
/// rounding; 1e-15 relative is roughly 4 ulp.
fn assert_close(actual: f64, expected: f64, what: &str) {
    let tol = 1e-15 * expected.abs() + 1e-300;
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: got {actual:?}, expected {expected:?} (tol {tol:?})"
    );
}

// ---------------------------------------------------------------------------
// (1) Seeding and lifting
// ---------------------------------------------------------------------------

#[test]
fn seed_produces_the_jth_basis_tangent_and_constant_produces_a_zero_tangent() {
    let n = 4;
    for j in 0..n {
        let d = Dual::seed(j, n);
        assert_eq!(d.width(), n, "seed({j}, {n}) width");
        assert_eq!(d.tangent.len(), n, "seed({j}, {n}) tangent length");
        let expected: Vec<f64> = (0..n).map(|k| if k == j { 1.0 } else { 0.0 }).collect();
        assert_eq!(d.tangent, expected, "seed({j}, {n}) must be the basis vector e_{j}");
    }

    let c = Dual::constant(7.5, n);
    assert_eq!(c.value, 7.5);
    assert_eq!(c.width(), n);
    assert_eq!(c.tangent, vec![0.0; n], "a constant carries an all-zero tangent");
}

#[test]
fn with_value_sets_the_primal_without_disturbing_the_seeded_tangent() {
    let d = Dual::seed(1, 3).with_value(-2.25);
    assert_eq!(d.value, -2.25);
    assert_eq!(d.tangent, vec![0.0, 1.0, 0.0]);
}

#[test]
fn width_zero_is_representable_so_a_seedless_problem_is_not_a_special_case() {
    let c = Dual::constant(3.0, 0);
    assert_eq!(c.width(), 0);
    assert!(c.tangent.is_empty());
    let s = c.add(&Dual::constant(4.0, 0));
    assert_eq!(s.value, 7.0);
    assert!(s.tangent.is_empty());
}

// ---------------------------------------------------------------------------
// (2) Sum and difference rules
// ---------------------------------------------------------------------------

/// A two-variable probe point: `f = 3*x0 + x1`, `g = x0*x1`, evaluated at
/// `(x0, x1) = (2.0, 5.0)`.  Returns `(f, g)` as duals of width 2.
fn probe_pair() -> (Dual, Dual, f64, f64) {
    let (x0, x1) = (2.0_f64, 5.0_f64);
    let dx0 = Dual::seed(0, 2).with_value(x0);
    let dx1 = Dual::seed(1, 2).with_value(x1);
    // f = 3*x0 + x1  →  f' = (3, 1)
    let f = Dual::constant(3.0, 2).mul(&dx0).add(&dx1);
    // g = x0*x1      →  g' = (x1, x0)
    let g = dx0.mul(&dx1);
    (f, g, x0, x1)
}

#[test]
fn sum_rule_adds_tangents_componentwise() {
    let (f, g, x0, x1) = probe_pair();
    let s = f.add(&g);
    assert_close(s.value, (3.0 * x0 + x1) + x0 * x1, "(f+g) value");
    // f' = (3, 1); g' = (x1, x0)
    assert_close(s.tangent[0], 3.0 + x1, "(f+g)'_0 = f'_0 + g'_0");
    assert_close(s.tangent[1], 1.0 + x0, "(f+g)'_1 = f'_1 + g'_1");
}

#[test]
fn difference_rule_subtracts_tangents_componentwise() {
    let (f, g, x0, x1) = probe_pair();
    let d = f.sub(&g);
    assert_close(d.value, (3.0 * x0 + x1) - x0 * x1, "(f-g) value");
    assert_close(d.tangent[0], 3.0 - x1, "(f-g)'_0 = f'_0 - g'_0");
    assert_close(d.tangent[1], 1.0 - x0, "(f-g)'_1 = f'_1 - g'_1");
}

// ---------------------------------------------------------------------------
// (3) Product rule
// ---------------------------------------------------------------------------

#[test]
fn product_rule_is_f_prime_g_plus_f_g_prime() {
    let (f, g, x0, x1) = probe_pair();
    let (fv, gv) = (f.value, g.value);
    let p = f.mul(&g);
    assert_close(p.value, fv * gv, "(f*g) value");
    // f' = (3, 1); g' = (x1, x0)
    assert_close(p.tangent[0], 3.0 * gv + fv * x1, "(f*g)'_0");
    assert_close(p.tangent[1], 1.0 * gv + fv * x0, "(f*g)'_1");
}

#[test]
fn product_rule_matches_the_expanded_polynomial_derivative() {
    // h(x0, x1) = (3*x0 + x1) * (x0*x1) = 3*x0^2*x1 + x0*x1^2
    // ∂h/∂x0 = 6*x0*x1 + x1^2 ; ∂h/∂x1 = 3*x0^2 + 2*x0*x1
    let (f, g, x0, x1) = probe_pair();
    let h = f.mul(&g);
    assert_close(h.tangent[0], 6.0 * x0 * x1 + x1 * x1, "∂h/∂x0");
    assert_close(h.tangent[1], 3.0 * x0 * x0 + 2.0 * x0 * x1, "∂h/∂x1");
}

// ---------------------------------------------------------------------------
// (4) Quotient rule
// ---------------------------------------------------------------------------

#[test]
fn quotient_rule_is_f_prime_g_minus_f_g_prime_over_g_squared() {
    let (f, g, x0, x1) = probe_pair();
    let (fv, gv) = (f.value, g.value);
    let q = f.div(&g);
    assert_close(q.value, fv / gv, "(f/g) value");
    assert_close(q.tangent[0], (3.0 * gv - fv * x1) / (gv * gv), "(f/g)'_0");
    assert_close(q.tangent[1], (1.0 * gv - fv * x0) / (gv * gv), "(f/g)'_1");
}

#[test]
fn reciprocal_derivative_is_minus_one_over_x_squared() {
    let x = Dual::seed(0, 1).with_value(4.0);
    let r = Dual::constant(1.0, 1).div(&x);
    assert_close(r.value, 0.25, "1/x value");
    assert_close(r.tangent[0], -1.0 / 16.0, "d(1/x)/dx = -1/x²");
}

// ---------------------------------------------------------------------------
// (5) Power rules
// ---------------------------------------------------------------------------

#[test]
fn integer_power_rule_is_k_f_pow_k_minus_one_times_f_prime() {
    let x = Dual::seed(0, 1).with_value(3.0);
    // (x^4)' = 4x^3 = 108
    let p = x.powi(4);
    assert_close(p.value, 81.0, "x^4 value");
    assert_close(p.tangent[0], 108.0, "d(x^4)/dx");

    // Negative exponent: (x^-2)' = -2x^-3
    let m = x.powi(-2);
    assert_close(m.value, 1.0 / 9.0, "x^-2 value");
    assert_close(m.tangent[0], -2.0 / 27.0, "d(x^-2)/dx");

    // k = 0 collapses to a constant with a zero tangent.
    let z = x.powi(0);
    assert_eq!(z.value, 1.0, "x^0 value");
    assert_eq!(z.tangent, vec![0.0], "d(x^0)/dx must be exactly zero");

    // k = 1 is the identity on the tangent.
    let one = x.powi(1);
    assert_close(one.value, 3.0, "x^1 value");
    assert_close(one.tangent[0], 1.0, "d(x^1)/dx");
}

#[test]
fn integer_power_rule_chains_through_an_inner_expression() {
    // f = 3*x0 + x1 at (2, 5) → f = 11, f' = (3, 1); (f^3)' = 3f²·f'
    let (f, _g, _x0, _x1) = probe_pair();
    let p = f.powi(3);
    let fv = 11.0_f64;
    assert_close(p.value, fv.powi(3), "f^3 value");
    assert_close(p.tangent[0], 3.0 * fv * fv * 3.0, "(f^3)'_0");
    assert_close(p.tangent[1], 3.0 * fv * fv * 1.0, "(f^3)'_1");
}

#[test]
fn general_power_rule_for_positive_base_is_f_pow_g_times_g_prime_ln_f_plus_g_f_prime_over_f() {
    // f = 3*x0 + x1 = 11 (f' = (3, 1)); g = x0*x1 = 10 (g' = (5, 2)) at (2, 5).
    let (f, g, x0, x1) = probe_pair();
    let (fv, gv) = (f.value, g.value);
    let p = f.powf(&g);
    assert_close(p.value, fv.powf(gv), "f^g value");

    let expect = |fp: f64, gp: f64| fv.powf(gv) * (gp * fv.ln() + gv * fp / fv);
    assert_close(p.tangent[0], expect(3.0, x1), "(f^g)'_0");
    assert_close(p.tangent[1], expect(1.0, x0), "(f^g)'_1");
}

#[test]
fn general_power_rule_with_a_constant_exponent_agrees_with_the_integer_rule() {
    let x = Dual::seed(0, 1).with_value(2.5);
    let via_powf = x.powf(&Dual::constant(3.0, 1));
    let via_powi = x.powi(3);
    assert_close(via_powf.value, via_powi.value, "x^3 value agreement");
    assert_close(via_powf.tangent[0], via_powi.tangent[0], "d(x^3)/dx agreement");
}

// ---------------------------------------------------------------------------
// (6) Negation
// ---------------------------------------------------------------------------

#[test]
fn negation_flips_the_sign_of_value_and_every_tangent_component() {
    let (f, _g, _x0, _x1) = probe_pair();
    let n = f.neg();
    assert_close(n.value, -f.value, "(-f) value");
    assert_close(n.tangent[0], -3.0, "(-f)'_0");
    assert_close(n.tangent[1], -1.0, "(-f)'_1");
}

// ---------------------------------------------------------------------------
// (7) THE AFFINE EXACTNESS IDENTITY
// ---------------------------------------------------------------------------

/// For `r(x) = a·x + b` with `a` finite, positive and non-subnormal, forward
/// mode computes the tangent as `fl(a·1.0) + fl(0.0·x)` then `fl(a + 0.0)`.
/// Multiplication by exactly `1.0` and addition of exactly `+0.0` are both
/// *exact* in IEEE-754 binary64 — neither introduces a rounding step — so the
/// resulting tangent is bit-identical to `a`.
///
/// This is asserted with `assert_eq!` and NO tolerance, and it is the *only*
/// assertion in this task's suite that is entitled to do so: every other
/// derivative comparison goes through the finite-difference tolerance.
#[test]
fn affine_residual_recovers_its_slope_bit_exactly() {
    let slopes = [
        1.0_f64,
        3.0,
        0.1,
        1.0 / 3.0,
        std::f64::consts::PI,
        1e-7,
        1e17,
        f64::MIN_POSITIVE, // smallest non-subnormal positive
        f64::MAX / 4.0,
    ];
    let intercepts = [0.0_f64, 1.0, -12.5, 1e9];
    let points = [0.0_f64, 1.0, -3.75, 1e6, -1e-6];

    for &a in &slopes {
        for &b in &intercepts {
            for &x0 in &points {
                let x = Dual::seed(0, 1).with_value(x0);
                let r = Dual::constant(a, 1).mul(&x).add(&Dual::constant(b, 1));
                assert_eq!(
                    r.tangent[0],
                    a,
                    "affine tangent must be bit-identical to the slope: \
                     a={a:?}, b={b:?}, x={x0:?}, got bits {:#018x} want {:#018x}",
                    r.tangent[0].to_bits(),
                    a.to_bits(),
                );
            }
        }
    }
}

#[test]
fn affine_residual_is_exact_in_every_column_of_a_multivariate_seed() {
    // r(x) = 2*x0 - 7*x1 + 0.5*x2 + 3
    let coeffs = [2.0_f64, -7.0, 0.5];
    let n = coeffs.len();
    let point = [1.25_f64, -0.5, 9.0];
    let mut acc = Dual::constant(3.0, n);
    for (j, &a) in coeffs.iter().enumerate() {
        let xj = Dual::seed(j, n).with_value(point[j]);
        acc = acc.add(&Dual::constant(a, n).mul(&xj));
    }
    for (j, &a) in coeffs.iter().enumerate() {
        assert_eq!(acc.tangent[j], a, "column {j} of an affine residual must be exact");
    }
}

// ---------------------------------------------------------------------------
// (8) Width mismatch is loud, never a silent truncation
// ---------------------------------------------------------------------------
//
// A width mismatch means two duals were seeded against *different* column
// vectors; combining them cannot produce a meaningful gradient row.  Silently
// truncating to the shorter width (or zero-extending to the longer) would
// hand the solver a well-typed WRONG Jacobian — the worst possible shape.
// The documented contract is a panic naming both widths.

#[test]
#[should_panic(expected = "dual width mismatch")]
fn add_with_mismatched_widths_panics_rather_than_truncating() {
    let _ = Dual::constant(1.0, 2).add(&Dual::constant(2.0, 3));
}

#[test]
#[should_panic(expected = "dual width mismatch")]
fn sub_with_mismatched_widths_panics_rather_than_truncating() {
    let _ = Dual::constant(1.0, 2).sub(&Dual::constant(2.0, 3));
}

#[test]
#[should_panic(expected = "dual width mismatch")]
fn mul_with_mismatched_widths_panics_rather_than_truncating() {
    let _ = Dual::constant(1.0, 4).mul(&Dual::constant(2.0, 1));
}

#[test]
#[should_panic(expected = "dual width mismatch")]
fn div_with_mismatched_widths_panics_rather_than_truncating() {
    let _ = Dual::constant(1.0, 4).div(&Dual::constant(2.0, 1));
}

#[test]
#[should_panic(expected = "dual width mismatch")]
fn powf_with_mismatched_widths_panics_rather_than_truncating() {
    let _ = Dual::constant(2.0, 2).powf(&Dual::constant(3.0, 5));
}

#[test]
#[should_panic(expected = "seed index")]
fn seed_index_out_of_range_panics_rather_than_producing_a_zero_tangent() {
    // `seed(3, 3)` has no basis vector — the caller mis-sized the column
    // vector.  Returning an all-zero tangent here would silently produce a
    // zero Jacobian column.
    let _ = Dual::seed(3, 3);
}

// ===========================================================================
// Step-3: the core `CompiledExpr` traversal
// ===========================================================================
//
// From here on the subject is `dual_eval::eval_dual`, which walks a real
// `CompiledExpr` and returns a `DualValue`.  The single most important
// property in this whole task is asserted first, and re-asserted over every
// tree in the corpus:
//
//     THE PRIMAL INVARIANT — eval_dual(e).value == eval_expr(e)
//
// The dual evaluator computes *only tangents*.  Every primal it returns comes
// from the existing implementation, so reify's value semantics (dimension
// algebra, strict-Undef propagation, sanitize, the Invariant V
// `Scalar{DIMENSIONLESS} → Real` collapse, `Point + Point` rejection) have
// exactly ONE implementation.  If this invariant ever fails, a second
// evaluator has been forked, which is the thing this task is forbidden to do.

use reify_core::{DimensionVector, Type, ValueCellId};
use reify_expr::branch_signature::BranchRecord;
use reify_expr::dual::Tangent;
use reify_expr::dual_eval::{Seeds, eval_dual};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};
use reify_test_support::builders::expr::{binop, literal, neg, value_ref_typed};

const ENT: &str = "part";

fn dim_len() -> DimensionVector {
    DimensionVector::LENGTH
}

fn scalar(v: f64, dim: DimensionVector) -> Value {
    Value::Scalar { si_value: v, dimension: dim }
}

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new(ENT, member)
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

fn assert_row_close(actual: &[f64], expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: row width");
    for (j, (&got, &want)) in actual.iter().zip(expected.iter()).enumerate() {
        let tol = 1e-15 * want.abs() + 1e-300;
        assert!(
            (got - want).abs() <= tol,
            "{label} column {j}: got {got:?}, expected {want:?}"
        );
    }
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

// ===========================================================================
// Step-5: agreement with central differences — THE HEADLINE SIGNAL
// ===========================================================================
//
// Every assertion below compares the AD tangent against a central-difference
// reference computed entirely through `eval_expr` on a perturbed `ValueMap`.
// The reference therefore shares NO code with the thing under test: if the
// dual traversal and the value evaluator ever disagree about what a builtin
// does, this suite says so.
//
// The step shape is the one already in `crates/reify-expr/src/calculus.rs:714`
//
//     h = 1e-6 * |x_j|.max(1e-3),   f'(x) ≈ (f(x+h) − f(x−h)) / (2h)
//
// reused rather than invented, so this suite is calibrated against the
// finite-difference machinery reify already ships.
//
// TOLERANCE, DERIVED — `|ad − cd| <= 1e-6·|cd| + 1e-8`:
//
//   * truncation error of central differences is h²·|f'''|/6; at |x| ≈ 1,
//     h = 1e-6 gives h²/6 ≈ 1.7e-13, and for this corpus (polynomials and
//     sin/cos/exp/sqrt/log/atan2 at well-conditioned points) |f'''| ≲ 10,
//     so ≲ 2e-12;
//   * cancellation error is ε_mach·|f|/h ≈ 2.2e-16·|f|/1e-6 = 2.2e-10·|f|;
//     for |f| ≲ 10 that is ≲ 2.2e-9.
//
// Total reference error ≲ 3e-9 absolute, i.e. ≲ 3e-8 relative against a
// derivative of magnitude ≥ 0.1.  The asserted 1e-6 relative bound clears the
// reference's own floor by ~1.5 orders; asserting anything tighter would be
// testing the reference rather than the subject.
//
// Every probe point is chosen so |∂r/∂x_j| >= 0.1 in SI units, which makes the
// RELATIVE arm binding and leaves the 1e-8 absolute floor guarding exact-zero
// columns only.  That precondition is itself asserted, so the suite cannot
// quietly decay into comparing two numbers that are both ~0.

use reify_test_support::builders::expr::fn_call;

/// Build a fixture of dimensionless `Real` cells, all seeded, in order.
fn probe(cells: &[(&str, f64)]) -> (ValueMap, Vec<ValueCellId>) {
    let mut values = ValueMap::new();
    let mut seed_cells = Vec::new();
    for (name, v) in cells {
        values.insert(cell(name), Value::Real(*v));
        seed_cells.push(cell(name));
    }
    (values, seed_cells)
}

fn pref(member: &str) -> CompiledExpr {
    value_ref_typed(ENT, member, Type::Scalar { dimension: DimensionVector::DIMENSIONLESS })
}

fn call1(name: &str, a: CompiledExpr) -> CompiledExpr {
    fn_call(name, &format!("std::{name}"), vec![a], Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    })
}

fn calln(name: &str, args: Vec<CompiledExpr>) -> CompiledExpr {
    fn_call(name, &format!("std::{name}"), args, Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    })
}

/// Central-difference reference for `∂expr/∂cell`, computed only through
/// `eval_expr` on a perturbed copy of the value map.
fn central_difference(expr: &CompiledExpr, values: &ValueMap, target: &ValueCellId) -> f64 {
    let base = values.get(target).cloned().expect("probe cell must exist");
    let x = base.as_f64().expect("probe cell must be numeric");
    let dim = base.dimension();
    // The step shape from calculus.rs:714 — relative for large |x|, with an
    // absolute floor so the step does not collapse to zero near the origin.
    let h = 1e-6_f64 * x.abs().max(1e-3);

    let at = |v: f64| -> f64 {
        let mut perturbed = values.clone();
        perturbed.insert(target.clone(), Value::from_real_scalar(v, dim));
        let ctx = EvalContext::simple(&perturbed);
        eval_expr(expr, &ctx).as_f64().expect("perturbed evaluation must stay numeric")
    };
    (at(x + h) - at(x - h)) / (2.0 * h)
}

/// The suite's single assertion shape: one dual traversal produces the whole
/// gradient row, and every column is checked against its own central
/// difference.
fn assert_ad_matches_cd(label: &str, expr: &CompiledExpr, values: &ValueMap, seed_cells: &[ValueCellId]) {
    let ctx = EvalContext::simple(values);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(expr, &ctx, &seeds, &mut record);

    assert_eq!(
        dual.value,
        eval_expr(expr, &ctx),
        "{label}: the primal invariant must hold for builtins too"
    );

    let ad = dual
        .tangent
        .materialize(seed_cells.len())
        .unwrap_or_else(|| panic!("{label}: expected a differentiable tangent, got Tangent::None"));

    for (j, target) in seed_cells.iter().enumerate() {
        let cd = central_difference(expr, values, target);
        assert!(
            cd.abs() >= 0.1,
            "{label} column {j}: probe point must have |∂r/∂x_j| >= 0.1 in SI units so the \
             RELATIVE arm of the tolerance binds; got {cd:?}. Fix the probe point, not the \
             tolerance."
        );
        let tol = 1e-6 * cd.abs() + 1e-8;
        assert!(
            (ad[j] - cd).abs() <= tol,
            "{label} column {j}: ad={:?} vs central-difference {cd:?} (tol {tol:?})",
            ad[j]
        );
    }
}

/// One-variable convenience: seed a single cell `x` at `x0` and compare.
fn assert_unary_builtin(name: &str, x0: f64) {
    let (values, seed_cells) = probe(&[("x", x0)]);
    let expr = call1(name, pref("x"));
    assert_ad_matches_cd(name, &expr, &values, &seed_cells);
}

// --- single-argument smooth builtins ---------------------------------------

#[test]
fn sqrt_tangent_agrees_with_central_differences() {
    assert_unary_builtin("sqrt", 2.0); // d/dx = 1/(2√2) ≈ 0.354
}

#[test]
fn exp_tangent_agrees_with_central_differences() {
    assert_unary_builtin("exp", 0.7); // d/dx = e^0.7 ≈ 2.014
}

#[test]
fn log_is_the_natural_logarithm_and_its_tangent_agrees_with_central_differences() {
    // There is no `ln` binding in the stdlib — `log` IS the natural log.
    assert_unary_builtin("log", 2.0); // d/dx = 1/2
}

#[test]
fn log10_tangent_agrees_with_central_differences() {
    assert_unary_builtin("log10", 2.0); // d/dx = 1/(2·ln 10) ≈ 0.217
}

#[test]
fn sin_tangent_agrees_with_central_differences() {
    assert_unary_builtin("sin", 0.7); // d/dx = cos(0.7) ≈ 0.765
}

#[test]
fn cos_tangent_agrees_with_central_differences() {
    assert_unary_builtin("cos", 0.7); // d/dx = −sin(0.7) ≈ −0.644
}

#[test]
fn tan_tangent_agrees_with_central_differences() {
    assert_unary_builtin("tan", 0.7); // d/dx = sec²(0.7) ≈ 1.690
}

#[test]
fn asin_tangent_agrees_with_central_differences() {
    assert_unary_builtin("asin", 0.5); // d/dx = 1/√0.75 ≈ 1.155
}

#[test]
fn acos_tangent_agrees_with_central_differences() {
    assert_unary_builtin("acos", 0.5); // d/dx = −1/√0.75 ≈ −1.155
}

#[test]
fn atan_tangent_agrees_with_central_differences() {
    assert_unary_builtin("atan", 0.5); // d/dx = 1/1.25 = 0.8
}

#[test]
fn sinh_tangent_agrees_with_central_differences() {
    assert_unary_builtin("sinh", 0.7); // d/dx = cosh(0.7) ≈ 1.255
}

#[test]
fn cosh_tangent_agrees_with_central_differences() {
    assert_unary_builtin("cosh", 0.7); // d/dx = sinh(0.7) ≈ 0.759
}

#[test]
fn tanh_tangent_agrees_with_central_differences() {
    assert_unary_builtin("tanh", 0.7); // d/dx = 1 − tanh²(0.7) ≈ 0.637
}

#[test]
fn abs_tangent_away_from_the_origin_agrees_with_central_differences() {
    // `abs` is a kink AT zero; away from zero it is smooth with derivative
    // sign(x).  Both sides are probed, because a sign error is invisible if you
    // only ever test the positive branch.
    assert_unary_builtin("abs", 2.0);
    let (values, seed_cells) = probe(&[("x", -1.3)]);
    assert_ad_matches_cd("abs_negative", &call1("abs", pref("x")), &values, &seed_cells);
}

// --- multi-argument smooth builtins ----------------------------------------

#[test]
fn atan2_tangent_agrees_with_central_differences_in_both_columns() {
    // NOTE the argument order: atan2(y, x), matching the stdlib binding.
    // ∂/∂y = x/(x²+y²) ≈ 0.368 ; ∂/∂x = −y/(x²+y²) ≈ −0.221
    let (values, seed_cells) = probe(&[("y", 1.2), ("x", 2.0)]);
    let expr = calln("atan2", vec![pref("y"), pref("x")]);
    assert_ad_matches_cd("atan2", &expr, &values, &seed_cells);
}

#[test]
fn pow_tangent_agrees_with_central_differences_in_both_columns() {
    // ∂/∂x = y·x^(y−1) ≈ 0.569 ; ∂/∂y = x^y·ln x ≈ 1.126
    let (values, seed_cells) = probe(&[("x", 2.0), ("y", 0.7)]);
    let expr = calln("pow", vec![pref("x"), pref("y")]);
    assert_ad_matches_cd("pow", &expr, &values, &seed_cells);
}

#[test]
fn lerp_tangent_agrees_with_central_differences_in_all_three_columns() {
    // lerp(a, b, t) = a + t(b − a): ∂/∂a = 1−t = 0.7, ∂/∂b = t = 0.3,
    // ∂/∂t = b−a = 4.0
    let (values, seed_cells) = probe(&[("a", 1.0), ("b", 5.0), ("t", 0.3)]);
    let expr = calln("lerp", vec![pref("a"), pref("b"), pref("t")]);
    assert_ad_matches_cd("lerp", &expr, &values, &seed_cells);
}

#[test]
fn remap_tangent_agrees_with_central_differences_in_every_column() {
    // remap(x, flo, fhi, tlo, thi) = tlo + (x−flo)(thi−tlo)/(fhi−flo)
    // At x=2, flo=0.5, fhi=10, tlo=100, thi=200 every column is well clear of
    // the 0.1 floor.
    let (values, seed_cells) =
        probe(&[("x", 2.0), ("flo", 0.5), ("fhi", 10.0), ("tlo", 100.0), ("thi", 200.0)]);
    let expr = calln("remap", vec![
        pref("x"),
        pref("flo"),
        pref("fhi"),
        pref("tlo"),
        pref("thi"),
    ]);
    assert_ad_matches_cd("remap", &expr, &values, &seed_cells);
}

// --- multivariate composites: one traversal, whole gradient row ------------

#[test]
fn circle_residual_gradient_agrees_with_central_differences_in_both_columns() {
    // r(x, y) = sqrt(x² + y²) − 5 — the canonical distance constraint.
    // ∂/∂x = x/√(x²+y²) = 0.6 ; ∂/∂y = 0.8
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let sum = binop(
        BinOp::Add,
        binop(BinOp::Pow, pref("x"), literal(Value::Int(2))),
        binop(BinOp::Pow, pref("y"), literal(Value::Int(2))),
    );
    let expr = binop(BinOp::Sub, call1("sqrt", sum), literal(Value::Real(5.0)));
    assert_ad_matches_cd("circle_residual", &expr, &values, &seed_cells);
}

#[test]
fn three_variable_trig_composite_gradient_agrees_with_central_differences() {
    // f(a, b, c) = sin(a)·cos(b) + exp(−c)
    // ∂/∂a = cos a cos b ≈ 0.704 ; ∂/∂b = −sin a sin b ≈ −0.251 ;
    // ∂/∂c = −e^(−c) ≈ −0.741
    let (values, seed_cells) = probe(&[("a", 0.7), ("b", 0.4), ("c", 0.3)]);
    let expr = binop(
        BinOp::Add,
        binop(BinOp::Mul, call1("sin", pref("a")), call1("cos", pref("b"))),
        call1("exp", neg(pref("c"))),
    );
    assert_ad_matches_cd("trig_composite", &expr, &values, &seed_cells);
}

#[test]
fn nested_sqrt_and_tanh_composite_gradient_agrees_with_central_differences() {
    // f(x, y) = sqrt(x·x + y·y) · tanh(x − y) — nests a builtin inside a
    // builtin inside arithmetic, so a chain-rule slip anywhere shows up.
    let (values, seed_cells) = probe(&[("x", 2.0), ("y", 1.2)]);
    let norm = call1(
        "sqrt",
        binop(
            BinOp::Add,
            binop(BinOp::Mul, pref("x"), pref("x")),
            binop(BinOp::Mul, pref("y"), pref("y")),
        ),
    );
    let expr =
        binop(BinOp::Mul, norm, call1("tanh", binop(BinOp::Sub, pref("x"), pref("y"))));
    assert_ad_matches_cd("sqrt_tanh_composite", &expr, &values, &seed_cells);
}

// ===========================================================================
// Step-11: arbitrary user algebra — the chain rule through user functions
// ===========================================================================
//
// PRD §7.7 gives AD one job the other two derivative sources cannot do:
// "constraint residuals of arbitrary user algebra".  Arbitrary means the
// residual may call functions the user wrote, which call functions the user
// wrote, and the chain rule has to survive the trip.

use std::sync::Arc;

use reify_core::ContentHash;
use reify_core::ValueCellId as VCell;
use reify_ir::{CompiledFnBody, CompiledFunction, FieldSourceKind};
use reify_test_support::builders::expr::{conditional_expr, user_fn_call};

fn dl() -> Type {
    Type::Scalar { dimension: DimensionVector::DIMENSIONLESS }
}

/// A cell inside function `fname`'s own scope — the shape
/// `eval_compiled_function_with_values` binds params under.
fn param_ref(fname: &str, pname: &str) -> CompiledExpr {
    CompiledExpr::value_ref(VCell::new(fname, pname), dl())
}

fn user_fn(
    name: &str,
    param_names: &[&str],
    let_bindings: Vec<(String, CompiledExpr)>,
    result_expr: CompiledExpr,
) -> CompiledFunction {
    let params: Vec<(String, Type)> =
        param_names.iter().map(|p| ((*p).to_string(), dl())).collect();
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: dl(),
        body: CompiledFnBody { let_bindings, result_expr },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    }
}

/// `fn hyp(a, b) = sqrt(a*a + b*b)`
fn hyp_fn() -> CompiledFunction {
    user_fn(
        "hyp",
        &["a", "b"],
        vec![],
        call1(
            "sqrt",
            binop(
                BinOp::Add,
                binop(BinOp::Mul, param_ref("hyp", "a"), param_ref("hyp", "a")),
                binop(BinOp::Mul, param_ref("hyp", "b"), param_ref("hyp", "b")),
            ),
        ),
    )
}

/// The step-5 central-difference driver, with user functions in scope.
fn assert_ad_matches_cd_with_fns(
    label: &str,
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
    functions: &[CompiledFunction],
) {
    let ctx = EvalContext::new(values, functions);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, eval_expr(expr, &ctx), "{label}: the primal invariant");

    let ad = dual
        .tangent
        .materialize(seed_cells.len())
        .unwrap_or_else(|| panic!("{label}: expected a differentiable tangent, got Tangent::None"));

    for (j, target) in seed_cells.iter().enumerate() {
        // Central differences, driven only through `eval_expr`.
        let base = values.get(target).cloned().expect("probe cell");
        let x = base.as_f64().expect("numeric probe cell");
        let dim = base.dimension();
        let h = 1e-6_f64 * x.abs().max(1e-3);
        let at = |v: f64| -> f64 {
            let mut perturbed = values.clone();
            perturbed.insert(target.clone(), Value::from_real_scalar(v, dim));
            eval_expr(expr, &EvalContext::new(&perturbed, functions))
                .as_f64()
                .expect("perturbed evaluation must stay numeric")
        };
        let cd = (at(x + h) - at(x - h)) / (2.0 * h);
        assert!(
            cd.abs() >= 0.1,
            "{label} column {j}: probe point must have |∂r/∂x_j| >= 0.1 so the relative arm \
             binds; got {cd:?}"
        );
        let tol = 1e-6 * cd.abs() + 1e-8;
        assert!(
            (ad[j] - cd).abs() <= tol,
            "{label} column {j}: ad={:?} vs central-difference {cd:?} (tol {tol:?})",
            ad[j]
        );
    }
}

// ---------------------------------------------------------------------------
// (1) + (2) A user-function residual: tangent vs central differences, and the
//           primal invariant
// ---------------------------------------------------------------------------

#[test]
fn a_user_function_residual_gradient_agrees_with_central_differences() {
    // r(x, y) = hyp(x, y) − 5, the distance constraint written as user algebra.
    // ∂r/∂x = x/hyp = 0.6, ∂r/∂y = 0.8 at (3, 4).
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = binop(
        BinOp::Sub,
        user_fn_call("hyp", vec![pref("x"), pref("y")], dl()),
        literal(Value::Real(5.0)),
    );
    assert_ad_matches_cd_with_fns("hyp_residual", &expr, &values, &seed_cells, &[hyp_fn()]);
}

#[test]
fn a_user_function_call_keeps_the_primal_invariant_exactly() {
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let fns = [hyp_fn()];
    let ctx = EvalContext::new(&values, &fns);
    let expr = user_fn_call("hyp", vec![pref("x"), pref("y")], dl());
    let seeds = Seeds::new(&seed_cells);
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, Value::Real(5.0));
    assert_eq!(dual.value, eval_expr(&expr, &ctx));
}

#[test]
fn a_user_function_let_binding_carries_its_tangent_into_the_result_expression() {
    // fn norm2(a, b) { let s = a*a + b*b; sqrt(s) }
    let f = user_fn(
        "norm2",
        &["a", "b"],
        vec![(
            "s".to_string(),
            binop(
                BinOp::Add,
                binop(BinOp::Mul, param_ref("norm2", "a"), param_ref("norm2", "a")),
                binop(BinOp::Mul, param_ref("norm2", "b"), param_ref("norm2", "b")),
            ),
        )],
        call1("sqrt", CompiledExpr::value_ref(VCell::new("norm2", "s"), dl())),
    );
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = user_fn_call("norm2", vec![pref("x"), pref("y")], dl());
    assert_ad_matches_cd_with_fns("norm2_let", &expr, &values, &seed_cells, &[f]);
}

// ---------------------------------------------------------------------------
// (3) Nested user-function calls
// ---------------------------------------------------------------------------

#[test]
fn a_user_function_calling_another_user_function_propagates_the_chain_rule() {
    // fn twice_hyp(a, b) = hyp(a, b) * 2
    let outer = user_fn(
        "twice_hyp",
        &["a", "b"],
        vec![],
        binop(
            BinOp::Mul,
            user_fn_call("hyp", vec![param_ref("twice_hyp", "a"), param_ref("twice_hyp", "b")], dl()),
            literal(Value::Real(2.0)),
        ),
    );
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = user_fn_call("twice_hyp", vec![pref("x"), pref("y")], dl());
    assert_ad_matches_cd_with_fns("nested", &expr, &values, &seed_cells, &[hyp_fn(), outer]);
}

// ---------------------------------------------------------------------------
// (4) A Lambda applied through `apply_lambda`
// ---------------------------------------------------------------------------

/// A `Value::Field` whose source is Analytical and whose lambda is `p ↦ p·p·p`.
/// Registering it at the cell `__field__::cube` makes `cube(x)` in an
/// expression resolve to `apply_lambda`, which is the reachable scalar-in
/// scalar-out lambda-application route in a residual.
fn cube_field_cell() -> (ValueCellId, Value) {
    let p = VCell::new("$lambda_cube", "p");
    let body = binop(
        BinOp::Mul,
        CompiledExpr::value_ref(p.clone(), dl()),
        binop(
            BinOp::Mul,
            CompiledExpr::value_ref(p.clone(), dl()),
            CompiledExpr::value_ref(p.clone(), dl()),
        ),
    );
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p)],
        body: Box::new(body),
        captures: ValueMap::new(),
    };
    let field = Value::Field {
        domain_type: dl(),
        codomain_type: dl(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };
    (ValueCellId::new(reify_core::FIELD_ENTITY_PREFIX, "cube"), field)
}

#[test]
fn a_lambda_applied_inside_a_residual_propagates_its_argument_tangent() {
    // cube(x) with x = 2 → 8, d/dx = 3x² = 12.
    let (mut values, seed_cells) = probe(&[("x", 2.0)]);
    let (cell_id, field) = cube_field_cell();
    values.insert(cell_id, field);
    let expr = call1("cube", pref("x"));
    assert_ad_matches_cd_with_fns("lambda_cube", &expr, &values, &seed_cells, &[]);
}

// ---------------------------------------------------------------------------
// (5) A kink inside a user-function body
// ---------------------------------------------------------------------------

#[test]
fn a_kink_inside_a_user_function_body_is_recorded_at_a_site_that_includes_the_call_site() {
    // fn clip(a) = if a > 1 then a*a else a
    let f = user_fn(
        "clip",
        &["a"],
        vec![],
        conditional_expr(
            binop(BinOp::Gt, param_ref("clip", "a"), literal(Value::Real(1.0))),
            binop(BinOp::Mul, param_ref("clip", "a"), param_ref("clip", "a")),
            param_ref("clip", "a"),
        ),
    );
    let fns = [f];
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let ctx = EvalContext::new(&values, &fns);
    let seeds = Seeds::new(&seed_cells);

    let expr = user_fn_call("clip", vec![pref("x")], dl());
    let mut record = BranchRecord::new();
    let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
    assert_eq!(dual.value, Value::Real(9.0));
    assert_eq!(
        dual.tangent.materialize(1).expect("differentiable"),
        vec![6.0],
        "the ACTIVE branch's derivative: d(a²)/da = 2a = 6"
    );

    // The comparison and the conditional are both kinks, and both are inside
    // the callee — so neither may report the bare root site, or two distinct
    // call sites of `clip` would be indistinguishable to λ.
    assert_eq!(record.len(), 2, "the `>` comparison and the `if`");
    for entry in record.entries() {
        assert!(
            !entry.site.path().is_empty(),
            "a kink inside a callee must carry the call-site descent in its path, got {:?}",
            entry.site.path()
        );
    }

    // Two distinct call sites of the same function get distinct sites.
    let two_calls = binop(
        BinOp::Add,
        user_fn_call("clip", vec![pref("x")], dl()),
        user_fn_call("clip", vec![pref("x")], dl()),
    );
    let mut rec2 = BranchRecord::new();
    let _ = eval_dual(&two_calls, &ctx, &seeds, &mut rec2);
    assert_eq!(rec2.len(), 4, "two calls × two kinks each");
    let sites: Vec<&[u16]> = rec2.entries().iter().map(|e| e.site.path()).collect();
    assert_ne!(sites[0], sites[2], "the same kink at two call sites must not collide");
    assert_ne!(sites[1], sites[3]);
}

// ---------------------------------------------------------------------------
// (6) The recursion guard
// ---------------------------------------------------------------------------

#[test]
fn unbounded_user_function_recursion_yields_undef_and_no_tangent_rather_than_a_stack_overflow() {
    // fn recur(a) = recur(a) — `eval_user_function_call` stops at
    // MAX_RECURSION_DEPTH (256) and returns Undef.  The dual evaluator must
    // hit the SAME guard, not run deeper and blow the 3 MiB test-thread stack.
    //
    // THE STACK SIZE IS PINNED, NOT "GENEROUS" — the same discipline as
    // `eval_user_fn_recursion_depth_exceeded` in `reify-expr/src/lib.rs`, which
    // this test is the dual-path sibling of.  Rust's test harness gives a
    // SPAWNED test thread only 2 MiB, which is already short of what plain
    // `eval_expr` needs at depth 256, so the wrapper is mandatory rather than
    // decorative.  `eval_dual`'s own frames sit on top of that budget, so this
    // figure is larger than the evaluator's 3 MiB — and it stays a working pin
    // for the dual path's per-frame budget only while it is tight enough that a
    // frame-size regression actually overflows it.
    //
    // MEASURED on this tree at MAX_RECURSION_DEPTH (256), debug profile, by
    // bisecting this constant: overflows at 2048 KiB, passes at 2176 KiB.  That
    // is within noise of the evaluator's own measured 2304 KiB, i.e. the dual
    // traversal costs no more per level than `eval_expr` does — which is the
    // whole point of the `#[inline(never)]` split around `eval_dual_fn_body`
    // and `eval_dual_lambda_apply`, and is what this test exists to keep true.
    //
    // 3 MiB = ~41% over the measured 2176 KiB requirement: enough that ordinary
    // codegen drift does not redden the gate, tight enough that a further
    // ~900 KiB (~3.5 KiB/level) regression does.  If a toolchain bump reddens
    // this, RE-MEASURE by bisecting the constant and update the figures here —
    // do not just raise it for headroom, which would silently retire the pin.
    let handle = std::thread::Builder::new()
        .stack_size(3 * 1024 * 1024)
        .spawn(|| {
            let f = user_fn("recur", &["a"], vec![], user_fn_call(
                "recur",
                vec![param_ref("recur", "a")],
                dl(),
            ));
            let fns = [f];
            let (values, seed_cells) = probe(&[("x", 2.0)]);
            let ctx = EvalContext::new(&values, &fns);
            let seeds = Seeds::new(&seed_cells);
            let expr = user_fn_call("recur", vec![pref("x")], dl());

            assert_eq!(eval_expr(&expr, &ctx), Value::Undef, "the existing guard returns Undef");
            let mut record = BranchRecord::new();
            let dual = eval_dual(&expr, &ctx, &seeds, &mut record);
            assert_eq!(dual.value, Value::Undef, "the dual path must hit the same guard");
            assert_eq!(
                dual.tangent,
                Tangent::None,
                "an Undef primal has no derivative — least of all a zero one"
            );
        })
        .expect("spawn");
    handle.join().expect("the recursion guard must fire before the stack runs out");
}

// ===========================================================================
// Step-13: the refusal is LOUD, never a silent zero row
// ===========================================================================
//
// PRD §7.7: "Non-numeric operands stop contributing phantom gradients."
// INV-SF-7: "a well-typed wrong value is the worst shape."
//
// `jacobian_row` is the boundary where a tangent becomes a number η can put in
// a matrix.  A zero row is a CLAIM — "this residual does not move when you
// move this variable" — and when the truth is "we could not tell", that claim
// is false in the most damaging possible way: the solver believes the residual
// is already stationary in that direction and stops pushing on it.  So every
// path that cannot produce a derivative returns a typed refusal naming the
// construct responsible, which η turns into a tier-2 refusal instead of
// stalling on a gradient it has no reason to trust.

use reify_expr::dual_eval::{NonDifferentiable, jacobian_row};

fn jrow(
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
) -> Result<(f64, Vec<f64>), NonDifferentiable> {
    let ctx = EvalContext::simple(values);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    jacobian_row(expr, &ctx, &seeds, &mut record)
}

/// `[literal, expr][index]` — an `IndexAccess` node, a kind this task does not
/// differentiate.
fn index_access(object: CompiledExpr, index: CompiledExpr) -> CompiledExpr {
    let content_hash =
        ContentHash::of(b"index_access").combine(object.content_hash).combine(index.content_hash);
    CompiledExpr {
        kind: reify_ir::CompiledExprKind::IndexAccess {
            object: Box::new(object),
            index: Box::new(index),
        },
        result_type: dl(),
        content_hash,
    }
}

// ---------------------------------------------------------------------------
// (1) The happy path
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_returns_the_si_primal_and_a_full_width_row_for_a_smooth_residual() {
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    // r = sqrt(x² + y²) − 5
    let expr = binop(
        BinOp::Sub,
        call1(
            "sqrt",
            binop(
                BinOp::Add,
                binop(BinOp::Mul, pref("x"), pref("x")),
                binop(BinOp::Mul, pref("y"), pref("y")),
            ),
        ),
        literal(Value::Real(5.0)),
    );
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("smooth residual");

    let ctx = EvalContext::simple(&values);
    assert_eq!(
        primal,
        eval_expr(&expr, &ctx).as_f64().unwrap(),
        "the returned primal is the residual's own SI value, not a recomputation"
    );
    assert_eq!(row.len(), seed_cells.len(), "one column per seed, always");
    assert!((row[0] - 0.6).abs() < 1e-12);
    assert!((row[1] - 0.8).abs() < 1e-12);
}

#[test]
fn a_residual_that_truly_does_not_move_gets_an_honest_zero_row() {
    // The distinction this whole step exists to preserve: a PROVABLE zero and
    // an unavailable derivative must not look alike.  Here the zero is real.
    let (values, seed_cells) = probe(&[("x", 3.0), ("y", 4.0)]);
    let expr = literal(Value::Real(7.0));
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("a constant is differentiable");
    assert_eq!(primal, 7.0);
    assert_eq!(row, vec![0.0, 0.0]);
}

// ---------------------------------------------------------------------------
// (2) An Undef primal
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_refuses_an_undef_primal_instead_of_returning_a_zero_row() {
    let mut values = ValueMap::new();
    values.insert(cell("x"), Value::Scalar { si_value: 3.0, dimension: DimensionVector::LENGTH });
    values.insert(cell("d"), Value::Real(0.0));
    let seed_cells = vec![cell("x"), cell("d")];

    // Dimension-mismatched addition: Scalar{LENGTH} + Real.
    let mismatch = binop(BinOp::Add, pref("x"), literal(Value::Real(1.0)));
    assert!(
        matches!(
            jrow(&mismatch, &values, &seed_cells),
            Err(NonDifferentiable::UndefPrimal { .. })
        ),
        "a dimension mismatch must refuse, got {:?}",
        jrow(&mismatch, &values, &seed_cells)
    );

    // Division by zero.
    let div0 = binop(BinOp::Div, pref("x"), pref("d"));
    assert!(matches!(
        jrow(&div0, &values, &seed_cells),
        Err(NonDifferentiable::UndefPrimal { .. })
    ));
}

// ---------------------------------------------------------------------------
// (3) An unsupported kind — but ONLY when it is seed-dependent
// ---------------------------------------------------------------------------

#[test]
fn a_seed_dependent_unsupported_kind_is_named_in_the_refusal() {
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    // [x, 1.0][0] — IndexAccess is not differentiated, and the list depends on
    // the seed, so the derivative is genuinely unknown here.
    let expr = index_access(
        reify_test_support::builders::expr::list_expr(vec![pref("x"), literal(Value::Real(1.0))]),
        literal(Value::Int(0)),
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert!(
                !kind.is_empty(),
                "the refusal must NAME the construct, or the user cannot act on it"
            );
        }
        other => panic!("expected UnsupportedKind, got {other:?}"),
    }
}

#[test]
fn the_same_unsupported_kind_is_an_honest_zero_row_when_it_is_seed_independent() {
    // The contrast that makes the refusal meaningful: an `IndexAccess` that
    // cannot reach a seed has a derivative, and it is zero.  Refusing here
    // would make every residual containing any unsupported construct
    // undifferentiable, however irrelevant that construct is.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let constant_index = index_access(
        reify_test_support::builders::expr::list_expr(vec![
            literal(Value::Real(2.0)),
            literal(Value::Real(1.0)),
        ]),
        literal(Value::Int(0)),
    );
    // r = x + [2.0, 1.0][0]
    let expr = binop(BinOp::Add, pref("x"), constant_index);
    let (primal, row) = jrow(&expr, &values, &seed_cells).expect("seed-independent ⇒ zero, not refusal");
    assert_eq!(primal, 5.0);
    assert_eq!(row, vec![1.0]);
}

// ---------------------------------------------------------------------------
// (4) A non-scalar root
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_rejects_a_non_scalar_root_naming_what_it_got() {
    let (values, seed_cells) = probe(&[("x", 3.0)]);

    // Bool root — a comparison is not a residual.
    let boolean = binop(BinOp::Gt, pref("x"), literal(Value::Real(5.0)));
    match jrow(&boolean, &values, &seed_cells) {
        Err(NonDifferentiable::NonScalarResult { got }) => assert_eq!(got, "Bool"),
        other => panic!("expected NonScalarResult, got {other:?}"),
    }

    // Point root.
    let point = literal(Value::Point(vec![Value::Real(1.0), Value::Real(2.0)]));
    match jrow(&point, &values, &seed_cells) {
        Err(NonDifferentiable::NonScalarResult { got }) => assert_eq!(got, "Point"),
        other => panic!("expected NonScalarResult, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (5) A non-finite tangent component
// ---------------------------------------------------------------------------

#[test]
fn jacobian_row_rejects_a_non_finite_tangent_even_when_the_primal_is_finite() {
    // r = y / x with x = 1e-200: the primal is a perfectly ordinary 1e200, but
    // the quotient rule's denominator x² UNDERFLOWS to exactly zero, so both
    // columns come out ±Inf.  Handing those to a linear solve would poison the
    // whole step, and the finite primal gives no hint anything is wrong.
    let (values, seed_cells) = probe(&[("y", 1.0), ("x", 1e-200)]);
    let expr = binop(BinOp::Div, pref("y"), pref("x"));

    let ctx = EvalContext::simple(&values);
    assert_eq!(
        eval_expr(&expr, &ctx),
        Value::Real(1e200),
        "the primal really is finite — that is what makes this case dangerous"
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::NonFiniteTangent { column }) => {
            assert!(column < 2, "the refusal names which column blew up");
        }
        other => panic!("expected NonFiniteTangent, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (6) Display
// ---------------------------------------------------------------------------

#[test]
fn every_non_differentiable_variant_display_names_the_offending_construct() {
    use reify_expr::branch_signature::KinkSite;
    let cases = [
        NonDifferentiable::UndefPrimal { site: KinkSite::new(vec![1, 2]) },
        NonDifferentiable::UnsupportedKind {
            kind: "IndexAccess",
            site: KinkSite::new(vec![0]),
        },
        NonDifferentiable::NonScalarResult { got: "Point" },
        NonDifferentiable::NonFiniteTangent { column: 3 },
    ];
    for case in &cases {
        let text = case.to_string();
        assert!(!text.is_empty(), "every variant must render");
        // The message has to be actionable on its own: η copies it into a
        // tier-2 refusal, where the user sees it without the surrounding code.
        assert!(
            text.len() > 20,
            "a refusal that only says its variant name is not actionable: {text:?}"
        );
    }
    assert!(
        NonDifferentiable::UnsupportedKind { kind: "IndexAccess", site: KinkSite::root() }
            .to_string()
            .contains("IndexAccess"),
        "the offending kind must appear verbatim in the message"
    );
    assert!(
        NonDifferentiable::NonScalarResult { got: "Point" }.to_string().contains("Point")
    );
    assert!(
        NonDifferentiable::NonFiniteTangent { column: 3 }.to_string().contains('3'),
        "the failing column index must appear"
    );
    // It is an error type, so `?` works in η's assembly loop.
    fn assert_is_error<E: std::error::Error>(_: &E) {}
    assert_is_error(&NonDifferentiable::NonFiniteTangent { column: 0 });
}

// ===========================================================================
// Step-21: the BOUNDED field reductions — max/min/argmax/argmin(field, bounds)
// ===========================================================================
//
// `eval_expr` intercepts TWO arities of field reduction, not one.  Alongside
// the 1-argument arms (reify-expr/src/lib.rs:459-484) it intercepts the
// BOUNDED form `max/min/argmax/argmin(field, bounds: BoundingBox)` at
// lib.rs:490-533 and routes it to `field_reductions::compute_*_bounded`.
//
// The dual path has to mirror BOTH, because the module's central invariant —
// `eval_dual(e).value == eval_expr(e)` — is only true "by construction" while
// the two interception tables agree.  When they do not, the failure is silent
// and expensive: `max(field, bbox)` is claimed instead by the BINARY NUMERIC
// `min`/`max` kink, handed to `reify_stdlib::eval_builtin`, and comes back
// `Value::Undef` because `Value::Field` has no `as_f64` mapping.  η then
// reports "residual evaluates to undef" for the canonical FEA-in-the-loop
// constraint `max(von_mises_field, region_bbox) - limit <= 0`, which the
// ordinary evaluator handles perfectly well.
//
// This block asserts the VALUE-semantics half of the contract; the branch
// record half lives in `dual_branch_signature_tests.rs`.

use std::sync::atomic::AtomicBool;

use reify_expr::dual_eval::{DualEnv, jacobian_row_with_env};
use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

/// A 1-D sampled field over x ∈ {0, 1, 2, 3} with data {5, 9, 2, 7}.
///
/// Global max 9 at x = 1, global min 2 at x = 2.  Restricted to x ∈ [2, 3] the
/// answers all MOVE: max 7 at x = 3, min 2 at x = 2.  The global maximiser is
/// deliberately outside the box, so a bounded result is distinguishable from
/// an unbounded one by value alone.
fn bounded_probe_field() -> Value {
    let sf = SampledField {
        name: "probe".into(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![3.0],
        spacing: vec![1.0],
        axis_grids: vec![vec![0.0, 1.0, 2.0, 3.0]],
        interpolation: InterpolationKind::Linear,
        data: vec![5.0, 9.0, 2.0, 7.0],
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: dl(),
        codomain_type: dl(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// The canonical `bounding_box(solid)` shape: two 3-component `Value::Point`
/// corners.  The field above has one axis, so only the x span is consulted.
fn bbox_x(lo: f64, hi: f64) -> Value {
    Value::BoundingBox {
        min: Box::new(Value::Point(vec![Value::Real(lo), Value::Real(0.0), Value::Real(0.0)])),
        max: Box::new(Value::Point(vec![Value::Real(hi), Value::Real(0.0), Value::Real(0.0)])),
    }
}

/// `(label, expected bounded answer)` for the four bounded reductions over
/// `bounded_probe_field()` restricted to x ∈ [2, 3].
const BOUNDED_CASES: [(&str, f64); 4] =
    [("max", 7.0), ("min", 2.0), ("argmax", 3.0), ("argmin", 2.0)];

/// Evaluate with an explicit cell map, returning `(primal, tangent)` and the
/// `eval_expr` reference primal alongside it.
fn dual_and_plain(
    expr: &CompiledExpr,
    values: &ValueMap,
    seed_cells: &[ValueCellId],
) -> (Value, Tangent, Value) {
    let ctx = EvalContext::simple(values);
    let seeds = Seeds::new(seed_cells);
    let mut record = BranchRecord::new();
    let dv = eval_dual(expr, &ctx, &seeds, &mut record);
    let plain = eval_expr(expr, &ctx);
    (dv.value, dv.tangent, plain)
}

// ---------------------------------------------------------------------------
// (1) THE PRIMAL INVARIANT at arity 2
// ---------------------------------------------------------------------------

#[test]
fn bounded_field_reductions_keep_the_primal_invariant_and_are_not_undef() {
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    for (name, expected) in BOUNDED_CASES {
        let expr =
            calln(name, vec![literal(bounded_probe_field()), literal(bbox_x(2.0, 3.0))]);
        let (dual_v, _, plain) = dual_and_plain(&expr, &values, &seed_cells);

        // The invariant itself.
        assert_eq!(
            dual_v, plain,
            "{name}(field, bbox): eval_dual must reuse the existing value semantics verbatim"
        );
        // ...with teeth: `Undef == Undef` would satisfy the line above while
        // both paths were broken.  The reference must be a real number.
        assert_ne!(
            plain,
            Value::Undef,
            "{name}(field, bbox): eval_expr must produce the bounded extremum, not Undef"
        );
        let got = plain
            .as_f64()
            .unwrap_or_else(|| panic!("{name}(field, bbox): expected a scalar, got {plain:?}"));
        assert!(
            (got - expected).abs() < 1e-12,
            "{name}(field, bbox) over x ∈ [2,3] must be the BOUNDS-RESTRICTED answer \
             {expected}, got {got} (the unbounded answer would be 9 / x=1)"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) Seed-independent ⇒ Zero, not a refusal
// ---------------------------------------------------------------------------

#[test]
fn a_seed_independent_bounded_reduction_is_flat_rather_than_a_refusal() {
    // Both operands are literals, so the reduction provably cannot move with
    // the seeds.  `Tangent::Zero` is the honest answer; `Tangent::None` here
    // would poison an otherwise perfectly differentiable Jacobian row.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    for (name, _) in BOUNDED_CASES {
        let expr =
            calln(name, vec![literal(bounded_probe_field()), literal(bbox_x(2.0, 3.0))]);
        let (_, tangent, _) = dual_and_plain(&expr, &values, &seed_cells);
        assert_eq!(
            tangent,
            Tangent::Zero,
            "{name}(field, bbox): a frozen bounded reduction is flat, not undifferentiable"
        );
    }
}

#[test]
fn a_bounded_reduction_inside_a_residual_contributes_a_zero_column_not_a_refusal() {
    // The end-to-end shape η actually sees: `x - max(field, bbox)`.  The row
    // must come back finite, with the reduction contributing nothing.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let expr = binop(
        BinOp::Sub,
        pref("x"),
        calln("max", vec![literal(bounded_probe_field()), literal(bbox_x(2.0, 3.0))]),
    );
    let (primal, row) =
        jrow(&expr, &values, &seed_cells).expect("a frozen bounded reduction is differentiable");
    assert!((primal - (2.0 - 7.0)).abs() < 1e-12, "primal must be 2 − 7 = −5, got {primal}");
    assert_eq!(row, vec![1.0]);
}

// ---------------------------------------------------------------------------
// (3) A seed-dependent FIELD refuses
// ---------------------------------------------------------------------------

#[test]
fn a_bounded_reduction_over_a_seed_dependent_field_refuses_a_tangent() {
    // This task does not differentiate THROUGH a field (PRD §7.7 makes field
    // and geometry derivatives the ANALYTIC source, not the AD one), so the
    // only honest answer is `Tangent::None` — matching the arity-1 rule.
    for (name, _) in BOUNDED_CASES {
        let mut values = ValueMap::new();
        values.insert(cell("fld"), bounded_probe_field());
        let seed_cells = vec![cell("fld")];
        let expr = calln(name, vec![
            value_ref_typed(ENT, "fld", dl()),
            literal(bbox_x(2.0, 3.0)),
        ]);
        let (_, tangent, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_ne!(plain, Value::Undef, "{name}: the reference primal is still a real number");
        assert_eq!(
            tangent,
            Tangent::None,
            "{name}(seeded field, bbox): a zero row would claim the residual is flat \
             in a variable it genuinely depends on"
        );
    }
}

// ---------------------------------------------------------------------------
// (4) A seed-dependent BOUNDS operand refuses too
// ---------------------------------------------------------------------------

#[test]
fn a_bounded_reduction_whose_bounds_move_with_the_seeds_refuses_a_tangent() {
    // The bounding box is itself a seeded cell, so the box — and therefore
    // WHICH grid node wins — genuinely moves with the solved variables.  A
    // tangent decision taken from the field operand alone would report
    // `Tangent::Zero` here, telling the solver the residual is flat in a
    // variable that in fact selects its value.
    for (name, _) in BOUNDED_CASES {
        let mut values = ValueMap::new();
        values.insert(cell("bb"), bbox_x(2.0, 3.0));
        let seed_cells = vec![cell("bb")];
        let expr = calln(name, vec![
            literal(bounded_probe_field()),
            value_ref_typed(ENT, "bb", Type::BoundingBox),
        ]);
        let (_, tangent, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_ne!(plain, Value::Undef, "{name}: the reference primal is still a real number");
        assert_eq!(
            tangent,
            Tangent::None,
            "{name}(field, seeded bbox): a seed-dependent box must refuse, never claim Zero"
        );
    }
}

#[test]
fn the_refusal_from_a_moving_bounded_reduction_names_the_field_reduction_itself() {
    // η copies this message into a tier-2 refusal, where the user reads it
    // without the surrounding code, so it has to name the construct.
    //
    // The field is reached through a `DualEnv` overlay rather than a seed
    // column — the shape the solver actually produces for a DERIVED cell — so
    // the reduction node is the FIRST thing on the path with no derivative
    // rule, and its own refusal is the one that survives `note_refusal`'s
    // first-wins rule.  (Seeding the field cell directly instead refuses one
    // level earlier, at "a seeded cell holding a non-scalar value"; that is
    // the arity-1 behaviour too, and is asserted by the tangent tests above.)
    let mut values = ValueMap::new();
    values.insert(cell("fld"), bounded_probe_field());
    let ctx = EvalContext::simple(&values);
    let seeds = Seeds::new(&[cell("x")]);
    let mut env = DualEnv::new();
    env.bind(cell("fld"), Tangent::Scalar(vec![1.0]));
    let expr =
        calln("max", vec![value_ref_typed(ENT, "fld", dl()), literal(bbox_x(2.0, 3.0))]);
    let mut record = BranchRecord::new();
    match jacobian_row_with_env(&expr, &ctx, &seeds, &env, &mut record) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert!(
                kind.contains("field reduction"),
                "the refusal must name the field reduction, got {kind:?}"
            );
        }
        other => panic!("expected an UnsupportedKind refusal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (5) NEGATIVE CONTROLS — the fix must stay narrow
// ---------------------------------------------------------------------------

#[test]
fn the_binary_numeric_max_over_two_scalars_is_untouched_by_the_bounded_route() {
    // `max(a, b)` over two ordinary scalars is the BINARY NUMERIC kink and must
    // keep selecting an operand and adopting its tangent.
    let (values, seed_cells) = probe(&[("a", 3.0), ("b", 5.0)]);
    for (name, winner, col) in [("max", 5.0, 1usize), ("min", 3.0, 0usize)] {
        let expr = calln(name, vec![pref("a"), pref("b")]);
        let (primal, row) = jrow(&expr, &values, &seed_cells)
            .unwrap_or_else(|e| panic!("{name}(a, b) must stay differentiable, got {e:?}"));
        assert!((primal - winner).abs() < 1e-12, "{name}(3, 5) = {winner}, got {primal}");
        let expected: Vec<f64> = (0..2).map(|j| if j == col { 1.0 } else { 0.0 }).collect();
        assert_eq!(row, expected, "{name}: the selected operand's tangent, unchanged");
    }
}

#[test]
fn malformed_two_argument_reduction_shapes_still_agree_with_eval_expr_on_undef() {
    // `eval_expr`'s arity-2 guard (lib.rs:490) requires a `BoundingBox` second
    // argument and a `Field` first one; anything else deliberately falls
    // through to `eval_builtin`, which returns `Undef` for these operand types
    // (numeric.rs:67).  The dual path must fall through in exactly the same
    // places — a widened intercept that swallowed these would be a SECOND
    // divergence, in the opposite direction.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let cases: [(&str, CompiledExpr); 4] = [
        (
            "max(field, 3.0)",
            calln("max", vec![literal(bounded_probe_field()), literal(Value::Real(3.0))]),
        ),
        (
            "min(field, 3.0)",
            calln("min", vec![literal(bounded_probe_field()), literal(Value::Real(3.0))]),
        ),
        ("max(2.0, bbox)", calln("max", vec![
            literal(Value::Real(2.0)),
            literal(bbox_x(2.0, 3.0)),
        ])),
        ("argmax(2.0, bbox)", calln("argmax", vec![
            literal(Value::Real(2.0)),
            literal(bbox_x(2.0, 3.0)),
        ])),
    ];
    for (label, expr) in cases {
        let (dual_v, _, plain) = dual_and_plain(&expr, &values, &seed_cells);
        assert_eq!(plain, Value::Undef, "{label}: eval_expr must still fall through to Undef");
        assert_eq!(dual_v, plain, "{label}: the dual path must fall through in the same place");
    }
}

// ===========================================================================
// Step-25: the finiteness guard must be MASKED by argument contribution, and
// the two spellings of a power must agree
// ===========================================================================
//
// `eval_dual_builtin` refuses the whole row whenever ANY entry of
// `builtin_partials` is non-finite.  For `pow` the exponent partial is
// `x^y·ln x`, which does not exist for `x <= 0` — so `pow(w, 2)` at a NEGATIVE
// or ZERO base refuses, even though `dR/dw = 2w` is perfectly finite and
// `combine` already skips the constant exponent's zero tangent so the offending
// partial is multiplied by nothing at all.
//
// w = 0.0 is the commonest initial guess a solver ever starts from and w < 0 is
// reachable at any Newton step, so this is not an exotic corner: it takes out
// the whole Jacobian of any residual containing a squared unknown written as
// `pow(w, 2)`.
//
// The fix has two independent halves, and both are pinned here:
//
//   (1) mask the guard by which arguments actually CARRY a tangent, so the
//       guard can never refuse over a partial `combine` provably discards;
//   (2) mirror `pow_tangent`'s constant-exponent identity — `d/dx (x^0) = 0`
//       exactly — into `builtin_partials`, which masking alone cannot reach
//       because that partial belongs to the argument that DOES move.
//
// The masking is deliberately NOT a blanket amnesty: a non-finite partial on a
// MOVING argument must still refuse, and the last block asserts exactly that.

/// The `pow(x, k)` FunctionCall spelling and the `x ^ k` BinOp spelling of the
/// same power, as an `(ad_tangent, primal)` pair each, seeded on `x` alone.
fn both_pow_spellings(x0: f64, k: i64) -> ((f64, f64), (f64, f64)) {
    let (values, seed_cells) = probe(&[("x", x0)]);
    let exponent = literal(Value::Int(k));
    let call = calln("pow", vec![pref("x"), exponent.clone()]);
    let binop_form = binop(BinOp::Pow, pref("x"), exponent);

    let unwrap_one = |label: &str, e: &CompiledExpr| -> (f64, f64) {
        let (primal, row) = jrow(e, &values, &seed_cells)
            .unwrap_or_else(|err| panic!("{label} at x={x0}, k={k}: refused with {err:?}"));
        assert_eq!(row.len(), 1, "{label}: one seed, one column");
        (primal, row[0])
    };
    (unwrap_one("pow(x, k)", &call), unwrap_one("x ^ k", &binop_form))
}

// ---------------------------------------------------------------------------
// (1) A constant exponent must not be refused over a base it never reads
// ---------------------------------------------------------------------------

#[test]
fn pow_with_a_constant_exponent_differentiates_at_a_negative_base() {
    // `powf` returns 9.0 for an integral exponent at a negative base (IEEE
    // 754-2008 §9.2.1), so the PRIMAL is an ordinary 9.0 — nothing about this
    // point is undefined.  Only the unread `x^y·ln x` column is NaN.
    let ((primal, ad), _) = both_pow_spellings(-3.0, 2);
    assert_eq!(primal, 9.0, "pow(-3, 2) is an ordinary finite primal");
    assert_close(ad, -6.0, "d(x²)/dx at x = −3");
}

#[test]
fn pow_with_a_constant_exponent_differentiates_at_a_zero_base() {
    // x = 0 is the commonest initial guess there is; `0^2·ln 0` = `0·−inf` = NaN.
    let ((primal, ad), _) = both_pow_spellings(0.0, 2);
    assert_eq!(primal, 0.0, "pow(0, 2) is an ordinary finite primal");
    assert_eq!(ad, 0.0, "d(x²)/dx at x = 0 is exactly zero");
}

#[test]
fn pow_with_an_odd_constant_exponent_at_a_negative_base_agrees_with_central_differences() {
    // ∂(x³)/∂x = 3x² = 12 at x = −2, comfortably clear of the 0.1 floor.
    let (values, seed_cells) = probe(&[("x", -2.0)]);
    let expr = calln("pow", vec![pref("x"), literal(Value::Int(3))]);
    assert_ad_matches_cd("pow(x, 3) at x = -2", &expr, &values, &seed_cells);
}

// ---------------------------------------------------------------------------
// (2) The two spellings of one power are the SAME function
// ---------------------------------------------------------------------------

#[test]
fn the_call_and_binop_spellings_of_a_constant_power_produce_bit_identical_tangents() {
    // `pow_tangent` (the BinOp path) already carries the constant-exponent
    // identity; the FunctionCall path never got it.  Nothing about a residual's
    // gradient may depend on which of two synonyms the author typed, so this is
    // asserted on the BITS, not within a tolerance.
    for k in [2_i64, 3] {
        for x0 in [-3.0_f64, 0.0, 2.0] {
            let ((call_primal, call_ad), (binop_primal, binop_ad)) = both_pow_spellings(x0, k);
            assert_eq!(
                call_primal.to_bits(),
                binop_primal.to_bits(),
                "pow(x, {k}) vs x ^ {k} at x = {x0}: primals must agree bit-for-bit"
            );
            assert_eq!(
                call_ad.to_bits(),
                binop_ad.to_bits(),
                "pow(x, {k}) vs x ^ {k} at x = {x0}: tangents must agree bit-for-bit \
                 (got {call_ad:?} vs {binop_ad:?})"
            );
        }
    }
}

#[test]
fn a_zero_exponent_collapses_to_an_exactly_zero_tangent_in_both_spellings() {
    // `d/dx (x^0) = d/dx (1) = 0` exactly, for EVERY x including 0 — whereas
    // the general base partial `y·x^(y−1)` evaluates `0 · 0^(−1)` = `0 · inf`
    // = NaN.  Masking cannot rescue this one: the NaN sits on the argument that
    // genuinely moves, so `builtin_partials` has to state the identity itself.
    for x0 in [-3.0_f64, 0.0, 2.0] {
        let ((call_primal, call_ad), (binop_primal, binop_ad)) = both_pow_spellings(x0, 0);
        assert_eq!(call_primal, 1.0, "x⁰ = 1 at x = {x0}");
        assert_eq!(binop_primal, 1.0, "x⁰ = 1 at x = {x0}");
        assert_eq!(call_ad, 0.0, "d(x⁰)/dx is exactly zero at x = {x0}");
        assert_eq!(
            call_ad.to_bits(),
            binop_ad.to_bits(),
            "pow(x, 0) vs x ^ 0 at x = {x0}: bit-identical zero tangents"
        );
    }
}

// ---------------------------------------------------------------------------
// (3) The mask is not a blanket amnesty
// ---------------------------------------------------------------------------

#[test]
fn a_non_finite_partial_on_a_moving_argument_still_refuses() {
    // At arity 1 the mask is a PROVABLE no-op: the all-zero-tangent case has
    // already returned `DualValue::constant` before the guard runs, so the sole
    // argument always contributes.  These three keep refusing byte for byte.
    let unary_refusals: [(&str, f64); 2] = [("sqrt", 0.0), ("asin", 1.0)];
    for (name, x0) in unary_refusals {
        let (values, seed_cells) = probe(&[("x", x0)]);
        let expr = call1(name, pref("x"));
        match jrow(&expr, &values, &seed_cells) {
            Err(NonDifferentiable::UnsupportedKind { kind, .. }) => assert_eq!(
                kind, "a builtin evaluated where its derivative does not exist",
                "{name}({x0}) must keep naming the derivative, not the primal"
            ),
            other => panic!("{name}({x0}): expected UnsupportedKind, got {other:?}"),
        }
    }

    // A MULTI-argument case where the mask is genuinely live and must not fire:
    // both base and exponent are seeded, so `x^y·ln x` belongs to an argument
    // that really does move, and `ln(−3)` really does not exist.
    let (values, seed_cells) = probe(&[("x", -3.0), ("y", 2.0)]);
    let expr = calln("pow", vec![pref("x"), pref("y")]);
    let ctx = EvalContext::simple(&values);
    assert_eq!(
        eval_expr(&expr, &ctx),
        Value::Real(9.0),
        "the primal is finite — that is exactly what makes a silent zero row dangerous here"
    );
    match jrow(&expr, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { .. }) => {}
        other => {
            panic!("pow(x, y) with both seeded at x = −3: expected UnsupportedKind, got {other:?}")
        }
    }
}

#[test]
fn a_builtin_whose_primal_is_undef_refuses_at_the_primal_cliff_regardless_of_the_mask() {
    // The other half of "not an amnesty", and a deliberate correction to the
    // naive reading that every non-finite-derivative point surfaces as
    // `UnsupportedKind`: `log(0)` = −inf and a degenerate `remap` range both
    // SANITIZE to `Value::Undef` (`reify-expr/src/sanitize.rs:24`), so the
    // Undef cliff fires strictly BEFORE the partials are ever computed.  The
    // masked guard is therefore unreachable for these, which is the point —
    // whichever refusal arrives first, a refusal is what must arrive.
    let (values, seed_cells) = probe(&[("x", 0.0)]);
    let log0 = call1("log", pref("x"));
    match jrow(&log0, &values, &seed_cells) {
        Err(NonDifferentiable::UndefPrimal { .. }) => {}
        other => panic!("log(0): expected UndefPrimal, got {other:?}"),
    }

    // remap with flo == fhi: every partial, ∂/∂x included, is ±inf or NaN.
    let (values, seed_cells) = probe(&[("x", 2.0)]);
    let degenerate = calln("remap", vec![
        pref("x"),
        literal(Value::Real(1.0)),
        literal(Value::Real(1.0)),
        literal(Value::Real(0.0)),
        literal(Value::Real(10.0)),
    ]);
    match jrow(&degenerate, &values, &seed_cells) {
        Err(NonDifferentiable::UndefPrimal { .. }) => {}
        other => {
            panic!("remap with a degenerate source range: expected UndefPrimal, got {other:?}")
        }
    }
}

// ===========================================================================
// Step-29: the `abs` KINK guard must be masked by argument contribution too
// ===========================================================================
//
// Step-25 masked the SMOOTH-builtin finiteness guard by which arguments
// actually carry a tangent.  `abs` is the one kink arm that was left refusing
// unconditionally: it fires whenever `xs[0] == 0.0`, with no check that the
// argument contributes anything, so it can veto a derivative that provably
// exists — the exact failure `contributes`' own doc forbids ("a partial the
// chain rule never reads must not be able to veto a derivative that exists").
//
// It is reachable for a SEED-INDEPENDENT `abs`.  The fast path only swallows a
// subtree that hides no kink, and `abs` is a kink builtin, so a constant
// `abs(...)` is always descended into; the kink dispatch then runs BEFORE the
// all-zero-tangent shortcut.  A residual containing `abs(offset)` where
// `offset` is a base-map constant that happens to be exactly 0.0 — a
// symmetric-tolerance default, a zeroed eccentricity, both routine in
// engineering models — therefore returns `Tangent::None`, the parent
// propagates opaque, and `jacobian_row` fails the WHOLE row: η gets a tier-2
// refusal for a residual that is differentiable in every seeded variable.
//
// As in step-25, the mask is NOT a blanket amnesty: a SEEDED `abs` sitting
// exactly on its kink must still refuse, and block (3) asserts exactly that.

// ---------------------------------------------------------------------------
// (1) A constant `abs` at the origin must not veto the row it never touches
// ---------------------------------------------------------------------------

#[test]
fn a_constant_abs_at_the_origin_still_yields_a_full_row() {
    // `abs(0) + w`, seeded on `w` alone.  ∂R/∂w = 1 exactly; the `abs` node
    // contributes nothing to that column and must not be able to withhold it.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let expr = binop(BinOp::Add, call1("abs", literal(Value::Real(0.0))), pref("w"));
    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(0) + w: refused with {err:?}"));
    assert_eq!(primal, 0.0, "the primal is an ordinary 0.0 — nothing here is undefined");
    assert_eq!(row, vec![1.0], "∂(abs(0) + w)/∂w = 1");
}

#[test]
fn a_constant_abs_at_the_origin_nested_a_level_down_still_yields_a_full_row() {
    // `w · (abs(0) + 1)` — the same node in a different syntactic position, so
    // the fix is a property of the NODE and not of one shape.  ∂R/∂w = 1.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let inner =
        binop(BinOp::Add, call1("abs", literal(Value::Real(0.0))), literal(Value::Real(1.0)));
    let expr = binop(BinOp::Mul, pref("w"), inner);
    let (primal, row) = jrow(&expr, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("w * (abs(0) + 1): refused with {err:?}"));
    assert_eq!(primal, 0.0, "0 · 1");
    assert_eq!(row, vec![1.0], "∂(w·(abs(0) + 1))/∂w = abs(0) + 1 = 1");
}

#[test]
fn a_constant_abs_has_no_cliff_at_zero() {
    // What "a constant contributes nothing" MEANS: the row does not depend on
    // where the constant argument sits relative to the kink.  A constant 0 and
    // a constant 2 must produce the SAME row — anything else is a cliff in the
    // Jacobian at a point no seeded variable can even move through.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let at_kink = binop(BinOp::Add, call1("abs", literal(Value::Real(0.0))), pref("w"));
    let away = binop(BinOp::Add, call1("abs", literal(Value::Real(2.0))), pref("w"));

    let (_, row_at_kink) = jrow(&at_kink, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(0) + w: refused with {err:?}"));
    let (_, row_away) = jrow(&away, &values, &seed_cells)
        .unwrap_or_else(|err| panic!("abs(2) + w: refused with {err:?}"));
    assert_eq!(
        row_at_kink, row_away,
        "a constant argument contributes nothing, so the kink's position cannot move the row"
    );
}

// ---------------------------------------------------------------------------
// (2) The mask is not a blanket amnesty
// ---------------------------------------------------------------------------

#[test]
fn a_seeded_abs_sitting_exactly_on_its_kink_still_refuses() {
    // The argument genuinely moves, so there genuinely is no two-sided
    // derivative.  A one-sided derivative dressed up as two-sided is exactly
    // what the refusal prevents, and masking must not reach this case.
    const ABS_KINK: &str = "`abs` evaluated exactly at its kink (x = 0)";

    let (values, seed_cells) = probe(&[("x", 0.0)]);
    let direct = call1("abs", pref("x"));
    match jrow(&direct, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_eq!(kind, ABS_KINK, "abs(x) at x = 0 must keep naming the abs kink");
        }
        other => panic!("abs(x) at x = 0: expected UnsupportedKind, got {other:?}"),
    }

    // And through a subtraction: seeded `x` sitting exactly ON the constant it
    // is measured against is the commonest way a solver arrives at the kink.
    let (values, seed_cells) = probe(&[("x", 3.0)]);
    let shifted = call1("abs", binop(BinOp::Sub, pref("x"), literal(Value::Real(3.0))));
    match jrow(&shifted, &values, &seed_cells) {
        Err(NonDifferentiable::UnsupportedKind { kind, .. }) => {
            assert_eq!(kind, ABS_KINK, "abs(x − 3) at x = 3 must keep naming the abs kink");
        }
        other => panic!("abs(x - 3) at x = 3: expected UnsupportedKind, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (3) `abs` is the lone outlier — pin that the other kinks stay that way
// ---------------------------------------------------------------------------

#[test]
fn every_other_all_constant_kink_already_yields_a_full_row() {
    // `min`/`max`/`clamp` adopt the chosen constant operand's `Tangent::Zero`
    // through `select`, `floor` returns a `DualValue::constant` outright, and
    // `mod` goes through `combine` — none of them can withhold a row.  They do
    // this today; this pins that they keep doing it, so the step-30 mask cannot
    // be read as "abs was special" and quietly regress elsewhere.
    let (values, seed_cells) = probe(&[("w", 0.0)]);
    let c = |v: f64| literal(Value::Real(v));
    // `mod` is the one Int-only stdlib binding (numeric.rs:89), so it gets Int
    // constants; 7 mod 3 = 1, which is also why the expected primal is per-case.
    let cases: [(&str, CompiledExpr, f64); 5] = [
        ("min(0, 0)", calln("min", vec![c(0.0), c(0.0)]), 0.0),
        ("max(0, 0)", calln("max", vec![c(0.0), c(0.0)]), 0.0),
        ("clamp(0, 0, 1)", calln("clamp", vec![c(0.0), c(0.0), c(1.0)]), 0.0),
        ("floor(0)", call1("floor", c(0.0)), 0.0),
        ("mod(7, 3)", calln("mod", vec![literal(Value::Int(7)), literal(Value::Int(3))]), 1.0),
    ];
    for (label, kink, expected_primal) in cases {
        let expr = binop(BinOp::Add, kink, pref("w"));
        let (primal, row) = jrow(&expr, &values, &seed_cells)
            .unwrap_or_else(|err| panic!("{label} + w: refused with {err:?}"));
        assert_eq!(primal, expected_primal, "{label} + w at w = 0");
        assert_eq!(row, vec![1.0], "{label} + w: ∂/∂w = 1");
    }
}
