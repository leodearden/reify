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
//! - `z + z` (SAME dimensioned Complex on both sides) → zero
//!   `ArithOperandKind` (legitimate arithmetic — contrast the
//!   mismatched-dimension row-C cases below)
//! - `let w = z + 1` then `let x = w + 1` (anti-cascade) → exactly ONE
//!   `ArithOperandKind`
//! - `unknown_var + z` (unresolved-name `Type::Error` operand, gradualism) →
//!   zero `ArithOperandKind`
//!
//! **Row B / Row C (task 5219 — dimensioned Complex vs dimensioned Scalar,
//! and mismatched-dimension Complex vs Complex; guarded here too):**
//! - `z + len` / `z - len` / `len + z` (`Complex<Length>` vs `Length` —
//!   dimensioned Complex vs ANY dimensioned, non-Int/Real `Scalar`) →
//!   `ArithOperandKind` in both operator directions AND both operand orders
//!   (row B; see `add_sub_dimensioned_complex_reject`'s doc in
//!   `type_compat.rs`)
//! - `z + zk` / `zk - z` (`Complex<Length>` vs `Complex<Mass>` — mismatched
//!   dimensions, via the `compile_two_complex_module` helper) →
//!   `ArithOperandKind` in both operator directions (row C). SAME-dimension
//!   Complex-vs-Complex (`z + z` above) remains legitimate arithmetic and
//!   stays zero-diagnostic.

use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};
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

/// Assert that `module` carries exactly ONE `Severity::Error`
/// `ArithOperandKind` diagnostic naming operator `op` (e.g. `"+"`), and that
/// the `let {binding} = ...` cell in structure `P` is poisoned to
/// `Type::Error`. Returns the flagged diagnostic so callers can layer
/// additional assertions (e.g. pinning the reported operand-kind substring,
/// per row-A's `z + 1` test) on top without re-deriving it.
///
/// Extracted shared boilerplate for the row-A/B/C dimensioned-Complex
/// error-path tests below (task 5219): each independently repeated the same
/// filter-to-`Severity::Error`/filter-to-`ArithOperandKind`/assert-count-1/
/// assert-operator-substring/assert-poison sequence, which invited drift as
/// more rows accreted.
fn assert_single_arith_operand_kind<'a>(
    module: &'a reify_compiler::CompiledModule,
    op: &str,
    binding: &str,
) -> &'a reify_core::Diagnostic {
    let flagged: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| d.code == Some(DiagnosticCode::ArithOperandKind))
        .collect();
    assert_eq!(
        flagged.len(),
        1,
        "expected exactly ONE ArithOperandKind for operator `{op}` on \
         `{binding}`; got diagnostics: {:?}",
        module.diagnostics
    );
    let operator_substring = format!("operator `{op}`");
    assert!(
        flagged[0].message.contains(&operator_substring),
        "error message must name the `{op}` operator; got: {:?}",
        flagged[0].message
    );

    let expr = get_let_expr_in(module, "P", binding);
    assert_eq!(
        expr.result_type,
        Type::Error,
        "`{binding}` must be poisoned to Type::Error, got: {:?}",
        expr.result_type
    );

    flagged[0]
}

// ── Error-path tests (RED until step-4) ──────────────────────────────────────

/// `z + 1` (`Complex<Length> + Int`) must produce exactly ONE
/// `ArithOperandKind` naming the `+` operator and the `Complex<` operand
/// kind, and must poison `w`'s static result type to `Type::Error`.
#[test]
fn dimensioned_complex_plus_int_emits_arith_operand_kind_and_poisons_result() {
    let module = compile_complex_expr("let w = z + 1");
    let flagged = assert_single_arith_operand_kind(&module, "+", "w");
    // Derive the expected operand-kind substring from `Type`'s own Display
    // impl (rather than a hardcoded `"Complex<"` literal) so this assertion
    // tracks the compiler's actual formatting instead of restating it.
    let expected_operand_kind = Type::complex(Type::length()).to_string();
    assert!(
        flagged.message.contains(&expected_operand_kind),
        "`z + 1` error message must name the {expected_operand_kind} operand \
         kind; got: {:?}",
        flagged.message
    );
    // The label text is the canonical `poison_arith_operand_kind` form
    // shared with the Mul/Div guard — pin it directly so a regression that
    // drops or alters the label goes caught here rather than only via the
    // message/code assertions above.
    assert!(
        flagged
            .labels
            .iter()
            .any(|label| label.message == "unsupported operand kinds"),
        "`z + 1` diagnostic must carry a label \"unsupported operand kinds\"; \
         got labels: {:?}",
        flagged.labels
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
    assert_single_arith_operand_kind(&module, "-", "w");
}

/// Order-reversed counterpart of the sibling pin above: `1 - z` must ALSO
/// produce `ArithOperandKind` — mirrors
/// `int_plus_dimensioned_complex_order_reversed_emits_arith_operand_kind`
/// for the `Sub` direction, closing the same order-dependent asymmetry —
/// AND must poison `w`'s result_type to `Type::Error`, verifying the
/// `make_poison_type` override fires for this operand order too, mirroring
/// the poison assertions on the other three error-path tests above.
#[test]
fn int_minus_dimensioned_complex_order_reversed_emits_arith_operand_kind() {
    let module = compile_complex_expr("let w = 1 - z");
    assert_eq!(
        arith_operand_kind_count(&module.diagnostics),
        1,
        "`1 - z` (order-reversed) must produce exactly ONE ArithOperandKind; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let w = get_let_expr_in(&module, "P", "w");
    assert_eq!(
        w.result_type,
        Type::Error,
        "`1 - z` (order-reversed) must poison `w`'s result_type to \
         Type::Error, got: {:?}",
        w.result_type
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
/// `Complex<Length>`) is legitimate Complex arithmetic — the runtime
/// `(Complex, Complex)` arm only returns `Value::Undef` when the two
/// dimensions differ (task 5219 row C, guarded via the
/// `mismatched_dimension_complex_*` tests below). Guards against a future
/// regression that broadens `add_sub_dimensioned_complex_reject`'s row-C
/// clause to also match same-dimension Complex-vs-Complex pairs.
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

// ── Row B: dimensioned Complex vs dimensioned Scalar (task 5219) ────────────

/// `z + len` (`Complex<Length> + Length`: dimensioned Complex vs a
/// dimensioned, non-Int/Real `Scalar`) must produce exactly ONE
/// `ArithOperandKind` and poison `w`'s result_type to `Type::Error` — task
/// 5219 row B: the runtime `eval_add`/`eval_sub` (reify-expr) have NO
/// `(Complex, Scalar)` arm at all, so a dimensioned `Complex` paired with ANY
/// dimensioned `Scalar` is a structural `Value::Undef` regardless of whether
/// the two dimensions match (see `add_sub_dimensioned_complex_reject`'s doc
/// in `type_compat.rs`).
#[test]
fn dimensioned_complex_plus_dimensioned_scalar_emits_arith_operand_kind() {
    let module = compile_complex_expr("let w = z + len");
    let flagged = assert_single_arith_operand_kind(&module, "+", "w");
    let expected_operand_kind = Type::complex(Type::length()).to_string();
    assert!(
        flagged.message.contains(&expected_operand_kind),
        "`z + len` error message must name the {expected_operand_kind} \
         operand kind; got: {:?}",
        flagged.message
    );
}

/// Sub-direction counterpart: `z - len` must also produce `ArithOperandKind`
/// and poison `w`'s result_type to `Type::Error`. Also pins the reported
/// operand-kind substring (like the primary `z + 1` and `z + len` tests) so
/// a regression that named the wrong operand kind (e.g. the bare `Scalar`
/// instead of the `Complex`) would be caught here too, not just on the
/// `Add` direction.
#[test]
fn dimensioned_complex_minus_dimensioned_scalar_emits_arith_operand_kind() {
    let module = compile_complex_expr("let w = z - len");
    let flagged = assert_single_arith_operand_kind(&module, "-", "w");
    let expected_operand_kind = Type::complex(Type::length()).to_string();
    assert!(
        flagged.message.contains(&expected_operand_kind),
        "`z - len` error message must name the {expected_operand_kind} \
         operand kind; got: {:?}",
        flagged.message
    );
}

/// Order-reversed counterpart: `len + z` (`Length + Complex<Length>`) must
/// ALSO produce `ArithOperandKind` and poison `w`'s result_type to
/// `Type::Error`. Mirrors row A's
/// `int_plus_dimensioned_complex_order_reversed_emits_arith_operand_kind`,
/// which exists because that pairing historically had an order-dependent
/// asymmetry bug (`infer_binop_type`'s `left.clone()` collapse silently
/// accepting one order but not the other) — this pins the same class of
/// regression for row B's `is_dimensioned_scalar(left) &&
/// is_dimensioned_complex(right)` clause, which the plain `z + len` /
/// `z - len` cases above (Complex always on the left) do not exercise.
#[test]
fn dimensioned_scalar_plus_dimensioned_complex_emits_arith_operand_kind() {
    let module = compile_complex_expr("let w = len + z");
    assert_single_arith_operand_kind(&module, "+", "w");
}

// ── Row C: mismatched-dimension Complex ± Complex (task 5219) ───────────────

/// Compile a `structure def P { param len : Length; param mass : Mass =
/// 1kg; let z = complex(len, len); let zk = complex(mass, mass) ... }` body
/// containing `extra` and return the compiled module. `z` types as
/// `Complex<Length>`, `zk` types as `Complex<Mass>` — two DIMENSIONED
/// `Complex` values with different concrete dimensions (task 5219 row C).
fn compile_two_complex_module(extra: &str) -> reify_compiler::CompiledModule {
    let source = format!(
        r#"
structure def P {{
    param len : Length
    param mass : Mass = 1kg
    let z = complex(len, len)
    let zk = complex(mass, mass)
    {extra}
}}
"#
    );
    compile_source(&source)
}

/// `z + zk` (`Complex<Length> + Complex<Mass>`, mismatched dimensions) must
/// produce exactly ONE `ArithOperandKind` and poison `w`'s result_type to
/// `Type::Error` — task 5219 row C: the runtime `eval_add`/`eval_sub`
/// (reify-expr) `(Complex, Complex)` arm returns `Value::Undef` when the two
/// dimensions differ. Also pins both reported operand-kind substrings (like
/// the primary `z + 1` test) so a regression that named the wrong operand
/// kind — e.g. the bare `Scalar` dimensions (`Length`/`Mass`) instead of the
/// wrapping `Complex<..>` types — would be caught here rather than only via
/// the count/operator assertions.
#[test]
fn mismatched_dimension_complex_plus_complex_emits_arith_operand_kind() {
    let module = compile_two_complex_module("let w = z + zk");
    let flagged = assert_single_arith_operand_kind(&module, "+", "w");
    let expected_left_operand_kind = Type::complex(Type::length()).to_string();
    let expected_right_operand_kind = Type::complex(Type::Scalar {
        dimension: DimensionVector::MASS,
    })
    .to_string();
    assert!(
        flagged.message.contains(&expected_left_operand_kind),
        "`z + zk` error message must name the {expected_left_operand_kind} \
         operand kind; got: {:?}",
        flagged.message
    );
    assert!(
        flagged.message.contains(&expected_right_operand_kind),
        "`z + zk` error message must name the {expected_right_operand_kind} \
         operand kind; got: {:?}",
        flagged.message
    );
}

/// Sub-direction counterpart: `zk - z` must also produce exactly ONE
/// `ArithOperandKind` and poison `w`'s result_type to `Type::Error`.
#[test]
fn mismatched_dimension_complex_minus_complex_emits_arith_operand_kind() {
    let module = compile_two_complex_module("let w = zk - z");
    assert_single_arith_operand_kind(&module, "-", "w");
}
