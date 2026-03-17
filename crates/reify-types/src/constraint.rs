use crate::diagnostics::Diagnostic;
use crate::expr::CompiledExpr;
use crate::identity::{ConstraintNodeId, ValueCellId};
use crate::ty::Type;
use crate::value::{Satisfaction, ValueMap};

/// Input to constraint checking: a batch of constraints with current values.
#[derive(Debug)]
pub struct ConstraintInput<'a> {
    /// The constraints to check, keyed by their node ID.
    pub constraints: Vec<(ConstraintNodeId, &'a CompiledExpr)>,
    /// Current values of all cells referenced by constraints.
    pub values: &'a ValueMap,
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
}

/// Trait for constraint checking. Lives in reify-types for dependency inversion —
/// implemented in reify-constraints, consumed by reify-eval.
pub trait ConstraintChecker: Send + Sync {
    /// Check a batch of constraints against current values.
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult>;
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

        let d3 = d.clone(); // Clone
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
        };
        assert_eq!(ap.id, ValueCellId::new("Bracket", "width"));
        assert_eq!(ap.param_type, Type::length());
        assert_eq!(ap.bounds, Some((0.01, 1.0)));
    }

    #[test]
    fn auto_param_without_bounds() {
        use crate::identity::ValueCellId;
        use crate::ty::Type;

        let ap = AutoParam {
            id: ValueCellId::new("Bracket", "angle"),
            param_type: Type::angle(),
            bounds: None,
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
        values.insert(ValueCellId::new("Bracket", "width"), crate::value::Value::length(0.08));

        let problem = ResolutionProblem {
            auto_params: vec![AutoParam {
                id: ValueCellId::new("Bracket", "width"),
                param_type: Type::length(),
                bounds: Some((0.01, 1.0)),
            }],
            constraints: vec![(
                ConstraintNodeId::new("Bracket", 0),
                make_literal_expr(),
            )],
            current_values: values,
            objective: Some(OptimizationObjective::Minimize(make_literal_expr())),
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
}
