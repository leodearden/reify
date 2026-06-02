//! Compiler-behaviour tests for task 3918: `undef` literal lowering.
//!
//! ## Coverage
//!
//! (a) `let a = undef` — the compiled let-expr must be
//!     `CompiledExprKind::Literal(Value::Undef)`, must emit NO "unresolved name"
//!     diagnostic, and must emit NO "not yet" placeholder diagnostic.
//!
//! (b) `let a = 5 * undef` — must emit no "unresolved name: undef" diagnostic
//!     (regression: before grammar fix `undef` was an undefined-variable ref).
//!
//! ## Step mapping
//!
//! These tests are RED against the step-4 stub (which emits a poison diagnostic
//! containing "not yet supported") and GREEN after step-6 replaces the stub with
//! `CompiledExpr::literal(Value::Undef, Type::Error)`.
//!
//! User-observable signal:
//!   `cargo test -p reify-compiler --test undef_literal_compile_tests`

use reify_ir::{CompiledExprKind, Value};
use reify_test_support::{compile_source, get_let_expr};

/// `let a = undef` must compile to `Literal(Value::Undef)` with no spurious
/// diagnostics — no "unresolved name" and no "not yet supported" placeholder.
///
/// RED against step-4 stub (stub emits "not yet supported" poison diagnostic).
/// GREEN after step-6 real lowering.
#[test]
fn undef_literal_compiles_to_value_undef() {
    let source = r#"
structure S {
    let a = undef
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "a");

    // The compiled expression must be a Value::Undef literal.
    assert!(
        matches!(&expr.kind, CompiledExprKind::Literal(Value::Undef)),
        "expected CompiledExprKind::Literal(Value::Undef) for `let a = undef`, got {:?}",
        expr.kind,
    );

    // No "unresolved name" diagnostic — undef must NOT be treated as an
    // undefined variable reference.
    let has_unresolved_name = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved name"));
    assert!(
        !has_unresolved_name,
        "expected no 'unresolved name' diagnostic for `undef` literal, got: {:?}",
        module.diagnostics,
    );

    // No "not yet" placeholder diagnostic — stub must be replaced.
    let has_not_yet = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("not yet"));
    assert!(
        !has_not_yet,
        "expected no 'not yet' placeholder diagnostic for `undef` literal (stub not replaced?), got: {:?}",
        module.diagnostics,
    );
}

/// `let a = 5 * undef` must emit no "unresolved name: undef" diagnostic.
///
/// Regression guard: before the grammar fix `undef` parsed as `identifier("undef")`
/// and the compiler emitted an "unresolved name" error. After the fix `undef` is a
/// literal, so the expression should compile cleanly.
///
/// RED against step-4 stub (stub emits "not yet supported" instead of a clean compile).
/// GREEN after step-6 real lowering.
#[test]
fn binary_with_undef_emits_no_unresolved_name_diagnostic() {
    let source = r#"
structure S {
    let a = 5 * undef
}
"#;
    let module = compile_source(source);

    // Must not emit "unresolved name" for `undef`.
    let has_unresolved_name = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved name"));
    assert!(
        !has_unresolved_name,
        "expected no 'unresolved name' diagnostic for `undef` in `5 * undef`, got: {:?}",
        module.diagnostics,
    );
}
