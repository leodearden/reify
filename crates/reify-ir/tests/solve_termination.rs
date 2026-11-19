//! The cause → diagnostic policy (code and severity) of a failed continuous solve — task #6675
//! (solver-unification η), PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.5. One assertion per arm: the mapping is a public contract.

use reify_core::{DiagnosticCode, Severity};
use reify_ir::{
    BudgetExhaustion, InfeasibilityCertificate, NonFiniteEntry, NonFiniteEvaluation,
    ResidualSource, RowBlame, SolveFailureCause, SolveStall,
};

fn certified() -> SolveFailureCause {
    SolveFailureCause::Infeasible(InfeasibilityCertificate {
        gradient_norm: 1e-3,
        range_cosine: 1e-12,
        max_scaled_residual: 4e5,
        blame: vec![RowBlame {
            source: ResidualSource::Relation(1),
            row: 3,
            scaled_residual: -4e5,
        }],
    })
}

fn exhausted() -> SolveFailureCause {
    SolveFailureCause::BudgetExhausted(BudgetExhaustion {
        iterations: 1,
        budget: 1,
        step_norms: vec![0.4],
        max_scaled_residual: 12.0,
    })
}

fn stalled() -> SolveFailureCause {
    SolveFailureCause::Stalled(SolveStall {
        iterations: 90,
        step_norm: 1e-17,
        range_cosine: 1.0,
        max_scaled_residual: 1e8,
    })
}

fn non_finite() -> SolveFailureCause {
    SolveFailureCause::NonFinite(NonFiniteEvaluation {
        iterations: 4,
        source: ResidualSource::Relation(2),
        row: 5,
        entry: NonFiniteEntry::Jacobian { column: 3 },
        value: f64::NAN,
    })
}

#[test]
fn a_certified_infeasibility_is_e_solver_infeasible() {
    assert_eq!(certified().code(), DiagnosticCode::SolverInfeasible);
}

#[test]
fn an_exhausted_budget_is_e_solver_budget_exhausted() {
    assert_eq!(exhausted().code(), DiagnosticCode::SolverBudgetExhausted);
}

#[test]
fn an_uncertified_stall_is_a_search_failure() {
    assert_eq!(stalled().code(), DiagnosticCode::ConstraintUnsatisfiable);
}

#[test]
fn a_non_finite_evaluation_is_a_search_failure() {
    assert_eq!(non_finite().code(), DiagnosticCode::ConstraintUnsatisfiable);
}

#[test]
fn every_cause_renders_an_error_coded_by_itself() {
    for cause in [certified(), exhausted(), stalled(), non_finite()] {
        let d = cause.diagnostic("m");
        assert_eq!(d.severity, Severity::Error, "{cause:?}");
        assert_eq!(d.code, Some(cause.code()), "{cause:?}");
        assert_eq!(d.message, "m", "{cause:?}");
    }
}
