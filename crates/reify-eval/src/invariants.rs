//! Task α (PRD `docs/prds/v0_6/eval-uniform-dependency-handling.md` §6.1):
//! the no-stale-Undef invariant checker.
//!
//! `check_no_stale_undef` is a pure function over retained post-eval state
//! (graph + values + trace_map + functions) that reports every value cell
//! which is currently `Value::Undef` despite having a `default_expr` and
//! every static dependency resolved — the "causeless staleness" §6.1 exists
//! to catch. A thin `Engine::check_no_stale_undef` wrapper (added in a later
//! step) threads the Engine's own retained `eval_state()` into this free
//! function for the debug-gate corpus harness.
//!
//! Lives in-crate (not in an integration test) because `Engine.functions` is
//! a private field — the future `@optimized` exclusion (§6.1 clause 5) needs
//! in-crate access to thread it through.

use std::collections::{HashMap, HashSet};

use reify_core::{RealizationNodeId, ValueCellId};
use reify_ir::{CompiledExprKind, CompiledFunction, DeterminacyState, PersistentMap, Value};

use crate::cache::NodeId;
use crate::deps::DependencyTrace;
use crate::graph::EvaluationGraph;

/// A single no-stale-Undef violation: `cell` is `Value::Undef` even though
/// every one of its static dependencies is resolved (§6.1 clause 4) and no
/// exclusion (auto, `@optimized`, guard-inactive, undef-literal) applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleUndefViolation {
    pub cell: ValueCellId,
    pub detail: String,
}

/// §6.1: report every value cell that is a stale-Undef violation.
///
/// A violation is a value cell `c` such that ALL of:
/// 1. `c` is not an auto cell;
/// 2. `c` has a `default_expr`;
/// 3. `values[c]` is `Value::Undef` (or absent from `values` entirely);
/// 4. every static dep in `trace(c).reads ∪ trace(c).realization_reads` is
///    resolved (present and non-Undef) — a missing-producer read, or a read
///    that is itself Undef, makes `c` EXEMPT, not violating;
/// 5. `c` is not `@optimized`-dispatched (a `UserFunctionCall` default_expr
///    whose matching `CompiledFunction` has `optimized_target: Some(..)` —
///    the exact predicate R3e uses, `engine_eval.rs`'s
///    `re_eval_consumers_of_in_walk_mints`, to avoid clobbering a
///    compute-dispatch result) and `c` is not a guard-inactive member of a
///    `GuardedGroupInfo` (mirroring `EvaluationGraph::active_constraint_ids`'s
///    guard-value dispatch, but over `members`/`else_members` rather than
///    `constraints`/`else_constraints`);
/// 6. `c`'s `default_expr` is not a `Literal(Value::Undef)` (the UserUndef
///    `param u = undef` / `let x = undef` shape) — α's sanctioned
///    exclusion-set refinement per §11 Q2/Q4, structural rather than via
///    the `undef_causes` map, beyond §6.1's two literal clause-5
///    exclusions. Deliberately does NOT exclude a pure op-contract-failure
///    cell (all deps resolved, the op itself returns `Undef`) — only task γ
///    (the op-sink) can identify that case (`UndefCause::OpContractFailed`,
///    `value.rs:3733-3788`); α honestly cannot distinguish it from genuine
///    staleness.
pub fn check_no_stale_undef(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    trace_map: &HashMap<NodeId, DependencyTrace>,
    functions: &[CompiledFunction],
) -> Vec<StaleUndefViolation> {
    let empty_trace = DependencyTrace::default();
    let guard_inactive_members = guard_inactive_members(graph, values);
    let mut violations = Vec::new();

    for (id, cell) in graph.value_cells.iter() {
        // Clause 1: auto cells are solver-owned; not candidates.
        if cell.kind.is_auto() {
            continue;
        }
        // Clause 2: no default_expr => nothing to have gone stale.
        let Some(expr) = cell.default_expr.as_ref() else {
            continue;
        };
        // Clause 3: only a currently-Undef cell can be stale.
        if !matches!(cell_value_or_undef(values, id), Value::Undef) {
            continue;
        }

        // Clause 4: EXEMPT (not a violation) if any static dep is missing
        // or itself unresolved.
        let trace = trace_map
            .get(&NodeId::Value(id.clone()))
            .unwrap_or(&empty_trace);
        let reads_resolved = trace
            .reads
            .iter()
            .all(|dep| value_cell_dep_is_resolved(graph, values, dep));
        let realization_reads_resolved = trace
            .realization_reads
            .iter()
            .all(|rid| realization_dep_is_resolved(graph, values, rid));
        if !reads_resolved || !realization_reads_resolved {
            continue;
        }

        // Clause 5a: `@optimized` UserFunctionCall — the EXACT predicate
        // R3e uses (engine_eval.rs's re_eval_consumers_of_in_walk_mints,
        // ~line 6759+) to avoid misclassifying a compute-dispatched cell
        // (evaluated outside the plain expr-eval path) as stale.
        if let CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } = &expr.kind
            && reify_expr::find_matching_compiled_function(functions, function_name, args)
                .and_then(|f| f.optimized_target.clone())
                .is_some()
        {
            continue;
        }

        // Clause 5b: guard-inactive member — this cell belongs to the
        // branch of a `GuardedGroupInfo` that the current guard value does
        // NOT select, so its Undef is expected (unevaluated branch), not
        // stale.
        if guard_inactive_members.contains(id) {
            continue;
        }

        // Clause 6: UserUndef structural exclusion (§11 Q2/Q4 — α's
        // sanctioned exclusion-set refinement beyond §6.1's two literal
        // clause-5 exclusions). A `param u = undef` / `let x = undef`
        // default is author-intended, not stale: clause 4 cannot exempt it
        // (zero deps is vacuously "all resolved"), so it is excluded
        // structurally — by the *shape* of `default_expr` — instead of via
        // the `undef_causes` map (this checker stays a pure function of
        // graph/values/trace_map/functions; the harness never calls
        // `set_capture_undef_causes`). Deliberately NOT excluded here: a
        // pure op-contract-failure cell (all deps resolved, yet the op
        // itself returns `Undef`, e.g. `sqrt` of a resolved-negative) — α
        // cannot distinguish that from genuine staleness;
        // `UndefCause::OpContractFailed` is recorded by task γ (the
        // op-sink) only (value.rs:3733-3788), never by α. Unbound/
        // propagated Undefs are already exempt via clause 4;
        // AwaitingSolve/SolveFailed auto cells via clause 1.
        if matches!(&expr.kind, CompiledExprKind::Literal(Value::Undef)) {
            continue;
        }

        violations.push(StaleUndefViolation {
            cell: id.clone(),
            detail: format!(
                "value cell {id:?} is Undef but all {} static dependency(ies) \
                 (reads + realization_reads) are resolved",
                trace.reads.len() + trace.realization_reads.len()
            ),
        });
    }

    violations
}

impl crate::Engine {
    /// Debug-gate corpus-harness wrapper: threads this Engine's own retained
    /// post-eval state (`eval_state()`) and function table into the
    /// free-function [`check_no_stale_undef`]. Returns an empty `Vec` when no
    /// eval has run yet (`eval_state()` is `None`) — there is no retained
    /// state to check.
    pub fn check_no_stale_undef(&self) -> Vec<StaleUndefViolation> {
        let Some(state) = self.eval_state() else {
            return Vec::new();
        };
        check_no_stale_undef(
            &state.snapshot.graph,
            &state.snapshot.values,
            &state.trace_map,
            &self.functions,
        )
    }
}

/// Computes the set of value cells that are members of the currently
/// INACTIVE branch of some `GuardedGroupInfo` in `graph.guarded_groups`.
/// Mirrors `EvaluationGraph::active_constraint_ids`'s guard-value dispatch
/// (`graph.rs`), but over `members`/`else_members` (`ValueCellId`) rather
/// than `constraints`/`else_constraints` (`ConstraintNodeId`):
/// - guard is `Bool(true)`  => `else_members` are inactive;
/// - guard is `Bool(false)` => `members` are inactive;
/// - guard is `Undef`/non-`Bool` (or missing from `values`) => BOTH
///   `members` and `else_members` are inactive (neither branch selected).
fn guard_inactive_members(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) -> HashSet<ValueCellId> {
    let mut inactive = HashSet::new();
    for group in &graph.guarded_groups {
        match cell_value_or_undef(values, &group.guard_cell) {
            Value::Bool(true) => inactive.extend(group.else_members.iter().cloned()),
            Value::Bool(false) => inactive.extend(group.members.iter().cloned()),
            _ => {
                inactive.extend(group.members.iter().cloned());
                inactive.extend(group.else_members.iter().cloned());
            }
        }
    }
    inactive
}

/// Look up `id`'s stored value in the retained post-eval values map,
/// treating an absent entry the same as `Value::Undef` (§6.1 clause 3
/// explicitly includes "or absent"). Mirrors `ValueMap::get_or_undef`
/// (`reify_ir::value::ValueMap`) for this crate's
/// `PersistentMap<ValueCellId, (Value, DeterminacyState)>` snapshot shape,
/// which — being a bare tuple map, not a `ValueMap` — has no such method of
/// its own.
fn cell_value_or_undef(
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    id: &ValueCellId,
) -> Value {
    values
        .get(id)
        .map(|(value, _determinacy)| value.clone())
        .unwrap_or(Value::Undef)
}

/// A `trace.reads` dependency is resolved iff its producer is present in
/// the graph AND its stored value is non-Undef. A missing producer or an
/// Undef producer both make the exemption fire (erring EXEMPT, never
/// falsely violating) — §6.1 clause 4.
fn value_cell_dep_is_resolved(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    dep: &ValueCellId,
) -> bool {
    if !graph.value_cells.contains_key(dep) {
        return false; // missing-producer
    }
    !matches!(cell_value_or_undef(values, dep), Value::Undef)
}

/// A `trace.realization_reads` dependency is resolved iff the realization is
/// present in the graph AND it names a backing geometry value cell that is
/// itself present and non-Undef. A realization with no backing
/// `geometry_cell` link cannot be checked for Undef-ness at all — treated as
/// unresolved (erring EXEMPT on ambiguity, per the same clause-4 policy).
fn realization_dep_is_resolved(
    graph: &EvaluationGraph,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    rid: &RealizationNodeId,
) -> bool {
    let Some(rnode) = graph.realizations.get(rid) else {
        return false; // missing-producer
    };
    match &rnode.geometry_cell {
        Some(cell) => value_cell_dep_is_resolved(graph, values, cell),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_core::{ContentHash, Type};
    use reify_ir::{CompiledExpr, CompiledExprKind, CompiledFnBody};

    use crate::graph::{GuardedGroupInfo, ValueCellNode};

    /// Fabricates a `Let` value cell with the given `default_expr`, registers
    /// it in `graph.value_cells`, and stamps its `(value, determinacy)` into
    /// `values`. Returns the cell's `ValueCellId`.
    #[allow(clippy::too_many_arguments)]
    fn seed_cell(
        graph: &mut EvaluationGraph,
        values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        entity: &str,
        member: &str,
        default_expr: CompiledExpr,
        value: Value,
        determinacy: DeterminacyState,
    ) -> ValueCellId {
        let id = ValueCellId::new(entity, member);
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Let,
                cell_type: Type::length(),
                default_expr: Some(default_expr),
                content_hash: ContentHash::of_str(&format!("{entity}.{member}")),
            },
        );
        values.insert(id.clone(), (value, determinacy));
        id
    }

    /// A zero-arg `CompiledFunction` whose `optimized_target` is `Some(target)`
    /// — matched by `find_matching_compiled_function` against a zero-arg
    /// `UserFunctionCall { function_name: name, args: vec![] }` (arity 0 == 0,
    /// vacuously "all params match").
    fn zero_arg_optimized_function(name: &str, target: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            params: vec![],
            param_defaults: vec![],
            return_type: Type::length(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: CompiledExpr::literal(Value::length(0.0), Type::length()),
            },
            content_hash: ContentHash::of_str(name),
            annotations: vec![],
            optimized_target: Some(target.to_string()),
            type_params: vec![],
        }
    }

    /// §6.1 clause-5 exclusions (RED until step-4 implements them): an
    /// `@optimized` `UserFunctionCall` cell and a guard-inactive member must
    /// NOT be reported, even though both are otherwise indistinguishable from
    /// a genuine stale-Undef violation under clauses 1-4 alone.
    #[test]
    fn optimized_and_guard_inactive_cells_are_excluded() {
        let mut graph = EvaluationGraph::default();
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();

        // A shared, resolved producer so every candidate below has "all deps
        // resolved" — isolating clause-5 as the only thing under test.
        let producer_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "producer",
            CompiledExpr::literal(Value::length(1.0), Type::length()),
            Value::length(1.0),
            DeterminacyState::Determined,
        );

        // The genuine violation: causeless staleness, no exclusion applies.
        let genuine_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "genuine",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(genuine_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );

        // (a) @optimized UserFunctionCall — zero-arg call matching a
        // fabricated CompiledFunction whose optimized_target is Some(..). No
        // trace_map entry => empty trace => vacuously "all deps resolved".
        let optimized_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "optimized",
            CompiledExpr {
                kind: CompiledExprKind::UserFunctionCall {
                    function_name: "opt_fn".to_string(),
                    args: vec![],
                },
                result_type: Type::length(),
                content_hash: ContentHash::of_str("optimized-call"),
            },
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        let functions = vec![zero_arg_optimized_function(
            "opt_fn",
            "test::optimized_target",
        )];

        // (b) guard-inactive member: guard resolves Bool(false), so
        // `members` (not `else_members`) are inactive.
        let guard_id = ValueCellId::new("Clause5", "guard");
        values.insert(
            guard_id.clone(),
            (Value::Bool(false), DeterminacyState::Determined),
        );
        let guard_member_id = seed_cell(
            &mut graph,
            &mut values,
            "Clause5",
            "guard_member",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(guard_member_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );
        graph.guarded_groups.push(GuardedGroupInfo {
            guard_cell: guard_id,
            members: vec![guard_member_id.clone()],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
        });

        let violations = check_no_stale_undef(&graph, &values, &trace_map, &functions);

        assert_eq!(
            violations.len(),
            1,
            "expected ONLY the genuine violation; @optimized cell {optimized_id:?} \
             and guard-inactive member {guard_member_id:?} must be excluded, got \
             {violations:?}"
        );
        assert_eq!(violations[0].cell, genuine_id);
    }

    /// §11 Q2/Q4 UserUndef structural exclusion (RED until step-6): a
    /// `param u = undef` / `let x = undef` cell — `default_expr` is a
    /// `Literal(Value::Undef)`, value `Undef`, ZERO deps — is the one case
    /// clause 4 CANNOT exempt (an empty dep set is vacuously "all resolved").
    /// Without a structural skip, every author-intended undef default would
    /// be misreported as a causeless violation.
    #[test]
    fn undef_literal_default_is_excluded() {
        let mut graph = EvaluationGraph::default();
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::new();
        let mut trace_map: HashMap<NodeId, DependencyTrace> = HashMap::new();

        // A shared, resolved producer so the genuine violation has "all deps
        // resolved" — isolating the undef-literal skip as the only thing
        // under test.
        let producer_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "producer",
            CompiledExpr::literal(Value::length(1.0), Type::length()),
            Value::length(1.0),
            DeterminacyState::Determined,
        );

        // The genuine violation: causeless staleness, no exclusion applies.
        let genuine_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "genuine",
            CompiledExpr::value_ref(producer_id.clone(), Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );
        trace_map.insert(
            NodeId::Value(genuine_id.clone()),
            DependencyTrace {
                reads: vec![producer_id.clone()],
                realization_reads: Vec::new(),
            },
        );

        // UserUndef: `param u = undef` shape — default_expr is Literal(Undef),
        // value Undef, ZERO deps (no trace_map entry => empty trace, so
        // clause 4's "every dep resolved" is vacuously true).
        let user_undef_id = seed_cell(
            &mut graph,
            &mut values,
            "UndefLiteral",
            "user_undef",
            CompiledExpr::literal(Value::Undef, Type::length()),
            Value::Undef,
            DeterminacyState::Undetermined,
        );

        let violations = check_no_stale_undef(&graph, &values, &trace_map, &[]);

        assert_eq!(
            violations.len(),
            1,
            "expected ONLY the genuine violation; the undef-literal (UserUndef) \
             cell {user_undef_id:?} must be excluded, got {violations:?}"
        );
        assert_eq!(violations[0].cell, genuine_id);
    }
}
