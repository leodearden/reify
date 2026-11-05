//! The identity under which two diagnostics count as the same finding.

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity};

/// Two diagnostics are the same finding exactly when they agree on severity,
/// code and message. Labels (and so spans) and candidates do not participate.
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
