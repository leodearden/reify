//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor.

use reify_types::{DimensionVector, Value};

use crate::helpers::{sanitize_value, unary};
use crate::matrix::matrix_components_f64;

/// Evaluate a stress-analysis builtin by name.
///
/// Returns `Some(value)` if the name is a recognised analysis function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_analysis(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "von_mises" => von_mises(args),
        "principal_stresses" => principal_stresses(args),
        "max_shear" => max_shear(args),
        "safety_factor" => {
            let _ = args;
            Value::Undef // stub — implementations added in subsequent steps
        }
        _ => return None,
    })
}

/// Compute von Mises equivalent stress from a 3×3 stress tensor.
///
/// Formula: σ_vm = √(0.5·((σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²+6·(σ_xy²+σ_yz²+σ_xz²)))
///
/// Uses the direct component formula (avoids eigenvalue computation).
/// Row-major flat layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
///                         d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
///                         d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
fn von_mises(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols); // used only for the 3×3 guard above

        let sxx = d[0];
        let syy = d[4];
        let szz = d[8];
        let sxy = d[1];
        let syz = d[5];
        let sxz = d[2];

        let vm = (0.5
            * ((sxx - syy).powi(2)
                + (syy - szz).powi(2)
                + (szz - sxx).powi(2)
                + 6.0 * (sxy.powi(2) + syz.powi(2) + sxz.powi(2))))
        .sqrt();

        sanitize_value(Value::from_component(vm, dim))
    })
}

/// Compute eigenvalues of a 3×3 matrix via Cardano's cubic formula.
///
/// Returns `Some([λ₁, λ₂, λ₃])` sorted ascending, or `None` for degenerate cases
/// (e.g. complex eigenvalues). Reuses the same algorithm as matrix.rs eigenvalues.
fn compute_eigenvalues_3x3(d: &[f64]) -> Option<[f64; 3]> {
    let (a, b, c) = (d[0], d[1], d[2]);
    let (dd, e, f) = (d[3], d[4], d[5]);
    let (g, h, i) = (d[6], d[7], d[8]);

    let p = a + e + i; // trace
    let q = (a * e - b * dd) + (a * i - c * g) + (e * i - f * h);
    let r = a * (e * i - f * h) - b * (dd * i - f * g) + c * (dd * h - e * g);

    let p3 = p / 3.0;
    let alpha = q - p * p / 3.0;
    let beta = -2.0 * p * p * p / 27.0 + p * q / 3.0 - r;

    if alpha >= 0.0 {
        if alpha == 0.0 && beta == 0.0 {
            // Triple root
            return Some([p3, p3, p3]);
        }
        // Complex eigenvalues or degenerate
        return None;
    }

    let neg_alpha = -alpha;
    let m = (neg_alpha / 3.0).sqrt();
    let cos_arg = (-beta / (2.0 * m * m * m)).clamp(-1.0, 1.0);
    let theta = cos_arg.acos();
    let two_m = 2.0 * m;

    let mut eigs = [
        two_m * (theta / 3.0).cos() + p3,
        two_m * ((theta + 2.0 * std::f64::consts::PI) / 3.0).cos() + p3,
        two_m * ((theta + 4.0 * std::f64::consts::PI) / 3.0).cos() + p3,
    ];
    eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(eigs)
}

/// Compute principal stresses (eigenvalues) of a 3×3 stress tensor.
///
/// Returns a sorted `Value::List` of 3 scalars (ascending order).
fn principal_stresses(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols);

        let eigs = match compute_eigenvalues_3x3(&d) {
            Some(e) => e,
            None => return Value::Undef,
        };

        let make_val = |x: f64| sanitize_value(Value::from_component(x, dim));
        Value::List(eigs.iter().map(|&e| make_val(e)).collect())
    })
}

/// Compute maximum shear stress from a 3×3 stress tensor.
///
/// max_shear = (σ₁ − σ₃) / 2 where σ₁ and σ₃ are the max and min principal stresses.
fn max_shear(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols);

        let eigs = match compute_eigenvalues_3x3(&d) {
            Some(e) => e,
            None => return Value::Undef,
        };

        // eigs is sorted ascending: [σ₃, σ₂, σ₁]
        let tau_max = (eigs[2] - eigs[0]) / 2.0;
        sanitize_value(Value::from_component(tau_max, dim))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_analysis("foo", &[]).is_none());
    }

    #[test]
    fn known_function_returns_some() {
        assert!(eval_analysis("von_mises", &[]).is_some());
        assert!(eval_analysis("principal_stresses", &[]).is_some());
        assert!(eval_analysis("max_shear", &[]).is_some());
        assert!(eval_analysis("safety_factor", &[]).is_some());
    }

    // ── test helpers ────────────────────────────────────────────────────────

    /// Build a dimensionless 3x3 matrix from rows.
    fn make_matrix(rows: &[&[f64]]) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| Value::Tensor(row.iter().map(|&v| Value::Real(v)).collect()))
                .collect(),
        )
    }

    /// Build a dimensioned 3x3 matrix with all elements sharing one dimension.
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

    // ── von_mises tests ─────────────────────────────────────────────────────

    #[test]
    fn von_mises_uniaxial_dimensionless() {
        // Uniaxial stress [[σ,0,0],[0,0,0],[0,0,0]] → von Mises = σ
        let sigma = 100.0;
        let tensor = make_matrix(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_real_approx!(result, sigma);
    }

    #[test]
    fn von_mises_uniaxial_pressure() {
        // Uniaxial stress with PRESSURE dimension → von Mises = σ (same dim)
        let sigma = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_hydrostatic_returns_zero() {
        // Hydrostatic stress [[p,0,0],[0,p,0],[0,0,p]] → von Mises = 0
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_scalar_approx!(result, 0.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_pure_shear() {
        // Pure shear [[0,τ,0],[τ,0,0],[0,0,0]] → von Mises = τ·√3
        let tau = 50e6;
        let tensor = make_dimensioned_matrix(
            &[&[0.0, tau, 0.0], &[tau, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        let expected = tau * 3.0_f64.sqrt();
        assert_scalar_approx!(result, expected, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_wrong_arg_count_returns_undef() {
        assert!(eval_analysis("von_mises", &[]).unwrap().is_undef());
        let t = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        assert!(eval_analysis("von_mises", &[t.clone(), t]).unwrap().is_undef());
    }

    #[test]
    fn von_mises_non_matrix_returns_undef() {
        assert!(eval_analysis("von_mises", &[Value::Real(42.0)])
            .unwrap()
            .is_undef());
    }

    #[test]
    fn von_mises_non_3x3_returns_undef() {
        // 2x2 matrix
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(eval_analysis("von_mises", &[m2x2]).unwrap().is_undef());
    }

    // ── principal_stresses tests ────────────────────────────────────────────

    #[test]
    fn principal_stresses_diagonal_dimensionless() {
        // Diagonal tensor [[100,0,0],[0,50,0],[0,0,25]] → sorted [25, 50, 100]
        let tensor = make_matrix(&[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]]);
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_real_approx!(items[0].clone(), 25.0);
                assert_real_approx!(items[1].clone(), 50.0);
                assert_real_approx!(items[2].clone(), 100.0);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_diagonal_pressure() {
        // Diagonal tensor with PRESSURE dimension → sorted List of Scalar
        let tensor = make_dimensioned_matrix(
            &[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 25.0, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[1].clone(), 50.0, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[2].clone(), 100.0, DimensionVector::PRESSURE);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_symmetric_tensor() {
        // Symmetric tensor [[2,1,0],[1,3,1],[0,1,2]] → eigenvalues [1, 2, 4]
        let tensor = make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_real_approx!(items[0].clone(), 1.0);
                assert_real_approx!(items[1].clone(), 2.0);
                assert_real_approx!(items[2].clone(), 4.0);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_hydrostatic() {
        // Hydrostatic [[p,0,0],[0,p,0],[0,0,p]] → [p, p, p]
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), p, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[1].clone(), p, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[2].clone(), p, DimensionVector::PRESSURE);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_wrong_args_return_undef() {
        assert!(eval_analysis("principal_stresses", &[]).unwrap().is_undef());
        assert!(eval_analysis("principal_stresses", &[Value::Real(1.0)])
            .unwrap()
            .is_undef());
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(eval_analysis("principal_stresses", &[m2x2])
            .unwrap()
            .is_undef());
    }

    // ── max_shear tests ─────────────────────────────────────────────────────

    #[test]
    fn max_shear_hydrostatic_returns_zero() {
        // Hydrostatic: all principal stresses equal → (σ₁−σ₃)/2 = 0
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, 0.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn eigenvalues_uniaxial_debug() {
        // Direct test of eigenvalue helper for uniaxial case
        let d = [200.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let eigs = compute_eigenvalues_3x3(&d)
            .expect("eigenvalues should compute for uniaxial matrix");
        assert!(
            (eigs[2] - 200.0).abs() < 1e-9,
            "largest eigenvalue should be 200, got {}",
            eigs[2]
        );
        assert!(
            eigs[0].abs() < 1e-9,
            "smallest eigenvalue should be ~0, got {}",
            eigs[0]
        );
    }

    #[test]
    fn max_shear_uniaxial() {
        // Uniaxial [[σ,0,0],[0,0,0],[0,0,0]] → max_shear = σ/2
        // Use small values to keep within absolute tolerance of assert macros
        let sigma = 200.0;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma / 2.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn max_shear_biaxial() {
        // Biaxial [[σ,0,0],[0,-σ,0],[0,0,0]] → max_shear = σ
        let sigma = 100.0;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, -sigma, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma, DimensionVector::PRESSURE);
    }

    #[test]
    fn max_shear_wrong_args_return_undef() {
        assert!(eval_analysis("max_shear", &[]).unwrap().is_undef());
        assert!(eval_analysis("max_shear", &[Value::Real(1.0)])
            .unwrap()
            .is_undef());
    }
}
