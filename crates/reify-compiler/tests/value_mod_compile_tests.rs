//! Compile-typing integration tests for the value-level `%` operator (task 3916).
//!
//! Tests the type-checker Int-only guard in `expr.rs` (spec §5.1):
//!
//! - Non-Int operands (`5mm % 2mm`, `1.5 % 2`) must emit `E_MODULO_REQUIRES_INT`.
//! - `Int % Int` (`7 % 3`, `5 % 2`) must compile clean with `result_type = Type::Int`
//!   and `op = BinOp::Mod`.
//!
//! # RED / GREEN structure
//!
//! **Error-path tests (step-3 / step-4):**
//! - RED in step-3 for TWO reasons:
//!   1. `DiagnosticCode::ModuloRequiresInt` does not yet exist (compile error).
//!   2. No such diagnostic is emitted by the compiler yet.
//! - GREEN after step-4 adds the variant and the guard in expr.rs.
//!
//! **Happy-path tests:**
//! - GREEN on arrival: `infer_binop_type(Mod, Int, Int) = left.clone() = Int`.
//!   These serve as regression guards for the clean Int%Int path.
//!
//! Note: `mm` is a built-in unit (hardcoded in `units.rs::unit_to_scalar`), so
//! `compile_source` (no stdlib) correctly resolves `5mm` to `Scalar{LENGTH}`.

mod common;

use common::expect_binop;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_ir::BinOp;
use reify_test_support::{compile_source, errors_only};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Compile `structure def S { let p = <expr> }` and return the default_expr of
/// the `p` cell.  Panics if there are compilation errors or if the cell is absent.
fn compile_let_expr(expr: &str) -> reify_ir::CompiledExpr {
    let source = format!("structure def S {{ let p = {expr} }}");
    let module = compile_source(&source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "unexpected errors for `{expr}`: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .unwrap_or_else(|| panic!("template S not found for `{expr}`"));
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .unwrap_or_else(|| panic!("cell p not found for `{expr}`"));
    cell.default_expr
        .clone()
        .unwrap_or_else(|| panic!("cell p has no default_expr for `{expr}`"))
}

/// Compile `structure def S { let p = <expr> }` and return all Error-severity
/// diagnostics.  Used for error-path tests where we expect compilation to fail
/// with a specific diagnostic code.
fn compile_let_expr_errors(expr: &str) -> Vec<reify_core::Diagnostic> {
    let source = format!("structure def S {{ let p = {expr} }}");
    let module = compile_source(&source);
    module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ── Error-path tests ──────────────────────────────────────────────────────────
//
// RED (step-3): `DiagnosticCode::ModuloRequiresInt` does not exist yet and no
// such diagnostic is emitted.  GREEN after step-4.

/// `5mm % 2mm` (Scalar{LENGTH} % Scalar{LENGTH}) must produce
/// `DiagnosticCode::ModuloRequiresInt`.
///
/// RED (step-3): variant absent, no diagnostic emitted.
/// GREEN (step-4): variant added; guard in expr.rs emits it.
#[test]
fn mod_dimensioned_dimensioned_flagged() {
    let errors = compile_let_expr_errors("5mm % 2mm");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ModuloRequiresInt));
    assert!(
        flagged,
        "5mm % 2mm should produce DiagnosticCode::ModuloRequiresInt, got errors: {:?}",
        errors
    );
}

/// `1.5 % 2` (Real % Int) must produce `DiagnosticCode::ModuloRequiresInt`.
///
/// RED (step-3): variant absent, no diagnostic emitted.
/// GREEN (step-4): variant added; guard fires on Real left operand.
#[test]
fn mod_real_int_flagged() {
    let errors = compile_let_expr_errors("1.5 % 2");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ModuloRequiresInt));
    assert!(
        flagged,
        "1.5 % 2 should produce DiagnosticCode::ModuloRequiresInt, got errors: {:?}",
        errors
    );
}

// ── Happy-path tests ──────────────────────────────────────────────────────────
//
// These are GREEN on arrival (no production change needed) — regression guards
// for the valid Int%Int path, mirroring value_pow_compile_tests.rs's
// `pow_int_int_result_type_is_int`.

/// `7 % 3` must compile with op `BinOp::Mod` and `result_type = Type::Int`.
/// No `ModuloRequiresInt` diagnostic may be emitted.
///
/// GREEN on arrival: `infer_binop_type(Mod, Int, Int) = left.clone() = Int`.
#[test]
fn mod_int_int_result_type_is_int() {
    let expr = compile_let_expr("7 % 3");
    assert_eq!(
        expr.result_type,
        Type::Int,
        "7 % 3 result_type should be Int, got {:?}",
        expr.result_type
    );
    let (op, _, _) = expect_binop(&expr);
    assert_eq!(*op, BinOp::Mod, "op should be BinOp::Mod");
}

/// `5 % 2` (plain Int literals) must compile with no errors, specifically
/// no `ModuloRequiresInt`.
///
/// GREEN on arrival.
#[test]
fn mod_plain_int_no_error() {
    let errors = compile_let_expr_errors("5 % 2");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ModuloRequiresInt));
    assert!(
        !flagged,
        "5 % 2 should NOT produce DiagnosticCode::ModuloRequiresInt, got errors: {:?}",
        errors
    );
}

// ── Anti-cascade tests ────────────────────────────────────────────────────────
//
// When an operand is already Type::Error (e.g. an unresolved variable), the
// guard must NOT emit a spurious secondary ModuloRequiresInt — only the
// underlying error should surface.

/// `unknown_var % 2` — the left operand fails to resolve (Type::Error).
/// The guard must stay silent: zero `ModuloRequiresInt` diagnostics, and the
/// underlying unresolved-variable error is the only error emitted.
///
/// This exercises the anti-cascade check:
///   `if !compiled_left.result_type.is_error() && ...`
/// in `expr.rs` at the BinOp::Mod site.
#[test]
fn mod_error_typed_left_no_spurious_modulo_diagnostic() {
    let errors = compile_let_expr_errors("unknown_var % 2");
    // The unresolved-variable error must be present (guard wasn't skipped entirely).
    assert!(
        !errors.is_empty(),
        "expected at least one error for `unknown_var % 2` (unresolved variable), got none"
    );
    // No secondary ModuloRequiresInt must appear.
    let spurious = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ModuloRequiresInt));
    assert!(
        !spurious,
        "unknown_var % 2 must NOT produce a spurious ModuloRequiresInt — \
         left operand is already Type::Error (anti-cascade). got errors: {:?}",
        errors
    );
}
