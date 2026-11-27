//! The `ElasticMaterial` input: the isotropic pair `youngs_modulus` /
//! `poisson_ratio` every FEA trampoline reads.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::{InputDecodeError, required_field};
use crate::arg_acceptance::{dimensionless_spec, pressure_spec};

const STRUCTURE: &str = "ElasticMaterial";

/// Read `youngs_modulus` (a Pressure) and `poisson_ratio` (dimensionless) off
/// an already-destructured instance.
pub(crate) fn isotropic_from_data(
    data: &StructureInstanceData,
) -> Result<IsotropicElastic, InputDecodeError> {
    Ok(IsotropicElastic {
        youngs_modulus: required_field(data, STRUCTURE, "youngs_modulus", &pressure_spec())?,
        poisson_ratio: required_field(data, STRUCTURE, "poisson_ratio", &dimensionless_spec())?,
    })
}

/// [`isotropic_from_data`] on a `Value`, which must be a structure instance.
pub(crate) fn extract_material(val: &Value) -> Result<IsotropicElastic, InputDecodeError> {
    match val {
        Value::StructureInstance(data) => isotropic_from_data(data),
        other => Err(InputDecodeError::not_a_structure(STRUCTURE, None, other)),
    }
}
