pub mod cache;
mod concurrent;
pub use concurrent::{ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
pub mod demand;
pub mod deps;
pub mod dirty;
pub mod freshness_walk;
pub mod gating;
mod engine_admin;
mod engine_build;
mod engine_constraints;
mod engine_edit;
mod engine_eval;
#[doc(hidden)]
pub use engine_eval::ASSERT_MSG_PREFIX;
#[doc(hidden)]
pub use engine_eval::is_representable_cell_type;
mod engine_purposes;
mod geometry_ops;
pub mod graph;
pub mod journal;
pub mod snapshot;
pub mod test_runner;
pub mod topology_selectors;
mod unfold;
pub mod warm_pool;
pub use test_runner::{TestResult, TestStatus, run_tests};

use std::collections::HashMap;
use std::sync::Arc;

use reify_compiler::{CompiledModule, CompiledPurpose};
use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintNodeId, ConstraintSolver, ContentHash,
    Diagnostic, FeatureTagTable, GeometryKernel, Mesh, OptimizationObjective, OptimizedImpl,
    Satisfaction, ValueCellId, ValueMap,
};

use crate::cache::{CacheStore, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex};
use crate::graph::GuardedGroupInfo;
use crate::journal::EventJournal;
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
        // Boxed to keep the variant — and therefore `Result<_, EngineError>` —
        // small enough to satisfy `clippy::result_large_err`. Task 2377 grew
        // `DimensionVector` from `[Rational; 9]` to `[Rational; 10]` (36→40
        // bytes), which pushed this variant to 128 bytes (= the lint
        // threshold). Boxing each `DimensionVector` keeps the variant ≤ 64
        // bytes so call-sites returning `EngineError` continue to compile
        // under `-Dclippy::result_large_err`.
        expected: Box<reify_types::DimensionVector>,
        got: Box<reify_types::DimensionVector>,
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
                write!(f, "value cell not found in evaluation graph: {cell}")
            }
            EngineError::DimensionMismatch {
                cell,
                expected,
                got,
            } => {
                write!(
                    f,
                    "dimension mismatch for {cell}: expected {expected}, got {got}"
                )
            }
            EngineError::TypeKindMismatch {
                cell,
                expected,
                got,
            } => {
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
        // Note: `Type::Geometry` and `Type::TypeParam` have no corresponding
        // `Value` variant, so any non-Undef value supplied to a cell of those
        // types falls through this `match` and returns `false`, triggering
        // `EngineError::TypeKindMismatch`. This default-reject behaviour is
        // sound because value cells never carry those types post-compilation —
        // an invariant enforced at runtime by the `#[cfg(debug_assertions)]`
        // `assert_value_cell_types_representable` check in
        // `crate::engine_eval::Engine::eval` (task 1867), and regression-locked
        // in CI by `crates/reify-eval/tests/value_cell_type_invariants.rs`.
        //
        // `Type::StructureRef` is allowed on value cells (task 1876): user
        // code may write `param material : Material = Material(...)` where
        // `Material` is a canonical struct. The struct-call default evaluates
        // to `Value::Undef` via `reify_stdlib::eval_builtin`'s fallthrough
        // path (structure constructors are not builtins), and `Value::Undef`
        // is accepted for any type by the arm above. No Value variant exists
        // for structure instances yet — if one is added later, add a matching
        // arm here. The `Type::StructureRef` _arm itself_ is intentionally
        // omitted: the default-reject case is correct for any non-Undef
        // value because we have no way to represent a structure instance.
        //
        // `Type::TraitObject` is allowed for the same reason (task 2287):
        // trait-typed params default to `Value::Undef` via the same fallthrough
        // path, and `Value::Undef` is accepted for any type by the arm above.
        // The `Type::TraitObject` arm is intentionally omitted: the
        // default-reject case is correct for any non-Undef value because we
        // have no way to represent a trait-object instance.
        //
        // If a future `Value::GeometryHandle`, `Value::TraitObjectInstance`, or
        // `Value::StructureInstance` variant is added, add a matching arm here
        // AND relax the runtime assertion so the compiler enforces completeness.
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
    /// Count of non-skipped guarded-group iterations across Phase 1 and Phase 3
    /// of the most recent `edit_source` or `edit_param` call. Reset to 0 at
    /// the start of each `edit_source` / `edit_param` call (before Phase 1).
    /// Incremented once per group that is NOT skipped by the guard-value-unchanged
    /// optimisation or the cross-phase dedup set. A group re-elaborated in
    /// Phase 1 is NOT counted again in Phase 3 (edit_param: task 2140;
    /// edit_source: task 2142).
    /// Used by tests to assert that the per-group skip is working correctly
    /// (e.g. only the affected group is re-elaborated, not all N groups).
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_guard_phase_group_evals()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// the writer sites in `engine_edit.rs` need no cfg-gating.
    last_guard_phase_group_evals: usize,
    /// Count of `detect_role_flip` invocations on the hot path during the most
    /// recent `edit_source` call. Reset to 0 at the start of each `edit_source`
    /// call. Incremented every time `detect_role_flip` is called (currently at
    /// most once per `edit_source` after the deferred-probe refactor).
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_role_flip_probes()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// the writer sites in `engine_edit.rs` need no cfg-gating.
    last_role_flip_probes: usize,
    /// The `(changed, added, removed)` triple returned by `diff_value_cells`
    /// during the most recent `edit_source` call. `None` means either no
    /// `edit_source` has been called yet on this `Engine`, or a subsequent
    /// `edit_param` has cleared the snapshot (both are the "no current
    /// edit_source diff" state; distinct from an empty diff).
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_diff_value_cells()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so the
    /// struct layout is identical in test and non-test builds; the writer site
    /// in `engine_edit.rs` is `#[cfg(any(test, feature = "test-instrumentation"))]`-gated
    /// to skip the three `HashSet` clones in production. The reset at the top
    /// of `edit_param` is gated for the same reason. In non-test builds the
    /// field is therefore neither written nor read, which is why
    /// `#[allow(dead_code)]` is required on this line.
    ///
    /// Canonical use case: T3 premise lock — asserts that `S.x` and `S.y` are
    /// absent from all three sets after a role-flip-only edit, confirming that
    /// `ValueCellNode::content_hash` does not incorporate the member/else_member
    /// role (task 2170).
    #[allow(dead_code)]
    last_diff_value_cells: Option<crate::engine_edit::ValueCellDiff>,
    /// Count of param-override rejections due to `TypeKindMismatch` during the
    /// most recent `eval()` or `eval_cached()` call. Reset to 0 at the start
    /// of each call. Incremented inside `emit_param_override_rejection_warning`
    /// for the `TypeKindMismatch` arm.
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_param_override_type_kind_rejections()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// writer sites in `engine_eval.rs` need no cfg-gating.
    last_param_override_type_kind_rejections: usize,
    /// Count of param-override rejections due to `ScalarDimensionMismatch` during
    /// the most recent `eval()` or `eval_cached()` call. Reset to 0 at the start
    /// of each call. Incremented inside `emit_param_override_rejection_warning`
    /// for the `ScalarDimensionMismatch` arm.
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_param_override_dimension_rejections()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// writer sites in `engine_eval.rs` need no cfg-gating.
    last_param_override_dimension_rejections: usize,
    /// Count of sub-component elaboration errors due to an unknown structure
    /// reference during the most recent `eval()` or `eval_cached()` call.
    /// Reset to 0 at the start of each call. Incremented directly at both
    /// writer sites in `engine_eval.rs` (eval path and eval_cached path).
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_sub_component_unknown_structure_errors()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// writer sites in `engine_eval.rs` need no cfg-gating.
    last_sub_component_unknown_structure_errors: usize,
    /// Event journal recording evaluation events.
    journal: EventJournal,
    /// User-defined functions from the last eval() call.
    /// Stored so that edit_param() and other incremental paths can evaluate
    /// expressions containing UserFunctionCall nodes.
    /// Wrapped in Arc so per-call clones in eval(), edit_param(), and
    /// prepare_concurrent_edit() become O(1) refcount bumps rather than deep
    /// copies of the entire compiled function tree (task #1997).
    functions: Arc<Vec<CompiledFunction>>,
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
    /// Stored as Arc so hot-path clones (e.g. before evaluate_let_bindings calls)
    /// are O(1) reference-count increments rather than deep HashMap copies.
    meta_map: Arc<HashMap<String, HashMap<String, String>>>,
    /// Template-native optimization objectives from the last eval() call.
    /// Maps template name → optimization objective declared in the template.
    /// Populated during eval() so that edit_param() can look up the objective
    /// by scope_name without needing access to the original templates.
    objectives: HashMap<String, OptimizationObjective>,
    /// Compiled field declarations from the last eval() / edit_source() call.
    ///
    /// Stored so that incremental paths — primarily `Engine::edit_param`
    /// (task 2343) — can re-elaborate composed fields when their tracked
    /// dependencies land in the dirty cone. Populated by both `Engine::eval`
    /// and `Engine::edit_source` from `module.fields`. Wrapped in `Arc` so
    /// the per-call clone in `edit_param` is an O(1) refcount bump rather
    /// than a deep copy of the field tree.
    compiled_fields: Arc<Vec<reify_compiler::CompiledField>>,
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
    /// Registry of named constraint solvers selectable via the `#solver(<name>)`
    /// module pragma (Task 2300). Populated at runtime startup via
    /// `register_solver`; the default fallback solver remains `self.solver`
    /// (set via `with_solver`). At solve time, `Engine::resolve_solver_for_module`
    /// looks up `module.solver_pragma.name` here; on miss it falls back to
    /// `self.solver` and emits a "named solver not registered" warning.
    solvers: HashMap<String, Box<dyn ConstraintSolver>>,
    /// Memory-budgeted pool that holds warm-start state donated by removed
    /// nodes between topology edits. Populated by `edit_source` when value
    /// cells / constraints / realizations are removed (donation), drained
    /// when topology re-adds the same `NodeId` (checkout). Per arch §4.3
    /// lines 539-540 and §6.4 lines 654-660.
    ///
    /// Initialised via `WarmStatePool::from_env_or_default()` in both
    /// `Engine::new` and `Engine::with_prelude`. Test-instrumentation accessors
    /// `warm_pool()` / `warm_pool_mut()` (cfg-gated to test/test-instrumentation
    /// builds) live in `engine_admin.rs`.
    warm_pool: crate::warm_pool::WarmStatePool,
    /// Maps each successfully-produced `GeometryHandleId` to the `FeatureTag`
    /// derived from its position in the realization's parallel `feature_tags`
    /// array. Populated by `Engine::execute_realization_ops` immediately after
    /// `kernel.execute(...)` returns `Ok(handle)`. Cleared and repopulated on
    /// each `build()` / `build_snapshot()` call.
    ///
    /// Exposed via `Engine::feature_tag_table()` so topology selectors and
    /// GUI consumers can correlate geometry handles back to source locations.
    feature_tag_table: FeatureTagTable,
    /// Test-instrumentation set of `ValueCellId`s whose let-binding evaluation
    /// should be force-panicked just before `reify_expr::eval_expr` runs.
    /// Populated only via `Engine::set_panic_on_eval` (cfg-gated to test /
    /// `test-instrumentation` builds); the field itself is always present so
    /// the struct layout is identical in test and production builds (same
    /// trick used by `last_guard_phase_group_evals` etc.).
    ///
    /// In production builds the set stays empty — the let-binding evaluator's
    /// hot path costs only a single `HashSet::contains` check per cell. The
    /// catch_unwind panic boundary in `evaluate_let_bindings` converts any
    /// panic raised here (or by any other path inside `eval_expr`) into a
    /// `Freshness::Failed { error }` write plus a single `EventKind::Failed`
    /// event, per arch §9.1 line 868–877.
    panic_on_eval_cells: std::collections::HashSet<ValueCellId>,
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
    /// Computed values for every cell whose evaluation produced one.
    ///
    /// **PARTIAL-MAP INVARIANT** (see engine_eval.rs ~L494-518): Param cells
    /// that have NO `default_expr` AND NO entry in `Engine::param_overrides`
    /// are intentionally OMITTED from this map — preserving the pre-task-2017
    /// silent-skip baseline.  Callers iterating `EvalResult.values` for Param
    /// cells MUST guard their lookups (e.g. `values.get_or_undef(&id)` on
    /// `ValueMap`) — `.get(&id).unwrap()` will panic on no-override-no-default
    /// cells.
    ///
    /// All OTHER paths populate `values`:
    /// - Auto cells → `Value::Undef`
    /// - Param with override (accepted) → the override value
    /// - Param with override (rejected) AND no default → `Value::Undef`
    /// - Param with default_expr → evaluated default
    /// - Let / guarded-group cells → see their respective evaluators
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

// Engine methods are split across sibling modules:
//   engine_admin.rs     — new, register/unregister_optimized_impl, accessors
//   engine_purposes.rs  — activate_purpose, deactivate_purpose, …
//   engine_constraints.rs — dispatch_constraints (pub(crate)), check, check_snapshot, …
//   engine_eval.rs      — eval, eval_cached, evaluate_let_bindings
//   engine_edit.rs      — set_param_and_invalidate, edit_param, edit_check
//   engine_build.rs     — build, build_snapshot, tessellate_*, execute_realization_ops
//   concurrent.rs       — prepare_concurrent_edit, apply_concurrent_edit, …

/// Canonical construction point for an [`reify_expr::EvalContext`] with meta-map binding.
///
/// Both `&mut self` methods (which previously had to inline the construction to avoid
/// conflicting borrows) and free-function helpers (e.g. `evaluate_let_bindings`,
/// `compile_geometry_op`) call this function to produce a consistently-wired context.
///
/// # Arguments
/// * `values`    — current cell values for the evaluation pass
/// * `functions` — compiled user functions available in scope
/// * `meta_map`  — entity-name → (key → string-value) meta block entries;
///   passed to `EvalContext::with_meta` so that `MetaAccess` expressions resolve
///   to the `Value::String` declared for `<entity>.<key>` in the source module's
///   `meta {}` blocks (or `Value::Undef` if no such entry exists).
pub(crate) fn eval_ctx_with_meta<'a>(
    values: &'a ValueMap,
    functions: &'a [CompiledFunction],
    meta_map: &'a HashMap<String, HashMap<String, String>>,
) -> reify_expr::EvalContext<'a> {
    reify_expr::EvalContext::new(values, functions).with_meta(meta_map)
}

/// Build the per-template meta-map consumed by `eval_ctx_with_meta`.
///
/// Filters out templates with empty `meta` blocks and clones each
/// non-empty entry into the returned `Arc`-wrapped HashMap so the result
/// can be cheaply shared (Arc::clone) with `ConcurrentEditSetup` and
/// other consumers without deep-copying the inner string maps.
///
/// Centralised in `lib.rs` so future shape changes (interning, additional
/// filter rules) land in exactly one place — see task 2216 / esc-397-72
/// suggestion 2.
pub(crate) fn build_meta_map(
    module: &CompiledModule,
) -> Arc<HashMap<String, HashMap<String, String>>> {
    Arc::new(
        module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect(),
    )
}

/// Merge a module's user functions with the prelude function table into a new
/// `Arc<Vec<CompiledFunction>>`.
///
/// # SHADOWING INVARIANT
/// Module (user) functions are stored **first**, then prelude functions are
/// appended after. `reify_expr::eval_user_function_call` resolves calls via a
/// first-match-wins linear scan on `(name, arity, param_types)`. Therefore,
/// any user function whose signature matches a prelude function takes precedence
/// and shadows the prelude implementation. The compiler's duplicate-function
/// check only compares user functions against each other (not the prelude), so
/// user code may freely redefine prelude signatures without diagnostics.
///
/// # COEXISTENCE COROLLARY
/// A user function whose `(name, arity, param_types)` triple differs from all
/// prelude functions does NOT shadow those prelude functions — both remain
/// independently callable.
///
/// # Unfiltered append
/// All prelude entries are appended unconditionally; entries whose signature
/// collides with a user function are permanently unreachable at dispatch time
/// (shadowed by the earlier match), so filtering is unnecessary. This diverges
/// from `reify_compiler::merge_prelude_functions`, which applies an explicit
/// filter to avoid ambiguous-overload errors at compile time; the eval dispatch
/// table is safe without it because first-match-wins is unambiguous by
/// construction.
///
/// # Performance
/// The merged table is built once per `eval()`/`edit_source()` call into a
/// local `Vec`, then sealed by `Arc::new`. Subsequent clones (e.g. in
/// `prepare_concurrent_edit`, `edit_param`) are O(1) refcount bumps.
pub(crate) fn merge_functions(
    module: &CompiledModule,
    prelude: &[CompiledFunction],
) -> Arc<Vec<CompiledFunction>> {
    Arc::new({
        let mut v = module.functions.clone();
        v.extend(prelude.iter().cloned());
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::Value;

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
        let expected = ContentHash::combine_all(std::iter::once(ContentHash::of_str(&format!(
            "guard:{}={:?}",
            cell, val
        ))));
        assert_eq!(result, expected);
    }

    #[test]
    fn guard_state_fingerprint_lenient_missing_value_uses_undef() {
        let cell = ValueCellId::new("E", "g");
        let values = ValueMap::new(); // cell absent
        let groups = vec![make_guard_group("E", "g")];
        let result = guard_state_fingerprint(&groups, &values, GuardLookup::Lenient);
        let expected = ContentHash::combine_all(std::iter::once(ContentHash::of_str(&format!(
            "guard:{}={:?}",
            cell,
            Value::Undef
        ))));
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
        let fp_a =
            guard_state_fingerprint(&[make_guard_group("A", "g")], &values, GuardLookup::Lenient);
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
    /// arm in `value_type_kind_matches`: a Matrix value is accepted by both Tensor and Matrix typed cells.
    /// This test verifies the kind-match only — `value_type_kind_matches` is a shallow check, so the
    /// Tensor's `rank`/`n` are NOT validated against the Matrix value's actual element count here
    /// (expected-unchecked at this layer).
    #[test]
    fn value_type_kind_matches_matrix_value_into_tensor_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        let t = Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::Real),
        };
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Matrix should be accepted by Type::Tensor (cross-variant Ok-path)"
        );
    }

    /// Value::Tensor supplied to Type::Matrix must return true.
    /// Regression-locks the `Value::Tensor(_) => matches!(ty, Type::Tensor { .. } | Type::Matrix { .. })`
    /// arm in `value_type_kind_matches`: a Tensor value is accepted by both Tensor and Matrix typed cells.
    /// This test verifies the kind-match only — `value_type_kind_matches` is a shallow check, so the
    /// Matrix's `m`/`n` are NOT validated against the Tensor value's actual element count here
    /// (expected-unchecked at this layer).
    #[test]
    fn value_type_kind_matches_tensor_value_into_matrix_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let t = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::Real),
        };
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

    // ── value_type_kind_matches: Bool arm direct coverage (task-1893) ────────
    // task-1922 added a Bool→Int negative lock above; these two tests complete
    // the symmetric set: Int→Bool negative and Bool→Bool positive.  Together the
    // three locks pin the arm against accidental widening or deletion.

    /// Negative lock for the Bool arm: non-Bool values must not satisfy Type::Bool.
    #[test]
    fn value_type_kind_matches_int_value_into_bool_type_returns_false() {
        use reify_types::{Type, Value};
        let v = Value::Int(1);
        let t = Type::Bool;
        assert!(
            !value_type_kind_matches(&v, &t),
            "Value::Int against Type::Bool must return false"
        );
    }

    /// Positive lock for the Bool arm: Bool values must satisfy Type::Bool.
    #[test]
    fn value_type_kind_matches_bool_value_into_bool_type_returns_true() {
        use reify_types::{Type, Value};
        let v = Value::Bool(true);
        let t = Type::Bool;
        assert!(
            value_type_kind_matches(&v, &t),
            "Value::Bool against Type::Bool must return true"
        );
    }

    // execute_realization_ops_* tests moved to engine_build.rs

    // ── Engine.functions accumulation regression (task 506 / 1873) ───────────

    /// Regression guard: `eval()` must **replace** the combined function table on
    /// every call, never extend it.  If `engine_eval.rs` ever changed from
    /// `self.functions = …` to `self.functions.extend(…)`, the count would grow
    /// with each call.
    ///
    /// This assertion accesses the private `Engine::functions` field directly —
    /// the test module is a child of `lib.rs` and inherits same-module visibility.
    /// No public accessor is needed.
    ///
    /// Value-level idempotency is covered separately by the sibling integration
    /// test `eval_is_idempotent_for_prelude_functions` in
    /// `crates/reify-eval/tests/stdlib_prelude_tests.rs`.
    #[test]
    fn eval_does_not_accumulate_functions() {
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::ModulePath;

        let source = r#"
fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {
    nominal - deviation
}

structure S {
    let v : Length = symmetric_tolerance(5mm, 2mm)
}
"#;
        let prelude = reify_compiler::stdlib_loader::load_stdlib();
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors: {:?}",
            parsed.errors
        );

        let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

        let checker = MockConstraintChecker::new();
        let mut engine = Engine::new(Box::new(checker), None);

        // First eval
        engine.eval(&compiled);
        let count1 = engine.functions.len();

        // Second eval on same engine — must not grow
        engine.eval(&compiled);
        let count2 = engine.functions.len();

        assert!(
            count1 > 0,
            "sanity: function table must be non-empty after eval (got 0 — check prelude wiring)"
        );
        assert_eq!(
            count1, count2,
            "eval() must replace, not extend, self.functions: count1={} count2={}",
            count1, count2
        );
    }

    // ── eval_ctx_with_meta helper ─────────────────────────────────────────────

    /// Canonical construction test: `eval_ctx_with_meta` must produce an
    /// `EvalContext` that resolves a `MetaAccess` expression to the correct
    /// `Value::String`.
    ///
    /// Expected compile-failure before step-4 impl: `eval_ctx_with_meta` does
    /// not exist — `error[E0425]: cannot find function 'eval_ctx_with_meta'`.
    #[test]
    fn eval_ctx_with_meta_resolves_meta_access() {
        use reify_types::{CompiledExpr, Value, ValueMap};
        use std::collections::HashMap;

        let values = ValueMap::new();
        let functions: &[reify_types::CompiledFunction] = &[];
        let mut widget_meta = HashMap::new();
        widget_meta.insert("description".to_string(), "A gadget".to_string());
        let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        meta_map.insert("Widget".to_string(), widget_meta);

        let ctx = eval_ctx_with_meta(&values, functions, &meta_map);

        let expr = CompiledExpr::meta_access("Widget".into(), "description".into());
        let result = reify_expr::eval_expr(&expr, &ctx);
        assert_eq!(
            result,
            Value::String("A gadget".to_string()),
            "eval_ctx_with_meta must produce an EvalContext that resolves MetaAccess correctly"
        );
    }

    // ── Arc-sharing invariant: Engine.meta_map ────────────────────────────────

    /// Arc-sharing invariant: after `prepare_concurrent_edit`, the
    /// `ConcurrentEditSetup.meta_map` must share the *same* Arc as
    /// `Engine.meta_map` (i.e. `Arc::ptr_eq` returns true, and `strong_count >= 2`).
    ///
    /// Expected compile-failure before step-2 impl: `Engine.meta_map` is
    /// `HashMap<String, HashMap<String, String>>`, not `Arc<...>`, so
    /// `Arc::ptr_eq(&engine.meta_map, ...)` is a type error.
    #[test]
    fn meta_map_arc_shared_with_concurrent_setup() {
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, literal};
        use reify_types::{ModulePath, Type, Value, ValueCellId};
        use std::sync::Arc;

        let meta_entries = {
            let mut m = std::collections::HashMap::new();
            m.insert("color".to_string(), "blue".to_string());
            m
        };

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("Widget")
                    .meta(meta_entries)
                    .param(
                        "Widget",
                        "width",
                        Type::Real,
                        Some(literal(Value::Real(1.0))),
                    )
                    .build(),
            )
            .build();

        let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        engine.eval(&module);

        let cell = ValueCellId::new("Widget", "width");
        let setup = engine
            .prepare_concurrent_edit(cell, Value::Real(2.0))
            .expect("prepare_concurrent_edit must succeed after eval");

        // Before step-2 this does not compile:
        //   error[E0308]: expected `&Arc<_>`, found `&HashMap<_, _>`
        assert!(
            Arc::ptr_eq(&engine.meta_map, &setup.meta_map),
            "Engine.meta_map and ConcurrentEditSetup.meta_map must share the same Arc (not deep clone)"
        );
        assert!(
            Arc::strong_count(&engine.meta_map) >= 2,
            "strong_count must be >= 2 (engine + setup both hold a ref); got {}",
            Arc::strong_count(&engine.meta_map)
        );
    }

    #[test]
    fn build_meta_map_filters_empty_and_preserves_non_empty_meta() {
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
        use reify_types::ModulePath;

        let meta_entries = {
            let mut m = std::collections::HashMap::new();
            m.insert("color".to_string(), "blue".to_string());
            m.insert("material".to_string(), "steel".to_string());
            m
        };

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("Widget")
                    .meta(meta_entries)
                    .build(),
            )
            .template(TopologyTemplateBuilder::new("Bare").build())
            .build();

        let result = build_meta_map(&module);
        let result = result.as_ref();

        assert_eq!(result.len(), 1, "only Widget has non-empty meta");
        assert!(result.contains_key("Widget"), "Widget must be present");
        assert!(!result.contains_key("Bare"), "Bare must be filtered out");
        assert_eq!(
            result["Widget"]["color"], "blue",
            "Widget.color must be 'blue'"
        );
        assert_eq!(
            result["Widget"]["material"], "steel",
            "Widget.material must be 'steel'"
        );
    }

    // ── Arc-sharing invariant: Engine.functions ───────────────────────────────

    /// Arc-sharing invariant: after `prepare_concurrent_edit`, the
    /// `ConcurrentEditSetup.functions` must share the *same* Arc allocation as
    /// `Engine.functions` (i.e. `Arc::ptr_eq` returns true, and
    /// `Arc::strong_count >= 2`). This proves the per-call clone is O(1)
    /// (a refcount bump), not an O(N) deep clone of the entire function table.
    ///
    /// Expected compile-failure before impl-1: `Engine.functions` is
    /// `Vec<CompiledFunction>`, not `Arc<Vec<CompiledFunction>>`, so
    /// `Arc::ptr_eq(&engine.functions, &setup.functions)` is a type error
    /// (`error[E0308]: mismatched types`). Both fields must be Arc'd before
    /// this test can compile (task #1997).
    #[test]
    fn prepare_concurrent_edit_shares_functions_arc_with_engine() {
        use reify_test_support::bracket_compiled_module;
        use reify_test_support::mocks::MockConstraintChecker;
        use std::sync::Arc;

        let module = bracket_compiled_module();
        let checker = MockConstraintChecker::new();
        let mut engine = Engine::new(Box::new(checker), None);
        engine.eval(&module);

        let cell = ValueCellId::new("Bracket", "width");
        let setup = engine
            .prepare_concurrent_edit(cell, Value::length(0.1))
            .expect("prepare_concurrent_edit must succeed after eval");

        assert!(
            Arc::ptr_eq(&engine.functions, &setup.functions),
            "ConcurrentEditSetup.functions must share the same Arc allocation as \
            Engine.functions — proves the per-call clone is O(1) Arc::clone, not a \
            deep clone of the function table (task #1997)"
        );
        assert!(
            Arc::strong_count(&engine.functions) >= 2,
            "strong_count must be >= 2 (engine + setup both hold a ref); got {}",
            Arc::strong_count(&engine.functions)
        );
    }

    // ── Arc-sharing invariant: ResolutionProblem.functions (task #2286) ───────

    /// Arc-sharing invariant: after `eval()`, the `ResolutionProblem.functions`
    /// passed to the solver must share the *same* Arc allocation as
    /// `Engine.functions` (i.e. `Arc::ptr_eq` returns true, and
    /// `Arc::strong_count >= 2`). This proves the per-solver-call construction is
    /// O(1) (a refcount bump), not an O(N) deep clone of the entire function table.
    #[test]
    fn eval_resolution_problem_shares_functions_arc_with_engine() {
        use reify_test_support::mocks::{MockConstraintChecker, SpyConstraintSolver};
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref,
        };
        use reify_types::{ModulePath, Type, ValueCellId};
        use std::collections::HashMap;
        use std::sync::Arc;

        let thickness_id = ValueCellId::new("S", "thickness");
        let mut solved_values = HashMap::new();
        solved_values.insert(thickness_id, mm(5.0));

        let spy = SpyConstraintSolver::new_solved(solved_values);
        let captured = spy.captured_problem();

        let template = TopologyTemplateBuilder::new("S")
            .auto_param("S", "thickness", Type::length())
            .constraint(
                "S",
                0,
                None,
                gt(value_ref("S", "thickness"), literal(mm(2.0))),
            )
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();

        let mut engine =
            Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

        engine.eval(&module);

        let guard = captured.lock().unwrap();
        let problem = guard
            .as_ref()
            .expect("solver should have been called during eval");

        assert!(
            Arc::ptr_eq(&engine.functions, &problem.functions),
            "ResolutionProblem.functions must share the same Arc allocation as \
            Engine.functions — proves the per-call clone is O(1) Arc::clone, not a \
            deep clone of the function table (task #2286)"
        );
        assert!(
            Arc::strong_count(&engine.functions) >= 2,
            "strong_count must be >= 2 (engine + captured problem both hold a ref); got {}",
            Arc::strong_count(&engine.functions)
        );
    }

    /// Arc-sharing invariant: after `edit_param()`, the `ResolutionProblem.functions`
    /// passed to the solver must share the *same* Arc allocation as
    /// `Engine.functions`. This covers the inline construction site in
    /// `engine_edit.rs` (task #2286).
    ///
    /// The test builds a module with an auto-param `thickness` constrained to be
    /// greater than a regular param `limit`. Calling `edit_param(limit, new_val)`
    /// dirties the constraint and triggers re-resolution, so the spy captures the
    /// new `ResolutionProblem` from the `edit_param` code path.
    #[test]
    fn edit_param_resolution_problem_shares_functions_arc_with_engine() {
        use reify_test_support::mocks::{MockConstraintChecker, SpyConstraintSolver};
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref,
        };
        use reify_types::{ModulePath, Type, ValueCellId};
        use std::collections::HashMap;
        use std::sync::Arc;

        let thickness_id = ValueCellId::new("S", "thickness");
        let limit_id = ValueCellId::new("S", "limit");

        // Solver returns thickness = 5mm each time it's called.
        let mut solved_values = HashMap::new();
        solved_values.insert(thickness_id.clone(), mm(5.0));

        let spy = SpyConstraintSolver::new_solved(solved_values);
        let captured = spy.captured_problem();

        // Template: auto thickness, regular param limit (default 2mm),
        // constraint: thickness > limit.
        let template = TopologyTemplateBuilder::new("S")
            .auto_param("S", "thickness", Type::length())
            .param("S", "limit", Type::length(), Some(literal(mm(2.0))))
            .constraint(
                "S",
                0,
                None,
                gt(value_ref("S", "thickness"), value_ref("S", "limit")),
            )
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();

        let mut engine =
            Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

        // Initial eval — solver fires once.
        engine.eval(&module);

        // Editing `limit` dirties the constraint and triggers re-resolution via
        // the engine_edit.rs path.
        engine.edit_param(limit_id, mm(3.0)).unwrap();

        // The spy now holds the problem from the most recent solver call
        // (the edit_param re-resolution).
        let guard = captured.lock().unwrap();
        let problem = guard
            .as_ref()
            .expect("solver should have been called during edit_param");

        assert!(
            Arc::ptr_eq(&engine.functions, &problem.functions),
            "ResolutionProblem.functions must share the same Arc allocation as \
            Engine.functions in the edit_param path — proves the construction is \
            O(1) Arc::clone, not a deep clone (task #2286)"
        );
        assert!(
            Arc::strong_count(&engine.functions) >= 2,
            "strong_count must be >= 2 (engine + captured problem both hold a ref); got {}",
            Arc::strong_count(&engine.functions)
        );
    }

    /// Arc-sharing invariant: after `resolve_concurrent_edit()`, the
    /// `ResolutionProblem.functions` passed to the solver must share the *same*
    /// Arc allocation as `Engine.functions`. This covers the inline construction
    /// site in `concurrent.rs` (task #2286).
    ///
    /// Changing `limit` (a regular param that appears in the constraint) creates
    /// a dirty cone on the constraint, causing `resolve_concurrent_edit` to fire
    /// the solver. The spy captures the `ResolutionProblem` from that call.
    #[test]
    fn resolve_concurrent_edit_resolution_problem_shares_functions_arc_with_engine() {
        use reify_test_support::mocks::{MockConstraintChecker, SpyConstraintSolver};
        use reify_test_support::{
            CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, mm, value_ref,
        };
        use reify_types::{ModulePath, Type, ValueCellId};
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        let thickness_id = ValueCellId::new("S", "thickness");
        let limit_id = ValueCellId::new("S", "limit");

        // Solver returns thickness = 5mm each time it's called.
        let mut solved_values = HashMap::new();
        solved_values.insert(thickness_id.clone(), mm(5.0));

        let spy = SpyConstraintSolver::new_solved(solved_values);
        let captured = spy.captured_problem();

        // Template: auto thickness, regular param limit (default 2mm),
        // constraint: thickness > limit.
        let template = TopologyTemplateBuilder::new("S")
            .auto_param("S", "thickness", Type::length())
            .param("S", "limit", Type::length(), Some(literal(mm(2.0))))
            .constraint(
                "S",
                0,
                None,
                gt(value_ref("S", "thickness"), value_ref("S", "limit")),
            )
            .build();

        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(template)
            .build();

        let mut engine =
            Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

        // Initial eval — solver fires once.
        engine.eval(&module);

        // Prepare a concurrent edit: change limit to 3mm (dirties the constraint).
        let setup = engine
            .prepare_concurrent_edit(limit_id, mm(3.0))
            .expect("prepare_concurrent_edit must succeed after eval");

        // Build a minimal ConcurrentEditResult using the setup's snapshot values.
        let mut result = ConcurrentEditResult {
            values: setup.values.clone(),
            snapshot_values: setup.snapshot_values.clone(),
            node_results: Vec::new(),
            actual_eval_set: Vec::new(),
            skipped: HashSet::new(),
            resolved_params: HashMap::new(),
            diagnostics: Vec::new(),
        };

        // resolve_concurrent_edit detects the dirty constraint and calls the solver.
        engine.resolve_concurrent_edit(&setup, &mut result);

        // The spy now holds the problem from the resolve_concurrent_edit call.
        let guard = captured.lock().unwrap();
        let problem = guard
            .as_ref()
            .expect("solver should have been called during resolve_concurrent_edit");

        assert!(
            Arc::ptr_eq(&engine.functions, &problem.functions),
            "ResolutionProblem.functions must share the same Arc allocation as \
            Engine.functions in the resolve_concurrent_edit path — proves the \
            construction is O(1) Arc::clone, not a deep clone (task #2286)"
        );
        assert!(
            Arc::strong_count(&engine.functions) >= 2,
            "strong_count must be >= 2 (engine + captured problem both hold a ref); got {}",
            Arc::strong_count(&engine.functions)
        );
    }

    // ── EngineError::DimensionMismatch Display regression (task-2442) ────────

    /// Regression lock (task-2442): `EngineError::DimensionMismatch` must render
    /// its `expected` and `got` `DimensionVector` fields using `Display` (unit
    /// notation like `"m"`, `"kg"`) rather than `Debug` (raw `Rational`-tuple dump).
    /// A reversion to `{:?}` would produce output like
    /// `"DimensionVector([Rational { num: 1, den: 1 }, ...])"` which is not
    /// user-facing friendly; this exact-equality assertion catches that immediately.
    #[test]
    fn engine_error_dimension_mismatch_display_uses_dimension_vector_display() {
        let err = EngineError::DimensionMismatch {
            cell: ValueCellId::new("Assembly", "height"),
            expected: Box::new(reify_types::DimensionVector::LENGTH),
            got: Box::new(reify_types::DimensionVector::MASS),
        };
        assert_eq!(
            err.to_string(),
            "dimension mismatch for Assembly.height: expected m, got kg"
        );
    }

    // ── Task 2345 step-3: Engine holds a WarmStatePool ───────────────────────
    //
    // Wires `Engine::warm_pool` per arch §4.3 / §6.4. The pool is initialised
    // via `WarmStatePool::from_env_or_default()`, so absent the
    // `REIFY_WARM_STATE_BUDGET_BYTES` env var the budget equals
    // `DEFAULT_BUDGET_BYTES` (2 GiB).
    //
    // ── Hermeticity note (amendment) ─────────────────────────────────────
    // The default-budget contract is verified via the `from_env_value(None)`
    // seam, which is the same code path `Engine::new` ends up exercising
    // when the env var is unset, but pinned without depending on the
    // ambient process environment. Asserting on `Engine::new(...).warm_pool()`
    // directly would be flaky if a developer or CI runner had
    // `REIFY_WARM_STATE_BUDGET_BYTES` exported (e.g. to `unlimited` or a
    // non-default integer) — the same flakiness the `from_env_or_default`
    // doc-comment calls out. The companion wiring assertion still pins
    // that `Engine::new` constructs a `WarmStatePool` without panicking
    // and exposes it through `warm_pool()`; just the budget value is
    // checked at the hermetic seam.

    #[test]
    fn engine_warm_pool_default_budget_is_two_gib() {
        // Hermetic: targets the env-parsing seam directly so the assertion
        // is independent of the ambient REIFY_WARM_STATE_BUDGET_BYTES value.
        use crate::warm_pool::{DEFAULT_BUDGET_BYTES, WarmStatePool};

        let pool = WarmStatePool::from_env_value(None);
        assert_eq!(
            pool.budget_bytes(),
            Some(DEFAULT_BUDGET_BYTES),
            "WarmStatePool::from_env_value(None) — the seam Engine::new uses \
             when REIFY_WARM_STATE_BUDGET_BYTES is unset — must yield the \
             default budget"
        );
    }

    #[test]
    fn engine_new_exposes_warm_pool_via_accessor() {
        // Wiring assertion: confirm Engine::new constructs and exposes the
        // pool through the test-instrumentation accessor. Does NOT assert on
        // the budget value (that's covered hermetically above) — keeps this
        // test independent of ambient REIFY_WARM_STATE_BUDGET_BYTES too.
        use reify_test_support::mocks::MockConstraintChecker;

        let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        // budget_bytes() returns Option<usize>; the call alone proves wiring.
        let _ = engine.warm_pool().budget_bytes();
        assert_eq!(
            engine.warm_pool().used_bytes(),
            0,
            "freshly-constructed Engine's warm_pool must start empty"
        );
    }

    /// Hermetic regression test: `Engine::new` must wire the warm pool through
    /// `WarmStatePool::from_env_or_default()`.
    ///
    /// # Hermeticity argument
    ///
    /// We snapshot `REIFY_WARM_STATE_BUDGET_BYTES` once before constructing the
    /// engine, then build the *expected* pool via `from_env_value(snapshot.as_deref())`
    /// — the same parsing path that `from_env_or_default()` delegates to.  Using a
    /// pre-captured snapshot eliminates the second `std::env::var` call that would
    /// otherwise create a TOCTOU window: a hypothetical concurrent test that mutates
    /// the env var between `Engine::new()` and a separate `from_env_or_default()` call
    /// would cause a spurious mismatch.  With the snapshot both sides use the same
    /// parsed value regardless of any intervening mutation.
    ///
    /// # What regressions this catches
    ///
    /// - Replacing `from_env_or_default()` with `WarmStatePool::unlimited()`:
    ///   `engine.budget_bytes()` would return `None`, while
    ///   `from_env_value(snapshot.as_deref())` returns `Some(DEFAULT)` when
    ///   the env var is absent — divergence detected.
    /// - Replacing with `WarmStatePool::new(42)`:
    ///   `engine.budget_bytes()` returns `Some(42)`, while
    ///   `from_env_value(snapshot.as_deref())` returns `Some(DEFAULT)` —
    ///   divergence detected.
    #[test]
    fn engine_new_wires_warm_pool_through_from_env_or_default() {
        use crate::warm_pool::{WarmStatePool, BUDGET_ENV_VAR};
        use reify_test_support::mocks::MockConstraintChecker;

        // Snapshot the env var before engine construction so both sides share a
        // single read — avoids TOCTOU with concurrent env-mutating tests.
        let snapshot = std::env::var(BUDGET_ENV_VAR).ok();
        let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        let expected = WarmStatePool::from_env_value(snapshot.as_deref());

        assert_eq!(
            engine.warm_pool().budget_bytes(),
            expected.budget_bytes(),
            "Engine::new must initialise warm_pool via \
             WarmStatePool::from_env_or_default(); a regression to \
             ::unlimited() or ::new(arbitrary) would diverge here \
             (engine and expected pool both resolve from the same env snapshot)"
        );
    }
}
