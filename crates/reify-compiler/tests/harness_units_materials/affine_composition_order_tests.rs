//! End-to-end compile+eval pin for the AffineMap composition-order convention
//! (task 3962, PRD §4.3 task δ).
//!
//! Composition order is a VALUE fact: both `a∘b` and `b∘a` type as `AffineMap(3)`,
//! so the existing typing harness (affine_algebra_typing_tests.rs) cannot distinguish
//! them. These tests drive the full reify-compiler → reify-eval pipeline and inspect
//! the evaluated `Value::AffineMap`, pinning the left-applied a∘b convention exactly
//! as integration example η will observe it.

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::eval_source;

/// Assert two 3×3 f64 matrices are element-wise equal within `tol`.
fn matrix_approx_eq(actual: [[f64; 3]; 3], expected: [[f64; 3]; 3], tol: f64, label: &str) {
    for (r, (arow, erow)) in actual.iter().zip(expected.iter()).enumerate() {
        for (c, (a, e)) in arow.iter().zip(erow.iter()).enumerate() {
            assert!(
                (a - e).abs() < tol,
                "{label} linear[{r}][{c}]: expected {e}, got {a} (tol {tol})"
            );
        }
    }
}

/// Extract the `linear` matrix from a `Value::AffineMap`, or panic.
fn unwrap_affine_linear(v: &Value, label: &str) -> [[f64; 3]; 3] {
    match v {
        Value::AffineMap { linear, .. } => *linear,
        other => panic!("{label}: expected Value::AffineMap, got {:?}", other),
    }
}

#[test]
fn affine_compose_order_e2e_forward_ab() {
    // compile+eval: compose(scale(2,1,1), shear_xy(1))
    // Expected linear: [[2,2,0],[0,1,0],[0,0,1]]  (a∘b = a.linear · b.linear)
    let source = r#"
        structure S {
            let composed = affine_compose(affine_scale(2.0, 1.0, 1.0), affine_shear_xy(1.0))
        }
    "#;
    let result = eval_source(source);
    let id = ValueCellId::new("S", "composed");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'composed' not found in eval result"));

    let linear = unwrap_affine_linear(val, "composed");
    matrix_approx_eq(
        linear,
        [[2.0, 2.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1e-12,
        "compose(scale(2,1,1), shear_xy(1))",
    );
}

#[test]
fn affine_compose_order_e2e_reversed_ba() {
    // compile+eval: compose(shear_xy(1), scale(2,1,1))
    // Expected linear: [[2,1,0],[0,1,0],[0,0,1]]  (b∘a = b.linear · a.linear)
    let source = r#"
        structure S {
            let reversed = affine_compose(affine_shear_xy(1.0), affine_scale(2.0, 1.0, 1.0))
        }
    "#;
    let result = eval_source(source);
    let id = ValueCellId::new("S", "reversed");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'reversed' not found in eval result"));

    let linear = unwrap_affine_linear(val, "reversed");
    matrix_approx_eq(
        linear,
        [[2.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1e-12,
        "compose(shear_xy(1), scale(2,1,1))",
    );
}

#[test]
fn affine_compose_order_e2e_both_orders_differ() {
    // Eval both orderings in the same structure and assert they produce different
    // linear matrices — directly pinning that the order is load-bearing through
    // the full compile→eval pipeline that integration example η consumes.
    let source = r#"
        structure S {
            let composed = affine_compose(affine_scale(2.0, 1.0, 1.0), affine_shear_xy(1.0))
            let reversed = affine_compose(affine_shear_xy(1.0), affine_scale(2.0, 1.0, 1.0))
        }
    "#;
    let result = eval_source(source);

    let composed_id = ValueCellId::new("S", "composed");
    let reversed_id = ValueCellId::new("S", "reversed");

    let composed_val = result
        .values
        .get(&composed_id)
        .unwrap_or_else(|| panic!("'composed' not found"));
    let reversed_val = result
        .values
        .get(&reversed_id)
        .unwrap_or_else(|| panic!("'reversed' not found"));

    let composed_linear = unwrap_affine_linear(composed_val, "composed");
    let reversed_linear = unwrap_affine_linear(reversed_val, "reversed");

    // composed[0][1] = 2.0 (scale then shear: 2×shear factor)
    // reversed[0][1] = 1.0 (shear then scale: unscaled shear factor)
    assert!(
        (composed_linear[0][1] - reversed_linear[0][1]).abs() > 0.5,
        "compose(a,b) and compose(b,a) must produce different linear matrices; \
         composed[0][1]={}, reversed[0][1]={}",
        composed_linear[0][1],
        reversed_linear[0][1]
    );
}
