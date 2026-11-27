//! The `ElasticMaterial` input: the isotropic pair `youngs_modulus` /
//! `poisson_ratio` every FEA trampoline reads.

use reify_ir::StructureInstanceData;
use reify_solver_elastic::IsotropicElastic;

use super::{DecodeStdlibInput, InputDecodeError, required_field};
use crate::arg_acceptance::{dimensionless_spec, pressure_spec};

/// `ElasticMaterial`'s isotropic pair, decoded off any conformer by shape:
/// `youngs_modulus` a Pressure, `poisson_ratio` dimensionless. `density` and
/// `yield_stress` stay with the trampolines that read them.
impl DecodeStdlibInput for IsotropicElastic {
    const STRUCTURE: &'static str = "ElasticMaterial";

    fn decode_fields(data: &StructureInstanceData) -> Result<Self, InputDecodeError> {
        Ok(IsotropicElastic {
            youngs_modulus: required_field(
                data,
                Self::STRUCTURE,
                "youngs_modulus",
                &pressure_spec(),
            )?,
            poisson_ratio: required_field(
                data,
                Self::STRUCTURE,
                "poisson_ratio",
                &dimensionless_spec(),
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::Value;
    use reify_solver_elastic::IsotropicElastic;

    use super::super::test_support::{
        hand_built_material, material_missing_poisson_ratio, material_with_length_youngs_modulus,
        steel_density, steel_youngs_modulus,
    };
    use super::super::{DecodeFailure, DecodeStdlibInput, InputDecodeError};
    use crate::arg_acceptance::FieldRejection;
    use crate::compute_targets::{scalar, structure};

    fn failure_at(field: Option<&'static str>, failure: DecodeFailure) -> InputDecodeError {
        InputDecodeError {
            input: "ElasticMaterial",
            field,
            failure,
        }
    }

    /// The decoder is shape-based: any conformer carrying the isotropic pair
    /// decodes, whatever its `type_name`.
    #[test]
    fn any_isotropic_shaped_conformer_decodes() {
        for type_name in ["HandBuiltSteel", "Steel_AISI_1045"] {
            let material = structure(
                type_name,
                vec![
                    ("youngs_modulus", steel_youngs_modulus()),
                    ("poisson_ratio", Value::Real(0.29)),
                    ("density", steel_density()),
                ],
            );
            assert_eq!(
                IsotropicElastic::decode(&material),
                Ok(IsotropicElastic {
                    youngs_modulus: 2.0e11,
                    poisson_ratio: 0.29,
                }),
                "{type_name}"
            );
        }
    }

    /// A missing field is named by the trait, not the conformer's type_name.
    #[test]
    fn a_missing_field_is_named_by_the_trait() {
        assert_eq!(
            IsotropicElastic::decode(&material_missing_poisson_ratio()),
            Err(failure_at(Some("poisson_ratio"), DecodeFailure::Missing))
        );
        assert_eq!(
            IsotropicElastic::decode(&hand_built_material(vec![(
                "poisson_ratio",
                Value::Real(0.29)
            )])),
            Err(failure_at(Some("youngs_modulus"), DecodeFailure::Missing))
        );
    }

    #[test]
    fn a_length_youngs_modulus_is_rejected_at_the_trait_path() {
        match IsotropicElastic::decode(&material_with_length_youngs_modulus()) {
            Err(InputDecodeError {
                failure: DecodeFailure::Rejected(FieldRejection { path, rejection }),
                ..
            }) => {
                assert_eq!(path, "ElasticMaterial.youngs_modulus");
                assert_eq!(rejection.expected, "Pressure");
            }
            other => panic!("a LENGTH youngs_modulus must be Rejected, got {other:?}"),
        }
    }

    #[test]
    fn a_dimensioned_poisson_ratio_is_rejected_at_the_trait_path() {
        let material = hand_built_material(vec![
            ("youngs_modulus", steel_youngs_modulus()),
            ("poisson_ratio", scalar(0.3, DimensionVector::PRESSURE)),
        ]);
        match IsotropicElastic::decode(&material) {
            Err(InputDecodeError {
                failure: DecodeFailure::Rejected(FieldRejection { path, rejection }),
                ..
            }) => {
                assert_eq!(path, "ElasticMaterial.poisson_ratio");
                assert_eq!(rejection.expected, "Real");
            }
            other => panic!("a PRESSURE poisson_ratio must be Rejected, got {other:?}"),
        }
    }

    #[test]
    fn an_undef_field_and_an_undef_material_are_unresolved() {
        let undef_modulus = hand_built_material(vec![
            ("youngs_modulus", Value::Undef),
            ("poisson_ratio", Value::Real(0.29)),
        ]);
        assert_eq!(
            IsotropicElastic::decode(&undef_modulus),
            Err(failure_at(
                Some("youngs_modulus"),
                DecodeFailure::Unresolved
            ))
        );
        assert_eq!(
            IsotropicElastic::decode(&Value::Undef),
            Err(failure_at(None, DecodeFailure::Unresolved))
        );
    }

    #[test]
    fn a_non_structure_material_is_not_a_structure_named_by_the_trait() {
        assert!(matches!(
            IsotropicElastic::decode(&Value::Real(1.0)),
            Err(InputDecodeError {
                input: "ElasticMaterial",
                field: None,
                failure: DecodeFailure::NotAStructure { .. },
            })
        ));
    }

    #[test]
    fn every_dimensionless_poisson_ratio_spelling_decodes() {
        for poisson_ratio in [
            Value::Real(0.0),
            Value::Int(0),
            scalar(0.0, DimensionVector::DIMENSIONLESS),
        ] {
            let material = hand_built_material(vec![
                ("youngs_modulus", steel_youngs_modulus()),
                ("poisson_ratio", poisson_ratio.clone()),
            ]);
            assert_eq!(
                IsotropicElastic::decode(&material),
                Ok(IsotropicElastic {
                    youngs_modulus: 2.0e11,
                    poisson_ratio: 0.0,
                }),
                "poisson_ratio {poisson_ratio:?}"
            );
        }
    }

    /// `decode_fields` reads an already-destructured instance the same way.
    #[test]
    fn decode_fields_reads_destructured_data() {
        let Value::StructureInstance(data) = material_missing_poisson_ratio() else {
            unreachable!("the preset is a StructureInstance");
        };
        assert_eq!(
            IsotropicElastic::decode_fields(&data),
            Err(failure_at(Some("poisson_ratio"), DecodeFailure::Missing))
        );
    }
}
