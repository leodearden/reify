//! PDIAG fixture — code-less sites under the reviewed `pdiag:allow` opt-out.
//! Contributes ZERO findings.
//!
//! The archetype is `crates/reify-stdlib/src/dfm.rs`, whose severity-
//! parameterized `{I,W,E}_DFM_*` message-prefix convention is documented as
//! code-less by design.

pub fn emit(out: &mut Vec<Diagnostic>) {
    out.push(Diagnostic::error("E_DFM_WALL_THIN".to_string())); // pdiag:allow — DFM prefix convention

    // The escape is honoured anywhere in the site's forward window, including
    // on a comment-only line of its own.
    out.push(Diagnostic::warning(format!(
        "W_DFM_DRAFT_ANGLE below {} degrees",
        1.5
    )));
    // pdiag:allow — DFM prefix convention
}

pub struct Diagnostic;

impl Diagnostic {
    pub fn error(_message: String) -> Self {
        Self
    }
    pub fn warning(_message: String) -> Self {
        Self
    }
}
