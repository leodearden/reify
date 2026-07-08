//! Compile-time operand-kind guard tests for `*`/`/` (task compiler-type-hygiene
//! β2, step-5/step-6, `E_ArithOperandKind`).
//!
//! Mirrors `and_or_operand_guard_tests.rs`'s structure: a helper compiles a
//! `structure def` body and filters `Severity::Error` diagnostics.
//!
//! ## Coverage
//!
//! **Error-path (RED until step-6):**
//! - `a * a` (`Vector3<Length> × Vector3<Length>`, unsupported) → `ArithOperandKind`, message mentions `*`
//! - `a / a` (same operands, unsupported) → `ArithOperandKind`, message mentions `/`
//! - `2.0 / a` (`Scalar / Vector`, Div non-commutative) → `ArithOperandKind`
//!
//! **Anti-cascade (RED until step-6):**
//! - `let v = a * a` then `let w = v + 1` → EXACTLY ONE `ArithOperandKind`
//!   (`v` poisons to `Type::Error`; `v + 1` does not cascade a second diagnostic)
//!
//! **Gradualism regressions (must stay GREEN):**
//! - `unknown_var * a` (unresolved-variable / `Type::Error` operand) → no `ArithOperandKind`
//! - `fn scale<T>(x: T) -> T { x * 2 }` (`TypeParam` operand) → no `ArithOperandKind`
//!
//! **Clean-path regressions (must stay GREEN throughout — satisfied by steps 2/4):**
//! - `let s = a * 2.0` (legal scale) → no `ArithOperandKind`
//! - `let s = a * 2.0` + `constraint s > 0` → `DiagnosticCode::CmpOperandKind`
//!   (the pre-existing 4490 guard, now reachable because `s` types `Vector3<Length>`;
//!   the eigenvalues/trace fixit is Tensor/Matrix-only, so only the CODE is asserted)
//! - `let q = w / 2.0` with `param w : Vector3<Force>` → no errors (runtime-legal scale)

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{assert_no_error_diagnostics, compile_source};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Compile a `structure def P { param a : Vector3<Length> ... }` body containing
/// `extra` and return all `Severity::Error` diagnostics.
fn compile_vector_expr_errors(extra: &str) -> Vec<reify_core::Diagnostic> {
    let source = format!(
        r#"
structure def P {{
    param a : Vector3<Length>
    {extra}
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
fn compile_vector_expr_all(extra: &str) -> Vec<reify_core::Diagnostic> {
    let source = format!(
        r#"
structure def P {{
    param a : Vector3<Length>
    {extra}
}}
"#
    );
    compile_source(&source).diagnostics
}

// ── Error-path tests (RED until step-6) ──────────────────────────────────────

/// `a * a` (`Vector3<Length> × Vector3<Length>`) must produce `ArithOperandKind`
/// with a message mentioning `*` and the `Vector3<` operand-kind spelling.
#[test]
fn vector_times_vector_emits_arith_operand_kind() {
    let errors = compile_vector_expr_errors("let v = a * a");
    let flagged: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .collect();
    assert!(
        !flagged.is_empty(),
        "`a * a` must produce DiagnosticCode::ArithOperandKind; got errors: {errors:?}"
    );
    assert!(
        flagged.iter().any(|d| d.message.contains('*')),
        "`a * a` error message must mention `*`; got: {flagged:?}"
    );
    assert!(
        flagged.iter().any(|d| d.message.contains("Vector3<")),
        "`a * a` error message must name the Vector3<..> operand kind; got: {flagged:?}"
    );
}

/// `a / a` (same operands as above) must produce `ArithOperandKind` with a
/// message mentioning `/`.
#[test]
fn vector_div_vector_emits_arith_operand_kind() {
    let errors = compile_vector_expr_errors("let q = a / a");
    let flagged: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .collect();
    assert!(
        !flagged.is_empty(),
        "`a / a` must produce DiagnosticCode::ArithOperandKind; got errors: {errors:?}"
    );
    assert!(
        flagged.iter().any(|d| d.message.contains('/')),
        "`a / a` error message must mention `/`; got: {flagged:?}"
    );
}

/// `2.0 / a` (`Scalar / Vector`) has no reverse-scale arm (Div is
/// non-commutative) — must produce `ArithOperandKind`.
#[test]
fn scalar_div_vector_emits_arith_operand_kind() {
    let errors = compile_vector_expr_errors("let r = 2.0 / a");
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ArithOperandKind));
    assert!(
        flagged,
        "`2.0 / a` must produce DiagnosticCode::ArithOperandKind; got errors: {errors:?}"
    );
}

// ── Anti-cascade (RED until step-6) ──────────────────────────────────────────

/// `let v = a * a` poisons `v` to `Type::Error`; the follow-on `let w = v + 1`
/// must NOT produce a second `ArithOperandKind` (or any other cascade
/// diagnostic) — exactly one error total.
#[test]
fn poisoned_mul_result_does_not_cascade_into_consuming_binding() {
    let errors = compile_vector_expr_errors(
        r#"let v = a * a
    let w = v + 1"#,
    );
    let arith_count = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .count();
    assert_eq!(
        arith_count, 1,
        "expected exactly ONE ArithOperandKind (anti-cascade); got {arith_count}: {errors:?}"
    );
}

// ── Gradualism regressions (must stay GREEN) ─────────────────────────────────

/// An unresolved-variable operand (`Type::Error`) must NOT produce a spurious
/// `ArithOperandKind` — only the underlying unresolved-variable error.
#[test]
fn error_typed_left_operand_no_spurious_arith_operand_kind() {
    let errors = compile_vector_expr_errors("let v = unknown_var * a");
    assert!(
        !errors.is_empty(),
        "expected at least one error for `unknown_var * a` (unresolved variable), got none"
    );
    let spurious = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ArithOperandKind));
    assert!(
        !spurious,
        "`unknown_var * a` must NOT produce a spurious ArithOperandKind — \
         left operand is Type::Error (anti-cascade). got errors: {errors:?}"
    );
}

/// A `TypeParam`-typed operand (unresolved generic) must NOT produce a
/// spurious `ArithOperandKind`. Mirrors
/// `comparison_operand_guard_tests.rs::type_param_in_comparison_no_cmp_operand_kind`.
#[test]
fn type_param_operand_no_spurious_arith_operand_kind() {
    let src = r#"
fn scale<T>(x: T) -> T { x * 2 }
"#;
    let module = compile_source(src);
    let errors: Vec<_> = module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let spurious = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::ArithOperandKind));
    assert!(
        !spurious,
        "TypeParam operand in `*` must NOT produce ArithOperandKind (gradualism). got: {errors:?}"
    );
}

// ── Clean-path regressions (must stay GREEN throughout) ──────────────────────

/// `a * 2.0` (`Vector3<Length> * dimensionless`) is a legal scale — must
/// produce zero error diagnostics.
#[test]
fn vector_times_dimensionless_compiles_clean() {
    let all = compile_vector_expr_all("let s = a * 2.0");
    assert_no_error_diagnostics(&all, "`a * 2.0` should compile cleanly (legal scale)");
}

/// `let s = a * 2.0` types `s` as `Vector3<Length>`; `constraint s > 0` then
/// reaches the pre-existing (task 4490) comparison-operand guard, which
/// rejects `Vector3<Length>` as non-orderable → `DiagnosticCode::CmpOperandKind`.
///
/// Only the CODE is asserted: the eigenvalues/trace fixit is Tensor/Matrix-only
/// (`emit_comparison_operand_diagnostics`), so a Vector operand does not carry it.
#[test]
fn vector_scale_result_compared_emits_cmp_operand_kind() {
    let errors = compile_vector_expr_errors(
        r#"let s = a * 2.0
    constraint s > 0"#,
    );
    let flagged = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind));
    assert!(
        flagged,
        "`constraint s > 0` on a Vector3<Length>-typed `s` must produce \
         DiagnosticCode::CmpOperandKind; got errors: {errors:?}"
    );
}

/// `w / 2.0` (`Vector3<Force> / dimensionless`) is a legal scale (β1 row-6
/// pin) — must produce zero error diagnostics.
#[test]
fn vector_force_div_dimensionless_compiles_clean() {
    let source = r#"
structure def Q {
    param w : Vector3<Force>
    let q = w / 2.0
}
"#;
    let module = compile_source(source);
    assert_no_error_diagnostics(&module.diagnostics, "`w / 2.0` should compile cleanly");
}
