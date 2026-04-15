/// Shared test helpers for unit-related integration test binaries.
///
/// Include in a test binary with `mod common;` at the top of the file.
/// Helpers are `pub` so they are visible after `use common::{...}`.
///
/// Most helpers have migrated to `reify_test_support`. This module retains only:
/// - `compile_with_stdlib_helper` — used by `imperial_units_tests.rs`
/// - `assert_single_non_empty_label` — specific to unit collision diagnostic tests
use reify_compiler::{CompiledModule, compile_with_prelude, stdlib_loader};
use reify_types::{Diagnostic, ModulePath, SourceSpan};

/// Parse `source` and compile it with the full stdlib prelude seeded into the
/// unit registry.  Panics if the parser returns any errors.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn compile_with_stdlib_helper(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, stdlib_loader::load_stdlib())
}

/// Assert that `diag` emits exactly one label and that the label's span is not
/// `SourceSpan::empty(0)`.
///
/// Used to guard the "exactly one non-empty label" invariant for cross-module
/// user unit collision diagnostics in both `module_dag_tests` and
/// `unit_registry_tests`.  The two test files share identical assertion logic,
/// so a single helper eliminates the duplication while keeping the loop-free,
/// direct-index form that follows naturally from the count assertion.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn assert_single_non_empty_label(diag: &Diagnostic) {
    assert_eq!(
        diag.labels.len(),
        1,
        "diagnostic should emit exactly one label, got {:?}",
        diag.labels
    );
    let empty_span = SourceSpan::empty(0);
    assert_ne!(
        diag.labels[0].span,
        empty_span,
        "diagnostic label '{}' has SourceSpan::empty(0) — misleading offset",
        diag.labels[0].message
    );
}
