//! PDIAG fixture — a neighbouring constructor's code absorbing a new site.
//!
//! Two brand-new code-less sites, each parked exactly where a CODED
//! constructor's `.with_code(` used to swallow it: one directly ABOVE the
//! coded site, one to its LEFT on a single line. Real statement terminators
//! are load-bearing here — unlike scenario01/06, which omit them — because the
//! probe is bounded by the ordered pair `; … <anchor>` and by neither half
//! alone.
//!
//! Under the UNBOUNDED probe this file censuses ZERO code-less sites: it is
//! the leak itself, invisible to the whole rest of this fixture tree. Under
//! the bounded probe it censuses exactly TWO, one per half. A fixture that
//! would red either way would prove nothing.
//!
//! Expected: ONE `NewFile` High for this file, naming BOTH code-less sites.

pub fn emit(out: &mut Vec<Diagnostic>, code: DiagnosticCode) {
    // Below-lines half: the coded site one line down used to absorb this one.
    out.push(Diagnostic::error("shell extraction failed".to_string()));
    out.push(Diagnostic::warning("wall thinner than draft".to_string()).with_code(code));

    // Same-line half: the coded site to the RIGHT used to absorb this one.
    out.push(Diagnostic::error("undercut".to_string())); out.push(Diagnostic::warning("draft".to_string()).with_code(code));
}

pub struct Diagnostic;

pub struct DiagnosticCode;

impl Diagnostic {
    pub fn error(_message: String) -> Self {
        Self
    }

    pub fn warning(_message: String) -> Self {
        Self
    }

    pub fn with_code(self, _code: DiagnosticCode) -> Self {
        Self
    }
}
