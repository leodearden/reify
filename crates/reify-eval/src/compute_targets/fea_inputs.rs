//! The FEA leaf input readers shared by the `solve_elastic_static` and
//! `solve_buckling` trampolines: an SI scalar, a structure's scalar / real
//! field, and the isotropic material. Each reads into a `Result` whose `Err` is
//! a [`FeaValueShapeError`], so a malformed input never panics.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::fea_value_shape::FeaValueShapeError;
use crate::arg_acceptance::{
    ArgSpec, FieldAcceptance, FieldRejection, accept_field, dimensionless_spec, pressure_spec,
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
/// Each field reads under the spec the as-printed reader
/// (`as_printed_material::read_base_elastic`) applies to the same
/// `ElasticMaterial` field, so the field accepts the same values whichever
/// trampoline reads it: `youngs_modulus` a Pressure `Scalar` only,
/// `poisson_ratio` a dimensionless number. Only absence differs, by trampoline:
/// here it is an error.
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
    match accept_field(data, key, spec) {
        FieldAcceptance::Accepted(si) => Ok(si),
        FieldAcceptance::Absent | FieldAcceptance::Undefined => {
            Err(FeaValueShapeError::MissingField {
                context: "isotropic_from_data",
                field: key,
            })
        }
        FieldAcceptance::Rejected(rejection) => {
            Err(FieldRejection::new(&data.type_name, key, rejection).into())
        }
    }
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

/// Extract SI scalar value from `Value::Scalar { si_value, .. }`.
pub(super) fn extract_scalar_si(val: &Value) -> Result<f64, FeaValueShapeError> {
    match val {
        Value::Scalar { si_value, .. } => Ok(*si_value),
        other => Err(FeaValueShapeError::ExpectedScalar {
            context: "extract_scalar_si",
            got: format!("{other:?}"),
        }),
    }
}
