// DimensionalSolver: Nelder-Mead based constraint solver for auto parameters.

#[cfg(test)]
mod tests {
    use reify_types::{
        ConstraintSolver, ResolutionProblem, SolveResult, ValueMap,
    };

    #[test]
    fn dimensional_solver_exists_and_implements_trait() {
        use crate::DimensionalSolver;

        // Verify it can be used as a trait object
        let solver = DimensionalSolver;
        let _boxed: Box<dyn ConstraintSolver> = Box::new(solver);
    }

    #[test]
    fn empty_problem_returns_solved() {
        use crate::DimensionalSolver;

        let solver = DimensionalSolver;
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: ValueMap::new(),
            objective: None,
        };

        let result = solver.solve(&problem);
        match result {
            SolveResult::Solved { values } => {
                assert!(values.is_empty(), "empty problem should return empty values");
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }
}
