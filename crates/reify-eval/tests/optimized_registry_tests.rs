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

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockOptimizedImpl, parse_and_compile};
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
    let compiled = parse_and_compile(&source);

    let mock = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

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
}

// ── Test 2: unregistered target falls back to the checker ──────────────────

#[test]
fn unregistered_optimized_target_falls_back_to_checker() {
    // Same module as above, but the optimized impl is NOT registered. The
    // language-level `SimpleConstraintChecker` should handle the constraint
    // and return Satisfied (1.0 == 1.0).
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
    let compiled = parse_and_compile(&source);

    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    // No optimized impl registered — falls back.

    let check_result = engine.check(&compiled);
    assert_eq!(check_result.constraint_results.len(), 1);
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "without a registered optimized impl the language-level checker \
         should handle the constraint (x == x is Satisfied)"
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
    let compiled = parse_and_compile(&source);

    let mock = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.register_optimized_impl("target_a", Box::new(mock));

    let check_result = engine.check(&compiled);

    assert_eq!(
        check_result.constraint_results.len(),
        2,
        "expected two constraint results, got {:?}",
        check_result.constraint_results
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
