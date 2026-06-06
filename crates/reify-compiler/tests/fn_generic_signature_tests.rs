//! Generic-function signature tests — task 4230 (generic-user-fns α).
//!
//! Verifies that:
//!   (step-1/step-2) `CompiledFunction.type_params` is correctly lowered from
//!     `fn_def.type_params` via `convert_type_params`, and is empty for non-generic fns.
//!   (step-3/step-4) Bare type-param names (`T`) resolve to `Type::TypeParam("T")` in
//!     fn parameter and return-type positions.
//!   (step-5/step-6) Type-param names resolve inside parameterised builtin types
//!     (`Field<D, C>`, `List<T>`).
//!   (step-7/step-8) An undeclared name in a generic-fn signature emits
//!     `DiagnosticCode::FnUnknownTypeParam`; a non-generic fn's unknown type still emits
//!     `DiagnosticCode::UnresolvedType` (INV-6 regression pin).
//!
//! All tests use `compile_source` (no stdlib) and minimal bodies (`{ value }`, `{ x }`)
//! that reference a param. `compile_function` does NOT type-check the body against the
//! declared return type, so trivial bodies produce no diagnostics and need no stdlib symbol.

use reify_test_support::compile_source;
use reify_core::{DiagnosticCode, Severity, Type};

// ────────────────────────────────────────────────────────────────────────────
// Step-1 / Step-2: CompiledFunction.type_params lowering
// ────────────────────────────────────────────────────────────────────────────

/// Generic fn's `type_params` is lowered from the declared `<D, C>` type-param list;
/// non-generic fn's `type_params` is empty (INV-6).
///
/// RED until step-2: `compile_function` stubs `type_params: Vec::new()`, so
/// `constant_field.type_params` is empty instead of ["D", "C"].
#[test]
fn generic_fn_lowers_type_params_and_nongeneric_is_empty() {
    let source = r#"
        fn constant_field<D, C>(value: C) -> Field<D, C> { value }
        fn plain(x: Real) -> Real { x }
    "#;
    let module = compile_source(source);

    // constant_field must be present (compile_function returns Some even with
    // unresolved signature types, falling back to Type::Real).
    let cf = module
        .functions
        .iter()
        .find(|f| f.name == "constant_field")
        .expect("function 'constant_field' should be compiled");

    // type_params should be lowered from <D, C>.
    assert_eq!(
        cf.type_params.len(),
        2,
        "constant_field should have 2 type params, got {:?}",
        cf.type_params.iter().map(|tp| &tp.name).collect::<Vec<_>>()
    );
    assert_eq!(cf.type_params[0].name, "D");
    assert_eq!(cf.type_params[1].name, "C");
    // No bounds, no default for simple type params.
    assert!(
        cf.type_params[0].bounds.is_empty(),
        "D should have no bounds"
    );
    assert!(
        cf.type_params[1].bounds.is_empty(),
        "C should have no bounds"
    );
    assert!(
        cf.type_params[0].default.is_none(),
        "D should have no default"
    );
    assert!(
        cf.type_params[1].default.is_none(),
        "C should have no default"
    );

    // Non-generic fn must have empty type_params (INV-6).
    let plain = module
        .functions
        .iter()
        .find(|f| f.name == "plain")
        .expect("function 'plain' should be compiled");
    assert!(
        plain.type_params.is_empty(),
        "non-generic fn 'plain' should have empty type_params, got {:?}",
        plain.type_params
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-3 / Step-4: bare type-param name resolves in param and return position
// ────────────────────────────────────────────────────────────────────────────

/// Bare type-param name `T` resolves to `Type::TypeParam("T")` in both the
/// parameter and the return-type positions of a generic fn, with zero Error
/// diagnostics.
///
/// RED until step-4: `compile_function` passes `empty_params` to
/// `resolve_type_expr_with_aliases`, so `T` is unknown → "unresolved type" Error +
/// `Type::Real` fallback.
#[test]
fn bare_type_param_resolves_in_param_and_return() {
    let source = r#"fn id<T>(x: T) -> T { x }"#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for fn id<T>, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "id")
        .expect("function 'id' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::TypeParam("T".to_string()),
        "param x should resolve to Type::TypeParam(\"T\")"
    );
    assert_eq!(
        func.return_type,
        Type::TypeParam("T".to_string()),
        "return type should resolve to Type::TypeParam(\"T\")"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5 / Step-6: type-param names resolve inside parameterised builtin types
// ────────────────────────────────────────────────────────────────────────────

/// Type-param names resolve correctly when used as type arguments to parameterised
/// builtins (`Field<D, C>`, `List<T>`).
///
/// RED until step-6: `resolve_parameterized_builtin_type` uses its own internal
/// `empty_type_params` for all inner `resolve_type_expr_with_aliases` calls, so D/C/T
/// remain unresolved → the outer Field/List returns None → "unresolved type" Error +
/// `Type::Real` fallback.
#[test]
fn type_param_resolves_inside_parameterized_builtin() {
    let source = r#"
        fn constant_field<D, C>(value: C) -> Field<D, C> { value }
        fn single<T>(x: T) -> List<T> { x }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    let cf = module
        .functions
        .iter()
        .find(|f| f.name == "constant_field")
        .expect("function 'constant_field' should be compiled");

    // param: C
    assert_eq!(
        cf.params[0].1,
        Type::TypeParam("C".to_string()),
        "constant_field param 'value' should resolve to Type::TypeParam(\"C\")"
    );

    // return type: Field<D, C>
    assert_eq!(
        cf.return_type,
        Type::Field {
            domain: Box::new(Type::TypeParam("D".to_string())),
            codomain: Box::new(Type::TypeParam("C".to_string())),
        },
        "constant_field return type should be Field<TypeParam(D), TypeParam(C)>"
    );

    let single = module
        .functions
        .iter()
        .find(|f| f.name == "single")
        .expect("function 'single' should be compiled");

    // return type: List<T>
    assert_eq!(
        single.return_type,
        Type::List(Box::new(Type::TypeParam("T".to_string()))),
        "single return type should be List<TypeParam(T)>"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-7 / Step-8: FnUnknownTypeParam diagnostic (INV-6 regression pin)
// ────────────────────────────────────────────────────────────────────────────

/// A generic fn with an undeclared name in its signature (`U` not in `<T>`)
/// must emit a diagnostic with `code == Some(DiagnosticCode::FnUnknownTypeParam)`,
/// and the message must mention the undeclared name.
///
/// RED until step-8: `DiagnosticCode::FnUnknownTypeParam` doesn't exist yet;
/// the generic case currently emits `DiagnosticCode::UnresolvedType`.
#[test]
fn generic_fn_undeclared_signature_param_emits_fn_unknown_type_param() {
    // `U` is not declared in `<T>` — an undeclared type-param name.
    let source = r#"fn f<T>(x: U) -> T { x }"#;
    let module = compile_source(source);

    let fn_unknown_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnUnknownTypeParam));
    assert!(
        fn_unknown_diag.is_some(),
        "expected a diagnostic with code FnUnknownTypeParam for undeclared type param 'U'; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let diag = fn_unknown_diag.unwrap();
    assert!(
        diag.message.contains('U'),
        "FnUnknownTypeParam diagnostic message should mention 'U', got: {:?}",
        diag.message
    );
}

/// A non-generic fn with an unknown type keeps `DiagnosticCode::UnresolvedType`
/// and its message unchanged (INV-6 regression pin).
///
/// RED until step-8: only meaningful once FnUnknownTypeParam exists; ensures
/// we didn't accidentally change non-generic fn behavior.
#[test]
fn nongeneric_unknown_type_keeps_unresolved_type() {
    let source = r#"fn g(x: NoSuchType) -> Real { x }"#;
    let module = compile_source(source);

    let unresolved_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::UnresolvedType) && d.severity == Severity::Error);
    assert!(
        unresolved_diag.is_some(),
        "expected a diagnostic with code UnresolvedType for non-generic fn with unknown type; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let diag = unresolved_diag.unwrap();
    assert!(
        diag.message.contains("unresolved type"),
        "UnresolvedType message should contain 'unresolved type', got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("NoSuchType"),
        "UnresolvedType message should mention 'NoSuchType', got: {:?}",
        diag.message
    );

    // Must NOT emit FnUnknownTypeParam for a non-generic fn.
    let fn_unknown_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnUnknownTypeParam));
    assert!(
        fn_unknown_diag.is_none(),
        "non-generic fn must not emit FnUnknownTypeParam, got: {:?}",
        fn_unknown_diag
    );
}
