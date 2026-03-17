// DimensionalSolver: Nelder-Mead based constraint solver for auto parameters.

use std::collections::HashMap;

use reify_types::{ConstraintSolver, ResolutionProblem, SolveResult};

/// Derivative-free constraint solver using Nelder-Mead optimization.
///
/// Solves for auto parameters by minimizing a penalty function that
/// encodes constraint violations. For pure feasibility (no objective),
/// the cost is the sum of squared constraint violations. For optimization,
/// the cost combines the objective value with a weighted penalty term.
pub struct DimensionalSolver;

impl ConstraintSolver for DimensionalSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // Trivial case: no auto parameters to solve for
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
            };
        }

        // TODO: implement Nelder-Mead solving
        SolveResult::NoProgress {
            reason: "not yet implemented".to_string(),
        }
    }
}

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
    fn build_trial_values_inserts_auto_params() {
        use super::build_trial_values;
        use reify_types::{AutoParam, DimensionVector, Type, Value, ValueCellId};

        let thickness_id = ValueCellId::new("Bracket", "thickness");
        let width_id = ValueCellId::new("Bracket", "width");

        // Base map has width=80mm
        let mut base = ValueMap::new();
        base.insert(
            width_id.clone(),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let params = vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }];

        let trial = build_trial_values(&base, &params, &[0.005]);

        // Auto param should be inserted with correct dimension
        let thickness = trial.get(&thickness_id).expect("thickness should exist");
        match thickness {
            &Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!(
                    (si_value - 0.005).abs() < 1e-15,
                    "si_value should be 0.005, got {}",
                    si_value
                );
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }

        // Non-auto value should be preserved
        let width = trial.get(&width_id).expect("width should be preserved");
        match width {
            &Value::Scalar { si_value, .. } => {
                assert!(
                    (si_value - 0.080).abs() < 1e-15,
                    "width should be 0.080"
                );
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
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
