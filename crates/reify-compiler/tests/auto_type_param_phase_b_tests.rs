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

use std::sync::atomic::{AtomicUsize, Ordering};

use reify_compiler::auto_type_param::*;
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder};
use reify_core::ConstraintNodeId;
use reify_ir::{CompiledExpr, CompiledFunction, ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction, Value};

/// Stateful mock whose verdict is keyed by *call number*, not by
/// `ConstraintNodeId`. Used to detect single-broadcast regressions in
/// `filter_feasible_candidates` — see test below.
struct StatefulMockConstraintChecker {
    /// Pre-set verdicts, one per expected `check()` invocation (indexed by call
    /// number). `check()` panics if the call count exceeds the vec length,
    /// which catches "too few calls" just as well as "too many calls".
    results: Vec<Satisfaction>,
    call_count: AtomicUsize,
}

impl StatefulMockConstraintChecker {
    fn new(results: Vec<Satisfaction>) -> Self {
        Self {
            results,
            call_count: AtomicUsize::new(0),
        }
    }

    /// Return the number of times `check()` has been invoked so far.
    fn calls(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl ConstraintChecker for StatefulMockConstraintChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);
        // Intentional panic on out-of-bounds: the test pre-sets exactly as many
        // verdicts as expected calls; more calls than expected is a bug.
        let satisfaction = self.results[n];
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }
}

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
#[should_panic(expected = "candidates slice must be non-empty")]
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

    let result =
        filter_feasible_candidates(&["ORingSeal".to_string()], &template, &checker, functions);

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
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Satisfied: every constraint result is Satisfied.
    let checker = MockConstraintChecker::new(); // default is Satisfied
    let functions: &[CompiledFunction] = &[];

    let result =
        filter_feasible_candidates(&["ORingSeal".to_string()], &template, &checker, functions);

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
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Violated: constraint 0 will be Violated.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Violated);
    let functions: &[CompiledFunction] = &[];

    let result =
        filter_feasible_candidates(&["ORingSeal".to_string()], &template, &checker, functions);

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
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();
    // Default Indeterminate: every constraint result is Indeterminate.
    let checker = MockConstraintChecker::new().with_default(Satisfaction::Indeterminate);
    let functions: &[CompiledFunction] = &[];

    let result =
        filter_feasible_candidates(&["ORingSeal".to_string()], &template, &checker, functions);

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
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
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

    let result =
        filter_feasible_candidates(&["ORingSeal".to_string()], &template, &checker, functions);

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
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
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

// ─── step-17: checker is invoked independently per candidate (not broadcast) ─

/// `filter_feasible_candidates` must invoke `constraint_checker.check()` once
/// per candidate, not once for all candidates.
///
/// **Why existing Phase B tests cannot distinguish per-candidate from broadcast:**
/// All earlier tests use `MockConstraintChecker`, which returns a verdict keyed
/// on `ConstraintNodeId`. In Phase B, every candidate receives a byte-identical
/// `ConstraintInput` (same constraint list, same empty `ValueMap`), so a
/// hypothetical regression that replaced the `for candidate in candidates` loop
/// with a single broadcast call — `let r = checker.check(&input); for c in
/// candidates { use r }` — would produce identical results and pass every
/// existing test. The per-candidate contract is therefore unobservable under
/// the current fixture.
///
/// **Asymmetric load-bearing assertion:**
/// This test uses `StatefulMockConstraintChecker`, whose verdict is keyed by
/// *call number* rather than `ConstraintNodeId`:
/// - Call #1 → `Violated` → candidate "A" goes to `rejected`.
/// - Call #2 → `Satisfied` → candidate "B" goes to `accepted`.
///
/// A single-broadcast regression would call `check()` only once. Both
/// candidates would then receive `Violated`, yielding
/// `Empty { rejected: [A, B] }` instead of the expected
/// `Feasible { accepted: [B], rejected: [A] }`. The asymmetric partition is
/// what makes the regression observable.
///
/// The `checker.calls() == 2` assertion additionally pins the exact invocation
/// count: it would catch a regression that skips one candidate entirely.
///
/// **Phase C preparation:** When the upcoming type-substitution pass lands
/// (substituting `Type::TypeParam(T)` → `Type::StructureRef(candidate)`), each
/// candidate's `ConstraintInput` will differ in its `ValueMap`. The
/// `StatefulMockConstraintChecker` primitive developed here will be useful for
/// future Phase C regression tests, at which point promotion to
/// `reify_test_support::mocks` is justified.
#[test]
fn filter_invokes_checker_independently_per_candidate() {
    let cnid = ConstraintNodeId::new("Bearing", 0);
    let expr = CompiledExpr::literal(Value::Bool(true), reify_core::Type::Bool);
    let template = TopologyTemplateBuilder::new("Bearing")
        .constraint("Bearing", 0, None, expr)
        .build();

    // Call #1 → Violated (candidate "A" rejected)
    // Call #2 → Satisfied (candidate "B" accepted)
    let checker =
        StatefulMockConstraintChecker::new(vec![Satisfaction::Violated, Satisfaction::Satisfied]);
    let functions: &[CompiledFunction] = &[];

    let result = filter_feasible_candidates(
        &["A".to_string(), "B".to_string()],
        &template,
        &checker,
        functions,
    );

    // The asymmetric partition is the load-bearing assertion: a single-broadcast
    // regression would yield Empty { rejected: [A, B] }.
    assert_eq!(
        result,
        FeasibilityResult::Feasible {
            accepted: vec!["B".to_string()],
            rejected: vec![RejectedCandidate {
                name: "A".to_string(),
                violated_constraints: vec![cnid],
            }],
        },
        "per-candidate check: A must be rejected (call #1=Violated), B accepted (call #2=Satisfied)"
    );

    // Explicitly pin the invocation count: exactly 2 calls, one per candidate.
    assert_eq!(
        checker.calls(),
        2,
        "check() must be called exactly once per candidate (2 candidates → 2 calls)"
    );
}
