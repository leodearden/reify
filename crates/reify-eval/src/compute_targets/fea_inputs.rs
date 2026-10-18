//! The FEA leaf input readers shared by the `solve_elastic_static` and
//! `solve_buckling` trampolines: a spec-classified component, a `Length`
//! input, a structure's scalar / real field, and the isotropic material. Each
//! reads into a `Result` whose `Err` is a [`FeaValueShapeError`], so a
//! malformed input never panics.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::as_printed_material::field_scalar;
use super::fea_value_shape::FeaValueShapeError;
use crate::arg_acceptance::{
    Acceptance, ArgSpec, accept_arg, dimensionless_spec, length_spec, pressure_spec,
};

/// Read a `Value::Scalar { si_value, .. }` field from a StructureInstance.
pub(super) fn scalar_si_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match data.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => Ok(*si_value),
        // `context` is a fixed label, so `got` carries the field key.
        Some(other) => Err(FeaValueShapeError::ExpectedScalar {
            context: "scalar_si_field",
            got: format!("{other:?} (field {key:?})"),
        }),
        None => Err(FeaValueShapeError::MissingField {
            context: "scalar_si_field",
            field: key,
        }),
    }
}

/// Read a `Value::Real` field from a StructureInstance.
pub(super) fn real_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match data.fields.get(key) {
        Some(Value::Real(r)) => Ok(*r),
        // `context` is a fixed label, so `got` carries the field key.
        Some(other) => Err(FeaValueShapeError::ExpectedReal {
            context: "real_field",
            got: format!("{other:?} (field {key:?})"),
        }),
        None => Err(FeaValueShapeError::MissingField {
            context: "real_field",
            field: key,
        }),
    }
}

/// Read `youngs_modulus`/`poisson_ratio` off an already-destructured
/// `StructureInstanceData` into `IsotropicElastic`. The core behind
/// [`extract_material`] and behind `law_from_value`'s isotropic fallback,
/// which already holds the law's destructured data; taking the data rather
/// than a second `&Value` means no caller can pass a pair describing two
/// different instances.
///
/// Each field reads through the strict `field_scalar` under the spec the
/// as-printed reader (`as_printed_material::read_base_elastic`) applies to the
/// same `ElasticMaterial` field, so the field accepts the same values whichever
/// trampoline reads it: `youngs_modulus` a Pressure `Scalar` only,
/// `poisson_ratio` a dimensionless number (and `density`, read by
/// `fea_loads::material_density`, a Density `Scalar` only). Only absence
/// differs, by trampoline: here it is an error.
pub(super) fn isotropic_from_data(
    data: &StructureInstanceData,
) -> Result<IsotropicElastic, FeaValueShapeError> {
    Ok(IsotropicElastic {
        youngs_modulus: isotropic_field(data, "youngs_modulus", &pressure_spec())?,
        poisson_ratio: isotropic_field(data, "poisson_ratio", &dimensionless_spec())?,
    })
}

/// Read the isotropic material's required field `key` through `spec`: a field
/// that is absent, `none` or unresolved is `MissingField`, and a present value
/// the spec refuses is `Rejected`, naming `<Struct>.<key>`.
fn isotropic_field(
    data: &StructureInstanceData,
    key: &'static str,
    spec: &ArgSpec,
) -> Result<f64, FeaValueShapeError> {
    field_scalar(data, key, spec)?.ok_or(FeaValueShapeError::MissingField {
        context: "isotropic_from_data",
        field: key,
    })
}

/// Extract `IsotropicElastic` from a `Value::StructureInstance` carrying
/// `youngs_modulus: Scalar<Pressure>` and `poisson_ratio: Real`. Thin
/// `&Value`-destructuring wrapper around `isotropic_from_data`, returning
/// `Err(FeaValueShapeError)` rather than panicking on a malformed `Value`.
///
/// Read by both FEA trampolines' input gates: `solve_elastic_static` through
/// `classify_material`'s isotropic fallback, and `solve_buckling` through
/// `read_buckling_inputs`.
pub(super) fn extract_material(val: &Value) -> Result<IsotropicElastic, FeaValueShapeError> {
    let data = match val {
        Value::StructureInstance(d) => d,
        other => {
            return Err(FeaValueShapeError::ExpectedStructureInstance {
                context: "extract_material",
                got: format!("{other:?}"),
            });
        }
    };
    isotropic_from_data(data)
}

/// Read a `Length` input — a trampoline's `length`/`width`/`height` — in
/// metres, through [`length_spec`]: a `Scalar` of another dimension is
/// `WrongDimension`, and any other value, `Undef` included, is
/// `ExpectedScalar`.
pub(super) fn extract_length_si(val: &Value) -> Result<f64, FeaValueShapeError> {
    const CONTEXT: &str = "extract_length_si";
    match spec_component(val, &length_spec(), CONTEXT) {
        ComponentRead::Accepted(si) => Ok(si),
        ComponentRead::Undefined => Err(FeaValueShapeError::ExpectedScalar {
            context: CONTEXT,
            got: format!("{val:?}"),
        }),
        ComponentRead::Rejected(err) => Err(err),
    }
}

/// The outcome of reading ONE numeric value — a triple's component or a
/// positional input — against an `ArgSpec`. Mirrors
/// `reify_ir::arg_acceptance::Acceptance` 1:1, carrying
/// [`FeaValueShapeError`] in place of `ArgRejection`.
///
/// `Undefined` is surfaced rather than folded into `Rejected` because the
/// right response to an unresolved value is the CALLER's to choose, not
/// [`spec_component`]'s (task #7019 review [reviewer_comprehensive], fixing a
/// regression where folding the two together made `elastic_static`'s
/// `read_direction_or_neg_z` hard-fail the whole solve on transient solver
/// state instead of degrading quietly). `extract_scalar_triple`'s `comp`
/// closure and [`extract_length_si`] treat `Undefined` as a shape error —
/// pre-existing behaviour at both positions (follow-up #7793 is filed to
/// decide, deliberately and with its own test coverage, whether to widen the
/// triple's) — while `read_direction_or_neg_z` degrades it quietly to `0.0`,
/// per PRD `dimension-checked-readers.md` decision 2 ("`Undef` in => `Undef`
/// out, quietly"). Dimension EXPECTATION (what `spec_component` classifies)
/// and undef POLICY (what the caller decides) are orthogonal axes; this enum
/// is what keeps a catch-all arm from fusing them back together.
pub(super) enum ComponentRead {
    /// The value has the expected dimension; carries the SI f64.
    Accepted(f64),
    /// The value is `Value::Undef` — an unresolved cell, not a mistake. The
    /// caller decides what that means at this position.
    Undefined,
    /// The value is defined but the wrong shape/dimension.
    Rejected(FeaValueShapeError),
}

/// Shared spec-driven classifier (task #7019): classifies `v` against `spec`
/// via the canonical `reify_ir::arg_acceptance::accept_arg`, so
/// `elastic_static`'s `dimensioned_component`/`dimensionless_component` and
/// [`extract_length_si`] differ only in which `ArgSpec` they pass, not in a
/// hand-written `match` over `Value`.
///
/// Returns [`ComponentRead`], not a `Result`: this function's job is to
/// CLASSIFY `v` against `spec`, not to decide what an `Acceptance::Undefined`
/// result means to its caller — see `ComponentRead`'s doc. The match below
/// names all three `Acceptance` variants explicitly, with no catch-all, so a
/// future fourth variant is a compile error here rather than a silent
/// miscategorisation.
///
/// The `matches!(v, Value::Scalar { .. })` discriminator applies to the
/// `Rejected` arms ONLY, and distinguishes two genuinely different faults: a
/// `Value::Scalar` whose `dimension` is wrong
/// ([`FeaValueShapeError::WrongDimension`]) vs. a value that is not a
/// `Value::Scalar` at all (`ExpectedScalar`) — which is what keeps every
/// non-Scalar, DEFINED value on the `ExpectedScalar` path. Both arms reuse
/// the `ArgRejection` that `accept_arg` already built (task #7019 review
/// [reviewer_comprehensive]) instead of re-deriving a second wording from
/// `format!("{v:?}")`: `rej.got` is `value_short_label`'s rendering (e.g.
/// `"Length Scalar"`) and `rej.expected` is `spec.type_name`, so a
/// `WrongDimension` at a `dimensionless_spec` position reads "expected Real
/// … got Length Scalar" instead of a raw `Value::Scalar { .. }` `Debug` dump
/// that names the dimension but not in the same words the `expected` side
/// uses.
pub(super) fn spec_component(v: &Value, spec: &ArgSpec, context: &'static str) -> ComponentRead {
    match accept_arg(v, spec) {
        Acceptance::Accepted(x) => ComponentRead::Accepted(x),
        Acceptance::Undefined => ComponentRead::Undefined,
        Acceptance::Rejected(rej) if matches!(v, Value::Scalar { .. }) => {
            ComponentRead::Rejected(FeaValueShapeError::WrongDimension {
                context,
                expected: rej.expected,
                got: rej.got,
            })
        }
        Acceptance::Rejected(rej) => ComponentRead::Rejected(FeaValueShapeError::ExpectedScalar {
            context,
            got: rej.got,
        }),
    }
}
