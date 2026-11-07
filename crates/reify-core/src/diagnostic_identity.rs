//! The identity under which two diagnostics read as the same reported line.

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};

/// Equal exactly when two diagnostics agree on severity, code and message.
/// Labels (and so spans) and candidates do not participate, so equal
/// identities do NOT prove one finding: two distinct findings can read
/// alike, and a consumer that must keep multiplicity must not collapse
/// diagnostics on this key alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagnosticIdentity<'d> {
    severity: Severity,
    code: Option<DiagnosticCode>,
    message: &'d str,
}

impl<'d> DiagnosticIdentity<'d> {
    pub fn of(diagnostic: &'d Diagnostic) -> Self {
        Self {
            severity: diagnostic.severity,
            code: diagnostic.code,
            message: &diagnostic.message,
        }
    }
}
