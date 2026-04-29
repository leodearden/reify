//! Phase B tests for `auto` type-parameter resolution per-candidate feasibility filter.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `filter_feasible_candidates` function and its two-arm result enum
//! [`FeasibilityResult`], plus the [`RejectedCandidate`] record type.
//! The PRD that drives this work is
//! `docs/prds/auto-type-param-resolution.md` and language spec §3.9 (lines 500-512).
//!
//! Phase B takes the candidate names produced by Phase A's [`enumerate_candidates`]
//! (a `&[String]` slice) and runs the value-auto solver's constraint feasibility
//! primitives on the parameterized definition's constraints, returning the subset
//! that does not provably falsify any constraint.
//!
//! # Feasibility predicate
//!
//! Architecture §2.5 monotonic-feasible rule: `feasible(c) ≡ satisfaction != Violated`.
//! Both `Satisfied` and `Indeterminate` count as feasible; only `Violated` causes
//! rejection. This is the "treat undef as feasible" rule from PRD §"Phase B".
//!
//! # Scope
//!
//! Phase B checks only the template's top-level (unguarded) constraints.
//! Guarded-group constraints are NOT collected here (that lives in `reify-eval`).
//! No type-substitution mechanics: with an empty `ValueMap`, the candidate name
//! does not yet vary constraint outcomes. Phase C (selection), D (topology trigger)
//! are out of scope here and live in follow-up tasks.
//!
//! # Test approach
//!
//! Tests use `MockConstraintChecker` (from `reify_test_support`) to drive
//! per-`ConstraintNodeId` satisfaction outcomes without spinning up the full
//! `SimpleConstraintChecker`. Templates are built via `TopologyTemplateBuilder`
//! with literal constraint expressions (the mock ignores expr content).

#![allow(unused_imports)]

use reify_compiler::auto_type_param::*;
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder};
use reify_types::{
    CompiledExpr, CompiledFunction, ConstraintNodeId, Satisfaction, Type, Value,
};

// ─── step-1: empty input returns FeasibilityResult::Empty ─────────────────

/// When `candidates` is empty, `filter_feasible_candidates` must return
/// `FeasibilityResult::Empty { rejected: vec![] }` without invoking the
/// constraint checker at all.
///
/// The `MockConstraintChecker` is configured with `Satisfaction::Violated` as
/// default — if the loop body were accidentally called for zero iterations,
/// any real invocation would surface via the checker's per-id routing and
/// would produce a non-empty rejected list. Using the Violated default makes
/// accidental invocations observable even without explicit call-count tracking.
#[test]
fn filter_returns_empty_for_empty_candidates_input() {
    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let result = filter_feasible_candidates(&[], &template, &checker, functions);

    assert!(
        matches!(result, FeasibilityResult::Empty { ref rejected } if rejected.is_empty()),
        "expected FeasibilityResult::Empty {{ rejected: [] }} for empty candidates input, got: {:?}",
        result
    );
}

// ─── step-3: no constraints → vacuous feasibility ─────────────────────────

/// When the parameterized template has zero top-level constraints, every
/// candidate is vacuously feasible (the per-candidate constraint loop body
/// produces zero results, so there is nothing to Violate). This test also
/// checks that the `MockConstraintChecker::with_default(Violated)` is truly
/// irrelevant when no constraints exist — the default would surface any
/// accidental invocation but zero constraints mean zero check calls.
#[test]
fn filter_accepts_single_candidate_when_template_has_no_constraints() {
    // No .constraint(...) calls → template has zero top-level constraints.
    let template = TopologyTemplateBuilder::new("Bearing").build();
    // Default Violated: if the checker were invoked for a non-existent
    // constraint, the logic would produce a non-empty violated list, making
    // this test fail. Vacuous feasibility must not depend on checker behavior.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let result = filter_feasible_candidates(
        &["ORingSeal".to_string()],
        &template,
        &checker,
        functions,
    );

    assert_eq!(
        result,
        FeasibilityResult::Feasible {
            accepted: vec!["ORingSeal".to_string()],
            rejected: vec![],
        },
        "zero constraints → vacuously feasible; expected single accepted candidate"
    );
}

// ─── step-5: all-Satisfied → accepted ────────────────────────────────────

/// When all constraints return `Satisfied`, the candidate passes the
/// feasibility filter. This pins the all-Satisfied path through the
/// `!= Violated` predicate: a constraint whose result is Satisfied must
/// never appear in `violated_constraints`.
///
/// Uses a boolean-typed literal expression (`Value::Bool(true)`) as the
/// constraint expression. The mock ignores the expression content entirely;
/// it's only there so `TopologyTemplateBuilder::constraint` has something
/// to store in the `CompiledConstraint::expr` field.
#[test]
fn filter_accepts_candidate_when_all_constraints_satisfied() {
    let cnid = ConstraintNodeId::new("Bearing", 0);
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Satisfied: every constraint result is Satisfied.
    let checker = MockConstraintChecker::new(); // default is Satisfied
    let functions: &[CompiledFunction] = &[];

    let result = filter_feasible_candidates(
        &["ORingSeal".to_string()],
        &template,
        &checker,
        functions,
    );

    assert_eq!(
        result,
        FeasibilityResult::Feasible {
            accepted: vec!["ORingSeal".to_string()],
            rejected: vec![],
        },
        "all-Satisfied constraints → candidate accepted; got: {:?}",
        result
    );
    let _ = cnid; // Used to document which constraint is in the template.
}

// ─── step-7: any-Violated → rejected with violated ids ────────────────────

/// When any constraint returns `Violated`, the candidate is rejected and
/// the violated constraint node id is recorded in
/// `RejectedCandidate::violated_constraints`.
///
/// Pins BOTH the rejection arm AND the specific content of the
/// `violated_constraints` field. A regression that (a) accepted a Violated
/// candidate or (b) recorded the wrong constraint id would fail loudly.
#[test]
fn filter_rejects_candidate_when_any_constraint_violated() {
    let cnid = ConstraintNodeId::new("Bearing", 0);
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Violated: constraint 0 will be Violated.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let result = filter_feasible_candidates(
        &["ORingSeal".to_string()],
        &template,
        &checker,
        functions,
    );

    assert_eq!(
        result,
        FeasibilityResult::Empty {
            rejected: vec![RejectedCandidate {
                name: "ORingSeal".to_string(),
                violated_constraints: vec![cnid],
            }],
        },
        "Violated constraint → candidate rejected with the violated constraint id recorded"
    );
}
