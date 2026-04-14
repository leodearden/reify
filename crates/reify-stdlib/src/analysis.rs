//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor.

use reify_types::Value;

/// Evaluate a stress-analysis builtin by name.
///
/// Returns `Some(value)` if the name is a recognised analysis function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_analysis(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "von_mises" | "principal_stresses" | "max_shear" | "safety_factor" => {
            let _ = args;
            Value::Undef // stub — implementations added in subsequent steps
        }
        _ => return None,
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
