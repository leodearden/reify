//! The FEA leaf input readers shared by the `solve_elastic_static` and
//! `solve_buckling` trampolines: a spec-classified component, a `Length`
//! input, an elastic law's required modulus / ratio field, and the isotropic
//! material. Each reads into a `Result` whose `Err` is a
//! [`FeaValueShapeError`], so a malformed input never panics.

use reify_ir::{StructureInstanceData, Value};
use reify_solver_elastic::IsotropicElastic;

use super::fea_value_shape::FeaValueShapeError;
use crate::arg_acceptance::{
    Acceptance, ArgSpec, FieldAcceptance, FieldRejection, accept_arg, accept_field,
    dimensionless_spec, length_spec, pressure_spec,
};

/// An elastic law's required modulus field `key` (`youngs_modulus`, `e1`,
/// `g_axial`, ...), in pascals: a Pressure `Scalar` only, as the as-printed
/// reader takes `ElasticMaterial.youngs_modulus`.
pub(super) fn modulus_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    required_field(data, key, &pressure_spec(), "modulus_field")
}

/// An elastic law's required Poisson ratio field `key` (`poisson_ratio`,
/// `nu12`, ...): a bare `Real`/`Int` or a dimensionless `Scalar`.
pub(super) fn ratio_field(
    data: &StructureInstanceData,
    key: &'static str,
) -> Result<f64, FeaValueShapeError> {
    required_field(data, key, &dimensionless_spec(), "ratio_field")
}

/// Read the required field `key` through `spec`: absent or `none` is
/// `MissingField`, `Undef` is `Unresolved`, and a present value the spec
/// refuses is `Rejected`, naming `<Struct>.<key>`.
fn required_field(
    data: &StructureInstanceData,
    key: &'static str,
    spec: &ArgSpec,
    context: &'static str,
) -> Result<f64, FeaValueShapeError> {
    match accept_field(data, key, spec) {
        FieldAcceptance::Accepted(si) => Ok(si),
        FieldAcceptance::Absent => Err(FeaValueShapeError::MissingField {
            context,
            field: key,
        }),
        FieldAcceptance::Undefined => Err(FeaValueShapeError::Unresolved {
            context,
            field: key,
        }),
        FieldAcceptance::Rejected(rejection) => {
            Err(FieldRejection::new(&data.type_name, key, rejection).into())
        }
    }
}

/// Read `youngs_modulus`/`poisson_ratio` off an already-destructured
/// `StructureInstanceData` into `IsotropicElastic`. The core behind
/// [`extract_material`] and behind `law_from_value`'s isotropic fallback,
/// which already holds the law's destructured data; taking the data rather
/// than a second `&Value` means no caller can pass a pair describing two
/// different instances.
pub(super) fn isotropic_from_data(
    data: &StructureInstanceData,
) -> Result<IsotropicElastic, FeaValueShapeError> {
    Ok(IsotropicElastic {
        youngs_modulus: modulus_field(data, "youngs_modulus")?,
        poisson_ratio: ratio_field(data, "poisson_ratio")?,
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
/// positional input — against an `ArgSpec`: `reify_ir`'s `Acceptance`, with a
/// [`FeaValueShapeError`] in place of its `ArgRejection`.
///
/// `Undefined` stays distinct from `Rejected` because the caller, not
/// [`spec_component`], decides what an unresolved value means at its position
/// (PRD `dimension-checked-readers.md` decision 2).
pub(super) enum ComponentRead {
    /// The value has the expected dimension; carries the SI f64.
    Accepted(f64),
    /// The value is `Value::Undef` — an unresolved cell, not a mistake. The
    /// caller decides what that means at this position.
    Undefined,
    /// The value is defined but the wrong shape/dimension.
    Rejected(FeaValueShapeError),
}

/// Classify `v` against `spec` through `accept_arg`. A refused `Value::Scalar`
/// is `WrongDimension`, naming the spec's `expected` type; any other refused
/// value is `ExpectedScalar`. Both reuse the rejection's `got` label.
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

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;

    use super::*;
    use crate::compute_targets::{scalar, structure};

    fn law(field: &str, value: Value) -> StructureInstanceData {
        match structure("OrthotropicMaterial", vec![(field, value)]) {
            Value::StructureInstance(data) => *data,
            other => unreachable!("`structure` builds a StructureInstance, got {other:?}"),
        }
    }

    fn refusal_of(res: Result<f64, FeaValueShapeError>) -> Option<(String, &'static str)> {
        match res {
            Err(FeaValueShapeError::Rejected(FieldRejection { path, rejection })) => {
                Some((path, rejection.expected))
            }
            _ => None,
        }
    }

    /// A modulus is a Pressure `Scalar`: a LENGTH or dimensionless `Scalar`,
    /// or a bare number, is refused rather than read as pascals.
    #[test]
    fn modulus_field_reads_a_pressure_and_refuses_every_other_value() {
        assert_eq!(
            modulus_field(&law("e1", scalar(2.0e11, DimensionVector::PRESSURE)), "e1"),
            Ok(2.0e11)
        );
        for refused in [
            scalar(2.0e11, DimensionVector::LENGTH),
            scalar(2.0e11, DimensionVector::DIMENSIONLESS),
            Value::Real(2.0e11),
            Value::Bool(true),
        ] {
            assert_eq!(
                refusal_of(modulus_field(&law("e1", refused.clone()), "e1")),
                Some(("OrthotropicMaterial.e1".to_string(), "Pressure")),
                "a modulus {refused:?} must be refused naming the field"
            );
        }
    }

    /// A Poisson ratio takes every dimensionless spelling, and only those.
    #[test]
    fn ratio_field_reads_every_dimensionless_spelling_and_refuses_a_dimension() {
        for (ratio, expected) in [
            (Value::Real(0.25), 0.25),
            (Value::Int(0), 0.0),
            (scalar(0.25, DimensionVector::DIMENSIONLESS), 0.25),
        ] {
            assert_eq!(
                ratio_field(&law("nu12", ratio.clone()), "nu12"),
                Ok(expected),
                "a ratio {ratio:?} must read"
            );
        }
        assert_eq!(
            refusal_of(ratio_field(
                &law("nu12", scalar(0.25, DimensionVector::PRESSURE)),
                "nu12"
            )),
            Some(("OrthotropicMaterial.nu12".to_string(), "Real"))
        );
    }

    /// A required field that is not there is missing; one that is there but
    /// has not resolved is not, so the two are told apart.
    #[test]
    fn a_required_field_tells_absent_from_unresolved() {
        let absent_spellings = [
            law("e2", scalar(1.0, DimensionVector::PRESSURE)),
            law("e1", Value::Option(None)),
        ];
        for absent in absent_spellings {
            assert!(
                matches!(
                    modulus_field(&absent, "e1"),
                    Err(FeaValueShapeError::MissingField { field: "e1", .. })
                ),
                "an absent or `none` e1 must be MissingField"
            );
        }
        let some_undef = Value::Option(Some(Box::new(Value::Undef)));
        for unresolved in [Value::Undef, some_undef] {
            let res = ratio_field(&law("nu12", unresolved.clone()), "nu12");
            assert!(
                matches!(
                    &res,
                    Err(FeaValueShapeError::Unresolved { field: "nu12", .. })
                ),
                "an unresolved nu12 ({unresolved:?}) must be Unresolved, got {res:?}"
            );
        }
    }
}
