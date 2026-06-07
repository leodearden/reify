//! Trait-bound validation on fn type-params at call sites — task 4232 (γ D5).
//!
//! All cases in this file are RED until step-2 wires `check_expr_fn_type_param_bounds`
//! into the `phase_fn_arg_conformance` post-pass.
//!
//! Pattern: user-defined `Rigid` trait + conforming `Bolt : Rigid` + non-conforming
//! `Widget`, mirroring `trait_bounds_tests.rs` and the `fn_generic_call_inference_tests.rs`
//! harness. Call sites live in entity bodies (`structure S { let v = f(Bolt()) }`),
//! exercising the post-pass path (NOT compile_function threading).

use reify_core::Severity;
use reify_test_support::compile_source;

// ── (a) Bound violation: non-conforming arg ────────────────────────────────

/// Calling `f<T: Rigid>(x: T)` with a `Widget` (not Rigid) must emit an Error
/// whose message contains both "Widget" and "Rigid".
///
/// RED until step-2: `check_type_param_bounds` is not wired for fn calls yet.
#[test]
fn fn_bound_violation_non_conforming_arg() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Widget { param x : Length = 5mm }
        fn f<T: Rigid>(x: T) -> T { x }
        structure S { let bad = f(Widget()) }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an error when passing Widget (not Rigid) to f<T: Rigid>, got none"
    );
    let has_bound_error = errors
        .iter()
        .any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_bound_error,
        "expected error mentioning Widget and Rigid, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (b) Bound satisfied: conforming arg — no error ─────────────────────────

/// Calling `f<T: Rigid>(x: T)` with a `Bolt` (implements Rigid) must produce NO
/// bound-violation error (no error message containing "does not satisfy").
///
/// RED until step-2 (should stay GREEN after — the check correctly skips valid args).
#[test]
fn fn_bound_valid_conforming_arg_no_error() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        fn f<T: Rigid>(x: T) -> T { x }
        structure Sok { let ok = f(Bolt()) }
    "#;
    let module = compile_source(source);

    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("does not satisfy"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "expected no bound-violation error for f(Bolt()), got: {:?}",
        bound_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (c) Transitive refinement chain ────────────────────────────────────────

/// `fn f<T: Rigid>` called with `Steel` which satisfies Rigid through a
/// refinement chain `Steel : Mid : Rigid`. Must compile without a bound error.
///
/// RED until step-2: the transitive check is in `satisfies_trait_bound` (already
/// used by the structure side); it just needs to be called for fn type-params.
#[test]
fn fn_bound_transitive_refinement_satisfied() {
    let source = r#"
        trait Rigid { param mass : Mass }
        trait Mid : Rigid { param mass : Mass }
        structure def Steel : Mid { param mass : Mass = 8kg }
        fn f<T: Rigid>(x: T) -> T { x }
        structure Strans { let v = f(Steel()) }
    "#;
    let module = compile_source(source);

    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("does not satisfy"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "expected no bound error for f(Steel()) with Steel:Mid:Rigid chain, got: {:?}",
        bound_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (d) Unbound bounded param — not checked ────────────────────────────────

/// `fn h<T: Rigid>() -> Int { 0 }` called as `h()` (no args → T unresolved):
/// must emit NO missing-type-arg error and NO bound error. The post-pass
/// self-fills unbound params as `TypeParam("T")` which `check_type_param_bounds`
/// skips at entity.rs:3692.
///
/// RED until step-2; the unbound-skip rule is enforced by the type_args construction.
#[test]
fn fn_bound_unbound_type_param_not_checked() {
    let source = r#"
        trait Rigid { param mass : Mass }
        fn h<T: Rigid>() -> Int { 0 }
        structure Su { let v = h() }
    "#;
    let module = compile_source(source);

    // No Error-severity diagnostics at all expected.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for h() with unbound T, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (e) Forward-reference order independence ───────────────────────────────

/// A non-conforming structure declared AFTER the call site still errors.
/// The post-pass runs after all entities compile, giving order independence.
///
/// RED until step-2: the post-pass path naturally provides this — tested
/// explicitly to pin the invariant.
#[test]
fn fn_bound_forward_ref_non_conforming_still_errors() {
    let source = r#"
        trait Rigid { param mass : Mass }
        fn f<T: Rigid>(x: T) -> T { x }
        structure Sbad { let bad = f(Widget()) }
        structure def Widget { param x : Length = 5mm }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected bound error for forward-declared Widget not satisfying Rigid"
    );
    let has_bound_error = errors
        .iter()
        .any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_bound_error,
        "expected error mentioning Widget and Rigid (forward-ref), got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (f) INV-6 unbounded param pin — no spurious check ─────────────────────

/// `fn id<T>(x: T) -> T { x }` has no bounds on T. Calling `id(Bolt())` must
/// NOT emit any bound error. Pins INV-6: unbounded type params are never checked.
///
/// This should be GREEN before and after step-2.
#[test]
fn fn_no_bound_on_type_param_no_check() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        fn id<T>(x: T) -> T { x }
        structure Sid { let v = id(Bolt()) }
    "#;
    let module = compile_source(source);

    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("does not satisfy"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "expected no bound error for id(Bolt()) — T has no bounds, got: {:?}",
        bound_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
