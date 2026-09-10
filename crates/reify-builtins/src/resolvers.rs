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

    // ── #6577's Field-argument contract ──────────────────────────────────────
    //
    // Ported from `crates/reify-compiler/src/analysis_signatures.rs`'s test
    // module (main, blob 780f4cab83) when task #6001 α merged main forward and
    // resolved that file as DELETE. Re-expressed over `&[Type]`, since
    // reify-builtins cannot see `reify_ir::CompiledExpr`.
    //
    // Eval does NOT reduce a field eagerly — it wraps it LAZILY and hands back a
    // `Value::Field` (`compute_von_mises` / `compute_max_shear` →
    // `wrap_tensor_field`, crates/reify-expr/src/analysis.rs). Since
    // `value_type_kind_matches` (crates/reify-eval/src/lib.rs:330) maps a
    // `Value::Field` onto `Type::Field` and nothing else, answering with a
    // reduced `Scalar` here would trade a dimension bug for a KIND LIE that
    // `engine_admin.rs` surfaces as `EngineError::TypeKindMismatch`.

    /// The domain both Field fixtures carry — and the one every Field answer
    /// must hand back verbatim, never re-derived.
    fn field_domain() -> Type {
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        })
    }

    /// `Field<Point3<Length>, Tensor<2,3,quantity>>` — the compile-time type of
    /// `solve_elastic_static(..).stress`.
    fn tensor_field(quantity: Type) -> Type {
        Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::tensor(2, 3, quantity)),
        }
    }

    /// The same Field shape with a `Matrix{m:3,n:3}` codomain. Legacy matched
    /// `Tensor{rank:2,n:3}` and `Matrix{m:3,n:3}` in ONE arm, so both must be
    /// admitted identically.
    fn matrix_field(quantity: Type) -> Type {
        Type::Field {
            domain: Box::new(field_domain()),
            codomain: Box::new(Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(quantity),
            }),
        }
    }

    fn pressure() -> Type {
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }
    }

    /// `Field<D, Tensor<2,3,Scalar<PRESSURE>>>` → `Field<D, Scalar<PRESSURE>>`.
    ///
    /// The resolver behind BOTH `von_mises` and `max_shear` (they share one
    /// row-level `ArgAware` resolver, exactly as they shared one legacy ladder
    /// arm). Mirrors `wrap_tensor_field` (analysis.rs:205-231, :263-265).
    #[test]
    fn a_pressure_tensor_field_reduces_to_a_pressure_field_not_a_scalar() {
        assert_eq!(
            tensor_scalar_reduction(&[tensor_field(pressure())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(pressure()),
            }),
            "a Field argument must yield a Field result carrying the tensor's \
             quantity — eval hands back a Value::Field, which \
             value_type_kind_matches maps onto Type::Field and nothing else"
        );
    }

    /// A `Matrix{m:3,n:3}` codomain is admitted exactly as the `Tensor` one is.
    #[test]
    fn a_matrix_codomain_field_is_admitted_exactly_as_a_tensor_codomain_is() {
        assert_eq!(
            tensor_scalar_reduction(&[matrix_field(pressure())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(pressure()),
            }),
            "legacy matched Tensor{{rank:2,n:3}} and Matrix{{m:3,n:3}} in ONE \
             arm — the Field prelude must not split them"
        );
    }

    /// A dimensionless tensor codomain routes through `scalar_or_real`, so the
    /// Field's codomain is `Type::dimensionless_scalar()` — NOT
    /// `Scalar{DIMENSIONLESS}`, for the reason `scalar_or_real` documents.
    #[test]
    fn a_dimensionless_tensor_field_yields_a_real_codomain_field() {
        assert_eq!(
            tensor_scalar_reduction(&[tensor_field(Type::dimensionless_scalar())]),
            Some(Type::Field {
                domain: Box::new(field_domain()),
                codomain: Box::new(Type::dimensionless_scalar()),
            }),
            "the Field prelude must wrap the SAME scalar_or_real codomain the \
             concrete path computes, dimensionless case included"
        );
    }
}
