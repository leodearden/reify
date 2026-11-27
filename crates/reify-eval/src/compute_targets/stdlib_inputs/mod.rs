//! The decode of a compute trampoline's stdlib inputs (PRD
//! `docs/prds/v0_6/trampoline-decode-and-result-build.md` §4 C1/C2).
//!
//! A trampoline reads each `value_inputs` entry through a reader here, or
//! through a [`DecodeStdlibInput`] decoder for a trait-typed input. Every
//! reader fails with the ONE error type, [`InputDecodeError`], and
//! [`InputDecodeError::into_failed`] is the one point where that error becomes
//! a `ComputeOutcome::Failed`. Dimension policy comes only from
//! `arg_acceptance` spec constructors, passed down by the caller.

use reify_core::{Diagnostic, DiagnosticCode};
use reify_ir::{StructureInstanceData, Value};

use crate::ComputeOutcome;
use crate::arg_acceptance::{
    Acceptance, ArgRejection, ArgSpec, FieldAcceptance, FieldRejection, accept_arg, accept_field,
};

mod elastic_material;
#[cfg(test)]
pub(crate) mod test_support;

/// A stdlib structure (or trait) a trampoline decodes from one input `Value`.
pub(crate) trait DecodeStdlibInput: Sized {
    /// The name every path the decoder emits starts with: the trait name for
    /// a trait-typed input, never a conformer's concrete `type_name`.
    const STRUCTURE: &'static str;

    /// Decode from an already-destructured instance.
    fn decode_fields(data: &StructureInstanceData) -> Result<Self, InputDecodeError>;

    /// Decode from the input `Value`: an `Undef` input is unresolved, and any
    /// other non-structure is not a structure.
    fn decode(value: &Value) -> Result<Self, InputDecodeError> {
        Self::decode_fields(structure_fields(value, Self::STRUCTURE)?)
    }
}

/// The fields of `value`, decoded as the structure `input`: an `Undef` value
/// is unresolved, and any other non-structure is not a structure.
pub(crate) fn structure_fields<'a>(
    value: &'a Value,
    input: &'static str,
) -> Result<&'a StructureInstanceData, InputDecodeError> {
    match value {
        Value::StructureInstance(data) => Ok(data),
        Value::Undef => Err(InputDecodeError::new(
            input,
            None,
            DecodeFailure::Unresolved,
        )),
        other => Err(InputDecodeError::not_a_structure(input, None, other)),
    }
}

/// Why a stdlib input could not be decoded, and where: `input` names what was
/// decoded (a structure or trait, a positional param, or a Rust-minted
/// payload), `field` the field inside it, if any.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InputDecodeError {
    pub input: &'static str,
    pub field: Option<&'static str>,
    pub failure: DecodeFailure,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DecodeFailure {
    /// A defined value where a structure instance belongs.
    NotAStructure { got: String },
    /// A declared non-`Option` field that is absent or `none`.
    Missing,
    /// A value that has not resolved yet (`Undef`): not a mistake.
    Unresolved,
    /// A defined value of the wrong shape: not a number, the wrong arity, a
    /// non-finite datum, a frame that does not decode.
    WrongShape { expected: &'static str, got: String },
    /// A number the position's `ArgSpec` refuses; the rejection names its
    /// own path.
    Rejected(FieldRejection),
    /// A structure whose kind the trampoline does not support.
    UnsupportedKind { kind: String },
}

impl InputDecodeError {
    pub(crate) fn new(
        input: &'static str,
        field: Option<&'static str>,
        failure: DecodeFailure,
    ) -> Self {
        Self {
            input,
            field,
            failure,
        }
    }

    pub(crate) fn not_a_structure(
        input: &'static str,
        field: Option<&'static str>,
        got: &Value,
    ) -> Self {
        Self::new(
            input,
            field,
            DecodeFailure::NotAStructure {
                got: format!("{got:?}"),
            },
        )
    }

    pub(crate) fn missing(input: &'static str, field: &'static str) -> Self {
        Self::new(input, Some(field), DecodeFailure::Missing)
    }

    pub(crate) fn wrong_shape(
        input: &'static str,
        field: Option<&'static str>,
        expected: &'static str,
        got: String,
    ) -> Self {
        Self::new(input, field, DecodeFailure::WrongShape { expected, got })
    }

    /// Lift a legacy reader's bare `FieldRejection`, which names its own path,
    /// read inside `input`.
    pub(crate) fn refused(input: &'static str, rejection: FieldRejection) -> Self {
        Self::new(input, None, DecodeFailure::Rejected(rejection))
    }

    pub(crate) fn is_unresolved(&self) -> bool {
        self.failure == DecodeFailure::Unresolved
    }

    /// The `Failed` outcome of `target` for this error: a refused number is
    /// `DimensionedArgRejected` in the shared rejection wording, an
    /// unsupported kind `FeaLoadKindUnsupported`, any other defined fault
    /// `ComputeInputMalformed`, and an unresolved input fails quietly, with
    /// no diagnostic.
    pub(crate) fn into_failed(self, target: &str) -> ComputeOutcome {
        let diagnostic = match &self.failure {
            DecodeFailure::Unresolved => None,
            DecodeFailure::Rejected(rejection) => Some(rejection.to_error(target)),
            DecodeFailure::UnsupportedKind { .. } => Some(
                Diagnostic::error(format!("{target}: {self}"))
                    .with_code(DiagnosticCode::FeaLoadKindUnsupported),
            ),
            DecodeFailure::NotAStructure { .. }
            | DecodeFailure::Missing
            | DecodeFailure::WrongShape { .. } => Some(
                Diagnostic::error(format!("{target}: {self}"))
                    .with_code(DiagnosticCode::ComputeInputMalformed),
            ),
        };
        ComputeOutcome::Failed {
            diagnostics: diagnostic.into_iter().collect(),
            structured_detail: vec![],
        }
    }
}

/// `"{input}[.{field}]: {reason}"`; a rejection renders in the shared
/// rejection wording, which names its own path.
impl std::fmt::Display for InputDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let DecodeFailure::Rejected(rejection) = &self.failure {
            return write!(f, "{rejection}");
        }
        f.write_str(self.input)?;
        if let Some(field) = self.field {
            write!(f, ".{field}")?;
        }
        match &self.failure {
            DecodeFailure::NotAStructure { got } => write!(f, ": expected a structure, got {got}"),
            DecodeFailure::Missing => f.write_str(": missing"),
            DecodeFailure::Unresolved => f.write_str(": unresolved (Undef)"),
            DecodeFailure::WrongShape { expected, got } => {
                write!(f, ": expected {expected}, got {got}")
            }
            DecodeFailure::UnsupportedKind { kind } => write!(f, ": unsupported kind '{kind}'"),
            DecodeFailure::Rejected(_) => Ok(()),
        }
    }
}

/// The required field `key` of `data`, read through `spec` as part of
/// `input`: absent or `none` is `Missing`, `Undef` is `Unresolved`, and a
/// present value the spec refuses is `Rejected`, naming `<input>.<key>`.
pub(crate) fn required_field(
    data: &StructureInstanceData,
    input: &'static str,
    key: &'static str,
    spec: &ArgSpec,
) -> Result<f64, InputDecodeError> {
    match accept_field(data, key, spec) {
        FieldAcceptance::Accepted(si) => Ok(si),
        FieldAcceptance::Absent => Err(InputDecodeError::missing(input, key)),
        FieldAcceptance::Undefined => Err(InputDecodeError::new(
            input,
            Some(key),
            DecodeFailure::Unresolved,
        )),
        FieldAcceptance::Rejected(rejection) => Err(InputDecodeError::new(
            input,
            Some(key),
            DecodeFailure::Rejected(FieldRejection::new(input, key, rejection)),
        )),
    }
}

/// The value of the required field `key` of `data`, part of `input`: absent
/// or `none` is `Missing`, and `some(v)` is `v`.
pub(crate) fn required_value<'a>(
    data: &'a StructureInstanceData,
    input: &'static str,
    key: &'static str,
) -> Result<&'a Value, InputDecodeError> {
    match data.fields.get(key) {
        None | Some(Value::Option(None)) => Err(InputDecodeError::missing(input, key)),
        Some(Value::Option(Some(inner))) => Ok(inner),
        Some(value) => Ok(value),
    }
}

/// The positional input `input` at `index` of `value_inputs`: an absent one
/// is `Missing`.
pub(crate) fn positional<'a>(
    value_inputs: &'a [Value],
    index: usize,
    input: &'static str,
) -> Result<&'a Value, InputDecodeError> {
    value_inputs
        .get(index)
        .ok_or_else(|| InputDecodeError::new(input, None, DecodeFailure::Missing))
}

/// A positional input `input`, read through `spec`: `Undef` is `Unresolved`.
pub(crate) fn read_positional(
    value: &Value,
    input: &'static str,
    spec: &ArgSpec,
) -> Result<f64, InputDecodeError> {
    read_number(value, input, None, spec)
}

/// A `Vector3` at `input[.field]`: exactly three components of a
/// `Value::Vector` or `Value::List`, each read through `spec`. `Undef`, for
/// the vector or a component, is `Unresolved`; any other shape is a
/// `WrongShape`.
pub(crate) fn read_vec3(
    value: &Value,
    input: &'static str,
    field: Option<&'static str>,
    spec: &ArgSpec,
) -> Result<[f64; 3], InputDecodeError> {
    match value {
        Value::Vector(components) | Value::List(components) => match components.as_slice() {
            [x, y, z] => Ok([
                read_number(x, input, field, spec)?,
                read_number(y, input, field, spec)?,
                read_number(z, input, field, spec)?,
            ]),
            _ => Err(InputDecodeError::wrong_shape(
                input,
                field,
                "Vector3",
                format!("{} components", components.len()),
            )),
        },
        Value::Undef => Err(InputDecodeError::new(
            input,
            field,
            DecodeFailure::Unresolved,
        )),
        other => Err(InputDecodeError::wrong_shape(
            input,
            field,
            "Vector3",
            format!("{other:?}"),
        )),
    }
}

/// A `List` at `input[.field]`, each entry read through `spec`. `Undef`, for
/// the list or an entry, is `Unresolved`; any other non-list is a
/// `WrongShape`.
pub(crate) fn read_list(
    value: &Value,
    input: &'static str,
    field: Option<&'static str>,
    spec: &ArgSpec,
) -> Result<Vec<f64>, InputDecodeError> {
    match value {
        Value::List(items) => items
            .iter()
            .map(|item| read_number(item, input, field, spec))
            .collect(),
        Value::Undef => Err(InputDecodeError::new(
            input,
            field,
            DecodeFailure::Unresolved,
        )),
        other => Err(InputDecodeError::wrong_shape(
            input,
            field,
            "List",
            format!("{other:?}"),
        )),
    }
}

/// One number at `input[.field]`, read through `spec`: `Undef` is
/// `Unresolved`.
fn read_number(
    value: &Value,
    input: &'static str,
    field: Option<&'static str>,
    spec: &ArgSpec,
) -> Result<f64, InputDecodeError> {
    match read_component(value, input, field, spec) {
        ComponentRead::Accepted(si) => Ok(si),
        ComponentRead::Undefined => Err(InputDecodeError::new(
            input,
            field,
            DecodeFailure::Unresolved,
        )),
        ComponentRead::Rejected(err) => Err(err),
    }
}

/// The outcome of reading ONE numeric value — a component or a positional
/// input — against an `ArgSpec`. `Undefined` stays distinct from `Rejected`
/// because the caller decides what an unresolved value means at its position
/// (PRD `dimension-checked-readers.md` decision 2).
pub(crate) enum ComponentRead {
    Accepted(f64),
    Undefined,
    Rejected(InputDecodeError),
}

/// Classify `value`, read at `input[.field]`, against `spec`: a refused
/// `Scalar` is `Rejected` at that path, and any other refused value is a
/// `WrongShape` naming the spec's type.
pub(crate) fn read_component(
    value: &Value,
    input: &'static str,
    field: Option<&'static str>,
    spec: &ArgSpec,
) -> ComponentRead {
    match accept_arg(value, spec) {
        Acceptance::Accepted(si) => ComponentRead::Accepted(si),
        Acceptance::Undefined => ComponentRead::Undefined,
        Acceptance::Rejected(rejection) if matches!(value, Value::Scalar { .. }) => {
            ComponentRead::Rejected(InputDecodeError::new(
                input,
                field,
                DecodeFailure::Rejected(rejection_at(input, field, rejection)),
            ))
        }
        Acceptance::Rejected(rejection) => ComponentRead::Rejected(InputDecodeError::wrong_shape(
            input,
            field,
            spec.type_name,
            rejection.got,
        )),
    }
}

/// The rejection of the value at `input[.field]`.
fn rejection_at(
    input: &'static str,
    field: Option<&'static str>,
    rejection: ArgRejection,
) -> FieldRejection {
    match field {
        Some(field) => FieldRejection::new(input, field, rejection),
        None => FieldRejection {
            path: input.to_string(),
            rejection,
        },
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{DiagnosticCode, DimensionVector, Severity};
    use reify_ir::{StructureInstanceData, Value};

    use super::*;
    use crate::ComputeOutcome;
    use crate::arg_acceptance::{
        ArgRejection, FieldRejection, dimensionless_spec, length_spec, pressure_spec, time_spec,
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
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "exactly one diagnostic: {diagnostics:?}"
                );
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
        assert!(
            d.message.contains(path),
            "the message names {path}: {:?}",
            d.message
        );
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
        let rejection =
            FieldRejection::new("ElasticMaterial", "youngs_modulus", length_rejection());
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
        assert_eq!(
            read(scalar(2.0e11, DimensionVector::PRESSURE), &pressure_spec()),
            Ok(2.0e11)
        );
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
            read(
                Value::Option(Some(Box::new(Value::Undef))),
                &pressure_spec()
            ),
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
            assert_eq!(
                read(ratio.clone(), &dimensionless_spec()),
                Ok(expected),
                "{ratio:?}"
            );
        }
    }

    // ── the positional reader ────────────────────────────────────────────────

    #[test]
    fn the_positional_reader_reads_an_accepted_value() {
        assert_eq!(
            read_positional(
                &scalar(2.5, DimensionVector::LENGTH),
                "length",
                &length_spec()
            ),
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
        match read_positional(
            &scalar(2.5, DimensionVector::MASS),
            "length",
            &length_spec(),
        ) {
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
    fn an_absent_positional_is_missing_at_the_input_name() {
        assert_eq!(
            positional(&[Value::Undef], 1, "direction"),
            Err(InputDecodeError {
                input: "direction",
                field: None,
                failure: DecodeFailure::Missing,
            })
        );
    }

    // ── the vector and list readers ──────────────────────────────────────────

    #[test]
    fn a_vec3_reads_exactly_three_components_of_a_vector_or_a_list() {
        let components = vec![Value::Real(1.0), Value::Int(2), Value::Real(3.0)];
        for value in [Value::Vector(components.clone()), Value::List(components)] {
            assert_eq!(
                read_vec3(&value, "Mode", Some("shape"), &dimensionless_spec()),
                Ok([1.0, 2.0, 3.0])
            );
        }
        for wrong in [
            Value::Vector(vec![Value::Real(1.0), Value::Real(2.0)]),
            Value::Real(1.0),
        ] {
            match read_vec3(&wrong, "Mode", Some("shape"), &dimensionless_spec()) {
                Err(InputDecodeError {
                    input: "Mode",
                    field: Some("shape"),
                    failure: DecodeFailure::WrongShape { expected, .. },
                }) => assert_eq!(expected, "Vector3"),
                other => panic!("{wrong:?} must be WrongShape, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_undef_vec3_or_component_is_unresolved() {
        let unresolved = Err(InputDecodeError {
            input: "direction",
            field: None,
            failure: DecodeFailure::Unresolved,
        });
        let undef_component = Value::Vector(vec![Value::Real(0.0), Value::Undef, Value::Real(1.0)]);
        for value in [Value::Undef, undef_component] {
            assert_eq!(
                read_vec3(&value, "direction", None, &dimensionless_spec()),
                unresolved
            );
        }
    }

    #[test]
    fn a_list_reads_every_entry_through_the_spec() {
        let times = Value::List(vec![
            scalar(0.0, DimensionVector::TIME),
            scalar(0.5, DimensionVector::TIME),
        ]);
        assert_eq!(
            read_list(&times, "SampledForce", Some("time_samples"), &time_spec()),
            Ok(vec![0.0, 0.5])
        );
        let mixed = Value::List(vec![
            scalar(0.0, DimensionVector::TIME),
            scalar(0.5, DimensionVector::LENGTH),
        ]);
        match read_list(&mixed, "SampledForce", Some("time_samples"), &time_spec()) {
            Err(InputDecodeError {
                failure: DecodeFailure::Rejected(FieldRejection { path, .. }),
                ..
            }) => assert_eq!(path, "SampledForce.time_samples"),
            other => panic!("a LENGTH entry must be Rejected, got {other:?}"),
        }
        match read_list(
            &Value::Real(0.5),
            "SampledForce",
            Some("time_samples"),
            &time_spec(),
        ) {
            Err(InputDecodeError {
                failure: DecodeFailure::WrongShape { expected, .. },
                ..
            }) => assert_eq!(expected, "List"),
            other => panic!("a non-list must be WrongShape, got {other:?}"),
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
