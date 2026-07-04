//! B1 resolution tests for generic-enum-typed fn PARAMS — task result-fallback
//! Layer-B substrate (PRD docs/prds/v0_6/result-and-fallback.md).
//!
//! `resolve_type_expr_with_aliases_kinded`'s only enum path is the bare-name
//! `RESOLUTION_ENUM_NAMES` fallback (`type_resolution.rs`), gated on
//! `type_args.is_empty()`. A generic-enum param type like `Result<T, E>` has
//! non-empty type args and falls through to `FnUnknownTypeParam` +
//! `Type::Error`. These tests pin the fix: `compile_function`'s param loop
//! must (a) install an `EnumNameScope` so an enum nested inside a
//! parameterized builtin (`Option<Color>`) resolves its inner name, and (b)
//! call `resolve_enum_type_with_args` in the param None-path fallback so a
//! generic-enum param (`Result<T, E>`) lowers to `Type::Applied`.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib};

// ── (a) generic-enum param (Result<T, E>) lowers to Type::Applied ───────────

/// [CORE SIGNAL] `fn probe<T,E>(r: Result<T,E>) -> Bool { true }` — the
/// param's declared type must resolve to
/// `Type::Applied{name:"Result", args:[TypeParam("T"), TypeParam("E")]}`,
/// with no Error-severity diagnostics.
///
/// RED: the kinded resolver has no enum-with-args path, so this currently
/// falls through to `push_signature_type_error` → `FnUnknownTypeParam` and
/// the param type is `Type::Error`.
///
/// Amendment (reviewer test_coverage pass): widened from a narrow
/// `FnUnknownTypeParam`-only filter to ALL Error-severity diagnostics —
/// mirroring sibling test (b) below — so a stale/duplicate diagnostic
/// pushed by the `resolve_enum_type_with_args` None-path fallback (e.g. a
/// future wrong-arity or non-generic-enum-given-args regression) cannot
/// slip past this assertion undetected.
#[test]
fn generic_enum_param_resolves_to_applied() {
    let source = r#"
fn probe<T, E>(r: Result<T, E>) -> Bool { true }
"#;
    let module = compile_source_with_stdlib(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for `r: Result<T, E>`, got: {:?}",
        errors
    );

    let probe = module
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap_or_else(|| panic!("function 'probe' not found"));
    assert_eq!(
        probe.params[0].1,
        Type::Applied {
            name: "Result".to_string(),
            args: vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string()),
            ],
        },
        "probe's `r` param should resolve to Applied{{Result,[T,E]}}, got {:?}",
        probe.params[0].1
    );
}

// ── (b) enum nested inside a parameterized builtin (EnumNameScope) ──────────

/// [CORE SIGNAL] `fn t(o: Option<Color>) -> Bool { true }`, with `Color`
/// declared as a same-module non-generic enum — the param's declared type
/// must resolve to `Type::Option(Box::new(Type::Enum("Color")))`, with no
/// Error diagnostics.
///
/// RED: `compile_function`'s param loop does not install an `EnumNameScope`,
/// so the inner `Color` type-arg resolution (behind
/// `resolve_parameterized_builtin_type`) cannot see the module's enum names
/// and fails → `UnresolvedType` + `Type::Error` (non-generic fn, so
/// `push_signature_type_error` takes the `UnresolvedType` branch, not
/// `FnUnknownTypeParam`).
#[test]
fn enum_nested_in_option_param_resolves_via_enum_name_scope() {
    let source = r#"
enum Color { RGB { r: Real } }

fn t(o: Option<Color>) -> Bool { true }
"#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for `o: Option<Color>`, got: {:?}",
        errors
    );

    let t_fn = module
        .functions
        .iter()
        .find(|f| f.name == "t")
        .unwrap_or_else(|| panic!("function 't' not found"));
    assert_eq!(
        t_fn.params[0].1,
        Type::Option(Box::new(Type::Enum("Color".to_string()))),
        "t's `o` param should resolve to Option<Enum(Color)>, got {:?}",
        t_fn.params[0].1
    );
}
