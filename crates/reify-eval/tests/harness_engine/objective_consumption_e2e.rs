//! Task 5417 (DIC γ) — `E_OBJECTIVE_UNCONSUMED`, the runtime half.
//!
//! PRD `docs/prds/v0_6/declared-intent-consumption-accounting.md` §3/§4.2.
//!
//! The compile half (`E_OBJECTIVE_INERT`) rejects an objective that provably
//! governs nothing. This half covers the case the compiler cannot see: an
//! objective that IS well-posed — it reaches a real solver variable — but that
//! the solver then silently discards, because the decomposition built no
//! component for it to attach to. `docs/prds/v0_6/fixtures/dic_min_unconstrained.ri`
//! is the canonical probe: `param a = auto(free)` + `minimize (a-3)*(a-3)` and
//! no constraints. Baseline before this rule: `a = undef (awaiting solve)` and
//! the objective is never mentioned — the declaration evaporates in silence,
//! which is the INV-SF-3 failure the PRD exists to eradicate.
//!
//! The rule must be quiet everywhere else. The negative cases below pin the
//! three ways it could go wrong: an objective the solver DID consume (O1), one
//! whose reachable autos are all concretely bound this run (O2, the
//! vacuous-healthy rule), and the synthesised Chebyshev-centre objective a
//! scope never declared (task 4013's exemption).
//!
//! Assertions target `DiagnosticCode`, never message substrings (INV-SF-6), and
//! the sources are byte-mirrors of the committed PRD fixtures so the tests track
//! the same user-observable signal the PRD measured.
//!
//! Written RED in step-9: nothing emits `ObjectiveUnconsumed` until step-10.
//!
//! **Placement note for the implementer.** This module is the arbiter of WHERE
//! the emission goes. The gate belongs beside the #4804
//! `W_SOLVER_OPTIMALITY_UNPROVEN` site on the single-scope objective path; if
//! `eval()` turns out to skip that path entirely for a zero-constraint scope,
//! the emission must move to whichever objective-path site actually executes for
//! the `dic_min_unconstrained` fixture, and the reason recorded there.

use reify_constraints::SolverRegistry;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Compile `source` (asserting it is compile-clean) and evaluate it with the
/// production [`SolverRegistry`] — the same one `reify-cli` wires
/// (`main.rs`'s `.with_solver(Box::new(SolverRegistry::production()))`) — so the
/// objective path these tests describe is the one users actually hit. A bare
/// `DimensionalSolver` is NOT equivalent: it solves the unconstrained fixture
/// outright and leaves an `==`-pinned auto undef, so both would describe an
/// engine no user runs.
///
/// The compile-clean assertion is load-bearing: every fixture here must pass the
/// compile half untouched, or a `ObjectiveUnconsumed`-shaped hole could be
/// masked by an `ObjectiveInert` Error raised earlier on the same source.
fn eval_with_solver(source: &str) -> reify_eval::EvalResult {
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "fixture must be compile-clean so the runtime rule is what is under \
         test; got {compile_errors:?}"
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(SolverRegistry::production()));
    engine.eval(&compiled)
}

/// Every `ObjectiveUnconsumed` diagnostic in an eval result.
fn unconsumed(result: &reify_eval::EvalResult) -> Vec<&Diagnostic> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ObjectiveUnconsumed))
        .collect()
}

/// Assert exactly one `ObjectiveUnconsumed` Error, that it names `cell`, and
/// that it is renderable.
///
/// "Exactly one" is the #5014 aggregation rule: one diagnostic per objective
/// *declaration* naming the FULL unconsumed set — never one per component, per
/// trial, or per auto.
fn assert_one_unconsumed_error_naming(result: &reify_eval::EvalResult, cell: &str) {
    let found = unconsumed(result);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one ObjectiveUnconsumed (the #5014 aggregation rule); \
         got {found:?}"
    );
    let diag = found[0];
    assert_eq!(
        diag.severity,
        Severity::Error,
        "E_* mnemonics are Errors by house convention; got {:?}",
        diag.severity
    );
    assert!(
        diag.message.contains("E_OBJECTIVE_UNCONSUMED"),
        "the message must carry the PRD-prose mnemonic so the CLI renders it; \
         got {:?}",
        diag.message
    );
    assert!(
        diag.message.contains(cell),
        "the message must name the unconsumed auto `{cell}`; got {:?}",
        diag.message
    );
}

/// Assert the run reports no unconsumed objective at all.
fn assert_no_unconsumed(result: &reify_eval::EvalResult, why: &str) {
    let found = unconsumed(result);
    assert!(found.is_empty(), "{why}; got {found:?}");
}

/// Anti-vacuity guard for the negative cases: the fixture must really have
/// reached the solver and bound the auto.
///
/// Without this a negative case degenerates into "nothing solved, so of course
/// nothing was reported" — it would keep passing even if the rule were rewritten
/// to fire on everything.
fn assert_bound(result: &reify_eval::EvalResult, entity: &str, member: &str) {
    let id = ValueCellId::new(entity, member);
    let got = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("cell {entity}.{member} is absent from the values map"));
    assert!(
        !matches!(got, reify_ir::Value::Undef),
        "cell {entity}.{member} must be concretely bound for this negative case \
         to mean anything; got {got:?}"
    );
}

// ── (B7) the target: a well-posed objective the solver drops ────────────────

/// Byte-mirror of `docs/prds/v0_6/fixtures/dic_min_unconstrained.ri`.
///
/// `minimize (a-3)*(a-3)` genuinely reaches the auto `a`, so it passes the
/// compile half. But with no constraints the decomposition builds zero
/// components, the registry has nothing to attach the cost to, and the
/// objective is dropped. That drop is exactly what must stop being silent.
const UNCONSTRAINED: &str = "\
module dic_min_unconstrained

structure DicMinUnconstrained {
    param a : Real = auto(free)
    minimize (a - 3.0) * (a - 3.0)
}
";

#[test]
fn unconstrained_objective_reports_unconsumed() {
    let result = eval_with_solver(UNCONSTRAINED);
    assert_one_unconsumed_error_naming(&result, "a");
}

/// The diagnostic is additive: `a`'s pre-existing undef classification is the
/// PRD's recorded baseline (`a = undef (awaiting solve)`) and must not shift.
/// γ reports the silence — it does not change what the solver does (O1).
#[test]
fn unconstrained_objective_leaves_the_undef_classification_alone() {
    let result = eval_with_solver(UNCONSTRAINED);
    let id = ValueCellId::new("DicMinUnconstrained", "a");
    let got = result
        .values
        .get(&id)
        .expect("DicMinUnconstrained.a must be present in the values map");
    assert!(
        matches!(got, reify_ir::Value::Undef),
        "baseline: `a` stays undef awaiting solve — γ adds a diagnostic, it does \
         not change the solve; got {got:?}"
    );
}

// ── (B8 / O1) a governing objective that solves stays quiet ─────────────────

/// The `bt1_single_scope.ri` shape: an auto bracketed by two constraints, with
/// an objective over it. The decomposition builds a component, the component
/// consumes the objective, the solve happens — nothing to report.
///
/// This is the case that proves the rule keys off *consumption*, not off
/// "an objective exists".
#[test]
fn governing_objective_that_solves_reports_nothing() {
    let source = "\
module dic_governing

structure DicGoverning {
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
    minimize w
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicGoverning", "w");
    assert_no_unconsumed(
        &result,
        "a component consumed this objective and solved it (O1)",
    );
}

// ── (O2) vacuous-healthy: every reachable auto is bound this run ────────────

/// An objective whose reachable autos are all concretely bound this run has
/// nothing left to optimise, and saying so would be noise on a healthy model.
///
/// The reach here spans BOTH an auto (`w`) and a concrete param (`base`), which
/// is what distinguishes this case from
/// `let_indirected_objective_over_a_solved_auto_reports_nothing` above: it pins
/// that the unbound-remainder test intersects with `auto_params` per id, so
/// `base` never enters the set and cannot keep the diagnostic alive once `w` is
/// bound. Consumption is `FallbackComponentZero` (the registry matches the
/// objective's DIRECT refs, which name only `total`), so condition 2 does not
/// fire — the vacuous-healthy rule is the sole reason this stays quiet.
///
/// **Pin-shape note (resolved in step-10).** The shape step-9 first wrote here —
/// `w` pinned by two mutually-tight inequalities — is RED on its *premise*, not
/// on the rule. Probing `SolverRegistry::production()` directly, none of
/// `constraint w == 25mm`, `w >= 25mm` + `w <= 25mm`, or `w == base` binds `w`
/// at all: each reports `constraints could not be satisfied (max absolute
/// residual: 5.00e-7)` and leaves the cell undef, so the anti-vacuity guard
/// tripped before the rule was ever consulted. The same probe found that
/// multi-auto shapes (two bracketed autos, autos coupled by an equality, or a
/// shared `w + h` bracket) also leave every auto undef. What DOES bind under the
/// production registry is a SINGLE auto with a two-sided inequality bracket,
/// which is the shape used here. That is a pre-existing solver characteristic,
/// independent of DIC γ — γ adds a diagnostic, it does not change what solves.
#[test]
fn objective_whose_autos_are_all_bound_reports_nothing() {
    let source = "\
module dic_vacuous

structure DicVacuous {
    param base : Length = 4mm
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
    let total = w + base
    minimize total
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicVacuous", "w");
    assert_no_unconsumed(
        &result,
        "every objective-reachable auto is bound this run — the vacuous-healthy \
         rule (O2)",
    );
}

// ── (task 4013) the synthesised centrality objective is exempt ──────────────

/// A scope that declares NO objective but picks up the synthesised
/// Chebyshev-centre one must stay silent: the rule reports *declared* intent
/// that the engine discarded, and there is no declaration here to discard.
///
/// The gate keys off `template.objective.is_some()`, which is the exact
/// structural test — a synthetic-centrality scope has `objective == None` at
/// compile time by construction.
#[test]
fn synthesised_centrality_objective_reports_nothing() {
    let source = "\
module dic_synthetic

structure DicSynthetic {
    param w : Length = auto
    constraint w >= 10mm
    constraint w <= 50mm
}
";
    let result = eval_with_solver(source);
    assert_no_unconsumed(
        &result,
        "no user-declared objective exists, so nothing was discarded (task 4013)",
    );
}

// ── order-independence w.r.t. PRD 2 ─────────────────────────────────────────

/// An objective that reads a `let` which reads a solved auto is transitively
/// consumed, and must stay silent — `dependent_cells` already encodes the
/// let-indirection, so this holds whichever order γ and PRD 2's let-tracing fix
/// (tasks 5396 / 5467-5474) land in.
#[test]
fn let_indirected_objective_over_a_solved_auto_reports_nothing() {
    let source = "\
module dic_let_indirect

structure DicLetIndirect {
    param w : Length = auto
    let doubled = w * 2.0
    constraint w >= 10mm
    constraint w <= 50mm
    minimize doubled
}
";
    let result = eval_with_solver(source);
    assert_bound(&result, "DicLetIndirect", "w");
    assert_no_unconsumed(
        &result,
        "the objective reaches `w` through the let and the component consumed \
         it — order-independent w.r.t. PRD 2",
    );
}
