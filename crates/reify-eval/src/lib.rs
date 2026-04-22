pub mod cache;
mod concurrent;
pub use concurrent::{ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
pub mod demand;
pub mod deps;
pub mod dirty;
mod engine_admin;
mod engine_build;
mod engine_constraints;
mod engine_edit;
mod engine_eval;
mod engine_purposes;
mod geometry_ops;
pub mod graph;
pub mod journal;
pub mod snapshot;
pub mod test_runner;
mod unfold;
pub use test_runner::{TestResult, TestStatus, run_tests};

use std::collections::HashMap;

use reify_compiler::{CompiledModule, CompiledPurpose};
use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintNodeId, ConstraintSolver, ContentHash,
    Diagnostic, GeometryKernel, Mesh, OptimizationObjective, OptimizedImpl, Satisfaction,
    ValueCellId, ValueMap,
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
                write!(f, "value cell not found in evaluation graph: {cell}")
            }
            EngineError::DimensionMismatch {
                cell,
                expected,
                got,
            } => {
                write!(
                    f,
                    "dimension mismatch for {cell}: expected {:?}, got {:?}",
                    expected, got
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
        // Note: `Type::Geometry`, `Type::StructureRef`, and `Type::TypeParam` have
        // no corresponding `Value` variant, so any non-Undef value supplied to a
        // cell of those types falls through this `match` and returns `false`,
        // triggering `EngineError::TypeKindMismatch`. This default-reject behaviour
        // is sound because value cells never carry those types post-compilation —
        // an invariant enforced at runtime by the `#[cfg(debug_assertions)]`
        // `assert_value_cell_types_representable` check in
        // `crate::engine_eval::Engine::eval` (task 1867), and regression-locked
        // in CI by `crates/reify-eval/tests/value_cell_type_invariants.rs`. If a
        // future `Value::GeometryHandle` variant is added, add a matching arm here
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
    /// Phase 1 is NOT counted again in Phase 3 (edit_param only; task 2140).
    /// Used by tests to assert that the per-group skip is working correctly
    /// (e.g. only the affected group is re-elaborated, not all N groups).
    ///
    /// Exposed to callers only under `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// via `Engine::last_guard_phase_group_evals()` in `engine_admin.rs`.
    /// The field itself is always present (module-private, no `pub`) so that
    /// the writer sites in `engine_edit.rs` need no cfg-gating.
    last_guard_phase_group_evals: usize,
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

// Engine methods are split across sibling modules:
//   engine_admin.rs     — new, register/unregister_optimized_impl, accessors
//   engine_purposes.rs  — activate_purpose, deactivate_purpose, …
//   engine_constraints.rs — dispatch_constraints (pub(crate)), check, check_snapshot, …
//   engine_eval.rs      — eval, eval_cached, evaluate_let_bindings
//   engine_edit.rs      — set_param_and_invalidate, edit_param, edit_check
//   engine_build.rs     — build, build_snapshot, tessellate_*, execute_realization_ops
//   concurrent.rs       — prepare_concurrent_edit, apply_concurrent_edit, …

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
}
