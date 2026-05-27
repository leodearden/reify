//! CP-SAT solver: pure-Rust backtracking constraint solver for logical/discrete constraints.
//!
//! Handles boolean SAT, enum constraints, integer constraints, implications,
//! cardinality, and all-different via forward-checking backtracking search
//! with eval_expr as the constraint checker.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Type, ValueCellId};
use reify_ir::{AutoParam, CompiledExpr, CompiledExprKind, ConstraintSolver, ResolutionProblem, SolveResult, Value, ValueMap};
use std::collections::{HashMap, HashSet};

/// Maximum number of integer domain values to enumerate.
/// If bounds produce a larger range, the solver returns NoProgress.
const MAX_INT_DOMAIN: i64 = 1000;

/// A discrete constraint solver using backtracking search with forward-checking.
///
/// Named CpSatSolver to match the OR-Tools CP-SAT interface from the task spec,
/// but implemented as a pure-Rust backtracking solver suitable for v0.1 problem sizes.
pub struct CpSatSolver;

/// A variable in the backtracking search, with its discrete domain.
struct Variable {
    id: ValueCellId,
    domain: Vec<Value>,
}

/// Collect all ValueCellId references from a constraint expression.
fn collect_constraint_refs(expr: &CompiledExpr) -> HashSet<ValueCellId> {
    let mut refs = HashSet::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::ValueRef(id) = &node.kind {
            refs.insert(id.clone());
        }
    });
    refs
}

/// Build the domain for a single auto param based on its type.
/// For Bool: {true, false}
/// For Int: enumerate lo..=hi from bounds (capped at MAX_INT_DOMAIN)
/// For Enum: extract variant literals from constraints
fn build_variable_domain(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Result<Vec<Value>, String> {
    match &param.param_type {
        Type::Bool => Ok(vec![Value::Bool(true), Value::Bool(false)]),
        Type::Int => {
            if let Some((lo, hi)) = param.bounds {
                // Validate bounds are finite (rejects infinity and NaN)
                if !lo.is_finite() || !hi.is_finite() {
                    return Err(format!(
                        "integer auto param {} has non-finite bounds [{}, {}]",
                        param.id, lo, hi
                    ));
                }
                // Validate bounds are representable as i64 (i64::MAX ≈ 9.22e18)
                const I64_MIN_F: f64 = i64::MIN as f64;
                const I64_MAX_F: f64 = i64::MAX as f64;
                let i64_range = I64_MIN_F..=I64_MAX_F;
                if !i64_range.contains(&lo) || !i64_range.contains(&hi) {
                    return Err(format!(
                        "integer auto param {} bounds [{}, {}] exceed i64 range",
                        param.id, lo, hi
                    ));
                }
                let lo_i = lo as i64;
                let hi_i = hi as i64;
                // Use checked arithmetic to prevent overflow
                let size = hi_i
                    .checked_sub(lo_i)
                    .and_then(|d| d.checked_add(1))
                    .unwrap_or(i64::MAX);
                if size > MAX_INT_DOMAIN || size <= 0 {
                    return Err(format!(
                        "integer domain for {} too large: [{}, {}] has {} values (max {})",
                        param.id, lo_i, hi_i, size, MAX_INT_DOMAIN
                    ));
                }
                Ok((lo_i..=hi_i).map(Value::Int).collect())
            } else {
                Err(format!(
                    "integer auto param {} has no bounds; cannot enumerate domain",
                    param.id
                ))
            }
        }
        Type::Enum(type_name) => {
            // Scan constraint expressions for Value::Enum literals with matching type_name
            let mut variants = Vec::new();
            let mut seen = HashSet::new();
            for (_, expr) in constraints {
                expr.walk(&mut |node| {
                    if let CompiledExprKind::Literal(Value::Enum {
                        type_name: tn,
                        variant,
                    }) = &node.kind
                        && tn == type_name
                        && seen.insert(variant.clone())
                    {
                        variants.push(Value::Enum {
                            type_name: tn.clone(),
                            variant: variant.clone(),
                        });
                    }
                });
            }
            if variants.is_empty() {
                return Err(format!(
                    "enum auto param {} (type {}) has no variant literals in constraints",
                    param.id, type_name
                ));
            }
            Ok(variants)
        }
        other => Err(format!(
            "CpSatSolver does not support param type {:?} for {}",
            other, param.id
        )),
    }
}

/// Recursive backtracking search with forward-checking.
///
/// At each level, picks the next unassigned variable, tries each domain value,
/// evaluates all constraints whose variables are fully assigned, and prunes
/// on violation.
fn backtrack(
    variables: &[Variable],
    var_index: usize,
    assignment: &mut ValueMap,
    constraints: &[(ConstraintNodeId, CompiledExpr, HashSet<ValueCellId>)],
    auto_param_ids: &HashSet<ValueCellId>,
    functions: &[reify_ir::CompiledFunction],
) -> Option<HashMap<ValueCellId, Value>> {
    // Base case: all variables assigned
    if var_index >= variables.len() {
        // Extract solution
        let mut solution = HashMap::new();
        for var in variables {
            if let Some(val) = assignment.get(&var.id).cloned() {
                solution.insert(var.id.clone(), val);
            }
        }
        return Some(solution);
    }

    let var = &variables[var_index];

    for value in &var.domain {
        // Assign this variable
        assignment.insert(var.id.clone(), value.clone());

        // Forward-check: evaluate all constraints whose auto-param refs are fully assigned
        let mut feasible = true;
        for (_, expr, refs) in constraints {
            // Only check constraints where ALL referenced auto params have been assigned
            let auto_refs: Vec<_> = refs.iter().filter(|r| auto_param_ids.contains(r)).collect();
            let all_assigned = auto_refs.iter().all(|r| assignment.get(r).is_some());
            if !all_assigned {
                continue;
            }

            let ctx = EvalContext::new(assignment, functions);
            let result = eval_expr(expr, &ctx);
            match result {
                Value::Bool(true) => {} // satisfied, continue
                Value::Bool(false) => {
                    feasible = false;
                    break;
                }
                _ => {
                    // Indeterminate or non-boolean — skip (don't prune)
                }
            }
        }

        if feasible
            && let Some(solution) = backtrack(
                variables,
                var_index + 1,
                assignment,
                constraints,
                auto_param_ids,
                functions,
            )
        {
            return Some(solution);
        }
    }

    // Undo assignment (remove from map)
    assignment.remove(&var.id);
    None
}

impl ConstraintSolver for CpSatSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // Fast path: no auto params → already solved
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        // Build variable domains
        let mut variables = Vec::with_capacity(problem.auto_params.len());
        for param in &problem.auto_params {
            match build_variable_domain(param, &problem.constraints) {
                Ok(domain) => variables.push(Variable {
                    id: param.id.clone(),
                    domain,
                }),
                Err(reason) => return SolveResult::NoProgress { reason },
            }
        }

        // Collect auto param IDs for forward-checking
        let auto_param_ids: HashSet<ValueCellId> =
            problem.auto_params.iter().map(|ap| ap.id.clone()).collect();

        // Pre-compute constraint refs
        let constraints_with_refs: Vec<_> = problem
            .constraints
            .iter()
            .map(|(id, expr)| (id.clone(), expr.clone(), collect_constraint_refs(expr)))
            .collect();

        // Initialize assignment with current_values (for non-auto-param refs)
        let mut assignment = problem.current_values.clone();

        // Run backtracking search
        match backtrack(
            &variables,
            0,
            &mut assignment,
            &constraints_with_refs,
            &auto_param_ids,
            &problem.functions,
        ) {
            Some(solution) => SolveResult::Solved {
                values: solution,
                unique: true,
            },
            None => SolveResult::Infeasible {
                diagnostics: vec![Diagnostic::error(format!(
                    "CpSatSolver: no satisfying assignment found for {} auto params with {} constraints",
                    problem.auto_params.len(),
                    problem.constraints.len()
                ))
                .with_code(DiagnosticCode::ConstraintUnsatisfiable)],
            },
        }
    }
}
