use nalgebra::DMatrix;
use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{binary, sanitize_value, tensor_components_f64, unary};

/// Compute the determinant of a 3×3 row-major matrix using the Sarrus /
/// cofactor expansion along the first row:
/// det = a(ei−fh) − b(di−fg) + c(dh−eg).
///
/// Shared by the `determinant` builtin's `AffineMap` arm (matrix.rs) and
/// `affine_mat3_inv` in geometry.rs — single source of truth for the formula.
pub(crate) fn mat3_det(m: [[f64; 3]; 3]) -> f64 {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
}

pub(crate) fn eval_matrix(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Advanced linear algebra: determinant, inverse, transpose, outer, trace, eigenvalues ---
        "determinant" => unary(args, |v| {
            // AffineMap: linear part is dimensionless (G6 contract) → Value::Real.
            if let Value::AffineMap { linear, .. } = v {
                return sanitize_value(Value::Real(mat3_det(*linear)));
            }
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef; // must be square
            }
            let det = match n {
                1 => data[0],
                2 => data[0] * data[3] - data[1] * data[2],
                3 => {
                    // Sarrus / cofactor expansion along first row
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);
                    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
                }
                _ => DMatrix::from_row_slice(n, n, &data).determinant(),
            };
            let result_dim = dim.pow(n as i8);
            if result_dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(det))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: det,
                    dimension: result_dim,
                })
            }
        }),

        "inverse" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef;
            }
            let inv_dim = if dim == DimensionVector::DIMENSIONLESS {
                DimensionVector::DIMENSIONLESS
            } else {
                DimensionVector::DIMENSIONLESS.div(&dim)
            };
            match n {
                1 => {
                    if data[0] == 0.0 {
                        return Value::Undef;
                    }
                    build_matrix_value(1, 1, &[1.0 / data[0]], inv_dim)
                }
                2 => {
                    let det = data[0] * data[3] - data[1] * data[2];
                    if det == 0.0 {
                        return Value::Undef;
                    }
                    let inv_det = 1.0 / det;
                    let inv_data = [
                        data[3] * inv_det,
                        -data[1] * inv_det,
                        -data[2] * inv_det,
                        data[0] * inv_det,
                    ];
                    build_matrix_value(2, 2, &inv_data, inv_dim)
                }
                3 => {
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);
                    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
                    if det == 0.0 {
                        return Value::Undef;
                    }
                    let inv_det = 1.0 / det;
                    // Cofactor matrix transposed (adjugate), divided by det
                    let inv_data = [
                        (e * i - f * h) * inv_det,
                        (c * h - b * i) * inv_det,
                        (b * f - c * e) * inv_det,
                        (f * g - d * i) * inv_det,
                        (a * i - c * g) * inv_det,
                        (c * d - a * f) * inv_det,
                        (d * h - e * g) * inv_det,
                        (b * g - a * h) * inv_det,
                        (a * e - b * d) * inv_det,
                    ];
                    build_matrix_value(3, 3, &inv_data, inv_dim)
                }
                _ => {
                    let m = DMatrix::from_row_slice(n, n, &data);
                    match m.try_inverse() {
                        Some(inv) => {
                            let mut inv_data = Vec::with_capacity(n * n);
                            for i in 0..n {
                                for j in 0..n {
                                    inv_data.push(inv[(i, j)]);
                                }
                            }
                            build_matrix_value(n, n, &inv_data, inv_dim)
                        }
                        None => Value::Undef,
                    }
                }
            }
        }),

        "transpose" => unary(args, |v| {
            let (nrows, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let mut transposed = vec![0.0; nrows * ncols];
            for r in 0..nrows {
                for c in 0..ncols {
                    transposed[c * nrows + r] = data[r * ncols + c];
                }
            }
            build_matrix_value(ncols, nrows, &transposed, dim)
        }),

        "outer" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let nrows = a_vals.len();
            let ncols = b_vals.len();
            let result_dim = a_dim.mul(&b_dim);
            let mut data = Vec::with_capacity(nrows * ncols);
            for ai in &a_vals {
                for bj in &b_vals {
                    data.push(ai * bj);
                }
            }
            build_matrix_value(nrows, ncols, &data, result_dim)
        }),

        "trace" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef; // must be square
            }
            let tr: f64 = (0..n).map(|i| data[i * n + i]).sum();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(tr))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: tr,
                    dimension: dim,
                })
            }
        }),

        "eigenvalues" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef;
            }
            let make_val = |x: f64| -> Value {
                if dim == DimensionVector::DIMENSIONLESS {
                    sanitize_value(Value::Real(x))
                } else {
                    sanitize_value(Value::Scalar {
                        si_value: x,
                        dimension: dim,
                    })
                }
            };
            match n {
                1 => Value::List(vec![make_val(data[0])]),
                2 => {
                    // char poly: λ² - (a+d)λ + (ad-bc) = 0
                    let (a, b) = (data[0], data[1]);
                    let (c, d) = (data[2], data[3]);
                    let tr = a + d;
                    let det = a * d - b * c;
                    let disc = tr * tr - 4.0 * det;
                    if disc < 0.0 {
                        return Value::Undef; // complex eigenvalues
                    }
                    let sqrt_disc = disc.sqrt();
                    let mut eigs = vec![(tr + sqrt_disc) / 2.0, (tr - sqrt_disc) / 2.0];
                    eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Value::List(eigs.into_iter().map(make_val).collect())
                }
                3 => {
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);

                    let p = a + e + i; // trace
                    let q = (a * e - b * d) + (a * i - c * g) + (e * i - f * h);
                    let r = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);

                    let p3 = p / 3.0;
                    let alpha = q - p * p / 3.0;
                    let beta = -2.0 * p * p * p / 27.0 + p * q / 3.0 - r;

                    if alpha >= 0.0 {
                        if alpha == 0.0 && beta == 0.0 {
                            let root = p3;
                            Value::List(vec![make_val(root), make_val(root), make_val(root)])
                        } else if alpha == 0.0 {
                            let t = (-beta).cbrt();
                            let _ = t;
                            Value::Undef
                        } else {
                            Value::Undef
                        }
                    } else {
                        let neg_alpha = -alpha;
                        let m = (neg_alpha / 3.0).sqrt();
                        let cos_arg = -beta / (2.0 * m * m * m);
                        let cos_arg = cos_arg.clamp(-1.0, 1.0);
                        let theta = cos_arg.acos();
                        let two_m = 2.0 * m;

                        let mut eigs = vec![
                            two_m * (theta / 3.0).cos() + p3,
                            two_m * ((theta + 2.0 * std::f64::consts::PI) / 3.0).cos() + p3,
                            two_m * ((theta + 4.0 * std::f64::consts::PI) / 3.0).cos() + p3,
                        ];
                        eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        Value::List(eigs.into_iter().map(make_val).collect())
                    }
                }
                _ => Value::Undef,
            }
        }),

        _ => return None,
    })
}

/// Extract a square or rectangular matrix from a `Value` into `(nrows, ncols, flat_data, element_dim)`.
///
/// Handles both `Value::Matrix(rows)` and nested `Value::Tensor` (rank-2 Tensor).
/// All elements must share the same dimension and be numeric.
pub(crate) fn matrix_components_f64(
    v: &Value,
) -> Option<(usize, usize, Vec<f64>, DimensionVector)> {
    enum Rows<'a> {
        Matrix(&'a [Vec<Value>]),
        Tensor(&'a [Value]),
    }
    let rows = match v {
        Value::Matrix(r) if !r.is_empty() => Rows::Matrix(r),
        Value::Tensor(items)
            if !items.is_empty() && items.iter().all(|r| matches!(r, Value::Tensor(_))) =>
        {
            Rows::Tensor(items)
        }
        _ => return None,
    };
    let (nrows, ncols) = match &rows {
        Rows::Matrix(r) => {
            let nc = r[0].len();
            if nc == 0 || r.iter().any(|row| row.len() != nc) {
                return None;
            }
            (r.len(), nc)
        }
        Rows::Tensor(items) => {
            let nc = match &items[0] {
                Value::Tensor(elems) => elems.len(),
                _ => return None,
            };
            if nc == 0
                || items.iter().any(|r| match r {
                    Value::Tensor(elems) => elems.len() != nc,
                    _ => true,
                })
            {
                return None;
            }
            (items.len(), nc)
        }
    };
    // Flatten and extract f64 values, checking uniform dimension.
    let first_elem = match &rows {
        Rows::Matrix(r) => &r[0][0],
        Rows::Tensor(items) => match &items[0] {
            Value::Tensor(elems) => &elems[0],
            _ => return None,
        },
    };
    let first_dim = first_elem.dimension();
    let mut data = Vec::with_capacity(nrows * ncols);
    let check_and_push = |elem: &Value, data: &mut Vec<f64>| -> bool {
        if elem.dimension() != first_dim {
            return false;
        }
        match elem.as_f64() {
            Some(x) => {
                data.push(x);
                true
            }
            None => false,
        }
    };
    match &rows {
        Rows::Matrix(r) => {
            for row in *r {
                for elem in row {
                    if !check_and_push(elem, &mut data) {
                        return None;
                    }
                }
            }
        }
        Rows::Tensor(items) => {
            for item in *items {
                if let Value::Tensor(elems) = item {
                    for elem in elems {
                        if !check_and_push(elem, &mut data) {
                            return None;
                        }
                    }
                }
            }
        }
    }
    Some((nrows, ncols, data, first_dim))
}

/// Build a nested `Value::Tensor` (rank-2) from flat f64 data.
fn build_matrix_value(nrows: usize, ncols: usize, data: &[f64], dim: DimensionVector) -> Value {
    let rows: Vec<Value> = (0..nrows)
        .map(|i| {
            let row: Vec<Value> = (0..ncols)
                .map(|j| {
                    let v = data[i * ncols + j];
                    if dim == DimensionVector::DIMENSIONLESS {
                        sanitize_value(Value::Real(v))
                    } else {
                        sanitize_value(Value::Scalar {
                            si_value: v,
                            dimension: dim,
                        })
                    }
                })
                .collect();
            Value::Tensor(row)
        })
        .collect();
    Value::Tensor(rows)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    use super::matrix_components_f64;

    fn make_matrix(rows: &[&[f64]]) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| Value::Tensor(row.iter().map(|&v| Value::Real(v)).collect()))
                .collect(),
        )
    }

    /// Helper: build a Tensor matrix with all elements having a given dimension.
    fn make_dimensioned_matrix(rows: &[&[f64]], dim: DimensionVector) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| {
                    Value::Tensor(
                        row.iter()
                            .map(|&v| Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

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
        let force_dim = reify_core::dimension::FORCE;
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
        let force_dim = reify_core::dimension::FORCE;
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

    // --- determinant(AffineMap) tests (step-1 RED / step-2 GREEN) ---

    /// Build a `Value::AffineMap` directly (no constructor — the stdlib
    /// constructors validate inputs; we need full control for the singular case).
    fn make_affine_map(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Value {
        Value::AffineMap {
            linear,
            translation,
        }
    }

    #[test]
    fn det_affine_diagonal_2_3_4() {
        // det(diag(2,3,4)) = 24 — exact product from Sarrus on a diagonal matrix.
        let a = make_affine_map(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]],
            [0.0, 0.0, 0.0],
        );
        assert_real_approx!(eval_builtin("determinant", &[a]), 24.0);
    }

    #[test]
    fn det_affine_shear_shape_is_1() {
        // Identity diagonal + one off-diagonal ⇒ det = 1 (volume-preserving shear).
        let a = make_affine_map(
            [[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0, 0.0, 0.0],
        );
        assert_real_approx!(eval_builtin("determinant", &[a]), 1.0);
    }

    #[test]
    fn det_affine_singular_zero_row_is_0() {
        // A zero row ⇒ det = 0.
        let a = make_affine_map(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0],
        );
        assert_real_approx!(eval_builtin("determinant", &[a]), 0.0);
    }

    #[test]
    fn det_affine_general_linear_2_3_1_0_4() {
        // linear = [[2,0,0],[0,3,0],[1,0,4]], det = 2*(3*4-0*0) - 0*... + 0*... = 24.
        let a = make_affine_map(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [1.0, 0.0, 4.0]],
            [0.1, 0.2, 0.3],
        );
        assert_real_approx!(eval_builtin("determinant", &[a]), 24.0);
    }

    #[test]
    fn det_affine_result_is_dimensionless_real_not_scalar() {
        // The linear part is dimensionless (G6 contract), so the result must be
        // Value::Real (not Value::Scalar with a dimension).
        let a = make_affine_map(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]],
            [0.0, 0.0, 0.0],
        );
        let result = eval_builtin("determinant", &[a]);
        assert!(
            matches!(result, Value::Real(_)),
            "determinant(AffineMap) must return Value::Real (dimensionless), got {:?}",
            result
        );
    }

    // --- N≥4 determinant tests (step-1 RED / step-2 GREEN) ---

    /// Build the well-conditioned tridiagonal 4×4 [[2,1,0,0],[1,2,1,0],[0,1,2,1],[0,0,1,2]]
    /// via the REAL `matrix()` constructor (anti-fake-done: no synthetic Value::Tensor).
    /// Exact determinant = 5; condition number κ ≈ 9.47.
    fn make_4x4_tridiagonal_dimensionless() -> Value {
        eval_builtin(
            "matrix",
            &[Value::List(vec![
                Value::List(vec![
                    Value::Real(2.0),
                    Value::Real(1.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]),
                Value::List(vec![
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(1.0),
                    Value::Real(0.0),
                ]),
                Value::List(vec![
                    Value::Real(0.0),
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(1.0),
                ]),
                Value::List(vec![
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(1.0),
                    Value::Real(2.0),
                ]),
            ])],
        )
    }

    /// Build the tridiagonal 4×4 with LENGTH-dimensioned cells via the real `matrix()` constructor.
    fn make_4x4_tridiagonal_length() -> Value {
        let l = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::LENGTH,
        };
        eval_builtin(
            "matrix",
            &[Value::List(vec![
                Value::List(vec![l(2.0), l(1.0), l(0.0), l(0.0)]),
                Value::List(vec![l(1.0), l(2.0), l(1.0), l(0.0)]),
                Value::List(vec![l(0.0), l(1.0), l(2.0), l(1.0)]),
                Value::List(vec![l(0.0), l(0.0), l(1.0), l(2.0)]),
            ])],
        )
    }

    /// (a) Happy path: det of well-conditioned 4×4 tridiagonal = 5 exactly.
    /// G6 1e-9 floor (κ≈9.47; LU residual ~2e-15; 1e-9 clears by 6 orders).
    #[test]
    fn det_4x4_tridiagonal_is_5() {
        let m = make_4x4_tridiagonal_dimensionless();
        let result = eval_builtin("determinant", &[m]);
        match result {
            Value::Real(v) => assert!(
                (v - 5.0).abs() < 1e-9,
                "det of 4×4 tridiagonal expected 5.0, got {v}"
            ),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    /// (b) Singular 4×4 (zero last row): det ≈ 0 as Real, NOT Undef.
    /// Matches 2×2/3×3 semantics: singular det → ~0 value, not Undef.
    #[test]
    fn det_4x4_singular_returns_zero_real_not_undef() {
        let m = eval_builtin(
            "matrix",
            &[Value::List(vec![
                Value::List(vec![
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0),
                ]),
                Value::List(vec![
                    Value::Real(5.0),
                    Value::Real(6.0),
                    Value::Real(7.0),
                    Value::Real(8.0),
                ]),
                Value::List(vec![
                    Value::Real(9.0),
                    Value::Real(10.0),
                    Value::Real(11.0),
                    Value::Real(12.0),
                ]),
                Value::List(vec![
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]),
            ])],
        );
        match eval_builtin("determinant", &[m]) {
            Value::Real(v) => assert!(
                v.abs() < 1e-9,
                "det of singular 4×4 expected ≈0, got {v}"
            ),
            other => panic!("expected Real(≈0.0), got {:?}", other),
        }
    }

    /// (c) Dimensioned 4×4 tridiagonal (LENGTH cells) → det dim = LENGTH^4, si ≈ 5.0.
    #[test]
    fn det_4x4_dimensioned_length_cells() {
        let m = make_4x4_tridiagonal_length();
        let result = eval_builtin("determinant", &[m]);
        let expected_dim = DimensionVector::LENGTH.pow(4);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 5.0).abs() < 1e-9,
                    "det si_value expected 5.0, got {si_value}"
                );
                assert_eq!(dimension, expected_dim, "det dimension mismatch");
            }
            other => panic!("expected Scalar{{si≈5.0, dim=LENGTH^4}}, got {:?}", other),
        }
    }

    // --- N≥4 inverse tests (step-3 RED / step-4 GREEN) ---

    /// (a) Happy path: inv(A)·A ≈ I₄ — max residual < 1e-9.
    /// Uses the tridiagonal 4×4 (κ≈9.47; measured ‖A·A⁻¹−I‖∞ ≈ 2.2e-16).
    #[test]
    fn inverse_4x4_times_original_approx_identity() {
        let m = make_4x4_tridiagonal_dimensionless();
        let inv = eval_builtin("inverse", std::slice::from_ref(&m));

        // Must not be Undef
        assert!(
            !inv.is_undef(),
            "inverse of well-conditioned 4×4 should not be Undef, got {:?}",
            inv
        );

        // Extract flat data for both A and A⁻¹
        let (n_a, _, a_data, _) = matrix_components_f64(&m).expect("A must parse");
        let (n_inv, _, inv_data, _) = matrix_components_f64(&inv).expect("inv must parse");
        assert_eq!(n_a, 4);
        assert_eq!(n_inv, 4);

        // Compute A · A⁻¹ and assert ‖ · − I‖∞ < 1e-9
        for r in 0..4 {
            for c in 0..4 {
                let product: f64 = (0..4).map(|k| a_data[r * 4 + k] * inv_data[k * 4 + c]).sum();
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (product - expected).abs() < 1e-9,
                    "A·A⁻¹[{r}][{c}] = {product}, expected {expected}"
                );
            }
        }
    }

    /// (b) Singular 4×4 (zero last row) → inverse returns Undef.
    #[test]
    fn inverse_4x4_singular_returns_undef() {
        let m = eval_builtin(
            "matrix",
            &[Value::List(vec![
                Value::List(vec![
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0),
                ]),
                Value::List(vec![
                    Value::Real(5.0),
                    Value::Real(6.0),
                    Value::Real(7.0),
                    Value::Real(8.0),
                ]),
                Value::List(vec![
                    Value::Real(9.0),
                    Value::Real(10.0),
                    Value::Real(11.0),
                    Value::Real(12.0),
                ]),
                Value::List(vec![
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]),
            ])],
        );
        assert!(
            eval_builtin("inverse", &[m]).is_undef(),
            "inverse of singular 4×4 should be Undef"
        );
    }

    /// (c) Dimensioned 4×4 (LENGTH cells) → inverse dim = DIMENSIONLESS / LENGTH.
    #[test]
    fn inverse_4x4_dimensioned_length_cells() {
        let m = make_4x4_tridiagonal_length();
        let inv = eval_builtin("inverse", &[m]);

        assert!(
            !inv.is_undef(),
            "inverse of dimensioned 4×4 should not be Undef"
        );

        let (_, _, _, inv_dim) = matrix_components_f64(&inv).expect("inv must parse");
        let expected_dim = DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH);
        assert_eq!(
            inv_dim, expected_dim,
            "inverse dim expected DIMENSIONLESS/LENGTH, got {:?}",
            inv_dim
        );
    }
}
