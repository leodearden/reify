//! The CrossDomain fallback strategy of PRD2 §4.1: an all-discrete component is
//! answered by CP-SAT, anything else by `DimensionalSolver` exactly as before.
//! The routing table lives in `docs/prds/v0_6/discrete-cost-minimisation.md` §4.1.

use crate::{CpSatSolver, DimensionalSolver};
use reify_ir::{
    ComputeDispatch, ConstraintSolver, RankedSolveResult, ResolutionProblem, SolveResult,
};

pub(crate) struct DiscreteFirstFallback;

/// The one routing decision every trait method forwards through, so the four
/// entry points cannot disagree about which solver owns a component.
fn route(problem: &ResolutionProblem) -> &'static dyn ConstraintSolver {
    if is_all_discrete(problem) {
        &CpSatSolver
    } else {
        &DimensionalSolver
    }
}

/// Discreteness is CP-SAT's enumeration CAPABILITY, judged against this
/// component's own constraints — the same authority decompose routes on.
fn is_all_discrete(problem: &ResolutionProblem) -> bool {
    !problem.auto_params.is_empty()
        && problem
            .auto_params
            .iter()
            .all(|ap| crate::cpsat::can_enumerate(ap, &problem.constraints))
}

impl ConstraintSolver for DiscreteFirstFallback {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        route(problem).solve(problem)
    }

    fn solve_with_dispatch(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn ComputeDispatch>,
    ) -> SolveResult {
        route(problem).solve_with_dispatch(problem, dispatch)
    }

    fn solve_ranked(&self, problem: &ResolutionProblem) -> RankedSolveResult {
        route(problem).solve_ranked(problem)
    }

    fn solve_ranked_with_dispatch(
        &self,
        problem: &ResolutionProblem,
        dispatch: Option<&dyn ComputeDispatch>,
    ) -> RankedSolveResult {
        route(problem).solve_ranked_with_dispatch(problem, dispatch)
    }
}

#[cfg(test)]
mod tests {
    use super::DiscreteFirstFallback;
    use crate::{CpSatSolver, DimensionalSolver};
    use reify_core::{DiagnosticCode, Type, ValueCellId};
    use reify_ir::{
        AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
        OptimalityStatus, RankedSolveResult, ResolutionProblem, SolveResult, Value, ValueMap,
    };
    use reify_test_support::{
        binop, cnid, conditional_expr, eq, ge, le, literal, value_ref_typed, vcid,
    };
    use std::collections::HashMap;

    const ENTITY: &str = "S";

    fn auto(member: &str, param_type: Type, bounds: Option<(f64, f64)>) -> AutoParam {
        AutoParam {
            id: vcid(ENTITY, member),
            param_type,
            bounds,
            free: true,
        }
    }

    fn bool_auto(member: &str) -> AutoParam {
        auto(member, Type::Bool, None)
    }

    fn real_auto(member: &str) -> AutoParam {
        auto(member, Type::dimensionless_scalar(), Some((-100.0, 100.0)))
    }

    /// An `Int` auto exactly as the engine's `build_auto_param_list` emits
    /// every one today: with no bounds, which CP-SAT cannot enumerate.
    fn engine_int_auto(member: &str) -> AutoParam {
        auto(member, Type::Int, None)
    }

    fn bool_ref(member: &str) -> CompiledExpr {
        value_ref_typed(ENTITY, member, Type::Bool)
    }

    fn real_ref(member: &str) -> CompiledExpr {
        value_ref_typed(ENTITY, member, Type::dimensionless_scalar())
    }

    fn real(v: f64) -> CompiledExpr {
        literal(Value::Real(v))
    }

    /// `if <member> then 1.0 else -1.0` — a Bool reach into Real arithmetic,
    /// which is what makes a component CrossDomain rather than Logical.
    fn sign_of(member: &str) -> CompiledExpr {
        conditional_expr(bool_ref(member), real(1.0), real(-1.0))
    }

    fn problem(
        auto_params: Vec<AutoParam>,
        constraints: Vec<CompiledExpr>,
        objective: Option<ObjectiveSet>,
    ) -> ResolutionProblem {
        ResolutionProblem {
            auto_params,
            constraints: (0u32..)
                .zip(constraints)
                .map(|(i, c)| (cnid(ENTITY, i), c))
                .collect(),
            current_values: ValueMap::new(),
            objective,
            functions: Vec::new().into(),
            dependent_cells: Vec::new(),
        }
    }

    /// A comparable projection of a solve outcome.
    ///
    /// Whole results are never compared by `Debug`: `HashMap`'s `Debug` order
    /// is per-instance random, so two equal value maps can print differently.
    #[derive(Debug, PartialEq)]
    enum Outcome {
        Solved {
            values: HashMap<ValueCellId, Value>,
            unique: bool,
        },
        Ranked {
            candidates: Vec<Candidate>,
            optimality: String,
        },
        Infeasible(Vec<(Option<DiagnosticCode>, String)>),
        NoProgress(String),
    }

    #[derive(Debug, PartialEq)]
    struct Candidate {
        values: HashMap<ValueCellId, Value>,
        unique: bool,
        objective_score_bits: Option<u64>,
    }

    impl From<SolveResult> for Outcome {
        fn from(result: SolveResult) -> Self {
            match result {
                SolveResult::Solved { values, unique } => Outcome::Solved { values, unique },
                not_solved => not_solved
                    .into_ranked_pass_through()
                    .expect("the Solved arm is handled above")
                    .into(),
            }
        }
    }

    impl From<RankedSolveResult> for Outcome {
        fn from(result: RankedSolveResult) -> Self {
            match result {
                RankedSolveResult::Ranked {
                    candidates,
                    optimality,
                } => Outcome::Ranked {
                    candidates: candidates
                        .into_iter()
                        .map(|c| Candidate {
                            values: c.values,
                            unique: c.unique,
                            objective_score_bits: c.objective_score.map(f64::to_bits),
                        })
                        .collect(),
                    optimality: format!("{optimality:?}"),
                },
                RankedSolveResult::Infeasible { diagnostics } => Outcome::Infeasible(
                    diagnostics
                        .into_iter()
                        .map(|d| (d.code, d.message))
                        .collect(),
                ),
                RankedSolveResult::NoProgress { reason } => Outcome::NoProgress(reason),
            }
        }
    }

    fn assert_same_outcome(actual: SolveResult, expected: SolveResult) {
        assert_eq!(Outcome::from(actual), Outcome::from(expected));
    }

    fn assert_same_ranked_outcome(actual: RankedSolveResult, expected: RankedSolveResult) {
        assert_eq!(Outcome::from(actual), Outcome::from(expected));
    }

    /// The route under test is only observable when the two candidate
    /// solvers disagree on the problem, so every fall-through test first
    /// proves CP-SAT would have answered differently.
    fn assert_cpsat_cannot_answer(p: &ResolutionProblem) {
        let cpsat = CpSatSolver.solve(p);
        assert!(
            matches!(cpsat, SolveResult::NoProgress { .. }),
            "precondition: CP-SAT must be unable to enumerate this problem; got {cpsat:?}"
        );
        assert_ne!(
            Outcome::from(DimensionalSolver.solve(p)),
            Outcome::from(cpsat),
            "precondition: the two routes must be distinguishable on this problem"
        );
    }

    #[test]
    fn an_all_bool_component_is_answered_by_cpsat() {
        let balance = eq(binop(BinOp::Add, sign_of("a"), sign_of("b")), real(0.0));
        let p = problem(
            vec![bool_auto("a"), bool_auto("b")],
            vec![balance.clone()],
            None,
        );

        let solved = DiscreteFirstFallback.solve(&p);
        match &solved {
            SolveResult::Solved { values, unique } => {
                assert!(
                    !unique,
                    "the balance has two models (a != b), so the answer is not unique"
                );
                assert!(
                    values.values().all(|v| matches!(v, Value::Bool(_))),
                    "every auto must resolve to an exact Bool; got {values:?}"
                );
            }
            other => panic!("expected Solved; got {other:?}"),
        }
        assert_same_outcome(solved, CpSatSolver.solve(&p));
        assert_same_outcome(
            DiscreteFirstFallback.solve_with_dispatch(&p, None),
            CpSatSolver.solve(&p),
        );

        let minimise_a = ObjectiveSet::single(ObjectiveSense::Minimize, sign_of("a"));
        let p = problem(
            vec![bool_auto("a"), bool_auto("b")],
            vec![balance],
            Some(minimise_a),
        );
        for ranked in [
            DiscreteFirstFallback.solve_ranked(&p),
            DiscreteFirstFallback.solve_ranked_with_dispatch(&p, None),
        ] {
            match &ranked {
                RankedSolveResult::Ranked {
                    candidates,
                    optimality: OptimalityStatus::ProvenOptimal,
                } => assert_eq!(
                    candidates[0].values.get(&vcid(ENTITY, "a")),
                    Some(&Value::Bool(false)),
                    "minimising sign(a) must pick a = false"
                ),
                other => panic!("expected a ProvenOptimal ranking from CP-SAT; got {other:?}"),
            }
            assert_same_ranked_outcome(ranked, CpSatSolver.solve_ranked(&p));
        }
    }

    #[test]
    fn a_mixed_bool_real_component_falls_through_to_dimensional_unchanged() {
        let autos = || vec![bool_auto("up"), real_auto("t")];
        let floor = ge(
            real_ref("t"),
            conditional_expr(bool_ref("up"), real(3.0), real(5.0)),
        );
        let ceiling = le(real_ref("t"), real(10.0));
        let p = problem(autos(), vec![floor.clone(), ceiling.clone()], None);

        assert_cpsat_cannot_answer(&p);
        assert_same_outcome(DiscreteFirstFallback.solve(&p), DimensionalSolver.solve(&p));
        assert_same_outcome(
            DiscreteFirstFallback.solve_with_dispatch(&p, None),
            DimensionalSolver.solve(&p),
        );

        let minimise_t = ObjectiveSet::single(ObjectiveSense::Minimize, real_ref("t"));
        let p = problem(autos(), vec![floor, ceiling], Some(minimise_t));
        assert_same_ranked_outcome(
            DiscreteFirstFallback.solve_ranked(&p),
            DimensionalSolver.solve_ranked(&p),
        );
        assert_same_ranked_outcome(
            DiscreteFirstFallback.solve_ranked_with_dispatch(&p, None),
            DimensionalSolver.solve_ranked(&p),
        );
    }

    #[test]
    fn an_all_continuous_component_falls_through_to_dimensional_unchanged() {
        let sum = eq(binop(BinOp::Add, real_ref("a"), real_ref("b")), real(10.0));
        let difference = eq(binop(BinOp::Sub, real_ref("a"), real_ref("b")), real(2.0));
        let p = problem(
            vec![real_auto("a"), real_auto("b")],
            vec![sum, difference],
            None,
        );

        assert_cpsat_cannot_answer(&p);
        assert_same_outcome(DiscreteFirstFallback.solve(&p), DimensionalSolver.solve(&p));
        assert_same_outcome(
            DiscreteFirstFallback.solve_with_dispatch(&p, None),
            DimensionalSolver.solve(&p),
        );
        assert_same_ranked_outcome(
            DiscreteFirstFallback.solve_ranked(&p),
            DimensionalSolver.solve_ranked(&p),
        );
        assert_same_ranked_outcome(
            DiscreteFirstFallback.solve_ranked_with_dispatch(&p, None),
            DimensionalSolver.solve_ranked(&p),
        );
    }

    /// A type-list predicate ("Bool and Int are discrete") would send this
    /// component to CP-SAT, which cannot enumerate an unbounded Int and would
    /// turn today's DimensionalSolver answer into a new failure.
    #[test]
    fn a_bool_beside_an_unenumerable_int_falls_through_rather_than_failing_in_cpsat() {
        let int_ref = || value_ref_typed(ENTITY, "n", Type::Int);
        let p = problem(
            vec![bool_auto("flag"), engine_int_auto("n")],
            vec![
                eq(bool_ref("flag"), literal(Value::Bool(true))),
                ge(int_ref(), literal(Value::Int(2))),
                le(int_ref(), literal(Value::Int(4))),
            ],
            None,
        );

        assert_cpsat_cannot_answer(&p);
        assert_same_outcome(DiscreteFirstFallback.solve(&p), DimensionalSolver.solve(&p));
        assert_same_outcome(
            DiscreteFirstFallback.solve_with_dispatch(&p, None),
            DimensionalSolver.solve(&p),
        );
    }
}
