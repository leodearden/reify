//! CP-SAT solver: pure-Rust backtracking constraint solver for logical/discrete constraints.
//!
//! Handles boolean SAT, enum constraints, integer constraints, implications,
//! cardinality, and all-different via forward-checking backtracking search
//! with eval_expr as the constraint checker.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, Type, ValueCellId};
use reify_ir::{AutoParam, BestFoundReason, CompiledExpr, CompiledExprKind, ConstraintSolver, OptimalityStatus, RankedCandidate, RankedSolveResult, ResolutionProblem, SolveResult, Value, ValueMap};
use std::collections::{HashMap, HashSet};

/// Maximum number of integer domain values to enumerate.
/// If bounds produce a larger range, the solver returns NoProgress.
const MAX_INT_DOMAIN: i64 = 1000;

/// Maximum number of search NODES one enumeration may visit (PRD2 §10 Q1,
/// "budget ~1e5 leaves" — resolved here, in task β #5468, as §10 Q1 directs).
///
/// # Why a node bound exists at all
///
/// A solution `cap` bounds SOLUTIONS COLLECTED; it does not bound NODES
/// VISITED, and the gap between those is where an unbounded solve hides.
/// `ConstraintSolver::solve` enumerates at `cap = 2`, because 2 is the minimum
/// that can tell "exactly one model" from "at least two" and so the minimum
/// that can derive `unique` honestly. But a problem with exactly ONE model
/// never reaches that second collection: proving no second solution exists
/// means refuting every remaining point — i.e. EXHAUSTING the space. Two
/// `Type::Int` autos at [`MAX_INT_DOMAIN`] is 10^6 leaves, and every node on
/// the way pays a total `fold_dependent_cells` pass plus a full constraint
/// sweep. Without this bound, `solve()` would silently regress from "stop at
/// the first solution" to "walk the whole product" on every uniquely-solvable
/// model — the common case, not a pathological one.
///
/// # What happens when it bites
///
/// The search reports `complete: false` on THE SAME channel the solution cap
/// already uses. That is deliberate: `complete` keeps meaning exactly one
/// thing — "the space was exhausted" — so every honesty claim conjoined with
/// it (`unique`, `OptimalityStatus::ProvenOptimal`) stays sound without any
/// consumer having to know WHICH truncation mode fired, and no second,
/// drifting notion of completeness can grow (D5).
const ENUMERATION_NODE_BUDGET: usize = 100_000;

/// How many ranked candidates a [`RankedSolveResult::Ranked`] carries out
/// (PRD2 §10 Q1, "suggested top-N 16").
///
/// A CARRIER bound, and nothing more. Truncation happens strictly AFTER the
/// sort, so it can only ever drop candidates the ranking already judged worse
/// than the sixteen it keeps — `candidates[0]` is unaffected, and so is the
/// `optimality` verdict, which reads `complete` alone. PRD2 §4.2 states that
/// separation directly ("truncation never affects optimality") because
/// conflating the two is the natural implementation slip: a list that got
/// shorter LOOKS like a search that stopped early, and it is not one.
const RANKED_CANDIDATE_CAP: usize = 16;

/// The solution cap the ranked path enumerates under.
///
/// Far above [`RANKED_CANDIDATE_CAP`] on purpose: the ranking has to SEE every
/// feasible model to know which one is the argmin, and a cap that stopped at 16
/// would return the sixteen models found FIRST rather than the sixteen best —
/// while `complete: false` would then also strip the honest `ProvenOptimal`
/// off every solve. The real bound on this path is
/// [`ENUMERATION_NODE_BUDGET`], which bounds work rather than answers.
const ENUMERATION_SOLUTION_CAP: usize = 100_000;

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

/// Can this solver actually ENUMERATE a domain for `param`, given `constraints`?
///
/// The single authority on CP-SAT's enumeration capability, consulted by
/// `decompose::domain_of_auto` so the ROUTING decision (which solver slot a
/// component is handed to) and the CAPABILITY (whether that solver can build a
/// domain for every auto in it) cannot drift apart. Before this predicate
/// existed, that caller re-derived the capability from a hand-written type
/// list in its own doc comment — a list that was already falsified by three
/// arms of `build_variable_domain` below (the `Type::Int`-without-bounds
/// rejection, the variant-less `Type::Enum` rejection, and the `other =>`
/// catch-all that rejects `String`/`List`/`Geometry`/…).
///
/// # Why it DELEGATES rather than re-matching
///
/// Answering "can it?" by asking "did it?" is the only formulation that cannot
/// drift: a new accepted or rejected shape in `build_variable_domain` changes
/// this predicate in the same commit, by construction.
///
/// # Cost, and why the caller MEMOIZES it
///
/// Not free, and not uniformly cheap: the `Type::Enum` arm walks EVERY
/// expression tree in `constraints`, and every rejecting arm allocates a
/// throwaway `format!` String. It no longer materialises the integer domain —
/// see [`DomainSpec`] — so the `Type::Int`-with-bounds arm costs a few
/// comparisons rather than up to `MAX_INT_DOMAIN` (1000) `Value::Int`s built
/// and dropped on the spot.
///
/// `decompose_into_components_with_reads` therefore caches the verdict PER AUTO
/// PARAM INDEX across its constraint loop, so a decomposition performs at most
/// one probe per auto param rather than one per (constraint × dependent-cell
/// read × auto behind it) — which, with the enum arm's whole-slice scan, was
/// quadratic in the constraint count. On a direct-only model the caller's
/// `reached` set is empty and the probe never runs at all (PRD2 D1/B2
/// identity).
pub(crate) fn can_enumerate(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> bool {
    domain_spec(param, constraints).is_ok()
}

/// A validated, not-yet-materialised description of an auto param's domain.
///
/// This type exists so the ACCEPT/REJECT decision and the MATERIALISATION are
/// separable without becoming two authorities that can disagree. [`domain_spec`]
/// owns every rejection; [`build_variable_domain`] is a TOTAL function over the
/// spec it returns. There is deliberately no fallible arm on the materialising
/// side — nothing there can reject, so nothing there can drift from what
/// [`can_enumerate`] answered.
///
/// `IntRange` carries the validated bounds instead of the expanded values so a
/// bare capability probe does not build (and immediately drop) up to
/// `MAX_INT_DOMAIN` `Value::Int`s. `EnumVariants` still carries the collected
/// variants because proving the enum arm ACCEPTS means proving at least one
/// variant literal exists, which is the scan itself — there is nothing left to
/// defer.
enum DomainSpec {
    Bool,
    IntRange(i64, i64),
    EnumVariants(Vec<Value>),
}

/// Build the domain for a single auto param based on its type.
/// For Bool: {true, false}
/// For Int: enumerate lo..=hi from bounds (capped at MAX_INT_DOMAIN)
/// For Enum: extract variant literals from constraints
///
/// This is the MATERIALISING half only. It is total over [`DomainSpec`]: every
/// rejection lives in [`domain_spec`], so this function cannot introduce an
/// accept/reject opinion of its own and cannot drift from what
/// [`can_enumerate`] answered.
fn build_variable_domain(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Result<Vec<Value>, String> {
    Ok(match domain_spec(param, constraints)? {
        DomainSpec::Bool => vec![Value::Bool(true), Value::Bool(false)],
        DomainSpec::IntRange(lo, hi) => (lo..=hi).map(Value::Int).collect(),
        DomainSpec::EnumVariants(variants) => variants,
    })
}

/// The SINGLE AUTHORITY on whether CP-SAT can build a domain for `param`, and
/// on what that domain is.
///
/// NOTE: every `Err` arm below is also a routing input — see
/// [`can_enumerate`], which is this function's `is_ok()` and is what
/// `decompose::domain_of_auto` consults. Adding or removing a rejection here
/// changes component routing too, deliberately and in the same edit.
fn domain_spec(
    param: &AutoParam,
    constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Result<DomainSpec, String> {
    match &param.param_type {
        Type::Bool => Ok(DomainSpec::Bool),
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
                Ok(DomainSpec::IntRange(lo_i, hi_i))
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
            Ok(DomainSpec::EnumVariants(variants))
        }
        other => Err(format!(
            "CpSatSolver does not support param type {:?} for {}",
            other, param.id
        )),
    }
}

/// The result of an ALL-SOLUTIONS enumeration (PRD2 §4.2, task β #5468).
///
/// # Why an enum, and not the `{ solutions, complete }` struct §4.2 sketches
///
/// [`build_variable_domain`] can REJECT an auto param outright — an unbounded
/// `Int`, a variant-less `Enum`, an unsupported type — and `solve()` has always
/// mapped that to `SolveResult::NoProgress { reason }`. A bare struct has
/// nowhere to put that rejection but `{ solutions: [], complete: ? }`, which is
/// exactly the shape a PROVEN CONTRADICTION produces. Two opposite verdicts —
/// "this model is unsatisfiable" and "I cannot even build a domain for it" —
/// would then be indistinguishable to every consumer, which is the silent
/// failure D5 exists to forbid. Keeping the rejection on its own variant also
/// lets `solve()` keep its `NoProgress` mapping byte-identical to pre-β
/// behaviour (D1), so the two cannot drift.
#[derive(Debug)]
pub enum SolveAllResult {
    /// A domain was built for every auto param and the search ran.
    Enumerated {
        /// Every solution found, in deterministic search order (D4): auto
        /// params in `auto_params` declaration order, values in `DomainSpec`
        /// construction order. No RNG, no clock.
        solutions: Vec<HashMap<ValueCellId, Value>>,
        /// `true` iff the search EXHAUSTED the space — i.e. every point was
        /// visited and either collected or pruned.
        ///
        /// `false` means the search STOPPED EARLY (the solution `cap`, or the
        /// node budget added in step β.4); it does NOT mean the space is empty.
        /// The distinction is load-bearing in both directions:
        /// `{ solutions: [], complete: true }` is a PROOF of unsatisfiability,
        /// while `{ solutions: [], complete: false }` proves nothing at all —
        /// and every honesty claim built on this carrier (`unique`,
        /// `ProvenOptimal`) is conjoined with this flag rather than asserted.
        ///
        /// Deliberately CONSERVATIVE at the cap boundary: the cap is checked at
        /// the push, so a search whose `cap`-th solution happened to be the last
        /// one in the space still reports `false`. It did not prove it had
        /// exhausted anything, and saying so would be a guess dressed as a
        /// proof.
        complete: bool,
    },
    /// No domain could be built for at least one auto param; no search ran.
    ///
    /// `reason` is verbatim the `domain_spec` rejection string `solve()` hands
    /// to `SolveResult::NoProgress`, so the two entry points cannot drift.
    NotEnumerable {
        /// The `domain_spec` rejection, naming the param it could not enumerate.
        reason: String,
    },
}

/// Everything a backtracking search READS but never mutates, gathered so the
/// recursion carries four parameters instead of nine.
///
/// Not merely cosmetic: `backtrack` already sat at seven parameters, and the
/// enumeration generalisation adds an output vector, a solution cap and (step
/// β.4) a node budget on top. Threading those positionally would trip
/// `clippy::too_many_arguments` under the workspace's `-D warnings` gate, and —
/// more to the point — a nine-argument recursive call is where a transposed
/// pair of same-typed arguments hides. The MUTABLE search state (`assignment`,
/// the collected solutions) stays out of here deliberately, so the borrow
/// checker keeps enforcing the split rather than the reader having to.
struct SearchContext<'a> {
    variables: &'a [Variable],
    /// Constraints paired with their pre-computed cell refs.
    constraints: &'a [(ConstraintNodeId, CompiledExpr, HashSet<ValueCellId>)],
    auto_param_ids: &'a HashSet<ValueCellId>,
    functions: &'a [reify_ir::CompiledFunction],
    dependent_cells: &'a [(ValueCellId, CompiledExpr)],
    /// Stop once this many solutions have been collected.
    cap: usize,
    /// Stop once this many search nodes have been visited, whether or not any
    /// solution turned up. See [`ENUMERATION_NODE_BUDGET`] for why the two
    /// bounds are both needed and why they share one `complete` channel.
    node_budget: usize,
}

/// The owned half of a search's inputs — what [`build_search_inputs`] produces
/// and [`SearchContext`] borrows.
struct SearchInputs {
    variables: Vec<Variable>,
    auto_param_ids: HashSet<ValueCellId>,
    constraints: Vec<(ConstraintNodeId, CompiledExpr, HashSet<ValueCellId>)>,
    /// The starting `ValueMap`: `current_values` with every auto id stripped
    /// back out. See the strip's own rationale below — it is load-bearing, not
    /// hygiene.
    assignment: ValueMap,
}

/// Build the inputs a CP-SAT search needs, or report why it cannot.
///
/// THE single preamble, consumed by both `ConstraintSolver::solve` and
/// [`CpSatSolver::solve_all`]. It exists as one function rather than two copies
/// because the auto-id strip below is a correctness repair (task #5467) that a
/// second, hand-copied preamble would silently omit — which is precisely how the
/// spike this task adapts got it wrong.
fn build_search_inputs(problem: &ResolutionProblem) -> Result<SearchInputs, String> {
    // Build variable domains. `domain_spec`'s rejections come through here, and
    // are the ONLY error this function can produce.
    let mut variables = Vec::with_capacity(problem.auto_params.len());
    for param in &problem.auto_params {
        variables.push(Variable {
            id: param.id.clone(),
            domain: build_variable_domain(param, &problem.constraints)?,
        });
    }

    // Collect auto param IDs for forward-checking
    let auto_param_ids: HashSet<ValueCellId> =
        problem.auto_params.iter().map(|ap| ap.id.clone()).collect();

    // Pre-compute constraint refs
    let constraints: Vec<_> = problem
        .constraints
        .iter()
        .map(|(id, expr)| (id.clone(), expr.clone(), collect_constraint_refs(expr)))
        .collect();

    // Initialize assignment with current_values (for non-auto-param refs).
    //
    // The parenthetical is load-bearing, so it is ENFORCED rather than
    // merely intended (task #5467): every auto id is stripped back out
    // below. `current_values` is the engine's whole value map
    // (`build_solver_problem`'s `current_values = values.clone()`) and DOES
    // carry same-scope
    // auto entries, so without the strip an unassigned auto holds a STALE
    // CONCRETE value instead of being absent — and `backtrack_all`'s
    // forward-check then prunes a feasible branch off a value the search
    // had not chosen yet. Three real sources, (iii) first because it needs
    // no eval layer at all and so is reachable purely inside this crate:
    //
    //  (iii) `SolverRegistry::solve_lexicographic` warm-starts stage N+1
    //        from stage N's solution INSIDE one `solve()` call.
    //  (i)   A second `Engine::eval_cached` at the same `VersionId` serves a
    //        previously-SOLVED auto straight back from cache into `values`,
    //        which is then cloned to here.
    //  (ii)  A `param_override` on an auto cell, written as `Determined` yet
    //        still admitted to `auto_params` by `build_auto_param_list`.
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
    // connector autos are inserted there by `build_solver_problem`), so
    // starting from
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

    Ok(SearchInputs {
        variables,
        auto_param_ids,
        constraints,
        assignment,
    })
}

/// Recursive backtracking search with forward-checking, collecting EVERY
/// solution it reaches until `ctx.cap` is met.
///
/// At each level, picks the next unassigned variable, tries each domain value,
/// materialises `dependent_cells` against that trial assignment, evaluates all
/// constraints whose variables are fully assigned, and prunes on violation.
/// When every variable is assigned, the point is a solution: it is pushed to
/// `out` and the search continues from the next sibling.
///
/// Returns `true` iff the subtree at this node was EXHAUSTED — every point
/// below it visited and either collected or pruned. `false` means the search
/// stopped early — because `ctx.cap` solutions had been collected, or because
/// `ctx.node_budget` nodes had been visited — and it propagates all the way
/// out: the caller's `complete` flag is exactly this return value at the root.
///
/// The two stop conditions deliberately share that ONE return channel rather
/// than reporting themselves separately. A caller asking "did you see the whole
/// space?" gets a sound answer either way, and cannot accidentally handle one
/// truncation mode while forgetting the other. Which bound fired is a matter
/// for the reason string a caller composes, not for the search's own contract.
///
/// # Why there is ONE backtracker and not two (PRD2 §3.9, G7)
///
/// `solve()` is `solve_all(problem, 1)` plus the first-solution extraction, and
/// it is written that way rather than as a sibling function. The spike this task
/// adapts kept a separate `backtrack_all` alongside `backtrack`; both the
/// per-trial fold below and the seed strip in [`build_search_inputs`] were
/// missing from that copy, which is the lock-step-twin failure mode in its most
/// literal form — a correctness repair applied to one twin and not the other.
/// With one function, the α (#5467) regression locks guarding `solve()` guard
/// the enumeration path too, by construction.
///
/// # Why the fold is inside the value loop (task #5467, PRD2 §3 decision 9)
///
/// Without it, a constraint that reads ONLY a dependent cell has an EMPTY
/// `auto_refs`, so `all_assigned` is vacuously true and the constraint is
/// evaluated against that cell's stale/absent base value. `eval_expr` returns a
/// non-`Bool`, the skip-don't-prune arm fires, and the search prunes nothing.
/// On the first-solution path that surfaced as the wrong VALUE; on the
/// enumeration path it surfaces as the wrong CARDINALITY — the entire domain
/// product comes back as "solutions", with `complete: true` then licensing a
/// `ProvenOptimal` ranking over a set full of infeasible points.
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
/// It holds unchanged under enumeration, and for the same reason: collecting a
/// solution at the base case does not stop the search, so the very next trial
/// re-folds every cell before reading one. The collected `HashMap` is a copy
/// taken at the base case, so nothing a later branch folds can reach back into
/// an already-collected solution.
///
/// That `Undef` re-evaluation is TRUE ONLY GIVEN THE STRIPPED SEED, and the
/// guarantor is named deliberately (task #5467): [`build_search_inputs`]
/// removes every auto id from the `current_values` seed before the search
/// starts (see the seed site there). A deeper auto is therefore genuinely
/// ABSENT, not holding a stale value carried in from a previous resolution
/// round or an earlier lexicographic stage. WITHOUT that strip this fold is not
/// self-correcting: it materialises dependent cells from a mix of trial and
/// stale values, and the `Bool(false)` arm below PRUNES A FEASIBLE BRANCH. The
/// same stale seed also defeats `all_assigned` on the direct path, with no
/// dependent cell involved at all — which is why the repair belongs at the seed
/// and not in `fold_dependent_cells`. The `*_stale_*` units below pin both arms.
///
/// # Cost of the TOTAL fold, and why the obvious saving is unsound as stated
///
/// The fold runs on EVERY trial value at EVERY depth, so it costs
/// `O(|dependent_cells| · Π|domain_i|)` expression evaluations plus one
/// persistent-map insert each — and a `Type::Int` domain runs to
/// `MAX_INT_DOMAIN` = 1000 values, so the multiplier is not academic. The
/// obvious saving is to hand this function each cell's transitive auto set
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
fn backtrack_all(
    ctx: &SearchContext<'_>,
    var_index: usize,
    assignment: &mut ValueMap,
    out: &mut Vec<HashMap<ValueCellId, Value>>,
    nodes: &mut usize,
) -> bool {
    // No room left for another solution. Reached only for `cap == 0` — every
    // other path short-circuits at the base case below — but stated here so the
    // function is TOTAL over `cap` rather than relying on a caller to rule the
    // degenerate case out.
    if out.len() >= ctx.cap {
        return false;
    }

    // Base case: all variables assigned — this point IS a solution.
    if var_index >= ctx.variables.len() {
        // Extract solution
        let mut solution = HashMap::new();
        for var in ctx.variables {
            if let Some(val) = assignment.get(&var.id).cloned() {
                solution.insert(var.id.clone(), val);
            }
        }
        out.push(solution);
        // Room for another? If not, stop the WHOLE search — `false` unwinds to
        // the root and becomes `complete: false` there.
        return out.len() < ctx.cap;
    }

    let var = &ctx.variables[var_index];

    for value in &var.domain {
        // Every trial value IS a node — counted here rather than at the base
        // case, so an infeasible-heavy space (where most branches are pruned
        // long before a leaf) is bounded just as tightly as a solution-rich
        // one. Leaf-counting would leave the 10^6-node walk this budget exists
        // to prevent entirely unbounded whenever the constraints prune well.
        //
        // `remove` before returning for the same reason the cap's unwind does:
        // a PREVIOUS iteration of this loop left its value in `assignment`, and
        // this iteration has not inserted yet. Checking before the insert also
        // makes the function total over `node_budget == 0` — it stops before
        // doing any work at all rather than always visiting one node first.
        *nodes += 1;
        if *nodes >= ctx.node_budget {
            assignment.remove(&var.id);
            return false;
        }

        // Assign this variable
        assignment.insert(var.id.clone(), value.clone());

        // Materialise the dependent cells against this trial assignment, in
        // STORED (topological) order, through THE fold body the DimensionalSolver
        // residual path uses — never a cpsat-local twin (PRD2 §3.9 G7).
        // `is_solver_owned` keeps a fold from clobbering a trial auto. An empty
        // `dependent_cells` early-returns inside the helper without touching
        // `assignment` or running any guard work, so the D1/B2 path is
        // byte-identical to pre-α.
        // `dispatch: None` — cpsat has no compute-dispatch plumbing of its own,
        // and its forward-check below evaluates through a bare
        // `EvalContext::new` for the same reason. Passing `None` keeps the fold
        // and the forward-check reading the SAME context shape, so this call
        // adds no dispatch capability cpsat did not already have.
        crate::solver::fold_dependent_cells(
            assignment,
            ctx.dependent_cells,
            ctx.functions,
            |id| ctx.auto_param_ids.contains(id),
            None,
        );

        // Forward-check: evaluate all constraints whose auto-param refs are fully assigned
        let mut feasible = true;
        for (_, expr, refs) in ctx.constraints {
            // Only check constraints where ALL referenced auto params have been assigned
            let auto_refs: Vec<_> = refs
                .iter()
                .filter(|r| ctx.auto_param_ids.contains(r))
                .collect();
            let all_assigned = auto_refs.iter().all(|r| assignment.get(r).is_some());
            if !all_assigned {
                continue;
            }

            let ctx_eval = EvalContext::new(assignment, ctx.functions);
            let result = eval_expr(expr, &ctx_eval);
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

        // A `false` from the child means the search STOPPED, not that this
        // branch failed — unwind immediately rather than trying the next
        // sibling, so the cap short-circuits the whole search rather than
        // merely the deepest level.
        if feasible && !backtrack_all(ctx, var_index + 1, assignment, out, nodes) {
            assignment.remove(&var.id);
            return false;
        }
    }

    // Undo assignment (remove from map)
    assignment.remove(&var.id);
    // Every value in this variable's domain was tried and none of them stopped
    // the search — neither the solution cap nor the node budget — so this
    // subtree is exhausted.
    true
}

impl CpSatSolver {
    /// Enumerate solutions to `problem`, collecting at most `cap` of them.
    ///
    /// The honest-enumeration core (PRD2 §4.2, task β #5468). `solve()` is this
    /// function at `cap = 1` plus the first-solution extraction; `unique`
    /// derivation (step β.6) is this function at `cap = 2` — the minimum that
    /// can tell "exactly one model" from "at least two"; the `solve_ranked`
    /// argmin override (step β.8) is this function at a large cap plus a scored
    /// sort.
    ///
    /// Deterministic (D4): variables are visited in `auto_params` declaration
    /// order and values in `DomainSpec` construction order — `[true, false]` for
    /// `Bool`, ascending `lo..=hi` for `Int`, constraint-scan order for `Enum`.
    /// No RNG, no clock.
    ///
    /// See [`SolveAllResult::Enumerated::complete`] for what a `false` there
    /// does and does not license a caller to conclude.
    ///
    /// Bounded by [`ENUMERATION_NODE_BUDGET`]; this function is exactly
    /// [`CpSatSolver::solve_all_with_budget`] at that budget, mirroring this
    /// crate's existing `solve` / `solve_with_dispatch` and `solve_ranked` /
    /// `solve_ranked_impl` specialization idiom. The parameterized form exists
    /// so a unit test can observe the bound without burning ~1e5 nodes to do
    /// it — a test that had to would be quietly deleted the first time CI got
    /// slow, and the bound would then be pinned by nothing.
    pub fn solve_all(&self, problem: &ResolutionProblem, cap: usize) -> SolveAllResult {
        self.solve_all_with_budget(problem, cap, ENUMERATION_NODE_BUDGET)
    }

    /// [`CpSatSolver::solve_all`] with the node bound supplied explicitly.
    ///
    /// THE body both entry points run; `solve_all` adds nothing but the
    /// production constant. See [`ENUMERATION_NODE_BUDGET`] for what
    /// `node_budget` bounds and why it is not the same thing as `cap`.
    pub(crate) fn solve_all_with_budget(
        &self,
        problem: &ResolutionProblem,
        cap: usize,
        node_budget: usize,
    ) -> SolveAllResult {
        let mut inputs = match build_search_inputs(problem) {
            Ok(inputs) => inputs,
            Err(reason) => return SolveAllResult::NotEnumerable { reason },
        };

        let ctx = SearchContext {
            variables: &inputs.variables,
            constraints: &inputs.constraints,
            auto_param_ids: &inputs.auto_param_ids,
            functions: &problem.functions,
            dependent_cells: &problem.dependent_cells,
            cap,
            node_budget,
        };

        let mut solutions = Vec::new();
        let mut nodes = 0usize;
        let complete = backtrack_all(&ctx, 0, &mut inputs.assignment, &mut solutions, &mut nodes);

        SolveAllResult::Enumerated {
            solutions,
            complete,
        }
    }

    /// [`ConstraintSolver::solve`] with the node bound supplied explicitly.
    ///
    /// THE body the trait method runs; `solve` adds nothing but
    /// [`ENUMERATION_NODE_BUDGET`]. Same rationale as
    /// [`CpSatSolver::solve_all_with_budget`]: the budget-exhaustion arm below
    /// is a user-visible behaviour change and has to be reachable from a unit
    /// test that runs in microseconds.
    pub(crate) fn solve_with_budget(
        &self,
        problem: &ResolutionProblem,
        node_budget: usize,
    ) -> SolveResult {
        // Fast path: no auto params → already solved.
        //
        // `unique: true` here is HONEST and must stay: with zero autos there is
        // exactly one assignment — the empty one — so it is trivially the unique
        // solution. This is NOT the hardcoded flag step β.6 replaces; that one
        // is on the `Solved` arm below.
        if problem.auto_params.is_empty() {
            return SolveResult::Solved {
                values: HashMap::new(),
                unique: true,
            };
        }

        // `solve()` IS `solve_all(problem, 2)` plus the first-solution
        // extraction — one backtracker, one preamble, one fold (PRD2 §3.9 G7).
        //
        // WHY `cap = 2` AND NOT 1. One is enough to ANSWER; two is the minimum
        // that can tell "exactly one model" from "at least two", and so the
        // minimum that can derive `unique` from something the solver actually
        // observed. The extra work is one more solution's worth of search,
        // bounded above by `node_budget`.
        //
        // It does not change WHICH point comes back. The search is depth-first
        // in a fixed order, so the first solution collected at `cap = 2` is the
        // same one `cap = 1` collected; the second is found strictly after it
        // and is discarded here (D1 / the `ConstraintSolver` I1 spirit — a
        // derivation must not perturb the thing it derives from). Pinned by
        // `deriving_unique_does_not_change_which_model_solve_returns`.
        match self.solve_all_with_budget(problem, 2, node_budget) {
            SolveAllResult::NotEnumerable { reason } => SolveResult::NoProgress { reason },
            SolveAllResult::Enumerated {
                solutions,
                complete,
            } => {
                // Read the count BEFORE the `into_iter` consumes the vector.
                let model_count = solutions.len();
                match solutions.into_iter().next() {
                Some(values) => SolveResult::Solved {
                    values,
                    // HONEST `unique` (PRD2 D3, §3 decision 5). This replaces a
                    // hardcoded `true` that every `Solved` arm emitted for every
                    // problem — never a checked claim, because the pre-β search
                    // stopped at the first solution and could not have known.
                    //
                    // `complete` is a CONJUNCT, not decoration. A search that
                    // stopped early — solution cap or node budget — has not
                    // proven a second model absent, and `model_count == 1` is
                    // TRUE in exactly that case: one collected, the rest never
                    // visited. Deriving the flag from the count alone would
                    // reproduce the original lie with better manners. Pinned by
                    // `a_model_found_before_the_budget_bit_is_not_reported_as_unique`.
                    //
                    // β sets the flag and stops there. It does NOT copy
                    // `DimensionalSolver::finalise_uniqueness` (solver.rs:2686),
                    // which demotes a non-unique STRICT-auto solve to
                    // `Infeasible { ConstraintNonUnique }`. The engine's
                    // non-unique warning is gated on `ap.free`
                    // (engine_eval.rs:3355/5975), so nothing user-visible turns
                    // on the strict case yet, and CP-SAT is unreachable in
                    // production until the γ wiring — so the demotion POLICY
                    // belongs with the step that first makes it observable. See
                    // `a_strict_auto_gets_the_same_honest_flag_and_no_demotion`
                    // and task #6554, which owns that observable half.
                    //
                    // #5388 is this fix's continuous-side twin — the same
                    // uniqueness-honesty question for `DimensionalSolver`, in
                    // `solver.rs`/`registry.rs` (PRD2 §0.1, §7). Disjoint files;
                    // coordinate, do not duplicate.
                    unique: complete && model_count == 1,
                },
                // Nothing found — and `complete` is the ENTIRE discriminator
                // between the two opposite verdicts that share this shape.
                //
                // Exhausted the space and found nothing: that is a PROOF of
                // unsatisfiability, and `Infeasible` is the honest answer. The
                // diagnostic is byte-identical to pre-β (D1).
                None if complete => SolveResult::Infeasible {
                    diagnostics: vec![Diagnostic::error(format!(
                        "CpSatSolver: no satisfying assignment found for {} auto params with {} constraints",
                        problem.auto_params.len(),
                        problem.constraints.len()
                    ))
                    .with_code(DiagnosticCode::ConstraintUnsatisfiable)],
                },
                // Stopped early and found nothing: that proves NOTHING. A
                // truncated search has not shown the constraints unsatisfiable,
                // and `Infeasible` is a diagnostic a user acts on by editing
                // their model — emitting it here sends them to rewrite a design
                // that was never wrong. `NoProgress` says the only true thing
                // available: the solver declined to answer, and why. This is the
                // loud direction D5 requires.
                None => SolveResult::NoProgress {
                    reason: enumeration_budget_exhausted_reason(problem, node_budget),
                },
                }
            }
        }
    }

    /// A ranking that makes NO ordering claim: one feasible point, no score.
    ///
    /// # Why this delegates instead of enumerating again
    ///
    /// A no-objective ranking needs exactly what `solve` already computes: one
    /// feasible point, plus an honest `unique` derived from a `cap = 2`
    /// enumeration — 2 being the minimum that distinguishes "exactly one model"
    /// from "at least two", and the right cap here precisely BECAUSE a
    /// feasibility ranking makes no ordering claim and so has no use for the
    /// rest of the space.
    ///
    /// Writing that out a second time would produce a function identical to
    /// `solve_with_budget` in everything but its return type — the lock-step
    /// twin this whole task is written to avoid (PRD2 §3.9, G7). Delegating
    /// makes `solve_ranked` and `solve` agree about the same problem BY
    /// CONSTRUCTION, across every arm at once: values, `unique`, the
    /// `Infeasible` diagnostic and its code, the budget-exhaustion reason
    /// string. There is no second implementation that could drift, so there is
    /// nothing to keep in step.
    ///
    /// `Infeasible` and `NoProgress` go through `into_ranked_pass_through`,
    /// reify-ir's canonical anti-drift seam for exactly these two arms
    /// (constraint.rs:349) — the same one the trait default lift uses.
    ///
    /// # The two callers
    ///
    /// An absent objective, and — deliberately — an objective that failed to
    /// score EVERY model. On that second path `FeasibilityOnly` is in tension
    /// with F-result I3's "iff no objective governs the solve", and it is still
    /// the right answer: the alternative is `BestFound` carrying
    /// `objective_score: None`, which is the pairing I4 forbids outright, and it
    /// would additionally assert a ranking quality over a set nothing could be
    /// ranked in. Making no ordering claim is the honest description of having
    /// no orderable scores.
    ///
    /// `node_budget` is threaded rather than defaulted so a budget-parameterized
    /// ranked solve cannot fall back into a full-budget `solve`.
    fn feasibility_ranking(
        &self,
        problem: &ResolutionProblem,
        node_budget: usize,
    ) -> RankedSolveResult {
        match self.solve_with_budget(problem, node_budget) {
            SolveResult::Solved { values, unique } => RankedSolveResult::Ranked {
                candidates: vec![RankedCandidate {
                    values,
                    objective_score: None,
                    unique,
                }],
                optimality: OptimalityStatus::FeasibilityOnly,
            },
            non_solved => non_solved
                .into_ranked_pass_through()
                .expect("Solved arm already handled above"),
        }
    }

    /// [`ConstraintSolver::solve_ranked`] with the node bound supplied
    /// explicitly.
    ///
    /// THE body the trait method runs; `solve_ranked` adds nothing but
    /// [`ENUMERATION_NODE_BUDGET`]. Same rationale as
    /// [`CpSatSolver::solve_all_with_budget`] and
    /// [`CpSatSolver::solve_with_budget`]: the truncated-enumeration arm below
    /// is the one that must NOT claim `ProvenOptimal`, and pinning that needs a
    /// unit test that reaches it in microseconds rather than by walking ~1e5
    /// nodes.
    ///
    /// # Degenerate arms, and where each one is decided
    ///
    /// | enumeration outcome                        | ranked result |
    /// |--------------------------------------------|---------------|
    /// | `NotEnumerable { reason }`                 | `NoProgress { reason }` — verbatim |
    /// | scored ≥ 1 model                           | `Ranked`, sorted, truncated |
    /// | scored 0 models (empty, unscorable, or no objective) | whatever `solve` says, lifted by [`CpSatSolver::feasibility_ranking`] |
    ///
    /// The last row folds three cases together on purpose. `{ solutions: [],
    /// complete: true }` is a proven contradiction and `solve` reports
    /// `Infeasible`; `{ solutions: [], complete: false }` is a truncated search
    /// and `solve` reports `NoProgress` naming the budget; an unscorable
    /// objective still has feasible models and `solve` reports the first one.
    /// Routing all three through `solve` means those three verdicts are decided
    /// in exactly ONE place for both entry points, instead of being re-derived
    /// here from the same `complete` flag and left to drift.
    pub(crate) fn solve_ranked_with_budget(
        &self,
        problem: &ResolutionProblem,
        node_budget: usize,
    ) -> RankedSolveResult {
        // No objective ⇒ no ordering claim to make, and nothing to enumerate
        // past `cap = 2`. See `feasibility_ranking` for why this delegates.
        let Some(objective) = problem.objective.as_ref() else {
            return self.feasibility_ranking(problem, node_budget);
        };

        let (solutions, complete) =
            match self.solve_all_with_budget(problem, ENUMERATION_SOLUTION_CAP, node_budget) {
                // Verbatim the reason `solve()` hands to `SolveResult::NoProgress`
                // for this problem, so the two entry points give a user the same
                // account of the same failure.
                SolveAllResult::NotEnumerable { reason } => {
                    return RankedSolveResult::NoProgress { reason };
                }
                SolveAllResult::Enumerated {
                    solutions,
                    complete,
                } => (solutions, complete),
            };

        // Score every model, remembering its ENUMERATION INDEX — the tiebreak
        // below needs it, and it is only available here.
        let mut scored: Vec<(usize, HashMap<ValueCellId, Value>, f64)> =
            Vec::with_capacity(solutions.len());
        for (index, solution) in solutions.into_iter().enumerate() {
            // `build_scoring_values` rather than a hand-rolled overlay. It
            // documents a crate-scoped INVARIANT that no site may build a
            // scoring map as `current_values.clone()` overlaid with the solved
            // autos: that leaves every DEPENDENT CELL holding its pre-solve
            // value, so an objective reading one scores the model against
            // arithmetic that never happened.
            //
            // `dispatch: None` — cpsat has no compute-dispatch plumbing of its
            // own, and its forward-check evaluates through a bare
            // `EvalContext::new` for the same reason. Passing `None` keeps
            // scoring and pruning reading the same context shape, so this adds
            // no dispatch capability cpsat did not already have.
            let full = crate::solver::build_scoring_values(
                &problem.current_values,
                &solution,
                &problem.dependent_cells,
                &problem.functions,
                None,
            );
            // `eval_objective_set` already normalises `Maximize` to
            // "lower is better" (it accumulates `-weight · v`) and rejects any
            // non-finite fold with `None`, so every score reaching the sort is
            // a finite, well-ordered, minimisation-sense f64 (F-result I2).
            if let Some(score) =
                crate::solver::eval_objective_set(objective, &full, &problem.functions, None)
            {
                scored.push((index, solution, score));
            }
            // A model that did not score is DROPPED, mirroring
            // `solve_ranked_impl`'s "Solved-but-unscored candidate is dropped".
            // Carrying it would mean a candidate with `objective_score: None`
            // inside a scored ranking — the pairing I4 forbids — and it could
            // not be placed in the order anyway.
        }

        // Nothing scored at all. Not an empty `Ranked` (I2 requires a non-empty
        // candidate list) and not a claim of infeasibility either — the models
        // exist, they just could not be ordered. See `feasibility_ranking`.
        if scored.is_empty() {
            return self.feasibility_ranking(problem, node_budget);
        }

        // Ascending score, ties broken by ascending enumeration index — the
        // same score-then-start-index shape `DimensionalSolver::solve_ranked_impl`
        // uses. Without the second key, tied candidates land wherever the sort
        // leaves them, which makes the ranking unstable run-to-run for exactly
        // the problems where the choice is arbitrary (D4).
        //
        // `partial_cmp` returns `None` only for NaN, which `eval_objective_set`
        // already filtered out, so `unwrap_or(Equal)` is a defensive fallback
        // that is never exercised.
        scored.sort_by(|a, b| {
            a.2.partial_cmp(&b.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        // `RankedCandidate.unique` is a claim about the SCORE — "no other
        // solution with the same objective value exists" — not about the model
        // count. Two models tying at the optimum makes the winner non-unique
        // even though each is individually a perfectly good answer.
        //
        // Exact `==` on f64 is deliberate: the question is whether two models
        // attained the SAME score, and `eval_objective_set` has already
        // filtered NaN out, so equality here is the total, reflexive kind.
        //
        // `complete` is a conjunct for the same reason it is in `solve`: a
        // truncated enumeration never saw the models it did not visit, so it
        // cannot know none of them ties.
        let best_score = scored[0].2;
        let winner_unique =
            complete && scored.iter().filter(|(_, _, s)| *s == best_score).count() == 1;

        // Derived from `complete` ALONE, and computed before truncation so the
        // dependency cannot accidentally acquire a second input. A shorter list
        // is not a shorter search (PRD2 §4.2).
        //
        // `IterationLimit` for the truncated case is a deliberate, imperfect
        // choice. It is the ONLY existing `BestFoundReason` variant that gates
        // the engine's `W_SOLVER_OPTIMALITY_UNPROVEN` warning — engine_eval.rs
        // (6127, 7539) matches on it explicitly, and `ConvergedWithinBudget` /
        // `Unreported` do NOT fire the warning. Picking either of those would
        // make a truncated enumeration SILENT, which is precisely what D5
        // forbids. The cost is that `describe()` says "iteration limit reached;
        // derivative-free solver cannot prove global optimality", which is
        // inaccurate for an enumeration cap on an exact solver. An honest
        // `BestFoundReason::EnumerationBudget` variant plus the two engine gate
        // arms is task #6553 — cross-crate (reify-ir + reify-eval), outside
        // this task's module scope.
        let optimality = if complete {
            OptimalityStatus::ProvenOptimal
        } else {
            OptimalityStatus::BestFound {
                reason: BestFoundReason::IterationLimit,
            }
        };

        // Truncate strictly AFTER the sort, so the carrier keeps the best
        // `RANKED_CANDIDATE_CAP` rather than the first ones found.
        let candidates: Vec<RankedCandidate> = scored
            .into_iter()
            .take(RANKED_CANDIDATE_CAP)
            .enumerate()
            .map(|(rank, (_, values, score))| RankedCandidate {
                values,
                objective_score: Some(score),
                // Only the winner can be unique; an alternative optimum is by
                // definition not *the* solution. Mirrors `solve_ranked_impl`.
                unique: rank == 0 && winner_unique,
            })
            .collect();

        RankedSolveResult::Ranked {
            candidates,
            optimality,
        }
    }
}

/// Why an enumeration declined to answer, in the words a user eventually reads.
///
/// Reached from ONE call site — [`CpSatSolver::solve_with_budget`] — and that is
/// the design, not an accident waiting to be inlined: `solve_ranked` gets this
/// same string by DELEGATING to `solve_with_budget` through
/// [`CpSatSolver::feasibility_ranking`] rather than by composing its own
/// account of the same event. Named as a function so that stays visible; a
/// `format!` buried in a match arm invites the second entry point to grow a
/// near-copy that says something subtly different about an identical solve.
///
/// Says explicitly what the search did NOT establish. The failure mode this
/// message guards against is a reader — or a downstream agent — treating "no
/// solutions came back" as "there are none".
fn enumeration_budget_exhausted_reason(problem: &ResolutionProblem, node_budget: usize) -> String {
    format!(
        "CpSatSolver: enumeration node budget of {} exhausted while searching {} auto params \
         with {} constraints; the search was TRUNCATED, so the constraints are not known to be \
         unsatisfiable",
        node_budget,
        problem.auto_params.len(),
        problem.constraints.len(),
    )
}

impl ConstraintSolver for CpSatSolver {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        self.solve_with_budget(problem, ENUMERATION_NODE_BUDGET)
    }

    /// Rank every feasible model by the governing objective (PRD2 §4.2, D2;
    /// F-result invariants I2/I4).
    ///
    /// # What this replaces
    ///
    /// Without this override, `CpSatSolver` inherits reify-ir's default lift
    /// (constraint.rs:564): `solve()`'s answer wrapped in one candidate with
    /// `objective_score: None` and `BestFound { Unreported }`. The lift is
    /// honest about knowing nothing, but its practical effect is that a
    /// `minimize` over a discrete model is SILENTLY IGNORED — `solve()` returns
    /// the first feasible point in search order, so flipping the sense to
    /// `maximize` returns the same point (PRD2 §2.3, spike rows n3/n3b).
    ///
    /// # Why `ProvenOptimal` is honest here
    ///
    /// Earned by three properties of this search, not asserted. Domains are
    /// FINITE and materialised before the search starts. Pruning is SOUND — a
    /// branch is cut only when a constraint whose auto refs are all assigned
    /// evaluates to `Bool(false)`; a non-`Bool` result takes the
    /// skip-don't-prune arm — so no feasible point is ever lost. And the
    /// per-trial fold means every point is scored against correctly
    /// materialised dependent cells. So `complete == true` means every feasible
    /// point was visited and scored, and the ascending minimum IS the global
    /// minimum. Mixed discrete+continuous components never reach here (PRD2 ζ
    /// owns them, and always reports `BestFound`), so the pure-discrete
    /// precondition D2 puts on `ProvenOptimal` holds by construction.
    ///
    /// # Invariant I1
    ///
    /// This method adds information; it never changes what `solve` returns.
    /// `solve` is untouched, and the argmin computed here is a separate
    /// projection over the same enumeration.
    fn solve_ranked(&self, problem: &ResolutionProblem) -> RankedSolveResult {
        self.solve_ranked_with_budget(problem, ENUMERATION_NODE_BUDGET)
    }
}

// ---------------------------------------------------------------------------
// SHARED FIXTURE VOCABULARY for cpsat's in-src test modules.
//
// Lifted out of `dependent_cell_forward_check_tests` (task #5468 / PRD2 β) when
// a second and third test module needed the same vocabulary. A sibling module
// cannot reach a private item of a sibling — Rust privacy is "visible in the
// defining module and its descendants" — so `use super::dependent_cell_forward_check_tests::*`
// was never an option without widening those items anyway. Lifting them to a
// shared parent-level module says what is actually true: this vocabulary is
// cpsat's test-side ALPHABET, not one lock module's private detail.
//
// Every item is `pub(super)`: `super` here is `mod cpsat`, so each is visible
// to cpsat and all of its descendants (i.e. every in-src test module) and to
// nothing else. NOT `pub` — that would leak them onto the crate's test-cfg
// surface for no reader's benefit.
//
// Nothing here is new; the bodies and their doc comments are the originals,
// moved verbatim except for the visibility qualifier and the `or` helper added
// for β's disjunction fixtures.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod cpsat_test_fixtures {
    use super::*;
    use reify_ir::{ObjectiveSet, UnOp};
    use std::sync::Arc;

    /// The value a stale same-scope auto entry carries into the search.
    ///
    /// Named rather than inlined — mirroring `STALE_SIDE` in
    /// `tests/joint_drive_per_trial_recompute.rs` — so a mixed-up assertion
    /// cannot pass by coincidence: every lock-2 fixture is built so that the
    /// STALE answer and the CORRECT answer are opposites, and naming the stale
    /// side makes that opposition explicit at the call site.
    ///
    /// A real one arrives via any of the three sources named in this module's
    /// header; `SolverRegistry::solve_lexicographic`'s warm-start is the one that
    /// needs no eval layer at all.
    pub(super) const STALE_SEED: bool = true;

    pub(super) fn bool_auto(member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Bool,
            bounds: None,
            free: true,
        }
    }

    /// [`bool_auto`] with `free: false` — a STRICT auto.
    ///
    /// Exists to pin that the distinction makes NO difference to what cpsat
    /// reports. `free` is what the engine gates its non-unique WARNING on
    /// (engine_eval.rs:3355/5975), and `DimensionalSolver::finalise_uniqueness`
    /// (solver.rs:2686) goes further still and DEMOTES a non-unique strict solve
    /// to `Infeasible { ConstraintNonUnique }`. cpsat does neither: it reports
    /// the honest flag and stops there. That is a deliberate scope line, not an
    /// oversight, so it gets a fixture and a unit rather than silence.
    pub(super) fn strict_bool_auto(member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Bool,
            bounds: None,
            free: false,
        }
    }

    /// An `Int` auto over the INCLUSIVE bound pair `[lo, hi]` —
    /// `build_variable_domain` enumerates `lo..=hi`, so the domain has
    /// `hi - lo + 1` values.
    pub(super) fn int_auto(member: &str, lo: i64, hi: i64) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Int,
            bounds: Some((lo as f64, hi as f64)),
            free: true,
        }
    }

    pub(super) fn bref(member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("S", member), Type::Bool)
    }

    pub(super) fn iref(member: &str) -> CompiledExpr {
        CompiledExpr::value_ref(ValueCellId::new("S", member), Type::Int)
    }

    pub(super) fn not(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::unop(UnOp::Not, e, Type::Bool)
    }

    pub(super) fn and(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(reify_ir::BinOp::And, l, r, Type::Bool)
    }

    pub(super) fn eq_true(e: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Eq,
            e,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            Type::Bool,
        )
    }

    pub(super) fn mul_int(e: CompiledExpr, k: i64) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Mul,
            e,
            CompiledExpr::literal(Value::Int(k), Type::Int),
            Type::Int,
        )
    }

    pub(super) fn add_int(e: CompiledExpr, k: i64) -> CompiledExpr {
        CompiledExpr::binop(
            reify_ir::BinOp::Add,
            e,
            CompiledExpr::literal(Value::Int(k), Type::Int),
            Type::Int,
        )
    }

    pub(super) fn eq_int(e: CompiledExpr, k: i64) -> CompiledExpr {
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
    pub(super) fn seed(member: &str, v: Value) -> ValueMap {
        let mut m = ValueMap::new();
        m.insert(ValueCellId::new("S", member), v);
        m
    }

    /// A problem with the `current_values` seed exposed — that seed IS the
    /// subject under test for lock 2, so it cannot be hard-wired the way
    /// [`problem`] wires it.
    pub(super) fn problem_with_seed(
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
    pub(super) fn problem(
        auto_params: Vec<AutoParam>,
        constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
        dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
    ) -> ResolutionProblem {
        problem_with_seed(auto_params, constraints, dependent_cells, ValueMap::new())
    }

    /// The solved value of `S.<member>`, or a panic naming what actually came
    /// back — an unpruned search reports `Solved`/`unique: true` with the WRONG
    /// value, so a test that only checked the variant would pass on the bug.
    pub(super) fn solved_value(result: &SolveResult, member: &str) -> Value {
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

    /// An `Int` auto with NO bounds — the shape `domain_spec` REJECTS
    /// ("integer auto param … has no bounds; cannot enumerate domain"), and so
    /// the only way to reach the not-enumerable channel from a unit test.
    ///
    /// Distinct from [`int_auto`] deliberately: the rejection is the subject
    /// under test, not an accident of a mis-built fixture, and a reader seeing
    /// this name at the call site knows which of `domain_spec`'s four `Err`
    /// arms is being exercised.
    pub(super) fn unbounded_int_auto(member: &str) -> AutoParam {
        AutoParam {
            id: ValueCellId::new("S", member),
            param_type: Type::Int,
            bounds: None,
            free: true,
        }
    }

    /// `if <cond> { Int(then) } else { Int(els) }`.
    ///
    /// The only way to get a NON-CONSTANT integer score out of `Bool` autos, and
    /// so the shape every ranked-override fixture folds its objective from.
    /// Built as a struct literal because `CompiledExpr` exposes no `conditional`
    /// constructor — the same thing `reify-compiler`'s own tests do, with the
    /// `TAG_CONDITIONAL` content-hash recipe copied from `compile_expr`
    /// (reify-compiler/src/expr.rs) so the node is indistinguishable from a
    /// compiled one.
    pub(super) fn int_if(cond: CompiledExpr, then_i: i64, els: i64) -> CompiledExpr {
        let then_branch = CompiledExpr::literal(Value::Int(then_i), Type::Int);
        let else_branch = CompiledExpr::literal(Value::Int(els), Type::Int);
        let content_hash = reify_core::ContentHash::of(&[reify_ir::TAG_CONDITIONAL])
            .combine(cond.content_hash)
            .combine(then_branch.content_hash)
            .combine(else_branch.content_hash);
        CompiledExpr {
            kind: reify_ir::CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            result_type: Type::Int,
            content_hash,
        }
    }

    /// `l + r` over two `Int` EXPRESSIONS.
    ///
    /// Distinct from [`add_int`], which adds a literal to an expression: an
    /// objective that has to score two independent autos needs to fold two
    /// sub-expressions, not an expression and a constant.
    pub(super) fn sum_int(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(reify_ir::BinOp::Add, l, r, Type::Int)
    }

    /// [`problem`] with an OBJECTIVE — the shape `solve_ranked`'s argmin path
    /// needs, and the one thing `problem`/`problem_with_seed` hard-wire to
    /// `None`.
    pub(super) fn problem_with_objective(
        auto_params: Vec<AutoParam>,
        constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
        objective: ObjectiveSet,
    ) -> ResolutionProblem {
        ResolutionProblem {
            auto_params,
            constraints,
            current_values: ValueMap::new(),
            objective: Some(objective),
            functions: Arc::from(Vec::new()),
            dependent_cells: Vec::new(),
        }
    }

    /// Disjunction — the shape every β enumeration fixture is built on, because
    /// `a || b` is the smallest constraint with MORE THAN ONE model (3 of the 4
    /// points) and so the smallest thing that can tell honest enumeration apart
    /// from "returned the first feasible point".
    pub(super) fn or(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
        CompiledExpr::binop(reify_ir::BinOp::Or, l, r, Type::Bool)
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
//  (iii) `SolverRegistry::solve_lexicographic` — lexicographic multi-rank
//        solving clones `base.current_values` once and then WARM-STARTS stage
//        N+1 from stage N's solution INSIDE a single `solve()` call.
//  (i)   `Engine::eval_cached` — a second call at the same `VersionId` serves a
//        previously-SOLVED auto from cache straight back into `values`, which
//        `build_solver_problem` clones wholesale into `current_values`.
//  (ii)  A `param_override` on an auto cell, which is written as `Determined`
//        yet is still admitted to `auto_params` by `build_auto_param_list`.
//        The odd one out: a deliberate user pin, not staleness. It is stripped
//        anyway, for consistency with `DimensionalSolver`, and
//        `an_overridden_auto_is_searched_rather_than_pinned_to_its_seed` pins
//        that choice explicitly.
//
// The two locks are ONE module because lock 2's `*_direct_path_*` unit is what
// proves lock 2's repair belongs at the seed rather than inside the fold — an
// argument only legible next to lock 1's fold units. (They also once shared a
// private fixture vocabulary; that has since moved up to
// `cpsat_test_fixtures`, task #5468, so it is no longer part of the reason.)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod dependent_cell_forward_check_tests {
    use super::*;
    // The fixture vocabulary these locks are written in now lives one level
    // up, shared with the β enumeration modules (task #5468).
    use super::cpsat_test_fixtures::*;

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
    /// (`DimensionalSolver`'s `build_trial_values`, via the shared
    /// `fold_dependent_cells`); this is the CP-SAT-side pin on that.
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
    /// `DeterminacyState::Determined`. Stripping it
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
    /// connector autos are written into it by `build_solver_problem`), so an
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

// ---------------------------------------------------------------------------
// PRD2 β (task #5468) — the HONEST ENUMERATION CORE.
//
// `CpSatSolver::solve_all(problem, cap)` is the all-solutions generalisation of
// the same forward-checking backtracker `solve()` runs. These units pin its
// CONTRACT — not just that it returns something, but the three things every
// downstream honesty claim in this PRD is derived from:
//
//   * the solution SET is right (count, membership, and that each member
//     genuinely satisfies the constraints);
//   * `complete` distinguishes "the space was exhausted" from "the search
//     stopped early", so `unique` (step β.6) and `ProvenOptimal` (step β.8) can
//     be conjoined with it rather than asserted on faith;
//   * "cannot enumerate this domain" stays on its OWN channel
//     (`NotEnumerable`) rather than collapsing into an empty solution set —
//     which is the D5 "never silent" requirement stated in carrier form.
//
// # Why (f), (g) and (h) below exist at all
//
// The spike this task adapts (commit 75cf3b4d19, 2026-07-24) predates PRD2 α
// (#5467). Its `backtrack_all` has NEITHER the per-trial `fold_dependent_cells`
// call NOR the auto-id strip on the `current_values` seed. Cherry-picked
// verbatim onto today's tree it would be wrong in two silent directions: with
// no fold, a constraint reading only a dependent cell never prunes and
// enumeration returns THE ENTIRE DOMAIN PRODUCT as "solutions"; with no strip,
// a stale seeded auto prunes feasible branches and enumeration returns an empty
// set for a satisfiable problem. `enumerating_a_let_indirected_constraint_*`,
// `chained_dependent_cells_*` and `a_stale_seeded_auto_*` are the three units
// that make either mistake impossible to land — they are the α locks restated
// against the enumeration entry point, because a generalisation that only
// `solve()` is tested through would let the enumeration path regress alone.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod solve_all_enumeration_tests {
    use super::*;
    use super::cpsat_test_fixtures::*;
    use std::collections::HashMap;

    /// A cap comfortably above every fixture's model count.
    ///
    /// STRICTLY GREATER, never equal, and that is load-bearing rather than
    /// stylistic: the cap is checked AT THE PUSH, so a search that collects its
    /// `cap`-th solution stops right there and reports `complete: false` even
    /// when that solution happened to be the last one in the space. The flag is
    /// deliberately conservative in the honest direction — "I did not prove I
    /// exhausted it" — so a fixture wanting `complete: true` must leave the cap
    /// room to come back empty-handed at least once.
    const GENEROUS_CAP: usize = 64;

    /// Unwrap the enumerated arm, or panic naming the variant that came back.
    ///
    /// A bare `matches!` would let a `NotEnumerable` regression pass as "well,
    /// it wasn't `Enumerated`" in some other assertion's shadow; naming the
    /// actual variant here means a domain-rejection regression reads as one.
    fn enumerated(result: SolveAllResult) -> (Vec<HashMap<ValueCellId, Value>>, bool) {
        match result {
            SolveAllResult::Enumerated {
                solutions,
                complete,
            } => (solutions, complete),
            SolveAllResult::NotEnumerable { reason } => {
                panic!("expected SolveAllResult::Enumerated; got NotEnumerable {{ {reason} }}")
            }
        }
    }

    /// The value of `S.<member>` in one enumerated solution.
    fn at(solution: &HashMap<ValueCellId, Value>, member: &str) -> Value {
        solution
            .get(&ValueCellId::new("S", member))
            .unwrap_or_else(|| panic!("no enumerated value for S.{member}; got {solution:?}"))
            .clone()
    }

    /// `S.<member> == <b>`, as a plain bool, for membership assertions.
    fn boolean(solution: &HashMap<ValueCellId, Value>, member: &str) -> bool {
        match at(solution, member) {
            Value::Bool(b) => b,
            other => panic!("expected S.{member} to be a Bool; got {other:?}"),
        }
    }

    /// The canonical two-`Bool`-auto disjunction: `a || b`, whose models are
    /// exactly the three points of `{true, false}²` minus `(false, false)`.
    ///
    /// Reused by nearly every unit here because 3 is the smallest model count
    /// that separates all four behaviours this module has to tell apart:
    /// first-solution-only (1), honest enumeration (3), a cap (2), and the
    /// whole unpruned domain product (4).
    fn a_or_b() -> ResolutionProblem {
        problem(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            Vec::new(),
        )
    }

    /// (a) COUNT + MEMBERSHIP. `a || b` has exactly 3 models, and enumeration
    /// must return all 3 — each genuinely satisfying the constraint.
    ///
    /// Both halves are needed. The count alone would pass on a search that
    /// returned three arbitrary points of the domain product; the membership
    /// check alone would pass on a search that returned only the first model.
    /// Together they say: the solution SET is right. `complete: true` is the
    /// third claim — the search proved there is no fourth model — and it is
    /// what step β.6 conjoins into `unique` and step β.8 into `ProvenOptimal`.
    #[test]
    fn enumerating_a_disjunction_returns_every_model_and_reports_completeness() {
        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&a_or_b(), GENEROUS_CAP));

        assert_eq!(
            solutions.len(),
            3,
            "`a || b` over two Bool autos has exactly 3 models. 1 means the \
             search still stops at the first solution; 4 means nothing pruned \
             and the whole domain product came back as `solutions`; got \
             {solutions:?}",
        );
        for s in &solutions {
            assert!(
                boolean(s, "a") || boolean(s, "b"),
                "every enumerated solution must SATISFY `a || b`; {s:?} does not",
            );
        }
        assert!(
            complete,
            "a 3-model space enumerated under a cap of {GENEROUS_CAP} was \
             exhausted, so `complete` must be true — this is the flag step β.6 \
             conjoins into `unique` and step β.8 into `ProvenOptimal`",
        );
    }

    /// (b) DETERMINISM (D4). Two successive `solve_all` calls on the same
    /// problem return the IDENTICAL solution sequence, and that sequence starts
    /// at the point the declared search order names.
    ///
    /// The repeat-call half pins the absence of any RNG or clock in the search.
    /// The first-solution half pins the ORDER itself: variables are visited in
    /// `auto_params` declaration order (`a` then `b`) and values in `DomainSpec`
    /// construction order (`[true, false]` for `Type::Bool`), so the first model
    /// reached is `a = true, b = true`. Asserting only "the two runs agree"
    /// would be satisfied by any stable-but-wrong order; asserting the head
    /// value pins which order it is.
    #[test]
    fn enumeration_order_is_deterministic_and_follows_declaration_then_domain_order() {
        let p = a_or_b();
        let (first_run, _) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));
        let (second_run, _) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));

        assert_eq!(
            first_run, second_run,
            "D4: enumeration uses no RNG and no clock, so two calls on the same \
             problem must return byte-identical solution sequences",
        );
        assert_eq!(
            (at(&first_run[0], "a"), at(&first_run[0], "b"),),
            (Value::Bool(true), Value::Bool(true)),
            "the search visits variables in `auto_params` declaration order \
             (a, then b) and values in `DomainSpec` construction order \
             ([true, false] for Bool), so the FIRST model reached is \
             a = true, b = true",
        );
    }

    /// (c) PROVEN INFEASIBLE. `a and not(a)` has no models, and enumeration must
    /// say so with `complete: true` — an empty set the search PROVED empty.
    ///
    /// The `complete` half is the whole point. `{ solutions: [], complete: true }`
    /// and `{ solutions: [], complete: false }` are different claims —
    /// "unsatisfiable" versus "I ran out of budget before finding anything" —
    /// and step β.4's `solve()` arm splits `Infeasible` from `NoProgress` on
    /// exactly this bit. A carrier that collapsed them would make CP-SAT report
    /// a truncated search as a proof of unsatisfiability, which is the precise
    /// silent lie D5 forbids. `budget_exhaustion_is_reported_as_an_incomplete_*`
    /// (step β.3) is this assertion's other half.
    #[test]
    fn a_contradiction_enumerates_to_an_empty_but_complete_solution_set() {
        let p = problem(
            vec![bool_auto("a")],
            vec![(
                ConstraintNodeId::new("S", 0),
                and(bref("a"), not(bref("a"))),
            )],
            Vec::new(),
        );

        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));

        assert!(
            solutions.is_empty(),
            "`a and not(a)` is false at both points of a Bool domain, so it has \
             NO models; got {solutions:?}",
        );
        assert!(
            complete,
            "the search visited both points and rejected both, so the empty set \
             is PROVEN empty — `complete: false` here would be indistinguishable \
             from a truncated search that simply never found anything",
        );
    }

    /// (d) CAP TRUNCATION. The same 3-model problem at `cap = 2` returns exactly
    /// 2 solutions and `complete: false`.
    ///
    /// This is what makes `cap` an honest instrument rather than a silent one:
    /// stopping early is REPORTED, on the same `complete` channel a node-budget
    /// stop uses (step β.4), so every consumer that reads `complete` gets both
    /// truncation modes for free and none of them can grow a second, drifting
    /// notion of "was this the whole story".
    #[test]
    fn a_solution_cap_truncates_the_search_and_reports_it_as_incomplete() {
        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&a_or_b(), 2));

        assert_eq!(
            solutions.len(),
            2,
            "`cap = 2` bounds the collected solutions at 2 even though the space \
             holds 3; got {solutions:?}",
        );
        assert!(
            !complete,
            "a capped search did NOT exhaust the space, and saying otherwise \
             would let step β.6 derive `unique` and step β.8 derive \
             `ProvenOptimal` from a search that never proved anything",
        );
    }

    /// (e) NOT-ENUMERABLE IS ITS OWN CHANNEL. An `Int` auto with no bounds
    /// cannot have a domain built for it at all, and that must surface as
    /// `NotEnumerable { reason }` naming the param — NOT as an empty
    /// `Enumerated`.
    ///
    /// This is why [`SolveAllResult`] is an enum rather than the bare
    /// `{ solutions, complete }` struct PRD §4.2 sketches. With a struct, a
    /// domain rejection has nowhere to go but `{ solutions: [], complete: ? }`,
    /// which is exactly the "genuinely infeasible" shape (c) pins — two
    /// opposite verdicts wearing one costume. Keeping the rejection on its own
    /// variant is also what lets `solve()` keep mapping it to
    /// `NoProgress { reason }` byte-identically to pre-β behaviour (D1).
    #[test]
    fn an_unbuildable_domain_is_reported_as_not_enumerable_rather_than_as_no_solutions() {
        let p = problem(
            vec![unbounded_int_auto("n")],
            vec![(ConstraintNodeId::new("S", 0), eq_int(iref("n"), 3))],
            Vec::new(),
        );

        match CpSatSolver.solve_all(&p, GENEROUS_CAP) {
            SolveAllResult::NotEnumerable { reason } => assert!(
                reason.contains("S.n"),
                "the rejection must NAME the param that could not be enumerated \
                 — this string is what `solve()` hands to `NoProgress` and what \
                 a user eventually reads; got {reason:?}",
            ),
            SolveAllResult::Enumerated {
                solutions,
                complete,
            } => panic!(
                "an unbounded `Int` auto has no buildable domain, so enumeration \
                 must report NotEnumerable. Getting \
                 Enumerated {{ solutions: {solutions:?}, complete: {complete} }} \
                 means 'cannot enumerate' was collapsed into 'no solutions' — \
                 indistinguishable from a proven contradiction",
            ),
        }
    }

    /// (f) THE FOLD IS INHERITED BY THE ENUMERATION PATH. `let f = not(up)` with
    /// `constraint f == true` enumerates to EXACTLY ONE solution, `up = false`.
    ///
    /// The α lock (`a_constraint_reading_only_a_dependent_cell_prunes_*`)
    /// restated against `solve_all`, and the single most important spike
    /// adaptation. A constraint reading ONLY a dependent cell has an EMPTY
    /// `auto_refs`, so `all_assigned` is VACUOUSLY true and the constraint IS
    /// evaluated — against an absent `S.f` if the per-trial fold is missing,
    /// which returns a non-`Bool`, takes the skip-don't-prune arm, and prunes
    /// NOTHING. On the first-solution path that surfaced as the wrong VALUE; on
    /// the enumeration path it surfaces as the wrong CARDINALITY: both `up`
    /// values come back, and `complete: true` then licenses a
    /// `ProvenOptimal` ranking over a set containing an infeasible point.
    #[test]
    fn enumerating_a_let_indirected_constraint_prunes_the_auto_it_derives_from() {
        let p = problem(
            vec![bool_auto("up")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("f")))],
            vec![(ValueCellId::new("S", "f"), not(bref("up")))],
        );

        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));

        assert_eq!(
            solutions.len(),
            1,
            "`constraint f == true` over `let f = not(up)` has exactly ONE \
             model. Getting 2 means the enumeration path lost the per-trial \
             `fold_dependent_cells` call and returned the whole Bool domain as \
             'solutions'; got {solutions:?}",
        );
        assert_eq!(
            at(&solutions[0], "up"),
            Value::Bool(false),
            "the one model is up = false",
        );
        assert!(complete, "a fully-explored 2-point space is complete");
    }

    /// (g) CHAINED CELLS ARE FOLDED IN STORED ORDER, PER TRIAL. `let f = n * 2`
    /// then `let g = f + 1`, with `constraint g == 7` and `constraint f == 6`
    /// over `n ∈ [0, 5]`, enumerates to exactly one solution `n = 3`.
    ///
    /// Discriminates three ways, each landing on a different observable — the
    /// same three the α lock names, read through cardinality instead of value:
    ///
    /// * NO FOLD → both constraints see absent cells, the skip-don't-prune arm
    ///   fires at every trial, and all SIX domain values are "solutions".
    /// * REVERSED stored order (or an `EvalContext` hoisted out of the fold
    ///   loop) → `g` lags one trial behind `f`, `n = 3` and `n = 4` are each
    ///   pruned by the constraint the other satisfies, and ZERO solutions come
    ///   back.
    /// * CORRECT → exactly one, `n = 3`.
    ///
    /// A 6-value `Int` domain rather than a 2-value `Bool` one because on two
    /// values a lagged read can coincide with the correct one.
    #[test]
    fn chained_dependent_cells_are_folded_in_stored_order_during_enumeration() {
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

        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));

        assert_eq!(
            solutions.len(),
            1,
            "`g = 2n + 1` with `constraint g == 7` and `constraint f == 6` has \
             exactly ONE model over n ∈ [0, 5]. Six means the fold never ran; \
             zero means `g` was folded BEFORE `f` and read the previous trial's \
             value; got {solutions:?}",
        );
        assert_eq!(
            at(&solutions[0], "n"),
            Value::Int(3),
            "the one model is n = 3"
        );
        assert!(complete, "a fully-explored 6-point space is complete");
    }

    /// (h) THE SEED STRIP IS INHERITED BY THE ENUMERATION PATH. Two Bool autos
    /// `[a, b]` with `current_values` carrying ONLY the stale `S.b`,
    /// `let f = not(b)`, `constraint f == true`: enumeration must find the
    /// feasible `b = false` models rather than reporting an empty set.
    ///
    /// The α lock (`stale_deeper_auto_behind_a_dependent_cell_*`) restated
    /// against `solve_all`, and the second spike adaptation: the spike seeds its
    /// assignment from `problem.current_values.clone()` VERBATIM. Without the
    /// auto-id strip, the depth-0 fold materialises `S.f = not(STALE_SEED) =
    /// false` off a value the search had not chosen yet; the constraint's
    /// `auto_refs` is empty, so it is evaluated right there and prunes BOTH `S.a`
    /// branches before the search ever descends to `S.b`. A satisfiable problem
    /// comes back as an empty — and, worse, COMPLETE — solution set, which
    /// downstream reads as a proof of unsatisfiability.
    #[test]
    fn a_stale_seeded_auto_must_not_prune_every_branch_during_enumeration() {
        let p = problem_with_seed(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("f")))],
            vec![(ValueCellId::new("S", "f"), not(bref("b")))],
            seed("b", Value::Bool(STALE_SEED)),
        );

        let (solutions, complete) = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));

        assert_eq!(
            solutions.len(),
            2,
            "`let f = not(b)` with `constraint f == true` pins b = false and \
             leaves the unconstrained `S.a` free, so there are exactly 2 models. \
             Zero means the stale `S.b = {STALE_SEED}` seed survived into the \
             search and the depth-0 fold pruned every branch; got {solutions:?}",
        );
        for s in &solutions {
            assert_eq!(
                at(s, "b"),
                Value::Bool(!STALE_SEED),
                "every model must pin b = false; {s:?} does not",
            );
        }
        assert!(complete, "a fully-explored 4-point space is complete");
    }

    // -----------------------------------------------------------------------
    // NODE BUDGET (PRD2 §10 Q1, explicitly "decide in β").
    //
    // `cap` bounds SOLUTIONS COLLECTED. It does NOT bound NODES VISITED, and
    // the difference is about to become load-bearing: step β.6 makes `solve()`
    // enumerate at `cap = 2`, because 2 is the minimum that can tell "exactly
    // one model" from "at least two". But proving there is no SECOND solution
    // means proving the rest of the space holds none — i.e. EXHAUSTING it. Two
    // `Int` autos at `MAX_INT_DOMAIN` = 1000 is 10^6 leaves, and every node on
    // the way pays a total `fold_dependent_cells` pass plus a full constraint
    // sweep. Without a node bound, `solve()` would silently regress from "stop
    // at the first solution" to "walk the whole product" — the same work
    // whether or not a second solution turns up on node two.
    //
    // The budget bounds that. When it bites, the search reports `complete:
    // false` on THE SAME channel the solution cap already uses, so `complete`
    // keeps meaning exactly one thing — "the space was exhausted" — and
    // everything conjoined with it (`unique`, `ProvenOptimal`) stays honest
    // without knowing WHICH truncation mode fired (D5).
    //
    // Every unit below drives `solve_all_with_budget` / `solve_with_budget`
    // rather than the production entry points: a unit test must never have to
    // burn ~1e5 nodes to observe the bound it is testing, and a test that had
    // to would be quietly deleted the first time CI got slow.
    // -----------------------------------------------------------------------

    /// A budget far below the 6 nodes an unconstrained `int_auto("n", 0, 5)`
    /// visits, so truncation is unmistakable rather than marginal.
    const TRUNCATING_BUDGET: usize = 3;

    /// A budget that stops the search BEFORE the only model of
    /// `n == 5` (reached at the sixth and last node) can be collected.
    const BUDGET_BELOW_THE_ONLY_SOLUTION: usize = 2;

    /// Comfortably above every fixture's node count here, so the budget is
    /// provably inert rather than accidentally slack.
    const INERT_BUDGET: usize = 1_000;

    /// Six models and six nodes: `n ∈ 0..=5` with nothing constraining it.
    ///
    /// Deliberately constraint-FREE so the node count and the model count
    /// coincide. That makes "how many solutions came back" a direct readout of
    /// "how many nodes the search was allowed", with no pruning in between to
    /// muddy which of the two the budget actually bounded.
    fn six_unconstrained_ints() -> ResolutionProblem {
        problem(vec![int_auto("n", 0, 5)], Vec::new(), Vec::new())
    }

    /// One model, and it is the LAST node the search reaches: `n ∈ 0..=5` with
    /// `constraint n == 5`.
    ///
    /// The shape that separates "budget stopped me" from "there is nothing
    /// here": every earlier node is pruned, so a search cut short reports the
    /// same EMPTY solution set a contradiction does — and only `complete` tells
    /// the two apart.
    fn only_solution_is_the_last_node() -> ResolutionProblem {
        problem(
            vec![int_auto("n", 0, 5)],
            vec![(ConstraintNodeId::new("S", 0), eq_int(iref("n"), 5))],
            Vec::new(),
        )
    }

    /// `a && !a` over one `Bool` auto — no model, and the search PROVES it by
    /// visiting both domain values.
    fn contradiction() -> ResolutionProblem {
        problem(
            vec![bool_auto("a")],
            vec![(
                ConstraintNodeId::new("S", 0),
                and(bref("a"), not(bref("a"))),
            )],
            Vec::new(),
        )
    }

    /// (a) THE BUDGET TRUNCATES, AND SAYS SO. A 6-node space walked under a
    /// 3-node budget comes back with strictly fewer than 6 solutions and
    /// `complete: false`.
    ///
    /// The bound is asserted as an INEQUALITY rather than an exact count on
    /// purpose: whether the budget is checked before or after the counter
    /// increments is an implementation detail worth exactly zero test
    /// coupling. What must hold is that a budget below the node count stops the
    /// search short AND reports it — pinning an off-by-one here would just make
    /// the honest half of the assertion harder to read.
    ///
    /// `cap` is left GENEROUS so nothing but the budget can end this search. If
    /// the two truncation modes were confused, this unit would still pass while
    /// (c) — the same problem at an inert budget — failed.
    #[test]
    fn a_node_budget_truncates_the_search_and_reports_it_as_incomplete() {
        let (solutions, complete) = enumerated(CpSatSolver.solve_all_with_budget(
            &six_unconstrained_ints(),
            GENEROUS_CAP,
            TRUNCATING_BUDGET,
        ));

        assert!(
            solutions.len() < 6,
            "a {TRUNCATING_BUDGET}-node budget cannot walk a 6-node space; \
             getting all 6 back means the budget is not consulted at all and \
             `solve()` at cap = 2 will exhaust every product it is handed; got \
             {solutions:?}",
        );
        assert!(
            !solutions.is_empty(),
            "the budget must bound the search, not abort it — a {TRUNCATING_BUDGET}-node \
             budget still admits the first nodes, and coming back empty would \
             mean the bound fires before any work happens; got {solutions:?}",
        );
        assert!(
            !complete,
            "a budget-truncated search did NOT exhaust the space. Reporting \
             `complete: true` here would license step β.6's `unique` and step \
             β.8's `ProvenOptimal` off a search that stopped early — the exact \
             silent lie D5 forbids",
        );
    }

    /// (b) BUDGET-EMPTY AND PROVEN-EMPTY ARE DIFFERENT ANSWERS. Both are
    /// asserted HERE, in one unit, so a collapse of the two fails loudly
    /// instead of leaving each half green in its own file.
    ///
    /// `{ solutions: [], complete: false }` says "I found nothing and I proved
    /// nothing". `{ solutions: [], complete: true }` says "I visited every
    /// point and there is no model" — a PROOF of unsatisfiability, and the only
    /// one `solve()` is entitled to report as `Infeasible`. They share a
    /// solution set, so `complete` is the entire discriminator; an
    /// implementation that dropped the flag on the empty path would look
    /// perfectly correct until it started calling a timeout a contradiction.
    #[test]
    fn a_budget_exhausted_empty_result_is_distinguishable_from_a_proven_contradiction() {
        let (cut_short, cut_short_complete) = enumerated(CpSatSolver.solve_all_with_budget(
            &only_solution_is_the_last_node(),
            GENEROUS_CAP,
            BUDGET_BELOW_THE_ONLY_SOLUTION,
        ));
        let (proven, proven_complete) = enumerated(CpSatSolver.solve_all_with_budget(
            &contradiction(),
            GENEROUS_CAP,
            INERT_BUDGET,
        ));

        assert!(
            cut_short.is_empty(),
            "`n == 5` prunes every node before the last, and a \
             {BUDGET_BELOW_THE_ONLY_SOLUTION}-node budget never reaches it; got {cut_short:?}",
        );
        assert!(
            proven.is_empty(),
            "`a && !a` has no model at all; got {proven:?}",
        );
        assert!(
            !cut_short_complete,
            "the budget stopped this search before it could reach the ONLY \
             model, so it proved nothing. `complete: true` here would turn a \
             timeout into a claim of unsatisfiability",
        );
        assert!(
            proven_complete,
            "`a && !a` was refuted at every point of a fully-walked 2-point \
             space — that IS the proof, and `complete: false` would throw it \
             away, downgrading a real `Infeasible` to `NoProgress`",
        );
        assert_ne!(
            cut_short_complete, proven_complete,
            "two empty solution sets, two OPPOSITE verdicts: `complete` is the \
             only thing telling them apart, so it must differ here",
        );
    }

    /// (c) A GENEROUS BUDGET IS INERT. The same 6-node space at a budget well
    /// above its node count returns all 6 models with `complete: true`.
    ///
    /// The counterweight to (a), and the reason it exists: a bound that clipped
    /// ordinary solves would be caught by nothing else here — every other unit
    /// in this module runs at the production budget, where a slack-by-a-factor
    /// bug is invisible. This unit says the budget costs nothing when it is not
    /// binding.
    #[test]
    fn a_budget_above_the_node_count_cannot_clip_a_normal_enumeration() {
        let (solutions, complete) = enumerated(CpSatSolver.solve_all_with_budget(
            &six_unconstrained_ints(),
            GENEROUS_CAP,
            INERT_BUDGET,
        ));

        assert_eq!(
            solutions.len(),
            6,
            "`n ∈ 0..=5` unconstrained has exactly 6 models, and a \
             {INERT_BUDGET}-node budget is far above the 6 nodes needed to \
             reach them; anything less means the budget is clipping a solve it \
             has no business bounding; got {solutions:?}",
        );
        assert!(
            complete,
            "a 6-node space walked under a {INERT_BUDGET}-node budget was \
             exhausted; `complete: false` here would silently strip `unique` \
             and `ProvenOptimal` off every honest solve",
        );
    }

    /// (d) `solve()` REPORTS BUDGET EXHAUSTION AS `NoProgress`, NEVER AS
    /// `Infeasible`. Both arms are asserted here for the same reason (b) pairs
    /// its two: the whole point is that they must not collapse.
    ///
    /// A search that ran out of budget has not shown the constraints are
    /// unsatisfiable — it has shown nothing whatsoever. `Infeasible` is a
    /// diagnostic a user acts on by editing their model; emitting it for a
    /// solver that simply stopped early sends them to rewrite a design that was
    /// never wrong. `NoProgress` is the honest, loud answer (D5).
    ///
    /// Driven through the `pub(crate)` `solve_with_budget` seam rather than by
    /// building a 100_000-node fixture: the production const is pinned
    /// separately in (e), and the two together say more than one slow test
    /// could.
    #[test]
    fn solve_reports_budget_exhaustion_as_no_progress_rather_than_infeasible() {
        let cut_short = CpSatSolver.solve_with_budget(
            &only_solution_is_the_last_node(),
            BUDGET_BELOW_THE_ONLY_SOLUTION,
        );
        let proven = CpSatSolver.solve_with_budget(&contradiction(), INERT_BUDGET);

        match cut_short {
            SolveResult::NoProgress { reason } => assert!(
                reason.contains("budget"),
                "the reason a user reads must NAME the exhausted enumeration \
                 budget — otherwise `NoProgress` is indistinguishable from \
                 every other reason the solver declines to answer; got {reason:?}",
            ),
            SolveResult::Infeasible { diagnostics } => panic!(
                "a budget-truncated search NEVER proved `n == 5` unsatisfiable \
                 — it never reached n = 5 at all. Reporting Infeasible \
                 {diagnostics:?} tells the user to go fix a model that is \
                 perfectly satisfiable",
            ),
            other => panic!(
                "expected NoProgress naming the budget; got {other:?} — note \
                 `Solved` would be worse still, since nothing was found",
            ),
        }
        assert!(
            matches!(proven, SolveResult::Infeasible { .. }),
            "the budget arm must not swallow the REAL one: `a && !a` walked to \
             exhaustion is genuinely unsatisfiable and must still report \
             Infeasible; got {proven:?}",
        );
    }

    /// (e) THE PUBLIC ENTRY POINT IS EXACTLY THE PRODUCTION SPECIALIZATION.
    ///
    /// Two claims, and both are needed. The const's VALUE is pinned so a
    /// silent retune shows up as a failing test with a number in it rather
    /// than as a mysterious slowdown or a mysterious `complete: false`. The
    /// EQUIVALENCE is pinned so `solve_all` cannot grow a second, private
    /// bound that drifts from the named one — the lock-step-twin failure this
    /// whole task is written to avoid (G7).
    ///
    /// The equivalence is asserted against a tiny budget as well as the
    /// production one. Agreeing with `ENUMERATION_NODE_BUDGET` alone would also
    /// be satisfied by a `solve_all` that ignored budgets entirely; DISagreeing
    /// with a truncating budget is what shows the parameter is genuinely
    /// threaded through the same code path.
    ///
    /// Not probed at the exact 100_000-node boundary: `MAX_INT_DOMAIN` caps one
    /// `Int` auto at 1000 values, so reaching that boundary needs two autos and
    /// a ~10^6-leaf walk, each node paying a full fold plus constraint sweep.
    /// That is a multi-second unit test to pin an integer literal already
    /// asserted on the line above.
    #[test]
    fn the_public_solve_all_is_exactly_the_production_budget_specialization() {
        assert_eq!(
            ENUMERATION_NODE_BUDGET, 100_000,
            "PRD2 §10 Q1 sets the enumeration bound at ~1e5 nodes; changing it \
             changes when `solve()` starts answering `NoProgress` instead of \
             `Solved`, so it changes user-visible behaviour and belongs in a \
             commit that says so",
        );

        let p = six_unconstrained_ints();
        let public = enumerated(CpSatSolver.solve_all(&p, GENEROUS_CAP));
        let explicit = enumerated(CpSatSolver.solve_all_with_budget(
            &p,
            GENEROUS_CAP,
            ENUMERATION_NODE_BUDGET,
        ));
        let truncated =
            enumerated(CpSatSolver.solve_all_with_budget(&p, GENEROUS_CAP, TRUNCATING_BUDGET));

        assert_eq!(
            public, explicit,
            "`solve_all(p, cap)` must BE `solve_all_with_budget(p, cap, \
             ENUMERATION_NODE_BUDGET)` — if the two can differ, the budget a \
             reader sees named in the source is not the budget production runs",
        );
        assert_ne!(
            public, truncated,
            "the budget parameter must actually reach the search: if \
             `solve_all_with_budget` returns the same thing at {TRUNCATING_BUDGET} \
             nodes as at {ENUMERATION_NODE_BUDGET}, then the equivalence above \
             proves nothing and the bound does not exist",
        );
    }
}

// ---------------------------------------------------------------------------
// `unique` HONESTY (PRD2 D3, §3 decision 5; task β #5468).
//
// `ConstraintSolver::solve` has always answered `Solved { unique: true }` —
// hardcoded, on every `Solved` arm, for every problem. It was never a claim the
// solver had checked; the pre-β search stopped at the FIRST solution and so had
// no way to know whether a second existed. `unique` is not a decorative field:
// the engine reads it, and a `true` there is a claim that this design has
// exactly one valid configuration.
//
// Deriving it honestly costs exactly one more solution's worth of search. `cap
// = 2` is the minimum that can tell "exactly one model" from "at least two", so
// the flag becomes `complete && solutions.len() == 1` — a CONJUNCTION, because
// a search that stopped early (solution cap or node budget) has not proven a
// second model absent and must not claim it did.
//
// Every unit here asserts a VALUE or a VARIANT alongside the flag, never the
// flag alone: CP-SAT is landed-but-unwired until PRD2 γ, so these units are the
// only thing standing between the flag and a silent regression, and a unit that
// only checked `unique` would pass on a solver that returned the wrong model.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod unique_honesty_tests {
    use super::*;
    use super::cpsat_test_fixtures::*;
    use std::collections::HashMap;

    /// Unwrap `Solved`, or panic naming what actually came back.
    ///
    /// Returning BOTH halves is the point: a unit that read only `unique` would
    /// pass on a solver that reported the right flag about the wrong model, and
    /// with cpsat unwired nothing downstream would notice.
    fn solved(result: SolveResult) -> (HashMap<ValueCellId, Value>, bool) {
        match result {
            SolveResult::Solved { values, unique } => (values, unique),
            other => panic!("expected SolveResult::Solved; got {other:?}"),
        }
    }

    /// The canonical 3-model problem, reused across these units so the flag is
    /// the only thing that varies between them.
    fn a_or_b_with(autos: Vec<AutoParam>) -> ResolutionProblem {
        problem(
            autos,
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            Vec::new(),
        )
    }

    /// (a) ONE MODEL => `unique: true`. `constraint up == true` over one `Bool`
    /// auto admits exactly `up = true`, and the honest answer is the same `true`
    /// the hardcode used to emit.
    ///
    /// Here so the fix cannot be "return `false` everywhere". That would pass
    /// (b), (d) and (f) while making the flag exactly as uninformative as the
    /// hardcode was — noise instead of a lie, but still not a fact. The VALUE
    /// assertion rides along because a solver that failed to prune would report
    /// `Solved` with `up = false`.
    #[test]
    fn a_problem_with_exactly_one_model_still_reports_unique() {
        let p = problem(
            vec![bool_auto("up")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("up")))],
            Vec::new(),
        );

        let (values, unique) = solved(CpSatSolver.solve(&p));

        assert_eq!(
            values.get(&ValueCellId::new("S", "up")),
            Some(&Value::Bool(true)),
            "`constraint up == true` pins up = true; got {values:?}",
        );
        assert!(
            unique,
            "the space was EXHAUSTED and held exactly one model, so `unique` is \
             an honest true here. An implementation that answered false would \
             have swapped a hardcoded lie for a hardcoded shrug",
        );
    }

    /// (b) THREE MODELS => `unique: false`. THE unit this step exists for.
    ///
    /// `a || b` is satisfied by 3 of the 4 points of `{true, false}²`. Every
    /// pre-β `solve()` reported `unique: true` for it, because the search
    /// stopped at the first model and the flag was a literal. The engine reads
    /// that flag as "this design has exactly one valid configuration" — a claim
    /// no CP-SAT solve has ever been in a position to make.
    #[test]
    fn a_problem_with_more_than_one_model_reports_non_unique() {
        let (_, unique) =
            solved(CpSatSolver.solve(&a_or_b_with(vec![bool_auto("a"), bool_auto("b")])));

        assert!(
            !unique,
            "`a || b` has THREE models. `unique: true` here is not an \
             approximation or a rounding — it is a false statement about the \
             design, emitted by a solver that never looked",
        );
    }

    /// (c) THE ANSWER ITSELF IS UNCHANGED (D1). Deriving `unique` from a cap-2
    /// enumeration must add information, never move the point `solve` returns.
    ///
    /// The pre-β search visited variables in `auto_params` declaration order and
    /// values in `DomainSpec` construction order (`[true, false]` for `Bool`),
    /// so the first model reached for `a || b` was `a = true, b = true`. Cap-2
    /// keeps searching PAST that point; this unit says the extra search does not
    /// change which model comes back — the `ConstraintSolver` I1 spirit, that a
    /// derivation must not perturb the thing it derives from.
    ///
    /// The whole map is compared, not just the two members, so an extra or
    /// missing entry fails here rather than surfacing somewhere downstream.
    #[test]
    fn deriving_unique_does_not_change_which_model_solve_returns() {
        let (values, _) =
            solved(CpSatSolver.solve(&a_or_b_with(vec![bool_auto("a"), bool_auto("b")])));

        let expected: HashMap<ValueCellId, Value> = [
            (ValueCellId::new("S", "a"), Value::Bool(true)),
            (ValueCellId::new("S", "b"), Value::Bool(true)),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            values, expected,
            "cap-2 enumeration searches PAST the first model to learn whether a \
             second exists; it must still RETURN the first. A different point \
             here means the uniqueness derivation silently changed every \
             CP-SAT answer in the codebase",
        );
    }

    /// (d) A TRUNCATED SEARCH IS NEVER UNIQUE. One model found, then the node
    /// budget bites: the search never proved a second does not exist, so it may
    /// not say so.
    ///
    /// This is the hardcoded lie in its most tempting costume. `solutions.len()
    /// == 1` is true here, and an implementation that derived `unique` from the
    /// count ALONE would report `true` — having proven nothing at all about the
    /// three nodes it never visited. `complete` is the conjunct that makes the
    /// claim honest, and this unit is the only thing that pins it.
    ///
    /// `n == 0` puts the single model at the FIRST node, so the budget is
    /// guaranteed to bite after the model is collected rather than before.
    #[test]
    fn a_model_found_before_the_budget_bit_is_not_reported_as_unique() {
        let p = problem(
            vec![int_auto("n", 0, 5)],
            vec![(ConstraintNodeId::new("S", 0), eq_int(iref("n"), 0))],
            Vec::new(),
        );

        let (values, unique) = solved(CpSatSolver.solve_with_budget(&p, 3));

        assert_eq!(
            values.get(&ValueCellId::new("S", "n")),
            Some(&Value::Int(0)),
            "the one model reachable inside the budget is n = 0, and it must \
             still be returned — a truncated search answers with less \
             CONFIDENCE, not with a different point; got {values:?}",
        );
        assert!(
            !unique,
            "exactly one model was COLLECTED, but the search stopped three \
             nodes in and never refuted the other five. Deriving `unique` from \
             the solution count alone would report true here and would be \
             wrong for the same reason the hardcode was: nobody looked",
        );
    }

    /// (e) THE EMPTY-AUTOS FAST PATH KEEPS ITS `unique: true`.
    ///
    /// That flag is HONEST and predates this step: with zero autos there is
    /// exactly one assignment — the empty one — so it is trivially the unique
    /// solution. It sits on a different arm from the hardcode being replaced,
    /// and a fix that swept the file for `unique: true` would take it out too,
    /// turning a true statement into a false one in the opposite direction.
    #[test]
    fn the_no_auto_params_fast_path_is_honestly_unique() {
        let (values, unique) =
            solved(CpSatSolver.solve(&problem(Vec::new(), Vec::new(), Vec::new())));

        assert!(
            values.is_empty(),
            "a problem with no auto params solves to the empty assignment; got {values:?}",
        );
        assert!(
            unique,
            "with zero autos the empty assignment is the ONLY assignment, so \
             this `unique: true` is a fact, not the hardcode — it must survive \
             the change untouched",
        );
    }

    /// (f) STRICT AND FREE AUTOS GET THE SAME FLAG. Deliberate scope line, so it
    /// is pinned rather than left to be inferred from an absence.
    ///
    /// β's job is to make `unique` TRUE-OR-FALSE-AS-MEASURED. It stops there.
    /// It does NOT copy `DimensionalSolver::finalise_uniqueness`
    /// (solver.rs:2686), which demotes a non-unique STRICT-auto solve all the
    /// way to `Infeasible { ConstraintNonUnique }`. Two reasons, both outside
    /// this task: the engine's non-unique warning is gated on `ap.free`
    /// (engine_eval.rs:3355/5975), so nothing user-visible turns on the strict
    /// case yet; and CP-SAT is unreachable in production until the γ wiring, so
    /// the demotion POLICY belongs with the step that first makes it observable.
    ///
    /// If a later step adds that demotion, this unit fails — which is the
    /// correct outcome. It asserts today's contract, not a wish.
    #[test]
    fn a_strict_auto_gets_the_same_honest_flag_and_no_demotion() {
        let (_, unique) = solved(CpSatSolver.solve(&a_or_b_with(vec![
            strict_bool_auto("a"),
            strict_bool_auto("b"),
        ])));

        assert!(
            !unique,
            "`free` is not consulted anywhere in cpsat, and the model count is \
             the same 3 either way — the flag must be the same honest false",
        );
    }
}

// ---------------------------------------------------------------------------
// `solve_ranked` ARGMIN OVERRIDE (PRD2 §4.2, D2; F-result invariants I2/I4;
// task β #5468).
//
// Without an override, `CpSatSolver` inherits the reify-ir default lift
// (constraint.rs:564): it calls `solve()`, wraps whatever came back in a
// single candidate with `objective_score: None`, and reports
// `BestFound { Unreported }`. That lift is honest about knowing nothing — but
// what it means in practice is that a `minimize` on a discrete model is
// SILENTLY IGNORED. `solve()` returns the first feasible point in search order;
// flipping the objective from Minimize to Maximize returns the same point
// (PRD2 §2.3 spike rows n3/n3b). The user wrote an objective and got a
// coin-flip.
//
// The override enumerates, scores every model through the SAME
// `build_scoring_values` + `eval_objective_set` pair the continuous path uses,
// and sorts. Because the domains are finite and materialised up front, and the
// forward-check only ever prunes a branch a constraint evaluated to
// `Bool(false)`, `complete == true` means every feasible point was visited and
// scored — so the ascending minimum IS the global minimum. That is what earns
// `OptimalityStatus::ProvenOptimal`, the codebase's first honest one (the
// F-result PRD grants it exclusively to PRD 2).
//
// Every score in these fixtures is a small integer, exactly representable in
// IEEE-754 and reached by exact integer arithmetic, so every assertion here is
// exact equality or strict ordering. No tolerances, no guessed thresholds.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod solve_ranked_override_tests {
    use super::*;
    use super::cpsat_test_fixtures::*;
    use reify_ir::{
        BestFoundReason, ObjectiveSense, ObjectiveSet, OptimalityStatus, RankedCandidate,
        RankedSolveResult,
    };

    /// The objective's weight on `a`, and on `b`. Distinct and non-equal so the
    /// three models of `a || b` score three DISTINCT values:
    ///
    /// | model        | raw score |
    /// |--------------|-----------|
    /// | `a=T, b=T`   | 11        |
    /// | `a=T, b=F`   | 1         |
    /// | `a=F, b=T`   | 10        |
    ///
    /// Chosen so the Minimize winner (`a=T, b=F`) is NOT the first point in
    /// enumeration order (`a=T, b=T`). If it were, this module could not tell
    /// an argmin from "returned the first feasible model", which is exactly the
    /// behaviour it exists to replace.
    const WEIGHT_A: i64 = 1;
    const WEIGHT_B: i64 = 10;

    /// Equal weights, for the TIE fixture: `a=T,b=F` and `a=F,b=T` both score 5,
    /// and both are the Minimize optimum.
    const TIED_WEIGHT: i64 = 5;

    /// `a || b` under `sense`, scored `WEIGHT_A·[a] + WEIGHT_B·[b]`.
    fn a_or_b_scored(sense: ObjectiveSense) -> ResolutionProblem {
        scored_disjunction(sense, WEIGHT_A, WEIGHT_B)
    }

    /// `a || b` under `sense`, scored with the given per-auto weights.
    fn scored_disjunction(sense: ObjectiveSense, wa: i64, wb: i64) -> ResolutionProblem {
        problem_with_objective(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            ObjectiveSet::single(
                sense,
                sum_int(int_if(bref("a"), wa, 0), int_if(bref("b"), wb, 0)),
            ),
        )
    }

    /// The three models of `a || b` with the score each one MUST receive,
    /// computed here from the weights rather than read back from the solver.
    ///
    /// Mirrors `eval_objective_set`'s normalisation: a `Maximize` term
    /// contributes `-weight · v`, so every score in this carrier is already
    /// "lower is better" (F-result I2). A test that read the solver's own
    /// scores back and then checked they were sorted would pass on a solver
    /// that scored every model wrong but consistently.
    fn expected(sense: ObjectiveSense, wa: i64, wb: i64) -> Vec<((bool, bool), f64)> {
        [(true, true), (true, false), (false, true)]
            .into_iter()
            .map(|(a, b)| {
                let raw = (if a { wa } else { 0 } + if b { wb } else { 0 }) as f64;
                let score = match sense {
                    ObjectiveSense::Minimize => raw,
                    ObjectiveSense::Maximize => -raw,
                };
                ((a, b), score)
            })
            .collect()
    }

    /// Unwrap `Ranked`, or panic naming the variant that came back.
    fn ranked(result: RankedSolveResult) -> (Vec<RankedCandidate>, OptimalityStatus) {
        match result {
            RankedSolveResult::Ranked {
                candidates,
                optimality,
            } => (candidates, optimality),
            other => panic!("expected RankedSolveResult::Ranked; got {other:?}"),
        }
    }

    /// One candidate as `((a, b), score, unique)` — the whole of what a caller
    /// can observe, in a shape that compares with `assert_eq!`.
    ///
    /// `RankedCandidate` derives neither `PartialEq` nor `Eq`, so "byte-identical
    /// across two runs" has to be spelled out; projecting to a comparable tuple
    /// says exactly which fields that claim covers.
    fn shape(c: &RankedCandidate) -> ((bool, bool), Option<f64>, bool) {
        let get = |m: &str| match c.values.get(&ValueCellId::new("S", m)) {
            Some(Value::Bool(v)) => *v,
            other => panic!("expected a Bool at S.{m}; got {other:?}"),
        };
        ((get("a"), get("b")), c.objective_score, c.unique)
    }

    /// The score a candidate MUST carry, or a panic — `None` is never a valid
    /// answer under a governing objective (F-result I4).
    fn score(c: &RankedCandidate) -> f64 {
        c.objective_score.unwrap_or_else(|| {
            panic!(
                "an objective governs this solve, so every candidate must carry \
                 a score; got objective_score: None for {:?}",
                c.values
            )
        })
    }

    /// (a) ASCENDING BY SCORE, AND `candidates[0]` IS THE TRUE ARGMIN (I2).
    ///
    /// Two independent claims. The ORDER claim is what I2 promises consumers:
    /// index 0 is the selected optimum and the list descends in quality from
    /// there. The ARGMIN claim is the one that matters — the expected minimum is
    /// computed from the fixture's weights in [`expected`], never read back from
    /// the solver, so a solver that scored every model wrong but sorted the
    /// wrong scores correctly still fails here.
    #[test]
    fn ranking_is_ascending_by_score_and_starts_at_the_global_argmin() {
        let (candidates, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));

        assert!(
            !candidates.is_empty(),
            "I2: a `Ranked` result's candidate list is never empty",
        );
        for pair in candidates.windows(2) {
            assert!(
                score(&pair[0]) < score(&pair[1]),
                "I2: candidates are ordered by ascending objective_score (lower \
                 is better); {:?} then {:?} is not",
                shape(&pair[0]),
                shape(&pair[1]),
            );
        }

        let table = expected(ObjectiveSense::Minimize, WEIGHT_A, WEIGHT_B);
        let (best_model, best_score) = table
            .iter()
            .min_by(|x, y| x.1.partial_cmp(&y.1).expect("scores are finite"))
            .expect("three models");

        assert_eq!(
            (shape(&candidates[0]).0, score(&candidates[0])),
            (*best_model, *best_score),
            "candidates[0] must be the model with the lowest score over ALL \
             three feasible points. Getting (true, true) here means the \
             objective was ignored and the FIRST point in enumeration order was \
             returned — the pre-β behaviour this override replaces",
        );
    }

    /// (b) FLIPPING THE SENSE FLIPS THE WINNER. The direct discriminator
    /// against today's behaviour.
    ///
    /// Under the default lift, `solve_ranked` returns whatever `solve()` found —
    /// the first feasible point in search order — and `solve()` never looks at
    /// the objective at all. So Minimize and Maximize produce the SAME winner
    /// (PRD2 §2.3, spike rows n3/n3b: "objective silently ignored"). A user who
    /// wrote `minimize` and a user who wrote `maximize` got identical answers
    /// and no diagnostic either way.
    ///
    /// The expected winners are named explicitly rather than merely asserted
    /// unequal: "they differ" would also be satisfied by two arbitrary wrong
    /// answers.
    #[test]
    fn flipping_the_objective_sense_selects_a_different_winner() {
        let (min_candidates, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));
        let (max_candidates, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Maximize)));

        assert_eq!(
            shape(&min_candidates[0]).0,
            (true, false),
            "minimising 1·[a] + 10·[b] over `a || b` picks a=true, b=false \
             (score 1)",
        );
        assert_eq!(
            shape(&max_candidates[0]).0,
            (true, true),
            "maximising the same expression picks a=true, b=true (raw 11, \
             normalised to -11)",
        );
        assert_ne!(
            shape(&min_candidates[0]).0,
            shape(&max_candidates[0]).0,
            "the winner MUST depend on the sense. Identical winners is the \
             pre-β signature: the objective was never consulted",
        );
    }

    /// (c) A COMPLETE ENUMERATION IS `ProvenOptimal` (D2).
    ///
    /// Earned, not asserted. The domains are finite and materialised before the
    /// search starts; the forward-check prunes a branch ONLY when a constraint
    /// whose auto refs are all assigned evaluates to `Bool(false)` (a non-`Bool`
    /// result takes the skip-don't-prune arm), so no feasible point is ever cut.
    /// `complete == true` therefore means every feasible point was visited and
    /// scored, and the ascending minimum is the GLOBAL minimum.
    ///
    /// This is the codebase's first honest `ProvenOptimal`. Every other producer
    /// is derivative-free or budget-truncated and must say `BestFound`
    /// (F-result I3).
    #[test]
    fn a_complete_enumeration_under_an_objective_is_proven_optimal() {
        let (_, optimality) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));

        assert!(
            matches!(optimality, OptimalityStatus::ProvenOptimal),
            "a finite domain walked to exhaustion with sound pruning IS a proof \
             of global optimality; got {optimality:?}",
        );
    }

    /// (d) EVERY CANDIDATE CARRIES A SCORE (I4).
    ///
    /// I4 forbids pairing a scored optimality status with `objective_score:
    /// None`. The default lift emits exactly that pairing today — `BestFound`
    /// alongside a `None` score — which reads downstream as "here is the best
    /// one, and no, I will not tell you how good it is".
    #[test]
    fn every_candidate_carries_a_score_when_an_objective_governs() {
        let (candidates, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));

        for c in &candidates {
            assert!(
                c.objective_score.is_some(),
                "I4: an objective governs this solve, so no candidate may carry \
                 objective_score: None; {:?} does",
                c.values,
            );
        }
    }

    /// (e) ALL THREE MODELS ARE RANKED, with the scores [`expected`] predicts.
    ///
    /// The count alone would pass on three arbitrary points; the score-set
    /// comparison says the ranking covers the actual model set. Well under the
    /// top-N carrier cap, so nothing is truncated here — that interaction is
    /// step β.9's (c).
    #[test]
    fn every_feasible_model_appears_in_the_ranking() {
        let (candidates, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));

        assert_eq!(
            candidates.len(),
            3,
            "`a || b` has three models and all three are scoreable; 1 means the \
             ranking is still `solve()`'s single answer in a richer wrapper",
        );

        let mut got: Vec<((bool, bool), f64)> =
            candidates.iter().map(|c| (shape(c).0, score(c))).collect();
        let mut want = expected(ObjectiveSense::Minimize, WEIGHT_A, WEIGHT_B);
        got.sort_by_key(|x| x.0);
        want.sort_by_key(|x| x.0);

        assert_eq!(
            got, want,
            "every model must appear exactly once, carrying the score the \
             fixture's weights give it",
        );
    }

    /// (f) THE WINNER'S `unique` MEANS "no OTHER model attained this score".
    ///
    /// `RankedCandidate.unique` is documented as "the solver certifies no other
    /// solution with the same objective value exists" — a claim about the SCORE,
    /// not about the model count. With distinct weights the optimum is attained
    /// once and the flag is `true`; with equal weights two models tie at 5 and
    /// it must be `false`, even though each is individually a perfectly good
    /// answer.
    ///
    /// Non-winners carry `false` unconditionally, mirroring
    /// `DimensionalSolver::solve_ranked_impl`: an alternative optimum is by
    /// definition not *the* unique solution.
    #[test]
    fn the_winner_reports_uniqueness_of_its_score_and_non_winners_never_do() {
        let (distinct, _) =
            ranked(CpSatSolver.solve_ranked(&a_or_b_scored(ObjectiveSense::Minimize)));
        assert!(
            distinct[0].unique,
            "score 1 is attained by exactly one of the three models, and the \
             enumeration was complete, so the winner is unique; got {:?}",
            shape(&distinct[0]),
        );
        for c in &distinct[1..] {
            assert!(
                !c.unique,
                "a non-winning candidate is by definition not *the* solution; \
                 {:?} claims otherwise",
                shape(c),
            );
        }

        let (tied, _) = ranked(CpSatSolver.solve_ranked(&scored_disjunction(
            ObjectiveSense::Minimize,
            TIED_WEIGHT,
            TIED_WEIGHT,
        )));
        assert_eq!(
            score(&tied[0]),
            TIED_WEIGHT as f64,
            "with equal weights the optimum is {TIED_WEIGHT}, attained by both \
             single-true models",
        );
        assert!(
            !tied[0].unique,
            "two models attain the best score, so the winner is NOT unique — \
             reporting true here would tell a user their design has one optimal \
             configuration when it has two",
        );
    }

    /// (g) DETERMINISM (D4), INCLUDING UNDER AN EXACT TIE.
    ///
    /// The repeat-call half pins the absence of any RNG or clock. The tie half
    /// pins the COMPARATOR: two models scoring exactly 5 have no score-based
    /// order between them, so something else must decide, and that something
    /// must not be `sort_by`'s behaviour on a partial order. Ascending
    /// enumeration index is the tiebreak — the same shape as
    /// `solve_ranked_impl`'s score-then-start-index comparator — so `a=T,b=F`
    /// (enumeration index 1) precedes `a=F,b=T` (index 2), run after run.
    #[test]
    fn ranking_is_deterministic_and_ties_break_by_enumeration_index() {
        let p = a_or_b_scored(ObjectiveSense::Minimize);
        let first: Vec<_> = ranked(CpSatSolver.solve_ranked(&p))
            .0
            .iter()
            .map(shape)
            .collect();
        let second: Vec<_> = ranked(CpSatSolver.solve_ranked(&p))
            .0
            .iter()
            .map(shape)
            .collect();

        assert_eq!(
            first, second,
            "D4: no RNG and no clock, so two calls on the same problem return \
             identical values, scores and ordering",
        );

        let (tied, _) = ranked(CpSatSolver.solve_ranked(&scored_disjunction(
            ObjectiveSense::Minimize,
            TIED_WEIGHT,
            TIED_WEIGHT,
        )));
        assert!(
            tied.len() >= 2,
            "the tie-order claim needs both tied models in the ranking; got {:?}",
            tied.iter().map(shape).collect::<Vec<_>>(),
        );
        assert_eq!(
            (shape(&tied[0]).0, shape(&tied[1]).0),
            ((true, false), (false, true)),
            "an exact tie must be broken by ascending ENUMERATION INDEX, so the \
             model found first comes first. Leaving tied candidates to fall \
             wherever the sort puts them makes the ranking unstable run-to-run \
             for exactly the problems where the choice is arbitrary — the worst \
             place to be non-reproducible",
        );
    }

    // -----------------------------------------------------------------------
    // HONESTY EDGES AND NON-OBJECTIVE ARMS (PRD2 §4.2, D2, D5; F-result I2/I4).
    //
    // The units above cover the happy path: an objective, a complete
    // enumeration, a clean argmin. These cover everything else — the arms where
    // a ranking is asked a question it cannot answer, and where the tempting
    // reply is a confident-looking one. Each is a place a `Ranked` could come
    // back well-formed and WRONG:
    //
    //   * an empty candidate list (I2 says non-empty)
    //   * a ranking of 3 models under no objective, claiming an order it has
    //     no basis for
    //   * `ProvenOptimal` on a search that stopped early (D2)
    //   * `BestFound` alongside `objective_score: None` (I4)
    //   * `Infeasible` for a search that merely ran out of budget (D5)
    //
    // Two of these — the top-N carrier cap and the node budget — BOTH produce a
    // shorter answer, and only one of them is a shorter SEARCH. Conflating them
    // is the single most likely implementation slip on this path, so (c) and (d)
    // pin them from opposite sides.
    // -----------------------------------------------------------------------

    /// Twenty models, `n ∈ 0..=19`, more than [`RANKED_CANDIDATE_CAP`] so the
    /// carrier truncation is genuinely exercised, and cheap enough (20 nodes)
    /// that the node budget is nowhere near binding.
    fn twenty_ints(sense: ObjectiveSense) -> ResolutionProblem {
        problem_with_objective(
            vec![int_auto("n", 0, 19)],
            Vec::new(),
            ObjectiveSet::single(sense, iref("n")),
        )
    }

    /// The `S.n` of one candidate.
    fn n_of(c: &RankedCandidate) -> i64 {
        match c.values.get(&ValueCellId::new("S", "n")) {
            Some(Value::Int(v)) => *v,
            other => panic!("expected an Int at S.n; got {other:?}"),
        }
    }

    /// (a) NO OBJECTIVE ⇒ ONE CANDIDATE, NO SCORE, `FeasibilityOnly`.
    ///
    /// The size-1 claim is the substantive one and is asserted explicitly. A
    /// feasibility ranking makes NO ordering claim — there is nothing to order
    /// by — so returning all three models would be a false ordering claim
    /// dressed as generosity: `candidates[0]` is documented as "the selected
    /// optimum", and under no objective there is no such thing.
    #[test]
    fn a_ranking_with_no_objective_is_one_unordered_feasible_point() {
        let p = problem(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            Vec::new(),
        );
        let (candidates, optimality) = ranked(CpSatSolver.solve_ranked(&p));

        assert_eq!(
            candidates.len(),
            1,
            "no objective means no ordering claim; returning all 3 models would \
             assert that candidates[0] is the best of them, which nothing here \
             established; got {:?}",
            candidates.iter().map(shape).collect::<Vec<_>>(),
        );
        assert_eq!(
            candidates[0].objective_score, None,
            "I4: a feasibility-only candidate carries no score - there is no \
             objective to have scored it with",
        );
        assert!(
            matches!(optimality, OptimalityStatus::FeasibilityOnly),
            "I3: `FeasibilityOnly` is used iff no objective governs the solve; \
             got {optimality:?}",
        );
    }

    /// (b) A FEASIBILITY RANKING'S `unique` AGREES WITH `solve()`.
    ///
    /// Both directions in ONE unit, and both compared against `solve()` on the
    /// SAME problem, because the failure worth catching is not "the flag is
    /// wrong" but "the two entry points disagree". A consumer that reads
    /// `solve()` in one place and `solve_ranked()` in another would then get
    /// two different accounts of one design's determinacy, with nothing
    /// anywhere to reconcile them.
    #[test]
    fn a_feasibility_rankings_unique_flag_matches_what_solve_reports() {
        let many = problem(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            Vec::new(),
        );
        let one = problem(
            vec![bool_auto("a")],
            vec![(ConstraintNodeId::new("S", 0), eq_true(bref("a")))],
            Vec::new(),
        );

        for (p, expected, label) in [(&many, false, "3 models"), (&one, true, "1 model")] {
            let (candidates, _) = ranked(CpSatSolver.solve_ranked(p));
            let solve_unique = match CpSatSolver.solve(p) {
                SolveResult::Solved { unique, .. } => unique,
                other => panic!("expected Solved for the {label} fixture; got {other:?}"),
            };

            assert_eq!(
                candidates[0].unique, expected,
                "the {label} fixture must report unique = {expected}",
            );
            assert_eq!(
                candidates[0].unique, solve_unique,
                "`solve_ranked` and `solve` must agree about the same problem \
                 ({label}); disagreeing gives a consumer two accounts of one \
                 design's determinacy and no way to reconcile them",
            );
        }
    }

    /// (c) A SHORTER LIST IS NOT A SHORTER SEARCH. Twenty models, complete
    /// enumeration, carrier cap 16: sixteen candidates AND `ProvenOptimal`.
    ///
    /// The direct pin on PRD2 §4.2's "truncated to top-N carrier cap
    /// (truncation never affects optimality)". The slip this catches is
    /// entirely natural — the code has just dropped four answers on the floor,
    /// and downgrading the verdict feels like the conservative thing to do. It
    /// is not: the search DID exhaust the space and DID prove the winner
    /// optimal; the four dropped candidates were the four WORST, and dropping
    /// them cannot unprove anything. Downgrading here would fire
    /// `W_SOLVER_OPTIMALITY_UNPROVEN` at a user whose answer is provably
    /// optimal.
    ///
    /// `candidates[0]` is asserted to be the true global argmin as well, so the
    /// unit cannot pass on an implementation that truncated BEFORE sorting and
    /// kept the first sixteen models found instead of the best sixteen.
    #[test]
    fn carrier_truncation_keeps_the_best_and_never_downgrades_optimality() {
        let (candidates, optimality) =
            ranked(CpSatSolver.solve_ranked(&twenty_ints(ObjectiveSense::Minimize)));

        assert_eq!(
            candidates.len(),
            RANKED_CANDIDATE_CAP,
            "20 scored models truncate to the {RANKED_CANDIDATE_CAP}-candidate \
             carrier cap",
        );
        assert_eq!(
            (n_of(&candidates[0]), score(&candidates[0])),
            (0, 0.0),
            "minimising `n` over 0..=19 has argmin n = 0. A different head means \
             truncation happened BEFORE the sort, keeping the first sixteen \
             models found rather than the sixteen best",
        );
        assert!(
            matches!(optimality, OptimalityStatus::ProvenOptimal),
            "the SEARCH exhausted the space; only the LIST was shortened, and \
             the four dropped candidates were the four worst. Downgrading here \
             would fire W_SOLVER_OPTIMALITY_UNPROVEN at a user whose answer is \
             provably optimal; got {optimality:?}",
        );
    }

    /// (d) A SHORTER SEARCH *IS* A WEAKER CLAIM. The same twenty models under a
    /// node budget that stops the search early: `BestFound`, never
    /// `ProvenOptimal`, and `candidates[0]` is the best OF THOSE ENUMERATED.
    ///
    /// `Maximize` is the sense on purpose. Under it the score is `-n`, so the
    /// GLOBAL argmin is `n = 19` — the very last node the search would reach,
    /// and one the budget guarantees it never does. So "best of those
    /// enumerated" and "best overall" give visibly different answers here, and
    /// the unit pins which one is returned rather than letting a fixture where
    /// they coincide prove nothing.
    ///
    /// The returned head is compared against the minimum over the returned SET
    /// rather than against a hardcoded `n`, so the assertion does not depend on
    /// exactly where the budget lands.
    #[test]
    fn a_budget_truncated_ranking_returns_the_best_of_what_it_saw_and_says_so() {
        let (candidates, optimality) =
            ranked(CpSatSolver.solve_ranked_with_budget(&twenty_ints(ObjectiveSense::Maximize), 5));

        assert!(
            candidates.len() >= 2 && candidates.len() < 20,
            "the budget must truncate the search without aborting it, or the \
             best-of-enumerated claim below is vacuous; got {} candidates",
            candidates.len(),
        );
        assert_ne!(
            n_of(&candidates[0]),
            19,
            "n = 19 is the GLOBAL argmin under Maximize (score -19) and sits at \
             the last node in the space — a 5-node budget cannot have reached \
             it, so returning it would mean the budget was not consulted",
        );

        let best_seen = candidates.iter().map(score).fold(f64::INFINITY, f64::min);
        assert_eq!(
            score(&candidates[0]),
            best_seen,
            "candidates[0] must be the best of what WAS enumerated — a \
             truncated search still ranks honestly over the set it saw",
        );
        assert!(
            matches!(
                optimality,
                OptimalityStatus::BestFound {
                    reason: BestFoundReason::IterationLimit
                }
            ),
            "a search that stopped early cannot have proven anything global. \
             `ProvenOptimal` here would be the loudest possible lie: it tells a \
             user their design is optimal on the strength of a search that \
             visited a quarter of it; got {optimality:?}",
        );
    }

    /// (e) THE OPTIMALITY VERDICT REACHES THE ENGINE'S WARNING GATE CORRECTLY.
    ///
    /// `engine_eval.rs` (6127, 7539) gates `W_SOLVER_OPTIMALITY_UNPROVEN` on
    /// `BestFound` AND `matches!(reason, BestFoundReason::IterationLimit)` —
    /// `ConvergedWithinBudget` and `Unreported` explicitly do NOT fire it. That
    /// predicate is REPRODUCED here rather than described, so the two arms are
    /// pinned as what they are: PRD2 B6 ("warning fires" on a capped
    /// enumeration) and B5 ("no warning" on a complete one) both remain
    /// satisfiable downstream at γ/ε with zero engine changes.
    ///
    /// The `IterationLimit` choice is deliberate and imperfect — its
    /// `describe()` text is inaccurate for an enumeration cap on an exact
    /// solver — but it is the ONLY variant that keeps the truncated case
    /// audible, and silence is the one outcome D5 rules out. Task #6553 owns
    /// the honest variant.
    #[test]
    fn the_optimality_verdict_matches_the_engines_warning_gate_predicate() {
        // Exactly `engine_eval.rs`'s gate, spelled out.
        fn fires_warning(o: &OptimalityStatus) -> bool {
            matches!(
                o,
                OptimalityStatus::BestFound {
                    reason: BestFoundReason::IterationLimit
                }
            )
        }

        let (_, truncated) =
            ranked(CpSatSolver.solve_ranked_with_budget(&twenty_ints(ObjectiveSense::Maximize), 5));
        let (_, complete) =
            ranked(CpSatSolver.solve_ranked(&twenty_ints(ObjectiveSense::Minimize)));

        assert!(
            fires_warning(&truncated),
            "PRD2 B6: a capped enumeration must reach the engine's warning gate. \
             `ConvergedWithinBudget` or `Unreported` would be SILENT — the user \
             would read an unproven answer as a proven one; got {truncated:?}",
        );
        assert!(
            !fires_warning(&complete),
            "PRD2 B5: an honest `ProvenOptimal` silences the warning with zero \
             engine changes; got {complete:?}",
        );
    }

    /// (f) A DOMAIN THAT CANNOT BE BUILT ⇒ `NoProgress`, NOT AN EMPTY `Ranked`.
    ///
    /// I2 requires `Ranked.candidates` to be non-empty, so a ranking has no
    /// well-formed way to say "I could not start". `NoProgress` is that channel,
    /// carrying the same `domain_spec` rejection `solve()` reports for this
    /// problem.
    #[test]
    fn an_unbuildable_domain_ranks_as_no_progress() {
        let p = problem_with_objective(
            vec![unbounded_int_auto("n")],
            Vec::new(),
            ObjectiveSet::single(ObjectiveSense::Minimize, iref("n")),
        );

        match CpSatSolver.solve_ranked(&p) {
            RankedSolveResult::NoProgress { reason } => assert!(
                reason.contains("S.n"),
                "the rejection must name the param it could not enumerate; got {reason:?}",
            ),
            other => panic!(
                "an unbuildable domain has no candidates to rank, and I2 forbids \
                 an empty `Ranked`; got {other:?}",
            ),
        }
    }

    /// (g) A PROVEN CONTRADICTION ⇒ `Infeasible`, WITH THE SAME CODE `solve()`
    /// REPORTS.
    ///
    /// The code is asserted, not just the variant: `ConstraintUnsatisfiable` is
    /// what a consumer branches on, and an `Infeasible` carrying a different
    /// code (or none) is a different diagnostic wearing the right variant.
    #[test]
    fn a_proven_contradiction_ranks_as_infeasible_with_the_same_code_solve_uses() {
        let p = problem_with_objective(
            vec![bool_auto("a")],
            vec![(
                ConstraintNodeId::new("S", 0),
                and(bref("a"), not(bref("a"))),
            )],
            ObjectiveSet::single(ObjectiveSense::Minimize, int_if(bref("a"), 1, 0)),
        );

        let ranked_codes = match CpSatSolver.solve_ranked(&p) {
            RankedSolveResult::Infeasible { diagnostics } => {
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
            }
            other => panic!(
                "`a && !a` was refuted at every point of a fully-walked space — \
                 that IS a proof, and the ranking must report it as one; got {other:?}",
            ),
        };
        let solve_codes = match CpSatSolver.solve(&p) {
            SolveResult::Infeasible { diagnostics } => {
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
            }
            other => panic!("expected solve() to report Infeasible too; got {other:?}"),
        };

        assert!(
            ranked_codes.contains(&Some(DiagnosticCode::ConstraintUnsatisfiable)),
            "the diagnostic a consumer branches on must be present; got {ranked_codes:?}",
        );
        assert_eq!(
            ranked_codes, solve_codes,
            "`solve_ranked` and `solve` must hand a user the SAME account of the \
             same contradiction",
        );
    }

    /// (h) AN OBJECTIVE THAT SCORES NOTHING STILL RETURNS A WELL-FORMED ANSWER.
    ///
    /// The objective here reads `S.ghost`, a cell that exists nowhere, so it
    /// folds to `Undef` at every model and `eval_objective_set` rejects all
    /// three. Every candidate is dropped — and the tempting reply is an empty
    /// `Ranked`, which I2 forbids outright.
    ///
    /// The fallback is the feasibility lift: one point, `objective_score: None`,
    /// and an optimality that makes NO scored claim. The pairing that must never
    /// appear is `BestFound` (or `ProvenOptimal`) alongside a `None` score —
    /// I4 — which would tell a consumer "here is the best one, and no, I will
    /// not say how good it is".
    #[test]
    fn an_objective_that_scores_no_model_falls_back_rather_than_ranking_nothing() {
        let p = problem_with_objective(
            vec![bool_auto("a"), bool_auto("b")],
            vec![(ConstraintNodeId::new("S", 0), or(bref("a"), bref("b")))],
            ObjectiveSet::single(ObjectiveSense::Minimize, iref("ghost")),
        );

        let (candidates, optimality) = ranked(CpSatSolver.solve_ranked(&p));

        assert!(
            !candidates.is_empty(),
            "I2: `Ranked.candidates` is never empty. The models EXIST — they \
             just could not be ordered — so dropping them all leaves a carrier \
             that violates its own contract",
        );
        assert!(
            candidates.iter().all(|c| c.objective_score.is_none()),
            "nothing scored, so nothing may carry a score",
        );
        assert!(
            matches!(optimality, OptimalityStatus::FeasibilityOnly),
            "I4: an unscored candidate list may not carry a SCORED optimality \
             verdict. `BestFound` here would claim a ranking quality over a set \
             that could not be ranked; got {optimality:?}",
        );
    }

    /// (i) NO AUTO PARAMS, UNDER AN OBJECTIVE, IS STILL A WELL-FORMED RANKING.
    ///
    /// The degenerate shape that most easily becomes a panic or an empty list:
    /// there is nothing to search, so the enumeration returns ONE point — the
    /// empty assignment — and it is trivially both feasible and optimal. The
    /// answer must agree with `solve()`'s zero-auto fast path, which reports
    /// `Solved { values: {}, unique: true }`.
    #[test]
    fn a_problem_with_no_auto_params_ranks_as_a_single_trivially_optimal_point() {
        let p = problem_with_objective(
            Vec::new(),
            Vec::new(),
            ObjectiveSet::single(
                ObjectiveSense::Minimize,
                CompiledExpr::literal(Value::Int(7), Type::Int),
            ),
        );

        let (candidates, optimality) = ranked(CpSatSolver.solve_ranked(&p));

        assert_eq!(
            candidates.len(),
            1,
            "the empty assignment is the only assignment; got {candidates:?}",
        );
        assert!(
            candidates[0].values.is_empty(),
            "there are no autos to report values for; got {:?}",
            candidates[0].values,
        );
        assert!(
            candidates[0].unique,
            "with zero autos the empty assignment is the ONLY one, so it is \
             honestly unique — the same answer `solve()`'s fast path gives",
        );
        assert!(
            matches!(
                optimality,
                OptimalityStatus::ProvenOptimal | OptimalityStatus::FeasibilityOnly
            ),
            "a one-point space is exhausted by definition, so the verdict may \
             not be `BestFound` — nothing was left unexplored; got {optimality:?}",
        );
    }
}
