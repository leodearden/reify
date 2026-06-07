//! Permissive generic body checking — task 4232 (γ D4).
//!
//! Verifies that `Type::TypeParam`-typed values act as resolution wildcards
//! inside a generic fn body so operations on them are NOT eagerly rejected.
//!
//! Case (a) is RED until step-4 adds the arg-side wildcard in
//! `resolve_function_overload` (type_compat.rs).
//! Cases (b) and (c) are pinned GREEN: (b) because the conformance check
//! already skips `TypeParam` args (conformance/mod.rs:915), and (c) because
//! `clamp` is a builtin that routes through the math/first-arg-fallback path,
//! not through user-function overload resolution.

use reify_core::Severity;
use reify_test_support::compile_source;

// ── (a) RED — arg-wildcard: TypeParam arg passed to concrete-param fn ────────

/// `fn sink(x: Real) -> Real { x }` called as `sink(x)` inside
/// `fn use_it<T>(x: T) -> Real { sink(x) }` must compile with NO
/// "no matching overload" Error.
///
/// Currently RED: `resolve_function_overload` matches params but not args —
/// Real param ≠ TypeParam("T") arg → NoMatch → "no matching overload" error.
///
/// After step-4: the arg-side `type_carries_type_param(arg_ty)` predicate
/// makes TypeParam("T") a wildcard → Resolved(sink) → no error.
#[test]
fn generic_body_type_param_arg_to_concrete_param_no_error() {
    let source = r#"
        fn sink(x: Real) -> Real { x }
        fn use_it<T>(x: T) -> Real { sink(x) }
        structure S { let v = use_it(5mm) }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for sink(x) in generic body, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (b) GREEN pin — trait-object param: conformance already skips TypeParam ─

/// `fn need_movable(m: Movable) -> Real { 0.0 }` called as `need_movable(x)`
/// inside `fn use2<T>(x: T) -> Real { need_movable(x) }` must emit NO
/// conformance error.
///
/// This is already GREEN: the conformance walker skips `TypeParam` args at
/// `conformance/mod.rs:915`, so no `TypeNotConformingToTrait` is ever emitted
/// for a TypeParam-typed arg. Pinning keeps this invariant explicit.
#[test]
fn generic_body_type_param_arg_to_trait_param_no_conformance_error() {
    let source = r#"
        trait Movable { param v : Length }
        fn need_movable(m: Movable) -> Real { 0.0 }
        fn use2<T>(x: T) -> Real { need_movable(x) }
        structure S2 { let v = use2(5mm) }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for need_movable(x) in generic body, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (c) GREEN pin — builtin (clamp) already permissive ────────────────────

/// `clamp(x, x, x)` inside `fn use_clamp<T>(x: T) -> T` must compile clean.
/// `clamp` is a math builtin (NoUserFunctions from resolve_function_overload)
/// so the conformance / overload-rejection path never fires — already GREEN.
/// Pins the D4 builtin baseline so regressions are caught immediately.
#[test]
fn generic_body_builtin_clamp_already_permissive() {
    let source = r#"
        fn use_clamp<T>(x: T) -> T { clamp(x, x, x) }
        structure S3 { let v = use_clamp(5mm) }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for clamp(x,x,x) in generic body, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
