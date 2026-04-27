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

use reify_test_support::{
    BrokenCountOptimizedImpl, MockOptimizedImpl, make_simple_engine, parse_and_compile,
};
use reify_types::{
    ConstraintDiagnostics, ConstraintNodeId, ConstraintResult, Diagnostic, Satisfaction, Severity,
};

/// Assert that `diagnostics` contains an `Error`-severity diagnostic describing
/// the OptimizedImpl-count-mismatch fallback for `expected_target`. Used by tests
/// 6, 8, 9, and 10 to DRY up the identical inline block that searches for a
/// diagnostic whose message contains "OptimizedImpl", "falling back", and the
/// registered target name.
fn assert_has_fallback_diagnostic(diagnostics: &[Diagnostic], expected_target: &str) {
    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic for the broken OptimizedImpl, got diagnostics: {:?}",
        diagnostics
    );
    let violation_diag = error_diags.iter().find(|d| {
        d.message.contains("OptimizedImpl")
            && d.message.contains("falling back")
            && d.message.contains(&format!("target {:?}", expected_target))
    });
    assert!(
        violation_diag.is_some(),
        "expected a diagnostic mentioning 'OptimizedImpl', 'falling back', and 'target {:?}', got error diagnostics: {:?}",
        expected_target,
        error_diags
    );
}

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

// ── Test 6: BrokenCountOptimizedImpl (returns 0 results) triggers fallback ───

#[test]
fn optimized_impl_wrong_result_count_falls_back_with_diagnostic() {
    // A module with a single @optimized("geo::coincident") constraint where
    // a == b (1.0 == 1.0). We register a BrokenCountOptimizedImpl that returns
    // an empty Vec, triggering the count mismatch.
    //
    // Expected behavior after Task 1657 impl:
    //   (a) no panic
    //   (b) check_result.diagnostics contains a Diagnostic with Severity::Error
    //       whose message contains "OptimizedImpl" and "falling back"
    //   (c) constraint_results has exactly 1 entry with Satisfaction::Satisfied
    //       (from the language-level fallback evaluating 1.0 == 1.0)
    //
    // Currently: panics at assert_eq! in dispatch_constraints.
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

    let mock = BrokenCountOptimizedImpl::new(vec![]); // wrong count: 0 results for 1 constraint
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

    let check_result = engine.check(&compiled);

    // (c) fallback evaluation: x == x is Satisfied
    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result from fallback, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "fallback checker must evaluate x == x as Satisfied"
    );

    // (b) diagnostic error for the contract violation
    assert_has_fallback_diagnostic(&check_result.diagnostics, "geo::coincident");

    // Broken impl was still invoked (before fallback kicked in)
    assert!(
        !calls.lock().unwrap().is_empty(),
        "the broken impl must have been called before the fallback"
    );
}

// ── Test 7: mixed batch — one broken impl, one working impl, one plain ────────

#[test]
fn mixed_batch_one_broken_optimized_impl_falls_back_correctly() {
    // Three constraints in one structure:
    //   OptA(@optimized("target_a"), a == b, a=b=1.0) — BrokenCountOptimizedImpl
    //      returns empty Vec → fallback → Satisfied
    //   OptB(@optimized("target_b"), a == b, a=b=1.0) — MockOptimizedImpl
    //      returns Violated → Violated
    //   PlainEq(no annotation, a == b, a=b=1.0) — language-level checker → Satisfied
    //
    // Assertions:
    //   (a) diagnostic error for target_a only (broken impl)
    //   (b) OptA gets Satisfaction::Satisfied (fallback evaluated 1.0 == 1.0)
    //   (c) OptB gets Satisfaction::Violated (working mock)
    //   (d) PlainEq gets Satisfaction::Satisfied (language-level checker)
    //   (e) order is preserved: OptA first, OptB second, PlainEq third
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
    constraint OptB(a: x, b: x)
    constraint PlainEq(a: x, b: x)
}
"#;
    let compiled = parse_and_compile(source);

    // Broken impl for target_a: returns empty Vec (0 results for 1 constraint)
    let broken = BrokenCountOptimizedImpl::new(vec![]);
    let broken_calls = broken.calls_handle();

    // Working mock for target_b: returns Violated
    let working = MockOptimizedImpl::new().with_default(Satisfaction::Violated);
    let working_calls = working.calls_handle();

    let mut engine = make_simple_engine();
    engine.register_optimized_impl("target_a", Box::new(broken));
    engine.register_optimized_impl("target_b", Box::new(working));

    let check_result = engine.check(&compiled);

    // (e) Three results in declaration order
    assert_eq!(
        check_result.constraint_results.len(),
        3,
        "expected three constraint results, got {:?}",
        check_result.constraint_results
    );

    let r0 = &check_result.constraint_results[0];
    let r1 = &check_result.constraint_results[1];
    let r2 = &check_result.constraint_results[2];

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

    // (b) OptA: broken impl fell back, language-level checker sees x == x → Satisfied
    assert_eq!(
        r0.satisfaction,
        Satisfaction::Satisfied,
        "OptA should be Satisfied via fallback"
    );
    // (c) OptB: working mock returns Violated
    assert_eq!(
        r1.satisfaction,
        Satisfaction::Violated,
        "OptB should be Violated via working mock"
    );
    // (d) PlainEq: language-level checker → Satisfied
    assert_eq!(
        r2.satisfaction,
        Satisfaction::Satisfied,
        "PlainEq should be Satisfied via language-level checker"
    );

    // (a) Exactly one Error diagnostic for target_a's contract violation
    let error_diags: Vec<_> = check_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        error_diags.len(),
        1,
        "expected exactly one Error diagnostic (for target_a), got: {:?}",
        error_diags
    );
    let diag = &error_diags[0];
    assert!(
        diag.message.contains("target_a"),
        "diagnostic should mention target_a, got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("falling back"),
        "diagnostic should mention falling back, got: {:?}",
        diag.message
    );

    // Both impls were invoked
    assert!(
        !broken_calls.lock().unwrap().is_empty(),
        "broken impl must have been invoked before fallback"
    );
    assert!(
        !working_calls.lock().unwrap().is_empty(),
        "working mock must have been invoked for OptB"
    );
}

// ── Test 8: BrokenCountOptimizedImpl returns TOO MANY results (count > expected)

#[test]
fn optimized_impl_wrong_count_nonzero_still_falls_back() {
    // A module with a single @optimized("geo::coincident") constraint (a == b,
    // 1.0 == 1.0). We register a BrokenCountOptimizedImpl that returns 2 results
    // for 1 constraint — testing the count > expected case (not just empty).
    //
    // Expected behavior:
    //   (a) no panic
    //   (b) Error diagnostic mentioning the target and falling back
    //   (c) constraint_results has exactly 1 entry with Satisfaction::Satisfied
    //       (fallback language-level checker evaluating 1.0 == 1.0)
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

    // Two dummy results for one constraint — wrong count in the other direction
    let dummy_id = ConstraintNodeId::new("dummy", 0);
    let fixed_results = vec![
        ConstraintResult {
            id: dummy_id.clone(),
            satisfaction: Satisfaction::Violated,
            diagnostics: ConstraintDiagnostics::default(),
        },
        ConstraintResult {
            id: dummy_id.clone(),
            satisfaction: Satisfaction::Violated,
            diagnostics: ConstraintDiagnostics::default(),
        },
    ];
    let mock = BrokenCountOptimizedImpl::new(fixed_results); // 2 results for 1 constraint
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

    let check_result = engine.check(&compiled);

    // (c) fallback: x == x is Satisfied
    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result from fallback, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "fallback checker must evaluate x == x as Satisfied"
    );

    // (b) Error diagnostic for the contract violation
    assert_has_fallback_diagnostic(&check_result.diagnostics, "geo::coincident");

    // Broken impl was still invoked
    assert!(
        !calls.lock().unwrap().is_empty(),
        "the broken impl must have been called before the fallback"
    );
}

// ── Test 9: fallback evaluates a constraint as Violated (not just Satisfied) ─

#[test]
fn optimized_impl_fallback_evaluates_violated_correctly() {
    // A module with a single @optimized("geo::coincident") constraint where
    // a != b (1.0 != 2.0). We register a BrokenCountOptimizedImpl that returns
    // an empty Vec, triggering the fallback path.
    //
    // This test verifies that the fallback faithfully reflects constraint
    // semantics — not just that it avoids crashing. The language-level checker
    // must evaluate 1.0 == 2.0 as Violated.
    //
    // Expected behavior:
    //   (a) no panic
    //   (b) Error diagnostic mentioning "OptimizedImpl" and "falling back"
    //   (c) constraint_results has exactly 1 entry with Satisfaction::Violated
    //       (fallback checker evaluating 1.0 != 2.0)
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

    let mock = BrokenCountOptimizedImpl::new(vec![]); // wrong count: 0 results for 1 constraint
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

    let check_result = engine.check(&compiled);

    // (c) fallback evaluation: x != y so constraint is Violated
    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "expected exactly one constraint result from fallback, got {:?}",
        check_result.constraint_results
    );
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "fallback checker must evaluate 1.0 == 2.0 as Violated"
    );

    // (b) Error diagnostic for the contract violation is still emitted
    assert_has_fallback_diagnostic(&check_result.diagnostics, "geo::coincident");

    // Broken impl was still invoked (before fallback kicked in)
    assert!(
        !calls.lock().unwrap().is_empty(),
        "the broken impl must have been called before the fallback"
    );
}

// ── Test 10: batch fallback — multiple constraints same target ───────────────

#[test]
fn optimized_impl_batch_fallback_two_constraints_both_get_fallback_results() {
    // A structure with TWO @optimized("geo::coincident") constraints routed to
    // a single BrokenCountOptimizedImpl that returns exactly ONE result for the
    // 2-constraint input.
    //
    // Code trace:
    //   Both constraints bucket under optimized_groups["geo::coincident"] with
    //   indices [0, 1]. imp.check() returns 1 result → count mismatch →
    //   Diagnostic::error emitted → fallback calls constraint_checker.check()
    //   with both constraints → zip with indices → results[0] and results[1].
    //
    // This is a regression-lock test: the dispatcher already handles this case.
    // Assertions verify:
    //   (c) constraint_results.len() == 2 (entire batch fell back)
    //   (d) slot 0 = Satisfied (x==x, 1.0==1.0), slot 1 = Violated (x==y, 1.0!=2.0)
    //       — distinct values so a swapped orig_idx cannot hide behind uniform results
    //   (f) Error diagnostic mentioning "OptimizedImpl" and "falling back"
    //   (g) broken impl was invoked before fallback ran
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
    constraint Coincident(a: x, b: x)
    constraint Coincident(a: x, b: y)
}
"#;
    let compiled = parse_and_compile(source);

    // 1 result for 2 constraints — triggers the count-mismatch fallback
    let dummy_id = ConstraintNodeId::new("dummy", 0);
    let fixed_results = vec![ConstraintResult {
        id: dummy_id.clone(),
        satisfaction: Satisfaction::Violated,
        diagnostics: ConstraintDiagnostics::default(),
    }];
    let mock = BrokenCountOptimizedImpl::new(fixed_results);
    let calls = mock.calls_handle();
    let mut engine = make_simple_engine();
    engine.register_optimized_impl("geo::coincident", Box::new(mock));

    let check_result = engine.check(&compiled);

    // (c) both constraints fell back → 2 results in total
    assert_eq!(
        check_result.constraint_results.len(),
        2,
        "expected two constraint results from batch fallback, got {:?}",
        check_result.constraint_results
    );

    // (d) per-slot fallback correctness AND orig_idx ordering end-to-end
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "first constraint (Coincident(a: x, b: x), 1.0 == 1.0) must be Satisfied via fallback"
    );
    assert_eq!(
        check_result.constraint_results[1].satisfaction,
        Satisfaction::Violated,
        "second constraint (Coincident(a: x, b: y), 1.0 == 2.0) must be Violated via fallback"
    );

    // (d-extended) id-based ordering lock: catches any orig_idx swap even if satisfaction
    // values happen to match in a future refactor — verifies declaration order is preserved
    // end-to-end through dispatch_constraints.
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template S must exist in the compiled module");
    assert_eq!(
        check_result.constraint_results[0].id, s_template.constraints[0].id,
        "result[0].id must match the first declared constraint (declaration order preserved)"
    );
    assert_eq!(
        check_result.constraint_results[1].id, s_template.constraints[1].id,
        "result[1].id must match the second declared constraint (declaration order preserved)"
    );

    // (f) fallback diagnostic via the new helper
    assert_has_fallback_diagnostic(&check_result.diagnostics, "geo::coincident");

    // (g) broken impl ran before fallback kicked in
    assert!(
        !calls.lock().unwrap().is_empty(),
        "the broken impl must have been called before the fallback"
    );
}
