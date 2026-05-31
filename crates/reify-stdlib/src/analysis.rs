//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor.

use reify_ir::Value;

use crate::helpers::{binary, sanitize_value, unary};
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
        "safety_factor" => safety_factor(args),
        _ => return None,
    })
}

/// Compute von Mises equivalent stress for a 3×3 row-major stress window.
///
/// Formula: σ_vm = √(0.5·((σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²+6·(σ_xy²+σ_yz²+σ_xz²)))
///
/// Uses the direct component formula (avoids eigenvalue computation).
/// Row-major flat layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
///                         d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
///                         d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
///
/// `pub` (widened from `pub(crate)`) so the formula has a single home — the
/// `Value::Tensor`-shaped `von_mises` builtin (below), the SampledField
/// hot-path projection in `crates/reify-stdlib/src/fea.rs`, AND the
/// cross-crate VonMises field reduction in
/// `crates/reify-expr/src/field_reductions.rs` all route through this kernel.
/// Re-exported via `pub use analysis::compute_von_mises_3x3` in `lib.rs` so
/// the call site in reify-expr is `reify_stdlib::compute_von_mises_3x3`.
/// Mirrors the `pub(crate)` promotion of `compute_eigenvalues_3x3` for the
/// same cross-module reuse pattern.
///
/// Window must be at least 9 floats long; only `d[0..9]` is read.
pub fn compute_von_mises_3x3(d: &[f64]) -> f64 {
    debug_assert!(
        d.len() >= 9,
        "compute_von_mises_3x3 requires at least 9 elements, got {}",
        d.len()
    );
    debug_assert!(
        {
            let tol = |a: f64, b: f64| (a - b).abs() <= 1e-10 * (1.0 + a.abs().max(b.abs()));
            tol(d[1], d[3]) && tol(d[2], d[6]) && tol(d[5], d[7])
        },
        "compute_von_mises_3x3: input matrix is not symmetric"
    );

    let sxx = d[0];
    let syy = d[4];
    let szz = d[8];
    let sxy = d[1];
    let syz = d[5];
    let sxz = d[2];

    (0.5 * ((sxx - syy).powi(2)
        + (syy - szz).powi(2)
        + (szz - sxx).powi(2)
        + 6.0 * (sxy.powi(2) + syz.powi(2) + sxz.powi(2))))
    .sqrt()
}

/// Compute von Mises equivalent stress from a 3×3 stress tensor `Value::Tensor`.
///
/// Wraps `compute_von_mises_3x3` for the dynamic-Value entry point used by
/// the `eval_builtin("von_mises", ...)` dispatch path.
fn von_mises(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols); // used only for the 3×3 guard above

        let vm = compute_von_mises_3x3(&d);

        sanitize_value(Value::from_real_scalar(vm, dim))
    })
}

/// Compute eigenvalues of a symmetric 3×3 matrix.
///
/// Uses the closed-form formula optimized for symmetric matrices: two eigenvalues
/// are computed trigonometrically and the third is recovered from the trace
/// constraint (trace = λ₁ + λ₂ + λ₃), which avoids precision loss at repeated roots.
///
/// Returns `Some([λ₁, λ₂, λ₃])` sorted ascending.
///
/// `pub(crate)` for cross-module reuse from
/// `crates/reify-stdlib/src/fea.rs::envelope_max_principal` — the
/// per-grid-point projection inlines this call on each 9-float row-major
/// stress window and selects `eigs[2]` (the largest principal stress).
pub(crate) fn compute_eigenvalues_3x3(d: &[f64]) -> Option<[f64; 3]> {
    debug_assert!(
        d.len() >= 9,
        "compute_eigenvalues_3x3 requires at least 9 elements, got {}",
        d.len()
    );
    // Row-major: d[0]=a00, d[1]=a01, d[2]=a02, d[4]=a11, d[5]=a12, d[8]=a22
    // This function assumes a symmetric matrix — only upper-triangle entries
    // (d[1], d[2], d[5]) are read. The lower-triangle (d[3], d[6], d[7]) is
    // ignored. For non-symmetric inputs the result will be silently wrong.
    debug_assert!(
        {
            let tol = |a: f64, b: f64| (a - b).abs() <= 1e-10 * (1.0 + a.abs().max(b.abs()));
            tol(d[1], d[3]) && tol(d[2], d[6]) && tol(d[5], d[7])
        },
        "compute_eigenvalues_3x3 assumes a symmetric matrix but got non-symmetric entries: \
         a01={} vs a10={}, a02={} vs a20={}, a12={} vs a21={}",
        d[1],
        d[3],
        d[2],
        d[6],
        d[5],
        d[7]
    );

    let a00 = d[0];
    let a11 = d[4];
    let a22 = d[8];
    let a01 = d[1];
    let a02 = d[2];
    let a12 = d[5];

    let q = (a00 + a11 + a22) / 3.0; // trace / 3

    // Sum of squared off-diagonal elements (symmetric, so count each pair once)
    let p1 = a01 * a01 + a02 * a02 + a12 * a12;

    if p1 <= 1e-30 {
        // Matrix is (effectively) diagonal — eigenvalues are the diagonal entries
        let mut eigs = [a00, a11, a22];
        eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        return Some(eigs);
    }

    let p2 = (a00 - q).powi(2) + (a11 - q).powi(2) + (a22 - q).powi(2) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();

    // B = (A - q·I) / p
    let b00 = (a00 - q) / p;
    let b11 = (a11 - q) / p;
    let b22 = (a22 - q) / p;
    let b01 = a01 / p;
    let b02 = a02 / p;
    let b12 = a12 / p;

    // det(B) / 2 — B is symmetric so b10=b01, b20=b02, b21=b12
    let det_b = b00 * (b11 * b22 - b12 * b12) - b01 * (b01 * b22 - b12 * b02)
        + b02 * (b01 * b12 - b11 * b02);
    let r = (det_b / 2.0).clamp(-1.0, 1.0);

    let phi = r.acos() / 3.0;

    // Two eigenvalues via trigonometry, third from trace constraint
    let eig1 = q + 2.0 * p * phi.cos();
    let eig3 = q + 2.0 * p * (phi + 4.0 * std::f64::consts::FRAC_PI_3).cos();
    let eig2 = 3.0 * q - eig1 - eig3;

    let mut eigs = [eig1, eig2, eig3];
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

        let make_val = |x: f64| sanitize_value(Value::from_real_scalar(x, dim));
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
        sanitize_value(Value::from_real_scalar(tau_max, dim))
    })
}

/// Compute safety factor: yield_strength / von_mises(tensor).
///
/// Returns a dimensionless Real. If von_mises is zero (e.g. hydrostatic stress),
/// the division produces infinity which sanitize_value converts to Undef.
fn safety_factor(args: &[Value]) -> Value {
    binary(args, |tensor, yield_val| {
        let yield_f64 = match yield_val.as_f64() {
            Some(v) => v,
            None => return Value::Undef,
        };

        // Compute von Mises of the tensor via the same logic as the von_mises builtin
        let vm = von_mises(std::slice::from_ref(tensor));
        let vm_f64 = match vm.as_f64() {
            Some(v) => v,
            None => return Value::Undef,
        };

        sanitize_value(Value::Real(yield_f64 / vm_f64))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

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
        assert!(
            eval_analysis("von_mises", &[t.clone(), t])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn von_mises_non_matrix_returns_undef() {
        assert!(
            eval_analysis("von_mises", &[Value::Real(42.0)])
                .unwrap()
                .is_undef()
        );
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
        assert!(
            eval_analysis("principal_stresses", &[Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(
            eval_analysis("principal_stresses", &[m2x2])
                .unwrap()
                .is_undef()
        );
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
        assert!(
            eval_analysis("max_shear", &[Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
    }

    // ── safety_factor tests ─────────────────────────────────────────────────

    #[test]
    fn safety_factor_safe_dimensionless() {
        // yield=250, uniaxial stress=100 → von_mises=100 → SF=250/100=2.5
        let sigma = 100.0;
        let yield_strength = Value::Real(250.0);
        let tensor = make_matrix(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert_real_approx!(result, 2.5);
    }

    #[test]
    fn safety_factor_safe_pressure() {
        // yield=250e6 Pa, uniaxial stress=100e6 Pa → SF=2.5 (dimensionless)
        let sigma = 100e6;
        let yield_strength = Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        // Result is dimensionless (pressure / pressure)
        assert_real_approx!(result, 2.5);
    }

    #[test]
    fn safety_factor_unsafe() {
        // yield=100e6, uniaxial stress=200e6 → SF=100/200=0.5 (<1, unsafe)
        let sigma = 200e6;
        let yield_strength = Value::Scalar {
            si_value: 100e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert_real_approx!(result, 0.5);
    }

    #[test]
    fn safety_factor_hydrostatic_returns_undef() {
        // Hydrostatic: von_mises = 0 → division by zero → Undef
        let p = 100e6;
        let yield_strength = Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert!(result.is_undef());
    }

    #[test]
    fn safety_factor_wrong_arg_count_returns_undef() {
        assert!(eval_analysis("safety_factor", &[]).unwrap().is_undef());
        let t = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        assert!(eval_analysis("safety_factor", &[t]).unwrap().is_undef());
    }
}
