// Split from lib.rs (task 2032) — edit methods.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::Instant;

use reify_compiler::{CompiledFunction, CompiledModule};
use reify_types::{
    AutoParam, ConstraintNodeId, ContentHash, DeterminacyState, Diagnostic, PersistentMap,
    RealizationNodeId, ResolutionProblem, SnapshotId, SnapshotProvenance, SolveResult, Value,
    ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CachedResult, EvalOutcome, NodeId};
use crate::deps::{DependencyTrace, extract_dependency_trace};
use crate::graph::{EvaluationGraph, GuardedGroupInfo};
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

/// Re-elaborate the active and inactive branches of a single guarded group
/// given the already-computed guard value.
///
/// - **Active branch** (`is_true` for `members`, `is_false` for `else_members`):
///   each cell's `default_expr` is evaluated with
///   `EvalContext::new(values, functions).with_meta(meta_map)` and written into
///   both `values` and `snapshot_values` with `DeterminacyState::Determined`.
///   Cells without a `default_expr` (or absent from the graph) are left
///   unchanged.
/// - **Inactive branch**: each cell is passed to `deactivate_if_not_auto`,
///   which writes `Undef / Undetermined` for non-Auto cells and skips Auto
///   cells (whose lifecycle is owned by the constraint solver).
///
/// The caller is responsible for computing and inserting the guard cell value
/// itself — this helper takes `guard_val` as input and handles only the member
/// propagation step.
fn reelaborate_guarded_group(
    graph: &EvaluationGraph,
    group: &GuardedGroupInfo,
    guard_val: &Value,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
) {
    let is_true = matches!(guard_val, Value::Bool(true));
    let is_false = matches!(guard_val, Value::Bool(false));

    for (cells, is_active) in [(&group.members, is_true), (&group.else_members, is_false)] {
        for mid in cells {
            if is_active {
                if let Some(node) = graph.value_cells.get(mid)
                    && let Some(ref expr) = node.default_expr
                {
                    let val = reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                    );
                    values.insert(mid.clone(), val.clone());
                    snapshot_values.insert(mid.clone(), (val, DeterminacyState::Determined));
                }
            } else {
                deactivate_if_not_auto(graph, mid, values, snapshot_values);
            }
        }
    }
}

/// Re-deactivate inactive-branch members for every guarded group that Phase 1
/// re-elaborated during the current edit call.
///
/// After wave2, the constraint solver may have re-evaluated inactive-branch
/// members (whose `default_expr` reads a resolved auto param) that Phase 1
/// previously deactivated (`Undef`). This cleanup restores Phase 1's
/// deactivation state so that Phase 3's `phase1_reelaborated` skip (when the
/// guard value is unchanged since Phase 1) yields a correct final result.
///
/// `phase1_reelaborated` maps each re-elaborated guard_cell to the guard value
/// Phase 1 recorded (task 2146). The value is NOT used here — this helper only
/// re-deactivates the current-inactive branch using the *current* guard value,
/// which may already differ from the Phase-1 recorded value if wave2 has
/// flipped the guard. Phase 3's flip-detection (comparing the recorded value
/// against the current guard value) handles that case separately.
///
/// Called from both `edit_param` post-wave2 (task 2140) and `edit_source`
/// post-wave2 (task 2142). Does nothing when `phase1_reelaborated` is empty.
/// By taking `graph` and `snapshot_values` as separate parameters the caller
/// can use field-level borrow splitting — no `.clone()` of `guarded_groups`
/// is required at either call site.
fn reapply_phase1_deactivations(
    graph: &EvaluationGraph,
    phase1_reelaborated: &HashMap<ValueCellId, Value>,
    values: &mut ValueMap,
    snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
) {
    if phase1_reelaborated.is_empty() {
        return;
    }
    for group in &graph.guarded_groups {
        if !phase1_reelaborated.contains_key(&group.guard_cell) {
            continue;
        }
        let guard_val = values
            .get(&group.guard_cell)
            .cloned()
            .expect("guard cell must have a value after Phase 1");
        let is_true = matches!(&guard_val, Value::Bool(true));
        let is_false = matches!(&guard_val, Value::Bool(false));
        for (cells, is_active) in [(&group.members, is_true), (&group.else_members, is_false)] {
            if !is_active {
                for mid in cells {
                    deactivate_if_not_auto(graph, mid, values, snapshot_values);
                }
            }
        }
    }
}

/// Build a role map from a slice of `GuardedGroupInfo` for the role-flip
/// probe in `Engine::edit_source`.
///
/// The returned map is keyed by `ValueCellId` and maps to
/// `(guard_cell, branch_tag)` where `branch_tag` is `0u8` for `members`
/// (guard = true) and `1u8` for `else_members` (guard = false).
///
/// When a `ValueCellId` appears in both `members` and `else_members` of the
/// **same** group, the `else_members` entry wins (last-write semantics); this
/// is an observable pattern in valid compiled modules (e.g. a cell that is the
/// "effective" output regardless of which branch is active).
///
/// # Panics (debug builds only)
///
/// In debug builds the function panics if any `ValueCellId` appears in two
/// groups that have **different** `guard_cell`s, i.e. the cell is claimed by
/// two distinct guards.  Intra-group duplicates (same `guard_cell`) are
/// permitted and resolved by last-write-wins.
fn build_old_role_map(groups: &[GuardedGroupInfo]) -> HashMap<ValueCellId, (ValueCellId, u8)> {
    let capacity: usize = groups.iter().map(|g| g.members.len() + g.else_members.len()).sum();
    let mut old_roles: HashMap<ValueCellId, (ValueCellId, u8)> = HashMap::with_capacity(capacity);
    for group in groups.iter() {
        for mid in &group.members {
            let prev = old_roles.insert(mid.clone(), (group.guard_cell.clone(), 0u8));
            debug_assert!(
                prev.is_none_or(|(prev_guard, _)| prev_guard == group.guard_cell),
                "ValueCellId {:?} appeared in multiple guarded-group roles",
                mid
            );
        }
        for mid in &group.else_members {
            let prev = old_roles.insert(mid.clone(), (group.guard_cell.clone(), 1u8));
            debug_assert!(
                prev.is_none_or(|(prev_guard, _)| prev_guard == group.guard_cell),
                "ValueCellId {:?} appeared in multiple guarded-group roles",
                mid
            );
        }
    }
    old_roles
}

/// Generic identity/equivalence diff between two `PersistentMap<Id, Node>`
/// collections.
///
/// Classifies every `Id` across the two maps into three disjoint sets by
/// comparing per-node content hashes (extracted via `content_hash_fn`):
///
/// - `changed`: present in both maps, content hash differs.
/// - `added`: present only in the new map.
/// - `removed`: present only in the old map.
///
/// A match signals "equivalent node; cached value is still valid"; a
/// mismatch signals "re-evaluate". This is the shared kernel of the three
/// graph-level diffs (`diff_value_cells`, `diff_constraints`,
/// `diff_realizations`) — every one of them wants the same three-set
/// classification, so any future tweak (e.g. returning counts, emitting a
/// Modified variant, handling content_hash collisions) lives in one place.
fn diff_nodes<Id, Node, F>(
    old_map: &PersistentMap<Id, Node>,
    new_map: &PersistentMap<Id, Node>,
    content_hash_fn: F,
) -> (HashSet<Id>, HashSet<Id>, HashSet<Id>)
where
    Id: Clone + Eq + Hash,
    Node: Clone,
    F: Fn(&Node) -> ContentHash,
{
    let mut changed = HashSet::new();
    let mut added = HashSet::new();
    for (id, new_node) in new_map.iter() {
        match old_map.get(id) {
            Some(old_node) => {
                if content_hash_fn(old_node) != content_hash_fn(new_node) {
                    changed.insert(id.clone());
                }
            }
            None => {
                added.insert(id.clone());
            }
        }
    }
    let mut removed = HashSet::new();
    for (id, _) in old_map.iter() {
        if !new_map.contains_key(id) {
            removed.insert(id.clone());
        }
    }
    (changed, added, removed)
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
    diff_nodes(&old_graph.value_cells, &new_graph.value_cells, |n| {
        n.content_hash
    })
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
    diff_nodes(&old_graph.constraints, &new_graph.constraints, |n| {
        n.content_hash
    })
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
    diff_nodes(&old_graph.realizations, &new_graph.realizations, |n| {
        n.content_hash
    })
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
        // Reset the per-edit guard-phase group evaluation counter before Phase 1.
        self.last_guard_phase_group_evals = 0;
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
        //
        // Cross-phase dedup (task 2140, 2146): the map is non-empty only when
        // Phase 1 fires; the else arm returns an empty HashMap so no allocation
        // is wasted when no guards are dirty. Phase 3 consults the map to skip
        // groups already re-elaborated here when the guard value is unchanged
        // since Phase 1 — but falls through to full re-elaboration if wave2 has
        // flipped the guard value after Phase 1 recorded it (task 2146 fix).
        let phase1_reelaborated: HashMap<ValueCellId, Value> = {
            let graph = &new_snapshot.graph;
            let has_dirty_guards = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            });

            if has_dirty_guards {
                let mut set = HashMap::new();
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
                    // Per-group skip: if this group's guard value is unchanged vs.
                    // the pre-edit snapshot, its activation state has not flipped
                    // and its members don't need re-elaboration. edit_param has no
                    // structural-add or role-flip trigger, so the skip condition is
                    // purely "guard value unchanged".
                    let old_guard_val = self
                        .eval_state
                        .as_ref()
                        .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                        .map(|(v, _)| v);
                    // Always write the guard cell value before the skip check.
                    // Phase 1 re-evaluates guards with a determinacy context that
                    // the main eval loop lacks; DeterminacyPredicate guards (e.g.
                    // `determined(x)`) evaluate to Undef in the main loop and must
                    // be corrected here — even when we skip member re-elaboration.
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));
                    if old_guard_val == Some(&guard_val) {
                        continue;
                    }
                    self.last_guard_phase_group_evals += 1;
                    // Record guard_cell → guard_val so Phase 3 can detect a
                    // wave2 flip: if the current guard value differs from the
                    // recorded value, Phase 3 falls through to full re-elaboration
                    // (task 2146 fix). The `.insert` sits after the skip-continue
                    // so only actually-processed groups land in the map.
                    set.insert(group.guard_cell.clone(), guard_val.clone());

                    reelaborate_guarded_group(
                        graph,
                        group,
                        &guard_val,
                        &mut values,
                        &mut new_snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
                }

                // Recompute topology fingerprint including guard states.
                let guard_state_hash =
                    guard_state_fingerprint(&graph.guarded_groups, &values, GuardLookup::Lenient);
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
                set
            } else {
                HashMap::new()
            }
        };

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

                // Post-wave2 cleanup (task 2140): wave2 can re-evaluate
                // inactive-branch members Phase 1 deactivated; restore
                // those deactivations so Phase 3's phase1_reelaborated
                // skip stays correct.  See `reapply_phase1_deactivations`.
                reapply_phase1_deactivations(
                    &new_snapshot.graph,
                    &phase1_reelaborated,
                    &mut values,
                    &mut new_snapshot.values,
                );
            }
        }

        // ── Guard re-elaboration phase ──────────────────────────────────
        // If any structure-controlling (guard) cells changed boolean value,
        // re-evaluate affected guarded group members: activate the correct
        // branch (members or else_members) and deactivate the other.
        // Finally, recompute topology fingerprint to reflect guard state.
        //
        // `guard_changed` is also true when Phase 1 processed a group with a
        // guard value that wave2 subsequently changed (flip-then-revert). In
        // that case the final guard value may match the pre-edit snapshot, but
        // Phase 1 left the group's member state inconsistent; Phase 3 must
        // re-elaborate to fix it (task 2146).
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let new_val = values.get(&group.guard_cell);
                let old_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                    .map(|(v, _)| v);
                if new_val != old_val {
                    return true;
                }
                // Phase-1 flip-then-revert: Phase 1 recorded a different guard
                // value than current → group state is inconsistent (task 2146).
                phase1_reelaborated
                    .get(&group.guard_cell)
                    .is_some_and(|p1| Some(p1) != new_val)
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
                    // Cross-phase dedup (task 2140, 2146): two cases:
                    //
                    // (a) Phase 1 processed this group with the SAME guard value as
                    //     current → Phase 1's work is still valid → skip.
                    //
                    // (b) Phase 1 processed this group with a DIFFERENT guard value
                    //     (wave2 flipped the guard after Phase 1 ran) → Phase 1 left
                    //     the group in an intermediate state; we MUST re-elaborate
                    //     regardless of the old-vs-new comparison, because the old and
                    //     current guard values may coincidentally match even though the
                    //     member state is wrong (e.g. old=false, Phase-1=true,
                    //     current=false — guard "unchanged" but members corrupted).
                    //
                    // (c) Phase 1 did NOT process this group → fall through to the
                    //     standard old-vs-new skip.
                    match phase1_reelaborated.get(&group.guard_cell) {
                        Some(p1_val) if p1_val == &guard_val => continue, // case (a)
                        Some(_) => {} // case (b): fall through unconditionally
                        None => {
                            // case (c): standard old-vs-new skip
                            let old_guard_val = self
                                .eval_state
                                .as_ref()
                                .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                                .map(|(v, _)| v);
                            if old_guard_val == Some(&guard_val) {
                                continue;
                            }
                        }
                    }
                    self.last_guard_phase_group_evals += 1;
                    reelaborate_guarded_group(
                        &new_snapshot.graph,
                        &group,
                        &guard_val,
                        &mut values,
                        &mut new_snapshot.values,
                        &functions,
                        &self.meta_map,
                    );
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
        // Disjoint-field borrow: Rust's NLL tracks this borrow as touching only
        // the `eval_state` field (not all of `self`), so later mutable borrows
        // of sibling fields — `self.param_overrides.retain(...)` and
        // `self.cache.invalidate(...)` — coexist without a lifetime conflict.
        // `eval_state` is used read-only throughout: parent_id, old graph,
        // reverse_index, and trace_map.
        let eval_state = self.eval_state.as_ref().unwrap();

        // (1) Capture the parent snapshot id before we mutate any state.
        let parent_id = eval_state.snapshot.id;

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

        // Shared demand-seeding helper with Engine::eval — see
        // `build_demand_for_graph` for the per-kind initialization.
        let new_demand = crate::engine_eval::build_demand_for_graph(&new_snapshot.graph);

        // (4) Diff the old and new graphs at value-cell granularity.
        let (changed, added, removed) = diff_value_cells(
            &eval_state.snapshot.graph,
            &new_snapshot.graph,
        );
        let changed_set: HashSet<ValueCellId> =
            changed.iter().chain(added.iter()).cloned().collect();

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
            &eval_state.snapshot.graph,
            &new_snapshot.graph,
        );
        let (changed_realizations, added_realizations, removed_realizations) = diff_realizations(
            &eval_state.snapshot.graph,
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
        //     fallback branch).
        //
        //     Resolution nodes are currently treated as not-still-present:
        //     they are live in the graph (`deps.rs`, `cache.rs`), but the
        //     eval() / edit_source() demand-seeding path does not
        //     `add_demand` them, and edit_source has no `diff_resolutions`
        //     helper yet. The moment Resolution demand is added, this arm
        //     becomes a latent staleness hazard — a Resolution dependent of
        //     a removed cell would silently retain a stale cached value.
        //
        //     TODO(resolution-diff): add a `diff_resolutions` helper and
        //     replace this `false` with a
        //     `new_snapshot.graph.resolutions.contains_key(rid)` presence
        //     check, symmetric with the other arms, once Resolution nodes
        //     participate in the demand set.
        {
            let old_reverse_index = &eval_state.reverse_index;
            for id in &removed {
                for dep in old_reverse_index.dependents_of(id) {
                    let still_present = match dep {
                        NodeId::Value(vcid) => new_snapshot.graph.value_cells.contains_key(vcid),
                        NodeId::Constraint(cid) => new_snapshot.graph.constraints.contains_key(cid),
                        NodeId::Realization(rid) => {
                            new_snapshot.graph.realizations.contains_key(rid)
                        }
                        NodeId::Resolution(_) => false, // TODO(resolution-diff)
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
        // Shortcut references into the prior snapshot for the seeding loop below.
        let old_graph_snapshot_values = &eval_state.snapshot.values;
        let old_graph_cells = &eval_state.snapshot.graph.value_cells;
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

        // Reset the per-edit guard-phase group evaluation counter. This counter
        // is incremented for each group that is NOT skipped in Phase 1 or Phase 3;
        // it is exposed via last_guard_phase_group_evals() for test assertions.
        self.last_guard_phase_group_evals = 0;

        // Cross-phase dedup map (task 2142, 2146): maps guard_cell → guard_val for
        // every group that Phase 1 actually re-elaborated in this edit_source call.
        // Phase 3 consults this map: it skips groups already covered by Phase 1 when
        // the recorded guard value matches the current value, but falls through to
        // full re-elaboration when wave2 has flipped the guard after Phase 1 recorded
        // it (task 2146 fix). Reelaboration is idempotent for a given guard value —
        // provided wave2 has not subsequently overwritten inactive members (the
        // post-wave2 cleanup in the solver block re-deactivates them, mirroring
        // task 2140). Declared at function scope because edit_source's Phase 1 is a
        // large multi-step block (role-flip probe + composite has_dirty_guards);
        // wrapping it as a block-expression would churn more lines than necessary.
        let mut phase1_reelaborated: HashMap<ValueCellId, Value> = HashMap::new();

        // ── Phase 1: Guard re-elaboration (dirty-cone trigger) ───────────
        // If any structure_controlling cell is in the dirty cone or
        // changed_set — e.g., because its expression or an input
        // changed — re-evaluate each guarded group's guard cell and
        // activate/deactivate branch members accordingly. This runs
        // BEFORE the resolution phase so guards gated on auto params
        // have the best-available (possibly Undef) inputs.
        //
        // We ALSO trigger Phase 1 when any `added` value cell intersects a
        // guarded group's members or else_members. This covers reviewer
        // comment #3: when an edit inserts a new `let` into an existing
        // `where … else` group without touching the guard expression or
        // any structure_controlling cell, the Step-12 per-cell eval loop
        // evaluates the new member's default_expr into a Determined value
        // — but if the new member lands on the *inactive* branch, cold eval
        // would deactivate it to Undef via `deactivate_if_not_auto`. Forcing
        // Phase 1 to run re-elaborates every guarded group, which routes
        // the added member through the correct activation path. This also
        // covers symmetric cases (added members on the active branch) —
        // Phase 1 just re-evaluates them, matching cold eval's behavior.
        //
        // We ALSO trigger Phase 1 when an existing cell's *role* within a
        // guarded group changes — i.e. it moves from the `members` branch
        // to the `else_members` branch (or vice versa) while its id and
        // expression text are unchanged. `diff_value_cells` compares per-cell
        // `content_hash` (id_hash.combine(expr_hash)), which has no notion of
        // containing group or branch, so a role-flipped cell is classified
        // neither `changed` nor `added`. Without this trigger, Phase 1 never
        // fires and the old-branch value survives on the wrong branch.
        // We detect this by building a per-cell role map (ValueCellId →
        // (guard_cell_id, branch_tag)) for both the old and new graphs and
        // firing when the maps differ. Phase 1's existing activation/deactivation
        // loop then routes every member through the correct path. Lock:
        // `edit_source_role_flipped_guard_member_matches_cold_eval` (task 2084).
        {
            let graph = &new_snapshot.graph;
            let has_added_guard_member = graph.guarded_groups.iter().any(|group| {
                group.members.iter().any(|m| added.contains(m))
                    || group.else_members.iter().any(|m| added.contains(m))
            });
            // Detect role flips with a short-circuiting probe: build one
            // HashMap for the old graph keyed by ValueCellId → (guard_cell_id,
            // branch_tag), then walk the new groups once, breaking as soon as
            // any mismatch is found. Skip entirely when both sides are empty so
            // the common no-guarded-group case is free. branch_tag 0 = members,
            // 1 = else_members.
            let eval_state = self.eval_state.as_ref().unwrap();
            let old_groups = &eval_state.snapshot.graph.guarded_groups;
            let new_groups = &graph.guarded_groups;
            let has_role_flipped_guard_member = if old_groups.is_empty() && new_groups.is_empty() {
                false
            } else {
                // Build old role map once.
                let old_roles = build_old_role_map(old_groups);
                // Walk new groups, short-circuit on first mismatch.
                let mut new_total = 0usize;
                let mut flipped = false;
                'outer: for group in new_groups.iter() {
                    for (mid, tag) in group
                        .members
                        .iter()
                        .map(|m| (m, 0u8))
                        .chain(group.else_members.iter().map(|m| (m, 1u8)))
                    {
                        new_total += 1;
                        match old_roles.get(mid) {
                            Some((gc, t)) if gc == &group.guard_cell && *t == tag => {}
                            _ => {
                                flipped = true;
                                break 'outer;
                            }
                        }
                    }
                }
                // Also flip if new graph has fewer members than old (a cell left a group).
                flipped || new_total != old_roles.len()
            };
            let has_dirty_guards = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            }) || has_added_guard_member
                || has_role_flipped_guard_member;

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
                    // Per-group skip: if this group's guard value is unchanged vs.
                    // the pre-edit snapshot, AND no members of this group were
                    // added in this edit, AND no role-flip was detected (role-flip
                    // suppresses all per-group skips because we can't identify
                    // which groups were affected without a second full walk), then
                    // skip the member re-elaboration for this group.
                    let old_guard_val = self
                        .eval_state
                        .as_ref()
                        .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                        .map(|(v, _)| v);
                    let has_added_in_group = group.members.iter().any(|m| added.contains(m))
                        || group.else_members.iter().any(|m| added.contains(m));
                    // Always write the guard cell value before the skip check.
                    // Phase 1 re-evaluates guards with a determinacy context that
                    // the main eval loop lacks; DeterminacyPredicate guards (e.g.
                    // `determined(x)`) evaluate to Undef in the main loop and must
                    // be corrected here — even when we skip member re-elaboration.
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));
                    if old_guard_val == Some(&guard_val)
                        && !has_added_in_group
                        && !has_role_flipped_guard_member
                    {
                        continue;
                    }
                    self.last_guard_phase_group_evals += 1;
                    // Record guard_cell → guard_val so Phase 3 can detect a
                    // wave2 flip: if the current guard value differs from the
                    // recorded value, Phase 3 falls through to full re-elaboration
                    // (task 2146 fix). The insert sits after the skip-continue so
                    // only actually-processed groups land in the map — guard-flip,
                    // added-member, and role-flip triggers all satisfy "Phase 1
                    // re-elaborated this group", which is precisely what Phase 3
                    // needs to know.
                    phase1_reelaborated.insert(group.guard_cell.clone(), guard_val.clone());

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

                // Post-wave2 cleanup (task 2142): wave2 can re-evaluate
                // inactive-branch members Phase 1 deactivated; restore
                // those deactivations so Phase 3's phase1_reelaborated
                // skip stays correct.  See `reapply_phase1_deactivations`.
                // edit_source's wave2 uses local new_reverse_index /
                // new_trace_map / new_demand (not self.eval_state), so
                // the call lives directly inside `if !all_resolved_ids…`.
                reapply_phase1_deactivations(
                    &new_snapshot.graph,
                    &phase1_reelaborated,
                    &mut values,
                    &mut new_snapshot.values,
                );
            }
        }

        // ── Phase 3: Guard re-elaboration (value-changed trigger) ────────
        // Catches guards whose computed boolean value differs from the
        // pre-edit snapshot — e.g., resolver resolved an auto param that
        // feeds the guard, or the dirty-cone path missed an edge (defensive).
        // Uses GuardLookup::Strict because eval() has populated every guard
        // cell by this point; a missing cell would be a logic error.
        //
        // `guard_changed` is also true when Phase 1 processed a group with a
        // guard value that wave2 subsequently changed (flip-then-revert). In
        // that case the final guard value may match the pre-edit snapshot, but
        // Phase 1 left the group's member state inconsistent; Phase 3 must
        // re-elaborate to fix it (task 2146).
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let new_val = values.get(&group.guard_cell);
                let old_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                    .map(|(v, _)| v);
                if new_val != old_val {
                    return true;
                }
                // Phase-1 flip-then-revert: Phase 1 recorded a different guard
                // value than current → group state is inconsistent (task 2146).
                phase1_reelaborated
                    .get(&group.guard_cell)
                    .is_some_and(|p1| Some(p1) != new_val)
            });

            if guard_changed {
                for group in new_snapshot.graph.guarded_groups.clone() {
                    // Phase 1 (the dirty-cone-triggered branch above) guarantees
                    // that every guard_cell in structure_controlling has a value
                    // in `values`. But Phase 3 is separately gated on
                    // `guard_changed` (value diff vs. old snapshot) — a future
                    // refactor that narrows structure_controlling could leave a
                    // guard_cell unevaluated here. Skip those defensively rather
                    // than panic; the old snapshot's guard value will be used for
                    // the downstream diff in subsequent edits.
                    let Some(guard_val) = values.get(&group.guard_cell).cloned() else {
                        continue;
                    };
                    // Cross-phase dedup (task 2142, 2146): three cases:
                    //
                    // (a) Phase 1 processed this group with the SAME guard value as
                    //     current → Phase 1's work is still valid → skip.
                    //
                    // (b) Phase 1 processed this group with a DIFFERENT guard value
                    //     (wave2 flipped the guard after Phase 1 ran) → Phase 1 left
                    //     the group in an intermediate state; we MUST re-elaborate
                    //     regardless of the old-vs-new comparison, because the old and
                    //     current guard values may coincidentally match even though the
                    //     member state is wrong (e.g. old=false, Phase-1=true,
                    //     current=false — guard "unchanged" but members corrupted).
                    //
                    // (c) Phase 1 did NOT process this group → fall through to the
                    //     standard old-vs-new skip (resolver-driven guard changes).
                    match phase1_reelaborated.get(&group.guard_cell) {
                        Some(p1_val) if p1_val == &guard_val => continue, // case (a)
                        Some(_) => {} // case (b): fall through unconditionally
                        None => {
                            // case (c): standard old-vs-new skip
                            // Phase 3 has no added-member or role-flip exception
                            // (those are Phase 1 concerns only).
                            let old_guard_val = self
                                .eval_state
                                .as_ref()
                                .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                                .map(|(v, _)| v);
                            if old_guard_val == Some(&guard_val) {
                                continue;
                            }
                        }
                    }
                    self.last_guard_phase_group_evals += 1;
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
    use reify_types::{
        CompiledExpr, ContentHash, DeterminacyState, PersistentMap, Type, Value, ValueCellId,
        ValueMap,
    };

    use std::collections::HashMap;

    use crate::graph::{EvaluationGraph, GuardedGroupInfo, ValueCellNode};

    use super::{deactivate_if_not_auto, reelaborate_guarded_group};

    /// Construct a [`ValueCellNode`] for use in unit tests.
    ///
    /// The `content_hash` is derived deterministically from `id.to_string()`
    /// (`"entity.member"` format), so every unique `ValueCellId` produces a
    /// distinct hash without requiring callers to supply one explicitly.
    fn make_cell(
        id: &ValueCellId,
        kind: ValueCellKind,
        cell_type: Type,
        default_expr: Option<CompiledExpr>,
    ) -> ValueCellNode {
        ValueCellNode {
            id: id.clone(),
            kind,
            cell_type,
            default_expr,
            content_hash: ContentHash::of_str(&id.to_string()),
        }
    }

    /// Run [`reelaborate_guarded_group`] with `guard_val = Bool(guard)` and
    /// empty functions / meta on the supplied graph and group, returning the
    /// resulting `(values, snapshot_values)` maps.
    ///
    /// Collapses the 7-line call-site boilerplate into a single line, leaving
    /// each test as a thin setup + assertion wrapper.
    fn run_with_guard(
        graph: EvaluationGraph,
        group: GuardedGroupInfo,
        guard: bool,
    ) -> (ValueMap, PersistentMap<ValueCellId, (Value, DeterminacyState)>) {
        let mut values = ValueMap::default();
        let mut snapshot_values = PersistentMap::default();
        reelaborate_guarded_group(
            &graph,
            &group,
            &Value::Bool(guard),
            &mut values,
            &mut snapshot_values,
            &[],
            &HashMap::new(),
        );
        (values, snapshot_values)
    }

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

    /// Happy-path characterization: two valid groups with non-overlapping
    /// members produce the expected four-entry role map.
    #[test]
    fn build_old_role_map_returns_expected_map_for_valid_groups() {
        use std::collections::HashMap;

        use crate::graph::GuardedGroupInfo;

        use super::build_old_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let g2 = ValueCellId::new("E2", "guard");
        let a = ValueCellId::new("E1", "a");
        let b = ValueCellId::new("E1", "b");
        let c = ValueCellId::new("E2", "c");
        let d = ValueCellId::new("E2", "d");

        let group1 = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![a.clone()],
            else_members: vec![b.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };
        let group2 = GuardedGroupInfo {
            guard_cell: g2.clone(),
            members: vec![c.clone()],
            else_members: vec![d.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let map: HashMap<ValueCellId, (ValueCellId, u8)> = build_old_role_map(&[group1, group2]);

        assert_eq!(map.len(), 4);
        assert_eq!(map.get(&a), Some(&(g1.clone(), 0u8)));
        assert_eq!(map.get(&b), Some(&(g1.clone(), 1u8)));
        assert_eq!(map.get(&c), Some(&(g2.clone(), 0u8)));
        assert_eq!(map.get(&d), Some(&(g2.clone(), 1u8)));
    }

    /// Duplicate ValueCellId across two groups must panic in debug builds.
    ///
    /// Gated by `#[cfg(debug_assertions)]` because `debug_assert!` is a no-op
    /// in release mode — without the gate `cargo test --release` would run the
    /// body, the silent overwrite would not panic, and `#[should_panic]` would
    /// fail. Pattern mirrors `crates/reify-expr/tests/gradient_tests.rs:4043`.
    #[cfg(debug_assertions)]
    #[should_panic(expected = "appeared in multiple guarded-group roles")]
    #[test]
    fn build_old_role_map_panics_on_duplicate_member() {
        use crate::graph::GuardedGroupInfo;

        use super::build_old_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let g2 = ValueCellId::new("E2", "guard");
        let shared = ValueCellId::new("E1", "shared");

        let group1 = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![shared.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };
        let group2 = GuardedGroupInfo {
            guard_cell: g2.clone(),
            members: vec![shared.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        // Must panic: `shared` appears in two groups.
        build_old_role_map(&[group1, group2]);
    }

    /// A ValueCellId in both `members` and `else_members` of the *same* group
    /// must NOT panic: intra-group duplicates are permitted and resolved by
    /// last-write semantics (else_members entry wins).
    ///
    /// This exercises the second `insert` call-site in `build_old_role_map` and
    /// pins the observable behavior for callers: the cell ends up mapped to
    /// `(guard_cell, 1u8)` (the else-branch tag) when it appears in both
    /// branches.  Real compiled modules can produce this pattern (e.g. an
    /// "effective" output cell that is active in both guard branches).
    #[test]
    fn build_old_role_map_intra_group_duplicate_last_write_wins() {
        use std::collections::HashMap;

        use crate::graph::GuardedGroupInfo;

        use super::build_old_role_map;

        let g1 = ValueCellId::new("E1", "guard");
        let shared = ValueCellId::new("E1", "shared");

        // `shared` appears in both `members` (branch 0) and `else_members`
        // (branch 1) of the same group.  Expected: no panic; else_members wins.
        let group = GuardedGroupInfo {
            guard_cell: g1.clone(),
            members: vec![shared.clone()],
            else_members: vec![shared.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let map: HashMap<ValueCellId, (ValueCellId, u8)> = build_old_role_map(&[group]);

        // One entry; else_members (branch 1) overwrites members (branch 0).
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&shared), Some(&(g1.clone(), 1u8)));
    }

    /// When `guard_val = Bool(true)`, `reelaborate_guarded_group` must:
    ///   (a) evaluate the active-branch `members` cell's `default_expr` and
    ///       write the result into both `values` and `snapshot_values` with
    ///       `DeterminacyState::Determined`;
    ///   (b) deactivate inactive non-Auto `else_members` cells
    ///       (`Value::Undef` / `Undetermined`);
    ///   (c) leave inactive Auto `else_members` cells absent from both maps
    ///       (Auto cell lifecycle is owned by the solver, not guard logic).
    #[test]
    fn reelaborate_guarded_group_activates_members_when_guard_true() {
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");
        let else_member_id = ValueCellId::new("E", "else_member");
        let auto_else_id = ValueCellId::new("E", "auto_else");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));
        graph.value_cells.insert(member_id.clone(), make_cell(&member_id, ValueCellKind::Param, Type::Int, Some(CompiledExpr::literal(Value::Int(42), Type::Int))));
        graph.value_cells.insert(else_member_id.clone(), make_cell(&else_member_id, ValueCellKind::Param, Type::Int, None));
        graph.value_cells.insert(auto_else_id.clone(), make_cell(&auto_else_id, ValueCellKind::Auto { free: false }, Type::Real, None));

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![else_member_id.clone(), auto_else_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member: evaluated default_expr → Int(42), Determined.
        assert_eq!(values.get(&member_id), Some(&Value::Int(42)));
        assert_eq!(
            snapshot_values.get(&member_id),
            Some(&(Value::Int(42), DeterminacyState::Determined))
        );

        // Inactive non-Auto else_member: deactivated to Undef / Undetermined.
        assert_eq!(values.get(&else_member_id), Some(&Value::Undef));
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined))
        );

        // Inactive Auto else_member: absent from both maps.
        assert!(values.get(&auto_else_id).is_none(), "Auto cell must not appear in values");
        assert!(
            snapshot_values.get(&auto_else_id).is_none(),
            "Auto cell must not appear in snapshot_values"
        );
    }

    /// Pins the documented "Cells without a `default_expr` … are left unchanged"
    /// contract for the **active branch** of `reelaborate_guarded_group`.
    ///
    /// The member cell IS present in `graph.value_cells` but its `default_expr`
    /// is `None`, so the inner `if let Some(ref expr) = node.default_expr` guard
    /// fails and the function must silently skip the cell — leaving both `values`
    /// and `snapshot_values` empty for it.
    ///
    /// A regression that replaced the guarded `if let Some(node) = … && let
    /// Some(ref expr) = node.default_expr` with an unconditional insert (or that
    /// silently inserted `Value::Undef` on the missing-expr branch) would be
    /// caught here.
    #[test]
    fn reelaborate_guarded_group_active_member_without_default_expr_is_noop() {
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");

        let mut graph = EvaluationGraph::default();
        // Guard cell is present (guard itself doesn't need a default_expr).
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));
        // Member cell IS present in the graph, but has no default_expr.
        graph.value_cells.insert(member_id.clone(), make_cell(&member_id, ValueCellKind::Param, Type::Int, None));

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member with no default_expr: must be left entirely untouched.
        assert!(
            values.get(&member_id).is_none(),
            "Active member with default_expr=None must not appear in values"
        );
        assert!(
            snapshot_values.get(&member_id).is_none(),
            "Active member with default_expr=None must not appear in snapshot_values"
        );
    }

    /// Pins the "absent from the graph" half of the documented "Cells without a
    /// `default_expr` (or absent from the graph) are left unchanged" contract for
    /// the **active branch** of `reelaborate_guarded_group`.
    ///
    /// The member ID is included in `group.members` but is NOT inserted into
    /// `graph.value_cells`, so the outer `if let Some(node) = graph.value_cells.get(mid)`
    /// guard fails and the function must silently skip the cell — leaving both
    /// `values` and `snapshot_values` empty for it.
    ///
    /// A regression that dropped this guard (e.g. via `&graph.value_cells[mid]`,
    /// `.unwrap()`, or any unconditional insert keyed on the raw member id) would
    /// be caught here.
    #[test]
    fn reelaborate_guarded_group_active_member_absent_from_graph_is_noop() {
        let guard_id = ValueCellId::new("E", "guard");
        // member_id is referenced in the group but intentionally NOT inserted
        // into graph.value_cells — it is wholly absent from the graph.
        let member_id = ValueCellId::new("E", "member");

        let mut graph = EvaluationGraph::default();
        // Only the guard cell is in the graph.
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone()],
            else_members: vec![],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Active member absent from graph: must be left entirely untouched.
        assert!(
            values.get(&member_id).is_none(),
            "Active member absent from graph must not appear in values"
        );
        assert!(
            snapshot_values.get(&member_id).is_none(),
            "Active member absent from graph must not appear in snapshot_values"
        );
    }

    /// Pins the behavior of `reelaborate_guarded_group` on the **inactive branch**
    /// for `else_members` when an `else_member` is present in the graph but its
    /// `default_expr` is `None`.
    ///
    /// With `guard_val = Bool(true)`, `else_members` are on the **inactive branch**
    /// and are passed to `deactivate_if_not_auto`. That helper does NOT inspect
    /// `default_expr` — it only checks whether the cell is `Auto`. A non-Auto cell
    /// (here `ValueCellKind::Param`) must be written as
    /// `Value::Undef / DeterminacyState::Undetermined` regardless of whether it
    /// carries a `default_expr`.
    ///
    /// A regression that skipped deactivation for cells without a `default_expr`
    /// (e.g. by wrapping the `deactivate_if_not_auto` call in a `default_expr.is_some()`
    /// guard) would be caught here.
    #[test]
    fn reelaborate_guarded_group_inactive_else_member_without_default_expr_is_deactivated() {
        let guard_id = ValueCellId::new("E", "guard");
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));
        // else_member IS in the graph but has no default_expr.
        graph.value_cells.insert(else_member_id.clone(), make_cell(&else_member_id, ValueCellKind::Param, Type::Int, None));

        // guard=true → members active, else_members INACTIVE → deactivate_if_not_auto.
        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // deactivate_if_not_auto does not check default_expr; Param → Undef/Undetermined.
        assert_eq!(
            values.get(&else_member_id),
            Some(&Value::Undef),
            "Inactive Param else_member with default_expr=None must be deactivated to Undef"
        );
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined)),
            "Inactive Param else_member with default_expr=None must be Undetermined in snapshot_values"
        );
    }

    /// Pins the behavior of `reelaborate_guarded_group` on the **inactive branch**
    /// for `else_members` when an `else_member` is wholly absent from
    /// `graph.value_cells`.
    ///
    /// With `guard_val = Bool(true)`, `else_members` are on the **inactive branch**
    /// and are passed to `deactivate_if_not_auto`. That helper treats a missing cell
    /// as non-Auto (preserving the prior `is_some_and` semantics documented in its
    /// docstring) and writes `Value::Undef / DeterminacyState::Undetermined`.
    ///
    /// A regression that skipped absent cells on the inactive branch (e.g. by
    /// wrapping the `deactivate_if_not_auto` call in a `graph.value_cells.get(mid)
    /// .is_some()` guard) would be caught here.
    #[test]
    fn reelaborate_guarded_group_inactive_else_member_absent_from_graph_is_deactivated() {
        let guard_id = ValueCellId::new("E", "guard");
        // else_member_id is included in the group but NOT inserted into graph.
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        // Only the guard cell is in the graph.
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));

        // guard=true → members active, else_members INACTIVE → deactivate_if_not_auto.
        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        let (values, snapshot_values) = run_with_guard(graph, group, true);

        // Missing cell → non-Auto treatment → Undef/Undetermined.
        assert_eq!(
            values.get(&else_member_id),
            Some(&Value::Undef),
            "Absent else_member must be deactivated to Undef on the inactive branch"
        );
        assert_eq!(
            snapshot_values.get(&else_member_id),
            Some(&(Value::Undef, DeterminacyState::Undetermined)),
            "Absent else_member must be Undetermined in snapshot_values on the inactive branch"
        );
    }

    /// Pins the symmetric behavior of the shared
    /// `for (cells, is_active) in [(&group.members, is_true), (&group.else_members, is_false)]`
    /// loop at engine_edit.rs:68 across both guard values in a single test.
    ///
    /// Under `guard=true`: `members` are on the active branch — cells without a
    /// `default_expr` (present or absent) are left untouched; `else_members` are
    /// on the inactive branch — all non-Auto cells become `Value::Undef /
    /// Undetermined`.
    ///
    /// Under `guard=false`: roles flip — `else_members` become active (→ no-op),
    /// `members` become inactive (→ `Value::Undef / Undetermined`).
    ///
    /// A hypothetical refactor that unrolls the loop and breaks one side
    /// asymmetrically would fail at least one assertion here.
    #[test]
    fn reelaborate_guarded_group_members_and_else_members_are_symmetric_across_guard_values() {
        let guard_id = ValueCellId::new("E", "guard");
        let member_present_id = ValueCellId::new("E", "member_present");
        let member_absent_id = ValueCellId::new("E", "member_absent");
        let else_present_id = ValueCellId::new("E", "else_present");
        let else_absent_id = ValueCellId::new("E", "else_absent");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));
        // Both present cells have no default_expr so the active-branch no-op path is exercised.
        graph.value_cells.insert(member_present_id.clone(), make_cell(&member_present_id, ValueCellKind::Param, Type::Int, None));
        graph.value_cells.insert(else_present_id.clone(), make_cell(&else_present_id, ValueCellKind::Param, Type::Int, None));
        // member_absent_id and else_absent_id are intentionally NOT inserted.

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_present_id.clone(), member_absent_id.clone()],
            else_members: vec![else_present_id.clone(), else_absent_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        // ── guard=true: members active (no-op), else_members inactive (→ Undef) ──
        let (values, snap) = run_with_guard(graph.clone(), group.clone(), true);
        assert!(values.get(&member_present_id).is_none(), "active member (no default_expr) must not appear in values");
        assert!(snap.get(&member_present_id).is_none(), "active member (no default_expr) must not appear in snapshot_values");
        assert!(values.get(&member_absent_id).is_none(), "active absent member must not appear in values");
        assert!(snap.get(&member_absent_id).is_none(), "active absent member must not appear in snapshot_values");
        assert_eq!(values.get(&else_present_id), Some(&Value::Undef), "inactive else_member must be Undef");
        assert_eq!(snap.get(&else_present_id), Some(&(Value::Undef, DeterminacyState::Undetermined)), "inactive else_member must be Undetermined");
        assert_eq!(values.get(&else_absent_id), Some(&Value::Undef), "inactive absent else_member must be Undef");
        assert_eq!(snap.get(&else_absent_id), Some(&(Value::Undef, DeterminacyState::Undetermined)), "inactive absent else_member must be Undetermined");

        // ── guard=false: else_members active (no-op), members inactive (→ Undef) ──
        let (values, snap) = run_with_guard(graph, group, false);
        assert!(values.get(&else_present_id).is_none(), "active else_member (no default_expr) must not appear in values");
        assert!(snap.get(&else_present_id).is_none(), "active else_member (no default_expr) must not appear in snapshot_values");
        assert!(values.get(&else_absent_id).is_none(), "active absent else_member must not appear in values");
        assert!(snap.get(&else_absent_id).is_none(), "active absent else_member must not appear in snapshot_values");
        assert_eq!(values.get(&member_present_id), Some(&Value::Undef), "inactive member must be Undef");
        assert_eq!(snap.get(&member_present_id), Some(&(Value::Undef, DeterminacyState::Undetermined)), "inactive member must be Undetermined");
        assert_eq!(values.get(&member_absent_id), Some(&Value::Undef), "inactive absent member must be Undef");
        assert_eq!(snap.get(&member_absent_id), Some(&(Value::Undef, DeterminacyState::Undetermined)), "inactive absent member must be Undetermined");
    }

    /// When `guard_val = Bool(false)`, `reelaborate_guarded_group` must
    /// activate `else_members` and deactivate `members`.
    ///
    /// Also covers the non-Bool (`Value::Undef`) guard edge case: neither
    /// branch becomes active, so ALL members and else_members follow the
    /// deactivation path (non-Auto → Undef, Auto → absent).
    #[test]
    fn reelaborate_guarded_group_activates_else_members_when_guard_false() {
        // ── Shared graph ──────────────────────────────────────────────────
        let guard_id = ValueCellId::new("E", "guard");
        let member_id = ValueCellId::new("E", "member");
        let auto_member_id = ValueCellId::new("E", "auto_member");
        let else_member_id = ValueCellId::new("E", "else_member");

        let mut graph = EvaluationGraph::default();
        graph.value_cells.insert(guard_id.clone(), make_cell(&guard_id, ValueCellKind::Param, Type::Bool, None));
        graph.value_cells.insert(member_id.clone(), make_cell(&member_id, ValueCellKind::Param, Type::Int, None));
        graph.value_cells.insert(auto_member_id.clone(), make_cell(&auto_member_id, ValueCellKind::Auto { free: false }, Type::Real, None));
        graph.value_cells.insert(else_member_id.clone(), make_cell(&else_member_id, ValueCellKind::Param, Type::Int, Some(CompiledExpr::literal(Value::Int(7), Type::Int))));

        let group = GuardedGroupInfo {
            guard_cell: guard_id.clone(),
            members: vec![member_id.clone(), auto_member_id.clone()],
            else_members: vec![else_member_id.clone()],
            constraints: vec![],
            else_constraints: vec![],
        };

        // ── guard = false: else_members active, members deactivated ───────
        {
            let mut values: ValueMap = ValueMap::default();
            let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
                PersistentMap::default();

            reelaborate_guarded_group(
                &graph,
                &group,
                &Value::Bool(false),
                &mut values,
                &mut snapshot_values,
                &[],
                &HashMap::new(),
            );

            // Active else_member: evaluated default_expr → Int(7), Determined.
            assert_eq!(values.get(&else_member_id), Some(&Value::Int(7)));
            assert_eq!(
                snapshot_values.get(&else_member_id),
                Some(&(Value::Int(7), DeterminacyState::Determined))
            );

            // Inactive non-Auto member: deactivated.
            assert_eq!(values.get(&member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );

            // Inactive Auto member: absent.
            assert!(values.get(&auto_member_id).is_none(), "Auto member must not appear");
            assert!(snapshot_values.get(&auto_member_id).is_none(), "Auto member must not appear");
        }

        // ── guard = Undef (non-Bool): both branches inactive ─────────────
        {
            let mut values: ValueMap = ValueMap::default();
            let mut snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
                PersistentMap::default();

            reelaborate_guarded_group(
                &graph,
                &group,
                &Value::Undef,
                &mut values,
                &mut snapshot_values,
                &[],
                &HashMap::new(),
            );

            // Both branches deactivated: non-Auto → Undef, Auto → absent.
            assert_eq!(values.get(&member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );
            assert!(values.get(&auto_member_id).is_none(), "Auto member must not appear");
            assert_eq!(values.get(&else_member_id), Some(&Value::Undef));
            assert_eq!(
                snapshot_values.get(&else_member_id),
                Some(&(Value::Undef, DeterminacyState::Undetermined))
            );
        }
    }
}
