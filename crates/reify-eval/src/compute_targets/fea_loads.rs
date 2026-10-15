//! The ONE reader of a magnitude off an FEA load struct (PRD
//! `dimension-checked-readers.md` §7 I7), shared by the elastic-static and
//! buckling trampolines.
//!
//! Every magnitude reads through `read_field_or_bare`: a `Scalar` must carry
//! its position's dimension, while a bare `Real`/`Int` is still read as the
//! legacy undimensioned spelling until γ3 #5802 narrows it. An absent, `none`
//! or `Undef` magnitude contributes nothing, quietly (PRD decision 2). Any
//! value it refuses — a `Scalar` of another dimension or not a number at all —
//! is a `Rejected` naming `<Struct>.<field>`, coded `DimensionedArgRejected`.

use reify_ir::{StructureInstanceData, Value};

use super::fea_value_shape::FeaValueShapeError;
use crate::arg_acceptance::{
    ArgSpec, FieldRejection, acceleration_spec, density_spec, force_spec, pressure_spec,
    read_field_or_bare,
};

/// The items of a `loads` list.
pub(super) fn load_items<'a>(
    loads: &'a Value,
    context: &'static str,
) -> Result<&'a [Value], FeaValueShapeError> {
    match loads {
        Value::List(items) => Ok(items),
        other => Err(FeaValueShapeError::ExpectedList {
            context,
            got: format!("{other:?}"),
        }),
    }
}

/// `PointLoad.force`, in newtons.
pub(super) fn point_load_force(
    load: &StructureInstanceData,
) -> Result<Option<f64>, FeaValueShapeError> {
    magnitude(load, "force", &force_spec())
}

/// `PressureLoad.magnitude`, in pascals.
pub(super) fn pressure_load_magnitude(
    load: &StructureInstanceData,
) -> Result<Option<f64>, FeaValueShapeError> {
    magnitude(load, "magnitude", &pressure_spec())
}

/// `Gravity.magnitude`, in m·s⁻².
pub(super) fn gravity_magnitude(
    load: &StructureInstanceData,
) -> Result<Option<f64>, FeaValueShapeError> {
    magnitude(load, "magnitude", &acceleration_spec())
}

/// The material's `density`, in kg·m⁻³. A bare `Real`/`Int` density is read
/// as kg·m⁻³ like every other magnitude here, so it drives a `Gravity` body
/// force. A material that is not a struct, or whose `density` is absent or
/// `Undef`, reads `0.0` (the default γ3 #5802 owns), so a `Gravity` load on it
/// contributes no body force.
pub(super) fn material_density(material: &Value) -> Result<f64, FeaValueShapeError> {
    let Value::StructureInstance(data) = material else {
        return Ok(0.0);
    };
    Ok(magnitude(data, "density", &density_spec())?.unwrap_or(0.0))
}

fn magnitude(
    data: &StructureInstanceData,
    field: &str,
    spec: &ArgSpec,
) -> Result<Option<f64>, FeaValueShapeError> {
    read_field_or_bare(data.fields.get(field), spec).map_err(|rejection| {
        FeaValueShapeError::Rejected(FieldRejection::new(&data.type_name, field, rejection))
    })
}
