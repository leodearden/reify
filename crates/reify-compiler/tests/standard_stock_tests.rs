//! Tests for the `std.stock` stdlib module — standard bolt lengths and
//! sheet thickness collections.

use reify_compiler::stdlib_loader;
use reify_test_support::collect_errors;
use reify_types::{DimensionVector, Type, Value, ValueMap};

/// Helper: load the stdlib and find the std.stock CompiledModule.
fn stock_module() -> &'static reify_compiler::CompiledModule {
    let modules = stdlib_loader::load_stdlib();
    modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/stock")
        .expect("std.stock module not found in stdlib")
}

// ─── step-1: module loads with no errors ─────────────────────────────────────

/// std.stock module is present in the stdlib and has zero error diagnostics.
#[test]
fn std_stock_module_loads_with_no_errors() {
    let module = stock_module();
    let errors = collect_errors(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "std.stock module has error diagnostics: {:?}",
        errors
    );
}
