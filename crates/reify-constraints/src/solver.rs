// DimensionalSolver: Nelder-Mead based constraint solver for auto parameters.

use std::collections::HashMap;

use reify_types::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintNodeId, ConstraintSolver,
    DimensionVector, ResolutionProblem, SolveResult, Type, Value, ValueMap,
};

/// Derivative-free constraint solver using Nelder-Mead optimization.
///
/// Solves for auto parameters by minimizing a penalty function that
/// encodes constraint violations. For pure feasibility (no objective),
/// the cost is the sum of squared constraint violations. For optimization,
/// the cost combines the objective value with a weighted penalty term.
pub struct DimensionalSolver;

/// Extract the DimensionVector from a Type, defaulting to DIMENSIONLESS.
fn dimension_of(ty: &Type) -> DimensionVector {
    match ty {
        Type::Scalar { dimension } => *dimension,
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// Build a ValueMap from a base map with trial auto-param values inserted.
///
/// Clones the base map (O(1) via PersistentMap structural sharing) and
/// inserts each auto param as a Value::Scalar with the correct dimension.
fn build_trial_values(base: &ValueMap, params: &[AutoParam], x: &[f64]) -> ValueMap {
    let mut values = base.clone();
    for (param, &val) in params.iter().zip(x.iter()) {
        values.insert(
            param.id.clone(),
            Value::Scalar {
                si_value: val,
                dimension: dimension_of(&param.param_type),
            },
        );
    }
    values
}

/// Extract initial parameter values from the problem.
///
/// For each auto param, uses the current value if available, otherwise
/// the midpoint of bounds, otherwise a small default (0.01 for lengths).
fn extract_initial_point(problem: &ResolutionProblem) -> Vec<f64> {
    problem
        .auto_params
        .iter()
        .map(|param| {
            // Try current value first
            if let Some(val) = problem.current_values.get(&param.id) {
                if let Some(f) = val.as_f64() {
                    return f;
                }
            }
            // Fall back to bounds midpoint
            if let Some((lo, hi)) = param.bounds {
                return (lo + hi) / 2.0;
            }
            // Default based on dimension
            0.01
        })
        .collect()
}

/// Compute the violation magnitude for a single comparison expression.
///
/// For comparison operators (Gt, Ge, Lt, Le), evaluates the left and right
/// sub-expressions to get numeric values and computes a continuous violation.
/// Returns 0.0 if satisfied. For non-decomposable boolean constraints,
/// uses a fixed penalty when violated.
fn comparison_violation(op: BinOp, left: &CompiledExpr, right: &CompiledExpr, values: &ValueMap) -> f64 {
    let lhs = reify_expr::eval_expr(left, values).as_f64();
    let rhs = reify_expr::eval_expr(right, values).as_f64();

    match (lhs, rhs) {
        (Some(l), Some(r)) => match op {
            // For l > r: violation when l <= r, magnitude = (r - l)
            BinOp::Gt => {
                if l > r { 0.0 } else { (r - l + 1e-12).powi(2) }
            }
            // For l >= r: violation when l < r
            BinOp::Ge => {
                if l >= r { 0.0 } else { (r - l + 1e-12).powi(2) }
            }
            // For l < r: violation when l >= r, magnitude = (l - r)
            BinOp::Lt => {
                if l < r { 0.0 } else { (l - r + 1e-12).powi(2) }
            }
            // For l <= r: violation when l > r
            BinOp::Le => {
                if l <= r { 0.0 } else { (l - r + 1e-12).powi(2) }
            }
            // For equality: distance squared
            BinOp::Eq => {
                let d = l - r;
                if d.abs() < 1e-15 { 0.0 } else { d.powi(2) }
            }
            BinOp::Ne => {
                if (l - r).abs() > 1e-15 { 0.0 } else { 1.0 }
            }
            // Not a comparison
            _ => 1.0,
        },
        // Can't decompose numerically; use fixed penalty
        _ => 1.0,
    }
}

/// Compute the violation for a single constraint expression.
///
/// Tries to decompose comparison expressions for continuous violation.
/// Falls back to binary penalty for non-decomposable expressions.
fn constraint_violation(expr: &CompiledExpr, values: &ValueMap) -> f64 {
    // First try decomposing into a comparison
    match &expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            match op {
                BinOp::Gt | BinOp::Ge | BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                    comparison_violation(*op, left, right, values)
                }
                BinOp::And => {
                    // AND: sum violations of both sides
                    constraint_violation(left, values) + constraint_violation(right, values)
                }
                BinOp::Or => {
                    // OR: minimum violation of both sides
                    let lv = constraint_violation(left, values);
                    let rv = constraint_violation(right, values);
                    lv.min(rv)
                }
                _ => {
                    // Not a logical/comparison op; evaluate as boolean
                    match reify_expr::eval_expr(expr, values) {
                        Value::Bool(true) => 0.0,
                        Value::Bool(false) => 1.0,
                        Value::Undef => 10.0,
                        _ => 1.0,
                    }
                }
            }
        }
        _ => {
            // Non-binop expression (e.g., literal bool, function call)
            match reify_expr::eval_expr(expr, values) {
                Value::Bool(true) => 0.0,
                Value::Bool(false) => 1.0,
                Value::Undef => 10.0,
                _ => 1.0,
            }
        }
    }
}

/// Compute the total violation across all constraints.
///
/// Returns the sum of squared violations. Zero means all constraints
/// are satisfied.
fn compute_total_violation(
    constraints: &[(ConstraintNodeId, CompiledExpr)],
    values: &ValueMap,
) -> f64 {
    constraints
        .iter()
        .map(|(_, expr)| constraint_violation(expr, values))
        .sum()
}

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
    fn compute_violation_satisfied_constraint() {
        use super::compute_total_violation;
        use reify_types::{
            BinOp, CompiledExpr, ConstraintNodeId, DimensionVector, Type, Value, ValueCellId,
        };

        // thickness > 2mm, thickness = 5mm → satisfied, violation = 0
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![(ConstraintNodeId::new("Bracket", 0), expr)];
        let violation = compute_total_violation(&constraints, &values);
        assert!(
            violation.abs() < 1e-15,
            "satisfied constraint should have zero violation, got {}",
            violation
        );
    }

    #[test]
    fn compute_violation_violated_constraint() {
        use super::compute_total_violation;
        use reify_types::{
            BinOp, CompiledExpr, ConstraintNodeId, DimensionVector, Type, Value, ValueCellId,
        };

        // thickness > 2mm, thickness = 1mm → violated
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![(ConstraintNodeId::new("Bracket", 0), expr)];
        let violation = compute_total_violation(&constraints, &values);
        assert!(
            violation > 0.0,
            "violated constraint should have positive violation"
        );
    }

    #[test]
    fn compute_violation_multiple_constraints() {
        use super::compute_total_violation;
        use reify_types::{
            BinOp, CompiledExpr, ConstraintNodeId, DimensionVector, Type, Value, ValueCellId,
        };

        // constraint 1: thickness > 2mm (satisfied, thickness=5mm)
        let thickness_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "thickness"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr1 = CompiledExpr::binop(BinOp::Gt, thickness_ref, two_mm, Type::Bool);

        // constraint 2: width > 100mm (violated, width=80mm)
        let width_ref =
            CompiledExpr::value_ref(ValueCellId::new("Bracket", "width"), Type::length());
        let hundred_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.100,
                dimension: DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let expr2 = CompiledExpr::binop(BinOp::Gt, width_ref, hundred_mm, Type::Bool);

        let mut values = ValueMap::new();
        values.insert(
            ValueCellId::new("Bracket", "thickness"),
            Value::Scalar {
                si_value: 0.005,
                dimension: DimensionVector::LENGTH,
            },
        );
        values.insert(
            ValueCellId::new("Bracket", "width"),
            Value::Scalar {
                si_value: 0.080,
                dimension: DimensionVector::LENGTH,
            },
        );

        let constraints = vec![
            (ConstraintNodeId::new("Bracket", 0), expr1),
            (ConstraintNodeId::new("Bracket", 1), expr2),
        ];
        let violation = compute_total_violation(&constraints, &values);
        // Only the violated constraint contributes
        assert!(
            violation > 0.0,
            "should have positive violation from width constraint"
        );
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
