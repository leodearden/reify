use reify_types::{DimensionVector, Value};
use crate::common::{unary, binary, sanitize_value, tensor_components_f64};

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
        // --- Advanced linear algebra: determinant, inverse, transpose, outer, trace, eigenvalues ---
        "determinant" => unary(args, |v| {
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
                _ => return Value::Undef, // only 1×1, 2×2, 3×3 supported
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
                _ => Value::Undef,
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
                    // Characteristic polynomial: λ³ - pλ² + qλ - r = 0
                    // where p = tr(A), q = sum of 2×2 principal minor dets, r = det(A)
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);

                    let p = a + e + i; // trace
                    let q = (a * e - b * d) + (a * i - c * g) + (e * i - f * h);
                    let r = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g); // det

                    // Depressed cubic: t³ + αt + β = 0 where λ = t + p/3
                    let p3 = p / 3.0;
                    let alpha = q - p * p / 3.0;
                    let beta = -2.0 * p * p * p / 27.0 + p * q / 3.0 - r;

                    // Discriminant for three real roots: 4α³ + 27β² ≤ 0
                    // Use trigonometric (Viète) method for the all-real-roots case
                    if alpha >= 0.0 {
                        // At most one real root when α ≥ 0 and β ≠ 0
                        if alpha == 0.0 && beta == 0.0 {
                            // Triple root
                            let root = p3;
                            Value::List(vec![make_val(root), make_val(root), make_val(root)])
                        } else if alpha == 0.0 {
                            // t³ = -β → single real cube root
                            let t = (-beta).cbrt();
                            // One real root + two complex; return Undef
                            // Actually: t³ + 0*t + β = 0 has one real root t = (-β)^(1/3)
                            // and two complex conjugate roots. But if beta = 0 handled above.
                            // For non-zero beta with alpha=0, we have a triple-like scenario
                            // returning only the real eigenvalue as Undef for now.
                            let _ = t;
                            Value::Undef
                        } else {
                            // General case with α > 0: complex eigenvalues possible
                            Value::Undef
                        }
                    } else {
                        // α < 0: use trigonometric method for three real roots
                        let neg_alpha = -alpha;
                        let m = (neg_alpha / 3.0).sqrt(); // sqrt(-α/3)
                        let cos_arg = -beta / (2.0 * m * m * m);
                        // Clamp for numerical stability
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
    };
    Some(v)
}

/// Extract a square or rectangular matrix from a `Value` into `(nrows, ncols, flat_data, element_dim)`.
///
/// Handles both `Value::Matrix(rows)` and nested `Value::Tensor` (rank-2 Tensor).
/// All elements must share the same dimension and be numeric.
pub(crate) fn matrix_components_f64(v: &Value) -> Option<(usize, usize, Vec<f64>, DimensionVector)> {
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
pub(crate) fn build_matrix_value(nrows: usize, ncols: usize, data: &[f64], dim: DimensionVector) -> Value {
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
mod dispatch_tests {
    use super::*;

    fn make_identity_matrix_2x2() -> Value {
        // Build a 2x2 identity matrix as Value::Tensor([[1.0, 0.0], [0.0, 1.0]])
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]),
            Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]),
        ])
    }

    #[test]
    fn linalg_dispatch_determinant_identity() {
        let mat = make_identity_matrix_2x2();
        let result = dispatch("determinant", &[mat]);
        assert!(result.is_some(), "determinant should be handled by linalg dispatch");
        assert!(
            matches!(result, Some(Value::Real(v)) if (v - 1.0).abs() < 1e-12),
            "determinant of identity matrix should be 1.0"
        );
    }

    #[test]
    fn linalg_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
