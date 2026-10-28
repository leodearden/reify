//! Task #6653 regression fixtures: a solved model must never report its own
//! equalities violated, while a real drift still does.
//!
//! The sources are inline byte-mirrors of the evidence probes (minus their
//! geometry cell). Every test drives `Engine::check`, the solve-then-fold path;
//! `Engine::eval` alone never folds verdicts and so cannot see the defect.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DiagnosticCode, Severity, ValueCellId};
use reify_eval::{CheckResult, Engine};
use reify_ir::{Satisfaction, Value};
use reify_test_support::compile_source_with_stdlib;

/// Compile `source` (asserting it is compile-clean) and check it on a
/// `SolverPosture::Solve` engine (the production solver registry, the wiring
/// `reify-cli` uses) with the real
/// [`SimpleConstraintChecker`] verdict fold.
fn check_with_production_solver(source: &str) -> CheckResult {
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "fixture must be compile-clean; got {compile_errors:?}"
    );
    let mut engine = Engine::new(reify_eval::SolverPosture::Solve(reify_ir::ResolutionProfile::FRESH), Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled)
}

fn diagnostics_with_code(result: &CheckResult, code: DiagnosticCode) -> Vec<&Diagnostic> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .collect()
}

fn satisfaction_at(result: &CheckResult, index: u32) -> Satisfaction {
    result
        .constraint_results
        .iter()
        .find(|entry| entry.id.index == index)
        .unwrap_or_else(|| {
            panic!(
                "no constraint result at index {index}; got {:?}",
                result.constraint_results
            )
        })
        .satisfaction
}

fn assert_all_satisfied(result: &CheckResult, n_expected: usize) {
    assert_eq!(
        result.constraint_results.len(),
        n_expected,
        "constraint results: {:?}",
        result.constraint_results
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} must be Satisfied; diagnostics: {:?}",
            entry.id,
            result.diagnostics
        );
    }
    for code in [
        DiagnosticCode::ConstraintViolated,
        DiagnosticCode::ConstraintUnsatisfiable,
    ] {
        let found = diagnostics_with_code(result, code);
        assert!(found.is_empty(), "unexpected {code:?}: {found:?}");
    }
}

fn assert_violated(result: &CheckResult, index: u32) {
    assert_eq!(
        satisfaction_at(result, index),
        Satisfaction::Violated,
        "constraint {index} must be Violated; diagnostics: {:?}",
        result.diagnostics
    );
}

fn si_value(result: &CheckResult, entity: &str, member: &str) -> f64 {
    match result.values.get(&ValueCellId::new(entity, member)) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("{entity}.{member} must be a bound Scalar; got {other:?}"),
    }
}

fn assert_si_near(result: &CheckResult, entity: &str, member: &str, expected: f64, tol: f64) {
    let got = si_value(result, entity, member);
    assert!(
        (got - expected).abs() <= tol,
        "{entity}.{member} = {got:e}, expected {expected:e} within {tol:e}"
    );
}

#[test]
fn two_coupled_autos_solved_inexactly_report_satisfied() {
    let result = check_with_production_solver(
        "\
module multi_auto2
pub structure MultiAuto2 {
    param a : Length = auto
    param b : Length = auto
    constraint a + b == 30mm
    constraint a - b == 7mm
}
",
    );
    assert_si_near(&result, "MultiAuto2", "a", 0.0185, 1e-9);
    assert_si_near(&result, "MultiAuto2", "b", 0.0115, 1e-9);
    assert_all_satisfied(&result, 2);
}

#[test]
fn six_coupled_autos_report_satisfied() {
    let result = check_with_production_solver(
        "\
module multi_auto6
pub structure MultiAuto6 {
    param a : Length = auto
    param b : Length = auto
    param c : Length = auto
    param d : Length = auto
    param e : Length = auto
    param f : Length = auto
    constraint a + b == 30mm
    constraint a - b == 7mm
    constraint c == a + 3.3mm
    constraint d == b * 1.7
    constraint e + f == c + d
    constraint e - f == 1.1mm
}
",
    );
    assert_all_satisfied(&result, 6);
}

#[test]
fn auto_pinned_to_derived_let_reports_satisfied() {
    let result = check_with_production_solver(
        "\
module auto_probe
pub structure AutoProbe {
    param x : Length = auto
    let d : Length = 100mm + 23mm
    constraint x == d
}
",
    );
    assert_si_near(&result, "AutoProbe", "x", 0.123, 1e-9);
    assert_all_satisfied(&result, 1);
}

#[test]
fn hand_written_equality_between_derived_cells_reports_satisfied() {
    let result = check_with_production_solver(
        "\
module derived_eq
pub structure DerivedEq {
    let a : Length = 0.1mm * 3
    let b : Length = 0.3mm
    constraint a == b
}
",
    );
    assert_all_satisfied(&result, 1);
}

#[test]
fn one_percent_drift_on_pinned_equality_still_reports_violated() {
    let result = check_with_production_solver(
        "\
module drift
pub structure Drift {
    param a : Length = 10mm
    let b : Length = 10.1mm
    constraint a == b
}
",
    );
    assert_violated(&result, 0);
    let violated: Vec<&Diagnostic> =
        diagnostics_with_code(&result, DiagnosticCode::ConstraintViolated)
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
    assert!(
        !violated.is_empty(),
        "a 1% drift must raise a ConstraintViolated Error; got {:?}",
        result.diagnostics
    );
}

#[test]
fn ne_complement_within_tolerance_reports_violated() {
    let result = check_with_production_solver(
        "\
module derived_ne
pub structure DerivedNe {
    let a : Length = 0.1mm * 3
    let b : Length = 0.3mm
    constraint a != b
}
",
    );
    assert_violated(&result, 0);
}

#[test]
fn ordering_comparison_stays_exact() {
    let result = check_with_production_solver(
        "\
module ord
pub structure Ord {
    let a : Length = 10mm
    let b : Length = 10mm + 0.0000001mm
    constraint a >= b
    constraint a == b
}
",
    );
    assert_violated(&result, 0);
    assert_eq!(
        satisfaction_at(&result, 1),
        Satisfaction::Satisfied,
        "the companion `a == b` must be Satisfied; diagnostics: {:?}",
        result.diagnostics
    );
}

fn assert_no_error_diagnostics(result: &CheckResult) {
    let errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected Error diagnostics: {errors:?}"
    );
}

#[test]
fn feasible_inequality_plus_equality_resolves_and_reports_satisfied() {
    let result = check_with_production_solver(
        "\
module ineq_eq
pub structure IneqEq {
    param w : Length = auto
    constraint w >= 8mm
    constraint w == 10mm
}
",
    );
    assert_si_near(&result, "IneqEq", "w", 0.01, 1e-6);
    assert_all_satisfied(&result, 2);
    assert_no_error_diagnostics(&result);
}

#[test]
fn angle_inequality_plus_equality_resolves_and_reports_satisfied() {
    let result = check_with_production_solver(
        "\
module ang_ineq_eq
pub structure AngIneqEq {
    param t : Angle = auto
    constraint t >= 10deg
    constraint t == 30deg
}
",
    );
    assert_all_satisfied(&result, 2);
    assert_no_error_diagnostics(&result);
}

#[test]
fn mm3_volume_auto_reports_satisfied() {
    let result = check_with_production_solver(
        "\
module vol_eq
pub structure VolEq {
    param v : Volume = auto
    constraint v == 1000mm^3
}
",
    );
    assert_all_satisfied(&result, 1);
}

#[test]
fn equality_conflict_beyond_tolerance_still_fails() {
    let result = check_with_production_solver(
        "\
module eq_conflict
pub structure EqConflict {
    param w : Length = auto
    constraint w == 10mm
    constraint w == 10.1mm
}
",
    );
    let unsatisfiable: Vec<&Diagnostic> =
        diagnostics_with_code(&result, DiagnosticCode::ConstraintUnsatisfiable)
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
    assert!(
        !unsatisfiable.is_empty(),
        "a 1% equality conflict must raise a ConstraintUnsatisfiable Error; got {:?}",
        result.diagnostics
    );
    for entry in &result.constraint_results {
        assert_ne!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} must not be Satisfied",
            entry.id
        );
    }
}
