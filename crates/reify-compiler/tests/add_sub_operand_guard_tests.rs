//! Compile-time operand-kind guard tests for `+`/`-` on a DIMENSIONED
//! `Complex` paired with a bare dimensionless `Int`/`Real` (task
//! compiler-type-hygiene follow-up 5163, reusing `E_ArithOperandKind`).
//!
//! Mirrors `mul_div_operand_guard_tests.rs`'s structure: a helper compiles a
//! `structure def` body via `reify_test_support::compile_source` and filters
//! `Severity::Error` diagnostics by `DiagnosticCode::ArithOperandKind`.
//!
//! The fixture constructs a DIMENSIONED `Complex` in source: `param len :
//! Length` then `let z = complex(len, len)` types statically as
//! `Complex<Length>` (`math_signatures.rs`'s `complex` arm clones the first
//! arg's type verbatim).
//!
//! ## Coverage
//!
//! **Error-path (RED until step-4):**
//! - `z + 1` (`Complex<Length> + Int`) → `ArithOperandKind`, message mentions
//!   `+` and the `Complex<` operand kind; `w`'s result_type poisons to
//!   `Type::Error`
//! - `1 + z` (order-reversed) → `ArithOperandKind` (closes the documented
//!   order-dependent asymmetry — previously this direction silently
//!   collapsed to bare `Int` with no diagnostic at all)
//! - `z - 1` (Sub direction) → `ArithOperandKind`
//! - `1 - z` (Sub, order-reversed) → `ArithOperandKind`
//! - `z + 1.5` (bare `Real` operand) → `ArithOperandKind`
//!
//! **No-false-positive / regression (must stay GREEN throughout):**
//! - `complex(1.0, 1.0) + 1` (dimensionless Complex widening, D3 policy) →
//!   zero `ArithOperandKind`
//! - `complex(1.0, 1.0) - 1` (dimensionless Complex widening, Sub direction)
//!   → zero `ArithOperandKind`
//! - `z + z` (same dimensioned Complex on both sides) → zero
//!   `ArithOperandKind` (Complex-vs-Complex is out of this guard's scope)
//! - `let w = z + 1` then `let x = w + 1` (anti-cascade) → exactly ONE
//!   `ArithOperandKind`
//! - `unknown_var + z` (unresolved-name `Type::Error` operand, gradualism) →
//!   zero `ArithOperandKind`
//!
//! **Documented unguarded gap (out of this task's scope, NOT fixed here):**
//! - `z + len` (`Complex<Length> + Length` — dimensioned Complex vs a
//!   dimensioned, non-Int/Real `Scalar`) → zero `ArithOperandKind`. This pins
//!   CURRENT behavior, not a correctness claim: task 5163 only guards the
//!   dimensioned-Complex-vs-bare-dimensionless-numeric row (see
//!   `add_sub_dimensioned_complex_reject`'s doc in `type_compat.rs`); this
//!   pairing, and mismatched-dimension `Complex<Q1> ± Complex<Q2>` (not
//!   pinned by any test here — see `z + z` above for the same-dimension,
//!   forever-legitimate case), are tracked by follow-up TODO(#5219).

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::{compile_source, get_let_expr_in};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Compile a `structure def P { param len : Length; let z = complex(len, len)
/// ... }` body containing `extra` and return the compiled module.
fn compile_complex_expr(extra: &str) -> reify_compiler::CompiledModule {
    let source = format!(
        r#"
structure def P {{
    param len : Length
    let z = complex(len, len)
    {extra}
}}
"#
    );
    compile_source(&source)
}

/// Same harness, returning only `Severity::Error` diagnostics.
fn compile_complex_expr_errors(extra: &str) -> Vec<reify_core::Diagnostic> {
    compile_complex_expr(extra)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn arith_operand_kind_count(diags: &[reify_core::Diagnostic]) -> usize {
    diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .count()
}

// ── Error-path tests (RED until step-4) ──────────────────────────────────────

/// `z + 1` (`Complex<Length> + Int`) must produce exactly ONE
/// `ArithOperandKind` naming the `+` operator and the `Complex<` operand
/// kind, and must poison `w`'s static result type to `Type::Error`.
#[test]
fn dimensioned_complex_plus_int_emits_arith_operand_kind_and_poisons_result() {
    let module = compile_complex_expr("let w = z + 1");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let flagged: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .collect();
    assert_eq!(
        flagged.len(),
        1,
        "`z + 1` (Complex<Length> + Int) must produce exactly ONE \
         ArithOperandKind; got errors: {errors:?}"
    );
    assert!(
        flagged[0].message.contains("operator `+`"),
        "`z + 1` error message must name the `+` operator; got: {:?}",
        flagged[0].message
    );
    // Derive the expected operand-kind substring from `Type`'s own Display
    // impl (rather than a hardcoded `"Complex<"` literal) so this assertion
    // tracks the compiler's actual formatting instead of restating it.
    let expected_operand_kind = Type::complex(Type::length()).to_string();
    assert!(
        flagged[0].message.contains(&expected_operand_kind),
        "`z + 1` error message must name the {expected_operand_kind} operand \
         kind; got: {:?}",
        flagged[0].message
    );

    let w = get_let_expr_in(&module, "P", "w");
    assert_eq!(
        w.result_type,
        Type::Error,
        "`z + 1` must poison `w`'s result_type to Type::Error, got: {:?}",
        w.result_type
    );
}

/// Order-reversed counterpart: `1 + z` must ALSO produce `ArithOperandKind`
/// — closes the documented order-dependent asymmetry (previously `1 + z`
/// silently collapsed to bare `Int` via `left.clone()`, with no diagnostic)
/// — AND must poison `w`'s result_type to `Type::Error`, verifying the
/// `make_poison_type` override (not just a diagnostic push) fires for the
/// reversed operand order too, mirroring the primary `z + 1` case above.
#[test]
fn int_plus_dimensioned_complex_order_reversed_emits_arith_operand_kind() {
    let module = compile_complex_expr("let w = 1 + z");
    assert_eq!(
        arith_operand_kind_count(&module.diagnostics),
        1,
        "`1 + z` (order-reversed) must produce exactly ONE ArithOperandKind; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let w = get_let_expr_in(&module, "P", "w");
    assert_eq!(
        w.result_type,
        Type::Error,
        "`1 + z` (order-reversed) must poison `w`'s result_type to \
         Type::Error, got: {:?}",
        w.result_type
    );
}

/// Sub direction: `z - 1` must also produce `ArithOperandKind`, with a
/// message mentioning `-`, AND must poison `w`'s result_type to
/// `Type::Error` — mirrors the `make_poison_type` override assertion on the
/// primary `z + 1` case above, so a regression that pushed the diagnostic
/// but skipped the poison for `Sub` specifically would be caught directly
/// rather than only indirectly via the Add-only anti-cascade test.
#[test]
fn dimensioned_complex_minus_int_emits_arith_operand_kind_and_poisons_result() {
    let module = compile_complex_expr("let w = z - 1");
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let flagged: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .collect();
    assert_eq!(
        flagged.len(),
        1,
        "`z - 1` (Complex<Length> - Int) must produce exactly ONE \
         ArithOperandKind; got errors: {errors:?}"
    );
    assert!(
        flagged[0].message.contains("operator `-`"),
        "`z - 1` error message must name the `-` operator; got: {:?}",
        flagged[0].message
    );

    let w = get_let_expr_in(&module, "P", "w");
    assert_eq!(
        w.result_type,
        Type::Error,
        "`z - 1` must poison `w`'s result_type to Type::Error, got: {:?}",
        w.result_type
    );
}

/// Order-reversed counterpart of the sibling pin above: `1 - z` must ALSO
/// produce `ArithOperandKind` — mirrors
/// `int_plus_dimensioned_complex_order_reversed_emits_arith_operand_kind`
/// for the `Sub` direction, closing the same order-dependent asymmetry.
#[test]
fn int_minus_dimensioned_complex_order_reversed_emits_arith_operand_kind() {
    let errors = compile_complex_expr_errors("let w = 1 - z");
    assert_eq!(
        arith_operand_kind_count(&errors),
        1,
        "`1 - z` (order-reversed) must produce exactly ONE ArithOperandKind; \
         got errors: {errors:?}"
    );
}

/// Bare `Real` operand (not `Int`): `z + 1.5` must also produce
/// `ArithOperandKind` — `Real` is `Scalar{DIMENSIONLESS}`, covered by
/// `is_dimensionless_numeric`.
#[test]
fn dimensioned_complex_plus_real_emits_arith_operand_kind() {
    let errors = compile_complex_expr_errors("let w = z + 1.5");
    assert_eq!(
        arith_operand_kind_count(&errors),
        1,
        "`z + 1.5` (Complex<Length> + Real) must produce exactly ONE \
         ArithOperandKind; got errors: {errors:?}"
    );
}

// ── No-false-positive / regression tests (must stay GREEN throughout) ───────

/// A DIMENSIONLESS `Complex` (`complex(1.0, 1.0)`) widening against a bare
/// `Int` is the pre-existing D3 arm — must NOT produce `ArithOperandKind`.
#[test]
fn dimensionless_complex_plus_int_no_spurious_arith_operand_kind() {
    let errors = compile_complex_expr_errors(
        r#"let d = complex(1.0, 1.0)
    let w = d + 1"#,
    );
    assert_eq!(
        arith_operand_kind_count(&errors),
        0,
        "`complex(1.0, 1.0) + 1` (dimensionless Complex widening) must NOT \
         produce ArithOperandKind; got errors: {errors:?}"
    );
}

/// Sub-direction counterpart of the sibling pin above: the dimensionless-
/// widening path is per-op, so a regression that mis-handles `Sub` there
/// (e.g. falsely rejecting) would not be caught by the `Add`-only case above.
#[test]
fn dimensionless_complex_minus_int_no_spurious_arith_operand_kind() {
    let errors = compile_complex_expr_errors(
        r#"let d = complex(1.0, 1.0)
    let w = d - 1"#,
    );
    assert_eq!(
        arith_operand_kind_count(&errors),
        0,
        "`complex(1.0, 1.0) - 1` (dimensionless Complex widening, Sub) must \
         NOT produce ArithOperandKind; got errors: {errors:?}"
    );
}

/// Two operands of the SAME dimensioned `Complex` type (`z + z`, both
/// `Complex<Length>`) is legitimate Complex arithmetic, out of this guard's
/// scope (`Complex<Q1> ± Complex<Q2>` is a separate, unguarded gap — see the
/// task analysis). Guards against a future regression that broadens
/// `add_sub_dimensioned_complex_reject` to also match dimensioned
/// Complex-vs-Complex pairs.
#[test]
fn dimensioned_complex_plus_same_dimensioned_complex_no_spurious_arith_operand_kind() {
    let errors = compile_complex_expr_errors("let w = z + z");
    assert_eq!(
        arith_operand_kind_count(&errors),
        0,
        "`z + z` (Complex<Length> + Complex<Length>) must NOT produce \
         ArithOperandKind; got errors: {errors:?}"
    );
}

/// Anti-cascade: `z + 1` poisons `w` to `Type::Error`; the follow-on
/// `x = w + 1` must NOT produce a second `ArithOperandKind`.
#[test]
fn poisoned_add_result_does_not_cascade_into_consuming_binding() {
    let errors = compile_complex_expr_errors(
        r#"let w = z + 1
    let x = w + 1"#,
    );
    assert_eq!(
        arith_operand_kind_count(&errors),
        1,
        "expected exactly ONE ArithOperandKind (anti-cascade); got: {errors:?}"
    );
}

/// An unresolved-variable operand (`Type::Error`) must NOT produce a
/// spurious `ArithOperandKind` — gradualism (mirrors the Mul/Div guard's
/// `error_typed_left_operand_no_spurious_arith_operand_kind`).
#[test]
fn unresolved_name_operand_no_spurious_arith_operand_kind() {
    let errors = compile_complex_expr_errors("let w = unknown_var + z");
    assert!(
        !errors.is_empty(),
        "expected at least one error for `unknown_var + z` (unresolved \
         variable), got none"
    );
    assert_eq!(
        arith_operand_kind_count(&errors),
        0,
        "`unknown_var + z` must NOT produce a spurious ArithOperandKind — \
         left operand is Type::Error (anti-cascade). got errors: {errors:?}"
    );
}

// ── Documented unguarded gap (out of this task's scope) ─────────────────────

/// `z + len` (`Complex<Length> + Length`: dimensioned Complex vs a
/// dimensioned, non-Int/Real `Scalar`) matches neither
/// `add_sub_dimensioned_complex_reject` (the right operand isn't a bare
/// dimensionless numeric) nor the pre-existing Scalar/Scalar dimension-compat
/// block in `expr.rs` (the left operand is `Complex`, not `Scalar`). This
/// pins CURRENT behavior — zero `ArithOperandKind` — as a documented,
/// out-of-scope gap alongside `z + z` above, NOT a correctness claim: task
/// 5163 only guards the dimensioned-Complex-vs-bare-dimensionless-numeric
/// row (see `add_sub_dimensioned_complex_reject`'s doc in `type_compat.rs`).
/// TODO(#5219): guard this row (or record a permanent-accept decision),
/// alongside the sibling mismatched-dimension `Complex<Q1> ± Complex<Q2>`
/// gap, which is not pinned by any test in this file.
#[test]
fn dimensioned_complex_plus_dimensioned_scalar_is_documented_unguarded_gap() {
    let errors = compile_complex_expr_errors("let w = z + len");
    assert_eq!(
        arith_operand_kind_count(&errors),
        0,
        "`z + len` (Complex<Length> + Length) is a documented out-of-scope \
         gap (dimensioned Complex vs dimensioned Scalar) — task 5163 only \
         guards the bare-dimensionless-numeric row; got errors: {errors:?}"
    );
}
