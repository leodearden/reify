//! The FEA value-shape error, and how it becomes a `Failed` outcome.
//!
//! Shared by the `solve_elastic_static` and `solve_buckling` trampolines: both
//! validate their inputs through Result-returning readers and report the first
//! `Err` as ONE Error diagnostic naming the trampoline and the offending arg.

use reify_core::{Diagnostic, DiagnosticCode};

use super::rejected_input_outcome;
use crate::ComputeOutcome;

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
    /// A present, defined value the position's `ArgSpec` refuses. `expected`
    /// names the spec's `type_name`. Task #7019's component readers raise it
    /// for a `Value::Scalar` of the wrong dimension only (any other value is
    /// their `ExpectedScalar`); the `fea_loads` magnitudes raise it for every
    /// value `read_arg_or_bare` refuses, a non-number included (task 6922).
    WrongDimension {
        context: &'static str,
        expected: &'static str,
        got: String,
    },
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
        }
    }
}

impl FeaValueShapeError {
    /// The structured code a consumer matches on (INV-SF-6): a wrong
    /// dimension is a `DimensionedArgRejected`; the shape faults carry none.
    pub(super) fn diagnostic_code(&self) -> Option<DiagnosticCode> {
        match self {
            FeaValueShapeError::WrongDimension { .. } => {
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

/// The `Failed` outcome a trampoline's input gate returns for `err` at `arg`:
/// one Error diagnostic, `"{trampoline}: {arg}: {err}"`, coded when the error
/// has a code.
pub(super) fn failed_outcome(
    trampoline: &str,
    arg: &str,
    err: &FeaValueShapeError,
) -> ComputeOutcome {
    let mut diagnostic = Diagnostic::error(format!("{trampoline}: {arg}: {err}"));
    if let Some(code) = err.diagnostic_code() {
        diagnostic = diagnostic.with_code(code);
    }
    rejected_input_outcome(diagnostic)
}
