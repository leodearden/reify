// Split from lib.rs (task 2032) — admin methods.

use crate::cache::{CacheStore, NodeId};
use crate::demand::DemandRegistry;
use crate::journal::EventJournal;
use crate::snapshot::Snapshot;
use crate::{Engine, EvaluationState};
use reify_compiler::{CompiledModule, ValueCellKind};
use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintSolver, Diagnostic, GeometryKernel,
    OptimizedImpl,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Why an attempted param_override was rejected for a target value cell.
/// Callers translate this into their own error channel:
/// - `Engine::eval` pushes a `Diagnostic::warning` and falls back to the
///   cell's `default_expr`.
/// - `Engine::edit_param` returns the corresponding `EngineError`
///   variant (`TypeKindMismatch` / `DimensionMismatch`).
///
/// Centralising the rejection vocabulary (task 2017 amend-pass → completed
/// under task 2178) lets a future third guard (e.g. Tensor shape, List
/// element-type check) land in one place rather than drifting between the
/// cold-start (`Engine::eval`) and incremental (`Engine::edit_param`) paths.
/// Both call sites now route through this helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamOverrideRejection {
    /// Value and cell type disagree at the type-kind level (Int vs
    /// Scalar, Bool vs List, etc).
    TypeKindMismatch,
    /// Both sides are Scalar, but their dimensions disagree (e.g. a
    /// LENGTH override pushed against a MASS cell).
    ///
    /// Boxed for size hygiene — `DimensionVector` is `[Rational; 10]`
    /// (40 bytes each), which would push this variant to ~88 bytes
    /// unboxed.  Consistent with the directly-downstream
    /// `EngineError::DimensionMismatch` (task 2430 / lib.rs:50-54).
    ScalarDimensionMismatch {
        expected: Box<reify_types::dimension::DimensionVector>,
        got: Box<reify_types::dimension::DimensionVector>,
    },
}

/// Validate that `value` is a safe write into a cell of type `cell_type`.
///
/// Returns `Ok(())` if the override is compatible, or the corresponding
/// [`ParamOverrideRejection`] explaining the mismatch.  The guard chain
/// currently enforces:
/// 1. Type-kind match via `value_type_kind_matches` (rejects e.g. an Int
///    value into a Scalar cell).
/// 2. Scalar dimension match when both sides are Scalar (rejects e.g. a
///    LENGTH value into a MASS cell).
///
/// Any future refinement lands here and is picked up by every call site
/// automatically.
pub(crate) fn validate_param_override(
    value: &reify_types::Value,
    cell_type: &reify_types::Type,
) -> Result<(), ParamOverrideRejection> {
    if !crate::value_type_kind_matches(value, cell_type) {
        return Err(ParamOverrideRejection::TypeKindMismatch);
    }
    if let reify_types::Type::Scalar {
        dimension: expected,
    } = cell_type
        && let reify_types::Value::Scalar { dimension: got, .. } = value
        && *got != *expected
    {
        return Err(ParamOverrideRejection::ScalarDimensionMismatch {
            expected: Box::new(*expected),
            got: Box::new(*got),
        });
    }
    Ok(())
}

impl Engine {
    /// Maximum allowed value for [`set_max_unfold_depth`][Engine::set_max_unfold_depth].
    ///
    /// Caps the recursion depth in `unfold_recursive_sub` (which uses real, non-iterative
    /// recursion) to prevent stack overflow on pathological inputs. The default depth is 64;
    /// a cap of 512 leaves 8× headroom over typical real-world use while staying well below
    /// depths that would exhaust the stack in release builds.
    ///
    /// Exposed as a `pub const` so callers can query the limit programmatically
    /// (`Engine::MAX_UNFOLD_DEPTH_LIMIT`) rather than hard-coding the magic number.
    ///
    /// See task 205 (review) / task 424.
    pub const MAX_UNFOLD_DEPTH_LIMIT: usize = 512;

    /// Construct an Engine with a caller-supplied prelude slice.
    ///
    /// Use this when you need to:
    /// - Opt out of the stdlib entirely: pass `&[]`.
    /// - Inject a custom isolated prelude for unit tests.
    /// - Supply a prelude that differs from the embedded stdlib.
    ///
    /// `Engine::new` is the ergonomic shorthand for the common case and
    /// delegates to this constructor with `stdlib_loader::load_stdlib()`.
    pub fn with_prelude(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
        prelude: &'static [CompiledModule],
    ) -> Self {
        let prelude_functions: Vec<CompiledFunction> = prelude
            .iter()
            .flat_map(|pm| pm.functions.iter().cloned())
            .collect();
        Self {
            constraint_checker,
            geometry_kernel,
            solver: None,
            cache: CacheStore::new(),
            prelude,
            prelude_functions,
            param_overrides: std::collections::HashMap::new(),
            eval_state: None,
            demand: DemandRegistry::new(),
            next_snapshot_id: 0,
            next_version_id: 0,
            last_eval_set: Vec::new(),
            last_guard_phase_group_evals: 0,
            last_role_flip_probes: 0,
            last_diff_value_cells: None,
            last_param_override_type_kind_rejections: 0,
            last_param_override_dimension_rejections: 0,
            last_sub_component_unknown_structure_errors: 0,
            journal: EventJournal::new(),
            functions: Arc::new(Vec::new()),
            compiled_purposes: Vec::new(),
            active_purposes: HashMap::new(),
            active_objective_map: HashMap::new(),
            objectives: HashMap::new(),
            meta_map: Arc::new(HashMap::new()),
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
            optimization_registry: HashMap::new(),
            solvers: HashMap::new(),
        }
    }

    /// Construct an Engine with the embedded stdlib as its prelude.
    ///
    /// This is the standard constructor for production use. For tests that
    /// require an isolated or empty prelude, use `Engine::with_prelude`.
    pub fn new(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self::with_prelude(
            constraint_checker,
            geometry_kernel,
            reify_compiler::stdlib_loader::load_stdlib(),
        )
    }

    /// Register an optimized implementation for constraints annotated with
    /// `@optimized("target")` (Task 273).
    ///
    /// Constraints compiled from a `constraint def` that carried a matching
    /// annotation are routed to `imp` instead of the language-level
    /// `ConstraintChecker`. If no impl is registered for a target, the
    /// language-level checker handles those constraints unchanged.
    ///
    /// If an impl is already registered for `target`, it is silently
    /// overwritten and the previous impl is dropped. This matches `HashMap`
    /// insert semantics and is intentional to support hot-reload and test
    /// fixture scenarios where callers swap impls between runs.
    ///
    /// Note: this registry is only consulted from the *checker* path inside
    /// `dispatch_constraints`. The solver path (`Engine::resolve` / the
    /// `ConstraintSolver` seam) does not yet route through `OptimizedImpl`,
    /// so `@optimized` constraints participate in auto-param resolution via
    /// the ordinary language-level solver. See [`OptimizedImpl`].
    pub fn register_optimized_impl(
        &mut self,
        target: impl Into<String>,
        imp: Box<dyn OptimizedImpl>,
    ) {
        self.optimization_registry.insert(target.into(), imp);
    }

    /// Remove a previously registered optimized impl for `target`.
    ///
    /// Returns `true` if an impl was registered (and has now been dropped),
    /// `false` otherwise. Primarily intended for tests and hot-reload
    /// scenarios where callers need to swap impls between runs.
    pub fn unregister_optimized_impl(&mut self, target: &str) -> bool {
        self.optimization_registry.remove(target).is_some()
    }

    /// Iterate over the targets that currently have a registered optimized
    /// impl, in unspecified order. Primarily intended for diagnostics and
    /// test assertions ("was this target registered?").
    pub fn optimized_targets(&self) -> impl Iterator<Item = &str> {
        self.optimization_registry.keys().map(String::as_str)
    }

    /// Returns the compiled stdlib prelude modules stored by this engine.
    pub fn prelude(&self) -> &[CompiledModule] {
        self.prelude
    }

    /// Set the maximum depth for recursive sub-component unfolding.
    /// The default is 64. Lower values are useful for tests to keep execution fast.
    ///
    /// # Panics
    /// Panics if `depth == 0`. At depth 0 the guard check fires before any child entity
    /// is created, so parent let-bindings referencing `child.*` would silently resolve to
    /// Undef. Only values >= 1 are safe.
    ///
    /// Panics if `depth > Engine::MAX_UNFOLD_DEPTH_LIMIT` (currently 512). The recursive
    /// `unfold_recursive_sub` implementation uses real stack recursion; unbounded depths
    /// (e.g., 10 000) would risk stack overflow on deeply nested structures. The cap is
    /// enforced at the API boundary rather than inside the implementation so that the
    /// failure is immediate and explicit rather than a silent stack exhaust.
    pub fn set_max_unfold_depth(&mut self, depth: usize) {
        // Panic-on-misuse matches sibling `set_max_unfold_nodes`; see `# Panics` above.
        assert!(depth >= 1, "max_unfold_depth must be >= 1");
        assert!(
            depth <= Self::MAX_UNFOLD_DEPTH_LIMIT,
            "max_unfold_depth must be <= {}",
            Self::MAX_UNFOLD_DEPTH_LIMIT,
        );
        self.max_unfold_depth = depth;
    }

    /// Set the maximum total nodes created during recursive sub-component unfolding.
    /// The default is 10,000. This prevents exponential blowup when templates have
    /// multiple recursive subs (B subs × D depth = B^D nodes without this limit).
    ///
    /// # Panics
    /// Panics if `limit == 0`.
    pub fn set_max_unfold_nodes(&mut self, limit: usize) {
        assert!(limit >= 1, "max_unfold_nodes must be >= 1");
        self.max_unfold_nodes = limit;
    }

    /// Set the constraint solver for resolving auto parameters.
    pub fn with_solver(mut self, solver: Box<dyn ConstraintSolver>) -> Self {
        self.solver = Some(solver);
        self
    }

    /// Register a named constraint solver selectable via the `#solver(<name>)`
    /// module pragma (Task 2300).
    ///
    /// Modules whose `solver_pragma.name` matches `name` route their solver
    /// invocations to `solver` instead of the default `self.solver` set via
    /// `with_solver`. If `name` is not the value of any module's
    /// `#solver(<name>)`, the registered solver is never invoked.
    ///
    /// If a solver is already registered for `name`, it is silently overwritten
    /// and the previous solver is dropped. Mirrors `register_optimized_impl`'s
    /// `HashMap::insert` semantics; intentional to support hot-reload and test
    /// fixture scenarios where callers swap impls between runs.
    pub fn register_solver(
        &mut self,
        name: impl Into<String>,
        solver: Box<dyn ConstraintSolver>,
    ) {
        self.solvers.insert(name.into(), solver);
    }

    /// Remove a previously registered named solver. Returns `true` if a solver
    /// was registered (and has now been dropped), `false` otherwise. Mirrors
    /// `unregister_optimized_impl`.
    pub fn unregister_solver(&mut self, name: &str) -> bool {
        self.solvers.remove(name).is_some()
    }

    /// Iterate over the names that currently have a registered solver, in
    /// unspecified order. Primarily intended for diagnostics and test
    /// assertions ("was this back-end registered?"). Mirrors
    /// `optimized_targets`.
    pub fn registered_solvers(&self) -> impl Iterator<Item = &str> {
        self.solvers.keys().map(String::as_str)
    }

    /// Look up the constraint solver to use for `module` (Task 2300).
    ///
    /// Resolution policy:
    /// - `module.solver_pragma == None`: return `self.solver.as_deref()`.
    /// - `module.solver_pragma == Some({ name, .. })` and `name` is in
    ///   `self.solvers`: return the named solver.
    /// - `module.solver_pragma == Some({ name, .. })` and `name` is NOT in
    ///   `self.solvers`: return `self.solver.as_deref()` (default fallback;
    ///   may itself be `None` if `with_solver` was never called).
    ///
    /// This helper is the lookup-only counterpart of
    /// [`Engine::resolve_solver_for_module`]: it does NOT emit the
    /// "not registered" warning. It is intended for the inner solver-invocation
    /// expression where the `&self` borrow only needs to live for one
    /// statement (the `.solve(&problem)` call). The warning is emitted by
    /// `resolve_solver_for_module` once per resolution call, before the
    /// template loop.
    pub(crate) fn lookup_solver_for_module(
        &self,
        module: &CompiledModule,
    ) -> Option<&dyn ConstraintSolver> {
        match module.solver_pragma.as_ref() {
            None => self.solver.as_deref(),
            Some(p) => self
                .solvers
                .get(&p.name)
                .map(|b| b.as_ref() as &dyn ConstraintSolver)
                .or_else(|| self.solver.as_deref()),
        }
    }

    /// Resolve the constraint solver to use for `module`, emitting the
    /// "named back-end not registered" warning at most once (Task 2300).
    ///
    /// Called once per `eval()` / `eval_cached()` invocation before the
    /// template loop so the warning is emitted at most once even when the
    /// module contains many auto-param templates that would otherwise iterate
    /// the solver in lock-step. The returned `Option<&dyn ConstraintSolver>`
    /// can be used directly, but callers iterating over templates that mutate
    /// `&mut self` between solver calls should use
    /// [`Engine::lookup_solver_for_module`] inside the inner loop and rely on
    /// this method only for the one-shot warning + overall availability check.
    ///
    /// Mirrors the design decision: "Encapsulate the eval/eval_cached solver
    /// lookup in a single helper".
    pub(crate) fn resolve_solver_for_module(
        &self,
        module: &CompiledModule,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<&dyn ConstraintSolver> {
        if let Some(p) = module.solver_pragma.as_ref()
            && !self.solvers.contains_key(&p.name)
        {
            diagnostics.push(Diagnostic::warning(format!(
                "#solver: named back-end '{}' is not registered; falling back to default solver",
                p.name
            )));
        }
        self.lookup_solver_for_module(module)
    }

    /// Access the cache store (for testing/inspection).
    pub fn cache_store(&self) -> &CacheStore {
        &self.cache
    }

    /// Whether the engine has been initialized by a call to eval().
    pub fn is_initialized(&self) -> bool {
        self.eval_state.is_some()
    }

    /// Access the consolidated evaluation state (for testing/inspection).
    pub fn eval_state(&self) -> Option<&EvaluationState> {
        self.eval_state.as_ref()
    }

    /// Access the current snapshot (for testing/inspection).
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.eval_state.as_ref().map(|s| &s.snapshot)
    }

    /// Clear all param overrides currently held by this engine.
    ///
    /// Intended for callers that semantically start fresh with respect to
    /// user edits — e.g. the CLI's `load_file` / `open_file` when the user
    /// opens a new source file and any overrides from the previous file
    /// should no longer apply.
    ///
    /// This method:
    /// - wipes every entry from `self.param_overrides`,
    /// - does NOT invalidate the cache — the next call to `eval()` rebuilds
    ///   the snapshot from the module defaults anyway (and the per-eval
    ///   purge step would drop the entries on its own once the module
    ///   changes, but this primitive makes the reset explicit for
    ///   topology-preserving reloads),
    /// - does NOT touch `eval_state`, `snapshot`, `cache`, or `journal`.
    ///
    /// Distinct from `set_param_and_invalidate` (which writes a single
    /// override) — the "clear" intent warrants its own entry point rather
    /// than being smuggled in as a sentinel value.
    pub fn clear_param_overrides(&mut self) {
        self.param_overrides.clear();
    }

    /// Retain only those entries in `self.param_overrides` whose target
    /// value cell still exists in `graph` and is currently a Param-kind
    /// cell. Drops entries whose cell disappeared from the module, whose
    /// kind changed from Param to Let/Auto, or which never existed.
    ///
    /// The zombie-resurrect scenario this prevents: a user sets an
    /// override on `S.width`, then edits the source to remove `width`,
    /// then later re-adds a cell with the same
    /// `ValueCellId::new("S", "width")`.  Without purging, the dormant
    /// override from the first edit would silently reapply in the third.
    ///
    /// Called by `Engine::eval` once the new snapshot graph has been
    /// materialised. `Engine::edit_source` performs an equivalent purge
    /// via an inline `self.param_overrides.retain(...)` against its
    /// post-edit graph; a follow-up task will migrate that site onto
    /// this helper (the amend-pass scope for task 2017 did not include
    /// `engine_edit.rs`).  Until that merge lands the two predicates
    /// must remain behaviourally identical — if you refine one, refine
    /// the other.
    pub(crate) fn prune_param_overrides_against(
        &mut self,
        graph: &crate::graph::EvaluationGraph,
    ) {
        self.param_overrides.retain(|id, _| {
            graph
                .value_cells
                .get(id)
                .map(|node| matches!(node.kind, ValueCellKind::Param))
                .unwrap_or(false)
        });
    }

    /// Access the eval set from the last eval() or edit_param() call.
    pub fn last_eval_set(&self) -> &[NodeId] {
        &self.last_eval_set
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Count of non-skipped guarded-group iterations across Phase 1 and Phase 3
    /// of the most recent `edit_source` or `edit_param` call.
    ///
    /// Resets to 0 at the top of each `edit_source` / `edit_param` invocation.
    /// A non-skipped iteration is one where the group's guard value actually
    /// changed vs. the pre-edit snapshot (or, in edit_source Phase 1, a group
    /// that has newly-added members or a role-flipped guard member). A group
    /// re-elaborated by Phase 1 is NOT counted again in Phase 3 — the
    /// cross-phase dedup set (`phase1_reelaborated`) skips it before the
    /// counter is incremented (edit_param: task 2140; edit_source: task 2142).
    /// Used by tests to assert that the per-group skip is working correctly.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method by adding a self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_guard_phase_group_evals(&self) -> usize {
        self.last_guard_phase_group_evals
    }

    /// Returns the number of times `detect_role_flip` was invoked during the
    /// most recent `edit_source` call. Reset to 0 at the start of each
    /// `edit_source` call.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_role_flip_probes(&self) -> usize {
        self.last_role_flip_probes
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Returns a reference to the `(changed, added, removed)` triple captured
    /// from the `diff_value_cells` call inside the most recent `edit_source`
    /// invocation, **or `None` if no `edit_source` has been called yet on this
    /// `Engine` or if a subsequent `edit_param` has cleared the snapshot** (the
    /// "most recent edit_source" invariant is enforced by a cfg-gated reset at
    /// the top of `edit_param`, not just documented).
    ///
    /// Canonical use case: T3 premise lock
    /// (`edit_source_role_flipped_member_in_unchanged_guard_group_forces_non_skip`)
    /// asserts that the role-flipped cells (`S.x`, `S.y`) are absent from all
    /// three sets, confirming that `ValueCellNode::content_hash` does not
    /// incorporate the member/else_member role (task 2170). If that premise is
    /// violated, T3's counter assertion would keep passing for the wrong reason
    /// (silent test-drift).
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_diff_value_cells(
        &self,
    ) -> Option<&crate::engine_edit::ValueCellDiff> {
        self.last_diff_value_cells.as_ref()
    }

    /// Returns the number of param-override rejections due to `TypeKindMismatch`
    /// during the most recent `eval()` or `eval_cached()` call. Reset to 0 at
    /// the start of each call.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_param_override_type_kind_rejections(&self) -> usize {
        self.last_param_override_type_kind_rejections
    }

    /// Returns the number of param-override rejections due to `ScalarDimensionMismatch`
    /// during the most recent `eval()` or `eval_cached()` call. Reset to 0 at
    /// the start of each call.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_param_override_dimension_rejections(&self) -> usize {
        self.last_param_override_dimension_rejections
    }

    /// Returns the number of sub-component elaboration errors due to an unknown
    /// structure reference during the most recent `eval()` or `eval_cached()`
    /// call. Reset to 0 at the start of each call.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_sub_component_unknown_structure_errors(&self) -> usize {
        self.last_sub_component_unknown_structure_errors
    }

    /// Access the event journal (for testing/inspection).
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::ParamOverrideRejection;
    use crate::Engine;

    // Pin that `ParamOverrideRejection` fits within 32 bytes.
    // See `ParamOverrideRejection::ScalarDimensionMismatch` doc for rationale.
    #[test]
    fn param_override_rejection_max_variant_is_small() {
        assert!(
            std::mem::size_of::<ParamOverrideRejection>() <= 32,
            "ParamOverrideRejection is {} bytes; expected <= 32. \
             Box the DimensionVector fields in ScalarDimensionMismatch.",
            std::mem::size_of::<ParamOverrideRejection>()
        );
    }

    /// `register_solver` stores a named solver in the registry; `unregister_solver`
    /// returns `true` when a matching name was registered (and removes it) and
    /// `false` otherwise. `registered_solvers()` iterates over the currently
    /// registered names. Mirrors the optimized-impl registry contract
    /// (`register_optimized_impl` / `unregister_optimized_impl` /
    /// `optimized_targets`) for the named-solver dispatch added by Task 2300.
    #[test]
    fn register_solver_stores_named_solver_and_unregister_returns_true_when_present() {
        use reify_test_support::mocks::{MockConstraintChecker, SpyConstraintSolver};
        use std::collections::HashMap;
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_solver(
            "libslvs",
            Box::new(SpyConstraintSolver::new_solved(HashMap::new())),
        );
        assert!(
            engine.registered_solvers().any(|n| n == "libslvs"),
            "expected 'libslvs' in registered_solvers(), got {:?}",
            engine.registered_solvers().collect::<Vec<_>>()
        );

        // Unregister returns true and the name is no longer registered.
        assert!(
            engine.unregister_solver("libslvs"),
            "expected unregister_solver('libslvs') to return true"
        );
        assert_eq!(
            engine.registered_solvers().count(),
            0,
            "expected registered_solvers() to be empty after unregister"
        );

        // Unregister of a missing name returns false.
        assert!(
            !engine.unregister_solver("missing"),
            "expected unregister_solver('missing') to return false"
        );
    }
}
