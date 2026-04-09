use reify_types::Value;

mod common;

mod complex;
mod frames;
mod geometry;
mod linalg;
mod numeric;
mod orientation;
mod stubs;
mod trig;
mod vector;

#[cfg(test)]
mod test_helpers;

// Re-export helpers needed by tests in this module until test migration steps
// (steps 19-27) move each test block to its submodule.
#[cfg(test)]
use crate::common::*;
#[cfg(test)]
pub(crate) use linalg::matrix_components_f64;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    if let Some(v) = numeric::dispatch(name, args) {
        return v;
    }
    if let Some(v) = trig::dispatch(name, args) {
        return v;
    }
    if let Some(v) = vector::dispatch(name, args) {
        return v;
    }
    if let Some(v) = complex::dispatch(name, args) {
        return v;
    }
    if let Some(v) = orientation::dispatch(name, args) {
        return v;
    }
    if let Some(v) = frames::dispatch(name, args) {
        return v;
    }
    if let Some(v) = geometry::dispatch(name, args) {
        return v;
    }
    if let Some(v) = linalg::dispatch(name, args) {
        return v;
    }
    if let Some(v) = stubs::dispatch(name, args) {
        return v;
    }
    Value::Undef
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;
    use crate::test_helpers::*;
    use crate::vector::construct_point_or_vector;

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "determined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "undetermined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "constrained stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "partially_determined stub should return Undef, got {:?}",
            result
        );
    }

    // --- Field operation stubs (step-25) ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        // gradient(field) on a scalar field should return Undef (stub).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(
            result.is_undef(),
            "gradient stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(
            result.is_undef(),
            "divergence stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(
            result.is_undef(),
            "curl stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
    }

    // ── Advanced linalg tests (task 337) ─────────────────────────────────────

    // --- determinant tests ---

    #[test]
    fn det_identity_2x2() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_2_times_identity_3x3() {
        // det(2*I₃) = 2³ = 8
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 2.0, 0.0], &[0.0, 0.0, 2.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 8.0);
    }

    #[test]
    fn det_singular_matrix() {
        // Singular: rows are linearly dependent
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 0.0);
    }

    #[test]
    fn det_dimensioned_3x3() {
        // det(Force_mat) has dimension Force³ for 3×3
        let force_dim = reify_types::dimension::FORCE;
        let m = make_dimensioned_matrix(
            &[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]],
            force_dim,
        );
        let result = eval_builtin("determinant", &[m]);
        let expected_dim = force_dim.pow(3);
        assert_scalar_approx!(result, 1.0, expected_dim);
    }

    #[test]
    fn det_1x1() {
        let m = make_matrix(&[&[42.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 42.0);
    }

    #[test]
    fn det_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("determinant", &[m]).is_undef());
    }

    // --- inverse tests ---

    #[test]
    fn inverse_2x2_identity() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&m));
        // inv(I) = I — check all four elements
        if let Value::Tensor(rows) = &inv {
            assert_eq!(rows.len(), 2);
            for (i, row) in rows.iter().enumerate() {
                if let Value::Tensor(elems) = row {
                    assert_eq!(elems.len(), 2);
                    for (j, elem) in elems.iter().enumerate() {
                        let expected = if i == j { 1.0 } else { 0.0 };
                        let val = elem.as_f64().unwrap();
                        assert!(
                            (val - expected).abs() < 1e-12,
                            "inv[{i}][{j}]: expected {expected}, got {val}"
                        );
                    }
                } else {
                    panic!("expected Tensor row");
                }
            }
        } else {
            panic!("expected Tensor, got {:?}", inv);
        }
    }

    #[test]
    fn inverse_times_original_approx_identity() {
        // A = [[1,2],[3,4]], verify inv(A)*A ≈ I via manual multiply
        let a = make_matrix(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        // Extract inv as flat
        let inv_data = matrix_components_f64(&inv).unwrap();
        let a_data = matrix_components_f64(&a).unwrap();
        // Manual 2×2 multiply: product = inv * a
        let (ai, ad) = (inv_data.2, a_data.2);
        let p00 = ai[0] * ad[0] + ai[1] * ad[2];
        let p01 = ai[0] * ad[1] + ai[1] * ad[3];
        let p10 = ai[2] * ad[0] + ai[3] * ad[2];
        let p11 = ai[2] * ad[1] + ai[3] * ad[3];
        assert!((p00 - 1.0).abs() < 1e-10, "p00={p00}");
        assert!((p01).abs() < 1e-10, "p01={p01}");
        assert!((p10).abs() < 1e-10, "p10={p10}");
        assert!((p11 - 1.0).abs() < 1e-10, "p11={p11}");
    }

    #[test]
    fn inverse_3x3() {
        let a = make_matrix(&[&[1.0, 2.0, 3.0], &[0.0, 1.0, 4.0], &[5.0, 6.0, 0.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        let inv_d = matrix_components_f64(&inv).unwrap();
        let a_d = matrix_components_f64(&a).unwrap();
        // 3×3 multiply to verify ≈ identity
        let (ai, ad) = (inv_d.2, a_d.2);
        for r in 0..3 {
            for c in 0..3 {
                let sum: f64 = (0..3).map(|k| ai[r * 3 + k] * ad[k * 3 + c]).sum();
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (sum - expected).abs() < 1e-10,
                    "product[{r}][{c}] = {sum}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn inverse_singular_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert!(
            eval_builtin("inverse", &[m]).is_undef(),
            "inverse of singular matrix should be Undef"
        );
    }

    // --- transpose tests ---

    #[test]
    fn transpose_symmetric_unchanged() {
        // Symmetric matrix: transpose should equal original
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[2.0, 5.0, 6.0], &[3.0, 6.0, 9.0]]);
        let t = eval_builtin("transpose", std::slice::from_ref(&m));
        let orig_d = matrix_components_f64(&m).unwrap();
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(orig_d.0, t_d.0);
        assert_eq!(orig_d.1, t_d.1);
        for (a, b) in orig_d.2.iter().zip(t_d.2.iter()) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn transpose_2x3() {
        // [[1,2,3],[4,5,6]] → [[1,4],[2,5],[3,6]]
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let t = eval_builtin("transpose", &[m]);
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(t_d.0, 3); // rows
        assert_eq!(t_d.1, 2); // cols
        assert!((t_d.2[0] - 1.0).abs() < 1e-12);
        assert!((t_d.2[1] - 4.0).abs() < 1e-12);
        assert!((t_d.2[2] - 2.0).abs() < 1e-12);
        assert!((t_d.2[3] - 5.0).abs() < 1e-12);
        assert!((t_d.2[4] - 3.0).abs() < 1e-12);
        assert!((t_d.2[5] - 6.0).abs() < 1e-12);
    }

    // --- trace tests ---

    #[test]
    fn trace_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 3.0);
    }

    #[test]
    fn trace_general_2x2() {
        let m = make_matrix(&[&[5.0, 3.0], &[7.0, 2.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 7.0);
    }

    #[test]
    fn trace_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("trace", &[m]).is_undef());
    }

    // --- outer product tests ---

    #[test]
    fn outer_two_vectors() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]);
        let b = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(5.0)]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.0, 2);
        assert_eq!(d.1, 3);
        // [[3,4,5],[6,8,10]]
        let expected = [3.0, 4.0, 5.0, 6.0, 8.0, 10.0];
        for (got, exp) in d.2.iter().zip(expected.iter()) {
            assert!((got - exp).abs() < 1e-12);
        }
    }

    #[test]
    fn outer_dimensioned_vectors() {
        let length_dim = DimensionVector::LENGTH;
        let force_dim = reify_types::dimension::FORCE;
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: length_dim,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: length_dim,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: force_dim,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: force_dim,
            },
        ]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.3, length_dim.mul(&force_dim));
    }

    // --- eigenvalues tests ---

    #[test]
    fn eigenvalues_diagonal_2x2() {
        let m = make_matrix(&[&[3.0, 0.0], &[0.0, 7.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 2);
            // Sorted: [3, 7]
            assert!((items[0].as_f64().unwrap() - 3.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 7.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_diagonal_3x3() {
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 5.0, 0.0], &[0.0, 0.0, 8.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Sorted: [2, 5, 8]
            assert!((items[0].as_f64().unwrap() - 2.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 5.0).abs() < 1e-10);
            assert!((items[2].as_f64().unwrap() - 8.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_symmetric_3x3() {
        // Symmetric matrix always has real eigenvalues
        let m = make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Eigenvalues of this matrix: 1, 2, 4
            let eigs: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap()).collect();
            assert!((eigs[0] - 1.0).abs() < 1e-10, "eig0={}", eigs[0]);
            assert!((eigs[1] - 2.0).abs() < 1e-10, "eig1={}", eigs[1]);
            assert!((eigs[2] - 4.0).abs() < 1e-10, "eig2={}", eigs[2]);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_1x1() {
        let m = make_matrix(&[&[42.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
            assert!((items[0].as_f64().unwrap() - 42.0).abs() < 1e-12);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            for item in &items {
                assert!((item.as_f64().unwrap() - 1.0).abs() < 1e-10);
            }
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn inverse_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("inverse", &[m]).is_undef());
    }

    #[test]
    fn determinant_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("determinant", &[]).is_undef());
    }

    #[test]
    fn inverse_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("inverse", &[]).is_undef());
    }

    #[test]
    fn transpose_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("transpose", &[]).is_undef());
    }

    #[test]
    fn trace_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("trace", &[]).is_undef());
    }

    #[test]
    fn eigenvalues_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("eigenvalues", &[]).is_undef());
    }

    #[test]
    fn outer_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("outer", &[]).is_undef());
    }

    #[test]
    fn determinant_non_matrix_returns_undef() {
        assert!(eval_builtin("determinant", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn inverse_dimensioned_2x2() {
        // inverse of dimensioned matrix has inverse dimension
        let length_dim = DimensionVector::LENGTH;
        let m = make_dimensioned_matrix(&[&[1.0, 0.0], &[0.0, 2.0]], length_dim);
        let inv = eval_builtin("inverse", &[m]);
        let d = matrix_components_f64(&inv).unwrap();
        let expected_dim = DimensionVector::DIMENSIONLESS.div(&length_dim);
        assert_eq!(d.3, expected_dim);
        // Check values: inv of diag(1,2) = diag(1, 0.5)
        assert!((d.2[0] - 1.0).abs() < 1e-12);
        assert!((d.2[1]).abs() < 1e-12);
        assert!((d.2[2]).abs() < 1e-12);
        assert!((d.2[3] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn matrix_value_form_works() {
        // Test that Value::Matrix is also accepted
        let m = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }
}
