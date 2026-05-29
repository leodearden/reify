//! Type-checking tests for the `implies` keyword operator (task 3921, step-7/step-8).
//!
//! ## Coverage
//! - `5 implies 3` (non-Bool operands) → `Severity::Error` diagnostic mentioning `implies`.
//! - `true implies false` → compiles without any `Severity::Error` diagnostics.
//!
//! ## Implementation note
//! The Bool-operand guard lives in `compile_binop` (`reify-compiler/src/expr.rs`).
//! `infer_binop_type(BinOp::Implies, _, _)` returns `Type::Bool` unconditionally, so
//! without the guard `5 implies 3` would silently type-check — these tests pin the guard.

use reify_core::Severity;
use reify_test_support::{assert_has_diagnostic, assert_no_error_diagnostics, compile_source};

/// `5 implies 3` must produce at least one `Severity::Error` diagnostic.
///
/// The operands are `Int`-typed, not `Bool`-typed, so the Bool-operand guard in
/// `compile_binop` must reject them with a clear error message containing "implies".
#[test]
fn implies_rejects_non_bool_operands() {
    let source = r#"
structure def Bad {
    let result : Bool = 5 implies 3
}
"#;
    let module = compile_source(source);
    assert_has_diagnostic(&module.diagnostics, Severity::Error, "implies");
}

/// `true implies false` is valid: both operands are `Bool`-typed.
///
/// The Bool-operand guard must NOT fire for Bool operands.
#[test]
fn implies_accepts_bool_operands() {
    let source = r#"
structure def Good {
    let result : Bool = true implies false
}
"#;
    let module = compile_source(source);
    assert_no_error_diagnostics(&module.diagnostics, "true implies false should compile cleanly");
}

/// `p implies true` (where `p` is a `Bool` param) must compile without errors.
///
/// A `param p : Bool` (no default) is Bool-typed at compile time even though
/// it evaluates to `Undef` at runtime.  This test pins that the Bool-operand guard
/// does not false-positive on a Bool-typed value reference — only literal Int/Real/etc
/// operands should trigger the diagnostic.
#[test]
fn implies_accepts_bool_param_operand() {
    let source = r#"
structure def GoodParam {
    param p : Bool
    let result : Bool = p implies true
}
"#;
    let module = compile_source(source);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "p implies true should compile cleanly when p is Bool",
    );
}
