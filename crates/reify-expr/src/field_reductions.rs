//! Eager Field reductions: `max`, `min`, `argmax`, `argmin`.
//!
//! Architecturally distinct from `analysis.rs` (which produces lazy
//! field-wrapper Values via `FieldSourceKind::VonMises`/etc.):
//! these reductions **collapse** a field to a single scalar (or a
//! single point) immediately. The dispatch arms in `lib.rs` invoke
//! these helpers on a `Value::Field` argument and return the resulting
//! `Value` directly to the caller.
//!
//! # Source-kind support (staged per task description)
//!
//! Only `FieldSourceKind::Sampled` is fully implemented in v0.3.
//! All other source kinds (`Analytical`, `Composed`, `Imported`, and
//! the derived wrappers `Gradient`/`Divergence`/`Curl`/`Laplacian`/
//! `VonMises`/`PrincipalStresses`/`MaxShear`/`SafetyFactor`) return
//! `Value::Undef`.
//!
//! The deferred path requires either numerical optimisation over an
//! analytical lambda's bounded domain (Nelder-Mead / golden-section /
//! coordinate descent) or sampled-subfield reduction for derived
//! wrappers — see `docs/prds/v0_3/structural-analysis-fea.md` task #6.
//! The PRD task description authorises this staging:
//! "Implementation can be staged — `sampled` first (FEA produces
//! sampled fields)."
//!
//! # NaN / empty data semantics
//!
//! `SampledField.data` is `Vec<f64>` and the elaborator
//! (`engine_eval::build_sampled_field`) does not reject NaN data values
//! — only NaN/inf spacings and degenerate axis grids. A reduction
//! must therefore handle NaN-bearing data: skip non-finite values
//! when reducing; if all values are non-finite (or `data.is_empty()`),
//! return `Value::Undef`. This matches the `safety_factor` poison
//! convention and the `sanitize_value` discipline elsewhere in stdlib.

use reify_types::{FieldSourceKind, SampledField, Type, Value};

/// Compute `max(field)` — return the maximum codomain value of a
/// `Sampled`-source field, wrapped per the field's `codomain_type`.
///
/// Other source kinds return `Value::Undef` (deferred — see module
/// doc-comment for the staging rationale).
pub(crate) fn compute_max(field_val: &Value) -> Value {
    compute_extremum(field_val, false)
}

/// Compute `min(field)` — return the minimum codomain value of a
/// `Sampled`-source field, wrapped per the field's `codomain_type`.
///
/// Other source kinds return `Value::Undef` (deferred — see module
/// doc-comment for the staging rationale).
pub(crate) fn compute_min(field_val: &Value) -> Value {
    compute_extremum(field_val, true)
}

/// Compute `argmax(field)` — return the domain coord at which a
/// `Sampled`-source field attains its maximum value, wrapped per the
/// field's `domain_type`.
///
/// Tie-break: lowest linear index wins (the `total_cmp` reduce keeps
/// the first-seen extremum on equal values).
///
/// Other source kinds return `Value::Undef` (deferred).
pub(crate) fn compute_argmax(field_val: &Value) -> Value {
    compute_argextremum(field_val, false)
}

/// Compute `argmin(field)` — return the domain coord at which a
/// `Sampled`-source field attains its minimum value, wrapped per the
/// field's `domain_type`.
///
/// Tie-break: lowest linear index wins (mirrors `compute_argmax`).
///
/// Other source kinds return `Value::Undef` (deferred).
pub(crate) fn compute_argmin(field_val: &Value) -> Value {
    compute_argextremum(field_val, true)
}

/// Shared body for `compute_max` / `compute_min`. `find_min == true`
/// selects the minimum, `false` selects the maximum.
fn compute_extremum(field_val: &Value, find_min: bool) -> Value {
    let (codomain_type, source, lambda) = match field_val {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => (codomain_type, source, lambda),
        _ => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => reduce_sampled_extremum(sf, codomain_type, find_min),
            // Defensive: a Sampled source must carry a SampledField in its
            // lambda slot. Anything else is a malformed runtime value;
            // return Undef rather than panicking.
            _ => Value::Undef,
        },
        // TODO(future): numerical optimisation over Analytical/Composed lambda
        // domains (Nelder-Mead / golden-section / coordinate descent); iterate
        // over Sampled subfield for derived (Gradient, VonMises, MaxShear, ...)
        // wrappers — see PRD docs/prds/v0_3/structural-analysis-fea.md task #6
        // (deferred per task description's "Implementation can be staged —
        // sampled first"). Imported fields carry Value::Undef in their lambda
        // slot and cannot be reduced without a backing data buffer.
        //
        // Pinned by the step-15 negative-path tests:
        // - all_reductions_on_analytical_field_return_undef
        // - all_reductions_on_composed_field_return_undef
        // - all_reductions_on_imported_field_return_undef
        // - all_reductions_on_derived_field_return_undef
        _ => Value::Undef,
    }
}

/// Reduce a `SampledField`'s data buffer to a single extremum value,
/// wrapped per the codomain type.
///
/// `find_min == false` → maximum; `find_min == true` → minimum.
///
/// # NaN / non-finite / empty handling
///
/// Non-finite values (NaN and ±∞) are skipped via `is_finite()` —
/// stricter than `!is_nan()` and matching the `sanitize_value`
/// discipline in `crates/reify-stdlib/src/helpers.rs`. The fold tracks
/// `Option<f64>` so that an empty data buffer or all-non-finite buffer
/// returns `Value::Undef` (no extremum exists).
///
/// Pinned by `max_sampled_with_nan_skips_nan_values`,
/// `all_reductions_sampled_all_nan_returns_undef`, and
/// `all_reductions_sampled_empty_data_returns_undef` in
/// `tests/field_reductions_tests.rs` (step-17 of plan 2913).
fn reduce_sampled_extremum(sf: &SampledField, codomain_type: &Type, find_min: bool) -> Value {
    let extremum = sf.data.iter().copied().filter(|x| x.is_finite()).fold(
        None::<f64>,
        |best, candidate| match best {
            None => Some(candidate),
            Some(b) => {
                let cmp = candidate.total_cmp(&b);
                let take = if find_min {
                    cmp.is_lt()
                } else {
                    cmp.is_gt()
                };
                Some(if take { candidate } else { b })
            }
        },
    );

    match extremum {
        Some(v) => wrap_codomain(v, codomain_type),
        None => Value::Undef,
    }
}

/// Shared body for `compute_argmax` / `compute_argmin`. Locates the
/// extremum's linear index in the Sampled data buffer, decomposes it
/// into per-axis coords via `axis_grids`, and wraps the result per
/// the field's `domain_type`.
fn compute_argextremum(field_val: &Value, find_min: bool) -> Value {
    let (domain_type, source, lambda) = match field_val {
        Value::Field {
            domain_type,
            source,
            lambda,
            ..
        } => (domain_type, source, lambda),
        _ => return Value::Undef,
    };

    match source {
        FieldSourceKind::Sampled => match lambda.as_ref() {
            Value::SampledField(sf) => match argmax_argmin_index(&sf.data, find_min) {
                Some(linear) => arg_coord_from_index(sf, linear, domain_type),
                None => Value::Undef,
            },
            // Defensive: see compute_extremum's matching defensive arm.
            _ => Value::Undef,
        },
        // TODO(future): see compute_extremum for the full deferred-path note.
        // Same staging rationale applies — argmax/argmin over a non-Sampled
        // source would require numerical optimisation, not yet in scope.
        // Pinned by the same step-15 tests as compute_extremum.
        _ => Value::Undef,
    }
}

/// Locate the linear index of the maximum (or minimum, when
/// `find_min`) finite value in `data`. Uses `total_cmp` for the
/// IEEE 754 totalOrder consistency (matches `Value::Real`/`Scalar`
/// `Ord` impls).
///
/// Non-finite values (NaN and ±∞) are skipped via `is_finite()` —
/// stricter than `!is_nan()` and matching the `sanitize_value`
/// discipline elsewhere. Returns `None` when `data` is empty or
/// contains no finite values. Tie-break: lowest linear index wins
/// (strict `<`/`>` rather than `<=`/`>=` keeps the first-seen
/// extremum on equal values).
///
/// Pinned by `argmax_sampled_with_nan_skips_nan_values` and the
/// all-NaN/empty-data branches of
/// `all_reductions_sampled_all_nan_returns_undef` /
/// `all_reductions_sampled_empty_data_returns_undef` in
/// `tests/field_reductions_tests.rs` (step-17 of plan 2913).
fn argmax_argmin_index(data: &[f64], find_min: bool) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &v) in data.iter().enumerate() {
        if !v.is_finite() {
            continue;
        }
        match best {
            None => best = Some((i, v)),
            Some((_, b)) => {
                let cmp = v.total_cmp(&b);
                let take = if find_min {
                    cmp.is_lt()
                } else {
                    cmp.is_gt()
                };
                if take {
                    best = Some((i, v));
                }
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Look up the per-axis SI coords at `linear_index` in `sf.axis_grids`
/// and wrap them per `domain_type`.
///
/// The N-D loop below is fully generic across 1/2/3 axes — the
/// `SampledGridKind` invariant (`Regular1D`/`Regular2D`/`Regular3D`)
/// is reinforced by the `debug_assert!` here and in `decompose_index`
/// below. Pinned by the 1-D / 2-D / 3-D test suites in
/// `tests/field_reductions_tests.rs` (`argmax|argmin_sampled_field_*d_*`).
fn arg_coord_from_index(sf: &SampledField, linear_index: usize, domain_type: &Type) -> Value {
    debug_assert!(
        matches!(sf.axis_grids.len(), 1 | 2 | 3),
        "SampledGridKind invariant: 1/2/3 axes only, got {}",
        sf.axis_grids.len()
    );
    // Decompose the linear index into per-axis indices (axis-0 outermost,
    // row-major).
    let axis_lengths: Vec<usize> = sf.axis_grids.iter().map(|g| g.len()).collect();
    let per_axis = decompose_index(linear_index, &axis_lengths);

    // Look up SI coords from axis_grids.
    let coords_si: Vec<f64> = per_axis
        .iter()
        .enumerate()
        .map(|(k, &i)| sf.axis_grids[k][i])
        .collect();

    wrap_coord_for_domain(&coords_si, domain_type)
}

/// Decompose a row-major linear index into per-axis indices.
///
/// Convention: axis-0 outermost (matches `engine_eval::build_sampled_field`
/// and `interp::interpolate_Nd`'s row-major layout).
///
/// For shape `(s0, s1, ..., s_{N-1})` and linear index `i`:
/// ```text
/// i_{N-1} = i % s_{N-1}
/// i_{N-2} = (i / s_{N-1}) % s_{N-2}
/// ...
/// i_0    = i / (s_1 * s_2 * ... * s_{N-1})
/// ```
///
/// Pinned by `argmax_sampled_field_2d_length_domain_returns_point2_at_max_index`
/// (3×2 shape, max at linear=4 → per-axis (2, 0)) and the symmetric
/// `argmin_..._2d` counterpart in `tests/field_reductions_tests.rs`.
/// The N-D loop is generic and the `SampledGridKind` invariant (1/2/3
/// axes) is reinforced by the `debug_assert!` below.
fn decompose_index(linear: usize, axis_lengths: &[usize]) -> Vec<usize> {
    debug_assert!(
        matches!(axis_lengths.len(), 1 | 2 | 3),
        "SampledGridKind invariant: 1/2/3 axes only"
    );
    let mut out = vec![0usize; axis_lengths.len()];
    let mut rem = linear;
    for k in (0..axis_lengths.len()).rev() {
        let s = axis_lengths[k];
        out[k] = rem % s;
        rem /= s;
    }
    out
}

/// Wrap per-axis SI coords as a `Value` per the field's `domain_type`.
///
/// - 1-D domain (`Type::Real`, `Type::Int`, `Type::Scalar { dim }`):
///   returns a single `Value::Real` (dimensionless) or `Value::Scalar`
///   (dimensioned). Asserts `coords_si.len() == 1`. Pinned by
///   `argmax_sampled_field_1d_length_domain_*` /
///   `argmax_sampled_field_1d_real_domain_*` and the symmetric
///   `argmin_..._1d_length_domain_*` test in
///   `tests/field_reductions_tests.rs`.
/// - N-D domain (`Type::Point { n, quantity }`): returns
///   `Value::Point(per-axis-coords)` where each component follows the
///   same per-quantity wrap rule. Asserts `coords_si.len() == n`.
///   Pinned by `argmax_sampled_field_2d_length_domain_*` /
///   `argmin_..._2d_length_domain_*` (and 3-D variants in step-13).
/// - Anything else → `Value::Undef`.
fn wrap_coord_for_domain(coords_si: &[f64], domain_type: &Type) -> Value {
    match domain_type {
        Type::Point { n, quantity } => {
            if coords_si.len() != *n {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[reify-expr] argmax/argmin: coord arity mismatch: domain n={n}, got {} coords",
                    coords_si.len()
                );
                return Value::Undef;
            }
            let components: Vec<Value> = coords_si
                .iter()
                .map(|&c| wrap_scalar_coord(c, quantity))
                .collect();
            Value::Point(components)
        }
        // 1-D scalar/dimensionless domain: single coord.
        Type::Real | Type::Int | Type::Scalar { .. } => {
            if coords_si.len() != 1 {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[reify-expr] argmax/argmin: coord arity mismatch: 1-D domain, got {} coords",
                    coords_si.len()
                );
                return Value::Undef;
            }
            wrap_scalar_coord(coords_si[0], domain_type)
        }
        _ => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[reify-expr] argmax/argmin: unsupported domain type: {:?}",
                domain_type
            );
            Value::Undef
        }
    }
}

/// Wrap a single SI coord per a scalar quantity type.
///
/// `Type::Scalar { dimension }` with non-dimensionless dim → `Value::Scalar`;
/// everything else → `Value::Real`.
fn wrap_scalar_coord(coord_si: f64, quantity: &Type) -> Value {
    match quantity {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: coord_si,
            dimension: *dimension,
        },
        _ => Value::Real(coord_si),
    }
}

/// Wrap an SI f64 in the field's codomain shape.
///
/// Mirrors `crate::sampled::wrap_result` exactly:
/// - `Type::Scalar { dimension }` with non-dimensionless `dimension`
///   (e.g. `PRESSURE`, `LENGTH`) → `Value::Scalar { si_value, dimension }`,
///   preserving the field's codomain dimension on the reduction result so
///   `max(von_mises(stress)) < yield_stress` etc. unify dimensionally.
/// - `Type::Real`, `Type::Int`, dimensionless `Type::Scalar`, and any
///   other codomain → `Value::Real(v)` (the `_` arm is the dimensionless
///   default; the codomain type is otherwise unused for max/min).
///
/// Pinned by `max_sampled_field_with_pressure_codomain_returns_dimensioned_scalar`
/// and the parallel `min_..._returns_dimensioned_scalar` in
/// `tests/field_reductions_tests.rs`.
fn wrap_codomain(v: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: v,
            dimension: *dimension,
        },
        _ => Value::Real(v),
    }
}
