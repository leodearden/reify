//! The FEA value-shape error, and how it becomes a `Failed` outcome.
//!
//! Shared by the `solve_elastic_static` and `solve_buckling` trampolines: both
//! validate their inputs through Result-returning readers and report the first
//! `Err` as ONE Error diagnostic naming the trampoline and the offending arg,
//! or the offending field when a reader refused one.

use reify_core::{Diagnostic, DiagnosticCode};

use super::rejected_input_outcome;
use crate::ComputeOutcome;
use crate::arg_acceptance::FieldRejection;

/// Why an FEA input `Value` does not have the shape its reader expects (PRD
/// compute-fea-hardening §C3). The readers that produce it live in
/// `elastic_static`, `fea_inputs`, `fea_loads` and `buckling`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum FeaValueShapeError {
    /// Expected a `Value::StructureInstance`, got something else.
    ExpectedStructureInstance { context: &'static str, got: String },
    /// Expected a `Value::Scalar { .. }`, got something else.
    ExpectedScalar { context: &'static str, got: String },
    /// Expected a `Value::Real`, got something else.
    ExpectedReal { context: &'static str, got: String },
    /// Expected an ordered component sequence (`Value::List`/`Point`/`Vector`),
    /// got something else, or the wrong arity.
    ExpectedList { context: &'static str, got: String },
    /// A required `StructureInstance` field was absent.
    MissingField {
        context: &'static str,
        field: &'static str,
    },
    /// A `Value::Scalar` component of the wrong dimension, raised by task
    /// #7019's component readers (any other value is their `ExpectedScalar`).
    /// `expected` names the spec's `type_name`.
    ///
    /// Transitional: it is the same rejection reason as [`Self::Rejected`] in
    /// a second wording. `Rejected` is the target shape; once those readers
    /// move onto it, this variant goes.
    WrongDimension {
        context: &'static str,
        expected: &'static str,
        got: String,
    },
    /// A present field value its reader's `ArgSpec` refuses, a non-number
    /// included: a `fea_loads` magnitude, lifted by `?` through the `From`
    /// impl below. Rendered through [`FieldRejection::to_error`], the wording
    /// every dimension-checked reader shares.
    Rejected(FieldRejection),
}

impl std::fmt::Display for FeaValueShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeaValueShapeError::ExpectedStructureInstance { context, got } => write!(
                f,
                "expected Value::StructureInstance for {context}, got {got}"
            ),
            FeaValueShapeError::ExpectedScalar { context, got } => {
                write!(f, "expected Value::Scalar for {context}, got {got}")
            }
            FeaValueShapeError::ExpectedReal { context, got } => {
                write!(f, "expected Value::Real for {context}, got {got}")
            }
            FeaValueShapeError::ExpectedList { context, got } => write!(
                f,
                "expected a list-shaped Value (List/Point/Vector) for {context}, got {got}"
            ),
            FeaValueShapeError::MissingField { context, field } => {
                write!(f, "missing field {field:?} for {context}")
            }
            FeaValueShapeError::WrongDimension {
                context,
                expected,
                got,
            } => {
                write!(f, "expected {expected} for {context}, got {got}")
            }
            FeaValueShapeError::Rejected(field) => write!(f, "{field}"),
        }
    }
}

impl FeaValueShapeError {
    /// The structured code a consumer matches on (INV-SF-6): a wrong
    /// dimension is a `DimensionedArgRejected`; the shape faults carry none.
    pub(super) fn diagnostic_code(&self) -> Option<DiagnosticCode> {
        match self {
            FeaValueShapeError::WrongDimension { .. } | FeaValueShapeError::Rejected(_) => {
                Some(DiagnosticCode::DimensionedArgRejected)
            }
            FeaValueShapeError::ExpectedStructureInstance { .. }
            | FeaValueShapeError::ExpectedScalar { .. }
            | FeaValueShapeError::ExpectedReal { .. }
            | FeaValueShapeError::ExpectedList { .. }
            | FeaValueShapeError::MissingField { .. } => None,
        }
    }
}

impl From<FieldRejection> for FeaValueShapeError {
    fn from(rejection: FieldRejection) -> Self {
        FeaValueShapeError::Rejected(rejection)
    }
}

/// The `Failed` outcome for a refused field: one `DimensionedArgRejected`
/// Error in the shared rejection wording, naming the field's own
/// `<Struct>.<field>` path. A reader that can only refuse a field returns a
/// bare [`FieldRejection`] and reaches this directly, with no arg label.
pub(super) fn rejected_field_outcome(
    trampoline: &str,
    rejection: &FieldRejection,
) -> ComputeOutcome {
    rejected_input_outcome(rejection.to_error(trampoline))
}

/// The `Failed` outcome a trampoline's input gate returns for `err`, read from
/// the input labelled `arg`. The label is rendered for a shape fault only, as
/// `"{trampoline}: {arg}: {err}"`, coded when the fault has a code; a
/// `Rejected` already names its field and goes to [`rejected_field_outcome`].
pub(super) fn failed_outcome(
    trampoline: &str,
    arg: &str,
    err: &FeaValueShapeError,
) -> ComputeOutcome {
    match err {
        FeaValueShapeError::Rejected(rejection) => rejected_field_outcome(trampoline, rejection),
        shape => rejected_input_outcome(shape_fault_error(trampoline, arg, shape)),
    }
}

fn shape_fault_error(trampoline: &str, arg: &str, shape: &FeaValueShapeError) -> Diagnostic {
    let diagnostic = Diagnostic::error(format!("{trampoline}: {arg}: {shape}"));
    match shape.diagnostic_code() {
        Some(code) => diagnostic.with_code(code),
        None => diagnostic,
    }
}
