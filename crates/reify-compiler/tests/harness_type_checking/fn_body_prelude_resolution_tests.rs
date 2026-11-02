//! A stdlib `.ri` fn called inside a user `fn` body binds to its stdlib
//! signature, exactly as the same call does at top level (task #8393, σ S-6).
//!
//! The defect these pin: a body that compiled against the user-only fn table
//! saw no stdlib `.ri` fn, so the call fell to `expr.rs`'s terminal first-arg
//! fallback. That typed it from its first argument (or `Real` for zero args),
//! lowered it to a generic `FunctionCall` that eval cannot dispatch, and
//! emitted nothing, because every prelude fn name is a declared name.
//!
//! Every assertion reads the public compiled IR: a body's result type and
//! kind, and the module's coded diagnostics.

use reify_core::{DiagnosticCode, Type};
use reify_ir::CompiledExprKind;
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The compiled body expression of the fn registered as `fn_name`.
fn fn_body_result_expr<'m>(
    module: &'m reify_compiler::CompiledModule,
    fn_name: &str,
) -> &'m reify_ir::CompiledExpr {
    &module
        .functions
        .iter()
        .find(|f| f.name == fn_name)
        .unwrap_or_else(|| panic!("no compiled fn named '{fn_name}'"))
        .body
        .result_expr
}

/// Every Error-severity diagnostic message in `module`, in source order.
fn errors(module: &reify_compiler::CompiledModule) -> Vec<&str> {
    errors_only(module)
        .into_iter()
        .map(|d| d.message.as_str())
        .collect()
}

/// `List<Length>`, the declared return type of `standard_bolt_lengths()`.
fn list_of_length() -> Type {
    Type::List(Box::new(Type::length()))
}

/// The diagnostics in `module` coded `NoMatchingOverload` whose message names
/// `callee`.
fn no_matching_overload_naming<'m>(
    module: &'m reify_compiler::CompiledModule,
    callee: &str,
) -> Vec<&'m reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::NoMatchingOverload))
        .filter(|d| d.message.contains(callee))
        .collect()
}

#[test]
fn fn_body_zero_arg_stdlib_fn_is_typed_from_its_signature() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn wrap_bolts() -> List<Length> { standard_bolt_lengths() }
    "#,
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
    assert_eq!(
        fn_body_result_expr(&module, "wrap_bolts").result_type,
        list_of_length()
    );
}

#[test]
fn fn_body_generic_field_stdlib_fn_is_typed_from_its_signature() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn above(f: Field<Real, Real>, v: Real) -> Field<Real, Bool> { threshold(f, v) }
    "#,
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
    assert_eq!(
        fn_body_result_expr(&module, "above").result_type,
        Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::Bool),
        }
    );
}

/// `pointwise_min` returns its argument type, so the body's type is the same
/// whether the call binds or falls through; the lowered kind is what tells
/// them apart.
#[test]
fn fn_body_pointwise_min_lowers_to_a_stdlib_fn_call() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn lower(a: Field<Real, Real>, b: Field<Real, Real>) -> Field<Real, Real> {
            pointwise_min(a, b)
        }
    "#,
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
    let body = fn_body_result_expr(&module, "lower");
    assert!(
        matches!(
            &body.kind,
            CompiledExprKind::UserFunctionCall { function_name, .. }
                if function_name == "pointwise_min"
        ),
        "expected a UserFunctionCall of pointwise_min, got {:?}",
        body.kind
    );
}

#[test]
fn fn_body_mis_arity_stdlib_call_reports_no_matching_overload_like_top_level() {
    let top_level = compile_source_with_stdlib(
        r#"
        structure def S { let x = standard_bolt_lengths(1.0) }
    "#,
    );
    assert_eq!(
        no_matching_overload_naming(&top_level, "standard_bolt_lengths").len(),
        1,
        "parity control: the top-level call must report NoMatchingOverload; got {:?}",
        top_level.diagnostics
    );

    let in_body = compile_source_with_stdlib(
        r#"
        pub fn bad() -> Real { standard_bolt_lengths(1.0) }
    "#,
    );
    assert_eq!(
        no_matching_overload_naming(&in_body, "standard_bolt_lengths").len(),
        1,
        "the same call in a fn body must report NoMatchingOverload; got {:?}",
        in_body.diagnostics
    );
}

/// CONTROL, green before and after #8393: an earlier-declared user fn with a
/// stdlib fn's signature hides it in later bodies. A table that simply
/// concatenated user and prelude fns would make this call `Ambiguous`.
#[test]
fn earlier_declared_user_fn_shadows_stdlib_fn_in_a_later_body() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn standard_bolt_lengths() -> Length { 5mm }
        pub fn wrap() -> Length { standard_bolt_lengths() }
    "#,
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
    assert_eq!(
        fn_body_result_expr(&module, "wrap").result_type,
        Type::length()
    );
}

/// A trait-STATIC fn body is compiled eagerly at trait declaration, under its
/// registered `Trait::method` symbol, and resolves stdlib calls the same way.
#[test]
fn trait_static_fn_body_stdlib_call_is_typed_from_its_signature() {
    let module = compile_source_with_stdlib(
        r#"
        trait Stock { fn bolts() -> List<Length> { standard_bolt_lengths() } }
    "#,
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
    assert_eq!(
        fn_body_result_expr(&module, "Stock::bolts").result_type,
        list_of_length()
    );
}
