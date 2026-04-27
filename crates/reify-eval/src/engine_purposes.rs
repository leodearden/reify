// Split from lib.rs (task 2032) — purposes methods.

use crate::Engine;
use crate::cache::NodeId;
use crate::deps::ReverseDependencyIndex;
use crate::graph::ValueCellNode;
use reify_compiler::{ResolvedSchemaQuery, ValueCellKind};
use reify_types::{
    CompiledExpr, CompiledExprKind, ConstraintNodeId, ContentHash, OptimizationObjective,
    PersistentMap, Type, ValueCellId,
};

impl Engine {
    /// Activate a purpose by name against a target entity.
    ///
    /// Looks up the compiled purpose by `purpose_name`, then injects its
    /// constraints into the current evaluation graph. The injected constraint
    /// node IDs use a `"purpose:<purpose_name>@<entity_ref>"` entity prefix to
    /// avoid collisions with structure-level constraints.
    ///
    /// Requires a prior call to `eval()` so an evaluation state exists.
    /// If the purpose is already active, this is a no-op.
    pub fn activate_purpose(&mut self, purpose_name: &str, entity_ref: &str) {
        // No-op if already active
        if self.active_purposes.contains_key(purpose_name) {
            return;
        }

        // Look up the compiled purpose
        let purpose = match self
            .compiled_purposes
            .iter()
            .find(|p| p.name == purpose_name)
        {
            Some(p) => p.clone(),
            None => return, // Purpose not found — silently ignore
        };

        // Get mutable access to the evaluation state
        let state = match self.eval_state.as_mut() {
            Some(s) => s,
            None => return, // No eval state — silently ignore
        };

        // Build a unique entity prefix for the purpose-injected constraints
        let purpose_entity = format!("purpose:{}@{}", purpose_name, entity_ref);

        // Rewrite compiled expressions: substitute ValueCellId(purpose_name, param)
        // with ValueCellId(entity_ref, param) so references resolve to existing
        // value cells in the evaluation graph.
        let mut rewritten_constraints = purpose.constraints.clone();
        for constraint in &mut rewritten_constraints {
            constraint.expr.remap_entity(purpose_name, entity_ref);
        }

        let rewritten_objective = purpose.objective.clone().map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    expr.remap_entity(purpose_name, entity_ref);
                }
            }
            obj
        });

        // Expand `PurposeReflectiveAggregation` placeholders into populated
        // `ListLiteral([ValueRef(entity_ref, member), ...])` nodes against the
        // bound entity (task-2289). Walks each constraint expression and the
        // objective immediately after the `remap_entity` rewrite. The walk
        // mirrors `remap_entity`'s arm-by-arm structure so future variant
        // additions in `crates/reify-types/src/expr.rs` only need to touch the
        // same places.
        //
        // Captured shape — `&purpose.resolved_queries`, `entity_ref`, and
        // `&state.snapshot.graph.value_cells` — is identical for the
        // constraints loop and the objective rewrite, so a closure keeps the
        // call sites in lockstep (any future arg change only touches one
        // signature).
        let expand_placeholders = |expr: &mut CompiledExpr| {
            expand_purpose_reflective_placeholders(
                expr,
                &purpose.resolved_queries,
                entity_ref,
                &state.snapshot.graph.value_cells,
            );
        };
        for constraint in &mut rewritten_constraints {
            expand_placeholders(&mut constraint.expr);
        }
        let rewritten_objective = rewritten_objective.map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    expand_placeholders(expr);
                }
            }
            obj
        });

        // Inject each of the purpose's compiled constraints into the evaluation graph
        let mut injected_ids = Vec::new();
        for (i, constraint) in rewritten_constraints.iter().enumerate() {
            let constraint_id = ConstraintNodeId::new(&purpose_entity, i as u32);
            let node = crate::graph::ConstraintNodeData {
                id: constraint_id.clone(),
                label: constraint.label.clone(),
                expr: constraint.expr.clone(),
                content_hash: ContentHash::of_str(&format!(
                    "purpose:{}:constraint:{}",
                    purpose_name, i
                )),
                optimized_target: constraint.optimized_target.clone(),
            };
            state
                .snapshot
                .graph
                .constraints
                .insert(constraint_id.clone(), node);
            injected_ids.push(constraint_id);
        }

        // Update demand registry: demand each newly injected constraint node.
        for id in &injected_ids {
            self.demand.add_demand(NodeId::Constraint(id.clone()));
        }

        self.active_purposes
            .insert(purpose_name.to_string(), injected_ids);

        // Rebuild infrastructure so incremental eval (edit_param) propagates
        // through purpose constraint dependencies correctly.
        //
        // We reborrow eval_state mutably here — the immutable borrow (`state`)
        // created earlier was already released after inserting into the graph.
        if let Some(state) = self.eval_state.as_mut() {
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Inject the optimization objective if the purpose has one
        if let Some(ref objective) = rewritten_objective {
            self.active_objective_map
                .insert(purpose_name.to_string(), objective.clone());
        }
    }

    /// Deactivate a purpose by name.
    ///
    /// Removes the constraints and objectives that were injected by `activate_purpose`.
    /// If the purpose is not active, this is a no-op.
    pub fn deactivate_purpose(&mut self, purpose_name: &str) {
        // Look up and remove the injected constraint IDs
        let injected_ids = match self.active_purposes.remove(purpose_name) {
            Some(ids) => ids,
            None => return, // Not active — no-op
        };

        // Update demand registry: remove demand for each ejected constraint node.
        for id in &injected_ids {
            self.demand.remove_demand(&NodeId::Constraint(id.clone()));
        }

        // Remove each injected constraint from the evaluation graph, then
        // rebuild the infrastructure so subsequent edit_param() calls no longer
        // route through purpose constraint dependencies.
        if let Some(state) = self.eval_state.as_mut() {
            for constraint_id in &injected_ids {
                state.snapshot.graph.constraints.remove(constraint_id);
            }
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Remove the objective if one was injected
        self.active_objective_map.remove(purpose_name);
    }

    /// Check whether a purpose is currently active.
    pub fn is_purpose_active(&self, purpose_name: &str) -> bool {
        self.active_purposes.contains_key(purpose_name)
    }

    /// Returns the currently active optimization objectives (injected by purposes).
    pub fn active_objectives(&self) -> Vec<&OptimizationObjective> {
        self.active_objective_map.values().collect()
    }
}

// ─── Activation-time reflective-aggregation expansion (task-2289) ────────────

/// Walk the given expression tree and rewrite every
/// `CompiledExprKind::PurposeReflectiveAggregation` placeholder into a
/// populated `CompiledExprKind::ListLiteral` of `ValueRef(entity_ref, member)`
/// elements sourced from the bound entity. Element `result_type` is taken
/// from the looked-up `ValueCellNode.cell_type` (cell-type lockstep, task-1904
/// cross-reference); the outer list `result_type` adopts
/// `Type::List(Box::new(first_element_type))` when populated, falling back to
/// `Type::List(Box::new(Type::Real))` for the empty-list case (no resolved
/// query / no scannable params — preserves today's vacuous-true behaviour for
/// `geometric_params` / `material_params`).
///
/// Resolution strategy for the `params` query:
///   1. Prefer the compile-time `ResolvedSchemaQuery` whose `query_kind`
///      matches the placeholder. This is populated for concrete-typed
///      purpose params (`subject : Bracket`) by `compile_purpose` in
///      `reify-compiler/src/traits.rs`.
///   2. Fall back to scanning `value_cells` for cells whose `entity` matches
///      `entity_ref` and whose `kind` is `Param` or `Auto`. This handles the
///      wildcard-subject case (`subject : Structure`) where the compiler
///      cannot pre-resolve queries because the template is not known until
///      activation.
///
/// For other query kinds (`geometric_params`, `material_params`), no compile-
/// time resolution and no fallback heuristic exists yet (task-1904 territory),
/// so the placeholder rewrites to an empty list — matching today's
/// vacuous-true behaviour for those members.
///
/// The walk mirrors `CompiledExpr::remap_entity`'s arm-by-arm traversal so a
/// future variant addition only touches the same places.
///
/// CONTRACT — content-hash staleness: replacing a placeholder node updates
/// that node's `content_hash` (via `CompiledExpr::list_literal`), but
/// **does not** rebuild ancestor hashes (e.g. the enclosing `Quantifier`
/// still carries the pre-rewrite hash). This is the same posture as
/// `CompiledExpr::remap_entity` (also called above). Today this is safe
/// because the only consumer of the rewritten expression — constraint
/// injection at lines ~96–113 — reseeds each constraint's `content_hash`
/// from `purpose:<name>:constraint:<i>` independently of the expression
/// hash, so injected nodes are never observed at their stale hash. If a
/// future caller relies on `expr.content_hash` for sub-expression cache
/// lookups inside the rewritten tree, add a bottom-up hash-rebuild pass
/// here (and on `remap_entity`/`remap_cell`).
fn expand_purpose_reflective_placeholders(
    expr: &mut CompiledExpr,
    queries: &[ResolvedSchemaQuery],
    entity_ref: &str,
    value_cells: &PersistentMap<ValueCellId, ValueCellNode>,
) {
    match &mut expr.kind {
        CompiledExprKind::PurposeReflectiveAggregation {
            param_name,
            query_kind,
        } => {
            // Resolve the member list for this placeholder. Prefer compile-
            // time `ResolvedSchemaQuery`; fall back to scanning `value_cells`
            // for the bound entity's params when the query is unresolved
            // (wildcard-subject case).
            //
            // The lookup filters by *both* `param_name` and `query_kind` —
            // the placeholder records which purpose param it was projected
            // from (e.g. `subject` vs a hypothetical `part`), and so does
            // each `ResolvedSchemaQuery`. Today's compiler only accepts a
            // single StructureRef param (task-2201), so collisions are
            // latent — but matching by query_kind alone would silently
            // misbind in any future multi-param purpose. Defending now is
            // cheaper than debugging that misbind later.
            let members: Vec<String> = if let Some(q) = queries
                .iter()
                .find(|q| q.param_name == *param_name && q.query_kind == *query_kind)
            {
                q.resolved_ids.iter().map(|id| id.member.clone()).collect()
            } else if query_kind == "params" {
                let mut members: Vec<String> = value_cells
                    .iter()
                    .filter(|(id, node)| {
                        id.entity == entity_ref
                            && matches!(
                                node.kind,
                                ValueCellKind::Param | ValueCellKind::Auto { .. }
                            )
                    })
                    .map(|(id, _)| id.member.clone())
                    .collect();
                // PersistentMap iteration order is not guaranteed stable
                // across runs; sort for determinism. Downstream tests
                // sort before comparison, so the surface order is not
                // load-bearing — but a stable order keeps logs and
                // hashes reproducible.
                members.sort();
                members
            } else {
                // geometric_params / material_params: no resolution path
                // yet (task-1904). Empty list ⇒ vacuous-true forall, same
                // as before this expansion existed.
                Vec::new()
            };

            // Build ValueRef elements with cell-type lockstep.
            let elements: Vec<CompiledExpr> = members
                .iter()
                .map(|member| {
                    let cell_id = ValueCellId::new(entity_ref, member);
                    let elem_type = value_cells
                        .get(&cell_id)
                        .map(|node| node.cell_type.clone())
                        .unwrap_or(Type::Real);
                    CompiledExpr::value_ref(cell_id, elem_type)
                })
                .collect();

            // Outer ListLiteral type: inherit first element's type when
            // populated; default to Type::Real on empty (anti-cascade).
            let element_type = elements
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or(Type::Real);
            *expr = CompiledExpr::list_literal(elements, Type::List(Box::new(element_type)));
        }
        CompiledExprKind::ValueRef(_)
        | CompiledExprKind::Literal(_)
        | CompiledExprKind::OptionNone
        | CompiledExprKind::MetaAccess { .. }
        | CompiledExprKind::DeterminacyPredicate { .. } => {
            // No children carrying potential placeholders.
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            expand_purpose_reflective_placeholders(left, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(right, queries, entity_ref, value_cells);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            expand_purpose_reflective_placeholders(operand, queries, entity_ref, value_cells);
        }
        CompiledExprKind::FunctionCall { args, .. }
        | CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            expand_purpose_reflective_placeholders(condition, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(then_branch, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(else_branch, queries, entity_ref, value_cells);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            expand_purpose_reflective_placeholders(discriminant, queries, entity_ref, value_cells);
            for arm in arms {
                expand_purpose_reflective_placeholders(
                    &mut arm.body,
                    queries,
                    entity_ref,
                    value_cells,
                );
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            expand_purpose_reflective_placeholders(body, queries, entity_ref, value_cells);
        }
        CompiledExprKind::ListLiteral(elements) | CompiledExprKind::SetLiteral(elements) => {
            for elem in elements {
                expand_purpose_reflective_placeholders(elem, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                expand_purpose_reflective_placeholders(key, queries, entity_ref, value_cells);
                expand_purpose_reflective_placeholders(val, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            expand_purpose_reflective_placeholders(object, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(index, queries, entity_ref, value_cells);
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            expand_purpose_reflective_placeholders(object, queries, entity_ref, value_cells);
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            expand_purpose_reflective_placeholders(collection, queries, entity_ref, value_cells);
            expand_purpose_reflective_placeholders(predicate, queries, entity_ref, value_cells);
        }
        CompiledExprKind::OptionSome(inner) => {
            expand_purpose_reflective_placeholders(inner, queries, entity_ref, value_cells);
        }
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                expand_purpose_reflective_placeholders(lo, queries, entity_ref, value_cells);
            }
            if let Some(hi) = upper {
                expand_purpose_reflective_placeholders(hi, queries, entity_ref, value_cells);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            expand_purpose_reflective_placeholders(base, queries, entity_ref, value_cells);
            for arg in args {
                expand_purpose_reflective_placeholders(arg, queries, entity_ref, value_cells);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests that drive `expand_purpose_reflective_placeholders`
    //! directly, bypassing `compile_purpose`. Useful for pinning contract
    //! invariants whose witness in an integration test would depend on
    //! incidental compiler behaviour (e.g. declaration-order preservation
    //! in `ResolvedSchemaQuery.resolved_ids`).
    use super::*;
    use crate::graph::ValueCellNode;

    /// Reviewer suggestion S2 (amendment round 2): pin the precedence
    /// contract — when a matching `ResolvedSchemaQuery` is supplied, its
    /// `resolved_ids` order wins over the alphabetical fallback scan of
    /// `value_cells`. Hand-crafting the inputs here means the test
    /// doesn't bake in `compile_purpose`'s incidental ordering: a future
    /// refactor that sorts inside the compiler would have made the
    /// integration-level witness vacuous (both paths would have produced
    /// the same alphabetical order), but this test stays sharp.
    #[test]
    fn expand_prefers_resolved_query_over_value_cells_scan() {
        let entity = "Foo";
        let cell_z = ValueCellId::new(entity, "z");
        let cell_a = ValueCellId::new(entity, "a");

        // Hand-crafted ResolvedSchemaQuery — order [z, a], NOT
        // alphabetical. The fallback scan would sort to [a, z], so the
        // two paths disagree on the witness.
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_z.clone(), cell_a.clone()],
        }];

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> =
            PersistentMap::default();
        for cell in [&cell_a, &cell_z] {
            value_cells.insert(
                cell.clone(),
                ValueCellNode {
                    id: cell.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&cell.member),
                },
            );
        }

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        let elements = match &expr.kind {
            CompiledExprKind::ListLiteral(elements) => elements,
            other => panic!("expected ListLiteral, got {:?}", other),
        };
        let expanded_order: Vec<&str> = elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.member.as_str(),
                other => panic!("expected ValueRef element, got {:?}", other),
            })
            .collect();
        assert_eq!(
            expanded_order,
            vec!["z", "a"],
            "resolved-query path must preserve resolved_ids order; \
             a [a, z] result would indicate the fallback scan won precedence"
        );
    }

    /// Guard: when a `ResolvedSchemaQuery` references a cell that is absent
    /// from `value_cells`, the missing-cell branch must emit exactly one WARN
    /// event scoped to `reify_eval::engine_purposes` (graph-vs-resolved-query
    /// wiring inconsistency signal).
    ///
    /// The test wraps the call in `catch_unwind` so it runs identically in
    /// both debug builds (where `debug_assert!(false)` panics) and release
    /// builds (where it does not). The WARN fires *before* the
    /// `debug_assert!`, so the counter increments regardless of build mode.
    #[test]
    fn expand_signals_when_resolved_query_cell_missing_from_value_cells() {
        use std::panic::AssertUnwindSafe;
        use std::sync::atomic::Ordering;
        use reify_test_support::CountingSubscriberBuilder;

        let entity = "Foo";
        let cell_present = ValueCellId::new(entity, "present");
        let cell_absent = ValueCellId::new(entity, "absent");

        // Query references both "present" and "absent" cells.
        let queries = vec![ResolvedSchemaQuery {
            param_name: "subject".to_string(),
            query_kind: "params".to_string(),
            resolved_ids: vec![cell_present.clone(), cell_absent.clone()],
        }];

        // value_cells contains ONLY the "present" cell — "absent" is
        // deliberately missing to trigger the new missing-cell branch.
        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> =
            PersistentMap::default();
        value_cells.insert(
            cell_present.clone(),
            ValueCellNode {
                id: cell_present.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("present"),
            },
        );

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::engine_purposes")
            .build();
        let warn_arc = counters[&tracing::Level::WARN].clone();

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        // Wrap in catch_unwind so debug builds (debug_assert! panics) and
        // release builds both complete and let us read the warn counter.
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tracing::subscriber::with_default(subscriber, || {
                expand_purpose_reflective_placeholders(
                    &mut expr,
                    &queries,
                    entity,
                    &value_cells,
                );
            });
        }));

        assert_eq!(
            warn_arc.load(Ordering::Acquire),
            1,
            "missing-cell wiring signal: expected exactly one WARN from \
             reify_eval::engine_purposes when a resolved-query cell is absent \
             from value_cells"
        );
    }

    /// Companion: when no matching `ResolvedSchemaQuery` is supplied
    /// (e.g. the wildcard-subject case), expansion falls back to a
    /// sorted scan of `value_cells` for the bound entity. This locks in
    /// the fallback's stable ordering — a regression that returned
    /// PersistentMap iteration order would break determinism.
    #[test]
    fn expand_falls_back_to_sorted_value_cells_scan_when_query_unresolved() {
        let entity = "Foo";
        let cell_z = ValueCellId::new(entity, "z");
        let cell_a = ValueCellId::new(entity, "a");

        // Empty queries — forces the fallback path for `params`.
        let queries: Vec<ResolvedSchemaQuery> = Vec::new();

        let mut value_cells: PersistentMap<ValueCellId, ValueCellNode> =
            PersistentMap::default();
        // Insert in non-alphabetical order to exercise the sort.
        for cell in [&cell_z, &cell_a] {
            value_cells.insert(
                cell.clone(),
                ValueCellNode {
                    id: cell.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(&cell.member),
                },
            );
        }

        let mut expr = CompiledExpr::purpose_reflective_aggregation(
            "subject".to_string(),
            "params".to_string(),
            Type::List(Box::new(Type::Real)),
        );

        expand_purpose_reflective_placeholders(&mut expr, &queries, entity, &value_cells);

        let elements = match &expr.kind {
            CompiledExprKind::ListLiteral(elements) => elements,
            other => panic!("expected ListLiteral, got {:?}", other),
        };
        let expanded_order: Vec<&str> = elements
            .iter()
            .map(|e| match &e.kind {
                CompiledExprKind::ValueRef(id) => id.member.as_str(),
                other => panic!("expected ValueRef element, got {:?}", other),
            })
            .collect();
        assert_eq!(
            expanded_order,
            vec!["a", "z"],
            "fallback scan must sort alphabetically for determinism"
        );
    }
}
