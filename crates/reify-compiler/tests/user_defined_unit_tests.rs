//! Tests for user-defined units: cross-module integration (task 209).
//!
//! Validates that `pub unit` declarations from one module are properly seeded
//! into the unit registry of importing modules, and that private units remain
//! invisible across module boundaries.

use std::fs;
use std::path::PathBuf;

use reify_compiler::{CompiledModule, compile, compile_with_prelude};
use reify_types::{ModulePath, Severity};

// ─── helpers ───────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile(&parsed)
}

fn compile_with_prelude_helper(source: &str, prelude: &[CompiledModule]) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, prelude)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Create a unique temp directory for filesystem-based tests.
fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("reify_unit_test_209")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ─── step-1: user-declared unit works in a let binding expression ─────────────

#[test]
fn user_unit_in_let_binding() {
    // Declare `thou`, use it in a param default and a let binding.
    // Verifies that QuantityLiteral resolution works in all expression contexts,
    // not only param defaults.
    let module = parse_and_compile(
        "unit thou : Length = 0.0000254\n\
         structure S {\n\
             param w : Length = 10thou\n\
             let w_thou = w + 5thou\n\
         }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    // Let binding should have produced a value cell
    let w_thou = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w_thou")
        .expect("w_thou value cell not found");
    assert!(
        w_thou.default_expr.is_some(),
        "w_thou should have a computed expression"
    );
}
