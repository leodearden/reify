//! Engine-level tests for the `@optimized` dispatch path (Task 273).
//!
//! These tests drive:
//!   - `Engine::register_optimized_impl` stores a boxed `OptimizedImpl`.
//!   - When a compiled module contains a constraint instantiation from a
//!     `@optimized("target")` constraint def AND `target` is registered, the
//!     optimized impl handles that constraint and the language-level
//!     `ConstraintChecker` does NOT see it.
//!   - When no impl is registered for `target`, the language-level checker
//!     handles the constraint as today (fallback).
//!   - Mixed batches (one optimized + one fallback) produce correct per-id
//!     results in the order `active_constraint_ids` returns them.

use reify_test_support::{MockOptimizedImpl, make_simple_engine, parse_and_compile};
use reify_types::Satisfaction;

// ── Test 1: register_optimized_impl stores & dispatches ─────────────────────

#[test]
fn engine_register_optimized_impl_stores_and_dispatches() {
    // A module using `@optimized("geo::coincident")` on a constraint def that
    // the language-level checker would evaluate as Satisfied (1.0 == 1.0). We
    // register a MockOptimizedImpl that returns Violated — so if the result
    // is Violated, the mock must have handled it (NOT the SimpleConstraintChecker).
    let source = r#"
@optimized("geo::coincident")
constraint def Coincident {
    param a: Real
    param b: Real
    a == b
}
structure def S {
    param x: Real = 1.0
    constraint Coincident(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);

    let mock = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    // Grab the call-tracking handle BEFORE the mock is moved into the box so
    // the test can directly assert dispatch rather than inferring it from
    // the returned Satisfaction.
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

    // Sanity check: the target is now registered.
    let targets: Vec<_> = engine.optimized_targets().collect();
    assert_eq!(targets, vec!["geo::coincident"]);

    let check_result = engine.check(&compiled);

    // The constraint should have been routed through the optimized impl,
    // so the result should be Violated (the mock's default), not Satisfied.
    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "expected the optimized impl to handle this constraint (Violated), \
         not the language-level checker (which would return Satisfied)"
    );

    // Direct dispatch assertion via the shared handle: exactly one call, and
    // the recorded ConstraintNodeId matches the compiled result's id.
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "expected MockOptimizedImpl to be invoked exactly once, got {:?}",
        recorded
    );
    assert_eq!(
        recorded[0], check_result.constraint_results[0].id,
        "the mock's recorded id should match the dispatched constraint id"
    );
}

// ── Test 2: unregistered target falls back to the checker ──────────────────

#[test]
fn unregistered_optimized_target_falls_back_to_checker() {
    // Same module as above, but the optimized impl is registered under a
    // DIFFERENT target ("other::target") so the constraint's "geo::coincident"
    // falls through to the language-level checker. The mock's handle lets us
    // additionally assert it was never invoked.
    let source = r#"
@optimized("geo::coincident")
constraint def Coincident {
    param a: Real
    param b: Real
    a == b
}
structure def S {
    param x: Real = 1.0
    constraint Coincident(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);

    let mock = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    // Registered under an unrelated target — the @optimized("geo::coincident")
    // constraint should still fall through to the language-level checker.
    engine.register_optimized_impl("other::target", Box::new(mock));

    let check_result = engine.check(&compiled);
    assert_eq!(check_result.constraint_results.len(), 1);
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "without a matching registered optimized impl the language-level \
         checker should handle the constraint (x == x is Satisfied)"
    );
    assert!(
        calls.lock().unwrap().is_empty(),
        "mock registered under a different target must not be invoked for \
         a constraint with @optimized(\"geo::coincident\"); recorded: {:?}",
        calls.lock().unwrap()
    );
}

// ── Test 3: mixed batch preserves order ─────────────────────────────────────

#[test]
fn mixed_optimized_and_fallback_batch_preserves_order() {
    // A structure with TWO active constraints on the same `x = 1.0`:
    //   1. `OptA(a: x, b: x)` — annotated `@optimized("target_a")`, registered
    //   2. `PlainEq(a: x, b: x)` — unannotated, handled by the language-level
    //      checker
    //
    // The language-level checker evaluates both predicates to Satisfied
    // (1.0 == 1.0). We register a mock for `target_a` that returns
    // `Violated` — so the optimized constraint will be Violated while the
    // unannotated one stays Satisfied. Both results must appear in the order
    // the constraints were declared in the structure.
    let source = r#"
@optimized("target_a")
constraint def OptA {
    param a: Real
    param b: Real
    a == b
}
constraint def PlainEq {
    param a: Real
    param b: Real
    a == b
}
structure def Mixed {
    param x: Real = 1.0
    constraint OptA(a: x, b: x)
    constraint PlainEq(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);

    let mock = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("target_a", Box::new(mock));

    let check_result = engine.check(&compiled);

    assert_eq!(
        check_result.constraint_results.len(),
        2,
        "expected two constraint results, got {:?}",
        check_result.constraint_results
    );

    // The optimized impl must have been invoked exactly once — for OptA only.
    // PlainEq should have gone to the language-level checker, so the mock's
    // recorded-calls list should have length 1.
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "expected optimized mock to be invoked exactly once (for OptA only), \
         got recorded calls: {:?}",
        recorded
    );

    // OptA must be first (Violated via the mock), PlainEq must be second
    // (Satisfied via the language-level checker). This asserts BOTH correct
    // dispatch AND preserved input order through `dispatch_constraints`.
    let first = &check_result.constraint_results[0];
    let second = &check_result.constraint_results[1];
    assert_eq!(
        first.id.entity, "Mixed",
        "first constraint should be on Mixed entity, got {:?}",
        first.id
    );
    assert_eq!(
        second.id.entity, "Mixed",
        "second constraint should be on Mixed entity, got {:?}",
        second.id
    );
    let first_label = first.label.as_deref().unwrap_or("");
    let second_label = second.label.as_deref().unwrap_or("");
    assert!(
        first_label.contains("OptA"),
        "first constraint should be OptA, got label={:?}",
        first.label,
    );
    assert!(
        second_label.contains("PlainEq"),
        "second constraint should be PlainEq, got label={:?}",
        second.label,
    );
    assert_eq!(
        first.satisfaction,
        Satisfaction::Violated,
        "OptA should be handled by the mock (Violated)"
    );
    assert_eq!(
        second.satisfaction,
        Satisfaction::Satisfied,
        "PlainEq should be handled by the language-level checker (Satisfied)"
    );
}

// ── Test 4: empty registry fast path — single @optimized constraint ──────────

#[test]
fn empty_registry_fast_path_returns_correct_results() {
    // A module with a single @optimized constraint and an empty registry.
    // With no registered impls the language-level checker must handle it.
    // This test validates the behavioral contract: the constraint should be
    // Satisfied (1.0 == 1.0) even though no optimized impl is registered.
    let source = r#"
@optimized("geo::coincident")
constraint def Coincident {
    param a: Real
    param b: Real
    a == b
}
structure def S {
    param x: Real = 1.0
    constraint Coincident(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);
    // Empty registry — no register_optimized_impl calls.
    let mut engine = make_simple_engine();

    let check_result = engine.check(&compiled);

    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "with empty registry the language-level checker must handle the constraint \
         (x == x is Satisfied)"
    );
}

// ── Test 4b: empty registry fast path — single Violated constraint ─────────

#[test]
fn empty_registry_fast_path_single_violated() {
    // Counterpart to Test 4: a single @optimized constraint that evaluates to
    // Violated (a != b where a=1.0, b=2.0). Confirms the fast path correctly
    // propagates a Violated result for a single entry.
    let source = r#"
@optimized("geo::coincident")
constraint def Coincident {
    param a: Real
    param b: Real
    a == b
}
structure def S {
    param x: Real = 1.0
    param y: Real = 2.0
    constraint Coincident(a: x, b: y)
}
"#;
    let compiled = parse_and_compile(source);
    let mut engine = make_simple_engine();

    let check_result = engine.check(&compiled);

    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "with empty registry the language-level checker must handle the constraint \
         (x == y where x=1.0, y=2.0 is Violated)"
    );
}

// ── Test 5: empty registry — multiple mixed constraints preserve order ────────

#[test]
fn empty_registry_multiple_constraints_preserves_order() {
    // Three constraints in a single structure: two @optimized-annotated (different
    // targets) and one plain. With empty optimization_registry ALL fall through to
    // the language-level checker. This test verifies:
    //   1. All constraints are evaluated (none silently dropped).
    //   2. Results appear in declaration order — first OptA, then OptB, then PlainEq.
    //   3. The language-level checker evaluates each predicate correctly.
    //
    // OptB intentionally evaluates to Violated (a > b with x=1.0, y=2.0) so the
    // ordering assertion depends on both label identity AND distinct satisfaction
    // values — a misordering cannot hide behind uniform Satisfied results.
    let source = r#"
@optimized("target_a")
constraint def OptA {
    param a: Real
    param b: Real
    a == b
}
@optimized("target_b")
constraint def OptB {
    param a: Real
    param b: Real
    a > b
}
constraint def PlainEq {
    param a: Real
    param b: Real
    a == b
}
structure def Multi {
    param x: Real = 1.0
    param y: Real = 2.0
    constraint OptA(a: x, b: x)
    constraint OptB(a: x, b: y)
    constraint PlainEq(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);
    // Empty registry — no register_optimized_impl calls.
    let mut engine = make_simple_engine();

    let check_result = engine.check(&compiled);

    assert_eq!(
        check_result.constraint_results.len(),
        3,
        "expected three constraint results, got {:?}",
        check_result.constraint_results
    );

    let r0 = &check_result.constraint_results[0];
    let r1 = &check_result.constraint_results[1];
    let r2 = &check_result.constraint_results[2];

    // Verify declaration order is preserved.
    let l0 = r0.label.as_deref().unwrap_or("");
    let l1 = r1.label.as_deref().unwrap_or("");
    let l2 = r2.label.as_deref().unwrap_or("");
    assert!(
        l0.contains("OptA"),
        "first result should be OptA, got label={:?}",
        r0.label
    );
    assert!(
        l1.contains("OptB"),
        "second result should be OptB, got label={:?}",
        r1.label
    );
    assert!(
        l2.contains("PlainEq"),
        "third result should be PlainEq, got label={:?}",
        r2.label
    );

    // Verify each predicate is evaluated correctly by the language-level checker.
    // The Satisfied/Violated mix means a misordering cannot hide behind uniform results.
    assert_eq!(
        r0.satisfaction,
        Satisfaction::Satisfied,
        "OptA: x == x (1.0 == 1.0) should be Satisfied"
    );
    assert_eq!(
        r1.satisfaction,
        Satisfaction::Violated,
        "OptB: x > y (1.0 > 2.0) should be Violated"
    );
    assert_eq!(
        r2.satisfaction,
        Satisfaction::Satisfied,
        "PlainEq: x == x (1.0 == 1.0) should be Satisfied"
    );
}
