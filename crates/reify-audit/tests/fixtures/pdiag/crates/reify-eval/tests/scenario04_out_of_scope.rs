//! PDIAG fixture — the out-of-scope control. Contributes ZERO findings.
//!
//! Five code-less sites, all invisible to the ratchet because this path has a
//! `tests` segment. INV-SF-6 governs *emitted* diagnostics; test scaffolding
//! that fabricates a `Diagnostic` to assert on is out of scope by
//! construction, not by exemption — which is why there is no escape comment
//! here and none is needed.

#[test]
fn diagnostics_render_their_message() {
    let _ = Diagnostic::error("a".to_string());
    let _ = Diagnostic::error("b".to_string());
    let _ = Diagnostic::warning("c".to_string());
    let _ = Diagnostic::warning("d".to_string());
    let _ = Diagnostic::error("e".to_string());
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
