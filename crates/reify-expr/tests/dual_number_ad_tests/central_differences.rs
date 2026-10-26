//! Agreement with central differences — THE HEADLINE SIGNAL.
//!
//! Every assertion here compares the AD tangent against a central-difference
//! reference computed entirely through `eval_expr` on a perturbed `ValueMap`.
//! The reference therefore shares NO code with the thing under test: if the
//! dual traversal and the value evaluator ever disagree about what a builtin
//! does, this suite says so.
//!
//! The step shape, the tolerance `|ad − cd| <= 1e-6·|cd| + 1e-8`, its derivation and
//! the 0.1 probe-slope floor live in `reify_test_support::finite_difference`.

use reify_ir::{BinOp, Value};
use reify_test_support::builders::expr::{binop, literal, neg};

use crate::support::{assert_ad_matches_cd, calln, pref, probe};

/// One-variable convenience: seed a single cell `x` at `x0` and compare.
fn assert_unary_builtin(name: &str, x0: f64) {
    let (values, seed_cells) = probe(&[("x", x0)]);
    let expr = calln(name, vec![pref("x")]);
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
    assert_unary_builtin("tan", 0.7); // d/dx = sec²(0.7) ≈ 1.7094
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
    assert_unary_builtin("tanh", 0.7); // d/dx = 1 − tanh²(0.7) ≈ 0.63474
}

#[test]
fn abs_tangent_away_from_the_origin_agrees_with_central_differences() {
    // `abs` is a kink AT zero; away from zero it is smooth with derivative
    // sign(x).  Both sides are probed, because a sign error is invisible if you
    // only ever test the positive branch.
    assert_unary_builtin("abs", 2.0);
    let (values, seed_cells) = probe(&[("x", -1.3)]);
    assert_ad_matches_cd("abs_negative", &calln("abs", vec![pref("x")]), &values, &seed_cells);
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
    let expr = binop(BinOp::Sub, calln("sqrt", vec![sum]), literal(Value::Real(5.0)));
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
        binop(BinOp::Mul, calln("sin", vec![pref("a")]), calln("cos", vec![pref("b")])),
        calln("exp", vec![neg(pref("c"))]),
    );
    assert_ad_matches_cd("trig_composite", &expr, &values, &seed_cells);
}

#[test]
fn nested_sqrt_and_tanh_composite_gradient_agrees_with_central_differences() {
    // f(x, y) = sqrt(x·x + y·y) · tanh(x − y) — nests a builtin inside a
    // builtin inside arithmetic, so a chain-rule slip anywhere shows up.
    let (values, seed_cells) = probe(&[("x", 2.0), ("y", 1.2)]);
    let norm = calln("sqrt", vec![binop(
        BinOp::Add,
        binop(BinOp::Mul, pref("x"), pref("x")),
        binop(BinOp::Mul, pref("y"), pref("y")),
    )]);
    let expr =
        binop(BinOp::Mul, norm, calln("tanh", vec![binop(BinOp::Sub, pref("x"), pref("y"))]));
    assert_ad_matches_cd("sqrt_tanh_composite", &expr, &values, &seed_cells);
}
