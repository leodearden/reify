//! PDIAG fixture — the backwards-escape leak.
//!
//! One brand-new UNREVIEWED code-less site sits immediately above a second
//! constructor carrying a reviewed trailing opt-out. Before the escape was
//! bounded by the nearest constructor above it, the lower opt-out absorbed the
//! upper site and this whole file contributed nothing — a silent INV-SF-6
//! hard-gate bypass available to any author willing to add a constructor
//! within 15 non-comment lines above an existing escape.
//!
//! The leak was invisible to every other test in the suite, including the
//! rest of this fixture tree, because no other fixture puts an unescaped site
//! inside an escape's window.
//!
//! Expected: ONE `NewFile` High for this file — the unreviewed site counted,
//! the reviewed one still suppressed.

pub fn emit(out: &mut Vec<Diagnostic>) {
    // Unreviewed: added later, and must not inherit the opt-out below it.
    out.push(Diagnostic::error("shell extraction failed".to_string()));
    out.push(Diagnostic::error("E_DFM_WALL_THIN".to_string())); // pdiag:allow — DFM prefix convention
}

pub struct Diagnostic;

impl Diagnostic {
    pub fn error(_message: String) -> Self {
        Self
    }
}
