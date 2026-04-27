//! Tests for the std.io stdlib module.
//!
//! Exercises: module presence, marker traits (Source/Sink), enums
//! (DiscardReason/DisposalMethod/OutputFormat), Provenance structure, and the
//! four refining traits (Input, Buy, Output, Discard) including Buy.unit_cost
//! having Money dimension.
//!
//! File-stem `io_traits` matches the `cargo test -p reify-compiler -- io_traits`
//! filter used in the task testStrategy.

use reify_compiler::stdlib_loader;
use reify_test_support::collect_errors;

// ─── step-1: module load ─────────────────────────────────────────────────────

/// The std.io module is present in the stdlib and compiles without errors.
#[test]
fn std_io_module_present_and_compiles_clean() {
    let modules = stdlib_loader::load_stdlib();

    let io_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib");

    let errors = collect_errors(&io_module.diagnostics);
    assert!(
        errors.is_empty(),
        "std.io module should have no error diagnostics, got: {:?}",
        errors
    );
}
