//! A relate Frame failure read through the `SolveResult` contract keeps its code
//! and Error severity through `SolverRegistry` and `Engine::eval` (task 9869, PRD
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.5).
//!
//! The relate registry routing does not exist yet (#6677 builds it), so a stub
//! solver hands the relate adapter's own `SolveResult` to the registry: the seam
//! #6677 will consume.

use reify_constraints::SolverRegistry;
use reify_constraints::relate_solve::{FrameSolveOutcome, FrameUnknown};
use reify_core::{DiagnosticCode, ModulePath, Severity, Type};
use reify_eval::{Engine, SolverPosture};
use reify_ir::{BudgetExhaustion, ResolutionProfile, SolveFailureCause};
use reify_test_support::mocks::{MockConstraintChecker, SequencedMockConstraintSolver};
use reify_test_support::{
    CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref,
};

#[test]
fn a_budget_exhausted_relate_solve_surfaces_solver_budget_exhausted_through_the_registry() {
    let carried = FrameSolveOutcome {
        iterations: 1,
        result: Err(SolveFailureCause::BudgetExhausted(BudgetExhaustion {
            iterations: 1,
            budget: 1,
            step_norms: vec![0.4],
            max_scaled_residual: 12.0,
        })),
    }
    .into_solve_result(&FrameUnknown {
        sub: "bolt".into(),
        free: false,
    });
    let registry = SolverRegistry::new(Box::new(SequencedMockConstraintSolver::new(vec![
        carried,
    ])));

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let mut engine = Engine::with_prelude(
        SolverPosture::Solve(ResolutionProfile::FRESH),
        Box::new(MockConstraintChecker::new()),
        None,
        &[],
    )
    .with_solver(Box::new(registry));

    let result = engine.eval(&module);

    let coded: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverBudgetExhausted))
        .collect();
    assert_eq!(coded.len(), 1, "{:?}", result.diagnostics);
    let exhausted = coded[0];
    assert_eq!(exhausted.severity, Severity::Error, "{exhausted:?}");
    for needle in ["`bolt`", "auto(seed"] {
        assert!(exhausted.message.contains(needle), "{exhausted:?}");
    }
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Constraint solver made no progress")),
        "the failure must not degrade to an uncoded warning: {:?}",
        result.diagnostics
    );
}
