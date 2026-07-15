// Split from lib.rs (task 2032) — build methods.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CompiledModule, CurveKind, GeomRef, ModifyKind, PatternKind,
    PrimitiveKind, ProfileKind, SubComponentDecl, SweepKind, TopologyTemplate, TransformKind,
};
use reify_core::{Diagnostic, DiagnosticLabel, RealizationNodeId, SourceSpan, VersionId};
use reify_ir::{
    AttributeHistory, BooleanOpHistoryRecords, BooleanOpParents, CapabilityDescriptor,
    CompiledFunction, ElementOrderTag, ErrorRef, ExportFormat, FeatureId, Freshness,
    GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelHandle,
    KernelId, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords, Operation, ReprKind, Role,
    SweepOpHistoryRecords, TopologyAttribute, TopologyAttributeTable, ValueMap, VolumeMesh,
};
use reify_ir::geometry::{ParentRole, descriptor_for};
use reify_shell_extract::{MidSurfaceMesh, ShellTetInterface};
use reify_solver_elastic::{
    Mesh2d, Mesh2dError, Mesh2dReport, MpcRow, SweepError, SweepParams, SweptMesh3d,
};

use crate::cache::{CacheStore, CachedResult, FAILED_REALIZATION_STUB_HANDLE, NodeCache, NodeId};
use crate::deps::{DependencyTrace, extract_realization_dependencies};
use crate::dispatcher::{DispatchPlan, dispatch, per_stage_tolerance_for_plan};
use crate::geometry_ops::compile_geometry_op;
use crate::journal::{EvalEvent, EventJournal, EventKind};
use crate::primitive_attribute_seed::{
    is_seedable_primitive, parse_bbox_xyz_min, record_solid_attribute,
    seed_primitive_attributes_for_handle,
};
use crate::realization_cache::{NO_OPTIONS, RealizationCache};
use crate::sweep_classifier::{
    SweptKind, SweptKindTable, classify_swept_body, swept_kind_to_sweep_params,
};
use crate::topology_attribute_propagation::{
    LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M, detect_local_index_reassignment_diagnostics,
    populate_extrude_attributes, populate_loft_attributes, populate_revolve_attributes,
    populate_sweep_attributes, propagate_attributes_via_brepalgoapi_history,
};
use crate::{BuildResult, Engine, EvaluationState, MeshSurface, TessellateResult};

/// Map a kernel registry name to the [`KernelId`] used to tag the handles that
/// kernel produces (task 4048).
///
/// Canonical inventory names (`"occt"`, `"manifold"`, …) map directly via
/// [`KernelId::from_registry_name`]. The synthetic backward-compat sentinel
/// [`Engine::DEFAULT_KERNEL_NAME`] — and any other non-canonical name — falls
/// back to [`KernelId::Occt`], the v0.2 single-kernel BRep default: a handle
/// tagged on that path exists only for step-index alignment / metadata and is
/// never re-routed by `.kernel` (per-kernel trait calls project `.id` and use
/// the resolved per-op kernel directly), so the tag is informational only.
///
/// # Why this tolerates non-canonical names while `dispatch()` panics
///
/// The sibling registry-name → `KernelId` bridge in
/// [`crate::dispatcher::dispatch`] `.expect()`s the lookup and panics on an
/// unknown name. That is sound *there* because the dispatcher's registry is
/// built from the inventory and so only ever holds canonical kernel names — a
/// miss is a genuine programming error. This helper deliberately takes the
/// opposite policy: it runs on the build path, where the resolved kernel name
/// legitimately includes non-canonical strings — [`Engine::DEFAULT_KERNEL_NAME`]
/// in production, and mock kernels registered under arbitrary names (e.g.
/// `"aaa"` / `"default"`) by the dispatch-routing unit tests further down this
/// file (`execute_realization_ops_routes_to_dispatcher_picked_kernel` and
/// peers). Because the `.kernel` tag is informational only, silently coercing
/// those to [`KernelId::Occt`] is correct, and a panic here would wrongly break
/// those tests. The divergence between the two bridges is therefore
/// intentional, not an oversight — picking the policy that fits each call
/// site's invariants (canonical-only registry → panic; informational tag over a
/// hot path that sees mock names → silent fallback).
///
/// # Revisit trigger
///
/// If `.kernel` ever becomes load-bearing (consulted to re-route dispatch /
/// export instead of being pure metadata), this silent coercion turns into a
/// mis-attribution hazard: a handle produced by manifold/fidget/gmsh under a
/// non-canonical name would be tagged `Occt` with no signal. At that point this
/// fallback must harden into a hard error so a stray name fails loudly — and the
/// mock kernels in the routing tests must be renamed to canonical names (the
/// same rename the dispatcher tests already took when `dispatch()` started
/// rejecting non-canonical names). Deferred alongside the
/// reify-ir / reify-config `KernelId` consolidation.
fn kernel_id_for_registry_name(name: &str) -> KernelId {
    KernelId::from_registry_name(name).unwrap_or(KernelId::Occt)
}

/// Forward a solid-level `TopologyAttribute` entry across the OCCT->Manifold
/// ingest seam (task #4636, LINK1).
///
/// A pure table op: `table.lookup(source).cloned()` then, on a hit,
/// `table.record(target, attr)`. No-op on a source miss (a non-seeded parent
/// — e.g. a non-primitive result feeding the conversion — degrades
/// gracefully to the existing `Ok(KernelAttributeOutcome::Discarded)` path
/// rather than panicking or erroring). Returns whether it recorded an entry,
/// so callers can track the forwarded TARGET handle alongside the SOURCE
/// solid handles they already track (task #4636 step-9 — see
/// `solid_attribute_handles` below) and exclude both from the post-loop
/// diagnostic scan.
///
/// Called from the `'convert:` loop below immediately after a successful
/// `target_kernel.ingest_mesh(&mesh)`, forwarding `{source_kernel_id, pid}`
/// (the pre-conversion parent handle, seeded by [`record_solid_attribute`]
/// at the engine seed site) onto `{target_kernel_id, handle.id}` (the fresh
/// ingested handle). This is what lets
/// `ManifoldKernel::propagate_attributes`'s `parent_map` lookup — keyed on
/// the SOLID parent handle — hit instead of missing.
///
/// Deliberately solid-granularity, not per-face: a per-face forward would
/// need `source_kernel.extract_faces(pid)`, which requires a `&mut` borrow
/// of the source kernel that conflicts with the concurrent `&mut
/// target_kernel` borrow already live at the ingest call site, and Manifold's
/// `parent_map` is keyed by per-solid `original_id()` regardless (a per-face
/// forward would have nothing coarser to attach to at this layer). Per-face
/// result-face persistence is deferred to #4263.
///
/// [`record_solid_attribute`]: crate::primitive_attribute_seed::record_solid_attribute
pub fn forward_solid_attribute_on_ingest(
    table: &mut TopologyAttributeTable,
    source: KernelHandle,
    target: KernelHandle,
) -> bool {
    if let Some(attr) = table.lookup(source).cloned() {
        table.record(target, attr);
        true
    } else {
        false
    }
    // TODO(#4263): forward per-face attributes via extract_faces for
    // descriptor-keyed result-face persistence (LINK3).
}

/// Per-op kind for `populate_single_parent_sweep_op` — the three single-
/// parent sweep variants (extrude, revolve, sweep) that share the
/// `SweepOpHistoryRecords` shape but emit different per-op
/// `Role` / `Cap`-flavor combos through their dedicated propagation
/// helper. Loft is *not* included here because it is multi-parent and
/// uses its own `LoftOpHistoryRecords` + `populate_loft_op` helper.
#[derive(Debug, Clone, Copy)]
enum SingleParentSweepKind {
    Extrude,
    Revolve,
    Sweep,
}

/// Bundle of `&mut` per-realization output tables that
/// `Engine::execute_realization_ops` writes into. Grouped (task 3119)
/// so each new per-realization side-channel adds one struct field
/// instead of growing the function signature by one parameter and
/// the diff at every call site.
///
/// **`produced_repr_out`** (task ε / 3436 step-10): channel through which the
/// executor surfaces the terminal output [`ReprKind`] for the realization
/// (i.e. the repr produced by the dispatcher-chosen kernel for the LAST
/// successful op of the realization, derived via [`plan_output_repr`]).
/// On cache hit the channel is set to [`ReprKind::BRep`] (the cache only
/// holds BRep-keyed entries). On rollback (`had_failure` or fewer handles
/// than ops produced) the channel is left untouched so the caller writes
/// nothing and the realization graph node retains its construction-time
/// default. The caller (`build` / `build_snapshot`) writes the value into
/// `self.eval_state.snapshot.graph.realizations[id].produced_repr` via
/// disjoint-field borrows immediately after `execute_realization_ops` returns.
struct RealizationOutputs<'a> {
    step_handles: &'a mut Vec<KernelHandle>,
    named_steps: &'a mut HashMap<String, KernelHandle>,
    /// By-name sibling of `named_steps` (task 5033 Gap #2 Gap A): records
    /// each named realization's resolved [`ReprKind`], so a LATER
    /// realization's cross-realization `GeomRef::Sub(name)` parent (e.g.
    /// "solid" in `let shell = isosurface(solid)`) can be resolved by NAME
    /// rather than by bare `GeometryHandleId` — matching a cross-kernel id
    /// against another realization's `realization_step_ids` risks a
    /// same-integer collision (#4349; a `GeometryHandleId` is only unique
    /// within its own kernel's handle space). See its two write sites and
    /// its read site in `available_for_op` below.
    named_step_reprs: &'a mut HashMap<String, ReprKind>,
    topology_attribute_table: &'a mut TopologyAttributeTable,
    swept_kind_table: &'a mut SweptKindTable,
    /// Terminal output [`ReprKind`] surfaced by the executor for the post-call
    /// `eval_state.snapshot.graph.realizations[id].produced_repr` write
    /// (task ε / 3436 step-10). See struct-level docstring above for the full
    /// write contract.
    produced_repr_out: &'a mut Option<ReprKind>,
}

impl<'a> RealizationOutputs<'a> {
    /// Positional constructor mirroring struct-declaration field order
    /// (tasks 3119 + 3133).  Call sites don't need to repeat field names;
    /// argument order is fixed by the struct definition.  Line count at
    /// each call site is unchanged from struct-literal form — the trade-off
    /// is fewer redundant identifiers vs. the named-field self-documentation
    /// of struct-literal syntax.
    fn new(
        step_handles: &'a mut Vec<KernelHandle>,
        named_steps: &'a mut HashMap<String, KernelHandle>,
        named_step_reprs: &'a mut HashMap<String, ReprKind>,
        topology_attribute_table: &'a mut TopologyAttributeTable,
        swept_kind_table: &'a mut SweptKindTable,
        produced_repr_out: &'a mut Option<ReprKind>,
    ) -> Self {
        Self {
            step_handles,
            named_steps,
            named_step_reprs,
            topology_attribute_table,
            swept_kind_table,
            produced_repr_out,
        }
    }
}

/// Bundle of the positional inputs `Engine::execute_realization_ops` reads
/// from. Input-side twin of [`RealizationOutputs`] above (task 3119 → task
/// 5054 ζ): together the two structs are the executor's full parameter
/// surface — `RealizationOpsInput` for what the op loop reads,
/// `RealizationOutputs` for the `&mut` tables it writes into.
///
/// Split into two constructor tiers to end the historical "thread a new arg
/// through every call site" signature-churn class (survey §H2, 11+ commits):
/// the 14 CORE borrows that have no meaningful default (`&`/`&mut` refs
/// can't implement `Default`, and these have not been the churn axis) are
/// positional in [`Self::new`]; the 8 ORTHOGONAL fields below — the fields
/// that HAVE been the historical churn axis — default in `new` and are set
/// via chainable `with_*` setters. A future orthogonal addition touches only
/// this struct and its producing call site, instead of every call site.
struct RealizationOpsInput<'a> {
    kernels: &'a mut BTreeMap<String, Box<dyn GeometryKernel>>,
    registry: &'a BTreeMap<String, &'a CapabilityDescriptor>,
    default_kernel_name: &'a str,
    operations: &'a [reify_compiler::CompiledGeometryOp],
    values: &'a ValueMap,
    functions: &'a [CompiledFunction],
    meta_map: &'a HashMap<String, HashMap<String, String>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    realization_id: &'a RealizationNodeId,
    realization_name: Option<&'a str>,
    realization_span: SourceSpan,
    kernel_error_out: &'a mut Option<ErrorRef>,
    realization_cache: &'a mut RealizationCache<KernelHandle>,
    demanded_tol: Option<f64>,
    // Task 4050 step-8: the realization's requested terminal [`ReprKind`]
    // (υ-derived in build/build_snapshot; `ReprKind::BRep` everywhere else).
    // Each op dispatches at this repr; a `None` plan with
    // `demanded_repr != BRep` falls back to a BRep dispatch (design_decision
    // 3) so a Mesh demand no linked kernel can satisfy routes BRep instead
    // of erroring. Slotted next to `demanded_tol`.
    demanded_repr: ReprKind,
    // Task 4092 step-18: whether this realization is *boundary*-demanded
    // (a registered boundary-demanding consumer references it). When `true`
    // AND `demanded_repr == VolumeMesh` AND the terminal is a BRep, the
    // VolumeMesh realization edge builds face anchors on the source kernel
    // and routes the surface through the gmsh
    // `mesh_surface_to_volume_attributed` producer, threading a
    // `BoundaryAssociation` onto the realized mesh; any failure degrades to
    // the plain `mesh_surface_to_volume` path (boundary `None`). `false`
    // everywhere boundary is not demanded (the tessellate/query paths and
    // every non-FEA realization), keeping existing VolumeMesh consumers
    // byte-identical.
    demanded_boundary: bool,
    // Task ε (3436) step-12: caller-write dispatch-count instrumentation
    // channel. Incremented once per `dispatch(...)` call inside the per-op
    // loop. The caller (build / build_snapshot / tessellate_*) resets the
    // backing `Engine::last_dispatch_count` field to 0 at the entry-point
    // and passes a mutable reference into it; the cache-hit short-circuit
    // returns BEFORE the loop, so the counter stays at 0 on a re-hit.
    dispatch_count: &'a mut usize,
    // Task ε (4741): per-realization sibling of `dispatch_count`. Bumped at
    // the SAME dispatch site, keyed by `realization_id`, so the caller's
    // `Engine::last_dispatch_count_by_realization` map attributes each
    // geometry-kernel dispatch to the realization that issued it. The
    // cache-hit short-circuit returns BEFORE the loop, so a re-hit adds
    // nothing for this realization (stays absent / unchanged).
    dispatch_count_by_realization: &'a mut HashMap<RealizationNodeId, usize>,
    // Task #3443 (ο): module-scoped `#kernel(...)` pragma preference.
    // `Some(name)` steers the terminal-stage kernel selection in
    // `dispatcher::dispatch` when the named kernel is registered and its
    // descriptor supports the demanded (op, repr); absent/unsatisfiable
    // falls through to the existing lex-min scan (PRD §5 "warning, not
    // error"). Callers on the build/tessellate entry-point paths supply
    // `module.kernel_pragma.as_deref()`; the tolerance-budget query and
    // the `DispatchTestState` pragma-agnostic tests pass `None`.
    prefer_kernel: Option<&'a str>,
    // Task 3437 (ζ): only the TERMINAL realization of an entity (the one
    // with the highest index, i.e. `r_idx + 1 == template.realizations.len()`)
    // should probe or insert into the `RealizationCache`. Intermediate
    // realizations all share the same `entity` cache key; if we probe/insert
    // for them we get false hits (realization N finds realization N-1's
    // result for the same entity key) which violates the per-build
    // reset invariant and produces wrong geometry (the intermediate let-
    // binding gets the terminal's handle instead of its own).
    is_terminal_realization: bool,
    // Task 4744 β (step-16): bundled morph-dispatch inputs. When a producer
    // + prior source + new-BRep graph are all present, the VolumeMesh
    // dispatch block attempts a connectivity-preserving morph before
    // remeshing (PRD §4.3); `disabled()` (every call site in step-16) keeps
    // the arm dormant so behaviour is byte-identical until a producer is
    // registered (step-18/22) and the e2e (step-19/20) drives the active
    // path.
    morph_io: crate::morph_producer::MorphDispatchIo<'a>,
    // GR-034 (task #3445): warn threshold for the long-chain diagnostic
    // (`LongChainRealization`). Threaded from the caller so the wiring
    // test can inject `Duration::ZERO` deterministically (env mutation is
    // unsafe in edition 2024). Production callers pass
    // `crate::dispatcher::long_chain_threshold_from_env()`.
    long_chain_threshold: Duration,
}

impl<'a> RealizationOpsInput<'a> {
    /// Positional constructor over the 14 CORE borrows that have no
    /// meaningful default — `&`/`&mut` refs can't implement `Default`, and
    /// these fields have not been the historical churn axis (see the
    /// struct-level doc). The 8 ORTHOGONAL fields default here to their
    /// documented values and are overridden via the chainable `with_*`
    /// setters below.
    #[allow(clippy::too_many_arguments)]
    fn new(
        kernels: &'a mut BTreeMap<String, Box<dyn GeometryKernel>>,
        registry: &'a BTreeMap<String, &'a CapabilityDescriptor>,
        default_kernel_name: &'a str,
        operations: &'a [reify_compiler::CompiledGeometryOp],
        values: &'a ValueMap,
        functions: &'a [CompiledFunction],
        meta_map: &'a HashMap<String, HashMap<String, String>>,
        diagnostics: &'a mut Vec<Diagnostic>,
        realization_id: &'a RealizationNodeId,
        realization_span: SourceSpan,
        kernel_error_out: &'a mut Option<ErrorRef>,
        realization_cache: &'a mut RealizationCache<KernelHandle>,
        dispatch_count: &'a mut usize,
        dispatch_count_by_realization: &'a mut HashMap<RealizationNodeId, usize>,
    ) -> Self {
        Self {
            kernels,
            registry,
            default_kernel_name,
            operations,
            values,
            functions,
            meta_map,
            diagnostics,
            realization_id,
            realization_name: None,
            realization_span,
            kernel_error_out,
            realization_cache,
            demanded_tol: None,
            demanded_repr: ReprKind::BRep,
            demanded_boundary: false,
            dispatch_count,
            dispatch_count_by_realization,
            prefer_kernel: None,
            is_terminal_realization: false,
            morph_io: crate::morph_producer::MorphDispatchIo::disabled(),
            // Cheap PRD-default constant, NOT `long_chain_threshold_from_env()`:
            // `new()` runs once per realization on the hot build path, and
            // every production call site immediately overrides this via
            // `.with_long_chain_threshold(long_chain_threshold)` with a value
            // it already resolved once at eval-loop entry (see the "GR-034:
            // resolve once per eval-loop entry" call sites). Reading + parsing
            // the env var here would be redundant work whose result is always
            // discarded on that path. Equal to `long_chain_threshold_from_env()`
            // when the env var is unset, so callers that omit the override
            // (the `run`/`run_demand` test wrapper) see unchanged behavior.
            long_chain_threshold: Duration::from_millis(
                crate::dispatcher::LONG_CHAIN_DEFAULT_THRESHOLD_MS,
            ),
        }
    }

    /// Override the realization's display name (default `None`, an
    /// anonymous realization).
    fn with_realization_name(mut self, v: Option<&'a str>) -> Self {
        self.realization_name = v;
        self
    }

    /// Override the demanded tolerance (default `None` — no tolerance
    /// contract, no cache write; see `Engine::execute_realization_ops`'s
    /// `demanded_tol` + `realization_cache` doc).
    fn with_demanded_tol(mut self, v: Option<f64>) -> Self {
        self.demanded_tol = v;
        self
    }

    /// Override the demanded terminal [`ReprKind`] (default `ReprKind::BRep`).
    fn with_demanded_repr(mut self, v: ReprKind) -> Self {
        self.demanded_repr = v;
        self
    }

    /// Override whether this realization is boundary-demanded (default
    /// `false`).
    fn with_demanded_boundary(mut self, v: bool) -> Self {
        self.demanded_boundary = v;
        self
    }

    /// Override the `#kernel(...)` pragma preference (default `None`).
    fn with_prefer_kernel(mut self, v: Option<&'a str>) -> Self {
        self.prefer_kernel = v;
        self
    }

    /// Override whether this is the entity's terminal realization (default
    /// `false` — conservative: no `RealizationCache` probe/insert).
    fn with_is_terminal_realization(mut self, v: bool) -> Self {
        self.is_terminal_realization = v;
        self
    }

    /// Override the morph-dispatch IO bundle (default
    /// `MorphDispatchIo::disabled()` — the morph arm never fires).
    fn with_morph_io(mut self, v: crate::morph_producer::MorphDispatchIo<'a>) -> Self {
        self.morph_io = v;
        self
    }

    /// Override the long-chain-diagnostic warn threshold (default
    /// `Duration::from_millis(crate::dispatcher::LONG_CHAIN_DEFAULT_THRESHOLD_MS)`
    /// — a cheap constant, not an env read; see the comment above this field's
    /// initializer in [`Self::new`] for why).
    fn with_long_chain_threshold(mut self, v: Duration) -> Self {
        self.long_chain_threshold = v;
        self
    }
}

/// One ordered action in a template's per-build schedule walk (task 4358 ε).
///
/// Under [`crate::engine_fixpoint::BuildScheduler::UnifiedDag`] the per-template
/// realization loop is driven by `run_unified_pass`'s Kahn order rather than
/// declaration order, so a curated selector value-cell (e.g. `edges_at_height`)
/// is hydrated at its scheduled slot BEFORE the realization that consumes it
/// (the curated `fillet(solid, edges, radius)`). Under `LegacyMultiPass` the
/// walk is simply `[Realize(0), Realize(1), …]` in declaration order with no
/// interleaved `HydrateCell` steps (selectors resolve in the post-process block,
/// exactly as before) — so the legacy path stays byte-identical.
enum BuildStep {
    /// Run `execute_realization_ops` for `template.realizations[usize]`.
    Realize(usize),
    /// Hydrate the named value cell at its scheduled slot (selector / geometry
    /// query) so a later realization in the schedule sees its resolved value.
    HydrateCell(reify_core::ValueCellId),
}

/// Task 3441 / 3814: seed compound-key entries `<sub>.<member> → handle` from
/// each non-collection sub's completed snapshot in `module_named_steps`.
///
/// **Two-mode behaviour (task 3814):**
///
/// * **No-args path** (`sub.args.is_empty()`): copies entries from
///   `module_named_steps[sub.structure_name]` into `named_steps["<sub>.<m>"]`
///   verbatim.  Two subs of the same child template therefore share the same
///   set of handles — `sub a = Inner(); sub b = Inner();` makes `a.body` and
///   `b.body` resolve to identical kernel handles.  Pinned by the
///   `cross_sub_same_template_subs_share_kernel_handle` regression test.
///
/// * **Override path** (`!sub.args.is_empty()`): re-executes the child
///   template's realization ops in a per-instance value scope built by
///   cloning `values` and overlaying, for each `(param_name, _)` in
///   `sub.args`, the scoped value at
///   `ValueCellId("<parent>.<sub_name>", param_name)` (already evaluated by
///   `unfold.rs::elaborate_child_instance`) into
///   `ValueCellId(child_template.name, param_name)`.  The resulting
///   per-instance handles override the structure-keyed snapshot entries.
///   Each non-collection sub with args gets its own independent re-execution,
///   so two same-template subs with distinct args produce distinct handles.
///   Pinned by the `cross_sub_two_subs_with_distinct_overrides_get_distinct_handles`
///   regression test.
///
/// No entries are produced for collection subs (compile-side blocks those),
/// or for subs whose child template isn't yet in `module_named_steps`
/// (forward-declared / recursive; fall through to the runtime error path).
///
/// On the override path, kernel errors / compile errors for a realization's
/// ops append a `Diagnostic::error` to `diagnostics` (mirroring
/// `execute_realization_ops`) and skip the rest of that realization's ops.
/// Error diagnostics carry a `DiagnosticLabel` at `sub.span` so the editor
/// can underline the sub-component declaration site.
///
/// Per-instance ops intentionally skip `topology_attribute_table` /
/// `swept_kind_table` population — those tables
/// are populated for the PARENT's own realization ops; the per-instance
/// pre-pass exists solely to produce the kernel handle referenced by
/// `GeomRef::Sub("<sub>.<member>")`.
///
/// **Scope boundary (v0.1):** one level of override depth only (parent →
/// direct child).  Nested sub-of-sub override propagation (Outer→Mid→Inner
/// where Mid passes args to Inner) is left for a follow-up task.  The
/// `GeomRef::Sub` resolver inside child ops is intentionally given an EMPTY
/// named-steps map, so any `self.<innersub>.body` reference inside the child's
/// own realization will produce a clear "unresolvable GeomRef::Sub" diagnostic
/// rather than accidentally resolving against the parent's scope.  Pinned by
/// `cross_sub_nested_sub_in_override_path_produces_compile_error`.
///
/// **Performance note:** the override path runs `kernel.execute_with_history`
/// for every op of every named realization of every overridden sub on EACH
/// invocation of this helper — including the invocation from
/// `tessellate_from_values`.  For the OCCT kernel, each call is real geometry
/// compute.  A same-call deduplicate cache (`per_call_dedup`) inside this
/// function eliminates redundant kernel ops when multiple subs of the same
/// child template share identical override values within one invocation.
/// Cross-call deduplication (across separate `build` / `tessellate_from_values`
/// calls) is left for a follow-up task.
#[allow(clippy::too_many_arguments)]
fn seed_cross_sub_named_steps(
    template: &reify_compiler::TopologyTemplate,
    module_named_steps: &HashMap<String, HashMap<String, KernelHandle>>,
    named_steps: &mut HashMap<String, KernelHandle>,
    kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &[reify_compiler::TopologyTemplate],
) {
    use reify_core::identity::ValueCellId;

    // Same-call dedup: (child_template_name, args_fingerprint, realization_name) → handle.
    // Two subs of the same child with identical override declarations share one
    // kernel-op sequence per invocation of this helper.  Uses Debug format of
    // `sub.args` (a `Vec<(String, CompiledExpr)>`) as the fingerprint — safe
    // because two syntactically-identical declarations always produce the same
    // effective override values via `elaborate_child_instance`.
    let mut per_call_dedup: HashMap<(String, String, String), KernelHandle> = HashMap::new();

    for sub in &template.sub_components {
        if sub.is_collection {
            continue;
        }

        if sub.args.is_empty() {
            // ── no-args path (existing behaviour) ───────────────────────────
            if let Some(child_snapshot) = module_named_steps.get(&sub.structure_name) {
                for (member, handle) in child_snapshot {
                    named_steps.insert(format!("{}.{}", sub.name, member), *handle);
                }
            }
        } else {
            // ── override path: per-instance re-realization ─────────────────
            //
            // 1. Locate the child template.  If it isn't in the module (e.g.
            //    an external or forward-declared structure) skip silently —
            //    the missing-structure diagnostic was already emitted during
            //    compilation.
            let child_template = match reify_compiler::find_template(templates, &sub.structure_name)
            {
                Some(t) => t,
                None => continue,
            };

            // 2. Obtain the default kernel.  The entry-point guards in
            //    `build` / `build_snapshot` / `tessellate_from_values` all
            //    verify `kernels.contains_key(default_kernel_name)` before
            //    entering the template loop, so this is unreachable when the
            //    kernel is absent.  Skip silently if somehow absent.
            let kernel = match kernels.get_mut(default_kernel_name) {
                Some(k) => k.as_mut(),
                None => continue,
            };

            // 3. Build per-instance overlay: clone the global `values` map
            //    and overwrite `ValueCellId(child_template.name, param_name)`
            //    with the scoped override value already computed by
            //    `unfold.rs::elaborate_child_instance` and stored at
            //    `ValueCellId("<parent>.<sub_name>", param_name)`.
            //
            //    Invariant: for every `(param_name, _)` in `sub.args`, a
            //    scoped cell `ValueCellId("<parent>.<sub>", param_name)` MUST
            //    exist in `values` — `elaborate_child_params_only` in
            //    `crates/reify-eval/src/unfold.rs:292-358` populates it
            //    unconditionally (override present → override value; absent →
            //    default value from child template).  A missing key means the
            //    eval phase failed to populate that cell before `build` was
            //    called, which would be a bug in the eval pipeline.
            let mut values_override = values.clone();
            let args_fingerprint = format!("{:?}", sub.args);
            for (param_name, _) in &sub.args {
                let scoped_key = ValueCellId::new(
                    format!("{}.{}", template.name, sub.name),
                    param_name.as_str(),
                );
                // `elaborate_child_params_only` guarantees this key exists.
                // The debug_assert catches regressions in test builds; in
                // release the silent fallback keeps child-template defaults.
                debug_assert!(
                    values.contains(&scoped_key),
                    "expected scoped override cell {:?} in values map (populated by \
                     unfold.rs::elaborate_child_params_only for sub {}.{} param {}); \
                     missing cell means eval phase failed to seed this param before build",
                    scoped_key,
                    template.name,
                    sub.name,
                    param_name,
                );
                if let Some(val) = values.get(&scoped_key) {
                    let child_key =
                        ValueCellId::new(child_template.name.as_str(), param_name.as_str());
                    values_override.insert(child_key, val.clone());
                }
            }

            // 4. Re-execute each named realization against the override values.
            //    Delegates to `realize_overridden_instance_into` — the shared
            //    helper used by both cross-sub and cross-let override paths.
            realize_overridden_instance_into(
                child_template,
                &values_override,
                kernel,
                default_kernel_name,
                functions,
                meta_map,
                diagnostics,
                &mut per_call_dedup,
                &sub.name,
                &args_fingerprint,
                &template.name,
                sub.span,
                "sub-component override declared here",
                named_steps,
            );
        }
    }
}

/// Shared per-realization re-execution loop used by both
/// [`seed_cross_sub_named_steps`] and [`seed_cross_let_named_steps`] on their
/// override paths.
///
/// For each named realization in `child_template`, compiles and executes the op
/// sequence against `values_override` and writes the terminal [`KernelHandle`]
/// into `named_steps` under `"<binding_prefix>.<realization_name>"`. Unnamed
/// realizations are skipped.
///
/// Same-call dedup via `per_call_dedup`: two bindings/subs of the same child def
/// with identical override declarations share one kernel-op sequence per invocation.
/// The dedup key is `(child_template.name, args_fingerprint, realization_name)`.
///
/// Diagnostics use `parent_name` + `binding_prefix` for context, and `span`/`label_msg`
/// as the secondary label
/// (`"sub-component override declared here"` vs `"let-binding declared here"`).
#[allow(clippy::too_many_arguments)]
fn realize_overridden_instance_into(
    child_template: &reify_compiler::TopologyTemplate,
    values_override: &ValueMap,
    kernel: &mut dyn GeometryKernel,
    default_kernel_name: &str,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    per_call_dedup: &mut HashMap<(String, String, String), KernelHandle>,
    binding_prefix: &str,
    args_fingerprint: &str,
    parent_name: &str,
    span: SourceSpan,
    label_msg: &'static str,
    named_steps: &mut HashMap<String, KernelHandle>,
) {
    for realization in &child_template.realizations {
        let realization_name = match realization.name.as_deref() {
            Some(n) => n,
            None => continue,
        };

        let dedup_key = (
            child_template.name.clone(),
            args_fingerprint.to_string(),
            realization_name.to_string(),
        );
        if let Some(&cached) = per_call_dedup.get(&dedup_key) {
            named_steps.insert(format!("{}.{}", binding_prefix, realization_name), cached);
            continue;
        }

        let mut per_instance_step_handles: Vec<GeometryHandleId> = Vec::new();
        let mut realization_ok = true;
        // v0.1 scope boundary: empty child named-steps so any `self.<innersub>.body`
        // reference produces "unresolvable GeomRef::Sub" rather than accidentally
        // resolving against the parent's scope.
        let child_named_steps: HashMap<String, KernelHandle> = HashMap::new();

        for op in &realization.operations {
            let geom_op = match compile_geometry_op(
                op,
                values_override,
                &per_instance_step_handles,
                functions,
                meta_map,
                &child_named_steps,
                diagnostics,
            ) {
                Ok(g) => g,
                Err(msg) => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "per-instance re-realization compile error for {}.{}.{}: {}",
                            parent_name, binding_prefix, realization_name, msg
                        ))
                        .with_label(DiagnosticLabel::new(span, label_msg)),
                    );
                    realization_ok = false;
                    break;
                }
            };

            match kernel.execute_with_history(&geom_op) {
                Ok((handle, _)) => {
                    per_instance_step_handles.push(handle.id);
                }
                Err(e) => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "per-instance re-realization kernel error for {}.{}.{}: {}",
                            parent_name, binding_prefix, realization_name, e
                        ))
                        .with_label(DiagnosticLabel::new(span, label_msg)),
                    );
                    realization_ok = false;
                    break;
                }
            }
        }

        if realization_ok && let Some(&final_handle_id) = per_instance_step_handles.last() {
            let final_handle = KernelHandle {
                kernel: kernel_id_for_registry_name(default_kernel_name),
                id: final_handle_id,
            };
            named_steps.insert(format!("{}.{}", binding_prefix, realization_name), final_handle);
            per_call_dedup.insert(dedup_key, final_handle);
        }
    }
}

/// Task 4628: per-binding cross-`let` realization snapshot keying.
///
/// Mirrors [`seed_cross_sub_named_steps`] but targets `let`-bound
/// `StructureRef` value cells (not `sub` components). For each
/// [`ValueCellDecl`] with `cell_type == Type::StructureRef(def_name)` and
/// `default_expr == Some(StructureInstanceCtor { ordered_args, .. })`,
/// populates `named_steps` with per-binding `"<binding>.<member>"` handles:
///
/// - **no-args** (`ordered_args` empty): copies
///   `module_named_steps[def_name][member]` entries under key
///   `"<binding>.<member>"` — byte-identical to the single-instance capstone
///   path (step-9), preserving child post-process handles.
///
/// - **args**: builds a per-instance value overlay by cloning `values` and,
///   for each `(param, expr)` in `ordered_args`, evaluating `expr` in the
///   PARENT scope (`reify_expr::eval_expr` + [`crate::eval_ctx_with_meta`])
///   and overlaying `ValueCellId(child_template.name, param)` → value. Then
///   re-executes each named child realization against the overlay via
///   [`realize_overridden_instance_into`] (shared with the cross-sub override
///   path) and writes each terminal `KernelHandle` to
///   `named_steps["<binding>.<member>"]`.
///
/// Called from `build()`'s per-template loop immediately after
/// `seed_cross_sub_named_steps`, running unconditionally on both
/// `LegacyMultiPass` and `UnifiedDag` schedulers (same as
/// `seed_cross_sub_named_steps`). Under `LegacyMultiPass` the args-path kernel
/// re-realizations are wasted work because `check_constraints_post_geometry` is
/// gated on `UnifiedDag`; geometry output (`terminal_handles`, `step_handles`)
/// is byte-identical since `named_steps` entries are only read by the
/// constraint executor. `snapshot_named_steps` captures the per-binding handles
/// into `module_named_steps[template.name]`.
/// `check_constraints_post_geometry` clones that map and reads the per-binding
/// handles via `resolve_geometry_handle_arg`'s `IndexAccess` arm (which
/// already reconstructs `"<binding>.<member>"`).
///
/// Forward-declared or external defs not yet present in
/// `module_named_steps` are skipped silently on the no-args path, and
/// handled via [`reify_compiler::find_template`] on the args path (which
/// searches the full templates slice regardless of order).
///
/// # Scope boundary
///
/// One level of override depth only — nested `StructureRef`-in-`StructureRef`
/// chains are out of scope (same limitation as `seed_cross_sub_named_steps`).
/// The child's op compiler receives an EMPTY `child_named_steps` map, so any
/// `self.<innersub>.body` reference inside the child's realization produces a
/// clear "unresolvable GeomRef::Sub" diagnostic rather than accidentally
/// resolving against the parent's scope.
#[allow(clippy::too_many_arguments)]
fn seed_cross_let_named_steps(
    template: &reify_compiler::TopologyTemplate,
    module_named_steps: &HashMap<String, HashMap<String, KernelHandle>>,
    named_steps: &mut HashMap<String, KernelHandle>,
    kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &[reify_compiler::TopologyTemplate],
) {
    use reify_core::identity::ValueCellId;

    // Same-call dedup: (child_template_name, args_fingerprint, realization_name) → handle.
    // Two let-bindings of the same child def with identical override declarations
    // share one kernel-op sequence per invocation.
    let mut per_call_dedup: HashMap<(String, String, String), KernelHandle> = HashMap::new();

    for cell in &template.value_cells {
        let reify_core::Type::StructureRef(def_name) = &cell.cell_type else {
            continue;
        };

        // Only cells whose default_expr is a StructureInstanceCtor (i.e. actual
        // let-bound instances, not bare StructureRef params without a ctor).
        let ordered_args = match cell.default_expr.as_ref().map(|e| &e.kind) {
            Some(reify_ir::CompiledExprKind::StructureInstanceCtor { ordered_args, .. }) => {
                ordered_args
            }
            _ => continue,
        };

        let binding_name = &cell.id.member;

        if ordered_args.is_empty() {
            // ── no-args path: copy the def's shared snapshot ────────────────
            if let Some(child_snapshot) = module_named_steps.get(def_name.as_str()) {
                for (member, handle) in child_snapshot {
                    named_steps.insert(format!("{}.{}", binding_name, member), *handle);
                }
            }
        } else {
            // ── override path: per-instance re-realization ─────────────────

            // 1. Locate the child template.  If absent (forward-declared / external
            //    def) skip silently — same policy as seed_cross_sub_named_steps.
            let child_template =
                match reify_compiler::find_template(templates, def_name.as_str()) {
                    Some(t) => t,
                    None => continue,
                };

            // 2. Obtain the default kernel.  The build() entry-point guards verify
            //    `kernels.contains_key(default_kernel_name)` before entering the
            //    template loop, so this is expected to always succeed.  A missing
            //    kernel here is a misconfiguration anomaly (distinct from the
            //    forward-declared-def skip above), so emit a debug_assert to
            //    make the degradation attributable rather than silent.
            let kernel = match kernels.get_mut(default_kernel_name) {
                Some(k) => k.as_mut(),
                None => {
                    debug_assert!(
                        false,
                        "seed_cross_let_named_steps: default kernel '{}' absent on args \
                         path for binding '{}'; fold degrades to Indeterminate. \
                         build() entry-point guard should have prevented this.",
                        default_kernel_name,
                        binding_name,
                    );
                    continue;
                }
            };

            // 3. Build per-instance overlay: clone the global `values` map and
            //    overwrite `ValueCellId(child_template.name, param)` with the result
            //    of evaluating each arg expr in the PARENT scope.  Non-overridden
            //    child params already hold their defaults in `values` from the child
            //    def's own top-level eval; only supplied overrides need overlaying.
            let mut values_override = values.clone();
            let args_fingerprint = format!("{:?}", ordered_args);
            for (param_name, arg_expr) in ordered_args {
                let val = reify_expr::eval_expr(
                    arg_expr,
                    &crate::eval_ctx_with_meta(values, functions, meta_map),
                );
                let child_key =
                    ValueCellId::new(child_template.name.as_str(), param_name.as_str());
                values_override.insert(child_key, val);
            }

            // 4. Re-execute each named realization against the override values.
            //    Delegates to `realize_overridden_instance_into` — the shared
            //    helper used by both cross-sub and cross-let override paths.
            realize_overridden_instance_into(
                child_template,
                &values_override,
                kernel,
                default_kernel_name,
                functions,
                meta_map,
                diagnostics,
                &mut per_call_dedup,
                binding_name,
                &args_fingerprint,
                &template.name,
                cell.span,
                "let-binding declared here",
                named_steps,
            );
        }
    }
}

/// task-4147: per-instance re-realization for overridden subs in the surfacing
/// walk (`walk_placed_realizations`).
///
/// When `sub b = Bar(len: 600mm)` is surfaced via the containment walk, the
/// child's default Phase-A handles (built against `len = 200mm`) give the wrong
/// geometry.  This helper re-executes every realization in `child_template`
/// against a per-instance value overlay (same override-scope construction as
/// `seed_cross_sub_named_steps`) and returns one `Option<KernelHandle>` per
/// realization index, aligned with `terminal_handles[child_idx]`.
///
/// **One-level boundary**: the child's op compiler receives an EMPTY
/// `child_named_steps` map, matching the v0.1 boundary documented in
/// `seed_cross_sub_named_steps` — nested sub-of-sub override propagation
/// is deferred.
///
/// Returns `None` for realizations that fail (compile or kernel error) or
/// produce no geometry.  On success, the terminal `KernelHandle` is tagged
/// with the default kernel's `KernelId` via `kernel_id_for_registry_name`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn realize_sub_override_handles(
    parent_name: &str,
    sub: &SubComponentDecl,
    child_template: &TopologyTemplate,
    geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
    default_kernel_name: &str,
    values: &ValueMap,
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Option<KernelHandle>> {
    use reify_core::identity::ValueCellId;

    debug_assert!(
        !sub.args.is_empty(),
        "realize_sub_override_handles called for arg-free sub {}.{}; \
         caller must guard on !sub.args.is_empty()",
        parent_name,
        sub.name
    );

    let n = child_template.realizations.len();

    let kernel = match geometry_kernels.get_mut(default_kernel_name) {
        Some(k) => k.as_mut(),
        None => return vec![None; n],
    };

    // Build per-instance value overlay: clone the global map and overlay
    // `ValueCellId(parent.sub, param)` → `ValueCellId(child, param)` for
    // each constructor arg.  `elaborate_child_instance` guarantees the
    // scoped cells exist; a missing key keeps the child's existing default.
    let mut values_override = values.clone();
    for (param_name, _) in &sub.args {
        let scoped_key =
            ValueCellId::new(format!("{}.{}", parent_name, sub.name), param_name.as_str());
        if let Some(val) = values.get(&scoped_key) {
            let child_key = ValueCellId::new(child_template.name.as_str(), param_name.as_str());
            values_override.insert(child_key, val.clone());
        }
    }

    // v0.1 boundary: empty child named-steps (no nested sub-of-sub propagation).
    let child_named_steps: HashMap<String, KernelHandle> = HashMap::new();

    let mut result: Vec<Option<KernelHandle>> = Vec::with_capacity(n);

    for realization in &child_template.realizations {
        let mut per_instance_step_handles: Vec<GeometryHandleId> = Vec::new();
        let mut realization_ok = true;

        for op in &realization.operations {
            let geom_op = match compile_geometry_op(
                op,
                &values_override,
                &per_instance_step_handles,
                functions,
                meta_map,
                &child_named_steps,
                diagnostics,
            ) {
                Ok(g) => g,
                Err(msg) => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "per-instance re-realization compile error for {}.{}: {}",
                            parent_name, sub.name, msg
                        ))
                        .with_label(DiagnosticLabel::new(
                            sub.span,
                            "sub-component override declared here",
                        )),
                    );
                    realization_ok = false;
                    break;
                }
            };

            match kernel.execute_with_history(&geom_op) {
                Ok((handle, _)) => {
                    per_instance_step_handles.push(handle.id);
                }
                Err(e) => {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "per-instance re-realization kernel error for {}.{}: {}",
                            parent_name, sub.name, e
                        ))
                        .with_label(DiagnosticLabel::new(
                            sub.span,
                            "sub-component override declared here",
                        )),
                    );
                    realization_ok = false;
                    break;
                }
            }
        }

        if realization_ok {
            if let Some(&final_id) = per_instance_step_handles.last() {
                result.push(Some(KernelHandle {
                    kernel: kernel_id_for_registry_name(default_kernel_name),
                    id: final_id,
                }));
            } else {
                result.push(None); // realization produced no geometry ops
            }
        } else {
            result.push(None);
        }
    }

    result
}

/// Task 3441: snapshot this template's `named_steps` under its template name
/// so a subsequent template with `sub <s> = <T>()` can seed compound-key
/// entries via [`seed_cross_sub_named_steps`].
///
/// Takes `named_steps` by value (amendment for the prior `.clone()` at this
/// call site) — the per-iteration `named_steps` is about to fall out of scope
/// at the end of the loop body, and the post-process helpers above (which
/// are the only readers between primary loop and this snapshot) read by
/// shared reference and do not need the local binding afterwards.
fn snapshot_named_steps(
    template: &reify_compiler::TopologyTemplate,
    named_steps: HashMap<String, KernelHandle>,
    module_named_steps: &mut HashMap<String, HashMap<String, KernelHandle>>,
) {
    module_named_steps.insert(template.name.clone(), named_steps);
}

/// Dispatch on `attribute_history` to populate `topology_attribute_table`
/// for sweep-style ops (extrude / revolve, currently). Called by
/// `Engine::execute_realization_ops` immediately after the existing
/// primitive-attribute seeding step.
///
/// For `AttributeHistory::None` this is a zero-cost no-op (no kernel
/// `extract_*` calls), so non-overriding kernels and non-attributable ops
/// pay nothing. For `Extrude(history)` / `Revolve(history)` it extracts
/// the profile and result face/edge handles in canonical TopExp order
/// and forwards to the appropriate per-op helper.
///
/// Failures (kernel `extract_*` errors, helper out-of-range index errors)
/// are returned to the caller, which surfaces them as `Diagnostic::warning`
/// and continues. Per task-2574 design, attribute population is auxiliary
/// metadata — a failure here must NOT regress the realization to Failed.
fn populate_attribute_history(
    table: &mut TopologyAttributeTable,
    kernel_id: KernelId,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    geom_op: &GeometryOp,
    result_handle: GeometryHandleId,
    attribute_history: &AttributeHistory,
) -> Result<(), reify_ir::QueryError> {
    match attribute_history {
        AttributeHistory::None => Ok(()),
        AttributeHistory::Extrude(history) => {
            let profile_handle = match geom_op {
                GeometryOp::Extrude { profile, .. } => *profile,
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::Extrude returned for non-Extrude GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel_id,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Extrude,
            )
        }
        AttributeHistory::Revolve(history) => {
            let profile_handle = match geom_op {
                GeometryOp::Revolve { profile, .. } => *profile,
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::Revolve returned for non-Revolve GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel_id,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Revolve,
            )
        }
        AttributeHistory::Sweep(history) => {
            // GeometryOp::Sweep is single-parent like Extrude/Revolve: the
            // profile is the operand whose sub-shapes propagate into the
            // result; the path/spine is not itself a parent.
            let profile_handle = match geom_op {
                GeometryOp::Sweep { profile, .. } => *profile,
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::Sweep returned for non-Sweep GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_single_parent_sweep_op(
                table,
                kernel_id,
                kernel,
                feature_id,
                profile_handle,
                result_handle,
                history,
                SingleParentSweepKind::Sweep,
            )
        }
        AttributeHistory::Loft(history) => {
            // GeometryOp::Loft is multi-parent: each profile section is a
            // parent; `parent_index` in `face_generated` denotes the
            // section index in `[0, profiles.len())`.
            let profiles = match geom_op {
                GeometryOp::Loft { profiles } => profiles,
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::Loft returned for non-Loft GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_loft_op(
                table, kernel_id, kernel, feature_id, profiles, result_handle, history,
            )
        }
        AttributeHistory::Boolean(history) => {
            // Binary boolean ops (Union/Difference/Intersection): two parents
            // — left (parent_index 0) and right (parent_index 1).
            let (left_handle, right_handle) = match geom_op {
                GeometryOp::Union { left, right }
                | GeometryOp::Difference { left, right }
                | GeometryOp::Intersection { left, right } => (*left, *right),
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::Boolean returned for non-boolean GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_boolean_op(
                table,
                kernel_id,
                kernel,
                feature_id,
                left_handle,
                right_handle,
                result_handle,
                history,
            )
        }
        AttributeHistory::LocalFeature(history) => {
            // Local-feature ops (fillet / chamfer): one target shape.
            let target_handle = match geom_op {
                GeometryOp::Fillet { target, .. }
                | GeometryOp::Chamfer { target, .. }
                | GeometryOp::ChamferAsymmetric { target, .. } => *target,
                _ => {
                    return Err(reify_ir::QueryError::QueryFailed(format!(
                        "AttributeHistory::LocalFeature returned for non-Fillet/Chamfer/\
                         ChamferAsymmetric GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_local_feature_op(
                table,
                kernel_id,
                kernel,
                feature_id,
                target_handle,
                result_handle,
                history,
            )
        }
    }
}

/// Emit one `Severity::Warning` per non-zero topology-correspondence-loss
/// counter found in `attribute_history`.
///
/// Called by `Engine::execute_realization_ops` immediately after
/// `populate_attribute_history` — both live at the same call site where
/// `attribute_history` and `diagnostics` are already in scope.
///
/// Covers all five unconsumed counters across the three op families:
/// - `Boolean`: `silent_drop_count`
/// - `Extrude` / `Revolve` / `Sweep`: `silent_drop_count`,
///   `unsynthesized_profile_edge_count`, `duplicate_parent_subshape_index_count`
/// - `LocalFeature`: `silent_drop_count`
///
/// `Loft` and `None` are explicit no-ops: `LoftOpHistoryRecords` has no
/// counters by design, and `None` means no history was returned.
///
/// Each warning carries [`reify_core::DiagnosticCode::TopologyCorrespondenceDropped`]
/// and a message of the form:
/// `"topology correspondence dropped: {op_kind} {counter_name}={count} context={context}"`.
///
/// The geometry is valid; only persistent-naming correspondence tracking is
/// degraded. Severity is `Warning` (never `Error`) per the task-2574 convention
/// that auxiliary-metadata degradation must not regress the realization to Failed.
fn diagnose_topology_correspondence_drops(
    attribute_history: &AttributeHistory,
    context: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use reify_core::DiagnosticCode;
    // Single canonical emit path: guarantees every warning uses the same
    // message format ("topology correspondence dropped: {op_kind}
    // {counter}={count} context={context}") and the same code, with no risk
    // of the five call sites drifting from each other.
    let mut emit = |op_kind: &str, counter: &str, count: u32| {
        if count > 0 {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "topology correspondence dropped: {op_kind} {counter}={count} context={context}"
                ))
                .with_code(DiagnosticCode::TopologyCorrespondenceDropped),
            );
        }
    };
    match attribute_history {
        AttributeHistory::Boolean(h) => {
            emit("boolean", "silent_drop_count", h.silent_drop_count);
        }
        // Each sweep variant gets its own arm so op_kind is determined
        // exhaustively without a nested re-match or a `_ => "sweep"` wildcard
        // that would silently mislabel any future AttributeHistory variant
        // sharing this arm.
        AttributeHistory::Extrude(h) => {
            emit("extrude", "silent_drop_count", h.silent_drop_count);
            emit(
                "extrude",
                "unsynthesized_profile_edge_count",
                h.unsynthesized_profile_edge_count,
            );
            emit(
                "extrude",
                "duplicate_parent_subshape_index_count",
                h.duplicate_parent_subshape_index_count,
            );
        }
        AttributeHistory::Revolve(h) => {
            emit("revolve", "silent_drop_count", h.silent_drop_count);
            emit(
                "revolve",
                "unsynthesized_profile_edge_count",
                h.unsynthesized_profile_edge_count,
            );
            emit(
                "revolve",
                "duplicate_parent_subshape_index_count",
                h.duplicate_parent_subshape_index_count,
            );
        }
        AttributeHistory::Sweep(h) => {
            emit("sweep", "silent_drop_count", h.silent_drop_count);
            emit(
                "sweep",
                "unsynthesized_profile_edge_count",
                h.unsynthesized_profile_edge_count,
            );
            emit(
                "sweep",
                "duplicate_parent_subshape_index_count",
                h.duplicate_parent_subshape_index_count,
            );
        }
        AttributeHistory::LocalFeature(h) => {
            emit("local_feature", "silent_drop_count", h.silent_drop_count);
        }
        AttributeHistory::Loft(_) | AttributeHistory::None => {
            // No counters in LoftOpHistoryRecords; None means no history returned.
        }
    }
}

/// Propagate local-feature (fillet / chamfer) history onto the result shape.
///
/// Mirrors [`populate_boolean_op`] but extracts target faces/edges/vertices
/// (three parent slices) rather than two operand face/edge slices.
/// Delegates to [`propagate_attributes_via_local_feature_history`] which runs
/// four independent per-stream cross-kind passes (face_modified←faces,
/// face_generated←edges, edge_modified←edges, edge_generated←vertices).
///
/// Failure semantics are identical to [`populate_boolean_op`]: a `QueryError`
/// returned here surfaces as `Diagnostic::warning` at the call site — never a
/// Failed-realization regression (per task-2574 convention).
fn populate_local_feature_op(
    table: &mut TopologyAttributeTable,
    kernel_id: KernelId,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    target_handle: GeometryHandleId,
    result_handle: GeometryHandleId,
    history: &LocalFeatureOpHistoryRecords,
) -> Result<(), reify_ir::QueryError> {
    let target_faces = kernel.extract_faces(target_handle)?;
    let target_edges = kernel.extract_edges(target_handle)?;
    let target_vertices = kernel.extract_vertices(target_handle)?;
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;

    crate::topology_attribute_propagation::propagate_attributes_via_local_feature_history(
        table,
        kernel_id,
        &target_faces,
        &target_edges,
        &target_vertices,
        &result_faces,
        &result_edges,
        history,
        feature_id,
    )
}

/// Build per-cap-face vertex-index-lists by position-matching cap-face vertex
/// BoundingBox payloads against a pre-built result-vertex position table.
///
/// For each `cap_idx` in `cap_face_indices`:
/// - Fetches `result_faces[cap_idx as usize]` as the cap face handle.
/// - Calls `kernel.extract_vertices(cap_face_handle)?` to get the cap-face's
///   vertex handles (these are freshly allocated ids — different from result
///   vertex ids even for the same underlying `TopoDS_Vertex`).
/// - For each cap-vertex, queries `GeometryQuery::BoundingBox` → parses
///   `(xmin, ymin, zmin)` via [`parse_bbox_xyz_min`].
/// - Searches `result_vertex_positions` for a position-match using EXACT f64
///   equality: safe because OCCT's `Bnd_Box` compute on the same
///   `gp_Pnt`-backed `TopoDS_Vertex` is byte-identical regardless of which
///   handle invoked the query.
/// - Pushes the matched result-vertex index (`u32`) into the inner `Vec`.
///   If no match is found (should not occur for valid OCCT geometry), the
///   vertex is silently skipped rather than hard-erroring, so a future kernel
///   variant that breaks shared-vertex identity degrades to auxiliary-metadata
///   loss rather than a geometry-regression diagnostic.
///
/// Returns one inner `Vec<u32>` per entry in `cap_face_indices`.
///
/// # Performance
///
/// `result_vertex_positions` is pre-built once per call-site invocation
/// (O(`result_vertices`) kernel round-trips) so per-cap-vertex position
/// matching is a linear scan over pre-fetched f64 triples — no additional
/// kernel queries inside this helper.  For typical sweep results (≤100
/// result vertices, ≤2 cap faces, ≤20 cap vertices) the comparison loop
/// is bounded at ≤4 000 f64 triple-compares per realization.
fn build_cap_vertex_index_lists(
    kernel: &mut dyn GeometryKernel,
    result_faces: &[GeometryHandleId],
    result_vertex_positions: &[(f64, f64, f64)],
    cap_face_indices: &[u32],
) -> Result<Vec<Vec<u32>>, reify_ir::QueryError> {
    let mut index_lists: Vec<Vec<u32>> = Vec::with_capacity(cap_face_indices.len());
    for &cap_idx in cap_face_indices {
        let cap_face_handle = result_faces.get(cap_idx as usize).copied().ok_or_else(|| {
            reify_ir::QueryError::QueryFailed(format!(
                "cap vertex index list: cap face index {cap_idx} is out of range \
                     for result_faces of len {}",
                result_faces.len()
            ))
        })?;
        let cap_vertices = kernel.extract_vertices(cap_face_handle)?;
        let mut inner: Vec<u32> = Vec::with_capacity(cap_vertices.len());
        for &cap_vertex_handle in &cap_vertices {
            let bbox = kernel.query(&GeometryQuery::BoundingBox(cap_vertex_handle))?;
            let (cx, cy, cz) = parse_bbox_xyz_min(&bbox)?;
            // Linear scan over pre-built result-vertex position table.
            // Exact f64 equality is safe: same underlying TopoDS_Vertex →
            // same Bnd_Box compute → byte-identical xmin/ymin/zmin.
            if let Some(result_idx) = result_vertex_positions
                .iter()
                .position(|&(rx, ry, rz)| rx == cx && ry == cy && rz == cz)
            {
                inner.push(result_idx as u32);
            }
            // No match: cap vertex absent from result-vertex set. Silently
            // skip rather than hard-error so a kernel variant that breaks
            // shared-vertex identity degrades to metadata loss (warning at
            // the populate_attribute_history call site) rather than a
            // Failed geometry regression.
        }
        index_lists.push(inner);
    }
    Ok(index_lists)
}

/// Attempt to extract result vertices and build per-cap-face vertex-index-lists
/// for a single-parent sweep op. Returns `(result_vertices, start_lists, end_lists)`.
///
/// Any failure (e.g. `QueryFailed` from a mock kernel that inherits
/// `GeometryKernel`'s default `extract_vertices`) is treated as auxiliary-
/// metadata failure and silently converted to `(empty, empty, empty)` by the
/// caller — this preserves the primary face/edge seeding path for mock kernels.
/// For real OCCT kernels, this always succeeds.
#[allow(clippy::type_complexity)]
fn try_extract_sweep_cap_vertex_data(
    kernel: &mut dyn GeometryKernel,
    result_faces: &[GeometryHandleId],
    result_handle: GeometryHandleId,
    start_cap_face_indices: &[u32],
    end_cap_face_indices: &[u32],
) -> Result<(Vec<GeometryHandleId>, Vec<Vec<u32>>, Vec<Vec<u32>>), reify_ir::QueryError> {
    let result_vertices = kernel.extract_vertices(result_handle)?;
    let result_vertex_positions: Vec<(f64, f64, f64)> = result_vertices
        .iter()
        .map(|&vh| {
            let bbox = kernel.query(&GeometryQuery::BoundingBox(vh))?;
            parse_bbox_xyz_min(&bbox)
        })
        .collect::<Result<_, _>>()?;
    let start_cap_vertex_index_lists = build_cap_vertex_index_lists(
        kernel,
        result_faces,
        &result_vertex_positions,
        start_cap_face_indices,
    )?;
    let end_cap_vertex_index_lists = build_cap_vertex_index_lists(
        kernel,
        result_faces,
        &result_vertex_positions,
        end_cap_face_indices,
    )?;
    Ok((
        result_vertices,
        start_cap_vertex_index_lists,
        end_cap_vertex_index_lists,
    ))
}

/// Shared helper for the three single-parent sweep variants (extrude,
/// revolve, sweep). Extracts the profile and result face/edge slices
/// from `kernel`, then dispatches to the appropriate per-op propagation
/// helper based on `kind`. Centralised so the extract sequence +
/// error-propagation shape stays uniform across the variants.
///
/// Vertex extraction and cap-vertex-index-list construction are attempted via
/// `try_extract_sweep_cap_vertex_data`. Failure (e.g. `QueryFailed` from a
/// mock kernel that inherits `GeometryKernel`'s default `extract_vertices`)
/// is caught locally — empty vertex slices are passed to the propagation
/// helper, and face/edge seeding proceeds normally. This ensures mock-kernel
/// tests that check face/edge attributes are not broken by the vertex wire.
#[allow(clippy::too_many_arguments)]
fn populate_single_parent_sweep_op(
    table: &mut TopologyAttributeTable,
    kernel_id: KernelId,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    profile_handle: GeometryHandleId,
    result_handle: GeometryHandleId,
    history: &SweepOpHistoryRecords,
    kind: SingleParentSweepKind,
) -> Result<(), reify_ir::QueryError> {
    let profile_faces = kernel.extract_faces(profile_handle)?;
    let profile_edges = kernel.extract_edges(profile_handle)?;
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;

    // Attempt vertex extraction + cap-vertex-index-list construction. A failure
    // here (e.g. `QueryFailed` from a mock kernel) is auxiliary-metadata only:
    // fall back to empty slices and continue with face/edge seeding.
    let (result_vertices, start_cap_vertex_index_lists, end_cap_vertex_index_lists) =
        try_extract_sweep_cap_vertex_data(
            kernel,
            &result_faces,
            result_handle,
            &history.start_cap_face_indices,
            &history.end_cap_face_indices,
        )
        .unwrap_or_else(|_| (Vec::new(), Vec::new(), Vec::new()));

    match kind {
        SingleParentSweepKind::Extrude => populate_extrude_attributes(
            table,
            kernel_id,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        ),
        SingleParentSweepKind::Revolve => populate_revolve_attributes(
            table,
            kernel_id,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        ),
        SingleParentSweepKind::Sweep => populate_sweep_attributes(
            table,
            kernel_id,
            feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        ),
    }
}

/// Multi-parent variant of `populate_single_parent_sweep_op` for
/// `GeometryOp::Loft`. Walks the `profiles` handle list, calls
/// `kernel.extract_faces` / `extract_edges` once per section to build
/// the per-section profile face/edge slice families, extracts the
/// result face/edge slices, and dispatches to
/// `populate_loft_attributes`. Failure semantics preserved (Diagnostic::
/// warning at the call site, no Failed regression per task-2574).
///
/// Duplicate handles in `profile_handles` (legal but unusual — a loft
/// referencing the same section twice) re-extract on each iteration
/// rather than memoising; loft profile counts are typically small (2–8)
/// so the per-call cost is negligible, and a memo would add a HashMap
/// allocation that is unwarranted for the common path. If real models
/// surface heavy duplicate-handle lofts a future task can introduce a
/// `HashMap<GeometryHandleId, Vec<GeometryHandleId>>` cache here.
///
/// The two extractions whose results are currently dropped inside
/// `populate_loft_attributes` (`extract_faces(profile_handle)` per section,
/// `extract_edges(result_handle)` once) are still performed eagerly because:
///   (a) loft profiles are typically wires (≈ 0 faces extracted), so
///       per-section `extract_faces` is near-free;
///   (b) result-edge extraction is a single call;
///   (c) calling `extract_faces` once per section keeps
///       `section_faces.len() == section_edges.len()`, which is the
///       two-way equality pinned by the lockstep `debug_assert_eq!` at the
///       top of `populate_loft_attributes` (see `topology_attribute_propagation.rs`);
///       the additional equality `== profile_handles.len()` is enforced
///       structurally by the single push-per-iteration loop above (one
///       `section_faces.push(...)` and one `section_edges.push(...)` per
///       `profile_handle`).  Skipping `extract_faces` and passing `&[]`
///       would still violate the assertion (because `section_edges` would
///       still be populated per-section).
fn populate_loft_op(
    table: &mut TopologyAttributeTable,
    kernel_id: KernelId,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    profile_handles: &[GeometryHandleId],
    result_handle: GeometryHandleId,
    history: &LoftOpHistoryRecords,
) -> Result<(), reify_ir::QueryError> {
    let mut section_faces: Vec<Vec<GeometryHandleId>> = Vec::with_capacity(profile_handles.len());
    let mut section_edges: Vec<Vec<GeometryHandleId>> = Vec::with_capacity(profile_handles.len());
    for &profile_handle in profile_handles {
        section_faces.push(kernel.extract_faces(profile_handle)?);
        section_edges.push(kernel.extract_edges(profile_handle)?);
    }
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;

    // Attempt vertex extraction + cap-vertex-index-list construction. A failure
    // here (e.g. `QueryFailed` from a mock kernel) is auxiliary-metadata only:
    // fall back to empty slices and continue with face/edge seeding.
    let (result_vertices, start_cap_vertex_index_lists, end_cap_vertex_index_lists) =
        try_extract_sweep_cap_vertex_data(
            kernel,
            &result_faces,
            result_handle,
            &history.start_cap_face_indices,
            &history.end_cap_face_indices,
        )
        .unwrap_or_else(|_| (Vec::new(), Vec::new(), Vec::new()));

    populate_loft_attributes(
        table,
        kernel_id,
        feature_id,
        &section_faces,
        &section_edges,
        &result_faces,
        &result_edges,
        history,
        &result_vertices,
        &start_cap_vertex_index_lists,
        &end_cap_vertex_index_lists,
    )
}

/// Binary-boolean variant of `populate_single_parent_sweep_op` for
/// `GeometryOp::{Union,Difference,Intersection}`.
///
/// Extracts the left and right operand face/edge slices live via
/// `kernel.extract_faces` / `kernel.extract_edges` (the same per-call
/// pattern as `populate_single_parent_sweep_op`), then extracts the result
/// face/edge slices, builds a
/// `BooleanOpParents::Binary { faces: [left, right], edges: [left, right] }`
/// and calls the existing `propagate_attributes_via_brepalgoapi_history`
/// helper (which implements split → `mod_history` `ModEntry` logic).
///
/// Modelled on `populate_single_parent_sweep_op`; failure semantics are
/// identical (returned `QueryError` surfaces as `Diagnostic::warning` at the
/// call site — no Failed regression, per the task-2574 convention).
#[allow(clippy::too_many_arguments)]
fn populate_boolean_op(
    table: &mut TopologyAttributeTable,
    kernel_id: KernelId,
    kernel: &mut dyn GeometryKernel,
    feature_id: &FeatureId,
    left_handle: GeometryHandleId,
    right_handle: GeometryHandleId,
    result_handle: GeometryHandleId,
    history: &BooleanOpHistoryRecords,
) -> Result<(), reify_ir::QueryError> {
    let left_faces = kernel.extract_faces(left_handle)?;
    let left_edges = kernel.extract_edges(left_handle)?;
    let right_faces = kernel.extract_faces(right_handle)?;
    let right_edges = kernel.extract_edges(right_handle)?;
    let result_faces = kernel.extract_faces(result_handle)?;
    let result_edges = kernel.extract_edges(result_handle)?;

    let parents = BooleanOpParents::Binary {
        faces: [left_faces.as_slice(), right_faces.as_slice()],
        edges: [left_edges.as_slice(), right_edges.as_slice()],
    };

    propagate_attributes_via_brepalgoapi_history(
        table,
        kernel_id,
        &parents,
        &result_faces,
        &result_edges,
        history,
        feature_id,
    )
}

/// Non-allocating parent-handle accessor returned by [`parent_handles_for_op`].
///
/// Two variants cover all cases without heap allocation:
///
/// - `Inline([H; 2], len)` — small fixed-capacity buffer with an active
///   length count (`len` ≤ 2).  Covers: zero parents (primitives,
///   curve constructors, `Pipe`), one parent (single-target/-profile ops),
///   and two parents (boolean ops).  Only the first `len` slots contain
///   meaningful handles; the rest are zero-initialized and never read.
/// - `Borrowed(&'a [H])` — borrows the profiles vec from `GeometryOp::Loft`
///   / `GeometryOp::LoftGuided` without cloning.
///
/// Supersedes the earlier four-variant `Zero`/`One`/`Two`/`Many` shape,
/// which was correct but more ceremonious than warranted for a type that
/// is only ever used to call `as_slice()` / `is_empty()` at one call site.
#[derive(Debug)]
enum ParentHandles<'a> {
    /// Inline buffer; only the first `len` elements are meaningful.
    Inline([GeometryHandleId; 2], usize),
    /// Borrowed slice for multi-profile loft ops.
    Borrowed(&'a [GeometryHandleId]),
}

impl<'a> ParentHandles<'a> {
    fn as_slice(&self) -> &[GeometryHandleId] {
        match self {
            Self::Inline(buf, len) => &buf[..*len],
            Self::Borrowed(s) => s,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Inline(_, len) => *len == 0,
            Self::Borrowed(s) => s.is_empty(),
        }
    }
}

/// Return the parent `GeometryHandleId`s whose sub-shapes the kernel should
/// propagate into the result of `op`.
///
/// The semantics mirror those established in `populate_attribute_history`
/// (engine_build.rs:103-114): the path/spine of a sweep is a route, not a
/// parent; only the profile's sub-shapes appear in the result.  Likewise,
/// guides in `SweepGuided`/`LoftGuided` are constraints, not parents, and
/// `Pipe`'s profile is a kernel-internal circle (private per the
/// `GeometryOp::Pipe` docstring) with no user-facing handle.
///
/// Returns a [`ParentHandles`] enum that is zero-allocation for all cases:
/// an inline `[H; 2]` buffer for 0/1/2 element cases, and a borrowed slice
/// for multi-profile Loft/LoftGuided.  Returning `Inline(_, 0)` for
/// primitives, curve constructors, and Pipe is intentional — the caller in
/// `execute_realization_ops` short-circuits on `is_empty()` so the kernel
/// hook is never invoked for these ops.
fn parent_handles_for_op(op: &GeometryOp) -> ParentHandles<'_> {
    // Placeholder fill for unused Inline buffer slots; only the first `len`
    // slots are ever read via `as_slice()`.
    let z = GeometryHandleId(0);

    // Classification is table-driven: the descriptor's parent_role determines
    // which field projection to apply. The inner OR-patterns are the
    // irreducible field reads (DD-6) — Rust cannot bind named fields across
    // variants without listing them.
    //
    // Two-tier safety net for new ops:
    //  1. A new variant with NO descriptor row panics here explicitly (not
    //     silently returning empty parents), caught at test time by
    //     `geometry_op_descriptors_table_is_complete` in reify-ir and by the
    //     coverage assertion in
    //     `parent_handles_for_op_returns_expected_handles_per_variant_family`.
    //  2. A new variant with a descriptor row but its role not matched by an
    //     inner arm hits `_ => unreachable!()`, also caught by the coverage
    //     assertion before it reaches production (DD-3 model).
    let role = descriptor_for(op.into())
        .expect("every GeometryOp variant must have a descriptor row in GEOMETRY_OP_DESCRIPTORS")
        .parent_role;

    match role {
        // Primitives, curve constructors, profile face producers, Pipe —
        // no user-facing parent handles.
        ParentRole::None => ParentHandles::Inline([z, z], 0),

        // Boolean ops — both operands are parents.
        ParentRole::Pair => match op {
            GeometryOp::Union { left, right }
            | GeometryOp::Difference { left, right }
            | GeometryOp::Intersection { left, right } => {
                ParentHandles::Inline([*left, *right], 2)
            }
            _ => unreachable!("descriptor role Pair but op lacks left/right fields"),
        },

        // Single-target shape-modifying and transform/pattern ops —
        // the target is the sole parent. Non-parent fields (Draft plane,
        // OffsetCurve reference, SweepGuided guide/path) are excluded per
        // `populate_attribute_history` (engine_build.rs:103-114).
        ParentRole::SingleTarget => match op {
            GeometryOp::Fillet { target, .. }
            | GeometryOp::Chamfer { target, .. }
            | GeometryOp::ChamferAsymmetric { target, .. }
            | GeometryOp::Translate { target, .. }
            | GeometryOp::Rotate { target, .. }
            | GeometryOp::Scale { target, .. }
            | GeometryOp::ScaleNonUniform { target, .. }
            | GeometryOp::RotateAround { target, .. }
            | GeometryOp::ApplyTransform { target, .. }
            | GeometryOp::AffineApply { target, .. }
            | GeometryOp::LinearPattern { target, .. }
            | GeometryOp::CircularPattern { target, .. }
            | GeometryOp::Mirror { target, .. }
            | GeometryOp::LinearPattern2D { target, .. }
            | GeometryOp::ArbitraryPattern { target, .. }
            | GeometryOp::Draft { target, .. }
            | GeometryOp::Thicken { target, .. }
            | GeometryOp::OffsetCurve { target, .. }
            | GeometryOp::OffsetSolid { target, .. }
            | GeometryOp::Shell { target, .. }
            | GeometryOp::ZoneSlab { target, .. } => ParentHandles::Inline([*target, z], 1),
            // Surface (isosurface, task 4999): the sole parent is `grid`, not
            // `target` — a dedicated arm, since the field name differs from
            // the shared OR-pattern above.
            GeometryOp::Surface { grid, .. } => ParentHandles::Inline([*grid, z], 1),
            _ => unreachable!("descriptor role SingleTarget but op lacks target field"),
        },

        // Single-profile sweep ops — profile only; path/spine excluded.
        // Per `populate_attribute_history` (engine_build.rs:103-114):
        // "the path/spine is not itself a parent".
        ParentRole::SingleProfile => match op {
            GeometryOp::Extrude { profile, .. }
            | GeometryOp::ExtrudeSymmetric { profile, .. }
            | GeometryOp::ExtrudeInfinite { profile, .. }
            | GeometryOp::Revolve { profile, .. }
            | GeometryOp::Sweep { profile, .. }
            | GeometryOp::SweepGuided { profile, .. } => ParentHandles::Inline([*profile, z], 1),
            _ => unreachable!("descriptor role SingleProfile but op lacks profile field"),
        },

        // Multi-profile loft ops — all profiles are parents; guides excluded.
        // Borrow the profiles vec directly to avoid a clone on every loft op.
        ParentRole::VariadicProfiles => match op {
            GeometryOp::Loft { profiles } | GeometryOp::LoftGuided { profiles, .. } => {
                ParentHandles::Borrowed(profiles.as_slice())
            }
            _ => unreachable!("descriptor role VariadicProfiles but op lacks profiles field"),
        },

        // Topology selectors — these are NOT realization ops and must never
        // flow through execute_realization_ops. Split is dispatched via
        // GeometryKernel::execute_split (eval-time topology selector path).
        ParentRole::TopologySelector => {
            unreachable!(
                "split is a topology selector; \
                 it is never inserted into the realization graph and \
                 must not reach parent_handles_for_op"
            )
        }
    }
}

/// Rewrite an op's parent/input handle ids through a substitution map
/// (task 4050 step-8). The cross-kernel conversion executor uses this to point
/// the final-stage op at the converted (ingested) target-kernel handles instead
/// of the original source-kernel handles. Mirrors [`parent_handles_for_op`]'s
/// variant coverage exactly so the compiler flags drift; non-parent fields
/// (sweep path/spine, guides, draft plane) and parent-less ops (primitives,
/// curve constructors, `Pipe`) are left untouched. A handle absent from the
/// map is left as-is.
fn substitute_op_parents(
    op: &mut GeometryOp,
    mapping: &HashMap<GeometryHandleId, GeometryHandleId>,
) {
    let sub = |h: &mut GeometryHandleId| {
        if let Some(&new) = mapping.get(h) {
            *h = new;
        }
    };

    // Compute the role via a shared reborrow BEFORE the mutable `match op`
    // borrow. `(&*op).into()` borrows `op` immutably for just this expression;
    // once `role` is a plain ParentRole value, the shared borrow is released
    // and the mutable inner matches can proceed without a borrow-checker conflict.
    // A new variant with no descriptor row panics here (fail-loud) rather than
    // silently skipping its parents; one with a row but missing from an inner
    // arm hits `_ => unreachable!()` (DD-3 model, same as parent_handles_for_op).
    let role = descriptor_for((&*op).into())
        .expect("every GeometryOp variant must have a descriptor row in GEOMETRY_OP_DESCRIPTORS")
        .parent_role;

    match role {
        // Primitives, curve constructors, profile face producers, Pipe —
        // no parent handles to substitute.
        ParentRole::None => {}

        // Boolean ops — both operands are parents.
        ParentRole::Pair => match op {
            GeometryOp::Union { left, right }
            | GeometryOp::Difference { left, right }
            | GeometryOp::Intersection { left, right } => {
                sub(left);
                sub(right);
            }
            _ => unreachable!("descriptor role Pair but op lacks left/right fields"),
        },

        // Single-target shape-modifying, transform, and pattern ops —
        // only target is a parent; non-parent fields (Draft.plane,
        // OffsetCurve.reference) are left untouched.
        ParentRole::SingleTarget => match op {
            GeometryOp::Fillet { target, .. }
            | GeometryOp::Chamfer { target, .. }
            | GeometryOp::ChamferAsymmetric { target, .. }
            | GeometryOp::Translate { target, .. }
            | GeometryOp::Rotate { target, .. }
            | GeometryOp::Scale { target, .. }
            | GeometryOp::ScaleNonUniform { target, .. }
            | GeometryOp::RotateAround { target, .. }
            | GeometryOp::ApplyTransform { target, .. }
            | GeometryOp::AffineApply { target, .. }
            | GeometryOp::LinearPattern { target, .. }
            | GeometryOp::CircularPattern { target, .. }
            | GeometryOp::Mirror { target, .. }
            | GeometryOp::LinearPattern2D { target, .. }
            | GeometryOp::ArbitraryPattern { target, .. }
            | GeometryOp::Draft { target, .. }
            | GeometryOp::Thicken { target, .. }
            | GeometryOp::OffsetCurve { target, .. }
            | GeometryOp::OffsetSolid { target, .. }
            | GeometryOp::Shell { target, .. }
            | GeometryOp::ZoneSlab { target, .. } => {
                sub(target);
            }
            // Surface (isosurface, task 4999): the sole parent is `grid`, not
            // `target` — a dedicated arm, mirroring parent_handles_for_op.
            GeometryOp::Surface { grid, .. } => {
                sub(grid);
            }
            _ => unreachable!("descriptor role SingleTarget but op lacks target field"),
        },

        // Single-profile sweep ops — profile only; path/spine/guide excluded.
        ParentRole::SingleProfile => match op {
            GeometryOp::Extrude { profile, .. }
            | GeometryOp::ExtrudeSymmetric { profile, .. }
            | GeometryOp::ExtrudeInfinite { profile, .. }
            | GeometryOp::Revolve { profile, .. }
            | GeometryOp::Sweep { profile, .. }
            | GeometryOp::SweepGuided { profile, .. } => {
                sub(profile);
            }
            _ => unreachable!("descriptor role SingleProfile but op lacks profile field"),
        },

        // Multi-profile loft ops — every profile is a parent; guides excluded.
        ParentRole::VariadicProfiles => match op {
            GeometryOp::Loft { profiles } | GeometryOp::LoftGuided { profiles, .. } => {
                for p in profiles.iter_mut() {
                    sub(p);
                }
            }
            _ => unreachable!("descriptor role VariadicProfiles but op lacks profiles field"),
        },

        // Topology selectors — never inserted into the realization graph.
        ParentRole::TopologySelector => {
            unreachable!(
                "split is a topology selector; \
                 it is never inserted into the realization graph and \
                 must not reach substitute_op_parents"
            )
        }
    }
}

/// Cache-key `entity` id for a cross-kernel conversion intermediate (task 4050
/// step-12).
///
/// The conversion executor tessellates each BRep input handle of an op and
/// ingests the result into the target kernel, producing a Mesh intermediate
/// that is cached (keyed `(entity, Mesh, per_stage_tol, NO_OPTIONS)`) so a later
/// realization can reuse it instead of re-tessellating. The `entity` component
/// must be both DISTINCT per input (so an op's N inputs cache as N separate
/// intermediates — no within-realization clobber) AND STABLE across identical
/// rebuilds of the same realization (so the reuse hit fires).
///
/// For a same-realization `Step` input — the only shape the v0.3-ε fixtures
/// exercise — the input's *local step index* (its position in
/// `realization_step_ids`) satisfies both: it is the input's slot in the op
/// stream, identical on every rebuild, and unique among the realization's
/// steps. A cross-realization (`Sub`) input is absent from
/// `realization_step_ids` and falls back to the input handle id, which is
/// itself a stable cached-terminal handle (the producing realization re-hits
/// its own terminal cache on rebuild and hands back the same id). The `#`
/// separator cannot occur in a DSL entity identifier, so the synthesised key
/// can never collide with a real entity's terminal-cache key.
///
/// **Cross-realization keying invariant.** The synthesised key embeds
/// `realization_entity` but NOT the realization's index within its template, so
/// two realizations that share an entity name (differing only by index) would
/// generate identical intermediate keys for their first conversion input. This
/// is deliberately consistent with the TERMINAL cache keying — the post-loop
/// `realization_cache.insert(&realization_id.entity, …)` likewise keys on
/// `entity` alone — and BOTH rely on the same invariant: within a single build a
/// realization's `entity` uniquely identifies it in the cache (distinct cached
/// realizations carry distinct entity names). If that invariant is ever weakened
/// (e.g. multiple indexed realizations of one entity become independently
/// cacheable), this key AND the terminal key must additionally incorporate
/// `realization_id.index`; they must change together to stay consistent.
fn conversion_intermediate_entity_id(
    realization_entity: &str,
    input_handle: GeometryHandleId,
    realization_step_ids: &[GeometryHandleId],
) -> String {
    match realization_step_ids
        .iter()
        .position(|id| *id == input_handle)
    {
        Some(idx) => format!("{realization_entity}#conv-step{idx}"),
        None => format!("{realization_entity}#conv-ext{}", input_handle.0),
    }
}

/// Total `GeometryOp` → `Operation` classifier used by the per-op dispatch
/// path (task ε / 3436, PRD §8 step-4).
///
/// Maps each runtime `GeometryOp` variant (`reify-types::geometry::GeometryOp`,
/// which carries the per-call parameters: handles, lengths, angles, …) to its
/// coarse [`Operation`] classifier (`reify-types::geometry::Operation`, used
/// as the BTreeMap key in `CapabilityDescriptor::supports`). The dispatcher
/// (`crate::dispatcher::dispatch`) consults the `(Operation, ReprKind)` table
/// to pick a kernel + conversion chain per op.
///
/// **Mirrors [`parent_handles_for_op`].** Both helpers exhaustively match
/// every `GeometryOp` variant; the compiler enforces drift between this table
/// and the variant set at the call site. Adding a new `GeometryOp` variant
/// requires adding an arm in both functions at the same diff site.
///
/// **No `Convert` arm.** `Operation::Convert { from }` is the only
/// `Operation` shape that does not correspond to a `GeometryOp` variant:
/// representation conversion (BRep→Mesh tessellation, Mesh→Sdf rasterisation,
/// …) is *not* an op the compiler emits today. Conversion-stage execution is
/// deferred to task ζ (#3437, Manifold execute arm) + new cross-kernel
/// mesh-ingest trait surface. ε surfaces non-empty dispatch plans as a
/// diagnostic rather than executing them (see PRD §8 design decision).
// Wired into `execute_realization_ops` in step-8 (#3436).
#[allow(dead_code)]
fn geometry_op_to_operation(op: &GeometryOp) -> Operation {
    // Classification is pure data: look up the L1 descriptor table and read
    // `operation`. Split's row has `operation: None`, which reproduces the
    // prior unreachable!() exactly — Split is a topology selector and must
    // never reach this function (it is never inserted into the realization
    // graph). All other 47 variants have `operation: Some(_)`.
    descriptor_for(op.into())
        .and_then(|d| d.operation)
        .unwrap_or_else(|| {
            unreachable!(
                "split is a topology selector; \
                 it is never inserted into the realization graph and \
                 must not reach geometry_op_to_operation"
            )
        })
}

/// Return the set of [`ReprKind`]s an [`Operation`] accepts as its geometric
/// input, per the PRD §3a.4 classifier table (task 4049).
///
/// Returns `None` for variants not yet classified — the conservative fallback
/// `op_accepts_repr` returns `false` (does not accept Mesh) for unclassified
/// ops. The `_ => None` catch-all is intentionally unreachable for all current
/// variants once step-4 is landed; it exists to handle genuinely-new future
/// variants conservatively until they are explicitly classified.
///
/// **Intentional asymmetry with `compiled_geometry_op_to_operation`**: that
/// function uses an exhaustive match (compile error on new variant), while this
/// function uses a `_ => None` catch-all (runtime miss → conservative BRep,
/// surfaced by the strum completeness test). Together they provide two
/// independent forcing functions — compile-time for structural mapping,
/// test-time for demand classification — so a new variant fails loudly on both
/// axes without coupling the two concerns.
///
/// Table (PRD §3a.4):
/// - Boolean* / Transform* / Pattern* → `[BRep, Mesh]`
/// - Modify* / Sweep*                 → `[BRep]` (BRep-only consumers)
/// - Convert { from }                 → `[BRep, Mesh]`
/// - Primitive* / Curve*              → `[BRep]` (sources; classified to
///   document the 'not a Mesh-accepting consumer' decision; step-4 adds arms)
#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)
fn classify_op_input_reprs(op: &Operation) -> Option<&'static [ReprKind]> {
    use Operation::*;
    use ReprKind::{BRep, Mesh, Voxel};
    const BREP_MESH: &[ReprKind] = &[BRep, Mesh];
    const BREP_ONLY: &[ReprKind] = &[BRep];
    const VOXEL_ONLY: &[ReprKind] = &[Voxel];
    match op {
        // Booleans — accept both reprs
        BooleanUnion | BooleanDifference | BooleanIntersection => Some(BREP_MESH),

        // Modify — BRep-only consumers
        ModifyFillet | ModifyChamfer | ModifyShell | ModifyDraft | ModifyThicken
        | ModifyOffsetCurve | ModifyZoneSlab | ModifyOffsetSolid => Some(BREP_ONLY),

        // Transform — accept both reprs. `TransformApplyTransform` is the
        // post-realization rigid-isometry application (task 3901); like the
        // scalar transforms it is repr-agnostic, so it accepts both BRep and
        // Mesh inputs. `TransformAffineApply` (task 3963) is the general
        // affine-map application (gp_GTrsf) — likewise repr-agnostic.
        TransformTranslate
        | TransformRotate
        | TransformScale
        | TransformRotateAround
        | TransformApplyTransform
        | TransformAffineApply => Some(BREP_MESH),

        // Pattern — accept both reprs
        PatternLinear | PatternCircular | PatternMirror | PatternLinear2D | PatternArbitrary => {
            Some(BREP_MESH)
        }

        // Sweep — BRep-only consumers
        SweepLoft
        | SweepExtrude
        | SweepRevolve
        | SweepSweep
        | SweepExtrudeSymmetric
        | SweepExtrudeInfinite
        | SweepSweepGuided
        | SweepLoftGuided
        | SweepPipe => Some(BREP_ONLY),

        // Convert — accepts both reprs (source repr is `from`, dest is the
        // second element of the capability tuple — not relevant here)
        Convert { .. } => Some(BREP_MESH),

        // Primitives — sources (no geometric input); classified as BRep to
        // document the conscious 'not a Mesh-accepting consumer' decision and
        // satisfy the strum-completeness test (test d, step-3).
        PrimitiveBox | PrimitiveCylinder | PrimitiveSphere | PrimitiveTube | PrimitiveCone
        | PrimitiveWedge | PrimitiveTorus | PrimitiveHalfSpace => Some(BREP_ONLY),

        // Curves — sources (no geometric input); same rationale as Primitives.
        CurveLineSegment | CurveArc | CurveHelix | CurveInterpCurve | CurveBezierCurve
        | CurveNurbsCurve => Some(BREP_ONLY),

        // Profile face producers — sources (no geometric input); same rationale.
        ProfileRectangle | ProfileCircle | ProfilePolygon | ProfileEllipse => Some(BREP_ONLY),

        // Surface producers — sources (no geometric input); same rationale as Primitives.
        SurfaceNurbs => Some(BREP_ONLY),

        // Surface (isosurface / marching-cubes, task 4999) — consumes a
        // voxel grid; Voxel-only input (PRD OQ-1).
        Surface => Some(VOXEL_ONLY),

        // Catch-all: genuinely-new future variants → conservative (None).
        // Unreachable for all current variants (strum test above enforces this).
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Return `true` if `op` accepts `repr` as a geometric input.
///
/// Unclassified ops (`classify_op_input_reprs` returns `None`) return `false`,
/// making them conservative: they do not accept Mesh, which forces their
/// producers to demand BRep.
#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)
fn op_accepts_repr(op: &Operation, repr: ReprKind) -> bool {
    classify_op_input_reprs(op).is_some_and(|s| s.contains(&repr))
}

/// Return `true` if `op` accepts Voxel input and NOTHING else (task 5000, PRD
/// `docs/prds/v0_3/voxel-to-mesh-surfacing.md` task β, C-3: Voxel demand is
/// opt-in). `Operation::Surface` (the isosurface builtin) is currently the
/// only such op — see the `Surface => Some(VOXEL_ONLY)` arm of
/// `classify_op_input_reprs`. Defined via `op_accepts_repr` rather than an
/// exact `Some(&[Voxel])` slice match so it stays correct if a future op is
/// classified with multiple reprs that happen to include Voxel alongside
/// Mesh/BRep — such an op would NOT be Voxel-only-input and must not force
/// its producer to Voxel demand.
#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)
fn op_is_voxel_only_input(op: &Operation) -> bool {
    op_accepts_repr(op, ReprKind::Voxel)
        && !op_accepts_repr(op, ReprKind::Mesh)
        && !op_accepts_repr(op, ReprKind::BRep)
}

/// Map a compiled geometry op to its `Operation` classifier key.
///
/// Exhaustive match over `CompiledGeometryOp`/kind sub-enums so a new variant
/// fails to compile until mapped — same discipline as `geometry_op_to_operation`
/// at :902, but over the compiled-IR form rather than the runtime `GeometryOp`.
fn compiled_geometry_op_to_operation(op: &CompiledGeometryOp) -> Operation {
    match op {
        CompiledGeometryOp::Primitive { kind, .. } => match kind {
            PrimitiveKind::Box => Operation::PrimitiveBox,
            PrimitiveKind::Cylinder => Operation::PrimitiveCylinder,
            PrimitiveKind::Sphere => Operation::PrimitiveSphere,
            PrimitiveKind::Tube => Operation::PrimitiveTube,
            PrimitiveKind::Cone => Operation::PrimitiveCone,
            PrimitiveKind::Wedge => Operation::PrimitiveWedge,
            PrimitiveKind::Torus => Operation::PrimitiveTorus,
            PrimitiveKind::HalfSpace => Operation::PrimitiveHalfSpace,
        },
        CompiledGeometryOp::Boolean { op, .. } => match op {
            BooleanOp::Union => Operation::BooleanUnion,
            BooleanOp::Difference => Operation::BooleanDifference,
            BooleanOp::Intersection => Operation::BooleanIntersection,
        },
        CompiledGeometryOp::Modify { kind, .. } => match kind {
            ModifyKind::Fillet => Operation::ModifyFillet,
            ModifyKind::Chamfer => Operation::ModifyChamfer,
            // Asymmetric chamfer shares the symmetric chamfer's BRep kernel
            // capability (BRepFilletAPI_MakeChamfer) — same Operation (β, task 4185).
            ModifyKind::ChamferAsymmetric => Operation::ModifyChamfer,
            ModifyKind::Shell => Operation::ModifyShell,
            ModifyKind::Draft => Operation::ModifyDraft,
            ModifyKind::Thicken => Operation::ModifyThicken,
            ModifyKind::ZoneSlab => Operation::ModifyZoneSlab,
            ModifyKind::OffsetSolid => Operation::ModifyOffsetSolid,
            ModifyKind::OffsetCurve => Operation::ModifyOffsetCurve,
        },
        CompiledGeometryOp::Transform { kind, .. } => match kind {
            TransformKind::Translate => Operation::TransformTranslate,
            TransformKind::Rotate => Operation::TransformRotate,
            TransformKind::Scale => Operation::TransformScale,
            TransformKind::RotateAround => Operation::TransformRotateAround,
            TransformKind::ApplyTransform => Operation::TransformApplyTransform,
            TransformKind::AffineApply => Operation::TransformAffineApply,
            // Per-axis (non-rigid) scale shares uniform Scale's Operation
            // classifier (task 4167) — see the GEOMETRY_OP_DESCRIPTORS row for
            // GeometryOp::ScaleNonUniform in reify-ir/src/geometry.rs.
            TransformKind::ScaleNonUniform => Operation::TransformScale,
        },
        CompiledGeometryOp::Pattern { kind, .. } => match kind {
            PatternKind::Linear => Operation::PatternLinear,
            PatternKind::Circular => Operation::PatternCircular,
            PatternKind::Mirror => Operation::PatternMirror,
            PatternKind::Linear2D => Operation::PatternLinear2D,
            PatternKind::Arbitrary => Operation::PatternArbitrary,
        },
        CompiledGeometryOp::Sweep { kind, .. } => match kind {
            SweepKind::Loft => Operation::SweepLoft,
            SweepKind::Extrude => Operation::SweepExtrude,
            SweepKind::Revolve => Operation::SweepRevolve,
            SweepKind::Sweep => Operation::SweepSweep,
            SweepKind::ExtrudeSymmetric => Operation::SweepExtrudeSymmetric,
            SweepKind::ExtrudeInfinite => Operation::SweepExtrudeInfinite,
            SweepKind::SweepGuided => Operation::SweepSweepGuided,
            SweepKind::LoftGuided => Operation::SweepLoftGuided,
            SweepKind::Pipe => Operation::SweepPipe,
        },
        CompiledGeometryOp::Curve { kind, .. } => match kind {
            CurveKind::LineSegment => Operation::CurveLineSegment,
            CurveKind::Arc => Operation::CurveArc,
            CurveKind::Helix => Operation::CurveHelix,
            CurveKind::InterpCurve => Operation::CurveInterpCurve,
            CurveKind::BezierCurve => Operation::CurveBezierCurve,
            CurveKind::NurbsCurve => Operation::CurveNurbsCurve,
        },
        CompiledGeometryOp::Profile { kind, .. } => match kind {
            ProfileKind::Rectangle => Operation::ProfileRectangle,
            ProfileKind::Circle => Operation::ProfileCircle,
            ProfileKind::Polygon => Operation::ProfilePolygon,
            ProfileKind::Ellipse => Operation::ProfileEllipse,
        },
        CompiledGeometryOp::Surface { kind, .. } => {
            use reify_compiler::SurfaceKind;
            match kind {
                SurfaceKind::Nurbs => Operation::SurfaceNurbs,
            }
        }
        CompiledGeometryOp::Isosurface { .. } => Operation::Surface,
    }
}

/// Collect all `GeomRef::Sub` operands referenced by a compiled geometry op.
///
/// Used by `available_for_op`'s cross-realization name resolution (task
/// 5033 Gap #2 Gap A) to look up a `GeomRef::Sub(name)` parent's produced
/// repr in `named_step_reprs` when it names a DIFFERENT, already-completed
/// realization rather than a step local to this one.
fn sub_refs_in_op(op: &CompiledGeometryOp) -> Vec<&str> {
    let mut refs = Vec::new();
    match op {
        CompiledGeometryOp::Boolean { left, right, .. } => {
            if let GeomRef::Sub(n) = left {
                refs.push(n.as_str());
            }
            if let GeomRef::Sub(n) = right {
                refs.push(n.as_str());
            }
        }
        CompiledGeometryOp::Modify { target, .. }
        | CompiledGeometryOp::Transform { target, .. }
        | CompiledGeometryOp::Pattern { target, .. } => {
            if let GeomRef::Sub(n) = target {
                refs.push(n.as_str());
            }
        }
        CompiledGeometryOp::Sweep { profiles, .. } => {
            for p in profiles {
                if let GeomRef::Sub(n) = p {
                    refs.push(n.as_str());
                }
            }
        }
        CompiledGeometryOp::Isosurface { grid, .. } => {
            if let GeomRef::Sub(n) = grid {
                refs.push(n.as_str());
            }
        }
        CompiledGeometryOp::Primitive { .. }
        | CompiledGeometryOp::Curve { .. }
        | CompiledGeometryOp::Profile { .. }
        | CompiledGeometryOp::Surface { .. } => {}
    }
    refs
}

impl Engine {
    /// Compute the per-realization demanded [`ReprKind`] for each template in
    /// `module`, given the build's output `format` (Stl/Obj → mesh sink;
    /// Step → BRep sink).
    ///
    /// Returns a positionally-indexed `Vec<Vec<ReprKind>>` aligned with
    /// `module.templates × realizations` — same `[t_idx][r_idx]` indexing as
    /// [`Self::compute_demanded_tols`].
    ///
    /// **Demand rule** (PRD §3a.4): a realization's OWN op kind does NOT factor
    /// into its own demand — only its consumers and (if terminal) its export-
    /// format sink do. Terminal realizations get Mesh for Stl/Obj, BRep for
    /// Step. Non-terminal realizations get Mesh unless a consumer op does not
    /// accept Mesh or a consumer already demands BRep (transitive). A single
    /// reverse-index pass computes transitive demand with no fixpoint loop
    /// because bindings reference only earlier bindings (producer-before-
    /// consumer ordering).
    ///
    /// **Consumer-edge encoding**: cross-realization dependencies are encoded
    /// as `GeomRef::Sub(name)` operands inside compiled ops; consumer edges are
    /// built by scanning ops and resolving name → realization index. Compound
    /// `"sub.member"` names (cross-template, Task 3441) are always routed to
    /// the conservative path (BRep) regardless of whether the base component
    /// coincidentally matches a local realization name — see step-8 for the
    /// debug log.
    pub(crate) fn compute_demanded_reprs(
        &self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> Vec<Vec<ReprKind>> {
        module
            .templates
            .iter()
            .map(|t| {
                let vm_demanded = self.volume_mesh_demanded_indices(module, t);
                demanded_reprs_for_template(t, format, &vm_demanded)
            })
            .collect()
    }

    /// Compute the set of realization indices in `template` whose demand the
    /// static VolumeMesh-demand pass overrides to [`ReprKind::VolumeMesh`]
    /// (task 4743 step-8, PRD §10 OQ-1 — the "consumer-op marker / small
    /// extension to demanded_reprs_for_template" resolution).
    ///
    /// A realization index `i` is included iff some `value_cell` in `template`
    /// has a [`reify_ir::CompiledExprKind::UserFunctionCall`] `default_expr`
    /// whose `function_name` resolves (via `module.functions` ∪
    /// `self.functions`) to an `@optimized` target registered
    /// VolumeMesh-demanding ([`Engine::register_volume_mesh_demand`]), AND
    /// that call has an argument that is a `ValueRef` to a `Type::Geometry`
    /// cell whose member name equals `template.realizations[i].name` — the
    /// SAME consumer→producer name-match the `geometry_cell` rule uses
    /// (`graph.rs:371`, `cell.id.member == realization.name && cell_type ==
    /// Geometry`).
    ///
    /// **Why the `self.functions` union (task 5008 GAP A).** A STDLIB
    /// `@optimized` geometry-consumer (`solve_elastic_static` /
    /// `solve_load_cases`) is absent from a compiled user module's
    /// `functions` table — `compile_with_stdlib` keeps stdlib fns in a
    /// separate prelude — but present in `self.functions` once `build()` →
    /// `check()` merges module + prelude (`merge_functions`), which runs
    /// BEFORE this pass. Scanning `module.functions` alone therefore misses
    /// every stdlib consumer. `self.functions` is a superset of
    /// `module.functions` on the build path, but names dedup in the
    /// `vm_demanding_fns` `HashSet<&str>` below, so the union is safe for the
    /// direct-call unit tests too (they seed `module.functions` without
    /// evaluating, so `self.functions` is empty there).
    ///
    /// **Why module-static.** Demand is computed early in `build`
    /// (`compute_demanded_reprs`), BEFORE compute nodes dispatch and BEFORE
    /// their `realization_inputs` are built (post-build redispatch). A runtime
    /// read of `realization_inputs` is therefore unavailable at demand time.
    /// This static path reaches the same producing realization the runtime β
    /// lowering (`build_compute_realization_inputs` /
    /// `redispatch_geometry_consuming_compute_nodes`) reaches via the consumer
    /// cell's `UserFunctionCall` default-expr — so it is timing-independent and
    /// needs no graph/eval-state. The runtime β lowering still drives the
    /// read-back path; only the demand half is new.
    fn realization_indices_where<F: Fn(&str) -> bool>(
        &self,
        module: &CompiledModule,
        template: &TopologyTemplate,
        target_demands: F,
    ) -> HashSet<usize> {
        let mut out: HashSet<usize> = HashSet::new();

        // realization name → index (named realizations only; mirrors the
        // `name_to_idx` map in `demanded_reprs_for_template`).
        let name_to_idx: HashMap<&str, usize> = template
            .realizations
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.name.as_deref().map(|name| (name, i)))
            .collect();

        // Pre-index once per template so the per-cell scan below is a chain of
        // O(1) lookups rather than nested linear `find`s. This pass runs once
        // per build per template, but the prior nested
        // `module.functions.find` + `value_cells.find` were
        // O(cells·funcs + cells·args·cells) — avoidable for large modules:
        //   • `vm_demanding_fns` — user-function names whose `@optimized` target
        //     is registered VolumeMesh-demanding. Collapses the function→target
        //     resolution AND the `demands_volume_mesh` membership check into a
        //     single set lookup.
        //   • `local_cell_is_geometry` — for each LOCAL value cell, whether its
        //     `cell_type` is `Type::Geometry`. Replaces the per-arg
        //     `value_cells.find(|c| c.id == *arg_cell)` linear scan while keeping
        //     the EXACT original geometry semantics: prefer the local cell's
        //     declared type, fall back to the arg's own `result_type` when the
        //     arg is NOT a local value-cell declaration. (A `let body = box(...)`
        //     geometry producer lowers to a *realization*, not a geometry-typed
        //     value cell whose id the consumer arg matches, so the `result_type`
        //     fallback is load-bearing on the real lowering path — the
        //     realization-name match below is what actually links consumer →
        //     producer.)
        //
        // `vm_demanding_fns` is sourced from the UNION of `module.functions` ∪
        // `self.functions` (task 5008 GAP A), not `module.functions` alone. A
        // STDLIB `@optimized` geometry-consumer (e.g. `solve_elastic_static` /
        // `solve_load_cases`) is never a member of a compiled user module's
        // `functions` table — `compile_with_stdlib` keeps stdlib fns in a
        // separate prelude — so scanning `module.functions` alone misses it and
        // the demand never fires. `self.functions: Arc<[CompiledFunction]>`
        // (`lib.rs:674`) holds the merged prelude+module table populated by
        // `build()` → `check()` (`merge_functions`), which runs BEFORE this
        // demand pass (`build_with_geometry_output`'s `self.check(module)`
        // precedes its `compute_demanded_reprs` call) — so it is already
        // populated here on the real build path. `self.functions` is a
        // superset of `module.functions` there, but names dedup in this
        // `HashSet<&str>`, so chaining both iterators is dedup-safe and keeps
        // the direct-call unit tests (which seed `module.functions` but never
        // eval, leaving `self.functions` empty) green.
        let vm_demanding_fns: HashSet<&str> = module
            .functions
            .iter()
            .chain(self.functions.iter())
            .filter(|f| {
                f.optimized_target
                    .as_deref()
                    .is_some_and(&target_demands)
            })
            .map(|f| f.name.as_str())
            .collect();
        let local_cell_is_geometry: HashMap<&reify_core::ValueCellId, bool> = template
            .value_cells
            .iter()
            .map(|c| (&c.id, c.cell_type == reify_core::Type::Geometry))
            .collect();

        for cell in &template.value_cells {
            let Some(expr) = &cell.default_expr else {
                continue;
            };
            // @optimized consumers lower to UserFunctionCall (the same variant
            // the β-lowering redispatch matches at engine_build.rs ~6693);
            // FunctionCall is the stdlib/builtin variant and never an
            // @optimized target.
            let reify_ir::CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } = &expr.kind
            else {
                continue;
            };
            // Resolve the called user function to its @optimized target and
            // require that target to be registered VolumeMesh-demanding.
            if !vm_demanding_fns.contains(function_name.as_str()) {
                continue;
            }
            // Each geometry `ValueRef` arg names a producing realization to
            // override to VolumeMesh demand.
            for arg in args {
                let reify_ir::CompiledExprKind::ValueRef(arg_cell) = &arg.kind else {
                    continue;
                };
                // The referenced arg must be a `Type::Geometry` cell (the same
                // gate `geometry_cell` applies). Prefer the local cell's declared
                // type; fall back to the arg's own `result_type` when the arg is
                // not a local value-cell declaration.
                let is_geometry = local_cell_is_geometry
                    .get(arg_cell)
                    .copied()
                    .unwrap_or(arg.result_type == reify_core::Type::Geometry);
                if !is_geometry {
                    continue;
                }
                if let Some(&p_idx) = name_to_idx.get(arg_cell.member.as_str()) {
                    // Cross-template guard (mirrors the conservative cross-template
                    // handling in `demanded_reprs_for_template`): only override
                    // when the arg's `entity` component matches the producing
                    // realization's entity. A cross-template `ValueRef` (e.g.
                    // `OtherStruct.body`) whose bare `member` coincidentally
                    // equals a local realization name carries a DIFFERENT
                    // `entity`, so it must NOT override the local realization's
                    // demand. (The bare-member `name_to_idx` match alone would
                    // alias across templates — the asymmetry the matching rule
                    // otherwise shares with `geometry_cell`.)
                    if arg_cell.entity == template.realizations[p_idx].id.entity {
                        out.insert(p_idx);
                    }
                }
            }
        }

        out
    }

    /// Realization indices in `template` whose demand the static pass overrides
    /// to [`ReprKind::VolumeMesh`]. Boundary-demand IMPLIES VolumeMesh demand
    /// (attribution needs a realized tet mesh), so both registries participate.
    fn volume_mesh_demanded_indices(
        &self,
        module: &CompiledModule,
        template: &TopologyTemplate,
    ) -> HashSet<usize> {
        self.realization_indices_where(module, template, |t| {
            self.demands_volume_mesh(t) || self.demands_boundary(t)
        })
    }

    /// Realization indices in `template` referenced by a *boundary*-demanding
    /// consumer (task 4092) — a subset of [`Self::volume_mesh_demanded_indices`].
    /// Gates the attributed-producer branch of the realization edge (step-18).
    fn volume_mesh_boundary_demanded_indices(
        &self,
        module: &CompiledModule,
        template: &TopologyTemplate,
    ) -> HashSet<usize> {
        self.realization_indices_where(module, template, |t| self.demands_boundary(t))
    }

    /// Per-`[t_idx][r_idx]` boundary-demand matrix, aligned with
    /// [`Self::compute_demanded_reprs`]. `true` ⇒ the realization edge routes the
    /// surface through the gmsh attributed producer and threads a
    /// [`reify_ir::BoundaryAssociation`] onto the realized VolumeMesh (step-18).
    pub(crate) fn compute_boundary_demands(&self, module: &CompiledModule) -> Vec<Vec<bool>> {
        module
            .templates
            .iter()
            .map(|t| {
                let demanded = self.volume_mesh_boundary_demanded_indices(module, t);
                (0..t.realizations.len())
                    .map(|i| demanded.contains(&i))
                    .collect()
            })
            .collect()
    }
}

fn demanded_reprs_for_template(
    template: &TopologyTemplate,
    format: ExportFormat,
    vm_demanded: &HashSet<usize>,
) -> Vec<ReprKind> {
    let n = template.realizations.len();
    if n == 0 {
        return vec![];
    }

    // Map realization name → index (only named realizations participate).
    let name_to_idx: HashMap<&str, usize> = template
        .realizations
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.name.as_deref().map(|name| (name, i)))
        .collect();

    // consumer_ops[p_idx] = list of (consumer_idx, consuming_Operation) pairs.
    // conservative_producers[p_idx] = true when a downstream reference to p_idx
    // could not be resolved (absent name / cross-template). Forces BRep on p_idx.
    let mut consumer_ops: Vec<Vec<(usize, Operation)>> = vec![vec![]; n];
    let mut conservative_producers: Vec<bool> = vec![false; n];

    for (c_idx, realization) in template.realizations.iter().enumerate() {
        for op in &realization.operations {
            let consuming_op = compiled_geometry_op_to_operation(op);
            for sub_name in sub_refs_in_op(op) {
                if sub_name.contains('.') {
                    // Compound "sub.member" names reference cross-template
                    // producers (Task 3441). Always conservative: even if the
                    // base component coincidentally matches a local realization
                    // name, the producer being referenced is a different
                    // template's output whose consumer requirements are unknown.
                    conservative_producers[c_idx] = true;
                    tracing::debug!(
                        target: "reify_eval::demanded_reprs",
                        unresolved_ref = sub_name,
                        realization_idx = c_idx,
                        "compound GeomRef::Sub '{}' in consumer realization \
                         (cross-template, Task 3441); defaulting realization and \
                         its producers to BRep demand (conservative)",
                        sub_name
                    );
                } else if let Some(&p_idx) = name_to_idx.get(sub_name) {
                    // Producer-before-consumer ordering: the consumer must have
                    // a HIGHER index than the producer so the reverse-pass can
                    // resolve consumer demand before reaching the producer.
                    //
                    // Ordering violations arise from realization↔realization
                    // cycles (task #4668 adds same-structure Sub refs;
                    // `run_unified_pass` emits `E_EVAL_CYCLE` for such cycles
                    // and places them in residue).  When violated, fall through
                    // to the conservative-BRep path — the over-conservative
                    // result is acceptable since the cycle is already an error.
                    if c_idx > p_idx {
                        consumer_ops[p_idx].push((c_idx, consuming_op));
                    } else {
                        conservative_producers[c_idx] = true;
                        tracing::debug!(
                            target: "reify_eval::demanded_reprs",
                            consumer_idx = c_idx,
                            producer_idx = p_idx,
                            sub_name = sub_name,
                            "producer-before-consumer ordering violated for Sub ref '{}' \
                             (consumer={}, producer={}): likely a realization cycle \
                             (Kahn emits E_EVAL_CYCLE); defaulting consumer to BRep demand",
                            sub_name, c_idx, p_idx
                        );
                    }
                } else {
                    // Unresolved: name absent from this template.
                    conservative_producers[c_idx] = true;
                    tracing::debug!(
                        target: "reify_eval::demanded_reprs",
                        unresolved_ref = sub_name,
                        realization_idx = c_idx,
                        "unresolved GeomRef::Sub '{}' in consumer realization; \
                         defaulting realization and its producers to BRep demand (conservative)",
                        sub_name
                    );
                }
            }
        }
    }

    // Compute demand by iterating realization indices in REVERSE order so
    // consumer demand is always resolved before its producers.
    let mut demand = vec![ReprKind::BRep; n];

    for r_idx in (0..n).rev() {
        // If this realization itself has an unresolved downstream ref, force BRep.
        if conservative_producers[r_idx] {
            demand[r_idx] = ReprKind::BRep;
        } else if consumer_ops[r_idx].is_empty() {
            // Terminal realization: sink determines demand.
            demand[r_idx] = match format {
                ExportFormat::Stl | ExportFormat::Obj | ExportFormat::ThreeMF => ReprKind::Mesh,
                ExportFormat::Step => ReprKind::BRep,
            };
        } else {
            // Non-terminal: Mesh unless a disqualifier forces BRep, UNLESS a
            // direct consumer is Voxel-only-input (task 5000, PRD §β C-3), in
            // which case Voxel takes precedence over the BRep fallback. A
            // Voxel-only-input consumer (e.g. the isosurface builtin) accepts
            // no other repr, so its producer MUST be Voxel — this is checked
            // BEFORE `needs_brep` because that op would otherwise also trip
            // the BRep disqualifier (`!op_accepts_repr(op, Mesh)`) below.
            // Only the DIRECT consumer op kind is inspected — no transitive
            // Voxel propagation (out of scope for the first β slice).
            //
            // `demand[*c_idx] == ReprKind::BRep` subsumes the conservative case:
            // any c_idx with conservative_producers[c_idx]==true had demand[c_idx]
            // set to BRep in the first branch above, and c_idx > r_idx so it was
            // resolved before this point in the reverse pass.
            let needs_voxel = consumer_ops[r_idx]
                .iter()
                .any(|(_, op)| op_is_voxel_only_input(op));
            let needs_brep = consumer_ops[r_idx].iter().any(|(c_idx, op)| {
                !op_accepts_repr(op, ReprKind::Mesh) || demand[*c_idx] == ReprKind::BRep
            });
            demand[r_idx] = if needs_voxel {
                ReprKind::Voxel
            } else if needs_brep {
                ReprKind::BRep
            } else {
                ReprKind::Mesh
            };
        }
    }

    // Task 4743 step-8 (PRD §10 OQ-1): OVERRIDE the VolumeMesh-demanded
    // realization indices computed by `Engine::volume_mesh_demanded_indices`.
    // VolumeMesh wins over the BRep/Mesh demand the reverse-pass derived: a
    // registered VolumeMesh-demanding consumer's geometry arg forces its
    // producing realization to VolumeMesh demand. Applied AFTER the reverse-pass
    // so the override is final (the runtime β lowering still drives the
    // read-back path; only this demand half is new).
    for &idx in vm_demanded {
        if idx < n {
            demand[idx] = ReprKind::VolumeMesh;
        }
    }

    demand
}

/// Compute per-realization dispatch-demand OVERRIDES for the two roles in an
/// isosurface-shaped Voxel/Mesh pipeline (task 5033 Gap D: the
/// `tessellate_from_values` sibling of `demanded_reprs_for_template`'s
/// `needs_voxel` rule above). Absent from the returned map ⇒ the caller's
/// pre-existing hardcoded `ReprKind::BRep` applies, unchanged.
///
/// `tessellate_from_values` (unlike `build`/`build_snapshot`/
/// `build_with_geometry_output`) does not call `compute_demanded_reprs`: its
/// per-op dispatch demand is unconditionally `ReprKind::BRep` (task 4050
/// step-8 design_decision 4), because every realization's terminal handle is
/// tessellated at the end regardless of demand, and forcing BRep keeps every
/// terminal handle on the default kernel for that trailing tessellate call.
/// A Voxel-only-input op (e.g. `isosurface`) is the one shape BRep-everywhere
/// cannot satisfy, in BOTH roles simultaneously:
///   - the CONSUMER realization itself (e.g. `shell` in `let shell =
///     isosurface(solid)`) must demand Mesh — no kernel declares a
///     `(Surface, BRep)` capability entry (only `(Surface, Mesh)`,
///     register.rs), so a BRep demand makes its own dispatch unsatisfiable
///     with no fallback (the BRep-fallback in `execute_realization_ops` is a
///     no-op when `demanded_repr == ReprKind::BRep` already).
///   - its DIRECT operand (e.g. `solid`) must demand Voxel, or the consumer's
///     `available_for_op` never contains Voxel and the same dispatch fails.
///
/// This narrow helper reproduces JUST those two overrides, leaving every
/// other realization's demand at the pre-existing hardcoded BRep — no
/// behavior change for any pipeline that does not use isosurface.
fn voxel_pipeline_demand_overrides(template: &TopologyTemplate) -> HashMap<usize, ReprKind> {
    let name_to_idx: HashMap<&str, usize> = template
        .realizations
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.name.as_deref().map(|name| (name, i)))
        .collect();
    let mut overrides = HashMap::new();
    for (c_idx, realization) in template.realizations.iter().enumerate() {
        for op in &realization.operations {
            // Review fix-forward (task 5033 amendment): cheap discriminant
            // pre-filter before building the full `Operation` classifier.
            // `Operation::Surface` — the only Voxel-only-input op — is
            // produced exclusively by `CompiledGeometryOp::Isosurface` (see
            // `compiled_geometry_op_to_operation`'s match arms), so every
            // other op discriminant can skip the
            // `compiled_geometry_op_to_operation` + `op_is_voxel_only_input`
            // call chain entirely. This function runs once per template on
            // EVERY `tessellate_from_values` call (potentially once per
            // tessellation), so avoiding the full classification for the
            // overwhelmingly common non-isosurface op is not just cosmetic.
            if !matches!(op, CompiledGeometryOp::Isosurface { .. }) {
                continue;
            }
            if !op_is_voxel_only_input(&compiled_geometry_op_to_operation(op)) {
                continue;
            }
            overrides.insert(c_idx, ReprKind::Mesh);
            for sub_name in sub_refs_in_op(op) {
                if let Some(&p_idx) = name_to_idx.get(sub_name) {
                    // Review fix-forward (task 5033 amendment): a realization
                    // can be BOTH an isosurface consumer (own demand = Mesh,
                    // inserted above at its own `c_idx`) and the Voxel
                    // operand of a DOWNSTREAM isosurface (chained: `b =
                    // isosurface(a); c = isosurface(b)`). A `GeomRef::Sub`
                    // operand only ever names an earlier-declared realization
                    // (`p_idx < c_idx`), so by construction any consumer-Mesh
                    // entry for `p_idx` was already inserted in an earlier
                    // iteration of this outer loop — don't clobber it here.
                    // Chained voxel pipelines are out of this task's scope
                    // (PRD voxel-to-mesh-surfacing.md targets the
                    // single-isosurface slice); silently overwriting would
                    // leave `b` demanding Voxel instead of the Mesh its own
                    // `Surface` dispatch requires. Single-isosurface
                    // pipelines never hit this branch (no node is ever both
                    // a `c_idx` and someone else's `p_idx`), so this is a
                    // pure no-op guard for the task's actual scope.
                    //
                    // Amendment (review): the clobber guard's correctness
                    // relies entirely on this ordering invariant — assert it
                    // rather than let a future realization-ordering change
                    // (e.g. a reordering compile pass) silently mis-demand a
                    // chained pipeline instead of failing loudly.
                    debug_assert!(
                        p_idx < c_idx,
                        "GeomRef::Sub({sub_name:?}) must name an earlier-declared \
                         realization (p_idx={p_idx}, c_idx={c_idx})"
                    );
                    if overrides.get(&p_idx) != Some(&ReprKind::Mesh) {
                        overrides.insert(p_idx, ReprKind::Voxel);
                    }
                }
            }
        }
    }
    overrides
}

/// Derive the output [`ReprKind`] for a dispatched op by reading the chosen
/// kernel's capability descriptor (task ε / 3436, PRD §8 step-6).
///
/// Given a [`DispatchPlan`] (whose `kernel` names the BTreeMap-key of the
/// kernel chosen to run the final op) and the dispatched [`Operation`], look
/// up `registry[plan.kernel].supports` for the first entry whose first tuple
/// element equals `op` and return its second tuple element — the output
/// `ReprKind` the kernel produces. This is the value
/// [`Engine::execute_realization_ops`] (step-10) will record into the
/// realization graph node's `produced_repr` field.
///
/// **Why the descriptor lookup, not just `demanded`.** [`dispatch`] guarantees
/// the chosen kernel supports `(op, demanded)` — so in the ε baseline
/// `demanded == ReprKind::BRep` and this helper trivially returns `BRep`.
/// However, in future seams (ζ/η/θ) where per-op demanded reprs vary per
/// kernel choice, the descriptor lookup is the single source of truth for
/// "what does this kernel actually produce?". Threading the demanded repr
/// instead would couple the produced-repr write to the dispatcher's input,
/// hiding mis-declarations in adapter descriptors.
///
/// **First-match semantics.** Returns the first matching entry in declaration
/// order. In v0.3 each kernel declares at most one repr per op (e.g. OCCT
/// declares `(BooleanUnion, BRep)` only, not also `(BooleanUnion, Mesh)`);
/// the dispatcher's `current_repr == demanded` invariant
/// (see [`crate::dispatcher::dispatch`]) enforces this for booleans/modify/
/// transform/pattern ops, since the same `ReprKind` slot encodes both input
/// and output. Multi-repr kernels are a forward-looking concern; first-match
/// is sufficient for ε.
///
/// **Returns `None`** when the plan's named kernel is absent from the
/// registry, or when the kernel's descriptor has no entry for `op`. Both
/// indicate an invariant violation (dispatch should not have chosen such a
/// kernel); the caller surfaces this as a diagnostic rather than fabricating
/// a repr.
// Wired into `execute_realization_ops` in step-10 (#3436).
fn plan_output_repr(
    registry: &BTreeMap<String, &CapabilityDescriptor>,
    plan: &DispatchPlan,
    op: Operation,
) -> Option<ReprKind> {
    let descriptor = registry.get(plan.kernel.as_str())?;
    descriptor
        .supports
        .iter()
        .find(|(o, _)| *o == op)
        .map(|(_, r)| *r)
}

/// Scan `values` for a [`reify_ir::Value::StructureInstance`] whose `geometry`
/// field is a [`reify_ir::Value::GeometryHandle`] with `kernel_handle == handle`
/// AND which carries a `material` field (i.e. it is a Physical body with a
/// Material). If found, resolves the body's `material.appearance.color` via
/// [`crate::appearance::resolve_appearance`] + [`crate::appearance::resolve_color`]
/// and returns `Some(Rgb8)`. Returns `None` for geometry with no owning body or no
/// material.
///
/// Used by [`Engine::build_outputs_with_result`] to thread the per-body color into
/// [`reify_ir::ExportOptions::color`] for the ThreeMF kernel arm (δ, task #4763).
///
/// v1 LIMITATION: matching is by kernel handle id, so a non-identity-pose placed
/// handle (assembly transform) may not match its source body and yields `None`.
/// Follow-up: key on entity_path (PRD-2's join key) to cover transformed bodies.
/// δ (task #4763): resolve the exported body's material color for 3MF egress.
///
/// The export walk yields an [`crate::geometry_ops::ExportBody`] identified by its
/// PRD §11.2 `entity_path` (e.g. `"Assembly.part"`) and a placed `handle_id`.  The
/// value map stores the body's `Physical` `StructureInstance` under its `__self`
/// cell — `ValueCellId { entity: <entity_path>, member: "__self" }` — but the
/// instance's `geometry` field is `Value::Undef` at export time (the realized
/// kernel handle lives in the export walk, not back-populated into the snapshot
/// value), so a handle-equality match never fires.  We therefore associate the
/// body with its instance by `entity_path` first (authoritative), and keep the
/// handle-equality scan as a fallback for any path that does back-populate the
/// geometry handle.  When the matched instance has a `material`, resolve its
/// appearance color via task β's `resolve_appearance`/`resolve_color` seam.
fn resolve_export_body_color(
    values: &ValueMap,
    entity_path: &str,
    handle: GeometryHandleId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<reify_ir::Rgb8> {
    // The export walk's `entity_path` carries a trailing `#realization[N]`
    // selector (e.g. `"Assembly.part#realization[0]"`); the body's `__self`
    // StructureInstance is keyed by the bare containment path (`"Assembly.part"`),
    // so strip the selector before matching.
    let body_path = entity_path.split('#').next().unwrap_or(entity_path);
    // Primary: match the body's `__self` StructureInstance by entity_path.
    for (cell, v) in values.iter() {
        if cell.entity == body_path
            && cell.member == "__self"
            && let reify_ir::Value::StructureInstance(data) = v
            && data.fields.get("material").is_some()
        {
            return Some(resolve_instance_color(v, diagnostics));
        }
    }
    // Fallback: a path that back-populates the geometry handle into the snapshot
    // value can still be matched by handle equality.
    for (_, v) in values.iter() {
        if let reify_ir::Value::StructureInstance(data) = v
            && let Some(reify_ir::Value::GeometryHandle { kernel_handle: Some(h), .. }) =
                data.fields.get("geometry")
            && *h == handle
            && data.fields.get("material").is_some()
        {
            return Some(resolve_instance_color(v, diagnostics));
        }
    }
    None
}

/// Resolve a `Physical` body instance's appearance color via task β's seam.
fn resolve_instance_color(
    instance: &reify_ir::Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> reify_ir::Rgb8 {
    let appearance = crate::appearance::resolve_appearance(instance);
    let color_field = if let reify_ir::Value::StructureInstance(app_data) = &appearance {
        app_data
            .fields
            .get("color")
            .cloned()
            .unwrap_or(reify_ir::Value::Undef)
    } else {
        reify_ir::Value::Undef
    };
    crate::appearance::resolve_color(&color_field, diagnostics)
}

impl Engine {
    /// Snapshot the realized-repr map from `eval_state` for the fail-closed
    /// region capability gate (task #4812, P0β).
    ///
    /// Returns `HashMap<RealizationNodeId, ReprKind>` built from
    /// `eval_state.snapshot.graph.realizations`, or an empty map when
    /// `eval_state` is `None` (first build — gate fails-open for unknown
    /// reprs, preserving pre-β behavior). Centralises the three duplicate
    /// constructions in `build_snapshot`, `build_with_geometry_output`
    /// (pre-loop and post-loop).
    ///
    /// Takes `&Option<EvaluationState>` rather than `&self` so callers that
    /// hold a concurrent `&mut self.geometry_kernels` borrow can still call
    /// it as `Engine::realized_reprs_snapshot(&self.eval_state)` — the borrow
    /// checker sees the two fields as disjoint.
    fn realized_reprs_snapshot(
        eval_state: &Option<EvaluationState>,
    ) -> HashMap<RealizationNodeId, ReprKind> {
        eval_state.as_ref().map_or_else(HashMap::new, |s| {
            s.snapshot.graph.realizations
                .iter()
                .map(|(id, data)| (id.clone(), data.produced_repr))
                .collect()
        })
    }

    /// Reset BOTH dispatch-attribution counters in lockstep at a build/tessellate
    /// entry point: the aggregate `last_dispatch_count` and the per-realization
    /// `last_dispatch_count_by_realization` map.
    ///
    /// Called at the top of every build/tessellate surface (`build_snapshot`,
    /// `build`, `tessellate_realizations`, `tessellate_snapshot`) so each call
    /// reports its OWN per-call dispatch attribution (and reports 0 / empty when
    /// fully served from the `RealizationCache`). A realization pruned from the
    /// demand cone never reaches `execute_realization_ops`, so it is never
    /// re-inserted into the map and its tally stays 0 — the headline hidden-body
    /// "0 ops" floor across a slider session.
    ///
    /// **Why a single helper:** both counters increment at the SAME dispatch site
    /// in `execute_realization_ops`, so `sum(map.values()) == last_dispatch_count`
    /// holds at every read (exact-by-construction). Resetting one without the
    /// other would silently break that equality — and the production GUI relies on
    /// it, surfacing the aggregate as `last_dispatch_count_by_realization().values()
    /// .sum()` because the gated `last_dispatch_count()` accessor is unreachable
    /// from a production (non-`test-instrumentation`) build. Zeroing both here
    /// makes it structurally impossible for the two resets to drift out of lockstep.
    #[inline]
    fn reset_dispatch_tallies(&mut self) {
        self.last_dispatch_count = 0;
        self.last_dispatch_count_by_realization.clear();
    }

    /// Bump BOTH dispatch-attribution counters in lockstep at the single
    /// `dispatch(...)` call site inside [`Self::execute_realization_ops`]: the
    /// aggregate `dispatch_count` and the per-realization
    /// `dispatch_count_by_realization` map (keyed by the issuing
    /// `realization_id`).
    ///
    /// **Why a single helper:** pairing the two increments here — the mirror of
    /// [`Self::reset_dispatch_tallies`] pairing the two resets — makes it
    /// structurally impossible to bump one without the other, so
    /// `sum(map.values()) == aggregate` holds at every read
    /// (exact-by-construction). The production GUI relies on that equality:
    /// `engine_state_json` surfaces the aggregate as
    /// `last_dispatch_count_by_realization().values().sum()` because the gated
    /// `last_dispatch_count()` accessor is unreachable from a non-
    /// `test-instrumentation` build. A future edit adding an aggregate bump
    /// elsewhere WITHOUT a paired map bump would silently break that production
    /// surface — routing every bump through this one helper prevents the drift.
    ///
    /// Uses a get_mut/insert split rather than `entry(realization_id.clone())`
    /// so the key is cloned ONLY on first insertion — the common
    /// re-dispatch-of-the-same-realization tick avoids the clone (the `entry`
    /// API always materializes its key argument).
    #[inline]
    fn bump_dispatch(
        dispatch_count: &mut usize,
        dispatch_count_by_realization: &mut HashMap<RealizationNodeId, usize>,
        realization_id: &RealizationNodeId,
    ) {
        *dispatch_count += 1;
        if let Some(count) = dispatch_count_by_realization.get_mut(realization_id) {
            *count += 1;
        } else {
            dispatch_count_by_realization.insert(realization_id.clone(), 1);
        }
    }

    /// Build geometry from the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists. Otherwise: checks constraints from
    /// snapshot (same as check_snapshot), then executes geometry operations from
    /// module realizations using the geometry kernel. This is the incremental
    /// companion to build(): after edit_param() updates values, call
    /// build_snapshot() to get updated geometry without a cold restart.
    ///
    /// # Tolerance wiring (task 2874)
    ///
    /// `build_snapshot` mirrors [`Self::build`] across all four production-
    /// wiring contracts (imported-tolerance-promise diagnostics, per-realization
    /// demanded tolerance, per-stage tolerance budget, `RealizationCache`
    /// populate/consult) — see [`Self::build`] for the full description. The
    /// only placement difference: because `build_snapshot` does NOT call
    /// `eval()` (it operates on the existing snapshot), the diagnostic-emission
    /// helper runs AFTER `check_constraints_against_templates` rather than
    /// before, since there is no eval-side scope clear to defend against.
    pub fn build_snapshot(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> Option<BuildResult> {
        // Task ε (3436) step-12: reset the dispatch-count instrumentation
        // counter at the entry to every build/tessellate surface so a second
        // build of the same module reports its own per-build dispatch tally
        // (and reports 0 when fully served from the RealizationCache).
        // Zeroes BOTH the aggregate and the per-realization tally in lockstep.
        self.reset_dispatch_tallies();
        // GHR-δ §5: clear the realization→handle validity map and reset the
        // revalidation slow-path counter at the start of every build surface;
        // the per-template `post_process_geometry_handle_cells` below
        // repopulates the map with this build's resolved handles.
        self.realization_handles.clear();
        self.reset_geometry_revalidation_slow_path_count();
        // γ (task 4739): demand-prune Pending producer. On the warm/selective
        // path (full_scope OFF) flip every pruned-Final cached node to Pending
        // so a hidden body's value is never served as a silently-stale Final
        // number (arch §8 prune-safety scenario 3). No-op under full_scope and
        // when every node is demanded; re-run every warm build (a cold pass can
        // re-Final a still-hidden body between warm edits).
        self.mark_demand_pruned_pending();
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let (constraint_results, mut diagnostics) =
            self.check_constraints_against_templates(module, &values, Some(&state.snapshot.values));

        // Task 2874: emit imported-tolerance-promise diagnostics
        // (`ImportedTolerancePromiseInsufficient` / `InputTolerancePromiseIsZero`)
        // for every (Input × Output × active-purpose-binding) triple recognised
        // in the post-eval snapshot. See `Engine::emit_imported_tolerance_promise_diagnostics_for_module`
        // for the recognition shapes and code-agnostic forwarding contract.
        // Mirrored in `build` and `tessellate_realizations`.
        self.emit_imported_tolerance_promise_diagnostics_for_module(module, &mut diagnostics);

        // Execute geometry operations. Use the snapshot's eval-round id rather
        // than `self.next_version_id`: build_snapshot is keyed off `state.snapshot.values`,
        // so Failed events must carry that snapshot's version, not the un-used
        // next round that `next_version_id` points at after prior eval/edit calls.
        let version_id = self.current_eval_version();
        // Task 2874 step-6: precompute per-realization demanded tolerance
        // BEFORE the `if let Some(ref mut kernel) = self.geometry_kernel`
        // borrow below so the `&self` queries inside
        // `compute_demanded_tols` don't collide with the kernel / table
        // mutable borrows handed to `execute_realization_ops`. Missing keys
        // are treated as `None`.
        let demanded_tols = self.compute_demanded_tols(module);
        // Task 4050 step-16 (gap 3 / υ wiring): derive the per-realization
        // demanded terminal `ReprKind` once per build, positionally aligned
        // with `demanded_tols` by `[t_idx][r_idx]`. Terminal Stl/Obj
        // realizations demand Mesh, driving the cross-kernel conversion
        // executor when a Mesh-capable kernel is registered (and otherwise
        // falling back to BRep — design_decision 3). Same `&self`-query
        // hoisting rationale as `compute_demanded_tols` above.
        let demanded_reprs = self.compute_demanded_reprs(module, format);
        let boundary_demands = self.compute_boundary_demands(module);
        // Task ε (3436): resolve the engine's default kernel through the new
        // multi-handle map. Single-handle surfaces (export, post-process)
        // operate on this kernel; per-op dispatch routing is delegated to
        // `execute_realization_ops` which takes the full kernels map +
        // dispatch registry (step-8 wiring).
        let default_kernel_name = self.default_kernel_name.clone();
        // Step-8 (task ε / 3436): source the capability-descriptor registry
        // ONCE per build via `collect_registry()` and materialise the
        // borrowed view that `dispatcher::dispatch` expects. The owned map
        // outlives the borrowed view because both are local bindings.
        // Mirrors the "one allocation per build, not per realization"
        // pattern established by `compute_tessellation_budgets`.
        //
        // Task 4050 test seam: in test / `test-instrumentation` builds an
        // injected `test_registry_override` (set via
        // `with_test_kernels_and_registry`) takes precedence over the link-time
        // inventory so the cross-kernel-handoff integration test can supply a
        // deterministic multi-kernel capability map (the live inventory links no
        // Mesh-capable boolean kernel). The override is cloned into an owned
        // local so the borrowed view below does not pin `&self`. Production
        // builds always use `collect_registry()` — the field is absent there.
        #[cfg(any(test, feature = "test-instrumentation"))]
        let registry_owned = self
            .test_registry_override
            .clone()
            .unwrap_or_else(crate::kernel_registry::collect_registry);
        #[cfg(not(any(test, feature = "test-instrumentation")))]
        let registry_owned = crate::kernel_registry::collect_registry();
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry_owned.iter().map(|(k, v)| (k.clone(), v)).collect();
        let geometry_output = if let Some(name) = default_kernel_name.as_deref()
            && self.geometry_kernels.contains_key(name)
        {
            let mut step_handles: Vec<KernelHandle> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            // θ (task 4361): record each realization's terminal handle positionally
            // by (t_idx, r_idx) for the Phase-B export walk — mirrors build()'s
            // terminal_handles pattern (:2608) so collect_export_bodies_walk can
            // surface the correct product body for each entity.
            let mut terminal_handles: Vec<Vec<Option<KernelHandle>>> = module
                .templates
                .iter()
                .map(|t| vec![None; t.realizations.len()])
                .collect();

            self.topology_attribute_table = TopologyAttributeTable::default();
            self.swept_kind_table = SweptKindTable::default();
            // Task 3441: cross-template `GeomRef::Sub` threading.  As each
            // template's realizations complete, snapshot its `named_steps`
            // under the template name so a subsequent template that has
            // `sub <s> = <T>()` can seed its local `named_steps` with
            // `<s>.<member> → handle` entries derived from `T`'s snapshot.
            // Declaration order is treated as topological for non-recursive
            // structures (compile_builder/entities_phase.rs pushes templates
            // in declaration order; SCC detection tags cycles but does not
            // reorder).  Forward-declared subs and recursive structures fall
            // back to the existing "named_steps miss → Error" path in
            // `geometry_ops.rs::resolve_geom_ref`.
            //
            // Helper invocations (`seed_cross_sub_named_steps`,
            // `snapshot_named_steps`) factor the per-template seed/snapshot
            // logic out so the three eval loop sites stay in sync.
            let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> =
                HashMap::new();
            // GR-034: resolve once per eval-loop entry; threaded per-iteration.
            let long_chain_threshold = crate::dispatcher::long_chain_threshold_from_env();
            for (t_idx, template) in module.templates.iter().enumerate() {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template `GeomRef::Sub`
                // references are now supported for non-collection subs via
                // compound keys `<sub_name>.<member>` seeded below (task 3441);
                // collection-sub geometry composition remains deferred (the
                // compile-side diagnostic in `expr.rs::try_emit_cross_sub_geometry`
                // continues to fire for those call sites).
                let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
                // Task 5033 Gap #2 Gap A: by-name repr sibling of `named_steps`.
                // See `RealizationOutputs::named_step_reprs` doc for why.
                let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
                seed_cross_sub_named_steps(
                    template,
                    &module_named_steps,
                    &mut named_steps,
                    &mut self.geometry_kernels,
                    name,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    &mut diagnostics,
                    &module.templates,
                );
                for (r_idx, realization) in template.realizations.iter().enumerate() {
                    // Task 2874, step-6 wiring: per-realization demanded
                    // tolerance for the cache-key triple `(entity_id,
                    // ReprKind::BRep, demanded_tol)`. Priority chain is
                    // `demanded_tolerance_for_output(template_name, entity)
                    // → active_tolerance_for(entity)`; when both return
                    // `None` no cache entry is written (the helper
                    // preserves historical "no tolerance contract → no
                    // caching" semantics for that branch). The Vec is
                    // precomputed above the kernel borrow.
                    // Task 3227: positional lookup by [t_idx][r_idx].
                    let demanded_tol = demanded_tols
                        .get(t_idx)
                        .and_then(|v| v.get(r_idx))
                        .copied()
                        .unwrap_or(None);
                    let mut kernel_error: Option<ErrorRef> = None;
                    // Step-10 (task ε / 3436): channel for the executor's
                    // terminal produced [`ReprKind`]; written into the
                    // snapshot graph node below via disjoint-field borrows
                    // of `self.geometry_kernels` vs. `self.eval_state`.
                    let mut produced_repr_out: Option<ReprKind> = None;
                    // Task 4248 piece-3: capture step_handles length before
                    // this realization to identify its terminal handle (mirrors
                    // the handle_start bookkeeping in build() at ~:2299).
                    let handle_start_snap = step_handles.len();
                    // Task 4744 β step-20: feed the real morph inputs. Disjoint
                    // immutable field borrows (morph_producer / morph_source /
                    // eval_state) compose with the disjoint &mut field borrows
                    // the call takes (geometry_kernels / tables / cache). The
                    // eval_state borrow ends when the call returns, before the
                    // `eval_state.as_mut()` write below.
                    let morph_io = crate::morph_producer::MorphDispatchIo {
                        producer: self.morph_producer.as_deref(),
                        source: self.morph_source.get(&realization.id),
                        new_graph: self.eval_state.as_ref().map(|s| &s.snapshot.graph),
                    };
                    let morph_source_out = Engine::execute_realization_ops(
                        RealizationOpsInput::new(
                            &mut self.geometry_kernels,
                            &registry_borrowed,
                            name,
                            &realization.operations,
                            &values,
                            &self.functions,
                            &self.meta_map,
                            &mut diagnostics,
                            &realization.id,
                            realization.span,
                            &mut kernel_error,
                            &mut self.realization_cache,
                            &mut self.last_dispatch_count,
                            &mut self.last_dispatch_count_by_realization,
                        )
                        .with_realization_name(realization.name.as_deref())
                        .with_demanded_tol(demanded_tol)
                        // Task 4050 step-16 (gap 3): pass the υ-derived
                        // per-realization demanded terminal repr, positionally
                        // aligned with `demanded_tols` (`[t_idx][r_idx]`);
                        // out-of-range defaults to BRep (backward-compat).
                        .with_demanded_repr(
                            demanded_reprs
                                .get(t_idx)
                                .and_then(|v| v.get(r_idx))
                                .copied()
                                .unwrap_or(ReprKind::BRep),
                        )
                        .with_demanded_boundary(
                            boundary_demands
                                .get(t_idx)
                                .and_then(|v| v.get(r_idx))
                                .copied()
                                .unwrap_or(false),
                        )
                        // Task #3443: thread module-scope #kernel(...) pragma
                        // from the public entry point into the per-op dispatcher.
                        .with_prefer_kernel(module.kernel_pragma.as_deref())
                        .with_is_terminal_realization(r_idx + 1 == template.realizations.len())
                        // Task 4744 β step-20: feed the real morph inputs
                        // (producer + prior-tick source + new BRep graph),
                        // bound to `morph_io` just above the call.
                        .with_morph_io(morph_io)
                        // GR-034: resolved once at entry (see above).
                        .with_long_chain_threshold(long_chain_threshold),
                        RealizationOutputs::new(
                            &mut step_handles,
                            &mut named_steps,
                            &mut named_step_reprs,
                            &mut self.topology_attribute_table,
                            &mut self.swept_kind_table,
                            &mut produced_repr_out,
                        ),
                    );
                    // θ (task 4361): record this realization's terminal handle
                    // by (t_idx, r_idx) for the Phase-B export walk, mirroring
                    // build()'s terminal_handles bookkeeping (:2803).
                    if step_handles.len() > handle_start_snap {
                        terminal_handles[t_idx][r_idx] = step_handles.last().copied();
                    }
                    // Step-10 (task ε / 3436): persist the executor's terminal
                    // [`ReprKind`] into the snapshot graph node. The
                    // `eval_state` field is disjoint from `geometry_kernels`,
                    // so the borrow is independent of the per-realization
                    // executor borrows above. On rollback / no-op the
                    // executor leaves the channel `None` and we skip the
                    // write so the construction-time default survives.
                    //
                    // Task 4248 piece-3: also write `produced_kernel` from the
                    // terminal KernelHandle (step_handles grew ↔ ops executed).
                    // Independent of produced_repr_out so cache-hit realizations
                    // that set only one channel still record their kernel.
                    //
                    if let Some(state) = self.eval_state.as_mut()
                        && let Some(node) =
                            state.snapshot.graph.realizations.get_mut(&realization.id)
                    {
                        if let Some(repr) = produced_repr_out {
                            node.produced_repr = repr;
                        }
                        if step_handles.len() > handle_start_snap {
                            node.produced_kernel = step_handles.last().map(|h| h.kernel);
                        }
                        // Task 4728 α: compute inside the guard so the work is
                        // skipped when the node is absent (eval_state None or
                        // realization not yet in the graph). `self.functions` and
                        // `self.meta_map` are disjoint fields from `eval_state`
                        // so the NLL borrow checker allows the immutable borrows
                        // here alongside the existing `&mut state` / `&mut node`.
                        // The [u8;32] result is Copy — no lifetime escapes the block.
                        // Unconditional on cache-hit vs cache-miss: the INPUT hash
                        // is well-defined regardless of whether ops ran.
                        // Consumer: task β recompute-then-compare seeding.
                        let input_cone_hash_snap = {
                            let ctx = crate::eval_ctx_with_meta(
                                &values,
                                &self.functions,
                                &self.meta_map,
                            );
                            compute_realization_upstream_values_hash(realization, &ctx)
                        };
                        node.input_cone_hash = Some(input_cone_hash_snap);
                    }
                    // Arch §9.1 lines 868–877: kernel error on a realization →
                    // mark realization NodeId as Failed { error } and emit one
                    // EventKind::Failed event. The Diagnostic::error("geometry
                    // error: …") inside `execute_realization_ops` is preserved.
                    if let Some(error) = kernel_error {
                        Engine::mark_realization_failed(
                            &mut self.cache,
                            &mut self.journal,
                            &realization.id,
                            error,
                            version_id,
                        );
                    }
                    // Task 4744 β step-20: stash this realization's source
                    // bundle (returned by the executor) for the NEXT tick's
                    // morph. `store_morph_source` needs `&mut self`; safe here —
                    // the executor's disjoint field borrows have all released.
                    if let Some(src) = morph_source_out {
                        self.store_morph_source(realization.id.clone(), src);
                    }
                }
                // Step-8 (task ε / 3436): the post-process helpers operate on
                // the engine's default kernel. We re-borrow it from the
                // `geometry_kernels` map here (after the per-realization loop
                // released its `&mut self.geometry_kernels` borrow). The
                // GHR-γ step-6: mirror of the build() hydration — stamp
                // Type::Geometry value cells with real kernel handles so
                // build_snapshot callers see the same GeometryHandle values.
                // GHR-δ: also records geometry-backed Realizations as
                // freshness-bearing cache nodes (esc-3606-37 ruling step 1).
                Engine::post_process_geometry_handle_cells(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    &mut self.cache,
                    &mut self.realization_handles,
                    version_id,
                );
                // Task #4726 / esc-3787-23: post-hydration re-dispatch pass.
                // Mirror of the `build()` call — see that site for the full
                // rationale.  Must call BEFORE re-borrowing `default_kernel`
                // from `self.geometry_kernels.get_mut(name)` to avoid a
                // conflicting whole-`self` borrow.
                self.redispatch_geometry_consuming_compute_nodes(
                    module,
                    &mut values,
                    version_id,
                    &mut diagnostics,
                );
                // Task #3787 ε: cascade re-dispatch for field-consuming nodes
                // (e.g. solve_elastic_static) that depend on the now-hydrated
                // as_printed_material field.  Mirrors the build() call site.
                self.redispatch_as_printed_consuming_compute_nodes(
                    module,
                    &mut values,
                    version_id,
                    &mut diagnostics,
                );
                // `expect` is justified by the outer `contains_key(name)` gate:
                // the executor never removes entries from the map.  Placed AFTER
                // `redispatch_geometry_consuming_compute_nodes` (task #4726) to
                // avoid the whole-`self` borrow conflict.
                let default_kernel = self.geometry_kernels.get_mut(name).expect(
                    "default kernel must remain in the map across the per-realization loop",
                );
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in `build` and
                // `tessellate_from_values` — keep all four call sites in
                // sync (follow-up: the broader build/build_snapshot
                // realization-loop duplication is noted separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    default_kernel.as_ref(),
                    &mut diagnostics,
                );
                // Task 2531: kinematic-query post-process (interferes /
                // interferes_with / min_clearance). Mirrors the conformance-
                // query wiring; runs after `named_steps` is populated so the
                // helpers can resolve each Snapshot body's `solid` String to
                // a `GeometryHandleId`.
                Engine::post_process_kinematic_queries(
                    template,
                    &named_steps,
                    &mut values,
                    default_kernel.as_mut(),
                    &mut diagnostics,
                );
                let realized_reprs = Engine::realized_reprs_snapshot(&self.eval_state);
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    default_kernel.as_mut(),
                    &self.topology_attribute_table,
                    &self.swept_kind_table,
                    &realized_reprs,
                    &mut diagnostics,
                    &module.templates,
                );
                // task 4222 δ: re-evaluate Undef Let cells with containment hook.
                // Mirrors the identical call in `build()` — see that site for the
                // rationale (post_process_derived_lets updates `restricted` but
                // evaluates v_in without containment → Undef; this pass fixes it).
                self.post_process_containment_samples(template, &mut values);
                // Task 3441: snapshot this template's `named_steps` so a
                // later template that subs from it can seed compound-key
                // entries.  Placed AFTER the post-process queries so the
                // local `named_steps` reflects the same view the post-process
                // helpers saw (the post-process helpers do not write to
                // `named_steps`, so ordering is informational rather than
                // load-bearing — but keeping the snapshot here documents the
                // "complete snapshot" intent).  `named_steps` is moved (not
                // cloned) — it would fall out of scope at the loop body's
                // end anyway, and the post-process helpers above only
                // borrow it.
                snapshot_named_steps(template, named_steps, &mut module_named_steps);
            }

            if step_handles.is_empty() {
                // Only emit the summary diagnostic when ops were actually declared
                // but all failed; when no ops were declared there is simply no geometry.
                if had_realization_ops {
                    diagnostics.push(Diagnostic::error(
                        "all geometry operations failed; no geometry output produced",
                    ));
                }
                None
            } else {
                // θ (task 4361): mirror build()'s Phase-B export walk — collect
                // placed-product BRep handles via collect_export_bodies_walk, then
                // export only the product (default_visible) bodies.  This replaces
                // the old `*step_handles.last()` single-handle export that did not
                // assemble a compound for multi-entity modules (the §6 export bug).
                let export_bodies = Self::collect_export_bodies_walk(
                    module,
                    &terminal_handles,
                    &mut self.geometry_kernels,
                    name,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    &mut diagnostics,
                    None,
                );

                let product_bodies: Vec<_> = export_bodies
                    .into_iter()
                    .filter(|b| b.default_visible)
                    .collect();

                match product_bodies.len() {
                    0 => {
                        if had_realization_ops {
                            diagnostics.push(Diagnostic::error(
                                "all realized bodies are aux; no product geometry to export",
                            ));
                        }
                        None
                    }
                    1 => {
                        // δ (task #4763): thread body color via export_with_options.
                        let mut output = Vec::new();
                        let default_kernel = self
                            .geometry_kernels
                            .get(name)
                            .expect("default kernel must remain in the map for export");
                        let body_color = resolve_export_body_color(
                            &values,
                            &product_bodies[0].entity_path,
                            product_bodies[0].handle_id,
                            &mut diagnostics,
                        );
                        match default_kernel.export_with_options(
                            product_bodies[0].handle_id,
                            format,
                            &reify_ir::ExportOptions {
                                step_schema: reify_ir::StepSchema::default(),
                                color: body_color,
                                include_materials: false,
                                include_colors: false,
                            },
                            &mut output,
                        ) {
                            Ok(_warnings) => Some(output),
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                                None
                            }
                        }
                    }
                    _ => {
                        let ids: Vec<GeometryHandleId> =
                            product_bodies.iter().map(|b| b.handle_id).collect();
                        let default_kernel = self
                            .geometry_kernels
                            .get_mut(name)
                            .expect("default kernel must remain in the map for compound export");
                        match default_kernel.make_compound(&ids) {
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(format!(
                                    "compound assembly error: {}",
                                    e
                                )));
                                None
                            }
                            Ok(compound) => {
                                let mut output = Vec::new();
                                let default_kernel = self
                                    .geometry_kernels
                                    .get(name)
                                    .expect("default kernel must remain in the map for export");
                                match default_kernel.export_with_options(
                                    compound.id,
                                    format,
                                    &reify_ir::ExportOptions {
                                        step_schema: reify_ir::StepSchema::default(),
                                        color: None,
                                        include_materials: false,
                                        include_colors: false,
                                    },
                                    &mut output,
                                ) {
                                    Ok(_warnings) => Some(output),
                                    Err(e) => {
                                        diagnostics.push(Diagnostic::error(format!(
                                            "export error: {}",
                                            e
                                        )));
                                        None
                                    }
                                }
                            }
                        }
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
    ///
    /// # Tolerance wiring (tasks 2874, 3103)
    ///
    /// `build` (alongside [`Self::build_snapshot`],
    /// [`Self::tessellate_realizations`], and [`Self::tessellate_snapshot`])
    /// participates in four production-wiring contracts that route the
    /// demanded-tolerance subsystem from authoring-time templates to
    /// kernel-time realization:
    ///
    /// 1. **Imported-tolerance-promise diagnostics** — invokes
    ///    [`Self::emit_imported_tolerance_promise_diagnostics_for_module`]
    ///    AFTER `check()`. Task 3103 consolidated the placement by preserving
    ///    `active_purpose_bindings` across `eval()` (see `engine_eval.rs`), so
    ///    the pre-check workaround is no longer required. All four surfaces now
    ///    emit AFTER their respective constraint check.
    /// 2. **Per-realization demanded tolerance** — computes
    ///    `(template_name, entity) → Option<f64>` via the
    ///    [`Engine::demanded_tolerance_for_output`] →
    ///    [`Engine::active_tolerance_for`] priority chain AFTER `check()`.
    ///    Eval preservation (task 3103) ensures the scope survives the
    ///    internal `eval()` round-trip inside `check()`.
    /// 3. **Per-stage tolerance budget** — routes the demanded tolerance
    ///    through [`Engine::compute_realization_tolerance_budget`] against
    ///    [`crate::kernel_registry::collect_registry`] so multi-kernel
    ///    chain dispatch (when v0.3 adapters land) splits the budget across
    ///    representation conversions; with the v0.2 occt-only inventory the
    ///    budget passes through unchanged.
    /// 4. **`RealizationCache` populate/consult** — `execute_realization_ops`
    ///    consults `realization_cache` at the top of the helper for an
    ///    `(entity, ReprKind::BRep, demanded_tol)` hit (cache short-circuits
    ///    kernel re-execution under the partial-order rule
    ///    `cached_tol ≤ requested_tol`) and, on a cache miss, populates the
    ///    same key with the terminal handle after a fully-successful
    ///    realization. Cache lifetime is engine-scoped — entries persist
    ///    across `build` / `build_snapshot` / `tessellate_realizations`.
    ///
    /// All four contracts are pinned end-to-end by
    /// `end_to_end_tolerance_wiring_threads_promise_diagnostic_cache_and_per_stage_budget`
    /// in `crates/reify-eval/tests/tolerance_wiring_e2e.rs`.
    pub fn build(&mut self, module: &CompiledModule, format: ExportFormat) -> BuildResult {
        // The public imperative build: realize geometry AND serialize the
        // Phase-B product bodies into `geometry_output` (the single-output,
        // format-from-a-flag path). Delegates to the shared realization worker
        // with the Phase-B product export ENABLED.
        self.build_with_geometry_output(module, format, true)
    }

    /// Internal realization worker shared by [`Self::build`] and
    /// [`Self::build_outputs`] (io-export δ).
    ///
    /// `emit_geometry_output` controls ONLY the trailing Phase-B product-body
    /// export: with `true` (the imperative [`Self::build`]) the product bodies
    /// are serialized into [`BuildResult::geometry_output`]; with `false`
    /// (`build_outputs`) that export is skipped and `geometry_output` is `None`.
    /// Realization, `Value::GeometryHandle` hydration, `realization_handles`
    /// population, and constraint checking are IDENTICAL on both paths — only the
    /// final serialization differs. `build_outputs` needs the hydrated handles
    /// but drives its own per-occurrence export, so the Phase-B export would be
    /// redundant work and — under a recording kernel — a spurious extra
    /// `export()` call that does not belong to any DSL `Output` occurrence.
    ///
    /// See [`Self::build`]'s doc comment for the four production-wiring contracts
    /// (tolerance-promise diagnostics, per-realization demanded tolerance,
    /// per-stage budget, `RealizationCache`) this worker threads.
    fn build_with_geometry_output(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
        emit_geometry_output: bool,
    ) -> BuildResult {
        // Geometric-relations ζ (task 4386) step-18: per-scope relate-solve.
        //
        // For each scope with `at auto` subs + relations, solve each auto sub's
        // 6-DOF assembly Frame from the scope's geometric relations and verify the
        // redundant remainder. This runs BEFORE the main check/surfacing so the
        // solved Frames can be injected into `values` (below) for the surfacing walk
        // to place via `eval_sub_pose`'s auto arm.
        //
        // Realization sub-builds each referenced leaf structure through `self`
        // (`relate_solve::solve_scopes` → `realize_operand_datums`), so it requires a
        // registered geometry kernel AND must run here, before this build's own state
        // resets: the sub-build mutates `self`'s transient build state, and the outer
        // resets + `self.check(module)` below re-establish the main-module state. ζ's
        // leaf structures carry no auto/relations, so the sub-build does not recurse
        // into another relate-solve (single-level). When no kernel is registered the
        // pass is skipped (auto subs degrade to identity; a geometry-less build has no
        // placement to compute).
        let kernel_available = match self.default_kernel_name.as_deref() {
            Some(name) => self.geometry_kernels.contains_key(name),
            None => false,
        };
        let relate_solutions: Vec<(String, crate::relate_solve::RelateSolution)> = if kernel_available
        {
            crate::relate_solve::solve_scopes(module, self)
        } else {
            Vec::new()
        };

        // Task ε (3436) step-12: reset the dispatch-count instrumentation
        // counter at the entry to every build/tessellate surface so a second
        // build of the same module reports its own per-build dispatch tally
        // (and reports 0 when fully served from the RealizationCache). Mirrors
        // the reset at the top of `build_snapshot` / `tessellate_realizations`
        // / `tessellate_snapshot` — must run BEFORE `check()` because no
        // dispatcher call should be counted against the build that hasn't
        // entered the per-realization op loop yet.
        // Zeroes BOTH the aggregate and the per-realization tally in lockstep.
        self.reset_dispatch_tallies();
        // Task 4355 β: capture declaration-order execution order for the
        // assert_dag_complete gate.  Realizations are visited in the same
        // order as the build loop below (templates × realizations in
        // declaration order, which compile_builder/entities_phase guarantees
        // is topological for non-recursive structures).  Captured here, once,
        // before any kernel work, so the assert can fire even when the
        // geometry block is skipped (no kernel registered).
        #[cfg(debug_assertions)]
        let exec_order: Vec<RealizationNodeId> = module
            .templates
            .iter()
            .flat_map(|t| t.realizations.iter().map(|r| r.id.clone()))
            .collect();
        // GHR-δ §5: clear the realization→handle validity map and reset the
        // revalidation slow-path counter at the start of the build; the
        // per-template `post_process_geometry_handle_cells` below repopulates
        // the map with this build's resolved handles.
        self.realization_handles.clear();
        self.reset_geometry_revalidation_slow_path_count();
        // PLACEMENT: AFTER check() — task 3103 consolidated the lifecycle so
        // eval() preserves active_purpose_bindings across the call, making the
        // pre-check workaround obsolete. All four surfaces (build /
        // build_snapshot / tessellate_realizations / tessellate_snapshot) now
        // share the post-check placement. See engine_eval.rs for the
        // preservation site (task 3103).
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        // Task 2874: emit imported-tolerance-promise diagnostics
        // (`ImportedTolerancePromiseInsufficient` / `InputTolerancePromiseIsZero`)
        // for every (Input × Output × active-purpose-binding) triple recognised
        // in the post-`check()` snapshot. See
        // `Engine::emit_imported_tolerance_promise_diagnostics_for_module` for
        // the recognition shapes and code-agnostic forwarding contract.
        self.emit_imported_tolerance_promise_diagnostics_for_module(module, &mut diagnostics);

        // Task 2874 step-6: precompute per-realization demanded tolerance
        // AFTER `self.check(module)` — eval() now preserves active_purpose_bindings
        // (task 3103), so the priority chain `demanded_tolerance_for_output →
        // active_tolerance_for` correctly reads the preserved/re-injected scope.
        // `build_snapshot` does NOT call eval, so its placement (after the
        // constraint check) was already semantically correct.
        let demanded_tols = self.compute_demanded_tols(module);
        // Task 4050 step-16 (gap 3 / υ wiring): derive the per-realization
        // demanded terminal `ReprKind` once per build, positionally aligned
        // with `demanded_tols` by `[t_idx][r_idx]`. Terminal Stl/Obj
        // realizations demand Mesh, driving the cross-kernel conversion
        // executor when a Mesh-capable kernel is registered (and otherwise
        // falling back to BRep — design_decision 3). Same post-`check()`
        // placement rationale as `compute_demanded_tols` above.
        let demanded_reprs = self.compute_demanded_reprs(module, format);
        let boundary_demands = self.compute_boundary_demands(module);
        // Task 2320: `values` is moved out of `check_result` here so the
        // per-template post-process can patch conformance-query results
        // (`is_watertight` / `is_manifold` / `is_orientable`) into the map
        // before it is moved into the returned `BuildResult` below.
        let mut values = check_result.values;

        // Geometric-relations ζ (task 4386) step-18: write each scope's solved
        // `at auto` pose Frame back into `values` (keyed by `auto_pose_cell`) so the
        // surfacing walk's `eval_sub_pose` auto arm reads it and places the sub via
        // `ApplyTransform` (task 3901). Surface the relate-solve verification
        // diagnostics here too — a redundant-remainder assertion failure or a
        // driving-set conflict is an `Error` that fails the build.
        //
        // Geometric-joints δ (task 4398) step-4/6: also write each scope's solved mount
        // Frame into the `"origin"` key of the joint Map the scope mounts for that sub
        // (via `reify_stdlib::set_mount_origin`), realising the mount→origin handshake
        // (PRD §7.2 ordering: solve→write→FK).
        //
        // The write is NARROW: `mounted_joint_cell` returns `Some` only when a
        // joint cell's constructor args reference the mounted sub (DD1 operand-
        // reference association rule, task #4399).  Joints that are not associated
        // with any relate scope carry NO `"origin"` key, preserving the byte-identical
        // B9 back-compat invariant (KIN-OFFSET α's absent-origin → identity no-op).
        //
        // Task #4399 has landed: `mounted_joint_cell` now scans `template.value_cells`
        // for a motion-joint constructor whose args decode to an OperandRef referencing
        // the mounted sub; the Some-branch (positive path) fires for relate-mounted
        // joints and is covered end-to-end by the B6 producer assertion in
        // `crates/reify-eval/tests/relate_mounted_joint_sweep_e2e.rs`.
        for (scope, solution) in &relate_solutions {
            for (sub, frame) in &solution.poses {
                values.insert(crate::relate_solve::auto_pose_cell(scope, sub), frame.clone());
                // ε: write mount Frame into the mounted joint's origin, if the scope
                // mounts one (DD1 operand-reference match, task #4399).  The Some-branch
                // is covered by the B6 producer assertion in
                // `relate_mounted_joint_sweep_e2e.rs`; the None path (no associated
                // motion joint, or literal-axis joint) is covered by B9 tests in
                // `relate_mount_origin_e2e.rs`.
                // Performance note: `mounted_joint_cell` re-finds the scope template and
                // rescans all value cells once per (scope, sub) pair — O(S × templates ×
                // cells) per scope. At current model sizes this is negligible; if profiling
                // ever flags it, resolve the template once outside the inner `sub` loop and
                // pass it in, or precompute a sub→joint-cell map for the scope before
                // iterating `solution.poses`.
                if let Some(cell_id) =
                    crate::relate_solve::mounted_joint_cell(scope, sub, module)
                    && let Some(joint_val) = values.get(&cell_id).cloned()
                {
                    values.insert(cell_id, reify_stdlib::set_mount_origin(joint_val, frame));
                }
            }
            diagnostics.extend(solution.diagnostics.iter().cloned());
        }

        // Use the eval round that produced `values`. `check()` already
        // called `eval()` which bumped `next_version_id` past
        // `snapshot.version`, so reading `self.next_version_id` here
        // would tag Failed events one round ahead of the values that
        // caused the kernel failure.
        let version_id = self.current_eval_version();
        // Task ε (3436): resolve default kernel through the multi-handle map
        // (see `build_snapshot` mirror for the same pattern).
        let default_kernel_name = self.default_kernel_name.clone();
        // Step-8 (task ε / 3436): source the capability-descriptor registry
        // once per build and materialise the borrowed view that
        // `dispatcher::dispatch` expects — see `build_snapshot` mirror for
        // the rationale (one allocation per build, not per realization).
        //
        // Task 4050 test seam: an injected `test_registry_override` takes
        // precedence over the link-time inventory in test / `test-instrumentation`
        // builds (see the `build_snapshot` mirror for the full rationale).
        // Production builds always use `collect_registry()`.
        #[cfg(any(test, feature = "test-instrumentation"))]
        let registry_owned = self
            .test_registry_override
            .clone()
            .unwrap_or_else(crate::kernel_registry::collect_registry);
        #[cfg(not(any(test, feature = "test-instrumentation")))]
        let registry_owned = crate::kernel_registry::collect_registry();
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry_owned.iter().map(|(k, v)| (k.clone(), v)).collect();

        // β (task 4738) step-4: route through `demand_scoped_unified_pass` for
        // the uniform demand seam (all three build/tessellate sites now go through
        // the same helper). Because `build()` called `self.check(module)` above
        // (which triggers `eval()` → `set_full_scope(true)`), `is_full_scope()`
        // is ALWAYS true at this point — the helper takes the full-scope branch:
        //   (Some(run_unified_pass(&state.snapshot.graph, &state.trace_map)), None)
        // → `unified_pass` is byte-identical to the pre-β inline call,
        // `demand_seed_build = None` → the fallback loop below appends all
        // uncovered realizations (byte-identical, step-3(b) guard stays GREEN).
        // `pass.diagnostics` is the full `run_unified_pass` diagnostic set
        // (E_EVAL_CYCLE / E_EVAL_UNRESOLVED) — preserved at line ~3626,
        // step-3(a) guard stays GREEN. The selective branch is reachable only
        // defensively (eval() always sets full_scope before this site); when
        // reached, `demand_seed_build=Some(seed)` guards the fallback below.
        // LegacyMultiPass → (None, None), byte-unchanged.
        let (unified_pass, demand_seed_build) = self.demand_scoped_unified_pass(&HashSet::new());

        // Task 4358 ε: the value cells read by ANY realization (the union of every
        // realization trace's `reads`). A selector cell in this set is consumed as
        // a curated fillet/chamfer/draft edge/face list, so
        // `hydrate_value_cell_in_loop` resolves it one step past its
        // `Value::Selector` descriptor to a concrete `List<Geometry>`; selector
        // cells consumed only by selector-composition value cells are absent here
        // and keep their descriptor form (so `reconstruct_selector_value` still
        // sees a `Value::Selector` child). Empty under LegacyMultiPass — the whole
        // schedule-driven hydration is gated on `unified_pass.is_some()`.
        // Empty under LegacyMultiPass (unified_pass is None → loop below never
        // uses the cells).  Delegate to the shared helper to avoid duplicating
        // the trace_map iteration.
        let realization_read_cells: HashSet<reify_core::ValueCellId> = if unified_pass.is_some() {
            self.realization_read_cells()
        } else {
            HashSet::new()
        };

        // Task 4358 ε (step-8): hoisted out of the `geometry_output` block so the
        // realization-produced per-template handle maps survive to the
        // post-geometry Constraint re-check below (the folding source for INLINE
        // geometry-query constraints under UnifiedDag). Populated by
        // `snapshot_named_steps` inside the realization loop on EITHER scheduler;
        // only READ post-loop under UnifiedDag, so LegacyMultiPass is unaffected.
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();

        let geometry_output = if let Some(name) = default_kernel_name.as_deref()
            && self.geometry_kernels.contains_key(name)
        {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<KernelHandle> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

            // T7 (task 3905): record each realization's terminal handle
            // positionally by (t_idx, r_idx) — mirrors the tessellate_from_values
            // Phase-A bookkeeping.  The Phase-B export walk (surface_export_bodies)
            // uses these handles to collect placed product bodies for STEP export.
            let mut terminal_handles: Vec<Vec<Option<KernelHandle>>> = module
                .templates
                .iter()
                .map(|t| vec![None; t.realizations.len()])
                .collect();

            self.topology_attribute_table = TopologyAttributeTable::default();
            self.swept_kind_table = SweptKindTable::default();
            // Task 3441: cross-template `GeomRef::Sub` threading.  As each
            // template's realizations complete, snapshot its `named_steps`
            // under the template name so a subsequent template that has
            // `sub <s> = <T>()` can seed its local `named_steps` with
            // `<s>.<member> → handle` entries derived from `T`'s snapshot.
            // Declaration order is treated as topological for non-recursive
            // structures (compile_builder/entities_phase.rs pushes templates
            // in declaration order; SCC detection tags cycles but does not
            // reorder).  Forward-declared subs and recursive structures fall
            // back to the existing "named_steps miss → Error" path in
            // `geometry_ops.rs::resolve_geom_ref`.
            //
            // Helper invocations (`seed_cross_sub_named_steps`,
            // `snapshot_named_steps`) factor the per-template seed/snapshot
            // logic out so the three eval loop sites stay in sync.
            // `module_named_steps` is declared above the `geometry_output` block
            // (task 4358 ε step-8) so it survives to the post-geometry Constraint
            // re-check; it is still populated here by `snapshot_named_steps`.

            // β (task 4738) amend: pre-extract demanded realization IDs as
            // references to avoid a per-iteration `RealizationNodeId` clone in
            // the uncovered-realization fallback.  Under build() full_scope is
            // always true → `demand_seed_build` is None → `demanded_rids_build`
            // is None → the fallback short-circuits via `is_none_or` and appends
            // all (byte-identical, step-3(b) guard preserved).  The defensive
            // selective branch is still covered without per-iteration alloc.
            let demanded_rids_build: Option<HashSet<&RealizationNodeId>> =
                demand_seed_build.as_ref().map(|seed| {
                    seed.iter()
                        .filter_map(|n| {
                            if let NodeId::Realization(r) = n {
                                Some(r)
                            } else {
                                None
                            }
                        })
                        .collect()
                });

            // GR-034: resolve once per eval-loop entry; threaded per-iteration.
            let long_chain_threshold = crate::dispatcher::long_chain_threshold_from_env();
            for (t_idx, template) in module.templates.iter().enumerate() {
                // `named_steps` is scoped per-template so that two structures
                // that each declare `let body = …` cannot clobber each other's
                // name → handle entries.  Cross-template `GeomRef::Sub`
                // references are now supported for non-collection subs via
                // compound keys `<sub_name>.<member>` seeded below (task 3441);
                // collection-sub geometry composition remains deferred (the
                // compile-side diagnostic in `expr.rs::try_emit_cross_sub_geometry`
                // continues to fire for those call sites).
                let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
                // Task 5033 Gap #2 Gap A: by-name repr sibling of `named_steps`.
                // See `RealizationOutputs::named_step_reprs` doc for why.
                let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
                seed_cross_sub_named_steps(
                    template,
                    &module_named_steps,
                    &mut named_steps,
                    &mut self.geometry_kernels,
                    name,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    &mut diagnostics,
                    &module.templates,
                );
                // Task 4628: seed per-binding cross-`let` handles. Runs on both
                // schedulers (same as seed_cross_sub_named_steps); named_steps entries
                // are only consumed by check_constraints_post_geometry (UnifiedDag-gated),
                // so LegacyMultiPass geometry output is byte-identical.
                seed_cross_let_named_steps(
                    template,
                    &module_named_steps,
                    &mut named_steps,
                    &mut self.geometry_kernels,
                    name,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    &mut diagnostics,
                    &module.templates,
                );
                // Task 4358 ε: order this template's realizations + selector/query
                // value-cells for the build walk. Under UnifiedDag the order is
                // `run_unified_pass`'s global Kahn schedule filtered to THIS
                // template's nodes (so a curated selector cell is hydrated before
                // the realization that consumes it); any realization not covered by
                // the schedule (e.g. residue downstream of a cycle, or a node with
                // no trace entry) is appended in declaration order so every
                // realization still runs exactly as legacy would. Under
                // LegacyMultiPass the order is simply declaration order with NO
                // interleaved HydrateCell steps — byte-identical to before.
                let build_steps: Vec<BuildStep> = match unified_pass.as_ref() {
                    Some(pass) => {
                        let mut steps: Vec<BuildStep> = Vec::new();
                        let mut realized: HashSet<usize> = HashSet::new();
                        for node in &pass.schedule {
                            match node {
                                NodeId::Realization(rid) if rid.entity == template.name => {
                                    if let Some(r_idx) =
                                        template.realizations.iter().position(|r| r.id == *rid)
                                    {
                                        steps.push(BuildStep::Realize(r_idx));
                                        realized.insert(r_idx);
                                    }
                                }
                                NodeId::Value(vid) if vid.entity == template.name => {
                                    steps.push(BuildStep::HydrateCell(vid.clone()));
                                }
                                _ => {}
                            }
                        }
                        for r_idx in 0..template.realizations.len() {
                            if !realized.contains(&r_idx) {
                                // β (task 4738) step-4: demand guard mirrors the one
                                // in tessellate_from_values. Under build() full_scope
                                // is always true → demanded_rids_build=None → append
                                // all (byte-identical, step-3(b) guard). The
                                // defensive selective branch: skip hidden realizations
                                // not in the demand cone.  `demanded_rids_build` is
                                // pre-extracted above so no per-iteration clone.
                                let rid = &template.realizations[r_idx].id;
                                if demanded_rids_build.as_ref().is_none_or(|rids| rids.contains(rid)) {
                                    steps.push(BuildStep::Realize(r_idx));
                                }
                            }
                        }
                        steps
                    }
                    None => (0..template.realizations.len())
                        .map(BuildStep::Realize)
                        .collect(),
                };
                // Fail-closed region gate (task #4812, P0β): build the repr
                // snapshot once before the step loop. Reprs from bodies in PRIOR
                // templates are already in eval_state at this point. Reprs
                // written by Realize steps WITHIN this loop are NOT reflected here
                // (they are written to eval_state mid-loop), but the gate's
                // fail-open contract handles that: an absent repr skips the gate
                // and falls through to today's generic-error path. On incremental
                // builds the prior-build repr is already correct. Eliminates the
                // O(cells × realizations) per-HydrateCell rebuild.
                let realized_reprs_for_hydration =
                    Engine::realized_reprs_snapshot(&self.eval_state);
                for build_step in &build_steps {
                    let (r_idx, realization) = match build_step {
                        BuildStep::Realize(r_idx) => (*r_idx, &template.realizations[*r_idx]),
                        BuildStep::HydrateCell(cell_id) => {
                            // ε: hydrate this selector / geometry-query value cell at
                            // its scheduled slot (UnifiedDag only — Legacy emits no
                            // HydrateCell steps) so a later consuming realization
                            // (e.g. a curated fillet) reads its resolved value rather
                            // than `Undef`. Re-borrow the default kernel from the map
                            // (the per-realization execute call's `&mut` borrow has
                            // ended); the post-process block below re-runs the same
                            // passes over all cells, so this is an additive early
                            // hydration, not the sole resolution site.
                            //
                            // Robustness (reviewer): degrade to SKIPPING this early
                            // hydration rather than aborting the whole build if the
                            // default kernel is somehow absent mid-walk. The invariant
                            // holds today — `name` was `contains_key`-checked at the top
                            // of this geometry_output block — so a miss is only reachable
                            // via a future refactor that removes a kernel mid-walk, not a
                            // runtime condition; `debug_assert!` surfaces it in dev/test.
                            // Skipping is safe precisely because the hydration is additive:
                            // the whole-template post-process below re-runs the same passes
                            // over every cell, so the cell still resolves before export —
                            // only the in-loop timing is lost (a downstream curated fillet
                            // would fall back to its all-edges path, the pre-ε behaviour).
                            let Some(kernel) = self.geometry_kernels.get_mut(name) else {
                                debug_assert!(
                                    false,
                                    "default kernel must remain in the map across the schedule walk"
                                );
                                continue;
                            };
                            Engine::hydrate_value_cell_in_loop(
                                template,
                                cell_id,
                                &named_steps,
                                &mut values,
                                &self.functions,
                                &self.meta_map,
                                kernel.as_mut(),
                                &self.topology_attribute_table,
                                &realization_read_cells,
                                &realized_reprs_for_hydration,
                                &mut diagnostics,
                            );
                            continue;
                        }
                    };
                    // Task 2874, step-6 wiring: per-realization demanded
                    // tolerance for the cache-key triple `(entity_id,
                    // ReprKind::BRep, demanded_tol)`. The Vec is precomputed
                    // above the kernel borrow.
                    // Task 3227: positional lookup by [t_idx][r_idx].
                    let demanded_tol = demanded_tols
                        .get(t_idx)
                        .and_then(|v| v.get(r_idx))
                        .copied()
                        .unwrap_or(None);
                    let mut kernel_error: Option<ErrorRef> = None;
                    // Step-10 (task ε / 3436): channel for the executor's
                    // terminal produced [`ReprKind`]; written into the
                    // snapshot graph node below via disjoint-field borrows.
                    let mut produced_repr_out: Option<ReprKind> = None;
                    // T7 (task 3905): capture step_handles length before this
                    // realization so we can identify its terminal handle below.
                    let handle_start = step_handles.len();
                    // Task 4744 β step-20: feed the real morph inputs. Disjoint
                    // immutable field borrows (morph_producer / morph_source /
                    // eval_state) compose with the disjoint &mut field borrows
                    // the call takes; the eval_state borrow ends at the call's
                    // return, before the `eval_state.as_mut()` write below.
                    let morph_io = crate::morph_producer::MorphDispatchIo {
                        producer: self.morph_producer.as_deref(),
                        source: self.morph_source.get(&realization.id),
                        new_graph: self.eval_state.as_ref().map(|s| &s.snapshot.graph),
                    };
                    let morph_source_out = Engine::execute_realization_ops(
                        RealizationOpsInput::new(
                            &mut self.geometry_kernels,
                            &registry_borrowed,
                            name,
                            &realization.operations,
                            &values,
                            &self.functions,
                            &self.meta_map,
                            &mut diagnostics,
                            &realization.id,
                            realization.span,
                            &mut kernel_error,
                            &mut self.realization_cache,
                            &mut self.last_dispatch_count,
                            &mut self.last_dispatch_count_by_realization,
                        )
                        .with_realization_name(realization.name.as_deref())
                        .with_demanded_tol(demanded_tol)
                        // Task 4050 step-16 (gap 3): pass the υ-derived
                        // per-realization demanded terminal repr, positionally
                        // aligned with `demanded_tols` (`[t_idx][r_idx]`);
                        // out-of-range defaults to BRep (backward-compat).
                        .with_demanded_repr(
                            demanded_reprs
                                .get(t_idx)
                                .and_then(|v| v.get(r_idx))
                                .copied()
                                .unwrap_or(ReprKind::BRep),
                        )
                        .with_demanded_boundary(
                            boundary_demands
                                .get(t_idx)
                                .and_then(|v| v.get(r_idx))
                                .copied()
                                .unwrap_or(false),
                        )
                        // Task #3443: thread module-scope #kernel(...) pragma
                        // from the public entry point into the per-op dispatcher.
                        .with_prefer_kernel(module.kernel_pragma.as_deref())
                        .with_is_terminal_realization(r_idx + 1 == template.realizations.len())
                        // Task 4744 β step-20: feed the real morph inputs
                        // (producer + prior-tick source + new BRep graph),
                        // bound to `morph_io` just above the call.
                        .with_morph_io(morph_io)
                        // GR-034: resolved once at entry (see above).
                        .with_long_chain_threshold(long_chain_threshold),
                        RealizationOutputs::new(
                            &mut step_handles,
                            &mut named_steps,
                            &mut named_step_reprs,
                            &mut self.topology_attribute_table,
                            &mut self.swept_kind_table,
                            &mut produced_repr_out,
                        ),
                    );
                    // T7 (task 3905): record this realization's terminal handle
                    // by (t_idx, r_idx) for the Phase-B export walk.  Mirrors
                    // the tessellate_from_values Phase-A bookkeeping.
                    if step_handles.len() > handle_start {
                        terminal_handles[t_idx][r_idx] = step_handles.last().copied();
                    }
                    // Step-10 (task ε / 3436): persist the executor's terminal
                    // [`ReprKind`] into the snapshot graph node. See the
                    // `build_snapshot` mirror for the full rationale; both
                    // call sites use disjoint-field borrows of
                    // `self.geometry_kernels` vs. `self.eval_state`.
                    //
                    // Task 4248 piece-3: also write `produced_kernel` from the
                    // terminal KernelHandle already bookmarked above via
                    // `handle_start` / `terminal_handles[t_idx][r_idx]`.
                    // Independent of produced_repr_out so cache-hit realizations
                    // still record their kernel.
                    //
                    if let Some(state) = self.eval_state.as_mut()
                        && let Some(node) =
                            state.snapshot.graph.realizations.get_mut(&realization.id)
                    {
                        if let Some(repr) = produced_repr_out {
                            node.produced_repr = repr;
                        }
                        if step_handles.len() > handle_start {
                            node.produced_kernel = step_handles.last().map(|h| h.kernel);
                        }
                        // Task 4728 α: mirrors build_snapshot above. Compute
                        // inside the guard to skip when the node is absent.
                        // `self.functions`/`self.meta_map` are disjoint fields
                        // (NLL allows the borrows alongside `&mut state`/`node`).
                        // Unconditional on cache-hit vs cache-miss (INPUT hash).
                        let input_cone_hash_out = {
                            let ctx = crate::eval_ctx_with_meta(
                                &values,
                                &self.functions,
                                &self.meta_map,
                            );
                            compute_realization_upstream_values_hash(realization, &ctx)
                        };
                        node.input_cone_hash = Some(input_cone_hash_out);
                    }
                    // Arch §9.1 lines 868–877: kernel error on a realization →
                    // mark realization NodeId as Failed { error } and emit one
                    // EventKind::Failed event. The Diagnostic::error("geometry
                    // error: …") inside `execute_realization_ops` is preserved.
                    if let Some(error) = kernel_error {
                        Engine::mark_realization_failed(
                            &mut self.cache,
                            &mut self.journal,
                            &realization.id,
                            error,
                            version_id,
                        );
                    }
                    // Task 4744 β step-20: stash this realization's source bundle
                    // (returned by the executor) for the NEXT tick's morph.
                    // `store_morph_source` needs `&mut self`; safe here — the
                    // executor's disjoint field borrows have all released.
                    if let Some(src) = morph_source_out {
                        self.store_morph_source(realization.id.clone(), src);
                    }
                    // Task 4358 ε: per-realization geometry-handle hydration slice
                    // (UnifiedDag only). `post_process_geometry_handle_cells` skips
                    // realizations whose name is not yet in `named_steps`, so calling
                    // it after EACH realization hydrates only the just-completed
                    // ones — making a freshly-produced body's `values` cell visible
                    // to a selector / geometry-query cell scheduled next (the
                    // HydrateCell step above). It writes no diagnostics and re-inserts
                    // the same handle, so it is idempotent with the whole-template
                    // call in the post-process block below. Skipped under
                    // LegacyMultiPass (`unified_pass` is `None`), so that path keeps
                    // its single post-loop hydration and stays byte-identical.
                    //
                    // COST (reviewer): the helper loops over ALL of the template's
                    // realizations each call (short-circuiting those not yet in
                    // `named_steps`), so invoking it after every Realize makes the
                    // per-realization hydration O(R²)-over-realizations across a
                    // template with R realizations, vs. Legacy's single O(R) post-loop
                    // call. The re-work is purely idempotent (re-inserting already
                    // resolved handles + re-recording the same freshness cache nodes),
                    // so it is correctness-neutral, and acceptable for the typical
                    // small-R template. If profiling ever shows it dominating on a
                    // many-realization, many-handle-cell template, restrict this call
                    // to the just-completed realization (the helper would need a
                    // single-realization filter param threaded through its 3 call
                    // sites — build / build_snapshot / tessellate_from_values) rather
                    // than rescanning the full realization list each iteration.
                    if unified_pass.is_some() {
                        Engine::post_process_geometry_handle_cells(
                            template,
                            &named_steps,
                            &mut values,
                            &self.functions,
                            &self.meta_map,
                            &mut self.cache,
                            &mut self.realization_handles,
                            version_id,
                        );
                    }
                }
                // GHR-γ step-6: hydrate Type::Geometry value cells with real
                // kernel handles before any downstream post-process that might
                // read geometry-handle cells. GHR-δ: also records geometry-backed
                // Realizations as freshness-bearing cache nodes (esc-3606-37
                // ruling step 1).
                Engine::post_process_geometry_handle_cells(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    &mut self.cache,
                    &mut self.realization_handles,
                    version_id,
                );
                // Task #4726 / esc-3787-23: post-hydration re-dispatch pass.
                // Geometry lets have no value cell at eval() time, so the
                // @optimized dispatch inside eval() sees body=Undef →
                // realization_inputs EMPTY → degraded field.  Now that
                // `post_process_geometry_handle_cells` has hydrated `values`
                // with the realized GeometryHandle, re-evaluate + re-dispatch
                // any @optimized node whose args now include a GeometryHandle.
                // NOTE: must call BEFORE re-borrowing `default_kernel` from
                // `self.geometry_kernels.get_mut(name)` — the whole-`self` mutable
                // borrow for this call would otherwise conflict with that
                // field borrow.  `default_kernel` is only used by the
                // post-process helpers AFTER this call.
                self.redispatch_geometry_consuming_compute_nodes(
                    module,
                    &mut values,
                    version_id,
                    &mut diagnostics,
                );
                // Task #3787 ε: cascade re-dispatch for field-consuming nodes
                // (e.g. solve_elastic_static) that depend on the now-hydrated
                // as_printed_material field.  Runs AFTER geometry re-dispatch
                // so the material cell is non-degraded when this fires.
                self.redispatch_as_printed_consuming_compute_nodes(
                    module,
                    &mut values,
                    version_id,
                    &mut diagnostics,
                );
                // Step-8 (task ε / 3436): re-borrow the default kernel from
                // the map for post-process — see `build_snapshot` mirror.
                // Placed AFTER `redispatch_geometry_consuming_compute_nodes`
                // to avoid a conflicting whole-`self` borrow (task #4726).
                let default_kernel = self.geometry_kernels.get_mut(name).expect(
                    "default kernel must remain in the map across the per-realization loop",
                );
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in
                // `build_snapshot` and `tessellate_from_values` — keep all
                // four call sites in sync (follow-up: the broader
                // build/build_snapshot realization-loop duplication is
                // noted separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    default_kernel.as_ref(),
                    &mut diagnostics,
                );
                // Task 2531: kinematic-query post-process (interferes /
                // interferes_with / min_clearance). Mirrors the conformance-
                // query wiring; runs after `named_steps` is populated so the
                // helpers can resolve each Snapshot body's `solid` String to
                // a `GeometryHandleId`.
                Engine::post_process_kinematic_queries(
                    template,
                    &named_steps,
                    &mut values,
                    default_kernel.as_mut(),
                    &mut diagnostics,
                );
                // Rebuild here (after the step loop) so run_post_processes sees
                // reprs written by Realize steps within this template's loop.
                let realized_reprs = Engine::realized_reprs_snapshot(&self.eval_state);
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    default_kernel.as_mut(),
                    &self.topology_attribute_table,
                    &self.swept_kind_table,
                    &realized_reprs,
                    &mut diagnostics,
                    &module.templates,
                );
                // task 4222 δ: re-evaluate Undef Let cells with the live
                // containment hook so `sample(restrict(field, region), point)`
                // yields the inner value (or Undef for outside) after geometry
                // hydration. `post_process_derived_lets` (inside run_post_processes
                // above) already promoted `restricted` from Undef to
                // `Value::Field{lambda:[inner,GeometryHandle]}`, but evaluated
                // sample(restricted,...) without containment → Undef. This pass
                // re-evaluates remaining Undef Let cells with `.with_containment(self)`.
                self.post_process_containment_samples(template, &mut values);
                // Task 3441: snapshot this template's `named_steps` so a
                // later template that subs from it can seed compound-key
                // entries.  Placed AFTER the post-process queries so the
                // local `named_steps` reflects the same view the post-process
                // helpers saw (the post-process helpers do not write to
                // `named_steps`, so ordering is informational rather than
                // load-bearing — but keeping the snapshot here documents the
                // "complete snapshot" intent).  `named_steps` is moved (not
                // cloned) — it would fall out of scope at the loop body's
                // end anyway, and the post-process helpers above only
                // borrow it.
                snapshot_named_steps(template, named_steps, &mut module_named_steps);
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
            } else if !emit_geometry_output {
                // io-export δ realize-only path (`build_outputs`): realization +
                // Value::GeometryHandle hydration above is everything the
                // occurrence-driven export needs, so skip the Phase-B product
                // export entirely. This both avoids redundant serialization work
                // (the bytes would be discarded) and keeps a recording kernel's
                // `export()` capture limited to the DSL-driven per-occurrence
                // calls `build_outputs` issues itself.
                None
            } else {
                // T7 (task 3905) Phase-B export walk: collect placed-product
                // BRep handles via the containment-tree surfacing walk, then
                // export only the product (default_visible == true) bodies.
                // This replaces the old *step_handles.last() single-handle
                // export that did not honor surfacing, composed transforms, or
                // aux exclusion.
                let export_bodies = Self::collect_export_bodies_walk(
                    module,
                    &terminal_handles,
                    &mut self.geometry_kernels,
                    name,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    &mut diagnostics,
                    None, // build() collects all product bodies; no path filter needed
                );

                // Keep only product (non-aux) bodies for export.
                let product_bodies: Vec<_> = export_bodies
                    .into_iter()
                    .filter(|b| b.default_visible)
                    .collect();

                match product_bodies.len() {
                    0 => {
                        // All bodies were aux — no product geometry to export.
                        if had_realization_ops {
                            diagnostics.push(Diagnostic::error(
                                "all realized bodies are aux; no product geometry to export",
                            ));
                        }
                        None
                    }
                    1 => {
                        // Single product body — export directly (preserves
                        // single-solid STEP byte-compatibility for bracket.ri etc.).
                        // δ (task #4763): resolve the body's material color and thread
                        // it via export_with_options so the 3MF arm writes <basematerials>.
                        // include_* false → W_3MF_NO_MATERIALS never fires on this path
                        // (back-compat; DSL flags only apply on the declarative path).
                        let mut output = Vec::new();
                        let default_kernel = self
                            .geometry_kernels
                            .get(name)
                            .expect("default kernel must remain in the map for export");
                        let body_color = resolve_export_body_color(
                            &values,
                            &product_bodies[0].entity_path,
                            product_bodies[0].handle_id,
                            &mut diagnostics,
                        );
                        match default_kernel.export_with_options(
                            product_bodies[0].handle_id,
                            format,
                            &reify_ir::ExportOptions {
                                step_schema: reify_ir::StepSchema::default(),
                                color: body_color,
                                include_materials: false,
                                include_colors: false,
                            },
                            &mut output,
                        ) {
                            Ok(_warnings) => Some(output), // include_* false → warnings always empty
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                                None
                            }
                        }
                    }
                    _ => {
                        // Multiple product bodies — assemble a compound then export.
                        // v1 LIMITATION: a compound has no single per-body color; color:None.
                        let ids: Vec<GeometryHandleId> =
                            product_bodies.iter().map(|b| b.handle_id).collect();
                        let default_kernel = self
                            .geometry_kernels
                            .get_mut(name)
                            .expect("default kernel must remain in the map for compound export");
                        // On compound-assembly error, push the diagnostic and
                        // fall through with no geometry output.  The canonical
                        // BuildResult construction at the end of build() handles
                        // all remaining fields — avoids a duplicate struct literal
                        // that would silently drift on future field additions
                        // (reviewer_comprehensive / robustness suggestion).
                        match default_kernel.make_compound(&ids) {
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(format!(
                                    "compound assembly error: {}",
                                    e
                                )));
                                None
                            }
                            Ok(compound) => {
                                let mut output = Vec::new();
                                let default_kernel = self
                                    .geometry_kernels
                                    .get(name)
                                    .expect("default kernel must remain in the map for export");
                                match default_kernel.export_with_options(
                                    compound.id,
                                    format,
                                    &reify_ir::ExportOptions {
                                        step_schema: reify_ir::StepSchema::default(),
                                        color: None, // compound: no single per-body color
                                        include_materials: false,
                                        include_colors: false,
                                    },
                                    &mut output,
                                ) {
                                    Ok(_warnings) => Some(output),
                                    Err(e) => {
                                        diagnostics.push(Diagnostic::error(format!(
                                            "export error: {}",
                                            e
                                        )));
                                        None
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            None
        };

        // Task 4355 β: assert_dag_complete gate — debug-only, zero release overhead.
        // Runs on EVERY build (geometry_output block may be skipped when no kernel
        // is registered, but the snapshot graph is always populated by check() above).
        // No-op when eval_state is None (empty module or compile-only build).
        //
        // SKIP when the unified planner detected a CYCLE: cyclic (and downstream-
        // stranded) nodes land in `unified_pass.residue` and are excluded from
        // `exec_order`, so `check_dag_complete` would flag the cycle's unavoidable
        // backward edge as a false "incomplete DAG" violation. A cycle is NOT a
        // completeness bug — the executor diagnoses it via E_EVAL_CYCLE /
        // unresolvable-Sub (appended just below). Post-γ (#4954) geometry lets carry
        // value cells, so a mutual geometry `let` cycle (`a = translate(b)`,
        // `b = rotate(a)`) now surfaces a value-cell/realization cycle here where
        // pre-γ it had no value-cell trace (geometry_sibling_realization_cycle_*).
        #[cfg(debug_assertions)]
        if let Some(state) = self.eval_state.as_ref() {
            let cycle_detected = unified_pass
                .as_ref()
                .is_some_and(|pass| !pass.residue.is_empty());
            if !cycle_detected {
                crate::dirty::assert_dag_complete_from_graph(
                    &state.snapshot.graph,
                    &module.fields,
                    &exec_order,
                );
            }
        }

        // Task 4357 δ / 4358 ε: unified build-DAG cycle contract. The planner
        // (`run_unified_pass`) was materialized up front as `unified_pass` so ε's
        // realization-loop driver could consume `pass.schedule` in Kahn order
        // (hydrating curated selector cells before their consuming realizations).
        // Here we append the SAME E_EVAL_CYCLE / E_EVAL_UNRESOLVED diagnostics at
        // the SAME point δ did, so the diagnostic vector stays byte-identical to δ
        // (the planner reads only `snapshot.graph` + `trace_map`, neither
        // structurally mutated by the realization loop, so an up-front vs.
        // here-recomputed pass yields identical diagnostics).
        //
        // `unified_pass` is `Some` iff the active scheduler is UnifiedDag AND
        // `eval_state` is present, so this is a no-op under LegacyMultiPass (the
        // default — byte-unchanged) and adds zero diagnostics on an acyclic module
        // (empty residue ⇒ zero cycle diagnostics; no auto-reaching constraint ⇒
        // zero unresolved diagnostics).
        //
        // KNOWN δ behaviour — cyclic modules carry TWO cycle reports: the legacy
        // `detect_let_cycle` (engine_eval.rs) un-coded "circular let-binding
        // dependency" string coexists with the driver's structured
        // `DiagnosticCode::EvalCycle`. De-duplicating / retiring the legacy
        // emission is deferred to ι (per δ's intentional additive wiring); ε does
        // not touch it.
        if let Some(pass) = unified_pass {
            diagnostics.extend(pass.diagnostics);
        }

        // Task 4229: re-check geometry-derived constraints after the realization
        // loop. Constraints that reference geometry-derived `let` cells — e.g.
        // `Rigid`'s positive-definiteness constraint on
        // `moi_principal = eigenvalues(moment_of_inertia(geometry, …))` — cannot
        // resolve during the `check()` above: the geometry kernel is only invoked
        // by the realization loop, so those cells are still `Undef` at the initial
        // constraint-check time and the constraint comes out `Indeterminate`
        // ("undefined inputs"). Now that the realization loop has patched the
        // geometry-derived cells into `values`, re-evaluate the active constraints
        // against the completed value map and adopt any verdict that resolved from
        // `Indeterminate` → `Satisfied`/`Violated`. A previously
        // `Satisfied`/`Violated` constraint cannot regress here, because the
        // re-check only ADDS now-resolved geometry cells (no prior value changes),
        // so we deliberately only touch entries that were `Indeterminate`.
        let mut constraint_results = check_result.constraint_results;
        if constraint_results
            .iter()
            .any(|e| e.satisfaction == reify_ir::Satisfaction::Indeterminate)
        {
            let determinacy = self.eval_state.as_ref().map(|s| &s.snapshot.values);
            // Task 4358 ε (step-8): under UnifiedDag, supersede the kernel-less
            // 4229 re-check SOURCE with the post-geometry Constraint executor. It
            // folds each active constraint's INLINE geometry-query leaves
            // (`bounding_box(part)` / `volume(part)` / …) against the live kernel +
            // the realization-produced `module_named_steps` BEFORE the kernel-less
            // `SimpleConstraintChecker` runs, so an inline leaf resolves to a
            // DEFINITE verdict (un-freezing "C7") instead of staying
            // `Indeterminate`. The downstream merge loop (which only upgrades
            // `Indeterminate` entries and drops the matching stale "undefined
            // inputs" warning) is reused verbatim. LegacyMultiPass — and the
            // no-default-kernel path — keep the original kernel-less re-check
            // (the executor defers to it when no kernel exists), so `reify check`
            // and the default build path stay byte-unchanged.
            // check_constraints_post_geometry returns a 3-tuple:
            // (constraint_results, labeled_constraint_diags, dfm_build_diags).
            // `dfm_build_diags` holds DFM diagnostics (e.g. W_DFM_BUILD_VOLUME) that
            // must be added to the build unconditionally — they do NOT contain the
            // constraint label so the needle filter below cannot carry them over.
            let (recheck_results, recheck_diags, dfm_build_diags) = if self.build_scheduler
                == crate::engine_fixpoint::BuildScheduler::UnifiedDag
                && let Some(kernel_name) = default_kernel_name.as_deref()
            {
                // Task 4358 ε step-12: the auto-constraint guard's decline set.
                // Constraints whose transitive auto-read closure reaches an `auto`
                // cell are SKIPPED by the executor (δ already emits their
                // `E_EVAL_UNRESOLVED` via `unresolved_diagnostics`). Deriving the
                // skip-set from the SAME `constraints_reaching_auto` predicate δ
                // uses guarantees the decline and the diagnostic cannot diverge.
                // Empty when no `eval_state` (then the executor has nothing to skip).
                let declined = self
                    .eval_state
                    .as_ref()
                    .map(|s| {
                        crate::engine_fixpoint::constraints_reaching_auto(
                            &s.snapshot.graph,
                            &s.trace_map,
                        )
                    })
                    .unwrap_or_default();
                self.check_constraints_post_geometry(
                    module,
                    &values,
                    &module_named_steps,
                    kernel_name,
                    determinacy,
                    &declined,
                )
            } else {
                // No-kernel path: no DFM build diagnostics (DFM harvest requires
                // a resolved bounding_box from the geometry kernel).
                let (r, d) =
                    self.check_constraints_against_templates(module, &values, determinacy);
                (r, d, Vec::new())
            };
            for entry in constraint_results.iter_mut() {
                if entry.satisfaction != reify_ir::Satisfaction::Indeterminate {
                    continue;
                }
                let Some(new_sat) = recheck_results
                    .iter()
                    .find(|r| r.id == entry.id)
                    .map(|r| r.satisfaction)
                else {
                    continue;
                };
                if new_sat == reify_ir::Satisfaction::Indeterminate {
                    continue;
                }
                // Match the stale/fresh constraint diagnostics by the same needle
                // the checker embeds: the constraint label when present (the id is
                // rewritten to the label by `labeled_diagnostics`), else the raw id.
                let needle = entry.label.clone().unwrap_or_else(|| entry.id.to_string());
                // Drop the stale "indeterminate: undefined inputs" warning emitted
                // by the first `check()` for this constraint.
                diagnostics.retain(|d| {
                    !(d.code == Some(reify_core::DiagnosticCode::ConstraintIndeterminate)
                        && d.message.contains(&needle))
                });
                // Carry over any fresh non-indeterminate diagnostic the re-check
                // produced for this constraint (e.g. a `ConstraintViolated` error
                // when an indefinite override fails positive-definiteness).
                for d in &recheck_diags {
                    if d.code != Some(reify_core::DiagnosticCode::ConstraintIndeterminate)
                        && d.message.contains(&needle)
                    {
                        diagnostics.push(d.clone());
                    }
                }
                entry.satisfaction = new_sat;
            }
            // A2 (task #4734): add DFM build-level diagnostics from the
            // post-geometry constraint harvest unconditionally. These are
            // W/E/I_DFM_BUILD_VOLUME diagnostics emitted by fits_build_volume
            // predicates in violated geometry-backed constraints (e.g. OversizedPart).
            // They do NOT contain the constraint label, so the needle filter above
            // cannot carry them — they live in `dfm_build_diags` instead.
            diagnostics.extend(dfm_build_diags);
        }

        BuildResult {
            values,
            constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Thin convenience wrapper over [`Self::build_outputs_with_result`] that
    /// returns ONLY the per-occurrence artifacts, discarding the bundled
    /// constraint results + diagnostics from the driver's single realization.
    ///
    /// Prefer [`Self::build_outputs_with_result`] when you ALSO need the
    /// exit-code signal (constraint results / diagnostics) without realizing the
    /// module a second time — that is exactly what the declarative `reify build`
    /// (no `-o`) path needs, so it must not pay for two realizations.
    pub fn build_outputs(
        &mut self,
        module: &CompiledModule,
        design_dir: &std::path::Path,
        out_dir_override: Option<&std::path::Path>,
    ) -> Vec<crate::ExportArtifact> {
        self.build_outputs_with_result(module, design_dir, out_dir_override)
            .artifacts
    }

    /// Occurrence-driven export driver (io-export δ, step-8): realize the module
    /// once, then emit one file [`crate::ExportArtifact`] per realized `Output`
    /// occurrence whose `format` and `path` come from the DSL.
    ///
    /// PRD: `docs/prds/v0_6/io-export-import-completion.md` §4.3/§7.3 (signals
    /// B5/B6/B7). Unlike the imperative [`Self::build`] (one output, format from
    /// a CLI flag), the *DSL* drives both the serializer (`STLOutput` →
    /// `ExportFormat::Stl`, `STEPOutput` → `Step`, …) and the destination path.
    ///
    /// Pipeline:
    /// 1. Reuse [`Self::build`] (with `ExportFormat::Step`) to realize geometry,
    ///    hydrate `Value::GeometryHandle` cells, populate `realization_handles`,
    ///    and run constraints. Its serialized `geometry_output` is discarded —
    ///    export is driven by the recognized occurrences below, not that format.
    /// 2. Walk `module.templates × sub_components` in declaration order. Each
    ///    `sub`'s occurrence template is resolved module-first, then via the
    ///    stdlib prelude ([`crate::engine_eval::find_template_with_prelude`]) —
    ///    stdlib `Output` templates (`STLOutput` et al.) live in the prelude, not
    ///    `CompiledModule::templates`. An occurrence is an `Output` iff it is an
    ///    `EntityKind::Occurrence` AND its trait bounds transitively conform to
    ///    `Output` (trait-bound conformance, not a name match, so user-defined
    ///    Output occurrences work too).
    /// 3. Read the per-instance export spec (`format`/`path`/`resolution`) off
    ///    the elaborated `Value::StructureInstance` at `ValueCellId(template,
    ///    sub)` via [`crate::tolerance_combine::extract_output_export_spec`].
    /// 4. Resolve `subject` → live kernel handle via the sub's `subject` ARG (a
    ///    `ValueRef` into the post-build hydrated values map).
    /// 5. Resolve the destination path (design-relative / `--out-dir` override)
    ///    via [`resolve_artifact_path`].
    /// 6. Emit the file via the default kernel's `export()`.
    ///
    /// Emits one artifact per recognized `Output` occurrence, in deterministic
    /// declaration order (`templates × sub_components`) — so a multi-output
    /// module produces a reproducible artifact sequence (B6).
    ///
    /// Returns a [`crate::BuildOutputs`] bundling those artifacts with the
    /// constraint results + diagnostics from the SINGLE realization in step 1,
    /// so a caller needing the exit-code signal reuses this one realization
    /// rather than calling [`Self::build`] (which would realize, constraint-check,
    /// and serialize the discarded Phase-B product bodies all over again).
    pub fn build_outputs_with_result(
        &mut self,
        module: &CompiledModule,
        design_dir: &std::path::Path,
        out_dir_override: Option<&std::path::Path>,
    ) -> crate::BuildOutputs {
        use crate::tolerance_combine::{
            OutputTarget, conforms_to_output, extract_output_export_spec,
        };

        // (1) Realize + hydrate Value::GeometryHandle cells by reusing the build
        //     worker with the Phase-B product export DISABLED: `build_outputs`
        //     drives its own per-occurrence export below, so the imperative
        //     single-output serialization would be redundant (and, under a
        //     recording kernel, a spurious extra `export()` call). The `format`
        //     argument is irrelevant when `emit_geometry_output == false`.
        let r = self.build_with_geometry_output(module, ExportFormat::Step, false);

        // Merge module trait defs with the prelude's: the `trait Output : Sink`
        // lattice lives in the prelude std.io module, and `module.trait_defs` is
        // empty for user modules. Built once; supports transitive user-defined
        // Output occurrences (`occurrence def Foo : MyExport`, `trait MyExport :
        // Output`). The direct `["Output"]` bound greens even without the merge.
        let mut merged_trait_defs: Vec<reify_compiler::CompiledTrait> = module.trait_defs.clone();
        for pm in self.prelude {
            merged_trait_defs.extend(pm.trait_defs.iter().cloned());
        }

        let default_kernel_name = self.default_kernel_name.clone();
        let mut artifacts: Vec<crate::ExportArtifact> = Vec::new();

        // (2) Deterministic declaration-order walk of every occurrence sub:
        //     emit one artifact per recognized Output occurrence (step-10).
        for template in &module.templates {
            for sub in &template.sub_components {
                // Resolve the occurrence template — module first, then prelude.
                let Some(occ_template) = crate::engine_eval::find_template_with_prelude(
                    module,
                    self.prelude,
                    &sub.structure_name,
                ) else {
                    continue;
                };
                // Gate: Output == an `occurrence def … : Output` (trait-bound
                // conformance, not a type-name match).
                if occ_template.entity_kind != reify_compiler::EntityKind::Occurrence {
                    continue;
                }
                if !conforms_to_output(&occ_template.trait_bounds, &merged_trait_defs) {
                    continue;
                }

                // (3) Read the per-instance export spec off the elaborated
                //     StructureInstance at ValueCellId(template, sub).
                let instance_id = reify_core::ValueCellId::new(&template.name, &sub.name);
                let Some(instance) = r.values.get(&instance_id) else {
                    continue;
                };
                let Some(spec) = extract_output_export_spec(instance) else {
                    continue;
                };
                // File targets serialize below; a DisplayOutput conforms to
                // Output but its file emission is DEFERRED (the viewport drive is
                // a sibling PRD). Rather than a silent skip, surface an
                // info-severity I_DISPLAY_OUTPUT_DEFERRED diagnostic so the user
                // learns the occurrence was recognized and intentionally
                // deferred (step-12). It is carried as a zero-byte "skipped
                // entry" (the step-14 placement choice): `bytes` is empty so the
                // CLI writes no file and `path` is empty (a viewport sink has no
                // destination); `format` is an unread placeholder because
                // `ExportFormat` has no `Display` variant. Consumers MUST gate
                // file-writing on `!bytes.is_empty()`, never on `format`.
                let export_format = match spec.format {
                    OutputTarget::File(f) => f,
                    OutputTarget::DisplayDeferred => {
                        artifacts.push(crate::ExportArtifact {
                            path: std::path::PathBuf::new(),
                            format: ExportFormat::Step,
                            bytes: Vec::new(),
                            diagnostics: vec![Diagnostic::info(format!(
                                "{}: DisplayOutput occurrence `{}.{}` recognized; \
                                 file emission deferred (the viewport drive is a \
                                 deferred sibling PRD)",
                                crate::I_DISPLAY_OUTPUT_DEFERRED,
                                template.name,
                                sub.name
                            ))],
                        });
                        continue;
                    }
                };

                // (5) Resolve the destination (design-relative / --out-dir) up
                //     front so any failure diagnostic below can name the path.
                let path = resolve_artifact_path(&spec.path, design_dir, out_dir_override);

                // (4) Resolve `subject` → live kernel handle via the sub's
                //     `subject` ARG: a ValueRef into the post-build hydrated map
                //     (NOT the pre-hydration StructureInstance.subject field).
                //
                // Per-occurrence failure isolation (step-14): a recognized
                // Output occurrence whose `subject` cannot be resolved to live
                // geometry — or whose kernel export() fails below — must NOT
                // abort the loop. It pushes a "partial" artifact (empty bytes
                // carrying an error-severity diagnostic that names the occurrence
                // + path) and `continue`s, so one bad Output never aborts the
                // others (PRD §4.3/§7.3). The CLI gates file-writing on
                // `!bytes.is_empty()`, so a partial artifact writes no file.
                let subject_handle = sub
                    .args
                    .iter()
                    .find_map(|(k, e)| (k.as_str() == "subject").then_some(e))
                    .and_then(|e| match &e.kind {
                        reify_ir::CompiledExprKind::ValueRef(id) => r.values.get(id),
                        _ => None,
                    })
                    .and_then(|v| match v {
                        reify_ir::Value::GeometryHandle { kernel_handle, .. } => *kernel_handle,
                        _ => None,
                    });
                let Some(handle_id) = subject_handle else {
                    artifacts.push(crate::ExportArtifact {
                        path: path.clone(),
                        format: export_format,
                        bytes: Vec::new(),
                        diagnostics: vec![Diagnostic::error(format!(
                            "Output occurrence `{}.{}` could not resolve its \
                             `subject` to realized geometry (export to {} skipped)",
                            template.name,
                            sub.name,
                            path.display()
                        ))],
                    });
                    continue;
                };

                // (6) Emit one file via the default kernel's export_with_options(); isolate
                //     a kernel failure as an error diagnostic + continue.
                //
                // (6a) Resolve per-body color (δ, task #4763 B7/B8): scan values for a
                //      Physical body whose geometry handle matches `handle_id` and has a
                //      material; resolve its appearance color. None for geometry with no
                //      owning material-bearing body (colorless export path).
                let include_colors = if let reify_ir::Value::StructureInstance(data) = instance {
                    match data.fields.get("include_colors") {
                        Some(reify_ir::Value::Bool(b)) => *b,
                        _ => true, // DSL default: include_colors = true
                    }
                } else {
                    true
                };
                let include_materials =
                    if let reify_ir::Value::StructureInstance(data) = instance {
                        match data.fields.get("include_materials") {
                            Some(reify_ir::Value::Bool(b)) => *b,
                            _ => true, // DSL default: include_materials = true
                        }
                    } else {
                        true
                    };
                let mut color_diags: Vec<Diagnostic> = Vec::new();
                // Declarative ThreeMFOutput path (#4287): the `subject` resolves to a
                // real `GeometryHandle`, so association is by handle equality — pass an
                // empty `entity_path` so the entity-path primary match is a no-op and
                // resolution falls through to the handle-equality scan.
                let body_color =
                    resolve_export_body_color(&r.values, "", handle_id, &mut color_diags);

                let mut bytes = Vec::new();
                let export_result = match default_kernel_name
                    .as_deref()
                    .and_then(|name| self.geometry_kernels.get(name))
                {
                    Some(kernel) => kernel.export_with_options(
                        handle_id,
                        export_format,
                        &reify_ir::ExportOptions {
                            step_schema: spec.step_schema,
                            color: body_color,
                            include_materials,
                            include_colors,
                        },
                        &mut bytes,
                    ),
                    None => Err(reify_ir::ExportError::FormatError(
                        "no default geometry kernel registered".to_string(),
                    )),
                };
                let warnings = match export_result {
                    Ok(warnings) => warnings,
                    Err(e) => {
                        artifacts.push(crate::ExportArtifact {
                            path: path.clone(),
                            format: export_format,
                            bytes: Vec::new(),
                            diagnostics: vec![Diagnostic::error(format!(
                                "Output occurrence `{}.{}` failed to export to {}: {}",
                                template.name,
                                sub.name,
                                path.display(),
                                e
                            ))],
                        });
                        continue;
                    }
                };

                // Translate each kernel-neutral ExportWarning into a user-facing
                // warning diagnostic (honest AP242→AP214 degradation, PRD §4.4).
                // The bytes were written successfully — a fallback is a warning,
                // not a failure — so they survive on the artifact alongside the
                // diagnostic. color_diags (unknown-name warnings from
                // resolve_export_body_color) are appended after.
                let mut diagnostics: Vec<Diagnostic> = warnings
                    .into_iter()
                    .map(|w| match w {
                        reify_ir::ExportWarning::StepAp242Fallback => Diagnostic::warning(format!(
                            "{}: STEPOutput occurrence `{}.{}` requested AP242 but the \
                                 linked OCCT rejected it; wrote AP214 instead",
                            crate::W_STEP_AP242_FALLBACK,
                            template.name,
                            sub.name
                        )),
                        reify_ir::ExportWarning::ThreeMfNoMaterials => Diagnostic::warning(format!(
                            "{}: ThreeMFOutput occurrence `{}.{}` requested material data \
                                 but no color was resolved for the exported body — geometry \
                                 written, materials omitted",
                            crate::W_3MF_NO_MATERIALS,
                            template.name,
                            sub.name
                        )),
                    })
                    .collect();
                diagnostics.extend(color_diags);

                artifacts.push(crate::ExportArtifact {
                    path,
                    format: export_format,
                    bytes,
                    diagnostics,
                });
            }
        }

        // Bundle the artifacts with the single realization's constraint results +
        // diagnostics so the CLI exit-code gate reuses THIS realization instead of
        // calling build() a second time (the `r` fields are moved out — the loop's
        // immutable borrows of `r.values` have all ended by here).
        crate::BuildOutputs {
            constraint_results: r.constraint_results,
            diagnostics: r.diagnostics,
            artifacts,
        }
    }

    /// T7 (task 3905): compute the minimum distance (SI metres) between two
    /// placed product bodies identified by their composed `entity_path` strings
    /// (e.g. `"Assembly.a#realization[0]"`).
    ///
    /// Runs the same Phase-A realization execution and Phase-B
    /// `surface_export_bodies` walk as [`Self::build`], resolves the two placed
    /// handles by `entity_path` (product bodies only: `default_visible == true`),
    /// and issues `GeometryQuery::Distance{from, to}` via the default kernel.
    ///
    /// Returns `Some(d)` where `d` is the BRepExtrema minimum distance in metres,
    /// or `None` if either path is unresolvable, no geometry kernel is configured,
    /// or the distance query fails (with a warning diagnostic). Consistent with the
    /// `kernel_distance` error-handling convention.
    ///
    /// Uses the engine's `RealizationCache` — if `build()` was called first on the
    /// same module the Phase-A kernel ops are served from cache and this method
    /// incurs only the surfacing + Distance query overhead.
    pub fn distance_between_placed(
        &mut self,
        module: &CompiledModule,
        path_a: &str,
        path_b: &str,
    ) -> Option<f64> {
        let name = self.default_kernel_name.as_deref()?;
        if !self.geometry_kernels.contains_key(name) {
            return None;
        }
        let name = name.to_owned();

        // Phase-A: evaluate the module and execute geometry ops to populate
        // terminal_handles, mirroring the build() realization loop.
        //
        // NOTE (task-3905 amendment, suggestion 1): This loop (~130 lines below)
        // mirrors build()'s Phase-A realization loop.  A full extraction into a
        // shared collect_placed_export_bodies helper would require restructuring
        // build()'s post-processing (conformance/kinematic queries, GHR, journal
        // writes) to run AFTER all templates complete — currently the
        // post-processing is interleaved per-template using a local `named_steps`
        // that is moved into module_named_steps before the next template.  This
        // carries semantic risk for cross-template geometry references (task 3441),
        // so Phase-A extraction is deferred; any changes to the realization
        // execution or terminal_handles bookkeeping in build() must be mirrored here.
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;
        let values = check_result.values;

        let demanded_tols = self.compute_demanded_tols(module);
        let demanded_reprs = self.compute_demanded_reprs(module, ExportFormat::Step);
        let boundary_demands = self.compute_boundary_demands(module);

        #[cfg(any(test, feature = "test-instrumentation"))]
        let registry_owned = self
            .test_registry_override
            .clone()
            .unwrap_or_else(crate::kernel_registry::collect_registry);
        #[cfg(not(any(test, feature = "test-instrumentation")))]
        let registry_owned = crate::kernel_registry::collect_registry();
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry_owned.iter().map(|(k, v)| (k.clone(), v)).collect();

        let mut step_handles: Vec<KernelHandle> = Vec::new();
        let mut terminal_handles: Vec<Vec<Option<KernelHandle>>> = module
            .templates
            .iter()
            .map(|t| vec![None; t.realizations.len()])
            .collect();

        // Scratch tables required by execute_realization_ops signature;
        // not used by the distance query (no post-process conformance/kinematic
        // queries needed — only raw geometry handles are needed).
        let mut scratch_topo_attrs = TopologyAttributeTable::default();
        let mut scratch_swept_kinds = SweptKindTable::default();
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();
        // GR-034: resolve once per eval-loop entry; threaded per-iteration.
        let long_chain_threshold = crate::dispatcher::long_chain_threshold_from_env();

        for (t_idx, template) in module.templates.iter().enumerate() {
            let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
            // Task 5033 Gap #2 Gap A: by-name repr sibling of `named_steps`.
            // See `RealizationOutputs::named_step_reprs` doc for why.
            let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
            seed_cross_sub_named_steps(
                template,
                &module_named_steps,
                &mut named_steps,
                &mut self.geometry_kernels,
                &name,
                &values,
                &self.functions,
                &self.meta_map,
                &mut diagnostics,
                &module.templates,
            );
            for (r_idx, realization) in template.realizations.iter().enumerate() {
                let demanded_tol = demanded_tols
                    .get(t_idx)
                    .and_then(|v| v.get(r_idx))
                    .copied()
                    .unwrap_or(None);
                let mut kernel_error: Option<ErrorRef> = None;
                let mut produced_repr_out: Option<ReprKind> = None;
                let handle_start = step_handles.len();
                Engine::execute_realization_ops(
                    RealizationOpsInput::new(
                        &mut self.geometry_kernels,
                        &registry_borrowed,
                        &name,
                        &realization.operations,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        &mut diagnostics,
                        &realization.id,
                        realization.span,
                        &mut kernel_error,
                        &mut self.realization_cache,
                        &mut self.last_dispatch_count,
                        &mut self.last_dispatch_count_by_realization,
                    )
                    .with_realization_name(realization.name.as_deref())
                    .with_demanded_tol(demanded_tol)
                    .with_demanded_repr(
                        demanded_reprs
                            .get(t_idx)
                            .and_then(|v| v.get(r_idx))
                            .copied()
                            .unwrap_or(ReprKind::BRep),
                    )
                    .with_demanded_boundary(
                        boundary_demands
                            .get(t_idx)
                            .and_then(|v| v.get(r_idx))
                            .copied()
                            .unwrap_or(false),
                    )
                    // Task #3443: the distance query path is outside the
                    // user's design pragma scope — pass None (lex-min default).
                    .with_prefer_kernel(None)
                    .with_is_terminal_realization(r_idx + 1 == template.realizations.len())
                    // Task 4744 β step-16: distance query never demands
                    // VolumeMesh, so the morph arm never fires here.
                    .with_morph_io(crate::morph_producer::MorphDispatchIo::disabled())
                    // GR-034: resolved once at entry (see above).
                    .with_long_chain_threshold(long_chain_threshold),
                    RealizationOutputs::new(
                        &mut step_handles,
                        &mut named_steps,
                        &mut named_step_reprs,
                        &mut scratch_topo_attrs,
                        &mut scratch_swept_kinds,
                        &mut produced_repr_out,
                    ),
                );
                if step_handles.len() > handle_start {
                    terminal_handles[t_idx][r_idx] = step_handles.last().copied();
                }
                // Kernel errors are recorded in diagnostics by execute_realization_ops;
                // the distance query will simply find no handle for failed realizations.
                let _ = kernel_error;
            }
            snapshot_named_steps(template, named_steps, &mut module_named_steps);
        }

        // Phase-B: collect placed product handles via the T7 surfacing walk.
        //
        // Short-circuit (T7 amendment, suggestion 3): pass the two target paths as
        // `path_filter` so `collect_export_bodies_walk` → `surface_export_bodies` →
        // `walk_placed_realizations` only calls `ApplyTransform` for realizations
        // at `path_a` or `path_b`.  All other bodies skip the kernel call entirely,
        // preventing transient handle accumulation on repeated distance queries over
        // the same module.
        let export_bodies = Self::collect_export_bodies_walk(
            module,
            &terminal_handles,
            &mut self.geometry_kernels,
            &name,
            &values,
            &self.functions,
            &self.meta_map,
            &mut diagnostics,
            Some((path_a, path_b)),
        );

        // Resolve the two product (default_visible == true) handles by entity_path.
        let find_handle = |path: &str| -> Option<GeometryHandleId> {
            export_bodies
                .iter()
                .find(|b| b.default_visible && b.entity_path == path)
                .map(|b| b.handle_id)
        };
        let handle_a = find_handle(path_a);
        let handle_b = find_handle(path_b);
        let (Some(from), Some(to)) = (handle_a, handle_b) else {
            diagnostics.push(Diagnostic::warning(format!(
                "distance_between_placed: could not resolve product handle(s) \
                 (path_a={path_a:?} → {handle_a:?}, path_b={path_b:?} → {handle_b:?})"
            )));
            return None;
        };

        // Issue GeometryQuery::Distance on the placed handles.
        let kernel = self
            .geometry_kernels
            .get(name.as_str())
            .expect("default kernel must remain in the map");
        crate::geometry_ops::kernel_distance(
            kernel.as_ref(),
            from,
            to,
            &mut diagnostics,
            "distance_between_placed",
        )
    }

    /// Phase-B helper: run the root + fallback `surface_export_bodies` walk and
    /// return all collected `ExportBody` entries.
    ///
    /// Factored out of both `build()` and `distance_between_placed()` to eliminate
    /// the ~50-line duplicated root/fallback traversal pattern.  Phase-A realization
    /// execution remains per-caller due to differing post-processing requirements:
    /// `build()` populates engine state (conformance/kinematic queries, GHR, journal)
    /// interleaved within the template loop; `distance_between_placed()` skips it.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn collect_export_bodies_walk(
        module: &CompiledModule,
        terminal_handles: &[Vec<Option<KernelHandle>>],
        geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
        name: &str,
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
        // T7 amendment (suggestion 3): when `Some((path_a, path_b))`, the export
        // walk short-circuits ApplyTransform for every entity path that does NOT match
        // either target — avoiding transient-handle accumulation on repeated
        // `distance_between_placed` calls.  Pass `None` for the full-collection
        // `build()` path (all product bodies are needed for the STEP compound).
        path_filter: Option<(&str, &str)>,
    ) -> Vec<crate::geometry_ops::ExportBody> {
        use crate::geometry_ops::{
            compose_pose_chain, non_final_realization_indices, reachable_template_indices,
            root_template_indices, surface_export_bodies,
        };
        let identity_world = compose_pose_chain(&[]);
        let roots = root_template_indices(module);
        // T7 robustness fix (esc-3905-277): a template's non-final realizations
        // are redundant intermediate lets whose geometry is inlined into the
        // final realization (the compiler inlines boolean/etc. operands rather
        // than cross-referencing them), so they must NOT be exported as
        // standalone solids. Restores the pre-T7 "final realization per template"
        // export semantics while keeping T7's multi-body-via-subs behavior.
        let skip = non_final_realization_indices(module, terminal_handles);
        // Construct the path-level pre-filter for the distance case.
        // Box the closure so `pre_filter` can hold a stable reference that outlives
        // the `.map()` call site.  `as_deref()` converts `Option<Box<dyn Fn(...)>>`
        // to `Option<&dyn Fn(...)>` by borrowing from the box for its lifetime.
        let boxed_filter: Option<Box<dyn Fn(usize, usize, &str) -> bool>> =
            path_filter.map(|(pa, pb)| {
                let pa = pa.to_owned();
                let pb = pb.to_owned();
                let f: Box<dyn Fn(usize, usize, &str) -> bool> =
                    Box::new(move |_t: usize, _r: usize, path: &str| path == pa || path == pb);
                f
            });
        let pre_filter: Option<&dyn Fn(usize, usize, &str) -> bool> = boxed_filter.as_deref();
        let mut export_bodies = Vec::new();
        for &root_idx in &roots {
            let root_prefix = module.templates[root_idx].name.clone();
            surface_export_bodies(
                module,
                root_idx,
                &root_prefix,
                false,
                &identity_world,
                0,
                terminal_handles,
                geometry_kernels,
                name,
                values,
                functions,
                meta_map,
                &skip,
                pre_filter,
                &mut export_bodies,
                diagnostics,
            );
        }
        // Fallback: surface any template unreachable from roots
        // (cycle/orphan guard — mirrors tessellate_from_values).
        let mut covered = reachable_template_indices(module, &roots);
        for t_idx in 0..module.templates.len() {
            if covered.contains(&t_idx) {
                continue;
            }
            let fallback_prefix = module.templates[t_idx].name.clone();
            surface_export_bodies(
                module,
                t_idx,
                &fallback_prefix,
                false,
                &identity_world,
                0,
                terminal_handles,
                geometry_kernels,
                name,
                values,
                functions,
                meta_map,
                &skip,
                pre_filter,
                &mut export_bodies,
                diagnostics,
            );
            covered.extend(reachable_template_indices(module, &[t_idx]));
        }
        export_bodies
    }

    /// β (task 4738): Compute the demand-scoped unified build-DAG plan.
    ///
    /// Returns `(unified_pass, demand_seed)` where:
    ///
    /// - LegacyMultiPass or no eval_state → `(None, None)`: declaration-order
    ///   fallback (byte-identical to the pre-θ path).
    /// - UnifiedDag + `full_scope` (cold path, set by `eval()`/`check()`) →
    ///   `(Some(run_unified_pass(...)), None)`: full schedule + diagnostics
    ///   preserved; `demand_seed=None` keeps the `build_steps` fallback
    ///   appending all uncovered realizations — byte-identical to pre-β.
    /// - UnifiedDag + selective (warm path, `full_scope` OFF) → the demanded
    ///   backward-closure nodes drive `run_unified_pass_seeded`; `demand_seed`
    ///   is the cone so the `build_steps` fallback skips hidden realizations.
    ///
    /// All three build/tessellate sites call this helper (step-2 wires the two
    /// tessellate sites; step-4 wires `build_with_geometry_output`) so the
    /// demand-seam is single and future changes touch one place.
    /// Compute the demand-scoped unified pass schedule and seed.
    ///
    /// `hash_exempt`: realizations whose input-cone hash is unchanged (returned
    /// by `refresh_and_gate_demanded_realizations`). On the selective path these
    /// are excluded from the seed so `tessellate_from_values` skips them and
    /// avoids redundant re-dispatch. Empty on the full-scope / cold path.
    fn demand_scoped_unified_pass(
        &self,
        hash_exempt: &HashSet<NodeId>,
    ) -> (
        Option<crate::engine_fixpoint::UnifiedPassResult>,
        Option<HashSet<NodeId>>,
    ) {
        if self.build_scheduler != crate::engine_fixpoint::BuildScheduler::UnifiedDag {
            return (None, None);
        }
        let Some(state) = self.eval_state.as_ref() else {
            return (None, None);
        };
        if self.demand.is_full_scope() {
            // Cold / full-scope path (set by eval()/check()): full schedule +
            // diagnostics (E_EVAL_CYCLE / E_EVAL_UNRESOLVED) preserved.
            // hash_exempt is always empty here (refresh_and_gate is a no-op
            // under full_scope), so ignoring it is safe.
            let pass = crate::engine_fixpoint::run_unified_pass(
                &state.snapshot.graph,
                &state.trace_map,
            );
            (Some(pass), None)
        } else {
            // Selective (warm) path: seed = backward closure of demanded
            // realizations. trace_map.keys() are all nodes the planner knows
            // about; filter to those demanded. When all bodies are visible
            // (all-visible set_demand_selective), every node is demanded →
            // seed = whole trace map → same schedule as the full pass →
            // byte-identical. When a body is hidden, its exclusive nodes are
            // absent from the seed and excluded from the schedule.
            //
            // δ (task 4740) step-6: also exclude `hash_exempt` realizations —
            // those whose input-cone hash is unchanged (populated by
            // refresh_and_gate_demanded_realizations). Excluding them means
            // tessellate_from_values skips their kernel ops, achieving "reuse"
            // without requiring a populated realization cache.
            //
            // DELTA CONTRACT: realizations absent from the scheduled seed are
            // also absent from `TessellateResult.meshes` because the uncovered-
            // realization fallback at engine_build.rs:5460 guards on
            // `demanded_rids.contains(rid)` — and `demanded_rids` is derived
            // from this seed.  This absence has TWO distinct semantics:
            //
            //   • HIDDEN body (not in demand cone): "remove this body from view"
            //   • HASH-EXEMPT body (demanded + inputs unchanged): "keep the
            //     previous mesh — no geometry change occurred"
            //
            // The consumer (GUI/Tauri) MUST treat a `TessellateResult` as an
            // INCREMENTAL DELTA, not a full snapshot: an absent mesh means
            // "retain the previously rendered mesh," not "remove this body."
            // Conflating the two absence reasons would cause an unchanged visible
            // body to silently vanish on a no-edit re-tessellate.
            //
            // The reuse-branch guard test
            // `redemand_body_b_no_edit_reuses_cached_geometry_hash_gate` documents
            // and pins this delta contract: it asserts that body_b's geometry is
            // reused (dispatch_count == 0, sb unchanged) after a no-edit un-hide,
            // even though body_b is absent from _tess3.meshes.  Reuse is achieved
            // via hash-exempt seed exclusion (body_b excluded from demand_seed_snap
            // → not in demanded_rids → execute_realization_ops not called), not
            // via a realization_cache hit (the cache is only populated when
            // demanded_tol = Some(...), which requires a RepresentationWithin
            // constraint or active purpose binding absent from that fixture).
            let seed: HashSet<NodeId> = state
                .trace_map
                .keys()
                .filter(|n| self.demand.is_demanded(n) && !hash_exempt.contains(*n))
                .cloned()
                .collect();
            let schedule =
                crate::engine_fixpoint::run_unified_pass_seeded(&state.trace_map, &seed);
            let pass = crate::engine_fixpoint::UnifiedPassResult {
                schedule,
                residue: HashSet::new(),
                diagnostics: Vec::new(),
            };
            (Some(pass), Some(seed))
        }
    }

    /// Returns the union of all realization traces' read [`reify_core::ValueCellId`]s.
    ///
    /// Used by [`hydrate_value_cell_in_loop`] for eager selector resolution at
    /// scheduled `HydrateCell` steps.  **Not** restricted to the demand cone:
    /// cells shared between demanded and hidden realizations must still be
    /// available for hydration.
    ///
    /// Returns an empty set when `eval_state` is absent (pre-eval or
    /// `LegacyMultiPass` with no eval state).  Callers gate on
    /// `unified_pass.is_some()` before calling to preserve the "empty under
    /// LegacyMultiPass" invariant while sharing the iteration.
    fn realization_read_cells(&self) -> HashSet<reify_core::ValueCellId> {
        let Some(state) = self.eval_state.as_ref() else {
            return HashSet::new();
        };
        state
            .trace_map
            .iter()
            .filter(|(node, _)| matches!(node, NodeId::Realization(_)))
            .flat_map(|(_, tr)| tr.reads.iter().cloned())
            .collect()
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
    ///
    /// # Tolerance wiring (task 2874)
    ///
    /// `tessellate_realizations` mirrors [`Self::build`] across all four
    /// production-wiring contracts — see that method's docstring for the
    /// full description (task 2874). Task 3103 consolidated the helper
    /// placement: all four surfaces (build / build_snapshot /
    /// tessellate_realizations / tessellate_snapshot) now emit diagnostics
    /// and compute demanded tolerances AFTER their respective constraint check.
    /// The snapshot variant [`Self::tessellate_snapshot`] was already
    /// post-check; the non-snapshot surfaces gained the same placement once
    /// eval() preserves `active_purpose_bindings` across the call. The
    /// integration smoke
    /// `end_to_end_tolerance_wiring_threads_promise_diagnostic_cache_and_per_stage_budget`
    /// in `crates/reify-eval/tests/tolerance_wiring_e2e.rs` pins all four
    /// axes (diagnostic emission, demanded-tolerance routing,
    /// per-stage budget, RealizationCache population) on this surface
    /// simultaneously. The single difference vs. `build`: this surface
    /// applies the budget at the `kernel.tessellate(handle, budget)` call
    /// site (the per-output budgeted tolerance directly drives the
    /// tessellation precision), whereas `build` applies it at the
    /// realization-cache key.
    pub fn tessellate_realizations(&mut self, module: &CompiledModule) -> TessellateResult {
        // Task ε (3436) step-12: reset the dispatch-count instrumentation
        // counter at the entry to every build/tessellate surface so a second
        // call against the same module reports its own per-build dispatch
        // tally (and reports 0 when fully served from the RealizationCache).
        // Mirrors `build` / `build_snapshot` / `tessellate_snapshot`.
        // Zeroes BOTH the aggregate and the per-realization tally in lockstep.
        self.reset_dispatch_tallies();
        // PLACEMENT: AFTER check() — task 3103 consolidated the lifecycle so
        // eval() preserves active_purpose_bindings across the call, making the
        // pre-check workaround obsolete. All four surfaces (build /
        // build_snapshot / tessellate_realizations / tessellate_snapshot) now
        // share the post-check placement. See engine_eval.rs for the
        // preservation site (task 3103).
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        // Task 2874: emit imported-tolerance-promise diagnostics AFTER
        // `self.check(module)` — eval() now preserves active_purpose_bindings
        // (task 3103), so the helper observes the preserved/re-injected scope.
        // `build_snapshot` does not call eval so it already emitted after
        // `check_constraints_against_templates`.
        self.emit_imported_tolerance_promise_diagnostics_for_module(module, &mut diagnostics);

        // Task 2874 step-6: precompute per-realization demanded tolerance
        // AFTER `self.check(module)` — eval() now preserves active_purpose_bindings
        // (task 3103), so the priority chain `demanded_tolerance_for_output →
        // active_tolerance_for` correctly reads the preserved/re-injected scope.
        // Missing keys are treated as `None` by `tessellate_from_values` callers.
        let demanded_tols = self.compute_demanded_tols(module);

        // Task 2874 step-12: precompute per-realization tessellation budget
        // AFTER `self.check(module)` for the same reason as `demanded_tols`.
        // Mirrored in `tessellate_snapshot`.
        let registry_owned = crate::kernel_registry::collect_registry();
        let tessellation_budgets =
            self.compute_tessellation_budgets(module, &demanded_tols, &registry_owned);
        // Step-8 (task ε / 3436): borrowed-view registry for per-op dispatch
        // routing — same pattern as the `build` / `build_snapshot` mirrors.
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry_owned.iter().map(|(k, v)| (k.clone(), v)).collect();
        // Task 2320 amendment: `values` is moved into a local mutable binding
        // here so `tessellate_from_values` can patch conformance-query results
        // (`is_watertight` / `is_manifold` / `is_orientable`) into the map
        // before it is moved into the returned `TessellateResult` below.
        // Keeps `TessellateResult.values` semantically aligned with
        // `BuildResult.values` — a reader of either map sees the same
        // kernel-resolved Bool answers (when a kernel is configured).
        let mut values = check_result.values;
        self.topology_attribute_table = TopologyAttributeTable::default();
        self.swept_kind_table = SweptKindTable::default();
        // Determinacy β (task 4198): clear the achieved-tol map at the start
        // of each tessellate_realizations call so stale entries from a prior
        // call do not leak into the new result.
        self.achieved_repr_tol.clear();
        // β (task 4738) step-2: demand-scoped plan for the tessellate_realizations
        // path. `demand_scoped_unified_pass()` replaces the inline
        // `run_unified_pass` call; the returned `demand_seed_tess` threads into
        // `tessellate_from_values` to guard the build_steps fallback.
        // For tessellate_realizations, `check()` above calls `eval()` which sets
        // `full_scope=true`, so this always takes the full-scope branch →
        // demand_seed_tess = None → byte-identical to pre-β.
        let (unified_pass_tess, demand_seed_tess) =
            self.demand_scoped_unified_pass(&HashSet::new());
        // realization_read_cells: union of all realization traces' reads (used by
        // hydrate_value_cell_in_loop to decide eager vs. descriptor resolution).
        // Empty under LegacyMultiPass (unified_pass_tess is None).  Delegated
        // to the shared helper to avoid duplicating the trace_map iteration.
        let realization_read_cells_tess: HashSet<reify_core::ValueCellId> =
            if unified_pass_tess.is_some() {
                self.realization_read_cells()
            } else {
                HashSet::new()
            };
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernels,
            &registry_borrowed,
            self.default_kernel_name.as_deref(),
            module,
            &mut values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
            &mut self.topology_attribute_table,
            &mut self.swept_kind_table,
            &mut self.realization_cache,
            &demanded_tols,
            &tessellation_budgets,
            &mut self.last_dispatch_count,
            &mut self.last_dispatch_count_by_realization,
            self.capture_repr_tol,
            &mut self.achieved_repr_tol,
            unified_pass_tess.as_ref(),
            &realization_read_cells_tess,
            demand_seed_tess.as_ref(),
        );

        TessellateResult {
            values,
            constraint_results: check_result.constraint_results,
            meshes,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Default tessellation tolerance in SI meters (0.1mm).
    const DEFAULT_TESSELLATION_TOLERANCE: f64 = 0.0001;

    /// Returns the tessellation tolerance to use for `module`, in SI metres.
    ///
    /// Threads the module-level `#precision` pragma value (stored on
    /// `CompiledModule::default_tolerance` by `apply_module_pragmas`) through
    /// to the kernel. Falls back to [`Self::DEFAULT_TESSELLATION_TOLERANCE`]
    /// when the pragma is absent or was malformed.
    ///
    /// **Role since task 2874 step-12**: this remains the module-pragma
    /// fallback that the per-realization budget pipeline consults when no
    /// per-output demanded tolerance exists. The active fallback chain at
    /// the `kernel.tessellate` call site is now:
    /// `demanded_tolerance_for_output(template_name, entity)` →
    /// `active_tolerance_for(entity)` → `effective_tessellation_tolerance(module)`.
    /// The first available entry feeds
    /// [`Self::compute_realization_tolerance_budget`], and the budget is
    /// what `kernel.tessellate(handle, budget)` ultimately receives.
    fn effective_tessellation_tolerance(module: &CompiledModule) -> f64 {
        module
            .default_tolerance
            .unwrap_or(Self::DEFAULT_TESSELLATION_TOLERANCE)
    }

    /// Compute the per-realization tolerance budget by routing `demanded_tol`
    /// through the dispatcher's per-stage allocation primitive.
    ///
    /// Synthesises a [`crate::dispatcher::DispatchPlan`] via
    /// [`dispatch`]`(registry, op, demanded, &available)` where the triple
    /// `(op, demanded, available)` is sourced from
    /// [`Self::BUDGET_QUERY_TRIPLE_V02`] (`(BooleanUnion, BRep, {BRep})`).
    /// On `Some(plan)` returns [`per_stage_tolerance_for_plan`]`(&plan,
    /// demanded_tol)`. On `None` (no plan: dispatcher could not find a
    /// kernel + conversion chain that satisfies the request against the
    /// supplied registry) returns `demanded_tol` unchanged — this mirrors
    /// the empty-conversion pass-through contract pinned by
    /// `dispatcher::tests::per_stage_tolerance_for_plan_empty_chain_returns_requested_tol_unchanged`,
    /// just one level up in the call stack: no plan ⇒ no budget allocation.
    ///
    /// **Why a named const for the triple**: per the task 2874 design
    /// decision the v0.2 occt-only inventory and BRep-on-BRep realization
    /// metadata baseline mean the realization-level budget query always
    /// issues `(BooleanUnion, BRep, {BRep})`. With that triple the BFS in
    /// [`dispatch`] returns at depth 0 whenever any kernel in the registry
    /// supports `(BooleanUnion, BRep)`, yielding a 0-conversion plan and
    /// `per_stage_tolerance_for_plan` passes the demand through unchanged.
    /// Multi-kernel adapters (PRD §"Resolved design decisions") will
    /// introduce richer per-realization `Operation`/`ReprKind` metadata;
    /// when that lands the call site that derives `(op, demanded, available)`
    /// from `RealizationDecl::operations.last()` becomes the new source of
    /// truth, and a single grep for `BUDGET_QUERY_TRIPLE_V02` surfaces every
    /// place the v0.2 placeholder is consumed.
    ///
    /// **Signature** (amendment 2): takes the borrowed-value
    /// `&BTreeMap<String, &CapabilityDescriptor>` map that [`dispatch`]
    /// already requires. The owned→borrowed conversion (one `String` clone
    /// per kernel-name) lives at the **single** call site
    /// [`Self::compute_tessellation_budgets`], where it runs **once per
    /// build** rather than once per realization. The earlier "owned-value
    /// at the boundary, borrow-build inside the helper" arrangement only
    /// relocated the per-call clone — for a build with `R` realizations
    /// and `K` kernels it allocated `R · K` strings; this signature keeps
    /// the cost at `K` per build regardless of `R`. Direct callers (today
    /// just the test seam) build the borrowed view themselves at the call
    /// site.
    ///
    /// **Signature** (amendment 3, task 3227): takes `available:
    /// &HashSet<ReprKind>` as a caller-supplied parameter rather than
    /// synthesising it from `BUDGET_QUERY_TRIPLE_V02.2` on every call.
    /// The slice inside the triple is `&'static [ReprKind]` so its
    /// contents are const; constructing a `HashSet` from it per-call was
    /// purely a translation artefact. The construction now lives in
    /// [`Self::compute_tessellation_budgets`] (one allocation per build,
    /// not one per realization). Direct callers (test seam) build the
    /// `HashSet` at their own call site, mirroring the amendment-2
    /// pattern for the borrowed registry view.
    ///
    /// **Production wiring** (task 2874 step-12): `tessellate_from_values`
    /// calls this indirectly through `compute_tessellation_budgets`,
    /// which collects the registry via
    /// [`crate::kernel_registry::collect_registry`] and constructs the
    /// borrowed-value view once before the per-realization loop. The
    /// integration test
    /// `tessellate_realizations_uses_demanded_tolerance_through_per_stage_budget`
    /// in `tests/tolerance_wiring_e2e.rs` pins that the demanded tolerance
    /// flows through the helper to the kernel rather than being replaced
    /// by the `effective_tessellation_tolerance(module)` module-pragma
    /// fallback.
    ///
    /// `&self` is taken for forward compatibility (the future
    /// `RealizationDecl`-driven variant will read realization metadata
    /// from `self`) but is currently unused.
    #[allow(clippy::unused_self)]
    pub fn compute_realization_tolerance_budget(
        &self,
        registry: &BTreeMap<String, &CapabilityDescriptor>,
        available: &HashSet<ReprKind>,
        demanded_tol: f64,
    ) -> f64 {
        // `op` and `demanded` are `Copy` scalars (enum variants) — destructuring
        // them from the const here rather than accepting them as parameters keeps
        // the signature minimal and avoids any per-call allocation.  Only
        // `available` is caller-supplied because constructing the `HashSet` is the
        // one allocation we hoist to `compute_tessellation_budgets` (task 3227).
        let (op, demanded, _) = Self::BUDGET_QUERY_TRIPLE_V02;
        match dispatch(registry, op, demanded, available, None) {
            Some(plan) => per_stage_tolerance_for_plan(&plan, demanded_tol),
            None => demanded_tol,
        }
    }

    /// Hard-coded `(op, demanded_repr, available_reprs)` triple used by
    /// [`Self::compute_realization_tolerance_budget`] to query the
    /// dispatcher for a per-stage budget plan in v0.2.
    ///
    /// Centralised here so that when v0.3 multi-kernel adapters land and
    /// realization metadata begins carrying its own
    /// `Operation`/`ReprKind`/`available` triple, every call site that
    /// depends on this placeholder can be located by a single grep and
    /// re-pointed at the realization-derived triple. See the
    /// `compute_realization_tolerance_budget` docstring for the
    /// 0-conversion-plan pass-through behaviour this triple yields with the
    /// v0.2 single-kernel registry.
    ///
    /// **Post task 3227**: the `available` slice (`.2`) is consumed
    /// **once per build** by [`Self::compute_tessellation_budgets`] to
    /// construct a `HashSet<ReprKind>`, which is then passed by reference
    /// to every `compute_realization_tolerance_budget` call in the
    /// realization loop — rather than reconstructed per call inside the
    /// helper. A single grep for `BUDGET_QUERY_TRIPLE_V02` or
    /// [`Self::budget_available_set`] surfaces every consumer; the latter
    /// is the supported external accessor for the available-repr set.
    pub(crate) const BUDGET_QUERY_TRIPLE_V02: (Operation, ReprKind, &'static [ReprKind]) =
        (Operation::BooleanUnion, ReprKind::BRep, &[ReprKind::BRep]);

    /// Returns the set of `ReprKind`s that the dispatcher considers
    /// available for the v0.2 single-kernel budget query.
    ///
    /// This is the **supported external accessor** for the available-repr
    /// set.  `BUDGET_QUERY_TRIPLE_V02` is `pub(crate)`-only and is not
    /// part of the public API; external callers (e.g. integration tests)
    /// should use this helper so that a future change to the underlying
    /// slice (e.g. when v0.3 multi-kernel adapters land) is caught
    /// automatically by any test that calls `budget_available_set`.  A
    /// single grep for `budget_available_set` or `BUDGET_QUERY_TRIPLE_V02`
    /// surfaces every consumer.
    pub fn budget_available_set() -> HashSet<ReprKind> {
        Self::BUDGET_QUERY_TRIPLE_V02.2.iter().copied().collect()
    }

    /// Precompute per-realization demanded tolerance for the cache-key
    /// `(entity_id, ReprKind::BRep, demanded_tol)` triple, plus the
    /// fallback chain for callers that need the value as a non-`Option`
    /// (e.g. tessellation-budget computation).
    ///
    /// Returns a positionally-indexed `Vec<Vec<Option<f64>>>` aligned with
    /// `module.templates × realizations` iteration order: the outer Vec has
    /// one entry per template (same order as `module.templates`), each inner
    /// Vec has one entry per realization (same order as
    /// `template.realizations`). Consumers index by
    /// `[template_idx][realization_idx]` — zero String clones, zero hashing,
    /// O(1) lookup (task 3227).
    ///
    /// Resolves each entry via [`Engine::demanded_tolerance_for_output`],
    /// which folds both an output-level `RepresentationWithin` constraint
    /// (when `eval_state` is populated) and the active-tolerance contributor
    /// for the subject entity into a single `Option<f64>` — returning `None`
    /// only when neither contributor is present.  Callers that need the f64
    /// fallback (typically the tessellation-budget computation) chain through
    /// to `effective_tessellation_tolerance` at the consumption site.
    ///
    /// Extracted in the task 2874 amendment from inline blocks duplicated
    /// across `build` / `build_snapshot` / `tessellate_realizations` /
    /// `tessellate_snapshot` so future invalidation / fallback-chain edits
    /// land in one place.
    pub(crate) fn compute_demanded_tols(&self, module: &CompiledModule) -> Vec<Vec<Option<f64>>> {
        module
            .templates
            .iter()
            .map(|t| {
                t.realizations
                    .iter()
                    .map(|r| self.demanded_tolerance_for_output(&t.name, &r.id.entity))
                    .collect()
            })
            .collect()
    }

    /// Precompute per-realization tessellation budgets for the
    /// `kernel.tessellate(handle, budget)` call site.
    ///
    /// Returns a positionally-indexed `Vec<Vec<f64>>` aligned with
    /// `module.templates × realizations` iteration order: the outer Vec has
    /// one entry per template, each inner Vec has one entry per realization.
    /// Consumers index by `[template_idx][realization_idx]` — zero String
    /// clones, zero hashing, O(1) lookup (task 3227).
    ///
    /// For each `[template_idx][realization_idx]` cell, applies the priority
    /// chain `demanded_tols[t_idx][r_idx].flatten()` →
    /// `effective_tessellation_tolerance(module)` to obtain the requested
    /// tolerance, then routes that through
    /// [`Engine::compute_realization_tolerance_budget`] against the supplied
    /// owned-value `registry` to obtain the budgeted tolerance.
    ///
    /// **Allocation budget per build (post task 3227)**: 1
    /// `HashSet<ReprKind>` + 1 `BTreeMap<String, &CapabilityDescriptor>` +
    /// 2 `Vec<Vec<…>>` per build — replacing the previous R per-call
    /// `HashSet<ReprKind>` and 2 `HashMap<(String, String), …>` per build.
    ///
    /// **Borrow-map allocation cost** (amendment 2): the borrowed-value
    /// view `BTreeMap<String, &CapabilityDescriptor>` that
    /// [`crate::dispatcher::dispatch`] requires is built **once** here,
    /// before the realization loop, and reused for every realization in
    /// this build. The earlier arrangement built it inside
    /// `compute_realization_tolerance_budget` per-realization, leaving the
    /// per-build kernel-name-string allocation count at `R · K` (R
    /// realizations × K registered kernels). Hoisting the construction
    /// here drops the cost back to `K` per build regardless of `R`.
    ///
    /// Extracted in the task 2874 amendment from inline blocks duplicated
    /// across `tessellate_realizations` / `tessellate_snapshot`.
    pub(crate) fn compute_tessellation_budgets(
        &self,
        module: &CompiledModule,
        demanded_tols: &[Vec<Option<f64>>],
        registry: &BTreeMap<String, CapabilityDescriptor>,
    ) -> Vec<Vec<f64>> {
        // Build the borrowed-value view that `dispatch` requires ONCE per
        // build — see the "Borrow-map allocation cost" note above.
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry.iter().map(|(k, v)| (k.clone(), v)).collect();
        // Hoist the HashSet<ReprKind> construction once per build alongside
        // the borrowed-registry view. The available slice inside
        // BUDGET_QUERY_TRIPLE_V02 is `&'static [ReprKind]` so its contents
        // are const; there is no need to rebuild the HashSet per realization.
        // Cost drops from R allocations to 1 per build (task 3227).
        let available: HashSet<ReprKind> =
            Self::BUDGET_QUERY_TRIPLE_V02.2.iter().copied().collect();
        module
            .templates
            .iter()
            .enumerate()
            .map(|(t_idx, t)| {
                t.realizations
                    .iter()
                    .enumerate()
                    .map(|(r_idx, _r)| {
                        // Task 3227 / 3297: direct positional index — the
                        // producer (`compute_demanded_tols`) and consumer (this fn)
                        // iterate the same `module.templates × realizations`
                        // product unconditionally, so OOB is unambiguously an
                        // internal bug; Rust's slice indexing panics with the
                        // precise OOB message at runtime in both debug and release.
                        let req_tol = demanded_tols[t_idx][r_idx]
                            .unwrap_or_else(|| Self::effective_tessellation_tolerance(module));
                        self.compute_realization_tolerance_budget(
                            &registry_borrowed,
                            &available,
                            req_tol,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    /// Shared helper: execute geometry operations and tessellate each realization.
    ///
    /// Used by both `tessellate_realizations()` and `tessellate_snapshot()`.
    ///
    /// `values` is mutable so that conformance-query helpers
    /// (`is_watertight` / `is_manifold` / `is_orientable`) — whose
    /// kernel-aware dispatch lives outside the pure-value `eval_expr` path —
    /// can be patched into the per-template `value_cells`. Reads of `values`
    /// inside `execute_realization_ops` happen *before* the post-process
    /// runs, so the patch is observable only on the final `TessellateResult`
    /// surface — matching the build-pipeline semantics.
    ///
    /// `demanded_tols` is a positionally-indexed `&[Vec<Option<f64>>]`
    /// (indexed `[template_idx][realization_idx]`, aligned with
    /// `module.templates × realizations` iteration order) precomputed by
    /// the caller via [`Engine::compute_demanded_tols`] — task 2874
    /// step-6 / task 3227 refactor. The precompute decouples the
    /// `&self`-needing query from the `&mut self.*` borrows already split
    /// across this static helper's parameter list. Missing entries (caller-
    /// side bug — should not happen since the producer iterates the same
    /// product) fall back to `None`.
    /// `realization_cache` is the engine's per-build cache that
    /// `execute_realization_ops` populates on success and (post step-8) will
    /// consult on entry.
    ///
    /// `tessellation_budgets` is a positionally-indexed `&[Vec<f64>]`
    /// (indexed `[template_idx][realization_idx]`, same alignment) precomputed
    /// by the caller via [`Engine::compute_tessellation_budgets`] (task 2874
    /// step-12 / task 3227 refactor). The slice carries the budgeted tolerance
    /// — the demanded tolerance routed through the dispatcher's per-stage
    /// allocation primitive, with fallback to
    /// [`Self::effective_tessellation_tolerance`] when no per-output demand
    /// exists — that this helper hands to `kernel.tessellate(handle, budget)`.
    /// Both slices are indexed directly by `[t_idx][r_idx]` (task 3297):
    /// the producers (`compute_demanded_tols`, `compute_tessellation_budgets`)
    /// and this consumer iterate the same `module.templates × realizations`
    /// product unconditionally, so OOB is an internal bug and panics at
    /// runtime rather than silently returning a fallback value.
    #[allow(clippy::too_many_arguments)]
    fn tessellate_from_values(
        geometry_kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
        registry: &BTreeMap<String, &CapabilityDescriptor>,
        default_kernel_name: Option<&str>,
        module: &CompiledModule,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
        topology_attribute_table: &mut TopologyAttributeTable,
        swept_kind_table: &mut SweptKindTable,
        realization_cache: &mut RealizationCache<KernelHandle>,
        demanded_tols: &[Vec<Option<f64>>],
        tessellation_budgets: &[Vec<f64>],
        // Task ε (3436) step-12: per-build dispatch-count instrumentation
        // forwarded from `tessellate_realizations` / `tessellate_snapshot`
        // (each passes `&mut self.last_dispatch_count`). Threaded as a
        // separate parameter rather than packed into a struct so the static
        // fn's signature mirrors the disjoint-field-borrow shape already in
        // use for the other &mut params.
        dispatch_count: &mut usize,
        // Task ε (4741): per-realization sibling of `dispatch_count`, forwarded
        // to `execute_realization_ops` at the dispatch call below so the
        // caller's `Engine::last_dispatch_count_by_realization` map is populated
        // by the tessellate paths too. One-for-one mirror of `dispatch_count`.
        dispatch_count_by_realization: &mut HashMap<RealizationNodeId, usize>,
        // Determinacy β (task 4198): when `true`, `surface_subtree` calls
        // `kernel.measure_mesh_deviation` and populates `achieved_repr_tol`.
        // `false` by default — zero hot-path overhead when γ assertions
        // (`RepresentationWithin`) are not active.  Mirrors `capture_undef_causes`.
        capture_repr_tol: bool,
        // Determinacy β (task 4198): cleared at entry to each
        // `tessellate_realizations` / `tessellate_snapshot` call by the
        // caller; populated inside `surface_subtree` after each successful
        // tessellation when `capture_repr_tol` is true. Threaded here as a
        // sibling of the other &mut tables (topology_attribute_table /
        // swept_kind_table).
        achieved_repr_tol: &mut std::collections::BTreeMap<String, f64>,
        // θ (task 4361) step-6: Kahn schedule from `run_unified_pass`, threaded
        // from the caller (`tessellate_realizations` / `tessellate_snapshot`).
        // `Some` iff the engine's `build_scheduler == UnifiedDag`; `None` keeps
        // the existing declaration-order loop (LegacyMultiPass — byte-identical
        // to the pre-θ behaviour).
        unified_pass: Option<&crate::engine_fixpoint::UnifiedPassResult>,
        // θ (task 4361) step-6: value cells read by ANY realization (the union
        // of every trace's `reads`). Used by `hydrate_value_cell_in_loop` to
        // decide whether a selector cell is resolved eagerly (realization-read)
        // or kept as a descriptor (composition-only). Empty under LegacyMultiPass.
        realization_read_cells: &HashSet<reify_core::ValueCellId>,
        // β (task 4738) step-2: demand seed threaded from
        // `demand_scoped_unified_pass`. `Some(seed)` on the selective-warm
        // branch → the build_steps fallback is demand-gated (hidden
        // realizations pruned from the uncovered-realization append loop).
        // `None` on the full-scope / LegacyMultiPass branches → append-all
        // (byte-identical to pre-β). Passing the pre-computed seed set
        // (not &DemandRegistry) keeps the fn self-contained and matches how
        // the seed is produced once by the caller's helper.
        demand_seed: Option<&HashSet<NodeId>>,
    ) -> Vec<MeshSurface> {
        let mut meshes = Vec::new();

        // Task ε (3436): the engine's default kernel is fetched by name from
        // the multi-handle map; `None` (or absent) matches the v0.2 "no kernel
        // configured" semantics. Per-op dispatch routing is delegated to
        // `execute_realization_ops` (step-8), which takes the full map and
        // the borrowed-view registry. Single-handle surfaces below (export,
        // tessellate, post-process) operate on the default kernel.
        let default_kernel_name = match default_kernel_name {
            Some(name) if geometry_kernels.contains_key(name) => name,
            _ => return meshes,
        };

        let mut step_handles: Vec<KernelHandle> = Vec::new();
        // Task 3441: cross-template `GeomRef::Sub` threading.  As each
        // template's realizations complete, snapshot its `named_steps`
        // under the template name so a subsequent template that has
        // `sub <s> = <T>()` can seed its local `named_steps` with
        // `<s>.<member> → handle` entries derived from `T`'s snapshot.
        // Declaration order is treated as topological for non-recursive
        // structures (compile_builder/entities_phase.rs pushes templates
        // in declaration order; SCC detection tags cycles but does not
        // reorder).  Forward-declared subs and recursive structures fall
        // back to the existing "named_steps miss → Error" path in
        // `geometry_ops.rs::resolve_geom_ref`.
        //
        // Helper invocations (`seed_cross_sub_named_steps`,
        // `snapshot_named_steps`) factor the per-template seed/snapshot
        // logic out so the three eval loop sites stay in sync.
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();

        // T5 step-4 (Phase A): record each realization's terminal `KernelHandle`
        // positionally by `(t_idx, r_idx)` instead of tessellating it here. The
        // Phase-B containment walk (below) tessellates these handles at the
        // composed world pose and pushes the `MeshSurface`s. Sized to the full
        // `templates × realizations` product so anonymous realizations are
        // addressable; `None` marks a realization that produced no geometry.
        // `KernelHandle` is `Copy`.
        let mut terminal_handles: Vec<Vec<Option<KernelHandle>>> = module
            .templates
            .iter()
            .map(|t| vec![None; t.realizations.len()])
            .collect();

        // β (task 4738) amend: pre-extract demanded realization IDs as references
        // to avoid a per-iteration `RealizationNodeId` clone in the uncovered-
        // realization fallback loop.  Under full scope (`demand_seed = None`)
        // this is `None` → the fallback takes the `is_none_or` short-circuit
        // and appends all, byte-identical to pre-β.
        let demanded_rids: Option<HashSet<&RealizationNodeId>> = demand_seed.map(|seed| {
            seed.iter()
                .filter_map(|n| {
                    if let NodeId::Realization(r) = n {
                        Some(r)
                    } else {
                        None
                    }
                })
                .collect()
        });
        // GR-034: resolve once per eval-loop entry; threaded per-iteration.
        let long_chain_threshold = crate::dispatcher::long_chain_threshold_from_env();

        for (t_idx, template) in module.templates.iter().enumerate() {
            // Task 5033 Gap D: the isosurface-pipeline exception to this
            // function's hardcoded-BRep dispatch demand — see
            // `voxel_pipeline_demand_overrides` doc for why this is scoped
            // narrowly rather than reusing `compute_demanded_reprs`.
            let voxel_demand_overrides = voxel_pipeline_demand_overrides(template);
            // `named_steps` is scoped per-template so that two structures
            // that each declare `let body = …` cannot clobber each other's
            // name → handle entries.  Cross-template `GeomRef::Sub`
            // references are now supported for non-collection subs via
            // compound keys `<sub_name>.<member>` seeded below (task 3441);
            // collection-sub geometry composition remains deferred (the
            // compile-side diagnostic in `expr.rs::try_emit_cross_sub_geometry`
            // continues to fire for those call sites).
            let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
            // Task 5033 Gap #2 Gap A: by-name repr sibling of `named_steps`.
            // See `RealizationOutputs::named_step_reprs` doc for why.
            let mut named_step_reprs: HashMap<String, ReprKind> = HashMap::new();
            seed_cross_sub_named_steps(
                template,
                &module_named_steps,
                &mut named_steps,
                geometry_kernels,
                default_kernel_name,
                values,
                functions,
                meta_map,
                diagnostics,
                &module.templates,
            );
            // θ (task 4361) step-6: order this template's realizations + selector/
            // query value-cells for the tessellate walk.  Under UnifiedDag the order
            // is `run_unified_pass`'s global Kahn schedule filtered to THIS template's
            // nodes; any realization not covered by the schedule is appended in
            // declaration order so every realization still runs exactly once.
            // Under LegacyMultiPass the order is declaration order with NO interleaved
            // HydrateCell steps — byte-identical to the pre-θ behaviour.
            // Mirrors build()'s and build_snapshot()'s `build_steps` pattern.
            let build_steps: Vec<BuildStep> = match unified_pass {
                Some(pass) => {
                    let mut steps: Vec<BuildStep> = Vec::new();
                    let mut realized: HashSet<usize> = HashSet::new();
                    for node in &pass.schedule {
                        match node {
                            NodeId::Realization(rid) if rid.entity == template.name => {
                                if let Some(r_idx) =
                                    template.realizations.iter().position(|r| r.id == *rid)
                                {
                                    steps.push(BuildStep::Realize(r_idx));
                                    realized.insert(r_idx);
                                }
                            }
                            NodeId::Value(vid) if vid.entity == template.name => {
                                steps.push(BuildStep::HydrateCell(vid.clone()));
                            }
                            _ => {}
                        }
                    }
                    for r_idx in 0..template.realizations.len() {
                        if !realized.contains(&r_idx) {
                            // β (task 4738) step-2: demand guard on the
                            // "uncovered realization" fallback. Under
                            // selective demand a hidden body's realization is
                            // deliberately excluded from the seed/schedule;
                            // the unguarded append would re-add and execute
                            // it, defeating the kernel-time saving. Guard:
                            // `demanded_rids = None` (full scope / build) →
                            // append all (byte-identical); `Some(rids)` →
                            // append only if the realization is in the cone.
                            // `demanded_rids` is pre-extracted above so this
                            // lookup requires no per-iteration clone.
                            let rid = &template.realizations[r_idx].id;
                            if demanded_rids.as_ref().is_none_or(|rids| rids.contains(rid)) {
                                steps.push(BuildStep::Realize(r_idx));
                            }
                        }
                    }
                    steps
                }
                None => (0..template.realizations.len())
                    .map(BuildStep::Realize)
                    .collect(),
            };
            for build_step in &build_steps {
                let (r_idx, realization) = match build_step {
                    BuildStep::Realize(r_idx) => (*r_idx, &template.realizations[*r_idx]),
                    BuildStep::HydrateCell(cell_id) => {
                        // θ (4361 step-6): early hydration of selector / geometry-query
                        // value cells before consuming realizations (UnifiedDag only).
                        // Mirrors build_snapshot's HydrateCell handling; degrade to
                        // SKIP rather than abort if the kernel is absent (additive
                        // hydration — the per-template post-process block below
                        // re-runs the same passes over every cell).
                        let Some(kernel) = geometry_kernels.get_mut(default_kernel_name) else {
                            debug_assert!(
                                false,
                                "default kernel must remain in the map across the schedule walk"
                            );
                            continue;
                        };
                        // tessellate_from_values is a static fn without snapshot
                        // access; pass an empty map (fail-open: gate skipped for
                        // unknown repr, preserving today's tessellation behaviour).
                        let realized_reprs_tess: HashMap<RealizationNodeId, ReprKind> =
                            HashMap::new();
                        Engine::hydrate_value_cell_in_loop(
                            template,
                            cell_id,
                            &named_steps,
                            values,
                            functions,
                            meta_map,
                            kernel.as_mut(),
                            topology_attribute_table,
                            realization_read_cells,
                            &realized_reprs_tess,
                            diagnostics,
                        );
                        continue;
                    }
                };
                let handle_start = step_handles.len();
                // Tessellate paths do not propagate kernel errors into
                // `Freshness::Failed` today (arch §9.1 wires that on the
                // build path only — see `Engine::build` / `Engine::build_snapshot`).
                // Pass `&mut None` so `execute_realization_ops` collects the
                // diagnostic but no caller acts on the kernel error here.
                let mut kernel_error: Option<ErrorRef> = None;
                // Step-10 (task ε / 3436): the tessellate path is a static
                // function without `&mut self` access to `eval_state`, so the
                // executor's terminal-repr signal is collected but discarded
                // here — produced_repr graph-node updates happen only on the
                // build/build_snapshot path per step-10's scope (the
                // `executor_writes_produced_repr_brep_on_build_snapshot`
                // forward-guard pins build_snapshot only).
                //
                // **Symmetric-write follow-up (task ζ / #3437)** — amendment
                // round 2: today the discard is benign because the
                // construction-time default (`ReprKind::BRep`, see
                // `graph.rs:53/329`) already matches what the v0.3-ε executor
                // produces, so any consumer that reads `produced_repr` after
                // a tessellate-only call sees the correct value by accident.
                // Once ζ / η make per-op `demanded` vary the tessellate path
                // would silently leave the graph node at the BRep default
                // while build / build_snapshot write the new repr — GUI
                // overlays that tessellate without exporting would see a
                // stale value. The fix is to extend `tessellate_from_values`
                // to return a `Vec<(RealizationNodeId, ReprKind)>` (or take a
                // disjoint-borrow `&mut` writer) and have
                // `tessellate_realizations` / `tessellate_snapshot` apply the
                // writes via the same idiom used in `build_snapshot`. Tracked
                // by task ζ (#3437); the symmetric-write requirement MUST
                // close before ζ ships.
                let mut produced_repr_out: Option<ReprKind> = None;
                // Task 3227 / 3297: direct positional index — no String clones,
                // no hashing. The producer (`compute_demanded_tols`) and this
                // consumer iterate the same `module.templates × realizations`
                // product unconditionally, so OOB is unambiguously an internal
                // bug; Rust's slice indexing panics with a precise OOB message
                // at runtime in both debug and release.
                let demanded_tol = demanded_tols[t_idx][r_idx];
                // Task 5033 Gap D: BRep for every realization EXCEPT the two
                // isosurface-pipeline roles overridden above — see
                // `voxel_pipeline_demand_overrides` doc for why this narrow
                // exception is correct and safe (every other realization's
                // demand is byte-identical to the pre-existing hardcoded-BRep
                // behavior below).
                let demanded_repr = voxel_demand_overrides
                    .get(&r_idx)
                    .copied()
                    .unwrap_or(ReprKind::BRep);
                Engine::execute_realization_ops(
                    RealizationOpsInput::new(
                        geometry_kernels,
                        registry,
                        default_kernel_name,
                        &realization.operations,
                        values,
                        functions,
                        meta_map,
                        diagnostics,
                        &realization.id,
                        realization.span,
                        &mut kernel_error,
                        realization_cache,
                        &mut *dispatch_count,
                        &mut *dispatch_count_by_realization,
                    )
                    .with_realization_name(realization.name.as_deref())
                    .with_demanded_tol(demanded_tol)
                    // Task 4050 step-8 / design_decision 4 (narrowed by task
                    // 5033 Gap D): BRep for every realization except the two
                    // isosurface-pipeline roles (operand → Voxel, consumer →
                    // Mesh) — computed above (`voxel_demand_overrides`).
                    // `walk_placed_realizations` (geometry_ops.rs) resolves the
                    // trailing tessellate call's kernel from each terminal
                    // handle's own `KernelHandle.kernel` (task 5033 Gap D
                    // sibling fix), so a non-default-kernel terminal (e.g. an
                    // OpenVDB Voxel/Mesh handle) no longer breaks that call.
                    .with_demanded_repr(demanded_repr)
                    // Tessellate path never demands VolumeMesh, so never boundary.
                    .with_demanded_boundary(false)
                    // Task #3443: thread module-scope #kernel(...) pragma
                    // from the tessellate entry point into the per-op dispatcher.
                    .with_prefer_kernel(module.kernel_pragma.as_deref())
                    .with_is_terminal_realization(r_idx + 1 == template.realizations.len())
                    // Task 4744 β step-16: tessellate path never demands
                    // VolumeMesh, so the morph arm never fires here.
                    .with_morph_io(crate::morph_producer::MorphDispatchIo::disabled())
                    // GR-034: resolved once at entry (see above).
                    .with_long_chain_threshold(long_chain_threshold),
                    RealizationOutputs::new(
                        &mut step_handles,
                        &mut named_steps,
                        &mut named_step_reprs,
                        &mut *topology_attribute_table,
                        &mut *swept_kind_table,
                        &mut produced_repr_out,
                    ),
                );

                // T5 step-4 (Phase A): record this realization's terminal
                // handle positionally instead of tessellating here. The mesh
                // push relocates to the Phase-B containment walk, which
                // tessellates the recorded handle at the composed world pose so
                // each contained descendant is surfaced ONCE under its composed
                // entity_path (no standalone double-surfacing). `KernelHandle`
                // is `Copy`; `step_handles` outlives this iteration so the
                // handle stays valid for Phase B (the kernel sessions in
                // `geometry_kernels` live for the whole call).
                if step_handles.len() > handle_start {
                    terminal_handles[t_idx][r_idx] = step_handles.last().copied();
                }
            }
            // Step-8 (task ε / 3436): re-borrow the default kernel from the
            // map for post-process — see `build` / `build_snapshot` mirror.
            let default_kernel = geometry_kernels
                .get_mut(default_kernel_name)
                .expect("default kernel must remain in the map across the per-realization loop");
            // Task 3616: hydrate geometry-handle value cells before any
            // post-process that reads them (topology selectors need the parent
            // Value::GeometryHandle in `values`). Mirrors the
            // `post_process_geometry_handle_cells` call in `build`/
            // `build_snapshot` but without cache/freshness recording, since
            // `tessellate_from_values` is a static fn without access to
            // `self.cache` or `self.realization_handles`.
            Engine::hydrate_geometry_handles_into_values(
                template,
                &named_steps,
                values,
                functions,
                meta_map,
            );
            // Task 2320 amendment: mirrors the `build` / `build_snapshot`
            // wire-up so `TessellateResult.values` exposes the same
            // kernel-resolved `Bool` for conformance-query cells as
            // `BuildResult.values`. See
            // `Engine::post_process_conformance_queries` docstring.
            Engine::post_process_conformance_queries(
                template,
                &named_steps,
                values,
                default_kernel.as_ref(),
                diagnostics,
            );
            // Task 2531: see the build / build_snapshot wire-up. Tessellate
            // surface exposes the same kernel-resolved kinematic-query
            // values as the build surface so GUI overlays stay consistent.
            Engine::post_process_kinematic_queries(
                template,
                &named_steps,
                values,
                default_kernel.as_mut(),
                diagnostics,
            );
            // tessellate_from_values is a static fn without snapshot access;
            // pass an empty realized_reprs map (fail-open: gate skipped for
            // unknown repr, preserving today's tessellation behaviour).
            let realized_reprs_tess: HashMap<RealizationNodeId, ReprKind> = HashMap::new();
            Engine::run_post_processes(
                template,
                &named_steps,
                values,
                functions,
                meta_map,
                default_kernel.as_mut(),
                topology_attribute_table,
                &*swept_kind_table,
                &realized_reprs_tess,
                diagnostics,
                &module.templates,
            );
            // Task 3441: snapshot this template's `named_steps` so a later
            // template that subs from it can seed compound-key entries.
            // See the matching wiring in `build` / `build_snapshot`.
            // `named_steps` is moved (not cloned) — it would fall out of
            // scope at the loop body's end anyway, and the post-process
            // helpers above only borrow it.
            snapshot_named_steps(template, named_steps, &mut module_named_steps);
        }

        // ── Phase B (T5 step-4): containment-tree surfacing ──────────────────
        // Walk each root template's sub-tree depth-first and surface every
        // contained descendant ONCE under its composed entity_path, tessellating
        // the terminal handle recorded in Phase A. Non-root (subbed) templates
        // are suppressed standalone — they appear only here, at their place in
        // the tree, at the composed world pose (identity at step-4; step-10
        // applies the composed transform before tessellation). Independent /
        // single templates are roots and surface bit-identically to pre-T5.
        // Roots start at the identity world transform (`compose_pose_chain(&[])`);
        // step-10 accrues each sub's `at` pose onto it down the walk.
        let identity_world = crate::geometry_ops::compose_pose_chain(&[]);
        let roots = crate::geometry_ops::root_template_indices(module);
        for &root_idx in &roots {
            let root_prefix = module.templates[root_idx].name.clone();
            crate::geometry_ops::surface_subtree(
                module,
                root_idx,
                &root_prefix,
                // Roots have no aux ancestor; inheritance accrues down the walk.
                false,
                &identity_world,
                0,
                &terminal_handles,
                geometry_kernels,
                default_kernel_name,
                tessellation_budgets,
                values,
                functions,
                meta_map,
                &mut meshes,
                diagnostics,
                capture_repr_tol,
                achieved_repr_tol,
            );
        }

        // T5 amendment (reviewer robustness_edge_case): surface any template that
        // is reachable from NO root. Such a template is excluded from the root
        // set (some sub names it) yet no root reaches it, which is only possible
        // inside a non-collection containment cycle with no acyclic entry point
        // (self-recursive `sub child : Self`, or a mutual `A -> B -> A`). Without
        // this it would be SILENTLY DROPPED — pre-T5 it surfaced standalone. We
        // seed each uncovered template once as a fallback root; its own
        // (cycle-guard-bounded) walk covers its cycle peers, so we extend
        // `covered` to avoid re-seeding them. In an acyclic module every template
        // is already covered by `roots`, so this loop is a no-op.
        let mut covered = crate::geometry_ops::reachable_template_indices(module, &roots);
        for t_idx in 0..module.templates.len() {
            if covered.contains(&t_idx) {
                continue;
            }
            let fallback_prefix = module.templates[t_idx].name.clone();
            crate::geometry_ops::surface_subtree(
                module,
                t_idx,
                &fallback_prefix,
                false,
                &identity_world,
                0,
                &terminal_handles,
                geometry_kernels,
                default_kernel_name,
                tessellation_budgets,
                values,
                functions,
                meta_map,
                &mut meshes,
                diagnostics,
                capture_repr_tol,
                achieved_repr_tol,
            );
            covered.extend(crate::geometry_ops::reachable_template_indices(
                module,
                &[t_idx],
            ));
        }

        meshes
    }

    /// Probe the per-engine [`RealizationCache`] for a cached terminal handle
    /// and — on a hit — apply the realization's cold-path success side
    /// effects, short-circuiting the kernel op loop.
    ///
    /// Extracted (task 5059 η, zero behavior change) from the inline
    /// cache-hit short-circuit that used to open [`Self::execute_realization_ops`]
    /// (task 2874 step-8); see that method's doc for the surrounding op-loop
    /// contract this probe short-circuits.
    ///
    /// Returns `true` after writing the full side-effect set below; returns
    /// `false` — with ZERO side effects on `outputs` — when the guard
    /// (`is_terminal_realization && demanded_tol.is_some() &&
    /// realization_name.is_some()`) is unmet or the cache probe misses.
    ///
    /// The hit/miss bit is all the sole production caller
    /// ([`Self::execute_realization_ops`]) needs — it only branches on
    /// whether to short-circuit, never on the cached handle or repr (both
    /// already landed in `outputs` as part of the side-effect set above). A
    /// prior revision returned `Option<CacheHit { handle, resolved_repr }>`
    /// so the unit tests below could assert the resolved handle/repr
    /// directly; that duplicated what the `outputs`-view assertions already
    /// cover, so the tests now assert this `bool` plus those views instead.
    ///
    /// # Invariants
    ///
    /// This method IS the enforcement mechanism for INV-BUILD-3 ("a
    /// cache-hit short-circuit produces the same observable side-effect set
    /// as the path it short-circuits" — `docs/invariants.md`, PRD §5.1).
    /// Four contract pins, each with its own unit test:
    ///
    /// 1. **Probes ONLY when `is_terminal_realization && demanded_tol.is_some()
    ///    && realization_name.is_some()`.** A guard-fail (or a plain cache
    ///    miss) returns `false` with zero side effects on `outputs` — no
    ///    push/insert on any output view, no `topology_attribute_table`
    ///    eviction. Pinned by
    ///    `probe_realization_cache_guard_requires_terminal_named_and_tol`.
    /// 2. **Primary lookup at `demanded_repr`; `BRep` fallback on a
    ///    non-`BRep` miss.** A `demanded_repr == BRep` demand never falls
    ///    back (the `or_else` short-circuits); both lookups honor the
    ///    cache's tol partial order (`cached_tol ≤ requested_tol`
    ///    satisfies). Pinned by
    ///    `probe_realization_cache_falls_back_to_brep_and_honors_tol_partial_order`.
    /// 3. **On a hit, the FULL cold-path side-effect set — and nothing
    ///    else.** `step_handles` push, `named_steps` + `named_step_reprs`
    ///    insert, `produced_repr_out` write, the `debug_assert_eq!`
    ///    consistency guard; dispatch counters are left at their entry
    ///    values, and the function returns before the op loop ever runs.
    ///    Pinned by
    ///    `probe_realization_cache_primary_hit_applies_full_side_effect_set`
    ///    (single hit) and, cold-vs-warm,
    ///    `probe_realization_cache_cold_warm_side_effect_set_parity`.
    /// 4. **The 4349 cross-kernel eviction is a documented interim
    ///    trade-off, not a fix** — see the `Cross-kernel collision guard
    ///    (task 4349)` comment at the `topology_attribute_table.remove(...)`
    ///    call site below for the mechanism and trade-off. Retiring it is
    ///    follow-up task θ's job, gated on #4351 — this task lands the
    ///    mechanism only, so INV-BUILD-3 stays `proposed`. Pinned by
    ///    `cache_hit_short_circuit_tolerates_cross_kernel_topology_attribute_id_collision`.
    #[allow(clippy::too_many_arguments)]
    fn probe_realization_cache(
        realization_cache: &RealizationCache<KernelHandle>,
        realization_id: &RealizationNodeId,
        realization_name: Option<&str>,
        demanded_repr: ReprKind,
        demanded_tol: Option<f64>,
        is_terminal_realization: bool,
        outputs: &mut RealizationOutputs<'_>,
    ) -> bool {
        // Task 2874, step-8: cache-hit short-circuit (extracted into this
        // helper by task 5059 η — see this function's rustdoc `# Invariants`
        // for the full contract). When the caller has threaded a demanded
        // tolerance AND the realization is named (the `named_steps` contract
        // requires a name to write into the map), probe the per-engine
        // `RealizationCache` at `(entity_id, cache_repr, demanded_tol)`. On
        // hit we push the cached terminal handle, write
        // `named_steps[name] = cached_handle`, and return `true` —
        // preserving the post-condition the success path establishes in the
        // caller, [`Self::execute_realization_ops`]. On miss (or when either
        // guard is `None`) we return `false`; the caller then falls through
        // to the kernel op loop, and step-6's post-success insert at the
        // bottom of that caller populates the cache for the NEXT call. The
        // lookup uses `RealizationCache`'s partial-order "tighter satisfies
        // looser" rule (`cached_tol ≤ requested_tol`), so a tighter request
        // automatically misses a looser cached entry (see step-13's pin).
        //
        // `cache_repr` is bound to a local so the lookup-key repr and the
        // `produced_repr_out` write below are sourced from the same value.
        // If a future change shifts the cache key to a non-`BRep` `ReprKind`
        // (the cache's `(entity, repr, tol, options)` shape already
        // supports it; see `RealizationCache::lookup`), the
        // `produced_repr_out` write follows without a separate edit.
        // `execute_realization_ops`'s post-loop insert-key fallback binds
        // the identical `demanded_repr` value independently below (not
        // through this function) — see its own comment for why.
        //
        // Task 4050 step-10 (gap 4): `cache_repr` is unpinned from `BRep` to
        // the realization's `demanded_repr` (the υ-derived requested terminal
        // repr, known before the caller's op loop). The cache-hit LOOKUP keys on
        // it, so a second identical Mesh build short-circuits at
        // `(entity, Mesh, tol)`. The caller's post-loop INSERT keys on the
        // RESOLVED repr instead, so a fallback realization that demanded Mesh
        // but resolved BRep is stored at BRep: a later Mesh lookup correctly
        // MISSES at the Mesh key (never returning a BRep handle as if it were
        // Mesh), and the BRep fallback probe added below then recovers the hit
        // at the resolved BRep key.
        let cache_repr = demanded_repr;
        // Probe the terminal cache at the demanded repr first, then — for a
        // non-BRep demand that missed — retry at `BRep`. This mirrors the
        // per-op dispatch BRep fallback (design_decision 3) at the cache
        // layer, and is the fix for a realization-cache perf regression
        // (reviewer_comprehensive #1) where a Mesh demand always missed; see
        // below for why.
        //
        // WHY THE FALLBACK PROBE IS LOAD-BEARING. With υ wired, an Stl/Obj export
        // marks its terminal realization `Mesh`, so the primary probe keys on
        // `(entity, Mesh, tol)`. But reify-eval links no Mesh-capable boolean
        // kernel (Cargo.toml: openvdb dep, occt dev-dep, no manifold), so every
        // op falls back to a BRep dispatch, the terminal RESOLVES to BRep, and
        // the post-loop INSERT keys on `(entity, BRep, tol)` (the resolved repr;
        // design_decision 2). Without this fallback probe a Mesh demand would
        // miss the BRep entry on EVERY rebuild and recompute the (typically most
        // expensive) terminal body in full — defeating the task-2874 cache for
        // the dominant production export path. Retrying at BRep lets the
        // fell-back realization hit its true resolved repr and report
        // `produced_repr = BRep` (exactly the cold-path value).
        //
        // SAFETY (no stale Mesh↦BRep substitution). A `BRep` cache entry is
        // written ONLY when a realization RESOLVED to BRep (the INSERT keys on
        // the resolved repr). On a Mesh-CAPABLE engine a Mesh demand resolves
        // Mesh and inserts at `(entity, Mesh, tol)`, so the PRIMARY probe hits
        // and the BRep fallback is never consulted for that entity. The only
        // residual edge — a Mesh-capable engine doing both a `Step` (BRep) build
        // and an `Stl` (Mesh) build of the SAME entity at the SAME tol, where the
        // fallback could serve the Step entry to the Stl demand — cannot arise in
        // reify-eval (no Mesh boolean kernel is linked, so a Mesh demand can never
        // resolve Mesh here) and is task ζ's (#3437) surface, not this task's.
        if is_terminal_realization && let (Some(tol), Some(name)) = (demanded_tol, realization_name)
        {
            let cache_probe = realization_cache
                .lookup(&realization_id.entity, cache_repr, tol, NO_OPTIONS)
                .map(|&handle| (handle, cache_repr))
                .or_else(|| {
                    if cache_repr != ReprKind::BRep {
                        realization_cache
                            .lookup(&realization_id.entity, ReprKind::BRep, tol, NO_OPTIONS)
                            .map(|&handle| (handle, ReprKind::BRep))
                    } else {
                        None
                    }
                });
            if let Some((cached_handle, resolved_repr)) = cache_probe {
                // Cross-kernel collision guard (task 4349), re-keyed by task
                // #4351: `TopologyAttributeTable` is now keyed by the full
                // `KernelHandle` (kernel + kernel-local id) rather than a bare
                // `GeometryHandleId`, so this eviction removes exactly the
                // cached handle's own entry and no longer collaterally evicts
                // a different kernel's sibling entry that happens to share the
                // same numeric id.
                //
                // We defensively remove any entry at `cached_handle` from the
                // table rather than asserting it is already absent. This is a
                // no-op in the common case (the per-build reset already
                // cleared the table) and enforces the #3226 spec ("a
                // cache-served handle has no entries in those tables on the
                // second build") when a stale entry from a prior build
                // happens to still be present at this exact handle.
                //
                // DO NOT retire this eviction — downstream task #5064 removes
                // it and installs a fail-closed assert once the surrounding
                // invariants are strong enough to guarantee the table is
                // already empty at this point.
                outputs.topology_attribute_table.remove(cached_handle);
                outputs.step_handles.push(cached_handle);
                outputs.named_steps.insert(name.to_string(), cached_handle);
                // Task 5033 Gap #2 Gap A: by-name repr sibling write. Mirrors
                // the `named_steps` insert above — see
                // `RealizationOutputs::named_step_reprs` doc for why a LATER
                // realization's cross-realization `GeomRef::Sub` parent must
                // be resolved by name rather than by bare handle-id.
                outputs.named_step_reprs.insert(name.to_string(), resolved_repr);
                // Step-10 (task ε / 3436): the [`RealizationCache`] key includes
                // the repr (see the post-success `realization_cache.insert` call
                // at the bottom of this function), so the cached terminal handle
                // was produced by a kernel capable of `resolved_repr` —
                // `cache_repr` on a primary hit, or `BRep` on the fallback hit.
                // Surface that SAME repr through `produced_repr_out` so the
                // caller writes into the realization graph node exactly what a
                // cold-path build of this realization would have written.
                *outputs.produced_repr_out = Some(resolved_repr);
                // **Task 4050 step-10**: consistency guard, reordered AFTER the
                // `produced_repr_out` write. As positioned, this is
                // tautological today — `produced_repr_out` was just set to
                // `Some(resolved_repr)` on the line above, so the
                // `unwrap_or` side can never disagree with `resolved_repr`
                // and the assert can never fire against the current code.
                // It is kept anyway as executable documentation of the
                // invariant it names (the surfaced `produced_repr` always
                // equals the cache key's repr on the cache-hit branch): if a
                // future edit reorders this write or lets the two values be
                // derived independently, the assert starts actually
                // exercising the check and will trip in any debug-assertions
                // build (including `cargo test`) the moment that invariant
                // breaks, rather than staying silently wrong.
                //
                // A regression test that made this genuinely non-tautological
                // would need a PRIMARY cache hit resolving to a non-`BRep`
                // repr from a real kernel dispatch — but reify-eval links no
                // Mesh-capable boolean kernel (see "WHY THE FALLBACK PROBE IS
                // LOAD-BEARING" above), so every resolved repr the cache can
                // actually contain today is `BRep`. Deferred until one does.
                debug_assert_eq!(
                    resolved_repr,
                    outputs.produced_repr_out.unwrap_or(ReprKind::BRep),
                    "cache-hit produced_repr must equal the cache key's repr",
                );
                // Task 4744 β step-20: a cache-served terminal produced no fresh
                // VolumeMesh this call, so there is nothing to stash.
                return true;
            }
        } // end is_terminal_realization cache-probe guard
        false
    }

    /// Execute the per-realization geometry operation loop and perform rollback
    /// on partial failure.
    ///
    /// Captures `handle_start = step_handles.len()` on entry.  For each op in
    /// `operations`, evaluates it via `compile_geometry_op` and dispatches to
    /// the kernel:
    ///
    /// - `Ok(geom_op)` — dispatches to the kernel; on success pushes
    ///   `handle.id` to `step_handles`; on kernel error emits a geometry-error
    ///   diagnostic and breaks the loop.  Kernel errors break immediately: a
    ///   geometry engine failure is often unrecoverable (e.g. corrupt state),
    ///   and subsequent ops that depend on the failed handle would fail too.
    /// - `Err(reason)` — pushes `GeometryHandleId::INVALID` sentinel, emits a
    ///   compile-error diagnostic, sets `had_failure = true`, and continues.
    ///   Compile errors are cheaper to continue past because the sentinel lets
    ///   independent ops proceed.
    ///
    /// After the op loop, if `had_failure` or fewer handles were produced than
    /// there are `operations`, truncates `step_handles` to `handle_start` (discards
    /// all partial handles from this realization).
    ///
    /// **Duplicate `realization_name` within a template:** last-write-wins —
    /// a later realization with the same name shadows the earlier one in
    /// `named_steps`.  Pinned by
    /// `execute_realization_ops_duplicate_name_shadows_previous`.
    ///
    /// **`kernel_error_out`** (arch §9.1 lines 868–877): when
    /// `kernel.execute(...)` returns `Err(...)`, the helper additionally writes
    /// `Some(ErrorRef::new("geometry error: …"))` to `*kernel_error_out` so the
    /// caller can mark the realization NodeId as `Freshness::Failed { error }`
    /// in the eval cache and emit a single `EventKind::Failed` event.  When
    /// the loop completes without a kernel error (success or compile-only
    /// failure), `*kernel_error_out` is left untouched (typically `None`).  The
    /// caller is responsible for the cache + journal writes because the
    /// realization NodeId, cache, and journal are not threaded into this
    /// helper — see `Engine::mark_realization_failed` for the wire site.
    ///
    /// **`demanded_tol` + `realization_cache`** (task 2874, step-6 wiring): the
    /// caller pre-computes the demanded tolerance for the realization via
    /// [`Engine::demanded_tolerance_for_output`] (with fallback to
    /// [`Engine::active_tolerance_for`]) and threads it in alongside a mutable
    /// borrow of [`Engine::realization_cache`]. After a fully-successful
    /// realization (the `step_handles[handle_start..].last()` branch that
    /// records `named_steps`), if `demanded_tol` is `Some(t)` the helper
    /// inserts `(realization_id.entity, ReprKind::BRep, t, last_handle)` into
    /// the cache. When `demanded_tol` is `None` (no demand contributor exists
    /// for this realization) no cache entry is written — preserving the
    /// historical "no tolerance contract → no caching" semantics.
    ///
    /// **Cache-hit short-circuit**: before the op loop even starts, this
    /// helper delegates to [`Self::probe_realization_cache`], which — on a
    /// hit — applies the realization's cold-path success side effects
    /// (`step_handles` push, `named_steps` + `named_step_reprs` insert,
    /// `produced_repr_out` write, and the task-4349 cross-kernel
    /// `topology_attribute_table` eviction) and returns early, before this
    /// helper's `RealizationOutputs` destructure ever runs. See that
    /// method's `# Invariants` doc (INV-BUILD-3) for the full
    /// guard/fallback/side-effect/eviction contract and its pinning tests.
    ///
    /// **Known limitation** (recorded as a design decision): a cache-hit
    /// short-circuit skips per-op `topology_attribute_table` population,
    /// including the kernel-attribute hook propagation added in task 2875.
    /// The table is reset to `default()` at the start of every `build()`
    /// (see callers around engine_build.rs
    /// `topology_attribute_table = TopologyAttributeTable::default()`), so a
    /// cache-served handle has no entries in the table on the second
    /// build. v0.2 callers do not combine `activate_purpose` with attribute
    /// queries today, so this is documented (not regressed) in scope; a
    /// follow-up task can either cache the table entries alongside the
    /// handle or skip the table reset for engines with non-empty cache.
    /// (The task-4349 cross-kernel `GeometryHandleId` collision guard that
    /// partially compensates for this on a cache-hit now lives on
    /// [`Self::probe_realization_cache`] — see that method's doc.)
    fn execute_realization_ops(
        input: RealizationOpsInput<'_>,
        mut outputs: RealizationOutputs<'_>,
        // Task 4744 β step-20: returns the source-bundle stash for this
        // realization (the freshly-produced VolumeMesh + a snapshot of the BRep
        // it was meshed from) when a morph producer is active, so the caller can
        // store it for the NEXT tick's morph. `None` whenever no producer is
        // registered or no VolumeMesh was produced (every off-build call site).
    ) -> Option<crate::morph_producer::MorphSource> {
        let RealizationOpsInput {
            kernels,
            registry,
            default_kernel_name,
            operations,
            values,
            functions,
            meta_map,
            diagnostics,
            realization_id,
            realization_name,
            realization_span,
            kernel_error_out,
            realization_cache,
            demanded_tol,
            demanded_repr,
            demanded_boundary,
            dispatch_count,
            dispatch_count_by_realization,
            prefer_kernel,
            is_terminal_realization,
            morph_io,
            long_chain_threshold,
        } = input;
        if Self::probe_realization_cache(
            realization_cache,
            realization_id,
            realization_name,
            demanded_repr,
            demanded_tol,
            is_terminal_realization,
            &mut outputs,
        ) {
            return None;
        }
        let RealizationOutputs {
            step_handles,
            named_steps,
            named_step_reprs,
            topology_attribute_table,
            swept_kind_table,
            produced_repr_out,
        } = outputs;
        let handle_start = step_handles.len();
        // Task 4744 β (step-20): source-bundle stash returned to the caller
        // (`build` / `build_snapshot`), which writes it into the per-realization
        // `Engine::morph_source` side-table AFTER this call returns (the engine
        // method needs `&mut self`, unavailable here). Stays `None` unless a
        // morph producer is active AND this realization produced a VolumeMesh —
        // so every `disabled()` call site (no producer) returns `None` and is
        // byte-identical. Populated in the success branch's VolumeMesh dispatch.
        let mut morph_source_stash: Option<crate::morph_producer::MorphSource> = None;
        // Task 4050 step-8: the per-op `available` set is no longer a hoisted
        // loop-invariant `{BRep}` constant — it is derived per op from the
        // reprs of that op's resolved input handles (`realization_step_reprs`,
        // tracked in lockstep with `realization_step_ids` below), defaulting to
        // `{BRep}` for primitives / unresolved refs (design_decision 6). This
        // lets a conversion stage that materialises a Mesh handle propagate the
        // Mesh repr to downstream ops while staying `{BRep}` for the v0.2 path.
        // Task 5059 η: `cache_repr` is also read by the post-loop cache-insert
        // below (`named_step_reprs`'s and `realization_cache.insert`'s
        // `last_produced_repr.unwrap_or(cache_repr)` fallback) — code that is
        // NOT part of the extracted `Self::probe_realization_cache`
        // short-circuit above. Bound independently here to the same
        // `demanded_repr` value that helper's own `cache_repr` binds (see
        // its comment) — a plain alias with no anticipated transform, so a
        // second identical binding is simpler than threading a shared
        // function through both call sites.
        let cache_repr = demanded_repr;

        // GR-034 (task #3445): measure realization wall-time from this point
        // so cache hits (which returned above) never contribute to elapsed.
        let realize_start = Instant::now();
        let mut had_failure = false;
        // Step-14 (task ε / 3436): captures the terminal output [`ReprKind`]
        // for the LAST op that successfully executed in this realization's
        // loop. After the loop, on the fully-successful (not rolled back)
        // branch, the value is written to `produced_repr_out`. On rollback
        // the channel is left untouched so the caller writes nothing — the
        // realization graph node retains its construction-time default.
        //
        // Replaces the step-10 `last_plan: Option<DispatchPlan>` /
        // `last_operation: Option<Operation>` pair (and the post-loop
        // `plan_output_repr(registry, last_plan, last_operation)` chain)
        // with a single capture-and-write idiom. This closes the
        // backward-compat production gap pinned by
        // `execute_realization_ops_writes_produced_repr_brep_in_none_fallback_backward_compat`:
        // the old write guard `if let (Some(plan), Some(op)) =
        // (last_plan.as_ref(), last_operation)` short-circuited in the
        // sentinel-gated None-fallback arm because that arm never set
        // `last_plan`, leaving `produced_repr_out` unwritten on the
        // `Engine::new(_, Some(kernel))` construction path whenever the
        // inventory registry lacks coverage for the caller-supplied kernel
        // (the v0.2 backward-compat baseline, which deliberately keeps the
        // caller's kernel out of `inventory::submit!`). The new channel is
        // set in BOTH success paths inside the per-op loop:
        //
        // (a) the `Some(plan) if plan.conversions.is_empty()` arm —
        //     `plan_output_repr(registry, plan, operation)` from the
        //     dispatcher-named kernel's descriptor (the step-10 derivation,
        //     now computed inside the match arm where `plan` is borrowed);
        // (b) the `None` backward-compat fallback arm (`default_kernel_name
        //     == Engine::DEFAULT_KERNEL_NAME &&
        //     kernels.contains_key(default_kernel_name)`) —
        //     `Some(ReprKind::BRep)` directly, the v0.2 single-kernel-path
        //     invariant (the synthetic default kernel's terminal handle is
        //     always BRep in the BRep baseline; no descriptor is available
        //     in the inventory registry for the caller-supplied kernel, so
        //     `plan_output_repr` is not applicable here).
        //
        // The `Some(_) =>` non-empty-conversion arm and the strict-mode
        // `None` arm break the loop before this channel is read, so they
        // leave it at the default `None` — the post-loop write then
        // short-circuits as before, preserving the rollback-untouched
        // contract.
        let mut last_produced_repr: Option<ReprKind> = None;
        // Captures the per-op `GeometryOp`s in lockstep with `step_handles`
        // for this realization. After the loop, if the realization succeeds
        // (no rollback), the parallel `(realization_ops, step_handles[handle_start..])`
        // pair is fed to `classify_swept_body` for Phase A swept-body
        // classification (task 2982). Cleared on rollback alongside
        // `step_handles.truncate(handle_start)` below.
        //
        // Pre-sized to `operations.len()` so `Vec` growth never reallocates on
        // the build hot path. Each successful op contributes exactly one entry,
        // so this is the upper bound on capacity needed.
        let mut realization_ops: Vec<GeometryOp> = Vec::with_capacity(operations.len());
        // `realization_step_ids` mirrors `step_handles[handle_start..]`:
        // every `step_handles.push(...)` below pushes the same `.id` here, so
        // the slice stays in lockstep without re-projecting per op.
        let mut realization_step_ids: Vec<GeometryHandleId> = Vec::with_capacity(operations.len());
        // Task #4636: handles that `record_solid_attribute` (LINK2) wrote a
        // per-solid representative entry for, below, PLUS the forwarded
        // TARGET handles `forward_solid_attribute_on_ingest` (LINK1) writes
        // at the OCCT->Manifold ingest seam. Both are internal
        // cross-kernel-forwarding bookkeeping (consumed by
        // `ManifoldKernel::propagate_attributes` and the OCCT->Manifold
        // ingest forwarder), not a user-selectable face/edge/vertex
        // attribute — neither must participate in the post-loop centroid /
        // local-index-reassignment diagnostic scan
        // (`collect_centroids_with_failure_summary` /
        // `detect_local_index_reassignment_diagnostics` below), which walks
        // `topology_attribute_table` filtered by `feature_id` alone and has
        // no other way to tell a solid-representative/forwarded entry apart
        // from a real face entry. Tracked separately from
        // `realization_step_ids` (which mixes in non-seedable ops too) so
        // the filter below excludes exactly the entries this task added.
        //
        // Holds full `KernelHandle`s (kernel + id), not bare
        // `GeometryHandleId`s, so the scan filter below matches on the full
        // key. A bare-id exclusion set would risk excluding a real
        // `{Occt, X}` face entry that happens to share its id with a
        // forwarded `{Manifold, X}` entry — the cross-kernel id-collision
        // class this task's ingest-forwarding path introduces.
        //
        // A `HashSet` (not `Vec`): `KernelHandle` already derives `Hash` +
        // `Eq`, so membership below (`.contains(kernel_handle)`, on the
        // scan's per-entry hot path) is O(1) instead of a linear scan — cheap
        // to get right while the code is fresh, per reviewer_comprehensive's
        // amendment note, even though S (a few solids/realization) keeps the
        // `Vec` cost negligible at today's scale.
        let mut solid_attribute_handles: HashSet<KernelHandle> = HashSet::new();
        // Task 4050 step-8: the produced [`ReprKind`] of each step handle,
        // tracked in lockstep with `realization_step_ids`. The per-op
        // `available` set is read from the reprs of the op's resolved input
        // handles (via this Vec); every push site below pushes here too so the
        // two Vecs stay index-aligned.
        let mut realization_step_reprs: Vec<ReprKind> = Vec::with_capacity(operations.len());
        // Task 4050 step-12: per-realization log of intermediate-cache keys the
        // conversion executor inserted, so step-14's rollback branch can drop
        // exactly those keys (atomic with `step_handles.truncate(handle_start)`).
        // Each entry is `(entity, repr, per_stage_tol, options_hash)`. The
        // options_hash is `NO_OPTIONS` for a Tessellate-sourced intermediate and
        // `surface_options_content_hash(iso, adaptive)` for a MarchingCubes-sourced
        // one (task γ / 5001) — rollback must remove the EXACT key Phase 2
        // inserted, so the log carries whichever hash was used. On the success
        // path the inserts stay committed so later same-build realizations reuse
        // them.
        let mut intermediate_cache_inserts: Vec<(String, ReprKind, f64, reify_core::ContentHash)> =
            Vec::new();
        // Task #3443 (S6): track whether the KernelPragmaUnsatisfiable warning
        // has already been emitted for this realization. The pragma is
        // module-scoped and applies uniformly to all ops; emitting once per
        // realization (on the first unsatisfiable op) avoids spamming the
        // author with one warning per op when the whole realization shares
        // the same unsatisfiable preference (PRD §5 "warning, not error").
        let mut pragma_warn_emitted = false;
        // GR-034 (task #3445): accumulate the plan with the most conversion
        // stages seen across all ops in this realization. The per-op `plan`
        // local is dropped at the end of each loop iteration, so the longest
        // plan is captured by clone into this Option. Used after the loop to
        // emit the at-most-one LongChainRealization diagnostic.
        let mut longest_chain_plan: Option<DispatchPlan> = None;
        for (op_idx, op) in operations.iter().enumerate() {
            let geom_op = compile_geometry_op(
                op,
                values,
                &realization_step_ids,
                functions,
                meta_map,
                named_steps,
                diagnostics,
            );
            match geom_op {
                Ok(mut geom_op) => {
                    // Step-8 (task ε / 3436): per-op dispatch routing.
                    // Map the compiled `GeometryOp` to its `Operation`
                    // classifier and ask the dispatcher for a plan.
                    let operation = geometry_op_to_operation(&geom_op);
                    // Task 4050 step-8: derive the per-op `available` set from
                    // the reprs of this op's resolved input handles. Each parent
                    // handle id is looked up in `realization_step_ids` (this
                    // realization's step handles) to read its produced repr from
                    // the lockstep `realization_step_reprs`; parents from other
                    // realizations (named_steps) or parent-less ops are absent,
                    // so the set defaults to `{BRep}` (design_decision 6).
                    let available_for_op: HashSet<ReprKind> = {
                        let parents = parent_handles_for_op(&geom_op);
                        let mut set: HashSet<ReprKind> = parents
                            .as_slice()
                            .iter()
                            .filter_map(|pid| {
                                realization_step_ids
                                    .iter()
                                    .position(|id| id == pid)
                                    .map(|idx| realization_step_reprs[idx])
                            })
                            .collect();
                        // Task 5033 Gap #2 Gap A: the lookup above is blind to
                        // a CROSS-realization parent (a `GeomRef::Sub(name)`
                        // naming a DIFFERENT, already-completed realization —
                        // e.g. "solid" in `let shell = isosurface(solid)`).
                        // Its producer handle lives in THAT realization's
                        // now-out-of-scope `realization_step_ids`, not this
                        // one's, so the filter_map above always misses it.
                        // Resolve it by NAME instead — never by matching the
                        // bare `GeometryHandleId` across realizations, which
                        // risks a same-integer collision (#4349: a
                        // `GeometryHandleId` is only unique within its own
                        // kernel's handle space) — via `named_step_reprs`,
                        // the by-name sibling of `named_steps` populated at
                        // this realization loop's two insertion points.
                        //
                        // Amendment (review): only DO this resolution for ops
                        // that cannot accept the `{BRep}` default at all
                        // (`op_is_voxel_only_input` — today exactly
                        // `Operation::Surface`, the isosurface builtin, per
                        // PRD OQ-1). For every other op the pre-existing
                        // `{BRep}` default (design_decision 6) remains
                        // exactly as before: it is always a member of a
                        // BRep/Mesh-accepting op's input set, so surfacing
                        // the producer's true repr here would only ever
                        // change *which* available entry the dispatcher
                        // picks among several acceptable ones — a kernel-
                        // selection shift with no existing regression
                        // coverage for non-isosurface multi-realization
                        // pipelines. Gating keeps this resolution scoped to
                        // the one op family that actually needs it.
                        if op_is_voxel_only_input(&operation) {
                            for name in sub_refs_in_op(op) {
                                if let Some(&repr) = named_step_reprs.get(name) {
                                    set.insert(repr);
                                }
                            }
                        }
                        if set.is_empty() {
                            set.insert(ReprKind::BRep);
                        }
                        set
                    };
                    // Task ε (3436 / 4741): bump BOTH dispatch-attribution
                    // counters EXACTLY at the `dispatch(...)` call site so the
                    // cache-hit short-circuit (which returns above without ever
                    // entering this loop) leaves them at 0. Bumped once at the
                    // primary dispatch; the design_decision-3 fallback
                    // re-dispatch below does not bump again. Routed through the
                    // single `bump_dispatch` helper (the mirror of
                    // `reset_dispatch_tallies`) so the aggregate and the
                    // per-realization map can never be incremented independently
                    // — `sum(map) == aggregate` is exact-by-construction.
                    // `realization_id` is in scope here as a `&RealizationNodeId`.
                    Self::bump_dispatch(
                        dispatch_count,
                        dispatch_count_by_realization,
                        realization_id,
                    );
                    // Task 4050 step-8: dispatch at `demanded_repr`, then FALL
                    // BACK to a BRep dispatch when the demand is unsatisfiable
                    // and `demanded_repr != BRep` (design_decision 3). Without
                    // this, every Stl/Obj-terminal Mesh demand with no linked
                    // Mesh kernel would hit the strict no-kernel-chain error arm
                    // and regress the whole suite; with it, such ops route BRep
                    // exactly as the v0.2 baseline did.
                    let plan = dispatch(
                        registry,
                        operation,
                        demanded_repr,
                        &available_for_op,
                        prefer_kernel,
                    )
                    .or_else(|| {
                        if demanded_repr != ReprKind::BRep {
                            // BRep fallback (design_decision 3): pragma preference
                            // is not forwarded here because the fallback fires only
                            // when the preferred repr is unsatisfiable — passing
                            // prefer_kernel on the fallback path would silently pick
                            // the pragma kernel at BRep demand even when the user's
                            // #kernel(X) intent was for the primary demanded repr.
                            dispatch(
                                registry,
                                operation,
                                ReprKind::BRep,
                                &available_for_op,
                                None,
                            )
                        } else {
                            None
                        }
                    });
                    // GR-034 (task #3445): update longest_chain_plan when this
                    // op's dispatch plan has more conversion stages than any
                    // seen so far. DispatchPlan derives Clone (dispatcher.rs
                    // :639); the owned local `plan` is dropped at the end of
                    // this loop iteration (documented at engine_build.rs:5982),
                    // so the capture requires a clone.
                    if let Some(ref p) = plan
                        && longest_chain_plan
                            .as_ref()
                            .is_none_or(|lc| p.conversions.len() > lc.conversions.len())
                    {
                        longest_chain_plan = Some(p.clone());
                    }
                    // Task #3443 (S6 amend): emit KernelPragmaUnsatisfiable
                    // warning keyed on the actual routing result — when
                    // prefer_kernel is Some(name) but the dispatch resolved a
                    // different kernel, the pragma was not honoured for this op.
                    // This avoids spurious warnings on intermediate ops in
                    // non-BRep-terminal realizations where the primary dispatch
                    // returns None (no kernel supports the demanded repr) and the
                    // BRep fallback above picks lex-min without forwarding
                    // prefer_kernel. `pragma_warn_emitted` deduplicates the
                    // warning across ops in the same realization
                    // (PRD §5 "warning, not error").
                    if let Some(name) = prefer_kernel
                        && !pragma_warn_emitted
                        && let Some(ref p) = plan
                        && p.kernel != name
                    {
                        diagnostics.push(
                            crate::dispatcher::kernel_pragma_unsatisfiable_diagnostic(
                                name,
                                operation,
                                demanded_repr,
                            ),
                        );
                        pragma_warn_emitted = true;
                    }
                    // Step-14 (task ε / 3436): the match returns a
                    // `(resolved_kernel_name, op_produced_repr)` tuple — a
                    // single source of truth that yokes the routing decision
                    // to the per-op output repr capture. Borrows `plan` here
                    // (`match &plan`) rather than moving it; the owned `plan`
                    // is dropped at the end of this loop iteration. The
                    // per-op `op_produced_repr` value is propagated into
                    // `last_produced_repr` after the successful kernel call
                    // below so the post-loop write sees the terminal op's
                    // repr (mirroring how `step_handles.push(handle.id)`
                    // tracks the terminal handle).
                    let (resolved_kernel_name, op_produced_repr): (String, Option<ReprKind>) =
                        match &plan {
                            Some(plan) if plan.conversions.is_empty() => {
                                // 0-conversion plan: route to plan.kernel,
                                // falling back to the engine's default kernel if
                                // the dispatcher named an entry not present in
                                // the kernels map (defence against
                                // dispatch/registry-vs-map drift; in practice the
                                // builder always loads one adapter per registry
                                // entry so the fallback is dormant).
                                //
                                // **Amendment round 2 (suggestion #3)**: also
                                // gate the default-fallback on
                                // `contains_key(default_kernel_name)` so the
                                // subsequent `.expect(...)` on `kernels.get_mut`
                                // is structurally honest. Without this gate a
                                // hypothetical caller that bypasses the entry-
                                // point `contains_key` check (build /
                                // build_snapshot / tessellate_from_values all gate
                                // there today) could land on a missing default
                                // and surface a confusing internal error several
                                // lines downstream. Mirrors the parallel
                                // `contains_key` gate in the `None` arm below and
                                // the post-loop `.expect` idiom at
                                // engine_build.rs:967 / :2626.
                                let name = if kernels.contains_key(plan.kernel.as_str()) {
                                    plan.kernel.clone()
                                } else if kernels.contains_key(default_kernel_name) {
                                    default_kernel_name.to_string()
                                } else {
                                    let err_msg = format!(
                                        "internal error: dispatcher named kernel '{}' \
                                     not present in engine.geometry_kernels; default \
                                     '{default_kernel_name}' also absent",
                                        plan.kernel,
                                    );
                                    diagnostics.push(
                                        Diagnostic::error(err_msg.clone()).with_label(
                                            DiagnosticLabel::new(
                                                realization_span,
                                                "in this realization",
                                            ),
                                        ),
                                    );
                                    if kernel_error_out.is_none() {
                                        *kernel_error_out = Some(ErrorRef::new(err_msg));
                                    }
                                    break;
                                };
                                // Step-14 (task ε / 3436): derive the per-op
                                // output repr from the dispatcher-named kernel's
                                // descriptor — the step-10 `plan_output_repr`
                                // derivation, now computed inline alongside the
                                // routing decision so both flow through the
                                // single capture-and-write idiom below. May
                                // return `None` if the named kernel's descriptor
                                // has no entry for `op` (an invariant violation
                                // that surfaces as "leave produced_repr_out
                                // untouched" rather than fabricating a repr).
                                (name, plan_output_repr(registry, plan, operation))
                            }
                            Some(plan) => {
                                // Task 4422 step-4: restructured MULTI-STAGE
                                // CONVERSION EXECUTOR. A non-empty `plan.conversions`
                                // chain names the repr crossings to perform before the
                                // final op runs on `plan.kernel`. The recipe is:
                                //
                                //   BRep→Mesh (tessellate on source kernel) +
                                //   Mesh→Voxel-or-Mesh (ingest_mesh on plan.kernel)
                                //
                                // run EXACTLY ONCE per op-input parent for the whole
                                // chain regardless of stage count. Mesh is the
                                // universal interchange: the final ingest into
                                // plan.kernel realises Mesh→Mesh (Manifold) or
                                // Mesh→Voxel (OpenVDB) depending on plan.kernel.
                                //
                                // Phase 1 validates every stage via
                                // `v03_conversion_projection`: an unknown crossing
                                // surfaces as a realization-failed diagnostic rather
                                // than a panic. Phase 2 executes the single
                                // tessellate+ingest recipe per parent, keying the
                                // intermediate cache at the chain's terminal `to`.
                                // This reduces to the prior behaviour for the 1-stage
                                // BRep→Mesh chain, so cross_kernel_handoff and all
                                // inline conversion-path/caching/rollback tests stay
                                // GREEN. (Intermediate caching: step-12; rollback:
                                // step-14.)

                                // The target kernel must be present in the map.
                                if !kernels.contains_key(plan.kernel.as_str()) {
                                    let err_msg = format!(
                                        "internal error: dispatcher named target kernel '{}' \
                                     not present in engine.geometry_kernels for a \
                                     conversion plan",
                                        plan.kernel,
                                    );
                                    diagnostics.push(
                                        Diagnostic::error(err_msg.clone()).with_label(
                                            DiagnosticLabel::new(
                                                realization_span,
                                                "in this realization",
                                            ),
                                        ),
                                    );
                                    if kernel_error_out.is_none() {
                                        *kernel_error_out = Some(ErrorRef::new(err_msg));
                                    }
                                    break;
                                }

                                // Tessellation tolerance for the BRep→Mesh source
                                // projection (default-tess tolerance when the caller
                                // threaded no demanded tolerance).
                                let per_stage_tol = per_stage_tolerance_for_plan(
                                    plan,
                                    demanded_tol.unwrap_or(Engine::DEFAULT_TESSELLATION_TOLERANCE),
                                );

                                // Snapshot the op's input handles before mutation.
                                let parents: Vec<GeometryHandleId> =
                                    parent_handles_for_op(&geom_op).as_slice().to_vec();

                                let mut substitution: HashMap<GeometryHandleId, GeometryHandleId> =
                                    HashMap::new();
                                let mut conversion_error: Option<String> = None;

                                // ── Phase 1: validate stages + find source ────────
                                // Walk the chain as a VALIDATION gate. Each stage
                                // must classify as a known ConversionProjection:
                                // - Tessellate: records the source kernel name (the
                                //   kernel that tessellates BRep → Mesh).
                                // - Voxelize: realised by ingest_mesh on plan.kernel
                                //   below; no separate action needed here.
                                // Unknown stage → graceful degradation.
                                // Contiguity is also validated: each stage's `from`
                                // must equal the prior stage's `to`.  Out-of-order
                                // chains (e.g. Mesh→Voxel before BRep→Mesh) would
                                // silently mis-key the intermediate cache under the
                                // single-recipe executor.
                                let mut tessellate_source: Option<&'static str> = None;
                                // Task γ (5001): the MarchingCubes counterpart of
                                // `tessellate_source` above — records the source
                                // kernel of an at-most-one Voxel→Mesh stage. Phase 2
                                // below picks whichever of the two is `Some` (the
                                // gate after this loop guarantees exactly one is).
                                let mut marching_cubes_source: Option<&'static str> = None;
                                // prev_to tracks the prior stage's output repr for
                                // the contiguity check below.
                                let mut prev_to: Option<ReprKind> = None;
                                // Terminal `to` drives the intermediate cache key
                                // (Mesh for 1-stage BRep→Mesh, Voxel for 2-stage).
                                // Safe: this arm is only reached for non-empty chains.
                                let terminal_to = plan
                                    .conversions
                                    .last()
                                    .map(|(_, _, to)| *to)
                                    .unwrap_or(ReprKind::Mesh);
                                for (stage_kernel, from, to) in &plan.conversions {
                                    // Contiguity assertion: each stage's `from` must
                                    // equal the prior stage's `to`.  Detects
                                    // out-of-order chains a future dispatcher change
                                    // could accidentally produce.
                                    if let Some(expected) = prev_to
                                        && *from != expected
                                    {
                                        conversion_error = Some(format!(
                                            "internal error: conversion chain for op \
                                             '{operation:?}' is non-contiguous: stage \
                                             {from:?}→{to:?} follows a stage that \
                                             produced {expected:?}; chain must be ordered \
                                             (e.g. BRep→Mesh then Mesh→Voxel)",
                                        ));
                                        break;
                                    }
                                    prev_to = Some(*to);

                                    use crate::dispatcher::{
                                        ConversionProjection, v03_conversion_projection,
                                    };
                                    match v03_conversion_projection(*from, *to) {
                                        None => {
                                            conversion_error = Some(format!(
                                                "conversion stage {from:?}→{to:?} for op \
                                                 '{operation:?}' is not executable in v0.3-β \
                                                 (supported: BRep→Mesh, Mesh→Voxel)",
                                            ));
                                            break;
                                        }
                                        Some(ConversionProjection::Tessellate) => {
                                            // Guard: a chain may contain AT MOST one
                                            // BRep→Mesh Tessellate stage.  Two
                                            // Tessellate stages would mean two distinct
                                            // source kernels, which the single-recipe
                                            // executor cannot represent — surface it as
                                            // a graceful diagnostic rather than
                                            // silently using the last one seen.
                                            if tessellate_source.is_some() {
                                                conversion_error = Some(format!(
                                                    "conversion chain for op '{operation:?}' \
                                                     has more than one Tessellate stage \
                                                     (BRep→Mesh); only one is supported \
                                                     in v0.3-β",
                                                ));
                                            } else {
                                                tessellate_source =
                                                    Some((*stage_kernel).as_registry_name());
                                            }
                                        }
                                        Some(ConversionProjection::Voxelize) => {
                                            // Realised by ingest_mesh on plan.kernel in
                                            // phase 2.  Guard: the Voxelize stage's
                                            // recorded kernel must match plan.kernel —
                                            // the executor always ingests into
                                            // plan.kernel, so a mismatch would ingest
                                            // into the wrong kernel silently.
                                            if stage_kernel.as_registry_name()
                                                != plan.kernel.as_str()
                                            {
                                                conversion_error = Some(format!(
                                                    "internal error: Voxelize stage kernel \
                                                     '{}' does not match plan.kernel '{}' \
                                                     for op '{operation:?}'; executor would \
                                                     ingest into the wrong kernel",
                                                    stage_kernel.as_registry_name(),
                                                    plan.kernel,
                                                ));
                                            }
                                        }
                                        Some(ConversionProjection::MarchingCubes) => {
                                            // Guard: a chain may contain AT MOST one
                                            // Voxel→Mesh MarchingCubes stage, mirroring
                                            // the Tessellate guard above — two
                                            // MarchingCubes stages would mean two
                                            // distinct source kernels, which the
                                            // single-recipe executor cannot represent.
                                            if marching_cubes_source.is_some() {
                                                conversion_error = Some(format!(
                                                    "conversion chain for op '{operation:?}' \
                                                     has more than one MarchingCubes stage \
                                                     (Voxel→Mesh); only one is supported \
                                                     in v0.3-γ",
                                                ));
                                            } else {
                                                marching_cubes_source =
                                                    Some((*stage_kernel).as_registry_name());
                                            }
                                        }
                                    }
                                }
                                // Task γ (5001): relax the "must have a Tessellate
                                // source" gate to "exactly one mesh-source stage —
                                // Tessellate (BRep→Mesh) OR MarchingCubes
                                // (Voxel→Mesh)". Neither present degrades exactly as
                                // before (now naming both supported source kinds in
                                // the message); BOTH present (a mixed
                                // BRep→Mesh→Voxel→Mesh chain, task ρ) is not yet
                                // representable by this single-recipe executor, so it
                                // also degrades gracefully rather than silently
                                // picking one source over the other.
                                if conversion_error.is_none() {
                                    match (tessellate_source, marching_cubes_source) {
                                        (None, None) => {
                                            conversion_error = Some(format!(
                                                "internal error: conversion chain for op \
                                                 '{operation:?}' has no mesh-source stage \
                                                 (no BRep→Mesh Tessellate or Voxel→Mesh \
                                                 MarchingCubes source kernel found in \
                                                 plan.conversions)"
                                            ));
                                        }
                                        (Some(_), Some(_)) => {
                                            conversion_error = Some(format!(
                                                "conversion chain for op '{operation:?}' has \
                                                 both a Tessellate (BRep→Mesh) and a \
                                                 MarchingCubes (Voxel→Mesh) source stage; \
                                                 mixed chains are not supported in v0.3-γ",
                                            ));
                                        }
                                        _ => {}
                                    }
                                }

                                // ── Phase 2: produce the interchange Mesh + ingest
                                // once per parent ──
                                // For each parent: produce a Mesh on the mesh-source
                                // kernel — `tessellate` for a Tessellate (BRep→Mesh)
                                // source, or `realize_mesh_from_voxel` for a
                                // MarchingCubes (Voxel→Mesh) source (task γ / 5001) —
                                // then ingest the Mesh into plan.kernel → fresh
                                // handle. The ingest call voxelises when plan.kernel
                                // is an OpenVDB kernel (Mesh→Voxel) and is a trivial
                                // Mesh→Mesh pass-through when plan.kernel is a
                                // Manifold/similar kernel.
                                if conversion_error.is_none() {
                                    // Phase 1's gate above guarantees exactly one of
                                    // these is `Some`.
                                    let (source_name, from_marching_cubes) =
                                        match (tessellate_source, marching_cubes_source) {
                                            (Some(name), None) => (name, false),
                                            (None, Some(name)) => (name, true),
                                            _ => unreachable!(
                                                "checked above: exactly one of \
                                                 tessellate_source/marching_cubes_source \
                                                 is Some"
                                            ),
                                        };
                                    // Task γ (5001): extract the marching-cubes
                                    // options from `geom_op` when it is the
                                    // options-carrying `GeometryOp::Surface` (task
                                    // 4999); otherwise default to
                                    // `MarchingCubesOptions::default()`'s equivalents
                                    // (0.0, false) — a MarchingCubes stage reached
                                    // under a non-Surface terminal still needs a
                                    // non-sentinel cache key (design decision 3).
                                    let (surface_iso_level, surface_adaptive) = match &geom_op {
                                        GeometryOp::Surface {
                                            iso_level,
                                            adaptive,
                                            ..
                                        } => (*iso_level, *adaptive),
                                        _ => (0.0, false),
                                    };
                                    // The intermediate-cache options key: NO_OPTIONS
                                    // for a Tessellate source (unchanged), or the
                                    // source kernel's own
                                    // `surface_options_content_hash` for a
                                    // MarchingCubes source — the single source of
                                    // truth for the hash (design decision 2), never
                                    // re-derived here. Resolved once outside the
                                    // per-parent loop since it does not depend on
                                    // `pid`.
                                    let options_hash = if from_marching_cubes {
                                        kernels.get(source_name).map_or(NO_OPTIONS, |src| {
                                            src.surface_options_content_hash(
                                                surface_iso_level,
                                                surface_adaptive,
                                            )
                                        })
                                    } else {
                                        NO_OPTIONS
                                    };
                                    'convert: for &pid in &parents {
                                        // Task 4050 step-12: the intermediate cache
                                        // key for THIS input — distinct per input
                                        // (stable across rebuilds; see
                                        // `conversion_intermediate_entity_id`).
                                        let intermediate_entity = conversion_intermediate_entity_id(
                                            &realization_id.entity,
                                            pid,
                                            &realization_step_ids,
                                        );
                                        // Consult the cache BEFORE any kernel work. A
                                        // hit returns the previously-ingested
                                        // target-kernel handle (Copy); reuse its id
                                        // and skip the redundant production+ingest.
                                        if let Some(&cached) = realization_cache.lookup(
                                            &intermediate_entity,
                                            terminal_to,
                                            per_stage_tol,
                                            options_hash,
                                        ) {
                                            // Task #4636 (LINK1): topology_attribute_table
                                            // is per-build, unlike realization_cache (which
                                            // persists the intermediate handle itself
                                            // across builds) — so a cache hit still needs
                                            // to (re-)forward the source solid's attribute
                                            // onto the reused target handle, or this
                                            // build's table would have no entry at all for
                                            // it despite the handle being valid.
                                            if forward_solid_attribute_on_ingest(
                                                topology_attribute_table,
                                                KernelHandle {
                                                    kernel: kernel_id_for_registry_name(source_name),
                                                    id: pid,
                                                },
                                                cached,
                                            ) {
                                                // Task #4636 step-9: exclude the forwarded
                                                // TARGET handle from the post-loop diagnostic
                                                // scan too (see `solid_attribute_handles`
                                                // declaration comment above).
                                                solid_attribute_handles.insert(cached);
                                            }
                                            substitution.insert(pid, cached.id);
                                            continue;
                                        }
                                        // Cache miss: produce the interchange Mesh on
                                        // the mesh-source kernel (`&self`); borrow
                                        // released before the `&mut` ingest borrow
                                        // below. Tessellate and MarchingCubes return
                                        // different error types (`TessError` /
                                        // `GeometryError`), so each arm maps its error
                                        // to `String` before the two branches unify.
                                        let mesh = match kernels.get(source_name) {
                                            Some(src) => {
                                                let produced = if from_marching_cubes {
                                                    src.realize_mesh_from_voxel(
                                                        pid,
                                                        surface_iso_level,
                                                        surface_adaptive,
                                                    )
                                                    .map_err(|e| {
                                                        format!("voxel surfacing error: {e}")
                                                    })
                                                } else {
                                                    src.tessellate(pid, per_stage_tol).map_err(
                                                        |e| format!("tessellation error: {e}"),
                                                    )
                                                };
                                                match produced {
                                                    Ok(mesh) => mesh,
                                                    Err(msg) => {
                                                        conversion_error = Some(msg);
                                                        break 'convert;
                                                    }
                                                }
                                            }
                                            None => {
                                                conversion_error = Some(format!(
                                                    "internal error: conversion source kernel \
                                                 '{source_name}' absent from \
                                                 engine.geometry_kernels"
                                                ));
                                                break 'convert;
                                            }
                                        };
                                        // Task #5103 (kernel-seam β, INV-GEO-1): check the
                                        // interchange Mesh against the mesh contract
                                        // (PRD docs/prds/kernel-seam-contracts.md §4 site 1)
                                        // right after it's produced by the Tessellate
                                        // (BRep→Mesh) source. WARN-default during rollout —
                                        // on a violation, push a Severity::Warning diagnostic
                                        // and fall through to ingest as normal; never abort
                                        // the build here. The fail-closed enforce flip
                                        // (reading a policy env var and aborting instead) is
                                        // task δ's scope, not β's. Scoped to the tessellate
                                        // producer only — the MarchingCubes (Voxel→Mesh)
                                        // producer is task γ's domain and is backstopped by
                                        // PRD site 2 (Manifold-ingest validation).
                                        if !from_marching_cubes
                                            && let Err(violation) =
                                                mesh.validate(per_stage_tol)
                                        {
                                            diagnostics.push(
                                                Diagnostic::warning(
                                                    violation
                                                        .into_geometry_error(source_name)
                                                        .to_string(),
                                                )
                                                .with_label(DiagnosticLabel::new(
                                                    realization_span,
                                                    "in this realization",
                                                )),
                                            );
                                        }
                                        // Ingest into the target kernel (`&mut`).
                                        // For a Manifold kernel this is Mesh→Mesh;
                                        // for an OpenVDB kernel this is normally
                                        // Mesh→Voxel — EXCEPT when this stage's
                                        // mesh IS the final op's own desired
                                        // output (task 5033 Gap #2/#3): a
                                        // MarchingCubes source feeding a
                                        // `Surface` terminal anchor already
                                        // produced exactly the mesh that op
                                        // wants, so re-voxelizing it via
                                        // `ingest_mesh` would be lossy and
                                        // pointless. That one case calls
                                        // `register_mesh_handle` instead, which
                                        // stores the mesh honestly as Mesh-repr
                                        // without re-deriving it.
                                        let target_kernel = kernels
                                            .get_mut(plan.kernel.as_str())
                                            .expect("plan.kernel presence checked above");
                                        let ingested = if from_marching_cubes
                                            && matches!(geom_op, GeometryOp::Surface { .. })
                                        {
                                            target_kernel.register_mesh_handle(&mesh)
                                        } else {
                                            target_kernel.ingest_mesh(&mesh)
                                        };
                                        match ingested {
                                            Ok(handle) => {
                                                // Wrap the fresh target-kernel handle
                                                // with its KernelId provenance, cache
                                                // it for cross-realization reuse, and
                                                // log the key for step-14's atomic
                                                // rollback.
                                                let intermediate_handle = KernelHandle {
                                                    kernel: kernel_id_for_registry_name(
                                                        plan.kernel.as_str(),
                                                    ),
                                                    id: handle.id,
                                                };
                                                realization_cache.insert(
                                                    &intermediate_entity,
                                                    terminal_to,
                                                    per_stage_tol,
                                                    options_hash,
                                                    intermediate_handle,
                                                );
                                                intermediate_cache_inserts.push((
                                                    intermediate_entity,
                                                    terminal_to,
                                                    per_stage_tol,
                                                    options_hash,
                                                ));
                                                // Task #4636 (LINK1): forward the source
                                                // solid's attribute (recorded by
                                                // record_solid_attribute at the seed site
                                                // above) across the ingest seam onto the
                                                // fresh target handle. A no-op when the
                                                // source was never seeded (e.g. a
                                                // non-primitive parent), degrading
                                                // gracefully to the existing Discarded path.
                                                if forward_solid_attribute_on_ingest(
                                                    topology_attribute_table,
                                                    KernelHandle {
                                                        kernel: kernel_id_for_registry_name(source_name),
                                                        id: pid,
                                                    },
                                                    intermediate_handle,
                                                ) {
                                                    // Task #4636 step-9: exclude the
                                                    // forwarded TARGET handle from the
                                                    // post-loop diagnostic scan too (see
                                                    // `solid_attribute_handles` declaration
                                                    // comment above).
                                                    solid_attribute_handles.insert(intermediate_handle);
                                                }
                                                substitution.insert(pid, handle.id);
                                            }
                                            Err(e) => {
                                                conversion_error =
                                                    Some(format!("mesh ingest error: {e}"));
                                                break 'convert;
                                            }
                                        }
                                    }
                                }
                                if let Some(err_msg) = conversion_error {
                                    diagnostics.push(
                                        Diagnostic::error(err_msg.clone()).with_label(
                                            DiagnosticLabel::new(
                                                realization_span,
                                                "in this realization",
                                            ),
                                        ),
                                    );
                                    if kernel_error_out.is_none() {
                                        *kernel_error_out = Some(ErrorRef::new(err_msg));
                                    }
                                    break;
                                }

                                // Point the final op at the converted handles and
                                // route it to the target kernel via the common
                                // execute path. `plan_output_repr` of the final op on
                                // `plan.kernel` becomes this op's produced repr
                                // (Mesh for Manifold, Voxel for OpenVDB).
                                substitute_op_parents(&mut geom_op, &substitution);
                                (
                                    plan.kernel.clone(),
                                    plan_output_repr(registry, plan, operation),
                                )
                            }
                            None => {
                                // dispatch returned None: no registered kernel
                                // claims `(op, BRep)` in the inventory-derived
                                // registry. Two cases:
                                //
                                // (a) Backward-compat mode — the engine was
                                //     constructed via `Engine::new(_, Some(k))` /
                                //     `with_prelude(_, Some(k), _)`, which wraps
                                //     the caller-supplied kernel under the
                                //     synthetic [`Engine::DEFAULT_KERNEL_NAME`]
                                //     sentinel. The inventory registry is
                                //     deliberately out of sync with the kernels
                                //     map in this mode (the caller's kernel
                                //     never submits to `inventory::submit!`).
                                //     For runtime behaviour to remain identical
                                //     to v0.2 in this path, fall back to the
                                //     default kernel — exactly as we already do
                                //     in the `Some(plan)` branch when the
                                //     dispatched name is absent from the kernels
                                //     map. Without this fallback, every
                                //     `Engine::new(Some(MockGeometryKernel))`
                                //     integration test that doesn't transitively
                                //     pull in an inventory-registered adapter
                                //     would regress to "no kernel chain" errors.
                                //
                                // (b) Strict mode — the engine was constructed
                                //     via `with_registered_kernels` (or the test
                                //     drives `execute_realization_ops` with a
                                //     non-synthetic `default_kernel_name`).
                                //     Emit the `NoKernelChain` diagnostic so the
                                //     missing-coverage configuration is surfaced
                                //     rather than silently masked.
                                //
                                // The sentinel comparison distinguishes the two
                                // paths without adding a separate flag — the
                                // name `"__reify_eval_default_kernel"` is chosen
                                // to be impossible for any real inventory
                                // registration (`"occt"`, `"manifold"`, …).
                                if default_kernel_name == Engine::DEFAULT_KERNEL_NAME
                                    && kernels.contains_key(default_kernel_name)
                                {
                                    // Step-14 (task ε / 3436): backward-compat
                                    // fallback success — yokes the routing
                                    // decision (default kernel) to a synthetic
                                    // `Some(ReprKind::BRep)` capture. The
                                    // inventory registry has no descriptor for
                                    // the caller-supplied kernel (it never
                                    // submits to `inventory::submit!`), so
                                    // `plan_output_repr` is not applicable here;
                                    // the v0.2 single-kernel-path invariant
                                    // guarantees the synthetic default kernel's
                                    // terminal handle is always BRep in the BRep
                                    // baseline, so direct `Some(ReprKind::BRep)`
                                    // capture is honest and complete. This is
                                    // the production gap closure for
                                    // `executor_writes_produced_repr_brep_on_build_snapshot`
                                    // (the step-13 unit test pins the same gap
                                    // with a synthetic registry, build-profile-
                                    // independent).
                                    (default_kernel_name.to_string(), Some(ReprKind::BRep))
                                } else {
                                    // Task 4050 step-8: report the op's actual
                                    // available reprs (not the hoisted v0.2
                                    // `{BRep}` triple). Both the demanded dispatch
                                    // AND the BRep fallback returned None here, so
                                    // `BRep` is the accurate "could not satisfy"
                                    // demand to surface.
                                    let available_reprs: Vec<ReprKind> =
                                        available_for_op.iter().copied().collect();
                                    let diag = crate::dispatcher::no_kernel_chain_diagnostic(
                                        operation,
                                        ReprKind::BRep,
                                        &available_reprs,
                                    )
                                    .with_label(
                                        DiagnosticLabel::new(
                                            realization_span,
                                            "in this realization",
                                        ),
                                    );
                                    diagnostics.push(diag);
                                    if kernel_error_out.is_none() {
                                        *kernel_error_out = Some(ErrorRef::new(format!(
                                            "no kernel chain for op '{:?}' producing '{:?}'",
                                            operation,
                                            ReprKind::BRep,
                                        )));
                                    }
                                    break;
                                }
                            }
                        };
                    // Amendment round 2 (suggestion #3): the
                    // `resolved_kernel_name` match arms above each guarantee
                    // `kernels.contains_key(resolved_kernel_name)`:
                    //
                    // - 0-conversion arm: routes to `plan.kernel` only when
                    //   `contains_key(plan.kernel)`; falls back to
                    //   `default_kernel_name` only when
                    //   `contains_key(default_kernel_name)`; otherwise
                    //   `break`s the op loop with a diagnostic.
                    // - Non-empty-conversion arm: `break`s before reaching
                    //   here.
                    // - `None` arm (backward-compat): falls back to
                    //   `default_kernel_name` only when
                    //   `contains_key(default_kernel_name)`; otherwise
                    //   `break`s the op loop with a diagnostic.
                    //
                    // So the `.expect` below is honest: a panic here would
                    // imply a key was removed from `kernels` between the
                    // `contains_key` guard and this `get_mut`, which the
                    // executor never does. Mirrors the post-loop `.expect`
                    // idiom at engine_build.rs:967 / :2626 for the same
                    // invariant on the default kernel.
                    let kernel: &mut dyn GeometryKernel = kernels
                        .get_mut(resolved_kernel_name.as_str())
                        .expect(
                            "resolved_kernel_name is guaranteed to be a key in `kernels` by \
                             the preceding match arms (each gates its fallback on \
                             `contains_key`); the executor never removes entries from the map",
                        )
                        .as_mut();

                    match kernel.execute_with_history(&geom_op) {
                        Ok((handle, attribute_history)) => {
                            // Task 4351 step-4: the op kernel actually dispatched
                            // to for this op (identical expression to the one
                            // that tags `step_handles` below), reused for both
                            // write-path attribute-recording calls immediately
                            // below so a manifold-routed op records under
                            // `KernelHandle{Manifold, id}` and an occt-routed op
                            // under `KernelHandle{Occt, id}` — replacing the
                            // former interim single-kernel `KernelId::Occt`.
                            let op_kernel = kernel_id_for_registry_name(&resolved_kernel_name);
                            // v0.2 persistent-naming-v2 (PRD task 6, #2574): seed
                            // per-face/per-edge `TopologyAttribute` records for
                            // primitive constructors (Box / Cylinder / Sphere).
                            // Non-primitive variants are no-ops at zero kernel
                            // cost — `seed_primitive_attributes_for_handle` skips
                            // the extract_* calls entirely for them. A seeding
                            // failure (e.g. extract_faces / FaceNormal query
                            // error) emits a Warning diagnostic and continues:
                            // attribute seeding is auxiliary metadata, not
                            // primary geometry, so it must not regress the
                            // realization to Failed when only the metadata path
                            // breaks. Per-task design decision recorded in
                            // .task/plan.json.
                            let feature_id = FeatureId::from(realization_id);
                            let seed_result = seed_primitive_attributes_for_handle(
                                topology_attribute_table,
                                op_kernel,
                                kernel,
                                handle.id,
                                &feature_id,
                                &geom_op,
                            );
                            if let Err(e) = &seed_result {
                                diagnostics.push(Diagnostic::warning(format!(
                                "topology-attribute seeding failed for {realization_id} op {op_idx}: {e}"
                            )));
                            }
                            // Task #4636 (LINK2): on a successful seed of a
                            // seedable primitive, also record a per-solid
                            // representative entry — the seed call above only
                            // writes per-face/edge/vertex entries, never the
                            // solid handle itself, which is exactly what
                            // ManifoldKernel::propagate_attributes' parent_map
                            // lookup (and the OCCT->Manifold ingest forwarder
                            // below) needs. Gated on the same seedability
                            // check the seed function itself uses, and on
                            // `Ok` so a seeding failure never leaves a
                            // solid-level entry orphaned from its face/edge
                            // siblings.
                            if seed_result.is_ok() && is_seedable_primitive(&geom_op) {
                                record_solid_attribute(
                                    topology_attribute_table,
                                    op_kernel,
                                    handle.id,
                                    &feature_id,
                                );
                                solid_attribute_handles.insert(KernelHandle {
                                    kernel: op_kernel,
                                    id: handle.id,
                                });
                            }
                            // v0.2 persistent-naming-v2 (PRD task 5a, #2573): per-op
                            // attribute population for sweep ops (extrude / revolve).
                            // Mirrors the seeding warning idiom above — a failure
                            // here is auxiliary-metadata-only and must not regress
                            // the realization to Failed. Non-attributable ops
                            // return `AttributeHistory::None` from the default
                            // `GeometryKernel::execute_with_history` impl, so this
                            // match is a no-op for them.
                            if let Err(e) = populate_attribute_history(
                                topology_attribute_table,
                                op_kernel,
                                kernel,
                                &feature_id,
                                &geom_op,
                                handle.id,
                                &attribute_history,
                            ) {
                                diagnostics.push(Diagnostic::warning(format!(
                                "topology-attribute attribute history population failed for {realization_id} op {op_idx}: {e}"
                            )));
                            }
                            // task 4545: surface topology-correspondence-loss counters
                            // from the kernel history record as structured Warnings.
                            // Called immediately after `populate_attribute_history`
                            // (independent of its Result) so the warning is emitted
                            // even when population also warns. Severity::Warning only
                            // — geometry is valid, only persistent-naming tracking
                            // is degraded (task-2574 auxiliary-metadata convention).
                            diagnose_topology_correspondence_drops(
                                &attribute_history,
                                &format!("{realization_id} op {op_idx}"),
                                diagnostics,
                            );
                            // v0.2 persistent-naming-v2 (task 2875): kernel-attribute-hook
                            // propagation for non-BRep kernels.  Runs immediately after
                            // `populate_attribute_history` (BRep-first ordering per design
                            // decision: OCCT-native population writes first; the hook is the
                            // non-BRep path that returns `FellThrough` for OCCT shapes — a
                            // near-zero-cost no-op — and routes to `propagate_attributes` for
                            // kernels that advertise a hook).  Skipped entirely when
                            // `parent_handles_for_op` returns an empty slice (primitives,
                            // curve constructors, Pipe) so vacuous hook calls are never made.
                            //
                            // Mutual-exclusion contract: a kernel MUST NOT both return a
                            // non-`None` `AttributeHistory` from `execute_with_history` AND
                            // advertise an `attribute_hook()` for the same op.  The engine
                            // invokes both paths unconditionally for every parent-having op;
                            // if both populate the same `(feature_id, handle)` slots, the
                            // second write wins silently.  This contract is currently only
                            // enforced by convention: OCCT's `attribute_hook()` returns
                            // `None`, and Manifold's `execute_with_history` always returns
                            // `AttributeHistory::None` — the two paths are cleanly disjoint
                            // for all kernels that exist today.
                            let parent_handles = parent_handles_for_op(&geom_op);
                            if !parent_handles.is_empty() {
                                // All three Ok variants (Propagated / Discarded /
                                // FellThrough) are intentionally swallowed: the hook
                                // emits its own tracing::warn! on Discarded; the
                                // dispatcher emits tracing::debug! when the kernel does
                                // not advertise a hook (None → FellThrough); a hook that
                                // itself returns Ok(FellThrough) is passed through
                                // silently; and Propagated is the success case.  Only
                                // Err(QueryError) needs user-facing visibility (mirrors
                                // the populate_attribute_history failure idiom above and
                                // the task-2574 "auxiliary metadata MUST NOT regress
                                // Failed" convention).
                                if let Err(e) = crate::kernel_attribute_hook::propagate_via_kernel_attribute_hook(
                                &*kernel,
                                topology_attribute_table,
                                &geom_op,
                                parent_handles.as_slice(),
                                handle.id,
                                &feature_id,
                            ) {
                                diagnostics.push(Diagnostic::warning(format!(
                                    "kernel attribute hook propagation failed for {realization_id} op {op_idx}: {e}"
                                )));
                            }
                            }
                            // Task 4048: tag the produced handle with the
                            // executing kernel's KernelId (the dispatcher-resolved
                            // `resolved_kernel_name` for this op). `realization_step_ids`
                            // mirrors the bare `.id` for the GeomRef::Step slice.
                            step_handles.push(KernelHandle {
                                kernel: kernel_id_for_registry_name(&resolved_kernel_name),
                                id: handle.id,
                            });
                            realization_step_ids.push(handle.id);
                            // Task 4050 step-8: keep `realization_step_reprs` in
                            // lockstep with `realization_step_ids` — record this
                            // op's produced repr so downstream ops derive their
                            // `available` set from it. `op_produced_repr` may be
                            // `None` only when a descriptor lacks the op (an
                            // invariant violation); default to BRep so the
                            // available-set derivation stays total.
                            realization_step_reprs.push(op_produced_repr.unwrap_or(ReprKind::BRep));
                            // Capture the compiled op parallel to step_handles for
                            // post-loop classification (task 2982). Cleared on
                            // rollback below. Pushed last in this arm so all the
                            // earlier `&geom_op` borrows above have already
                            // released — we move ownership rather than cloning.
                            realization_ops.push(geom_op);
                            // Step-14 (task ε / 3436): capture the terminal
                            // op's output [`ReprKind`] for the post-loop
                            // `produced_repr_out` write. `op_produced_repr`
                            // was bound by the match above and carries the
                            // per-arm derivation:
                            //
                            // - `Some(plan)` 0-conversion success:
                            //   `plan_output_repr(registry, plan, operation)`
                            //   (may be `None` if the named kernel's
                            //   descriptor has no entry for `op` — an
                            //   invariant violation that defensively leaves
                            //   `produced_repr_out` untouched).
                            // - `None` backward-compat fallback success:
                            //   `Some(ReprKind::BRep)` (the v0.2 single-
                            //   kernel-path invariant; pinned by
                            //   `execute_realization_ops_writes_produced_repr_brep_in_none_fallback_backward_compat`).
                            //
                            // Every subsequent loop iteration overwrites
                            // this capture, so `last_produced_repr` reflects
                            // the terminal op's repr when the loop exits.
                            last_produced_repr = op_produced_repr;
                        }
                        Err(e) => {
                            let err_msg = format!("geometry error: {}", e);
                            diagnostics.push(Diagnostic::error(err_msg.clone()).with_label(
                                DiagnosticLabel::new(realization_span, "in this realization"),
                            ));
                            // Arch §9.1 lines 868–877: surface the kernel error to the
                            // caller so the realization NodeId can be marked Failed in
                            // the eval cache and a single EventKind::Failed event emitted.
                            // First-error-wins inside a single realization: if a later
                            // call into this helper somehow triggers another kernel error
                            // (it won't — we `break` immediately), the first one is kept.
                            if kernel_error_out.is_none() {
                                *kernel_error_out = Some(ErrorRef::new(err_msg));
                            }
                            break;
                        }
                    }
                }
                Err(err) => {
                    diagnostics.push(
                        Diagnostic::error(format!("failed to compile geometry operation: {}", err))
                            .with_label(DiagnosticLabel::new(
                                realization_span,
                                "in this realization",
                            )),
                    );
                    // Task 4048: index-alignment sentinel for a failed compile.
                    // `resolved_kernel_name` is not yet bound in this pre-dispatch
                    // arm, so tag with the default kernel's KernelId — the handle
                    // is never read as a real handle (see `kernel_id_for_registry_name`).
                    step_handles.push(KernelHandle {
                        kernel: kernel_id_for_registry_name(default_kernel_name),
                        id: GeometryHandleId::INVALID,
                    });
                    realization_step_ids.push(GeometryHandleId::INVALID);
                    // Task 4050 step-8: keep the parallel repr Vec in lockstep on
                    // the failed-compile sentinel path too (BRep placeholder; the
                    // whole realization rolls back below, so this is never read as
                    // a real produced repr).
                    realization_step_reprs.push(ReprKind::BRep);
                    had_failure = true;
                }
            }
        }
        // GR-034 (task #3445): emit the long-chain diagnostic at most once per
        // realization, from the longest captured DispatchPlan + measured
        // wall-time. Emitted BEFORE the `rolled_back` determination so the
        // warning fires independent of whether the chain executed successfully
        // — the routing decision and wall-time are valid observations even when
        // execution later fails on an unsupported conversion crossing.
        // `long_chain_diagnostic` internally gates on `is_long_chain_realization`
        // (conversions.len() > 2 AND elapsed > threshold) and returns None when
        // the gate fails, so the caller needs no extra guard.
        //
        // NOTE: `elapsed` is the WHOLE-REALIZATION total wall-time (spanning ALL
        // ops in this realization), not the individual execution time of the
        // named chain's op alone. The pairing is intentional: the longest-chain
        // plan identifies *where* the conversion budget goes; the aggregate
        // elapsed signals *how much* total time was spent. In practice a single
        // long-chain op dominates the total, and the aggregate is the right proxy
        // for user-visible latency. Reads as: "this realization took Xms total
        // and its longest conversion chain was N stages through <kernels>".
        let elapsed = realize_start.elapsed();
        if let Some(ref p) = longest_chain_plan
            && let Some(diag) =
                crate::dispatcher::long_chain_diagnostic(p, elapsed, long_chain_threshold)
        {
            diagnostics.push(diag);
        }
        // Discard intermediate handles from partially-failed realizations
        let rolled_back =
            had_failure || step_handles.len().saturating_sub(handle_start) < operations.len();
        if rolled_back {
            step_handles.truncate(handle_start);
            // Task 4050 step-14: atomic intermediate-cache rollback. Drop every
            // intermediate key this realization inserted (step-12) so a failed
            // realization leaves NO cache entry behind — its handle truncation
            // and its cache mutations roll back together (PRD §9 OQ9,
            // provisional). `remove` is an exact-tolerance delete that no-ops on
            // an absent key, so it is safe even if a key was never committed.
            // The SUCCESS branch below deliberately does NOT drain this log: a
            // completed realization's intermediates stay committed so later
            // same-build realizations reuse them (step-11's reuse requirement).
            for (entity, repr, tol, options_hash) in &intermediate_cache_inserts {
                realization_cache.remove(entity, *repr, *tol, *options_hash);
            }
        } else {
            // Fully-successful realization. Three things land here, all keyed
            // on `step_handles[handle_start..].last()` so that an empty-ops
            // realization (operations.len() == 0) contributes nothing rather
            // than inheriting the final handle of the previous realization:
            //
            // 1. Phase A swept-body classification (task 2982) —
            //    `realization_ops` is parallel to `step_handles[handle_start..]`
            //    because every successful op pushed both in lockstep on the
            //    kernel-success branch above; on any failure (compile or
            //    kernel) the rolled_back branch is taken instead, so the
            //    parallelism holds whenever we enter this arm.
            // 2. `name → final_handle` recording (post-rollback so failed
            //    realizations never leave a stale entry that would let later
            //    realizations resolve a name whose geometry was never
            //    successfully produced).
            // 3. RealizationCache populate (task 2874, step-6) keyed on
            //    `(entity_id, ReprKind::BRep, demanded_tol)` when a demanded
            //    tolerance was threaded in. The bucket's partial-order rule
            //    may reject this insert if a tighter or equal entry is
            //    already cached; either way the post-condition "a satisfying
            //    entry exists at `(entity, BRep, tol)`" holds.
            //
            //    **Symmetric insert↔lookup gate (task 3176)**: we only insert
            //    when BOTH `demanded_tol.is_some()` AND
            //    `realization_name.is_some()` — exactly the pair the cache-hit
            //    short-circuit at the top of this function requires (see the
            //    `if let (Some(tol), Some(name)) = (demanded_tol,
            //    realization_name)` guard above). The lookup path also writes
            //    `named_steps[name] = cached_handle`, which is unreachable
            //    without a name, so symmetry is required by contract.
            //
            //    The production compiler always emits `Some(name)` for every
            //    `RealizationDecl` (crates/reify-compiler/src/types.rs:848-857),
            //    so this gate is a no-op for production builds — anonymous
            //    realizations can only originate from
            //    `TopologyTemplateBuilder::realization(...)` test-support code.
            //    Pinned by
            //    `anonymous_realization_does_not_populate_realization_cache_when_lookup_gate_requires_name`
            //    in tests/tolerance_wiring_e2e.rs.
            if let Some(kind) = classify_swept_body(&realization_ops, &realization_step_ids)
                && let Some(&last_id) = realization_step_ids.last()
            {
                swept_kind_table.record(last_id, kind);
            }
            // v0.2 persistent-naming-v2 (PRD task 4 / #2654): construction-time
            // fragility detection for local_index reassignment. The
            // topology_attribute_table is fully populated for this realization
            // at this point — every per-op `seed_primitive_attributes_for_handle`,
            // `populate_attribute_history`, and `propagate_via_kernel_attribute_hook`
            // call has already run on the success branch above. We filter the
            // table to entries scoped to THIS realization's `feature_id`,
            // query each face's centroid via the kernel, and warn the user
            // about (feature_id, role) groups that have geometrically tied
            // local_index assignments. The kernel's enumeration order is what
            // breaks the tie today, and a future edit could shuffle it.
            //
            // PRD line 72: emitted alongside but disjoint from the post-split
            // `TopologyAttributeAmbiguousAfterSplit` diagnostic (the helper's
            // `mod_history.is_empty()` filter cleanly separates the two
            // codes). Centroid-query failures emit a Warning and skip the
            // affected handle — auxiliary metadata MUST NOT regress the
            // realization to Failed, mirroring the
            // `seed_primitive_attributes_for_handle` and
            // `populate_attribute_history` warning idioms above.
            //
            // Per-realization tolerance threading is deferred — we use a
            // fixed `1e-9 m` (kernel-epsilon-tight) sentinel here per the
            // task-4 design decision recorded in `.task/plan.json`.
            let realization_feature_id = FeatureId::from(realization_id);
            // Per-realization scan: re-walks the full `topology_attribute_table`
            // to filter entries whose `feature_id` matches the current realization,
            // giving O(R·N) total cost per build (R = realizations, N = total table
            // entries). Acceptable today (R≈10, N≈100 → ≈1 000 filter ops per build,
            // no profiler hits observed). If a profiler hits this site, two preferred
            // fixes are: (i) thread a per-realization start-index into the table so we
            // walk only newly added entries, or (ii) maintain a secondary
            // `HashMap<FeatureId, Vec<GeometryHandleId>>` index inside
            // `TopologyAttributeTable` so `entries_for_feature(feature_id)` is
            // O(per-feature-entries). Per task #3369 review of #2654.
            // Non-threaded reader (interim single-kernel Occt, #4351): this
            // diagnostic scan is not part of the resolver/selector scoping
            // this task threads (step-6) — every SURVIVING handle (i.e. after
            // the `solid_attribute_handles` exclusion below) is Occt-scoped,
            // so collapsing to a bare `GeometryHandleId` in the `.map` below
            // is behavior-preserving. Mixed-kernel-aware diagnostics are
            // downstream work.
            // Task #4636: also excludes `solid_attribute_handles` — the
            // per-solid representative entries `record_solid_attribute`
            // wrote (source, Occt-scoped) and the entries
            // `forward_solid_attribute_on_ingest` forwarded (target,
            // Manifold-scoped) share this realization's `feature_id` but are
            // not real face/edge/vertex attributes, so they must not be
            // centroid-queried or fed into the local-index-reassignment tie
            // scan (see the declaration comment above). The filter matches
            // the FULL `KernelHandle` (kernel + id), not just the bare id, so
            // excluding a forwarded `{Manifold, X}` entry can never
            // over-exclude a real `{Occt, X}` face entry that happens to
            // share id `X`.
            let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
                topology_attribute_table
                    .iter()
                    .filter(|(kernel_handle, attr)| {
                        attr.feature_id == realization_feature_id
                            && !solid_attribute_handles.contains(kernel_handle)
                    })
                    .map(|(kernel_handle, attr)| (kernel_handle.id, attr))
                    .collect();
            if !realization_attrs.is_empty() {
                // Step-8 (task ε / 3436): the centroid query is a
                // single-handle query surface that runs against the engine's
                // default kernel. In the v0.3-ε baseline every realization's
                // terminal handle lives on the BRep-preferring lex-min
                // kernel (the default), so routing centroid queries through
                // it matches the v0.2 single-kernel semantics.
                let default_kernel: &mut dyn GeometryKernel = kernels
                    .get_mut(default_kernel_name)
                    .expect("default kernel must remain in the map for centroid queries")
                    .as_mut();
                let (centroids, centroid_diags) = collect_centroids_with_failure_summary(
                    &realization_attrs,
                    default_kernel,
                    realization_id,
                );
                diagnostics.extend(centroid_diags);
                detect_local_index_reassignment_diagnostics(
                    &realization_attrs,
                    &centroids,
                    LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                    realization_span,
                    diagnostics,
                );
            }
            // ── Task 4743 (α): VolumeMesh realization call edge ───────────────
            //
            // Demand is computed module-statically in `compute_demanded_reprs`
            // (a registered VolumeMesh-demanding `@optimized` consumer over this
            // realization overrides its demanded repr to VolumeMesh — the OQ-1
            // resolution). A box/primitive op produces BRep and Gmsh advertises
            // only `(Convert{from: Mesh}, VolumeMesh)`, so the per-op dispatcher
            // cannot emit VolumeMesh for a primitive realization: `demanded_repr
            // == VolumeMesh` fell back to a BRep terminal in the loop above
            // (design_decision 3). This dedicated post-loop edge realizes the
            // BRep→Mesh→VolumeMesh chain through the (otherwise orphaned)
            // `dispatch_volume_mesh` tet path exactly as the task specifies:
            // tessellate the terminal handle on its source kernel → gmsh
            // `mesh_surface_to_volume` (P1) → gmsh `store_volume_mesh`, then push
            // the gmsh VolumeMesh handle as the new realization terminal so the
            // existing `named_steps` / `realization_cache` / `produced_repr_out`
            // writes below carry VolumeMesh automatically. The caller
            // (`build` / `build_snapshot`) then sets `produced_kernel` from
            // `step_handles.last().kernel` and `realization_handles[node]` from
            // the same handle, so the read side (`volume_mesh()`) is unchanged.
            //
            // Element order is fixed to P1 for α (design_decision 6);
            // consumer-driven order is the FEA/morph arm's concern. The
            // `Swept(SweptMesh3d)` hex/wedge outcome has no `volume_mesh()` tet
            // read-back projection, so the tet path is forced (`force_tet=true`)
            // and a swept outcome degrades with a diagnostic rather than being
            // mis-stored as a tet VolumeMesh. Any failure (no gmsh kernel,
            // tessellation/mesh error, swept outcome) leaves the realization at
            // its BRep/Mesh fallback (honest degradation, never a hard error).
            // ── Task 4744 β (step-16): morph-or-remesh decision arm ───────────
            //
            // Before remeshing a VolumeMesh-demanded terminal, probe for a
            // registered morph producer + a prior morph source. When both (and
            // the new-BRep graph) are present, build a `MorphRequest` over the
            // new-BRep terminal kernel and ask `decide_morph_or_remesh`:
            //   • `Morphed(mesh)` ⇒ store the connectivity-preserving mesh on
            //     gmsh (same store path as remesh) + push the handle as the new
            //     terminal, and SKIP the remesh block (`morph_stored = true`).
            //   • `Remesh`        ⇒ honest fallback: the remesh block below runs.
            // With no producer registered (every step-16 call site passes
            // `disabled()`), `morph_io.producer` is `None`, the arm is skipped,
            // and the remesh block runs byte-identically. Feeding the real
            // producer/source/graph + the source-bundle stash is step-20.
            let mut morph_stored = false;
            // Task 4744 β step-20: the freshly-produced VolumeMesh (morph OR
            // remesh) captured for the source-bundle stash below. Cloned just
            // before `store_volume_mesh` consumes the mesh in each store arm.
            let mut produced_vm: Option<reify_ir::VolumeMesh> = None;
            // Task 4744 β step-20: hoist the BRep terminal's face/edge/vertex
            // slices ONCE, on the terminal's source kernel, BEFORE the
            // morph/remesh arms push a VolumeMesh handle (which would make
            // `step_handles[..].last()` the VolumeMesh, not the BRep). Used by
            // BOTH the morph arm's `new_brep` (this tick's NEW shape) AND the
            // stash's `old_brep` (the shape the produced mesh was meshed from,
            // becoming "old" on the next tick). Gated on `producer.is_some()` so
            // the no-producer path skips the extra kernel queries entirely
            // (byte-identical to pre-step-20). Honest-degrade to `None` on any
            // extraction failure.
            //
            // (kernel registry name, face handles, edge handles, vertex handles);
            // aliased to tame clippy::type_complexity.
            type BRepTerminalSlices = (
                String,
                Vec<GeometryHandleId>,
                Vec<GeometryHandleId>,
                Vec<GeometryHandleId>,
            );
            let brep_terminal: Option<BRepTerminalSlices> =
                if morph_io.producer.is_some()
                    && demanded_repr == ReprKind::VolumeMesh
                    && is_terminal_realization
                    && let Some(&terminal) = step_handles[handle_start..].last()
                {
                    let terminal_name = if kernels.contains_key(terminal.kernel.as_registry_name()) {
                        terminal.kernel.as_registry_name().to_string()
                    } else {
                        default_kernel_name.to_string()
                    };
                    match kernels.get_mut(&terminal_name) {
                        Some(src) => {
                            let faces = src.extract_faces(terminal.id).unwrap_or_default();
                            let edges = src.extract_edges(terminal.id).unwrap_or_default();
                            let vertices = src.extract_vertices(terminal.id).unwrap_or_default();
                            Some((terminal_name, faces, edges, vertices))
                        }
                        None => None,
                    }
                } else {
                    None
                };
            // ── Morph arm: fires only on the warm path (a prior source exists) ─
            if let Some((terminal_name, faces, edges, vertices)) = brep_terminal.as_ref()
                && let (Some(producer), Some(source), Some(new_graph)) =
                    (morph_io.producer, morph_io.source, morph_io.new_graph)
                && let Some(kernel) = kernels.get(terminal_name.as_str())
            {
                let new_brep = crate::morph_producer::BRepSnapshot {
                    graph: new_graph,
                    values,
                    topology_attributes: &*topology_attribute_table,
                    faces,
                    edges,
                    vertices,
                };
                let decision = crate::morph_producer::decide_morph_or_remesh(
                    Some(producer),
                    Some(source),
                    new_brep,
                    kernel.as_ref(),
                    realization_id,
                    diagnostics,
                );
                if let crate::morph_producer::MorphDecision::Morphed(mesh) = decision {
                    // Store the morphed (connectivity-preserving) mesh on the
                    // gmsh kernel — the SAME store path the remesh arm uses —
                    // and push the handle as the new terminal so the read
                    // side (`volume_mesh()` / `boundary()`) is unchanged.
                    match kernels.get(KernelId::Gmsh.as_registry_name()) {
                        Some(gmsh) => {
                            let stash_vm = mesh.clone();
                            match gmsh.store_volume_mesh(mesh) {
                                Ok(id) => {
                                    step_handles.push(KernelHandle {
                                        kernel: KernelId::Gmsh,
                                        id,
                                    });
                                    last_produced_repr = Some(ReprKind::VolumeMesh);
                                    morph_stored = true;
                                    produced_vm = Some(stash_vm);
                                }
                                Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                    "VolumeMesh realization {realization_id}: morph \
                                     store_volume_mesh failed ({e}); falling back to remesh"
                                ))),
                            }
                        }
                        None => diagnostics.push(Diagnostic::warning(format!(
                            "VolumeMesh realization {realization_id}: morph produced a mesh \
                             but no gmsh kernel is registered to store it; falling back to \
                             remesh"
                        ))),
                    }
                }
            }
            if !morph_stored
                && demanded_repr == ReprKind::VolumeMesh
                && is_terminal_realization
                && let Some(&terminal) = step_handles[handle_start..].last()
            {
                let tol = demanded_tol.unwrap_or(Self::DEFAULT_TESSELLATION_TOLERANCE);
                // (1) Tessellate the terminal BRep/Mesh handle to a surface Mesh
                // on its source kernel (`&self`; the owned result releases the
                // borrow before the gmsh borrow below — non-conflicting
                // sequential `kernels.get()` immutable borrows).
                // The terminal handle is keyed in `kernels` by the name the
                // per-op loop produced it under — that is `default_kernel_name`
                // for a primitive box realization (the same key the centroid
                // block above resolves via `kernels.get_mut(default_kernel_name)`).
                // The `KernelHandle::kernel` registry name can differ from that
                // map key (e.g. a synthetic default-kernel holder), so prefer the
                // terminal's own registry name only when it is actually a key,
                // else fall back to `default_kernel_name`.
                let terminal_name = if kernels.contains_key(terminal.kernel.as_registry_name()) {
                    terminal.kernel.as_registry_name()
                } else {
                    default_kernel_name
                };
                let surface = match kernels.get(terminal_name) {
                    Some(src) => match src.tessellate(terminal.id, tol) {
                        Ok(mesh) => Some(mesh),
                        Err(e) => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "VolumeMesh realization {realization_id}: tessellation of the \
                                 terminal handle on kernel '{terminal_name}' failed ({e}); \
                                 leaving the BRep/Mesh fallback"
                            )));
                            None
                        }
                    },
                    None => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "VolumeMesh realization {realization_id}: terminal source kernel \
                             '{terminal_name}' absent from the kernel map; leaving the BRep/Mesh \
                             fallback"
                        )));
                        None
                    }
                };
                // (2)+(3) Route the surface through `dispatch_volume_mesh` (tet
                // path) bound to the gmsh `mesh_surface_to_volume` trait method,
                // then `store_volume_mesh` the produced tet VolumeMesh and push
                // the gmsh handle as the new realization terminal.
                if let Some(surface) = surface {
                    // Task 4092 step-18: when this realization is boundary-demanded,
                    // build face anchors on the SOURCE kernel (extract_faces +
                    // Centroid) for the attributed producer below, and derive a
                    // nearest-anchor match tolerance from the surface bbox. The
                    // `get_mut` borrow ends before the gmsh immut borrow.
                    // `(face_anchors, match_tolerance)` for the attributed
                    // producer; `None` ⇒ degrade to the plain
                    // `mesh_surface_to_volume` path (boundary None).
                    type AttributedAnchorInput =
                        Option<(Vec<(reify_ir::GeometryHandleId, [f64; 3])>, f64)>;
                    // Task 4744 β step-20: the morph source's `BoundaryAssociation`
                    // (which `compute_dirichlet_bcs` projects through the
                    // correspondence map) comes from this 4092 attributed branch,
                    // gated on `demanded_boundary`. We do NOT force it on merely
                    // because a morph producer is registered: the attributed gmsh
                    // producer SIGSEGVs in tetgen boundary recovery on real
                    // OCCT-tessellated surfaces (#4876 — a crash `catch_unwind`
                    // cannot trap), so forcing it would crash every production
                    // VolumeMesh build once reify-cli installs the producer. Until
                    // #4876 hardens the producer, a morph source carries a boundary
                    // ONLY when its realization is boundary-demanded; a source
                    // without a boundary honestly degrades to remesh in
                    // `decide_morph_or_remesh`. The morph arm is otherwise fully
                    // wired and unit-tested; the real-OCCT morph e2e is gated on
                    // #4876 (see tests/morph_arm_e2e.rs).
                    let attributed: AttributedAnchorInput =
                        if demanded_boundary {
                            match kernels.get_mut(terminal_name) {
                                Some(src) => {
                                    let anchors =
                                        crate::compute_targets::bc_resolve::build_face_anchors(
                                            src.as_mut(),
                                            terminal.id,
                                            diagnostics,
                                        );
                                    if anchors.is_empty() {
                                        diagnostics.push(Diagnostic::warning(format!(
                                            "VolumeMesh realization {realization_id}: no face \
                                             anchors built for boundary attribution; degrading to \
                                             the plain producer (boundary None)"
                                        )));
                                        None
                                    } else {
                                        // min bbox extent → 0.3·extent: above gmsh's
                                        // face-entity centroid drift, below the
                                        // inter-face spacing (faces never cross-match).
                                        let mut lo = [f64::INFINITY; 3];
                                        let mut hi = [f64::NEG_INFINITY; 3];
                                        for v in surface.vertices.chunks_exact(3) {
                                            for k in 0..3 {
                                                let c = v[k] as f64;
                                                if c < lo[k] {
                                                    lo[k] = c;
                                                }
                                                if c > hi[k] {
                                                    hi[k] = c;
                                                }
                                            }
                                        }
                                        let min_extent = (0..3)
                                            .map(|k| hi[k] - lo[k])
                                            .fold(f64::INFINITY, f64::min);
                                        let match_tol =
                                            if min_extent.is_finite() && min_extent > 0.0 {
                                                0.3 * min_extent
                                            } else {
                                                tol
                                            };
                                        Some((anchors, match_tol))
                                    }
                                }
                                None => None,
                            }
                        } else {
                            None
                        };
                    match kernels.get(KernelId::Gmsh.as_registry_name()) {
                        Some(gmsh) => {
                            // Task 4092 step-18: attributed path first (when
                            // boundary-demanded + anchors built); on ANY failure
                            // degrade to the plain mesh_surface_to_volume path
                            // (boundary None) — honest degradation.
                            let mut stored = false;
                            if let Some((anchors, match_tol)) = &attributed {
                                match gmsh.mesh_surface_to_volume_attributed(
                                    &surface,
                                    ElementOrderTag::P1,
                                    anchors,
                                    *match_tol,
                                ) {
                                    Ok(vm) => {
                                        // Task 4744 β step-20: clone for the
                                        // source-bundle stash (store consumes vm).
                                        let stash_vm = vm.clone();
                                        match gmsh.store_volume_mesh(vm) {
                                        Ok(id) => {
                                            step_handles.push(KernelHandle {
                                                kernel: KernelId::Gmsh,
                                                id,
                                            });
                                            last_produced_repr = Some(ReprKind::VolumeMesh);
                                            stored = true;
                                            produced_vm = Some(stash_vm);
                                        }
                                        Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                            "VolumeMesh realization {realization_id}: attributed \
                                             store_volume_mesh failed ({e}); degrading to the \
                                             plain producer (boundary None)"
                                        ))),
                                    }
                                    }
                                    Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                        "VolumeMesh realization {realization_id}: attributed gmsh \
                                         meshing failed ({e}); degrading to the plain producer \
                                         (boundary None)"
                                    ))),
                                }
                            }
                            if !stored {
                            let outcome = dispatch_volume_mesh(
                                None,  // swept_kind: force the tet path
                                true,  // force_tet
                                false, // require_hex_wedge
                                &realization_ops,
                                &realization_step_ids,
                                |_swept| unreachable!("gmsh_2d unreachable: force_tet=true"),
                                |_params, _mesh| {
                                    unreachable!("sweep_step unreachable: force_tet=true")
                                },
                                || gmsh.mesh_surface_to_volume(&surface, ElementOrderTag::P1),
                            );
                            match outcome {
                                Ok(VolumeMeshOutcome::Tet(vm)) => {
                                    // Task 4744 β step-20: clone for the
                                    // source-bundle stash (store consumes vm).
                                    let stash_vm = vm.clone();
                                    match gmsh.store_volume_mesh(vm) {
                                        Ok(id) => {
                                            step_handles.push(KernelHandle {
                                                kernel: KernelId::Gmsh,
                                                id,
                                            });
                                            last_produced_repr = Some(ReprKind::VolumeMesh);
                                            produced_vm = Some(stash_vm);
                                        }
                                        Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                            "VolumeMesh realization {realization_id}: gmsh \
                                             store_volume_mesh failed ({e}); leaving the \
                                             BRep/Mesh fallback"
                                        ))),
                                    }
                                }
                                Ok(VolumeMeshOutcome::Swept(swept)) => {
                                    // α forces `force_tet=true`, so the swept arm is
                                    // unreachable in practice; degrade honestly if a
                                    // future change relaxes that. Read the swept
                                    // payload (node/layer counts) into the diagnostic
                                    // so the variant field is genuinely consumed (no
                                    // dead-code allow): a swept hex/wedge mesh has no
                                    // `volume_mesh()` tet read-back projection, so it
                                    // is NOT stored as a tet VolumeMesh.
                                    diagnostics.push(Diagnostic::warning(format!(
                                        "VolumeMesh realization {realization_id}: dispatch \
                                         produced a swept hex/wedge mesh ({} nodes, {} layers), \
                                         which has no volume_mesh() tet read-back projection; \
                                         leaving the BRep/Mesh fallback (the tet path is α's \
                                         read path)",
                                        swept.vertices.len() / 3,
                                        swept.layers,
                                    )));
                                }
                                Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                    "VolumeMesh realization {realization_id}: gmsh tet meshing \
                                     failed ({e}); leaving the BRep/Mesh fallback"
                                ))),
                            }
                            }
                        }
                        None => diagnostics.push(Diagnostic::warning(format!(
                            "VolumeMesh realization {realization_id}: no gmsh kernel registered \
                             (call ensure_gmsh_kernel()); leaving the BRep/Mesh fallback"
                        ))),
                    }
                }
            }
            // ── Task 5033 (GAP #2): Voxel realization call edge ───────────────
            //
            // Mirrors the VolumeMesh edge above but for `demanded_repr ==
            // Voxel` and NON-terminal-gated: `isosurface`'s operand (e.g.
            // `solid` in `let shell = isosurface(solid)`) is demanded Voxel by
            // `compute_demanded_reprs`' Voxel-only-input consumer rule (β,
            // task 5000), and — unlike VolumeMesh, which only ever anchors a
            // terminal FEA/optimization consumer — this operand is virtually
            // always a NON-terminal (intermediate let-binding) realization. A
            // primitive/BRep op has no kernel that emits Voxel directly, so
            // design_decision 3's BRep fallback already ran in the per-op loop
            // above; this dedicated post-loop edge forces that BRep/Mesh
            // terminal THROUGH the existing BRep→Mesh (Tessellate) → Mesh→Voxel
            // (Voxelize) conversion pair, mirroring the VolumeMesh edge's
            // tessellate→ingest shape. Guarded on `last_produced_repr !=
            // Some(Voxel)` so this edge is purely additive: a realization whose
            // per-op loop already resolved Voxel directly (e.g. an OpenVDB-
            // native Boolean op) skips it untouched. Any failure (no OpenVDB
            // kernel, tessellation/ingest error) leaves the realization at its
            // BRep/Mesh fallback — honest degradation, never a hard error.
            if demanded_repr == ReprKind::Voxel
                && last_produced_repr != Some(ReprKind::Voxel)
                && let Some(&terminal) = step_handles[handle_start..].last()
            {
                let tol = demanded_tol.unwrap_or(Self::DEFAULT_TESSELLATION_TOLERANCE);
                // Same registry-name-vs-map-key dance as the VolumeMesh edge
                // above (backward-compat sentinel mode keys OCCT's handle under
                // `default_kernel_name`, not `"occt"`).
                let terminal_name = if kernels.contains_key(terminal.kernel.as_registry_name()) {
                    terminal.kernel.as_registry_name()
                } else {
                    default_kernel_name
                };
                let surface = match kernels.get(terminal_name) {
                    Some(src) => match src.tessellate(terminal.id, tol) {
                        Ok(mesh) => Some(mesh),
                        Err(e) => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "Voxel realization {realization_id}: tessellation of the \
                                 terminal handle on kernel '{terminal_name}' failed ({e}); \
                                 leaving the BRep/Mesh fallback"
                            )));
                            None
                        }
                    },
                    None => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Voxel realization {realization_id}: terminal source kernel \
                             '{terminal_name}' absent from the kernel map; leaving the BRep/Mesh \
                             fallback"
                        )));
                        None
                    }
                };
                if let Some(surface) = surface {
                    match kernels.get_mut(KernelId::OpenVdb.as_registry_name()) {
                        Some(openvdb) => match openvdb.ingest_mesh(&surface) {
                            Ok(handle) => {
                                step_handles.push(KernelHandle {
                                    kernel: KernelId::OpenVdb,
                                    id: handle.id,
                                });
                                last_produced_repr = Some(ReprKind::Voxel);
                            }
                            Err(e) => diagnostics.push(Diagnostic::warning(format!(
                                "Voxel realization {realization_id}: openvdb ingest_mesh failed \
                                 ({e}); leaving the BRep/Mesh fallback"
                            ))),
                        },
                        None => diagnostics.push(Diagnostic::warning(format!(
                            "Voxel realization {realization_id}: no openvdb kernel registered \
                             (call ensure_openvdb_kernel()); leaving the BRep/Mesh fallback"
                        ))),
                    }
                }
            }
            // ── Task 4744 β step-20: source-bundle stash ─────────────────────
            //
            // When a morph producer is active and this realization produced a
            // VolumeMesh (morph OR remesh), snapshot the bundle the NEXT tick's
            // morph needs: the produced mesh (carrying its 4092 boundary on the
            // attributed remesh path) + an OWNED snapshot of the BRep it was
            // meshed from. The owned snapshot is mandatory — the live `graph`,
            // `values`, and `topology_attribute_table` are all wiped/replaced by
            // the next build, so `morph_eligible` Stage-A/B could not otherwise
            // see the OLD shape. `EvaluationGraph`/`ValueMap`/`TopologyAttribute`
            // clone cheaply (persistent maps / small records). Returned to the
            // caller, which writes it into `Engine::morph_source` (needs
            // `&mut self`, unavailable here).
            if let (Some((_, faces, edges, vertices)), Some(vm), Some(new_graph)) =
                (brep_terminal, produced_vm, morph_io.new_graph)
            {
                morph_source_stash = Some(crate::morph_producer::MorphSource {
                    source_mesh: vm,
                    old_brep: crate::morph_producer::OwnedBRepSnapshot {
                        graph: (*new_graph).clone(),
                        values: (*values).clone(),
                        topology_attributes: topology_attribute_table.clone(),
                        faces,
                        edges,
                        vertices,
                    },
                });
            }
            if let Some(&last) = step_handles[handle_start..].last() {
                if let Some(name) = realization_name {
                    // Bare-name key (e.g. "b") backs same-structure GeomRef::Sub("b")
                    // refs emitted by the compiler's sibling pre-check (task #4668
                    // step-2, geometry.rs).  Cross-sub keys ("sub.member") are seeded
                    // separately via the compound-key injection path below.  Both are
                    // consumed by geometry_ops.rs::resolve_geom_ref's Sub arm.
                    named_steps.insert(name.to_string(), last);
                    // Task 5033 Gap #2 Gap A: by-name repr sibling write,
                    // unconditional (unlike the terminal-only cache insert
                    // below) because a NON-terminal cross-realization
                    // producer (e.g. "solid" in `let shell =
                    // isosurface(solid)`) still needs its repr resolvable
                    // by name for `available_for_op` below. Same
                    // resolved-repr expression as the terminal cache-key
                    // computation just below: the RESOLVED repr
                    // (`last_produced_repr`), falling back to `cache_repr`
                    // only when no op captured one.
                    named_step_reprs.insert(name.to_string(), last_produced_repr.unwrap_or(cache_repr));
                }
                if is_terminal_realization
                    && let (Some(tol), Some(_name)) = (demanded_tol, realization_name)
                {
                    // **Task 4050 step-10 (gap 4)**: key the INSERT on the
                    // RESOLVED terminal repr (`last_produced_repr`), falling
                    // back to `cache_repr` only when no op captured a repr. On
                    // the non-fallback path resolved == demanded == cache_repr,
                    // so the lookup and insert coincide and the next identical
                    // build hits. On a fallback realization (demanded Mesh but
                    // resolved BRep because no Mesh kernel was linked) this
                    // stores at BRep, so a later Mesh lookup correctly MISSES
                    // rather than handing back a BRep handle as if it were Mesh.
                    //
                    // **Task 3437 (ζ): guard INSERT on is_terminal_realization.**
                    // Non-terminal realizations (intermediate let-bindings in
                    // a structure) share the same `entity` cache key as the
                    // terminal.  Without this guard, box_a's BRep handle would
                    // be stored at `(entity, BRep, tol)` before the terminal's
                    // ops run.  On a Mesh-capable engine the terminal's BRep
                    // fallback probe would then find the intermediate handle,
                    // and since that same handle is recorded in
                    // `topology_attribute_table` (from its own op run earlier in
                    // this build), the per-build reset debug_assert fires.  Only the
                    // TERMINAL realization's result is a valid cache entry for
                    // the entity+tol key — intermediate lets are intra-build
                    // scratch and must not pollute the cross-build cache.
                    let resolved_repr = last_produced_repr.unwrap_or(cache_repr);
                    realization_cache.insert(
                        &realization_id.entity,
                        resolved_repr,
                        tol,
                        NO_OPTIONS,
                        last,
                    );
                }
                // Step-14 (task ε / 3436): surface the terminal op's output
                // [`ReprKind`] through `produced_repr_out` so the caller
                // (`build` / `build_snapshot`) writes it into
                // `eval_state.snapshot.graph.realizations[id].produced_repr`.
                // Gated on `last_handle.is_some()` (the same gate the
                // `named_steps` and `realization_cache` writes use) so an
                // empty-operations realization contributes nothing and the
                // construction-time default survives.
                //
                // `last_produced_repr` is the single capture-and-write
                // channel that honors both per-op success paths uniformly:
                // (a) the `Some(plan)` 0-conversion arm wrote
                // `plan_output_repr(registry, plan, operation)` from the
                // dispatcher-named kernel's descriptor; (b) the `None`
                // backward-compat fallback arm wrote `Some(ReprKind::BRep)`
                // directly (the v0.2 single-kernel-path invariant for the
                // synthetic default kernel). A `None` value here means
                // either: (i) no op succeeded for this realization (the
                // outer `last_handle.is_some()` gate would have already
                // short-circuited), or (ii) the dispatcher-named kernel's
                // descriptor had no entry for the terminal op — an
                // invariant violation that defensively leaves the channel
                // untouched rather than fabricating a repr.
                if let Some(repr) = last_produced_repr {
                    *produced_repr_out = Some(repr);
                }
            }
        }
        // Task 4744 β step-20: hand the caller the source-bundle stash (Some only
        // when a morph producer is active AND this realization produced a
        // VolumeMesh; None on every off-build/no-producer call site).
        morph_source_stash
    }

    /// Returns the `VersionId` of the current eval round — the id stamped into
    /// `eval_state.snapshot` by the most recent `eval()` or `edit_param()` call.
    ///
    /// Both `build` and `build_snapshot` must tag kernel-error `Failed` events
    /// with this version (not `self.next_version_id`, which already points at
    /// the *next*, un-used round after `eval()` bumped the counter). Centralising
    /// the read here means a future call site cannot accidentally use the wrong
    /// counter.
    ///
    /// Panics if `eval_state` is not yet populated.
    fn current_eval_version(&self) -> VersionId {
        self.eval_state
            .as_ref()
            .expect("eval_state must be populated before reading current_eval_version")
            .snapshot
            .version
    }

    /// Mark a realization NodeId as `Freshness::Failed { error }` in the eval
    /// cache and emit a single `EventKind::Failed` event in the journal.
    ///
    /// Implements arch §9.1 lines 868–877 (kernel.execute(...) Err → mark
    /// realization Failed + emit one error event). Called from `build` and
    /// `build_snapshot` after `execute_realization_ops` surfaced a kernel
    /// error via the `kernel_error_out` parameter.
    ///
    /// Behavior:
    /// - If a cache entry already exists under `NodeId::Realization(rid)`:
    ///   uses [`CacheStore::mark_failed`] to flip `freshness` in place,
    ///   preserving the prior `result` and `dependency_trace`.
    /// - If no entry exists yet (cold-start build before any successful
    ///   handle was produced for this realization): inserts a stub entry
    ///   with `CachedResult::GeometryHandle(FAILED_REALIZATION_STUB_HANDLE)`
    ///   and `Freshness::Failed { error }` directly. The stub const
    ///   ([`FAILED_REALIZATION_STUB_HANDLE`] in `cache.rs`) is `u64::MAX - 1`
    ///   — explicitly **not** `0` (which is plausibly a real handle in
    ///   counters that start at zero) and not `GeometryHandleId::INVALID`
    ///   (`u64::MAX`) because `GeometryHandleId::content_hash` debug-asserts
    ///   on INVALID and `NodeCache::new` always hashes its result.
    ///   Consumers MUST gate on `Freshness::Failed` before reading the
    ///   handle — this stub is defence-in-depth, not an escape hatch.
    /// - Records exactly one `EventKind::Failed { error }` event scoped to
    ///   `NodeId::Realization(rid)`. The pre-existing
    ///   `Diagnostic::error("geometry error: …")` from
    ///   `execute_realization_ops` is left unchanged on `BuildResult.diagnostics`.
    ///
    /// Pinned by
    /// `tests/failed_propagation.rs::kernel_execute_error_marks_realization_failed_and_emits_one_error_event`.
    fn mark_realization_failed(
        cache: &mut CacheStore,
        journal: &mut EventJournal,
        rid: &RealizationNodeId,
        error: ErrorRef,
        version: VersionId,
    ) {
        let r_node = NodeId::Realization(rid.clone());
        // Try the in-place mutation first; if no entry exists, create a stub.
        if !cache.mark_failed(&r_node, error.clone()) {
            cache.put(
                r_node.clone(),
                NodeCache::new(
                    CachedResult::GeometryHandle(FAILED_REALIZATION_STUB_HANDLE),
                    Freshness::Failed {
                        error: error.clone(),
                    },
                    DependencyTrace::default(),
                    version,
                ),
            );
        }
        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id: r_node,
            kind: EventKind::Failed { error },
            version,
            payload: None,
        });
    }

    /// Hydrate `Type::Geometry` value cells from the realization-execution
    /// path (GHR-γ step-6).
    ///
    /// For each named [`RealizationDecl`] whose name matches a
    /// `ValueCellDecl` with `cell_type == Type::Geometry` in `template`,
    /// constructs `Value::GeometryHandle { realization_ref, upstream_values_hash,
    /// kernel_handle }` and writes it into `values`.
    ///
    /// `upstream_values_hash` is a deterministic 32-byte digest derived by
    /// folding the `content_hash()` of each scalar arg value across all ops
    /// in the realization (using `reify_core::hash::ContentHash` / XXH3-128).
    /// The first 16 bytes hold the combined hash; the second 16 bytes hold a
    /// salted variant to avoid all-zero output for empty arg lists.
    ///
    /// Runs in `build` and `build_snapshot` immediately before the
    /// conformance- and kinematic-query post-processes, so downstream value
    /// cells that read a `GeometryHandle` see the hydrated value.
    ///
    /// **GHR-δ (esc-3606-37 ruling step 1):** in addition to hydrating the GH
    /// cell value, this records each geometry-backed Realization as a
    /// freshness-bearing eval-cache node under `NodeId::Realization(rid)` with
    /// `Freshness::Final` and a trace of its scalar reads
    /// ([`extract_realization_dependencies`]). The PRD §5/§7.1 contract — "the
    /// cell's freshness is the meet of (VC-input freshness, all referenced
    /// Realization freshness)" — presupposes the referenced Realization carries
    /// a freshness value in the cache; on the success path nothing else creates
    /// that entry (the failure path uses [`Engine::mark_realization_failed`]).
    /// Only geometry-backed realizations are recorded here; non-geometry
    /// realizations continue to use the synthetic-insert test helper.
    // GHR-δ added `realization_handles`, pushing this to 8 distinct inputs;
    // matches the sibling post-process helpers' allow (e.g. lines 158/2065/2396).
    #[allow(clippy::too_many_arguments)]
    fn post_process_geometry_handle_cells(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        cache: &mut CacheStore,
        // GHR-δ §5: the per-Engine `realization_ref → handle` validity map.
        // Each geometry-backed realization records the handle it resolved to,
        // so a later read can revalidate a cell's `kernel_handle` against the
        // current Engine. Disjoint from `cache` / `values` (separate fields).
        realization_handles: &mut HashMap<reify_core::RealizationNodeId, GeometryHandleId>,
        version: VersionId,
    ) {
        use reify_core::identity::ValueCellId;
        use reify_ir::Value;

        // Two-phase approach: collect entries while holding a &ValueMap borrow
        // (via eval_ctx), then write them back via &mut ValueMap. This avoids a
        // split-borrow conflict between the read and write phases.
        let mut entries: Vec<(ValueCellId, Value)> = Vec::new();

        {
            let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);

            for realization in &template.realizations {
                let name = match &realization.name {
                    Some(n) => n.as_str(),
                    None => continue,
                };
                let kernel_handle = match named_steps.get(name) {
                    Some(kh) => kh.id,
                    None => continue,
                };
                // Hydrate all named realizations — geometry params AND geometry
                // lets. The compiler skips creating value cells for geometry lets
                // (entity.rs:1138), but topology selectors (post-process tier)
                // need to look up parent GeometryHandle via values.get(). Omitting
                // the old `has_geometry_cell` guard ensures both lets and params
                // are present in `values` before `run_post_processes` fires.

                // GHR-δ §5: record this realization's resolved handle in the
                // Engine's validity map (the read-time revalidation oracle).
                // `named_steps` already mapped this realization's name to the
                // handle the kernel produced for this build.
                realization_handles.insert(realization.id.clone(), kernel_handle);

                // GHR-δ / esc-3606-37 ruling step 1: record this geometry-backed
                // Realization as a freshness-bearing eval-cache node on the build
                // success path. The PRD §5/§7.1 realization_reads meet (folded by
                // `derive_output_freshness_from_trace_with_cause`) and the
                // freshness walk's `width → Realization → GH-cell` cascade both
                // require a markable `NodeId::Realization` entry here; previously
                // only the failure path created one (`mark_realization_failed`).
                // The trace records the realization's scalar reads (e.g. `width`)
                // so a dirtied scalar input re-derives R0 Pending. `cache` is a
                // disjoint Engine field from the `values`/`functions`/`meta_map`
                // borrows held by `ctx`.
                cache.record_evaluation_with_freshness(
                    NodeId::Realization(realization.id.clone()),
                    CachedResult::GeometryHandle(kernel_handle),
                    version,
                    extract_realization_dependencies(&realization.operations),
                    Freshness::Final,
                );

                let upstream_values_hash =
                    compute_realization_upstream_values_hash(realization, &ctx);

                entries.push((
                    ValueCellId::new(realization.id.entity.as_str(), name),
                    Value::GeometryHandle {
                        realization_ref: realization.id.clone(),
                        upstream_values_hash,
                        kernel_handle: Some(kernel_handle),
                    },
                ));
            }
        } // ctx dropped — &ValueMap borrow released

        for (cell_id, value) in entries {
            values.insert(cell_id, value);
        }
    }

    /// Post-hydration re-dispatch pass for `@optimized` ComputeNodes that
    /// consume a Solid body (task #4726 / esc-3787-23 root cause).
    ///
    /// Called immediately AFTER `post_process_geometry_handle_cells` in both
    /// `build()` and `build_snapshot()`. Finds ComputeNodes in the snapshot
    /// graph with EMPTY `realization_inputs` (because the body arg was `Undef`
    /// at the original dispatch — geometry lets have no value cell until
    /// `post_process_geometry_handle_cells` hydrates them) and whose re-evaluated
    /// args now include at least one `Value::GeometryHandle`. For each such
    /// node it:
    ///
    ///   1. Re-evaluates the arg_values from the cell's `default_expr`.
    ///   2. Re-builds `realization_inputs` via `build_compute_realization_inputs`.
    ///   3. Updates the existing `ComputeNodeData.realization_inputs` in the
    ///      snapshot graph (step-1 assertion: non-empty after build).
    ///   4. Re-runs `run_compute_dispatch` and overwrites the cell value in both
    ///      `values` and `eval_state.snapshot.values` (step-3: non-degraded field).
    ///
    /// Gate (narrow regression scope): only nodes with `realization_inputs.is_empty()`
    /// AND at least one arg evaluating to `Value::GeometryHandle` are re-dispatched.
    /// Non-geometry `@optimized` nodes (FEA scalar-dims, dynamics) are untouched.
    fn redispatch_geometry_consuming_compute_nodes(
        &mut self,
        module: &reify_compiler::CompiledModule,
        values: &mut ValueMap,
        version_id: VersionId,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if self.eval_state.is_none() {
            return;
        }

        // ── Phase 1: collect candidates ──────────────────────────────────────
        //
        // Snapshot the graph and collect all the info needed for dispatch
        // WITHOUT holding a borrow on `self` during the actual dispatch calls.
        //
        // `graph_snapshot` is cloned once: used as the stable `&EvaluationGraph`
        // arg to `build_compute_realization_inputs` (which needs `&mut self`)
        // and to `persistent_cache_key` (a static fn) throughout Phase 2, where
        // we cannot hold `&self.eval_state`.

        struct Candidate {
            c_id: reify_core::ComputeNodeId,
            target: String,
            output_cell: reify_core::ValueCellId,
            arg_exprs: Vec<reify_ir::CompiledExpr>,
        }

        let graph_snapshot;
        let candidates: Vec<Candidate> = {
            let state = self.eval_state.as_ref().unwrap();
            graph_snapshot = state.snapshot.graph.clone();

            let mut cands: Vec<Candidate> = Vec::new();
            for (c_id, node_data) in state.snapshot.graph.compute_nodes.iter() {
                // Only re-dispatch nodes that got EMPTY realization_inputs at
                // the original dispatch (body was Undef → no handle).
                if !node_data.realization_inputs.is_empty() {
                    continue;
                }
                if node_data.output_value_cells.is_empty() {
                    continue;
                }

                let output_cell = &node_data.output_value_cells[0];
                let entity_name = output_cell.entity.as_str();

                // Find the template that owns this output cell.
                let Some(template) = module.templates.iter().find(|t| t.name == entity_name) else {
                    continue;
                };

                // Find the value-cell declaration (must be a Let with a default
                // FunctionCall expression to carry the @optimized target).
                let Some(cell_decl) = template.value_cells.iter().find(|c| c.id == *output_cell) else {
                    continue;
                };
                let Some(default_expr) = &cell_decl.default_expr else {
                    continue;
                };

                // @optimized functions are lowered to UserFunctionCall by the
                // compiler (engine_eval.rs:4740 dispatches on UserFunctionCall).
                // FunctionCall is the stdlib/builtin variant — using it here
                // causes candidates to never be found and the redispatch to be
                // a no-op.  Must match the same variant the original dispatch
                // checks.
                if let reify_ir::CompiledExprKind::UserFunctionCall { args, .. } =
                    &default_expr.kind
                {
                    cands.push(Candidate {
                        c_id: c_id.clone(),
                        target: node_data.target.clone(),
                        output_cell: output_cell.clone(),
                        arg_exprs: args.clone(),
                    });
                }
            }
            cands
        };
        // `state` borrow dropped here; `graph_snapshot` is owned.

        // ── Phase 2: re-evaluate, gate, re-dispatch, patch ───────────────────

        for cand in candidates {
            // Guard: trampoline must still be registered (it was at the
            // original dispatch — this is a defensive check).
            if self.compute_dispatch(&cand.target).is_none() {
                continue;
            }

            // Re-evaluate arg_values using the now-hydrated `values`.
            let arg_values: Vec<reify_ir::Value> = {
                let ctx = crate::eval_ctx_with_meta(values, &self.functions, &self.meta_map);
                cand.arg_exprs
                    .iter()
                    .map(|a| reify_expr::eval_expr(a, &ctx))
                    .collect()
            };

            // Gate: at least one arg must now be a GeometryHandle (body was
            // hydrated by `post_process_geometry_handle_cells` above).
            if !arg_values
                .iter()
                .any(|v| matches!(v, reify_ir::Value::GeometryHandle { .. }))
            {
                continue;
            }

            // Part A (task #4726 step-4): before building realization_inputs,
            // ensure each BRep body consumed by this @optimized node has its
            // mesh in the projection store.  BRep is identity-only for
            // non-compute consumers (PRD §4 D1); pre-tessellating here only
            // adds a SurfaceMesh entry under the same (realization_id,
            // content_hash) key, leaving produced_repr intact (export stays
            // BRep).  The store-hit path in `project_realization_read_handle`
            // (realization_content.rs ~line 171) then serves it → body_aabb
            // sees a real mesh → non-degraded field (step-3 test goes GREEN).
            for arg in &arg_values {
                let reify_ir::Value::GeometryHandle {
                    realization_ref,
                    kernel_handle: Some(_),
                    ..
                } = arg
                else {
                    continue;
                };
                let Some(node_data) = graph_snapshot.realizations.get(realization_ref) else {
                    continue;
                };
                let content_hash = node_data.content_hash;
                let produced_repr = node_data.produced_repr;
                let produced_kernel = node_data.produced_kernel;
                if produced_repr != ReprKind::BRep
                    || self
                        .realization_projection_store
                        .get(realization_ref, content_hash)
                        .is_some()
                {
                    continue;
                }
                // Pattern from `project_realization_read_handle`'s Mesh arm:
                // compute the owned RealizedContent BEFORE the &mut store
                // insert to release the immutable kernel borrow first.
                let projected: Option<crate::engine_compute::RealizedContent> = self
                    .resolve_realization_kernel(realization_ref, produced_kernel)
                    .and_then(|(kernel, handle_id)| {
                        kernel
                            .tessellate(handle_id, Self::DEFAULT_TESSELLATION_TOLERANCE)
                            .ok()
                    })
                    .map(|mesh| {
                        crate::engine_compute::RealizedContent::SurfaceMesh(
                            std::sync::Arc::new(mesh),
                        )
                    });
                if let Some(content) = projected {
                    self.realization_projection_store
                        .insert(realization_ref.clone(), content_hash, content);
                }
                // If tessellation failed, leave store empty — dispatch degrades
                // honestly (lambda=Undef via existing degraded_field() path).
            }

            // Rebuild realization_inputs from the hydrated arg_values.
            // After the pre-tessellation pass above, BRep bodies hit the store
            // and project_realization_read_handle returns Some(SurfaceMesh).
            let (realization_inputs, realization_read_handles, proj_diags) =
                self.build_compute_realization_inputs(&arg_values, &graph_snapshot);
            diagnostics.extend(proj_diags);

            if realization_inputs.is_empty() {
                // Defensive: should not be reached on the green path.
                continue;
            }

            // Step 1: update the snapshot ComputeNodeData's realization_inputs.
            // This is the non-empty-realization_inputs gate (step-1 test).
            if let Some(state) = self.eval_state.as_mut()
                && let Some(node) = state.snapshot.graph.get_compute_node_mut(&cand.c_id)
            {
                node.realization_inputs = realization_inputs;
            }
            // `state` mut-borrow dropped.

            // Re-dispatch.  Use ContentHash(0) for the persistent-cache key:
            // the cache dir is None in all current tests so the key is inert.
            let cancel = crate::graph::CancellationHandle::new();
            match self.run_compute_dispatch(
                &cand.c_id,
                std::slice::from_ref(&cand.output_cell),
                &cand.target,
                &arg_values,
                &realization_read_handles,
                &reify_ir::Value::Undef, // options
                &cancel,
                version_id,
                reify_core::ContentHash(0),
            ) {
                Ok((result, diags, _)) => {
                    diagnostics.extend(diags);
                    // Overwrite the output-cell value in the local map.
                    values.insert(cand.output_cell.clone(), result.clone());
                    // Overwrite in the snapshot so `eval_state().snapshot.values`
                    // reflects the post-hydration result (step-3 test).
                    if let Some(state) = self.eval_state.as_mut() {
                        state.snapshot.values.insert(
                            cand.output_cell.clone(),
                            (result, reify_ir::DeterminacyState::Determined),
                        );
                        if let Some(n) = state.snapshot.graph.get_compute_node_mut(&cand.c_id) {
                            n.running = None;
                        }
                    }
                }
                Err(_) => {
                    // Dispatch failed (e.g. BRep body not yet mesh-projected).
                    // `realization_inputs` was already updated above, so the
                    // step-1 non-empty assertion still passes.
                }
            }
        }
    }

    /// Cascade re-dispatch for ComputeNodes that consume an AsPrintedZones
    /// material field (task #3787 ε — FDM integration gate).
    ///
    /// Called immediately AFTER [`Self::redispatch_geometry_consuming_compute_nodes`]
    /// in both `build()` and `build_snapshot()`.  By the time this function runs,
    /// `as_printed_material` has been re-dispatched and its output cell in `values`
    /// holds a non-degraded `Value::Field { source: AsPrintedZones, lambda: List }`.
    ///
    /// During the initial `eval()` pass, ComputeNodes that consume this material
    /// field (e.g. `solve_elastic_static(material, ...)`) ran with a degraded
    /// (lambda=Undef) field and returned `ComputeOutcome::Failed` — the graceful
    /// guard added to `solve_elastic_static_trampoline` (task #3787).  This pass
    /// re-dispatches those nodes with the now-hydrated material.
    ///
    /// Gate: only re-dispatches nodes with:
    ///   1. `realization_inputs.is_empty()` — the node had no geometry args, so it
    ///      was not touched by `redispatch_geometry_consuming_compute_nodes`.
    ///   2. At least one arg that evaluates to `Value::Field { source:
    ///      AsPrintedZones, lambda: non-Undef }` — confirms the material is now
    ///      available.
    ///   3. A `UserFunctionCall` default_expr (same precondition as the geometry
    ///      re-dispatch — `@optimized` functions lower to this variant).
    fn redispatch_as_printed_consuming_compute_nodes(
        &mut self,
        module: &reify_compiler::CompiledModule,
        values: &mut ValueMap,
        version_id: VersionId,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if self.eval_state.is_none() {
            return;
        }

        struct Candidate {
            c_id: reify_core::ComputeNodeId,
            target: String,
            output_cell: reify_core::ValueCellId,
            arg_exprs: Vec<reify_ir::CompiledExpr>,
        }

        let candidates: Vec<Candidate> = {
            let state = self.eval_state.as_ref().unwrap();
            let mut cands: Vec<Candidate> = Vec::new();
            for (c_id, node_data) in state.snapshot.graph.compute_nodes.iter() {
                // Only nodes with empty realization_inputs (no geometry args):
                // geometry-consuming nodes were already handled by the prior pass.
                if !node_data.realization_inputs.is_empty() {
                    continue;
                }
                if node_data.output_value_cells.is_empty() {
                    continue;
                }
                let output_cell = &node_data.output_value_cells[0];
                let entity_name = output_cell.entity.as_str();
                let Some(template) = module.templates.iter().find(|t| t.name == entity_name) else {
                    continue;
                };
                let Some(cell_decl) = template.value_cells.iter().find(|c| c.id == *output_cell) else {
                    continue;
                };
                let Some(default_expr) = &cell_decl.default_expr else {
                    continue;
                };
                if let reify_ir::CompiledExprKind::UserFunctionCall { args, .. } =
                    &default_expr.kind
                {
                    cands.push(Candidate {
                        c_id: c_id.clone(),
                        target: node_data.target.clone(),
                        output_cell: output_cell.clone(),
                        arg_exprs: args.clone(),
                    });
                }
            }
            cands
        };

        for cand in candidates {
            if self.compute_dispatch(&cand.target).is_none() {
                continue;
            }

            // Re-evaluate args with the now-hydrated values (material is non-degraded).
            let arg_values: Vec<reify_ir::Value> = {
                let ctx = crate::eval_ctx_with_meta(values, &self.functions, &self.meta_map);
                cand.arg_exprs.iter().map(|a| reify_expr::eval_expr(a, &ctx)).collect()
            };

            // Gate: at least one arg must be a non-degraded AsPrintedZones field.
            let has_ready_as_printed = arg_values.iter().any(|v| {
                matches!(
                    v,
                    reify_ir::Value::Field {
                        source: reify_ir::FieldSourceKind::AsPrintedZones,
                        lambda,
                        ..
                    }
                    if !matches!(lambda.as_ref(), reify_ir::Value::Undef)
                )
            });
            if !has_ready_as_printed {
                continue;
            }

            // No BRep tessellation needed (this is a field-consuming node, not
            // geometry-consuming): pass empty realization handles.
            let cancel = crate::graph::CancellationHandle::new();
            match self.run_compute_dispatch(
                &cand.c_id,
                std::slice::from_ref(&cand.output_cell),
                &cand.target,
                &arg_values,
                &[], // no realization handles — field input, not geometry
                &reify_ir::Value::Undef, // options
                &cancel,
                version_id,
                reify_core::ContentHash(0),
            ) {
                Ok((result, diags, _)) => {
                    diagnostics.extend(diags);
                    values.insert(cand.output_cell.clone(), result.clone());
                    if let Some(state) = self.eval_state.as_mut() {
                        state.snapshot.values.insert(
                            cand.output_cell.clone(),
                            (result, reify_ir::DeterminacyState::Determined),
                        );
                        if let Some(n) = state.snapshot.graph.get_compute_node_mut(&cand.c_id) {
                            n.running = None;
                        }
                    }
                }
                Err(_) => {
                    // Cascade dispatch failed — leave the cell as-is (Undef).
                }
            }
        }
    }

    /// Lightweight geometry-handle hydration for the tessellate path.
    ///
    /// Inserts `Value::GeometryHandle` entries into `values` for every named
    /// realization that has a resolved kernel handle in `named_steps`. This is
    /// the values-only subset of `post_process_geometry_handle_cells` — it does
    /// NOT touch `cache` or `realization_handles` (which are unavailable in the
    /// static `tessellate_from_values` function).
    ///
    /// Must run before `run_post_processes` so that topology selectors can
    /// resolve the parent `Value::GeometryHandle` via `values.get(arg_cell_id)`.
    fn hydrate_geometry_handles_into_values(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) {
        use reify_core::identity::ValueCellId;
        use reify_ir::Value;

        let mut entries: Vec<(ValueCellId, Value)> = Vec::new();
        {
            let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
            for realization in &template.realizations {
                let name = match &realization.name {
                    Some(n) => n.as_str(),
                    None => continue,
                };
                let kernel_handle = match named_steps.get(name) {
                    Some(kh) => kh.id,
                    None => continue,
                };
                let upstream_values_hash =
                    compute_realization_upstream_values_hash(realization, &ctx);
                entries.push((
                    ValueCellId::new(realization.id.entity.as_str(), name),
                    Value::GeometryHandle {
                        realization_ref: realization.id.clone(),
                        upstream_values_hash,
                        kernel_handle: Some(kernel_handle),
                    },
                ));
            }
        }
        for (cell_id, value) in entries {
            values.insert(cell_id, value);
        }
    }

    /// Kernel-free symbolic geometry handle mint for the pure-eval path (R2a, task #4652).
    ///
    /// For each named [`RealizationDecl`] across all templates in `module`,
    /// inserts `Value::GeometryHandle { realization_ref, upstream_values_hash,
    /// kernel_handle: None }` into `values` when the cell does not already hold
    /// a realized handle (`kernel_handle: Some(_)`).
    ///
    /// Mirrors [`Self::hydrate_geometry_handles_into_values`] but without a kernel:
    /// `kernel_handle` is `None` (symbolic/unrealized).  Called from
    /// `Engine::eval` and `Engine::eval_cached` after the scalar value-cell
    /// pass so that geometry constructors (`box()`, `bounding_box()`) resolve to
    /// a content-stable symbolic handle rather than `Value::Undef`.
    ///
    /// **§7.1 identity guarantee (R2a)**: `upstream_values_hash` uses the
    /// identical fold (seed `b"uvh1"`, per-op arg iteration, CrossSubGeometryRef
    /// skip, `b"uvh2"` lo/hi packing) as `post_process_geometry_handle_cells`
    /// and `hydrate_geometry_handles_into_values` so a subsequent `build()` call
    /// produces a realized handle that compares equal and hashes identically
    /// (GHR-β already excludes `kernel_handle` from `content_hash`/`PartialEq`).
    /// Per-cell helper for R3d dependency-ordered in-walk geometry-handle mint
    /// (task #4900).
    ///
    /// Looks up the named realization matching `cell_id` in `realizations`,
    /// computes `compute_realization_upstream_values_hash` over the now-resolved
    /// params already in `values` (guaranteeing byte-identical GHR-β identity
    /// with the build path), and returns `Value::GeometryHandle { kernel_handle:
    /// None, .. }`.  Returns `None` when:
    /// - no named realization matches `cell_id`;
    /// - the cell already holds a realized handle (`kernel_handle: Some(_)`).
    ///
    /// Must be called AFTER the cell's upstream params are in `values` (i.e. at
    /// the cell's topological slot) so the hash fold sees resolved values.
    pub(crate) fn mint_symbolic_geometry_handle_for_cell(
        cell_id: &reify_core::identity::ValueCellId,
        realizations: &[reify_compiler::RealizationDecl],
        values: &reify_ir::ValueMap,
        functions: &[reify_ir::CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) -> Option<reify_ir::Value> {
        use reify_ir::Value;

        // Don't clobber a realized handle stamped by the build path.
        if matches!(
            values.get(cell_id),
            Some(Value::GeometryHandle { kernel_handle: Some(_), .. })
        ) {
            return None;
        }
        // Find the named realization matching this cell (entity + member name).
        let realization = realizations.iter().find(|r| {
            r.name.as_deref() == Some(cell_id.member.as_str())
                && r.id.entity == cell_id.entity
        })?;
        let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
        let upstream_values_hash = compute_realization_upstream_values_hash(realization, &ctx);
        Some(Value::GeometryHandle {
            realization_ref: realization.id.clone(),
            upstream_values_hash,
            kernel_handle: None,
        })
    }

    /// Per-cell geometry-handle mint using the evaluation graph's `RealizationNodeData`.
    ///
    /// Semantically identical to [`Engine::mint_symbolic_geometry_handle_for_cell`]
    /// but works with graph-resident nodes (`RealizationNodeData.operations`) so
    /// callers that lack access to the `CompiledModule`'s `RealizationDecl` slice —
    /// specifically the `edit_param` reeval walk (R3d, task #4900) — can still mint
    /// an in-walk symbolic `GeometryHandle` with a byte-identical
    /// `upstream_values_hash` (GHR-β invariant preserved).
    ///
    /// The lookup key is `RealizationNodeData.geometry_cell` — the graph-side link
    /// from a realization to its backing `Type::Geometry` value cell, set at
    /// graph-construction time in `EvaluationGraph::from_templates`.
    pub(crate) fn mint_symbolic_geometry_handle_for_cell_from_graph(
        cell_id: &reify_core::identity::ValueCellId,
        graph: &crate::graph::EvaluationGraph,
        values: &reify_ir::ValueMap,
        functions: &[reify_ir::CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) -> Option<reify_ir::Value> {
        use reify_ir::Value;

        // Don't clobber a realized handle stamped by the build path.
        if matches!(
            values.get(cell_id),
            Some(Value::GeometryHandle { kernel_handle: Some(_), .. })
        ) {
            return None;
        }
        // Find the realization whose geometry_cell link points at this cell.
        let (rid, rnode) = graph
            .realizations
            .iter()
            .find(|(_, rnode)| rnode.geometry_cell.as_ref() == Some(cell_id))?;

        let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
        let upstream_values_hash =
            compute_realization_upstream_values_hash_from_ops(&rnode.operations, &ctx);
        Some(Value::GeometryHandle {
            realization_ref: rid.clone(),
            upstream_values_hash,
            kernel_handle: None,
        })
    }

    /// **No flip tracking (amendment, task #4946):** this handle-mint pass
    /// does not report which cells it flipped Undef/absent → non-Undef. An
    /// earlier draft threaded a `track_flips: bool` parameter and a
    /// `HashSet<ValueCellId>` return so a caller *could* feed this mint's
    /// flips into [`Engine::re_eval_consumers_of_in_walk_mints`] (or its
    /// `_from_graph` sibling) — mirroring the sibling
    /// [`crate::geometry_ops::mint_symbolic_topology_selectors_into_values`],
    /// whose flipped set IS consumed there. That wiring was tried and
    /// reverted: it regressed `restrict_field_b5_integration`
    /// (`v_in = sample(restrict(field, region), pt)`, `region` a bare
    /// geometry LET) — see each of the three call sites' comments
    /// (`engine_eval.rs`'s `eval()`/`eval_cached()`, `engine_edit.rs`'s
    /// `edit_source()`) for the full rationale: a direct, non-selector
    /// consumer of this mint's placeholder
    /// `GeometryHandle { kernel_handle: None, .. }` must stay `Value::Undef`
    /// here so `build()`'s later real-kernel post-process passes (which only
    /// revisit still-`Undef` cells) get a chance to resolve it for real.
    /// With every caller permanently declining to consume a flipped set,
    /// the bool + `HashSet` return was speculative generality on a hot path
    /// (`eval()`/`eval_cached()` run on every check/build/keystroke) —
    /// removed; this always takes the plain write-back path.
    pub(crate) fn mint_symbolic_geometry_handles_into_values(
        module: &CompiledModule,
        values: &mut ValueMap,
        functions: &[reify_ir::CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) {
        use reify_core::identity::ValueCellId;
        use reify_ir::Value;

        // Two-phase: collect while holding a &ValueMap borrow (via eval_ctx),
        // then write back via &mut ValueMap to avoid a split-borrow conflict.
        let mut entries: Vec<(ValueCellId, Value)> = Vec::new();
        {
            let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
            for realization in module.templates.iter().flat_map(|t| &t.realizations) {
                let name = match &realization.name {
                    Some(n) => n.as_str(),
                    None => continue, // unnamed realizations have no named cell
                };
                let cell_id = ValueCellId::new(realization.id.entity.as_str(), name);
                // Do not clobber a realized handle already stamped by the build path.
                if matches!(
                    values.get(&cell_id),
                    Some(Value::GeometryHandle { kernel_handle: Some(_), .. })
                ) {
                    continue;
                }
                // Delegate to the single canonical fold (step-6, task #4652):
                // guarantees byte-identical upstream_values_hash with the build
                // path so eval-mint == build-realize (§7.1 identity, GHR-β).
                let upstream_values_hash =
                    compute_realization_upstream_values_hash(realization, &ctx);
                entries.push((
                    cell_id,
                    Value::GeometryHandle {
                        realization_ref: realization.id.clone(),
                        upstream_values_hash,
                        kernel_handle: None,
                    },
                ));
            }
        } // ctx dropped — &ValueMap borrow released
        // No caller reads a flipped set (see doc comment above) — just write
        // back the mint results, skipping any per-entry `values.get`
        // re-probe or `HashSet` allocation.
        for (cell_id, value) in entries {
            values.insert(cell_id, value);
        }
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`.
    ///
    /// For each `ValueCellDecl` in `template.value_cells` whose `default_expr`
    /// is a recognised conformance-query helper (`is_watertight`,
    /// `is_manifold`, `is_orientable`), this writes the kernel-resolved
    /// `Value::Bool(_)` answer (or the user-assertion override) into
    /// `values`, overwriting the `Value::Undef` left behind by the pure
    /// `eval_expr` path. Cells whose `default_expr` is `None` or whose
    /// dispatch returns `None` (literal arg, unresolvable cell-member name,
    /// non-helper function call) are left untouched — see
    /// [`crate::geometry_ops::try_eval_conformance_query`]'s `None`-return
    /// contract.
    ///
    /// Called once per template from `build` / `build_snapshot` and
    /// `tessellate_realizations` / `tessellate_snapshot` after each path's
    /// per-realization loop has populated `named_steps`. Tessellation
    /// itself does not consume value cells, but the surfaced
    /// `TessellateResult.values` map *is* read by callers (e.g. GUI
    /// overlays that show query-helper results next to a mesh), so the
    /// post-process must run on those paths too — without it, the
    /// tessellate surface would expose `Value::Undef` for these cells
    /// while the build surface exposes the kernel-resolved Bool.
    ///
    /// Pinned by `tests/conformance_runtime.rs::*` (task 2320 step-11)
    /// and the tessellate-path coverage in
    /// `tessellate_realizations_post_processes_conformance_queries`
    /// (task 2320 amendment).
    fn post_process_conformance_queries(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_conformance_query(
                default_expr,
                &template.trait_bounds,
                named_steps,
                kernel,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`, dispatching the kinematic-query helpers
    /// `interferes` / `interferes_with` / `min_clearance` (task 2531).
    ///
    /// Sibling to `post_process_conformance_queries`. For each
    /// `ValueCellDecl` in `template.value_cells` whose `default_expr` is a
    /// recognised kinematic-query helper, this writes the kernel-resolved
    /// value (`Value::List(_)`, `Value::Bool(_)`, or
    /// `Value::Scalar { dimension: LENGTH, .. }`) into `values`,
    /// overwriting the `Value::Undef` left behind by the pure `eval_expr`
    /// path. Cells whose dispatch returns `None` (literal arg, missing
    /// snapshot in `values`, non-helper function call) are left untouched.
    ///
    /// Called from the same three sites as
    /// `post_process_conformance_queries` so build / build_snapshot /
    /// tessellate paths agree on the patched value.
    fn post_process_kinematic_queries(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `values` directly without snapshotting (parallels the
        // `post_process_conformance_queries` sibling above). Safe because
        // none of the kinematic helpers chain — a later cell's dispatch
        // reads `args[0]` as a `ValueRef` to a Snapshot let-cell filled by
        // the regular `eval_expr` pass, never to another kinematic-query
        // cell, so an earlier patch in this loop cannot influence a later
        // dispatch's input.
        // Pose cache shared across all kinematic-query cells in this template:
        // a typical structure calls interferes/interferes_with/min_clearance on
        // the same snapshot, so without a cache each non-identity body's
        // world_transform is re-applied once per query. The cache is keyed on
        // (source handle id, rotation bits, translation bits) and lives only
        // for the duration of this post-process call — handle ids are
        // build-local and must not cross build passes.
        let mut pose_cache: HashMap<
            (reify_ir::GeometryHandleId, [u64; 4], [u64; 3]),
            reify_ir::GeometryHandleId,
        > = HashMap::new();
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_kinematic_query(
                default_expr,
                named_steps,
                values,
                kernel,
                diagnostics,
                &mut pose_cache,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Post-process value cells for a template, dispatching the RBD-β
    /// `body_mass_props(body, density?)` dynamics-query builtin (task 3829;
    /// PRD `docs/prds/v0_3/rigid-body-dynamics.md` §2.1/§5.4).
    ///
    /// Sibling to `post_process_conformance_queries` /
    /// `post_process_kinematic_queries`. For each `ValueCellDecl` whose
    /// `default_expr` is a recognised `body_mass_props(...)` call,
    /// [`crate::dynamics_ops::try_eval_body_mass_props`] runs the density
    /// priority ladder (emitting `E_DynamicsNoDensity` when no density resolves)
    /// and writes the assembled `MassProperties` `StructureInstance`
    /// into `values`, overwriting the `Value::Undef` left by the pure
    /// `eval_expr` path (the builtin `FunctionCall` has no pure-eval rule).
    /// Cells whose dispatch returns `None` (non-call expr, a different function
    /// name, an unresolvable body arg) are left untouched — the geometry_ops
    /// `None`-means-skip contract.
    ///
    /// The KGQ kernel query is wired (task 4237 / KGQ-λ): when the body
    /// resolves to a `Value::GeometryHandle`,
    /// [`crate::dynamics_ops::try_eval_body_mass_props`] routes the
    /// Volume / CenterOfMass / InertiaTensor queries through the kernel, so
    /// the geometric fields (`mass`/`com`/`inertia`) carry real values.
    /// Bodies without a geometry handle (and kernel-error downgrades) keep
    /// the deferred `Value::Undef` sentinel; the existing MassProperties
    /// PSD hook (engine_eval.rs) classifies an `Undef` inertia as `Skip`, so
    /// such instances are neither clobbered nor flagged.
    ///
    /// **Ordering contract (task 4538):** this pass runs AFTER both selector
    /// passes (`post_process_topology_selectors` / `post_process_ad_hoc_selectors`)
    /// inside `run_post_processes`. A body produced by a selector (e.g.
    /// `single(edges(s))`) would still be `Value::Undef` if this pass ran
    /// first, causing the kernel queries to be silently skipped. The ordering
    /// is pinned by the regression test
    /// `run_post_processes_selector_produced_body_gets_real_mass_props`.
    ///
    /// Takes `kernel: &dyn GeometryKernel` (immutable — the dispatch only holds
    /// the kernel for the geometric query and does not mutate it);
    /// `run_post_processes` reborrows its `&mut dyn` kernel as `&*kernel`.
    /// Called from `run_post_processes` so build / build_snapshot /
    /// tessellate_from_values agree on the patched value (task 3745).
    fn post_process_body_mass_props(
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `values` directly without snapshotting (parallels the
        // `post_process_kinematic_queries` sibling above). Safe because
        // `body_mass_props` does not chain through value cells — its body arg
        // resolves to a let-bound `Value` already populated by `eval_expr`,
        // never to another `body_mass_props` cell, so an earlier patch in this
        // loop cannot influence a later dispatch's input. The immutable
        // `values` borrow taken by `try_eval_body_mass_props` ends before the
        // owned `Value` is inserted.
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::dynamics_ops::try_eval_body_mass_props(
                default_expr,
                values,
                kernel,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Build-time mechanism-mass pre-derivation pass (task 4472, rung (b)).
    ///
    /// Iterates all entries in `values`, calls
    /// [`crate::dynamics_ops::derive_mechanism_mass_props`] on each, and
    /// writes back any `Some(patched)` results after the iteration loop (so
    /// the immutable borrow from `values.iter()` is fully released before the
    /// mutable insert). Non-mechanism cells and mechanism cells with no
    /// geometry-backed body are silently skipped (the `None`-means-skip
    /// post-process contract).
    ///
    /// Takes `kernel: &dyn GeometryKernel` (immutable — the derivation pass
    /// only issues read-only KGQ round-trips and does not mutate the kernel);
    /// `run_post_processes` reborrows its `&mut dyn` kernel as `&*kernel`.
    /// Wired into `run_post_processes` AFTER the selector passes (resolves the
    /// task-3620 ordering guard — see the comment in `run_post_processes`).
    fn post_process_mechanism_mass_props(
        values: &mut ValueMap,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Collect all patched (id, value) pairs first, then insert — avoids
        // holding the immutable `values.iter()` borrow while mutating `values`.
        let patches: Vec<(reify_core::identity::ValueCellId, reify_ir::Value)> = values
            .iter()
            .filter_map(|(id, v)| {
                crate::dynamics_ops::derive_mechanism_mass_props(v, kernel, diagnostics)
                    .map(|patched| (id.clone(), patched))
            })
            .collect();
        for (id, patched) in patches {
            values.insert(id, patched);
        }
    }

    /// Task 4358 ε: hydrate a SINGLE value cell at its scheduled slot under
    /// [`crate::engine_fixpoint::BuildScheduler::UnifiedDag`], mirroring the
    /// per-cell body of `post_process_geometry_queries` +
    /// `post_process_topology_selectors` for one cell instead of looping the whole
    /// template. Driven by the `HydrateCell` build step so a geometry-query cell
    /// (`volume`/`area`/`centroid`/`bounding_box`) or a topology-selector cell
    /// (`edges_at_height` / `closest_point` / a `ResolveSelector` coercion …)
    /// resolves the moment its producing realization(s) complete — BEFORE a later
    /// realization in the Kahn schedule consumes it (e.g. a curated
    /// `fillet(solid, edges, radius)` reads the resolved edge `List` rather than
    /// `Undef`).
    ///
    /// # Selector cells consumed by a realization resolve to a `List`, not a `Selector`
    ///
    /// A curated edge/face selector (`edges_at_height`, `faces_by_normal`, …) is a
    /// `Value::Selector`-typed cell whose `try_eval_topology_selector` result is a
    /// kernel-FREE `Value::Selector` DESCRIPTOR (task 4118 γ). A consuming curated
    /// `fillet(solid, edges, radius)` realization, however, reads its `edges` arg
    /// as a `Value::List<Geometry>` — the legacy `compile_geometry_op` Fillet arm
    /// errors ("curated edge selection is not yet available …") on a bare
    /// descriptor, the exact P2-before-P4 staging gap tasks 4360/4358 close. So
    /// when this selector cell is read by ANY realization (`realization_read_cells`
    /// = the union of every realization trace's `reads`), the descriptor is
    /// resolved one step further to its concrete sub-handle `List` via
    /// `resolve_selector_to_list` (the kernel-bearing query runs HERE, at the
    /// scheduled slot where the parent solid is already realized). Selector cells
    /// consumed ONLY by selector-composition value cells
    /// (`union`/`intersect`/`difference`, whose `reconstruct_selector_value`
    /// REQUIRES a `Value::Selector` child) are NOT in `realization_read_cells`, so
    /// they keep their descriptor form and composition stays correct. The negative
    /// side of this gate (composition-only child selectors keep their descriptors so
    /// a curated fillet over `union(e1, e2)` still resolves non-empty edges in-loop)
    /// is pinned by `tests/unified_dag_geometry_executors.rs::
    /// unified_dag_curated_fillet_over_selector_composition_resolves_edges`.
    ///
    /// Resolution order otherwise matches `run_post_processes` (geometry query →
    /// selector→list → topology selector → resolve-selector coercion → feature
    /// accessor); the first helper that returns `Some` wins. A cell whose
    /// `default_expr` is not a recognised query/selector is left untouched. Only
    /// the *timing* (before vs.
    /// after the consuming realization) differs from the whole-template
    /// post-process below, and only under UnifiedDag. Pinned by
    /// `unified_dag_curated_fillet_resolves_edges_in_loop`.
    ///
    /// SYNC REQUIREMENT: this single-cell ladder and the whole-template pass order
    /// in [`Engine::run_post_processes`] MUST change together — see the matching
    /// "SYNC REQUIREMENT" note on that function. A divergence would change which
    /// helper wins for a given cell only under UnifiedDag, only in-loop.
    #[allow(clippy::too_many_arguments)]
    fn hydrate_value_cell_in_loop(
        template: &reify_compiler::TopologyTemplate,
        cell_id: &reify_core::ValueCellId,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        kernel: &mut dyn GeometryKernel,
        table: &TopologyAttributeTable,
        realization_read_cells: &HashSet<reify_core::ValueCellId>,
        realized_reprs: &HashMap<RealizationNodeId, ReprKind>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let Some(cell) = template.value_cells.iter().find(|c| &c.id == cell_id) else {
            return;
        };
        let Some(default_expr) = cell.default_expr.as_ref() else {
            return;
        };
        // (a) whole-handle geometry query (volume/area/centroid/bounding_box,
        //     incl. the nested operand-cell case). Read-only kernel access.
        if let Some(value) = crate::geometry_ops::try_eval_geometry_query(
            default_expr,
            named_steps,
            values,
            functions,
            meta_map,
            &*kernel,
            diagnostics,
        ) {
            values.insert(cell.id.clone(), value);
            return;
        }
        // (b) selector cell consumed by a realization → resolve the descriptor to
        //     its concrete `List<Geometry>` sub-handles so the consuming curated
        //     fillet/chamfer/draft realization reads a List (see the doc comment).
        //     Gated on `realization_read_cells` so composition-only selector cells
        //     keep their `Value::Selector` descriptor. `resolve_selector_to_list`
        //     returns `None` for a non-selector expr, so a non-selector
        //     realization-read cell (e.g. a scalar param) falls through to (c)/(d).
        if realization_read_cells.contains(&cell.id)
            && let Some(value) = crate::geometry_ops::resolve_selector_to_list(
                default_expr,
                named_steps,
                values,
                kernel,
                table,
                realized_reprs,
                diagnostics,
            )
        {
            values.insert(cell.id.clone(), value);
            return;
        }
        // (c) topology selector descriptor / scalar / bool / point (closest_point /
        //     is_on / angle_between_surfaces / edges_at_height / …).
        if let Some(value) = crate::geometry_ops::try_eval_topology_selector(
            default_expr,
            named_steps,
            values,
            kernel,
            diagnostics,
        ) {
            values.insert(cell.id.clone(), value);
            return;
        }
        // (d) ResolveSelector coercion → `List<Geometry>` (curated edge/face
        //     selectors consumed by a 3-arg fillet/chamfer).
        if let Some(value) = crate::geometry_ops::try_eval_resolve_selector(
            default_expr,
            named_steps,
            values,
            kernel,
            table,
            realized_reprs,
            diagnostics,
        ) {
            values.insert(cell.id.clone(), value);
            return;
        }
        // (e) explicit projection `feature(geometry) : Feature` (PRD D1, task
        //     4830, P3α). Placed LAST — after (c)/(d) so a sub-shape arg (e.g.
        //     `faces(b)[0]`) is already hydrated to a `Value::GeometryHandle`
        //     before resolution. Mirrors the placement of
        //     `Engine::post_process_feature_accessor` in `run_post_processes`
        //     (after the topology-selector-family passes).
        if let Some(value) = crate::geometry_ops::try_eval_feature_accessor(
            default_expr,
            values,
            table,
            diagnostics,
        ) {
            values.insert(cell.id.clone(), value);
        }
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`, dispatching the whole-handle geometry
    /// queries `volume` / `area` / `centroid` / `bounding_box` on a
    /// `Value::GeometryHandle` (task 3608, GHR-ζ; PRD
    /// `docs/prds/v0_3/geometry-handle-runtime.md` §8 Phase 6).
    ///
    /// Sibling to `post_process_conformance_queries` /
    /// `post_process_body_mass_props`. For each `ValueCellDecl` whose
    /// `default_expr` is a recognised geometry-query call,
    /// [`crate::geometry_ops::try_eval_geometry_query`] resolves the handle and
    /// dispatches to the kernel, writing the typed `Value` (`Scalar<Volume>` /
    /// `Scalar<Area>` / `Point3<Length>` / `BoundingBox`) into `values`,
    /// overwriting the `Value::Undef` left by the pure `eval_expr` path (these
    /// geometry-query builtins have no pure-eval rule). Cells whose dispatch
    /// returns `None` (non-call expr, a different function name, an unresolvable
    /// handle arg) are left untouched — the geometry_ops `None`-means-skip
    /// contract.
    ///
    /// Takes `kernel: &dyn GeometryKernel` (immutable — the dispatch only issues
    /// read-only `kernel.query(...)` round-trips and does not mutate the
    /// kernel); `run_post_processes` reborrows its `&mut dyn` kernel as
    /// `&*kernel`. Wired into `run_post_processes` (task 3745 consolidation
    /// point) so build / build_snapshot / tessellate_from_values all pick it up.
    fn post_process_geometry_queries(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `template.value_cells` and insert into `values` in place,
        // without snapshotting `values` (parallels the
        // `post_process_body_mass_props` sibling). The DIRECT case is safe: a
        // geometry-query cell's arg resolves to a `named_steps` handle
        // (populated by `execute_realization_ops`), never to another value cell.
        // The NESTED case (`try_eval_geometry_query` step-10) reads operand
        // cells from `values` (e.g. `material.density`) — those are non-query
        // cells populated by the eval pass that produced `values`
        // (engine_build.rs:1802), which this loop never overwrites (it inserts
        // only into geometry-query cells), so their values are independent of
        // iteration order. `functions` / `meta_map` build the `EvalContext` for
        // that nested recompute.
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_geometry_query(
                default_expr,
                named_steps,
                values,
                functions,
                meta_map,
                kernel,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Post-process value cells for a template, dispatching the explicit
    /// projection `feature(geometry) : Feature` (PRD D1, task 4830, P3α).
    ///
    /// Sibling to `post_process_geometry_queries`, using the same
    /// collect-nothing / insert-in-place loop shape (`feature()` issues no
    /// kernel query, so there is no borrow hazard to avoid). For each
    /// `ValueCellDecl` whose `default_expr` is a `feature(...)` call,
    /// [`crate::geometry_ops::try_eval_feature_accessor`] resolves the arg's
    /// handle — a `table`-recorded sub-shape wins, else the handle's
    /// realization — and writes the resulting `Value::Feature(_)` into
    /// `values`, overwriting the `Value::Undef` left by the pure `eval_expr`
    /// path. Cells whose dispatch returns `None` (non-call expr, a different
    /// function name, or an arg that doesn't resolve to a realized
    /// `Value::GeometryHandle` — which also pushes a
    /// `QueryNotSupportedOnRepr` diagnostic, OQ#2) are left untouched.
    ///
    /// Wired into `run_post_processes` AFTER the selector passes (so
    /// table-keyed sub-shape handles are already populated — see that
    /// function's call-site comment) and mirrored into
    /// `hydrate_value_cell_in_loop` per the documented SYNC REQUIREMENT.
    fn post_process_feature_accessor(
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        table: &TopologyAttributeTable,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_feature_accessor(
                default_expr,
                values,
                table,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Run all selector / AdHocSelector post-process passes for a template
    /// after `execute_realization_ops` has populated `named_steps`.
    ///
    /// Calls `post_process_topology_selectors` then
    /// `post_process_ad_hoc_selectors` in order, consolidating the identical
    /// two-call block that previously appeared verbatim in `build`,
    /// `build_snapshot`, and `tessellate_from_values` (task 3745).  Any future
    /// sibling passes should be added here so all three call sites pick them up
    /// automatically.
    ///
    /// `functions` / `meta_map` build the `EvalContext` that
    /// `post_process_geometry_queries` uses to recompute nested geometry-query
    /// expressions (GHR-ζ step-10, e.g. `mass = volume(g) * material.density`).
    ///
    /// # SYNC REQUIREMENT with [`Engine::hydrate_value_cell_in_loop`] (task 4358 ε)
    ///
    /// The UnifiedDag schedule-driven build loop hydrates a SINGLE value cell at
    /// its scheduled slot via [`Engine::hydrate_value_cell_in_loop`], which mirrors
    /// the per-cell resolution ladder this whole-template pass applies (geometry
    /// query → selector→list → topology selector → resolve-selector coercion →
    /// feature accessor). The two sites MUST stay in sync: if the ORDER or the SET
    /// of helpers below changes, the in-loop single-cell ladder in
    /// `hydrate_value_cell_in_loop` must change identically, or a cell's
    /// resolution would diverge (which helper "wins") only under UnifiedDag, only
    /// when that cell is hydrated in-loop ahead of a consuming realization. See
    /// that function's doc comment for the matching ladder and the rationale for
    /// the one deliberate divergence (a realization-consumed selector is resolved
    /// one step further, to a `List`).
    //
    // `functions` + `meta_map` (added by GHR-ζ for the geometry-query EvalContext)
    // push this consolidator to 8 args; matches the sibling post-process helpers'
    // allow (e.g. post_process_geometry_handle_cells at line 3694).
    #[allow(clippy::too_many_arguments)]
    fn run_post_processes(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        kernel: &mut dyn GeometryKernel,
        table: &TopologyAttributeTable,
        swept_kinds: &SweptKindTable,
        realized_reprs: &HashMap<RealizationNodeId, ReprKind>,
        diagnostics: &mut Vec<Diagnostic>,
        templates: &[reify_compiler::TopologyTemplate],
    ) {
        // GHR-ζ (task 3608): whole-handle geometry-query dispatch
        // (volume / area / centroid / bounding_box). Added here — rather than a
        // separate explicit call at each build / build_snapshot /
        // tessellate_from_values site — so all three sites pick it up
        // automatically (task 3745 consolidation contract). Reborrows the `&mut`
        // kernel as `&dyn`: the dispatch only issues read-only queries.
        // Order-independent w.r.t. the sibling passes — geometry-query cells are
        // not consumed by body_mass_props or the selector passes, and this pass
        // reads only `named_steps` handles + eval_expr-populated cells.
        Engine::post_process_geometry_queries(
            template,
            named_steps,
            values,
            functions,
            meta_map,
            &*kernel,
            diagnostics,
        );
        Engine::post_process_topology_selectors(
            template,
            named_steps,
            values,
            kernel,
            table,
            realized_reprs,
            diagnostics,
        );
        // geometric-relations ε: feature → datum projections (`feature.axis` /
        // `.plane` / `.point` / `.dir`). Placed AFTER post_process_topology_selectors
        // so the receiver body handles (`let cyl = revolve(...)`) are populated as
        // `Value::GeometryHandle` cells, and BEFORE post_process_derived_lets so a
        // pure let depending on a projected datum sees the patched value.
        Engine::post_process_feature_datum_projections(
            template,
            values,
            kernel,
            swept_kinds,
            diagnostics,
        );
        // task 4725: fold scoped scalar cross-sub value cells
        // (`ValueCellId("<parent>.<sub>", member)`, e.g. `self.b01.mass`) from
        // each sub-instance's realized geometry. Must run BEFORE
        // `post_process_derived_lets` so a parent aggregate cell (e.g.
        // `total_mass = all_masses.sum`) observes the folded scoped cells in
        // the same fixpoint pass below.
        Engine::post_process_cross_sub_value_cells(
            template,
            named_steps,
            values,
            functions,
            meta_map,
            &*kernel,
            diagnostics,
            templates,
        );
        // task 4229: re-evaluate Let cells whose expressions depend on
        // topology-selector-derived cells (e.g. `moi_principal =
        // eigenvalues(moment_of_inertia)` where `moment_of_inertia` was just
        // patched above). Must run after `post_process_topology_selectors` so
        // the patched values are visible.
        Engine::post_process_derived_lets(template, values, functions, meta_map, diagnostics);
        // DFM let-cell diagnostic harvest (task #4734 A1): fold bounding_box leaves in
        // fits_build_volume-bearing Let cells and emit W/E/I_DFM_BUILD_VOLUME diagnostics.
        // Runs after post_process_geometry_queries (handles already in values) and
        // post_process_derived_lets (pure-math lets resolved). Kernel + named_steps are
        // available here, enabling the geometry-query fold.
        Engine::harvest_dfm_let_diagnostics(
            template,
            named_steps,
            values,
            functions,
            meta_map,
            &*kernel,
            diagnostics,
        );
        Engine::post_process_ad_hoc_selectors(
            template,
            named_steps,
            values,
            kernel,
            table,
            diagnostics,
        );
        // P3α (task 4830): explicit projection `feature(geometry) : Feature`
        // (PRD D1). Placed AFTER post_process_topology_selectors AND
        // post_process_ad_hoc_selectors so table-keyed sub-shape handles (e.g.
        // a curated `faces(b)[0]`) are already hydrated to `Value::GeometryHandle`
        // cells before a `feature(...)` accessor resolves its arg. Dedicated
        // pass (not folded into post_process_geometry_queries above): `feature()`
        // issues no kernel query and needs `table` for sub-shape resolution,
        // neither of which that pass threads.
        Engine::post_process_feature_accessor(template, values, table, diagnostics);
        // RBD-β (task 3829): body_mass_props dispatch. Added here — rather than
        // a fourth explicit call at each build / build_snapshot /
        // tessellate_from_values site — so all three sites pick it up
        // automatically (task 3745 consolidation contract). Reborrows the
        // `&mut` kernel as `&dyn`: the dispatch only holds the kernel for the
        // geometric query and does not mutate it.
        //
        // ORDERING CONTRACT (task 4538): this pass runs LAST — after
        // post_process_geometry_queries, post_process_topology_selectors, and
        // post_process_ad_hoc_selectors — so every handle-producing pass has
        // populated body handles before mass-props reads them. A body whose
        // cell is produced by a selector pass (e.g. `single(edges(s))`) would
        // still be `Value::Undef` when mass-props ran in the old (pre-4538)
        // position, yielding `Undef` geometric fields even though the KGQ
        // kernel query is live (task 4237 / KGQ-λ). The correct order is
        // enforced by the regression test
        // `run_post_processes_selector_produced_body_gets_real_mass_props`
        // (engine_build.rs tests, task 4538 step-1).
        //
        // No inverse dependency: the selector and geometry-query passes consume
        // geometry handles / points, never a MassProperties value, so this call
        // has no consumer within run_post_processes and is safe to run last.
        //
        // Sibling task 4472 (post_process_mechanism_mass_props) is also
        // specified to run after the selector passes; when added it should be
        // placed here, after post_process_body_mass_props.
        Engine::post_process_body_mass_props(template, values, &*kernel, diagnostics);
        // Mechanism-mass pre-derivation pass (task 4472, rung (b)). Placed here,
        // after post_process_body_mass_props, exactly as the ORDERING CONTRACT
        // above (task 4538) directs: both mass-props passes run AFTER the
        // selector passes, so every handle-producing pass has populated body
        // handles before either pass issues its LIVE (non-deferred) per-body
        // kernel query. Running this before the selector passes would risk
        // reading a mechanism body whose value a selector post-process has not
        // yet populated. This is the mechanism-body half of the task-3620
        // wiring that task 4538 re-evaluated and resolved by moving the
        // body-mass pass last; the same resolution covers this sibling pass.
        Engine::post_process_mechanism_mass_props(values, &*kernel, diagnostics);
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`, dispatching the topology-selector helpers
    /// `closest_point` / `is_on` / `angle_between_surfaces` (task 2324).
    ///
    /// Sibling to `post_process_conformance_queries` and
    /// `post_process_kinematic_queries`. For each `ValueCellDecl` in
    /// `template.value_cells` whose `default_expr` is a recognised
    /// topology-selector helper, this writes the kernel-resolved value
    /// (`Value::Point(_)` for `closest_point`, `Value::Bool(_)` for `is_on`,
    /// `Value::Scalar { dimension: ANGLE, .. }` for `angle_between_surfaces`)
    /// into `values`, overwriting the `Value::Undef` left behind by the pure
    /// `eval_expr` path. Cells whose dispatch returns `None` (literal arg,
    /// missing `named_steps` or `values` entry, non-helper function call)
    /// are left untouched — see
    /// [`crate::geometry_ops::try_eval_topology_selector`]'s `None`-return
    /// contract.
    ///
    /// Called from the same three sites as `post_process_conformance_queries`
    /// and `post_process_kinematic_queries` so build / build_snapshot /
    /// tessellate paths agree on the patched value.
    fn post_process_topology_selectors(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
        table: &TopologyAttributeTable,
        realized_reprs: &HashMap<RealizationNodeId, ReprKind>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `values` directly without snapshotting (parallels the
        // `post_process_kinematic_queries` sibling above). Safe because
        // topology-selector helpers do not chain through value cells —
        // each helper's args resolve to either a let-bound `Value::Point`
        // already populated by `eval_expr` or a `named_steps` handle
        // populated by `execute_realization_ops`, never to another
        // topology-selector cell, so an earlier patch in this loop cannot
        // influence a later dispatch's input.
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_topology_selector(
                default_expr,
                named_steps,
                values,
                kernel,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            } else if let Some(value) = crate::geometry_ops::try_eval_resolve_selector(
                // Task 4118 (γ): the compiler-inserted `ResolveSelector` coercion
                // node (and `IndexAccess` over a selector) resolves a typed
                // `Value::Selector` cell to a `Value::List<Geometry>` HERE. The
                // inner selector is reconstructed INLINE from its nested
                // FunctionCall, so the "do not chain through value cells"
                // invariant above is preserved — no dependency on another
                // selector cell already being patched in this loop.
                //
                // Task 4536: `table` carries the realized body's recorded
                // topology attributes so a `mid_surface(body)` (`ByRole`) leaf
                // resolves against `Role::MidSurfaceFace` entries.
                default_expr,
                named_steps,
                values,
                kernel,
                table,
                realized_reprs,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// Post-process value cells whose initializer is a feature → datum projection
    /// (`feature.axis` / `.plane` / `.point` / `.dir`), geometric-relations ε
    /// (design §7.2).
    ///
    /// The compiler lowers such a projection to a `MethodCall` whose receiver is
    /// a realized `Value::GeometryHandle` cell; the pure `eval_expr` path cannot
    /// reach the kernel, the construction history, or the dedup primitive, so it
    /// leaves the cell at `Value::Undef`. This pass resolves each still-`Undef`
    /// cell via [`crate::geometry_ops::try_eval_feature_datum_projection`], which
    /// builds the feature's deduplicated datum bundle (analytic ∪ the
    /// `swept_kinds` construction history) and refines it to the requested
    /// projection — a unique datum ⇒ its `Value`, a zero/many group ⇒ a
    /// select-a-subfeature `FeatureDatumAmbiguous` error + `Value::Undef`.
    ///
    /// Cells whose dispatch returns `None` (non-projection initializer, or a
    /// receiver that is not a realized geometry handle — e.g. a β datum receiver
    /// `axis.dir`, owned by the pure projection path) are left untouched.
    ///
    /// **Ordering contract**: must run AFTER `post_process_topology_selectors` so
    /// the receiver body handles are populated, and BEFORE
    /// `post_process_derived_lets` so a pure let depending on a projected datum
    /// sees the patched value.
    fn post_process_feature_datum_projections(
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
        swept_kinds: &SweptKindTable,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Collect (cell id, expr) for still-`Undef` cells first, to avoid holding
        // a borrow on `values` while also inserting into it (parallels
        // `post_process_derived_lets`). A projection cell is `Undef` after the
        // pure eval pass, so the filter is both an optimisation and correct.
        let candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = template
            .value_cells
            .iter()
            .filter(|cell| values.get(&cell.id).is_none_or(|v| v.is_undef()))
            .filter_map(|cell| {
                cell.default_expr
                    .as_ref()
                    .map(|e| (cell.id.clone(), e.clone()))
            })
            .collect();

        for (cell_id, expr) in candidates {
            if let Some(value) = crate::geometry_ops::try_eval_feature_datum_projection(
                &expr,
                values,
                kernel,
                swept_kinds,
                diagnostics,
            ) {
                values.insert(cell_id, value);
            }
        }
    }

    /// Fold scoped scalar cross-sub value cells — `ValueCellId("<parent>.<sub>",
    /// member)` — that stayed `Value::Undef` after the pure eval pass (task 4725).
    ///
    /// `self.<sub>.<member>` cross-sub SCALAR member access
    /// (reify-compiler/src/expr.rs, the `Some(ty)` branch of the non-collection
    /// sub member-access match) compiles to a plain `ValueRef` keyed
    /// `ValueCellId("<parent>.<sub>", member)` — deliberately NOT
    /// `CrossSubGeometryRef` (reserved for genuine geometry-realization members
    /// like `self.sub.body`, which `unreachable!()`s in `reify_expr::eval_expr`
    /// outside a bare-let top level). `elaborate_child_lets_only` (unfold.rs)
    /// already creates this scoped cell during the PURE eval pass, evaluating
    /// the child template's own `default_expr` (e.g. `mass = volume(geometry) *
    /// material.density`) — but a geometry-query leaf can't resolve without a
    /// kernel there, so the cell folds to `Value::Undef`.
    ///
    /// This pass re-attempts that fold with kernel access. For each
    /// non-collection sub-component, for each of the child template's OWN
    /// value cells that is still `Undef` at its scoped id, this takes the
    /// child cell's `default_expr` and rescopes every `ValueRef` (via
    /// [`reify_ir::CompiledExpr::map_value_refs`]) whose entity is the child
    /// template's own name to the sub-instance's scoped entity, then
    /// dispatches the rescoped expression exactly like the per-DEF fold
    /// ([`crate::geometry_ops::try_eval_geometry_query`]):
    ///
    ///   - A rescoped geometry-query leaf's arg (e.g.
    ///     `ValueRef("<parent>.<sub>", "geometry")`) resolves through
    ///     `resolve_geometry_handle_arg`'s existing dotted-entity branch to the
    ///     compound key `named_steps["<sub>.<member>"]` — the same key
    ///     `seed_cross_sub_named_steps` (task 3441) already populates.
    ///   - Non-query operands (e.g. `material.density`) resolve against the
    ///     scoped `values` cells already populated by
    ///     `elaborate_child_params_only` / `elaborate_child_lets_only`
    ///     (unfold.rs), which reflect any per-instance param overrides.
    ///
    /// Cells whose rescoped expr contains a `CrossSubGeometryRef` are skipped
    /// (same guard as `post_process_derived_lets` below — that variant
    /// `unreachable!()`s in `reify_expr::eval_expr`), as are cells whose
    /// dispatch returns `None` (no geometry-query leaf, or an unresolvable
    /// handle) — they are left at their compiled `Value::Undef`.
    ///
    /// **Ordering contract**: must run BEFORE `post_process_derived_lets` so
    /// the parent's aggregate `Let` cells (e.g. `total_mass = all_masses.sum`)
    /// observe these freshly-folded scoped cells in the same fixpoint pass.
    #[allow(clippy::too_many_arguments)]
    fn post_process_cross_sub_value_cells(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
        templates: &[reify_compiler::TopologyTemplate],
    ) {
        // Collect (scoped_cell_id, rescoped_expr) candidates first to avoid
        // holding a borrow on `values` while also inserting into it (parallels
        // `post_process_derived_lets` / `post_process_feature_datum_projections`).
        let mut candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = Vec::new();

        for sub in &template.sub_components {
            // Collection subs have no single scoped entity to fold into (each
            // element would need its own indexed scope); out of scope here,
            // mirroring `seed_cross_sub_named_steps`'s no-collection-subs
            // contract.
            if sub.is_collection {
                continue;
            }
            let Some(child_template) =
                reify_compiler::find_template(templates, &sub.structure_name)
            else {
                continue;
            };
            let scoped_entity = format!("{}.{}", template.name, sub.name);
            let child_entity = child_template.name.as_str();

            for child_cell in &child_template.value_cells {
                let Some(default_expr) = &child_cell.default_expr else {
                    continue;
                };
                let scoped_id = reify_core::ValueCellId::new(
                    scoped_entity.clone(),
                    child_cell.id.member.as_str(),
                );
                if values.get(&scoped_id).is_some_and(|v| !v.is_undef()) {
                    // Already folded (e.g. a Param cell populated by
                    // `elaborate_child_params_only`) — nothing to do.
                    continue;
                }
                let rescoped = default_expr.clone().map_value_refs(&mut |id| {
                    if id.entity == child_entity {
                        reify_core::ValueCellId::new(scoped_entity.clone(), id.member)
                    } else {
                        id
                    }
                });
                // Skip CrossSubGeometryRef expressions — same guard as
                // `post_process_derived_lets`; that variant `unreachable!()`s in
                // `reify_expr::eval_expr` outside a bare-let top level.
                if arg_contains_cross_sub_geometry_ref(&rescoped) {
                    continue;
                }
                candidates.push((scoped_id, rescoped));
            }
        }

        for (scoped_id, rescoped_expr) in candidates {
            if let Some(value) = crate::geometry_ops::try_eval_geometry_query(
                &rescoped_expr,
                named_steps,
                values,
                functions,
                meta_map,
                kernel,
                diagnostics,
            ) {
                values.insert(scoped_id, value);
            }
        }
    }

    /// Re-evaluate `Let` value cells that are still `Undef` after the
    /// topology-selector post-processing pass (`post_process_topology_selectors`).
    ///
    /// Some `Let` cells depend on geometry-derived cells that are patched by
    /// `post_process_topology_selectors` AFTER the main `evaluate_params_and_lets_unified`
    /// pass.  During the main pass, the geometry-derived cell is still `Undef`
    /// (the kernel hasn't been queried yet), so any pure-math let that depends
    /// on it also evaluates to `Undef`.  Example: task 4229's
    /// `let moi_principal = eigenvalues(moment_of_inertia)` where
    /// `moment_of_inertia` is patched by `post_process_topology_selectors`.
    ///
    /// This pass iterates over `Let`-kind cells that are currently `Undef`
    /// (deeply — see below) and re-evaluates their `default_expr` using the
    /// now-updated `values` map, REPEATING until a full round writes no
    /// new value — a true fixpoint (task 4725 amendment), not a single
    /// pass. Only cells whose re-evaluation yields a non-`Undef` result are
    /// written back; cells whose arguments are still `Undef` (missing
    /// kernel, no geometry) remain `Undef` and are left untouched.
    ///
    /// **Why a real fixpoint, not one pass**: a round evaluates candidates
    /// in `template.value_cells` declaration order, mutating `values` as it
    /// goes — so a later cell in the SAME round observes an earlier cell's
    /// freshly-folded value, but an earlier cell does not observe a later
    /// one's. `let total_mass = all_masses.sum` only folds within a single
    /// round if `all_masses` happens to be declared first. Looping to a
    /// fixpoint removes that declaration-order dependency: a cell that
    /// missed its dependency this round (e.g. `total_mass` before
    /// `all_masses` folded) is still deep-Undef, so it re-enters the
    /// candidate set on the next round and folds once its dependency has.
    /// Rounds are capped at `value_cells.len()` (a same-template Let-cell
    /// dependency chain cannot be deeper than the number of Let cells) as a
    /// defensive backstop, not because termination depends on it —
    /// progress is tracked by VALUE equality, not merely "was written", so
    /// a round that re-derives the identical value for every remaining
    /// candidate (e.g. a `List` that stays partially `Undef` forever)
    /// reports no change and the loop stops after that round regardless of
    /// the cap.
    ///
    /// **Ordering contract**: must run after `post_process_topology_selectors`
    /// (and `post_process_geometry_queries`) so that patched-in geometry-derived
    /// values are visible; runs before `post_process_body_mass_props` and
    /// `post_process_mechanism_mass_props` (those passes do not produce `Let`
    /// cells that downstream pure-math lets could consume). Also runs after
    /// `post_process_cross_sub_value_cells` (task 4725) so a parent aggregate
    /// `Let` cell (e.g. `total_mass = all_masses.sum`) observes freshly-folded
    /// cross-sub scalar cells.
    fn post_process_derived_lets(
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) {
        // task 4725: the Undef check is DEEP (`value_is_or_contains_undef`),
        // not `Value::is_undef`'s shallow top-level check. An aggregate cell
        // like `let all_masses = [self.a.mass, self.b.mass]` evaluates to a
        // concrete `Value::List([Value::Undef, Value::Undef])` on the pure
        // pass (before `post_process_cross_sub_value_cells` folds the scoped
        // `mass` cells) — a `List` is never `Value::Undef` at the top level,
        // so the shallow check would permanently exclude it from
        // re-evaluation even after its element dependencies resolve. The
        // deep check (already used by the §6.1 stale-Undef invariant checker
        // for the identical `all_masses`/`total_mass` shape) re-selects it.
        //
        // The write-back guard just below stays SHALLOW (`!new_val.is_undef()`),
        // deliberately asymmetric with the deep candidate filter above: a
        // `List` (or other aggregate) result is written back even when it
        // still contains a nested `Undef`, so a partially-resolved aggregate
        // is visible to — and can converge further via — a later round of
        // this same fixpoint. This is safe only because (a) each round
        // re-derives a candidate's value FRESH from `default_expr` against
        // the current `values` map, never incrementally from the cell's own
        // prior value, and (b) no other pass populates a `List`-valued `Let`
        // cell with out-of-band per-element data — every element ultimately
        // comes from a scoped scalar cross-sub cell
        // (`post_process_cross_sub_value_cells`) or a same-template cell,
        // both of which only move `Undef` → resolved, never the reverse. If
        // a future pass ever writes partially-resolved elements directly
        // into a `List` `Let` cell (bypassing `default_expr` re-evaluation),
        // this guard would need to become "no less resolved than before"
        // rather than merely shallow-non-Undef.
        let max_rounds = template.value_cells.len().max(1);
        for _round in 0..max_rounds {
            // Collect candidates first to avoid holding a borrow on `values`
            // while also inserting into it.
            let candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = template
                .value_cells
                .iter()
                .filter(|cell| matches!(cell.kind, reify_compiler::ValueCellKind::Let))
                .filter(|cell| {
                    values
                        .get(&cell.id)
                        .is_none_or(crate::invariants::value_is_or_contains_undef)
                })
                .filter_map(|cell| {
                    cell.default_expr
                        .as_ref()
                        // Skip expressions that contain a CrossSubGeometryRef — those
                        // are consumed by entity.rs at the bare-let drop site and must
                        // never reach `reify_expr::eval_expr`, which `unreachable!()`s
                        // on them (see reify-expr/src/lib.rs:179, task-3508).
                        .filter(|e| !arg_contains_cross_sub_geometry_ref(e))
                        .map(|e| (cell.id.clone(), e.clone()))
                })
                .collect();

            let mut changed = false;
            for (cell_id, expr) in candidates {
                let new_val = {
                    let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
                    reify_expr::eval_expr(&expr, &ctx)
                };
                if !new_val.is_undef() {
                    if values.get(&cell_id) != Some(&new_val) {
                        changed = true;
                    }
                    values.insert(cell_id, new_val);
                }
            }

            if !changed {
                break;
            }
        }
    }

    /// Post-geometry DFM let-cell diagnostic harvest (task #4734 step-2 / A1).
    ///
    /// `post_process_derived_lets` re-evaluates Undef Let cells with a kernel-less,
    /// sink-less `EvalContext` — so `fits_build_volume(bounding_box(part), ...)` cells
    /// cannot fire `emit_dfm_diagnostics`: `bounding_box` stays `Undef` (no kernel),
    /// and even if it folded the diagnostic would be dropped (no sink).
    ///
    /// For each `Let` cell whose `default_expr` is a top-level `fits_build_volume` call:
    ///
    /// 1. Fold geometry-query leaves to Literals via `rewrite_geometry_queries` (resolves
    ///    nested `bounding_box(solid)` calls to concrete `Value::BoundingBox`).
    /// 2. Evaluate the folded expression with a `RefCell<Vec<Diagnostic>>` sink wired in
    ///    via `eval_ctx_with_meta(...).with_runtime_diagnostics(&sink)` so the
    ///    `emit_dfm_diagnostics` hook in `eval_expr`'s `FunctionCall` arm fires and
    ///    pushes `W/E/I_DFM_BUILD_VOLUME` diagnostics.
    /// 3. Drain the sink into `diagnostics`.
    ///
    /// Ordering: runs after `post_process_geometry_queries` (handles already in `values`)
    /// and `post_process_derived_lets` (pure-math lets resolved). Called from
    /// `run_post_processes` after `post_process_derived_lets`.
    fn harvest_dfm_let_diagnostics(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for cell in &template.value_cells {
            if !matches!(cell.kind, reify_compiler::ValueCellKind::Let) {
                continue;
            }
            let Some(expr) = cell.default_expr.as_ref() else {
                continue;
            };
            // Skip CrossSubGeometryRef expressions (same guard as post_process_derived_lets).
            if arg_contains_cross_sub_geometry_ref(expr) {
                continue;
            }
            // Only harvest cells whose top-level call is fits_build_volume.
            if !is_fits_build_volume_call(expr) {
                continue;
            }
            // Fold geometry-query leaves so fits_build_volume receives concrete BBoxes.
            let folded = crate::geometry_ops::rewrite_geometry_queries(
                expr,
                named_steps,
                kernel,
                diagnostics,
            );
            // Evaluate with a diagnostics sink so emit_dfm_diagnostics fires and
            // W/E/I_DFM_BUILD_VOLUME diagnostics are collected.
            diagnostics.extend(eval_folded_expr_for_dfm_diagnostics(
                &folded,
                values,
                functions,
                meta_map,
            ));
        }
    }

    /// Re-evaluate remaining Undef Let cells with the live containment hook wired
    /// in (task 4222 δ, PRD §5.3 option (b)).
    ///
    /// `run_post_processes` calls `post_process_derived_lets` which re-evaluates
    /// Undef Let cells using a basic `eval_ctx_with_meta` (no containment). Cells
    /// that sample a `restrict(field, region)` field — e.g. `v_in = sample(restricted, pt)` —
    /// stay Undef there because the Restricted sample arm requires `ctx.containment`
    /// to resolve geometry point-in-solid membership.
    ///
    /// This pass runs immediately after `run_post_processes` with the same Undef
    /// filter but an EvalContext that includes `.with_containment(self)`, so the
    /// kernel-backed containment hook fires and the correct inside/Undef result is
    /// stored.
    ///
    /// Ordering invariant: must be called AFTER `run_post_processes` so that:
    ///   (a) `post_process_geometry_handle_cells` has already stamped the region
    ///       cell with a `Value::GeometryHandle`, AND
    ///   (b) `post_process_derived_lets` has already re-evaluated `restricted`
    ///       (Undef → `Value::Field { lambda: List[inner, GeometryHandle] }`),
    ///       making the hydrated handle visible via the values map when this pass
    ///       looks up `restricted` to evaluate `v_in`.
    ///
    /// Short-circuits to a no-op when no default kernel is registered: without a
    /// kernel `ContainmentQuery::contains` on `Engine` always returns `None`, so
    /// re-evaluating with containment wired in would still yield `Value::Undef`.
    ///
    /// Mirrors the two-phase (collect-then-write) discipline of
    /// `post_process_derived_lets` to avoid split-borrow conflicts.
    fn post_process_containment_samples(
        &self,
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
    ) {
        if self.default_query_kernel().is_none() {
            return;
        }

        let candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = template
            .value_cells
            .iter()
            .filter(|cell| matches!(cell.kind, reify_compiler::ValueCellKind::Let))
            .filter(|cell| values.get(&cell.id).is_none_or(|v| v.is_undef()))
            .filter_map(|cell| {
                cell.default_expr
                    .as_ref()
                    .filter(|e| !arg_contains_cross_sub_geometry_ref(e))
                    .map(|e| (cell.id.clone(), e.clone()))
            })
            .collect();

        for (cell_id, expr) in candidates {
            let new_val = {
                let ctx = crate::eval_ctx_with_meta(values, &self.functions, &self.meta_map)
                    .with_containment(self);
                reify_expr::eval_expr(&expr, &ctx)
            };
            if !new_val.is_undef() {
                values.insert(cell_id, new_val);
            }
        }
    }

    /// Post-process value cells for a template after `execute_realization_ops`
    /// has populated `named_steps`, dispatching `@face("name")` and
    /// `@edge("name")` AdHocSelector expressions (task 3463).
    ///
    /// Sibling to `post_process_topology_selectors`. For each
    /// `ValueCellDecl` in `template.value_cells` whose `default_expr` is a
    /// `CompiledExprKind::AdHocSelector` with `SelectorKind::Face` or
    /// `SelectorKind::Edge`, this writes the kernel-resolved `Value::Frame`
    /// into `values`, overwriting the `Value::Undef` left behind by the
    /// pure `eval_expr` path. `@point` AdHocSelectors are handled
    /// entirely by `eval_expr` (Layer 1) and produce `None` here, so
    /// their cells are left untouched.
    ///
    /// Cells whose dispatch returns `None` (non-AdHocSelector expression,
    /// `@point`, missing `named_steps` entry, non-string-literal arg) are
    /// left untouched — see
    /// [`crate::geometry_ops::try_eval_ad_hoc_selector`]'s `None`-return
    /// contract.
    ///
    /// Cells that dispatch but fail to resolve (Unresolved /
    /// AmbiguousAfterSplit / kernel error) receive `Some(Value::Undef)`:
    /// the cell is patched to signal that the dispatch fired but produced
    /// no geometry, and the resolver/kernel pre-emitted a Warning
    /// diagnostic.
    ///
    /// Called from the same three sites as `post_process_topology_selectors`
    /// so build / build_snapshot / tessellate paths agree on the patched
    /// value.
    ///
    /// Signature takes `kernel: &mut dyn GeometryKernel` (mutable borrow)
    /// because `extract_faces` / `extract_edges` require `&mut self` on the
    /// `GeometryKernel` trait. The existing sibling functions take
    /// `kernel: &dyn GeometryKernel` (immutable); this one diverges from
    /// that convention because the attribute-lookup step needs sub-shape
    /// extraction before the read-only resolver and kernel-query steps.
    fn post_process_ad_hoc_selectors(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, KernelHandle>,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
        table: &TopologyAttributeTable,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `values` directly without snapshotting (same discipline as
        // `post_process_topology_selectors`). AdHocSelector cells do not chain
        // — an `@face` cell's inputs are the `named_steps` handle and a
        // string literal, never another AdHocSelector cell's output.
        for cell in &template.value_cells {
            let default_expr = match &cell.default_expr {
                Some(e) => e,
                None => continue,
            };
            if let Some(value) = crate::geometry_ops::try_eval_ad_hoc_selector(
                default_expr,
                named_steps,
                kernel,
                table,
                cell.span,
                diagnostics,
            ) {
                values.insert(cell.id.clone(), value);
            }
        }
    }

    /// δ (task 4740) step-5: refresh stale demanded value cells and gate
    /// realization recompute on the input-cone hash.
    ///
    /// Called from `tessellate_snapshot` AFTER `mark_demand_pruned_pending` and
    /// BEFORE the snapshot-values copy into the working `ValueMap`.
    ///
    /// **Part B — value refresh:** iterates the compiled module's value cells and
    /// re-evaluates those that are (a) demanded, (b) `Pending` in the cache (marked
    /// at HIDE time by the `mark_demand_pruned_pending` call added to
    /// `set_demand_selective`), and (c) have a non-CrossSubGeometryRef `default_expr`.
    /// The refreshed values are written back to `snapshot.values` so the copy loop
    /// in `tessellate_snapshot` picks up the CURRENT param-derived scalars.
    ///
    /// **Part C — hash gate:** for each DEMANDED realization, computes the current
    /// input-cone hash (`compute_realization_upstream_values_hash`) against the just-
    /// refreshed snapshot values and compares to the stored
    /// `RealizationNodeData.input_cone_hash`.  If the hashes DIFFER (inputs changed),
    /// `realization_cache.clear_entity` drops stale geometry so
    /// `tessellate_from_values` is forced to re-execute.  If SAME, the cached
    /// geometry is still valid and is reused (no re-dispatch → `last_dispatch_count`
    /// stays 0).  The stored hash is updated to the current value in both cases.
    ///
    /// **NOTE (esc-4740-29):** the "clear on hash mismatch" branch is currently a
    /// no-op in practice — `clear_realization_cache()` at `edit_param` entry already
    /// drops all entries, so the entity bucket will be empty when we call
    /// `clear_entity`.  The method is wired now so the logic is correct when
    /// eviction γ (task 4730) lands selective cache retention.
    /// Returns exempt realizations: those whose input-cone hash is UNCHANGED
    /// (`stored == Some(current_hash)`). These are excluded from the
    /// `demand_scoped_unified_pass` seed so they are not re-dispatched when
    /// inputs haven't changed (the "reuse" branch of the hash gate).
    fn refresh_and_gate_demanded_realizations(
        &mut self,
        module: &CompiledModule,
    ) -> HashSet<NodeId> {
        // Only meaningful under selective demand; full_scope means every node is
        // demanded, no stale cells possible, and the realization cache is managed
        // by the cold eval/build paths.
        if self.demand.is_full_scope() {
            return HashSet::new();
        }
        if self.eval_state.is_none() {
            return HashSet::new();
        }

        // ── Part B: refresh stale (Pending) demanded value cells ─────────────
        //
        // Phase 1: collect (cell_id, default_expr) pairs to re-evaluate.
        // Uses immutable borrows of self.eval_state, self.demand, self.cache —
        // all disjoint Engine fields; Rust NLL allows their simultaneous use.
        let candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = {
            module
                .templates
                .iter()
                .flat_map(|tmpl| {
                    tmpl.value_cells.iter().filter_map(|cell| {
                        // Refresh only Let cells: only a Let's `default_expr` is
                        // the authoritative CURRENT expression (e.g. `sb = w*2`).
                        // A Param's `default_expr` is its ORIGINAL default LITERAL;
                        // re-evaluating it would silently REVERT the user's
                        // `edit_param` to the declared default. An Auto cell's
                        // value is solver-determined, not its `default_expr`.
                        // This matches the established gate in deps.rs:149 and
                        // deps.rs:2795 (both gated on `ValueCellKind::Let`).
                        if !matches!(cell.kind, reify_compiler::ValueCellKind::Let) {
                            return None;
                        }
                        let node = NodeId::Value(cell.id.clone());
                        // Refresh only cells that are demanded AND Pending in
                        // the value cache AND have an evaluable expression.
                        let is_demanded = self.demand.is_demanded(&node);
                        let is_pending = self
                            .cache
                            .get(&node)
                            .map(|c| matches!(c.freshness, Freshness::Pending { .. }))
                            .unwrap_or(false);
                        if !is_demanded || !is_pending {
                            return None;
                        }
                        // Skip CrossSubGeometryRef args — eval_expr unreachable!()s
                        // on them (task-3508); mirrors post_process_derived_lets.
                        cell.default_expr
                            .as_ref()
                            .filter(|e| !arg_contains_cross_sub_geometry_ref(e))
                            .map(|e| (cell.id.clone(), e.clone()))
                    })
                })
                .collect()
        };

        if !candidates.is_empty() {
            // Phase 2: build working ValueMap for the eval context.
            let mut ctx_values = ValueMap::new();
            for (id, (val, _)) in self.eval_state.as_ref().unwrap().snapshot.values.iter() {
                ctx_values.insert(id.clone(), val.clone());
            }

            // Re-evaluate each candidate in template order so chain deps resolve
            // (e.g. `sb = w*2` where w is a Param already in ctx_values).
            let mut refreshed: Vec<(
                reify_core::ValueCellId,
                reify_ir::Value,
                reify_ir::DeterminacyState,
            )> = Vec::new();
            for (cell_id, expr) in &candidates {
                let new_val = {
                    // `self.functions` and `self.meta_map` are disjoint from the
                    // local `ctx_values` — no borrow conflict.
                    let ctx = crate::eval_ctx_with_meta(
                        &ctx_values,
                        &self.functions,
                        &self.meta_map,
                    );
                    reify_expr::eval_expr(expr, &ctx)
                };
                if !new_val.is_undef() {
                    // Update local context for chain deps.
                    ctx_values.insert(cell_id.clone(), new_val.clone());
                    // Preserve existing DeterminacyState from snapshot.values.
                    let det = self
                        .eval_state
                        .as_ref()
                        .and_then(|s| s.snapshot.values.get(cell_id))
                        .map(|(_, d)| *d)
                        .unwrap_or(reify_ir::DeterminacyState::Determined);
                    refreshed.push((cell_id.clone(), new_val, det));
                }
            }

            // Phase 3: write refreshed values back to snapshot.values.
            // Takes &mut self.eval_state; no other field borrow is active.
            if let Some(state) = self.eval_state.as_mut() {
                for (cell_id, new_val, det) in refreshed {
                    state.snapshot.values.insert(cell_id, (new_val, det));
                }
            }
        }

        // ── Part C: hash gate ─────────────────────────────────────────────────
        //
        // Build a ValueMap from the now-refreshed snapshot for hash computation.
        let ctx_for_hash = {
            let mut vm = ValueMap::new();
            for (id, (val, _)) in self.eval_state.as_ref().unwrap().snapshot.values.iter() {
                vm.insert(id.clone(), val.clone());
            }
            vm
        };

        // Two-phase: collect decisions, then apply mutations.
        //
        // `entities_to_clear` uses a HashSet keyed on `realization_decl.id.entity`
        // (the actual cache key — see the cross-realization keying invariant at
        // engine_build.rs:1663) rather than `tmpl.name`.  For simple top-level
        // templates the two are identical, but for sub/indexed entities they may
        // diverge, and using the wrong key would silently miss the cache entry when
        // eviction γ (task 4730) lands selective retention.  The HashSet also
        // deduplicates: a template with N demanded realizations all having stale
        // inputs would otherwise produce N identical `clear_entity` calls.
        let mut entities_to_clear: HashSet<String> = HashSet::new();
        let mut hash_updates: Vec<(reify_core::RealizationNodeId, [u8; 32])> = Vec::new();
        // Exempt realizations: those whose input-cone hash is UNCHANGED
        // (stored == Some(current_hash)). Returned to the caller so
        // demand_scoped_unified_pass can exclude them from the selective seed,
        // preventing unnecessary re-dispatch when inputs haven't changed.
        let mut exempt: HashSet<NodeId> = HashSet::new();

        {
            // Immutable borrows: eval_state, demand, functions, meta_map —
            // all disjoint; NLL allows their simultaneous use.
            let state = self.eval_state.as_ref().unwrap();
            for tmpl in &module.templates {
                for realization_decl in &tmpl.realizations {
                    let node = NodeId::Realization(realization_decl.id.clone());
                    if !self.demand.is_demanded(&node) {
                        continue;
                    }
                    let ctx = crate::eval_ctx_with_meta(
                        &ctx_for_hash,
                        &self.functions,
                        &self.meta_map,
                    );
                    let current_hash =
                        compute_realization_upstream_values_hash(realization_decl, &ctx);
                    let stored = state
                        .snapshot
                        .graph
                        .realizations
                        .get(&realization_decl.id)
                        .and_then(|n| n.input_cone_hash);
                    if stored != Some(current_hash) {
                        // Inputs changed (or no stored hash yet): invalidate stale
                        // geometry so tessellate_from_values re-executes.
                        // Forward-looking: currently a no-op since edit_param's
                        // clear_realization_cache already removed the entry
                        // (esc-4740-29).
                        // Key on `realization_decl.id.entity` — the actual cache key
                        // (invariant at engine_build.rs:1663), NOT `tmpl.name`.
                        entities_to_clear.insert(realization_decl.id.entity.clone());
                    } else {
                        // Inputs unchanged: exempt from re-dispatch this tessellate.
                        exempt.insert(node);
                    }
                    // Always record the new hash (even if hash was unchanged, to
                    // ensure the stored hash is set after the first tessellate).
                    hash_updates.push((realization_decl.id.clone(), current_hash));
                }
            }
        }

        // Apply: clear stale cache entries + update stored hashes.
        for entity in &entities_to_clear {
            self.realization_cache.clear_entity(entity);
        }
        if let Some(state) = self.eval_state.as_mut() {
            for (real_id, new_hash) in hash_updates {
                if let Some(node_data) = state.snapshot.graph.realizations.get_mut(&real_id) {
                    node_data.input_cone_hash = Some(new_hash);
                }
            }
        }
        exempt
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
        // Task ε (3436) step-12: reset the dispatch-count instrumentation
        // counter at the entry to every build/tessellate surface so a second
        // call against the same module reports its own per-build dispatch
        // tally (and reports 0 when fully served from the RealizationCache).
        // Mirrors `build` / `build_snapshot` / `tessellate_realizations`.
        // Zeroes BOTH the aggregate and the per-realization tally in lockstep.
        self.reset_dispatch_tallies();
        // γ (task 4739): demand-prune Pending producer — THE primary warm
        // pruning surface. On the warm/selective path (full_scope OFF) flip
        // every pruned-Final cached node to Pending so a hidden body's value is
        // never served as a silently-stale Final number (arch §8 prune-safety
        // scenario 3). No-op under full_scope and when every node is demanded;
        // re-run every warm build (a cold pass can re-Final a still-hidden body
        // between warm edits).
        self.mark_demand_pruned_pending();
        // δ (task 4740) step-5: refresh stale demanded value cells and gate
        // realization recompute on the input-cone hash.  Must run AFTER
        // `mark_demand_pruned_pending` (which identifies Pending cells) and BEFORE
        // the snapshot-values copy below (so the copy picks up fresh scalars).
        // Returns the exempt set (realizations with unchanged input-cone hash) for
        // the demand_scoped_unified_pass seed filter below.
        let hash_exempt = self.refresh_and_gate_demanded_realizations(module);
        let state = self.eval_state.as_ref()?;

        // β (task 4738) step-2: demand-scoped plan for the warm tessellate_snapshot
        // path — THE ONE SITE THAT ACTUALLY PRUNES (no eval()/check() call here,
        // so full_scope stays OFF when a GUI set_demand_selective is in effect).
        // `demand_scoped_unified_pass()` returns the seeded schedule + demand_seed
        // on the selective branch; `demand_seed_snap` threads into
        // `tessellate_from_values` to guard the build_steps fallback so hidden
        // bodies are not re-appended and dispatched.
        //
        // δ (task 4740) step-6: `hash_exempt` (realizations with unchanged
        // input-cone hash) is also excluded from the seed so `tessellate_from_values`
        // skips their kernel ops — implementing the "reuse" branch of the hash gate
        // without requiring a populated realization cache.
        //
        // SAFETY: `state = self.eval_state.as_ref()` is a SHARED borrow of
        // `self.eval_state`.  `demand_scoped_unified_pass()` takes `&self`, also
        // a shared borrow.  Both are immutable — Rust NLL allows multiple shared
        // borrows to coexist.  The `&mut self.*` borrows below start only after
        // `state`'s last use (line ~8298), which NLL confirms.
        let (unified_pass_snap, demand_seed_snap) =
            self.demand_scoped_unified_pass(&hash_exempt);
        // realization_read_cells: union of all realization traces' reads. Used by
        // hydrate_value_cell_in_loop for eager selector resolution at scheduled
        // HydrateCell steps. Not restricted to the demand cone: cells shared
        // between demanded and hidden realizations must still be available.
        // Empty under LegacyMultiPass (unified_pass_snap is None).  Delegated
        // to the shared helper to avoid duplicating the trace_map iteration.
        let realization_read_cells_snap: HashSet<reify_core::ValueCellId> =
            if unified_pass_snap.is_some() {
                self.realization_read_cells()
            } else {
                HashSet::new()
            };

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let (constraint_results, mut diagnostics) =
            self.check_constraints_against_templates(module, &values, Some(&state.snapshot.values));

        // Task 2874 (amendment): emit imported-tolerance-promise diagnostics
        // (`ImportedTolerancePromiseInsufficient` / `InputTolerancePromiseIsZero`)
        // for every (Input × Output × active-purpose-binding) triple recognised
        // in the post-eval snapshot. Mirrors the placement used by
        // `build_snapshot` (after `check_constraints_against_templates`) — both
        // surfaces operate on the existing snapshot without re-calling `eval()`,
        // so the placement constraint that motivated the BEFORE-`check()` order
        // in `build` / `tessellate_realizations` does not apply.
        self.emit_imported_tolerance_promise_diagnostics_for_module(module, &mut diagnostics);

        // Execute geometry and tessellate. `values` is passed `&mut` so the
        // post-process inside `tessellate_from_values` can patch
        // conformance-query results (`is_watertight` / `is_manifold` /
        // `is_orientable`) before they're surfaced via `TessellateResult`
        // (task 2320 amendment).
        // Task 2874 step-6: precompute per-realization demanded tolerance
        // before the `&mut self.*` borrows. See sibling
        // `tessellate_realizations` for rationale.
        let demanded_tols = self.compute_demanded_tols(module);
        // Task 2874 step-12: precompute per-realization tessellation budget.
        // See `tessellate_realizations` for the budget-routing rationale.
        let registry_owned = crate::kernel_registry::collect_registry();
        let tessellation_budgets =
            self.compute_tessellation_budgets(module, &demanded_tols, &registry_owned);
        // Step-8 (task ε / 3436): borrowed-view registry for per-op dispatch
        // routing — same pattern as the `tessellate_realizations` mirror.
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            registry_owned.iter().map(|(k, v)| (k.clone(), v)).collect();
        self.topology_attribute_table = TopologyAttributeTable::default();
        self.swept_kind_table = SweptKindTable::default();
        // Determinacy β (task 4198): clear the achieved-tol map at the start
        // of each tessellate_snapshot call (mirrors tessellate_realizations).
        self.achieved_repr_tol.clear();
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernels,
            &registry_borrowed,
            self.default_kernel_name.as_deref(),
            module,
            &mut values,
            &self.functions,
            &mut diagnostics,
            &self.meta_map,
            &mut self.topology_attribute_table,
            &mut self.swept_kind_table,
            &mut self.realization_cache,
            &demanded_tols,
            &tessellation_budgets,
            &mut self.last_dispatch_count,
            &mut self.last_dispatch_count_by_realization,
            self.capture_repr_tol,
            &mut self.achieved_repr_tol,
            unified_pass_snap.as_ref(),
            &realization_read_cells_snap,
            demand_seed_snap.as_ref(),
        );

        Some(TessellateResult {
            values,
            constraint_results,
            meshes,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }
}

/// Collect centroid values for each topology-attribute handle, coalescing
/// kernel query errors and parse errors into at most one summary warning each.
///
/// A wedged kernel can otherwise dump dozens of identical diagnostics into the
/// user-facing stream — auxiliary metadata storms degrade UX more than missing
/// fragility signal does. We retain the first error message verbatim for
/// diagnosability.
///
/// Returns a pair `(centroids, warnings)` where `centroids` maps each
/// `GeometryHandleId` to `[x, y, z]` for every handle successfully queried
/// and parsed.  `warnings` contains at most one `Warning` per failure class
/// (`query_fail`, `parse_fail`).  The caller is responsible for extending its
/// diagnostics buffer with the returned `warnings`.
///
/// Handles that fail either step are omitted from `centroids`.
/// Parse a BoundingBox JSON payload (from [`GeometryQuery::BoundingBox`]) and
/// return all six extents as `(xmin, ymin, zmin, xmax, ymax, zmax)`.
///
/// Expects the format `{"xmin":…,"ymin":…,"zmin":…,"xmax":…,"ymax":…,"zmax":…}`.
///
/// NOTE: `parse_bbox_xyz_min` in `primitive_attribute_seed.rs` parses the same
/// format but returns only the min triple (and is out of scope for this amendment
/// — that file is not in the locked module set for task #4734).
fn parse_bbox_all_extents(
    value: &reify_ir::Value,
) -> Result<(f64, f64, f64, f64, f64, f64), reify_ir::QueryError> {
    let s = match value {
        reify_ir::Value::String(s) => s,
        other => {
            return Err(reify_ir::QueryError::QueryFailed(format!(
                "BoundingBox returned non-string value: {other:?}"
            )));
        }
    };
    let inner = s
        .trim()
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| {
            reify_ir::QueryError::QueryFailed(format!(
                "BoundingBox returned malformed JSON: {s:?}"
            ))
        })?;
    let mut xmin: Option<f64> = None;
    let mut ymin: Option<f64> = None;
    let mut zmin: Option<f64> = None;
    let mut xmax: Option<f64> = None;
    let mut ymax: Option<f64> = None;
    let mut zmax: Option<f64> = None;
    for part in inner.split(',') {
        let mut kv = part.splitn(2, ':');
        let key = kv
            .next()
            .ok_or_else(|| {
                reify_ir::QueryError::QueryFailed(format!(
                    "BoundingBox returned malformed JSON (missing key): {s:?}"
                ))
            })?
            .trim()
            .trim_matches('"');
        let val = kv
            .next()
            .ok_or_else(|| {
                reify_ir::QueryError::QueryFailed(format!(
                    "BoundingBox returned malformed JSON (missing value): {s:?}"
                ))
            })?
            .trim();
        let f: f64 = val.parse::<f64>().map_err(|_| {
            reify_ir::QueryError::QueryFailed(format!(
                "BoundingBox {key} is not a valid f64: {val:?} (full payload {s:?})"
            ))
        })?;
        match key {
            "xmin" => xmin = Some(f),
            "ymin" => ymin = Some(f),
            "zmin" => zmin = Some(f),
            "xmax" => xmax = Some(f),
            "ymax" => ymax = Some(f),
            "zmax" => zmax = Some(f),
            _ => {}
        }
    }
    let xmin = xmin.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing xmin: {s:?}"))
    })?;
    let ymin = ymin.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing ymin: {s:?}"))
    })?;
    let zmin = zmin.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing zmin: {s:?}"))
    })?;
    let xmax = xmax.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing xmax: {s:?}"))
    })?;
    let ymax = ymax.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing ymax: {s:?}"))
    })?;
    let zmax = zmax.ok_or_else(|| {
        reify_ir::QueryError::QueryFailed(format!("BoundingBox payload missing zmax: {s:?}"))
    })?;
    Ok((xmin, ymin, zmin, xmax, ymax, zmax))
}

/// Parse a BoundingBox JSON payload (from [`GeometryQuery::BoundingBox`]) and
/// return the midpoint `[(xmin+xmax)/2, (ymin+ymax)/2, (zmin+zmax)/2]`.
///
/// Used by [`collect_centroids_with_failure_summary`] for `Role::NewEdge`
/// handles (B1, task #4734): edge shapes have 1D geometry, so
/// `GeometryQuery::Centroid` routes through `query_centroid`
/// (VolumeProperties), which returns the origin (0,0,0) for zero-mass 1D
/// shapes. This makes ALL edge handles appear co-located, spuriously tripping
/// the within-1e-9 tie test in
/// `detect_local_index_reassignment_diagnostics` even for a plain box with 12
/// geometrically distinct edges.
///
/// The bounding-box midpoint is a reliable geometric discriminator:
/// - Geometrically distinct edges have distinct midpoints → no spurious tie.
/// - Genuinely coincident edges share the same bbox → same midpoint → tie
///   correctly detected (e.g. `union(box, box)` with coincident placement).
///
/// Face handles keep the existing `GeometryQuery::Centroid` path (surface
/// properties, which correctly returns the face centroid).
///
/// Delegates to [`parse_bbox_all_extents`] for the actual JSON parsing.
fn parse_bbox_midpoint(value: &reify_ir::Value) -> Result<[f64; 3], reify_ir::QueryError> {
    let (xmin, ymin, zmin, xmax, ymax, zmax) = parse_bbox_all_extents(value)?;
    Ok([
        (xmin + xmax) / 2.0,
        (ymin + ymax) / 2.0,
        (zmin + zmax) / 2.0,
    ])
}

fn collect_centroids_with_failure_summary(
    realization_attrs: &[(GeometryHandleId, &TopologyAttribute)],
    kernel: &dyn GeometryKernel,
    realization_id: &RealizationNodeId,
) -> (HashMap<GeometryHandleId, [f64; 3]>, Vec<Diagnostic>) {
    let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
    let mut query_fail_count: usize = 0;
    let mut query_fail_first: Option<String> = None;
    let mut parse_fail_count: usize = 0;
    let mut parse_fail_first: Option<String> = None;
    for (handle_id, attr) in realization_attrs {
        // B1 (task #4734): edge handles (Role::NewEdge) have 1D geometry.
        // GeometryQuery::Centroid routes through query_centroid
        // (VolumeProperties), which returns the origin (0,0,0) for 1D shapes
        // (mass=0, CentreOfMass defaults to origin on the OCCT VolumeProperties
        // path). This makes ALL edge handles appear co-located, spuriously
        // tripping the within-1e-9 tie test even for a plain box with 12
        // geometrically distinct edges.
        //
        // Fix: for Role::NewEdge, use GeometryQuery::BoundingBox + bbox midpoint
        // instead. The midpoint is geometrically distinct for non-coincident edges
        // (so plain-box edges produce 0 warnings) and identical for genuinely
        // coincident edges (so union(box,box) coincident detection still fires).
        // Face handles (Role::Side, Role::Cap) keep the existing Centroid path
        // (SurfaceProperties, which correctly returns the face centroid).
        let query = if attr.role == Role::NewEdge {
            GeometryQuery::BoundingBox(*handle_id)
        } else {
            GeometryQuery::Centroid(*handle_id)
        };
        match kernel.query(&query) {
            Ok(value) => {
                let parse_result: Result<[f64; 3], reify_ir::QueryError> =
                    if attr.role == Role::NewEdge {
                        parse_bbox_midpoint(&value)
                    } else {
                        crate::topology_selectors::parse_xyz_value(
                            &value,
                            "local_index_reassignment_centroid",
                        )
                    };
                match parse_result {
                    Ok(xyz) => {
                        centroids.insert(*handle_id, xyz);
                    }
                    Err(e) => {
                        parse_fail_count += 1;
                        if parse_fail_first.is_none() {
                            parse_fail_first = Some(e.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                query_fail_count += 1;
                if query_fail_first.is_none() {
                    query_fail_first = Some(e.to_string());
                }
            }
        }
    }
    let mut diags: Vec<Diagnostic> = Vec::new();
    if query_fail_count > 0 {
        let first = query_fail_first.unwrap_or_else(|| "<no message>".to_string());
        diags.push(Diagnostic::warning(format!(
            "topology-attribute centroid query failed for {query_fail_count} \
             handle(s) in {realization_id} (first: {first})"
        )));
    }
    if parse_fail_count > 0 {
        let first = parse_fail_first.unwrap_or_else(|| "<no message>".to_string());
        diags.push(Diagnostic::warning(format!(
            "topology-attribute centroid parse failed for {parse_fail_count} \
             handle(s) in {realization_id} (first: {first})"
        )));
    }
    (centroids, diags)
}

// ── dispatch_volume_mesh ──────────────────────────────────────────────────────

/// Outcome of [`dispatch_volume_mesh`]: either a tetrahedral volume mesh (tet
/// fall-back path) or a swept hex/wedge mesh (swept path).
///
/// Returned so the caller can choose downstream handling: FEA assembly for
/// tets uses `tet_indices` with stride-4/10; hex/wedge assembly uses
/// `connectivity` from [`SweptMesh3d`].
#[derive(Debug, Clone)]
pub(crate) enum VolumeMeshOutcome {
    /// Tet mesh produced by the tet fall-back path
    /// (`mesh_surface_to_volume_with_diagnostics`).
    Tet(VolumeMesh),
    /// Swept hex/wedge mesh produced by the swept path
    /// (`gmsh_2d` + `sweep_2d_mesh_to_3d`).
    Swept(SweptMesh3d),
}

/// Dispatch between the swept hex/wedge path and the tet fall-back path,
/// implementing the 8-case truth table from the hex/wedge PRD pseudo-code.
///
/// # Parameters
///
/// - `swept_kind`: Phase A swept-body classification from [`SweptKindTable`].
///   `None` means the geometry is not a recognised swept body.
/// - `force_tet`: when `true`, always use the tet path, ignoring the
///   classifier output (`ElasticOptions.force_tet`).
/// - `require_hex_wedge`: when `true`, treat any swept-path failure as a
///   hard error rather than falling back to tets
///   (`ElasticOptions.require_hex_wedge`).
/// - `ops`: the parallel compiled-op slice from the realization (forwarded to
///   [`swept_kind_to_sweep_params`] for the `SweepLinear` arm's path-handle
///   resolution; ignored for `Extrude`/`Revolve`).
/// - `handles`: the parallel handle-id slice from the same realization (same
///   usage as `ops`).
/// - `gmsh_2d`: closure that 2D-meshes the swept cross-section profile;
///   receives `&SweptKind`. Signature:
///   `FnOnce(&SweptKind) -> Result<Mesh2dReport, Mesh2dError>`.
/// - `sweep_step`: closure that extrudes/revolves the 2D mesh into a 3D
///   hex/wedge mesh; receives `(&SweepParams, &Mesh2d)` where `SweepParams`
///   is built internally via [`swept_kind_to_sweep_params`]. Signature:
///   `FnOnce(&SweepParams, &Mesh2d) -> Result<SweptMesh3d, SweepError>`.
/// - `tet_path`: closure that produces a tet mesh via
///   `mesh_surface_to_volume_with_diagnostics`; called as the fall-back.
///   Signature: `FnOnce() -> Result<VolumeMesh, GeometryError>`.
///
/// # Truth table
///
/// | `swept_kind` | `force_tet` | `require_hex_wedge` | `gmsh_2d` | `sweep_step` | result |
/// |--------------|-------------|---------------------|-----------|--------------|--------|
/// | any          | true        | any                 | skip      | skip         | `Tet` |
/// | `None`       | false       | false               | skip      | skip         | `Tet` |
/// | `None`       | false       | true                | skip      | skip         | `Err("body not swept")` |
/// | `Some(_)`    | false       | any                 | `Ok`      | `Ok`         | `Swept` |
/// | `Some(_)`    | false       | false               | `Err`     | skip         | `Tet` (fallback) |
/// | `Some(_)`    | false       | false               | `Ok`      | `Err`        | `Tet` (fallback) |
/// | `Some(_)`    | false       | true                | `Err`     | skip         | `Err("swept hex/wedge path failed: …")` |
/// | `Some(_)`    | false       | true                | `Ok`      | `Err`        | `Err("swept hex/wedge path failed: …")` |
// Un-orphaned by task #4743 (α): the `execute_realization_ops` VolumeMesh call
// edge above is now the production caller (tet path); `clippy::too_many_arguments`
// is retained (still an 8-arg higher-order dispatcher).
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_volume_mesh<G, S, T>(
    swept_kind: Option<&SweptKind>,
    force_tet: bool,
    require_hex_wedge: bool,
    ops: &[GeometryOp],
    handles: &[GeometryHandleId],
    gmsh_2d: G,
    sweep_step: S,
    tet_path: T,
) -> Result<VolumeMeshOutcome, GeometryError>
where
    G: FnOnce(&SweptKind) -> Result<Mesh2dReport, Mesh2dError>,
    S: FnOnce(&SweepParams, &Mesh2d) -> Result<SweptMesh3d, SweepError>,
    T: FnOnce() -> Result<VolumeMesh, GeometryError>,
{
    // Step-4: force_tet short-circuit — bypass classifier entirely.
    if force_tet {
        return tet_path().map(VolumeMeshOutcome::Tet);
    }

    let Some(swept) = swept_kind else {
        // Steps 6 + 8: no classifier match.
        return if require_hex_wedge {
            Err(GeometryError::OperationFailed("body not swept".to_string()))
        } else {
            tet_path().map(VolumeMeshOutcome::Tet)
        };
    };

    // Steps 10 + 12 + 14: swept path — call gmsh_2d then sweep_step.
    // Build SweepParams via the canonical converter in sweep_classifier.rs so
    // there is a single conversion path.  Returns None only for SweepLinear
    // with an unresolvable path handle — treat as a swept-path failure.
    let params = match swept_kind_to_sweep_params(swept, ops, handles) {
        Some(p) => p,
        None => {
            return if require_hex_wedge {
                Err(GeometryError::OperationFailed(
                    "swept hex/wedge path failed: cannot resolve SweepLinear path handle"
                        .to_string(),
                ))
            } else {
                tet_path().map(VolumeMeshOutcome::Tet)
            };
        }
    };
    match gmsh_2d(swept) {
        Ok(report) => match sweep_step(&params, &report.mesh) {
            Ok(mesh3d) => Ok(VolumeMeshOutcome::Swept(mesh3d)),
            Err(e) if require_hex_wedge => Err(GeometryError::OperationFailed(format!(
                "swept hex/wedge path failed: {e:?}"
            ))),
            Err(_) => tet_path().map(VolumeMeshOutcome::Tet),
        },
        Err(e) if require_hex_wedge => Err(GeometryError::OperationFailed(format!(
            "swept hex/wedge path failed: {e:?}"
        ))),
        Err(_) => tet_path().map(VolumeMeshOutcome::Tet),
    }
}

// ── build_mixed_region_mesh (T12 layer B) ─────────────────────────────────────
//
// Routing + merge + MPC wiring for a mixed shell/tet body (PRD v0.4
// structural-analysis-shells.md §124). Consumes already-meshed inputs (a
// shell `MidSurfaceMesh` from T9 + a tet `VolumeMesh` from the existing
// `dispatch_volume_mesh` tet seam) plus the kernel-agnostic
// `ShellTetInterface` descriptors from `reify_shell_extract::partition`, and
// produces a unified node/element list tagged per element (shell vs. tet)
// together with the interface `MpcRow` constraint set. It does NOT invoke
// Gmsh, build element stiffness, or run the solve — those live in the existing
// tet seam, T6, and the engine-bridge PRD (δ/ε) respectively.
//
// The whole seam is `#[allow(dead_code)]` because its consumer — the
// engine-bridge mixed solve wiring — is a future task; this mirrors the
// `dispatch_volume_mesh` G-allow pattern above.

/// Per-element kind tag in a [`MixedRegionMesh`].
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnifiedElementKind {
    /// A mid-surface shell element (one per shell triangle, 6 DOF/node).
    Shell,
    /// A volumetric tet element (one per tet, 3 DOF/node).
    Tet,
}

/// One element of the unified mixed mesh, referencing unified node ids.
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnifiedElement {
    /// Whether this element is meshed as a shell or a tet.
    pub kind: UnifiedElementKind,
    /// Unified node indices (shell nodes first, tet nodes offset by the shell
    /// node count). Length 3 for a shell triangle, 4/10 for a P1/P2 tet.
    pub connectivity: Vec<usize>,
}

/// Unified mixed shell/tet mesh: a single node list, per-element kind tags, and
/// the shell↔tet interface MPC constraint rows.
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
#[derive(Debug, Clone)]
pub(crate) struct MixedRegionMesh {
    /// Unified node positions (world, f64). Shell vertices first, then tet
    /// vertices (f32 → f64) appended at offset `n_shell_nodes`.
    pub nodes: Vec<[f64; 3]>,
    /// Unified elements, both shell and tet, referencing `nodes` indices.
    pub elements: Vec<UnifiedElement>,
    /// Interface tying constraints under the global D=6 DOF layout (see
    /// [`build_mixed_region_mesh`]). Empty when there are no interfaces.
    pub mpc_rows: Vec<MpcRow>,
}

/// Errors returned by [`build_mixed_region_mesh`].
#[allow(dead_code)] // variants constructed in the interface-wiring path (step-12 + amendment)
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MixedRegionError {
    /// An interface could not be tied because a required tie node was missing
    /// — the shell side has no vertices, or the tet side has no nodes, so the
    /// nearest-node resolution has no candidate.
    InterfaceResolutionFailed {
        /// Index of the offending interface in the input `interfaces` slice.
        interface_index: usize,
    },
    /// An interface's tie geometry violates `MpcRow::shell_tet_tying`'s
    /// preconditions — a non-unit `normal` or a non-positive `thickness`, both
    /// of which that builder asserts on (and would panic). `partition_body`
    /// guarantees these invariants, so this only arises for an interface
    /// constructed directly by a caller that bypasses the partition layer.
    InvalidInterfaceGeometry {
        /// Index of the offending interface in the input `interfaces` slice.
        interface_index: usize,
    },
}

impl std::fmt::Display for MixedRegionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixedRegionError::InterfaceResolutionFailed { interface_index } => write!(
                f,
                "interface {interface_index} could not be tied: the shell or tet side \
                 has no candidate tie node (empty mesh on one side)"
            ),
            MixedRegionError::InvalidInterfaceGeometry { interface_index } => write!(
                f,
                "interface {interface_index} has invalid tie geometry: `normal` must be \
                 a unit vector and `thickness` must be positive \
                 (MpcRow::shell_tet_tying preconditions)"
            ),
        }
    }
}

impl std::error::Error for MixedRegionError {}

/// Merge a shell [`MidSurfaceMesh`] and a tet [`VolumeMesh`] into one unified
/// mesh and wire the shell↔tet interface MPC rows (PRD T12).
///
/// # Node numbering
///
/// Shell vertices are numbered first (`0..n_shell`), keeping their index; tet
/// vertices are appended (`f32 → f64`) at offset `n_shell`, so tet local node
/// `m` becomes unified node `n_shell + m`. This deterministic offset map is
/// shared by the element connectivity and the MPC DOF wiring.
///
/// # Elements
///
/// One [`UnifiedElementKind::Shell`] element per shell triangle (connectivity =
/// the triangle's vertex indices) and one [`UnifiedElementKind::Tet`] per tet
/// (connectivity chunked from `tet.tet_indices` by the per-element node count
/// from `element_order`, offset by `n_shell`).
///
/// # Errors
///
/// Returns [`MixedRegionError::InterfaceResolutionFailed`] if an interface
/// cannot be resolved to tie nodes (empty shell or tet mesh on one side).
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
pub(crate) fn build_mixed_region_mesh(
    shell: &MidSurfaceMesh,
    tet: &VolumeMesh,
    interfaces: &[ShellTetInterface],
) -> Result<MixedRegionMesh, MixedRegionError> {
    // ── Merge nodes: shell vertices first, then tet vertices (f32 → f64) ──────
    let n_shell = shell.vertices.len();
    let mut nodes: Vec<[f64; 3]> = Vec::with_capacity(n_shell + tet.vertices.len() / 3);
    nodes.extend_from_slice(&shell.vertices);
    for chunk in tet.vertices.chunks_exact(3) {
        nodes.push([chunk[0] as f64, chunk[1] as f64, chunk[2] as f64]);
    }

    // ── Elements: one shell element per triangle, one tet element per tet ─────
    let tet_indices = tet
        .tet_indices()
        .expect("build_mixed_region_mesh: tet-only (hex/wedge VolumeMesh not supported)");
    let mut elements: Vec<UnifiedElement> =
        Vec::with_capacity(shell.triangles.len() + tet_indices.len());
    for tri in &shell.triangles {
        elements.push(UnifiedElement {
            kind: UnifiedElementKind::Shell,
            connectivity: vec![tri[0] as usize, tri[1] as usize, tri[2] as usize],
        });
    }
    // Per-tet node count from the element order (P1 = 4, P2 = 10); tet local
    // node `m` → unified node `n_shell + m`.
    let nodes_per_tet = match tet
        .element_order()
        .expect("build_mixed_region_mesh: tet-only (hex/wedge VolumeMesh not supported)")
    {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };
    for tet_conn in tet_indices.chunks_exact(nodes_per_tet) {
        elements.push(UnifiedElement {
            kind: UnifiedElementKind::Tet,
            connectivity: tet_conn.iter().map(|&i| n_shell + i as usize).collect(),
        });
    }

    // ── Interface → MPC wiring (D=6 unified DOF layout) ───────────────────────
    //
    // Shell elements force the global DOFs-per-node to 6 (shell dominates, as
    // assemble_global_stiffness derives D = max d_e), so the tie rows are
    // emitted in D=6 from the start. Under `6·node + axis`: shell tie node `n` →
    // disp `[6n+0,1,2]` / rot `[6n+3,4,5]`; tet node `m` (unified) → disp
    // `[6m+0,1,2]`. Downstream T11 assembly / the engine bridge consume these
    // rows directly, so they reference the same DOF space the solve will use.
    let n_tet = nodes.len() - n_shell;
    let mut mpc_rows: Vec<MpcRow> = Vec::new();
    for (interface_index, iface) in interfaces.iter().enumerate() {
        // Validate the tie geometry up front. `MpcRow::shell_tet_tying` asserts a
        // unit `normal` and a positive `thickness` (mpc.rs) and would panic
        // otherwise. `partition_body` guarantees both invariants, but this seam
        // is reachable directly — and its `Result` return type implies graceful
        // handling — so a violating interface is surfaced as a structured error
        // instead of a panic. The accept conditions mirror the downstream asserts
        // exactly, so any interface passing here also passes `shell_tet_tying`;
        // binding to booleans first keeps a NaN normal/thickness rejected (NaN
        // comparisons are false) without tripping clippy::neg_cmp_op_on_partial_ord.
        let normal_mag = (iface.normal[0] * iface.normal[0]
            + iface.normal[1] * iface.normal[1]
            + iface.normal[2] * iface.normal[2])
            .sqrt();
        let thickness_ok = iface.thickness > 0.0;
        let normal_is_unit = (normal_mag - 1.0).abs() < 1e-9;
        if !thickness_ok || !normal_is_unit {
            return Err(MixedRegionError::InvalidInterfaceGeometry { interface_index });
        }

        // Shell tie node: nearest shell vertex to the interface location. Its
        // unified index equals the shell vertex index (shell nodes are first).
        let shell_n = nearest_node_index(&nodes[..n_shell], iface.location)
            .ok_or(MixedRegionError::InterfaceResolutionFailed { interface_index })?;
        // The through-thickness tie needs 3 distinct tet nodes (top/mid/bot);
        // fewer means the interface cannot be resolved.
        if n_tet < 3 {
            return Err(MixedRegionError::InterfaceResolutionFailed { interface_index });
        }
        // 3 tet nodes nearest the location (local indices into the tet block),
        // ordered by projection onto the normal: top (max) … bot (min).
        //
        // CAVEAT (load-bearing geometric assumption): the 3 Euclidean-nearest tet
        // nodes are assumed to form a through-thickness column — one above / near
        // / below the mid-surface. On a dense volumetric mesh they can instead
        // cluster on the near face, so `mid` (used for the displacement tie) may
        // not be the true through-thickness midpoint the MPC assumes; the
        // single-column tie fixtures here mask this. When the engine-bridge
        // consumer lands, prefer selecting by signed projection distance along
        // `normal` (one node above, one near, one below `location`) over pure
        // nearest-3. Tracked as a T12 follow-up.
        let mut nearest3 = three_nearest_node_indices(&nodes[n_shell..], iface.location);
        nearest3.sort_by(|&m1, &m2| {
            let p1 = dot3(nodes[n_shell + m1], iface.normal);
            let p2 = dot3(nodes[n_shell + m2], iface.normal);
            p2.partial_cmp(&p1).unwrap_or(std::cmp::Ordering::Equal)
        });
        let tet_top = n_shell + nearest3[0];
        let tet_mid = n_shell + nearest3[1];
        let tet_bot = n_shell + nearest3[2];

        let dofs = |node: usize| [6 * node, 6 * node + 1, 6 * node + 2];
        let shell_rot = [6 * shell_n + 3, 6 * shell_n + 4, 6 * shell_n + 5];

        mpc_rows.extend(MpcRow::shell_tet_tying(
            dofs(shell_n),
            shell_rot,
            dofs(tet_top),
            dofs(tet_mid),
            dofs(tet_bot),
            iface.normal,
            iface.thickness,
        ));
    }

    Ok(MixedRegionMesh {
        nodes,
        elements,
        mpc_rows,
    })
}

/// Dot product of two 3-vectors.
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Squared Euclidean distance between two 3-vectors.
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
fn dist3_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

/// Index of the node in `nodes` nearest (Euclidean) to `target`; `None` if
/// `nodes` is empty. Ties resolve to the lowest index (deterministic).
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
fn nearest_node_index(nodes: &[[f64; 3]], target: [f64; 3]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &p) in nodes.iter().enumerate() {
        let d_sq = dist3_sq(p, target);
        if best.is_none_or(|(_, bd)| d_sq < bd) {
            best = Some((i, d_sq));
        }
    }
    best.map(|(i, _)| i)
}

/// The 3 indices of `nodes` nearest `target`, nearest first. The caller
/// guarantees `nodes.len() >= 3`.
#[allow(dead_code)] // T12 layer-B seam; consumer pending engine-bridge mixed solve (PRD δ/ε)
fn three_nearest_node_indices(nodes: &[[f64; 3]], target: [f64; 3]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..nodes.len()).collect();
    idx.sort_by(|&a, &b| {
        dist3_sq(nodes[a], target)
            .partial_cmp(&dist3_sq(nodes[b], target))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idx.truncate(3);
    idx
}

/// Returns `true` if `expr`'s compiled tree contains a `CrossSubGeometryRef`
/// at any depth.
///
/// The `upstream_values_hash` fold (in `post_process_geometry_handle_cells`
/// and `hydrate_geometry_handles_into_values`) evaluates each realization-op
/// scalar arg via `reify_expr::eval_expr`, which `unreachable!()`s on a
/// `CrossSubGeometryRef` (`reify-expr/src/lib.rs:177`). Such a geometry-ref can
/// be the top-level arg (`rotate(self.inner.body, …)`) *or* nested inside a
/// larger operator node (`translate(rotate(self.inner.body, …), …)`), so a
/// top-level `matches!` is insufficient — we walk the whole tree via the
/// canonical [`reify_ir::CompiledExpr::walk`]. A geometry-ref's identity is
/// already captured by the op's `GeomRef` target/profiles, so any arg
/// containing one is skipped from hashing entirely (task 3616; regression
/// pinned by `cross_sub_geometry_anti_cascade_no_spurious_errors_in_translate_chain`).
fn arg_contains_cross_sub_geometry_ref(expr: &reify_ir::CompiledExpr) -> bool {
    let mut found = false;
    expr.walk(&mut |e| {
        if matches!(e.kind, reify_ir::CompiledExprKind::CrossSubGeometryRef(_)) {
            found = true;
        }
    });
    found
}

/// Returns `true` if `expr` is a top-level `FunctionCall` to `fits_build_volume`.
///
/// Used by [`Engine::harvest_dfm_let_diagnostics`] to identify DFMSeverityBridge
/// let-cells (task #4734 A1). Matches on `function.name` (short name, without
/// namespace qualification) so it is robust to stdlib module-path changes.
pub(crate) fn is_fits_build_volume_call(expr: &reify_ir::CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        reify_ir::CompiledExprKind::FunctionCall { function, .. }
            if function.name == "fits_build_volume"
    )
}

/// Evaluate a geometry-folded expression with a `RefCell<Vec<Diagnostic>>` sink
/// and return the diagnostics collected by the `emit_dfm_diagnostics` hook.
///
/// Used by both [`Engine::harvest_dfm_let_diagnostics`] (A1) and
/// [`Engine::check_constraints_post_geometry`] (A2/A3, via `engine_constraints`)
/// to avoid duplicating the sink-wiring boilerplate. Both callers fold geometry
/// queries first (via `rewrite_geometry_queries`) so that `fits_build_volume` sees
/// concrete `BoundingBox` literals and the `Bool(false)` violation path fires
/// `dfm_diagnose` into the sink.
pub(crate) fn eval_folded_expr_for_dfm_diagnostics(
    expr: &reify_ir::CompiledExpr,
    values: &reify_ir::ValueMap,
    functions: &[reify_ir::CompiledFunction],
    meta_map: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> Vec<reify_core::Diagnostic> {
    use std::cell::RefCell;
    let sink: RefCell<Vec<reify_core::Diagnostic>> = RefCell::new(Vec::new());
    let ctx = crate::eval_ctx_with_meta(values, functions, meta_map)
        .with_runtime_diagnostics(&sink);
    let _ = reify_expr::eval_expr(expr, &ctx);
    sink.into_inner()
}

/// Compute the 32-byte `upstream_values_hash` for a single realization.
///
/// Folds the `content_hash()` of each scalar-arg value across all ops in the
/// realization using `reify_core::hash::ContentHash` (XXH3-128) with:
/// - seed `b"uvh1"`
/// - per-op arg iteration (skipping [`CrossSubGeometryRef`] args — those
///   panic inside `reify_expr::eval_expr`)
/// - `b"uvh2"` lo/hi packing into the final 32-byte field
///
/// **Single canonical implementation** shared by all three callers —
/// `post_process_geometry_handle_cells`, `hydrate_geometry_handles_into_values`,
/// and `mint_symbolic_geometry_handles_into_values` — removing the three-way
/// duplication flagged at engine_build.rs:7575-7576 and guaranteeing §7.1
/// identity: a symbolic eval-path handle (`kernel_handle = None`) and the
/// corresponding build-path handle (`kernel_handle = Some(kh)`) always have
/// the same `upstream_values_hash` → `content_hash`-equal and `PartialEq`-equal
/// (GHR-β: `kernel_handle` excluded from both; step-6, task #4652).
fn compute_realization_upstream_values_hash(
    realization: &reify_compiler::RealizationDecl,
    ctx: &reify_expr::EvalContext<'_>,
) -> [u8; 32] {
    compute_realization_upstream_values_hash_from_ops(&realization.operations, ctx)
}

/// Inner implementation of [`compute_realization_upstream_values_hash`] that
/// operates directly on a `CompiledGeometryOp` slice.
///
/// Split out so the R3d (`#4900`) in-walk mint for the `edit_param` reeval
/// walk can call it using graph-resident `RealizationNodeData.operations`
/// (which is the same type but is not wrapped in a `RealizationDecl`).
fn compute_realization_upstream_values_hash_from_ops(
    operations: &[reify_compiler::CompiledGeometryOp],
    ctx: &reify_expr::EvalContext<'_>,
) -> [u8; 32] {
    use reify_core::hash::ContentHash;

    let mut h = ContentHash::of(b"uvh1");
    for op in operations {
        let args: &[(String, reify_ir::CompiledExpr)] = match op {
            reify_compiler::CompiledGeometryOp::Primitive { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Modify { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Transform { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Pattern { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Sweep { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Curve { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Profile { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Surface { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Isosurface { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Boolean { .. } => &[],
        };
        for (arg_name, expr) in args {
            // CrossSubGeometryRef (`self.<sub>.<member>`) is a geometry-ref arg
            // compiled into the scalar args list. eval_expr unreachable!()s on it;
            // it may be top-level OR nested, so walk the whole arg tree. Its
            // identity is already captured in the GeomRef target/profiles — skip.
            if arg_contains_cross_sub_geometry_ref(expr) {
                continue;
            }
            let v = reify_expr::eval_expr(expr, ctx);
            h = h
                .combine(ContentHash::of_str(arg_name))
                .combine(v.content_hash());
        }
    }
    // Pack the 128-bit XXH3 hash into a 32-byte field:
    // bytes [0..16]  = h (the main combined hash)
    // bytes [16..32] = h salted with "uvh2" (distinct second half)
    let lo = h.0.to_le_bytes();
    let hi = h.combine(ContentHash::of(b"uvh2")).0.to_le_bytes();
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&lo);
    out[16..].copy_from_slice(&hi);
    out
}

/// Resolves an `Output` occurrence's raw `path` field into the fully-resolved
/// destination written by [`Engine::build_outputs`] (io-export δ).
///
/// The B7 design-relative-path rule
/// (`docs/prds/v0_6/io-export-import-completion.md` §7.3): an absolute `raw`
/// path is returned verbatim; a relative `raw` path is joined onto
/// `out_dir_override` when present (a CI escape hatch that beats the design
/// dir), otherwise onto `design_dir` (the directory containing the `.ri` design
/// file). Keeping the rule in one pure function makes `ExportArtifact.path`
/// fully resolved and unit-testable without spawning the CLI binary.
fn resolve_artifact_path(
    raw: &str,
    design_dir: &std::path::Path,
    out_dir_override: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let raw_path = std::path::Path::new(raw);
    if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        out_dir_override.unwrap_or(design_dir).join(raw_path)
    }
}

#[cfg(test)]
mod tests;

// ── populate_attribute_history LocalFeature unit tests (step-3, RED) ────────

/// Tests for `populate_attribute_history` with `AttributeHistory::LocalFeature`.
///
/// RED: `AttributeHistory::LocalFeature` variant and the dispatch arm in
/// `populate_attribute_history` do not exist yet. Tests compile after step-4.
#[cfg(test)]
mod populate_local_feature_tests;

// ── dispatch_volume_mesh unit tests ──────────────────────────────────────────

#[cfg(test)]
mod dispatch_volume_mesh_tests;

/// Produce an info-level diagnostic when a swept body is meshed with P1
/// hex/wedge despite the user requesting `element_order = P2`.
///
/// P2 hex/wedge is deferred to v0.4+; the runtime silently produces P1 hex
/// instead. This helper is the canonical source of that per-body diagnostic,
/// cited by PRD `docs/prds/v0_3/hex-wedge-meshing.md` task #10.
///
/// # Contract
///
/// Returns `Some(Diagnostic::info(...))` only when ALL of the following hold:
/// - `swept_kind` is `Some(_)` — the body qualified for hex/wedge promotion.
/// - `force_tet` is `false` — hex/wedge meshing was not suppressed by the
///   caller before we got here.
/// - `element_order == ElementOrderTag::P2` — a substitution is actually
///   happening (P1 is correct behaviour; only P2 triggers the warning).
///
/// Returns `None` in all other cases (no diagnostic to emit).
///
/// # One-shot guarantee
///
/// The helper is stateless. "One diagnostic per body" is enforced at the call
/// site — each realization-final body handle invokes this helper exactly once,
/// matching the `swept_kind_table.record(handle, kind)` per-handle pattern.
///
/// # Variant invariance
///
/// The message wording is variant-invariant per PRD task #10 — it does not
/// distinguish hex vs wedge meshing outcomes (that is determined downstream by
/// the gmsh recombine path, not by the sweep classifier variant). All three
/// `SweptKind` variants (Extrude, Revolve, SweepLinear) produce the same
/// message text when the three emission conditions hold; only the body label
/// differs.
// Integration-test follow-up: once this helper is wired into the engine's
// realization pipeline, add an end-to-end test that runs a P2 elastic solve
// on a scene with at least two qualifying swept bodies and asserts exactly
// one `Severity::Info` diagnostic per body (not zero, not two). The unit tests
// below exercise the helper's contract but cannot verify the one-shot guarantee
// at the call-site level.
//
// Not yet wired into the engine's realization pipeline; blocked on task
// #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in
// dispatch_volume_mesh). See compute-node-contract.md §6 for the full task
// history and rejected-alternative rationale.
#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β)
pub(crate) fn p2_substitution_diagnostic(
    swept_kind: Option<&SweptKind>,
    force_tet: bool,
    element_order: reify_ir::ElementOrderTag,
    body_label: &str,
) -> Option<Diagnostic> {
    // Three suppression guards — ordered cheapest first for short-circuit:
    // 1. swept_kind=None: body didn't qualify for hex/wedge promotion.
    // 2. force_tet: hex/wedge was suppressed upstream; no substitution occurs.
    // 3. element_order=P1: user didn't request P2, nothing to warn about.
    swept_kind?;
    if force_tet {
        return None;
    }
    if element_order != reify_ir::ElementOrderTag::P2 {
        return None;
    }
    Some(Diagnostic::info(format!(
        "Body {body_label} qualified for hex/wedge meshing; P1 hex used despite \
`element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet."
    )))
}

// ── p2_substitution_diagnostic unit tests ────────────────────────────────────

#[cfg(test)]
mod p2_substitution_diagnostic_tests;

// ── build_mixed_region_mesh unit tests (T12 layer B) ──────────────────────────

#[cfg(test)]
mod mixed_region_tests;

// ── post_process_mechanism_mass_props unit tests (task 4472 step-7) ───────────
//
// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
// The test calls it directly to verify the engine pass iterates values and
// writes `derived_mass_props` back into mechanism cells.

#[cfg(test)]
mod post_process_mechanism_mass_props_tests;

// ── post_process_cross_sub_value_cells unit tests (task 4725 amendment) ──────
//
// Direct-call unit pin (`MockGeometryKernel`, no OCCT) for the rescope
// (`CompiledExpr::map_value_refs`) + dispatch + is_collection /
// already-folded guards inside `post_process_cross_sub_value_cells`. The
// OCCT-gated `cross_entity_aggregate_folds_via_fixpoint` /
// `total_mass_computed` integration pins only exercise this pass when OCCT
// is available (reviewer finding: no isolated regression pin existed).

#[cfg(test)]
mod post_process_cross_sub_value_cells_tests;

// ── diagnose_topology_correspondence_drops unit tests (task 4545 step-3) ─────
//
// RED: `diagnose_topology_correspondence_drops` does not exist yet.
// These tests drive the pure helper over hand-built AttributeHistory values
// to verify the expected Warning diagnostics (one per non-zero counter).
// No OCCT kernel is required — all counters are plain u32 fields.

#[cfg(test)]
mod diagnose_topology_correspondence_drops_tests;
