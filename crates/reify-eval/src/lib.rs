pub mod cache;
mod concurrent;
pub use concurrent::{ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
pub mod demand;
pub mod deps;
pub mod dirty;
mod geometry_ops;
use geometry_ops::compile_geometry_op;
pub mod graph;
pub mod journal;
pub mod snapshot;
pub mod test_runner;
mod unfold;
use unfold::{elaborate_child_instance, unfold_recursive_sub};
mod engine_admin;
mod engine_purposes;
mod engine_constraints;
mod engine_eval;
mod engine_edit;
mod engine_build;
pub use test_runner::{TestResult, TestStatus, run_tests};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use reify_compiler::{
    CompiledConstraint, CompiledModule, CompiledPurpose, TopologyTemplate, ValueCellKind,
};
use reify_types::{
    AutoParam, CompiledExpr, CompiledFunction, ConstraintChecker, ConstraintInput,
    ConstraintNodeId, ConstraintResult, ConstraintSolver, ContentHash, DeterminacyState,
    Diagnostic, ExportFormat, FIELD_ENTITY_PREFIX, GeometryHandleId, GeometryKernel, Mesh,
    OptimizationObjective, OptimizedImpl, OptimizedImplInput, PersistentMap, ResolutionProblem,
    Satisfaction, SnapshotId, SnapshotProvenance, SolveResult, Value, ValueCellId, ValueMap,
    VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex, extract_dependency_trace};
use crate::dirty::topological_sort;
use crate::graph::GuardedGroupInfo;
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};
use crate::snapshot::Snapshot;

/// Error returned when an operation requires prior eval() but none has been performed.
#[derive(Debug)]
pub enum EngineError {
    /// The engine has not been initialized — call eval() first.
    NotInitialized,
    /// The specified ValueCellId does not exist in the evaluation graph.
    CellNotFound { cell: reify_types::ValueCellId },
    /// The supplied value's dimension does not match the cell's declared type.
    DimensionMismatch {
        cell: reify_types::ValueCellId,
        expected: reify_types::DimensionVector,
        got: reify_types::DimensionVector,
    },
    /// The supplied value's type variant does not match the cell's declared type kind.
    /// (e.g., passing Value::Bool to a Type::Scalar cell.)
    TypeKindMismatch {
        cell: reify_types::ValueCellId,
        expected: Box<reify_types::Type>,
        got: Box<reify_types::Value>,
    },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotInitialized => {
                write!(
                    f,
                    "engine not initialized: call eval() before this operation"
                )
            }
            EngineError::CellNotFound { cell } => {
                write!(
                    f,
                    "value cell not found in evaluation graph: {cell}"
                )
            }
            EngineError::DimensionMismatch { cell, expected, got } => {
                write!(
                    f,
                    "dimension mismatch for {cell}: expected {:?}, got {:?}",
                    expected, got
                )
            }
            EngineError::TypeKindMismatch { cell, expected, got } => {
                write!(
                    f,
                    "type-kind mismatch for {cell}: expected {expected}, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Returns `true` when the outer variant of `value` is compatible with `ty`.
///
/// This is a shallow kind-level check — it does NOT validate dimension, inner
/// element types, or structural fields.  `Value::Undef` is accepted for any
/// type because it is the universal "no value / Auto" sentinel.
///
/// If `ty` is `Type::Error` (the type-inference poison sentinel), this returns
/// `true` unconditionally to avoid a second diagnostic on top of the root-cause
/// compile error.  The compiler already emitted the defect at the point it
/// produced `Type::Error`; rejecting here with `EngineError::TypeKindMismatch`
/// would be a cascade.  Mirrors the guards in
/// `reify_compiler::type_compat::{implicitly_converts_to, type_compatible}`
/// (task-448 / task-1922).
fn value_type_kind_matches(value: &reify_types::Value, ty: &reify_types::Type) -> bool {
    use reify_types::{Type, Value};
    // Anti-cascade guard — see function doc.
    if ty.is_error() {
        return true;
    }
    match value {
        // Undef is the Auto/no-value sentinel — always accepted.
        Value::Undef => true,
        // Exact outer-variant correspondences.
        Value::Bool(_) => matches!(ty, Type::Bool),
        // Allow numeric coercion: Real values may be supplied to Int cells and
        // vice versa.  The engine handles these mismatches by emitting a Warning
        // diagnostic rather than a hard error, so the kind check must not reject
        // them.  This is intentional and mirrors the pre-existing collection
        // count behaviour (see edit_param_non_int_*_count_emits_warning tests).
        Value::Int(_) => matches!(ty, Type::Int | Type::Real),
        Value::Real(_) => matches!(ty, Type::Real | Type::Int),
        Value::String(_) => matches!(ty, Type::String),
        Value::Scalar { .. } => matches!(ty, Type::Scalar { .. }),
        Value::Enum { .. } => matches!(ty, Type::Enum(_)),
        Value::List(_) => matches!(ty, Type::List(_)),
        Value::Set(_) => matches!(ty, Type::Set(_)),
        Value::Map(_) => matches!(ty, Type::Map(_, _)),
        Value::Option(_) => matches!(ty, Type::Option(_)),
        Value::Field { .. } => matches!(ty, Type::Field { .. }),
        Value::Lambda { .. } => matches!(ty, Type::Function { .. }),
        Value::Tensor(_) => matches!(ty, Type::Tensor { .. } | Type::Matrix { .. }),
        Value::Matrix(_) => matches!(ty, Type::Tensor { .. } | Type::Matrix { .. }),
        Value::Point(_) => matches!(ty, Type::Point { .. }),
        Value::Vector(_) => matches!(ty, Type::Vector { .. }),
        Value::Complex { .. } => matches!(ty, Type::Complex(_)),
        Value::Orientation { .. } => matches!(ty, Type::Orientation(_)),
        Value::Frame { .. } => matches!(ty, Type::Frame(_)),
        Value::Transform { .. } => matches!(ty, Type::Transform(_)),
        Value::Plane { .. } => matches!(ty, Type::Plane),
        Value::Axis { .. } => matches!(ty, Type::Axis),
        Value::BoundingBox { .. } => matches!(ty, Type::BoundingBox),
        Value::Range { .. } => matches!(ty, Type::Range(_)),
        // Note: `Type::Geometry`, `Type::StructureRef`, and `Type::TypeParam` have
        // no corresponding `Value` variant today, so any non-Undef value supplied
        // to a cell of those types will return `false` here and trigger
        // `TypeKindMismatch`.  That is the desired behaviour — only `Value::Undef`
        // (the Auto sentinel, handled unconditionally above) is accepted.
        // If a future `Value::GeometryHandle` variant is added, add a matching arm
        // here so the compiler enforces completeness.
    }
}

/// Consolidated evaluation state produced by eval().
///
/// Groups the snapshot, reverse dependency index, and trace map that are
/// always set/unset atomically. This replaces three separate Option fields
/// in Engine, enforcing the invariant that all three are present together.
#[derive(Debug)]
pub struct EvaluationState {
    /// Current snapshot from last eval() or edit_param().
    pub snapshot: Snapshot,
    /// Reverse dependency index for dirty cone computation.
    pub reverse_index: ReverseDependencyIndex,
    /// Forward dependency trace map for topological sort.
    pub trace_map: HashMap<NodeId, DependencyTrace>,
}

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    constraint_checker: Box<dyn ConstraintChecker>,
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
    solver: Option<Box<dyn ConstraintSolver>>,
    cache: CacheStore,
    /// Compiled stdlib prelude modules (cached via OnceLock; zero-cost borrow).
    prelude: &'static [CompiledModule],
    /// Pre-flattened cache of all functions from every prelude module, computed
    /// once at Engine construction time. Avoids iterating over the nested
    /// `prelude: &'static [CompiledModule]` structure on every `eval()` call;
    /// the per-eval clone cost (one `CompiledFunction` clone per entry) is
    /// unchanged — only the outer module-level iteration is eliminated.
    ///
    /// Note: this duplicates data already held in the static `prelude` slice,
    /// adding per-Engine memory proportional to the number of prelude functions.
    prelude_functions: Vec<CompiledFunction>,
    /// Overridden param values (set by set_param_and_invalidate).
    param_overrides: std::collections::HashMap<ValueCellId, reify_types::Value>,
    /// Consolidated evaluation state from last eval() or edit_param().
    /// None before the first eval() call; always Some after.
    eval_state: Option<EvaluationState>,
    /// Demand registry tracking which nodes are demanded.
    demand: DemandRegistry,
    /// Counter for snapshot IDs.
    next_snapshot_id: u64,
    /// Counter for version IDs.
    next_version_id: u64,
    /// The eval set from the last edit_param() or eval() call.
    last_eval_set: Vec<NodeId>,
    /// Event journal recording evaluation events.
    journal: EventJournal,
    /// User-defined functions from the last eval() call.
    /// Stored so that edit_param() and other incremental paths can evaluate
    /// expressions containing UserFunctionCall nodes.
    functions: Vec<CompiledFunction>,
    /// Compiled purpose declarations from the last eval() call.
    /// Stored so activate_purpose/deactivate_purpose can look up purposes by name.
    compiled_purposes: Vec<CompiledPurpose>,
    /// Currently active purposes: maps purpose name → injected constraint IDs.
    /// Used by deactivate_purpose to remove the injected constraints.
    active_purposes: HashMap<String, Vec<ConstraintNodeId>>,
    /// Active optimization objectives injected by purposes.
    /// Maps purpose name → optimization objective.
    active_objective_map: HashMap<String, OptimizationObjective>,
    /// Template meta entries from the last eval() call.
    /// Maps template name → meta key/value pairs from the template's meta block.
    /// Populated during eval() so that edit_param() and other incremental paths
    /// can resolve MetaAccess expressions without re-reading the module.
    meta_map: HashMap<String, HashMap<String, String>>,
    /// Template-native optimization objectives from the last eval() call.
    /// Maps template name → optimization objective declared in the template.
    /// Populated during eval() so that edit_param() can look up the objective
    /// by scope_name without needing access to the original templates.
    objectives: HashMap<String, OptimizationObjective>,
    /// Maximum depth for recursive sub-component unfolding.
    /// Prevents runaway recursion when guard expressions don't terminate.
    /// Default: 64.
    max_unfold_depth: usize,
    /// Maximum total nodes created during recursive sub-component unfolding.
    /// Prevents exponential blowup when a template has multiple recursive subs
    /// (e.g., binary tree with `left` and `right` produces B^D nodes).
    /// Default: 10_000.
    max_unfold_nodes: usize,
    /// Registry of optimized constraint implementations, keyed by the target
    /// name declared on a constraint def's `@optimized("target")` annotation.
    /// Populated via `register_optimized_impl`. At check time, any constraint
    /// whose `optimized_target` matches a registered key is routed to that
    /// impl instead of the language-level `constraint_checker` (Task 273).
    optimization_registry: HashMap<String, Box<dyn OptimizedImpl>>,
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
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// Result of checking constraints.
#[derive(Debug)]
pub struct CheckResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub diagnostics: Vec<Diagnostic>,
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// A single constraint's check result.
#[derive(Debug, Clone)]
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
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// Result of tessellating all realizations in a module for GUI mesh rendering.
///
/// Similar to [`BuildResult`] but produces per-realization meshes instead of
/// a single exported geometry file. Each mesh is paired with its entity path
/// (e.g., `"Bracket#realization[0]"`).
#[derive(Debug)]
pub struct TessellateResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    /// Per-realization tessellated meshes: `(entity_path, mesh)`.
    pub meshes: Vec<(String, Mesh)>,
    pub diagnostics: Vec<Diagnostic>,
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

// Concurrent edit structs and Engine methods live in concurrent.rs.

/// Controls how `guard_state_fingerprint` handles guard cells absent from the value map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuardLookup {
    /// Missing cells are treated as `Value::Undef` (safe during initial evaluation).
    Lenient,
    /// Missing cells cause a panic (required after `eval()` has fully populated all cells).
    Strict,
}

/// Compute a content hash over the current guard-cell values in `groups`.
///
/// Each guard cell is hashed as `"guard:{cell}={value:?}"` to ensure that two
/// different cells holding the same value still produce distinct hashes.
///
/// `GuardLookup::Lenient`: missing cells are treated as `Value::Undef`.
/// Use this during or immediately after initial evaluation, where a guard cell
/// might not yet have a value (semantically undetermined).
///
/// `GuardLookup::Strict`: panics if any guard cell is absent from `values`.
/// Use this after `eval()` has completed, where every guard cell must be
/// populated; a missing cell would silently produce a wrong fingerprint and
/// corrupt the incremental cache.
fn guard_state_fingerprint(
    groups: &[GuardedGroupInfo],
    values: &ValueMap,
    mode: GuardLookup,
) -> ContentHash {
    let hashes = groups.iter().map(|g| {
        let val = match mode {
            GuardLookup::Strict => values
                .get(&g.guard_cell)
                .cloned()
                .expect("guard cell must have a value after initial evaluation"),
            GuardLookup::Lenient => values.get_or_undef(&g.guard_cell),
        };
        ContentHash::of_str(&format!("guard:{}={:?}", g.guard_cell, val))
    });
    ContentHash::combine_all(hashes)
}

impl Engine {
    /// Dispatch a batch of constraints to either their registered optimized
    /// implementation or the language-level `ConstraintChecker`, preserving
    /// the order of `entries` in the returned results (Task 273).
    ///
    /// Each entry is `(id, expr, optimized_target)`. Constraints whose
    /// `optimized_target` is `Some(t)` AND `t` is in `optimization_registry`
    /// are sent to that impl; everything else falls through to
    /// `self.constraint_checker`.
    ///
    /// Dispatch across registered targets happens in deterministic order
    /// (targets are iterated via a `BTreeMap`) so that any side effects —
    /// logging, metrics, impls that share mutable state — are reproducible
    /// from run to run.
    fn dispatch_constraints<'a>(
        &self,
        entries: Vec<(ConstraintNodeId, &'a CompiledExpr, Option<&'a str>)>,
        values: &'a ValueMap,
        functions: &'a [CompiledFunction],
        determinacy: Option<&'a PersistentMap<ValueCellId, (Value, DeterminacyState)>>,
    ) -> (Vec<ConstraintResult>, Vec<Diagnostic>) {
        if entries.is_empty() {
            return (Vec::new(), Vec::new());
        }

        // Fast path: when no optimized impls are registered every entry goes to
        // the language-level fallback. Skip the BTreeMap/Option-Vec/unzip
        // allocations and go directly to the checker — same code path as before
        // Task 273 introduced the bucketing logic.
        if self.optimization_registry.is_empty() {
            let constraints = entries
                .into_iter()
                .map(|(id, expr, _target)| (id, expr))
                .collect();
            let input = ConstraintInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            return (self.constraint_checker.check(&input), Vec::new());
        }

        // Results in input order. We fill slots as each path completes.
        let mut results: Vec<Option<ConstraintResult>> =
            (0..entries.len()).map(|_| None).collect();

        // Diagnostics emitted by this function (contract violations only —
        // per-constraint diagnostics remain inside ConstraintResult).
        let mut dispatch_diagnostics: Vec<Diagnostic> = Vec::new();

        // Bucket entries by registered target. Keys borrow from the entry's
        // `Option<&'a str>` — no allocation. A `BTreeMap` gives deterministic
        // dispatch order across targets. `None` targets and targets with no
        // registered impl go to the language-level fallback bucket.
        //
        // We move `(ConstraintNodeId, &CompiledExpr)` directly into the
        // buckets so the dispatch path never clones a `ConstraintNodeId`.
        //
        // Each bucket entry keeps the *original index* alongside the payload
        // so the merge step below can weave results back into the caller-
        // visible order regardless of which group they were dispatched to.
        type BucketEntry<'b> = (usize, (ConstraintNodeId, &'b CompiledExpr));
        let mut optimized_groups: BTreeMap<&'a str, Vec<BucketEntry<'a>>> = BTreeMap::new();
        let mut fallback: Vec<BucketEntry<'a>> = Vec::new();
        for (i, (id, expr, target)) in entries.into_iter().enumerate() {
            match target {
                Some(t) if self.optimization_registry.contains_key(t) => {
                    optimized_groups.entry(t).or_default().push((i, (id, expr)));
                }
                _ => fallback.push((i, (id, expr))),
            }
        }

        // Dispatch each optimized group through its registered impl. The
        // contract is that the impl returns one `ConstraintResult` per input
        // constraint, in the same order. We weave results back into the
        // original result vector via each entry's recorded original index.
        //
        // On a count mismatch (third-party impl bug): emit a Diagnostic::error
        // and fall back to the language-level ConstraintChecker for this batch.
        // The entire batch is discarded — partial results cannot be reliably
        // correlated when we don't know which constraints they correspond to.
        for (target, bucket) in optimized_groups {
            let imp = self
                .optimization_registry
                .get(target)
                .expect("target was just bucketed from optimization_registry");
            let (indices, constraints): (Vec<usize>, Vec<(ConstraintNodeId, &'a CompiledExpr)>) =
                bucket.into_iter().unzip();
            let input = OptimizedImplInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            let output = imp.check(&input);
            if output.results.len() != indices.len() {
                // Contract violation: the impl returned the wrong number of
                // results. Emit an error diagnostic and fall back to the
                // language-level checker for this entire batch.
                dispatch_diagnostics.push(Diagnostic::error(format!(
                    "OptimizedImpl for target {:?} returned {} results for {} constraints \
                     — falling back to language-level checker for this batch",
                    target,
                    output.results.len(),
                    indices.len(),
                )));
                let fallback_input = ConstraintInput {
                    constraints: input.constraints,
                    values,
                    functions,
                    determinacy,
                };
                let fallback_results = self.constraint_checker.check(&fallback_input);
                assert_eq!(
                    fallback_results.len(),
                    indices.len(),
                    "ConstraintChecker returned {} results for {} constraints during \
                     OptimizedImpl fallback",
                    fallback_results.len(),
                    indices.len(),
                );
                for (orig_idx, result) in indices.into_iter().zip(fallback_results) {
                    results[orig_idx] = Some(result);
                }
            } else {
                for (orig_idx, result) in indices.into_iter().zip(output.results) {
                    results[orig_idx] = Some(result);
                }
            }
        }

        // Dispatch the remainder through the language-level checker — same
        // input shape the callers used before Task 273.
        if !fallback.is_empty() {
            let (indices, constraints): (Vec<usize>, Vec<(ConstraintNodeId, &'a CompiledExpr)>) =
                fallback.into_iter().unzip();
            let input = ConstraintInput {
                constraints,
                values,
                functions,
                determinacy,
            };
            let fallback_results = self.constraint_checker.check(&input);
            assert_eq!(
                fallback_results.len(),
                indices.len(),
                "ConstraintChecker returned {} results for {} constraints",
                fallback_results.len(),
                indices.len(),
            );
            for (orig_idx, result) in indices.into_iter().zip(fallback_results) {
                results[orig_idx] = Some(result);
            }
        }

        let constraint_results = results
            .into_iter()
            .map(|r| r.expect("dispatch_constraints: every slot must be filled"))
            .collect();
        (constraint_results, dispatch_diagnostics)
    }

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

    // prepare_concurrent_edit, rollback_concurrent_edit, apply_concurrent_edit,
    // resolve_concurrent_edit methods moved to concurrent.rs

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
        self.functions.extend(self.prelude_functions.iter().cloned());
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

        // Evaluate field declarations first: they must be available in the
        // values map before templates are evaluated, because structure
        // expressions may reference fields (e.g., `sample(my_field, point)`).
        for field in &module.fields {
            let lambda_value = match &field.source {
                reify_compiler::CompiledFieldSource::Analytical { expr } => {
                    let ctx =
                        reify_expr::EvalContext::new(&values, &functions).with_meta(&self.meta_map);
                    let val = reify_expr::eval_expr(expr, &ctx);
                    Box::new(val)
                }
                reify_compiler::CompiledFieldSource::Composed { expr } => {
                    let ctx =
                        reify_expr::EvalContext::new(&values, &functions).with_meta(&self.meta_map);
                    let val = reify_expr::eval_expr(expr, &ctx);
                    Box::new(val)
                }
                reify_compiler::CompiledFieldSource::Sampled { .. }
                | reify_compiler::CompiledFieldSource::Imported => Box::new(Value::Undef),
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
                let child_template = match module
                    .templates
                    .iter()
                    .find(|t| t.name == sub.structure_name)
                {
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
            let guard_state_hash =
                guard_state_fingerprint(&snapshot.graph.guarded_groups, &values, GuardLookup::Lenient);
            snapshot.topology_fingerprint =
                snapshot.graph.topology_fingerprint().combine(guard_state_hash);
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
        if let reify_types::Type::Scalar { dimension: expected } = cell_node.cell_type
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
                            // Skip Auto params — their lifecycle is managed by the
                            // solver, not guard activation/deactivation.
                            let is_auto = graph
                                .value_cells
                                .get(mid)
                                .is_some_and(|n| n.kind.is_auto());
                            if !is_auto {
                                values.insert(mid.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    mid.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
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
                            // Skip Auto params — their lifecycle is managed by the
                            // solver, not guard activation/deactivation.
                            let is_auto = graph
                                .value_cells
                                .get(mid)
                                .is_some_and(|n| n.kind.is_auto());
                            if !is_auto {
                                values.insert(mid.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    mid.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
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
                            // Deactivate: set to Undef — but skip Auto params whose
                            // lifecycle is managed by the solver, not guard activation.
                            let is_auto = new_snapshot
                                .graph
                                .value_cells
                                .get(member_id)
                                .is_some_and(|n| n.kind.is_auto());
                            if !is_auto {
                                values.insert(member_id.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    member_id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
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
                            // Deactivate: set to Undef — but skip Auto params whose
                            // lifecycle is managed by the solver, not guard activation.
                            let is_auto = new_snapshot
                                .graph
                                .value_cells
                                .get(member_id)
                                .is_some_and(|n| n.kind.is_auto());
                            if !is_auto {
                                values.insert(member_id.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    member_id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        }
                    }
                }

                // Recompute topology fingerprint to include guard states.
                let guard_state_hash = guard_state_fingerprint(
                    &new_snapshot.graph.guarded_groups,
                    &values,
                    GuardLookup::Strict,
                );
                new_snapshot.topology_fingerprint =
                    new_snapshot.graph.topology_fingerprint().combine(guard_state_hash);
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

    /// Replace occurrences of the raw ConstraintNodeId string in diagnostic
    /// messages with a human-readable label, when a label is present.
    ///
    /// This enriches engine-level diagnostics for constraint def instantiations
    /// so that messages read "constraint MinWall[0] violated" instead of
    /// "constraint S#constraint[0] violated". When `label` is `None` (inline
    /// constraints without a label), the messages are returned unchanged.
    fn labeled_diagnostics(
        messages: Vec<Diagnostic>,
        id: &reify_types::ConstraintNodeId,
        label: Option<&str>,
    ) -> Vec<Diagnostic> {
        let Some(lbl) = label else {
            return messages;
        };
        let id_str = id.to_string();
        messages
            .into_iter()
            .map(|mut d| {
                d.message = d.message.replace(&id_str, lbl);
                d
            })
            .collect()
    }

    /// Incrementally re-evaluate and check constraints after changing a parameter.
    ///
    /// Combines edit_param() (incremental value evaluation + re-resolution)
    /// with constraint satisfaction checking against the updated values.
    /// Check all constraints against the given values.
    ///
    /// Returns constraint check entries and any diagnostics produced by
    /// violated constraints. Uses the current snapshot's constraint graph.
    ///
    /// This is the shared constraint-checking logic used by both `edit_check`
    /// (sequential path) and `edit_check_concurrent` (concurrent path).
    pub fn check_constraints_with_values(
        &self,
        values: &ValueMap,
    ) -> Result<(Vec<ConstraintCheckEntry>, Vec<Diagnostic>), EngineError> {
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        let active_ids = state.snapshot.graph.active_constraint_ids(values);
        let constraint_nodes: Vec<_> = state
            .snapshot
            .graph
            .constraints
            .iter()
            .map(|(_, cnode)| cnode)
            .filter(|cnode| active_ids.contains(&cnode.id))
            .collect();

        if !constraint_nodes.is_empty() {
            let entries: Vec<_> = constraint_nodes
                .iter()
                .map(|cnode| {
                    (
                        cnode.id.clone(),
                        &cnode.expr,
                        cnode.optimized_target.as_deref(),
                    )
                })
                .collect();

            let (results, dispatch_diags) = self.dispatch_constraints(
                entries,
                values,
                &self.functions,
                Some(&state.snapshot.values),
            );
            diagnostics.extend(dispatch_diags);
            for (result, cnode) in results.into_iter().zip(constraint_nodes.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    cnode.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: cnode.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        Ok((constraint_results, diagnostics))
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

    /// Check constraints using the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (i.e. eval() hasn't been called yet).
    /// Otherwise builds a ValueMap from the snapshot, runs constraint checking,
    /// and returns constraint results. This is the incremental companion to check():
    /// after edit_param() updates values, call check_snapshot() to see constraint
    /// status without destroying the incremental state.
    pub fn check_snapshot(&self, module: &CompiledModule) -> Option<CheckResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if active_constraints.is_empty() {
                continue;
            }

            let entries: Vec<_> = active_constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr, c.optimized_target.as_deref()))
                .collect();

            let (results, dispatch_diags) = self.dispatch_constraints(
                entries,
                &values,
                &self.functions,
                Some(&state.snapshot.values),
            );
            diagnostics.extend(dispatch_diags);

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    compiled.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: compiled.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        Some(CheckResult {
            values,
            constraint_results,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Collect active constraints from a template given current values.
    ///
    /// Returns top-level constraints unconditionally, plus guarded constraints
    /// whose guard is currently active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither branch).
    fn collect_active_constraints<'a>(
        template: &'a TopologyTemplate,
        values: &ValueMap,
    ) -> Vec<&'a CompiledConstraint> {
        let mut active: Vec<&'a CompiledConstraint> = Vec::new();

        // Top-level (unguarded) constraints are always active
        for c in &template.constraints {
            active.push(c);
        }

        // Guard-gated constraints
        for group in &template.guarded_groups {
            let guard_val = values.get(&group.guard_value_cell);
            match guard_val {
                Some(Value::Bool(true)) => {
                    for c in &group.constraints {
                        active.push(c);
                    }
                }
                Some(Value::Bool(false)) => {
                    for c in &group.else_constraints {
                        active.push(c);
                    }
                }
                _ => {
                    // Undef or non-Bool: neither branch active
                }
            }
        }

        active
    }

    /// Evaluate and check constraints (guard-aware).
    ///
    /// Checks top-level (unguarded) constraints unconditionally, plus
    /// guarded constraints whose guard is active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither).
    pub fn check(&mut self, module: &CompiledModule) -> CheckResult {
        let eval_result = self.eval(module);
        let mut constraint_results = Vec::new();
        let mut diagnostics = eval_result.diagnostics;

        for template in &module.templates {
            // Collect active constraints: top-level + guard-aware guarded
            let active_constraints =
                Self::collect_active_constraints(template, &eval_result.values);

            if active_constraints.is_empty() {
                continue;
            }

            let entries: Vec<_> = active_constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr, c.optimized_target.as_deref()))
                .collect();

            // After eval(), eval_state is always Some — unwrap is safe here.
            let det_values = &self.eval_state.as_ref().unwrap().snapshot.values;
            let (results, dispatch_diags) = self.dispatch_constraints(
                entries,
                &eval_result.values,
                &self.functions,
                Some(det_values),
            );
            diagnostics.extend(dispatch_diags);

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    compiled.label.as_deref(),
                ));
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
            resolved_params: eval_result.resolved_params,
        }
    }

    /// Build geometry from the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists. Otherwise: checks constraints from
    /// snapshot (same as check_snapshot), then executes geometry operations from
    /// module realizations using the geometry kernel. This is the incremental
    /// companion to build(): after edit_param() updates values, call
    /// build_snapshot() to get updated geometry without a cold restart.
    pub fn build_snapshot(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> Option<BuildResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if !active_constraints.is_empty() {
                let entries: Vec<_> = active_constraints
                    .iter()
                    .map(|c| (c.id.clone(), &c.expr, c.optimized_target.as_deref()))
                    .collect();

                let (results, dispatch_diags) = self.dispatch_constraints(
                    entries,
                    &values,
                    &self.functions,
                    Some(&state.snapshot.values),
                );
                diagnostics.extend(dispatch_diags);

                for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                    diagnostics.extend(Self::labeled_diagnostics(
                        result.diagnostics.messages,
                        &result.id,
                        compiled.label.as_deref(),
                    ));
                    constraint_results.push(ConstraintCheckEntry {
                        id: result.id,
                        label: compiled.label.clone(),
                        satisfaction: result.satisfaction,
                    });
                }
            }
        }

        // Execute geometry operations
        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module.templates.iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                for realization in &template.realizations {
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                    );
                }
            }

            if !had_realization_ops {
                None
            } else if step_handles.is_empty() {
                diagnostics.push(Diagnostic::error(
                    "all geometry operations failed; no geometry output produced",
                ));
                None
            } else {
                let export_handle = *step_handles.last().unwrap();
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                        None
                    }
                }
            }
        } else {
            None
        };

        Some(BuildResult {
            values,
            constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(&mut self, module: &CompiledModule, format: ExportFormat) -> BuildResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module.templates.iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            for template in &module.templates {
                for realization in &template.realizations {
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
                        &realization.operations,
                        &check_result.values,
                        &self.functions,
                        &self.meta_map,
                        &mut step_handles,
                        &mut diagnostics,
                    );
                }
            }

            if step_handles.is_empty() {
                // No geometry handles available — nothing to export.
                // Only emit the summary diagnostic when ops were actually declared
                // but all failed; when no ops were declared there is simply no geometry.
                if had_realization_ops {
                    diagnostics.push(Diagnostic::error(
                        "all geometry operations failed; no geometry output produced",
                    ));
                }
                None
            } else {
                // Safety: step_handles is non-empty (guarded by the is_empty() check above),
                // so last() is always Some and unwrap() cannot panic.
                let export_handle = *step_handles.last().unwrap();
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                        None
                    }
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
            resolved_params: check_result.resolved_params,
        }
    }

    /// Tessellate all realizations in the module for GUI mesh rendering.
    ///
    /// Evaluates the module via [`check()`], then executes geometry operations
    /// per realization (same loop as [`build()`]) and tessellates each
    /// realization's final shape. Returns one `(entity_path, Mesh)` pair per
    /// realization that produced geometry.
    ///
    /// When no geometry kernel is configured, returns empty meshes with no
    /// error diagnostics (matching the pattern in [`build()`]).
    pub fn tessellate_realizations(&mut self, module: &CompiledModule) -> TessellateResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &check_result.values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
        );

        TessellateResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            meshes,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Default tessellation tolerance in SI meters (0.1mm).
    const DEFAULT_TESSELLATION_TOLERANCE: f64 = 0.0001;

    /// Shared helper: execute geometry operations and tessellate each realization.
    ///
    /// Used by both `tessellate_realizations()` and `tessellate_snapshot()`.
    fn tessellate_from_values(
        geometry_kernel: &mut Option<Box<dyn GeometryKernel>>,
        module: &CompiledModule,
        values: &ValueMap,
        functions: &[CompiledFunction],
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) -> Vec<(String, Mesh)> {
        let mut meshes = Vec::new();

        let kernel = match geometry_kernel.as_mut() {
            Some(k) => k,
            None => return meshes,
        };

        let mut step_handles: Vec<GeometryHandleId> = Vec::new();

        for template in &module.templates {
            for realization in &template.realizations {
                let handle_start = step_handles.len();
                Engine::execute_realization_ops(
                    kernel.as_mut(),
                    &realization.operations,
                    values,
                    functions,
                    meta_map,
                    &mut step_handles,
                    diagnostics,
                );

                // Tessellate this realization's final handle (if any new handles were produced)
                if step_handles.len() > handle_start {
                    let last_handle = step_handles[step_handles.len() - 1];
                    match kernel.tessellate(last_handle, Self::DEFAULT_TESSELLATION_TOLERANCE) {
                        Ok(mesh) => {
                            meshes.push((realization.id.to_string(), mesh));
                        }
                        Err(e) => {
                            diagnostics
                                .push(Diagnostic::error(format!("tessellation error: {}", e)));
                        }
                    }
                }
            }
        }

        meshes
    }

    /// Execute the per-realization geometry operation loop and perform rollback
    /// on partial failure.
    ///
    /// Captures `handle_start = step_handles.len()` on entry.  For each op in
    /// `operations`, evaluates it via `compile_geometry_op` and dispatches to
    /// the kernel:
    ///
    /// - `Some(Ok(handle))` — pushes `handle.id` to `step_handles`.
    /// - `Some(Err(e))` — emits a geometry-error diagnostic and breaks the loop.
    ///   Kernel errors break immediately: a geometry engine failure is often
    ///   unrecoverable (e.g. corrupt state), and subsequent ops that depend on
    ///   the failed handle would fail too.
    /// - `None` — pushes `GeometryHandleId::INVALID` sentinel, emits a
    ///   compile-error diagnostic, sets `had_failure = true`, and continues.
    ///   Compile errors are cheaper to continue past because the sentinel lets
    ///   independent ops proceed.
    ///
    /// After the op loop, if `had_failure` or fewer handles were produced than
    /// there are `operations`, truncates `step_handles` to `handle_start` (discards
    /// all partial handles from this realization).
    fn execute_realization_ops(
        kernel: &mut dyn GeometryKernel,
        operations: &[reify_compiler::CompiledGeometryOp],
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        step_handles: &mut Vec<GeometryHandleId>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let handle_start = step_handles.len();
        let mut had_failure = false;
        for op in operations {
            let geom_op = compile_geometry_op(
                op,
                values,
                &step_handles[handle_start..],
                functions,
                meta_map,
                diagnostics,
            );
            match geom_op {
                Some(geom_op) => match kernel.execute(&geom_op) {
                    Ok(handle) => {
                        step_handles.push(handle.id);
                    }
                    Err(e) => {
                        diagnostics
                            .push(Diagnostic::error(format!("geometry error: {}", e)));
                        break;
                    }
                },
                None => {
                    diagnostics.push(Diagnostic::error(
                        "failed to compile geometry operation",
                    ));
                    step_handles.push(GeometryHandleId::INVALID);
                    had_failure = true;
                }
            }
        }
        // Discard intermediate handles from partially-failed realizations
        if had_failure || step_handles.len() - handle_start < operations.len() {
            step_handles.truncate(handle_start);
        }
    }

    /// Tessellate realizations from the current snapshot values, without
    /// re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (no prior `eval()` call).
    /// Otherwise: checks constraints from snapshot, then executes geometry
    /// operations and tessellates each realization. This is the incremental
    /// companion to `tessellate_realizations()`: after `edit_param()` updates
    /// values, call `tessellate_snapshot()` to get updated meshes without a
    /// cold restart.
    pub fn tessellate_snapshot(&mut self, module: &CompiledModule) -> Option<TessellateResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if !active_constraints.is_empty() {
                let entries: Vec<_> = active_constraints
                    .iter()
                    .map(|c| (c.id.clone(), &c.expr, c.optimized_target.as_deref()))
                    .collect();

                let (results, dispatch_diags) = self.dispatch_constraints(
                    entries,
                    &values,
                    &self.functions,
                    Some(&state.snapshot.values),
                );
                diagnostics.extend(dispatch_diags);

                for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                    diagnostics.extend(Self::labeled_diagnostics(
                        result.diagnostics.messages,
                        &result.id,
                        compiled.label.as_deref(),
                    ));
                    constraint_results.push(ConstraintCheckEntry {
                        id: result.id,
                        label: compiled.label.clone(),
                        satisfaction: result.satisfaction,
                    });
                }
            }
        }

        // Execute geometry and tessellate
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
        );

        Some(TessellateResult {
            values,
            constraint_results,
            meshes,
            diagnostics,
            resolved_params: HashMap::new(),
        })
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
            .filter(|c| c.kind == ValueCellKind::Let && c.default_expr.is_some())
            .map(|c| {
                (
                    NodeId::Value(c.id.clone()),
                    c.default_expr.as_ref().unwrap(),
                )
            })
            .collect();

        let let_node_ids: HashSet<NodeId> = let_cells.keys().cloned().collect();
        let let_traces: HashMap<NodeId, DependencyTrace> = let_cells
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
            let cell_id = match &node_id {
                NodeId::Value(vcid) => vcid,
                _ => unreachable!(),
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
                node_id,
                kind: EventKind::Completed { outcome },
                version: VersionId(version_id),
                payload: Some(EventPayload::Duration(start.elapsed())),
            });
        }
    }
}

// geometry_ops functions moved to geometry_ops.rs
// unfold functions (unfold_recursive_sub, elaborate_child_instance,
// elaborate_child_params_only, elaborate_child_lets_only) moved to unfold.rs

#[cfg(test)]
mod tests {
    use super::*;

    // ── guard_state_fingerprint unit tests ────────────────────────────────────

    fn make_guard_group(entity: &str, member: &str) -> GuardedGroupInfo {
        GuardedGroupInfo {
            guard_cell: ValueCellId::new(entity, member),
            members: vec![],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
        }
    }

    #[test]
    fn guard_state_fingerprint_empty_groups_returns_combine_all_empty() {
        let values = ValueMap::new();
        let result = guard_state_fingerprint(&[], &values, GuardLookup::Lenient);
        let expected = ContentHash::combine_all(std::iter::empty());
        assert_eq!(result, expected);
    }

    #[test]
    fn guard_state_fingerprint_single_group_present_value() {
        let cell = ValueCellId::new("E", "g");
        let val = Value::Bool(true);
        let mut values = ValueMap::new();
        values.insert(cell.clone(), val.clone());
        let groups = vec![make_guard_group("E", "g")];
        let result = guard_state_fingerprint(&groups, &values, GuardLookup::Lenient);
        let expected = ContentHash::combine_all(std::iter::once(
            ContentHash::of_str(&format!("guard:{}={:?}", cell, val)),
        ));
        assert_eq!(result, expected);
    }

    #[test]
    fn guard_state_fingerprint_lenient_missing_value_uses_undef() {
        let cell = ValueCellId::new("E", "g");
        let values = ValueMap::new(); // cell absent
        let groups = vec![make_guard_group("E", "g")];
        let result = guard_state_fingerprint(&groups, &values, GuardLookup::Lenient);
        let expected = ContentHash::combine_all(std::iter::once(
            ContentHash::of_str(&format!("guard:{}={:?}", cell, Value::Undef)),
        ));
        assert_eq!(result, expected);
    }

    #[test]
    fn guard_state_fingerprint_strict_present_matches_lenient() {
        let cell = ValueCellId::new("E", "g");
        let val = Value::Bool(false);
        let mut values = ValueMap::new();
        values.insert(cell.clone(), val.clone());
        let groups = vec![make_guard_group("E", "g")];
        let strict = guard_state_fingerprint(&groups, &values, GuardLookup::Strict);
        let lenient = guard_state_fingerprint(&groups, &values, GuardLookup::Lenient);
        assert_eq!(strict, lenient);
    }

    #[test]
    #[should_panic(expected = "guard cell must have a value")]
    fn guard_state_fingerprint_strict_missing_panics() {
        let values = ValueMap::new(); // cell absent
        let groups = vec![make_guard_group("E", "g")];
        guard_state_fingerprint(&groups, &values, GuardLookup::Strict);
    }

    #[test]
    fn guard_state_fingerprint_distinct_cells_same_value_produce_distinct_hashes() {
        // Two distinct cells ("A.g" and "B.g") both mapped to Value::Bool(true).
        // Each cell must contribute its identity to the hash, so the two groups
        // produce different per-entry hashes and different combined fingerprints.
        let cell_a = ValueCellId::new("A", "g");
        let cell_b = ValueCellId::new("B", "g");
        let mut values = ValueMap::new();
        values.insert(cell_a, Value::Bool(true));
        values.insert(cell_b, Value::Bool(true));

        let fp_ab = guard_state_fingerprint(
            &[make_guard_group("A", "g"), make_guard_group("B", "g")],
            &values,
            GuardLookup::Lenient,
        );
        let fp_a = guard_state_fingerprint(
            &[make_guard_group("A", "g")],
            &values,
            GuardLookup::Lenient,
        );
        let fp_ba = guard_state_fingerprint(
            &[make_guard_group("B", "g"), make_guard_group("A", "g")],
            &values,
            GuardLookup::Lenient,
        );

        assert_ne!(
            fp_ab, fp_a,
            "two-group fingerprint must differ from single-group fingerprint"
        );
        assert_ne!(
            fp_ab, fp_ba,
            "cell ordering must affect the fingerprint (cell identity contributes to the hash)"
        );
    }

    // ── value_type_kind_matches: Tensor↔Matrix cross-variant unit tests ───────

    /// Value::Matrix supplied to Type::Tensor must return true.
    /// Regression-locks the `Value::Matrix(_) => matches!(ty, Type::Tensor { .. } | Type::Matrix { .. })`
    /// arm (lib.rs:125): a Matrix value is accepted by both Tensor and Matrix typed cells.
    #[test]
    fn value_type_kind_matches_matrix_value_into_tensor_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        let t = Type::Tensor { rank: 2, n: 3, quantity: Box::new(Type::Real) };
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Matrix should be accepted by Type::Tensor (cross-variant Ok-path)"
        );
    }

    /// Value::Tensor supplied to Type::Matrix must return true.
    /// Regression-locks the `Value::Tensor(_) => matches!(ty, Type::Tensor { .. } | Type::Matrix { .. })`
    /// arm (lib.rs:124): a Tensor value is accepted by both Tensor and Matrix typed cells.
    #[test]
    fn value_type_kind_matches_tensor_value_into_matrix_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let t = Type::Matrix { m: 3, n: 3, quantity: Box::new(Type::Real) };
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Tensor should be accepted by Type::Matrix (cross-variant Ok-path)"
        );
    }

    /// Value::Tensor supplied to Type::Real must return false.
    /// Regression-locks the *negative* path: Tensor values are rejected by
    /// non-Tensor/non-Matrix types, confirming the `matches!` guard cannot be
    /// trivially widened to `_ => true` without breaking this assertion.
    #[test]
    fn value_type_kind_matches_tensor_value_into_real_type_returns_false() {
        use reify_types::{Type, Value};
        let v = Value::Tensor(vec![]);
        let t = Type::Real;
        assert!(
            !value_type_kind_matches(&v, &t),
            "Value::Tensor should be rejected by Type::Real (negative kind-check path)"
        );
    }

    /// Value::Matrix supplied to Type::Real must return false.
    /// Regression-locks the *negative* path for Matrix, symmetric to the
    /// Tensor case above — confirms the `matches!` guard is not trivially dropped.
    #[test]
    fn value_type_kind_matches_matrix_value_into_real_type_returns_false() {
        use reify_types::{Type, Value};
        let v = Value::Matrix(vec![]);
        let t = Type::Real;
        assert!(
            !value_type_kind_matches(&v, &t),
            "Value::Matrix should be rejected by Type::Real (negative kind-check path)"
        );
    }

    // ── value_type_kind_matches: Type::Error anti-cascade guard (task-1922 / task-448) ──

    /// `Value::Real` paired with `Type::Error` must return `true`.
    ///
    /// Anti-cascade invariant (task-1922 / task-448): when a cell's declared type is
    /// the `Type::Error` poison sentinel the kind-check must not emit a spurious
    /// `TypeKindMismatch` on top of the root-cause compile error.  Returning `true`
    /// means "accept any value against a poisoned type" — the compiler already
    /// reported the defect.  Mirrors the guard in
    /// `reify_compiler::type_compat::{implicitly_converts_to, type_compatible}`.
    #[test]
    fn value_type_kind_matches_real_value_into_error_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Real(1.0);
        let t = Type::Error;
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Real against Type::Error must return true (anti-cascade guard)"
        );
    }

    /// `Value::Bool` paired with `Type::Error` must return `true`.
    ///
    /// Anti-cascade invariant (task-1922 / task-448): covers the non-numeric
    /// primitive arm — `Value::Bool` would normally only be accepted by `Type::Bool`,
    /// but a poisoned cell type must not trigger `TypeKindMismatch`.
    #[test]
    fn value_type_kind_matches_bool_value_into_error_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Bool(true);
        let t = Type::Error;
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Bool against Type::Error must return true (anti-cascade guard)"
        );
    }

    /// `Value::List` paired with `Type::Error` must return `true`.
    ///
    /// Anti-cascade invariant (task-1922 / task-448): covers the compound-value
    /// arm — `Value::List` would normally only be accepted by `Type::List(_)`,
    /// but a poisoned cell type must not trigger `TypeKindMismatch`.
    #[test]
    fn value_type_kind_matches_list_value_into_error_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::List(vec![Value::Int(1)]);
        let t = Type::Error;
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::List against Type::Error must return true (anti-cascade guard)"
        );
    }

    /// `Value::Undef` paired with `Type::Error` must return `true`.
    ///
    /// Regression lock (task-1922): `Value::Undef` is the Auto/no-value sentinel
    /// and is always accepted regardless of the cell type.  This test confirms
    /// that adding the early `Type::Error` guard does not perturb the already-true
    /// `Undef` arm — the guard fires first but the end result must remain `true`.
    #[test]
    fn value_type_kind_matches_undef_value_into_error_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Undef;
        let t = Type::Error;
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Undef against Type::Error must return true (Undef sentinel always accepted)"
        );
    }

    /// `Value::Bool` paired with `Type::Int` must return `false`.
    ///
    /// Negative-case lock (task-1922): the `Type::Error` early-return guard is the
    /// *only* unconditional-true path besides `Value::Undef`.  A mismatched
    /// value/type pair where `ty` is **not** `Type::Error` must still be rejected,
    /// ensuring a future refactor cannot accidentally widen the guard (e.g. by
    /// replacing `if ty.is_error()` with an always-true condition).
    #[test]
    fn value_type_kind_matches_bool_value_into_int_type_returns_false() {
        use reify_types::{Type, Value};
        let v = Value::Bool(true);
        let t = Type::Int;
        assert!(
            !value_type_kind_matches(&v, &t),
            "Value::Bool against Type::Int must return false (Type::Error guard must not over-fire)"
        );
    }

    // ── execute_realization_ops unit tests ────────────────────────────────────

    /// Happy path: all operations compile and execute successfully.
    /// Appends exactly one handle and emits no diagnostics.
    #[test]
    fn execute_realization_ops_happy_path_appends_handle() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| {
            CompiledExpr::literal(
                reify_test_support::mm(v),
                Type::length(),
            )
        };

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
        );

        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        assert!(diagnostics.is_empty(), "expected no diagnostics");
    }

    /// Compile failure: a Boolean op with out-of-bounds step references causes
    /// `compile_geometry_op` to return `None`. Truncates `step_handles` back to
    /// `handle_start` and emits 1 compile-error diagnostic.
    #[test]
    fn execute_realization_ops_compile_failure_truncates_handles() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty → compile_geometry_op returns None
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        // Pre-seed with a sentinel so we can assert truncation went back to exactly
        // this pre-call length, distinguishing "INVALID pushed then truncated" from
        // "INVALID never pushed at all".
        let pre_existing = GeometryHandleId(0xCAFE);
        let mut step_handles: Vec<GeometryHandleId> = vec![pre_existing];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
        );

        assert_eq!(
            step_handles.len(),
            1,
            "step_handles should be truncated back to pre-call length of 1; \
             the INVALID sentinel must not remain"
        );
        assert_eq!(
            step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        let compile_failures = diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures,
            1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures,
            diagnostics
        );
    }

    /// Kernel error: ops compile successfully but `kernel.execute()` returns `Err`.
    /// Truncates `step_handles` to `handle_start` and emits exactly 1 geometry-error
    /// diagnostic.
    #[test]
    fn execute_realization_ops_kernel_error_truncates_handles() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::FailingMockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| {
            CompiledExpr::literal(reify_test_support::mm(v), Type::length())
        };

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernel = FailingMockGeometryKernel;
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
        );

        assert!(
            step_handles.is_empty(),
            "handles should be truncated back to handle_start (0)"
        );
        let geometry_errors = diagnostics
            .iter()
            .filter(|d| d.message.contains("geometry error"))
            .count();
        assert_eq!(
            geometry_errors,
            1,
            "expected exactly 1 geometry-error diagnostic, got {}: {:?}",
            geometry_errors,
            diagnostics
        );
    }

    /// Multi-op rollback: a realization where the first op succeeds (real handle
    /// pushed) and a later op fails via compile error. Verifies that the real
    /// handle from the first op is discarded — `step_handles` is truncated back
    /// to its pre-call length, leaving only the handles that were there before
    /// `execute_realization_ops` was called.
    #[test]
    fn execute_realization_ops_partial_success_then_failure_discards_earlier_handles() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Two-op realization:
        //   op 0 — Box primitive: compiles and executes OK (real handle pushed)
        //   op 1 — Boolean union of Step(99) and Step(99): Step(99) is OOB
        //          (step_handles[handle_start..] will only have 1 entry after op 0)
        //          → compile_geometry_op returns None → rollback triggered
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Union,
                left: GeomRef::Step(99),
                right: GeomRef::Step(99),
            },
        ];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        // Pre-seed step_handles with a sentinel to verify truncation goes back
        // to exactly this pre-call length, not to zero.
        let pre_existing = GeometryHandleId(0xBEEF);
        let mut step_handles: Vec<GeometryHandleId> = vec![pre_existing];
        let mut diagnostics: Vec<Diagnostic> = vec![];

        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &values,
            &functions,
            &meta_map,
            &mut step_handles,
            &mut diagnostics,
        );

        // The real handle produced by op 0 must have been discarded.
        // Only the pre-existing handle should remain.
        assert_eq!(
            step_handles.len(),
            1,
            "step_handles should be truncated back to the pre-call length of 1; \
             the real handle from op 0 must be gone"
        );
        assert_eq!(
            step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        // Exactly one compile-error diagnostic from the failing op 1
        let compile_failures = diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures,
            1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures,
            diagnostics
        );
    }
}

