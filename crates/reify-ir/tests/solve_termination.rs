//! The cause → diagnostic-code policy of a failed continuous solve — task #6675
//! (solver-unification η), PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md`
//! §7.5. One assertion per arm: the mapping is a public contract.

use reify_core::DiagnosticCode;
use reify_ir::{
    BudgetExhaustion, InfeasibilityCertificate, ResidualSource, RowBlame, SolveFailureCause,
    SolveStall,
};

#[test]
fn a_certified_infeasibility_is_e_solver_infeasible() {
    let cause = SolveFailureCause::Infeasible(InfeasibilityCertificate {
        gradient_norm: 1e-3,
        range_cosine: 1e-12,
        max_scaled_residual: 4e5,
        blame: vec![RowBlame {
            source: ResidualSource::Relation(1),
            row: 3,
            scaled_residual: -4e5,
        }],
    });
    assert_eq!(cause.code(), DiagnosticCode::SolverInfeasible);
}

#[test]
fn an_exhausted_budget_is_e_solver_budget_exhausted() {
    let cause = SolveFailureCause::BudgetExhausted(BudgetExhaustion {
        iterations: 1,
        budget: 1,
        step_norms: vec![0.4],
        max_scaled_residual: 12.0,
    });
    assert_eq!(cause.code(), DiagnosticCode::SolverBudgetExhausted);
}

#[test]
fn an_uncertified_stall_is_a_search_failure() {
    let cause = SolveFailureCause::Stalled(SolveStall {
        iterations: 90,
        step_norm: 1e-17,
        range_cosine: 1.0,
        max_scaled_residual: 1e8,
    });
    assert_eq!(cause.code(), DiagnosticCode::ConstraintUnsatisfiable);
}
