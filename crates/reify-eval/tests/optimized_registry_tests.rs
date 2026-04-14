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
