// Split from lib.rs (task 2032) — admin methods.

use crate::cache::{CacheStore, NodeId};
use crate::demand::DemandRegistry;
use crate::journal::EventJournal;
use crate::snapshot::Snapshot;
use crate::{Engine, EvaluationState};
use reify_compiler::{CompiledModule, EntityKind, ValueCellKind};
use reify_types::{
    CompiledFunction, ConstraintChecker, ConstraintSolver, Diagnostic, FeatureTagTable,
    GeometryKernel, OptimizedImpl, TopologyAttributeTable,
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
/// Returns `true` iff at least one template in `module` declares at least one
/// auto-param value cell (`ValueCellKind::Auto { .. }`).
///
/// Used by `resolve_solver_for_module` to suppress the "named back-end not
/// registered, falling back to default solver" warning on modules that would
/// never invoke the solver anyway: such modules see `build_solver_problem`
/// return `None` for every template and the warning would refer to a fallback
/// that never happens. This keeps `#solver(<name>)` usable as a forward-
/// compatible declaration on auto-param-free modules without surfacing noise.
fn module_has_auto_cells(module: &CompiledModule) -> bool {
    module
        .templates
        .iter()
        .any(|t| t.value_cells.iter().any(|c| c.kind.is_auto()))
}

/// Walk every `EntityKind::Structure` template in `modules` and intern a
/// [`reify_types::StructureMeta`] per template into `registry` (task 3540 /
/// SIR-α step-12).
///
/// `Occurrence` templates (instances) are skipped — only `structure def`-kind
/// declarations back the `Value::StructureInstance` side-table. `intern` is
/// idempotent on the structure name: an already-interned name keeps its
/// `StructureTypeId` stable and only its `StructureMeta` is overwritten, so
/// calling this at construction (prelude) and again per `eval()` (user module)
/// is a safe incremental refresh.
///
/// `version` is fixed at `1` here; the `@version(N)` annotation read-side
/// (task 3540 / step-14) will source it from `TopologyTemplate.version` once
/// that field exists. `source` is `None` (templates carry no single decl
/// span; per-cell spans live on `value_cells`).
pub(crate) fn populate_structure_registry(
    registry: &mut reify_types::StructureRegistry,
    modules: &[CompiledModule],
) {
    for module in modules {
        for tmpl in &module.templates {
            if tmpl.entity_kind != EntityKind::Structure {
                continue;
            }
            let field_layout: Vec<(String, reify_types::Type)> = tmpl
                .value_cells
                .iter()
                .filter(|c| matches!(c.kind, ValueCellKind::Param))
                .map(|c| (c.id.member.clone(), c.cell_type.clone()))
                .collect();
            registry.intern(
                &tmpl.name,
                reify_types::StructureMeta {
                    name: tmpl.name.clone(),
                    version: 1,
                    declared_trait_bounds: tmpl.trait_bounds.clone(),
                    source: None,
                    field_layout,
                },
            );
        }
    }
}

pub(crate) fn validate_param_override(
    value: &reify_types::Value,
    cell_type: &reify_types::Type,
) -> Result<(), ParamOverrideRejection> {
    // `registry: None` for now — param-override validation does not yet
    // consult the per-Engine structure side-table. Trait-bound conformance
    // for `Value::StructureInstance` is proven at compile time; the registry
    // is plumbed into the eval path in a later step (3540 / SIR-α step-12).
    if !crate::value_type_kind_matches(value, cell_type, None) {
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
        // Seed the structure side-table from the prelude's `structure def`
        // templates (task 3540 / SIR-α step-12). Refreshed incrementally per
        // `eval()` from the user module's templates.
        let mut structure_registry = reify_types::StructureRegistry::new();
        populate_structure_registry(&mut structure_registry, prelude);
        Self {
            constraint_checker,
            geometry_kernel,
            solver: None,
            cache: CacheStore::new(),
            prelude,
            prelude_functions,
            structure_registry,
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
            functions: Vec::<CompiledFunction>::new().into(),
            compiled_purposes: Vec::new(),
            active_purposes: HashMap::new(),
            active_purpose_bindings: HashMap::new(),
            active_tolerance_scope: HashMap::new(),
            active_objective_map: HashMap::new(),
            objectives: HashMap::new(),
            compiled_fields: Arc::new(Vec::new()),
            meta_map: Arc::new(HashMap::new()),
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
            optimization_registry: HashMap::new(),
            compute_registry: crate::engine_compute::ComputeDispatchRegistry::new(),
            solvers: HashMap::new(),
            // Read REIFY_WARM_STATE_BUDGET_BYTES once at construction; falls
            // back to DEFAULT_BUDGET_BYTES (2 GiB) when unset. Per arch §4.3.
            warm_pool: crate::warm_pool::WarmStatePool::from_env_or_default(),
            feature_tag_table: FeatureTagTable::default(),
            // v0.2 persistent-naming-v2 attribute store. Always empty after
            // construction — task 2590 added the field + accessor as the
            // foundation; tasks 5-8 wire per-op auto-population.
            topology_attribute_table: TopologyAttributeTable::default(),
            // Phase A swept-body classifier table (task 2982). Always empty
            // after construction; populated per-realization by
            // `Engine::execute_realization_ops` and cleared at every build
            // entry point.
            swept_kind_table: crate::sweep_classifier::SweptKindTable::default(),
            // Empty realization cache (task 2874). Populated by
            // `execute_realization_ops` after fully-successful realizations
            // when a demanded tolerance is available; consulted at the start
            // of the helper to short-circuit kernel re-execution when a
            // cached handle satisfies the request under the partial-order
            // rule.
            realization_cache: crate::realization_cache::RealizationCache::new(),
            // Only initialised in test / `test-instrumentation` builds; the
            // field is absent in production (see lib.rs and engine_eval.rs
            // for the matching cfg gates on the declaration and read site).
            #[cfg(any(test, feature = "test-instrumentation"))]
            panic_on_eval_cells: std::collections::HashSet::new(),
        }
    }

    /// Return a reference to the feature-tag table populated by the most recent
    /// `build()` or `build_snapshot()` call.
    ///
    /// Maps each `GeometryHandleId` produced during geometry execution to the
    /// `FeatureTag` derived from its position in the parallel `feature_tags`
    /// array on `RealizationDecl`. See task 2323 for full design rationale.
    pub fn feature_tag_table(&self) -> &FeatureTagTable {
        &self.feature_tag_table
    }

    /// Return a reference to the v0.2 persistent-naming-v2 attribute table on
    /// this engine.
    ///
    /// Maps each `GeometryHandleId` to a `TopologyAttribute` record
    /// (`feature_id`, `role`, `local_index`, optional `user_label`,
    /// `mod_history`). Per the task-2590 plan, the table is always empty
    /// after construction; tasks 5-8 wire per-op auto-population during
    /// `execute_realization_ops`, and task 2 (#2570) wires selector lookup
    /// against this table. Once the attribute path covers all selector
    /// vocabulary (tasks 9-10), `feature_tag_table` is retired and this
    /// becomes the only naming store.
    pub fn topology_attribute_table(&self) -> &TopologyAttributeTable {
        &self.topology_attribute_table
    }

    /// Return a reference to the Phase A swept-body classifier table populated
    /// by the most recent `build()` / `build_snapshot()` /
    /// `tessellate_realizations()` / `tessellate_snapshot()` call.
    ///
    /// Maps each successful realization's final `GeometryHandleId` to the
    /// `SweptKind` returned by `classify_swept_body(ops, handles)` — see
    /// `crates/reify-eval/src/sweep_classifier.rs` for the classifier surface
    /// and Phase A acceptance matrix. Realizations whose final op is not a
    /// recognised swept body (or whose sweep path is curved/twisted) leave no
    /// entry. Task 2982.
    pub fn swept_kind_table(&self) -> &crate::sweep_classifier::SweptKindTable {
        &self.swept_kind_table
    }

    /// **Test-instrumentation only — not a stable public surface.**
    ///
    /// Immutable access to the per-Engine [`reify_types::StructureRegistry`]
    /// seeded from the prelude at construction and refreshed per `eval()`
    /// from the user module's `structure def` templates (task 3540 / SIR-α
    /// step-12). Tests assert that a known prelude structure (e.g.
    /// `Steel_AISI_1045`) is interned with its declared trait bounds, default
    /// version, and declaration-order field layout, and that unknown names
    /// resolve to `None`.
    ///
    /// Mirrors the cfg-gating of [`realization_cache`](Self::realization_cache):
    /// `StructureTypeId`s are per-Engine ephemeral handles, so exposing the
    /// table in production builds would leak an internal id space into the
    /// public surface. Only available under
    /// `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn structure_registry(&self) -> &reify_types::StructureRegistry {
        &self.structure_registry
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Immutable access to the per-engine [`RealizationCache`] populated by
    /// `execute_realization_ops` after fully-successful realizations whose
    /// demanded tolerance is known. Tests use this accessor to assert that
    /// `(entity_id, ReprKind::BRep, demanded_tol)` lookups return the
    /// expected cached `GeometryHandleId` after `build()` /
    /// `build_snapshot()` / `tessellate_realizations()` runs.
    ///
    /// Mirrors the cfg-gating pattern used by [`cache_store`](Self::cache_store):
    /// the cache stores kernel-internal `GeometryHandleId` values, so exposing
    /// the accessor in production builds would leak kernel implementation
    /// detail into the public surface. Task 2874 (initial cache wiring)
    /// adds the read-only test seam; broader cache invalidation control
    /// surfaces are deferred to a follow-up.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn realization_cache(
        &self,
    ) -> &crate::realization_cache::RealizationCache<reify_types::GeometryHandleId> {
        &self.realization_cache
    }

    /// Flush the per-engine [`RealizationCache`](crate::realization_cache::RealizationCache),
    /// dropping every cached `(entity_id, repr_kind, demanded_tol) →
    /// GeometryHandleId` entry.
    ///
    /// **Production escape hatch (task 2874, step-22).** Gives callers
    /// explicit control over cache invalidation when they know an external
    /// event has rendered cached `GeometryHandleId`s stale — for example,
    /// swapping the geometry kernel via test seams, or an upstream module
    /// reload that did not flow through `edit_source` (e.g. a CLI workflow
    /// that constructs a fresh `CompiledModule` and feeds it in via a
    /// non-`edit_source` path).
    ///
    /// **Most callers do NOT need to call this manually.** Both
    /// [`Engine::edit_param`](crate::Engine::edit_param) and
    /// [`Engine::edit_source`](crate::Engine::edit_source) already invoke
    /// the same reset internally near function entry (the auto-invalidation
    /// hook points pinned by tests
    /// `edit_param_clears_realization_cache_to_prevent_stale_handle_on_subsequent_build_snapshot`
    /// and `edit_source_clears_realization_cache_to_prevent_stale_handle_on_subsequent_build`
    /// in `tests/tolerance_wiring_e2e.rs`). This method is the escape hatch
    /// for scenarios that fall OUTSIDE those hook points; it is NOT a
    /// required pre-`build_snapshot` step.
    ///
    /// **Shape**: takes `&mut self`, returns nothing, idempotent on an
    /// already-empty cache. Mirrors the precedent set by
    /// [`Engine::clear_param_overrides`](Self::clear_param_overrides) — no
    /// cfg gate, no return value, single-purpose mutator. The READ-side
    /// accessor [`Engine::realization_cache`](Self::realization_cache)
    /// keeps its `#[cfg(any(test, feature = "test-instrumentation"))]`
    /// gate (cache contents are kernel-internal `GeometryHandleId` values
    /// that should not leak into the production surface), but the
    /// WRITE-side mutator is unconditionally available so production
    /// callers can satisfy the cache-invalidation contract that the
    /// `Engine::realization_cache` field's docstring documents.
    ///
    /// **What this method does NOT touch**: `eval_state`, `snapshot`,
    /// `cache`, `journal`, `feature_tag_table`, `topology_attribute_table`,
    /// `param_overrides`, registered solvers/kernels, or any other engine
    /// state. The reset is single-purpose: only `realization_cache` is
    /// reseat to a fresh [`RealizationCache::new()`](crate::realization_cache::RealizationCache::new).
    ///
    /// Pinned by `clear_realization_cache_public_api_resets_cache_for_production_callers`
    /// in `tests/tolerance_wiring_e2e.rs`.
    pub fn clear_realization_cache(&mut self) {
        self.realization_cache = crate::realization_cache::RealizationCache::new();
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

    /// Construct an Engine using the inventory-driven multi-kernel registry
    /// (v0.2 entry point per `docs/prds/v0_2/multi-kernel.md` "Resolved
    /// design decisions").
    ///
    /// Reads the static linker-collected set of [`reify_types::KernelRegistration`] records
    /// once at startup, picks the **BRep-preferring lex-smallest** entry (see
    /// [`crate::kernel_registry::pick_lexmin_brep_kernel`]), invokes its
    /// `factory` to instantiate a [`GeometryKernel`], and forwards to
    /// [`Engine::with_prelude`] with the embedded stdlib.
    ///
    /// # v0.2 single-kernel scope
    ///
    /// In v0.2 OCCT is the only adapter that submits a registration (gated on
    /// `cfg(has_occt)` in `crates/reify-kernel-occt/src/register.rs`), so the
    /// BRep-preferring pick is unambiguous. In v0.3 once additional adapters
    /// (`reify-kernel-manifold`, `-fidget`, `-openvdb`) ship, the per-op
    /// dispatch decision moves into [`crate::dispatcher::dispatch`] — which
    /// already accepts a `&BTreeMap<String, &CapabilityDescriptor>`
    /// borrowed from `collect_registry()`'s output — and this constructor
    /// will become a multi-kernel engine builder rather than a startup-time
    /// single-kernel picker.
    ///
    /// # Empty-registry semantics
    ///
    /// When no adapter has submitted a registration (e.g. stub-mode build with
    /// `cfg(has_occt)` off, or a binary that links no `reify-kernel-*` crate
    /// at all), `kernel` is forwarded as `None`, matching `Engine::new(checker,
    /// None)`. The existing build-path error surface ("no geometry kernel
    /// registered") fires cleanly without a non-functional stub kernel.
    ///
    /// # BRep-preferring picker (task 3224)
    ///
    /// This constructor uses [`crate::kernel_registry::pick_lexmin_brep_kernel`]
    /// rather than the pure [`crate::kernel_registry::pick_lexmin_kernel`].  The
    /// distinction matters if a binary ever links both `reify-kernel-manifold`
    /// (Mesh-only stub; name `"manifold"`) and `reify-kernel-occt` (full BRep;
    /// name `"occt"`): `"manifold" < "occt"` in ASCII order, so a pure lex-min
    /// pick would silently route every BRep op through the Manifold stub which
    /// returns `OperationFailed`.  The BRep-preferring picker avoids this by
    /// filtering for BRep-capable kernels first.
    ///
    /// When no registered kernel claims any BRep pair (e.g. a hypothetical
    /// Mesh-only build), the picker falls back to pure lex-min — a Mesh-only
    /// binary still wants *some* kernel.  Empty-registry semantics are
    /// unchanged: `None` is returned and forwarded to `Engine::with_prelude`
    /// as before.
    ///
    /// # Why additive (not a replacement)
    ///
    /// `Engine::new` and `Engine::with_prelude` remain unchanged: ~70
    /// integration tests across `crates/reify-eval/tests/` and the
    /// `reify-cli` / `gui-tauri` binaries pass kernels they constructed
    /// themselves (e.g. `MockGeometryKernel`, `SingleKernelHolder`). The
    /// CLI/GUI call sites now use `with_registered_kernel` — see task 2808.
    ///
    /// # Operator visibility
    ///
    /// A structured tracing event is emitted after the pick; see
    /// [`crate::kernel_registry::emit_kernel_selection`] for the level-selection
    /// contract. The event fires only when a [`tracing::Subscriber`] is installed,
    /// so bare tests and binaries that install no subscriber are unaffected.
    pub fn with_registered_kernel(constraint_checker: Box<dyn ConstraintChecker>) -> Self {
        // BRep-preferring lex-min picker (task 3224): uses pick_lexmin_brep_kernel
        // rather than pick_lexmin_kernel so a Mesh-only kernel registered under a
        // lex-smaller name (e.g. "manifold" < "occt") cannot silently win the pick
        // when a BRep-capable kernel is also registered. The fallback to pure lex-min
        // (when no entry claims a BRep pair) preserves Mesh-only-binary semantics.
        // The helper reads the OnceLock-memoized registry(), so the inventory walk
        // happens at most once per process even if other call paths also hit it.
        let picked = crate::kernel_registry::pick_lexmin_brep_kernel();
        if let Some(reg) = picked {
            let total = crate::kernel_registry::registry().len();
            // Same `&'static` map as pick_lexmin_brep_kernel saw — registry() is
            // OnceLock-memoized, so the count cannot disagree with the pick.
            crate::kernel_registry::emit_kernel_selection(reg.name, total);
        }
        let kernel: Option<Box<dyn GeometryKernel>> = picked.map(|reg| (reg.factory)());
        Self::with_prelude(
            constraint_checker,
            kernel,
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

    // ── Compute-trampoline registry (task γ / 3422) ─────────────────────────

    /// Register a compute trampoline for `target`.
    ///
    /// When the value-cell eval loop encounters a `UserFunctionCall` whose
    /// `CompiledFunction.optimized_target == Some(target)`, it inserts a
    /// `ComputeNode` into the evaluation graph and invokes `f` synchronously
    /// instead of body-inlining the function.
    ///
    /// `target` must be a `&'static str` (a string literal); this mirrors the
    /// zero-allocation design of the dispatch registry itself.
    ///
    /// # Panics
    ///
    /// Panics if a trampoline is already registered for `target`, naming the
    /// duplicated target in the panic message (PRD §4 hard-error contract).
    /// Silent overwrite would mask accidental double-registration by two
    /// independent crates claiming the same target string.
    ///
    /// See `docs/prds/v0_3/compute-node-contract.md` §4.
    pub fn register_compute_fn(
        &mut self,
        target: &'static str,
        f: crate::engine_compute::ComputeFn,
    ) {
        use std::collections::hash_map::Entry;
        match self.compute_registry.fns.entry(target) {
            Entry::Vacant(v) => {
                v.insert(f);
            }
            Entry::Occupied(_) => {
                panic!(
                    "register_compute_fn: duplicate target {:?} — \
                     a ComputeFn is already registered for this target",
                    target
                );
            }
        }
    }

    /// Look up the [`ComputeFn`][crate::engine_compute::ComputeFn] registered
    /// for `target`.
    ///
    /// Returns `Some(f)` if a trampoline was previously registered via
    /// [`register_compute_fn`][Self::register_compute_fn], `None` otherwise.
    /// The returned value is a plain function pointer and therefore `Copy`.
    ///
    /// See `docs/prds/v0_3/compute-node-contract.md` §4.
    pub fn compute_dispatch(
        &self,
        target: &str,
    ) -> Option<crate::engine_compute::ComputeFn> {
        self.compute_registry.fns.get(target).copied()
    }

    /// Synchronous dispatch helper — invoke the trampoline registered for
    /// `target` with the provided inputs and return the result or diagnostics.
    ///
    /// # Arguments
    ///
    /// - `target`: the `@optimized("…")` target string to dispatch to
    /// - `value_inputs`: resolved `Value` inputs for this invocation
    /// - `realization_inputs`: resolved geometry inputs (read-only handles)
    /// - `options`: per-invocation option map (`Value::Map` or `Value::Undef`)
    /// - `prior_warm_state`: warm-start state from a previous invocation, if any
    ///
    /// A fresh [`CancellationHandle`][crate::engine_compute::CancellationHandle]
    /// is created per invocation (pending/cancel lifecycle is deferred to δ/ε).
    ///
    /// # Returns
    ///
    /// - `Ok((result, diagnostics))` — trampoline returned `Completed`
    /// - `Err(diagnostics)` — target unregistered, `Failed`, or `Cancelled`
    ///   (in each case at least one `Severity::Error` diagnostic is present)
    ///
    /// See `docs/prds/v0_3/compute-node-contract.md` §4 and task γ (3422).
    pub fn dispatch_compute_node(
        &self,
        target: &str,
        value_inputs: &[reify_types::Value],
        realization_inputs: &[crate::engine_compute::RealizationReadHandle],
        options: &reify_types::Value,
        prior_warm_state: Option<&reify_types::OpaqueState>,
    ) -> Result<(reify_types::Value, Vec<reify_types::Diagnostic>), Vec<reify_types::Diagnostic>>
    {
        use crate::engine_compute::ComputeOutcome;
        use crate::graph::CancellationHandle;

        match self.compute_registry.fns.get(target).copied() {
            Some(f) => {
                let handle = CancellationHandle::new();
                match f(
                    value_inputs,
                    realization_inputs,
                    options,
                    prior_warm_state,
                    &handle,
                ) {
                    ComputeOutcome::Completed {
                        result,
                        diagnostics,
                        ..
                    } => Ok((result, diagnostics)),
                    ComputeOutcome::Failed { diagnostics } => Err(diagnostics),
                    ComputeOutcome::Cancelled => Err(vec![reify_types::Diagnostic::error(
                        format!(
                            "@optimized target {:?}: compute trampoline was cancelled",
                            target
                        ),
                    )]),
                }
            }
            // The "(falling back to body-inlining)" clause is intentionally
            // omitted here: fallback is the eval-loop caller's behaviour, not
            // this helper's. Direct callers of `dispatch_compute_node`
            // (including the unit tests in this crate) do NOT body-inline,
            // so the diagnostic would be misleading. The lowering site in
            // `engine_eval.rs` emits its own diagnostic that DOES mention
            // body-inline fallback, where that wording is accurate.
            None => Err(vec![reify_types::Diagnostic::error(format!(
                "@optimized target {:?}: no registered compute trampoline",
                target
            ))]),
        }
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
    pub fn register_solver(&mut self, name: impl Into<String>, solver: Box<dyn ConstraintSolver>) {
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
                .or(self.solver.as_deref()),
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
    /// The warning is suppressed when the module has zero auto-param cells
    /// across all templates: such a module never invokes the solver, so a
    /// "falling back" warning would be misleading (no solver was ever
    /// consulted). This lets users write `#solver(libslvs)` purely as a
    /// forward-compatible declaration on a module without any auto params
    /// without surfacing a noisy diagnostic.
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
            && module_has_auto_cells(module)
        {
            diagnostics.push(Diagnostic::warning(format!(
                "#solver: named back-end '{}' is not registered; falling back to default solver",
                p.name
            )));
        }
        self.lookup_solver_for_module(module)
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Immutable access to the cache store, used by integration tests that need
    /// to inspect cache state after evaluation.  Mirrors the gating on the
    /// mutable [`cache_store_mut`](Self::cache_store_mut) accessor.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
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
    pub(crate) fn prune_param_overrides_against(&mut self, graph: &crate::graph::EvaluationGraph) {
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
    pub fn last_diff_value_cells(&self) -> Option<&crate::engine_edit::ValueCellDiff> {
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

    /// Return the current [`Freshness`] of `node` from the engine's cache.
    ///
    /// This is the **stable, always-public** read path that GUI and LSP
    /// consumers use to surface computation state without reaching into the
    /// test-instrumentation-gated [`cache_store()`](Self::cache_store).
    ///
    /// Mirrors the design of [`Engine::journal()`] — always public, no cfg gate.
    /// See arch §7.1 lines 716-728 and the task #2337 design decision on
    /// "Add a public `Engine::freshness` accessor on the Engine facade".
    ///
    /// Returns [`Freshness::Final`] (the default) when `node` has no cache
    /// entry — identical to [`CacheStore::freshness`]'s own default behaviour,
    /// so that "unknown = Final" is enforced in one place and callers never
    /// need to handle a missing-node error path.
    pub fn freshness(&self, node: &NodeId) -> reify_types::Freshness {
        self.cache.freshness(node)
    }

    /// Return the chain-root [`NodeId`] that caused `node` to be Pending, if
    /// one has been recorded.
    ///
    /// This is the **stable, always-public** read path that GUI and LSP
    /// consumers use to identify the originating failed cell without reaching
    /// into the test-instrumentation-gated [`cache_store()`](Self::cache_store).
    ///
    /// Mirrors the design of [`Engine::freshness()`] — always public, no cfg
    /// gate. See arch §9.2 lines 880-890 and the `pending_cause` side-table
    /// contract at `cache.rs:147-156`.
    ///
    /// The returned `NodeId`, when `Some`, may be any of the valid chain-root
    /// variants (per `docs/prds/v0_3/compute-node-contract.md §3`):
    ///
    /// - `NodeId::Compute(_)` — an **in-flight ComputeNode** is itself the
    ///   chain root (PRD §3 "Chain-root contract extension"). UI tooling
    ///   should render this as "computing" (recomputation in flight).
    /// - `NodeId::Value(_)` — an **upstream Failed leaf** gated the downstream
    ///   cell (existing behaviour). UI tooling should render this as "waiting
    ///   on upstream error".
    /// - `None` — the node is the originating cause (chain root) or has no
    ///   recorded cause at all.
    ///
    /// Returns `None` in three cases:
    /// - `node` has no cache entry (unknown node; identical to
    ///   [`CacheStore::pending_cause`]'s "default to None" behaviour).
    /// - `node` is a Failed root — Failed entries do NOT record a cause
    ///   because they are themselves the chain root, not forwarders.
    /// - `node`'s cache entry was written via the bulk `mark_pending` path
    ///   (cache.rs:482-513) that intentionally omits a cause.
    pub fn pending_cause(&self, node: &NodeId) -> Option<NodeId> {
        self.cache.pending_cause(node)
    }

    /// Drive a freshness-only propagation sweep from the supplied changed
    /// ValueCells. This is the engine's production trigger surface for the
    /// `freshness_walk::propagate_freshness_only` walk (arch §3.5 lines
    /// 432-436): when upstream cells flip Pending/Intermediate → Final (or
    /// any other freshness transition) WITHOUT a value change, callers
    /// invoke this method so the freshness transition propagates through
    /// the reverse-dependency graph WITHOUT firing any value evaluator.
    ///
    /// Intended consumers: a future kernel-completion handler that flips a
    /// Compute or Realization node's freshness, GUI/LSP notifications,
    /// async-job completion sinks, or any other site that observes an
    /// upstream freshness transition (see audit M-013 in
    /// `docs/architecture-audit/findings/freshness-4-variant.md`).
    ///
    /// Returns the set of [`NodeId`]s whose freshness was actually updated by
    /// the walk; the early-cutoff gate prunes nodes whose derived freshness
    /// matches their current freshness. Returns an empty set when no
    /// `eval_state` is present (engine has not yet been initialised by a
    /// successful `eval()` / `eval_cached()` / `edit_source()` call).
    ///
    /// The `generation` argument is forwarded verbatim to the §7.2 truth
    /// table consulted by the walk — callers that care about Intermediate
    /// fan-in should pass the current refinement generation; callers that
    /// only care about Final propagation may pass any value.
    pub fn propagate_freshness_only(
        &mut self,
        changed: &std::collections::HashSet<reify_types::ValueCellId>,
        generation: u64,
    ) -> std::collections::HashSet<crate::cache::NodeId> {
        // `eval_state` and `cache` are disjoint Engine fields, so the
        // borrow checker accepts a simultaneous immutable borrow of
        // `eval_state` and a mutable borrow of `cache` — no clone needed.
        // No-op when eval_state is None — there is no graph to walk.
        let Some(state) = self.eval_state.as_ref() else {
            return std::collections::HashSet::new();
        };
        crate::freshness_walk::propagate_freshness_only(
            &mut self.cache,
            &state.reverse_index,
            &state.snapshot.graph,
            changed,
            generation,
        )
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Immutable access to the engine's warm-state pool. Per arch §4.3 / §6.4,
    /// the pool holds donated `OpaqueState` for removed topology nodes and
    /// services checkouts when topology re-adds them. Tests use this accessor
    /// to assert pool state after `edit_source` removes/adds nodes.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// The field itself is always present (struct layout is identical in test
    /// and non-test builds); only the accessor is gated.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn warm_pool(&self) -> &crate::warm_pool::WarmStatePool {
        &self.warm_pool
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Mutable access to the engine's warm-state pool, primarily used by
    /// integration tests to swap in a tiny-budget pool (e.g. via
    /// `*engine.warm_pool_mut() = WarmStatePool::new(50);`) to exercise the
    /// LRU-eviction → None-checkout → cold-fallback path described in arch
    /// §4.3 lines 539-540.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn warm_pool_mut(&mut self) -> &mut crate::warm_pool::WarmStatePool {
        &mut self.warm_pool
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Mutable access to the cache store, used by integration tests that need
    /// to inject warm state (`donate_warm_state`) on a freshly-created cache
    /// entry to simulate a future WarmStartable producer's output. Mirrors the
    /// existing immutable `cache_store()` accessor.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn cache_store_mut(&mut self) -> &mut CacheStore {
        &mut self.cache
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Register a `ValueCellId` whose let-binding evaluation must force a
    /// panic immediately before `reify_expr::eval_expr` runs, exercising the
    /// arch §9.1 panic-boundary path in `evaluate_let_bindings`.
    ///
    /// The let-binding evaluator wraps `eval_expr` in `catch_unwind`; on
    /// panic it writes `Freshness::Failed { error }` via
    /// `CacheStore::mark_failed` and emits a single `EventKind::Failed`
    /// event, skipping the normal `EventKind::Completed` event. Multiple
    /// calls accumulate cells into the set.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see
    /// `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn set_panic_on_eval(&mut self, cell: reify_types::ValueCellId) {
        self.panic_on_eval_cells.insert(cell);
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Remove a single previously-registered cell from the panic-injection
    /// set. Returns `true` if `cell` was present and removed, `false`
    /// otherwise. Symmetric counterpart to [`Engine::set_panic_on_eval`] —
    /// lets tests verify the recovery path (re-evaluate the cell after the
    /// panic injection is withdrawn) without rebuilding the engine.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn remove_panic_on_eval(&mut self, cell: &reify_types::ValueCellId) -> bool {
        self.panic_on_eval_cells.remove(cell)
    }

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Clear every cell from the panic-injection set in one call. Wholesale
    /// counterpart to [`Engine::remove_panic_on_eval`]; useful when a test
    /// drives several `set_panic_on_eval` calls then wants to verify the
    /// engine returns to the unforced path on the next eval.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn clear_panic_on_eval(&mut self) {
        self.panic_on_eval_cells.clear();
    }

    // ── Task 3541: warm-pool event drain + journal recording ─────────────────

    /// Drain the warm pool's buffered telemetry events, record each as an
    /// [`crate::journal::EvalEvent`] on the diagnostic journal, and return the
    /// drained [`Vec<crate::warm_pool::WarmPoolEvent>`] so the GUI layer can
    /// surface them.
    ///
    /// # Invariants
    ///
    /// - After this call, `self.warm_pool.drain_events()` returns an empty Vec.
    /// - Each drained event is recorded on the journal with:
    ///   - `version = VersionId(self.next_version_id.saturating_sub(1))` — the
    ///     most recently assigned eval version (or 0 before the first eval).
    ///   - `timestamp = Instant::now()` at drain time.
    ///
    /// # Drain site
    ///
    /// Called by `EngineSession::drain_and_emit_warm_pool_events` (engine.rs)
    /// after each engine call site that may produce donations or evictions
    /// (check, edit_check, build, tessellate_snapshot, etc.).  This is the
    /// eval-boundary call site that wires the existing warm_pool event buffer
    /// to the diagnostic journal, subsuming M-010.
    pub fn drain_and_record_warm_pool_events(
        &mut self,
    ) -> Vec<crate::warm_pool::WarmPoolEvent> {
        let events = self.warm_pool.drain_events();
        let version = reify_types::VersionId(self.next_version_id.saturating_sub(1));
        let timestamp = std::time::Instant::now();
        for ev in &events {
            let eval_event =
                crate::journal::translate_warm_pool_event_to_eval_event(ev, version, timestamp);
            self.journal.record(eval_event);
        }
        events
    }

    /// Number of events currently recorded in the engine's diagnostic journal.
    ///
    /// Exposed only in test and test-instrumentation builds — callers use it
    /// to assert that `drain_and_record_warm_pool_events` correctly records
    /// events on the journal.
    #[cfg(test)]
    pub fn journal_event_count(&self) -> usize {
        self.journal.len()
    }
}

/// Perform the full startup-sweep of `cache_root`, binding the current build's
/// engine-version hash so the live cache directory is never pruned.
///
/// This is the single engine-admin startup seam that callers (reify-cli, GUI
/// startup) should invoke with their resolved `cache_root` before the first
/// cache lookup. The `Engine` struct itself is cache-root-agnostic — it has no
/// persistent-cache field — so the resolved `cache_root` is supplied by the
/// caller, mirroring how `reify-cli` supplies `cache_root` to `evict_over_cap`.
///
/// Delegates to [`crate::persistent_cache::sweep_on_startup`] with
/// [`crate::persistent_cache::ENGINE_VERSION_HASH`] as the `current_engine_version`
/// argument, which ensures that the live cache subdir (`<cache_root>/<hash>`)
/// is excluded from the orphan-dir prune even if its mtime is somehow old.
///
/// Returns the aggregated [`crate::persistent_cache::SweepReport`]. An absent
/// or inaccessible `cache_root` is a no-op that returns
/// `SweepReport::default()` — the sweep never fails startup.
pub fn sweep_persistent_cache_at_startup(
    cache_root: &std::path::Path,
) -> crate::persistent_cache::SweepReport {
    crate::persistent_cache::sweep_on_startup(
        cache_root,
        crate::persistent_cache::ENGINE_VERSION_HASH,
    )
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

    /// `set_panic_on_eval` accumulates cells and `remove_panic_on_eval` /
    /// `clear_panic_on_eval` provide the symmetric withdraw-paths required
    /// for tests that want to verify recovery after a forced-panic eval
    /// (re-eval the same cell once the injection is removed). Without these
    /// accessors, tests would have to rebuild the engine to clear the
    /// hook — see review suggestion on `set_panic_on_eval` (task #2330
    /// amendment).
    #[test]
    fn panic_on_eval_set_remove_and_clear_round_trip() {
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::ValueCellId;

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        let a = ValueCellId::new("M", "a");
        let b = ValueCellId::new("M", "b");
        let c = ValueCellId::new("M", "c");

        engine.set_panic_on_eval(a.clone());
        engine.set_panic_on_eval(b.clone());
        engine.set_panic_on_eval(c.clone());

        // Single removal returns true and only takes that cell out.
        assert!(
            engine.remove_panic_on_eval(&b),
            "remove_panic_on_eval(b) should return true when b was registered"
        );
        // Removing a missing cell returns false.
        assert!(
            !engine.remove_panic_on_eval(&b),
            "remove_panic_on_eval(b) should return false on second call"
        );
        // a and c are still registered.
        assert!(engine.panic_on_eval_cells.contains(&a));
        assert!(engine.panic_on_eval_cells.contains(&c));
        assert!(!engine.panic_on_eval_cells.contains(&b));

        // Bulk clear empties the set.
        engine.clear_panic_on_eval();
        assert!(
            engine.panic_on_eval_cells.is_empty(),
            "clear_panic_on_eval should empty the set; got {:?}",
            engine.panic_on_eval_cells
        );
        // Idempotent on empty.
        engine.clear_panic_on_eval();
        assert!(engine.panic_on_eval_cells.is_empty());
    }

    /// `Engine::topology_attribute_table` returns a borrow of the v0.2
    /// attribute table on the engine. After construction (both
    /// `Engine::new` and `Engine::with_prelude`), the table must be empty
    /// — that is the documented post-condition relied on by tasks 5-8
    /// (which assume an empty table at the start of `execute_realization_ops`)
    /// and by integration tests that seed the table by hand.
    #[test]
    fn topology_attribute_table_starts_empty_on_new_and_with_prelude() {
        use reify_test_support::mocks::MockConstraintChecker;

        // Engine::new path.
        let engine_new = Engine::new(Box::new(MockConstraintChecker::new()), None);
        let table_new = engine_new.topology_attribute_table();
        assert!(table_new.is_empty());
        assert_eq!(table_new.len(), 0);

        // Engine::with_prelude path (empty prelude).
        let engine_wp = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        let table_wp = engine_wp.topology_attribute_table();
        assert!(table_wp.is_empty());
        assert_eq!(table_wp.len(), 0);
    }

    /// `Engine::freshness` is a stable always-public read accessor (no cfg
    /// gate) that returns `Freshness::Final` for unknown nodes and correctly
    /// reflects the cache state after a forced-panic eval.
    ///
    /// (a) On a fresh engine with no eval, unknown node → `Freshness::Final`.
    /// (b) After `set_panic_on_eval(cell)` + `eval()`, the cell's node →
    ///     `Freshness::Failed { error }`.
    ///
    /// Step-1 test: this will fail until `Engine::freshness` is implemented
    /// in step-2 (the method does not yet exist).
    #[test]
    fn freshness_returns_final_for_unknown_node_and_failed_after_forced_panic() {
        use crate::cache::NodeId;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
        use reify_types::{Freshness, ModulePath, Type, Value, ValueCellId};

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

        // (a) Unknown node → Final (default) even before any eval.
        let unknown = ValueCellId::new("Ghost", "x");
        let unknown_node = NodeId::Value(unknown.clone());
        assert_eq!(
            engine.freshness(&unknown_node),
            Freshness::Final,
            "freshness of an unknown node must default to Freshness::Final"
        );

        // Build a 1-cell synthetic module: `let b = 1.0` in template T.
        let b_id = ValueCellId::new("T", "b");
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .let_binding(
                        "T",
                        "b",
                        Type::Real,
                        reify_test_support::builders::literal(Value::Real(1.0)),
                    )
                    .build(),
            )
            .build();

        // (b) Force a panic on `b`; after eval the cache must record Failed.
        engine.set_panic_on_eval(b_id.clone());
        let _ = engine.eval(&module);

        let b_node = NodeId::Value(b_id.clone());
        match engine.freshness(&b_node) {
            Freshness::Failed { error } => {
                assert!(
                    !error.message().is_empty(),
                    "Failed error message must be non-empty"
                );
            }
            other => panic!(
                "expected Freshness::Failed after forced-panic eval, got {:?}",
                other
            ),
        }
    }

    /// `Engine::pending_cause` returns the chain-root `NodeId` for a Pending
    /// node and `None` in all other cases.
    ///
    /// (a) Unknown node → `None` (no cache entry, default).
    /// (b) A Failed root → `None` (Failed nodes are the chain root, not
    ///     forwarders; per the `CacheStore::pending_cause` contract at
    ///     cache.rs:597-603).
    /// (c) A Pending consumer whose upstream Failed → `Some(NodeId::Value(a))`.
    /// (d) A transitively-Pending node (Pending whose upstream is itself Pending)
    ///     → `Some(<chain root>)` — the upstream Pending node's own NodeId is
    ///     NOT the cause; only Failed leaves are chain roots, and a Pending
    ///     input forwards its `pending_cause` per cache.rs:147-156 and
    ///     `derive_output_freshness_with_cause` at cache.rs:961-1013.
    #[test]
    fn pending_cause_returns_failed_leaf_for_pending_dependent() {
        use crate::cache::NodeId;
        use reify_test_support::builders::{binop, literal, value_ref};
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
        use reify_types::{BinOp, ModulePath, Type, Value, ValueCellId};

        let a_id = ValueCellId::new("T", "a");
        let b_id = ValueCellId::new("T", "b");
        let c_id = ValueCellId::new("T", "c");

        // Build a 3-cell module: `let a = 1.0`, `let b = a + 1.0`, `let c = b + 1.0`.
        // b reads a, so when a fails b becomes Pending (arch §9.2).
        // c reads b, so when b is Pending c becomes transitively Pending.
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .template(
                TopologyTemplateBuilder::new("T")
                    .let_binding("T", "a", Type::Real, literal(Value::Real(1.0)))
                    .let_binding(
                        "T",
                        "b",
                        Type::Real,
                        binop(BinOp::Add, value_ref("T", "a"), literal(Value::Real(1.0))),
                    )
                    .let_binding(
                        "T",
                        "c",
                        Type::Real,
                        binop(BinOp::Add, value_ref("T", "b"), literal(Value::Real(1.0))),
                    )
                    .build(),
            )
            .build();

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

        // Pass 1: cold eval — initialises cache (all cells → Final).
        let _ = engine.eval(&module);

        // Pass 2: force `a` to fail; `b` depends on `a` so it becomes Pending.
        engine.set_panic_on_eval(a_id.clone());
        let _ = engine.eval(&module);

        // (a) Unknown node → None (no cache entry).
        let unknown = NodeId::Value(ValueCellId::new("Ghost", "x"));
        assert!(
            engine.pending_cause(&unknown).is_none(),
            "pending_cause of an unknown node must return None"
        );

        // (b) Failed root → None (Failed nodes are chain roots, not forwarders).
        let a_node = NodeId::Value(a_id.clone());
        assert!(
            engine.pending_cause(&a_node).is_none(),
            "pending_cause of a Failed root must return None; \
             Failed nodes are the chain root, not forwarders"
        );

        // (c) Pending consumer → Some(Failed root).
        let b_node = NodeId::Value(b_id.clone());
        let expected_cause = NodeId::Value(a_id.clone());
        assert_eq!(
            engine.pending_cause(&b_node),
            Some(expected_cause),
            "pending_cause of the Pending consumer must return Some(NodeId::Value(a))"
        );

        // (d) Pending dependent of a Pending dependent → chain root forwarded.
        //     `c` reads `b` (Pending with cause = a). Per the forwarding contract
        //     at cache.rs:147-156 and the chain-forwarding logic in
        //     `derive_output_freshness_with_cause` (cache.rs:961-1013), a Pending
        //     input forwards the upstream entry's `pending_cause` — the Pending
        //     node's own NodeId is NOT used (only Failed leaves are chain roots).
        //     So c.pending_cause must read `Some(a)`, not `Some(b)`.
        let c_node = NodeId::Value(c_id.clone());
        assert_eq!(
            engine.pending_cause(&c_node),
            Some(NodeId::Value(a_id.clone())),
            "pending_cause of a transitively-Pending node must forward the chain root \
             (a), not the immediate Pending upstream (b); see cache.rs:147-156 and \
             derive_output_freshness_with_cause at cache.rs:961-1013"
        );
    }

    /// `Engine::pending_cause` admits `NodeId::Compute(_)` as a valid chain-root
    /// cause per PRD §3 "Chain-root contract extension". Exercises the engine-level
    /// delegation contract using direct `cache_store_mut()` injection (no dispatch
    /// machinery yet — that is task γ).
    ///
    /// Downstream `NodeId::Value(V)` whose `pending_cause = Some(Compute(N))`
    /// → `engine.pending_cause(&V) == Some(NodeId::Compute(N))`.
    #[test]
    fn pending_cause_admits_compute_node_id_as_chain_root() {
        use crate::cache::{CachedResult, NodeCache, NodeId};
        use crate::deps::DependencyTrace;
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_types::{
            ComputeNodeId, DeterminacyState, Freshness, Value, ValueCellId, VersionId,
        };

        let v_id = ValueCellId::new("T", "v");
        let v_node = NodeId::Value(v_id.clone());
        let compute_node = NodeId::Compute(ComputeNodeId::new("T", 0));

        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);

        // Seed a Value entry for v directly — no eval pipeline needed; the test
        // exercises only the pending_cause delegation contract, not eval behaviour.
        engine.cache_store_mut().put(
            v_node.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(1),
            ),
        );

        // Wire: mark v as Pending with compute_node as the chain-root cause.
        // No cache entry for `compute_node` is needed — `pending_cause` reads
        // only the side-table on v's entry, never the cause node's entry.
        let marked = engine
            .cache_store_mut()
            .mark_pending_with_cause(&v_node, compute_node.clone());
        assert!(
            marked,
            "mark_pending_with_cause must return true for the existing Value entry"
        );

        // Engine delegation: v's pending_cause must be the Compute chain root.
        assert_eq!(
            engine.pending_cause(&v_node),
            Some(compute_node.clone()),
            "engine.pending_cause must return Some(NodeId::Compute(N)) for a Value \
             entry whose pending_cause was set to a Compute node \
             (PRD §3 chain-root contract extension)"
        );
    }

    // ── sweep_persistent_cache_at_startup tests (step-11) ───────────────────

    /// Step-11 RED: `sweep_persistent_cache_at_startup` binds the live engine
    /// version (`persistent_cache::ENGINE_VERSION_HASH`) and delegates to
    /// `persistent_cache::sweep_on_startup`.
    ///
    /// Fixture:
    /// * An old `.tmp.*` file under a shard dir in the current engine-version
    ///   subdir → must be swept.
    /// * An old non-current orphan engine-version subdir → must be pruned.
    /// * A subdir named exactly `ENGINE_VERSION_HASH` backdated > 30d → must
    ///   NOT be pruned (proves the wrapper passes `ENGINE_VERSION_HASH` as
    ///   `current_engine_version`).
    ///
    /// Returns a `SweepReport` reflecting both removals.
    #[test]
    fn sweep_persistent_cache_at_startup_binds_live_engine_version() {
        use crate::persistent_cache::{
            ENGINE_VERSION_HASH, ORPHAN_DIR_AGE, STALE_TEMPFILE_AGE, backdate_mtime, shard_dir,
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // (i) Old .tmp.* file in the current engine-version shard.
        let inp = "abcd111111111111111111111111abcd";
        let sd = shard_dir(root, ENGINE_VERSION_HASH, inp);
        std::fs::create_dir_all(&sd).unwrap();
        let stale_tmp = sd.join(".tmp.stale");
        std::fs::write(&stale_tmp, b"crash-leftover").unwrap();
        backdate_mtime(&stale_tmp, STALE_TEMPFILE_AGE.as_secs() + 120);

        // (ii) Old orphan engine-version subdir.
        let orphan_eng = "beef000000000000000000000000beef";
        let orphan_dir = root.join(orphan_eng);
        std::fs::create_dir_all(&orphan_dir).unwrap();
        backdate_mtime(&orphan_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        // (iii) Current engine-version subdir backdated > 30d — must survive.
        let current_dir = root.join(ENGINE_VERSION_HASH);
        // current_dir already exists (created via sd above); backdate it.
        backdate_mtime(&current_dir, ORPHAN_DIR_AGE.as_secs() + 60);

        let report = crate::engine_admin::sweep_persistent_cache_at_startup(root);

        assert!(
            !stale_tmp.exists(),
            "stale .tmp.* must be removed by sweep_persistent_cache_at_startup"
        );
        assert!(
            !orphan_dir.exists(),
            "orphan engine-version dir must be pruned"
        );
        assert!(
            current_dir.exists(),
            "ENGINE_VERSION_HASH subdir must never be pruned (wrapper must pass it as current)"
        );
        assert_eq!(report.tempfiles_removed, 1, "tempfiles_removed must be 1");
        assert_eq!(
            report.orphan_dirs_removed, 1,
            "orphan_dirs_removed must be 1"
        );
    }
}
