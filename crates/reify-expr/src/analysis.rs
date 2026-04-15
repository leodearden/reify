//! Field-level stress analysis wrappers.
//!
//! When analysis builtins (von_mises, principal_stresses, max_shear, safety_factor)
//! receive a Field<Point3, Tensor> argument, these functions create a new Field
//! that applies the analysis pointwise when sampled.
//!
//! Follows the same FieldSourceKind pattern as gradient/divergence/curl/laplacian
//! in calculus.rs: the original field is stored in the lambda slot, and the sample
//! handler in lib.rs dispatches to pointwise evaluation via reify_stdlib.

use reify_types::{DimensionVector, FieldSourceKind, Type, Value};

use super::{EvalContext, apply_lambda};

/// Extract the element dimension from a 3×3 matrix/tensor codomain type.
///
/// Returns `Some(dimension)` for:
/// - `Type::Matrix { m: 3, n: 3, quantity }` where quantity is scalar-compatible
/// - `Type::Tensor { rank: 2, n: 3, quantity }` where quantity is scalar-compatible
///
/// Returns `None` for all other types.
fn tensor_element_dimension(codomain: &Type) -> Option<DimensionVector> {
    match codomain {
        Type::Matrix { m: 3, n: 3, quantity } | Type::Tensor { rank: 2, n: 3, quantity } => {
            match quantity.as_ref() {
                Type::Scalar { dimension } => Some(*dimension),
                Type::Real | Type::Int => Some(DimensionVector::DIMENSIONLESS),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Validate that a value is a field with a tensor codomain suitable for analysis.
///
/// Performs validation analogous to `calculus::validate_differentiable_field`:
/// 1. `field_val` must be `Value::Field { .. }`
/// 2. `source` must be `Analytical` or `Composed` (derived fields store the
///    original field in the lambda slot, not a callable Lambda)
/// 3. `lambda` slot must be `Value::Lambda { .. }` (callable)
/// 4. `codomain_type` must be a 3×3 matrix/tensor with scalar elements
///
/// Returns `Some((domain_type, codomain_type, element_dimension))` if all
/// checks pass, `None` otherwise.
fn validate_tensor_field<'a>(
    field_val: &'a Value,
    op: &str,
) -> Option<(&'a Type, &'a Type, DimensionVector)> {
    let (domain_type, codomain_type, source, lambda) = match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } => (domain_type, codomain_type, source, lambda),
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] {op}: argument is not a Field: {:?}",
                field_val
            );
            return None;
        }
    };

    if !matches!(
        source,
        FieldSourceKind::Analytical | FieldSourceKind::Composed
    ) {
        #[cfg(debug_assertions)]
        eprintln!("[reify-expr] {op}: unsupported source kind {:?}", source);
        return None;
    }

    if !matches!(lambda.as_ref(), Value::Lambda { .. }) {
        #[cfg(debug_assertions)]
        eprintln!(
            "[reify-expr] {op}: lambda slot is not callable: {:?}",
            lambda
        );
        return None;
    }

    let elem_dim = match tensor_element_dimension(codomain_type) {
        Some(d) => d,
        None => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] {op}: codomain is not a 3×3 tensor: {:?}",
                codomain_type
            );
            return None;
        }
    };

    Some((domain_type, codomain_type, elem_dim))
}

/// Build the scalar result type from an element dimension.
///
/// Returns `Type::Real` for dimensionless, `Type::Scalar { dimension }` otherwise.
fn scalar_type_for_dim(dim: DimensionVector) -> Type {
    if dim == DimensionVector::DIMENSIONLESS {
        Type::Real
    } else {
        Type::Scalar { dimension: dim }
    }
}

/// Create a VonMises-wrapped field from a tensor field.
///
/// Given a `Field<D, Matrix3x3<Q>>`, returns a `Field<D, Scalar<Q>>` with
/// `source = FieldSourceKind::VonMises` and the original field stored in the
/// lambda slot.
pub(crate) fn compute_von_mises(field_val: &Value) -> Value {
    let (domain_type, _codomain_type, elem_dim) =
        match validate_tensor_field(field_val, "von_mises") {
            Some(triple) => triple,
            None => return Value::Undef,
        };

    let result_codomain = scalar_type_for_dim(elem_dim);

    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::VonMises,
        lambda: Box::new(field_val.clone()),
    }
}

/// Sample the inner field at a point, handling the multi-param unpacking convention.
///
/// When the inner lambda has multiple params (e.g., `|x, y, z|`) and the point
/// is a `Value::Point` with matching length, unpacks the point components into
/// individual scalar arguments. A single-param lambda receives the whole Point.
///
/// Mirrors the unpacking logic in the `sample` handler for `Value::Lambda` fields.
fn sample_inner_field(lambda: &Value, point: &Value, ctx: &EvalContext) -> Value {
    if let Value::Lambda { params, .. } = lambda {
        if params.len() > 1 {
            if let Value::Point(items) = point {
                if params.len() == items.len() {
                    return apply_lambda(lambda, items.as_slice(), ctx);
                }
            }
        }
        apply_lambda(lambda, std::slice::from_ref(point), ctx)
    } else {
        Value::Undef
    }
}

/// Sample a VonMises-wrapped field at a point.
///
/// Evaluates the original tensor field's lambda at the given point, then
/// applies `von_mises` pointwise via `reify_stdlib::eval_builtin`.
pub(crate) fn sample_von_mises_at_point(
    inner_lambda: &Value,
    point: &Value,
    _codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    let tensor = sample_inner_field(inner_lambda, point, ctx);
    if tensor.is_undef() {
        return Value::Undef;
    }
    reify_stdlib::eval_builtin("von_mises", &[tensor])
}
