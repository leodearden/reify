//! The FEA value-shape error, and how it becomes a `Failed` outcome.
//!
//! Shared by the `solve_elastic_static` and `solve_buckling` trampolines: both
//! validate their inputs through Result-returning readers and report the first
//! `Err` as ONE Error diagnostic naming the trampoline and the offending arg.

use reify_core::{Diagnostic, DiagnosticCode};

use crate::ComputeOutcome;

/// Errors produced by the FEA `Value`-shape leaf extractors/classifiers when
/// the input does not match the expected shape (PRD compute-fea-hardening
/// §C3).
///
/// This module's extract/classify helpers were migrated (tasks D2-D9) from
/// "panic on malformed `Value`" to `Result<_, FeaValueShapeError>`. D9 (task
/// 5087) completed the migration: every production call site in
/// `solve_elastic_static_trampoline` now runs through that fn's
/// validate-all-inputs gate, which `match`es each `Result` and returns
/// `ComputeOutcome::Failed` with a diagnostic naming the offending arg on the
/// first `Err`, instead of panicking. The transitional `FeaShimExt::fea_shim`
/// bridging shim (D2-D8) has been removed.
#[allow(dead_code)] // ExpectedStructureInstance is constructed by D3/D5/D7, not D2
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
    /// A present, defined `Value::Scalar` whose dimension is not the one this
    /// reader position expects (task #7019). Distinct from `ExpectedScalar`,
    /// which fires when the value is not a `Value::Scalar` at all: this
    /// variant is for the wrong dimension on the RIGHT shape — e.g. a
    /// LENGTH-dimensioned Scalar at a position that expects DIMENSIONLESS,
    /// or vice versa. `expected` names the position's `ArgSpec::type_name`.
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
    ComputeOutcome::Failed {
        diagnostics: vec![diagnostic],
        structured_detail: vec![],
    }
}
