//! The FEA leaf input readers shared by the `solve_elastic_static` and
//! `solve_buckling` trampolines: an SI scalar, a structure's scalar / real
//! field, and the isotropic material. Each reads into a `Result` whose `Err` is
//! a [`FeaValueShapeError`], so a malformed input never panics.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::fea_value_shape::FeaValueShapeError;

/// Read a `Value::Scalar { si_value, .. }` field from a StructureInstance.
pub(super) fn scalar_si_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match data.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => Ok(*si_value),
        // `context` stays the fixed function-name label per the PRD §C3
        // contract (`ExpectedScalar { context: &'static str, got: String }`,
        // shared with D3/D5/D7/D8/D9); `got`'s wording is unpinned (open
        // question 2), so the offending field key rides along in `got`
        // instead so a `panic!("{e}")`/eventual D9 diagnostic can name it.
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
        // See `scalar_si_field`'s comment above: `context` stays the fixed
        // function-name label; the field key rides in `got` instead so it
        // isn't lost for the eventual D9 diagnostic.
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
/// `youngs_modulus` is read DIMENSION-BLIND: `scalar_si_field` takes the
/// `si_value` of a `Value::Scalar` of any dimension, so a LENGTH scalar is
/// read as pascals. This reader is outside the reader table of
/// `docs/prds/v0_6/dimension-checked-readers.md`; gating it is follow-up
/// ticket `tkt_0RVSW4XAXWVR0RAHCG3QTCRYM8`. `poisson_ratio` accepts only a
/// bare `Value::Real`.
pub(super) fn isotropic_from_data(
    data: &StructureInstanceData,
) -> Result<IsotropicElastic, FeaValueShapeError> {
    let youngs_modulus = scalar_si_field(data, "youngs_modulus")?;
    let poisson_ratio = real_field(data, "poisson_ratio")?;
    Ok(IsotropicElastic {
        youngs_modulus,
        poisson_ratio,
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
