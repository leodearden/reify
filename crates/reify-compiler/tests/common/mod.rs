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

use reify_compiler::{CompiledModule, module_dag::ModuleResolver, stdlib_loader};
use reify_types::{Diagnostic, DimensionVector, Severity, SourceSpan};

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

/// Return the compiled `std/units` module from the cached stdlib.
///
/// Uses the `OnceLock`-backed `load_stdlib()` so repeated calls are free.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn units_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/units")
        .expect("std/units module not found")
}

/// Assert `a ≈ b` within `max(|a|, |b|) * rel_tol`.
///
/// Uses the larger magnitude as the scale so the tolerance is robust when one
/// operand is zero. When both operands are zero, the scale is zero too — falls
/// back to `rel_tol` as an absolute tolerance so the comparison still
/// distinguishes 0 from a tiny non-zero value rather than producing `tol = 0`
/// (which would falsely fail on bit-equal zeros under `<`).
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn assert_eq_rel(a: f64, b: f64, rel_tol: f64, msg: &str) {
    let scale = a.abs().max(b.abs());
    let tol = if scale == 0.0 { rel_tol } else { scale * rel_tol };
    assert!(
        (a - b).abs() < tol,
        "{}: expected {} ≈ {} (tol {})",
        msg,
        a,
        b,
        tol
    );
}

/// Assert that a named unit in `std/units` has the expected dimension, factor
/// (within `rel_tol` relative tolerance), and no offset.
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn assert_simple_unit(
    name: &str,
    expected_dim: DimensionVector,
    expected_factor: f64,
    rel_tol: f64,
) {
    let module = units_module();
    let u = module
        .units
        .iter()
        .find(|u| u.name == name)
        .unwrap_or_else(|| panic!("unit '{}' not found in std/units", name));
    assert_eq!(u.dimension, expected_dim, "unit '{}' dimension wrong", name);
    assert_eq_rel(
        u.factor,
        expected_factor,
        rel_tol,
        &format!("unit '{}' factor", name),
    );
    assert!(u.offset.is_none(), "unit '{}' should have no offset", name);
}

/// Compile a structure with a single default-valued param and return the
/// Scalar's (si_value, dimension) from its default expression.
///
/// Source compiled: `structure def S { param x : <param_type> = <literal> }`
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn stdlib_param_si_value(param_type: &str, literal: &str) -> (f64, DimensionVector) {
    let source = format!(
        "structure def S {{ param x : {} = {} }}",
        param_type, literal
    );
    let module = compile_with_stdlib_helper(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "source `{}` produced errors: {:?}",
        source,
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x cell not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    expect_scalar(expr)
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

/// Assert that `trait_def` has a `DefaultKind::Constraint` whose expression is
/// `BinOp { op: expected_op, left: Ident(expected_member), right: NumberLiteral(rhs) }`
/// where `|rhs - expected_rhs| <= rhs_epsilon`.
///
/// This tightens the constraint-present check: a regression that flips the
/// operator (e.g. `>=` → `>`) or changes the bound (e.g. `0.0` instead of
/// `1500.0`) will fail here.  Shape is:
///   `BinOp { op: expected_op, left: Ident(expected_member), right: NumberLiteral(rhs) }`
/// with `|rhs - expected_rhs| <= rhs_epsilon`.
#[track_caller]
#[allow(dead_code)] // used by some, but not all, test binaries that include this module
pub fn assert_trait_constraint_binop(
    trait_def: &reify_compiler::CompiledTrait,
    trait_name: &str,
    expected_member: &str,
    expected_op: &str,
    expected_rhs: f64,
    rhs_epsilon: f64,
) {
    use reify_compiler::DefaultKind;
    use reify_syntax::ExprKind;

    let constraint_default = trait_def
        .defaults
        .iter()
        .find(|d| {
            if let DefaultKind::Constraint(decl) = &d.kind {
                matches!(&decl.expr.kind, ExprKind::BinOp { left, .. }
                    if matches!(&left.kind, ExprKind::Ident(n) if n == expected_member))
            } else {
                false
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "{} must have a constraint default on '{}', got defaults: {:?}",
                trait_name,
                expected_member,
                trait_def
                    .defaults
                    .iter()
                    .map(|d| format!("{:?}", d.kind))
                    .collect::<Vec<_>>()
            )
        });

    if let DefaultKind::Constraint(decl) = &constraint_default.kind {
        if let ExprKind::BinOp { op, left: _, right } = &decl.expr.kind {
            assert_eq!(
                op.as_str(),
                expected_op,
                "{} constraint op for '{}' should be '{}', got '{}'",
                trait_name,
                expected_member,
                expected_op,
                op
            );
            match &right.kind {
                ExprKind::NumberLiteral(v) => assert!(
                    (*v - expected_rhs).abs() <= rhs_epsilon,
                    "{} constraint RHS for '{}' should be {} (±{}), got {}",
                    trait_name,
                    expected_member,
                    expected_rhs,
                    rhs_epsilon,
                    v
                ),
                other => panic!(
                    "{} constraint RHS for '{}' should be NumberLiteral, got {:?}",
                    trait_name, expected_member, other
                ),
            }
        }
    }
}
