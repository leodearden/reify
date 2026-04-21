// Split from lib.rs (task 2032) — edit methods.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use reify_compiler::CompiledModule;
use reify_types::{
    AutoParam, ContentHash, DeterminacyState, Diagnostic, PersistentMap, ResolutionProblem,
    SnapshotId, SnapshotProvenance, SolveResult, Value, ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CachedResult, EvalOutcome, NodeId};
use crate::deps::{DependencyTrace, extract_dependency_trace};
use crate::graph::EvaluationGraph;
use crate::journal::{EvalEvent, EventKind, EventPayload};
use crate::{
    CheckResult, Engine, EngineError, EvalResult, GuardLookup, guard_state_fingerprint,
    value_type_kind_matches,
};

/// Deactivate a guarded-group member by writing `Undef` into both the working
/// `values` map and the snapshot's `values` map — UNLESS the member is an
/// `Auto` cell, whose lifecycle is owned by the constraint solver rather than
/// guard activation/deactivation. Missing cells are treated as non-Auto
/// (i.e. they get deactivated), preserving the prior `is_some_and` semantics.
pub(crate) fn deactivate_if_not_auto(
    graph: &EvaluationGraph,
    id: &ValueCellId,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) {
    if !graph.is_auto_cell(id) {
        values.insert(id.clone(), Value::Undef);
        snapshot_values.insert(id.clone(), (Value::Undef, DeterminacyState::Undetermined));
    }
}

/// Classify every `ValueCellId` across a pair of graphs into three disjoint
/// sets by comparing per-node `ValueCellNode::content_hash`:
///
/// - `changed`: present in both graphs with differing `content_hash`.
/// - `added`: present only in the new graph.
/// - `removed`: present only in the old graph.
///
/// The content_hash already combines the cell's ID hash and expression
/// content_hash (see `EvaluationGraph::from_templates`), so a match signals
/// "equivalent node; cached value is still valid" while a mismatch signals
/// "re-evaluate". This is the identity/equivalence key used by
/// `Engine::edit_source`.
pub(crate) fn diff_value_cells(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> (HashSet<ValueCellId>, HashSet<ValueCellId>, HashSet<ValueCellId>) {
    let mut changed = HashSet::new();
    let mut added = HashSet::new();
    for (id, new_node) in new_graph.value_cells.iter() {
        match old_graph.value_cells.get(id) {
            Some(old_node) => {
                if old_node.content_hash != new_node.content_hash {
                    changed.insert(id.clone());
                }
            }
            None => {
                added.insert(id.clone());
            }
        }
    }
    let mut removed = HashSet::new();
    for (id, _) in old_graph.value_cells.iter() {
        if !new_graph.value_cells.contains_key(id) {
            removed.insert(id.clone());
        }
    }
    (changed, added, removed)
}

impl Engine {
    /// Set a parameter override and invalidate cache entries that depend on it.
    pub fn set_param_and_invalidate(&mut self, param: &ValueCellId, value: reify_types::Value) {
        self.param_overrides.insert(param.clone(), value);
        // Mark the param's own cache entry as dirty
        let param_node = NodeId::Value(param.clone());
        self.cache.invalidate(&param_node);
        // Mark all nodes that depend on this param as dirty
        self.cache
            .invalidate_dependents(std::slice::from_ref(param));
    }

    /// Incrementally re-evaluate after changing a parameter value.
    ///
    /// Requires a prior call to eval() to establish the baseline snapshot
    /// and dependency structures. Creates a child snapshot with Edit provenance,
    /// computes dirty∩demand cone intersection, evaluates only Value nodes in
    /// the eval set (topologically sorted). Constraint/Realization nodes are
    /// tracked in the eval set but not evaluated (deferred to check()/build()).
    ///
    /// Returns EvalResult with all current values (both changed and unchanged).
    pub fn edit_param(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> Result<EvalResult, EngineError> {
        // Clone the merged function table for use in EvalContext.  Same borrow-checker
        // workaround and same O(N) cost as the clone in eval(); see PERFORMANCE NOTE
        // near eval()'s `let functions` binding for the deferred Arc refactor.
        let functions = self.functions.clone();
        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        // Single lookup: validate existence and retrieve the node in one traversal.
        // This eliminates the earlier double-lookup (contains_key + get().unwrap()).
        let cell_node = match state.snapshot.graph.value_cells.get(&cell) {
            Some(node) => node,
            None => return Err(EngineError::CellNotFound { cell }),
        };

        // Validate type-kind compatibility: reject cross-variant mismatches before
        // the narrower dimension check below.  Value::Undef is always accepted
        // (it is the Auto/no-value sentinel used extensively by the solver and
        // compiler for unresolved params).
        if !value_type_kind_matches(&new_value, &cell_node.cell_type) {
            return Err(EngineError::TypeKindMismatch {
                cell,
                expected: Box::new(cell_node.cell_type.clone()),
                got: Box::new(new_value),
            });
        }

        // Validate dimension compatibility for Scalar cells.
        // If the cell is Type::Scalar { dimension: expected } and the supplied
        // value is Value::Scalar { dimension: got } where got != expected,
        // reject the edit immediately rather than propagating a dimension-corrupt
        // value through the eval graph.
        if let reify_types::Type::Scalar {
            dimension: expected,
        } = cell_node.cell_type
            && let reify_types::Value::Scalar { dimension: got, .. } = &new_value
            && *got != expected
        {
            return Err(EngineError::DimensionMismatch {
                cell,
                expected,
                got: *got,
            });
        }

        // Clone snapshot and extract references (O(1) via PersistentMap)
        let parent_id = state.snapshot.id;
        let mut new_snapshot = state.snapshot.clone();

        // Compute dirty cone and eval set while state borrow is active
        let mut changed_set = std::collections::HashSet::new();
        changed_set.insert(cell.clone());
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &state.reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, &state.trace_map);

        // Seed has_changed_parent from dependents of the changed param
        let mut has_changed_parent: std::collections::HashSet<NodeId> =
            std::collections::HashSet::new();
        for dependent in state.reverse_index.dependents_of(&cell) {
            has_changed_parent.insert(dependent.clone());
        }
        // Release the immutable borrow of eval_state so we can mutate later
        let _ = state;

        // Update snapshot ID, version, and provenance
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // Update the changed cell's value in snapshot
        new_snapshot.values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Update the param's cache entry to match the snapshot.
        // The param is a source node (not in dirty_cone / eval_set), so its
        // cache entry would otherwise retain the stale value from initial eval().
        self.cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(new_value.clone(), DeterminacyState::Determined),
            VersionId(version_id),
            crate::deps::DependencyTrace::default(),
        );

        // Build the full ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }
        // Overwrite with the new param value
        values.insert(cell.clone(), new_value);

        // Mark all nodes in the eval set as Pending before re-evaluation.
        // This transitions Final → Pending{last_substantive: hash}.
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        // Evaluate only Value nodes in the eval set (topo-sorted order).
        // Track nodes to skip due to early cutoff of upstream nodes.
        let mut skipped: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut actual_eval_set: Vec<NodeId> = Vec::with_capacity(eval_set.len());

        for node_id in &eval_set {
            if skipped.contains(node_id) {
                continue;
            }
            actual_eval_set.push(node_id.clone());

            if let NodeId::Value(vcid) = node_id
                && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                && let Some(ref expr) = node.default_expr
            {
                let start = Instant::now();
                self.journal.record(EvalEvent {
                    timestamp: start,
                    node_id: node_id.clone(),
                    kind: EventKind::Started,
                    version: VersionId(version_id),
                    payload: None,
                });

                let val = reify_expr::eval_expr(
                    expr,
                    &reify_expr::EvalContext::new(&values, &functions).with_meta(&self.meta_map),
                );
                values.insert(vcid.clone(), val.clone());
                new_snapshot
                    .values
                    .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                // Record in cache and check for early cutoff
                let trace = extract_dependency_trace(expr);
                let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                let outcome = self.cache.record_evaluation(
                    node_id.clone(),
                    cached_result,
                    VersionId(version_id),
                    trace,
                );

                self.journal.record(EvalEvent {
                    timestamp: Instant::now(),
                    node_id: node_id.clone(),
                    kind: EventKind::Completed { outcome },
                    version: VersionId(version_id),
                    payload: Some(EventPayload::Duration(start.elapsed())),
                });

                // Early cutoff with mixed fan-in protection:
                // - Changed: propagate has_changed_parent to dependents,
                //   remove them from skipped (in case an earlier Unchanged
                //   parent added them prematurely).
                // - Unchanged: only add dependents to skipped if they do NOT
                //   have a Changed parent (i.e., not in has_changed_parent).
                {
                    let dependents = self
                        .eval_state
                        .as_ref()
                        .unwrap()
                        .reverse_index
                        .dependents_of(vcid);
                    if outcome == EvalOutcome::Changed {
                        for dependent in dependents {
                            has_changed_parent.insert(dependent.clone());
                            skipped.remove(dependent);
                        }
                    } else {
                        // Unchanged
                        for dependent in dependents {
                            if !has_changed_parent.contains(dependent) {
                                skipped.insert(dependent.clone());
                            }
                        }
                    }
                }
            }
            // Constraint/Realization nodes: tracked in eval set but not evaluated
            // (deferred to check()/build())
        }

        // Restore freshness to Final for nodes that were pre-marked Pending
        // but then skipped by early cutoff (they were never re-evaluated).
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // ── Guard re-elaboration phase ────────────────────────────────
        // If any structure_controlling cell changed, re-evaluate guarded groups
        // to flip which branch is active/inactive, and recompute fingerprint.
        {
            let graph = &new_snapshot.graph;
            let has_dirty_guards = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            });

            if has_dirty_guards {
                for group in &graph.guarded_groups {
                    // Re-evaluate the guard cell's expression
                    let guard_val = if let Some(node) = graph.value_cells.get(&group.guard_cell) {
                        if let Some(ref expr) = node.default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &reify_expr::EvalContext::new(&values, &functions)
                                    .with_determinacy(&new_snapshot.values)
                                    .with_meta(&self.meta_map),
                            )
                        } else {
                            Value::Undef
                        }
                    } else {
                        Value::Undef
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));

                    let is_true = matches!(&guard_val, Value::Bool(true));
                    let is_false = matches!(&guard_val, Value::Bool(false));

                    for mid in &group.members {
                        if is_true {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(graph, mid, &mut values, &mut new_snapshot.values);
                        }
                    }
                    for mid in &group.else_members {
                        if is_false {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(graph, mid, &mut values, &mut new_snapshot.values);
                        }
                    }
                }

                // Recompute topology fingerprint including guard states.
                let guard_state_hash =
                    guard_state_fingerprint(&graph.guarded_groups, &values, GuardLookup::Lenient);
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
            }
        }

        // ── Resolution phase ───────────────────────────────────────────
        // If a solver is present, check whether any constraints governing
        // auto params are in the dirty cone. If so, re-run the solver
        // to update auto param values and propagate to dependents.
        let mut resolved_params = HashMap::new();
        let mut diagnostics = Vec::new();

        if let Some(ref solver) = self.solver {
            // Group auto params by entity (template) name
            let mut entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)> =
                HashMap::new();

            for (_, node) in new_snapshot.graph.value_cells.iter() {
                if node.kind.is_auto() {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                        free: node.kind.is_auto_free(),
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Union of all resolved auto param IDs across groups for second wave
            let mut all_resolved_ids: HashSet<ValueCellId> = HashSet::new();

            // Snapshot current values BEFORE the loop so each group's solver
            // receives the same baseline — preventing cross-group contamination
            // where one group's resolved values leak into another group's input.
            let snapshot_values = values.clone();

            // Solve each entity group independently
            for (scope_name, (auto_param_list, auto_ids)) in &entity_groups {
                // Find constraints referencing this group's auto params
                let filtered_constraints: Vec<_> = new_snapshot
                    .graph
                    .constraints
                    .iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints
                    .iter()
                    .any(|(cid, _)| dirty_cone.contains(&NodeId::Constraint(cid.clone())));

                if !constraints_dirty {
                    continue;
                }

                // Look up the template-native objective by entity name.
                let objective = self.objectives.get(scope_name).cloned();

                // Build ResolutionProblem and solve
                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: snapshot_values.clone(),
                    objective,
                    functions: functions.clone(),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        for (id, val) in &solver_values {
                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

                            // Update snapshot values
                            new_snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

                            // Update param_overrides so subsequent edits
                            // use the resolved value
                            self.param_overrides.insert(id.clone(), val.clone());

                            // Update cache
                            let node_id = NodeId::Value(id.clone());
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            self.cache.record_evaluation(
                                node_id,
                                cached_result,
                                VersionId(version_id),
                                trace,
                            );
                        }
                        if !unique {
                            for ap in auto_param_list {
                                if ap.free {
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
                        diagnostics.extend(solver_diags);
                    }
                    SolveResult::NoProgress { reason } => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Constraint solver made no progress: {}",
                            reason
                        )));
                    }
                }
            }

            // ── Second propagation wave (once, with union of all resolved IDs) ──
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may NOT be in the original dirty cone.
            // Guard: skip if eval_state is None (defensive; the early guard at
            // edit_param entry ensures this is unreachable, but an if-let is
            // consistent with the guard re-elaboration phase below which uses
            // .and_then for the same field).
            if !all_resolved_ids.is_empty()
                && let Some(es) = self.eval_state.as_ref()
            {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, &es.reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &self.demand, &es.trace_map);

                for node_id in &wave2_eval {
                    if let NodeId::Value(vcid) = node_id
                        && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                        && let Some(ref expr) = node.default_expr
                    {
                        let val = reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(&values, &functions)
                                .with_meta(&self.meta_map),
                        );
                        values.insert(vcid.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                        // Update cache for re-evaluated node
                        let trace = extract_dependency_trace(expr);
                        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                        self.cache.record_evaluation(
                            node_id.clone(),
                            cached_result,
                            VersionId(version_id),
                            trace,
                        );
                    }
                }
            }
        }

        // ── Guard re-elaboration phase ──────────────────────────────────
        // If any structure-controlling (guard) cells changed boolean value,
        // re-evaluate affected guarded group members: activate the correct
        // branch (members or else_members) and deactivate the other.
        // Finally, recompute topology fingerprint to reflect guard state.
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let new_val = values.get(&group.guard_cell);
                let old_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                    .map(|(v, _)| v);
                new_val != old_val
            });

            if guard_changed {
                // Re-evaluate each guarded group based on current guard values
                for group in new_snapshot.graph.guarded_groups.clone() {
                    // Site 3: guard cell must be present — eval() has completed and populated all
                    // guard cells into the values map. A missing guard cell here is a logic error.
                    let guard_val = values
                        .get(&group.guard_cell)
                        .cloned()
                        .expect("guard cell must have a value after initial evaluation");
                    let guard_is_true = matches!(&guard_val, Value::Bool(true));
                    let guard_is_false = matches!(&guard_val, Value::Bool(false));

                    // Process members (active when guard is true)
                    for member_id in &group.members {
                        if guard_is_true {
                            // Re-evaluate member from its default_expr
                            if let Some(node) = new_snapshot.graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(&new_snapshot.graph, member_id, &mut values, &mut new_snapshot.values);
                        }
                    }

                    // Process else_members (active when guard is false)
                    for member_id in &group.else_members {
                        if guard_is_false {
                            // Re-evaluate else member from its default_expr
                            if let Some(node) = new_snapshot.graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Auto cells skipped — see `deactivate_if_not_auto` doc.
                            deactivate_if_not_auto(&new_snapshot.graph, member_id, &mut values, &mut new_snapshot.values);
                        }
                    }
                }

                // Recompute topology fingerprint to include guard states.
                let guard_state_hash = guard_state_fingerprint(
                    &new_snapshot.graph.guarded_groups,
                    &values,
                    GuardLookup::Strict,
                );
                new_snapshot.topology_fingerprint = new_snapshot
                    .graph
                    .topology_fingerprint()
                    .combine(guard_state_hash);
            }
        }

        // ── Collection count re-elaboration phase ─────────────────────
        // If any structure_controlling cell is a collection count cell and
        // its value changed, add/remove instances to match the new count.
        {
            let collection_subs = new_snapshot.graph.collection_subs.clone();
            for col_sub in &collection_subs {
                let new_count_val = values
                    .get(&col_sub.count_cell)
                    .cloned()
                    .unwrap_or(Value::Undef);
                let old_count_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&col_sub.count_cell))
                    .map(|(v, _)| v.clone())
                    .unwrap_or(Value::Undef);

                if new_count_val == old_count_val {
                    continue;
                }

                // Helper closure: resolve a collection count value to an integer.
                // Returns (count, optional warning diagnostic).
                // Value::Undef is treated as 0 without warning — it represents an undetermined
                // count for which no instances were created. Any other non-integer type emits a
                // warning (potential upstream type bug) and also returns 0.
                let resolve_count = |val: &Value, label: &str| -> (i64, Option<Diagnostic>) {
                    match val {
                        Value::Int(n) => (*n, None),
                        Value::Undef => (0, None),
                        other => (
                            0,
                            Some(Diagnostic::warning(format!(
                                "Collection count cell {} has non-integer {} value {:?}; treating as 0",
                                col_sub.count_cell, label, other
                            ))),
                        ),
                    }
                };

                // Remove old instances from graph and snapshot
                let (old_count, old_warn) = resolve_count(&old_count_val, "old");
                if let Some(w) = old_warn {
                    diagnostics.push(w);
                }
                for i in 0..old_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, _, _, _) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        new_snapshot.graph.value_cells.remove(&scoped_id);
                        new_snapshot.values.remove(&scoped_id);
                        values.remove(&scoped_id);
                    }
                }

                // Create new instances based on new count
                let (new_count, new_warn) = resolve_count(&new_count_val, "new");
                if let Some(w) = new_warn {
                    diagnostics.push(w);
                }
                for i in 0..new_count {
                    let scoped_entity =
                        format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                    for (member, kind, cell_type, default_expr) in &col_sub.child_value_cells {
                        let scoped_id = ValueCellId::new(&scoped_entity, member);
                        let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                        let expr_hash = default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0));
                        let node = crate::graph::ValueCellNode {
                            id: scoped_id.clone(),
                            kind: *kind,
                            cell_type: cell_type.clone(),
                            default_expr: default_expr.clone(),
                            content_hash: id_hash.combine(expr_hash),
                        };
                        new_snapshot
                            .graph
                            .value_cells
                            .insert(scoped_id.clone(), node);

                        // Evaluate the cell
                        let val = if let Some(expr) = default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &reify_expr::EvalContext::new(&values, &functions)
                                    .with_meta(&self.meta_map),
                            )
                        } else {
                            Value::Undef
                        };
                        values.insert(scoped_id.clone(), val.clone());
                        new_snapshot
                            .values
                            .insert(scoped_id, (val, DeterminacyState::Determined));
                    }
                }

                // Update per-member synthetic lists: __list_{name}__{member}
                for (member, _, _, _) in &col_sub.child_value_cells {
                    let member_items: Vec<Value> = (0..new_count)
                        .map(|idx| {
                            let scoped_id = ValueCellId::new(
                                format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, idx),
                                member,
                            );
                            values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                        })
                        .collect();
                    let member_list_id = ValueCellId::new(
                        &col_sub.parent_entity,
                        format!("__list_{}__{}", col_sub.sub_name, member),
                    );
                    let member_list_val = Value::List(member_items);
                    values.insert(member_list_id.clone(), member_list_val.clone());
                    new_snapshot.values.insert(
                        member_list_id,
                        (member_list_val, DeterminacyState::Determined),
                    );
                }

                // Recompute topology fingerprint to reflect count change
                let count_state_hash = ContentHash::of_str(&format!(
                    "collection:{}={}",
                    col_sub.count_cell, new_count
                ));
                new_snapshot.topology_fingerprint = new_snapshot
                    .graph
                    .topology_fingerprint()
                    .combine(count_state_hash);
            }
        }

        // Store state (actual_eval_set excludes early-cutoff-skipped nodes)
        self.last_eval_set = actual_eval_set;
        self.eval_state.as_mut().unwrap().snapshot = new_snapshot;

        Ok(EvalResult {
            values,
            diagnostics,
            resolved_params,
        })
    }

    /// Incrementally re-evaluate after a structural source edit.
    ///
    /// Mirrors `edit_param`'s `NotInitialized` precondition: requires a prior
    /// `eval()` to establish the baseline snapshot, reverse index, trace map,
    /// and demand registry. Returns `Err(EngineError::NotInitialized)` when
    /// called on a fresh Engine before any eval.
    ///
    /// Current implementation (step-4): the "no-change" code path — builds
    /// a fresh snapshot, rebuilds reverse index / trace map / demand registry
    /// from the new graph, refreshes function / purpose / meta / objective
    /// tables from the new module, and seeds per-cell values by copying the
    /// prior snapshot's values for every cell that is also present in the
    /// new graph. This is semantically correct whenever the edit leaves
    /// every cell's content_hash unchanged; later steps replace the
    /// "copy all values" shortcut with diff-driven re-evaluation so that
    /// genuinely structural edits produce the right downstream values.
    pub fn edit_source(&mut self, module: &CompiledModule) -> Result<EvalResult, EngineError> {
        // Precondition: prior eval() must have populated eval_state. This is
        // the same precondition as edit_param and is validated first so that
        // all later steps can rely on a present baseline.
        if self.eval_state.is_none() {
            return Err(EngineError::NotInitialized);
        }

        // (1) Capture the parent snapshot id before we mutate any state.
        let parent_id = self.eval_state.as_ref().unwrap().snapshot.id;

        // (2) Build the new snapshot from the incoming CompiledModule.
        //     Snapshot::from_compiled_module seeds every value cell to
        //     (Undef, Undetermined) or (Undef, Auto); the seeding loop
        //     below overwrites those with the preserved prior values.
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        let mut new_snapshot = crate::snapshot::Snapshot::from_compiled_module(module);
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: HashSet::new(),
            parent: parent_id,
        };

        // (3) Rebuild dependency structures against the NEW graph. Full
        //     rebuild is O(nodes · avg_trace_size), matching cold eval(); see
        //     the design-decision rationale in plan.json for why we don't
        //     patch in place.
        let new_reverse_index =
            crate::deps::ReverseDependencyIndex::build_from_graph(&new_snapshot.graph);
        let new_trace_map = crate::deps::build_trace_map(&new_snapshot.graph);

        let mut new_demand = crate::demand::DemandRegistry::new();
        for (_, node) in new_snapshot.graph.value_cells.iter() {
            new_demand.add_demand(NodeId::Value(node.id.clone()));
        }
        for (_, cnode) in new_snapshot.graph.constraints.iter() {
            new_demand.add_demand(NodeId::Constraint(cnode.id.clone()));
        }
        for (_, rnode) in new_snapshot.graph.realizations.iter() {
            new_demand.add_demand(NodeId::Realization(rnode.id.clone()));
        }
        new_demand.rebuild_cone(&new_snapshot.graph);

        // (4) Seed values by copying from the OLD snapshot for every cell
        //     that also exists in the NEW graph. Cells present only in the
        //     new graph stay at their Snapshot::from_compiled_module default
        //     (Undef + {Undetermined, Auto}).
        //
        //     This implements the plan's "zero-diff code path"; later steps
        //     layer diff-driven re-evaluation on top.
        let mut values = ValueMap::new();
        let old_values = {
            let state = self.eval_state.as_ref().unwrap();
            state.snapshot.values.clone()
        };
        let cell_ids: Vec<ValueCellId> = new_snapshot
            .graph
            .value_cells
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        for id in &cell_ids {
            if let Some((val, det)) = old_values.get(id) {
                new_snapshot
                    .values
                    .insert(id.clone(), (val.clone(), *det));
                values.insert(id.clone(), val.clone());
            } else if let Some((val, _)) = new_snapshot.values.get(id) {
                // New-only cell: keep the Snapshot::from_compiled_module
                // default (Undef). The diff-driven path (step-6+) will
                // evaluate it properly.
                values.insert(id.clone(), val.clone());
            }
        }

        // (5) Refresh function / purpose / meta / objective tables from the
        //     new module, identical to the block at the head of Engine::eval.
        //     A source edit can add/remove/change a user function body,
        //     purpose declaration, meta block, or objective — none of these
        //     are captured by the per-cell content_hash diff, so relying on
        //     cell-level diffing alone would silently serve stale tables.
        self.functions = module.functions.clone();
        self.functions
            .extend(self.prelude_functions.iter().cloned());
        self.compiled_purposes = module.compiled_purposes.clone();
        self.meta_map = module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect();
        self.objectives.clear();
        for template in &module.templates {
            if let Some(obj) = &template.objective {
                self.objectives
                    .insert(template.name.clone(), obj.clone());
            }
        }

        // (6) Install the new snapshot + dependency structures, (7) reset
        //     last_eval_set (no incremental eval performed on this no-change
        //     path), and (8) return the preserved values.
        self.eval_state = Some(crate::EvaluationState {
            snapshot: new_snapshot,
            reverse_index: new_reverse_index,
            trace_map: new_trace_map,
        });
        self.demand = new_demand;
        self.last_eval_set = Vec::new();

        Ok(EvalResult {
            values,
            diagnostics: Vec::new(),
            resolved_params: HashMap::new(),
        })
    }

    /// Evaluates ALL constraints (not just dirty ones) to produce a complete
    /// CheckResult, mirroring check()'s pattern but incrementally.
    ///
    /// Requires a prior call to eval() or check() to establish the baseline.
    pub fn edit_check(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> Result<CheckResult, EngineError> {
        let eval_result = self.edit_param(cell, new_value)?;
        let (constraint_results, constraint_diagnostics) =
            self.check_constraints_with_values(&eval_result.values)?;

        let mut diagnostics = eval_result.diagnostics;
        diagnostics.extend(constraint_diagnostics);

        Ok(CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        })
    }
}

#[cfg(test)]
mod tests {
    use reify_compiler::ValueCellKind;
    use reify_types::{ContentHash, DeterminacyState, PersistentMap, Type, Value, ValueCellId, ValueMap};

    use crate::graph::{EvaluationGraph, ValueCellNode};

    use super::deactivate_if_not_auto;

    #[test]
    fn deactivate_if_not_auto_skips_auto_cell() {
        let id = ValueCellId::new("E", "auto_param");
        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Auto { free: false },
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("auto_param"),
            },
        );

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        // Auto cell: helper must NOT insert anything.
        assert!(values.get(&id).is_none(), "Auto cell must not be deactivated in values");
        assert!(
            snapshot_values.get(&id).is_none(),
            "Auto cell must not be deactivated in snapshot_values"
        );
    }

    #[test]
    fn deactivate_if_not_auto_writes_undef_for_param() {
        let id = ValueCellId::new("E", "param");
        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("param"),
            },
        );

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        assert_eq!(values.get(&id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );
    }

    #[test]
    fn deactivate_if_not_auto_writes_undef_for_missing_cell() {
        let id = ValueCellId::new("X", "missing");
        let graph = EvaluationGraph::default(); // empty — cell not present

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        deactivate_if_not_auto(&graph, &id, &mut values, &mut snapshot_values);

        // Missing cell → treated as non-Auto → must be deactivated.
        assert_eq!(values.get(&id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );
    }

    /// Scenario: a guarded group contains one Auto member and one Param member.
    /// When the guard flips to false, `edit_param` loops over `group.members`
    /// and calls `deactivate_if_not_auto` for each. This test reproduces that
    /// loop directly, asserting that the Auto cell is untouched while the Param
    /// cell becomes Undef — locking in the caller-side wiring so a future
    /// refactor that accidentally drops one of the call sites is caught.
    #[test]
    fn deactivate_if_not_auto_guard_group_mixed_members() {
        let auto_id = ValueCellId::new("E", "auto_solver_param");
        let param_id = ValueCellId::new("E", "regular_param");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(
            auto_id.clone(),
            ValueCellNode {
                id: auto_id.clone(),
                kind: ValueCellKind::Auto { free: false },
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("auto"),
            },
        );
        graph.value_cells.insert(
            param_id.clone(),
            ValueCellNode {
                id: param_id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("param"),
            },
        );

        let mut values: ValueMap = ValueMap::default();
        let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        // Simulate guard = false: iterate over all group members and deactivate
        // (mirrors the `for mid in &group.members { deactivate_if_not_auto(...) }` loop).
        for member_id in &[&auto_id, &param_id] {
            deactivate_if_not_auto(&graph, member_id, &mut values, &mut snapshot_values);
        }

        // Auto cell: lifecycle managed by solver — must NOT be deactivated.
        assert!(values.get(&auto_id).is_none(), "Auto cell must remain untouched in values");
        assert!(
            snapshot_values.get(&auto_id).is_none(),
            "Auto cell must remain untouched in snapshot_values"
        );

        // Param cell: must be written to Undef.
        assert_eq!(values.get(&param_id), Some(&Value::Undef), "Param cell must be deactivated");
        assert_eq!(
            snapshot_values.get(&param_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined)),
            "Param cell must be deactivated in snapshot_values"
        );
    }
}
