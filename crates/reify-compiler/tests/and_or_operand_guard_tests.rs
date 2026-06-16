//! Compile-time operand-kind guard tests for `and`, `or`, and `implies` (task 4490, step-1/step-2).
//!
//! ## Coverage
//!
//! **Error-path (RED until step-2):**
//! - `5 and flag` (non-Bool left) → `Severity::Error`, code `LogicalOperandNotBool`, message "and"
//! - `flag and 3` (non-Bool right) → same
//! - `flag or 3` (non-Bool right) → `Severity::Error`, code `LogicalOperandNotBool`, message "or"
//! - `5 or flag` (non-Bool left) → same
//! - `5 implies 3` → `Severity::Error` WITH code `LogicalOperandNotBool` (previously uncoded)
//!
//! **Regression assertions (must stay GREEN before AND after step-2):**
//! - `true and false` → no error diagnostics
//! - `false or true` → no error diagnostics
//! - `p and true` (Bool param) → no error diagnostics
//! - `true implies false` → no error diagnostics
//!
//! **Gradualism regression (must stay GREEN):**
//! - `unknown_var and flag` → no secondary `LogicalOperandNotBool` (anti-cascade on `Type::Error`)
//!
//! ## Implementation note
//! The guard lives in `compile_binop` (`reify-compiler/src/expr.rs`).
//! `infer_binop_type(BinOp::And|Or|Implies, _, _)` returns `Type::Bool` unconditionally,
//! so without the guard `5 and 3` silently type-checks as Bool — these tests pin the guard.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{assert_no_error_diagnostics, compile_source};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Compile a `structure def` that contains `let result : Bool = <expr>` and a
/// `Bool` param `flag`. Returns all `Severity::Error` diagnostics.
///
/// The `flag` param gives us a Bool-typed value reference for mixed
/// (non-Bool op Bool) tests without requiring `true`/`false` literals.
fn compile_logical_expr_errors(expr: &str) -> Vec<reify_core::Diagnostic> {
    let source = format!(
        r#"
structure def T {{
    param flag : Bool
    let result : Bool = {expr}
}}
"#
    );
    let module = compile_source(&source);
    module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Compile the same harness and return all diagnostics (any severity).
fn compile_logical_expr(expr: &str) -> Vec<reify_core::Diagnostic> {
    let source = format!(
        r#"
structure def T {{
    param flag : Bool
    let result : Bool = {expr}
}}
"#
    );
    let module = compile_source(&source);
    module.diagnostics
}

// ── Error-path tests (RED until step-2) ──────────────────────────────────────

/// `5 and flag` (non-Bool left operand) must produce `LogicalOperandNotBool`
/// with a message containing "and".
///
/// RED (step-1): guard not yet written.
/// GREEN (step-2): guard extended to And/Or.
#[test]
fn and_non_bool_left_emits_logical_operand_not_bool() {
    let errors = compile_logical_expr_errors("5 and flag");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        flagged,
        "`5 and flag` must produce DiagnosticCode::LogicalOperandNotBool; got errors: {:?}",
        errors
    );
    // message must mention the operator so callers can distinguish and/or/implies
    let has_and_msg = errors
        .iter()
        .any(|d| d.message.contains("and"));
    assert!(
        has_and_msg,
        "`5 and flag` error message must mention \"and\"; got errors: {:?}",
        errors
    );
}

/// `flag and 3` (non-Bool right operand) must produce `LogicalOperandNotBool`
/// with a message containing "and".
///
/// RED (step-1): guard not yet written.
/// GREEN (step-2): guard extended to And/Or.
#[test]
fn and_non_bool_right_emits_logical_operand_not_bool() {
    let errors = compile_logical_expr_errors("flag and 3");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        flagged,
        "`flag and 3` must produce DiagnosticCode::LogicalOperandNotBool; got errors: {:?}",
        errors
    );
    let has_and_msg = errors
        .iter()
        .any(|d| d.message.contains("and"));
    assert!(
        has_and_msg,
        "`flag and 3` error message must mention \"and\"; got errors: {:?}",
        errors
    );
}

/// `5 or flag` (non-Bool left operand) must produce `LogicalOperandNotBool`
/// with a message containing "or".
///
/// RED (step-1): guard not yet written.
/// GREEN (step-2): guard extended to And/Or.
#[test]
fn or_non_bool_left_emits_logical_operand_not_bool() {
    let errors = compile_logical_expr_errors("5 or flag");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        flagged,
        "`5 or flag` must produce DiagnosticCode::LogicalOperandNotBool; got errors: {:?}",
        errors
    );
    let has_or_msg = errors
        .iter()
        .any(|d| d.message.contains("or"));
    assert!(
        has_or_msg,
        "`5 or flag` error message must mention \"or\"; got errors: {:?}",
        errors
    );
}

/// `flag or 3` (non-Bool right operand) must produce `LogicalOperandNotBool`
/// with a message containing "or".
///
/// RED (step-1): guard not yet written.
/// GREEN (step-2): guard extended to And/Or.
#[test]
fn or_non_bool_right_emits_logical_operand_not_bool() {
    let errors = compile_logical_expr_errors("flag or 3");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        flagged,
        "`flag or 3` must produce DiagnosticCode::LogicalOperandNotBool; got errors: {:?}",
        errors
    );
    let has_or_msg = errors
        .iter()
        .any(|d| d.message.contains("or"));
    assert!(
        has_or_msg,
        "`flag or 3` error message must mention \"or\"; got errors: {:?}",
        errors
    );
}

/// `5 implies 3` must produce `DiagnosticCode::LogicalOperandNotBool`
/// (the existing Implies guard is currently uncoded — step-2 attaches the code).
///
/// RED (step-1): implies guard exists but emits without a code.
/// GREEN (step-2): implies guard generalised to And|Or|Implies and receives
///                 `with_code(LogicalOperandNotBool)`.
#[test]
fn implies_non_bool_emits_logical_operand_not_bool_code() {
    let errors = compile_logical_expr_errors("5 implies 3");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        flagged,
        "`5 implies 3` must produce DiagnosticCode::LogicalOperandNotBool; got errors: {:?}",
        errors
    );
}

// ── Regression assertions (must stay GREEN before AND after step-2) ───────────

/// `true and false` is valid: both operands are `Bool`-typed.
///
/// Must produce no error diagnostics both before (no guard) and after (guard
/// accepts Bool) step-2.
#[test]
fn and_bool_bool_compiles_clean() {
    let all = compile_logical_expr("true and false");
    assert_no_error_diagnostics(&all, "`true and false` should compile cleanly");
}

/// `false or true` is valid: both operands are `Bool`-typed.
#[test]
fn or_bool_bool_compiles_clean() {
    let all = compile_logical_expr("false or true");
    assert_no_error_diagnostics(&all, "`false or true` should compile cleanly");
}

/// `p and true` — `p` is a `Bool`-typed param, typed `Bool` at compile time
/// (though it evaluates to `Undef` at runtime when no default is given).
///
/// The guard must NOT fire on a Bool-typed value reference, only on operands
/// whose compile-time type is non-Bool.
#[test]
fn and_bool_param_true_compiles_clean() {
    let all = compile_logical_expr("flag and true");
    assert_no_error_diagnostics(
        &all,
        "`flag and true` should compile cleanly (flag is Bool-typed)",
    );
}

/// `true implies false` is valid and must stay clean after step-2 generalises
/// the implies guard.
#[test]
fn implies_bool_bool_compiles_clean() {
    let all = compile_logical_expr("true implies false");
    assert_no_error_diagnostics(&all, "`true implies false` should compile cleanly");
}

/// `flag implies true` — Bool param implies Bool literal — must compile clean.
#[test]
fn implies_bool_param_compiles_clean() {
    let all = compile_logical_expr("flag implies true");
    assert_no_error_diagnostics(
        &all,
        "`flag implies true` should compile cleanly (flag is Bool-typed)",
    );
}

// ── Gradualism / anti-cascade tests ──────────────────────────────────────────

/// `unknown_var and flag` — the left operand fails to resolve (`Type::Error`).
///
/// The guard must stay silent: zero `LogicalOperandNotBool` diagnostics.
/// Only the underlying unresolved-variable error should surface.
///
/// This exercises the gradualism early-return:
///   "if either operand `is_error()` or matches `Type::TypeParam(_)`, return"
/// in `expr.rs` at the And/Or/Implies site.
#[test]
fn and_error_typed_left_no_spurious_logical_diagnostic() {
    let errors = compile_logical_expr_errors("unknown_var and flag");
    // There must be at least one error (the unresolved-variable one).
    assert!(
        !errors.is_empty(),
        "expected at least one error for `unknown_var and flag` (unresolved variable), got none"
    );
    // No secondary LogicalOperandNotBool may appear.
    let spurious = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::LogicalOperandNotBool));
    assert!(
        !spurious,
        "`unknown_var and flag` must NOT produce a spurious LogicalOperandNotBool — \
         left operand is Type::Error (anti-cascade). got errors: {:?}",
        errors
    );
}
