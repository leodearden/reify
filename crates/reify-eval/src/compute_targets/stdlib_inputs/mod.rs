#[cfg(test)]
mod tests {
    use reify_core::{DiagnosticCode, DimensionVector, Severity};
    use reify_ir::{StructureInstanceData, Value};

    use super::*;
    use crate::ComputeOutcome;
    use crate::arg_acceptance::{
        ArgRejection, FieldRejection, dimensionless_spec, length_spec, pressure_spec,
    };
    use crate::compute_targets::{scalar, structure};

    const TARGET: &str = "solve_elastic_static_trampoline";

    fn data(type_name: &str, fields: Vec<(&str, Value)>) -> StructureInstanceData {
        match structure(type_name, fields) {
            Value::StructureInstance(data) => *data,
            other => unreachable!("`structure` builds a StructureInstance, got {other:?}"),
        }
    }

    fn error(field: Option<&'static str>, failure: DecodeFailure) -> InputDecodeError {
        InputDecodeError {
            input: "ElasticMaterial",
            field,
            failure,
        }
    }

    fn length_rejection() -> ArgRejection {
        ArgRejection {
            got: "Length Scalar".to_string(),
            expected: "Pressure",
            migration_hint: None,
        }
    }

    /// The one diagnostic of a `Failed` outcome.
    fn only_diagnostic(outcome: ComputeOutcome) -> reify_core::Diagnostic {
        match outcome {
            ComputeOutcome::Failed {
                mut diagnostics, ..
            } => {
                assert_eq!(diagnostics.len(), 1, "exactly one diagnostic: {diagnostics:?}");
                diagnostics.remove(0)
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn assert_malformed(err: InputDecodeError, path: &str) {
        let d = only_diagnostic(err.into_failed(TARGET));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::ComputeInputMalformed));
        assert!(
            d.message.starts_with(&format!("{TARGET}: ")),
            "the message names the target first: {:?}",
            d.message
        );
        assert!(d.message.contains(path), "the message names {path}: {:?}", d.message);
    }

    #[test]
    fn a_missing_field_fails_as_one_malformed_input_error_naming_its_path() {
        assert_malformed(
            error(Some("poisson_ratio"), DecodeFailure::Missing),
            "ElasticMaterial.poisson_ratio",
        );
    }

    #[test]
    fn a_non_structure_and_a_wrong_shape_fail_as_malformed_input_errors() {
        assert_malformed(
            error(
                None,
                DecodeFailure::NotAStructure {
                    got: "Real(1.0)".to_string(),
                },
            ),
            "ElasticMaterial",
        );
        assert_malformed(
            InputDecodeError {
                input: "length",
                field: None,
                failure: DecodeFailure::WrongShape {
                    expected: "Length",
                    got: "Real".to_string(),
                },
            },
            "length",
        );
    }

    #[test]
    fn a_rejected_field_fails_in_the_shared_rejection_wording() {
        let rejection = FieldRejection::new("ElasticMaterial", "youngs_modulus", length_rejection());
        let expected = rejection.to_error(TARGET);
        let d = only_diagnostic(
            error(Some("youngs_modulus"), DecodeFailure::Rejected(rejection)).into_failed(TARGET),
        );
        assert_eq!(d.code, Some(DiagnosticCode::DimensionedArgRejected));
        assert_eq!(d.severity, expected.severity);
        assert_eq!(d.message, expected.message);
    }

    #[test]
    fn an_unsupported_kind_fails_as_one_unsupported_kind_error_naming_it() {
        let d = only_diagnostic(
            InputDecodeError {
                input: "ForcingFunction",
                field: None,
                failure: DecodeFailure::UnsupportedKind {
                    kind: "MyForce".to_string(),
                },
            }
            .into_failed("transient_response"),
        );
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::FeaLoadKindUnsupported));
        assert!(d.message.contains("MyForce"), "{:?}", d.message);
    }

    /// Undef in, Undef out, quietly: an unresolved input is no Error.
    #[test]
    fn an_unresolved_input_fails_quietly() {
        match error(Some("youngs_modulus"), DecodeFailure::Unresolved).into_failed(TARGET) {
            ComputeOutcome::Failed {
                diagnostics,
                structured_detail,
            } => {
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                assert!(structured_detail.is_empty());
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // ── the strict field reader ──────────────────────────────────────────────

    fn read(field: Value, spec: &crate::arg_acceptance::ArgSpec) -> Result<f64, InputDecodeError> {
        required_field(
            &data("HandBuiltSteel", vec![("youngs_modulus", field)]),
            "ElasticMaterial",
            "youngs_modulus",
            spec,
        )
    }

    #[test]
    fn the_field_reader_reads_an_accepted_value_in_si() {
        assert_eq!(read(scalar(2.0e11, DimensionVector::PRESSURE), &pressure_spec()), Ok(2.0e11));
    }

    #[test]
    fn an_absent_or_none_field_is_missing_and_names_input_and_field() {
        let missing = Err(error(Some("youngs_modulus"), DecodeFailure::Missing));
        assert_eq!(read(Value::Option(None), &pressure_spec()), missing);
        assert_eq!(
            required_field(
                &data("HandBuiltSteel", vec![]),
                "ElasticMaterial",
                "youngs_modulus",
                &pressure_spec(),
            ),
            missing
        );
    }

    #[test]
    fn an_undef_field_is_unresolved_through_an_option_too() {
        let unresolved = Err(error(Some("youngs_modulus"), DecodeFailure::Unresolved));
        assert_eq!(read(Value::Undef, &pressure_spec()), unresolved);
        assert_eq!(
            read(Value::Option(Some(Box::new(Value::Undef))), &pressure_spec()),
            unresolved
        );
    }

    #[test]
    fn a_refused_field_is_rejected_naming_input_dot_field() {
        for refused in [scalar(2.0e11, DimensionVector::LENGTH), Value::Real(2.0e11)] {
            match read(refused.clone(), &pressure_spec()) {
                Err(InputDecodeError {
                    failure: DecodeFailure::Rejected(FieldRejection { path, rejection }),
                    ..
                }) => {
                    assert_eq!(path, "ElasticMaterial.youngs_modulus");
                    assert_eq!(rejection.expected, "Pressure");
                }
                other => panic!("{refused:?} must be Rejected, got {other:?}"),
            }
        }
    }

    #[test]
    fn every_dimensionless_spelling_reads_at_a_dimensionless_field() {
        for (ratio, expected) in [
            (Value::Real(0.25), 0.25),
            (Value::Int(0), 0.0),
            (scalar(0.25, DimensionVector::DIMENSIONLESS), 0.25),
        ] {
            assert_eq!(read(ratio.clone(), &dimensionless_spec()), Ok(expected), "{ratio:?}");
        }
    }

    // ── the positional reader ────────────────────────────────────────────────

    #[test]
    fn the_positional_reader_reads_an_accepted_value() {
        assert_eq!(
            read_positional(&scalar(2.5, DimensionVector::LENGTH), "length", &length_spec()),
            Ok(2.5)
        );
    }

    #[test]
    fn an_undef_positional_is_unresolved() {
        assert_eq!(
            read_positional(&Value::Undef, "length", &length_spec()),
            Err(InputDecodeError {
                input: "length",
                field: None,
                failure: DecodeFailure::Unresolved,
            })
        );
    }

    #[test]
    fn a_wrong_dimension_positional_is_rejected_at_the_input_name() {
        match read_positional(&scalar(2.5, DimensionVector::MASS), "length", &length_spec()) {
            Err(InputDecodeError {
                input: "length",
                failure: DecodeFailure::Rejected(FieldRejection { path, rejection }),
                ..
            }) => {
                assert_eq!(path, "length");
                assert_eq!(rejection.expected, "Length");
            }
            other => panic!("a MASS length must be Rejected, got {other:?}"),
        }
    }

    #[test]
    fn any_other_defined_positional_is_a_wrong_shape_naming_the_spec() {
        for wrong in [Value::Real(2.5), Value::Bool(true)] {
            match read_positional(&wrong, "length", &length_spec()) {
                Err(InputDecodeError {
                    input: "length",
                    field: None,
                    failure: DecodeFailure::WrongShape { expected, .. },
                }) => assert_eq!(expected, "Length"),
                other => panic!("{wrong:?} must be WrongShape, got {other:?}"),
            }
        }
    }
}
