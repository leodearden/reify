//! Trait member merging eval tests — task 190.
//!
//! Full pipeline (parse → compile → eval/check) tests verifying that merged
//! trait constraints are actually enforced and let defaults are evaluated.

use reify_types::{ModulePath, Satisfaction, Severity};

// ── Helper ───────────────────────────────────────────────────────────────────

/// Parse `source`, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
fn parse_compile_check(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    compiled
}
