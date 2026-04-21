// Split from lib.rs (task 2032) — edit methods.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use reify_compiler::CompiledModule;
use reify_types::{
    AutoParam, ConstraintNodeId, ContentHash, DeterminacyState, Diagnostic, PersistentMap,
    RealizationNodeId, ResolutionProblem, SnapshotId, SnapshotProvenance, SolveResult, Value,
    ValueCellId, ValueMap, VersionId,
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

/// Constraint-node analogue of [`diff_value_cells`]: classify every
/// `ConstraintNodeId` across a pair of graphs into `(changed, added, removed)`
/// by comparing per-node `ConstraintNodeData::content_hash`.
///
/// `ConstraintNodeId` is positional (`entity, index`) within its template, so a
/// re-ordering of constraint declarations in source surfaces here as a
/// `changed` diff at the shifted indexes — not as add+remove. This matches
/// `EvaluationGraph::from_templates`, which assigns indexes from the
/// constraint's declaration order.
pub(crate) fn diff_constraints(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> (
    HashSet<ConstraintNodeId>,
    HashSet<ConstraintNodeId>,
    HashSet<ConstraintNodeId>,
) {
    let mut changed = HashSet::new();
    let mut added = HashSet::new();
    for (id, new_node) in new_graph.constraints.iter() {
        match old_graph.constraints.get(id) {
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
    for (id, _) in old_graph.constraints.iter() {
        if !new_graph.constraints.contains_key(id) {
            removed.insert(id.clone());
        }
    }
    (changed, added, removed)
}

/// Realization-node analogue of [`diff_value_cells`]: classify every
/// `RealizationNodeId` across a pair of graphs into `(changed, added, removed)`
/// by comparing per-node `RealizationNodeData::content_hash`.
///
/// Uses the same positional-identity convention as constraints.
pub(crate) fn diff_realizations(
    old_graph: &EvaluationGraph,
    new_graph: &EvaluationGraph,
) -> (
    HashSet<RealizationNodeId>,
    HashSet<RealizationNodeId>,
    HashSet<RealizationNodeId>,
) {
    let mut changed = HashSet::new();
    let mut added = HashSet::new();
    for (id, new_node) in new_graph.realizations.iter() {
        match old_graph.realizations.get(id) {
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
    for (id, _) in old_graph.realizations.iter() {
        if !new_graph.realizations.contains_key(id) {
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
    /// Algorithm (step-6 — diff-driven incremental eval):
    /// 1. Build a fresh `Snapshot`, `ReverseDependencyIndex`, trace map, and
    ///    `DemandRegistry` from the new module.
    /// 2. Diff the old and new `EvaluationGraph`s at value-cell granularity
    ///    via `diff_value_cells` → `(changed, added, removed)`.
    /// 3. Compute `dirty_cone` via `compute_dirty_cone` over
    ///    `changed ∪ added`, augment with the changed/added cells themselves
    ///    (so their own `default_expr` re-evaluates) and with dependents of
    ///    removed cells via the OLD reverse_index (defensively, gated on
    ///    presence in the new graph).
    /// 4. `eval_set = compute_eval_set(dirty_cone, new_demand, new_trace_map)`.
    /// 5. Seed the working `values` map and `new_snapshot.values`: for every
    ///    cell present in both graphs with unchanged `content_hash`, copy the
    ///    prior `(Value, DeterminacyState)`; for changed/added cells keep the
    ///    `Snapshot::from_compiled_module` default (Undef) — the eval loop
    ///    below fills these in.
    /// 6. Invalidate cache entries for removed and changed value cells.
    /// 7. Refresh `self.functions` / `self.compiled_purposes` / `self.meta_map`
    ///    / `self.objectives` from the new module (module-level state a pure
    ///    cell diff cannot detect).
    /// 8. Per-cell eval loop (shape mirrors `edit_param`): iterate the
    ///    topologically sorted `eval_set`, evaluate each Value node's
    ///    `default_expr`, record a cache entry, and propagate the
    ///    Changed/Unchanged outcome via `has_changed_parent` / `skipped` so
    ///    unchanged sub-cones short-circuit.
    /// 9. Install the new snapshot (with `Edit { changed, parent }`
    ///    provenance), `reverse_index`, `trace_map`, and `demand` into
    ///    `self`; stash `actual_eval_set` in `self.last_eval_set`.
    ///
    /// Constraint / realization diffing and the solver / guard / collection
    /// re-elaboration phases are deferred to later steps (see `.task/plan.json`
    /// steps 10 and 14).
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
        //     below overwrites those with the preserved prior values for
        //     cells whose content_hash matches the old graph.
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        let mut new_snapshot = crate::snapshot::Snapshot::from_compiled_module(module);
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

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

        // (4) Diff the old and new graphs at value-cell granularity.
        let (changed, added, removed) = diff_value_cells(
            &self.eval_state.as_ref().unwrap().snapshot.graph,
            &new_snapshot.graph,
        );
        let mut changed_set: HashSet<ValueCellId> = HashSet::new();
        for id in &changed {
            changed_set.insert(id.clone());
        }
        for id in &added {
            changed_set.insert(id.clone());
        }

        // (4b) Diff constraints and realizations (step-10). These nodes are
        //      positional (`entity, index`) and have their own content_hash on
        //      `ConstraintNodeData` / `RealizationNodeData`. A re-ordered
        //      declaration surfaces as `changed` at the shifted index, not
        //      add+remove. We don't evaluate constraint/realization expressions
        //      at edit_source time — they are deferred to check() / build() —
        //      but we DO want them to appear in `last_eval_set()` when changed
        //      or added, so callers can observe the diff, and we want their
        //      stale cache entries invalidated when removed.
        let (changed_constraints, added_constraints, removed_constraints) = diff_constraints(
            &self.eval_state.as_ref().unwrap().snapshot.graph,
            &new_snapshot.graph,
        );
        let (changed_realizations, added_realizations, removed_realizations) = diff_realizations(
            &self.eval_state.as_ref().unwrap().snapshot.graph,
            &new_snapshot.graph,
        );

        // (5) Compute the dirty cone over changed ∪ added using the NEW
        //     reverse index (which reflects post-edit dependencies). The
        //     compute_dirty_cone helper excludes the roots themselves, so
        //     we also splice in NodeId::Value for each changed/added cell
        //     — their own default_expr must be re-evaluated.
        let mut dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &new_reverse_index);
        for id in &changed_set {
            dirty_cone.insert(NodeId::Value(id.clone()));
        }

        // (6) Defensively include dependents of REMOVED cells via the OLD
        //     reverse index, gated on presence in the new graph. A removed
        //     cell typically also forces its dependents to be classified as
        //     `changed` (their expressions lost a ValueRef), but the OLD
        //     reverse index is the authoritative source for "what used to
        //     read this cell"; skipping it would miss dependents whose
        //     expressions happen to remain shape-compatible (e.g., a
        //     fallback branch). Resolution nodes are intentionally excluded
        //     here — the resolution-node diff is handled in step-10.
        {
            let old_reverse_index = &self.eval_state.as_ref().unwrap().reverse_index;
            for id in &removed {
                for dep in old_reverse_index.dependents_of(id) {
                    let still_present = match dep {
                        NodeId::Value(vcid) => new_snapshot.graph.value_cells.contains_key(vcid),
                        NodeId::Constraint(cid) => new_snapshot.graph.constraints.contains_key(cid),
                        NodeId::Realization(rid) => {
                            new_snapshot.graph.realizations.contains_key(rid)
                        }
                        NodeId::Resolution(_) => false,
                    };
                    if still_present {
                        dirty_cone.insert(dep.clone());
                    }
                }
            }
        }

        // (6b) Insert Constraint / Realization nodes for changed + added
        //      entries into dirty_cone so they appear in last_eval_set. Every
        //      constraint and realization is demanded by eval() / edit_source()
        //      (see the `new_demand` rebuild above), so any entry we splice in
        //      here survives the demand ∩ dirty intersection in compute_eval_set.
        //
        //      Constraint/realization nodes are tracked but NOT evaluated
        //      eagerly here — the expressions are deferred to check() / build()
        //      via `check_constraints_with_values`, which reads the installed
        //      snapshot and the up-to-date graph. This preserves edit_param's
        //      contract (its eval loop also skips Constraint/Realization nodes).
        for cid in &changed_constraints {
            dirty_cone.insert(NodeId::Constraint(cid.clone()));
        }
        for cid in &added_constraints {
            dirty_cone.insert(NodeId::Constraint(cid.clone()));
        }
        for rid in &changed_realizations {
            dirty_cone.insert(NodeId::Realization(rid.clone()));
        }
        for rid in &added_realizations {
            dirty_cone.insert(NodeId::Realization(rid.clone()));
        }

        // (7) Compute eval_set (topo-sorted) from dirty ∩ demand.
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &new_demand, &new_trace_map);

        // (8) Seed values by preserving unchanged-content_hash entries from
        //     the old snapshot, with `param_overrides` winning for Param cells
        //     (step-12). Changed cells retain their
        //     Snapshot::from_compiled_module default (Undef) so the eval loop
        //     fills them in; added cells are seeded from overrides (if any,
        //     for Param kind) else left Undef for the eval loop. Removed cells
        //     are simply absent from the new graph, and their override entries
        //     are purged from `self.param_overrides` below.
        let mut values = ValueMap::new();
        let old_graph_snapshot_values = self.eval_state.as_ref().unwrap().snapshot.values.clone();
        let old_graph_cells = self
            .eval_state
            .as_ref()
            .unwrap()
            .snapshot
            .graph
            .value_cells
            .clone();
        for (id, new_node) in new_snapshot.graph.value_cells.iter() {
            // `param_overrides` wins for Param cells whose content_hash is
            // unchanged across the edit. This mirrors eval_cached's precedence
            // rule ("override always wins for Param cells") and ensures an
            // override established before a structural edit survives the edit.
            // For Param cells whose content_hash CHANGED (e.g. the source
            // default was edited), we intentionally skip the override — the
            // diff has classified the cell as dirty and the eval loop will
            // re-derive it from the new default_expr. If the user wants the
            // override to persist across a content-hash-shifting edit, they
            // can re-install it via set_param_and_invalidate after edit_source.
            let unchanged_hash = old_graph_cells
                .get(id)
                .map(|old_node| old_node.content_hash == new_node.content_hash)
                .unwrap_or(false);

            if matches!(new_node.kind, reify_compiler::ValueCellKind::Param)
                && unchanged_hash
                && let Some(override_val) = self.param_overrides.get(id)
            {
                new_snapshot.values.insert(
                    id.clone(),
                    (override_val.clone(), DeterminacyState::Determined),
                );
                values.insert(id.clone(), override_val.clone());
                continue;
            }

            if unchanged_hash
                && let Some((val, det)) = old_graph_snapshot_values.get(id)
            {
                new_snapshot
                    .values
                    .insert(id.clone(), (val.clone(), *det));
                values.insert(id.clone(), val.clone());
                continue;
            }
            // Changed/added/no prior entry: read the Undef seed placed by
            // Snapshot::from_compiled_module so the working values map has
            // an entry for every present cell (downstream expressions can
            // fail-stop on missing reads).
            if let Some((val, _)) = new_snapshot.values.get(id) {
                values.insert(id.clone(), val.clone());
            }
        }

        // (8b) Purge param_overrides entries for cells that no longer exist
        //      in the new graph (step-12). A dormant override on a removed
        //      cell has nothing to apply to and, if left in place, would
        //      zombie-resurrect if a future edit re-adds a cell with the same
        //      ValueCellId. We also drop overrides for cells that still exist
        //      but are no longer Param (kind changed from Param to Let or
        //      Auto) — the override is only meaningful for Param cells.
        self.param_overrides.retain(|id, _| {
            new_snapshot
                .graph
                .value_cells
                .get(id)
                .map(|node| matches!(node.kind, reify_compiler::ValueCellKind::Param))
                .unwrap_or(false)
        });

        // (9) Invalidate cache entries for changed and removed cells, plus
        //     changed/removed constraints and realizations (step-10). Added
        //     entries have no prior cache entry, so we skip them — the per-cell
        //     eval loop (for value cells) and the downstream check()/build()
        //     path (for constraints/realizations) will populate fresh entries.
        //     Dependents of value-cell changes are refreshed (or transitioned
        //     through Pending) by the per-cell eval loop below.
        for id in &changed {
            self.cache.invalidate(&NodeId::Value(id.clone()));
        }
        for id in &removed {
            self.cache.invalidate(&NodeId::Value(id.clone()));
        }
        for cid in &changed_constraints {
            self.cache.invalidate(&NodeId::Constraint(cid.clone()));
        }
        for cid in &removed_constraints {
            self.cache.invalidate(&NodeId::Constraint(cid.clone()));
        }
        for rid in &changed_realizations {
            self.cache.invalidate(&NodeId::Realization(rid.clone()));
        }
        for rid in &removed_realizations {
            self.cache.invalidate(&NodeId::Realization(rid.clone()));
        }

        // (10) Attach provenance: Edit with the value-cell-level changed set
        //      (constraints / realizations remain implicit in the new graph;
        //      see plan.json design decision).
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // (11) Refresh function / purpose / meta / objective tables from the
        //      new module. A source edit can add/remove/change any of these;
        //      none are captured by the per-cell content_hash diff, so
        //      relying on cell-level diffing alone would silently serve
        //      stale tables (see eval() for the same refresh rationale).
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

        // Snapshot the merged function table for EvalContext; see the
        // PERFORMANCE NOTE in Engine::eval about Arc<Vec<CompiledFunction>>
        // (task #1997) — same deferral here.
        let functions = self.functions.clone();

        // (12) Per-cell eval loop (shape mirrors edit_param's). Transitions
        //      cache entries in the eval set through Pending, iterates in
        //      topological order, evaluates each Value node's default_expr,
        //      and propagates Changed/Unchanged outcomes via
        //      has_changed_parent / skipped for early cutoff.
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        // Seed has_changed_parent from the dependents of every cell in the
        // changed_set (via the NEW reverse index) — these start the edit in
        // the "must not skip" state even before the root itself is evaluated.
        let mut has_changed_parent: HashSet<NodeId> = HashSet::new();
        for id in &changed_set {
            for dep in new_reverse_index.dependents_of(id) {
                has_changed_parent.insert(dep.clone());
            }
        }

        let mut skipped: HashSet<NodeId> = HashSet::new();
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

                // Early-cutoff propagation — identical policy to edit_param:
                // - Changed: dependents inherit has_changed_parent and are
                //   unmarked from `skipped` (a Mixed-fan-in dependent may
                //   have been optimistically added by an Unchanged sibling).
                // - Unchanged: dependents enter `skipped` only if no Changed
                //   parent has been seen for them yet.
                let dependents = new_reverse_index.dependents_of(vcid);
                if outcome == EvalOutcome::Changed {
                    for dep in dependents {
                        has_changed_parent.insert(dep.clone());
                        skipped.remove(dep);
                    }
                } else {
                    for dep in dependents {
                        if !has_changed_parent.contains(dep) {
                            skipped.insert(dep.clone());
                        }
                    }
                }
            }
            // Constraint / Realization nodes: tracked in eval_set but not
            // evaluated here (deferred to check() / build()), same as in
            // edit_param.
        }

        // (13) Restore Final freshness for nodes the early-cutoff path
        //      skipped (they were pre-marked Pending but never re-evaluated).
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // ── Post-eval phases — parity with edit_param's tail (step-14) ──
        //
        // The following four phases mirror the logic at the tail of
        // `edit_param` (guard re-elaboration, solver resolution + second
        // wave, post-resolution guard re-elaboration, collection-count
        // re-elaboration). Without them, a source edit that touches a
        // guard expression, a constraint governing an auto param, or a
        // collection count cell would leave downstream cells stale or
        // Undef. The cross-check test
        // `edit_source_matches_cold_eval_on_mixed_bracket_edit` and the
        // dedicated `edit_source_guard_expr_change_flips_active_branch`
        // test pin these phases.
        //
        // Differences from edit_param: phase 2's second-wave dirty cone
        // / eval-set use the NEW graph's `new_reverse_index` and
        // `new_trace_map` (rather than `self.eval_state.as_ref()`'s
        // pre-edit structures) because a source edit can change edges,
        // so dependents in the new graph may differ from the old. Phases
        // 3 and 4 still read `self.eval_state.as_ref()` for pre-edit
        // guard/count values; self.eval_state has NOT yet been replaced
        // (that happens in step 15 below).

        // ── Phase 1: Guard re-elaboration (dirty-cone trigger) ───────────
        // If any structure_controlling cell is in the dirty cone or
        // changed_set — e.g., because its expression or an input
        // changed — re-evaluate each guarded group's guard cell and
        // activate/deactivate branch members accordingly. This runs
        // BEFORE the resolution phase so guards gated on auto params
        // have the best-available (possibly Undef) inputs.
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

        // ── Phase 2: Solver resolution + second-wave propagation ─────────
        // Reuses the same structure as edit_param's resolution phase, but
        // with two key substitutions: (a) the second-wave dirty cone and
        // eval set use `new_reverse_index`, `new_trace_map`, and
        // `new_demand` (rather than the pre-edit `self.eval_state` /
        // `self.demand`) because edit_source can reshape dependency edges;
        // (b) we draw `scope_name` from `self.objectives` just as before.
        let mut resolved_params: HashMap<ValueCellId, Value> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

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

            // ── Second propagation wave ─────────────────────────────────
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may not be in the original dirty cone.
            // For edit_source we MUST use the NEW reverse_index / trace_map
            // / demand (rather than self.eval_state's stale pre-edit
            // structures) because dependency edges may have shifted.
            if !all_resolved_ids.is_empty() {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, &new_reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &new_demand, &new_trace_map);

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

        // ── Phase 3: Guard re-elaboration (value-changed trigger) ────────
        // Catches guards whose computed boolean value differs from the
        // pre-edit snapshot — e.g., resolver resolved an auto param that
        // feeds the guard, or the dirty-cone path missed an edge (defensive).
        // Uses GuardLookup::Strict because eval() has populated every guard
        // cell by this point; a missing cell would be a logic error.
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
                for group in new_snapshot.graph.guarded_groups.clone() {
                    let guard_val = values
                        .get(&group.guard_cell)
                        .cloned()
                        .expect("guard cell must have a value after initial evaluation");
                    let guard_is_true = matches!(&guard_val, Value::Bool(true));
                    let guard_is_false = matches!(&guard_val, Value::Bool(false));

                    for member_id in &group.members {
                        if guard_is_true {
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
                            deactivate_if_not_auto(
                                &new_snapshot.graph,
                                member_id,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }

                    for member_id in &group.else_members {
                        if guard_is_false {
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
                            deactivate_if_not_auto(
                                &new_snapshot.graph,
                                member_id,
                                &mut values,
                                &mut new_snapshot.values,
                            );
                        }
                    }
                }

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

        // ── Phase 4: Collection count re-elaboration ─────────────────────
        // If any structure_controlling count cell's value changed vs. the
        // pre-edit snapshot, add/remove instances to match the new count.
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

        // (15) Install the new snapshot, dep structures, and demand; record
        //      actual_eval_set (excludes early-cutoff-skipped nodes).
        self.eval_state = Some(crate::EvaluationState {
            snapshot: new_snapshot,
            reverse_index: new_reverse_index,
            trace_map: new_trace_map,
        });
        self.demand = new_demand;
        self.last_eval_set = actual_eval_set;

        Ok(EvalResult {
            values,
            diagnostics,
            resolved_params,
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
