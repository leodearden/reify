//! Shared helpers for the compiler's signature-family modules.
//!
//! `scalar_or_real` is the load-bearing Scalar-vs-Real boundary helper used
//! by both the math-linalg family (`math_signatures.rs`) and the
//! analysis-reduction family (`analysis_signatures.rs`).  It is hoisted here
//! so that a future change to the boundary logic has one canonical home.
//!
//! # Status
//!
//! `math_signatures.rs` still carries a private copy of `scalar_or_real`
//! (dedup is outside the scope of task 2884 — `math_signatures.rs` is not in
//! that task's locked file list).  The two copies are bit-for-bit identical and
//! covered by independent test suites.  Unifying them is a clean-up task.

use reify_core::{DimensionVector, Type};

/// Route the dimensionless case to `Type::dimensionless_scalar()` (NOT `Scalar{DIMENSIONLESS}`).
///
/// This matches the eval boundary: a dimensionless result produces
/// `Value::Real`, and `value_type_kind_matches(Value::Real,
/// Scalar{DIMENSIONLESS})` is `false` — so a dimensionless arm MUST return
/// `Type::dimensionless_scalar()` to keep the compile-type and eval-value in agreement.
///
/// Identical to `math_signatures::scalar_or_real` (task 2884 δ: extracted from
/// `analysis_signatures.rs` where it was a verbatim copy of the math helper).
pub(crate) fn scalar_or_real(dim: DimensionVector) -> Type {
    if dim.is_dimensionless() {
        Type::dimensionless_scalar()
    } else {
        Type::Scalar { dimension: dim }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    /// Dimensionless input → `Type::dimensionless_scalar()` (NOT `Type::Scalar{DIMENSIONLESS}`).
    #[test]
    fn dimensionless_routes_to_real() {
        assert_eq!(
            scalar_or_real(DimensionVector::DIMENSIONLESS),
            Type::dimensionless_scalar(),
            "scalar_or_real(DIMENSIONLESS) must be Type::dimensionless_scalar()"
        );
    }

    /// Pressure input → `Type::Scalar<PRESSURE>`.
    #[test]
    fn pressure_routes_to_scalar_pressure() {
        assert_eq!(
            scalar_or_real(DimensionVector::PRESSURE),
            Type::Scalar { dimension: DimensionVector::PRESSURE },
            "scalar_or_real(PRESSURE) must be Type::Scalar<PRESSURE>"
        );
    }
}
