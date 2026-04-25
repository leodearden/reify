//! Shared test helpers for unit-related integration test binaries.
//!
//! Include in a test binary with `mod common;` at the top of the file.
//! Helpers are `pub` so they are visible after `use common::{...}`.
//!
//! Most helpers have migrated to `reify_test_support`. This module retains only:
//! - `compile_with_stdlib_helper` — delegates to `reify_test_support::compile_source_with_stdlib`
//! - `assert_single_non_empty_label` — specific to unit collision diagnostic tests
//! - `compile_errors` / `compile_errors_with_stdlib` — compile a project and return Error-severity diagnostics

use std::path::Path;

use reify_compiler::module_dag::ModuleResolver;
use reify_types::{Diagnostic, Severity, SourceSpan};

/// Parse `source` and compile it with the full stdlib prelude seeded into the
/// unit registry.  Panics if the parser returns any errors.
///
/// Delegates to [`reify_test_support::compile_source_with_stdlib`]; kept as a
/// thin wrapper so that `imperial_units_tests.rs` (outside this task's scope)
/// can continue importing via `mod common`.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn compile_with_stdlib_helper(source: &str) -> reify_compiler::CompiledModule {
    reify_test_support::compile_source_with_stdlib(source)
}

/// Compile the named entry file within `dir` using `stdlib` as the stdlib root
/// and return the Error-severity diagnostics from the last (entry) module.
///
/// This is the flexible variant used when the test needs a custom stdlib
/// directory (e.g. one built inside the test's temp dir).  Panics if
/// `compile_project` returns `Err` or yields no modules.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn compile_errors_with_stdlib(dir: &Path, entry: &str, stdlib: &Path) -> Vec<Diagnostic> {
    let resolver = ModuleResolver::new(dir, stdlib);
    let result = reify_compiler::module_dag::compile_project(&dir.join(entry), &resolver);
    let modules = result.expect("compile_project should return Ok even with diagnostics");
    let last = modules.into_iter().last().expect("no modules returned");
    last.diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Compile the named entry file within `dir` using `dir/stdlib` as the stdlib
/// root and return the Error-severity diagnostics from the last (entry) module.
///
/// Delegates to [`compile_errors_with_stdlib`] with the conventional default
/// stdlib path `dir.join("stdlib")`.  Panics if `compile_project` returns
/// `Err` or yields no modules.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn compile_errors(dir: &Path, entry: &str) -> Vec<Diagnostic> {
    compile_errors_with_stdlib(dir, entry, &dir.join("stdlib"))
}

/// Assert that `diag` emits exactly two labels with the prelude-collision shape:
/// - `labels[0]`: user's in-file duplicate decl (span is not empty and not
///   `SourceSpan::empty(0)`)
/// - `labels[1]`: prelude sentinel (`span.is_prelude()`) with a message that
///   contains "prelude" to convey provenance
///
/// Used by cross-module user unit collision diagnostics in `module_dag_tests`,
/// `unit_registry_tests`, and `user_defined_unit_tests`.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn assert_prelude_collision_labels(diag: &Diagnostic) {
    assert_eq!(
        diag.labels.len(),
        2,
        "diagnostic should emit exactly two labels (user dup + prelude sentinel), got {:?}",
        diag.labels
    );
    let empty_span = SourceSpan::empty(0);
    assert_ne!(
        diag.labels[0].span, empty_span,
        "first label '{}' must not be SourceSpan::empty(0)",
        diag.labels[0].message
    );
    assert!(
        diag.labels[1].span.is_prelude(),
        "second label '{}' must have is_prelude() span, got {:?}",
        diag.labels[1].message,
        diag.labels[1].span
    );
    assert!(
        diag.labels[1].message.contains("prelude"),
        "second label message must contain 'prelude', got: {:?}",
        diag.labels[1].message
    );
}

/// Standard tolerance for unit SI-value assertions across the test suite.
///
/// All unit SI-value comparisons (e.g. `10mm → 0.01 m`) use this epsilon.
/// A tighter tolerance (e.g. 1e-10) is unnecessary and risks spurious failures
/// on platforms with slightly different FP rounding.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub const UNIT_EPSILON: f64 = 1e-9;

/// Extract a `(si_value, dimension)` pair from a `Scalar` literal expression.
///
/// Panics with a descriptive message if `expr` is not a
/// `CompiledExprKind::Literal(Value::Scalar { .. })`.  Use this instead of
/// the three-level `if let` / `else panic!` pattern that previously appeared
/// at every scalar-assertion site.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn expect_scalar(expr: &reify_types::CompiledExpr) -> (f64, reify_types::DimensionVector) {
    match &expr.kind {
        reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value,
            dimension,
        }) => (*si_value, *dimension),
        other => panic!(
            "expected CompiledExprKind::Literal(Value::Scalar {{ .. }}), got {:?}",
            other
        ),
    }
}

/// Extract an `(op, left, right)` triple from a `BinOp` expression.
///
/// Panics with a descriptive message if `expr` is not a
/// `CompiledExprKind::BinOp { .. }`.  Use this instead of the nested
/// `if let` / `else panic!` pattern that previously appeared at every
/// BinOp-assertion site.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn expect_binop(
    expr: &reify_types::CompiledExpr,
) -> (
    &reify_types::BinOp,
    &reify_types::CompiledExpr,
    &reify_types::CompiledExpr,
) {
    match &expr.kind {
        reify_types::CompiledExprKind::BinOp { op, left, right } => (op, left, right),
        other => panic!("expected CompiledExprKind::BinOp {{ .. }}, got {:?}", other),
    }
}
