// Split from lib.rs (task 2032) — admin methods.

use crate::cache::{CacheStore, NodeId};
use crate::demand::DemandRegistry;
use crate::journal::EventJournal;
use crate::snapshot::Snapshot;
use crate::{Engine, EvaluationState};
use reify_compiler::CompiledModule;
use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintSolver, GeometryKernel, OptimizedImpl,
};
use std::collections::HashMap;

impl Engine {
    pub fn new(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        let prelude = reify_compiler::stdlib_loader::load_stdlib();
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
            journal: EventJournal::new(),
            functions: Vec::new(),
            compiled_purposes: Vec::new(),
            active_purposes: HashMap::new(),
            active_objective_map: HashMap::new(),
            objectives: HashMap::new(),
            meta_map: HashMap::new(),
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
            optimization_registry: HashMap::new(),
        }
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

    /// Returns the number of functions currently loaded in the engine's
    /// combined function table (user + prelude). Used by integration tests
    /// to assert that repeated `eval()` calls do not accumulate entries.
    #[doc(hidden)]
    pub fn functions_count(&self) -> usize {
        self.functions.len()
    }

    /// Set the maximum depth for recursive sub-component unfolding.
    /// The default is 64. Lower values are useful for tests to keep execution fast.
    ///
    /// # Panics
    /// Panics if `depth == 0`. At depth 0 the guard check fires before any child entity
    /// is created, so parent let-bindings referencing `child.*` would silently resolve to
    /// Undef. Only values >= 1 are safe.
    pub fn set_max_unfold_depth(&mut self, depth: usize) {
        assert!(depth >= 1, "max_unfold_depth must be >= 1");
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

    /// Access the eval set from the last eval() or edit_param() call.
    pub fn last_eval_set(&self) -> &[NodeId] {
        &self.last_eval_set
    }

    /// Access the event journal (for testing/inspection).
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }
}
