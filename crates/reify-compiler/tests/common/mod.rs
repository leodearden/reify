/// Shared test helpers for unit-related integration test binaries.
///
/// Include in a test binary with `mod common;` at the top of the file.
/// All three helper functions are `pub` so they are visible after `use common::{...}`.
use reify_compiler::{CompiledModule, compile, compile_with_prelude, stdlib_loader};
use reify_types::{ModulePath, Severity};

/// Parse `source` and compile it as a single module named `"unit_test"`.
/// Panics if the parser returns any errors.
pub fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile(&parsed)
}

/// Return only the `Severity::Error` diagnostics from a compiled module.
pub fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Parse `source` and compile it with the full stdlib prelude seeded into the
/// unit registry.  Panics if the parser returns any errors.
pub fn compile_with_stdlib_helper(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, stdlib_loader::load_stdlib())
}
