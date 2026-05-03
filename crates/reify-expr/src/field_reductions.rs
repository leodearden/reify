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
            Value::SampledField(sf) => reduce_sampled_extremum(sf, codomain_type, false),
            _ => Value::Undef,
        },
        // TODO(future): numerical optimisation over Analytical/Composed lambda
        // domains; iterate over Sampled subfield for derived (Gradient, VonMises,
        // MaxShear, ...) wrappers — see PRD docs/prds/v0_3/structural-analysis-fea.md
        // task #6 (deferred per task description's "Implementation can be staged
        // — sampled first").
        _ => Value::Undef,
    }
}

/// Reduce a `SampledField`'s data buffer to a single extremum value,
/// wrapped per the codomain type.
///
/// `find_min == false` → maximum; `find_min == true` → minimum.
///
/// NaN/non-finite values are skipped. Empty / all-non-finite buffers
/// return `Value::Undef`.
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

/// Wrap an SI f64 in the field's codomain shape. Matches the convention
/// in `crate::sampled::wrap_result`: dimensionless codomain
/// (`Type::Real` / `Type::Int`) → `Value::Real`; dimensioned `Type::Scalar`
/// → `Value::Scalar { si_value, dimension }`.
fn wrap_codomain(v: f64, codomain_type: &Type) -> Value {
    match codomain_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Value::Scalar {
            si_value: v,
            dimension: *dimension,
        },
        _ => Value::Real(v),
    }
}
