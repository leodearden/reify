// Split from lib.rs (task 2032) — eval methods.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use reify_compiler::{find_template, CompiledModule, ValueCellKind};
use reify_types::{
    AutoParam, CompiledFunction, DeterminacyState, Diagnostic, FIELD_ENTITY_PREFIX,
    ResolutionProblem, SnapshotId, SnapshotProvenance, SolveResult, Value, ValueCellId, ValueMap,
    VersionId,
};

use crate::cache::{CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex, extract_dependency_trace, take_trace};
use crate::dirty::topological_sort;
use crate::journal::{EvalEvent, EventKind, EventPayload};
use crate::snapshot::Snapshot;
use crate::unfold::{elaborate_child_instance, unfold_recursive_sub};
use crate::{
    CacheStats, CachedEvalResult, Engine, EvalResult, EvaluationState, GuardLookup,
    guard_state_fingerprint,
};

/// Populate a fresh `DemandRegistry` with the full per-node-kind demand
/// set for a graph and rebuild its cone.
///
/// This is the demand-initialization block shared by `Engine::eval` and
/// `Engine::edit_source`: every value cell, constraint, and realization
/// node in the graph is marked always-demanded, then `rebuild_cone` is
/// called so `is_demanded` reflects the transitive closure. Kept as a
/// single helper so a future node kind (e.g. Resolution, once it
/// participates in demand) is added once rather than drifting between
/// the two call sites.
pub(crate) fn build_demand_for_graph(
    graph: &crate::graph::EvaluationGraph,
) -> DemandRegistry {
    let mut demand = DemandRegistry::new();
    for (_, node) in graph.value_cells.iter() {
        demand.add_demand(NodeId::Value(node.id.clone()));
    }
    for (_, cnode) in graph.constraints.iter() {
        demand.add_demand(NodeId::Constraint(cnode.id.clone()));
    }
    for (_, rnode) in graph.realizations.iter() {
        demand.add_demand(NodeId::Realization(rnode.id.clone()));
    }
    demand.rebuild_cone(graph);
    demand
}

impl Engine {
    /// Evaluate a compiled module, returning computed values.
    ///
    /// This is a cold-start evaluation that builds a new Snapshot and
    /// dependency structures. Subsequent calls to edit_param() can perform
    /// incremental re-evaluation using these structures.
    pub fn eval(&mut self, module: &CompiledModule) -> EvalResult {
        // Store functions and purposes for this module (used by edit_param and purpose activation).
        //
        // SHADOWING INVARIANT: module (user) functions are stored FIRST, then prelude functions
        // are appended after. `reify_expr::eval_user_function_call` resolves calls via
        // `ctx.functions.iter().find(...)` — a first-match-wins linear scan on (name, arity,
        // param types). Therefore, any user function whose signature matches a prelude function
        // takes precedence and shadows the prelude implementation. The compiler's
        // duplicate-function check only compares user functions against each other, not against
        // the prelude, so user code may freely redefine prelude signatures without diagnostics.
        //
        // COEXISTENCE COROLLARY: a user function whose (name, arity, param types) triple
        // differs from all prelude functions does NOT shadow those prelude functions — both
        // remain independently callable. The compiler includes non-shadowed prelude functions
        // in its overload resolution so each call site is resolved to whichever signature
        // matches the arguments, regardless of whether the user also defines a same-named
        // function with a different arity or param types.
        self.functions = module.functions.clone();
        // Unfiltered append: intentionally adds ALL prelude functions without filtering out
        // entries whose (name, arity, param_types) triple matches a user function.
        // Correctness is preserved because `reify_expr::eval_user_function_call` resolves
        // via first-match-wins linear scan, and user functions are stored FIRST (see
        // SHADOWING INVARIANT above) — shadowed prelude entries can never be the first
        // match, so they are permanently unreachable at dispatch time.
        //
        // This diverges from the compiler's `resolution_functions` build in
        // compile_with_prelude_refs, which applies an explicit shadow filter via
        // `reify_compiler::merge_prelude_functions`. That filter is a compile-time
        // concern (it avoids ambiguous-overload errors in the resolution table); the
        // eval dispatch table does not need it — unfiltered ≡ filtered under
        // first-match-wins semantics. The shadow predicate itself is canonical in
        // `merge_prelude_functions`; if the filtering rule changes, update that
        // function and verify the dispatch-time equivalence still holds.
        self.functions
            .extend(self.prelude_functions.iter().cloned());
        self.compiled_purposes = module.compiled_purposes.clone();
        // Clear stale purpose state from previous eval() calls — the fresh
        // snapshot discards all purpose-injected constraints/objectives.
        self.active_purposes.clear();
        self.active_objective_map.clear();
        // Build meta_map: template name → meta key/value pairs.
        // Only includes templates with non-empty meta blocks.
        self.meta_map = module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect();
        // Use the merged function table (user functions prepended before prelude functions) so
        // that EvalContext has the full dispatch set — both user-defined overloads AND
        // non-shadowed prelude functions. This matches the SHADOWING INVARIANT: first-match-wins
        // linear scan means user functions take precedence when signatures collide, while
        // prelude functions with distinct (name, arity, param types) triples remain callable.
        // Clone here to satisfy the borrow checker: `evaluate_let_bindings` borrows `self`
        // mutably, which would conflict with an immutable borrow of `self.functions`.
        //
        // PERFORMANCE NOTE: eval() currently clones the merged function table TWICE per call —
        // once when assigning `self.functions = module.functions.clone()` (then extending in
        // place with the prelude above), and again into the local `functions` below so
        // EvalContext can hold it without aliasing `self`. Each CompiledFunction contains a
        // boxed expression tree, so for a nontrivial user module plus the 11-module stdlib
        // the double-clone is a non-trivial allocation on the hot path (every edit_param,
        // check, build, and tessellate triggers an eval). The natural fix is to change
        // `self.functions` to `Arc<Vec<CompiledFunction>>` so both clones become O(1):
        //
        //   self.functions = Arc::new({ let mut v = module.functions.clone();
        //                               v.extend(prelude); v });
        //   let functions = Arc::clone(&self.functions);   // O(1)
        //
        // That refactor also requires updating `ConcurrentEditSetup::functions` in
        // concurrent.rs (field type `Vec<CompiledFunction>`, assigned as
        // `functions: self.functions.clone()`) — which lies outside this task's locked
        // modules. The same pattern repeats in edit_param() below. Deferred to
        // task #1997 (perf: Arc<Vec<CompiledFunction>> in Engine::eval/edit_param).
        let functions: Vec<CompiledFunction> = self.functions.clone();

        let mut values = ValueMap::new();
        let mut diagnostics = Vec::new();

        // Build Snapshot from CompiledModule (creates EvaluationGraph internally)
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;

        let mut snapshot = Snapshot::from_compiled_module(module);
        snapshot.id = SnapshotId(snapshot_id);
        snapshot.version = VersionId(version_id);
        snapshot.provenance = SnapshotProvenance::Initial;

        // Build dependency structures from the graph
        let reverse_index = ReverseDependencyIndex::build_from_graph(&snapshot.graph);
        let trace_map = crate::deps::build_trace_map(&snapshot.graph);

        // Set up demand registry: demand all value cells, constraints, and
        // realizations, then rebuild the cone. Shared helper keeps this in
        // sync with the matching block in `Engine::edit_source`.
        let demand = build_demand_for_graph(&snapshot.graph);

        // Evaluate field declarations first: they must be available in the
        // values map before templates are evaluated, because structure
        // expressions may reference fields (e.g., `sample(my_field, point)`).
        for field in &module.fields {
            // Both variants carry a callable lambda expr; evaluation is identical.
            let lambda_value = match &field.source {
                reify_compiler::CompiledFieldSource::Analytical { expr }
                | reify_compiler::CompiledFieldSource::Composed { expr } => {
                    let ctx =
                        reify_expr::EvalContext::new(&values, &functions).with_meta(&self.meta_map);
                    let val = reify_expr::eval_expr(expr, &ctx);
                    Arc::new(val)
                }
                reify_compiler::CompiledFieldSource::Sampled { .. }
                | reify_compiler::CompiledFieldSource::Imported => Arc::new(Value::Undef),
            };

            let source_kind = match &field.source {
                reify_compiler::CompiledFieldSource::Analytical { .. } => {
                    reify_types::FieldSourceKind::Analytical
                }
                reify_compiler::CompiledFieldSource::Sampled { .. } => {
                    reify_types::FieldSourceKind::Sampled
                }
                reify_compiler::CompiledFieldSource::Composed { .. } => {
                    reify_types::FieldSourceKind::Composed
                }
                reify_compiler::CompiledFieldSource::Imported => {
                    reify_types::FieldSourceKind::Imported
                }
            };

            let field_value = Value::Field {
                domain_type: field.domain_type.clone(),
                codomain_type: field.codomain_type.clone(),
                source: source_kind,
                lambda: lambda_value,
            };

            let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
            values.insert(field_id.clone(), field_value.clone());
            snapshot
                .values
                .insert(field_id, (field_value, DeterminacyState::Determined));
        }

        // Two-pass evaluation (same logic as before)
        for template in &module.templates {
            // First pass: evaluate Param defaults and Auto cells to populate the value map
            for cell in &template.value_cells {
                if cell.kind.is_auto() {
                    // Auto cells: Undef with DeterminacyState::Auto
                    let node_id = NodeId::Value(cell.id.clone());
                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version: VersionId(version_id),
                        payload: None,
                    });

                    values.insert(cell.id.clone(), reify_types::Value::Undef);
                    snapshot.values.insert(
                        cell.id.clone(),
                        (reify_types::Value::Undef, DeterminacyState::Auto),
                    );

                    // Record in cache
                    let trace = DependencyTrace::default();
                    let cached_result =
                        CachedResult::Value(reify_types::Value::Undef, DeterminacyState::Auto);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                } else if cell.kind == ValueCellKind::Param
                    && let Some(ref expr) = cell.default_expr
                {
                    let node_id = NodeId::Value(cell.id.clone());
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
                        &reify_expr::EvalContext::new(&values, &functions)
                            .with_meta(&self.meta_map)
                            .with_determinacy(&snapshot.values),
                    );
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot
                        .values
                        .insert(cell.id.clone(), (val.clone(), DeterminacyState::Determined));

                    // Record in cache
                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                }
            }

            // Second pass: evaluate Let bindings in topological order
            // (handles forward references where a let declared earlier
            //  depends on a let declared later)
            {
                let meta_map = self.meta_map.clone();
                self.evaluate_let_bindings(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    &functions,
                    &meta_map,
                    &mut diagnostics,
                );
            }

            // Third pass: evaluate guarded groups.
            // Guard cells are Let-kind synthetic cells — evaluate their expressions,
            // then conditionally evaluate members based on guard truth value.
            for group in &template.guarded_groups {
                // Evaluate the guard cell expression
                let guard_val = reify_expr::eval_expr(
                    &group.guard_expr,
                    &reify_expr::EvalContext::new(&values, &functions)
                        .with_meta(&self.meta_map)
                        .with_determinacy(&snapshot.values),
                );
                values.insert(group.guard_value_cell.clone(), guard_val.clone());

                let guard_determinacy = match &guard_val {
                    Value::Bool(_) => DeterminacyState::Determined,
                    _ => DeterminacyState::Undetermined,
                };
                snapshot.values.insert(
                    group.guard_value_cell.clone(),
                    (guard_val.clone(), guard_determinacy),
                );

                let guard_is_true = matches!(&guard_val, Value::Bool(true));
                let guard_is_false = matches!(&guard_val, Value::Bool(false));

                // Evaluate members (active when guard is true)
                for cell in &group.members {
                    if guard_is_true {
                        // Evaluate normally
                        if cell.kind == ValueCellKind::Param || cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind.is_auto() {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is false or Undef — member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind.is_auto() {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot.values.insert(cell.id.clone(), (Value::Undef, det));
                    }
                }

                // Evaluate else_members (active when guard is false)
                for cell in &group.else_members {
                    if guard_is_false {
                        if cell.kind == ValueCellKind::Param || cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind.is_auto() {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is true or Undef — else member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind.is_auto() {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot.values.insert(cell.id.clone(), (Value::Undef, det));
                    }
                }
            }
        }

        // Sub-component elaboration: evaluate child template params/lets
        // for each sub_component in each template.
        for template in &module.templates {
            for sub in &template.sub_components {
                // Find the referenced child template by name
                let child_template = match find_template(&module.templates, &sub.structure_name) {
                    Some(t) => t,
                    None => {
                        diagnostics.push(Diagnostic::error(format!(
                            "sub-component \"{}\" references unknown structure \"{}\"",
                            sub.name, sub.structure_name
                        )));
                        continue;
                    }
                };

                // Collection sub: determine count, then elaborate N instances
                if sub.is_collection {
                    let count = if let Some(ref count_cell_id) = sub.count_cell {
                        // The count cell value should already be evaluated (it's a let binding)
                        match values.get(count_cell_id) {
                            Some(Value::Int(n)) => Some(*n),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(n) = count {
                        for idx in 0..n {
                            let scoped_entity = format!("{}.{}[{}]", template.name, sub.name, idx);
                            elaborate_child_instance(
                                &mut values,
                                &mut snapshot,
                                &functions,
                                &mut self.journal,
                                &mut self.cache,
                                version_id,
                                child_template,
                                &scoped_entity,
                                &sub.args,
                                &self.meta_map,
                                &mut diagnostics,
                            );
                        }

                        // Create per-member synthetic lists: __list_{name}__{member} for each value cell
                        for child_cell in &child_template.value_cells {
                            let member_items: Vec<Value> = (0..n)
                                .map(|idx| {
                                    let scoped_id = ValueCellId::new(
                                        format!("{}.{}[{}]", template.name, sub.name, idx),
                                        &child_cell.id.member,
                                    );
                                    values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                                })
                                .collect();
                            let member_list_id = ValueCellId::new(
                                &template.name,
                                format!("__list_{}__{}", sub.name, child_cell.id.member),
                            );
                            let member_list_val = Value::List(member_items);
                            values.insert(member_list_id.clone(), member_list_val.clone());
                            snapshot.values.insert(
                                member_list_id,
                                (member_list_val, DeterminacyState::Determined),
                            );
                        }
                    }
                    // If count is None (Undef), no instances are created
                    continue;
                }

                // Guarded sub on a non-recursive template: do NOT elaborate.
                // A guard on a sub is meaningful only in a recursive context. The
                // child-frame analog is the `child_template.is_recursive` filter
                // inside `unfold_recursive_sub` (see the recursive-subs collection
                // below). This branch provides the symmetric root-frame gate: if
                // the top-level template is non-recursive and the sub is guarded,
                // Phase 2 must not materialise the sub's entity.
                if !template.is_recursive && sub.guard_expr.is_some() {
                    continue;
                }

                // Recursive sub: evaluate guard before elaborating, then unfold recursively.
                if template.is_recursive && sub.guard_expr.is_some() {
                    let mut unfold_budget = self.max_unfold_nodes;
                    unfold_recursive_sub(
                        &mut values,
                        &mut snapshot,
                        &functions,
                        &mut self.journal,
                        &mut self.cache,
                        version_id,
                        template,       // scope_template: owns `sub` (guard/arg refs match)
                        child_template, // target template for Phase 1 instantiation
                        sub,
                        &template.name,
                        0,
                        self.max_unfold_depth,
                        &self.meta_map,
                        &mut diagnostics,
                        &module.templates,
                        &mut unfold_budget,
                    );
                    continue;
                }

                // Build scoped entity prefix: "ParentName.sub_name"
                let scoped_entity = format!("{}.{}", template.name, sub.name);

                elaborate_child_instance(
                    &mut values,
                    &mut snapshot,
                    &functions,
                    &mut self.journal,
                    &mut self.cache,
                    version_id,
                    child_template,
                    &scoped_entity,
                    &sub.args,
                    &self.meta_map,
                    &mut diagnostics,
                );
            }

            // Re-evaluate let bindings that may depend on sub-component cells:
            // - collection subs create __list_* synthetic cells
            // - regular subs create {parent}.{sub}.{member} cells via elaborate_child_instance
            // Both become available only after elaboration, so re-evaluate if any subs exist.
            if !template.sub_components.is_empty() {
                let meta_map = self.meta_map.clone();
                self.evaluate_let_bindings(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    &functions,
                    &meta_map,
                    &mut diagnostics,
                );
            }
        }

        // Resolution phase: resolve auto params using the constraint solver.
        let mut resolved_params = HashMap::new();
        if self.solver.is_some() {
            // Refresh template-native objectives so edit_param() can access them.
            self.objectives.clear();
            for template in &module.templates {
                if let Some(obj) = &template.objective {
                    self.objectives.insert(template.name.clone(), obj.clone());
                }
            }
            for template in &module.templates {
                // Collect auto param IDs for this template
                let auto_ids: std::collections::HashSet<ValueCellId> = template
                    .value_cells
                    .iter()
                    .filter(|cell| cell.kind.is_auto())
                    .map(|cell| cell.id.clone())
                    .collect();

                if auto_ids.is_empty() {
                    continue;
                }

                // Find constraints whose dependency traces reference auto params
                let filtered_constraints: Vec<_> = template
                    .constraints
                    .iter()
                    .filter(|c| {
                        let trace = extract_dependency_trace(&c.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|c| (c.id.clone(), c.expr.clone()))
                    .collect();

                // Build AutoParam list from template value cells
                let auto_param_list: Vec<AutoParam> = template
                    .value_cells
                    .iter()
                    .filter(|cell| cell.kind.is_auto())
                    .map(|cell| AutoParam {
                        id: cell.id.clone(),
                        param_type: cell.cell_type.clone(),
                        bounds: None,
                        free: cell.kind.is_auto_free(),
                    })
                    .collect();

                // Build ResolutionProblem
                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: values.clone(),
                    objective: template.objective.clone(),
                    functions: functions.clone(),
                };

                let parent_snap_id = snapshot.id;
                // Use a temporary borrow of the solver so the reference
                // doesn't outlive the solve() call — this allows &mut self
                // for evaluate_let_bindings below.
                let solve_result = self.solver.as_ref().unwrap().solve(&problem);

                match solve_result {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        // Allocate new snapshot/version IDs BEFORE recording cache
                        // entries so all resolution-phase entries share the same
                        // basis_version as the snapshot. This preserves the invariant
                        // that try_fast_path relies on for incremental evaluation.
                        let res_snapshot_id = self.next_snapshot_id;
                        self.next_snapshot_id += 1;
                        let res_version_id = self.next_version_id;
                        self.next_version_id += 1;

                        // Update values map with resolved values
                        let mut resolved_ids = std::collections::HashSet::new();
                        for (id, val) in &solver_values {
                            let node_id = NodeId::Value(id.clone());
                            let start = Instant::now();
                            self.journal.record(EvalEvent {
                                timestamp: start,
                                node_id: node_id.clone(),
                                kind: EventKind::Started,
                                version: VersionId(res_version_id),
                                payload: None,
                            });

                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            resolved_ids.insert(id.clone());

                            // Update snapshot values with (resolved_val, Determined)
                            snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

                            // Update cache with res_version_id (matches snapshot)
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            let outcome = self.cache.record_evaluation(
                                node_id.clone(),
                                cached_result,
                                VersionId(res_version_id),
                                trace,
                            );

                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed { outcome },
                                version: VersionId(res_version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                        }

                        // Emit warning for free auto params when solution is non-unique
                        if !unique {
                            for ap in &auto_param_list {
                                if ap.free {
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }

                        // Set child snapshot with Resolution provenance
                        snapshot.id = SnapshotId(res_snapshot_id);
                        snapshot.version = VersionId(res_version_id);
                        snapshot.provenance = SnapshotProvenance::Resolution {
                            scope: template.name.clone(),
                            resolved: resolved_ids,
                            parent: parent_snap_id,
                        };

                        // Re-run let binding evaluation in topological order
                        let meta_map = self.meta_map.clone();
                        self.evaluate_let_bindings(
                            template,
                            &mut values,
                            &mut snapshot,
                            res_version_id,
                            &functions,
                            &meta_map,
                            &mut diagnostics,
                        );
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
        }

        // ── Guard-state fingerprinting ──────────────────────────────
        // Include guard-cell boolean states in the topology fingerprint so that
        // eval() and edit_param() produce identical fingerprints for the same
        // logical guard configuration.
        if !snapshot.graph.guarded_groups.is_empty() {
            let guard_state_hash = guard_state_fingerprint(
                &snapshot.graph.guarded_groups,
                &values,
                GuardLookup::Lenient,
            );
            snapshot.topology_fingerprint = snapshot
                .graph
                .topology_fingerprint()
                .combine(guard_state_hash);
        }

        // Store internal state for incremental evaluation
        self.eval_state = Some(EvaluationState {
            snapshot,
            reverse_index,
            trace_map,
        });
        self.demand = demand;
        self.last_eval_set = Vec::new(); // Cold start: no incremental eval set

        EvalResult {
            values,
            diagnostics,
            resolved_params,
        }
    }

    /// Evaluate a compiled module with caching and early cutoff.
    ///
    /// On first call (cold start), behaves like eval() but populates the cache.
    /// On subsequent calls with the same version, uses version fast path.
    /// On calls with a new version after invalidation, re-evaluates dirty nodes
    /// and uses early cutoff to avoid propagating unchanged results.
    pub fn eval_cached(&mut self, module: &CompiledModule, version: VersionId) -> CachedEvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();
        let mut stats = CacheStats::default();

        // Build meta_map from module templates (same logic as eval()).
        // This ensures MetaAccess expressions resolve correctly even when
        // eval_cached is called without a prior eval().
        self.meta_map = module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect();

        for template in &module.templates {
            // First pass: evaluate Param defaults, Auto cells, (or use overrides)
            for cell in &template.value_cells {
                if cell.kind.is_auto() {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check cache reuse (not dirty, no override)
                    if !self.param_overrides.contains_key(&cell.id)
                        && !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    // Use override if available, otherwise Undef with Auto determinacy
                    let (val, det) = if let Some(override_val) = self.param_overrides.get(&cell.id)
                    {
                        (override_val.clone(), DeterminacyState::Determined)
                    } else {
                        (reify_types::Value::Undef, DeterminacyState::Auto)
                    };

                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val.clone(), det);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                    }

                    values.insert(cell.id.clone(), val);
                } else if cell.kind == ValueCellKind::Param {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check if cache entry still exists and is not dirty.
                    // For params without overrides, we can reuse cached values.
                    if !self.param_overrides.contains_key(&cell.id)
                        && !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    // Use override if available, otherwise evaluate default
                    let val = if let Some(override_val) = self.param_overrides.get(&cell.id) {
                        override_val.clone()
                    } else if let Some(ref expr) = cell.default_expr {
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(&values, &self.functions)
                                .with_meta(&self.meta_map),
                        )
                    } else {
                        reify_types::Value::Undef
                    };

                    // Build dependency trace (params have no reads - they are roots)
                    let trace = DependencyTrace::default();

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                    }

                    values.insert(cell.id.clone(), val);
                }
            }

            // Second pass: evaluate Let bindings
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Let
                    && let Some(ref expr) = cell.default_expr
                {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check if cache entry still exists and is not dirty.
                    // If so, the node's dependencies haven't changed, so we
                    // can reuse the cached result and update its basis_version.
                    if !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;
                    self.cache.clear_dirty(&node_id);

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    let val = reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(&values, &self.functions)
                            .with_meta(&self.meta_map),
                    );

                    // Build dependency trace from expression refs
                    let trace = extract_dependency_trace(expr);

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                        // Early cutoff: clear dirty flags on nodes that
                        // depend on this cell, since its result hasn't changed.
                        self.cache.clear_dependents_dirty(&cell.id);
                    }

                    values.insert(cell.id.clone(), val);
                }
            }
        }

        CachedEvalResult {
            eval_result: EvalResult {
                values,
                diagnostics,
                resolved_params: HashMap::new(),
            },
            stats,
        }
    }

    /// Evaluate let bindings from a template in topological order.
    ///
    /// Collects let cells with expressions, builds dependency traces,
    /// topologically sorts, and evaluates each in order — recording
    /// journal events and cache entries. Used by both the initial eval()
    /// pass and the post-resolution re-evaluation pass.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_let_bindings(
        &mut self,
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        snapshot: &mut Snapshot,
        version_id: u64,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let let_cells: HashMap<NodeId, &reify_types::CompiledExpr> = template
            .value_cells
            .iter()
            .filter(|c| c.kind == ValueCellKind::Let)
            .filter_map(|c| {
                c.default_expr
                    .as_ref()
                    .map(|expr| (NodeId::Value(c.id.clone()), expr))
            })
            .collect();

        let let_node_ids: HashSet<NodeId> = let_cells.keys().cloned().collect();
        let mut let_traces: HashMap<NodeId, DependencyTrace> = let_cells
            .iter()
            .map(|(nid, expr)| (nid.clone(), extract_dependency_trace(expr)))
            .collect();

        let sorted_lets = topological_sort(&let_node_ids, &let_traces);

        // Detect cyclic let-binding dependencies: if topological_sort dropped nodes
        // (Kahn's algorithm silently omits nodes in cycles), report them.
        if sorted_lets.len() < let_node_ids.len() {
            let sorted_set: HashSet<&NodeId> = sorted_lets.iter().collect();
            let mut cyclic_members: Vec<&str> = let_node_ids
                .iter()
                .filter(|nid| !sorted_set.contains(nid))
                .filter_map(|nid| match nid {
                    NodeId::Value(vcid) => Some(vcid.member.as_str()),
                    _ => None,
                })
                .collect();
            cyclic_members.sort();
            diagnostics.push(Diagnostic::error(format!(
                "circular let-binding dependency in template {}: [{}]",
                template.name,
                cyclic_members.join(", "),
            )));
        }

        for node_id in sorted_lets {
            let expr = let_cells[&node_id];
            // let_cells is keyed exclusively by NodeId::Value; topological_sort returns
            // only keys from that set — so this assertion holds in all correct code paths.
            // In debug/test builds it fires loud; in release the diagnostic+continue handles
            // any accidental invariant violation gracefully.
            debug_assert!(
                matches!(node_id, NodeId::Value(_)),
                "evaluate_let_bindings: sorted_lets produced a non-Value NodeId: {:?}; construction invariant violated",
                node_id,
            );
            let cell_id = match &node_id {
                NodeId::Value(vcid) => vcid,
                _ => {
                    diagnostics.push(Diagnostic::error(format!(
                        "let-binding evaluation: expected NodeId::Value, got {:?}; skipping",
                        node_id,
                    )));
                    continue;
                }
            };

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
                &reify_expr::EvalContext::new(values, functions)
                    .with_meta(meta_map)
                    .with_determinacy(&snapshot.values),
            );
            values.insert(cell_id.clone(), val.clone());

            snapshot
                .values
                .insert(cell_id.clone(), (val.clone(), DeterminacyState::Determined));

            // sorted_lets and let_traces are built from the same key set, so remove() cannot fail.
            let trace = take_trace(&mut let_traces, &node_id, "sorted_lets");
            let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
            let outcome = self.cache.record_evaluation(
                node_id.clone(),
                cached_result,
                VersionId(version_id),
                trace,
            );

            self.journal.record(EvalEvent {
                timestamp: Instant::now(),
                node_id,
                kind: EventKind::Completed { outcome },
                version: VersionId(version_id),
                payload: Some(EventPayload::Duration(start.elapsed())),
            });
        }
    }
}
