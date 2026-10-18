//! The ONE reader of a magnitude off an FEA load struct (PRD
//! `dimension-checked-readers.md` §7 I7), shared by the elastic-static and
//! buckling trampolines, and of the material density a `Gravity` load scales.
//!
//! Every load magnitude reads through `read_field_or_bare`: a `Scalar` must
//! carry its position's dimension, while a bare `Real`/`Int` is still read as
//! the legacy undimensioned spelling until γ3 #5802 narrows it. The material
//! density is no load magnitude and has no legacy spelling, so it reads through
//! the strict `field_scalar`, the reader the as-printed trampoline gives the
//! same `ElasticMaterial.density`. An absent, `none` or `Undef` value
//! contributes nothing, quietly (PRD decision 2). Any value refused — a
//! `Scalar` of another dimension or not a number at all — is a
//! [`FieldRejection`] naming `<Struct>.<field>`, coded `DimensionedArgRejected`.
//! A field reader can fail no other way, so it returns the bare rejection; `?`
//! lifts it into a [`FeaValueShapeError`].

use reify_ir::{StructureInstanceData, Value};

use super::as_printed_material::field_scalar;
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
) -> Result<Option<f64>, FieldRejection> {
    magnitude(load, "force", &force_spec())
}

/// `PressureLoad.magnitude`, in pascals.
pub(super) fn pressure_load_magnitude(
    load: &StructureInstanceData,
) -> Result<Option<f64>, FieldRejection> {
    magnitude(load, "magnitude", &pressure_spec())
}

/// `Gravity.magnitude`, in m·s⁻².
pub(super) fn gravity_magnitude(
    load: &StructureInstanceData,
) -> Result<Option<f64>, FieldRejection> {
    magnitude(load, "magnitude", &acceleration_spec())
}

/// The material's `density`, in kg·m⁻³: a `Scalar<Density>` only, exactly as
/// the as-printed reader accepts it, so a bare `Real`/`Int` is refused. A
/// material that is not a struct, or whose `density` is absent or `Undef`,
/// reads `0.0` (the default γ3 #5802 owns), so a `Gravity` load on it
/// contributes no body force.
pub(super) fn material_density(material: &Value) -> Result<f64, FieldRejection> {
    let Value::StructureInstance(data) = material else {
        return Ok(0.0);
    };
    Ok(field_scalar(data, "density", &density_spec())?.unwrap_or(0.0))
}

fn magnitude(
    data: &StructureInstanceData,
    field: &str,
    spec: &ArgSpec,
) -> Result<Option<f64>, FieldRejection> {
    read_field_or_bare(data.fields.get(field), spec)
        .map_err(|rejection| FieldRejection::new(&data.type_name, field, rejection))
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;

    use super::*;
    use crate::compute_targets::{scalar, structure};

    fn instance_data(type_name: &str, fields: Vec<(&str, Value)>) -> StructureInstanceData {
        match structure(type_name, fields) {
            Value::StructureInstance(data) => *data,
            other => unreachable!("`structure` builds a StructureInstance, got {other:?}"),
        }
    }

    fn point_load(force: Value) -> StructureInstanceData {
        instance_data("PointLoad", vec![("force", force)])
    }

    fn material(density: Value) -> Value {
        structure("Steel_AISI_1045", vec![("density", density)])
    }

    #[test]
    fn point_load_force_reads_a_force_scalar_like_the_bare_spellings() {
        let expected = Ok(Some(1000.0));
        assert_eq!(
            point_load_force(&point_load(scalar(1000.0, DimensionVector::FORCE))),
            expected
        );
        assert_eq!(point_load_force(&point_load(Value::Real(1000.0))), expected);
        assert_eq!(point_load_force(&point_load(Value::Int(1000))), expected);
    }

    /// An `Option`-typed magnitude reads like any other struct field: `Some`
    /// is unwrapped, and `none` reads as absent, as do `Undef` and no field.
    #[test]
    fn point_load_force_unwraps_an_option_and_reads_none_undef_or_no_field_as_absent() {
        let some = |v: Value| Value::Option(Some(Box::new(v)));
        assert_eq!(
            point_load_force(&point_load(some(scalar(1000.0, DimensionVector::FORCE)))),
            Ok(Some(1000.0))
        );
        assert_eq!(point_load_force(&point_load(Value::Option(None))), Ok(None));
        assert_eq!(point_load_force(&point_load(Value::Undef)), Ok(None));
        assert_eq!(
            point_load_force(&instance_data("PointLoad", vec![])),
            Ok(None)
        );
    }

    #[test]
    fn pressure_and_gravity_magnitudes_read_their_own_dimension() {
        let pressure = instance_data(
            "PressureLoad",
            vec![("magnitude", scalar(5.0e6, DimensionVector::PRESSURE))],
        );
        assert_eq!(pressure_load_magnitude(&pressure), Ok(Some(5.0e6)));
        let gravity = instance_data(
            "Gravity",
            vec![("magnitude", scalar(9.8, DimensionVector::ACCELERATION))],
        );
        assert_eq!(gravity_magnitude(&gravity), Ok(Some(9.8)));
    }

    #[test]
    fn each_load_magnitude_rejects_a_wrong_dimension_naming_its_field() {
        type Reader = fn(&StructureInstanceData) -> Result<Option<f64>, FieldRejection>;
        let cases: [(&str, &str, Reader, DimensionVector, &str); 3] = [
            (
                "PointLoad",
                "force",
                point_load_force,
                DimensionVector::LENGTH,
                "Force",
            ),
            (
                "PressureLoad",
                "magnitude",
                pressure_load_magnitude,
                DimensionVector::FORCE,
                "Pressure",
            ),
            (
                "Gravity",
                "magnitude",
                gravity_magnitude,
                DimensionVector::FORCE,
                "Acceleration",
            ),
        ];
        for (type_name, field, read, wrong, expected_name) in cases {
            let res = read(&instance_data(type_name, vec![(field, scalar(2.0, wrong))]));
            assert!(
                matches!(
                    &res,
                    Err(FieldRejection { path, rejection })
                        if *path == format!("{type_name}.{field}")
                            && rejection.expected == expected_name
                ),
                "{type_name}.{field}: a wrong-dimension Scalar must be a \
                 rejection expecting {expected_name:?}, got {res:?}"
            );
        }
    }

    /// A present value that is not a number is refused, never read as absent.
    #[test]
    fn a_non_numeric_force_is_rejected_not_read_as_absent() {
        let res = point_load_force(&point_load(Value::String("1 kN".to_string())));
        assert!(
            matches!(&res, Err(FieldRejection { path, .. }) if path == "PointLoad.force"),
            "a String force must be a rejection naming PointLoad.force, got {res:?}"
        );
    }

    #[test]
    fn material_density_gates_the_density_dimension() {
        assert_eq!(
            material_density(&material(scalar(7850.0, DimensionVector::MASS_DENSITY))),
            Ok(7850.0)
        );
        assert_eq!(material_density(&material(Value::Undef)), Ok(0.0));
        let res = material_density(&material(scalar(7850.0, DimensionVector::PRESSURE)));
        assert!(
            matches!(
                &res,
                Err(FieldRejection { path, rejection })
                    if path == "Steel_AISI_1045.density" && rejection.expected == "Density"
            ),
            "a PRESSURE density must be a rejection naming the density, got {res:?}"
        );
    }

    /// The material density is not a load magnitude: it takes no legacy bare
    /// spelling, so it accepts exactly what the as-printed reader accepts.
    #[test]
    fn material_density_refuses_a_bare_density_like_the_as_printed_reader() {
        for bare in [Value::Real(7850.0), Value::Int(7850)] {
            let res = material_density(&material(bare.clone()));
            assert!(
                matches!(
                    &res,
                    Err(FieldRejection { path, rejection })
                        if path == "Steel_AISI_1045.density" && rejection.expected == "Density"
                ),
                "a bare {bare:?} density must be a rejection naming the density, got {res:?}"
            );
        }
    }

    /// The defensive fallback, made explicit so it is distinguishable from a
    /// latent bug: every `ElasticMaterial` conformer declares `density`, so it
    /// arises only from a malformed or absent material value.
    #[test]
    fn material_density_reads_zero_for_no_density_field_or_a_non_struct_material() {
        assert_eq!(
            material_density(&structure("NoFieldMaterial", vec![])),
            Ok(0.0)
        );
        assert_eq!(material_density(&Value::Real(42.0)), Ok(0.0));
    }
}
