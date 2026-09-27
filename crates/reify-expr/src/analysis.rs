//! Field-level stress analysis wrappers.
//!
//! When analysis builtins (von_mises, principal_stresses, max_shear, safety_factor)
//! receive a Field<Point3, Tensor> argument, these functions create a new Field
//! that applies the analysis pointwise when sampled.
//!
//! Follows the same FieldSourceKind pattern as gradient/divergence/curl/laplacian
//! in calculus.rs: the original field is stored in the lambda slot, and the sample
//! handler in lib.rs forwards that whole slot to the `sample_*_at_point`
//! functions here, which dispatch on how the tensor field is backed.
//!
//! Two tensor-field backings are accepted, and the accepted set is expressed as
//! a `(source, lambda)` PAIR (see `tensor_backing`):
//!
//! - `(Analytical | Composed, Value::Lambda { .. })` — a callable backing,
//!   evaluated pointwise on sample.
//! - `(Sampled, Value::SampledField(_))` — a grid backing, e.g. the stress
//!   field `solve_elastic_static` returns. This mirrors the `calculus.rs`
//!   Sampled arms, which likewise test both halves of the pair.
//!
//! The wrapper stays lazy over a Sampled backing: nothing is projected here.
//!
//! # Reachability contract for a `Sampled` tensor backing
//!
//! Over a grid backing, a wrapper's value at a grid node is the shared
//! `reify_stdlib` kernel (`compute_von_mises_3x3`, `compute_max_shear_3x3`,
//! `compute_eigenvalues_3x3`; safety factor = yield / von Mises) applied to
//! that node's stride-9 row-major window. Two consumers read those node values:
//!
//! - Reductions, in `field_reductions.rs`: `project_sampled_tensor_windows`
//!   projects every window and hands the stride-1 result to
//!   `reduce_sampled_extremum`. Out-of-solid `f64::NAN` sentinel windows
//!   project to NaN and are dropped by the `is_finite()` gate in
//!   `argmax_argmin_index`; an all-non-finite buffer reduces to `Value::Undef`.
//! - Pointwise `sample()`, here (`sample_tensor_grid_at_point`): project every
//!   window FIRST, then interpolate the projected node values with the grid's
//!   own method. A sample whose interpolation stencil holds a non-finite node
//!   value (an out-of-solid sentinel window, or a hydrostatic node's infinite
//!   safety factor) is `Value::Undef`, and an out-of-bounds query is
//!   `Value::Undef` with one `W_FIELD_OUT_OF_BOUNDS` warning per backing field
//!   per session.
//!
//! The two consumers agree wherever the stencil is finite: at a grid node a
//! sample equals the value the reductions scan, and under Linear /
//! NearestNeighbor every sample lies within the wrapper's `[min, max]`. Each
//! per-kind kernel is applied in both places — the `sample_*_at_point`
//! closures here and the `project_*_sampled` functions in
//! `field_reductions.rs` — and
//! `every_wrapper_kind_samples_to_its_reduction_extrema_at_their_arg_coordinates`
//! (`tests/field_analysis_tests.rs`) pins the two to the same node values.
//!
//! Node equality has two exceptions:
//!
//! - A finite node beside a non-finite one. The interpolation stencil includes
//!   zero-weight corners — a Linear query AT a node still reads the neighbour
//!   across its cell, with weight 0 — and a NaN there poisons the sum anyway.
//!   Such a node samples as `Value::Undef` although the reductions count its
//!   value; on a real solve that is the solid's surface, where the peak often
//!   sits, so `sample(W, argmax(W))` can be `Value::Undef` there.
//! - A non-positive safety-factor yield. The reductions refuse it
//!   (`project_safety_factor_sampled` → `Value::Undef`), while `sample()`
//!   follows the pointwise `safety_factor` builtin and returns
//!   yield / von Mises.
//!
//! What that makes reachable for a `Field { source: Sampled }` tensor input:
//!
//! - `sample()` — all four wrapper kinds.
//! - `max` / `min` / `argmax` / `argmin` (1-arg) — all four wrapper kinds.
//! - The 2-arg bounded `max` / `min` / `argmax` / `argmin` — `VonMises` only.
//!
//! What is NOT reachable: the 2-arg bounded forms of `MaxShear`,
//! `SafetyFactor` and `PrincipalStresses` return `Value::Undef`. Pre-existing
//! and unrelated to the Sampled backing — the bounded dispatch simply has no
//! arm for them. Already documented at the head of `field_reductions.rs`.

use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_ir::{FieldSourceKind, SampledField, Value};

use super::sanitize::sanitize_value;
use super::{EvalContext, apply_lambda_with_point_unpacking, sampled};

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

/// How an analysis wrapper's tensor field is backed. Within this module
/// [`tensor_backing`] alone decides which backings are admitted, so a wrapper
/// that can be built ([`validate_tensor_field`]) is exactly one that can be
/// sampled (the four `sample_*_at_point` functions). The reductions re-match
/// the Grid pair on their own side, in
/// `field_reductions::project_sampled_tensor_windows`.
enum TensorBacking<'a> {
    /// A callable lambda: evaluated at the query point, then handed to the
    /// pointwise `reify_stdlib` builtin.
    Callable(&'a Value),
    /// A grid holding one stride-9 row-major 3×3 tensor window per node.
    Grid(&'a SampledField),
}

/// Classify `field_val`'s tensor backing from its `(source, lambda)` PAIR:
///
/// - `(Analytical | Composed, Value::Lambda { .. })` → [`TensorBacking::Callable`],
///   the analytical / derived path.
/// - `(Sampled, Value::SampledField(_))` → [`TensorBacking::Grid`], e.g. the
///   stress field `solve_elastic_static` returns
///   (`reify_eval::compute_targets::sampled_stress_field`).
/// - Anything else, a non-`Field` value included → `None`.
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
fn tensor_backing(field_val: &Value) -> Option<TensorBacking<'_>> {
    let Value::Field { source, lambda, .. } = field_val else {
        return None;
    };
    match (source, lambda.as_ref()) {
        (
            FieldSourceKind::Analytical | FieldSourceKind::Composed,
            callable @ Value::Lambda { .. },
        ) => Some(TensorBacking::Callable(callable)),
        (FieldSourceKind::Sampled, Value::SampledField(grid)) => Some(TensorBacking::Grid(grid)),
        _ => None,
    }
}

/// Validate that a value is a field with a tensor codomain suitable for analysis.
///
/// Performs validation analogous to `calculus::validate_differentiable_field`:
/// 1. `field_val` must be `Value::Field { .. }`
/// 2. The `(source, lambda)` PAIR must be an admitted [`TensorBacking`] — see
///    [`tensor_backing`].
/// 3. `codomain_type` must be a 3×3 matrix/tensor with scalar elements
///
/// Returns `Some((domain_type, codomain_type, element_dimension))` if all
/// checks pass, `None` otherwise.
///
/// For a `Sampled` backing the wrapper this validation gates is LAZY: the
/// stride-9 window projection, the shared `reify_stdlib` per-window kernels and
/// the non-finite handling all happen later — in
/// `field_reductions::project_sampled_tensor_windows`
/// (`crates/reify-expr/src/field_reductions.rs`) when the wrapper is reduced,
/// and in [`sample_tensor_grid_at_point`] when it is sampled.
///
/// NOTE: check 1 duplicates `calculus::validate_differentiable_field`; check 2
/// deliberately DIVERGES from it, so only check 1 is shared logic that a future
/// common base validator could hoist. `calculus.rs` still hard-rejects every
/// non-`Analytical | Composed` source inside its validator and handles the
/// `(Sampled, Value::SampledField)` pair EARLIER, as an eager lowering in
/// `compute_gradient` / `compute_divergence` / `compute_curl` /
/// `compute_laplacian` (a Sampled source with any other lambda slot falls
/// through to the validator and is still rejected). This wrapper instead admits
/// the pair here and stays LAZY — nothing is projected at construction time.
fn validate_tensor_field<'a>(
    field_val: &'a Value,
    op: &str,
) -> Option<(&'a Type, &'a Type, DimensionVector)> {
    let (domain_type, codomain_type) = match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            ..
        } => (domain_type, codomain_type),
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] {op}: argument is not a Field: {:?}",
                field_val
            );
            return None;
        }
    };

    // The (source, lambda) PAIR — `tensor_backing` owns the admitted set and
    // why either half alone is wrong (Imported is also SampledField-backed).
    if tensor_backing(field_val).is_none() {
        #[cfg(debug_assertions)]
        eprintln!(
            "[reify-expr] {op}: unsupported (source, lambda) pair: {:?}",
            field_val
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
/// `Value::List` of 3 scalars (the eigenvalues sorted ascending), so the
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
//
// Each `sample_*_at_point` takes the wrapper's whole lambda slot — the original
// tensor field, or `List[field, yield]` for safety_factor — and dispatches on
// its `TensorBacking`: a Callable backing through the pointwise builtin, a Grid
// backing through the same per-window kernel the reductions use.

/// Floats per node of a Grid tensor backing: one row-major 3×3 window.
const TENSOR_WINDOW_LEN: usize = 9;

/// The builtins' sanitize rule for a Grid-path sample: a non-finite `Real` or
/// `Scalar` becomes `Undef`, and a `List` becomes `Undef` AS A WHOLE when any
/// element is non-finite — as the `principal_stresses` builtin returns `Undef`
/// itself rather than a list of `Undef`s.
fn finite_or_undef(value: Value) -> Value {
    match value {
        Value::List(items) => {
            let items: Vec<Value> = items.into_iter().map(sanitize_value).collect();
            if items.iter().any(Value::is_undef) {
                Value::Undef
            } else {
                Value::List(items)
            }
        }
        other => sanitize_value(other),
    }
}

/// Sample a Grid tensor backing's per-node projection at `point`: every node's
/// window goes through `project`, the projected node values are interpolated
/// with the grid's own method and wrapped per the wrapper's `codomain_type`,
/// and the result is sanitized by [`finite_or_undef`].
fn sample_tensor_grid_at_point<const K: usize>(
    grid: &SampledField,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
    project: impl Fn(&[f64]) -> [f64; K],
) -> Value {
    finite_or_undef(sampled::sample_window_projection_at_point(
        grid,
        TENSOR_WINDOW_LEN,
        project,
        point,
        codomain_type,
        ctx,
    ))
}

/// Sample a unary analysis field over a Callable tensor backing at a point:
/// evaluate the backing lambda there, then apply the named builtin.
///
/// The Callable-backing path shared by `sample_von_mises_at_point`,
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

/// Sample a VonMises-wrapped field at a point: the von Mises stress of the
/// wrapped `tensor_field` there.
pub(crate) fn sample_von_mises_at_point(
    tensor_field: &Value,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    match tensor_backing(tensor_field) {
        Some(TensorBacking::Callable(lambda)) => {
            sample_unary_analysis_at_point(lambda, point, ctx, "von_mises")
        }
        Some(TensorBacking::Grid(grid)) => {
            sample_tensor_grid_at_point(grid, point, codomain_type, ctx, |w| {
                [reify_stdlib::compute_von_mises_3x3(w)]
            })
        }
        None => Value::Undef,
    }
}

/// Sample a PrincipalStresses-wrapped field at a point: the principal stresses
/// of the wrapped `tensor_field` there, as a `Value::List` in ascending order.
pub(crate) fn sample_principal_stresses_at_point(
    tensor_field: &Value,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    match tensor_backing(tensor_field) {
        Some(TensorBacking::Callable(lambda)) => {
            sample_unary_analysis_at_point(lambda, point, ctx, "principal_stresses")
        }
        Some(TensorBacking::Grid(grid)) => {
            sample_tensor_grid_at_point(grid, point, codomain_type, ctx, |w| {
                reify_stdlib::compute_eigenvalues_3x3(w).unwrap_or([f64::NAN; 3])
            })
        }
        None => Value::Undef,
    }
}

/// Sample a MaxShear-wrapped field at a point: the maximum shear stress of the
/// wrapped `tensor_field` there.
pub(crate) fn sample_max_shear_at_point(
    tensor_field: &Value,
    point: &Value,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    match tensor_backing(tensor_field) {
        Some(TensorBacking::Callable(lambda)) => {
            sample_unary_analysis_at_point(lambda, point, ctx, "max_shear")
        }
        Some(TensorBacking::Grid(grid)) => {
            sample_tensor_grid_at_point(grid, point, codomain_type, ctx, |w| {
                [reify_stdlib::compute_max_shear_3x3(w)]
            })
        }
        None => Value::Undef,
    }
}

/// Sample a SafetyFactor-wrapped field at a point.
///
/// The lambda slot contains a `Value::List([original_field, yield_val])`; the
/// result is yield / von Mises of the original field's tensor at the point.
/// Like the pointwise `safety_factor` builtin, neither path guards a
/// non-positive yield.
pub(crate) fn sample_safety_factor_at_point(
    captured: &Value,
    point: &Value,
    codomain_type: &Type,
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

    match tensor_backing(field_val) {
        Some(TensorBacking::Callable(lambda)) => {
            let tensor = apply_lambda_with_point_unpacking(lambda, point, ctx);
            if tensor.is_undef() {
                return Value::Undef;
            }
            reify_stdlib::eval_builtin("safety_factor", &[tensor, yield_val.clone()])
        }
        Some(TensorBacking::Grid(grid)) => {
            let Some(yield_si) = yield_val.as_f64() else {
                return Value::Undef;
            };
            sample_tensor_grid_at_point(grid, point, codomain_type, ctx, move |w| {
                [yield_si / reify_stdlib::compute_von_mises_3x3(w)]
            })
        }
        None => Value::Undef,
    }
}
