//! Trait member merging tests — task 190.
//!
//! Focused tests for the trait member merging behaviour:
//! two-trait merge, shared-param dedup, diamond dedup,
//! conflict detection, constraint conjunction, and let-binding merge/conflict.

use reify_compiler::*;
use reify_types::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Parse `source` and compile, returning the full CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse `source`, compile, and return the first template together with all
/// diagnostics emitted during compilation.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected at least 1 template");
    (template, module.diagnostics)
}
