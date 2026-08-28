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

    let mut at = |v: f64| -> f64 {
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
