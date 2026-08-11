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
///
/// That `Undef` re-evaluation is TRUE ONLY GIVEN THE STRIPPED SEED, and the
/// guarantor is named deliberately (task #5467): `solve` removes every auto id
/// from the `current_values` seed before the search starts (see the seed site
/// there). A deeper auto is therefore genuinely ABSENT, not holding a stale
/// value carried in from a previous resolution round or an earlier
/// lexicographic stage. WITHOUT that strip this fold is not self-correcting:
/// it materialises dependent cells from a mix of trial and stale values, and
/// the `Bool(false)` arm below PRUNES A FEASIBLE BRANCH. The same stale seed
/// also defeats `all_assigned` on the direct path, with no dependent cell
/// involved at all — which is why the repair belongs at the seed and not in
/// `fold_dependent_cells`. The `*_stale_*` units below pin both arms.
///
/// # Cost of the TOTAL fold, and why the obvious saving is unsound as stated
///
/// The fold runs on EVERY trial value at EVERY depth, so it costs
/// `O(|dependent_cells| · Π|domain_i|)` expression evaluations plus one
/// persistent-map insert each — and a `Type::Int` domain runs to
/// `MAX_INT_DOMAIN` = 1000 values, so the multiplier is not academic. The
/// obvious saving is to hand `backtrack` each cell's transitive auto set
/// (`decompose::dependent_cell_auto_reads`, which `SolverRegistry::solve_inner`
/// already builds once per solve) and SKIP a cell at a depth where any of its
/// autos is still unassigned — the folded value would be `Undef` there anyway,
/// and `get_or_undef` treats absent and `Undef` alike.
///
/// SKIPPING IS UNSOUND, and the correction is not obvious from the sketch: a
/// cell skipped at depth k is not ABSENT — it still holds whatever an ABANDONED
/// DEEPER branch folded into it, which is exactly the value the total fold
/// overwrites with `Undef` and which
/// `two_autos_do_not_observe_a_stale_dependent_value_from_an_abandoned_sibling_branch`
/// exists to pin. A sound version must `remove` the unfoldable cell, not skip
/// it. The saving survives that correction (an O(1) map op in place of an
/// expression eval), but the `remove` is mandatory, not an optimisation detail.
///
/// Not done here: CP-SAT is landed-but-unwired — unreachable in production
/// until PRD2 γ — so nothing pays this cost yet, and the change needs its own
/// unwind-safety units rather than a rider on an amendment pass.
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

        // Initialize assignment with current_values (for non-auto-param refs).
        //
        // The parenthetical is load-bearing, so it is ENFORCED rather than
        // merely intended (task #5467): every auto id is stripped back out
        // below. `current_values` is the engine's whole value map
        // (`values.clone()`, engine_eval.rs:2326) and DOES carry same-scope
        // auto entries, so without the strip an unassigned auto holds a STALE
        // CONCRETE value instead of being absent — and `backtrack`'s
        // forward-check then prunes a feasible branch off a value the search
        // had not chosen yet. Three real sources, (iii) first because it needs
        // no eval layer at all and so is reachable purely inside this crate:
        //
        //  (iii) `solve_lexicographic` (registry.rs:668,717-720) warm-starts
        //        stage N+1 from stage N's solution INSIDE one `solve()` call.
        //  (i)   A second `eval_cached` at the same `VersionId` serves a
        //        previously-SOLVED auto straight back from cache into `values`
        //        (engine_eval.rs:6949-6962), which is then cloned to here.
        //  (ii)  A `param_override` on an auto cell (engine_eval.rs:7075-7080),
        //        written as `Determined` yet still admitted to `auto_params`.
        //
        // (ii) is NOT staleness and is not described as such: it is a user's
        // explicit pin, and stripping it means CP-SAT searches that auto's whole
        // domain and can answer with a value the override did not name. It is
        // stripped anyway because that is what the only production-reachable
        // solver already does — `solver.rs`'s `build_trial_values` clones
        // `current_values` and OVERWRITES every auto id in it at every trial
        // point, so `DimensionalSolver` ignores such an override too. Honouring
        // it here and nowhere else would make the two solvers disagree about the
        // same model. Pinned by
        // `an_overridden_auto_is_searched_rather_than_pinned_to_its_seed`.
        // Whether an overridden auto belongs in `auto_params` AT ALL is the real
        // upstream question and lives in `build_auto_param_list`.
        //
        // Strip ONLY the auto ids: `current_values` is the sole channel by
        // which a CP-SAT constraint sees a NON-auto base value (pinned
        // connector autos land there at engine_eval.rs:2327), so starting from
        // an empty map instead would silently break every such model.
        //
        // Clone-then-`remove` rather than a filtered rebuild: `ValueMap`
        // (reify-ir/src/value.rs) wraps a persistent `im::HashMap`, so `Clone`
        // is an O(1) structural share and this loop is O(#auto_params) —
        // cheaper than an O(|current_values|) rebuild, and `ValueMap` has no
        // `FromIterator` impl to `collect` into anyway.
        let mut assignment = problem.current_values.clone();
        for id in &auto_param_ids {
            assignment.remove(id);
        }

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
// REGRESSION LOCKS for the CP-SAT forward-check's two dependent-cell hazards
// (task #5467 / PRD2 α, §3 decision 9). Both are FIXED above; these units are
// what keeps them fixed. CP-SAT is landed-but-unwired — unreachable in
// production until PRD2 γ — so this module is the ONLY behavioural pin on
// either, which is why every assertion names an expected VALUE or VARIANT
// rather than settling for "did not panic".
//
// LOCK 1 — the per-trial fold at the top of `backtrack`'s value loop.
// `backtrack` computes `auto_refs = refs ∩ auto_param_ids`. For a constraint
// that reads ONLY a dependent cell that set is EMPTY, so `all_assigned` is
// VACUOUSLY true and the constraint IS evaluated. Before the fold it was
// evaluated against that cell's stale/absent base value: `eval_expr` returned a
// non-`Bool`, the `_ => // Indeterminate — skip (don't prune)` arm fired, and
// nothing was ever pruned. `solve` then returned the FIRST domain value
// regardless — `[Bool(true), Bool(false)]` for `Type::Bool`, i.e.
// deterministically `true`, reported as `Solved`/`unique` rather than as a
// failure. Removing the fold reintroduces exactly that.
//
// LOCK 2 — the auto-id strip on the `current_values` seed in `solve`. Before
// it, `assignment` started out carrying entries for auto params, so at depth k
// the autos k+1..n held STALE CONCRETE values instead of being absent. The
// forward-check then took the `Bool(false)` PRUNE arm on a FEASIBLE branch,
// turning a satisfiable problem into `Infeasible` — wrong in the loud
// direction, but wrong. Three real sources of such a stale entry, (iii) first
// because it needs no eval layer at all and so is reachable purely inside
// reify-constraints:
//
//  (iii) `registry.rs:668,717-720` — lexicographic multi-rank solving clones
//        `base.current_values` once and then WARM-STARTS stage N+1 from stage
//        N's solution INSIDE a single `solve()` call.
//  (i)   `engine_eval.rs:6949-6962` — a second `eval_cached` at the same
//        `VersionId` serves a previously-SOLVED auto from cache straight back
//        into `values`, which `build_solver_problem` clones wholesale into
//        `current_values` at engine_eval.rs:2326.
//  (ii)  `engine_eval.rs:7075-7080` — a `param_override` on an auto cell, which
//        is written as `Determined` yet is still admitted to `auto_params`.
//        The odd one out: a deliberate user pin, not staleness. It is stripped
//        anyway, for consistency with `DimensionalSolver`, and
//        `an_overridden_auto_is_searched_rather_than_pinned_to_its_seed` pins
//        that choice explicitly.
//
// The two locks are ONE module because they share a fixture vocabulary and
// because lock 2's `*_direct_path_*` unit is what proves lock 2's repair
// belongs at the seed rather than inside the fold — an argument only legible
// next to lock 1's fold units.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod dependent_cell_forward_check_tests {
    use super::*;
    use reify_ir::{ObjectiveSet, UnOp};
    use std::sync::Arc;

    /// The value a stale same-scope auto entry carries into the search.
    ///
    /// Named rather than inlined — mirroring `STALE_SIDE` in
    /// `tests/joint_drive_per_trial_recompute.rs:998` — so a mixed-up assertion
    /// cannot pass by coincidence: every lock-2 fixture is built so that the
    /// STALE answer and the CORRECT answer are opposites, and naming the stale
    /// side makes that opposition explicit at the call site.
    ///
    /// A real one arrives via any of the three sources named in this module's
    /// header; `registry.rs:717-720`'s lexicographic warm-start is the one that
    /// needs no eval layer at all.
    const STALE_SEED: bool = true;

    fn bool_auto(member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Bool,
            bounds: None,
            free: true,
        }
    }

    /// An `Int` auto over the INCLUSIVE bound pair `[lo, hi]` —
    /// `build_variable_domain` enumerates `lo..=hi`, so the domain has
    /// `hi - lo + 1` values.
    fn int_auto(member: &str, lo: i64, hi: i64) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Int,
            bounds: Some((lo as f64, hi as f64)),
            free: true,
        }
    }

    fn bref(member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("S", member), Type::Bool)
    }

    fn iref(member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("S", member), Type::Int)
    }

    fn not(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::unop(UnOp::Not, e, Type::Bool)
    }

    fn and(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(reify_ir::BinOp::And, l, r, Type::Bool)
    }

    fn eq_true(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Eq,
            e,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            Type::Bool,
        )
    }

    fn mul_int(e: CompiledExpr, k: i64) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Mul,
            e,
            CompiledExpr::literal(Value::Int(k), Type::Int),
            Type::Int,
        )
    }

    fn add_int(e: CompiledExpr, k: i64) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Add,
            e,
            CompiledExpr::literal(Value::Int(k), Type::Int),
            Type::Int,
        )
    }

    fn eq_int(e: CompiledExpr, k: i64) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Eq,
            e,
            CompiledExpr::literal(Value::Int(k), Type::Int),
            Type::Bool,
        )
    }

    /// A `ValueMap` seeded with exactly one `S.<member>` entry — every lock-2
    /// fixture needs precisely one, and keeping it to one makes the thing under
    /// test unmistakable at the call site.
    ///
    /// `ValueMap` has no `FromIterator` impl (reify-ir/src/value.rs), so this
    /// is `new` + `insert` rather than a `collect`.
    fn seed(member: &str, v: Value) -> ValueMap {
        let mut m = ValueMap::new();
        m.insert(ValueCellId::new("S", member), v);
        m
    }

    /// A problem with the `current_values` seed exposed — that seed IS the
    /// subject under test for lock 2, so it cannot be hard-wired the way
    /// [`problem`] wires it.
    fn problem_with_seed(
        auto_params: Vec<AutoParam>,
        constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
        dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
        current_values: ValueMap,
    ) -> ResolutionProblem {
        ResolutionProblem {
            auto_params,
            constraints,
            current_values,
            objective: None::<ObjectiveSet>,
            functions: Arc::from(Vec::new()),
            dependent_cells,
        }
    }

    /// [`problem_with_seed`] with an EMPTY seed — the shape every lock-1
    /// fixture wants, and the shape all 11 `ResolutionProblem` literals in
    /// `tests/cpsat_tests.rs` build.
    fn problem(
        auto_params: Vec<AutoParam>,
        constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
        dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
    ) -> ResolutionProblem {
        problem_with_seed(auto_params, constraints, dependent_cells, ValueMap::new())
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

    /// LOCK 1 — LET-INDIRECTED PRUNING. A constraint reading ONLY a dependent
    /// cell must still prune the domain of the auto that cell is derived from.
    ///
    /// `let f = not(up)`, `constraint f == true`. The unique satisfying
    /// assignment is `up = false`.
    ///
    /// Without the per-trial fold `backtrack` never materialises `S.f`, so
    /// `eval_expr` sees an absent cell, returns a non-`Bool`, takes the
    /// skip-don't-prune arm, and `solve` hands back the first domain value
    /// `Bool(true)` — reported as `Solved` with `unique: true`, i.e. SILENTLY
    /// WRONG rather than loudly unsolved.
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

    /// LOCK 1 — UNWIND SAFETY. A dependent value left behind by an ABANDONED
    /// sibling branch must never be observed by the branch that follows it.
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

    /// LOCK 1 — D1/B2 IDENTITY, negative half. The SAME logical problem written
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

    /// LOCK 1 — D1/B2 IDENTITY, positive half. A direct-only problem whose
    /// answer is `true` still returns `true`.
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

    /// LOCK 1 — a MULTI-VALUE domain, so the fold is exercised across several
    /// abandoned branches rather than the single one a `Type::Bool` domain
    /// affords.
    ///
    /// `n : Int` bounded `[0, 5]` (`build_variable_domain` enumerates
    /// `Int(0) ..= Int(5)`), `let f = n * 2`, `constraint f == 6`. The unique
    /// satisfying assignment is `n = 3` — neither the first nor the last domain
    /// value, and three trials deep, so a search that pruned for the wrong
    /// reason cannot land on it by accident the way it can on a two-value
    /// domain. Without the per-trial fold `S.f` is absent, `eval_expr` returns
    /// a non-`Bool`, the skip-don't-prune arm fires, and `solve` returns the
    /// first domain value `Int(0)`.
    ///
    /// The unsatisfiable half (`f == 7`, unreachable because `n * 2` is even
    /// over the whole domain) is what proves the fold is EVALUATED rather than
    /// merely inserted: it forces every one of the six trials to be folded and
    /// rejected on its own merits.
    #[test]
    fn an_int_domain_dependent_cell_prunes_across_multiple_abandoned_branches() {
        let build = |target: i64| {
            problem(
                vec![int_auto("n", 0, 5)],
                vec![(ConstraintNodeId::new("S", 0), eq_int(iref("f"), target))],
                vec![(ValueCellId::new("S", "f"), mul_int(iref("n"), 2))],
            )
        };

        assert_eq!(
            solved_value(&CpSatSolver.solve(&build(6)), "n"),
            Value::Int(3),
            "`let f = n * 2` with `constraint f == 6` over n ∈ [0, 5] is \
             satisfied ONLY by n = 3. Without a per-trial fold of \
             `dependent_cells` the forward-check evaluates against an absent \
             `S.f`, takes the skip-don't-prune arm, and returns the FIRST \
             domain value `Int(0)` as a `Solved`/`unique` answer",
        );

        let odd = CpSatSolver.solve(&build(7));
        assert!(
            matches!(odd, SolveResult::Infeasible { .. }),
            "`n * 2` is even for every n ∈ [0, 5], so `constraint f == 7` is \
             UNSATISFIABLE. Getting `Solved` means the fold never produced a \
             `Bool` the forward-check could act on and the search fell through \
             to its first domain value; got {odd:?}",
        );
    }

    /// LOCK 1 — STORED-ORDER DEPENDENCE, the one property the fold's
    /// correctness argument rests on and that no other unit here exercises.
    ///
    /// Every other lock-1 fixture holds exactly ONE dependent cell, so a
    /// regression that reordered `dependent_cells` — or hoisted the
    /// `EvalContext` out of `fold_dependent_cells`' loop so a later cell no
    /// longer sees an earlier one — would leave them all green. `solver.rs`'s
    /// fold consumes the list in `build_dependent_cells`' STORED topological
    /// order with the context rebuilt against the RUNNING map each iteration
    /// (solver.rs:222-224, 262-266); this is the CP-SAT-side pin on that.
    ///
    /// `n : Int` bounded `[0, 5]`; CHAINED cells `let f = n * 2` then
    /// `let g = f + 1`, stored in that order; `constraint g == 7` plus the
    /// redundant-but-load-bearing `constraint f == 6`. `g = 2n + 1`, so the
    /// unique satisfying assignment is `n = 3`.
    ///
    /// Discriminates in THREE directions, each landing on a DIFFERENT observable
    /// — all three verified by mutating the code, not reasoned about:
    ///
    /// * NO FOLD at all → `S.f`/`S.g` are absent at every trial, both
    ///   constraints evaluate to a non-`Bool`, the skip-don't-prune arm fires,
    ///   and the FIRST trial is accepted whole → `Int(0)`.
    /// * REVERSED stored order (or an `EvalContext` hoisted out of the fold
    ///   loop) → `g` reads the PREVIOUS trial's `f` and lags one step, so `n = 3`
    ///   sees `f = 6` but the stale `g = 5` and is wrongly PRUNED, while `n = 4`
    ///   sees `g = 7` but `f = 8` and is pruned by the second constraint →
    ///   `Infeasible`.
    /// * CORRECT → `Int(3)`.
    ///
    /// `constraint f == 6` is what separates the reversed case from the no-fold
    /// case. Without it, reversal leaves `g` `Undef` at `n = 0`, the
    /// skip-don't-prune arm accepts that first trial outright, and reversal
    /// reports `Int(0)` — still red, but indistinguishable from no fold at all.
    /// A 6-value `Int` domain rather than a 2-value `Bool` one for the same
    /// reason: on two values a lagged read can coincide with the correct one.
    #[test]
    fn chained_dependent_cells_are_folded_in_stored_order_within_one_trial() {
        let p = problem(
            vec![int_auto("n", 0, 5)],
            vec![
                (ConstraintNodeId::new("S", 0), eq_int(iref("g"), 7)),
                (ConstraintNodeId::new("S", 1), eq_int(iref("f"), 6)),
            ],
            vec![
                (ValueCellId::new("S", "f"), mul_int(iref("n"), 2)),
                (ValueCellId::new("S", "g"), add_int(iref("f"), 1)),
            ],
        );

        assert_eq!(
            solved_value(&CpSatSolver.solve(&p), "n"),
            Value::Int(3),
            "`let f = n * 2; let g = f + 1` with `constraint g == 7` gives \
             g = 2n + 1, satisfied ONLY by n = 3. `Int(0)` means the fold never \
             ran at all; `Infeasible` means `g` was folded BEFORE `f` (or \
             against an `EvalContext` hoisted out of the fold loop) and read the \
             previous trial's `f`, which prunes the correct answer",
        );
    }

    /// LOCK 2 — SCOPE BOUNDARY: an auto carrying a `param_override` is SEARCHED,
    /// not pinned, and that is deliberate rather than an accident of the strip.
    ///
    /// Source (ii) in this module's header is the odd one out: unlike the
    /// lexicographic warm start and the `eval_cached` replay, a `param_override`
    /// on an auto cell is a user's explicit pin, written as
    /// `DeterminacyState::Determined` (engine_eval.rs:7075-7080). Stripping it
    /// from the seed means CP-SAT searches that auto's whole domain and can
    /// return a value the override did not name.
    ///
    /// That is nonetheless the RIGHT choice here, because it is the behaviour
    /// the only production-reachable solver already has: `build_auto_param_list`
    /// admits an overridden auto to `auto_params` unchanged, and
    /// `solver.rs`'s `build_trial_values` then clones `current_values` and
    /// OVERWRITES every auto id in it at every trial point — so
    /// `DimensionalSolver` ignores such an override too. Keeping the override in
    /// the CP-SAT seed would make the two solvers disagree about the same model.
    ///
    /// Whether an overridden auto should be excluded from `auto_params` OUTRIGHT
    /// (expressing the pin as "not an auto to solve") is the real upstream
    /// question. It belongs to `build_auto_param_list`/`build_solver_problem` and
    /// changes BOTH solvers' semantics, so it is filed as follow-up work rather
    /// than decided on an amendment pass. This unit pins today's answer so the
    /// choice is explicit and a future change to it is loud.
    #[test]
    fn an_overridden_auto_is_searched_rather_than_pinned_to_its_seed() {
        // `S.a` carries the "override" and NO constraint mentions it; `S.b` is
        // pinned by a constraint purely so the problem is not vacuous.
        let p = problem_with_seed(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("b")))],
            Vec::new(),
            seed("a", Value::Bool(false)),
        );

        let result = CpSatSolver.solve(&p);
        assert_eq!(
            solved_value(&result, "a"),
            Value::Bool(true),
            "`S.a` is unconstrained, so the search returns its FIRST domain \
             value `Bool(true)` — NOT the `Bool(false)` its `current_values` \
             entry named. Getting `false` means the seed survived the strip and \
             CP-SAT now honours a `param_override` on an auto that \
             `DimensionalSolver` (via `build_trial_values`) does not",
        );
        assert_eq!(
            solved_value(&result, "b"),
            Value::Bool(true),
            "fixture integrity: the constrained auto must still solve, or the \
             assertion above could pass on a solver that failed outright",
        );
    }

    /// LOCK 2 — STALE DEEPER AUTO REACHED THROUGH A DEPENDENT CELL.
    ///
    /// Two `Bool` autos in `auto_params` order `[S.a, S.b]`; `current_values`
    /// seeds ONLY `S.b`, with the stale value; `let f = not(b)`;
    /// `constraint f == true`. The unique satisfying assignment is
    /// `b = false` (`S.a` is unconstrained and comes back as its first domain
    /// value).
    ///
    /// `collect_constraint_refs` gives `{S.f}`, whose intersection with
    /// `auto_param_ids` is EMPTY, so `all_assigned` is VACUOUSLY true and the
    /// constraint is evaluated at depth 0 — where `S.f` has already folded to
    /// `not(STALE_SEED) = false` off the stale `S.b`. Both `S.a` branches
    /// therefore prune before the search ever descends to `S.b`. Removing the
    /// strip turns this satisfiable problem back into
    /// `Infeasible { ConstraintUnsatisfiable }`.
    #[test]
    fn stale_deeper_auto_behind_a_dependent_cell_must_not_prune_a_feasible_branch() {
        let p = problem_with_seed(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("f")))],
            vec![(ValueCellId::new("S", "f"), not(bref("b")))],
            seed("b", Value::Bool(STALE_SEED)),
        );

        assert_eq!(
            solved_value(&CpSatSolver.solve(&p), "b"),
            Value::Bool(!STALE_SEED),
            "`let f = not(b)` with `constraint f == true` is satisfied ONLY by \
             b = false. The stale `S.b = {STALE_SEED}` seed makes the depth-0 \
             fold materialise `S.f = false`, so the forward-check prunes both \
             `S.a` branches and the search never reaches `S.b` at all — a \
             feasible problem reported Infeasible",
        );
    }

    /// LOCK 2 — STALE AUTO ON THE DIRECT PATH, the same hazard with NO
    /// dependent cells at all.
    ///
    /// Same two autos and the same stale `S.b` seed, but `dependent_cells` is
    /// EMPTY and the single constraint reads the auto DIRECTLY:
    /// `not(b) == true`. Here `auto_refs = {S.b}` is non-empty, but
    /// `all_assigned` tests `assignment.get(r).is_some()` — and the stale seed
    /// makes that prematurely TRUE at depth 0. The constraint is evaluated
    /// against the stale value and prunes both `S.a` branches.
    ///
    /// LOAD-BEARING for the repair's SHAPE: this proves the defect is not
    /// dependent-cell-specific and therefore belongs at the SEED, not inside
    /// `fold_dependent_cells` — a fold-local guard cannot reach this path,
    /// which never touches the fold. Removing the strip turns this satisfiable
    /// problem back into `Infeasible { ConstraintUnsatisfiable }`.
    #[test]
    fn stale_auto_on_the_direct_path_must_not_prune_a_feasible_branch() {
        let p = problem_with_seed(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(not(bref("b"))))],
            Vec::new(),
            seed("b", Value::Bool(STALE_SEED)),
        );

        assert_eq!(
            solved_value(&CpSatSolver.solve(&p), "b"),
            Value::Bool(!STALE_SEED),
            "`constraint not(b) == true` is satisfied ONLY by b = false, and it \
             reads the auto DIRECTLY with an EMPTY `dependent_cells`. The stale \
             `S.b = {STALE_SEED}` seed makes `all_assigned` true at depth 0, so \
             both `S.a` branches prune on a value the search had not chosen yet",
        );
    }

    /// LOCK 2 — D1/B2 IDENTITY: a NON-auto seed entry must SURVIVE.
    ///
    /// This is the guard that matters most. `current_values` is the ONLY
    /// channel by which a CP-SAT constraint sees a non-auto base value (pinned
    /// connector autos are written into it at engine_eval.rs:2327), so an
    /// over-broad "just start from `ValueMap::new()`" strip would satisfy the
    /// two units above while silently breaking every such model.
    ///
    /// One `Bool` auto `S.a`; `current_values` seeds the NON-auto cell `S.k`;
    /// `let g = k and a`; `constraint g == true`. With `k = true` the answer is
    /// `a = true`; flipping the seed to `k = false` makes `g` identically
    /// `false` and the problem genuinely INFEASIBLE. Asserting BOTH halves is
    /// what makes this fail loudly if the seed filter strips more than the auto
    /// ids: with `S.k` gone, `g` evaluates to a non-`Bool`, the
    /// skip-don't-prune arm fires, and the verdict stops depending on the seed.
    ///
    #[test]
    fn a_non_auto_seed_entry_survives_and_still_drives_the_verdict() {
        let build = |k: bool| {
            problem_with_seed(
                vec![bool_auto("a")],
                vec![(ConstraintNodeId::new("S", 0), eq_true(bref("g")))],
                vec![(ValueCellId::new("S", "g"), and(bref("k"), bref("a")))],
                seed("k", Value::Bool(k)),
            )
        };

        assert_eq!(
            solved_value(&CpSatSolver.solve(&build(true)), "a"),
            Value::Bool(true),
            "`let g = k and a` with `constraint g == true` and the NON-auto \
             seed `S.k = true` is satisfied only by a = true",
        );

        let flipped = CpSatSolver.solve(&build(false));
        assert!(
            matches!(flipped, SolveResult::Infeasible { .. }),
            "flipping the NON-auto seed to `S.k = false` makes `g = false and \
             a` identically false, so `constraint g == true` is UNSATISFIABLE. \
             Getting anything else means the seed's non-auto entry was stripped \
             along with the autos and the verdict no longer depends on it; got \
             {flipped:?}",
        );
    }
}
