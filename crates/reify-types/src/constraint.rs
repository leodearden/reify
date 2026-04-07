use std::collections::HashMap;

use crate::diagnostics::Diagnostic;
use crate::expr::{CompiledExpr, CompiledFunction};
use crate::identity::{ConstraintNodeId, ValueCellId};
use crate::persistent::PersistentMap;
use crate::ty::Type;
use crate::value::{DeterminacyState, Satisfaction, Value, ValueMap};

/// Input to constraint checking: a batch of constraints with current values.
#[derive(Debug)]
pub struct ConstraintInput<'a> {
    /// The constraints to check, keyed by their node ID.
    pub constraints: Vec<(ConstraintNodeId, &'a CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub values: &'a ValueMap,
    /// User-defined functions available for evaluation within constraint expressions.
    pub functions: &'a [CompiledFunction],
    /// Optional determinacy snapshot for evaluating DeterminacyPredicate expressions
    /// within constraints. When `Some`, the checker passes this to `EvalContext::with_determinacy()`
    /// so that `determined()`, `undetermined()`, `constrained()`, and `partially_determined()`
    /// predicates can look up cell determinacy states.
    ///
    /// Defaults to `None` for backward compatibility — existing callers that don't need
    /// determinacy context can omit this field.
    pub determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
}

/// Result of checking a single constraint.
#[derive(Debug, Clone)]
pub struct ConstraintResult {
    pub id: ConstraintNodeId,
    pub satisfaction: Satisfaction,
    pub diagnostics: ConstraintDiagnostics,
}

/// Diagnostic information from constraint checking.
#[derive(Debug, Clone, Default)]
pub struct ConstraintDiagnostics {
    /// Human-readable messages about the constraint state.
    pub messages: Vec<Diagnostic>,
}

/// The domain of a constraint, determining which solver handles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintDomain {
    /// Dimensional constraints (e.g., length ratios, unit conversions).
    Dimensional,
    /// Geometric constraints (e.g., parallelism, tangency).
    Geometric,
    /// Logical constraints (e.g., boolean conditions).
    Logical,
    /// Cross-domain constraints spanning multiple domains.
    CrossDomain,
}

/// Optimization objective for constraint resolution.
#[derive(Debug, Clone)]
pub enum OptimizationObjective {
    /// Minimize the value of the expression.
    Minimize(CompiledExpr),
    /// Maximize the value of the expression.
    Maximize(CompiledExpr),
}

/// An auto parameter to be resolved by the constraint solver.
#[derive(Debug, Clone)]
pub struct AutoParam {
    /// The value cell this auto param corresponds to.
    pub id: ValueCellId,
    /// The declared type of the parameter.
    pub param_type: Type,
    /// Optional lower and upper bounds for numeric resolution.
    pub bounds: Option<(f64, f64)>,
    /// Whether this is an `auto(free)` parameter that skips uniqueness verification.
    /// When `true`, the solver skips the perturbation-based uniqueness check and
    /// returns `SolveResult::Solved { unique: false }` directly.
    pub free: bool,
}

/// The result of a constraint solve attempt.
#[derive(Debug, Clone)]
pub enum SolveResult {
    /// Successfully resolved all auto parameters.
    ///
    /// **Note:** `Solved` indicates constraint satisfaction but does not guarantee
    /// objective optimality. When an optimization objective is present, the
    /// Nelder-Mead optimizer may have hit the iteration limit without full
    /// convergence; the returned values satisfy all constraints but the objective
    /// value may not be globally optimal.
    Solved {
        /// Resolved values for auto parameters.
        values: HashMap<ValueCellId, Value>,
    },
    /// The constraints are infeasible — no solution exists.
    Infeasible {
        /// Diagnostics explaining why the constraints are infeasible.
        diagnostics: Vec<Diagnostic>,
    },
    /// The solver made no progress (e.g., iteration limit reached).
    NoProgress {
        /// Human-readable reason for no progress.
        reason: String,
    },
}

/// A constraint resolution problem — input to the constraint solver.
#[derive(Debug, Clone)]
pub struct ResolutionProblem {
    /// The auto parameters to resolve.
    pub auto_params: Vec<AutoParam>,
    /// Constraints to satisfy, each paired with its compiled expression.
    pub constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub current_values: ValueMap,
    /// Optional optimization objective.
    pub objective: Option<OptimizationObjective>,
    /// User-defined functions available for evaluating expressions.
    pub functions: Vec<CompiledFunction>,
}

/// Trait for constraint checking. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintChecker: Send + Sync {
    /// Check a batch of constraints against current values.
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult>;
}

/// Trait for constraint solving. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintSolver: Send + Sync {
    /// Attempt to resolve auto parameters to satisfy constraints.
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraint_domain_all_variants_exist() {
        let _dimensional = ConstraintDomain::Dimensional;
        let _geometric = ConstraintDomain::Geometric;
        let _logical = ConstraintDomain::Logical;
        let _cross = ConstraintDomain::CrossDomain;
    }

    #[test]
    fn constraint_domain_is_copy_clone_eq_hash() {
        let d = ConstraintDomain::Dimensional;
        let d2 = d; // Copy
        assert_eq!(d, d2); // PartialEq + Eq

        let d3 = Clone::clone(&d); // Clone
        assert_eq!(d, d3);

        // Hash: usable as HashMap key
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(ConstraintDomain::Dimensional, "dim");
        map.insert(ConstraintDomain::Geometric, "geo");
        assert_eq!(map.get(&ConstraintDomain::Dimensional), Some(&"dim"));
    }

    #[test]
    fn constraint_domain_variants_are_distinct() {
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::Geometric);
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::Logical);
        assert_ne!(ConstraintDomain::Dimensional, ConstraintDomain::CrossDomain);
        assert_ne!(ConstraintDomain::Geometric, ConstraintDomain::Logical);
        assert_ne!(ConstraintDomain::Geometric, ConstraintDomain::CrossDomain);
        assert_ne!(ConstraintDomain::Logical, ConstraintDomain::CrossDomain);
    }

    #[test]
    fn constraint_domain_debug() {
        assert!(format!("{:?}", ConstraintDomain::Dimensional).contains("Dimensional"));
        assert!(format!("{:?}", ConstraintDomain::CrossDomain).contains("CrossDomain"));
    }

    #[test]
    fn auto_param_with_bounds() {
        use crate::identity::ValueCellId;
        use crate::ty::Type;

        let ap = AutoParam {
            id: ValueCellId::new("Bracket", "width"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: false,
        };
        assert_eq!(ap.id, ValueCellId::new("Bracket", "width"));
        assert_eq!(ap.param_type, Type::length());
        assert_eq!(ap.bounds, Some((0.01, 1.0)));
    }

    #[test]
    fn auto_param_with_free_flag() {
        use crate::identity::ValueCellId;
        use crate::ty::Type;

        let strict = AutoParam {
            id: ValueCellId::new("Bracket", "width"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: false,
        };
        assert!(!strict.free);

        let free = AutoParam {
            id: ValueCellId::new("Bracket", "height"),
            param_type: Type::length(),
            bounds: Some((0.01, 1.0)),
            free: true,
        };
        assert!(free.free);
    }

    #[test]
    fn auto_param_without_bounds() {
        use crate::identity::ValueCellId;
        use crate::ty::Type;

        let ap = AutoParam {
            id: ValueCellId::new("Bracket", "angle"),
            param_type: Type::angle(),
            bounds: None,
            free: false,
        };
        assert!(ap.bounds.is_none());

        // Debug works
        let debug = format!("{:?}", ap);
        assert!(debug.contains("AutoParam"));
    }

    fn make_literal_expr() -> CompiledExpr {
        use crate::hash::ContentHash;
        use crate::value::Value;
        CompiledExpr {
            kind: crate::expr::CompiledExprKind::Literal(Value::Real(1.0)),
            result_type: Type::Real,
            content_hash: ContentHash::of(b"test"),
        }
    }

    #[test]
    fn optimization_objective_minimize() {
        let expr = make_literal_expr();
        let obj = OptimizationObjective::Minimize(expr);
        let debug = format!("{:?}", obj);
        assert!(debug.contains("Minimize"));
    }

    #[test]
    fn optimization_objective_maximize() {
        let expr = make_literal_expr();
        let obj = OptimizationObjective::Maximize(expr);
        let debug = format!("{:?}", obj);
        assert!(debug.contains("Maximize"));
    }

    #[test]
    fn resolution_problem_empty() {
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: crate::value::ValueMap::new(),
            objective: None,
            functions: vec![],
        };
        assert!(problem.auto_params.is_empty());
        assert!(problem.constraints.is_empty());
        assert!(problem.current_values.is_empty());
        assert!(problem.objective.is_none());
    }

    #[test]
    fn resolution_problem_populated() {
        use crate::identity::ValueCellId;
        let mut values = crate::value::ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "width"),
            crate::value::Value::length(0.08),
        );

        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: ValueCellId::new("Bracket", "width"),
                param_type: Type::length(),
                bounds: Some((0.01, 1.0)),
                free: false,
            }],
            constraints: vec![(ConstraintNodeId::new("Bracket", 0), make_literal_expr())],
            current_values: values,
            objective: Some(OptimizationObjective::Minimize(make_literal_expr())),
            functions: vec![],
        };
        assert_eq!(problem.auto_params.len(), 1);
        assert_eq!(problem.constraints.len(), 1);
        assert!(!problem.current_values.is_empty());
        assert!(problem.objective.is_some());

        // Debug works
        let debug = format!("{:?}", problem);
        assert!(debug.contains("ResolutionProblem"));
    }

    #[test]
    fn optimization_objective_clone() {
        let expr = make_literal_expr();
        let obj = OptimizationObjective::Minimize(expr);
        let obj2 = obj.clone();
        let d1 = format!("{:?}", obj);
        let d2 = format!("{:?}", obj2);
        assert_eq!(d1, d2);
    }

    #[test]
    fn solve_result_solved() {
        use crate::identity::ValueCellId;
        use crate::value::Value;
        use std::collections::HashMap;

        let mut values = HashMap::new();
        values.insert(ValueCellId::new("Bracket", "width"), Value::length(0.05));

        let result = SolveResult::Solved { values };
        match &result {
            SolveResult::Solved { values } => {
                assert_eq!(values.len(), 1);
                assert!(values.contains_key(&ValueCellId::new("Bracket", "width")));
            }
            _ => panic!("expected Solved"),
        }
    }

    #[test]
    fn solve_result_infeasible() {
        use crate::diagnostics::{Diagnostic, Severity};

        let result = SolveResult::Infeasible {
            diagnostics: vec![Diagnostic {
                message: "constraint unsatisfiable".to_string(),
                severity: Severity::Error,
                labels: vec![],
            }],
        };
        match &result {
            SolveResult::Infeasible { diagnostics } => {
                assert_eq!(diagnostics.len(), 1);
                assert!(diagnostics[0].message.contains("unsatisfiable"));
            }
            _ => panic!("expected Infeasible"),
        }
    }

    #[test]
    fn solve_result_no_progress() {
        let result = SolveResult::NoProgress {
            reason: "iteration limit reached".to_string(),
        };
        match &result {
            SolveResult::NoProgress { reason } => {
                assert_eq!(reason, "iteration limit reached");
            }
            _ => panic!("expected NoProgress"),
        }
    }

    #[test]
    fn solve_result_clone() {
        let result = SolveResult::NoProgress {
            reason: "test".to_string(),
        };
        let result2 = result.clone();
        let d1 = format!("{:?}", result);
        let d2 = format!("{:?}", result2);
        assert_eq!(d1, d2);
    }

    struct MockSolver;

    impl ConstraintSolver for MockSolver {
        fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
            SolveResult::NoProgress {
                reason: "mock".to_string(),
            }
        }
    }

    #[test]
    fn constraint_solver_trait_call() {
        let solver = MockSolver;
        let problem = ResolutionProblem {
            auto_params: vec![],
            constraints: vec![],
            current_values: crate::value::ValueMap::new(),
            objective: None,
            functions: vec![],
        };
        let result = solver.solve(&problem);
        match result {
            SolveResult::NoProgress { reason } => assert_eq!(reason, "mock"),
            _ => panic!("expected NoProgress"),
        }
    }

    #[test]
    fn constraint_solver_is_send_sync() {
        // Verify the trait requires Send + Sync by using it as a trait object
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockSolver>();

        // Can be used as Box<dyn ConstraintSolver>
        let _boxed: Box<dyn ConstraintSolver> = Box::new(MockSolver);
    }
}
