pub mod cache;
pub mod demand;
pub mod deps;
pub mod dirty;
pub mod graph;
pub mod snapshot;

use std::collections::HashMap;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_types::{
    ConstraintChecker, ConstraintInput, DeterminacyState, Diagnostic, ExportFormat,
    GeometryHandleId, GeometryKernel, Satisfaction, SnapshotId, SnapshotProvenance,
    ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{extract_dependency_trace, DependencyTrace, ReverseDependencyIndex};
use crate::snapshot::Snapshot;

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    constraint_checker: Box<dyn ConstraintChecker>,
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
    cache: CacheStore,
    /// Overridden param values (set by set_param_and_invalidate).
    param_overrides: std::collections::HashMap<ValueCellId, reify_types::Value>,
    /// Current snapshot from last eval() or edit_param().
    current_snapshot: Option<Snapshot>,
    /// Reverse dependency index for dirty cone computation.
    reverse_index: Option<ReverseDependencyIndex>,
    /// Forward dependency trace map for topological sort.
    trace_map: Option<HashMap<NodeId, DependencyTrace>>,
    /// Demand registry tracking which nodes are demanded.
    demand: DemandRegistry,
    /// Counter for snapshot IDs.
    next_snapshot_id: u64,
    /// Counter for version IDs.
    next_version_id: u64,
    /// The eval set from the last edit_param() or eval() call.
    last_eval_set: Vec<NodeId>,
}

/// Statistics about cache behavior during a cached evaluation.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub early_cutoffs: usize,
}

/// Result of a cached evaluation, wrapping EvalResult with stats.
#[derive(Debug)]
pub struct CachedEvalResult {
    pub eval_result: EvalResult,
    pub stats: CacheStats,
}

/// Result of evaluating a compiled module.
#[derive(Debug)]
pub struct EvalResult {
    pub values: ValueMap,
    pub diagnostics: Vec<Diagnostic>,
}

/// Result of checking constraints.
#[derive(Debug)]
pub struct CheckResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A single constraint's check result.
#[derive(Debug)]
pub struct ConstraintCheckEntry {
    pub id: reify_types::ConstraintNodeId,
    pub label: Option<String>,
    pub satisfaction: Satisfaction,
}

/// Result of a full build (eval + geometry).
#[derive(Debug)]
pub struct BuildResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub geometry_output: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Engine {
    pub fn new(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self {
            constraint_checker,
            geometry_kernel,
            cache: CacheStore::new(),
            param_overrides: std::collections::HashMap::new(),
            current_snapshot: None,
            reverse_index: None,
            trace_map: None,
            demand: DemandRegistry::new(),
            next_snapshot_id: 0,
            next_version_id: 0,
            last_eval_set: Vec::new(),
        }
    }

    /// Access the cache store (for testing/inspection).
    pub fn cache_store(&self) -> &CacheStore {
        &self.cache
    }

    /// Access the current snapshot (for testing/inspection).
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.current_snapshot.as_ref()
    }

    /// Access the eval set from the last eval() or edit_param() call.
    pub fn last_eval_set(&self) -> &[NodeId] {
        &self.last_eval_set
    }

    /// Set a parameter override and invalidate cache entries that depend on it.
    pub fn set_param_and_invalidate(
        &mut self,
        param: &ValueCellId,
        value: reify_types::Value,
    ) {
        self.param_overrides.insert(param.clone(), value);
        // Mark the param's own cache entry as dirty
        let param_node = NodeId::Value(param.clone());
        self.cache.invalidate(&param_node);
        // Mark all nodes that depend on this param as dirty
        self.cache.invalidate_dependents(std::slice::from_ref(param));
    }

    /// Evaluate a compiled module, returning computed values.
    ///
    /// This is a cold-start evaluation that builds a new Snapshot and
    /// dependency structures. Subsequent calls to edit_param() can perform
    /// incremental re-evaluation using these structures.
    pub fn eval(
        &mut self,
        module: &CompiledModule,
    ) -> EvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();

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

        // Set up demand registry: demand all value cells, constraints, and realizations
        let mut demand = DemandRegistry::new();
        for (_, node) in snapshot.graph.value_cells.iter() {
            demand.add_demand(NodeId::Value(node.id.clone()));
        }
        for (_, cnode) in snapshot.graph.constraints.iter() {
            demand.add_demand(NodeId::Constraint(cnode.id.clone()));
        }
        for (_, rnode) in snapshot.graph.realizations.iter() {
            demand.add_demand(NodeId::Realization(rnode.id.clone()));
        }
        demand.rebuild_cone(&snapshot.graph);

        // Two-pass evaluation (same logic as before)
        for template in &module.templates {
            // First pass: evaluate Param defaults to populate the value map
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Param
                    && let Some(ref expr) = cell.default_expr
                {
                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot.values.insert(
                        cell.id.clone(),
                        (val.clone(), DeterminacyState::Determined),
                    );

                    // Record in cache
                    let node_id = NodeId::Value(cell.id.clone());
                    let trace = DependencyTrace::default();
                    let cached_result =
                        CachedResult::Value(val, DeterminacyState::Determined);
                    self.cache.record_evaluation(
                        node_id,
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );
                }
            }

            // Second pass: evaluate Let bindings (which may reference params)
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Let
                    && let Some(ref expr) = cell.default_expr
                {
                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot.values.insert(
                        cell.id.clone(),
                        (val.clone(), DeterminacyState::Determined),
                    );

                    // Record in cache with dependency trace
                    let node_id = NodeId::Value(cell.id.clone());
                    let trace = extract_dependency_trace(expr);
                    let cached_result =
                        CachedResult::Value(val, DeterminacyState::Determined);
                    self.cache.record_evaluation(
                        node_id,
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );
                }
            }
        }

        // Store internal state for incremental evaluation
        self.current_snapshot = Some(snapshot);
        self.reverse_index = Some(reverse_index);
        self.trace_map = Some(trace_map);
        self.demand = demand;
        self.last_eval_set = Vec::new(); // Cold start: no incremental eval set

        EvalResult { values, diagnostics }
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
    ) -> EvalResult {
        let snapshot = self.current_snapshot.as_ref()
            .expect("edit_param requires a prior call to eval()");
        let reverse_index = self.reverse_index.as_ref()
            .expect("edit_param requires reverse_index from eval()");
        let trace_map = self.trace_map.as_ref()
            .expect("edit_param requires trace_map from eval()");

        // Clone snapshot (O(1) via PersistentMap)
        let parent_id = snapshot.id;
        let mut new_snapshot = snapshot.clone();

        // Update snapshot ID, version, and provenance
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        let mut changed_set = std::collections::HashSet::new();
        changed_set.insert(cell.clone());
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // Update the changed cell's value in snapshot
        new_snapshot.values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Compute dirty cone and eval set
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, trace_map);

        // Build the full ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }
        // Overwrite with the new param value
        values.insert(cell.clone(), new_value);

        // Mark all nodes in the eval set as Pending before re-evaluation.
        // This transitions Final → Pending{last_substantive: hash}.
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
                let val = reify_expr::eval_expr(expr, &values);
                values.insert(vcid.clone(), val.clone());
                new_snapshot.values.insert(
                    vcid.clone(),
                    (val.clone(), DeterminacyState::Determined),
                );

                // Record in cache and check for early cutoff
                let trace = extract_dependency_trace(expr);
                let cached_result =
                    CachedResult::Value(val, DeterminacyState::Determined);
                let outcome = self.cache.record_evaluation(
                    node_id.clone(),
                    cached_result,
                    VersionId(version_id),
                    trace,
                );

                // Early cutoff: if result unchanged, remove downstream
                // dependents from remaining eval set
                if outcome == EvalOutcome::Unchanged
                    && let Some(rev_idx) = &self.reverse_index
                {
                    for dependent in rev_idx.dependents_of(vcid) {
                        skipped.insert(dependent.clone());
                    }
                }
            }
            // Constraint/Realization nodes: tracked in eval set but not evaluated
            // (deferred to check()/build())
        }

        // Store state (actual_eval_set excludes early-cutoff-skipped nodes)
        self.last_eval_set = actual_eval_set;
        self.current_snapshot = Some(new_snapshot);

        EvalResult {
            values,
            diagnostics: Vec::new(),
        }
    }

    /// Evaluate a compiled module with caching and early cutoff.
    ///
    /// On first call (cold start), behaves like eval() but populates the cache.
    /// On subsequent calls with the same version, uses version fast path.
    /// On calls with a new version after invalidation, re-evaluates dirty nodes
    /// and uses early cutoff to avoid propagating unchanged results.
    pub fn eval_cached(
        &mut self,
        module: &CompiledModule,
        version: VersionId,
    ) -> CachedEvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();
        let mut stats = CacheStats::default();

        for template in &module.templates {
            // First pass: evaluate Param defaults (or use overrides)
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Param {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
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
                        self.cache.record_evaluation(
                            node_id,
                            result,
                            version,
                            trace,
                        );
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    // Use override if available, otherwise evaluate default
                    let val = if let Some(override_val) = self.param_overrides.get(&cell.id) {
                        override_val.clone()
                    } else if let Some(ref expr) = cell.default_expr {
                        reify_expr::eval_expr(expr, &values)
                    } else {
                        reify_types::Value::Undef
                    };

                    // Build dependency trace (params have no reads - they are roots)
                    let trace = DependencyTrace::default();

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome =
                        self.cache
                            .record_evaluation(node_id, cached_result, version, trace);
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
                        self.cache.record_evaluation(
                            node_id,
                            result,
                            version,
                            trace,
                        );
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;
                    self.cache.clear_dirty(&node_id);
                    let val = reify_expr::eval_expr(expr, &values);

                    // Build dependency trace from expression refs
                    let trace = extract_dependency_trace(expr);

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome =
                        self.cache
                            .record_evaluation(node_id, cached_result, version, trace);
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
            eval_result: EvalResult { values, diagnostics },
            stats,
        }
    }

    /// Evaluate and check constraints.
    pub fn check(
        &mut self,
        module: &CompiledModule,
    ) -> CheckResult {
        let eval_result = self.eval(module);
        let mut constraint_results = Vec::new();
        let mut diagnostics = eval_result.diagnostics;

        for template in &module.templates {
            if template.constraints.is_empty() {
                continue;
            }

            // Build ConstraintInput batch for this template
            let constraint_pairs: Vec<_> = template
                .constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr))
                .collect();

            let input = ConstraintInput {
                constraints: constraint_pairs,
                values: &eval_result.values,
            };

            let results = self.constraint_checker.check(&input);

            for (result, compiled) in results.into_iter().zip(template.constraints.iter()) {
                diagnostics.extend(result.diagnostics.messages);
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: compiled.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
        }
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> BuildResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut last_handle: Option<GeometryHandleId> = None;

            for template in &module.templates {
                for realization in &template.realizations {
                    for op in &realization.operations {
                        let geom_op =
                            compile_geometry_op(op, &check_result.values, &last_handle);
                        match geom_op {
                            Some(geom_op) => match kernel.execute(&geom_op) {
                                Ok(handle) => {
                                    last_handle = Some(handle.id);
                                }
                                Err(e) => {
                                    diagnostics.push(Diagnostic::error(
                                        format!("geometry error: {}", e),
                                    ));
                                }
                            },
                            None => {
                                diagnostics.push(Diagnostic::error(
                                    "failed to compile geometry operation",
                                ));
                            }
                        }
                    }
                }
            }

            // Export the result
            let export_handle = last_handle.unwrap_or(GeometryHandleId(0));
            let mut output = Vec::new();
            match kernel.export(export_handle, format, &mut output) {
                Ok(()) => Some(output),
                Err(e) => {
                    diagnostics.push(Diagnostic::error(
                        format!("export error: {}", e),
                    ));
                    None
                }
            }
        } else {
            None
        };

        BuildResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            geometry_output,
            diagnostics,
        }
    }
}

/// Compile a CompiledGeometryOp into a GeometryOp by evaluating expressions.
fn compile_geometry_op(
    op: &reify_compiler::CompiledGeometryOp,
    values: &ValueMap,
    last_handle: &Option<GeometryHandleId>,
) -> Option<reify_types::GeometryOp> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

    match op {
        CompiledGeometryOp::Primitive { kind, args } => {
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| reify_expr::eval_expr(expr, values))
                    .unwrap_or(reify_types::Value::Undef)
            };

            match kind {
                PrimitiveKind::Box => Some(reify_types::GeometryOp::Box {
                    width: eval_arg("width"),
                    height: eval_arg("height"),
                    depth: eval_arg("depth"),
                }),
                PrimitiveKind::Cylinder => Some(reify_types::GeometryOp::Cylinder {
                    radius: eval_arg("radius"),
                    height: eval_arg("height"),
                }),
                PrimitiveKind::Sphere => Some(reify_types::GeometryOp::Sphere {
                    radius: eval_arg("radius"),
                }),
            }
        }
        CompiledGeometryOp::Boolean { op, left, right } => {
            let resolve_ref = |r: &GeomRef| -> Option<GeometryHandleId> {
                match r {
                    GeomRef::Step(_idx) => *last_handle,
                    GeomRef::Sub(_name) => *last_handle,
                }
            };
            let left_id = resolve_ref(left)?;
            let right_id = resolve_ref(right)?;
            match op {
                BooleanOp::Union => Some(reify_types::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Some(reify_types::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Some(reify_types::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(_) | GeomRef::Sub(_) => (*last_handle)?,
            };
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| reify_expr::eval_expr(expr, values))
                    .unwrap_or(reify_types::Value::Undef)
            };
            match kind {
                reify_compiler::ModifyKind::Fillet => Some(reify_types::GeometryOp::Fillet {
                    target: target_id,
                    radius: eval_arg("radius"),
                }),
                reify_compiler::ModifyKind::Chamfer => Some(reify_types::GeometryOp::Chamfer {
                    target: target_id,
                    distance: eval_arg("distance"),
                }),
            }
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(_) | GeomRef::Sub(_) => (*last_handle)?,
            };
            let eval_arg_f64 = |name: &str| -> f64 {
                args.iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, expr)| reify_expr::eval_expr(expr, values).as_f64())
                    .unwrap_or(0.0)
            };
            match kind {
                reify_compiler::TransformKind::Translate => {
                    Some(reify_types::GeometryOp::Translate {
                        target: target_id,
                        dx: eval_arg_f64("dx"),
                        dy: eval_arg_f64("dy"),
                        dz: eval_arg_f64("dz"),
                    })
                }
                reify_compiler::TransformKind::Rotate => {
                    Some(reify_types::GeometryOp::Rotate {
                        target: target_id,
                        axis: [
                            eval_arg_f64("axis_x"),
                            eval_arg_f64("axis_y"),
                            eval_arg_f64("axis_z"),
                        ],
                        angle_rad: eval_arg_f64("angle"),
                    })
                }
            }
        }
    }
}

