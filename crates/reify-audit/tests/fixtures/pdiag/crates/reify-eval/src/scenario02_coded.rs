//! PDIAG fixture — every site carries a code. Contributes ZERO findings.
//!
//! Covers both coded shapes the detector must accept: the code on the anchor
//! line, and the code arriving several lines later after a wrapped `format!`
//! (the dominant real shape — only ~16% of sites fit on one line).

pub fn emit(out: &mut Vec<Diagnostic>, code: Code) {
    out.push(Diagnostic::error("same-line".to_string()).with_code(code));

    out.push(
        Diagnostic::warning(format!(
            "tolerance {} exceeds the configured envelope of {}",
            0.5, 0.1
        ))
        .with_code(code),
    );
}

pub struct Diagnostic;
#[derive(Clone, Copy)]
pub struct Code;

impl Diagnostic {
    pub fn error(_message: String) -> Self {
        Self
    }
    pub fn warning(_message: String) -> Self {
        Self
    }
    pub fn with_code(self, _code: Code) -> Self {
        self
    }
}
