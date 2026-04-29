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

use reify_compiler::auto_type_param::*;
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder};
use reify_types::{CompiledExpr, CompiledFunction, ConstraintNodeId, Satisfaction, Value};

// ─── step-1: empty input is a precondition violation (debug_assert!) ─────────

/// Passing an empty `candidates` slice to `filter_feasible_candidates` is a
/// caller bug per the function's documented precondition. Phase A's
/// [`CandidateEnumeration::Found`] arm guarantees ≥1 candidate, so in normal
/// usage this precondition is always satisfied. The `debug_assert!` exists to
/// catch bypass-Phase-A misuse (e.g., wiring a hand-constructed empty slice
/// directly to Phase B).
///
/// The `#[cfg(debug_assertions)]` gate skips this test in release builds where
/// `debug_assert!` is a no-op, avoiding spurious test failures in optimized
/// profiles. In debug builds (the default for `cargo test`), the assert fires
/// and the `#[should_panic(expected = ...)]` attribute pins the exact message
/// substring so that any future weakening or removal of the assert fails loudly.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "filter_feasible_candidates: candidates slice must be non-empty")]
fn filter_panics_on_empty_candidates_input() {
    let template = TopologyTemplateBuilder::new("Bearing").build();
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let _ = filter_feasible_candidates(&[], &template, &checker, functions);
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

// ─── step-9: Indeterminate is feasible (architecture §2.5) ────────────────

/// Architecture §2.5 monotonic-feasible: "treat undef constraints as
/// feasible" — `Indeterminate` must NOT trigger rejection.
///
/// This test pins the `!= Violated` predicate specifically: a regression
/// that flipped the predicate to `== Satisfied` (excluding `Indeterminate`)
/// would cause this test to reject the candidate, failing loudly.
///
/// PRD §"Phase B": "If `Satisfaction::Indeterminate`, the candidate is
/// considered feasible (undef does not falsify)."
#[test]
fn filter_treats_indeterminate_as_feasible_per_arch_2_5() {
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Indeterminate: every constraint result is Indeterminate.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);
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
        "Indeterminate result must be treated as feasible (arch §2.5); candidate must be accepted"
    );
}

// ─── step-11: only Violated ids are recorded, not Indeterminate ──────────

/// When a template has two constraints and constraint 0 is Violated while
/// constraint 1 is Indeterminate, only id 0 must appear in
/// `RejectedCandidate::violated_constraints` — id 1 must NOT appear
/// (Indeterminate does not falsify).
///
/// Pins the "only-Violated-ids" contract from the design decision.
/// A regression that recorded all non-Satisfied ids (including Indeterminate)
/// would fail this test by including id 1.
#[test]
fn filter_only_violated_constraints_are_recorded_in_rejection() {
    let cnid_0 = ConstraintNodeId::new("Bearing", 0);
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr.clone())
        // Constraint index 1 exists in the template but returns Indeterminate —
        // it must NOT appear in violated_constraints.
        .constraint("Bearing", 1, None, expr)
        .build();
    // Constraint 0: Violated; constraint 1: Indeterminate (the default).
    let checker = MockConstraintChecker::new()
        .with_default(Satisfaction::Indeterminate)
        .with_result(cnid_0.clone(), Satisfaction::Violated);
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
                // Only id 0 (Violated); id 1 (Indeterminate) must NOT appear.
                violated_constraints: vec![cnid_0],
            }],
        },
        "only Violated constraint ids must be recorded; Indeterminate id must not appear"
    );
}

// ─── step-13: input order is preserved (not re-sorted by Phase B) ─────────

/// Phase B must preserve the input order of candidates in both `accepted`
/// and `rejected`. Phase A supplies candidates in alphabetical order but
/// Phase B must not re-sort them — it trusts Phase A's guarantee and
/// iterates in input order.
///
/// This test supplies a deliberately unsorted slice ["Charlie", "Alpha", "Bravo"]
/// (as if a caller bypassed Phase A's sort) and asserts that Phase B
/// preserves the order verbatim rather than re-sorting alphabetically.
///
/// Practical implication: when Phase A feeds Phase B in alphabetical order,
/// the output vecs are also alphabetical — the invariant threads through
/// both phases.
#[test]
fn filter_preserves_input_order_in_both_accepted_and_rejected() {
    // No constraints → all candidates are vacuously accepted.
    let template = TopologyTemplateBuilder::new("T").build();
    let checker = MockConstraintChecker::new(); // default Satisfied
    let functions: &[CompiledFunction] = &[];

    let candidates = vec![
        "Charlie".to_string(),
        "Alpha".to_string(),
        "Bravo".to_string(),
    ];
    let result = filter_feasible_candidates(&candidates, &template, &checker, functions);

    assert_eq!(
        result,
        FeasibilityResult::Feasible {
            accepted: vec![
                "Charlie".to_string(),
                "Alpha".to_string(),
                "Bravo".to_string(),
            ],
            rejected: vec![],
        },
        "Phase B must NOT re-sort candidates; input order must be preserved verbatim"
    );
}

// ─── step-15: all candidates rejected preserves order in rejected vec ─────

/// Realistic multi-candidate scenario: all three candidates are rejected
/// (default-Violated mock) and the `rejected` Vec preserves input order
/// ["A", "B", "C"]. Pins that:
/// 1. Every candidate is processed (no short-circuit after first rejection).
/// 2. The `rejected` Vec preserves input alphabetical order.
/// 3. The constraint violated id appears in each RejectedCandidate.
#[test]
fn filter_partitions_mixed_candidates_into_accepted_and_rejected() {
    let cnid = ConstraintNodeId::new("T", 0);
    let expr = CompiledExpr::literal(Value::Bool(true), reify_types::Type::Bool);
    let template = TopologyTemplateBuilder::new("T")
        .constraint("T", 0, None, expr)
        .build();
    // Default Violated: all candidates are rejected.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let candidates = vec!["A".to_string(), "B".to_string(), "C".to_string()];
    let result = filter_feasible_candidates(&candidates, &template, &checker, functions);

    assert_eq!(
        result,
        FeasibilityResult::Empty {
            rejected: vec![
                RejectedCandidate {
                    name: "A".to_string(),
                    violated_constraints: vec![cnid.clone()],
                },
                RejectedCandidate {
                    name: "B".to_string(),
                    violated_constraints: vec![cnid.clone()],
                },
                RejectedCandidate {
                    name: "C".to_string(),
                    violated_constraints: vec![cnid.clone()],
                },
            ],
        },
        "all candidates rejected: rejected vec must contain all three in input order"
    );
}
