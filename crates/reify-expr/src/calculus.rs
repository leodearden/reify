use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_ir::{FieldSourceKind, Value};

use super::{EvalContext, apply_lambda};

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
        Type::Scalar { dimension } if *dimension == DimensionVector::DIMENSIONLESS => Type::dimensionless_scalar(),
        _ => ty.clone(),
    }
}

/// Wrap a raw `f64` result as the appropriate `Value` variant for a scalar-valued operator.
///
/// - `Type::Scalar { dimension }` → `Value::Scalar { si_value: value, dimension }`
/// - Any other type (typically `Type::dimensionless_scalar()`) → `Value::Real(value)`
///
/// The helper wraps blindly — it does **not** collapse a dimensionless `Type::Scalar`
/// to `Type::dimensionless_scalar()`.  This is intentional: all three callers (`compute_divergence`,
/// `compute_curl`, `compute_laplacian`) pre-normalise the codomain upstream via
/// [`dimensionless_fallback`] before stamping the `Field`, so by the time
/// `wrap_scalar_result` sees the codomain any dimensionless `Scalar` has already been
/// collapsed to `Type::dimensionless_scalar()`.  Re-normalising here would be redundant work and, worse,
/// would hide genuinely mis-stamped codomains from the `debug_assert` guards in
/// `compute_numerical_divergence_at_point`, `compute_numerical_curl_at_point`, and
/// `compute_numerical_laplacian_at_point`.
///
/// At the curl callsite, `component_codomain` is the unwrapped inner quantity of the
/// already-stamped `Type::vec3(result_component)` (extracted in
/// `compute_numerical_curl_at_point`), so it is already a scalar-compatible type.
///
/// See also: `Value::from_real_scalar` in `reify-types` — the *normalising* counterpart
/// that collapses `DIMENSIONLESS` → `Value::Real` at the call site.  That helper is used
/// for complex-component extraction and magnitude; `wrap_scalar_result` is the
/// non-normalising variant used by the differential operators.
fn wrap_scalar_result(value: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: value,
            dimension: *dimension,
        },
        _ => Value::Real(value),
    }
}

/// Validate that a value is a differentiable field and extract its types.
///
/// Performs the 3-part validation shared by all type-level differential operators:
/// 1. `field_val` must be `Value::Field { .. }` (otherwise logs a debug message and returns None)
/// 2. `source` must be `Analytical` or `Composed` (Gradient/Divergence/Curl/Laplacian fields
///    store the original field in the lambda slot, not a callable Lambda)
/// 3. `lambda` slot must be `Value::Lambda { .. }` (callable)
///
/// Returns `Some((domain_type, codomain_type))` if all checks pass, `None` otherwise.
/// The `op` string is used only in the debug `eprintln!` message to identify the operator.
fn validate_differentiable_field<'a>(
    field_val: &'a Value,
    op: &str,
) -> Option<(&'a Type, &'a Type)> {
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

    // Only Analytical and Composed fields support differential operators.
    // Gradient/Divergence/Curl/Laplacian fields store the original field in the
    // lambda slot — not a callable Value::Lambda — so numerical differentiation
    // via apply_lambda is not possible.
    if !matches!(
        source,
        FieldSourceKind::Analytical | FieldSourceKind::Composed
    ) {
        #[cfg(debug_assertions)]
        eprintln!("[reify-expr] {op}: unsupported source kind {:?}", source);
        return None;
    }

    // Lambda slot must be callable.
    if !matches!(lambda.as_ref(), Value::Lambda { .. }) {
        #[cfg(debug_assertions)]
        eprintln!(
            "[reify-expr] {op}: lambda slot is not callable: {:?}",
            lambda
        );
        return None;
    }

    Some((domain_type, codomain_type))
}

/// Compute the result-codomain type for the gradient operator.
///
/// Returns `Some(result_codomain)` when `domain_type` is scalar (`1D → n = 1`)
/// or `Point { n, scalar }` (`nD → n`); returns `None` for non-scalar/non-Point
/// domains.
///
/// For `n == 1` returns the gradient quantity directly (scalar derivative);
/// for `n > 1` wraps it in `Type::Vector { n, quantity: gradient_quantity }`.
///
/// For non-scalar codomains `dim_quotient_type` falls back to
/// `dimensionless_fallback(codomain_type)`, matching the original
/// Analytical/Composed path behavior (construction always succeeds for
/// valid domain; the codomain guard fires only at sampling time).
///
/// Used by both the Sampled eager-lower path (ε) and the existing
/// Analytical/Composed path, so the codomain computation is a single source
/// of truth and the D6 typing is guaranteed identical across both paths.
fn gradient_result_codomain(domain_type: &Type, codomain_type: &Type) -> Option<Type> {
    // Determine n from domain.
    let n = match domain_type {
        _ if scalar_dimension(domain_type).is_some() => 1,
        Type::Point { n, quantity } if scalar_dimension(quantity).is_some() => *n,
        _ => return None,
    };
    let gradient_quantity = dim_quotient_type(
        scalar_dimension(codomain_type),
        domain_dimension(domain_type),
        1,
        dimensionless_fallback(codomain_type),
    );
    Some(if n == 1 {
        gradient_quantity
    } else {
        Type::Vector {
            n,
            quantity: Box::new(gradient_quantity),
        }
    })
}

pub(crate) fn compute_gradient(field_val: &Value) -> Value {
    // ε: Sampled eager-lower — dispatch when the field carries a real SampledField payload.
    // A Sampled source with any other lambda slot (e.g. Value::Lambda, the malformed case in
    // gradient_tests.rs::gradient_sampled_field_returns_undef) falls through to
    // validate_differentiable_field, which hard-rejects Sampled → Value::Undef (unchanged).
    if let Value::Field {
        source: FieldSourceKind::Sampled,
        lambda,
        domain_type,
        codomain_type,
    } = field_val
        && let Value::SampledField(sf) = lambda.as_ref()
        && let Some(result_codomain) = gradient_result_codomain(domain_type, codomain_type)
    {
        let out = crate::sampled_fd::sampled_differential(
            sf,
            crate::sampled_fd::DifferentialOp::Gradient,
        );
        return Value::Field {
            domain_type: domain_type.clone(),
            codomain_type: result_codomain,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(out)),
        };
    }

    let (domain_type, codomain_type) = match validate_differentiable_field(field_val, "gradient") {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    let result_codomain = match gradient_result_codomain(domain_type, codomain_type) {
        Some(c) => c,
        None => return Value::Undef,
    };

    // Return a gradient field: source=Gradient, lambda slot stores the original field.
    // The sample handler detects lambda=Field + source=Gradient and dispatches to
    // compute_numerical_gradient_at_point.
    //
    // FIXME(perf): `field_val.clone()` copies the outer Value::Field struct
    // (domain_type, codomain_type, source); only the inner lambda field is O(1)
    // via Arc::clone.  A full O(1) wrap requires callers to pass Arc<Value> so
    // the entire source field can be ref-counted rather than cloned.  This needs
    // the evaluator's `evaluated_args: Vec<Value>` to become `Vec<Arc<Value>>`
    // — a broader architectural change (tracked by task 4551).
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::Gradient,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Compute the divergence of a vector field.
///
/// Returns a new scalar Field with `FieldSourceKind::Divergence` whose lambda slot stores
/// the original field. The sample handler dispatches to `compute_numerical_divergence_at_point`.
///
/// Validation:
/// - Argument must be an Analytical or Composed Field
/// - Domain must be `Point{n, scalar}` (n ≥ 1)
/// - Codomain must be `Vector{n, scalar}` with matching dimension n
///
/// **Scope note:** Sampled-field eager-lowering (analogous to the gradient/laplacian ε branch)
/// is deliberately deferred to task ζ (divergence/curl over Sampled vector input). Until then,
/// a `FieldSourceKind::Sampled` argument falls through `validate_differentiable_field` →
/// `Value::Undef`, which is the correct ζ-pending behaviour.
pub(crate) fn compute_divergence(field_val: &Value) -> Value {
    let (domain_type, codomain_type) = match validate_differentiable_field(field_val, "divergence")
    {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    // Domain must be a Point with scalar quantity
    let n = match domain_type {
        Type::Point { n, quantity } if scalar_dimension(quantity).is_some() => *n,
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] divergence: domain must be Point{{n}}, got {:?}",
                domain_type
            );
            return Value::Undef;
        }
    };

    // Codomain must be Vector{n, scalar}; capture the unwrapped quantity for later use.
    let (vec_n, codomain_quantity) = match codomain_type {
        Type::Vector { n, quantity } if scalar_dimension(quantity).is_some() => {
            (*n, quantity.as_ref())
        }
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] divergence: codomain must be Vector{{n}}, got {:?}",
                codomain_type
            );
            return Value::Undef;
        }
    };

    if vec_n != n {
        #[cfg(debug_assertions)]
        eprintln!("[reify-expr] divergence: domain dimension {n} ≠ codomain dimension {vec_n}");
        return Value::Undef;
    }

    // Compute result codomain type: codomain_component_dim / domain_dim.
    // Preserve-codomain fallback: downgrade dimensionless Scalar to Real, else keep component type.
    // codomain_quantity is the unwrapped Vector quantity — no outer Vector match needed.
    let result_codomain = dim_quotient_type(
        scalar_dimension(codomain_quantity),
        domain_dimension(domain_type),
        1,
        dimensionless_fallback(codomain_quantity),
    );

    // Result: scalar field with dimensionally-correct codomain.
    // FIXME(perf): see compute_gradient for note on Arc<Value> caller optimization. (task 4551)
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::Divergence,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Compute the curl of a 3D vector field.
///
/// Returns a new vector Field with `FieldSourceKind::Curl` whose lambda slot stores
/// the original field. The sample handler dispatches to `compute_numerical_curl_at_point`.
///
/// Validation:
/// - Argument must be an Analytical or Composed Field
/// - Domain must be `Point{3, scalar}`
/// - Codomain must be `Vector{3, scalar}`
///
/// **Scope note:** Sampled-field eager-lowering (analogous to the gradient/laplacian ε branch)
/// is deliberately deferred to task ζ (divergence/curl over Sampled vector input). Until then,
/// a `FieldSourceKind::Sampled` argument falls through `validate_differentiable_field` →
/// `Value::Undef`, which is the correct ζ-pending behaviour.
pub(crate) fn compute_curl(field_val: &Value) -> Value {
    let (domain_type, codomain_type) = match validate_differentiable_field(field_val, "curl") {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    // Domain must be Point{3, scalar}
    match domain_type {
        Type::Point { n: 3, quantity }
            if matches!(
                quantity.as_ref(),
                Type::Int | Type::Scalar { .. }
            ) => {}
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] curl: domain must be Point{{3}}, got {:?}",
                domain_type
            );
            return Value::Undef;
        }
    }

    // Codomain must be Vector{3, scalar}; capture the unwrapped quantity for dim propagation.
    let codomain_quantity = match codomain_type {
        Type::Vector { n: 3, quantity }
            if matches!(
                quantity.as_ref(),
                Type::Int | Type::Scalar { .. }
            ) =>
        {
            quantity.as_ref()
        }
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] curl: codomain must be Vector{{3}}, got {:?}",
                codomain_type
            );
            return Value::Undef;
        }
    };

    // Compute result component type: codomain_component_dim / domain_dim.
    // Same pattern as compute_divergence, but wrapped back in Vector{3, ...}.
    let result_component = dim_quotient_type(
        scalar_dimension(codomain_quantity),
        domain_dimension(domain_type),
        1,
        dimensionless_fallback(codomain_quantity),
    );

    // Result: vector field with dimensionally-correct codomain.
    // FIXME(perf): see compute_gradient for note on Arc<Value> caller optimization. (task 4551)
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: Type::vec3(result_component),
        source: FieldSourceKind::Curl,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Compute the result-codomain type for the laplacian operator.
///
/// Returns `Some(result_codomain)` when:
/// - `domain_type` is scalar (1D) or `Point { n, scalar }` (nD), **and**
/// - `codomain_type` is scalar (`Real`, `Int`, or `Scalar { .. }`).
///
/// The result codomain is always a scalar quotient: `codomain_dim / domain_dim²`
/// (i.e. `dim_quotient_type` with `domain_exponent = 2`).
///
/// Returns `None` for non-scalar domain or codomain, matching the `Value::Undef`
/// path in the existing validate-and-reject logic.
///
/// Used by both the Sampled eager-lower path (ε) and the existing
/// Analytical/Composed path to guarantee identical D6 typing across both paths.
fn laplacian_result_codomain(domain_type: &Type, codomain_type: &Type) -> Option<Type> {
    // Domain must be scalar or Point{n, scalar}.
    match domain_type {
        _ if scalar_dimension(domain_type).is_some() => {}
        Type::Point { quantity, .. } if scalar_dimension(quantity).is_some() => {}
        _ => return None,
    }
    // Codomain must be scalar.
    scalar_dimension(codomain_type)?;
    Some(dim_quotient_type(
        scalar_dimension(codomain_type),
        domain_dimension(domain_type),
        2,
        dimensionless_fallback(codomain_type),
    ))
}

/// Compute the Laplacian of a scalar field.
///
/// Returns a new scalar Field with `FieldSourceKind::Laplacian` whose lambda slot stores
/// the original field. The sample handler dispatches to `compute_numerical_laplacian_at_point`.
///
/// Validation:
/// - Argument must be an Analytical or Composed Field
/// - Domain must be scalar or `Point{n, scalar}`
/// - Codomain must be scalar (Real, Int, or Scalar)
///
/// For `FieldSourceKind::Sampled` with a real `Value::SampledField` lambda slot, the
/// operator eager-lowers (ε): dispatches `sampled_differential(Laplacian)` and returns a
/// `source:Sampled` output field indistinguishable to `sample()`/`max()` from any other
/// Sampled scalar field.  A Sampled source with any other lambda slot falls through to
/// the existing `validate_differentiable_field` reject → `Value::Undef`.
pub(crate) fn compute_laplacian(field_val: &Value) -> Value {
    // ε: Sampled eager-lower — dispatch when the field carries a real SampledField payload.
    if let Value::Field {
        source: FieldSourceKind::Sampled,
        lambda,
        domain_type,
        codomain_type,
    } = field_val
        && let Value::SampledField(sf) = lambda.as_ref()
        && let Some(result_codomain) = laplacian_result_codomain(domain_type, codomain_type)
    {
        let out = crate::sampled_fd::sampled_differential(
            sf,
            crate::sampled_fd::DifferentialOp::Laplacian,
        );
        return Value::Field {
            domain_type: domain_type.clone(),
            codomain_type: result_codomain,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(out)),
        };
    }

    let (domain_type, codomain_type) = match validate_differentiable_field(field_val, "laplacian") {
        Some(pair) => pair,
        None => return Value::Undef,
    };

    let result_codomain = match laplacian_result_codomain(domain_type, codomain_type) {
        Some(c) => c,
        None => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] laplacian: unsupported domain {:?} or codomain {:?}",
                domain_type, codomain_type
            );
            return Value::Undef;
        }
    };

    // Result: scalar field with dimensionally-correct codomain.
    // FIXME(perf): see compute_gradient for note on Arc<Value> caller optimization. (task 4551)
    Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: result_codomain,
        source: FieldSourceKind::Laplacian,
        lambda: Arc::new(field_val.clone()),
    }
}

/// Extract the explicit SI dimension from a domain type, if present.
///
/// Returns `Some(dimension)` only when the domain type is:
/// - `Type::Scalar { dimension }` — a dimensioned scalar
/// - `Type::Point { quantity: Type::Scalar { dimension }, .. }` — a multi-dimensional
///   domain with a dimensioned scalar quantity
///
/// Returns `None` for `Type::dimensionless_scalar()`, `Type::Int`, `Type::Point { quantity: Type::dimensionless_scalar()/Int }`,
/// and all other types.
///
/// This differs from [`domain_dimension`] which returns `Some(DIMENSIONLESS)` for `Real`/`Int`.
/// Here, `None` means "construct perturbed args as `Value::Real`, not `Value::Scalar`", which
/// is the calling convention used by all 4 numerical differential operator functions.
fn extract_explicit_domain_dim(domain_type: &Type) -> Option<DimensionVector> {
    match domain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(*dimension),
        Type::Point { quantity, .. } => match quantity.as_ref() {
            Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(*dimension),
            _ => None,
        },
        _ => None,
    }
}

/// Convert a slice of `Value` items to a `Vec<f64>`, returning `None` if any item is
/// non-numeric, NaN, or infinite.
///
/// Shared by [`extract_coords`] and [`extract_point_coords`] to avoid duplicating the
/// per-element finite-check loop.
fn items_to_f64_vec(items: &[Value]) -> Option<Vec<f64>> {
    if items.is_empty() {
        return None;
    }
    let mut v = Vec::with_capacity(items.len());
    for item in items {
        match item.as_f64() {
            Some(f) if f.is_finite() => v.push(f),
            _ => return None,
        }
    }
    Some(v)
}

/// Extract f64 coordinates from a `Value` — wide variant.
///
/// Accepts `Real`, `Int`, `Scalar`, `Point`, and `Vector`. Returns `None` for any
/// other variant, and for `Real`/`Scalar` values that are NaN or infinite.
///
/// Used by `compute_numerical_gradient_at_point` and `compute_numerical_laplacian_at_point`.
fn extract_coords(point: &Value) -> Option<Vec<f64>> {
    match point {
        Value::Real(r) if r.is_finite() => Some(vec![*r]),
        Value::Real(_) => None,                 // NaN or Inf
        Value::Int(i) => Some(vec![*i as f64]), // i64 can never be NaN/Inf
        Value::Scalar { si_value, .. } if si_value.is_finite() => Some(vec![*si_value]),
        Value::Scalar { .. } => None, // NaN or Inf
        Value::Point(items) | Value::Vector(items) => items_to_f64_vec(items),
        _ => None,
    }
}

/// Extract f64 coordinates from a `Value` — point/vector only variant.
///
/// Accepts only `Point` and `Vector`; returns `None` for any other variant and for empty
/// collections.
///
/// Used by `compute_numerical_divergence_at_point` (directly) and
/// `compute_numerical_curl_at_point` (with an additional `len == 3` guard).
fn extract_point_coords(point: &Value) -> Option<Vec<f64>> {
    match point {
        Value::Point(items) | Value::Vector(items) => items_to_f64_vec(items),
        _ => None,
    }
}

/// Construct a perturbed domain argument from an `f64` value and optional dimension.
///
/// Returns `Value::Scalar { si_value: val, dimension: dim }` when `domain_dim` is
/// `Some`, and `Value::Real(val)` when `domain_dim` is `None` (dimensionless domain).
///
/// This replaces the identical `make_arg` closure duplicated in all 4 numerical
/// differential operator functions.
fn make_domain_arg(val: f64, domain_dim: Option<DimensionVector>) -> Value {
    match domain_dim {
        Some(dim) => Value::Scalar {
            si_value: val,
            dimension: dim,
        },
        None => Value::Real(val),
    }
}

/// Detect whether a lambda uses the single-Point calling convention.
///
/// Returns `true` when `lambda` is a `Lambda` with exactly 1 parameter *and* `n > 1`,
/// meaning the caller should wrap perturbed coordinates in a `Value::Point` instead of
/// passing `n` individual scalar arguments.
///
/// Returns `false` for non-Lambda values or when `n <= 1` (single-coordinate domain).
///
/// See [`reify_types::Value::Field`] for the authoritative calling-convention contract this helper enforces.
fn detect_single_point_param(lambda: &Value, n: usize) -> bool {
    match lambda {
        Value::Lambda { params, .. } => params.len() == 1 && n > 1,
        _ => false,
    }
}

/// Initialise the two scratch buffers used by each axis iteration of the numerical
/// differential operator functions.
///
/// Returns `(work_args, work_point)`:
/// - `work_args`: an empty `Vec<Value>` with capacity 1 (single-point path) or `n`
///   (decomposed path).
/// - `work_point`: pre-populated with `make_domain_arg(coords[j], domain_dim)` for all `j`
///   when `single_point_param` is true; empty otherwise.
fn init_work_buffers(
    coords: &[f64],
    single_point_param: bool,
    domain_dim: Option<DimensionVector>,
) -> (Vec<Value>, Vec<Value>) {
    let n = coords.len();
    let args_capacity = if single_point_param { 1 } else { n };
    let work_args = Vec::with_capacity(args_capacity);
    let work_point = if single_point_param {
        coords
            .iter()
            .map(|&v| make_domain_arg(v, domain_dim))
            .collect()
    } else {
        Vec::new()
    };
    (work_args, work_point)
}

/// Evaluate the lambda at a single perturbed point and recover `work_point`.
///
/// This helper encapsulates the duplicated eval-and-recover sequence for the
/// `single_point_param` and decomposed paths in `compute_numerical_gradient_at_point`.
/// It is called twice per axis (f_plus and f_minus), so factoring it out eliminates
/// the verbatim duplication in the hot loop.
///
/// # Arguments
///
/// * `lambda` — The original field lambda to evaluate.
/// * `work_coords` — The already-perturbed coordinate slice (length n).
/// * `work_args` — Scratch `Vec<Value>` reused across calls; cleared and filled here.
/// * `work_point` — Scratch inner `Vec<Value>` for the `single_point_param` path;
///   transferred into `work_args` via `std::mem::take` and recovered after the call.
///   Must be empty in the decomposed path (enforced by `debug_assert`); ignored when
///   `single_point_param` is false.
/// * `single_point_param` — Whether the lambda expects one `Point` arg or n scalar args.
/// * `i` — The perturbed axis index (used to update `work_point[i]` efficiently).
/// * `n` — Total domain dimension; used for the post-recovery `debug_assert_eq!`.
/// * `make_arg` — Converts an `f64` coordinate to the appropriate `Value` variant
///   (e.g., `Value::Real` for dimensionless, `Value::Scalar { … }` for dimensioned).
/// * `ctx` — Evaluation context forwarded to `apply_lambda`.
///
/// # Invariant
///
/// After the call (when `single_point_param` is true), `work_point.len() == n`.
/// A `debug_assert_eq!` enforces this in debug builds.
///
/// # Performance
///
/// `std::mem::take` transfers ownership of `work_point`'s inner Vec into
/// `work_args` without any allocation, avoiding the per-axis `.collect()`
/// that would otherwise rebuild the inner Vec from scratch each call.
/// However, `apply_lambda` still clones the Vec once per evaluation when
/// populating its `eval_map` via `eval_map.insert(id.clone(), arg.clone())`,
/// so the savings are roughly halving inner-Vec allocations rather than
/// reducing to O(1) overall.
#[allow(clippy::too_many_arguments)]
fn eval_perturbed_point<F: Fn(f64) -> Value>(
    lambda: &Value,
    work_coords: &[f64],
    work_args: &mut Vec<Value>,
    work_point: &mut Vec<Value>,
    single_point_param: bool,
    i: usize,
    n: usize,
    make_arg: &F,
    ctx: &EvalContext,
) -> Value {
    debug_assert!(
        work_point.is_empty() || single_point_param,
        "work_point must be empty in decomposed path (single_point_param=false); \
         got {} element(s)",
        work_point.len(),
    );
    work_args.clear();
    if single_point_param {
        // Update only the perturbed element; transfer ownership via take.
        // Note: apply_lambda still clones the Vec once per eval when populating
        // its eval_map, so the savings are roughly halving inner-Vec allocations,
        // not reducing to O(1) overall.
        work_point[i] = make_arg(work_coords[i]);
        work_args.push(Value::Point(std::mem::take(work_point)));
    } else {
        work_args.extend(work_coords.iter().map(|&v| make_arg(v)));
    }
    let result = apply_lambda(lambda, work_args, ctx);
    // Recover work_point from work_args (single_point_param only).
    // apply_lambda borrows &[Value] and cannot mutate work_args, so pop() returns
    // exactly the Value::Point we pushed; any other outcome is a programming error.
    //
    // Structural precondition: this recovery MUST complete before the next
    // `work_args.clear()` executes (i.e., at the top of the next call to this
    // function). If a future refactor ever called `work_args.clear()` after the
    // `std::mem::take` push but before this pop, the `Value::Point` (and the
    // inner Vec it holds) would be dropped by `clear()`, destroying `work_point`
    // state and causing the next axis iteration to panic on `work_point[i] = …`
    // (empty index). The current layout — clear at the top, push/take, apply,
    // pop/restore — preserves this invariant.
    if single_point_param {
        match work_args.pop() {
            Some(Value::Point(inner)) => *work_point = inner,
            other => unreachable!("expected Value::Point after apply_lambda, got {other:?}"),
        }
        debug_assert_eq!(
            work_point.len(),
            n,
            "work_point must be recovered to n={n} elements after eval_perturbed_point; \
             got {} — apply_lambda may have changed the Point's inner Vec length",
            work_point.len(),
        );
    }
    result
}

/// Compute the numerical gradient of a field at a given point via central differences.
///
/// For each axis i, perturbs coordinate i by ±h where h = 1e-6 * max(|coord_i|, 1e-3),
/// evaluates the original lambda, and computes df/dx_i ≈ (f(x+h) - f(x-h)) / (2h).
///
/// Returns:
/// - Scalar (Real) for 1D fields
/// - Vector for nD fields
/// - Undef if any perturbation evaluation fails
pub(crate) fn compute_numerical_gradient_at_point(
    lambda: &Value,
    point: &Value,
    domain_type: &Type,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    // Decompose point into f64 coordinates.
    // Guard every extracted value with is_finite() — NaN/Inf coordinates
    // would produce NaN step sizes and silently corrupt gradient results.
    let Some(coords) = extract_coords(point) else {
        return Value::Undef;
    };

    let n = coords.len();
    // Defense in depth (task 3749): make the n>=1 contract independently true at the
    // gradient boundary, decoupling from extract_coords's empty-input None return;
    // prevents Value::Vector(vec![]) escaping to a Value::infer_type call that would
    // panic under the Shape-C debug_assert.
    if n == 0 {
        return Value::Undef;
    }

    // Determine if domain is dimensioned (for constructing perturbed args)
    let domain_dim = extract_explicit_domain_dim(domain_type);

    // Detect calling convention: single-Point param vs decomposed params.
    // If lambda has 1 param and n > 1, wrap perturbed coords in a Value::Point.
    // If lambda has n params, pass individual scalar values (current behavior).
    let single_point_param = detect_single_point_param(lambda, n);

    // Warn in debug builds when arity doesn't match and calling convention is
    // decomposed. An arity mismatch silently produces Undef via apply_lambda;
    // the warning surfaces the root cause during development.
    #[cfg(debug_assertions)]
    if let Value::Lambda { params, .. } = lambda
        && !single_point_param
        && params.len() != n
    {
        eprintln!(
            "[reify-expr] gradient: lambda has {} params but point has {} coords",
            params.len(),
            n
        );
    }

    // DESIGN DECISION: trust-the-declaration
    // result_dim is derived from the declared codomain_type, not from the runtime return
    // type of the lambda. This means a misconfigured codomain_type will silently produce
    // gradient values with the declared (wrong) dimension rather than the runtime dimension.
    // This is intentional: changing to runtime-driven dimensioning would require propagating
    // dimension metadata through all arithmetic operations, which is architecturally expensive
    // and error-prone. The two-tier validation strategy is:
    //   - Dimensionless domains (domain_dim = None): hard assert_eq! below catches mismatches;
    //     the lambda's return type is unambiguous (receives Real, must return Real or Scalar).
    //   - Dimensioned domains (domain_dim = Some): soft eprintln! warning below catches
    //     mismatches; hard assertions would produce false positives because arithmetic like
    //     `Real * Scalar<LENGTH>` naturally returns `Scalar<LENGTH>` regardless of the
    //     declared codomain.
    //
    // Extract per-component gradient dimension from codomain_type.
    // codomain_type is now the gradient field's codomain (already R/Q-divided by
    // compute_gradient), so no further division is needed here:
    //   - 1D field  → Scalar { dimension } or Real
    //   - nD field  → Vector { n, quantity: Length { dimension } } or Vector { n, quantity: Real }
    let result_dim = match codomain_type {
        Type::Vector { quantity, .. } => match quantity.as_ref() {
            Type::Scalar { dimension } => *dimension,
            _ => {
                debug_assert!(
                    matches!(quantity.as_ref(), Type::Int),
                    "[reify-expr] gradient: unexpected Vector quantity variant in result_dim catch-all: {:?}",
                    quantity
                );
                DimensionVector::DIMENSIONLESS
            }
        },
        Type::Scalar { dimension } => *dimension,
        _ => {
            debug_assert!(
                matches!(codomain_type, Type::Int),
                "[reify-expr] gradient: unexpected codomain_type variant in result_dim catch-all: {:?}",
                codomain_type
            );
            DimensionVector::DIMENSIONLESS
        }
    };

    // Hoist make_arg before the loop — it only captures domain_dim (Copy),
    // so constructing it once per axis would be redundant.
    let make_arg = |val: f64| make_domain_arg(val, domain_dim);

    // Take ownership of coords — no clone needed.
    // work_coords[i] equals the original coords[i] bit-for-bit at the start of
    // every axis iteration: at the bottom of the loop, work_coords[i] is restored
    // via direct assignment (`work_coords[i] = coord_i`), which is an exact
    // bit-identical restore of the value captured at the top of the iteration.
    // Invariant: work_point[j] == make_arg(work_coords[j]) at axis start.
    let mut work_coords = coords;
    let (mut work_args, mut work_point) =
        init_work_buffers(&work_coords, single_point_param, domain_dim);

    let mut gradient_components = Vec::with_capacity(n);

    for i in 0..n {
        // Read from work_coords — the original coords Vec was taken by ownership
        // above and is no longer accessible.  work_coords[i] holds the original
        // value bit-for-bit at this point: on the first iteration it comes
        // directly from the caller; on subsequent iterations the bottom of the
        // loop restores it via `work_coords[i] = coord_i` (direct assignment,
        // not arithmetic, so no ULP drift).
        let coord_i = work_coords[i];
        let h = 1e-6_f64 * coord_i.abs().max(1e-3);

        // Perturb forward (+h), evaluate, recover work_point.
        work_coords[i] += h;
        let f_plus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        // On the first axis, validate the lambda's runtime return dimension against the
        // declared codomain (via codomain_type). Two-tier strategy:
        //
        //   Tier 1 — hard assertion (dimensionless domains, domain_dim = None):
        //     make_arg passes Real to the lambda, so the return type is unambiguous.
        //     assert_eq! panics on mismatch — a clear misconfiguration.
        //
        //   Tier 2 — soft warning (dimensioned domains, domain_dim = Some):
        //     make_arg passes Scalar<domain_dim>. Operations like
        //     `Real * Scalar<LENGTH>` naturally return Scalar<LENGTH> regardless of
        //     the declared codomain — the codomain_type is metadata, not a runtime
        //     constraint. Hard assertions here produce false positives. A soft
        //     eprintln! warns for genuine mismatches without blocking execution.
        //
        // result_dim is the gradient's per-component dimension (codomain/domain).
        // f_plus comes from the original lambda, so its dimension is the ORIGINAL
        // codomain's dimension. Reconstruct:
        //   dimensionless domain: original_codomain = result_dim
        //   dimensioned domain:   original_codomain = result_dim * domain_dim
        #[cfg(debug_assertions)]
        if i == 0 && domain_dim.is_none() {
            let runtime_dim = f_plus.dimension();
            let expected_codomain_dim = result_dim;
            assert_eq!(
                runtime_dim, expected_codomain_dim,
                "codomain_type does not match runtime return dimension: \
                 declared codomain expects dimension {:?} but lambda returned {:?}",
                expected_codomain_dim, runtime_dim,
            );
        }
        // Tier 2: soft warning for dimensioned-domain mismatches.
        // original_codomain = result_dim * domain_dim; compare with f_plus dimension.
        #[cfg(debug_assertions)]
        if i == 0
            && let Some(dom_dim) = domain_dim
        {
            let expected_original_codomain = result_dim.mul(&dom_dim);
            let runtime_dim = f_plus.dimension();
            if runtime_dim != expected_original_codomain {
                eprintln!(
                    "[reify-expr] gradient: codomain_type mismatch (non-fatal): \
                     declared original codomain dimension {:?} but lambda returned {:?}",
                    expected_original_codomain, runtime_dim,
                );
            }
        }

        // Swing to backward (−h from original = −2h from current), evaluate, recover work_point.
        work_coords[i] -= 2.0 * h;
        let f_minus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        // Restore coord[i] to original value.
        // Use exact restore (direct assignment) instead of arithmetic
        // restore (+= h) to avoid IEEE 754 round-trip accumulation (~4 ULP
        // drift from x + h - 2h + h ≠ x in floating-point).
        work_coords[i] = coord_i;
        // Keep work_point in sync with the invariant.
        if single_point_param {
            work_point[i] = make_arg(coord_i);
        }

        // Extract numeric values, propagate Undef.
        // Guard with is_finite() — as_f64() returns Some(NaN) for
        // Value::Real(NaN) and Some(Inf) for Value::Real(Inf), so
        // the None check alone doesn't catch degenerate values.
        //
        // Ownership note: the three early returns below (fp non-finite, fm
        // non-finite, deriv non-finite) are all safe to issue mid-loop.
        // `work_coords` is a locally-owned Vec<f64> (taken from `coords` at
        // the top of this function) and `work_point` is a locally-owned
        // Vec<Value> (allocated by `init_work_buffers`). Both are dropped
        // with the function frame on any early return, so no perturbed state
        // can leak to the caller regardless of how far into the axis loop we
        // are when the non-finite value is detected. The coord[i] restore
        // above this block has already executed, but that only matters for
        // the happy path; early return discards everything cleanly.
        let fp = match f_plus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };
        let fm = match f_minus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };

        let deriv = (fp - fm) / (2.0 * h);
        // Guard the derivative: even finite fp/fm can produce
        // Inf via overflow (e.g., (MAX - (-MAX)) / small_h).
        if !deriv.is_finite() {
            return Value::Undef;
        }

        if result_dim != DimensionVector::DIMENSIONLESS {
            gradient_components.push(Value::Scalar {
                si_value: deriv,
                dimension: result_dim,
            });
        } else {
            gradient_components.push(Value::Real(deriv));
        }
    }

    // For n==1 the loop above always pushes exactly one component (or returns
    // early via Undef). The unwrap_or(Undef) fallback is unreachable in practice;
    // this assert documents and enforces that invariant in debug builds.
    debug_assert!(
        n != 1 || !gradient_components.is_empty(),
        "gradient loop must push exactly one component for n==1"
    );
    if n == 1 {
        gradient_components
            .into_iter()
            .next()
            .unwrap_or(Value::Undef)
    } else {
        Value::Vector(gradient_components)
    }
}

/// Compute the numerical divergence of a vector field at a given point via central differences.
///
/// For an n-dimensional vector field F: R^n → R^n, the divergence is:
///   div F(p) = Σ_i ∂Fi/∂xi ≈ Σ_i (F(p+h*ei)[i] - F(p-h*ei)[i]) / (2h)
///
/// `point` may be either `Value::Point` or `Value::Vector` — both share the same
/// structural representation and are extracted identically by `extract_point_coords`.
/// `eval_perturbed_point` re-wraps perturbed coordinates as `Value::Point` (in the
/// single-point-param path), so the caller's `Point`-vs-`Vector` choice does not leak
/// through to the lambda.
///
/// `codomain_type` is the divergence field's already-divided codomain (stamped by
/// `compute_divergence`). Follows the trust-the-declaration pattern established in
/// `compute_numerical_gradient_at_point`: no further division is performed here.
///
/// Returns:
/// - Scalar with the declared codomain dimension for dimensioned fields
/// - Real scalar for dimensionless fields
/// - Undef if any perturbation evaluation fails or the lambda returns non-vector
pub(crate) fn compute_numerical_divergence_at_point(
    lambda: &Value,
    point: &Value,
    domain_type: &Type,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    debug_assert!(
        matches!(codomain_type, Type::Scalar { .. }),
        "divergence/laplacian codomain must be scalar, got {:?}",
        codomain_type
    );
    // Accept both Point and Vector — they share structural representation.
    // eval_perturbed_point re-wraps as Value::Point, so the lambda always sees a Point.
    let Some(coords) = extract_point_coords(point) else {
        return Value::Undef;
    };

    let n = coords.len();
    // Defense in depth (task 3749): make the n>=1 contract independently true at the
    // divergence boundary, decoupling from extract_point_coords's empty-input None return;
    // prevents Value::Vector(vec![]) escaping to a Value::infer_type call that would
    // panic under the Shape-C debug_assert.
    if n == 0 {
        return Value::Undef;
    }

    let domain_dim = extract_explicit_domain_dim(domain_type);

    let single_point_param = detect_single_point_param(lambda, n);

    #[cfg(debug_assertions)]
    if let Value::Lambda { params, .. } = lambda
        && !single_point_param
        && params.len() != n
    {
        eprintln!(
            "[reify-expr] divergence: lambda has {} params but point has {} coords",
            params.len(),
            n
        );
    }

    let make_arg = |val: f64| make_domain_arg(val, domain_dim);

    let mut work_coords = coords;
    let (mut work_args, mut work_point) =
        init_work_buffers(&work_coords, single_point_param, domain_dim);

    let mut divergence = 0.0_f64;

    for i in 0..n {
        let coord_i = work_coords[i];
        let h = 1e-6_f64 * coord_i.abs().max(1e-3);

        work_coords[i] += h;
        let f_plus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        work_coords[i] -= 2.0 * h;
        let f_minus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        work_coords[i] = coord_i;
        if single_point_param {
            work_point[i] = make_arg(coord_i);
        }

        // Extract the i-th component from the vector output
        let fp_i = match &f_plus {
            Value::Vector(comps) if comps.len() > i => match comps[i].as_f64() {
                Some(v) if v.is_finite() => v,
                _ => return Value::Undef,
            },
            _ => return Value::Undef,
        };
        let fm_i = match &f_minus {
            Value::Vector(comps) if comps.len() > i => match comps[i].as_f64() {
                Some(v) if v.is_finite() => v,
                _ => return Value::Undef,
            },
            _ => return Value::Undef,
        };

        let deriv = (fp_i - fm_i) / (2.0 * h);
        if !deriv.is_finite() {
            return Value::Undef;
        }
        divergence += deriv;
    }

    if !divergence.is_finite() {
        return Value::Undef;
    }
    wrap_scalar_result(divergence, codomain_type)
}

/// Compute the numerical curl of a 3D vector field at a given point via central differences.
///
/// For a 3D vector field F: R^3 → R^3, the curl is:
///   curl F = (∂F3/∂y − ∂F2/∂z,  ∂F1/∂z − ∂F3/∂x,  ∂F2/∂x − ∂F1/∂y)
///
/// This is computed by building columns of the Jacobian via perturbation along each axis.
/// For each axis j, perturb to get F(p±h*ej), then for each component i compute:
///   J[i][j] = (F(p+h*ej)[i] − F(p−h*ej)[i]) / (2h)
///
/// `point` may be either `Value::Point` or `Value::Vector` — both share the same
/// structural representation and are extracted identically by `extract_point_coords`.
/// `eval_perturbed_point` re-wraps perturbed coordinates as `Value::Point` (in the
/// single-point-param path), so the caller's `Point`-vs-`Vector` choice does not leak
/// through to the lambda.
///
/// `codomain_type` is the curl field's already-divided codomain (stamped by
/// `compute_curl`). Follows the trust-the-declaration pattern established in
/// `compute_numerical_gradient_at_point`: no further division is performed here.
///
/// Returns:
/// - Vector3 of Real or Scalar components (dimensioned when codomain has a dimension)
/// - Undef if any evaluation fails
pub(crate) fn compute_numerical_curl_at_point(
    lambda: &Value,
    point: &Value,
    domain_type: &Type,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    debug_assert!(
        matches!(codomain_type, Type::Vector { .. }),
        "curl codomain must be vector, got {:?}",
        codomain_type
    );
    // Accept both Point and Vector — they share structural representation.
    // Only defined for 3D domains, so enforce len == 3 after extraction.
    // eval_perturbed_point re-wraps as Value::Point, so the lambda always sees a Point.
    let coords = match extract_point_coords(point) {
        Some(c) if c.len() == 3 => c,
        _ => return Value::Undef,
    };

    let n = 3;
    let domain_dim = extract_explicit_domain_dim(domain_type);

    // Extract the per-component codomain type from the already-divided codomain_type
    // (stamped by compute_curl). Curl produces Vector{3, component}, so unwrap the
    // quantity; for any other type fall back to codomain_type itself.
    let component_codomain: &Type = match codomain_type {
        Type::Vector { quantity, .. } => quantity.as_ref(),
        _ => codomain_type,
    };

    // n is constant 3 here, so the n > 1 condition in detect_single_point_param is
    // always satisfied; the result depends solely on params.len() == 1.
    let single_point_param = detect_single_point_param(lambda, n);

    let make_arg = |val: f64| make_domain_arg(val, domain_dim);

    let mut work_coords = coords;
    let (mut work_args, mut work_point) =
        init_work_buffers(&work_coords, single_point_param, domain_dim);

    // Jacobian columns: jac[j][i] = ∂Fi/∂xj
    let mut jac = [[0.0_f64; 3]; 3];

    for j in 0..n {
        let coord_j = work_coords[j];
        let h = 1e-6_f64 * coord_j.abs().max(1e-3);

        work_coords[j] += h;
        let f_plus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            j,
            n,
            &make_arg,
            ctx,
        );

        work_coords[j] -= 2.0 * h;
        let f_minus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            j,
            n,
            &make_arg,
            ctx,
        );

        work_coords[j] = coord_j;
        if single_point_param {
            work_point[j] = make_arg(coord_j);
        }

        // Extract all 3 components from the vector results
        let fp = match &f_plus {
            Value::Vector(comps) if comps.len() == 3 => {
                let mut arr = [0.0_f64; 3];
                for (k, c) in comps.iter().enumerate() {
                    match c.as_f64() {
                        Some(v) if v.is_finite() => arr[k] = v,
                        _ => return Value::Undef,
                    }
                }
                arr
            }
            _ => return Value::Undef,
        };
        let fm = match &f_minus {
            Value::Vector(comps) if comps.len() == 3 => {
                let mut arr = [0.0_f64; 3];
                for (k, c) in comps.iter().enumerate() {
                    match c.as_f64() {
                        Some(v) if v.is_finite() => arr[k] = v,
                        _ => return Value::Undef,
                    }
                }
                arr
            }
            _ => return Value::Undef,
        };

        // jac[j] = column j of Jacobian: ∂Fi/∂xj for i in 0..3
        for i in 0..3 {
            let d = (fp[i] - fm[i]) / (2.0 * h);
            if !d.is_finite() {
                return Value::Undef;
            }
            jac[j][i] = d;
        }
    }

    // curl = (J[1][2] - J[2][1], J[2][0] - J[0][2], J[0][1] - J[1][0])
    // where J[j][i] = ∂Fi/∂xj, so:
    //   curl_x = ∂F2/∂y - ∂F1/∂z → jac[1][2] - jac[2][1]  (∂F3/∂y - ∂F2/∂z in 1-indexed)
    //   curl_y = ∂F0/∂z - ∂F2/∂x → jac[2][0] - jac[0][2]
    //   curl_z = ∂F1/∂x - ∂F0/∂y → jac[0][1] - jac[1][0]
    let curl_x = jac[1][2] - jac[2][1];
    let curl_y = jac[2][0] - jac[0][2];
    let curl_z = jac[0][1] - jac[1][0];

    if !curl_x.is_finite() || !curl_y.is_finite() || !curl_z.is_finite() {
        return Value::Undef;
    }

    Value::Vector(vec![
        wrap_scalar_result(curl_x, component_codomain),
        wrap_scalar_result(curl_y, component_codomain),
        wrap_scalar_result(curl_z, component_codomain),
    ])
}

/// Compute the numerical Laplacian of a scalar field at a given point via central differences.
///
/// For a scalar field f: R^n → R, the Laplacian is:
///   Δf(p) = Σ_i ∂²f/∂xi² ≈ Σ_i (f(p+h*ei) − 2*f(p) + f(p−h*ei)) / h²
///
/// `point` may be `Value::Point`, `Value::Vector`, `Value::Real`, `Value::Int`, or
/// `Value::Scalar` — the wide `extract_coords` helper accepts all of these.
/// `eval_perturbed_point` re-wraps perturbed coordinates as `Value::Point` (in the
/// single-point-param path), so the caller's `Point`-vs-`Vector` choice does not leak
/// through to the lambda.
///
/// `codomain_type` is the Laplacian field's already-divided codomain (stamped by
/// `compute_laplacian`). Follows the trust-the-declaration pattern established in
/// `compute_numerical_gradient_at_point`: no further division is performed here.
///
/// Returns:
/// - Scalar with the declared codomain dimension for dimensioned fields
/// - Real scalar for dimensionless fields
/// - Undef if any evaluation fails
pub(crate) fn compute_numerical_laplacian_at_point(
    lambda: &Value,
    point: &Value,
    domain_type: &Type,
    codomain_type: &Type,
    ctx: &EvalContext,
) -> Value {
    debug_assert!(
        matches!(codomain_type, Type::Scalar { .. }),
        "divergence/laplacian codomain must be scalar, got {:?}",
        codomain_type
    );
    // Accept Point, Vector, Real, Int, and Scalar — the wide extract_coords variant.
    // eval_perturbed_point re-wraps as Value::Point, so the lambda always sees a Point.
    let Some(coords) = extract_coords(point) else {
        return Value::Undef;
    };

    let n = coords.len();
    // Defense in depth (task 3749): make the n>=1 contract independently true at the
    // laplacian boundary, decoupling from extract_coords's empty-input None return;
    // prevents Value::Vector(vec![]) escaping to a Value::infer_type call that would
    // panic under the Shape-C debug_assert.
    if n == 0 {
        return Value::Undef;
    }

    let domain_dim = extract_explicit_domain_dim(domain_type);

    let single_point_param = detect_single_point_param(lambda, n);

    #[cfg(debug_assertions)]
    if let Value::Lambda { params, .. } = lambda
        && !single_point_param
        && params.len() != n
    {
        eprintln!(
            "[reify-expr] laplacian: lambda has {} params but point has {} coords",
            params.len(),
            n
        );
    }

    let make_arg = |val: f64| make_domain_arg(val, domain_dim);

    // Evaluate f at the center point once
    let mut center_args: Vec<Value> = if single_point_param {
        let inner: Vec<Value> = coords.iter().map(|&v| make_arg(v)).collect();
        vec![Value::Point(inner)]
    } else {
        coords.iter().map(|&v| make_arg(v)).collect()
    };
    let f_center_val = apply_lambda(lambda, &center_args, ctx);
    let f_center = match f_center_val.as_f64() {
        Some(v) if v.is_finite() => v,
        _ => return Value::Undef,
    };
    // Reuse center_args as work_args; clear it after center eval
    center_args.clear();
    let mut work_args = center_args;

    // work_args is reused from center_args above; init_work_buffers provides work_point.
    let mut work_coords = coords;
    let (_, mut work_point) = init_work_buffers(&work_coords, single_point_param, domain_dim);

    let mut laplacian = 0.0_f64;

    for i in 0..n {
        let coord_i = work_coords[i];
        let h = 1e-6_f64 * coord_i.abs().max(1e-3);

        work_coords[i] += h;
        let f_plus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        work_coords[i] -= 2.0 * h;
        let f_minus = eval_perturbed_point(
            lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            single_point_param,
            i,
            n,
            &make_arg,
            ctx,
        );

        work_coords[i] = coord_i;
        if single_point_param {
            work_point[i] = make_arg(coord_i);
        }

        let fp = match f_plus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };
        let fm = match f_minus.as_f64() {
            Some(v) if v.is_finite() => v,
            _ => return Value::Undef,
        };

        let second_deriv = (fp - 2.0 * f_center + fm) / (h * h);
        if !second_deriv.is_finite() {
            return Value::Undef;
        }
        laplacian += second_deriv;
    }

    if !laplacian.is_finite() {
        return Value::Undef;
    }
    wrap_scalar_result(laplacian, codomain_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::ValueCellId;
    use reify_ir::{CompiledExpr, ValueMap};

    /// Helper: build a minimal 1-param identity lambda `|param_name: Real| param_name`
    /// with an empty capture map.
    ///
    /// Used by tests that need a syntactically valid `Value::Lambda` without caring about
    /// its body semantics.  The parameter name is configurable so tests asserting on specific
    /// parameter names (e.g. `"pt"`) can use their own.
    fn make_scalar_lambda(param_name: &str) -> Value {
        let id = ValueCellId::new("$lambda0.S", param_name);
        let body = CompiledExpr::value_ref(id.clone(), Type::dimensionless_scalar());
        Value::Lambda {
            params: vec![(param_name.to_string(), id)],
            body: Box::new(body),
            captures: ValueMap::new(),
        }
    }

    // --- scalar_dimension unit tests ---

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

    // --- domain_dimension unit tests ---

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

    // --- dim_quotient_type unit tests ---

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

    // --- gradient result_dim catch-all guard tests ---

    /// Verify that the outer catch-all in `compute_numerical_gradient_at_point`'s
    /// `result_dim` match panics (in debug mode) when handed an unexpected codomain_type.
    ///
    /// `Type::Bool` is not a valid gradient codomain — it is not `Scalar`, `Real`, or `Vector`.
    /// The current code silently maps it to DIMENSIONLESS; after the debug guard is added the
    /// test must panic with a message containing "unexpected codomain_type".
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unexpected codomain_type")]
    fn gradient_result_dim_unexpected_codomain_panics_in_debug() {
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_gradient_at_point(
            &lambda,
            &Value::Real(1.0),
            &Type::dimensionless_scalar(),
            &Type::Bool,
            &ctx,
        );
    }

    /// Verify that the inner catch-all (inside the `Type::Vector` arm) of
    /// `compute_numerical_gradient_at_point`'s `result_dim` match panics (in debug mode)
    /// when the Vector's quantity is an unexpected type.
    ///
    /// `Type::vec3(Type::Bool)` hits the outer arm (`Type::Vector { quantity, .. }`) and then
    /// `quantity = Type::Bool` hits the inner catch-all (`_ => DIMENSIONLESS`).  After the
    /// debug guard is added the test must panic with a message containing
    /// "unexpected Vector quantity".
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unexpected Vector quantity")]
    fn gradient_result_dim_unexpected_vector_quantity_panics_in_debug() {
        // Single-parameter lambda: |pt| pt (body irrelevant — panic fires before evaluation)
        let lambda = make_scalar_lambda("pt");

        // 3D point input — lambda has 1 param and n=3 so single_point_param=true
        let point = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);

        // domain: Point3<Real>, codomain: Vector3<Bool> — Bool is unexpected in the inner arm
        let domain_type = Type::point3(Type::dimensionless_scalar());
        let codomain_type = Type::vec3(Type::Bool);

        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_gradient_at_point(
            &lambda,
            &point,
            &domain_type,
            &codomain_type,
            &ctx,
        );
    }

    // --- divergence/laplacian codomain guard tests ---

    /// Verify that `compute_numerical_divergence_at_point` panics (in debug mode) when
    /// `codomain_type` is not `Type::dimensionless_scalar()` or `Type::Scalar`.
    ///
    /// `Type::Bool` is not a valid divergence codomain. The debug_assert fires before any
    /// coordinate extraction, so the lambda/point/domain_type need not be functionally correct.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "divergence/laplacian codomain must be scalar")]
    fn divergence_unexpected_codomain_panics_in_debug() {
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_divergence_at_point(
            &lambda,
            &Value::Real(1.0),
            &Type::dimensionless_scalar(),
            &Type::Bool,
            &ctx,
        );
    }

    /// Verify that `compute_numerical_divergence_at_point` panics (in debug mode) when
    /// `codomain_type` is `Type::vec3(Type::dimensionless_scalar())` — a non-scalar Vector type.
    ///
    /// `Type::vec3(Type::dimensionless_scalar())` is a `Vector` type: it is neither `Type::dimensionless_scalar()` nor
    /// `Type::Scalar`, so it trips the same `debug_assert` as `Type::Bool` but via the
    /// Vector shape of the non-scalar branch.  This complements the existing `Type::Bool`
    /// test by covering a structurally distinct kind of invalid codomain (a non-scalar
    /// wrapper type), guarding against future narrowing of the guard condition.
    ///
    /// The debug_assert fires before any coordinate extraction, so the
    /// lambda/point/domain_type need not be functionally correct.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "divergence/laplacian codomain must be scalar")]
    fn divergence_unexpected_vector_codomain_panics_in_debug() {
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_divergence_at_point(
            &lambda,
            &Value::Real(1.0),
            &Type::dimensionless_scalar(),
            &Type::vec3(Type::dimensionless_scalar()),
            &ctx,
        );
    }

    /// Verify that `compute_numerical_laplacian_at_point` panics (in debug mode) when
    /// `codomain_type` is not `Type::dimensionless_scalar()` or `Type::Scalar`.
    ///
    /// Same pattern as the divergence test above. The debug_assert fires before any
    /// coordinate extraction, so the lambda/point/domain_type need not be functionally correct.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "divergence/laplacian codomain must be scalar")]
    fn laplacian_unexpected_codomain_panics_in_debug() {
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_laplacian_at_point(
            &lambda,
            &Value::Real(1.0),
            &Type::dimensionless_scalar(),
            &Type::Bool,
            &ctx,
        );
    }

    /// Verify that `compute_numerical_curl_at_point` panics (in debug mode) when
    /// `codomain_type` is not `Type::Vector`.
    ///
    /// `Type::Bool` is not a valid curl codomain. The debug_assert fires before any
    /// coordinate extraction, so the lambda/point/domain_type need not be functionally correct.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "curl codomain must be vector")]
    fn curl_unexpected_codomain_panics_in_debug() {
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let _result = compute_numerical_curl_at_point(
            &lambda,
            &Value::Real(1.0),
            &Type::dimensionless_scalar(),
            &Type::Bool,
            &ctx,
        );
    }

    // --- eval_perturbed_point unit tests ---

    /// Verify the happy path for `eval_perturbed_point` with `single_point_param=true`.
    ///
    /// Constructs an identity lambda `|pt| pt`, calls `eval_perturbed_point` with a
    /// 1-element point, and verifies:
    /// (a) the result equals the expected `Value::Point` with the perturbed coordinate, and
    /// (b) `work_point` is correctly recovered with length `n=1` after the call.
    #[test]
    fn eval_perturbed_point_single_recovers_work_point() {
        // Identity lambda: |pt| pt
        let lambda = make_scalar_lambda("pt");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let work_coords = vec![1.0_f64];
        let mut work_args: Vec<Value> = Vec::new();
        // Pre-populated: work_point[j] == make_arg(work_coords[j]) initially.
        let mut work_point: Vec<Value> = vec![Value::Real(1.0)];
        let make_arg = |v: f64| Value::Real(v);

        let result = eval_perturbed_point(
            &lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            true, // single_point_param
            0,    // i — perturb axis 0
            1,    // n
            &make_arg,
            &ctx,
        );

        // The identity lambda returns the Point it received unchanged.
        assert_eq!(result, Value::Point(vec![Value::Real(1.0)]));
        // work_point must be recovered to its original length (n=1).
        assert_eq!(work_point.len(), 1);
        assert_eq!(work_point[0], Value::Real(1.0));
    }

    /// Verify the decomposed (non-`single_point_param`) path of `eval_perturbed_point`.
    ///
    /// Constructs an identity lambda `|x| x`, calls `eval_perturbed_point` with
    /// `single_point_param=false` and an empty `work_point`, and verifies:
    /// (a) the result equals `Value::Real(1.0)`, and
    /// (b) `work_point` remains empty after the call (the decomposed path never touches it).
    #[test]
    fn eval_perturbed_point_decomposed_returns_correct_result() {
        // Identity lambda: |x| x
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let work_coords = vec![1.0_f64];
        let mut work_args: Vec<Value> = Vec::new();
        // Decomposed path: work_point must be empty (doc contract).
        let mut work_point: Vec<Value> = Vec::new();
        let make_arg = |v: f64| Value::Real(v);

        let result = eval_perturbed_point(
            &lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            false, // single_point_param — decomposed path
            0,     // i (unused in decomposed path)
            1,     // n
            &make_arg,
            &ctx,
        );

        // The identity lambda returns the scalar it received.
        assert_eq!(result, Value::Real(1.0));
        // work_point is never touched by the decomposed path.
        assert!(work_point.is_empty());
    }

    /// Verify that calling `eval_perturbed_point` with `single_point_param=false` but a
    /// non-empty `work_point` panics in debug builds with a message containing
    /// "work_point must be empty".
    ///
    /// The `debug_assert` guard enforcing the doc contract fires before the lambda is invoked,
    /// so the test panics with the expected message.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "work_point must be empty")]
    fn eval_perturbed_point_decomposed_nonempty_work_point_panics_in_debug() {
        // Identity lambda: |x| x
        let lambda = make_scalar_lambda("x");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);

        let work_coords = vec![1.0_f64];
        let mut work_args: Vec<Value> = Vec::new();
        // Violates the doc contract: work_point must be empty in the decomposed path.
        let mut work_point: Vec<Value> = vec![Value::Real(99.0)];
        let make_arg = |v: f64| Value::Real(v);

        let _result = eval_perturbed_point(
            &lambda,
            &work_coords,
            &mut work_args,
            &mut work_point,
            false, // single_point_param — decomposed path
            0,
            1,
            &make_arg,
            &ctx,
        );
    }

    // --- validate_differentiable_field unit tests ---

    /// Helper: build a minimal valid Analytical Field with the given lambda.
    fn make_analytical_field(lambda: Value) -> Value {
        Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(lambda),
        }
    }

    #[test]
    fn validate_differentiable_field_analytical_with_lambda_returns_some() {
        let lambda = make_scalar_lambda("x");
        let field = make_analytical_field(lambda);
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_some());
        let (domain, codomain) = result.unwrap();
        assert_eq!(domain, &Type::dimensionless_scalar());
        assert_eq!(codomain, &Type::dimensionless_scalar());
    }

    #[test]
    fn validate_differentiable_field_composed_with_lambda_returns_some() {
        let lambda = make_scalar_lambda("x");
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Composed,
            lambda: Arc::new(lambda),
        };
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_some());
    }

    #[test]
    fn validate_differentiable_field_non_field_returns_none() {
        let result = validate_differentiable_field(&Value::Real(1.0), "test");
        assert!(result.is_none());
    }

    #[test]
    fn validate_differentiable_field_sampled_source_returns_none() {
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef),
        };
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_none());
    }

    #[test]
    fn validate_differentiable_field_gradient_source_returns_none() {
        // Gradient fields store the original field in the lambda slot, not a callable Lambda.
        let lambda = make_scalar_lambda("x");
        let original_field = make_analytical_field(lambda);
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Gradient,
            lambda: Arc::new(original_field),
        };
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_none());
    }

    #[test]
    fn validate_differentiable_field_imported_source_returns_none() {
        let lambda = make_scalar_lambda("x");
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Imported,
            lambda: Arc::new(lambda),
        };
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_none());
    }

    #[test]
    fn validate_differentiable_field_non_lambda_slot_returns_none() {
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = validate_differentiable_field(&field, "test");
        assert!(result.is_none());
    }

    // --- dimensionless_fallback unit tests ---

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

    // --- extract_explicit_domain_dim unit tests ---

    #[test]
    fn extract_explicit_domain_dim_scalar_length_returns_some_length() {
        assert_eq!(
            extract_explicit_domain_dim(&Type::length()),
            Some(DimensionVector::LENGTH)
        );
    }

    #[test]
    fn extract_explicit_domain_dim_point_scalar_mass_returns_some_mass() {
        let ty = Type::point3(Type::Scalar {
            dimension: DimensionVector::MASS,
        });
        assert_eq!(
            extract_explicit_domain_dim(&ty),
            Some(DimensionVector::MASS)
        );
    }

    #[test]
    fn extract_explicit_domain_dim_real_returns_none() {
        // Real is dimensionless — numerical functions pass Value::Real, not Value::Scalar
        assert_eq!(extract_explicit_domain_dim(&Type::dimensionless_scalar()), None);
    }

    #[test]
    fn extract_explicit_domain_dim_int_returns_none() {
        assert_eq!(extract_explicit_domain_dim(&Type::Int), None);
    }

    #[test]
    fn extract_explicit_domain_dim_point_real_returns_none() {
        assert_eq!(extract_explicit_domain_dim(&Type::point3(Type::dimensionless_scalar())), None);
    }

    #[test]
    fn extract_explicit_domain_dim_bool_returns_none() {
        assert_eq!(extract_explicit_domain_dim(&Type::Bool), None);
    }

    // --- make_domain_arg unit tests ---

    #[test]
    fn make_domain_arg_none_returns_real() {
        assert_eq!(make_domain_arg(2.71, None), Value::Real(2.71));
    }

    #[test]
    fn make_domain_arg_some_length_returns_scalar() {
        assert_eq!(
            make_domain_arg(2.5, Some(DimensionVector::LENGTH)),
            Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::LENGTH,
            }
        );
    }

    // --- detect_single_point_param unit tests ---

    /// Lambda with 1 param and n=3 → wraps in Point, returns true.
    #[test]
    fn detect_single_point_param_one_param_n_gt_1_returns_true() {
        let lambda = make_scalar_lambda("x"); // 1-param lambda
        assert!(detect_single_point_param(&lambda, 3));
    }

    /// Lambda with 3 params and n=3 → decomposed path, returns false.
    #[test]
    fn detect_single_point_param_three_params_n_3_returns_false() {
        let a_id = ValueCellId::new("$lambda0.S", "a");
        let b_id = ValueCellId::new("$lambda0.S", "b");
        let c_id = ValueCellId::new("$lambda0.S", "c");
        let body = CompiledExpr::value_ref(a_id.clone(), Type::dimensionless_scalar());
        let lambda = Value::Lambda {
            params: vec![
                ("a".to_string(), a_id),
                ("b".to_string(), b_id),
                ("c".to_string(), c_id),
            ],
            body: Box::new(body),
            captures: ValueMap::new(),
        };
        assert!(!detect_single_point_param(&lambda, 3));
    }

    /// Lambda with 1 param and n=1 → not "single point" (n not > 1), returns false.
    #[test]
    fn detect_single_point_param_one_param_n_1_returns_false() {
        let lambda = make_scalar_lambda("x"); // 1-param lambda
        assert!(!detect_single_point_param(&lambda, 1));
    }

    /// Non-Lambda value → always false.
    #[test]
    fn detect_single_point_param_non_lambda_returns_false() {
        assert!(!detect_single_point_param(&Value::Real(1.0), 3));
    }

    // --- extract_coords unit tests (wide: Real, Int, Scalar, Point, Vector) ---

    #[test]
    fn extract_coords_real_finite_returns_singleton() {
        assert_eq!(extract_coords(&Value::Real(1.5)), Some(vec![1.5]));
    }

    #[test]
    fn extract_coords_real_nan_returns_none() {
        assert_eq!(extract_coords(&Value::Real(f64::NAN)), None);
    }

    #[test]
    fn extract_coords_int_returns_singleton() {
        assert_eq!(extract_coords(&Value::Int(3)), Some(vec![3.0]));
    }

    #[test]
    fn extract_coords_scalar_finite_returns_si_value() {
        assert_eq!(
            extract_coords(&Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            }),
            Some(vec![2.0])
        );
    }

    #[test]
    fn extract_coords_scalar_inf_returns_none() {
        assert_eq!(
            extract_coords(&Value::Scalar {
                si_value: f64::INFINITY,
                dimension: DimensionVector::LENGTH,
            }),
            None
        );
    }

    #[test]
    fn extract_coords_point_returns_f64_vec() {
        let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert_eq!(extract_coords(&point), Some(vec![1.0, 2.0, 3.0]));
    }

    #[test]
    fn extract_coords_vector_with_nan_element_returns_none() {
        let vec_val = Value::Vector(vec![Value::Real(1.0), Value::Real(f64::NAN)]);
        assert_eq!(extract_coords(&vec_val), None);
    }

    #[test]
    fn extract_coords_bool_returns_none() {
        assert_eq!(extract_coords(&Value::Bool(true)), None);
    }

    /// Exercises the `as_f64() => None` path of `items_to_f64_vec` with a non-numeric
    /// element inside a Point. This complements
    /// `extract_coords_vector_with_nan_element_returns_none` (which covers the NaN branch
    /// via `is_finite` false) by hitting the non-numeric `_ => None` branch.
    #[test]
    fn extract_coords_point_with_non_numeric_element_returns_none() {
        let point = Value::Point(vec![Value::Real(1.0), Value::Bool(true)]);
        assert_eq!(extract_coords(&point), None);
    }

    // --- extract_point_coords unit tests (point/vector only) ---

    #[test]
    fn extract_point_coords_point_returns_f64_vec() {
        let point = Value::Point(vec![Value::Real(4.0), Value::Real(5.0)]);
        assert_eq!(extract_point_coords(&point), Some(vec![4.0, 5.0]));
    }

    #[test]
    fn extract_point_coords_vector_returns_f64_vec() {
        let vec_val = Value::Vector(vec![Value::Real(7.0), Value::Real(8.0), Value::Real(9.0)]);
        assert_eq!(extract_point_coords(&vec_val), Some(vec![7.0, 8.0, 9.0]));
    }

    /// Real → None (key difference from extract_coords which returns Some([r])).
    #[test]
    fn extract_point_coords_real_returns_none() {
        assert_eq!(extract_point_coords(&Value::Real(1.0)), None);
    }

    #[test]
    fn extract_point_coords_empty_point_returns_none() {
        assert_eq!(extract_point_coords(&Value::Point(vec![])), None);
    }

    // --- init_work_buffers unit tests ---

    /// single_point_param=false: work_args pre-allocated for n=3, work_point is empty.
    #[test]
    fn init_work_buffers_decomposed_allocates_n_capacity() {
        let coords = vec![1.0_f64, 2.0, 3.0];
        let (work_args, work_point) = init_work_buffers(&coords, false, None);
        assert_eq!(work_args.capacity(), 3);
        assert!(work_args.is_empty());
        assert!(work_point.is_empty());
    }

    /// single_point_param=true, domain_dim=None: work_args capacity=1,
    /// work_point has 3 Value::Real elements.
    #[test]
    fn init_work_buffers_single_point_dimensionless_populates_work_point() {
        let coords = vec![1.0_f64, 2.0, 3.0];
        let (work_args, work_point) = init_work_buffers(&coords, true, None);
        assert_eq!(work_args.capacity(), 1);
        assert!(work_args.is_empty());
        assert_eq!(work_point.len(), 3);
        assert_eq!(work_point[0], Value::Real(1.0));
        assert_eq!(work_point[1], Value::Real(2.0));
        assert_eq!(work_point[2], Value::Real(3.0));
    }

    /// single_point_param=true, domain_dim=Some(LENGTH): work_point elements are Scalar.
    #[test]
    fn init_work_buffers_single_point_dimensioned_populates_scalar_work_point() {
        let coords = vec![4.0_f64, 5.0];
        let (work_args, work_point) =
            init_work_buffers(&coords, true, Some(DimensionVector::LENGTH));
        assert_eq!(work_args.capacity(), 1);
        assert!(work_args.is_empty());
        assert_eq!(work_point.len(), 2);
        assert_eq!(
            work_point[0],
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH
            }
        );
        assert_eq!(
            work_point[1],
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH
            }
        );
    }

    // --- wrap_scalar_result unit tests ---

    /// Scalar codomain with LENGTH dimension wraps as Value::Scalar{si_value, LENGTH}.
    #[test]
    fn wrap_scalar_result_scalar_length_returns_scalar() {
        let result = wrap_scalar_result(
            42.0,
            &Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        assert_eq!(
            result,
            Value::Scalar {
                si_value: 42.0,
                dimension: DimensionVector::LENGTH,
            }
        );
    }

    /// Real codomain wraps as Value::Real.
    #[test]
    fn wrap_scalar_result_real_returns_real() {
        let result = wrap_scalar_result(42.0, &Type::dimensionless_scalar());
        assert_eq!(result, Value::Real(42.0));
    }


    // --- Integration tests for refactored differential operators ---
    //
    // These exercise `compute_gradient`, `compute_divergence`, `compute_curl`, and
    // `compute_laplacian` end-to-end on Analytical fields, asserting that the refactored
    // validation + fallback logic produces a `Value::Field` with the expected
    // `codomain_type` and `source`. They complement the fine-grained unit tests above
    // (which cover individual helpers) by guarding against semantic drift in the
    // composed behaviour of the public entry points.

    /// Build a minimal valid Analytical Field with explicit domain/codomain types.
    /// The lambda stored in the field is a trivial 1-param lambda — validation only
    /// checks for `Value::Lambda { .. }`, not arity.
    fn make_analytical_field_with_types(domain: Type, codomain: Type) -> Value {
        Value::Field {
            domain_type: domain,
            codomain_type: codomain,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(make_scalar_lambda("x")),
        }
    }

    /// compute_gradient on a 1D Real→Real Analytical field produces a Gradient field.
    /// In the 1D case, `result_codomain` is the scalar `gradient_quantity`, which for
    /// dimensionless codomain/domain is `Real` (via the dimensionless fallback).
    #[test]
    fn compute_gradient_analytical_real_to_real_returns_gradient_field() {
        let field = make_analytical_field_with_types(Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let result = compute_gradient(&field);
        match result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                lambda: _,
            } => {
                assert_eq!(source, FieldSourceKind::Gradient);
                assert_eq!(domain_type, Type::dimensionless_scalar());
                // 1D dimensionless domain + dimensionless codomain → Real.
                assert_eq!(codomain_type, Type::dimensionless_scalar());
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    /// compute_gradient on a 3D Point→Real Analytical field wraps the gradient quantity
    /// in `Vector{3, ...}`, exercising the nD branch of the refactored logic.
    #[test]
    fn compute_gradient_analytical_point3_to_real_returns_vector3_gradient() {
        let field = make_analytical_field_with_types(Type::point3(Type::dimensionless_scalar()), Type::dimensionless_scalar());
        let result = compute_gradient(&field);
        match result {
            Value::Field {
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(source, FieldSourceKind::Gradient);
                assert_eq!(codomain_type, Type::vec3(Type::dimensionless_scalar()));
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    /// compute_divergence on a 3D vector field produces a scalar Divergence field.
    #[test]
    fn compute_divergence_analytical_point3_to_vec3_returns_scalar_divergence_field() {
        let field =
            make_analytical_field_with_types(Type::point3(Type::dimensionless_scalar()), Type::vec3(Type::dimensionless_scalar()));
        let result = compute_divergence(&field);
        match result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(source, FieldSourceKind::Divergence);
                assert_eq!(domain_type, Type::point3(Type::dimensionless_scalar()));
                // Divergence of a dimensionless vector field collapses to scalar Real.
                assert_eq!(codomain_type, Type::dimensionless_scalar());
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    /// compute_curl on a 3D vector field produces a Vec3 Curl field.
    #[test]
    fn compute_curl_analytical_point3_to_vec3_returns_vec3_curl_field() {
        let field =
            make_analytical_field_with_types(Type::point3(Type::dimensionless_scalar()), Type::vec3(Type::dimensionless_scalar()));
        let result = compute_curl(&field);
        match result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(source, FieldSourceKind::Curl);
                assert_eq!(domain_type, Type::point3(Type::dimensionless_scalar()));
                // Curl of a dimensionless Vec3 field is still a dimensionless Vec3.
                assert_eq!(codomain_type, Type::vec3(Type::dimensionless_scalar()));
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    /// compute_laplacian on a 1D Real→Real Analytical field produces a scalar Laplacian
    /// field. Exercises the scalar-domain branch and the `domain_exponent=2` quotient.
    #[test]
    fn compute_laplacian_analytical_real_to_real_returns_scalar_laplacian_field() {
        let field = make_analytical_field_with_types(Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let result = compute_laplacian(&field);
        match result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(source, FieldSourceKind::Laplacian);
                assert_eq!(domain_type, Type::dimensionless_scalar());
                // Dimensionless domain² / dimensionless codomain → Real.
                assert_eq!(codomain_type, Type::dimensionless_scalar());
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    /// compute_laplacian on a 3D Point→Real Analytical field exercises the Point-domain
    /// branch.
    #[test]
    fn compute_laplacian_analytical_point3_to_real_returns_scalar_laplacian_field() {
        let field = make_analytical_field_with_types(Type::point3(Type::dimensionless_scalar()), Type::dimensionless_scalar());
        let result = compute_laplacian(&field);
        match result {
            Value::Field {
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(source, FieldSourceKind::Laplacian);
                assert_eq!(codomain_type, Type::dimensionless_scalar());
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    // --- n==0 guard regression tests ---

    /// Pin that `compute_numerical_gradient_at_point` returns `Value::Undef` without
    /// panicking when handed an empty `Value::Point(vec![])` (zero-dimension path).
    ///
    /// `Value::Undef` is produced via `extract_coords` returning `None` for an empty
    /// Point (items_to_f64_vec's empty-input → None contract).  The `if n == 0` guard
    /// at the function boundary (task 3749) provides independent defense-in-depth —
    /// the Undef contract holds regardless of which mechanism fires first.
    #[test]
    fn gradient_empty_point_returns_undef_no_panic() {
        let lambda = make_scalar_lambda("p");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        let result = compute_numerical_gradient_at_point(
            &lambda,
            &Value::Point(vec![]),
            &Type::dimensionless_scalar(),
            &Type::dimensionless_scalar(),
            &ctx,
        );
        assert_eq!(result, Value::Undef);
    }

    /// Pin that `compute_numerical_divergence_at_point` returns `Value::Undef` without
    /// panicking when handed an empty `Value::Point(vec![])` (zero-dimension path).
    ///
    /// Divergence codomain must be scalar (Type::dimensionless_scalar()) per the function's debug_assert.
    /// `Value::Undef` is produced via `extract_coords` returning `None` for the empty
    /// Point; the `if n == 0` guard at the function boundary (task 3749) provides
    /// independent defense-in-depth — the Undef contract holds regardless of which
    /// mechanism fires first.
    #[test]
    fn divergence_empty_point_returns_undef_no_panic() {
        let lambda = make_scalar_lambda("p");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        let result = compute_numerical_divergence_at_point(
            &lambda,
            &Value::Point(vec![]),
            &Type::dimensionless_scalar(),
            &Type::dimensionless_scalar(),
            &ctx,
        );
        assert_eq!(result, Value::Undef);
    }

    /// Pin that `compute_numerical_laplacian_at_point` returns `Value::Undef` without
    /// panicking when handed an empty `Value::Point(vec![])` (zero-dimension path).
    ///
    /// Laplacian codomain must be scalar (Type::dimensionless_scalar()) per the function's debug_assert.
    /// `Value::Undef` is produced via `extract_coords` returning `None` for the empty
    /// Point; the `if n == 0` guard at the function boundary (task 3749) provides
    /// independent defense-in-depth — the Undef contract holds regardless of which
    /// mechanism fires first.
    #[test]
    fn laplacian_empty_point_returns_undef_no_panic() {
        let lambda = make_scalar_lambda("p");
        let values = ValueMap::new();
        let ctx = EvalContext::simple(&values);
        let result = compute_numerical_laplacian_at_point(
            &lambda,
            &Value::Point(vec![]),
            &Type::dimensionless_scalar(),
            &Type::dimensionless_scalar(),
            &ctx,
        );
        assert_eq!(result, Value::Undef);
    }

    // ─── ε step-1: gradient eager-lower RED tests ─────────────────────────────

    /// Build a uniform Regular1D scalar SampledField (mirrors sampled_fd.rs make_1d_scalar).
    fn make_sampled_1d_scalar(n: usize, h: f64, f: impl Fn(f64) -> f64) -> reify_ir::SampledField {
        use std::sync::atomic::AtomicBool;
        let axis: Vec<f64> = (0..n).map(|i| i as f64 * h).collect();
        let data: Vec<f64> = axis.iter().map(|&x| f(x)).collect();
        reify_ir::SampledField {
            name: "test-1d".to_string(),
            kind: reify_ir::SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![(n - 1) as f64 * h],
            spacing: vec![h],
            axis_grids: vec![axis],
            interpolation: reify_ir::InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Build a `Value::Field { source: Sampled, lambda: Arc::new(Value::SampledField(sf)) }`.
    fn make_sampled_field_value(sf: reify_ir::SampledField, domain: Type, codomain: Type) -> Value {
        Value::Field {
            domain_type: domain,
            codomain_type: codomain,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(sf)),
        }
    }

    /// compute_gradient on a well-formed 1D sampled scalar field (f = 2x+3) returns
    /// a Sampled field whose data equals the exact gradient (2.0 everywhere, <1e-12)
    /// and whose codomain_type is the 1D gradient quotient (Real for dimensionless).
    #[test]
    fn gradient_sampled_1d_affine_returns_sampled_field_with_exact_gradient() {
        let sf = make_sampled_1d_scalar(5, 1.0, |x| 2.0 * x + 3.0);
        let n_nodes = sf.axis_grids[0].len(); // 5
        let field = make_sampled_field_value(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

        let result = compute_gradient(&field);

        // Must be a Value::Field with source=Sampled
        let (out_sf, codomain) = match &result {
            Value::Field {
                source,
                lambda,
                codomain_type,
                ..
            } => {
                assert_eq!(
                    *source,
                    FieldSourceKind::Sampled,
                    "gradient of Sampled field must return source=Sampled, got {:?}",
                    source
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => (sf, codomain_type),
                    other => panic!("lambda slot must be SampledField, got {:?}", other),
                }
            }
            other => panic!("expected Value::Field, got {:?}", other),
        };

        // 1D gradient: out_stride=1, data.len() == n_nodes
        assert_eq!(out_sf.data.len(), n_nodes, "1D gradient output must have stride-1 data");

        // Gradient of 2x+3 is 2.0 at every node
        for (g, &val) in out_sf.data.iter().enumerate() {
            assert!(
                (val - 2.0).abs() < 1e-12,
                "node {g}: gradient = {val}, expected 2.0"
            );
        }

        // codomain_type for dimensionless 1D field is Real
        assert_eq!(*codomain, Type::dimensionless_scalar(), "1D gradient codomain must be Real");
    }

    /// compute_gradient on a Sampled field whose lambda slot is a Value::Lambda (malformed)
    /// still returns Value::Undef.  This is the existing contract that must remain unchanged
    /// after step-2 adds the well-formed dispatch branch.
    ///
    /// (This is the same contract as gradient_tests.rs::gradient_sampled_field_returns_undef,
    /// reproduced here as a unit-level pin for the calculus.rs change.)
    #[test]
    fn gradient_sampled_malformed_lambda_slot_returns_undef() {
        // Sampled source but lambda slot is a Value::Lambda — not a SampledField.
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(make_scalar_lambda("x")),
        };
        assert_eq!(
            compute_gradient(&field),
            Value::Undef,
            "gradient of Sampled field with non-SampledField lambda must return Undef"
        );
    }

    // ─── ε step-3: laplacian eager-lower RED tests ────────────────────────────

    /// compute_laplacian on a well-formed 1D sampled scalar field (f = a*x² + b*x + c)
    /// returns a Sampled field whose data equals the exact constant 2nd derivative (2a,
    /// <1e-12 at every node incl. boundaries) and whose codomain_type is Real.
    #[test]
    fn laplacian_sampled_1d_quadratic_returns_sampled_field_with_exact_laplacian() {
        // f(x) = 3x² + 2x + 1  ⟹  ∇²f = 6 everywhere (2a = 2×3 = 6)
        let a = 3.0_f64;
        let sf = make_sampled_1d_scalar(5, 1.0, |x| a * x * x + 2.0 * x + 1.0);
        let n_nodes = sf.axis_grids[0].len(); // 5
        let field = make_sampled_field_value(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

        let result = compute_laplacian(&field);

        // Must be a Value::Field with source=Sampled
        let (out_sf, codomain) = match &result {
            Value::Field {
                source,
                lambda,
                codomain_type,
                ..
            } => {
                assert_eq!(
                    *source,
                    FieldSourceKind::Sampled,
                    "laplacian of Sampled field must return source=Sampled, got {:?}",
                    source
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => (sf, codomain_type),
                    other => panic!("lambda slot must be SampledField, got {:?}", other),
                }
            }
            other => panic!("expected Value::Field, got {:?}", other),
        };

        // 1D laplacian: out_stride=1, data.len() == n_nodes
        assert_eq!(out_sf.data.len(), n_nodes, "laplacian output must have stride-1 data");

        // Laplacian of a*x² is 2a = 6 at every node
        let expected_lap = 2.0 * a;
        for (g, &val) in out_sf.data.iter().enumerate() {
            assert!(
                (val - expected_lap).abs() < 1e-12,
                "node {g}: laplacian = {val}, expected {expected_lap}"
            );
        }

        // codomain_type for dimensionless 1D scalar field is Real
        assert_eq!(*codomain, Type::dimensionless_scalar(), "1D laplacian codomain must be Real");
    }

    /// compute_laplacian on a Sampled field whose lambda slot is a Value::Lambda (malformed)
    /// still returns Value::Undef.  The non-SampledField slot falls through to the unchanged
    /// validate path, preserving the existing behaviour.
    #[test]
    fn laplacian_sampled_malformed_lambda_slot_returns_undef() {
        let field = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(make_scalar_lambda("x")),
        };
        assert_eq!(
            compute_laplacian(&field),
            Value::Undef,
            "laplacian of Sampled field with non-SampledField lambda must return Undef"
        );
    }

    // ─── ζ step-1: divergence eager-lower RED tests ───────────────────────────

    /// Build a Regular3D stride-3 SampledField (interleaved node-major:
    /// data[g*3+0]=cx, data[g*3+1]=cy, data[g*3+2]=cz).
    /// Nodes are ordered x-major: for each x, for each y, for each z.
    fn make_3d_vector_sf(
        nx: usize,
        ny: usize,
        nz: usize,
        h: f64,
        f: impl Fn(f64, f64, f64) -> [f64; 3],
    ) -> reify_ir::SampledField {
        use std::sync::atomic::AtomicBool;
        let xs: Vec<f64> = (0..nx).map(|i| i as f64 * h).collect();
        let ys: Vec<f64> = (0..ny).map(|j| j as f64 * h).collect();
        let zs: Vec<f64> = (0..nz).map(|k| k as f64 * h).collect();
        let mut data = Vec::with_capacity(nx * ny * nz * 3);
        for &x in &xs {
            for &y in &ys {
                for &z in &zs {
                    let v = f(x, y, z);
                    data.push(v[0]);
                    data.push(v[1]);
                    data.push(v[2]);
                }
            }
        }
        reify_ir::SampledField {
            name: "test-3d-vec".to_string(),
            kind: reify_ir::SampledGridKind::Regular3D,
            bounds_min: vec![0.0, 0.0, 0.0],
            bounds_max: vec![(nx - 1) as f64 * h, (ny - 1) as f64 * h, (nz - 1) as f64 * h],
            spacing: vec![h, h, h],
            axis_grids: vec![xs, ys, zs],
            interpolation: reify_ir::InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// compute_divergence on a well-formed 3D Sampled vector field F=(x, 2y, 3z)
    /// returns a Sampled field whose data equals the exact divergence (6.0 everywhere,
    /// <1e-12) and whose codomain_type is Real (dimensionless strain quotient).
    ///
    /// Numeric premise: div F = ∂Fx/∂x + ∂Fy/∂y + ∂Fz/∂z = 1 + 2 + 3 = 6.
    /// FD is algebraically exact for degree-1 polys (truncation ∝ 2nd derivative = 0).
    /// Tolerance 1e-12 = δ-contract floor (PRD §6/D4).
    ///
    /// **RED**: compute_divergence currently returns Value::Undef for Sampled source;
    /// this test drives the step-2 Sampled eager-lower branch.
    #[test]
    fn divergence_sampled_3d_affine_returns_sampled_field_with_exact_divergence() {
        let n = 4_usize;
        let sf = make_3d_vector_sf(n, n, n, 1.0, |x, y, z| [x, 2.0 * y, 3.0 * z]);
        let grid_count = n * n * n;
        let domain = Type::Point { n: 3, quantity: Box::new(Type::dimensionless_scalar()) };
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let field = make_sampled_field_value(sf, domain, codomain);

        let result = compute_divergence(&field);

        // Must be Value::Field with source=Sampled
        let (out_sf, result_codomain) = match &result {
            Value::Field {
                source,
                lambda,
                codomain_type,
                ..
            } => {
                assert_eq!(
                    *source,
                    FieldSourceKind::Sampled,
                    "divergence of Sampled vector field must return source=Sampled, got {:?}",
                    source
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => (sf, codomain_type),
                    other => panic!("lambda slot must be SampledField, got {:?}", other),
                }
            }
            other => panic!("expected Value::Field, got {:?}", other),
        };

        // Divergence → scalar output: stride-1, data.len() == grid_count
        assert_eq!(
            out_sf.data.len(),
            grid_count,
            "divergence output must have stride-1 data (len=grid_count={grid_count}), got {}",
            out_sf.data.len()
        );

        // Every node must be within 1e-12 of 6.0
        for (g, &val) in out_sf.data.iter().enumerate() {
            assert!(
                (val - 6.0).abs() < 1e-12,
                "node {g}: divergence = {val}, expected 6.0 (error {})",
                (val - 6.0).abs()
            );
        }

        // codomain_type for dimensionless 3D vector field is Real (dimensionless strain)
        assert_eq!(
            *result_codomain,
            Type::dimensionless_scalar(),
            "divergence codomain for dimensionless vector field must be Real, got {:?}",
            result_codomain
        );
    }

    /// compute_divergence on a Sampled field whose lambda slot is a Value::Lambda (malformed)
    /// still returns Value::Undef.  The fallthrough to validate_differentiable_field → Undef
    /// must hold both before and after step-2 adds the well-formed dispatch branch.
    #[test]
    fn divergence_sampled_malformed_lambda_slot_returns_undef() {
        let domain = Type::Point { n: 3, quantity: Box::new(Type::dimensionless_scalar()) };
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        let field = Value::Field {
            domain_type: domain,
            codomain_type: codomain,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(make_scalar_lambda("p")),
        };
        assert_eq!(
            compute_divergence(&field),
            Value::Undef,
            "divergence of Sampled field with non-SampledField lambda must return Undef"
        );
    }
}
