//! PDIAG fixture — ONE code-less `Diagnostic::error(...)` at a swept path.
//!
//! Lifted into a throwaway git repo by `tests/cli.rs`, where the fixture root
//! IS the project root, so this file's tracked path is
//! `crates/reify-eval/src/scenario01_codeless.rs` — squarely inside the sweep.
//! In THIS repo it sits under `crates/reify-audit/`, which
//! `SCOPE_EXCLUDE_PREFIXES` excludes, so the detector never scans its own
//! fixtures.
//!
//! Expected: one `NewFile` High (no baseline exists in the fixture tree).

pub fn emit(out: &mut Vec<Diagnostic>) {
    out.push(Diagnostic::error(format!("shell extraction failed at face {}", 3)));
}

pub struct Diagnostic;

impl Diagnostic {
    pub fn error(_message: String) -> Self {
        Self
    }
}
