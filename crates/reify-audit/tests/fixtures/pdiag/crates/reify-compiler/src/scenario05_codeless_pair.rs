//! PDIAG fixture — TWO code-less sites in one file, in a second crate.
//!
//! Load-bearing against an off-by-design error: the exit code is the count of
//! High FINDINGS, and a file with any number of code-less sites yields exactly
//! ONE `NewFile` finding. Together with `scenario01_codeless.rs` the fixture
//! tree holds 3 code-less sites across 2 files, so an exit code of 2 (not 3)
//! is what proves the ratchet is per-file.

pub fn emit(out: &mut Vec<Diagnostic>) {
    out.push(Diagnostic::error("unresolved parameter binding".to_string()));

    out.push(Diagnostic::warning(format!(
        "shadowed definition of {} — the outer binding is unreachable",
        "radius"
    )));
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
