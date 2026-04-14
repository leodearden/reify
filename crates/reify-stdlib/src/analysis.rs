//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor.

use reify_types::Value;

use crate::helpers::{sanitize_value, unary};
use crate::matrix::matrix_components_f64;

/// Evaluate a stress-analysis builtin by name.
///
/// Returns `Some(value)` if the name is a recognised analysis function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_analysis(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "von_mises" => von_mises(args),
        "principal_stresses" | "max_shear" | "safety_factor" => {
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
}
