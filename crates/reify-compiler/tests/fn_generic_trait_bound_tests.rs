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

// ── (e) Multiple call sites: both violations emitted ──────────────────────

/// When both a valid and an invalid call to `f<T: Rigid>` appear in separate
/// structures, only the invalid one produces a bound error. The post-pass
/// walks ALL compiled templates so the violation in `Sbad` is caught even
/// though `Sok` compiled cleanly — pins that the post-pass runs globally.
///
/// RED until step-2: the post-pass bound-check walker is not yet wired.
#[test]
fn fn_bound_multiple_call_sites_only_bad_errors() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Widget { param x : Length = 5mm }
        fn f<T: Rigid>(x: T) -> T { x }
        structure Sok { let ok = f(Bolt()) }
        structure Sbad { let bad = f(Widget()) }
    "#;
    let module = compile_source(source);

    // There must be at least one bound-violation error mentioning Widget+Rigid.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected bound error for f(Widget()) in Sbad"
    );
    let has_bound_error = errors
        .iter()
        .any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_bound_error,
        "expected error mentioning Widget and Rigid, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    // No error should mention Bolt.
    let has_bolt_error = errors.iter().any(|e| e.message.contains("Bolt"));
    assert!(
        !has_bolt_error,
        "Bolt satisfies Rigid — no error expected for f(Bolt()), got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (g) Forwarded unbounded type-param — deferred-soundness pin ───────────

/// `fn g<U>(x: U) -> U { f(x) }` with `fn f<T: Rigid>(x: T)`.
///
/// Inside g's body, `x` has type `TypeParam("U")`.  When the post-pass sees
/// `f(x)`, it unifies `TypeParam("T")` with `TypeParam("U")` → subst =
/// {"T": TypeParam("U")}.  `check_type_param_bounds` receives
/// `type_args = [TypeParam("U")]` and skips it at entity.rs:3692 — no eager
/// bound error, even for `g(Widget())`.
///
/// This pins the **deferred-soundness contract**: the bound is enforced at the
/// concrete outer call site only if `g` itself declares `<U: Rigid>`.  A
/// future monomorphisation phase would re-check at that point; the eager
/// post-pass correctly defers.
#[test]
fn fn_bound_unbounded_forwarded_to_bounded_no_eager_error() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Widget { param x : Length = 5mm }
        fn f<T: Rigid>(x: T) -> T { x }
        fn g<U>(x: U) -> U { f(x) }
        structure Sfw { let v = g(Widget()) }
    "#;
    let module = compile_source(source);

    // No eager bound-violation error: TypeParam("U") is a forwarded type-param
    // and check_type_param_bounds skips it (entity.rs:3692).
    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("does not satisfy"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "forwarding TypeParam(U) into f<T: Rigid> should not emit an eager bound error \
         (deferred to g's concrete call site); got: {:?}",
        bound_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (h) Multi-bound: one bound satisfied, one violated ────────────────────

/// `fn dual<T: HasMass + HasWidth>(x: T)` requires T to satisfy BOTH bounds.
/// `Bolt` only implements `HasMass`, not `HasWidth`. Calling `dual(Bolt())`
/// must emit an error mentioning the missing bound `HasWidth`.
///
/// Exercises the per-bound loop inside `check_type_param_bounds` (entity.rs:3703):
/// each bound is checked independently, so a partial violator still gets caught.
#[test]
fn fn_bound_multi_bound_partial_violation() {
    let source = r#"
        trait HasMass { param mass : Mass }
        trait HasWidth { param width : Length }
        structure def Bolt : HasMass { param mass : Mass = 1kg }
        fn dual<T: HasMass + HasWidth>(x: T) -> T { x }
        structure S { let bad = dual(Bolt()) }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected a bound error for Bolt missing HasWidth, got none"
    );
    let has_width_error = errors.iter().any(|e| e.message.contains("HasWidth"));
    assert!(
        has_width_error,
        "expected error mentioning HasWidth (the unsatisfied bound), got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Same multi-bound fn `dual<T: HasMass + HasWidth>`, but with `Full` which
/// satisfies both bounds — no error expected.
#[test]
fn fn_bound_multi_bound_both_satisfied_no_error() {
    let source = r#"
        trait HasMass { param mass : Mass }
        trait HasWidth { param width : Length }
        structure def Full : HasMass + HasWidth {
            param mass : Mass = 2kg
            param width : Length = 10mm
        }
        fn dual<T: HasMass + HasWidth>(x: T) -> T { x }
        structure Sok { let ok = dual(Full()) }
    "#;
    let module = compile_source(source);

    let bound_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("does not satisfy"))
        .collect();
    assert!(
        bound_errors.is_empty(),
        "Full satisfies HasMass + HasWidth, expected no error, got: {:?}",
        bound_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── (i) Nested-constructor (List<T>) unification path ─────────────────────

/// `fn wrap<T: Rigid>(items: List<T>) -> List<T>` called with a `List<Bolt>`
/// (ok) and a `List<Widget>` (violation).
///
/// Exercises the constructor-recursion path in `unify` (type_compat.rs:452):
/// `unify(List(TypeParam("T")), List(UserDefined("Bolt")))` recurses into
/// `unify(TypeParam("T"), UserDefined("Bolt"))`, binding T → Bolt.
/// Without this path the subst would remain empty and the bound check would
/// skip T (treating it as unbound), silently missing the violation.
///
/// To construct a `List<Bolt>` value at the call site we chain through
/// `fn single<T>(x: T) -> List<T> { [x] }` — proven to produce a
/// `List<UserDefined(Bolt)>` result_type (fn_generic_call_inference_tests.rs B2).
#[test]
fn fn_bound_nested_list_constructor_violation() {
    let source = r#"
        trait Rigid { param mass : Mass }
        structure def Bolt : Rigid { param mass : Mass = 1kg }
        structure def Widget { param x : Length = 5mm }
        fn single<T>(x: T) -> List<T> { [x] }
        fn wrap<T: Rigid>(items: List<T>) -> List<T> { items }
        structure Sok  { let ok  = wrap(single(Bolt())) }
        structure Sbad { let bad = wrap(single(Widget())) }
    "#;
    let module = compile_source(source);

    // Must have exactly one bound error — for Widget, not Bolt.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected a bound error for List<Widget> passed to wrap<T: Rigid>, got none"
    );
    let has_widget_error = errors.iter().any(|e| e.message.contains("Widget") && e.message.contains("Rigid"));
    assert!(
        has_widget_error,
        "expected error mentioning Widget and Rigid, got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let has_bolt_error = errors.iter().any(|e| e.message.contains("Bolt"));
    assert!(
        !has_bolt_error,
        "Bolt satisfies Rigid — no error expected for wrap(single(Bolt())), got: {:?}",
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
