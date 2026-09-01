//! Field-level stress analysis wrappers.
//!
//! When analysis builtins (von_mises, principal_stresses, max_shear, safety_factor)
//! receive a Field<Point3, Tensor> argument, these functions create a new Field
//! that applies the analysis pointwise when sampled.
//!
//! Follows the same FieldSourceKind pattern as gradient/divergence/curl/laplacian
//! in calculus.rs: the original field is stored in the lambda slot, and the sample
//! handler in lib.rs dispatches to pointwise evaluation via reify_stdlib.
//!
//! Two tensor-field backings are accepted, and the accepted set is expressed as
//! a `(source, lambda)` PAIR (see `validate_tensor_field`):
//!
//! - `(Analytical | Composed, Value::Lambda { .. })` — a callable backing,
//!   evaluated pointwise on sample.
//! - `(Sampled, Value::SampledField(_))` — a grid backing, e.g. the stress
//!   field `solve_elastic_static` returns. This mirrors the `calculus.rs`
//!   Sampled arms, which likewise test both halves of the pair.
//!
//! The wrapper stays lazy over a Sampled backing: nothing is projected here.

use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_ir::{FieldSourceKind, Value};

use super::{EvalContext, apply_lambda_with_point_unpacking};

/// Extract the element dimension from a 3×3 matrix/tensor codomain type.
///
/// Returns `Some(dimension)` for:
/// - `Type::Matrix { m: 3, n: 3, quantity }` where quantity is scalar-compatible
/// - `Type::Tensor { rank: 2, n: 3, quantity }` where quantity is scalar-compatible
///
/// Returns `None` for all other types.
fn tensor_element_dimension(codomain: &Type) -> Option<DimensionVector> {
    match codomain {
        Type::Matrix {
            m: 3,
            n: 3,
            quantity,
        }
        | Type::Tensor {
            rank: 2,
            n: 3,
            quantity,
        } => match quantity.as_ref() {
            Type::Scalar { dimension } => Some(*dimension),
            Type::Int => Some(DimensionVector::DIMENSIONLESS),
            _ => None,
        },
        _ => None,
    }
}

/// Validate that a value is a field with a tensor codomain suitable for analysis.
///
/// Performs validation analogous to `calculus::validate_differentiable_field`:
/// 1. `field_val` must be `Value::Field { .. }`
/// 2. The `(source, lambda)` PAIR must be one of exactly two accepted shapes:
///    - `(Analytical | Composed, Value::Lambda { .. })` — the analytical /
///      derived path: the backing is a callable lambda, sampled pointwise.
///    - `(Sampled, Value::SampledField(_))` — a grid-backed tensor field, e.g.
///      the stress field `solve_elastic_static` returns
///      (`reify_eval::compute_targets::sampled_stress_field`).
/// 3. `codomain_type` must be a 3×3 matrix/tensor with scalar elements
///
/// Returns `Some((domain_type, codomain_type, element_dimension))` if all
/// checks pass, `None` otherwise.
///
/// The pair must be matched on BOTH halves, never on either alone.
/// `FieldSourceKind::Imported` also carries a `Value::SampledField` in its
/// lambda slot (`lib.rs`, the Imported sample-dispatch arm), so "any source
/// with a SampledField lambda" would admit it — but
/// [`field_reductions::project_sampled_tensor_windows`] requires
/// `source: Sampled` on the inner field, so an Imported-backed wrapper would
/// construct successfully and then silently reduce to `Value::Undef`.
/// Symmetrically, "Sampled with any lambda" would admit malformed fields whose
/// lambda slot holds no grid at all. The `calculus.rs` eager-lowering arms
/// (gradient / divergence / curl / laplacian) test both halves for the same
/// reason.
///
/// For a `Sampled` backing the wrapper this validation gates is LAZY: the
/// stride-9 window projection, the shared `reify_stdlib` per-window kernels and
/// the out-of-solid NaN skip all happen later, in
/// `field_reductions::project_sampled_tensor_windows`
/// (`crates/reify-expr/src/field_reductions.rs`), when the wrapper is reduced.
///
/// NOTE: Checks 1–2 duplicate the logic in `calculus::validate_differentiable_field`.
/// A shared base validator would eliminate this duplication, but `calculus.rs` is
/// outside the scope of this task's module locks. See reviewer suggestion #2.
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

    // Match the (source, lambda) PAIR — see the doc comment for why either
    // half alone is wrong (Imported is also SampledField-backed).
    if !matches!(
        (source, lambda.as_ref()),
        (
            FieldSourceKind::Analytical | FieldSourceKind::Composed,
            Value::Lambda { .. }
        ) | (FieldSourceKind::Sampled, Value::SampledField(_))
    ) {
        #[cfg(debug_assertions)]
        eprintln!(
            "[reify-expr] {op}: unsupported (source, lambda) pair: source {:?}, lambda {:?}",
            source, lambda
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
/// Returns `Type::dimensionless_scalar()` for dimensionless, `Type::Scalar { dimension }` otherwise.
fn scalar_type_for_dim(dim: DimensionVector) -> Type {
    if dim == DimensionVector::DIMENSIONLESS {
        Type::dimensionless_scalar()
    } else {
        Type::Scalar { dimension: dim }
    }
}

// ── Shared field-wrapping helper ───────────────────────────────────────────

/// Validate a tensor field and wrap it with the given `FieldSourceKind`.
///
/// Shared implementation for `compute_von_mises` and `compute_max_shear`.
/// Each differs only in the source kind variant.
///
/// The resulting field has `codomain_type = Scalar<element_dim>`.
fn wrap_tensor_field(field_val: &Value, op: &str, source_kind: FieldSourceKind) -> Value {
    let (domain_type, _codomain_type, elem_dim) = match validate_tensor_field(field_val, op) {
        Some(triple) => triple,
        None => return Value::Undef,
    };

    let result_codomain = scalar_type_for_dim(elem_dim);

    // Perf note: `field_val.clone()` copies the outer Value::Field struct; only the
    // inner lambda is O(1) via Arc::clone.  See compute_gradient in calculus.rs for
    // the full note on the Arc<Value> caller optimization.
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: source_kind,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Create a VonMises-wrapped field from a tensor field.
///
/// Given a `Field<D, Matrix3x3<Q>>`, returns a `Field<D, Scalar<Q>>` with
/// `source = FieldSourceKind::VonMises` and the original field stored in the
/// lambda slot.
pub(crate) fn compute_von_mises(field_val: &Value) -> Value {
    wrap_tensor_field(field_val, "von_mises", FieldSourceKind::VonMises)
}

/// Create a PrincipalStresses-wrapped field from a tensor field.
///
/// Given a `Field<D, Matrix3x3<Q>>`, returns a `Field<D, List<Scalar<Q>>>` with
/// `source = FieldSourceKind::PrincipalStresses`. Sampling produces a
/// `Value::List` of 3 scalars (the eigenvalues sorted descending), so the
/// codomain is `Type::List(Box<scalar_type>)` rather than a bare scalar.
pub(crate) fn compute_principal_stresses(field_val: &Value) -> Value {
    let (domain_type, _codomain_type, elem_dim) =
        match validate_tensor_field(field_val, "principal_stresses") {
            Some(triple) => triple,
            None => return Value::Undef,
        };

    let scalar_ty = scalar_type_for_dim(elem_dim);
    let result_codomain = Type::List(Box::new(scalar_ty));

    // Perf note: see wrap_tensor_field for note on Arc<Value> caller optimization.
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::PrincipalStresses,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Create a MaxShear-wrapped field from a tensor field.
///
/// Given a `Field<D, Matrix3x3<Q>>`, returns a `Field<D, Scalar<Q>>` with
/// `source = FieldSourceKind::MaxShear` and the original field stored in the
/// lambda slot.
pub(crate) fn compute_max_shear(field_val: &Value) -> Value {
    wrap_tensor_field(field_val, "max_shear", FieldSourceKind::MaxShear)
}

/// Create a SafetyFactor-wrapped field from a tensor field and yield strength.
///
/// Given a `Field<D, Matrix3x3<Q>>` and a yield strength value, returns a
/// `Field<D, Real>` with `source = FieldSourceKind::SafetyFactor`. The yield
/// strength is captured in the field value as a `Value::List` containing the
/// original field and the yield value in the lambda slot.
pub(crate) fn compute_safety_factor(field_val: &Value, yield_val: &Value) -> Value {
    let (domain_type, _codomain_type, _elem_dim) =
        match validate_tensor_field(field_val, "safety_factor") {
            Some(triple) => triple,
            None => return Value::Undef,
        };

    // Validate yield_val is numeric
    if yield_val.as_f64().is_none() {
        #[cfg(debug_assertions)]
        eprintln!(
            "[reify-expr] safety_factor: yield strength is not numeric: {:?}",
            yield_val
        );
        return Value::Undef;
    }

    // Safety factor is dimensionless (yield / von_mises cancels dimensions)
    let result_codomain = Type::dimensionless_scalar();

    // Store both the original field and yield value in the lambda slot as a List
    let captured = Value::List(vec![field_val.clone(), yield_val.clone()]);

    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::SafetyFactor,
        lambda: Arc::new(captured),
    }
}

// ── Sampling functions ──────────────────────────────────────────────────────

/// Sample a unary analysis field at a point.
///
/// Shared implementation for `sample_von_mises_at_point`,
/// `sample_principal_stresses_at_point`, and `sample_max_shear_at_point`.
/// Each differs only in the builtin name passed to `eval_builtin`.
fn sample_unary_analysis_at_point(
    inner_lambda: &Value,
    point: &Value,
    ctx: &EvalContext,
    builtin_name: &str,
) -> Value {
    let tensor = apply_lambda_with_point_unpacking(inner_lambda, point, ctx);
    if tensor.is_undef() {
        return Value::Undef;
    }
    reify_stdlib::eval_builtin(builtin_name, &[tensor])
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
    sample_unary_analysis_at_point(inner_lambda, point, ctx, "von_mises")
}

/// Sample a PrincipalStresses-wrapped field at a point.
pub(crate) fn sample_principal_stresses_at_point(
    inner_lambda: &Value,
    point: &Value,
    _codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    sample_unary_analysis_at_point(inner_lambda, point, ctx, "principal_stresses")
}

/// Sample a MaxShear-wrapped field at a point.
pub(crate) fn sample_max_shear_at_point(
    inner_lambda: &Value,
    point: &Value,
    _codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    sample_unary_analysis_at_point(inner_lambda, point, ctx, "max_shear")
}

/// Sample a SafetyFactor-wrapped field at a point.
///
/// The lambda slot contains a `Value::List([original_field, yield_val])`.
/// Extracts the original field, samples it at the point, then computes
/// safety_factor(tensor, yield_val) pointwise.
pub(crate) fn sample_safety_factor_at_point(
    captured: &Value,
    point: &Value,
    _codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    // Extract original field and yield value from the List
    let (field_val, yield_val) = match captured {
        Value::List(items) if items.len() == 2 => (&items[0], &items[1]),
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] safety_factor sample: expected List[field, yield], got {:?}",
                captured
            );
            return Value::Undef;
        }
    };

    // Extract the inner field's lambda
    let inner_lambda = match field_val {
        Value::Field { lambda, .. } => lambda.as_ref(),
        _ => return Value::Undef,
    };

    let tensor = apply_lambda_with_point_unpacking(inner_lambda, point, ctx);
    if tensor.is_undef() {
        return Value::Undef;
    }
    reify_stdlib::eval_builtin("safety_factor", &[tensor, yield_val.clone()])
}
