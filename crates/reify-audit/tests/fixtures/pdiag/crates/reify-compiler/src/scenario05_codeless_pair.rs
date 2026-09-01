//! PDIAG fixture — TWO code-less sites in one file, in a second crate.
//!
//! Load-bearing against an off-by-design error: the exit code is the count of
//! High FINDINGS, and a file with any number of code-less sites yields exactly
//! ONE `NewFile` finding — so the exit code counts code-less FILES, not sites,
//! and this file (two sites, one finding) is what makes the two differ.
//!
//! The tree-wide arithmetic is deliberately NOT restated here: it changes
//! every time a scenario is added, and this doc block went stale exactly that
//! way once already. `tests/cli.rs::
//! pdiag_fixture_tree_hard_gates_code_less_files_and_suppresses_the_rest`
//! owns the expected count and enumerates the contributing files in its
//! assertion message — read it there.

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
