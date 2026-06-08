// Split from lib.rs (task 2032) — admin methods.

use crate::cache::{CacheStore, NodeId};
use crate::demand::DemandRegistry;
use crate::journal::EventJournal;
use crate::snapshot::Snapshot;
use crate::{Engine, EvaluationState};
use reify_compiler::{CompiledModule, EntityKind, ValueCellKind};
use reify_core::Diagnostic;
use reify_ir::{
    CompiledFunction, ConstraintChecker, ConstraintSolver, FeatureTagTable, GeometryKernel,
    OptimizedImpl, TopologyAttributeTable,
};
use std::collections::{BTreeMap, HashMap};
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
        expected: Box<reify_core::dimension::DimensionVector>,
        got: Box<reify_core::dimension::DimensionVector>,
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
    registry: &mut reify_ir::StructureRegistry,
    modules: &[CompiledModule],
) {
    for module in modules {
        for tmpl in &module.templates {
            if tmpl.entity_kind != EntityKind::Structure {
                continue;
            }
            let field_layout: Vec<(String, reify_core::Type)> = tmpl
                .value_cells
                .iter()
                .filter(|c| matches!(c.kind, ValueCellKind::Param))
                .map(|c| (c.id.member.clone(), c.cell_type.clone()))
                .collect();
            registry.intern(
                &tmpl.name,
                reify_ir::StructureMeta {
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
    value: &reify_ir::Value,
    cell_type: &reify_core::Type,
) -> Result<(), ParamOverrideRejection> {
    // `registry: None` for now — param-override validation does not yet
    // consult the per-Engine structure side-table. Trait-bound conformance
    // for `Value::StructureInstance` is proven at compile time; the registry
    // is plumbed into the eval path in a later step (3540 / SIR-α step-12).
    if !crate::value_type_kind_matches(value, cell_type, None) {
        return Err(ParamOverrideRejection::TypeKindMismatch);
    }
    if let reify_core::Type::Scalar {
        dimension: expected,
    } = cell_type
        && let reify_ir::Value::Scalar { dimension: got, .. } = value
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

    /// Synthetic key used to insert a user-supplied
    /// `Option<Box<dyn GeometryKernel>>` (passed through `Engine::new` /
    /// `Engine::with_prelude`) into the multi-handle `geometry_kernels` map.
    ///
    /// Task ε (3436) reshaped the v0.2 single-kernel field
    /// `geometry_kernel: Option<Box<dyn GeometryKernel>>` to a
    /// `BTreeMap<String, Box<dyn GeometryKernel>>` keyed on kernel name.
    /// `GeometryKernel` carries no `name()` method (capabilities live in the
    /// external inventory registry, not on the trait); a fixed synthetic
    /// constant lets the new map hold a caller-supplied kernel without
    /// requiring a trait extension. The const is prefixed `"__"` and named
    /// distinctly so it cannot collide with any real inventory-registered
    /// kernel name (`"occt"`, `"manifold"`, …).
    ///
    /// `Engine::default_kernel_name` is set to `Some(DEFAULT_KERNEL_NAME)`
    /// when a non-`None` kernel is supplied through `with_prelude`, so the
    /// engine's single-handle surfaces (`export`, `tessellate`,
    /// post-process) resolve to the same kernel the caller passed.
    pub const DEFAULT_KERNEL_NAME: &'static str = "__reify_eval_default_kernel";

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
        // Task ε (3436): wrap a single user-supplied kernel into the new
        // BTreeMap-keyed multi-handle field under the synthetic constant
        // [`Self::DEFAULT_KERNEL_NAME`]. `None` → empty map, `default_kernel_name`
        // = `None` (matches v0.2 semantics where no kernel was configured).
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        let default_kernel_name: Option<String> = geometry_kernel.map(|k| {
            let name = Engine::DEFAULT_KERNEL_NAME.to_string();
            geometry_kernels.insert(name.clone(), k);
            name
        });
        Self::with_prelude_and_kernels(
            constraint_checker,
            geometry_kernels,
            default_kernel_name,
            prelude,
        )
    }

    /// Internal constructor accepting the new multi-handle kernel map shape
    /// directly. Used by both [`Self::with_prelude`] (wraps a single
    /// `Option<Box<…>>` into the map) and [`Self::with_registered_kernels`]
    /// (loads one entry per inventory registration). Centralises the field
    /// initialisation so future engine fields land in one place.
    fn with_prelude_and_kernels(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>>,
        default_kernel_name: Option<String>,
        prelude: &'static [CompiledModule],
    ) -> Self {
        let prelude_functions: Vec<CompiledFunction> = prelude
            .iter()
            .flat_map(|pm| pm.functions.iter().cloned())
            .collect();
        // Seed the structure side-table from the prelude's `structure def`
        // templates (task 3540 / SIR-α step-12). Refreshed incrementally per
        // `eval()` from the user module's templates.
        let mut structure_registry = reify_ir::StructureRegistry::new();
        populate_structure_registry(&mut structure_registry, prelude);
        Self {
            constraint_checker,
            geometry_kernels,
            default_kernel_name,
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
            last_dispatch_count: 0,
            // Task 4050 test seam: no registry override by default; installed
            // only via `with_test_kernels_and_registry`. cfg-gated to match the
            // field declaration in lib.rs (absent in production builds).
            #[cfg(any(test, feature = "test-instrumentation"))]
            test_registry_override: None,
            // GHR-δ §5: empty until the first build() populates it.
            realization_handles: HashMap::new(),
            geometry_revalidation_slow_path: std::sync::atomic::AtomicUsize::new(0),
            journal: EventJournal::new(),
            functions: Vec::<CompiledFunction>::new().into(),
            compiled_purposes: Vec::new(),
            active_purposes: HashMap::new(),
            active_purpose_bindings: HashMap::new(),
            active_tolerance_scope: HashMap::new(),
            active_objective_map: HashMap::new(),
            active_purpose_let_cells: HashMap::new(),
            objectives: HashMap::new(),
            centrality_synthesized_scopes: std::collections::HashSet::new(),
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
            // undef-self-describing α (task 4321): capture disabled by default
            // to guarantee zero overhead on the hot path.
            capture_undef_causes: false,
            last_undef_causes: HashMap::new(),
            // Task 4198 (Determinacy β): capture disabled by default so the
            // hot path pays zero overhead (no BRepExtrema projection) when γ
            // assertions are not active. Enable via set_capture_repr_tol(true).
            capture_repr_tol: false,
            // Task 4198 (Determinacy β): empty until tessellate_realizations()
            // / tessellate_snapshot() populates it via measure_mesh_deviation.
            achieved_repr_tol: BTreeMap::new(),
        }
    }

    /// **`#[cfg(any(test, feature = "test-instrumentation"))]`-gated** test
    /// constructor (task 4050): build an `Engine` from a caller-supplied kernel
    /// map AND a caller-supplied dispatch capability registry, bypassing the
    /// link-time `inventory` (`crate::kernel_registry::collect_registry()`).
    ///
    /// This is the harness seam the cross-kernel-handoff integration test needs:
    /// reify-eval links no Mesh-capable boolean kernel (no `reify-kernel-manifold`
    /// dependency), so the live registry cannot drive a BRep→Mesh cross-kernel
    /// realization, and there is no public constructor accepting a custom kernel
    /// map + descriptors. Injecting both the counting mock kernels and a
    /// deterministic `{occt, manifold}` capability map lets `build()` exercise
    /// the conversion executor hermetically, independent of which kernel crates
    /// are compiled in.
    ///
    /// Wraps the private [`Self::with_prelude_and_kernels`] (embedded stdlib
    /// prelude, matching `Engine::new`), then installs `registry` as the
    /// `test_registry_override` consulted at the two build-path dispatch sites
    /// in `engine_build.rs`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn with_test_kernels_and_registry(
        constraint_checker: Box<dyn ConstraintChecker>,
        kernels: BTreeMap<String, Box<dyn GeometryKernel>>,
        registry: BTreeMap<String, reify_ir::CapabilityDescriptor>,
        default_kernel_name: Option<String>,
    ) -> Self {
        let mut engine = Self::with_prelude_and_kernels(
            constraint_checker,
            kernels,
            default_kernel_name,
            reify_compiler::stdlib_loader::load_stdlib(),
        );
        engine.test_registry_override = Some(registry);
        engine
    }

    /// **`#[cfg(any(test, feature = "test-instrumentation"))]`-gated** accessor
    /// (task 4050): return the terminal `KernelHandle` cached for a realization
    /// at `(entity, repr, tol, NO_OPTIONS)`, or `None` if no entry satisfies.
    ///
    /// The terminal handle is not graph-observable (a `RealizationNodeData`
    /// stores only `produced_repr: ReprKind`, not the originating `KernelId`),
    /// so the cross-kernel-handoff test reads it back through the realization
    /// cache to assert the terminal kernel is Manifold (gap-3 cross-kernel
    /// routing). `KernelHandle` is `Copy`, so the borrowed cache entry is copied
    /// out without disturbing the cache.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn test_terminal_handle(
        &self,
        entity: &str,
        repr: reify_ir::ReprKind,
        tol: f64,
    ) -> Option<reify_ir::KernelHandle> {
        self.realization_cache
            .lookup(entity, repr, tol, crate::NO_OPTIONS)
            .copied()
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

    /// Return the achieved representation tolerance (SI metres) for the given
    /// realized-occurrence name, or `None` if the occurrence was never
    /// tessellated, its mesh was empty, the kernel has no exact surface to
    /// project onto (non-OCCT kernels — B3 honest absence), or
    /// [`set_capture_repr_tol`](Self::set_capture_repr_tol) was not called with
    /// `true` before `tessellate_realizations()` / `tessellate_snapshot()`.
    ///
    /// The key format is `"{entity}#realization[{index}]"` — the same
    /// `MeshSurface.entity_path` the surfacing layer computes.
    ///
    /// Populated by `tessellate_realizations()` / `tessellate_snapshot()` only
    /// when `capture_repr_tol` is `true`; cleared at the start of each call.
    /// A missing key is never a stale value — it always means "not recorded
    /// this build" (either flag-off or B3 absence).
    ///
    /// # Sampled lower bound
    ///
    /// The returned value is a **sampled lower bound** on the true
    /// Hausdorff / chord deviation (4 interior points per triangle — centroid
    /// and 3 edge midpoints).  A mesh whose true deviation exceeds a tolerance
    /// can still produce a value below it if the sample points land close to
    /// the surface.  Task γ (`RepresentationWithin`) should document this at
    /// the assertion site.
    ///
    /// Task 4198 (Determinacy β) — γ (`RepresentationWithin` assertion) reads
    /// this to compare the measured deviation against the demanded tolerance.
    pub fn achieved_repr_tol(&self, occurrence: &str) -> Option<f64> {
        self.achieved_repr_tol.get(occurrence).copied()
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
    pub fn structure_registry(&self) -> &reify_ir::StructureRegistry {
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
    ) -> &crate::realization_cache::RealizationCache<reify_ir::KernelHandle> {
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
    ///
    /// # Construction cost (task ε / 3436, amendment round 2)
    ///
    /// **This alias instantiates only the lex-min-picked adapter, not every
    /// registered one**, mirroring the historical pre-ε single-pick semantics.
    /// PRD §9 Q9.1 keeps the alias in place through v0.3.x and schedules its
    /// removal (with the `#[deprecated]` attribute + caller migration) for
    /// v0.4 — a v0.4 follow-up task SHOULD be filed when that release cycle
    /// opens, but no such task exists yet because no v0.4-tracked work has
    /// been queued.
    ///
    /// Round 1 of this task's amendment cycle initially delegated to
    /// [`Self::with_registered_kernels`] (which factory-instantiates every
    /// adapter). Round 2 reviewer feedback (catalogued in the task plan)
    /// flagged the latent regression: in an OCCT-only build the two paths are
    /// identical, but the moment a second adapter (`"manifold"`, `"fidget"`,
    /// `"openvdb"`) lands in the inventory, every legacy caller of
    /// `with_registered_kernel` would silently allocate and hold the extra
    /// adapter even though it never reads through to it. Reverting the alias
    /// to single-pick keeps the cost identical to v0.2 until the v0.4
    /// deprecation cycle migrates call sites to
    /// [`Self::with_registered_kernels`] explicitly.
    ///
    /// Runtime behaviour is unchanged from the round-1 alias: the BRep-
    /// preferring lex-min picker is invariant under "load all then pick" vs
    /// "pick then load one" — both yield the same kernel name. The selection
    /// event is still emitted exactly once per construction (the
    /// `engine_with_registered_kernel_emits_one_selection_event` integration
    /// pin continues to assert this).
    pub fn with_registered_kernel(constraint_checker: Box<dyn ConstraintChecker>) -> Self {
        // Amendment round 2 (task ε / 3436): pick the lex-min BRep-capable
        // entry from the inventory and instantiate only it, then forward to
        // `with_prelude` (which wraps it under `DEFAULT_KERNEL_NAME` via the
        // single-kernel `Option<Box<dyn GeometryKernel>>` API). This restores
        // pre-ε single-pick allocation cost so additional adapter
        // registrations cannot silently regress legacy callers (reify-cli,
        // gui-tauri, integration tests that route through this constructor).
        //
        // The selection event is emitted directly here rather than via
        // `with_registered_kernels` because we are bypassing the
        // load-all-then-pick path. Total count is the full inventory size so
        // operators still see the INFO-level tie-break notification when
        // multiple adapters are registered.
        let picked = crate::kernel_registry::pick_lexmin_brep_kernel();
        let total = crate::kernel_registry::registry().len();
        if let Some(reg) = picked {
            crate::kernel_registry::emit_kernel_selection(reg.name, total);
        }
        let single_kernel: Option<Box<dyn GeometryKernel>> = picked.map(|reg| (reg.factory)());
        Self::with_prelude(
            constraint_checker,
            single_kernel,
            reify_compiler::stdlib_loader::load_stdlib(),
        )
    }

    /// Construct an Engine using the inventory-driven multi-kernel registry,
    /// loading **one adapter per registered descriptor** rather than a single
    /// pick (task ε / 3436; PRD `docs/prds/v0_3/multi-kernel-phase-3.md` §8).
    ///
    /// Walks the static linker-collected set of [`reify_types::KernelRegistration`]
    /// records, instantiates each adapter via its `factory` function, and
    /// inserts the result into the engine's
    /// [`Engine::geometry_kernels`][`Engine`]-internal `BTreeMap<String,
    /// Box<dyn GeometryKernel>>` keyed on the adapter's `name`. The
    /// `default_kernel_name` is set via the BRep-preferring lex-min picker
    /// [`crate::kernel_registry::pick_lexmin_brep_kernel`] so single-handle
    /// surfaces (`export`, `tessellate`, conformance/kinematic post-process)
    /// resolve to the same kernel the historical single-pick constructor
    /// returned — preserving runtime behaviour for OCCT-only builds where the
    /// loaded set is just `"occt"`.
    ///
    /// # Empty-registry semantics
    ///
    /// When no adapter has submitted a registration (e.g. stub-mode build with
    /// `cfg(has_occt)` off, or a binary that links no `reify-kernel-*` crate
    /// at all), `geometry_kernels` is empty and `default_kernel_name` is
    /// `None`, matching `Engine::new(checker, None)`. The existing build-path
    /// error surface ("no geometry kernel registered") fires cleanly.
    ///
    /// # Per-op routing
    ///
    /// Per-op dispatch routing in `execute_realization_ops` (step-8) consults
    /// [`crate::dispatcher::dispatch`] per op against the descriptor map and
    /// indexes into `geometry_kernels` by the dispatcher-named kernel. In the
    /// v0.2 baseline (`demanded=BRep`, `available={BRep}`) every op resolves
    /// to a 0-conversion plan naming the BRep kernel — identical runtime
    /// behaviour to the historical single-pick constructor.
    ///
    /// # Operator visibility
    ///
    /// One structured tracing event is emitted (via
    /// [`crate::kernel_registry::emit_kernel_selection`]): `INFO` when more
    /// than one adapter is registered (lex-min tie-break notification),
    /// `DEBUG` when only one is. The event fires only when a
    /// [`tracing::Subscriber`] is installed.
    pub fn with_registered_kernels(constraint_checker: Box<dyn ConstraintChecker>) -> Self {
        // Walk the OnceLock-memoized registry and instantiate every adapter.
        // BTreeMap iteration order is lexicographic on `name`, matching the
        // dispatcher's tie-break contract (PRD `docs/prds/v0_3/multi-kernel-phase-3.md`).
        let registry = crate::kernel_registry::registry();
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        for (name, reg) in registry.iter() {
            geometry_kernels.insert(name.clone(), (reg.factory)());
        }
        // BRep-preferring lex-min picker (task 3224 carry-over): a Mesh-only
        // kernel registered under a lex-smaller name (e.g. `"manifold" <
        // "occt"`) must not silently become the default kernel for single-
        // handle surfaces (export / tessellate) when a BRep-capable kernel is
        // also loaded.  Falls back to pure lex-min when no entry claims any
        // BRep pair (preserves Mesh-only-binary semantics).
        let default_kernel_name: Option<String> =
            crate::kernel_registry::pick_lexmin_brep_kernel().map(|reg| reg.name.to_string());
        if let Some(name) = default_kernel_name.as_deref() {
            crate::kernel_registry::emit_kernel_selection(name, geometry_kernels.len());
        }
        Self::with_prelude_and_kernels(
            constraint_checker,
            geometry_kernels,
            default_kernel_name,
            reify_compiler::stdlib_loader::load_stdlib(),
        )
    }

    /// Iterate over the names of every kernel currently held by this engine.
    ///
    /// Lexicographic order (the underlying field is a `BTreeMap` keyed on
    /// kernel name). Returns the synthetic [`Self::DEFAULT_KERNEL_NAME`] for
    /// the single-kernel `Engine::new` / `Engine::with_prelude` wrapping
    /// case; returns adapter names (`"occt"`, future `"manifold"`, …) for
    /// the inventory-driven `Engine::with_registered_kernels` case. Empty
    /// iterator when `Engine::new(checker, None)` was used (no kernel
    /// configured).
    pub fn registered_kernel_names(&self) -> impl Iterator<Item = &str> {
        self.geometry_kernels.keys().map(String::as_str)
    }

    /// Number of geometry kernels currently held by this engine.
    ///
    /// `0` matches the historical `Engine::new(checker, None)` semantics
    /// (no kernel configured). `1` is the wrapping case (single kernel
    /// inserted under [`Self::DEFAULT_KERNEL_NAME`]). `>1` arises with the
    /// inventory-driven [`Self::with_registered_kernels`] constructor once
    /// additional adapters are linked beyond OCCT.
    pub fn kernel_count(&self) -> usize {
        self.geometry_kernels.len()
    }

    /// Return the name of the engine's default kernel — the entry used for
    /// single-handle surfaces (export, tessellate, post-process) and as the
    /// fallback when a dispatcher plan names a kernel absent from the map.
    /// `None` when no kernel is configured.
    pub fn default_kernel_name(&self) -> Option<&str> {
        self.default_kernel_name.as_deref()
    }

    // Note (amendment, task ε / 3436): earlier drafts added
    // `default_kernel_mut(&mut self)` / `default_kernel_ref(&self)` helpers
    // intended to centralise the BTreeMap-keyed default-kernel lookup used by
    // `build` / `build_snapshot` / `tessellate_from_values`. The helpers were
    // unusable in practice: the post-process call sites pair the default
    // kernel with sibling-field borrows like `&self.topology_attribute_table`
    // (see `run_post_processes`), which only compile under Rust's
    // disjoint-field-borrow analysis. A `&mut self` method call collapses to
    // a whole-self borrow that conflicts with those siblings, so the call
    // sites must keep the inline `self.geometry_kernels.get_mut(name)`
    // pattern. Helpers removed rather than left dead-shielded.

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
    // G-allow: task #3422 ComputeDispatchRegistry + Engine API; engine call-site wiring lands in subsequent #3422 steps
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
    pub fn compute_dispatch(&self, target: &str) -> Option<crate::engine_compute::ComputeFn> {
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
        value_inputs: &[reify_ir::Value],
        realization_inputs: &[crate::engine_compute::RealizationReadHandle],
        options: &reify_ir::Value,
        prior_warm_state: Option<&reify_ir::OpaqueState>,
    ) -> Result<(reify_ir::Value, Vec<reify_core::Diagnostic>), Vec<reify_core::Diagnostic>> {
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
                    ComputeOutcome::Cancelled => Err(vec![reify_core::Diagnostic::error(format!(
                        "@optimized target {:?}: compute trampoline was cancelled",
                        target
                    ))]),
                }
            }
            // The "(falling back to body-inlining)" clause is intentionally
            // omitted here: fallback is the eval-loop caller's behaviour, not
            // this helper's. Direct callers of `dispatch_compute_node`
            // (including the unit tests in this crate) do NOT body-inline,
            // so the diagnostic would be misleading. The lowering site in
            // `engine_eval.rs` emits its own diagnostic that DOES mention
            // body-inline fallback, where that wording is accurate.
            None => Err(vec![reify_core::Diagnostic::error(format!(
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

    /// Return the set of scope names that received a synthetic Chebyshev-centre
    /// (max-min slack) centrality objective on the most recent `eval()` call.
    ///
    /// A scope appears here when `template.objective.is_none()` AND all its auto
    /// cells are `Type::Scalar` AND at least one constraint decomposes into an
    /// inequality slack (Ge/Gt/Le/Lt).  The set is cleared and repopulated on
    /// every `eval()` call.
    ///
    /// Used by the η integration test (task 4013) and by task θ's
    /// `ObjectiveProvenance` to record the `synthetic_centrality = true` flag
    /// in explain output (I5 provenance hook).
    pub fn centrality_synthesized_scopes(&self) -> &std::collections::HashSet<String> {
        &self.centrality_synthesized_scopes
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

    /// Test-only mutable access to the current snapshot.
    ///
    /// Used by task ε (3436) step-9 to pre-corrupt a realization node's
    /// `produced_repr` and prove that step-10's executor-write actually
    /// restores it (rather than the assertion trivially passing because the
    /// construction-time default in `EvaluationGraph::from_templates`
    /// already matches). Future test-instrumentation use cases that need to
    /// reach inside the snapshot (e.g. corrupt cached values, force
    /// determinacy transitions, surgically edit graph nodes) can reuse this
    /// same gated accessor instead of adding bespoke per-field hooks.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn snapshot_mut(&mut self) -> Option<&mut Snapshot> {
        self.eval_state.as_mut().map(|s| &mut s.snapshot)
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

    /// Returns the number of `dispatcher::dispatch` invocations on the per-op
    /// hot path during the most recent `build()` / `build_snapshot()` /
    /// `tessellate_realizations()` / `tessellate_snapshot()` call. Reset to 0
    /// at the start of each entry point.
    ///
    /// Task ε (3436) step-12 — pins the cache-rehit signal: a second build of
    /// the same module with the same demanded tolerance hits the
    /// `RealizationCache` short-circuit at the top of
    /// `execute_realization_ops`, which returns BEFORE the per-op loop runs,
    /// so the counter reports 0 on the second build.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn last_dispatch_count(&self) -> usize {
        self.last_dispatch_count
    }

    /// GHR-δ §5: reset the geometry-handle revalidation slow-path counter to 0.
    ///
    /// Called at the start of every `build()` / `build_snapshot()` (alongside
    /// clearing `realization_handles`), so the count reported afterwards
    /// reflects only the revalidation reads since the most recent build —
    /// mirroring the reset-at-operation-start discipline of the `last_*`
    /// counters. Takes `&self` because the counter is an `AtomicUsize`
    /// (interior mutability); the reader below observes it. Always available
    /// (NOT test-gated) since the reset site in `engine_build.rs` is production
    /// code.
    pub(crate) fn reset_geometry_revalidation_slow_path_count(&self) {
        self.geometry_revalidation_slow_path
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns the number of geometry-handle revalidation SLOW-PATH firings
    /// (stale-handle re-resolution OR absent-realization → `Undef`) since the
    /// last `build()` / `build_snapshot()`. The fast path (handle already
    /// matches, or non-handle value) does NOT increment it.
    ///
    /// GHR-δ §5 / §9 Q4 — gives the integration test (S15) a deterministic
    /// "slow path fires exactly once per boundary" signal (exact `==`, since a
    /// stale read both re-resolves AND writes the fresh handle back, so the
    /// immediately-following read takes the fast path).
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn geometry_revalidation_slow_path_count(&self) -> usize {
        self.geometry_revalidation_slow_path
            .load(std::sync::atomic::Ordering::Relaxed)
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
    pub fn freshness(&self, node: &NodeId) -> reify_ir::Freshness {
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
    /// The `generation` argument is forwarded verbatim to the §7.2
    /// Intermediate fan-in truth table consulted by the walk — callers must
    /// always pass the engine's current refinement generation.  A stale
    /// generation mis-gates any downstream cell that is legitimately
    /// Intermediate, producing incorrect freshness results.
    pub fn propagate_freshness_only<'a>(
        &mut self,
        changed: impl IntoIterator<Item = &'a reify_core::ValueCellId>,
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
    pub fn set_panic_on_eval(&mut self, cell: reify_core::ValueCellId) {
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
    pub fn remove_panic_on_eval(&mut self, cell: &reify_core::ValueCellId) -> bool {
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
    // G-allow: task #3541 eval-boundary warm-pool→journal drain; consumer EngineSession::drain_and_emit_warm_pool_events (engine.rs) wiring lands in subsequent #3541 steps
    pub fn drain_and_record_warm_pool_events(&mut self) -> Vec<crate::warm_pool::WarmPoolEvent> {
        let events = self.warm_pool.drain_events();
        let version = reify_core::VersionId(self.next_version_id.saturating_sub(1));
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

    /// **Test-instrumentation only — not a stable public metric.**
    ///
    /// Returns the most-recently-recorded content-hash for an imported field
    /// source file path, or `None` if no hash has been recorded yet for `path`
    /// (cold start or after a `cache.clear()`).
    ///
    /// Used by cache-invalidation integration tests (task 3576 step-9/10) to
    /// assert that `Engine::eval` records the file's content-hash after each
    /// elaboration of an `Imported` field and that the hash updates when the
    /// file's content changes between evals on the same engine.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    /// Integration tests reach this method via the self-dev-dep with the
    /// `test-instrumentation` feature enabled (see `crates/reify-eval/Cargo.toml`).
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn imported_file_content_hash(&self, path: &str) -> Option<reify_core::ContentHash> {
        self.cache.get_imported_file_hash(path)
    }

    // ── undef-self-describing α (task 4321) ──────────────────────────────────

    /// Enable or disable the per-cell UndefCause classification pass in `eval()`.
    ///
    /// Disabled by default (zero overhead on the hot path: no allocation, no
    /// classification work). Call with `true` before `eval()` to populate the
    /// per-cell origin map accessible via [`Self::undef_causes()`].
    ///
    /// Mirrors the `last_*` instrumentation-field + `engine_admin.rs` accessor
    /// convention (e.g. `last_sub_component_unknown_structure_errors` /
    /// `last_dispatch_count`).
    pub fn set_capture_undef_causes(&mut self, on: bool) {
        self.capture_undef_causes = on;
    }

    /// Enable or disable the achieved-representation-tolerance metric.
    ///
    /// When `true`, `tessellate_realizations()` / `tessellate_snapshot()` call
    /// `kernel.measure_mesh_deviation` for each successfully tessellated
    /// occurrence and record the result in [`Self::achieved_repr_tol`].
    ///
    /// Defaults to `false` — zero overhead on the hot path when γ assertions
    /// (`RepresentationWithin`) are not active. Mirrors `set_capture_undef_causes`.
    pub fn set_capture_repr_tol(&mut self, on: bool) {
        self.capture_repr_tol = on;
    }

    /// **Test-instrumentation only — not a stable public surface.**
    ///
    /// Replace the engine's `achieved_repr_tol` map with the supplied
    /// synthetic map, bypassing the normal `tessellate_realizations`
    /// population path.
    ///
    /// Used by `representation_within_assertion.rs` non-OCCT tests to inject
    /// known deviation values so that `dispatch_constraints`'s
    /// `RepresentationWithin` interception can be exercised without a geometry
    /// kernel.  Mirrors the gating pattern of `set_capture_repr_tol` and
    /// `snapshot_mut`.
    ///
    /// Only available under `#[cfg(any(test, feature = "test-instrumentation"))]`.
    #[cfg(any(test, feature = "test-instrumentation"))]
    pub fn set_achieved_repr_tol_for_test(
        &mut self,
        map: std::collections::BTreeMap<String, f64>,
    ) {
        self.achieved_repr_tol = map;
    }

    /// Returns the per-cell `UndefCause` map from the most recent `eval()` call.
    ///
    /// Empty when `capture_undef_causes` is `false` (the default). Only
    /// *originating* undef cells appear — purely-propagated cells are absent (A3
    /// in the PRD: a cell whose undef status is fully explained by an undef input
    /// records no cause here, leaving its entry absent).
    ///
    /// Cleared at the start of every `eval()` call so stale data from a prior
    /// pass never leaks through, even if `capture_undef_causes` is toggled off
    /// between calls.
    pub fn undef_causes(
        &self,
    ) -> &std::collections::HashMap<reify_core::ValueCellId, reify_ir::UndefCause> {
        &self.last_undef_causes
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

// ── ShellGuiMeshData — engine-side shell GUI mesh accessor (task θ / #3598) ──

/// Per-shell-body mesh data surfaced by `Engine::shell_gui_mesh_data`.
///
/// The accessor scans the evaluation graph for paired `shell-extract::extract`
/// and `solver::elastic_static` ComputeNodes, parses their cached results,
/// recovers per-vertex von Mises (top / mid / bottom), and derives per-face
/// shell normals — returning one entry per shell-classified body.
///
/// Field lengths are self-consistent by construction:
/// - `vertices.len() % 3 == 0`; `vertex_count = vertices.len() / 3`
/// - `indices.len() % 3 == 0`; `face_count = indices.len() / 3`
/// - `element_kind.len() == face_count`
/// - `region_tags.len() == face_count`
/// - `von_mises_{top,mid,bottom}.len() == vertex_count`
/// - `shell_normals_per_face.len() == 3 * face_count`
///
/// Physical stress accuracy is intentionally out of scope for v0.4 (PRD §11
/// OQ-2: the solver's internal flat-plate mesh ≠ the extraction mid-surface);
/// tests assert structural/length/finiteness only.
#[derive(Debug, Clone)]
pub struct ShellGuiMeshData {
    /// Template entity name (e.g. `"FeaShellFlexure"`). Used by
    /// `apply_shell_channels` to match against `MeshData.entity_path` by
    /// comparing the entity prefix before the `#realization[N]` suffix.
    pub entity_path: String,
    /// Mid-surface vertex positions, flat XYZ f32 (len == 3 * vertex_count).
    pub vertices: Vec<f32>,
    /// Mid-surface triangle indices, flat u32 (len == 3 * face_count).
    pub indices: Vec<u32>,
    /// Element-kind byte per triangle: all == 1 (shell triangle).
    /// len == face_count.
    pub element_kind: Vec<u8>,
    /// Segmentation region label per triangle (from
    /// `SegmentationResult.triangle_labels`). len == face_count.
    pub region_tags: Vec<u32>,
    /// Recovered per-vertex von Mises stress at the top fibre (z = +t/2).
    /// len == vertex_count; all values ≥ 0.0 (von Mises is non-negative).
    pub von_mises_top: Vec<f32>,
    /// Recovered per-vertex von Mises stress at the mid surface.
    /// len == vertex_count; all values ≥ 0.0.
    pub von_mises_mid: Vec<f32>,
    /// Recovered per-vertex von Mises stress at the bottom fibre (z = -t/2).
    /// len == vertex_count; all values ≥ 0.0.
    pub von_mises_bottom: Vec<f32>,
    /// Per-face geometric shell normals (cross-product of triangle edges),
    /// flat XYZ f32. len == 3 * face_count.
    /// The channel name `"shell_normal_per_face"` ends in `_per_face` so
    /// `apply_shell_channels` honours the `PER_FACE_CHANNEL_SUFFIX` contract.
    pub shell_normals_per_face: Vec<f32>,
}

impl Engine {
    /// Scan the evaluation graph for shell-classified bodies and return one
    /// [`ShellGuiMeshData`] per body.
    ///
    /// Returns an empty `Vec` when:
    /// - `eval()` has not yet been called (no `eval_state`).
    /// - No `shell-extract::extract` ComputeNode is present (non-shell scene).
    /// - A shell-extract node exists but its paired `solver::elastic_static`
    ///   node lacks `shell_channels` (tet fallback or partial solve).
    ///
    /// Overhead is negligible for non-shell scenes: one scan of
    /// `snapshot.graph.compute_nodes` (typically a handful of entries) plus
    /// a few cache lookups.
    ///
    /// # OQ-2 note
    ///
    /// The v0.4 stress solver uses an internal flat-plate mesh that differs
    /// from the extraction mid-surface (PRD §11 OQ-2). The accessor recovers
    /// von Mises with an element-count guard: when the per-element channel
    /// count ≠ the mid-surface triangle count, recovery runs over the minimum
    /// and the remainder is zero-filled. Output length always equals
    /// `n_mid_vertices` (the scalar_channels length contract is satisfied).
    pub fn shell_gui_mesh_data(&self) -> Vec<ShellGuiMeshData> {
        use crate::cache::CachedResult;

        let Some(eval_state) = self.eval_state.as_ref() else {
            return Vec::new();
        };
        let snapshot = &eval_state.snapshot;

        // ── Pass 1: collect cached Values for extract + elastic nodes ─────────
        // Keyed by entity name. When multiple nodes exist for the same entity
        // (unusual but possible), last-write wins; we take the first result
        // that successfully parses below.
        let mut extract_vals: HashMap<String, reify_ir::Value> = HashMap::new();
        let mut elastic_vals: HashMap<String, reify_ir::Value> = HashMap::new();

        for (c_id, node) in snapshot.graph.compute_nodes.iter() {
            let output_cell = match node.output_value_cells.first() {
                Some(c) => c,
                None => continue,
            };
            let cached = match self.cache.get(&NodeId::Value(output_cell.clone())) {
                Some(e) => e,
                None => continue,
            };
            let value = match &cached.result {
                CachedResult::Value(v, _) => v.clone(),
                _ => continue,
            };

            match node.target.as_str() {
                "shell-extract::extract" => {
                    extract_vals.insert(c_id.entity.clone(), value);
                }
                "solver::elastic_static" => {
                    elastic_vals.insert(c_id.entity.clone(), value);
                }
                _ => {}
            }
        }

        // ── Pass 2: parse + recover ───────────────────────────────────────────
        let mut result = Vec::new();

        for (entity, extract_val) in extract_vals.iter() {
            // Parse ShellExtractionResult → mid-surface geometry + labels.
            let (verts_f64, tris_usize, tri_labels) =
                match parse_shell_extraction_result(extract_val) {
                    Some(x) => x,
                    None => continue,
                };

            // Find matching elastic result with shell channels.
            let elastic_val = match elastic_vals.get(entity) {
                Some(v) => v,
                None => continue,
            };
            let (top_data, mid_data, bottom_data) =
                match parse_shell_channels_from_elastic(elastic_val) {
                    Some(x) => x,
                    None => continue, // tet result — no shell_channels
                };

            let n_verts = verts_f64.len() / 3;
            let n_tris = tris_usize.len() / 3;

            // Per-vertex von Mises recovery for each through-thickness channel.
            let von_mises_top =
                recover_von_mises_channel(n_verts, &tris_usize, &verts_f64, &top_data);
            let von_mises_mid =
                recover_von_mises_channel(n_verts, &tris_usize, &verts_f64, &mid_data);
            let von_mises_bottom =
                recover_von_mises_channel(n_verts, &tris_usize, &verts_f64, &bottom_data);

            // Per-face shell normals (geometric cross product of triangle edges).
            let shell_normals_per_face =
                compute_shell_normals_per_face(&verts_f64, &tris_usize);

            result.push(ShellGuiMeshData {
                entity_path: entity.clone(),
                vertices: verts_f64.iter().map(|&x| x as f32).collect(),
                indices: tris_usize.iter().map(|&i| i as u32).collect(),
                element_kind: vec![1u8; n_tris],
                region_tags: tri_labels,
                von_mises_top,
                von_mises_mid,
                von_mises_bottom,
                shell_normals_per_face,
            });
        }

        result
    }
}

// ── shell_gui_mesh_data helpers ───────────────────────────────────────────────

/// Parse a `Value::StructureInstance("ShellExtractionResult")` into flat
/// (vertices_f64, indices_usize, triangle_labels_u32).
///
/// Reverses `shell_extraction_result_to_value` for the fields needed by the
/// GUI populator: `mid_surface.vertices`, `mid_surface.triangles`, and
/// `segmentation.triangle_labels`.
fn parse_shell_extraction_result(
    val: &reify_ir::Value,
) -> Option<(Vec<f64>, Vec<usize>, Vec<u32>)> {
    use reify_ir::Value;
    let data = match val {
        Value::StructureInstance(d) if d.type_name == "ShellExtractionResult" => d,
        _ => return None,
    };

    // mid_surface.vertices: List of List([Real, Real, Real])
    let mid_surface = match data.fields.get("mid_surface")? {
        Value::StructureInstance(d) if d.type_name == "MidSurfaceMesh" => d,
        _ => return None,
    };

    let verts_f64: Vec<f64> = match mid_surface.fields.get("vertices")? {
        Value::List(verts) => {
            let mut flat = Vec::with_capacity(verts.len() * 3);
            for v in verts.iter() {
                match v {
                    Value::List(coords) => {
                        for c in coords.iter() {
                            match c {
                                Value::Real(x) => flat.push(*x),
                                _ => return None,
                            }
                        }
                    }
                    _ => return None,
                }
            }
            flat
        }
        _ => return None,
    };

    // mid_surface.triangles: List of List([Int, Int, Int])
    let tris_usize: Vec<usize> = match mid_surface.fields.get("triangles")? {
        Value::List(tris) => {
            let mut flat = Vec::with_capacity(tris.len() * 3);
            for t in tris.iter() {
                match t {
                    Value::List(idxs) => {
                        for i in idxs.iter() {
                            match i {
                                Value::Int(x) => flat.push(*x as usize),
                                _ => return None,
                            }
                        }
                    }
                    _ => return None,
                }
            }
            flat
        }
        _ => return None,
    };

    // segmentation.triangle_labels: List of Int
    let seg = match data.fields.get("segmentation")? {
        Value::StructureInstance(d) if d.type_name == "SegmentationResult" => d,
        _ => return None,
    };
    let tri_labels: Vec<u32> = match seg.fields.get("triangle_labels")? {
        Value::List(labels) => labels
            .iter()
            .map(|l| match l {
                Value::Int(x) => Some(*x as u32),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };

    Some((verts_f64, tris_usize, tri_labels))
}

/// Parse an `ElasticResult` `Value::StructureInstance` and extract
/// `shell_channels.{top, mid, bottom}` field data.
///
/// Returns `None` when:
/// - `val` is not an `ElasticResult` StructureInstance.
/// - `shell_channels` is `Value::Undef` (tet/solid result).
/// - Any of the three channel fields is missing or not a Sampled `Value::Field`.
fn parse_shell_channels_from_elastic(
    val: &reify_ir::Value,
) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    use reify_ir::{FieldSourceKind, Value};

    let data = match val {
        Value::StructureInstance(d) if d.type_name == "ElasticResult" => d,
        _ => return None,
    };

    // shell_channels must be ShellStress (not Undef = tet solve).
    let sc_data = match data.fields.get("shell_channels")? {
        Value::StructureInstance(d) if d.type_name == "ShellStress" => d,
        _ => return None,
    };

    let extract_sampled = |field_name: &str| -> Option<Vec<f64>> {
        match sc_data.fields.get(field_name)? {
            Value::Field {
                source: FieldSourceKind::Sampled,
                lambda,
                ..
            } => match lambda.as_ref() {
                Value::SampledField(sf) => Some(sf.data.clone()),
                _ => None,
            },
            _ => None,
        }
    };

    let top = extract_sampled("top")?;
    let mid = extract_sampled("mid")?;
    let bottom = extract_sampled("bottom")?;

    Some((top, mid, bottom))
}

/// Recover per-vertex von Mises from a flat per-element stress channel.
///
/// Uses `recover_nodal_stress_p1` (volume-weighted averaging) + `compute_von_mises_3x3`.
/// `channel_data` is flat with 9 `f64` per element (row-major 3×3 tensor).
/// Von Mises is rotation-invariant so local-frame computation is correct.
///
/// # OQ-2 guard
/// When `channel_data.len() / 9 != n_tri`, recovery runs over
/// `min(n_elem_channel, n_tri)` elements; unweighted vertices receive the zero
/// tensor → zero von Mises. Output length is always `n_vertices`.
fn recover_von_mises_channel(
    n_vertices: usize,
    triangles: &[usize],
    vertices_f64: &[f64],
    channel_data: &[f64],
) -> Vec<f32> {
    use reify_solver_elastic::{StressElement, recover_nodal_stress_p1};
    use reify_stdlib::compute_von_mises_3x3;

    if channel_data.is_empty() || !channel_data.len().is_multiple_of(9) || triangles.is_empty() {
        return vec![0.0_f32; n_vertices];
    }

    let n_tri = triangles.len() / 3;
    let n_elem_channel = channel_data.len() / 9;
    // OQ-2 guard: use the minimum to handle solver-mesh vs extraction-mesh mismatch.
    let n_recover = n_elem_channel.min(n_tri);

    let elements: Vec<StressElement<'_>> = (0..n_recover)
        .map(|i| {
            let conn = &triangles[3 * i..3 * i + 3];
            let sflat = &channel_data[9 * i..9 * (i + 1)];
            let stress = [
                [sflat[0], sflat[1], sflat[2]],
                [sflat[3], sflat[4], sflat[5]],
                [sflat[6], sflat[7], sflat[8]],
            ];
            // Triangle area as volume proxy for volume-weighted recovery.
            let i0 = conn[0];
            let i1 = conn[1];
            let i2 = conn[2];
            let v0 = &vertices_f64[3 * i0..3 * i0 + 3];
            let v1 = &vertices_f64[3 * i1..3 * i1 + 3];
            let v2 = &vertices_f64[3 * i2..3 * i2 + 3];
            let e0 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let e1 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let cross = [
                e0[1] * e1[2] - e0[2] * e1[1],
                e0[2] * e1[0] - e0[0] * e1[2],
                e0[0] * e1[1] - e0[1] * e1[0],
            ];
            let area = 0.5
                * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2])
                    .sqrt();
            let volume = area.max(1e-30_f64);
            StressElement { connectivity: conn, stress, volume }
        })
        .collect();

    let nodal_tensors = recover_nodal_stress_p1(n_vertices, &elements);

    nodal_tensors
        .iter()
        .map(|t| {
            // Flatten [[f64;3];3] to [f64;9] row-major for compute_von_mises_3x3.
            let flat = [
                t[0][0], t[0][1], t[0][2], t[1][0], t[1][1], t[1][2], t[2][0], t[2][1],
                t[2][2],
            ];
            compute_von_mises_3x3(&flat) as f32
        })
        .collect()
}

/// Compute per-face geometric shell normals from mid-surface geometry.
///
/// For each triangle `(i0, i1, i2)`, the normal is `unit((v1-v0) × (v2-v0))`.
/// Degenerate triangles (zero area) yield `(0, 0, 1)` as a fallback.
/// Returns a flat buffer `[nx0, ny0, nz0, ...]` with len == 3 * face_count.
fn compute_shell_normals_per_face(vertices_f64: &[f64], triangles: &[usize]) -> Vec<f32> {
    let n_tri = triangles.len() / 3;
    let mut normals = Vec::with_capacity(3 * n_tri);

    for i in 0..n_tri {
        let i0 = triangles[3 * i];
        let i1 = triangles[3 * i + 1];
        let i2 = triangles[3 * i + 2];
        let v0 = &vertices_f64[3 * i0..3 * i0 + 3];
        let v1 = &vertices_f64[3 * i1..3 * i1 + 3];
        let v2 = &vertices_f64[3 * i2..3 * i2 + 3];
        let e0 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e1 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let cross = [
            e0[1] * e1[2] - e0[2] * e1[1],
            e0[2] * e1[0] - e0[0] * e1[2],
            e0[0] * e1[1] - e0[1] * e1[0],
        ];
        let len =
            (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        let (nx, ny, nz) = if len > 0.0 {
            (cross[0] / len, cross[1] / len, cross[2] / len)
        } else {
            (0.0, 0.0, 1.0)
        };
        normals.push(nx as f32);
        normals.push(ny as f32);
        normals.push(nz as f32);
    }

    normals
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
        use reify_core::ValueCellId;
        use reify_test_support::mocks::MockConstraintChecker;

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
        use reify_core::{ModulePath, Type, ValueCellId};
        use reify_ir::{Freshness, Value};
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};

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
        use reify_core::{ModulePath, Type, ValueCellId};
        use reify_ir::{BinOp, Value};
        use reify_test_support::builders::{binop, literal, value_ref};
        use reify_test_support::mocks::MockConstraintChecker;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};

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
        use reify_core::{ComputeNodeId, ValueCellId, VersionId};
        use reify_ir::{DeterminacyState, Freshness, Value};
        use reify_test_support::mocks::MockConstraintChecker;

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
