pub mod cache;
pub mod deps;
pub mod graph;
pub mod snapshot;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_types::{
    ConstraintChecker, ConstraintInput, DeterminacyState, Diagnostic, ExportFormat,
    GeometryHandleId, GeometryKernel, Satisfaction, ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::deps::DependencyTrace;

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    constraint_checker: Box<dyn ConstraintChecker>,
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
    cache: CacheStore,
    /// Overridden param values (set by set_param_and_invalidate).
    param_overrides: std::collections::HashMap<ValueCellId, reify_types::Value>,
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
        }
    }

    /// Access the cache store (for testing/inspection).
    pub fn cache_store(&self) -> &CacheStore {
        &self.cache
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
    pub fn eval(
        &mut self,
        module: &CompiledModule,
    ) -> EvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();

        for template in &module.templates {
            // First pass: evaluate Param defaults to populate the value map
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Param
                    && let Some(ref expr) = cell.default_expr
                {
                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val);
                }
            }

            // Second pass: evaluate Let bindings (which may reference params)
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Let
                    && let Some(ref expr) = cell.default_expr
                {
                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val);
                }
            }
        }

        EvalResult { values, diagnostics }
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

/// Extract a dependency trace from a compiled expression by collecting all ValueRef ids.
fn extract_dependency_trace(expr: &reify_types::CompiledExpr) -> DependencyTrace {
    let mut reads = Vec::new();
    collect_value_refs(expr, &mut reads);
    DependencyTrace { reads }
}

fn collect_value_refs(expr: &reify_types::CompiledExpr, out: &mut Vec<ValueCellId>) {
    use reify_types::CompiledExprKind;
    match &expr.kind {
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ValueRef(id) => {
            out.push(id.clone());
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_value_refs(left, out);
            collect_value_refs(right, out);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_value_refs(operand, out);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_value_refs(arg, out);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_value_refs(condition, out);
            collect_value_refs(then_branch, out);
            collect_value_refs(else_branch, out);
        }
    }
}
