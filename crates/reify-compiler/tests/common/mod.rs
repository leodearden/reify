/// Shared test helpers for unit-related integration test binaries.
///
/// Include in a test binary with `mod common;` at the top of the file.
/// All three helper functions are `pub` so they are visible after `use common::{...}`.
use reify_compiler::{CompiledModule, compile, compile_with_prelude, stdlib_loader};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, Diagnostic, DimensionVector, ModulePath, Severity,
    SourceSpan, Value,
};

/// Parse `source` and compile it as a single module named `"unit_test"`.
/// Panics if the parser returns any errors.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
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
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

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
pub fn expect_scalar(expr: &CompiledExpr) -> (f64, DimensionVector) {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, dimension }) => {
            (*si_value, *dimension)
        }
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
pub fn expect_binop(expr: &CompiledExpr) -> (&BinOp, &CompiledExpr, &CompiledExpr) {
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => (op, left, right),
        other => panic!(
            "expected CompiledExprKind::BinOp {{ .. }}, got {:?}",
            other
        ),
    }
}
