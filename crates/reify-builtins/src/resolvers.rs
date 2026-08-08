//! `ArgAware` result-type resolvers and the type helpers they are built from.
//!
//! Pure `reify_core::Type` algebra. These are compile-time type computation,
//! never eval — the crate holds no `Value` (PRD decision 3).

use reify_core::{DimensionVector, Type};

/// Route the dimensionless case to `Type::dimensionless_scalar()` (NOT `Scalar{DIMENSIONLESS}`).
///
/// This matches the eval boundary: a dimensionless result produces
/// `Value::Real`, and `value_type_kind_matches(Value::Real,
/// Scalar{DIMENSIONLESS})` is `false` — so a dimensionless arm MUST return
/// `Type::dimensionless_scalar()` to keep the compile-type and eval-value in agreement.
///
/// Moved verbatim from `crates/reify-compiler/src/signatures_common.rs:27`
/// (task #6001 α), whose last user disappeared when the analysis family moved
/// to the registry. `math_signatures.rs` keeps its own bit-identical private
/// copy; deduping that is τ-numeric's, not α's.
pub fn scalar_or_real(dim: DimensionVector) -> Type {
    if dim.is_dimensionless() {
        Type::dimensionless_scalar()
    } else {
        Type::Scalar { dimension: dim }
    }
}

/// The quantity dimension carried by a `Tensor` / `Matrix` arg at position
/// `i`, defaulting to `DIMENSIONLESS` when the arg is absent or not a tensor.
///
/// Ported from `crates/reify-compiler/src/analysis_signatures.rs:96` and
/// re-signatured from `&[CompiledExpr]` to `&[Type]` — reify-builtins cannot
/// see reify-ir. Same `Tensor|Matrix ⇒ Scalar{dimension}` match, same
/// DIMENSIONLESS default.
fn tensor_quantity(args: &[Type], i: usize) -> DimensionVector {
    match args.get(i) {
        Some(Type::Tensor { quantity, .. }) | Some(Type::Matrix { quantity, .. }) => {
            match quantity.as_ref() {
                Type::Scalar { dimension } => *dimension,
                _ => DimensionVector::DIMENSIONLESS,
            }
        }
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// `von_mises` / `max_shear`: scalar reduction carrying the tensor's quantity.
///
/// A Pressure tensor → `Scalar<Pressure>`; a dimensionless tensor → `Real`.
/// Mirrors `trace` / `magnitude` in `math_fn_result_type`.
///
/// Returns `Some(..)` unconditionally: legacy behaviour never rejected an
/// argument shape, and α preserves it exactly. Wiring the `None` ⇒
/// `E_BuiltinArgShape` path is τ-numeric's work (PRD §3 decision 5).
pub(crate) fn tensor_scalar_reduction(args: &[Type]) -> Option<Type> {
    Some(scalar_or_real(tensor_quantity(args, 0)))
}

/// `principal_stresses`: the same reduction, wrapped in a `List`.
///
/// Mirrors the `eigenvalues` arm in `math_fn_result_type` (matrix → List of
/// eigenvalues). Total for the same reason as [`tensor_scalar_reduction`].
pub(crate) fn tensor_scalar_reduction_list(args: &[Type]) -> Option<Type> {
    Some(Type::List(Box::new(scalar_or_real(tensor_quantity(
        args, 0,
    )))))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Type::Scalar {
                dimension: DimensionVector::PRESSURE
            },
            "scalar_or_real(PRESSURE) must be Type::Scalar<PRESSURE>"
        );
    }

    /// `tensor_quantity` reads a `Tensor`'s and a `Matrix`'s quantity alike,
    /// and defaults to DIMENSIONLESS for absent / non-tensor / non-scalar-
    /// quantity args.
    #[test]
    fn tensor_quantity_reads_tensor_and_matrix_alike() {
        let pressure = Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        });
        let tensor = Type::Tensor {
            rank: 2,
            n: 3,
            quantity: pressure.clone(),
        };
        let matrix = Type::Matrix {
            m: 3,
            n: 3,
            quantity: pressure,
        };

        assert_eq!(
            tensor_quantity(&[tensor], 0),
            DimensionVector::PRESSURE,
            "Tensor quantity"
        );
        assert_eq!(
            tensor_quantity(&[matrix], 0),
            DimensionVector::PRESSURE,
            "Matrix quantity — legacy matched both in ONE arm"
        );

        assert_eq!(
            tensor_quantity(&[], 0),
            DimensionVector::DIMENSIONLESS,
            "absent arg defaults to DIMENSIONLESS"
        );
        assert_eq!(
            tensor_quantity(&[Type::String], 0),
            DimensionVector::DIMENSIONLESS,
            "non-tensor arg defaults to DIMENSIONLESS"
        );
        assert_eq!(
            tensor_quantity(
                &[Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(Type::String),
                }],
                0
            ),
            DimensionVector::DIMENSIONLESS,
            "a non-Scalar quantity defaults to DIMENSIONLESS"
        );
    }
}
