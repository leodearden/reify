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
                        ..
                    }) = &node.kind
                        && tn == type_name
                        && seen.insert(variant.clone())
                    {
                        variants.push(Value::enum_unit(tn.clone(), variant.clone()));
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
/// materialises `dependent_cells` against that trial assignment, evaluates all
/// constraints whose variables are fully assigned, and prunes on violation.
///
/// # Why the fold is inside the value loop (task #5467, PRD2 §3 decision 9)
///
/// Without it, a constraint that reads ONLY a dependent cell has an EMPTY
/// `auto_refs`, so `all_assigned` is vacuously true and the constraint is
/// evaluated against that cell's stale/absent base value. `eval_expr` returns a
/// non-`Bool`, the skip-don't-prune arm fires, and the search prunes nothing.
///
/// # Why no explicit unwind is needed
///
/// The fold is TOTAL — it recomputes every dependent cell from the running
/// assignment — and runs after EVERY trial insert, before any constraint is
/// evaluated. So entries left behind by an abandoned sibling branch are always
/// overwritten before they can be read: a cell whose expression reads a
/// not-yet-assigned deeper auto simply re-evaluates to `Undef` here rather than
/// retaining the abandoned branch's value. `assignment.remove` on unwind
/// therefore only has to drop the variable itself. The
/// `two_autos_*_abandoned_sibling_branch` unit below pins this, because the
/// argument is not obvious from the code alone.
fn backtrack(
    variables: &[Variable],
    var_index: usize,
    assignment: &mut ValueMap,
    constraints: &[(ConstraintNodeId, CompiledExpr, HashSet<ValueCellId>)],
    auto_param_ids: &HashSet<ValueCellId>,
    functions: &[reify_ir::CompiledFunction],
    dependent_cells: &[(ValueCellId, CompiledExpr)],
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

        // Materialise the dependent cells against this trial assignment, in
        // STORED (topological) order, through THE fold body the DimensionalSolver
        // residual path uses — never a cpsat-local twin (PRD2 §3.9 G7).
        // `is_solver_owned` keeps a fold from clobbering a trial auto. An empty
        // `dependent_cells` early-returns inside the helper without touching
        // `assignment` or running any guard work, so the D1/B2 path is
        // byte-identical to pre-α.
        crate::solver::fold_dependent_cells(assignment, dependent_cells, functions, |id| {
            auto_param_ids.contains(id)
        });

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
                dependent_cells,
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
            &problem.dependent_cells,
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

// ---------------------------------------------------------------------------
// LAYER 3 remainder — the CP-SAT forward-check has no per-trial dependent-cell
// fold (task #5467 / PRD2 α, §3 decision 9, step-10 RED).
//
// `backtrack` computes `auto_refs = refs ∩ auto_param_ids`. For a constraint
// that reads ONLY a dependent cell that set is EMPTY, so `all_assigned` is
// VACUOUSLY true and the constraint IS evaluated — against the stale/absent
// base value of that cell. `eval_expr` returns a non-`Bool`, the
// `_ => // Indeterminate — skip (don't prune)` arm fires, and nothing is ever
// pruned. `solve` therefore returns the FIRST domain value regardless, and
// `build_variable_domain` yields `[Bool(true), Bool(false)]` for `Type::Bool`,
// so "first" is deterministically `true`.
//
// CP-SAT is landed-but-unwired (unreachable in production until PRD2 γ), so
// these units are the ONLY behavioural pin on this half — they assert the
// WRONG VALUE loudly rather than merely "did not panic".
// ---------------------------------------------------------------------------
#[cfg(test)]
mod dependent_cell_fold_tests {
    use super::*;
    use reify_ir::{ObjectiveSet, UnOp};
    use std::sync::Arc;

    fn bool_auto(member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Bool,
            bounds: None,
            free: true,
        }
    }

    fn bref(member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("S", member), Type::Bool)
    }

    fn not(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::unop(UnOp::Not, e, Type::Bool)
    }

    fn eq_true(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Eq,
            e,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            Type::Bool,
        )
    }

    fn problem(
        auto_params: Vec<AutoParam>,
        constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
        dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
    ) -> ResolutionProblem {
        ResolutionProblem {
            auto_params,
            constraints,
            current_values: ValueMap::new(),
            objective: None::<ObjectiveSet>,
            functions: Arc::from(Vec::new()),
            dependent_cells,
        }
    }

    /// The solved value of `S.<member>`, or a panic naming what actually came
    /// back — an unpruned search reports `Solved`/`unique: true` with the WRONG
    /// value, so a test that only checked the variant would pass on the bug.
    fn solved_value(result: &SolveResult, member: &str) -> Value {
        match result {
            SolveResult::Solved { values, .. } => values
                .get(&ValueCellId::new("S", member))
                .unwrap_or_else(|| {
                    panic!("no solved value for S.{member}; got values {values:?}")
                })
                .clone(),
            other => panic!("expected SolveResult::Solved for S.{member}; got {other:?}"),
        }
    }

    /// (a) LET-INDIRECTED PRUNING — a constraint reading ONLY a dependent cell
    /// must still prune the domain of the auto that cell is derived from.
    ///
    /// `let f = not(up)`, `constraint f == true`. The unique satisfying
    /// assignment is `up = false`.
    ///
    /// RED today: `backtrack` never materialises `S.f`, so `eval_expr` sees an
    /// absent cell, returns a non-`Bool`, takes the skip-don't-prune arm, and
    /// `solve` hands back the first domain value `Bool(true)` — reported as
    /// `Solved` with `unique: true`, i.e. SILENTLY WRONG rather than loudly
    /// unsolved.
    #[test]
    fn a_constraint_reading_only_a_dependent_cell_prunes_the_auto_it_derives_from() {
        let p = problem(
            vec![bool_auto("up")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("f")))],
            vec![(ValueCellId::new("S", "f"), not(bref("up")))],
        );

        let got = solved_value(&CpSatSolver.solve(&p), "up");

        assert_eq!(
            got,
            Value::Bool(false),
            "`constraint f == true` over `let f = not(up)` is satisfied ONLY by \
             up = false. Without a per-trial fold of `dependent_cells`, the \
             forward-check evaluates the constraint against an absent `S.f`, \
             takes the skip-don't-prune arm, and returns the FIRST domain \
             value (`true`) as a `Solved`/`unique` answer",
        );
    }

    /// UNWIND SAFETY — a dependent value left behind by an ABANDONED sibling
    /// branch must never be observed by the branch that follows it.
    ///
    /// Two autos, `up` and `dn`. `let f = not(up) and dn`,
    /// `constraint f == true`, `constraint dn == true` — unique satisfying
    /// assignment `up = false, dn = true`.
    ///
    /// `Type::Bool` domains enumerate `[true, false]`, so the search tries
    /// `up = true` FIRST. That branch folds `S.f` to `false` and is pruned,
    /// leaving `S.f = false` behind in the assignment map. The sibling
    /// `up = false` branch then re-folds `S.f` before the forward-check reads
    /// it. Discriminating in BOTH directions: with no fold at all the
    /// constraint never prunes and `up` comes back as the first domain value
    /// `true`; with a fold that is not re-run per trial, the stale `S.f =
    /// false` prunes the correct branch too and the solve returns Infeasible.
    ///
    /// This is what pins `backtrack`'s claim that no explicit undo of the
    /// folded entries is required.
    #[test]
    fn two_autos_do_not_observe_a_stale_dependent_value_from_an_abandoned_sibling_branch() {
        let and = |l: CompiledExpr, r: CompiledExpr| {
            CompiledExpr::binop(reify_ir::BinOp::And, l, r, Type::Bool)
        };
        let p = problem(
            vec![bool_auto("up"), bool_auto("dn")],
            vec![
                (ConstraintNodeId::new("S", 0), eq_true(bref("f"))),
                (ConstraintNodeId::new("S", 1), eq_true(bref("dn"))),
            ],
            vec![(
                ValueCellId::new("S", "f"),
                and(not(bref("up")), bref("dn")),
            )],
        );

        let result = CpSatSolver.solve(&p);
        assert_eq!(
            solved_value(&result, "up"),
            Value::Bool(false),
            "`f = not(up) and dn` with `f == true` forces up = false. Getting \
             `true` means the let-indirected constraint never pruned; getting \
             Infeasible means the abandoned `up = true` branch's stale `S.f` \
             was read instead of being re-folded",
        );
        assert_eq!(
            solved_value(&result, "dn"),
            Value::Bool(true),
            "`f = not(up) and dn` with `f == true` forces dn = true",
        );
    }

    /// (b) D1/B2 IDENTITY, negative half — the SAME logical problem written
    /// with the auto read DIRECTLY and an EMPTY `dependent_cells` still solves
    /// to `up = false`.
    #[test]
    fn a_direct_constraint_with_no_dependent_cells_still_prunes_to_false() {
        let p = problem(
            vec![bool_auto("up")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(not(bref("up"))))],
            Vec::new(),
        );

        assert_eq!(
            solved_value(&CpSatSolver.solve(&p), "up"),
            Value::Bool(false),
            "`constraint not(up) == true` reads the auto DIRECTLY, so it prunes \
             today and must keep pruning — the fold must not perturb the \
             empty-`dependent_cells` path",
        );
    }

    /// (b) D1/B2 IDENTITY, positive half — a direct-only problem whose answer
    /// is `true` still returns `true`.
    ///
    /// Paired with the negative half deliberately: alone, either one could pass
    /// on a solver that always returned the first domain value.
    #[test]
    fn a_direct_constraint_with_no_dependent_cells_still_solves_to_true() {
        let p = problem(
            vec![bool_auto("up")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("up")))],
            Vec::new(),
        );

        assert_eq!(
            solved_value(&CpSatSolver.solve(&p), "up"),
            Value::Bool(true),
            "`constraint up == true` is satisfied only by up = true",
        );
    }
}
