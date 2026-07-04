//! B1 resolution tests for generic-enum-typed fn RETURN types — task
//! result-fallback Layer-B substrate (return-position sibling of #4991's
//! param-position `fn_enum_param_resolution_tests.rs`).
//!
//! `compile_function`'s return-type `None` arm only calls
//! `resolve_type_expr_with_aliases_kinded`, which (a) has no generic-enum-
//! with-args path and (b) installs no `EnumNameScope`. A generic-enum return
//! type like `Result<T, E>` has non-empty type args and falls through to
//! `FnUnknownTypeParam` + `Type::Error`. These tests pin the fix:
//! `compile_function`'s return-type resolution must (a) call
//! `resolve_enum_type_with_args` in the return-type None-path fallback so a
//! generic-enum return type (`Result<T, E>`) lowers to `Type::Applied`, and
//! (b) install an `EnumNameScope` so an enum nested inside a parameterized
//! builtin (`Option<Color>`) resolves its inner name.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib};

// ── (a) generic-enum return type (Result<T, E>) lowers to Type::Applied ─────

/// [CORE SIGNAL] `fn probe<T, E>(seed: T, err: E) -> Result<T, E> { true }` —
/// the fn's declared return type must resolve to
/// `Type::Applied{name:"Result", args:[TypeParam("T"), TypeParam("E")]}`,
/// with no Error-severity diagnostics.
///
/// A trivial `{ true }` body is used because `compile_function` performs no
/// body-vs-return-type compatibility check (verified), so this isolates the
/// assertion to pure return-TYPE resolution.
///
/// RED: the return-type `None` arm has no enum-with-args path, so this
/// currently falls through to `push_signature_type_error` →
/// `FnUnknownTypeParam` and the return type is `Type::Error`.
#[test]
fn generic_enum_return_resolves_to_applied() {
    let source = r#"
fn probe<T, E>(seed: T, err: E) -> Result<T, E> { true }
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for `-> Result<T, E>`, got: {:?}",
        errors
    );

    let probe = module
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap_or_else(|| panic!("function 'probe' not found"));
    assert_eq!(
        probe.return_type,
        Type::Applied {
            name: "Result".to_string(),
            args: vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string()),
            ],
        },
        "probe's return type should resolve to Applied{{Result,[T,E]}}, got {:?}",
        probe.return_type
    );
}
