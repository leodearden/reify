//! Shared field-differential-operator codomain logic for the Reify type system.
//!
//! This module is the single source of truth for the codomain-type derivation
//! performed by the four differential operators (gradient, divergence, curl,
//! laplacian) on `Type::Field` values.  Both the compile layer
//! (`reify-compiler::units`) and the eval layer (`reify-expr::calculus`) delegate
//! here, making their typing provably consistent.
//!
//! Previously the logic was duplicated as:
//! - four private helpers + four arms in `reify-compiler/src/units.rs`
//!   (`scalar_dimension_for_field_op`, `domain_dimension_for_field_op`,
//!   `dimensionless_fallback_for_field_op`, `dim_quotient_type_for_field_op`)
//! - four private helpers + four `*_result_codomain` functions in
//!   `reify-expr/src/calculus.rs`
//!
//! The duplication required manual lockstep maintenance against PRD §5.1.  This
//! module eliminates that hazard.
//!
//! # B1 invariant
//!
//! This module uses only `crate::{Type, DimensionVector}` — no `reify-*`
//! dependencies are introduced.

use crate::{DimensionVector, Type};

/// The four differential operators supported on `Type::Field` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifferentialOp {
    Gradient,
    Divergence,
    Curl,
    Laplacian,
}

// ---------------------------------------------------------------------------
// Private dim helpers — moved verbatim from reify-expr/src/calculus.rs:19-94.
// They operate purely on crate::{Type, DimensionVector} and require no edits.
// ---------------------------------------------------------------------------

/// Unify scalar-quantity validation with dimension extraction.
///
/// Returns `Some(dimension)` for `Type::Scalar { dimension }` and
/// `Some(DimensionVector::DIMENSIONLESS)` for `Type::dimensionless_scalar() | Type::Int`,
/// since those are inherently dimensionless scalars.
///
/// Returns `None` for all other types (Point, Vector, Bool, etc.).
///
/// Using `scalar_dimension(ty).is_some()` replaces `matches!(ty, Type::dimensionless_scalar() | Type::Int |
/// Type::Scalar { .. })` at every call site, yielding both the validation result and the
/// dimension in a single call.
fn scalar_dimension(ty: &Type) -> Option<DimensionVector> {
    match ty {
        Type::Scalar { dimension } => Some(*dimension),
        Type::Int => Some(DimensionVector::DIMENSIONLESS),
        _ => None,
    }
}

/// Domain-side analog of `scalar_dimension`, handling the `Point{quantity}` wrapper.
///
/// For multi-dimensional domains (`Type::Point { quantity, .. }`), delegates to
/// `scalar_dimension(quantity)` to extract the inner quantity's dimension.
/// For all other types (including direct scalars, Real, Int), delegates directly
/// to `scalar_dimension`.
///
/// Returns `None` for non-scalar, non-Point types (e.g., Vector, Bool).
fn domain_dimension(ty: &Type) -> Option<DimensionVector> {
    match ty {
        Type::Point { quantity, .. } => scalar_dimension(quantity),
        _ => scalar_dimension(ty),
    }
}

/// Compute the quotient Type for a differential operator result.
///
/// Given optional codomain and domain dimensions and an exponent, returns:
/// - `Scalar { dimension: cd / dd^exponent }` when both dimensions are present and
///   neither is DIMENSIONLESS and the resulting quotient is not DIMENSIONLESS.
/// - `Type::dimensionless_scalar()` when both dimensions are present, neither is DIMENSIONLESS, but
///   the quotient itself is DIMENSIONLESS.
/// - `fallback` in all other cases (either dimension absent or DIMENSIONLESS).
///
/// The `domain_exponent` parameter is `i8` to match `DimensionVector::pow`. Only
/// values 1 and 2 are used in practice (exponent=1 skips the `pow` call).
fn dim_quotient_type(
    codomain_dim: Option<DimensionVector>,
    domain_dim: Option<DimensionVector>,
    domain_exponent: i8,
    fallback: Type,
) -> Type {
    match (codomain_dim, domain_dim) {
        (Some(cd), Some(dd))
            if cd != DimensionVector::DIMENSIONLESS && dd != DimensionVector::DIMENSIONLESS =>
        {
            let result_dim = if domain_exponent == 1 {
                cd.div(&dd)
            } else {
                cd.div(&dd.pow(domain_exponent))
            };
            if result_dim != DimensionVector::DIMENSIONLESS {
                Type::Scalar {
                    dimension: result_dim,
                }
            } else {
                Type::dimensionless_scalar()
            }
        }
        _ => fallback,
    }
}

/// Compute the "dimensionless fallback" type for a differential operator result.
///
/// Returns `Type::dimensionless_scalar()` if `ty` is `Scalar { dimension: DIMENSIONLESS }`,
/// otherwise clones and returns `ty` unchanged.
///
/// This is the shared pattern used by all 4 type-level differential operators
/// (gradient, divergence, curl, laplacian) when computing the fallback type
/// passed to `dim_quotient_type`. A dimensionless `Scalar` is normalised to
/// `Real` so the operator result has no spurious dimensionless-scalar wrapping.
fn dimensionless_fallback(ty: &Type) -> Type {
    match ty {
        Type::Scalar { dimension } if *dimension == DimensionVector::DIMENSIONLESS => {
            Type::dimensionless_scalar()
        }
        _ => ty.clone(),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute the result CODOMAIN type for a differential operator on a field.
///
/// Returns `Some(codomain_type)` when the `domain` and `codomain` argument types
/// form a valid input for `op`; returns `None` on any shape mismatch.
///
/// The return value is the result **codomain** only (an `Option<Type>`), NOT a
/// wrapped `Type::Field`.  Callers that need a full Field type wrap it themselves:
///
/// ```text
/// differential_codomain(op, domain, codomain)
///     .map(|cod| Type::Field { domain: domain.clone(), codomain: Box::new(cod) })
/// ```
///
/// # PRD §5.1 contract
///
/// | op         | domain              | codomain               | result codomain            |
/// |------------|---------------------|------------------------|----------------------------|
/// | gradient   | scalar (1D)         | any scalar             | `gradient_quantity`        |
/// | gradient   | Point{n, scalar}    | any scalar             | `Vector{n, gradient_qty}`  |
/// | divergence | Point{n, scalar}    | Vector{n, scalar}      | `dim_quotient`             |
/// | curl       | Point{3, scalar}    | Vector{3, scalar}      | `vec3(dim_quotient)`       |
/// | laplacian  | scalar/Point{n,sc}  | scalar                 | `dim_quotient(exp=2)`      |
///
/// `None` is returned for any mismatch (wrong dimensionality, non-scalar quantity,
/// vector length mismatch, etc.).
pub fn differential_codomain(
    op: DifferentialOp,
    domain: &Type,
    codomain: &Type,
) -> Option<Type> {
    // STUB: always returns None until step-2 implements the body.
    let _ = (op, domain, codomain);
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- scalar_dimension unit tests (ported verbatim from calculus.rs:1542-1586) ---

    #[test]
    fn scalar_dimension_scalar_length_returns_length() {
        assert_eq!(
            scalar_dimension(&Type::length()),
            Some(DimensionVector::LENGTH)
        );
    }

    #[test]
    fn scalar_dimension_scalar_dimensionless_returns_dimensionless() {
        let ty = Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(scalar_dimension(&ty), Some(DimensionVector::DIMENSIONLESS));
    }

    #[test]
    fn scalar_dimension_real_returns_dimensionless() {
        assert_eq!(
            scalar_dimension(&Type::dimensionless_scalar()),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn scalar_dimension_int_returns_dimensionless() {
        assert_eq!(
            scalar_dimension(&Type::Int),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn scalar_dimension_point3_real_returns_none() {
        assert_eq!(scalar_dimension(&Type::point3(Type::dimensionless_scalar())), None);
    }

    #[test]
    fn scalar_dimension_vec3_real_returns_none() {
        assert_eq!(scalar_dimension(&Type::vec3(Type::dimensionless_scalar())), None);
    }

    #[test]
    fn scalar_dimension_bool_returns_none() {
        assert_eq!(scalar_dimension(&Type::Bool), None);
    }

    // --- domain_dimension unit tests (ported verbatim from calculus.rs:1591-1646) ---

    #[test]
    fn domain_dimension_scalar_length_returns_length() {
        assert_eq!(
            domain_dimension(&Type::length()),
            Some(DimensionVector::LENGTH)
        );
    }

    #[test]
    fn domain_dimension_real_returns_dimensionless() {
        assert_eq!(
            domain_dimension(&Type::dimensionless_scalar()),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn domain_dimension_int_returns_dimensionless() {
        assert_eq!(
            domain_dimension(&Type::Int),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn domain_dimension_point3_scalar_mass_returns_mass() {
        let ty = Type::point3(Type::Scalar {
            dimension: DimensionVector::MASS,
        });
        assert_eq!(domain_dimension(&ty), Some(DimensionVector::MASS));
    }

    #[test]
    fn domain_dimension_point3_real_returns_dimensionless() {
        assert_eq!(
            domain_dimension(&Type::point3(Type::dimensionless_scalar())),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn domain_dimension_point3_int_returns_dimensionless() {
        assert_eq!(
            domain_dimension(&Type::point3(Type::Int)),
            Some(DimensionVector::DIMENSIONLESS)
        );
    }

    #[test]
    fn domain_dimension_vec3_real_returns_none() {
        assert_eq!(domain_dimension(&Type::vec3(Type::dimensionless_scalar())), None);
    }

    #[test]
    fn domain_dimension_bool_returns_none() {
        assert_eq!(domain_dimension(&Type::Bool), None);
    }

    // --- dim_quotient_type unit tests (ported verbatim from calculus.rs:1651-1737) ---

    #[test]
    fn dim_quotient_both_dimensional_exp1_returns_scalar() {
        // LENGTH / TIME (exp=1) → Scalar{LENGTH/TIME}
        let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
        let result = dim_quotient_type(
            Some(DimensionVector::LENGTH),
            Some(DimensionVector::TIME),
            1,
            Type::dimensionless_scalar(),
        );
        assert_eq!(
            result,
            Type::Scalar {
                dimension: expected_dim
            }
        );
    }

    #[test]
    fn dim_quotient_both_dimensional_quotient_dimensionless_returns_real() {
        // LENGTH / LENGTH → DIMENSIONLESS → Type::dimensionless_scalar()
        let result = dim_quotient_type(
            Some(DimensionVector::LENGTH),
            Some(DimensionVector::LENGTH),
            1,
            Type::Int,
        );
        assert_eq!(result, Type::dimensionless_scalar());
    }

    #[test]
    fn dim_quotient_codomain_none_returns_fallback() {
        let result = dim_quotient_type(None, Some(DimensionVector::LENGTH), 1, Type::Int);
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn dim_quotient_domain_none_returns_fallback() {
        let result = dim_quotient_type(Some(DimensionVector::LENGTH), None, 1, Type::Int);
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn dim_quotient_codomain_dimensionless_returns_fallback() {
        let result = dim_quotient_type(
            Some(DimensionVector::DIMENSIONLESS),
            Some(DimensionVector::LENGTH),
            1,
            Type::Int,
        );
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn dim_quotient_domain_dimensionless_returns_fallback() {
        let result = dim_quotient_type(
            Some(DimensionVector::LENGTH),
            Some(DimensionVector::DIMENSIONLESS),
            1,
            Type::Int,
        );
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn dim_quotient_exp2_divides_by_domain_squared() {
        // VOLUME (L^3) / LENGTH^2 → LENGTH (L^1) → Scalar{LENGTH}
        let result = dim_quotient_type(
            Some(DimensionVector::VOLUME),
            Some(DimensionVector::LENGTH),
            2,
            Type::dimensionless_scalar(),
        );
        assert_eq!(
            result,
            Type::Scalar {
                dimension: DimensionVector::LENGTH
            }
        );
    }

    #[test]
    fn dim_quotient_fallback_returned_verbatim() {
        // Both None → fallback returned as-is (using a non-trivial fallback)
        let fallback = Type::length();
        let result = dim_quotient_type(None, None, 1, fallback.clone());
        assert_eq!(result, fallback);
    }

    // --- dimensionless_fallback unit tests (ported verbatim from calculus.rs:2102-2123) ---

    #[test]
    fn dimensionless_fallback_dimensionless_scalar_returns_real() {
        let ty = Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_eq!(dimensionless_fallback(&ty), Type::dimensionless_scalar());
    }

    #[test]
    fn dimensionless_fallback_length_scalar_returns_clone() {
        let ty = Type::length();
        assert_eq!(dimensionless_fallback(&ty), ty.clone());
    }

    #[test]
    fn dimensionless_fallback_real_returns_real() {
        assert_eq!(dimensionless_fallback(&Type::dimensionless_scalar()), Type::dimensionless_scalar());
    }

    #[test]
    fn dimensionless_fallback_int_returns_int() {
        assert_eq!(dimensionless_fallback(&Type::Int), Type::Int);
    }

    // --- differential_codomain PRD §5.1 table tests (step-1: RED until step-2) ---
    // Expected values extracted from the validated units.rs reference
    // (field_op_result_type_gradient_is_codomain_correct) and the
    // *_result_codomain functions being moved from calculus.rs.

    // Gradient: 1D dimensionless
    #[test]
    fn differential_codomain_gradient_1d_dimensionless_returns_dimensionless() {
        assert_eq!(
            differential_codomain(
                DifferentialOp::Gradient,
                &Type::dimensionless_scalar(),
                &Type::dimensionless_scalar(),
            ),
            Some(Type::dimensionless_scalar()),
            "gradient(Field<Real,Real>) result codomain must be Real"
        );
    }

    // Gradient: nD dimensioned — Point3<Length>, Scalar<Temperature> → Vector{3, Scalar<Temp/Length>}
    #[test]
    fn differential_codomain_gradient_nd_dimensioned_returns_vector() {
        let domain = Type::point3(Type::length());
        let codomain = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        };
        let expected = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH),
            }),
        };
        assert_eq!(
            differential_codomain(DifferentialOp::Gradient, &domain, &codomain),
            Some(expected),
            "gradient nD dimensioned codomain mismatch"
        );
    }

    // Divergence: Point{3,Length}, Vector{3, Scalar<Pressure>} → Scalar<Pressure/Length>
    #[test]
    fn differential_codomain_divergence_dimensioned_returns_scalar() {
        let domain = Type::point3(Type::length());
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            }),
        };
        let expected = Type::Scalar {
            dimension: DimensionVector::PRESSURE.div(&DimensionVector::LENGTH),
        };
        assert_eq!(
            differential_codomain(DifferentialOp::Divergence, &domain, &codomain),
            Some(expected),
            "divergence dimensioned codomain mismatch"
        );
    }

    // Curl: Point3<Length>, vec3(Scalar<Q>) → vec3(Scalar<Q/Length>)
    #[test]
    fn differential_codomain_curl_dimensioned_returns_vec3() {
        let q = DimensionVector::TEMPERATURE; // arbitrary named dimension Q
        let domain = Type::point3(Type::length());
        let codomain = Type::vec3(Type::Scalar { dimension: q });
        let expected = Type::vec3(Type::Scalar {
            dimension: q.div(&DimensionVector::LENGTH),
        });
        assert_eq!(
            differential_codomain(DifferentialOp::Curl, &domain, &codomain),
            Some(expected),
            "curl dimensioned codomain mismatch"
        );
    }

    // Laplacian: Point{3,Length}, Scalar<Temperature> → Scalar<Temperature/Length²>
    #[test]
    fn differential_codomain_laplacian_dimensioned_returns_scalar() {
        let domain = Type::point3(Type::length());
        let codomain = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        };
        let expected = Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2)),
        };
        assert_eq!(
            differential_codomain(DifferentialOp::Laplacian, &domain, &codomain),
            Some(expected),
            "laplacian dimensioned codomain mismatch"
        );
    }

    // Dimensionless variants → dimensionless_scalar
    #[test]
    fn differential_codomain_divergence_dimensionless_returns_dimensionless() {
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::vec3(Type::dimensionless_scalar());
        assert_eq!(
            differential_codomain(DifferentialOp::Divergence, &domain, &codomain),
            Some(Type::dimensionless_scalar()),
        );
    }

    #[test]
    fn differential_codomain_curl_dimensionless_returns_dimensionless_vec3() {
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::vec3(Type::dimensionless_scalar());
        assert_eq!(
            differential_codomain(DifferentialOp::Curl, &domain, &codomain),
            Some(Type::vec3(Type::dimensionless_scalar())),
        );
    }

    #[test]
    fn differential_codomain_laplacian_dimensionless_returns_dimensionless() {
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::dimensionless_scalar();
        assert_eq!(
            differential_codomain(DifferentialOp::Laplacian, &domain, &codomain),
            Some(Type::dimensionless_scalar()),
        );
    }

    // Shape-mismatch cases → None

    /// gradient: Vector domain → None (Point domain required for nD)
    #[test]
    fn differential_codomain_gradient_vector_domain_returns_none() {
        let domain = Type::vec3(Type::dimensionless_scalar());
        let codomain = Type::dimensionless_scalar();
        assert_eq!(
            differential_codomain(DifferentialOp::Gradient, &domain, &codomain),
            None,
            "gradient with Vector domain must return None"
        );
    }

    /// divergence: non-Vector codomain → None
    #[test]
    fn differential_codomain_divergence_scalar_codomain_returns_none() {
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::dimensionless_scalar(); // not a Vector
        assert_eq!(
            differential_codomain(DifferentialOp::Divergence, &domain, &codomain),
            None,
            "divergence with non-Vector codomain must return None"
        );
    }

    /// divergence: vec_n ≠ n → None
    #[test]
    fn differential_codomain_divergence_dimension_mismatch_returns_none() {
        // domain=Point{3,…} but codomain=Vector{2,…}
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::Vector {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        assert_eq!(
            differential_codomain(DifferentialOp::Divergence, &domain, &codomain),
            None,
            "divergence with vec_n ≠ n must return None"
        );
    }

    /// curl: non-3D domain → None
    #[test]
    fn differential_codomain_curl_non_3d_domain_returns_none() {
        let domain = Type::Point {
            n: 2,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let codomain = Type::vec3(Type::dimensionless_scalar());
        assert_eq!(
            differential_codomain(DifferentialOp::Curl, &domain, &codomain),
            None,
            "curl with non-3D domain must return None"
        );
    }

    /// laplacian: Vector codomain → None
    #[test]
    fn differential_codomain_laplacian_vector_codomain_returns_none() {
        let domain = Type::point3(Type::dimensionless_scalar());
        let codomain = Type::vec3(Type::dimensionless_scalar()); // not a scalar
        assert_eq!(
            differential_codomain(DifferentialOp::Laplacian, &domain, &codomain),
            None,
            "laplacian with Vector codomain must return None"
        );
    }
}
