// Split from lib.rs (task 2032) — build methods.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CompiledModule, CurveKind, GeomRef, ModifyKind, PatternKind,
    PrimitiveKind, ProfileKind, SubComponentDecl, SweepKind, TopologyTemplate, TransformKind,
};
use reify_core::{Diagnostic, DiagnosticLabel, RealizationNodeId, SourceSpan, VersionId};
use reify_ir::{
    AttributeHistory, BooleanOpHistoryRecords, BooleanOpParents, CapabilityDescriptor,
    CompiledFunction, ElementOrderTag, ErrorRef, ExportFormat, FeatureId, FeatureTag,
    FeatureTagTable, Freshness, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp,
    GeometryQuery, KernelHandle, KernelId, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords,
    Operation, ReprKind, SweepOpHistoryRecords, TopologyAttribute, TopologyAttributeTable,
    ValueMap, VolumeMesh,
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
use crate::primitive_attribute_seed::{parse_bbox_xyz_min, seed_primitive_attributes_for_handle};
use crate::realization_cache::{NO_OPTIONS, RealizationCache};
use crate::sweep_classifier::{
    SweptKind, SweptKindTable, classify_swept_body, swept_kind_to_sweep_params,
};
use crate::topology_attribute_propagation::{
    LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M, detect_local_index_reassignment_diagnostics,
    populate_extrude_attributes, populate_loft_attributes, populate_revolve_attributes,
    populate_sweep_attributes, propagate_attributes_via_brepalgoapi_history,
};
use crate::{BuildResult, Engine, MeshSurface, TessellateResult};

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
    feature_tag_table: &'a mut FeatureTagTable,
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
        feature_tag_table: &'a mut FeatureTagTable,
        topology_attribute_table: &'a mut TopologyAttributeTable,
        swept_kind_table: &'a mut SweptKindTable,
        produced_repr_out: &'a mut Option<ReprKind>,
    ) -> Self {
        Self {
            step_handles,
            named_steps,
            feature_tag_table,
            topology_attribute_table,
            swept_kind_table,
            produced_repr_out,
        }
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
/// Per-instance ops intentionally skip `feature_tag_table` /
/// `topology_attribute_table` / `swept_kind_table` population — those tables
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

            // 4. Re-execute each named realization of the child template
            //    against the override values map.  Uses
            //    `compile_geometry_op` + `kernel.execute_with_history`
            //    directly — bypasses `RealizationCache` (keyed by entity
            //    name, so two subs of the same child would collide) and the
            //    multi-kernel dispatcher (no per-op routing needed here;
            //    the default kernel handles every primitive/transform op in
            //    the child's realization chain).
            //
            //    `GeomRef::Step(i)` within a realization is resolved against
            //    the per-realization `per_instance_step_handles` accumulator,
            //    so multi-op child realizations (e.g. `translate(box(...), …)`)
            //    chain correctly even when the intermediate step handle was not
            //    produced by the outer `Engine::execute_realization_ops`.
            for realization in &child_template.realizations {
                let realization_name = match realization.name.as_deref() {
                    Some(n) => n,
                    None => continue, // unnamed realizations carry no user-visible handle
                };

                // Same-call dedup: reuse a previously computed per-instance
                // handle when two subs of the same child have identical args.
                let dedup_key = (
                    child_template.name.clone(),
                    args_fingerprint.clone(),
                    realization_name.to_string(),
                );
                if let Some(&cached) = per_call_dedup.get(&dedup_key) {
                    named_steps.insert(format!("{}.{}", sub.name, realization_name), cached);
                    continue;
                }

                // Accumulates handles for `GeomRef::Step` resolution within
                // this realization's ops (resets per-realization).
                let mut per_instance_step_handles: Vec<GeometryHandleId> = Vec::new();
                let mut realization_ok = true;

                // v0.1 scope boundary: pass an EMPTY named-steps map to the
                // child's op compiler so that any `self.<innersub>.body`
                // reference inside the child's realization reliably produces
                // "unresolvable GeomRef::Sub" rather than accidentally
                // resolving against the parent's scope.  Nested sub-of-sub
                // override propagation is out of scope for this task (see
                // rustdoc above and the pinning test
                // `cross_sub_nested_sub_in_override_path_produces_compile_error`).
                let child_named_steps: HashMap<String, reify_ir::KernelHandle> = HashMap::new();

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
                                    "per-instance re-realization compile error for \
                                     {}.{}.{}: {}",
                                    template.name, sub.name, realization_name, msg
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
                                    "per-instance re-realization kernel error for \
                                     {}.{}.{}: {}",
                                    template.name, sub.name, realization_name, e
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

                if realization_ok && let Some(&final_handle) = per_instance_step_handles.last() {
                    // Override-path handles are produced by `default_kernel_name`
                    // (the kernel borrowed above), so tag them with that kernel's
                    // KernelId. (The no-args path copies child-snapshot handles
                    // verbatim, preserving whichever kernel produced each one.)
                    let final_handle = KernelHandle {
                        kernel: kernel_id_for_registry_name(default_kernel_name),
                        id: final_handle,
                    };
                    named_steps.insert(format!("{}.{}", sub.name, realization_name), final_handle);
                    per_call_dedup.insert(dedup_key, final_handle);
                }
            }
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
            populate_loft_op(table, kernel, feature_id, profiles, result_handle, history)
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
fn populate_single_parent_sweep_op(
    table: &mut TopologyAttributeTable,
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
fn populate_boolean_op(
    table: &mut TopologyAttributeTable,
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
            | GeometryOp::RotateAround { target, .. }
            | GeometryOp::ApplyTransform { target, .. }
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
            _ => unreachable!("descriptor role SingleTarget but op lacks target field"),
        },

        // Single-profile sweep ops — profile only; path/spine excluded.
        // Per `populate_attribute_history` (engine_build.rs:103-114):
        // "the path/spine is not itself a parent".
        ParentRole::SingleProfile => match op {
            GeometryOp::Extrude { profile, .. }
            | GeometryOp::ExtrudeSymmetric { profile, .. }
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
            | GeometryOp::RotateAround { target, .. }
            | GeometryOp::ApplyTransform { target, .. }
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
            _ => unreachable!("descriptor role SingleTarget but op lacks target field"),
        },

        // Single-profile sweep ops — profile only; path/spine/guide excluded.
        ParentRole::SingleProfile => match op {
            GeometryOp::Extrude { profile, .. }
            | GeometryOp::ExtrudeSymmetric { profile, .. }
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
    use ReprKind::{BRep, Mesh};
    const BREP_MESH: &[ReprKind] = &[BRep, Mesh];
    const BREP_ONLY: &[ReprKind] = &[BRep];
    match op {
        // Booleans — accept both reprs
        BooleanUnion | BooleanDifference | BooleanIntersection => Some(BREP_MESH),

        // Modify — BRep-only consumers
        ModifyFillet | ModifyChamfer | ModifyShell | ModifyDraft | ModifyThicken
        | ModifyOffsetCurve | ModifyZoneSlab | ModifyOffsetSolid => Some(BREP_ONLY),

        // Transform — accept both reprs. `TransformApplyTransform` is the
        // post-realization rigid-isometry application (task 3901); like the
        // scalar transforms it is repr-agnostic, so it accepts both BRep and
        // Mesh inputs.
        TransformTranslate
        | TransformRotate
        | TransformScale
        | TransformRotateAround
        | TransformApplyTransform => Some(BREP_MESH),

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
        | PrimitiveWedge | PrimitiveTorus => Some(BREP_ONLY),

        // Curves — sources (no geometric input); same rationale as Primitives.
        CurveLineSegment | CurveArc | CurveHelix | CurveInterpCurve | CurveBezierCurve
        | CurveNurbsCurve => Some(BREP_ONLY),

        // Profile face producers — sources (no geometric input); same rationale.
        ProfileRectangle | ProfileCircle | ProfilePolygon | ProfileEllipse => Some(BREP_ONLY),

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

/// Map a compiled geometry op to its `Operation` classifier key.
///
/// Exhaustive match over `CompiledGeometryOp`/kind sub-enums so a new variant
/// fails to compile until mapped — same discipline as `geometry_op_to_operation`
/// at :902, but over the compiled-IR form rather than the runtime `GeometryOp`.
#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)
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
    }
}

/// Collect all `GeomRef::Sub` operands referenced by a compiled geometry op.
#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)
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
        CompiledGeometryOp::Primitive { .. }
        | CompiledGeometryOp::Curve { .. }
        | CompiledGeometryOp::Profile { .. } => {}
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
            .map(|t| demanded_reprs_for_template(t, format))
            .collect()
    }
}

fn demanded_reprs_for_template(template: &TopologyTemplate, format: ExportFormat) -> Vec<ReprKind> {
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
            // Non-terminal: Mesh unless a disqualifier forces BRep.
            // `demand[*c_idx] == ReprKind::BRep` subsumes the conservative case:
            // any c_idx with conservative_producers[c_idx]==true had demand[c_idx]
            // set to BRep in the first branch above, and c_idx > r_idx so it was
            // resolved before this point in the reverse pass.
            let needs_brep = consumer_ops[r_idx].iter().any(|(c_idx, op)| {
                !op_accepts_repr(op, ReprKind::Mesh) || demand[*c_idx] == ReprKind::BRep
            });
            demand[r_idx] = if needs_brep {
                ReprKind::BRep
            } else {
                ReprKind::Mesh
            };
        }
    }

    demand
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

impl Engine {
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
        self.last_dispatch_count = 0;
        // GHR-δ §5: clear the realization→handle validity map and reset the
        // revalidation slow-path counter at the start of every build surface;
        // the per-template `post_process_geometry_handle_cells` below
        // repopulates the map with this build's resolved handles.
        self.realization_handles.clear();
        self.reset_geometry_revalidation_slow_path_count();
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

            self.feature_tag_table = FeatureTagTable::default();
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
                    Engine::execute_realization_ops(
                        &mut self.geometry_kernels,
                        &registry_borrowed,
                        name,
                        &realization.operations,
                        &realization.feature_tags,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        RealizationOutputs::new(
                            &mut step_handles,
                            &mut named_steps,
                            &mut self.feature_tag_table,
                            &mut self.topology_attribute_table,
                            &mut self.swept_kind_table,
                            &mut produced_repr_out,
                        ),
                        &mut diagnostics,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                        &mut self.realization_cache,
                        demanded_tol,
                        // Task 4050 step-16 (gap 3): pass the υ-derived
                        // per-realization demanded terminal repr, positionally
                        // aligned with `demanded_tols` (`[t_idx][r_idx]`);
                        // out-of-range defaults to BRep (backward-compat).
                        demanded_reprs
                            .get(t_idx)
                            .and_then(|v| v.get(r_idx))
                            .copied()
                            .unwrap_or(ReprKind::BRep),
                        &mut self.last_dispatch_count,
                        // Task #3443: thread module-scope #kernel(...) pragma
                        // from the public entry point into the per-op dispatcher.
                        module.kernel_pragma.as_deref(),
                        r_idx + 1 == template.realizations.len(),
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
                }
                // Step-8 (task ε / 3436): the post-process helpers operate on
                // the engine's default kernel. We re-borrow it from the
                // `geometry_kernels` map here (after the per-realization loop
                // released its `&mut self.geometry_kernels` borrow). The
                // `expect` is justified by the outer `contains_key(name)`
                // gate: the executor never removes entries from the map.
                let default_kernel = self.geometry_kernels.get_mut(name).expect(
                    "default kernel must remain in the map across the per-realization loop",
                );
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
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    default_kernel.as_mut(),
                    &self.topology_attribute_table,
                    &self.swept_kind_table,
                    &mut diagnostics,
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
                        let mut output = Vec::new();
                        let default_kernel = self
                            .geometry_kernels
                            .get(name)
                            .expect("default kernel must remain in the map for export");
                        match default_kernel.export(product_bodies[0].handle_id, format, &mut output) {
                            Ok(()) => Some(output),
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
                                match default_kernel.export(compound.id, format, &mut output) {
                                    Ok(()) => Some(output),
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
        // Task ε (3436) step-12: reset the dispatch-count instrumentation
        // counter at the entry to every build/tessellate surface so a second
        // build of the same module reports its own per-build dispatch tally
        // (and reports 0 when fully served from the RealizationCache). Mirrors
        // the reset at the top of `build_snapshot` / `tessellate_realizations`
        // / `tessellate_snapshot` — must run BEFORE `check()` because no
        // dispatcher call should be counted against the build that hasn't
        // entered the per-realization op loop yet.
        self.last_dispatch_count = 0;
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
        // Task 2320: `values` is moved out of `check_result` here so the
        // per-template post-process can patch conformance-query results
        // (`is_watertight` / `is_manifold` / `is_orientable`) into the map
        // before it is moved into the returned `BuildResult` below.
        let mut values = check_result.values;

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

        // Task 4358 ε: compute the unified build-DAG plan ONCE, up front, when the
        // active scheduler is UnifiedDag. δ previously materialized this AFTER the
        // realization loop purely for its cycle/unresolved diagnostics (and then
        // discarded `schedule`). ε consumes `pass.schedule` to drive the
        // realization loop below in Kahn order (so a curated selector cell is
        // hydrated before its consuming realization), while still appending
        // `pass.diagnostics` at the SAME later site to keep the diagnostic vector
        // byte-identical to δ. `run_unified_pass` returns an OWNED triple and
        // reads only `snapshot.graph` + `trace_map` (neither mutated by the
        // realization loop, which only patches `produced_repr`/`produced_kernel`
        // node fields), so hoisting the call here is behaviour-preserving. The
        // call is skipped entirely under LegacyMultiPass (the default), so that
        // path pays nothing and stays byte-unchanged.
        let unified_pass: Option<crate::engine_fixpoint::UnifiedPassResult> = if self
            .build_scheduler
            == crate::engine_fixpoint::BuildScheduler::UnifiedDag
        {
            self.eval_state.as_ref().map(|state| {
                crate::engine_fixpoint::run_unified_pass(&state.snapshot.graph, &state.trace_map)
            })
        } else {
            None
        };

        // Task 4358 ε: the value cells read by ANY realization (the union of every
        // realization trace's `reads`). A selector cell in this set is consumed as
        // a curated fillet/chamfer/draft edge/face list, so
        // `hydrate_value_cell_in_loop` resolves it one step past its
        // `Value::Selector` descriptor to a concrete `List<Geometry>`; selector
        // cells consumed only by selector-composition value cells are absent here
        // and keep their descriptor form (so `reconstruct_selector_value` still
        // sees a `Value::Selector` child). Empty under LegacyMultiPass — the whole
        // schedule-driven hydration is gated on `unified_pass.is_some()`.
        let realization_read_cells: HashSet<reify_core::ValueCellId> = self
            .eval_state
            .as_ref()
            .filter(|_| unified_pass.is_some())
            .map(|state| {
                state
                    .trace_map
                    .iter()
                    .filter(|(node, _)| matches!(node, NodeId::Realization(_)))
                    .flat_map(|(_, tr)| tr.reads.iter().cloned())
                    .collect()
            })
            .unwrap_or_default();

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

            self.feature_tag_table = FeatureTagTable::default();
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
                                steps.push(BuildStep::Realize(r_idx));
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
                    Engine::execute_realization_ops(
                        &mut self.geometry_kernels,
                        &registry_borrowed,
                        name,
                        &realization.operations,
                        &realization.feature_tags,
                        &values,
                        &self.functions,
                        &self.meta_map,
                        RealizationOutputs::new(
                            &mut step_handles,
                            &mut named_steps,
                            &mut self.feature_tag_table,
                            &mut self.topology_attribute_table,
                            &mut self.swept_kind_table,
                            &mut produced_repr_out,
                        ),
                        &mut diagnostics,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                        &mut self.realization_cache,
                        demanded_tol,
                        // Task 4050 step-16 (gap 3): pass the υ-derived
                        // per-realization demanded terminal repr, positionally
                        // aligned with `demanded_tols` (`[t_idx][r_idx]`);
                        // out-of-range defaults to BRep (backward-compat).
                        demanded_reprs
                            .get(t_idx)
                            .and_then(|v| v.get(r_idx))
                            .copied()
                            .unwrap_or(ReprKind::BRep),
                        &mut self.last_dispatch_count,
                        // Task #3443: thread module-scope #kernel(...) pragma
                        // from the public entry point into the per-op dispatcher.
                        module.kernel_pragma.as_deref(),
                        r_idx + 1 == template.realizations.len(),
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
                // Step-8 (task ε / 3436): re-borrow the default kernel from
                // the map for post-process — see `build_snapshot` mirror.
                let default_kernel = self.geometry_kernels.get_mut(name).expect(
                    "default kernel must remain in the map across the per-realization loop",
                );
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
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    &self.functions,
                    &self.meta_map,
                    default_kernel.as_mut(),
                    &self.topology_attribute_table,
                    &self.swept_kind_table,
                    &mut diagnostics,
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
                        let mut output = Vec::new();
                        let default_kernel = self
                            .geometry_kernels
                            .get(name)
                            .expect("default kernel must remain in the map for export");
                        match default_kernel.export(
                            product_bodies[0].handle_id,
                            format,
                            &mut output,
                        ) {
                            Ok(()) => Some(output),
                            Err(e) => {
                                diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                                None
                            }
                        }
                    }
                    _ => {
                        // Multiple product bodies — assemble a compound then export.
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
                                match default_kernel.export(compound.id, format, &mut output) {
                                    Ok(()) => Some(output),
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
        #[cfg(debug_assertions)]
        if let Some(state) = self.eval_state.as_ref() {
            crate::dirty::assert_dag_complete_from_graph(
                &state.snapshot.graph,
                &module.fields,
                &exec_order,
            );
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
            let (recheck_results, recheck_diags) = if self.build_scheduler
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
                self.check_constraints_against_templates(module, &values, determinacy)
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
                        reify_ir::Value::GeometryHandle { kernel_handle, .. } => {
                            Some(*kernel_handle)
                        }
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

                // (6) Emit one file via the default kernel's export(); isolate a
                //     kernel failure as an error diagnostic + continue.
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
                // diagnostic.
                let diagnostics = warnings
                    .into_iter()
                    .map(|w| match w {
                        reify_ir::ExportWarning::StepAp242Fallback => Diagnostic::warning(format!(
                            "{}: STEPOutput occurrence `{}.{}` requested AP242 but the \
                                 linked OCCT rejected it; wrote AP214 instead",
                            crate::W_STEP_AP242_FALLBACK,
                            template.name,
                            sub.name
                        )),
                    })
                    .collect();

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
        let mut scratch_feature_tags = FeatureTagTable::default();
        let mut scratch_topo_attrs = TopologyAttributeTable::default();
        let mut scratch_swept_kinds = SweptKindTable::default();
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();

        for (t_idx, template) in module.templates.iter().enumerate() {
            let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
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
                    &mut self.geometry_kernels,
                    &registry_borrowed,
                    &name,
                    &realization.operations,
                    &realization.feature_tags,
                    &values,
                    &self.functions,
                    &self.meta_map,
                    RealizationOutputs::new(
                        &mut step_handles,
                        &mut named_steps,
                        &mut scratch_feature_tags,
                        &mut scratch_topo_attrs,
                        &mut scratch_swept_kinds,
                        &mut produced_repr_out,
                    ),
                    &mut diagnostics,
                    &realization.id,
                    realization.name.as_deref(),
                    realization.span,
                    &mut kernel_error,
                    &mut self.realization_cache,
                    demanded_tol,
                    demanded_reprs
                        .get(t_idx)
                        .and_then(|v| v.get(r_idx))
                        .copied()
                        .unwrap_or(ReprKind::BRep),
                    &mut self.last_dispatch_count,
                    // Task #3443: the distance query path is outside the
                    // user's design pragma scope — pass None (lex-min default).
                    None,
                    r_idx + 1 == template.realizations.len(),
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
        self.last_dispatch_count = 0;
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
        self.feature_tag_table = FeatureTagTable::default();
        self.topology_attribute_table = TopologyAttributeTable::default();
        self.swept_kind_table = SweptKindTable::default();
        // Determinacy β (task 4198): clear the achieved-tol map at the start
        // of each tessellate_realizations call so stale entries from a prior
        // call do not leak into the new result.
        self.achieved_repr_tol.clear();
        // θ (task 4361) step-6: compute the unified pass and realization_read_cells
        // from eval_state BEFORE the &mut self.geometry_kernels borrow so both can
        // be threaded into tessellate_from_values for Kahn-order scheduling.
        // Empty / None under LegacyMultiPass (tessellate_from_values falls back to
        // declaration order, byte-identical to the pre-θ behaviour).
        let (unified_pass_tess, realization_read_cells_tess) = {
            if self.build_scheduler == crate::engine_fixpoint::BuildScheduler::UnifiedDag {
                if let Some(state) = self.eval_state.as_ref() {
                    let pass =
                        crate::engine_fixpoint::run_unified_pass(&state.snapshot.graph, &state.trace_map);
                    let cells: HashSet<reify_core::ValueCellId> = state
                        .trace_map
                        .iter()
                        .filter(|(node, _)| matches!(node, NodeId::Realization(_)))
                        .flat_map(|(_, tr)| tr.reads.iter().cloned())
                        .collect();
                    (Some(pass), cells)
                } else {
                    (None, HashSet::new())
                }
            } else {
                (None, HashSet::new())
            }
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
            &mut self.feature_tag_table,
            &mut self.topology_attribute_table,
            &mut self.swept_kind_table,
            &mut self.realization_cache,
            &demanded_tols,
            &tessellation_budgets,
            &mut self.last_dispatch_count,
            self.capture_repr_tol,
            &mut self.achieved_repr_tol,
            unified_pass_tess.as_ref(),
            &realization_read_cells_tess,
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
        feature_tag_table: &mut FeatureTagTable,
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
        // Determinacy β (task 4198): when `true`, `surface_subtree` calls
        // `kernel.measure_mesh_deviation` and populates `achieved_repr_tol`.
        // `false` by default — zero hot-path overhead when γ assertions
        // (`RepresentationWithin`) are not active.  Mirrors `capture_undef_causes`.
        capture_repr_tol: bool,
        // Determinacy β (task 4198): cleared at entry to each
        // `tessellate_realizations` / `tessellate_snapshot` call by the
        // caller; populated inside `surface_subtree` after each successful
        // tessellation when `capture_repr_tol` is true. Threaded here as a
        // sibling of the other &mut tables (feature_tag_table /
        // topology_attribute_table / swept_kind_table).
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
                            steps.push(BuildStep::Realize(r_idx));
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
                Engine::execute_realization_ops(
                    geometry_kernels,
                    registry,
                    default_kernel_name,
                    &realization.operations,
                    &realization.feature_tags,
                    values,
                    functions,
                    meta_map,
                    RealizationOutputs::new(
                        &mut step_handles,
                        &mut named_steps,
                        &mut *feature_tag_table,
                        &mut *topology_attribute_table,
                        &mut *swept_kind_table,
                        &mut produced_repr_out,
                    ),
                    diagnostics,
                    &realization.id,
                    realization.name.as_deref(),
                    realization.span,
                    &mut kernel_error,
                    realization_cache,
                    demanded_tol,
                    // Task 4050 step-8 / design_decision 4: the tessellate path
                    // discards produced_repr and stays on a BRep demand
                    // permanently (a Manifold terminal would break the trailing
                    // default-kernel tessellate call).
                    ReprKind::BRep,
                    &mut *dispatch_count,
                    // Task #3443: thread module-scope #kernel(...) pragma
                    // from the tessellate entry point into the per-op dispatcher.
                    module.kernel_pragma.as_deref(),
                    r_idx + 1 == template.realizations.len(),
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
            Engine::run_post_processes(
                template,
                &named_steps,
                values,
                functions,
                meta_map,
                default_kernel.as_mut(),
                topology_attribute_table,
                &*swept_kind_table,
                diagnostics,
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
    /// **Cache-hit short-circuit** (task 2874, step-8 wiring): at the very
    /// start of the helper — BEFORE the `for (op_idx, op) in
    /// operations.iter().enumerate()` op loop — when both `demanded_tol`
    /// and `realization_name` are `Some(_)` AND
    /// `realization_cache.lookup(realization_id.entity, ReprKind::BRep, t, NO_OPTIONS)`
    /// returns `Some(&handle)`, the helper:
    ///   - pushes the cached handle onto `step_handles` (mirrors the
    ///     successful-realization handle-stack post-condition),
    ///   - inserts `(name, cached_handle)` into `named_steps` (mirrors the
    ///     post-rollback `named_steps` write so downstream
    ///     `GeomRef::Sub("body")` lookups continue to resolve),
    ///   - returns early — skipping the kernel op loop, the
    ///     `compile_geometry_op` evaluations, the per-op
    ///     `feature_tag_table` / `topology_attribute_table` populations, the
    ///     rollback-truncation gate, and the post-loop cache-insert
    ///     (idempotent: the entry already exists, and re-inserting at the
    ///     same `(entity, repr, tol, NO_OPTIONS)` key would be a no-op under
    ///     the partial-order semantics).
    ///
    ///   `NO_OPTIONS` = `ContentHash(0)` is the PRD §4 "no options" sentinel;
    ///   tasks δ (3435) and ξ (3442) will thread real per-op option hashes
    ///   here when wiring `TessellateOptions` / `VolumeMeshOptions`.
    ///
    /// `realization_name = None` paths (anonymous realizations) bypass the
    /// short-circuit so the named_steps write is never skipped where it
    /// otherwise would not happen — anonymous realizations are not part of
    /// the cache contract today. The post-condition the cache-hit branch
    /// preserves is "after this helper returns successfully, the terminal
    /// handle is the last entry in `step_handles[handle_start..]` AND
    /// `named_steps[name] = terminal_handle`" — exactly the contract the
    /// op-loop success path establishes (see the post-rollback
    /// `step_handles[handle_start..].last()` block below).
    ///
    /// **Known limitation** (recorded as a design decision): a cache-hit
    /// short-circuit skips per-op `feature_tag_table` /
    /// `topology_attribute_table` populations, including the kernel-attribute
    /// hook propagation added in task 2875. Both tables are reset to
    /// `default()` at the start of every `build()` (see callers around
    /// engine_build.rs `feature_tag_table = FeatureTagTable::default()` /
    /// `topology_attribute_table = TopologyAttributeTable::default()`), so a
    /// cache-served handle has no entries in those tables on the second
    /// build. v0.2 callers do not combine `activate_purpose` with attribute
    /// queries today, so this is documented (not regressed) in scope; a
    /// follow-up task can either cache the table entries alongside the
    /// handle or skip the table reset for engines with non-empty cache.
    ///
    /// **Cross-kernel collision guard** (task 4349): on cache-hit the helper
    /// calls `feature_tag_table.remove(cached_handle.id)` (and analogously for
    /// `topology_attribute_table`) to evict any entry that a cross-kernel
    /// sibling op may have recorded at the same bare `GeometryHandleId`. Both
    /// tables are keyed by `GeometryHandleId` only (not the full `KernelHandle`),
    /// and each kernel's counter starts at 1 — so OCCT and Manifold independently
    /// produce `GeometryHandleId(1)`. A Manifold op earlier in the same build may
    /// have written `feature_tag_table.record(GeometryHandleId(1), tag)` before
    /// this cache-hit returns `{Occt, GeometryHandleId(1)}` from a prior build,
    /// collapsing two distinct `KernelHandle`s onto one key. The `remove` is a
    /// no-op in the common single-kernel case (the per-build reset already cleared
    /// the table) and enforces the #3226 spec ("cache-served handle has no entries
    /// in those tables") in the cross-kernel case. The principled re-key of both
    /// tables to `KernelHandle` is deferred to follow-up task #4351.
    #[allow(clippy::too_many_arguments)]
    fn execute_realization_ops(
        kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
        registry: &BTreeMap<String, &CapabilityDescriptor>,
        default_kernel_name: &str,
        operations: &[reify_compiler::CompiledGeometryOp],
        feature_tags: &[FeatureTag],
        values: &ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        outputs: RealizationOutputs<'_>,
        diagnostics: &mut Vec<Diagnostic>,
        realization_id: &RealizationNodeId,
        realization_name: Option<&str>,
        realization_span: SourceSpan,
        kernel_error_out: &mut Option<ErrorRef>,
        realization_cache: &mut RealizationCache<KernelHandle>,
        demanded_tol: Option<f64>,
        // Task 4050 step-8: the realization's requested terminal [`ReprKind`]
        // (υ-derived in build/build_snapshot; `ReprKind::BRep` everywhere else).
        // Each op dispatches at this repr; a `None` plan with
        // `demanded_repr != BRep` falls back to a BRep dispatch (design_decision
        // 3) so a Mesh demand no linked kernel can satisfy routes BRep instead
        // of erroring. Slotted next to `demanded_tol`.
        demanded_repr: ReprKind,
        // Task ε (3436) step-12: caller-write dispatch-count instrumentation
        // channel. Incremented once per `dispatch(...)` call inside the per-op
        // loop. The caller (build / build_snapshot / tessellate_*) resets the
        // backing `Engine::last_dispatch_count` field to 0 at the entry-point
        // and passes a mutable reference into it; the cache-hit short-circuit
        // returns BEFORE the loop, so the counter stays at 0 on a re-hit.
        dispatch_count: &mut usize,
        // Task #3443 (ο): module-scoped `#kernel(...)` pragma preference.
        // `Some(name)` steers the terminal-stage kernel selection in
        // `dispatcher::dispatch` when the named kernel is registered and its
        // descriptor supports the demanded (op, repr); absent/unsatisfiable
        // falls through to the existing lex-min scan (PRD §5 "warning, not
        // error"). Callers on the build/tessellate entry-point paths supply
        // `module.kernel_pragma.as_deref()`; the tolerance-budget query and
        // the `DispatchTestState` pragma-agnostic tests pass `None`.
        prefer_kernel: Option<&str>,
        // Task 3437 (ζ): only the TERMINAL realization of an entity (the one
        // with the highest index, i.e. `r_idx + 1 == template.realizations.len()`)
        // should probe or insert into the `RealizationCache`. Intermediate
        // realizations all share the same `entity` cache key; if we probe/insert
        // for them we get false hits (realization N finds realization N-1's
        // result for the same entity key) which violates the per-build
        // reset invariant and produces wrong geometry (the intermediate let-
        // binding gets the terminal's handle instead of its own).
        is_terminal_realization: bool,
    ) {
        let RealizationOutputs {
            step_handles,
            named_steps,
            feature_tag_table,
            topology_attribute_table,
            swept_kind_table,
            produced_repr_out,
        } = outputs;
        let handle_start = step_handles.len();
        // Task 4050 step-8: the per-op `available` set is no longer a hoisted
        // loop-invariant `{BRep}` constant — it is derived per op from the
        // reprs of that op's resolved input handles (`realization_step_reprs`,
        // tracked in lockstep with `realization_step_ids` below), defaulting to
        // `{BRep}` for primitives / unresolved refs (design_decision 6). This
        // lets a conversion stage that materialises a Mesh handle propagate the
        // Mesh repr to downstream ops while staying `{BRep}` for the v0.2 path.

        // Task 2874, step-8: cache-hit short-circuit. When the caller has
        // threaded a demanded tolerance AND the realization is named (the
        // `named_steps` contract requires a name to write into the map),
        // probe the per-engine `RealizationCache` at
        // `(entity_id, cache_repr, demanded_tol)`. On hit we push the
        // cached terminal handle, write `named_steps[name] = cached_handle`,
        // and return — preserving the post-condition the success path
        // establishes below. On miss (or when either guard is `None`) we
        // fall through to the kernel op loop, and step-6's post-success
        // insert at the bottom of the helper populates the cache for the
        // NEXT call. The lookup uses `RealizationCache`'s partial-order
        // "tighter satisfies looser" rule (`cached_tol ≤ requested_tol`),
        // so a tighter request automatically misses a looser cached entry
        // (see step-13's pin).
        //
        // **Amendment (suggestion #3)**: the cache repr is bound to a local
        // `cache_repr` so the lookup-key repr and the `produced_repr_out`
        // write below are sourced from the same value. If a future change
        // shifts the cache key to a non-`BRep` `ReprKind` (the cache's
        // `(entity, repr, tol, options)` shape already supports it; see
        // `RealizationCache::lookup`), the `produced_repr_out` write follows
        // without a separate edit.
        //
        // **Task 4050 step-10 (gap 4)**: `cache_repr` is unpinned from `BRep` to
        // the realization's `demanded_repr` (the υ-derived requested terminal
        // repr, known before the op loop). The cache-hit LOOKUP keys on it, so a
        // second identical Mesh build short-circuits at `(entity, Mesh, tol)`.
        // The post-loop INSERT keys on the RESOLVED repr instead (see below), so
        // a fallback realization that demanded Mesh but resolved BRep is stored
        // at BRep: a later Mesh lookup correctly MISSES at the Mesh key (never
        // returning a BRep handle as if it were Mesh), and the BRep fallback
        // probe added below then recovers the hit at the resolved BRep key.
        let cache_repr = demanded_repr;
        // **Amendment (reviewer_comprehensive #1, perf regression)**: probe the
        // terminal cache at the demanded repr first, then — for a non-BRep
        // demand that missed — RETRY at `BRep`. This mirrors the per-op dispatch
        // BRep fallback (design_decision 3) at the cache layer, and is the fix
        // for the realization-cache regression flagged in review.
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
                // Cross-kernel collision guard (task 4349): `FeatureTagTable`
                // and `TopologyAttributeTable` are keyed by bare
                // `GeometryHandleId` — NOT by the full `KernelHandle`. Each
                // kernel's handle-id counter starts at 1, so OCCT and Manifold
                // independently produce `GeometryHandleId(1)` for their first
                // handle. Within one build a Manifold op may record
                // `feature_tag_table.record(GeometryHandleId(1), tag)` before
                // this cache-hit short-circuit returns the cached
                // `{Occt, GeometryHandleId(1)}` from a prior build — two
                // distinct `KernelHandle`s collapsing onto the same numeric key.
                //
                // Rather than asserting the table is empty at the cached key
                // (which fails under cross-kernel collision even though the
                // per-build reset is unconditional), we defensively remove any
                // entry at `cached_handle.id` from both tables. This is a no-op
                // in the common single-kernel case (the per-build reset already
                // cleared the table) and enforces the #3226 spec ("a cache-served
                // handle has no entries in those tables on the second build") in
                // the cross-kernel case by evicting the colliding sibling entry.
                //
                // Trade-off: before this change the SIBLING handle (e.g.
                // Manifold's `GeometryHandleId(1)`) was the last writer and
                // therefore returned its correct tag on `lookup(1)` — only the
                // cache-served handle read the wrong (foreign) value.  After
                // `remove()`, the sibling's `lookup(1)` also returns `None`,
                // regressing it from correct to absent.  This is the accepted
                // interim cost of enforcing the #3226 spec on the cached handle;
                // only follow-up task #4351's `KernelHandle` re-key will
                // preserve both entries independently and eliminate the
                // regression.
                feature_tag_table.remove(cached_handle.id);
                topology_attribute_table.remove(cached_handle.id);
                step_handles.push(cached_handle);
                named_steps.insert(name.to_string(), cached_handle);
                // Step-10 (task ε / 3436): the [`RealizationCache`] key includes
                // the repr (see the post-success `realization_cache.insert` call
                // at the bottom of this function), so the cached terminal handle
                // was produced by a kernel capable of `resolved_repr` —
                // `cache_repr` on a primary hit, or `BRep` on the fallback hit.
                // Surface that SAME repr through `produced_repr_out` so the
                // caller writes into the realization graph node exactly what a
                // cold-path build of this realization would have written.
                *produced_repr_out = Some(resolved_repr);
                // **Task 4050 step-10**: consistency guard, reordered AFTER the
                // `produced_repr_out` write. The surfaced produced_repr always
                // equals the cache key's repr on the cache-hit branch (the line
                // above just wrote `Some(resolved_repr)`); kept as a documented
                // invariant guarding future edits to the probe/surface pair.
                debug_assert_eq!(
                    resolved_repr,
                    produced_repr_out.unwrap_or(ReprKind::BRep),
                    "cache-hit produced_repr must equal the cache key's repr",
                );
                return;
            }
        } // end is_terminal_realization cache-probe guard

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
        // Task 4050 step-8: the produced [`ReprKind`] of each step handle,
        // tracked in lockstep with `realization_step_ids`. The per-op
        // `available` set is read from the reprs of the op's resolved input
        // handles (via this Vec); every push site below pushes here too so the
        // two Vecs stay index-aligned.
        let mut realization_step_reprs: Vec<ReprKind> = Vec::with_capacity(operations.len());
        // Task 4050 step-12: per-realization log of intermediate-cache keys the
        // conversion executor inserted, so step-14's rollback branch can drop
        // exactly those keys (atomic with `step_handles.truncate(handle_start)`).
        // Each entry is `(entity, repr, per_stage_tol)`; the options_hash is
        // always `NO_OPTIONS` for conversion intermediates. On the success path
        // the inserts stay committed so later same-build realizations reuse them.
        let mut intermediate_cache_inserts: Vec<(String, ReprKind, f64)> = Vec::new();
        // Task #3443 (S6): track whether the KernelPragmaUnsatisfiable warning
        // has already been emitted for this realization. The pragma is
        // module-scoped and applies uniformly to all ops; emitting once per
        // realization (on the first unsatisfiable op) avoids spamming the
        // author with one warning per op when the whole realization shares
        // the same unsatisfiable preference (PRD §5 "warning, not error").
        let mut pragma_warn_emitted = false;
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
                        if set.is_empty() {
                            set.insert(ReprKind::BRep);
                        }
                        set
                    };
                    // Task ε (3436) step-12: bump the per-build dispatch
                    // counter EXACTLY at the `dispatch(...)` call site so the
                    // cache-hit short-circuit (which returns above without
                    // ever entering this loop) leaves the counter at 0. Bumped
                    // once at the primary dispatch; the design_decision-3
                    // fallback re-dispatch below does not bump again.
                    *dispatch_count += 1;
                    // Task #3443 (S6): emit KernelPragmaUnsatisfiable warning
                    // (at most once per realization) when the prefer_kernel
                    // from the `#kernel(...)` pragma cannot serve this op at
                    // the demanded repr. The realization proceeds normally via
                    // lex-min fallback (PRD §5 "warning, not error"). The flag
                    // ensures one warning per module-scoped pragma regardless
                    // of how many ops share the same unsatisfiable preference.
                    if let Some(name) = prefer_kernel {
                        if !pragma_warn_emitted
                            && !crate::dispatcher::kernel_pragma_satisfiable(
                                registry,
                                name,
                                operation,
                                demanded_repr,
                            )
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
                    }
                    // Task 4050 step-8: dispatch at `demanded_repr`, then FALL
                    // BACK to a BRep dispatch when the demand is unsatisfiable
                    // and `demanded_repr != BRep` (design_decision 3). Without
                    // this, every Stl/Obj-terminal Mesh demand with no linked
                    // Mesh kernel would hit the strict no-kernel-chain error arm
                    // and regress the whole suite; with it, such ops route BRep
                    // exactly as the v0.2 baseline did.
                    let plan = dispatch(registry, operation, demanded_repr, &available_for_op, prefer_kernel)
                        .or_else(|| {
                            if demanded_repr != ReprKind::BRep {
                                // BRep fallback (design_decision 3): pragma preference
                                // is not forwarded here because the fallback fires only
                                // when the preferred repr is unsatisfiable — passing
                                // prefer_kernel on the fallback path would silently pick
                                // the pragma kernel at BRep demand even when the user's
                                // #kernel(X) intent was for the primary demanded repr.
                                dispatch(registry, operation, ReprKind::BRep, &available_for_op, None)
                            } else {
                                None
                            }
                        });
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
                                    }
                                }
                                if conversion_error.is_none() && tessellate_source.is_none() {
                                    conversion_error = Some(format!(
                                        "internal error: conversion chain for op \
                                         '{operation:?}' has no Tessellate stage (no \
                                         BRep→Mesh source kernel found in plan.conversions)"
                                    ));
                                }

                                // ── Phase 2: tessellate + ingest once per parent ──
                                // For each parent: tessellate on the Tessellate-stage
                                // source kernel → Mesh, then ingest the Mesh into
                                // plan.kernel → fresh handle. The ingest call voxelises
                                // when plan.kernel is an OpenVDB kernel (Mesh→Voxel)
                                // and is a trivial Mesh→Mesh pass-through when
                                // plan.kernel is a Manifold/similar kernel.
                                if conversion_error.is_none() {
                                    let source_name = tessellate_source.expect("checked above");
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
                                        // and skip the redundant tessellate+ingest.
                                        if let Some(&cached) = realization_cache.lookup(
                                            &intermediate_entity,
                                            terminal_to,
                                            per_stage_tol,
                                            NO_OPTIONS,
                                        ) {
                                            substitution.insert(pid, cached.id);
                                            continue;
                                        }
                                        // Cache miss: tessellate on the source kernel
                                        // (`&self`); borrow released before the
                                        // `&mut` ingest borrow below.
                                        let mesh = match kernels.get(source_name) {
                                            Some(src) => match src.tessellate(pid, per_stage_tol) {
                                                Ok(mesh) => mesh,
                                                Err(e) => {
                                                    conversion_error =
                                                        Some(format!("tessellation error: {e}"));
                                                    break 'convert;
                                                }
                                            },
                                            None => {
                                                conversion_error = Some(format!(
                                                    "internal error: conversion source kernel \
                                                 '{source_name}' absent from \
                                                 engine.geometry_kernels"
                                                ));
                                                break 'convert;
                                            }
                                        };
                                        // Ingest into the target kernel (`&mut`).
                                        // For a Manifold kernel this is Mesh→Mesh;
                                        // for an OpenVDB kernel this is Mesh→Voxel.
                                        let ingested = kernels
                                            .get_mut(plan.kernel.as_str())
                                            .expect("plan.kernel presence checked above")
                                            .ingest_mesh(&mesh);
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
                                                    NO_OPTIONS,
                                                    intermediate_handle,
                                                );
                                                intermediate_cache_inserts.push((
                                                    intermediate_entity,
                                                    terminal_to,
                                                    per_stage_tol,
                                                ));
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
                            // Record the parallel-array feature tag for this handle.
                            if let Some(&tag) = feature_tags.get(op_idx) {
                                feature_tag_table.record(handle.id, tag);
                            }
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
                            if let Err(e) = seed_primitive_attributes_for_handle(
                                topology_attribute_table,
                                kernel,
                                handle.id,
                                &feature_id,
                                &geom_op,
                            ) {
                                diagnostics.push(Diagnostic::warning(format!(
                                "topology-attribute seeding failed for {realization_id} op {op_idx}: {e}"
                            )));
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
            for (entity, repr, tol) in &intermediate_cache_inserts {
                realization_cache.remove(entity, *repr, *tol, NO_OPTIONS);
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
            let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
                topology_attribute_table
                    .iter()
                    .filter(|(_, attr)| attr.feature_id == realization_feature_id)
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
            if let Some(&last) = step_handles[handle_start..].last() {
                if let Some(name) = realization_name {
                    // Bare-name key (e.g. "b") backs same-structure GeomRef::Sub("b")
                    // refs emitted by the compiler's sibling pre-check (task #4668
                    // step-2, geometry.rs).  Cross-sub keys ("sub.member") are seeded
                    // separately via the compound-key injection path below.  Both are
                    // consumed by geometry_ops.rs::resolve_geom_ref's Sub arm.
                    named_steps.insert(name.to_string(), last);
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
                    // `feature_tag_table` (from its own op run earlier in this
                    // build), the per-build reset debug_assert fires.  Only the
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
        use reify_core::{hash::ContentHash, identity::ValueCellId};
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

                // Fold content_hashes of all scalar-arg values across the
                // realization's ops to form upstream_values_hash. Boolean
                // ops (left/right GeomRefs) carry no scalar args and are
                // skipped. Domain separator `b"uvh2"` ensures non-zero
                // output even when all arg lists are empty (e.g. a zero-arg
                // primitive that still needs a non-zero hash tag).
                let mut h = ContentHash::of(b"uvh1");
                for op in &realization.operations {
                    let args: &[(String, reify_ir::CompiledExpr)] = match op {
                        reify_compiler::CompiledGeometryOp::Primitive { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Modify { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Transform { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Pattern { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Sweep { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Curve { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Profile { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Boolean { .. } => &[],
                    };
                    for (arg_name, expr) in args {
                        // A CrossSubGeometryRef (`self.<sub>.<member>`) is a
                        // geometry-ref arg compiled into the scalar args list
                        // by compile_expr (geometry.rs:749). eval_expr would
                        // panic on it (unreachable! in reify-expr). It may be
                        // the top-level arg OR nested inside a larger operator
                        // node (e.g. `translate(rotate(self.inner.body, …), …)`),
                        // so walk the whole arg tree. Its identity is already
                        // captured in the GeomRef `target`/`profiles` field —
                        // skip the arg for hash purposes.
                        if arg_contains_cross_sub_geometry_ref(expr) {
                            continue;
                        }
                        let v = reify_expr::eval_expr(expr, &ctx);
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
                let mut upstream_values_hash = [0u8; 32];
                upstream_values_hash[..16].copy_from_slice(&lo);
                upstream_values_hash[16..].copy_from_slice(&hi);

                entries.push((
                    ValueCellId::new(realization.id.entity.as_str(), name),
                    Value::GeometryHandle {
                        realization_ref: realization.id.clone(),
                        upstream_values_hash,
                        kernel_handle,
                    },
                ));
            }
        } // ctx dropped — &ValueMap borrow released

        for (cell_id, value) in entries {
            values.insert(cell_id, value);
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
        use reify_core::{hash::ContentHash, identity::ValueCellId};
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
                let mut h = ContentHash::of(b"uvh1");
                for op in &realization.operations {
                    let args: &[(String, reify_ir::CompiledExpr)] = match op {
                        reify_compiler::CompiledGeometryOp::Primitive { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Modify { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Transform { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Pattern { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Sweep { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Curve { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Profile { args, .. } => args,
                        reify_compiler::CompiledGeometryOp::Boolean { .. } => &[],
                    };
                    for (arg_name, expr) in args {
                        // A CrossSubGeometryRef (`self.<sub>.<member>`) is a geometry-ref
                        // arg compiled into the scalar args list (geometry.rs). eval_expr
                        // would panic on it (unreachable! in reify-expr); it may be the
                        // top-level arg OR nested inside a larger operator node, so walk
                        // the whole arg tree. Its identity is already captured in the
                        // GeomRef target/profiles. Skip the arg for hashing.
                        if arg_contains_cross_sub_geometry_ref(expr) {
                            continue;
                        }
                        let v = reify_expr::eval_expr(expr, &ctx);
                        h = h
                            .combine(ContentHash::of_str(arg_name))
                            .combine(v.content_hash());
                    }
                }
                let lo = h.0.to_le_bytes();
                let hi = h.combine(ContentHash::of(b"uvh2")).0.to_le_bytes();
                let mut upstream_values_hash = [0u8; 32];
                upstream_values_hash[..16].copy_from_slice(&lo);
                upstream_values_hash[16..].copy_from_slice(&hi);
                entries.push((
                    ValueCellId::new(realization.id.entity.as_str(), name),
                    Value::GeometryHandle {
                        realization_ref: realization.id.clone(),
                        upstream_values_hash,
                        kernel_handle,
                    },
                ));
            }
        }
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
    /// selector→list → topology selector → resolve-selector coercion); the first
    /// helper that returns `Some` wins. A cell whose `default_expr` is not a
    /// recognised query/selector is left untouched. Only the *timing* (before vs.
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
    /// query → selector→list → topology selector → resolve-selector coercion). The
    /// two sites MUST stay in sync: if the ORDER or the SET of helpers below
    /// changes, the in-loop single-cell ladder in `hydrate_value_cell_in_loop` must
    /// change identically, or a cell's resolution would diverge (which helper
    /// "wins") only under UnifiedDag, only when that cell is hydrated in-loop ahead
    /// of a consuming realization. See that function's doc comment for the matching
    /// ladder and the rationale for the one deliberate divergence (a
    /// realization-consumed selector is resolved one step further, to a `List`).
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
        diagnostics: &mut Vec<Diagnostic>,
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
        // task 4229: re-evaluate Let cells whose expressions depend on
        // topology-selector-derived cells (e.g. `moi_principal =
        // eigenvalues(moment_of_inertia)` where `moment_of_inertia` was just
        // patched above). Must run after `post_process_topology_selectors` so
        // the patched values are visible.
        Engine::post_process_derived_lets(template, values, functions, meta_map, diagnostics);
        Engine::post_process_ad_hoc_selectors(
            template,
            named_steps,
            values,
            kernel,
            table,
            diagnostics,
        );
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
    /// and re-evaluates their `default_expr` using the now-updated `values`
    /// map.  Only cells whose re-evaluation yields a non-`Undef` result are
    /// updated — cells whose arguments are still `Undef` (missing kernel,
    /// no geometry) remain `Undef` and are left untouched.
    ///
    /// **Ordering contract**: must run after `post_process_topology_selectors`
    /// (and `post_process_geometry_queries`) so that patched-in geometry-derived
    /// values are visible; runs before `post_process_body_mass_props` and
    /// `post_process_mechanism_mass_props` (those passes do not produce `Let`
    /// cells that downstream pure-math lets could consume).
    fn post_process_derived_lets(
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        _diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Collect candidates first to avoid holding a borrow on `values`
        // while also inserting into it.
        let candidates: Vec<(reify_core::ValueCellId, reify_ir::CompiledExpr)> = template
            .value_cells
            .iter()
            .filter(|cell| matches!(cell.kind, reify_compiler::ValueCellKind::Let))
            .filter(|cell| values.get(&cell.id).is_none_or(|v| v.is_undef()))
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

        for (cell_id, expr) in candidates {
            let new_val = {
                let ctx = crate::eval_ctx_with_meta(values, functions, meta_map);
                reify_expr::eval_expr(&expr, &ctx)
            };
            if !new_val.is_undef() {
                values.insert(cell_id, new_val);
            }
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
        self.last_dispatch_count = 0;
        let state = self.eval_state.as_ref()?;

        // θ (task 4361) step-6: compute the unified pass and realization_read_cells
        // from eval_state EARLY (immutable borrows only) before the &mut self.* borrows
        // needed by `tessellate_from_values`. Both `build_scheduler` and `eval_state`
        // are separate fields; Rust allows disjoint shared borrows here.
        // Empty / None under LegacyMultiPass (tessellate_from_values falls back to
        // declaration order, byte-identical to the pre-θ behaviour).
        let (unified_pass_snap, realization_read_cells_snap) = {
            if self.build_scheduler == crate::engine_fixpoint::BuildScheduler::UnifiedDag {
                let pass =
                    crate::engine_fixpoint::run_unified_pass(&state.snapshot.graph, &state.trace_map);
                let cells: HashSet<reify_core::ValueCellId> = state
                    .trace_map
                    .iter()
                    .filter(|(node, _)| matches!(node, NodeId::Realization(_)))
                    .flat_map(|(_, tr)| tr.reads.iter().cloned())
                    .collect();
                (Some(pass), cells)
            } else {
                (None, HashSet::new())
            }
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
        self.feature_tag_table = FeatureTagTable::default();
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
            &mut self.feature_tag_table,
            &mut self.topology_attribute_table,
            &mut self.swept_kind_table,
            &mut self.realization_cache,
            &demanded_tols,
            &tessellation_budgets,
            &mut self.last_dispatch_count,
            self.capture_repr_tol,
            &mut self.achieved_repr_tol,
            unified_pass_snap.as_ref(),
            &realization_read_cells_snap,
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
    for (handle_id, _) in realization_attrs {
        match kernel.query(&GeometryQuery::Centroid(*handle_id)) {
            Ok(value) => match crate::topology_selectors::parse_xyz_value(
                &value,
                "local_index_reassignment_centroid",
            ) {
                Ok(xyz) => {
                    centroids.insert(*handle_id, xyz);
                }
                Err(e) => {
                    parse_fail_count += 1;
                    if parse_fail_first.is_none() {
                        parse_fail_first = Some(e.to_string());
                    }
                }
            },
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
#[allow(dead_code)]
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
#[allow(dead_code, clippy::too_many_arguments)]
// G-allow: §3.2 realization-kind dispatch seam (VolumeMesh) per engine-integration-norm §3.2; consumer pending task #3429 (CN-contract §8 task κ — adds execute_realization_ops call edge) / mesh-morph #2947
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
    let mut elements: Vec<UnifiedElement> =
        Vec::with_capacity(shell.triangles.len() + tet.tet_indices.len());
    for tri in &shell.triangles {
        elements.push(UnifiedElement {
            kind: UnifiedElementKind::Shell,
            connectivity: vec![tri[0] as usize, tri[1] as usize, tri[2] as usize],
        });
    }
    // Per-tet node count from the element order (P1 = 4, P2 = 10); tet local
    // node `m` → unified node `n_shell + m`.
    let nodes_per_tet = match tet.element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };
    for tet_conn in tet.tet_indices.chunks_exact(nodes_per_tet) {
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
mod tests {
    use super::*;

    /// step-05 (RED): `resolve_artifact_path` resolves an `Output` occurrence's
    /// raw `path` field against the design-file directory, an optional
    /// `--out-dir` override, or verbatim when already absolute.
    ///
    /// This is the pure core of the B7 design-relative-path rule
    /// (`docs/prds/v0_6/io-export-import-completion.md` §7.3): a relative
    /// occurrence path joins onto `out_dir_override.unwrap_or(design_dir)` — so
    /// the override is a CI escape hatch that beats the design dir — while an
    /// absolute path ignores both bases. Encapsulating the rule here makes
    /// `build_outputs`'s `ExportArtifact.path` fully resolved and unit-testable
    /// without spawning the CLI binary.
    #[test]
    fn resolve_artifact_path_handles_relative_override_and_absolute() {
        use std::path::{Path, PathBuf};

        // Relative path + design dir, no override → joins onto the design dir.
        assert_eq!(
            resolve_artifact_path("o.stl", Path::new("/d"), None),
            PathBuf::from("/d/o.stl"),
        );

        // Relative path + override → the override wins over the design dir.
        assert_eq!(
            resolve_artifact_path("o.stl", Path::new("/d"), Some(Path::new("/ci"))),
            PathBuf::from("/ci/o.stl"),
        );

        // Absolute path → verbatim, ignoring both bases.
        assert_eq!(
            resolve_artifact_path("/abs/x.stl", Path::new("/d"), Some(Path::new("/ci"))),
            PathBuf::from("/abs/x.stl"),
        );
    }

    // ── build_outputs occurrence-driven export (io-export δ steps 7–14) ───────

    /// Recording kernel for the io-export δ driver tests: delegates the full
    /// `GeometryKernel` surface to a `MockGeometryKernel`, and additionally
    /// captures (a) every handle `execute` produced — so a test can identify the
    /// realized geometry handle (e.g. the `part` box) the occurrence's `subject`
    /// must resolve to — and (b) every `export(handle, format)` call's
    /// `(handle, format)` pair. `export` still delegates to the inner mock (which
    /// writes `MOCK_EXPORT_DATA`), so `ExportArtifact.bytes` is non-empty.
    /// Capturing the export format proves the DSL `Output` occurrence — not a
    /// hardcoded CLI flag — drove the serializer.
    struct ExportRecordingKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        executed: std::sync::Arc<std::sync::Mutex<Vec<reify_ir::GeometryHandleId>>>,
        exported: std::sync::Arc<
            std::sync::Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>,
        >,
        /// Per-call `(handle, format, step_schema)` recorded by
        /// `export_with_options` — proves the DSL `version` reached the kernel
        /// as a [`reify_ir::StepSchema`].
        exported_options: std::sync::Arc<
            std::sync::Mutex<
                Vec<(
                    reify_ir::GeometryHandleId,
                    reify_ir::ExportFormat,
                    reify_ir::StepSchema,
                )>,
            >,
        >,
        /// Warnings `export_with_options` returns. The live OCCT AP242 fallback
        /// can't be triggered in-build (this build supports AP242DIS), so the
        /// `W_STEP_AP242_FALLBACK` diagnostic wiring is exercised by injecting
        /// [`reify_ir::ExportWarning::StepAp242Fallback`] here. Default empty.
        warnings_to_return: Vec<reify_ir::ExportWarning>,
    }

    impl ExportRecordingKernel {
        /// Construct a recording kernel sharing the caller's `executed` and
        /// `exported` capture buffers, with a fresh empty `exported_options`
        /// log and no injected warnings. New fields acquire their defaults
        /// here, so adding one no longer ripples across every call site.
        ///
        /// Read the per-call `(handle, format, step_schema)` log back via
        /// [`recorded_options`](Self::recorded_options); inject fallback
        /// warnings via [`with_warnings`](Self::with_warnings).
        fn new(
            executed: std::sync::Arc<std::sync::Mutex<Vec<reify_ir::GeometryHandleId>>>,
            exported: std::sync::Arc<
                std::sync::Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>,
            >,
        ) -> Self {
            Self {
                inner: reify_test_support::mocks::MockGeometryKernel::new(),
                executed,
                exported,
                exported_options: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                warnings_to_return: Vec::new(),
            }
        }

        /// A clone of the shared `exported_options` handle — the per-call
        /// `(handle, format, step_schema)` records `export_with_options`
        /// captured. Grab it before the kernel is moved into the `Engine`.
        fn recorded_options(
            &self,
        ) -> std::sync::Arc<
            std::sync::Mutex<
                Vec<(
                    reify_ir::GeometryHandleId,
                    reify_ir::ExportFormat,
                    reify_ir::StepSchema,
                )>,
            >,
        > {
            std::sync::Arc::clone(&self.exported_options)
        }

        /// Builder: seed the warnings `export_with_options` returns. The live
        /// OCCT AP242 fallback can't be triggered in-build (this build supports
        /// AP242DIS), so the `W_STEP_AP242_FALLBACK` diagnostic wiring is
        /// exercised by injecting [`reify_ir::ExportWarning::StepAp242Fallback`].
        fn with_warnings(mut self, warnings: Vec<reify_ir::ExportWarning>) -> Self {
            self.warnings_to_return = warnings;
            self
        }
    }

    impl reify_ir::GeometryKernel for ExportRecordingKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            let result = self.inner.execute(op);
            if let Ok(handle) = &result {
                self.executed.lock().unwrap().push(handle.id);
            }
            result
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.exported.lock().unwrap().push((handle, format));
            self.inner.export(handle, format, writer)
        }

        fn export_with_options(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            options: &reify_ir::ExportOptions,
            writer: &mut dyn std::io::Write,
        ) -> Result<Vec<reify_ir::ExportWarning>, reify_ir::ExportError> {
            // Record the schema the driver threaded from the DSL `version`, then
            // delegate to `export` (which records (handle, format) for the prior
            // δ tests and writes bytes via the inner mock). Return the
            // configured warnings so the W_STEP_AP242_FALLBACK diagnostic wiring
            // can be exercised without a live OCCT AP242 rejection.
            self.exported_options
                .lock()
                .unwrap()
                .push((handle, format, options.step_schema));
            self.export(handle, format, writer)?;
            Ok(self.warnings_to_return.clone())
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn make_compound(
            &mut self,
            handles: &[reify_ir::GeometryHandleId],
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.make_compound(handles)
        }
    }

    /// step-07 (RED): `build_outputs` drives a single `STLOutput` occurrence to
    /// exactly one `ExportArtifact` whose `format` (STL) and `path` ("o.stl",
    /// resolved design-relative) come from the DSL, and whose exported handle is
    /// the realized `part` box (the occurrence's `subject`).
    ///
    /// Asserting the single export's `format == Stl` proves the DSL occurrence —
    /// not a hardcoded flag — chose the serializer (B5); asserting its handle is
    /// one the kernel realized proves the `subject: part` arg resolved to live
    /// geometry.
    ///
    /// RED until step-08 adds `Engine::build_outputs`: the method does not yet
    /// exist, so this test fails to compile.
    #[test]
    fn build_outputs_drives_single_stl_output() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, resolution: 0.2mm, path: "o.stl")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            1,
            "exactly one ExportArtifact for the single STLOutput occurrence, got {}",
            artifacts.len()
        );
        let art = &artifacts[0];
        assert_eq!(
            art.format,
            reify_ir::ExportFormat::Stl,
            "the DSL STLOutput occurrence must drive ExportFormat::Stl"
        );
        assert_eq!(
            art.path,
            PathBuf::from("/tmp/d/o.stl"),
            "a relative occurrence path joins onto the design dir (B7)"
        );
        assert!(
            !art.bytes.is_empty(),
            "the kernel export() must have written bytes into the artifact"
        );

        let exported = exported.lock().unwrap().clone();
        assert_eq!(
            exported.len(),
            1,
            "exactly one export() call for the single occurrence, got {}",
            exported.len()
        );
        assert_eq!(
            exported[0].1,
            reify_ir::ExportFormat::Stl,
            "the recorded export() format must be Stl (DSL-driven, not flag-driven)"
        );
        let executed = executed.lock().unwrap().clone();
        assert!(
            executed.contains(&exported[0].0),
            "the exported handle {:?} must be a realized kernel handle (the resolved \
             `subject: part`); realized handles were {:?}",
            exported[0].0,
            executed
        );
    }

    /// step-09 (ε / task 4288): the `build_outputs` driver threads each
    /// STEPOutput occurrence's STEP schema — read off its `version` field by
    /// `extract_output_export_spec` — into the kernel via `export_with_options`,
    /// proving the DSL `version`, not a hardcoded default, reaches the
    /// serializer.
    ///
    /// `version: STEPVersion.AP203` → the recording kernel observes exactly one
    /// `export_with_options` call whose recorded `step_schema == Ap203`; a
    /// STEPOutput with no `version` field defaults to `Ap214` (the DSL default
    /// `version : STEPVersion = STEPVersion.AP214`).
    #[test]
    fn build_outputs_threads_step_version_into_export_options() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::Path;
        use std::sync::{Arc, Mutex};

        // Run build_outputs on `src` and return the per-call `step_schema`s the
        // kernel recorded via `export_with_options`, in call order.
        let run = |src: &str| -> Vec<reify_ir::StepSchema> {
            let module = parse_and_compile_with_stdlib(src);
            let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
                Arc::new(Mutex::new(Vec::new()));
            let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
                Arc::new(Mutex::new(Vec::new()));
            let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
            let exported_options = kernel.recorded_options();
            let mut engine = crate::Engine::new(
                Box::new(MockConstraintChecker::new()),
                Some(Box::new(kernel)),
            );
            engine.build_outputs(&module, Path::new("/tmp/d"), None);
            let recorded = exported_options.lock().unwrap().clone();
            recorded.into_iter().map(|(_, _, schema)| schema).collect()
        };

        // version: STEPVersion.AP203 → exactly one export_with_options call, Ap203.
        let ap203 = run(r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub s = STEPOutput(subject: part, version: STEPVersion.AP203, path: "p.step")
}"#);
        assert_eq!(
            ap203,
            vec![reify_ir::StepSchema::Ap203],
            "the DSL `version: STEPVersion.AP203` must thread Ap203 into export_with_options"
        );

        // No `version` field → DSL default Ap214.
        let default = run(r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub d = STEPOutput(subject: part, path: "def.step")
}"#);
        assert_eq!(
            default,
            vec![reify_ir::StepSchema::Ap214],
            "a STEPOutput with no `version` defaults to Ap214 (the DSL default)"
        );
    }

    /// step-11 (ε / task 4288): when the kernel reports an AP242→AP214
    /// fallback (`ExportWarning::StepAp242Fallback`), the driver surfaces it as
    /// exactly one warning-severity diagnostic carrying the
    /// `W_STEP_AP242_FALLBACK` code and naming the occurrence — *without*
    /// dropping the successfully written bytes (a fallback is honest
    /// degradation, not a failure). The live OCCT AP242 fallback cannot be
    /// triggered in this build (it supports AP242DIS), so the warning is
    /// injected via the recording kernel's `warnings_to_return`.
    #[test]
    fn build_outputs_surfaces_ap242_fallback_warning() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::Path;
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub s = STEPOutput(subject: part, version: STEPVersion.AP242, path: "x.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        // Inject the AP242→AP214 fallback the in-build OCCT can't produce.
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported))
            .with_warnings(vec![reify_ir::ExportWarning::StepAp242Fallback]);
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            1,
            "exactly one ExportArtifact for the single STEPOutput occurrence, got {}",
            artifacts.len()
        );
        let art = &artifacts[0];
        assert!(
            !art.bytes.is_empty(),
            "a fallback is a WARNING, not a failure: the written bytes must survive"
        );

        let fallback_diags: Vec<&reify_core::Diagnostic> = art
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("W_STEP_AP242_FALLBACK"))
            .collect();
        assert_eq!(
            fallback_diags.len(),
            1,
            "exactly one W_STEP_AP242_FALLBACK diagnostic for the injected fallback, got {}",
            fallback_diags.len()
        );
        assert_eq!(
            fallback_diags[0].severity,
            Severity::Warning,
            "the AP242 fallback must be warning-severity (honest degradation, not an error)"
        );
        assert!(
            fallback_diags[0].message.contains("D.s"),
            "the diagnostic must name the occurrence (`D.s`); message was: {}",
            fallback_diags[0].message
        );
    }

    /// step-09 (RED): `build_outputs` emits one [`crate::ExportArtifact`] per
    /// recognized `Output` occurrence, in declaration order (B6).
    ///
    /// Two occurrences on the same solid — `sub o = STLOutput(...)` then
    /// `sub s = STEPOutput(...)` — must yield exactly two artifacts in source
    /// order: `[{Stl, "/tmp/d/o.stl"}, {Step, "/tmp/d/o2.step"}]`, and the
    /// recording kernel must observe the two `export()` calls as `[Stl, Step]`
    /// in that same order. The `STEPOutput` occurrence's `format` default
    /// (`OutputFormat.STEP`) must route to `ExportFormat::Step`, proving the
    /// per-occurrence DSL format — not a single shared flag — drives each file.
    ///
    /// RED until step-10: the step-08 happy path breaks after the FIRST
    /// recognized occurrence, so it emits a single STL artifact and this test's
    /// `artifacts.len() == 2` (and the `[Stl, Step]` export order) fail.
    #[test]
    fn build_outputs_emits_one_artifact_per_occurrence_in_declaration_order() {
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub s = STEPOutput(subject: part, path: "o2.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        assert_eq!(
            artifacts.len(),
            2,
            "one artifact per Output occurrence (STLOutput + STEPOutput), got {}",
            artifacts.len()
        );
        // Declaration order: STLOutput first, STEPOutput second.
        assert_eq!(artifacts[0].format, reify_ir::ExportFormat::Stl);
        assert_eq!(artifacts[0].path, PathBuf::from("/tmp/d/o.stl"));
        assert_eq!(
            artifacts[1].format,
            reify_ir::ExportFormat::Step,
            "the STEPOutput occurrence's format default (STEP) must route to Step"
        );
        assert_eq!(artifacts[1].path, PathBuf::from("/tmp/d/o2.step"));

        let exported = exported.lock().unwrap().clone();
        let formats: Vec<reify_ir::ExportFormat> = exported.iter().map(|(_, f)| *f).collect();
        assert_eq!(
            formats,
            vec![reify_ir::ExportFormat::Stl, reify_ir::ExportFormat::Step],
            "the recording kernel must observe per-occurrence exports [Stl, Step] \
             in declaration order, got {:?}",
            formats
        );
        let executed = executed.lock().unwrap().clone();
        for (handle, _) in &exported {
            assert!(
                executed.contains(handle),
                "each exported handle {:?} must be a realized `subject: part` \
                 handle; realized handles were {:?}",
                handle,
                executed
            );
        }
    }

    /// step-11 (RED): `build_outputs` RECOGNIZES a `DisplayOutput` occurrence as
    /// a conforming `Output` but DEFERS its file emission (the viewport drive is
    /// a sibling PRD), surfacing an info-severity [`crate::I_DISPLAY_OUTPUT_DEFERRED`]
    /// diagnostic instead of a file — while an `Input` occurrence (`STEPInput`)
    /// is EXCLUDED entirely (it conforms to `Input`, not `Output`).
    ///
    /// The module mixes all three: one `STLOutput` (a file), one `DisplayOutput`
    /// (recognize-but-defer), one `STEPInput` (not an Output at all). The driver
    /// must therefore produce exactly ONE file artifact (the STLOutput, with
    /// non-empty bytes), surface exactly ONE `I_DISPLAY_OUTPUT_DEFERRED` info
    /// diagnostic for the DisplayOutput, and emit NEITHER artifact NOR diagnostic
    /// for the STEPInput. The recording kernel must observe exactly ONE
    /// `export()` call (the STLOutput) — proving DisplayOutput/STEPInput drove no
    /// serialization.
    ///
    /// RED until step-12: the step-8/10 happy path `continue`s silently on a
    /// `DisplayDeferred` target, so no `I_DISPLAY_OUTPUT_DEFERRED` diagnostic is
    /// surfaced and this test's diagnostic assertion fails.
    #[test]
    fn build_outputs_defers_display_output_and_excludes_input() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub d = DisplayOutput(subject: part)
    sub i = STEPInput(source: "in.step")
}"#,
        );

        let executed: Arc<Mutex<Vec<reify_ir::GeometryHandleId>>> =
            Arc::new(Mutex::new(Vec::new()));
        let exported: Arc<Mutex<Vec<(reify_ir::GeometryHandleId, reify_ir::ExportFormat)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let kernel = ExportRecordingKernel::new(Arc::clone(&executed), Arc::clone(&exported));
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        // Exactly one FILE artifact (non-empty bytes): the STLOutput. The
        // DisplayOutput is recognized-but-deferred (a zero-byte skipped entry,
        // never a written file); STEPInput contributes no entry at all.
        let files: Vec<&crate::ExportArtifact> =
            artifacts.iter().filter(|a| !a.bytes.is_empty()).collect();
        assert_eq!(
            files.len(),
            1,
            "exactly one FILE artifact (the STLOutput); DisplayOutput defers and \
             STEPInput is excluded, got files {:?}",
            files.iter().map(|a| &a.path).collect::<Vec<_>>()
        );
        assert_eq!(files[0].format, reify_ir::ExportFormat::Stl);
        assert_eq!(files[0].path, PathBuf::from("/tmp/d/o.stl"));

        // Exactly one info-severity I_DISPLAY_OUTPUT_DEFERRED diagnostic, for the
        // DisplayOutput. "Result diagnostics" = every artifact's diagnostics.
        let display_diags: Vec<&reify_core::Diagnostic> = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.message.contains(crate::I_DISPLAY_OUTPUT_DEFERRED))
            .collect();
        assert_eq!(
            display_diags.len(),
            1,
            "exactly one I_DISPLAY_OUTPUT_DEFERRED diagnostic for the single \
             DisplayOutput occurrence, got {}",
            display_diags.len()
        );
        assert_eq!(
            display_diags[0].severity,
            Severity::Info,
            "the DisplayOutput-deferred diagnostic must be info-severity (not an \
             error that would fail the build)"
        );

        // STEPInput (an `Input`, not an `Output`) produces NO diagnostic of any
        // kind — it is filtered out by the conforms_to_output gate before any
        // spec read.
        let input_diags = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.message.contains("STEPInput") || d.message.contains(".i"))
            .count();
        assert_eq!(
            input_diags, 0,
            "STEPInput is not an Output: it must produce neither artifact nor diagnostic"
        );

        // The kernel serialized exactly once — the STLOutput. DisplayOutput and
        // STEPInput drove no export() call.
        let exported = exported.lock().unwrap().clone();
        assert_eq!(
            exported.len(),
            1,
            "only the STLOutput exports; DisplayOutput defers and STEPInput is \
             excluded, got {} export() calls",
            exported.len()
        );
        assert_eq!(exported[0].1, reify_ir::ExportFormat::Stl);
    }

    /// step-13 helper: a kernel whose FIRST `export()` call fails with a
    /// [`reify_ir::ExportError`] and whose subsequent calls succeed (delegated
    /// to the inner mock). With `build_outputs`'s Phase-B product export
    /// disabled, the only `export()` calls are the per-occurrence ones, so call
    /// #1 is the first `Output` occurrence and call #2 the second — letting a
    /// test drive "first occurrence fails, second succeeds".
    struct FailFirstExportKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        export_calls: std::sync::Mutex<usize>,
    }

    impl reify_ir::GeometryKernel for FailFirstExportKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            let mut n = self.export_calls.lock().unwrap();
            *n += 1;
            if *n == 1 {
                return Err(reify_ir::ExportError::FormatError(
                    "injected failure (first export)".to_string(),
                ));
            }
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn make_compound(
            &mut self,
            handles: &[reify_ir::GeometryHandleId],
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.make_compound(handles)
        }
    }

    /// step-13 (RED): a per-occurrence export failure must be ISOLATED — it
    /// emits an error diagnostic and the loop CONTINUES, so a later valid
    /// `Output` occurrence still serializes its file. One bad Output never
    /// aborts the others (PRD §4.3/§7.3 per-artifact failure isolation).
    ///
    /// Two `STLOutput`s on the same solid; the kernel fails the FIRST `export()`
    /// (occurrence `o`) and succeeds the second (occurrence `s`). The driver
    /// must NOT panic/abort: it surfaces an error-severity diagnostic naming the
    /// failed occurrence's path (`o.stl`) AND still produces a written artifact
    /// (non-empty bytes) for the valid `s` (`o2.stl`).
    ///
    /// RED until step-14: the step-8 happy path `continue`s SILENTLY on an
    /// export `Err` (no diagnostic), so the error-diagnostic assertion fails.
    #[test]
    fn build_outputs_isolates_per_occurrence_export_failure() {
        use reify_core::Severity;
        use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};
        use std::path::{Path, PathBuf};

        let module = parse_and_compile_with_stdlib(
            r#"structure def D {
    let part = box(10mm, 20mm, 5mm)
    sub o = STLOutput(subject: part, path: "o.stl")
    sub s = STLOutput(subject: part, path: "o2.stl")
}"#,
        );

        let kernel = FailFirstExportKernel {
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            export_calls: std::sync::Mutex::new(0),
        };
        let mut engine = crate::Engine::new(
            Box::new(MockConstraintChecker::new()),
            Some(Box::new(kernel)),
        );

        // Must not panic even though the first occurrence's export errors.
        let artifacts = engine.build_outputs(&module, Path::new("/tmp/d"), None);

        // The failed occurrence (`o`) carries an error-severity diagnostic that
        // names its path, so the failure is attributable and not silent.
        let error_diags: Vec<&reify_core::Diagnostic> = artifacts
            .iter()
            .flat_map(|a| &a.diagnostics)
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(
            error_diags.len(),
            1,
            "the failed occurrence must surface exactly one error diagnostic, got {}",
            error_diags.len()
        );
        assert!(
            error_diags[0].message.contains("o.stl"),
            "the error diagnostic must name the failed occurrence's path (o.stl); got {:?}",
            error_diags[0].message
        );

        // Isolation: the valid SECOND occurrence (`s`) still produced a written
        // file with bytes despite the first occurrence failing.
        let written: Vec<&crate::ExportArtifact> =
            artifacts.iter().filter(|a| !a.bytes.is_empty()).collect();
        assert_eq!(
            written.len(),
            1,
            "the valid second occurrence still serializes a file despite the \
             first failing, got {} written artifacts",
            written.len()
        );
        assert_eq!(
            written[0].path,
            PathBuf::from("/tmp/d/o2.stl"),
            "the surviving artifact must be the second (valid) occurrence o2.stl"
        );
    }

    /// step-09 (RED): `seed_cross_sub_named_steps` must thread [`KernelHandle`]
    /// (not bare [`GeometryHandleId`]) through `named_steps` /
    /// `module_named_steps`.
    ///
    /// Exercises the no-args seeding path: a parent template with
    /// `sub a = Inner()` copies the child template's completed `Inner.body`
    /// snapshot entry into the parent's `named_steps` under the compound key
    /// `a.body`. The seeded value is a [`KernelHandle`] carrying the producing
    /// kernel's [`KernelId`] (Manifold here) alongside the kernel-local
    /// [`GeometryHandleId`]; the no-args path copies it verbatim, so `a.body`
    /// must resolve to exactly that [`KernelHandle`] — `.id` equal to the seeded
    /// handle id and `.kernel` equal to the seeding kernel's [`KernelId`].
    ///
    /// RED on the pre-migration signature: `module_named_steps` / `named_steps`
    /// are typed `…GeometryHandleId`, so passing `…KernelHandle` maps fails to
    /// type-check until step-10 flips the value type.
    #[test]
    fn seed_cross_sub_named_steps_threads_kernel_handle_on_no_args_path() {
        use reify_ir::{GeometryHandleId, KernelHandle, KernelId};
        use reify_test_support::builders::TopologyTemplateBuilder;

        // Parent template: `sub a = Inner()` — no args, non-collection.
        let template = TopologyTemplateBuilder::new("Parent")
            .sub_component("a", "Inner", Vec::new())
            .build();

        // Child snapshot: `Inner.body` was produced by the Manifold kernel as
        // GeometryHandleId(5), recorded as a KernelHandle.
        let seeded = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };
        let mut inner_snapshot: HashMap<String, KernelHandle> = HashMap::new();
        inner_snapshot.insert("body".to_string(), seeded);
        let mut module_named_steps: HashMap<String, HashMap<String, KernelHandle>> = HashMap::new();
        module_named_steps.insert("Inner".to_string(), inner_snapshot);

        // The no-args path reads only `template.sub_components` +
        // `module_named_steps`; the kernel/value/function/template inputs are
        // unused on this path, so empty instances suffice.
        let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let templates: Vec<TopologyTemplate> = Vec::new();

        seed_cross_sub_named_steps(
            &template,
            &module_named_steps,
            &mut named_steps,
            &mut kernels,
            "default",
            &values,
            &functions,
            &meta_map,
            &mut diagnostics,
            &templates,
        );

        let got = named_steps
            .get("a.body")
            .copied()
            .expect("no-args seeding must insert compound key `a.body`");
        assert_eq!(
            got, seeded,
            "named_steps value type must be KernelHandle, copied verbatim from the child snapshot"
        );
        assert_eq!(
            got.id,
            GeometryHandleId(5),
            ".id must equal the seeded GeometryHandleId"
        );
        assert_eq!(
            got.kernel,
            KernelId::Manifold,
            ".kernel must equal the seeding kernel's KernelId"
        );
        assert!(diagnostics.is_empty(), "no-args path emits no diagnostics");
    }

    /// `arg_contains_cross_sub_geometry_ref` must detect a `CrossSubGeometryRef`
    /// at the top level *and* nested inside a larger operator node, and must not
    /// false-positive on ref-free args. The nested case is the task-3616
    /// regression: the old top-level-only `matches!` guard let a
    /// `CrossSubGeometryRef` nested in a transform-chain arg
    /// (`translate(rotate(self.inner.body, …), …)`) reach `eval_expr`'s
    /// `unreachable!()` — pinned end-to-end by
    /// `cross_sub_geometry_anti_cascade_no_spurious_errors_in_translate_chain`.
    #[test]
    fn arg_contains_cross_sub_geometry_ref_walks_nested_refs() {
        use reify_core::Type;
        use reify_core::identity::ValueCellId;
        use reify_ir::{BinOp, CompiledExpr};

        // Top-level cross-sub ref → detected.
        let xref = CompiledExpr::cross_sub_geometry_ref(
            ValueCellId::new("Parent.sub", "body"),
            Type::Geometry,
        );
        assert!(arg_contains_cross_sub_geometry_ref(&xref));

        // Cross-sub ref nested inside an operator node → detected (the case the
        // old top-level `matches!` missed).
        let scalar = CompiledExpr::value_ref(ValueCellId::new("E", "width"), Type::Bool);
        let nested = CompiledExpr::binop(BinOp::Gt, xref.clone(), scalar, Type::Bool);
        assert!(arg_contains_cross_sub_geometry_ref(&nested));

        // Ref-free arg → not skipped.
        let plain = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(ValueCellId::new("E", "a"), Type::Bool),
            CompiledExpr::value_ref(ValueCellId::new("E", "b"), Type::Bool),
            Type::Bool,
        );
        assert!(!arg_contains_cross_sub_geometry_ref(&plain));
    }

    // ── shared test helpers (task ε / 3436, step-8) ───────────────────────────

    /// Build a [`CapabilityDescriptor`] that supports every [`Operation`]
    /// variant against [`ReprKind::BRep`]. Used by the
    /// `execute_realization_ops_*` unit tests below to construct a synthetic
    /// dispatch registry that routes every supported op to a single
    /// kernel-by-name (`"default"`) — preserving the v0.2 single-kernel
    /// behaviour while exercising the per-op dispatch routing seam wired in
    /// step-8.
    ///
    /// Tests that exercise the "no kernel for op" path (`dispatch` returns
    /// `None`) construct their own minimal descriptor inline instead.
    fn dispatch_test_descriptor_all_brep() -> CapabilityDescriptor {
        CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::PrimitiveCylinder, ReprKind::BRep),
                (Operation::PrimitiveSphere, ReprKind::BRep),
                (Operation::PrimitiveTube, ReprKind::BRep),
                (Operation::PrimitiveCone, ReprKind::BRep),
                (Operation::PrimitiveWedge, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
                (Operation::BooleanDifference, ReprKind::BRep),
                (Operation::BooleanIntersection, ReprKind::BRep),
                (Operation::ModifyFillet, ReprKind::BRep),
                (Operation::ModifyChamfer, ReprKind::BRep),
                (Operation::ModifyShell, ReprKind::BRep),
                (Operation::ModifyDraft, ReprKind::BRep),
                (Operation::ModifyThicken, ReprKind::BRep),
                (Operation::ModifyOffsetCurve, ReprKind::BRep),
                (Operation::ModifyZoneSlab, ReprKind::BRep),
                (Operation::ModifyOffsetSolid, ReprKind::BRep),
                (Operation::TransformTranslate, ReprKind::BRep),
                (Operation::TransformRotate, ReprKind::BRep),
                (Operation::TransformScale, ReprKind::BRep),
                (Operation::TransformRotateAround, ReprKind::BRep),
                (Operation::TransformApplyTransform, ReprKind::BRep),
                (Operation::PatternLinear, ReprKind::BRep),
                (Operation::PatternCircular, ReprKind::BRep),
                (Operation::PatternMirror, ReprKind::BRep),
                (Operation::PatternLinear2D, ReprKind::BRep),
                (Operation::PatternArbitrary, ReprKind::BRep),
                (Operation::SweepLoft, ReprKind::BRep),
                (Operation::SweepExtrude, ReprKind::BRep),
                (Operation::SweepRevolve, ReprKind::BRep),
                (Operation::SweepSweep, ReprKind::BRep),
                (Operation::SweepExtrudeSymmetric, ReprKind::BRep),
                (Operation::SweepSweepGuided, ReprKind::BRep),
                (Operation::SweepLoftGuided, ReprKind::BRep),
                (Operation::SweepPipe, ReprKind::BRep),
                (Operation::CurveLineSegment, ReprKind::BRep),
                (Operation::CurveArc, ReprKind::BRep),
                (Operation::CurveHelix, ReprKind::BRep),
                (Operation::CurveInterpCurve, ReprKind::BRep),
                (Operation::CurveBezierCurve, ReprKind::BRep),
                (Operation::CurveNurbsCurve, ReprKind::BRep),
            ],
        }
    }

    /// Wrap a single boxed [`GeometryKernel`] into a multi-handle kernel map
    /// keyed by `"default"`. Returns the map ready to pass as
    /// `&mut kernels` to [`Engine::execute_realization_ops`]. Mirrors what
    /// `with_prelude`/`new` do for the production builders (synthetic default
    /// name) while keeping per-test setup terse.
    fn dispatch_test_kernels(
        kernel: Box<dyn GeometryKernel>,
    ) -> BTreeMap<String, Box<dyn GeometryKernel>> {
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert("default".to_string(), kernel);
        kernels
    }

    /// Build the "single-default" borrowed registry view used by most
    /// `execute_realization_ops_*` unit tests. The descriptor must outlive the
    /// returned map because the `&CapabilityDescriptor` value borrows from it;
    /// callers typically use the pattern
    /// `let desc = dispatch_test_descriptor_all_brep(); let registry =
    /// dispatch_test_single_default_registry(&desc);`.
    fn dispatch_test_single_default_registry(
        descriptor: &CapabilityDescriptor,
    ) -> BTreeMap<String, &CapabilityDescriptor> {
        let mut r: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        r.insert("default".to_string(), descriptor);
        r
    }

    /// Per-test mutable state for the `execute_realization_ops_*` unit tests
    /// (amendment to task ε / 3436 — addresses reviewer suggestion #1).
    ///
    /// Owns the bag of `&mut`-borrowed scratch storage that
    /// [`Engine::execute_realization_ops`] writes into — step handles,
    /// diagnostics, named-steps map, attribute tables, kernel-error channel,
    /// realization cache, dispatch counter, and the produced-repr out-param.
    /// Constructed via [`Default::default`] and inspected via public fields
    /// after [`Self::run`] returns.
    ///
    /// Tests with pre-seeded `step_handles` (the rollback-truncation tests)
    /// push directly into `state.step_handles` before the call. Tests that
    /// drive multiple sequential realizations against the same state (the
    /// `_shadows_previous` / `_failed_shadow_…` tests) call
    /// [`Self::reset_attribute_tables`] between calls, mirroring the per-build
    /// reset in production.
    ///
    /// A future signature change to `Engine::execute_realization_ops` updates
    /// [`Self::run`] alone instead of every per-test call site.
    struct DispatchTestState {
        step_handles: Vec<KernelHandle>,
        diagnostics: Vec<Diagnostic>,
        named_steps: HashMap<String, KernelHandle>,
        feature_tag_table: FeatureTagTable,
        topology_attribute_table: TopologyAttributeTable,
        swept_kind_table: SweptKindTable,
        kernel_error_out: Option<ErrorRef>,
        realization_cache: RealizationCache<KernelHandle>,
        dispatch_count: usize,
        produced_repr_out: Option<ReprKind>,
    }

    // Hand-written `Default` instead of `#[derive(Default)]`: the inner
    // `RealizationCache<KernelHandle>` does not satisfy the derive bound
    // (`V: Default`) — `KernelHandle` pairs a `KernelId` with a `NewType(u64)`
    // and has no `Default` impl — but `RealizationCache::new()` constructs an empty cache
    // without that bound. Mirrors how production code initialises the field
    // (engine_admin.rs `Engine::with_prelude_and_kernels`).
    impl Default for DispatchTestState {
        fn default() -> Self {
            Self {
                step_handles: Vec::new(),
                diagnostics: Vec::new(),
                named_steps: HashMap::new(),
                feature_tag_table: FeatureTagTable::default(),
                topology_attribute_table: TopologyAttributeTable::default(),
                swept_kind_table: SweptKindTable::default(),
                kernel_error_out: None,
                realization_cache: RealizationCache::new(),
                dispatch_count: 0,
                produced_repr_out: None,
            }
        }
    }

    impl DispatchTestState {
        /// Reset the three per-realization attribute tables (mirrors the
        /// per-build reset in production at `build` / `build_snapshot` /
        /// `tessellate_*`). Called by the shadow tests between sequential
        /// realizations so the second call sees the same clean-table state the
        /// first did.
        fn reset_attribute_tables(&mut self) {
            self.feature_tag_table = FeatureTagTable::default();
            self.topology_attribute_table = TopologyAttributeTable::default();
            self.swept_kind_table = SweptKindTable::default();
        }

        /// Drive [`Engine::execute_realization_ops`] against this state with
        /// the canonical unit-test boilerplate — empty `ValueMap` /
        /// `functions` / `meta_map`, the canonical `TestEntity` realization
        /// id, and `demanded_tol = None` (the cache short-circuit is exercised
        /// from the integration tests in `tests/multi_handle_engine_dispatch.rs`,
        /// not from this unit-test surface).
        ///
        /// A future signature change to `execute_realization_ops` updates
        /// this method alone instead of every per-test call site (~14
        /// mechanical edits).
        fn run(
            &mut self,
            kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
            registry: &BTreeMap<String, &CapabilityDescriptor>,
            default_kernel: &str,
            ops: &[reify_compiler::CompiledGeometryOp],
            realization_name: Option<&str>,
            realization_span: SourceSpan,
            // Task #3443: pragma preference forwarded to `execute_realization_ops`.
            // Existing pragma-agnostic tests pass `None`; the S3 pragma steering
            // test supplies `Some("occt")`.
            prefer_kernel: Option<&str>,
        ) {
            let values = ValueMap::new();
            let functions: Vec<CompiledFunction> = vec![];
            let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
            let test_realization_id = RealizationNodeId::new("TestEntity", 0);
            Engine::execute_realization_ops(
                kernels,
                registry,
                default_kernel,
                ops,
                &[],
                &values,
                &functions,
                &meta_map,
                RealizationOutputs::new(
                    &mut self.step_handles,
                    &mut self.named_steps,
                    &mut self.feature_tag_table,
                    &mut self.topology_attribute_table,
                    &mut self.swept_kind_table,
                    &mut self.produced_repr_out,
                ),
                &mut self.diagnostics,
                &test_realization_id,
                realization_name,
                realization_span,
                &mut self.kernel_error_out,
                &mut self.realization_cache,
                None,
                // Task 4050 step-8: the existing single-kernel unit tests want
                // the v0.2 BRep demand; the cross-kernel tests use `run_demand`.
                ReprKind::BRep,
                &mut self.dispatch_count,
                prefer_kernel,
                // Test helpers operate on a single realization; it is always terminal.
                true,
            );
        }

        /// Like [`Self::run`] but threads a caller-controlled `demanded_repr`,
        /// `demanded_tol`, `realization_id`, and `realization_name` so the
        /// conversion-executor / cache-unpin tests (task 4050 steps 7/9/11/13)
        /// can drive a `Mesh` demand, name a realization for caching, and reuse
        /// `self`'s shared `realization_cache` / `dispatch_count` across
        /// sequential calls. `run` hard-codes `demanded_tol = None` /
        /// `demanded_repr = BRep` / `TestEntity`, which the v0.2 single-kernel
        /// tests want; the cross-kernel tests need all four under their own
        /// control.
        #[allow(clippy::too_many_arguments)]
        fn run_demand(
            &mut self,
            kernels: &mut BTreeMap<String, Box<dyn GeometryKernel>>,
            registry: &BTreeMap<String, &CapabilityDescriptor>,
            default_kernel: &str,
            ops: &[reify_compiler::CompiledGeometryOp],
            realization_id: &RealizationNodeId,
            realization_name: Option<&str>,
            realization_span: SourceSpan,
            demanded_repr: ReprKind,
            demanded_tol: Option<f64>,
            // Task #3443: pragma preference forwarded to `execute_realization_ops`.
            // Existing pragma-agnostic tests pass `None`.
            prefer_kernel: Option<&str>,
        ) {
            let values = ValueMap::new();
            let functions: Vec<CompiledFunction> = vec![];
            let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
            Engine::execute_realization_ops(
                kernels,
                registry,
                default_kernel,
                ops,
                &[],
                &values,
                &functions,
                &meta_map,
                RealizationOutputs::new(
                    &mut self.step_handles,
                    &mut self.named_steps,
                    &mut self.feature_tag_table,
                    &mut self.topology_attribute_table,
                    &mut self.swept_kind_table,
                    &mut self.produced_repr_out,
                ),
                &mut self.diagnostics,
                realization_id,
                realization_name,
                realization_span,
                &mut self.kernel_error_out,
                &mut self.realization_cache,
                demanded_tol,
                demanded_repr,
                &mut self.dispatch_count,
                prefer_kernel,
                // Test helpers operate on a single realization; it is always terminal.
                true,
            );
        }
    }

    // ── execute_realization_ops unit tests ────────────────────────────────────

    /// Happy path: all operations compile and execute successfully.
    /// Appends exactly one handle and emits no diagnostics.
    #[test]
    fn execute_realization_ops_happy_path_appends_handle() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert_eq!(state.step_handles.len(), 1, "expected one handle appended");
        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count so unrelated warning regressions still
        // fail the test instead of being silently absorbed by the
        // error-severity filter above. Per primitive op that succeeds at the
        // kernel level, the seeder makes exactly one warn-and-continue
        // attempt (extract_faces fails first on this mock kernel because
        // no topology fixture is configured). One Box op → 1 seeder warning.
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 warning (seeder extract_faces failure on mock kernel), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings[0]
                .message
                .contains("topology-attribute seeding failed"),
            "the single warning must be the seeder's auxiliary-metadata failure, got: {:?}",
            warnings[0].message
        );
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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        // Pre-seed with a sentinel so we can assert truncation went back to exactly
        // this pre-call length, distinguishing "INVALID pushed then truncated" from
        // "INVALID never pushed at all".
        let pre_existing = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xCAFE),
        };
        let mut state = DispatchTestState::default();
        state.step_handles.push(pre_existing);
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles should be truncated back to pre-call length of 1; \
             the INVALID sentinel must not remain"
        );
        assert_eq!(
            state.step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        let compile_failures = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, state.diagnostics
        );
    }

    /// Kernel error: ops compile successfully but `kernel.execute()` returns `Err`.
    /// Truncates `step_handles` to `handle_start` and emits exactly 1 geometry-error
    /// diagnostic.
    #[test]
    fn execute_realization_ops_kernel_error_truncates_handles() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::FailingMockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(FailingMockGeometryKernel));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        assert!(
            state.step_handles.is_empty(),
            "handles should be truncated back to handle_start (0)"
        );
        let geometry_errors = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("geometry error"))
            .count();
        assert_eq!(
            geometry_errors, 1,
            "expected exactly 1 geometry-error diagnostic, got {}: {:?}",
            geometry_errors, state.diagnostics
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
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

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

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        // Pre-seed step_handles with a sentinel to verify truncation goes back
        // to exactly this pre-call length, not to zero.
        let pre_existing = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xBEEF),
        };
        let mut state = DispatchTestState::default();
        state.step_handles.push(pre_existing);
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // The real handle produced by op 0 must have been discarded.
        // Only the pre-existing handle should remain.
        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles should be truncated back to the pre-call length of 1; \
             the real handle from op 0 must be gone"
        );
        assert_eq!(
            state.step_handles[0], pre_existing,
            "the pre-existing handle must be preserved unchanged"
        );
        // Exactly one compile-error diagnostic from the failing op 1
        let compile_failures = state
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .count();
        assert_eq!(
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, state.diagnostics
        );
    }

    /// Richer error propagation: the compile-failure Error diagnostic must include
    /// the specific reason from `compile_geometry_op`'s `Err(reason)`, not just the
    /// generic prefix.  Uses a Boolean op whose GeomRef::Step(99) is out-of-bounds
    /// so the reason string contains "unresolvable" / "Step" / "99".
    ///
    /// This test drives step-4: it fails until `execute_realization_ops` appends
    /// the `err` string to the diagnostic message.
    #[test]
    fn execute_realization_ops_compile_failure_diagnostic_includes_specific_reason() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty →
        // compile_geometry_op returns Err("unresolvable GeomRef::Step(99) …")
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // The Error diagnostic must contain the standard prefix (preserves
        // existing integration-test substring checks) AND the specific reason.
        let compile_err_diag = state
            .diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, reify_core::Severity::Error)
            })
            .expect("expected an Error diagnostic with 'failed to compile geometry operation'");

        assert!(
            compile_err_diag.message.contains("unresolvable")
                || compile_err_diag.message.contains("Step")
                || compile_err_diag.message.contains("99"),
            "Error diagnostic should include the specific reason (unresolvable / Step / 99), \
             got: {:?}",
            compile_err_diag.message
        );
    }

    // ── named_steps plumbing tests (step-7) ───────────────────────────────────

    /// Happy-path naming: a successful named realization populates `named_steps`
    /// with the kernel-returned handle after execution completes.
    ///
    /// Fails to compile until step-8 adds `named_steps: &mut HashMap<String,
    /// GeometryHandleId>` and `realization_name: Option<&str>` to
    /// `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_named_realization_populates_named_steps() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );

        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder extract-failure per
        // successful primitive op). See the happy-path test for the rationale.
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly 1 warning (seeder extract_faces failure on mock kernel), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings[0]
                .message
                .contains("topology-attribute seeding failed"),
            "the single warning must be the seeder's auxiliary-metadata failure, got: {:?}",
            warnings[0].message
        );
        assert_eq!(state.step_handles.len(), 1, "expected one handle appended");
        let body_handle = state.named_steps.get("body").copied();
        assert!(
            body_handle.is_some(),
            "named_steps should contain 'body' after successful named realization"
        );
        assert_eq!(
            body_handle.unwrap(),
            state.step_handles[0],
            "named_steps['body'] should equal the handle returned by the kernel"
        );
    }

    /// Rollback-must-not-leak: a named realization that fails (Boolean op with
    /// out-of-bounds GeomRef::Step triggers compile failure + rollback) must NOT
    /// leave any entry in `named_steps` — stale entries would let later
    /// realizations resolve a name that never actually produced valid geometry.
    ///
    /// Fails to compile until step-8 adds the `named_steps` parameter.
    #[test]
    fn execute_realization_ops_rollback_does_not_leak_into_named_steps() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_test_support::mocks::MockGeometryKernel;

        // A realization named "bad" whose only op is an OOB Boolean → compile
        // failure → rollback path; named_steps must not contain "bad" afterwards.
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            Some("bad"),
            SourceSpan::new(0, 0),
            None,
        );

        assert!(
            !state.named_steps.contains_key("bad"),
            "named_steps must NOT contain 'bad' after rollback; stale entries \
             would let later realizations resolve a name whose geometry was never \
             successfully produced"
        );
        // Verify rollback did happen (existing invariant)
        assert!(
            state.step_handles.is_empty(),
            "handles should be truncated on failure"
        );
    }

    /// Pins the last-write-wins (shadowing) semantics for `named_steps` when
    /// two sibling realizations share the same `realization_name`.  Reify's
    /// source syntax permits two sibling `let body = …` geometry bindings
    /// inside a structure with no compile error (`CompilationScope::register`
    /// uses plain `HashMap::insert` without a duplicate-name check).  When
    /// that happens, `execute_realization_ops` must overwrite the earlier
    /// entry so that `named_steps["body"]` resolves to the most-recent
    /// successful binding.  A regression flipping `HashMap::insert` to
    /// `entry().or_insert(…)` (first-write-wins) must fail this test.
    #[test]
    fn execute_realization_ops_duplicate_name_shadows_previous() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let box_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];
        let cyl_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Cylinder,
            args: vec![
                ("radius".into(), mm_lit(5.0)),
                ("height".into(), mm_lit(20.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();

        // First binding: let body = box(…)
        state.run(
            &mut kernels,
            &registry,
            "default",
            &box_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        // Snapshot via the contract-visible map entry, not by positional index,
        // so the snapshot stays correct if internal handle-slot layout changes.
        let h1 = state.named_steps["body"];

        // Second binding: let body = cylinder(…) — same name, different primitive.
        // Reset the attribute tables between calls to mirror the per-build
        // reset in production (each realization sees clean attribute state).
        state.reset_attribute_tables();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &cyl_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        let h2 = state.named_steps["body"];

        // The kernel must have issued distinct handles so the test is non-trivial
        assert_ne!(
            h1, h2,
            "MockGeometryKernel must return distinct handles for distinct ops"
        );

        // Last-write-wins: named_steps["body"] must equal h2 (the cylinder binding)
        assert_eq!(
            state.named_steps.get("body").copied(),
            Some(h2),
            "shadowing contract: the second `let body` binding must overwrite \
             the first — named_steps[\"body\"] must be the handle from the \
             most-recent successful realization"
        );

        // Explicit anti-assertion: a first-write-wins regression must fail here
        assert_ne!(
            state.named_steps.get("body").copied(),
            Some(h1),
            "first-write-wins regression guard: named_steps[\"body\"] must NOT \
             resolve to the first binding's handle after the second binding has \
             shadowed it"
        );

        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "no errors expected for two valid realizations, got: {:?}",
            errors
        );
        // Pin the expected warning count: this test runs two successful
        // primitive ops (Box, then Cylinder) through the same `diagnostics`
        // Vec, so one seeder warning per op accumulates → 2 total.
        let warnings: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
            .collect();
        assert_eq!(
            warnings.len(),
            2,
            "expected exactly 2 warnings (one seeder failure per successful primitive op), \
             got {}: {:?}",
            warnings.len(),
            warnings
        );
        assert!(
            warnings
                .iter()
                .all(|w| w.message.contains("topology-attribute seeding failed")),
            "every warning must be a seeder auxiliary-metadata failure, got: {:?}",
            warnings
        );
    }

    /// Pins the rollback-vs-shadowing interaction: when a named realization
    /// fails (compile error → rollback path), the function must NOT overwrite
    /// a prior successful binding for the same name in `named_steps`.  This
    /// covers the intersection between the shadowing semantics tested above and
    /// the rollback invariant tested in
    /// `execute_realization_ops_rollback_does_not_leak_into_named_steps`.
    ///
    /// If the guard inside `execute_realization_ops` (the `else if` branch that
    /// only inserts into `named_steps` after a fully successful realization)
    /// were removed, a failed second binding would silently clear or overwrite
    /// the first successful one, causing later `GeomRef::Sub("body")` lookups
    /// to fail or resolve to invalid geometry.
    #[test]
    fn execute_realization_ops_failed_shadow_does_not_overwrite_previous() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let box_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];
        // A realization that will fail to compile: OOB step reference forces the
        // compile-error path → had_failure = true → rollback.
        let fail_ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let mut state = DispatchTestState::default();

        // First binding: let body = box(…) — succeeds, populates named_steps.
        state.run(
            &mut kernels,
            &registry,
            "default",
            &box_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );
        let h1 = state.named_steps["body"];
        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "first realization must succeed cleanly, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder failure for the
        // successful Box op). See the happy-path test for the rationale.
        let warnings_after_first: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
            .collect();
        assert_eq!(
            warnings_after_first.len(),
            1,
            "first realization should emit exactly 1 seeder warning, \
             got {}: {:?}",
            warnings_after_first.len(),
            warnings_after_first
        );

        // Second binding: let body = <invalid> — fails (rollback path).
        // Reset attribute tables between realizations.
        state.reset_attribute_tables();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &fail_ops,
            Some("body"),
            SourceSpan::new(0, 0),
            None,
        );

        // The failed shadow must NOT have overwritten the successful binding.
        assert_eq!(
            state.named_steps.get("body").copied(),
            Some(h1),
            "rollback guard: a failed shadow must not overwrite the previous \
             successful binding — named_steps[\"body\"] must still resolve to h1"
        );

        // The second call must have emitted a diagnostic (compile failure).
        assert!(
            !state.diagnostics.is_empty(),
            "expected a diagnostic from the failed second realization"
        );
        // Pin the warning count after the second call: the second op fails
        // before reaching `kernel.execute`, so the seeder is never invoked
        // and no NEW warning lands on top of the one from the first call.
        let warnings_after_second: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Warning))
            .collect();
        assert_eq!(
            warnings_after_second.len(),
            1,
            "after the failing second realization the warning count must remain \
             at 1 (only the first realization's seeder warning); the failing op \
             never reaches the seeder. Got {}: {:?}",
            warnings_after_second.len(),
            warnings_after_second
        );
    }

    // ── span-label threading tests ─────────────────────────────────────────────

    /// Pins that the compile-failure Error diagnostic emitted by
    /// `execute_realization_ops` carries a `DiagnosticLabel` whose span
    /// equals the supplied `realization_span`.
    ///
    /// Uses an OOB `GeomRef::Step(99)` to force the compile-failure path
    /// (same trigger as `execute_realization_ops_compile_failure_diagnostic_includes_specific_reason`).
    /// Passes a distinct non-zero span `SourceSpan::new(100, 150)` so the
    /// assertion cannot collide with a sentinel value.
    ///
    /// This test fails to compile until step-6 adds the `realization_span:
    /// SourceSpan` parameter to `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_compile_failure_diagnostic_has_realization_span_label() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_core::{Severity, SourceSpan};
        use reify_test_support::mocks::MockGeometryKernel;

        // Step(99) is out-of-bounds when step_handles is empty →
        // compile_geometry_op returns Err("unresolvable GeomRef::Step(99) …")
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let realization_span = SourceSpan::new(100, 150);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            realization_span,
            None,
        );

        // Find the compile-failure Error diagnostic.
        let compile_err_diag = state
            .diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, Severity::Error)
            })
            .expect("expected an Error diagnostic with 'failed to compile geometry operation'");

        assert_eq!(
            compile_err_diag.labels.len(),
            1,
            "compile-failure diagnostic should carry exactly 1 DiagnosticLabel, \
             got {}: {:?}",
            compile_err_diag.labels.len(),
            compile_err_diag.labels
        );
        assert_eq!(
            compile_err_diag.labels[0].span, realization_span,
            "compile-failure label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span, compile_err_diag.labels[0].span
        );
    }

    /// Pins that the kernel-error Error diagnostic emitted by
    /// `execute_realization_ops` carries a `DiagnosticLabel` whose span
    /// equals the supplied `realization_span`.
    ///
    /// Uses `FailingMockGeometryKernel` (ops compile but kernel.execute returns Err)
    /// so we exercise the kernel-error path.  Passes a distinct non-zero span
    /// `SourceSpan::new(200, 250)`.
    ///
    /// After step-6, this test FAILS because step-6 only attaches the label to
    /// the compile-failure path.  Step-8 will attach it to the kernel-error path
    /// and make this test pass.
    #[test]
    fn execute_realization_ops_kernel_error_diagnostic_has_realization_span_label() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{Severity, SourceSpan, Type};
        use reify_ir::CompiledExpr;
        use reify_test_support::mocks::FailingMockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut kernels = dispatch_test_kernels(Box::new(FailingMockGeometryKernel));
        let desc = dispatch_test_descriptor_all_brep();
        let registry = dispatch_test_single_default_registry(&desc);
        let realization_span = SourceSpan::new(200, 250);
        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            realization_span,
            None,
        );

        // Find the kernel-error Error diagnostic.
        let kernel_err_diag = state
            .diagnostics
            .iter()
            .find(|d| d.message.contains("geometry error") && matches!(d.severity, Severity::Error))
            .expect("expected an Error diagnostic with 'geometry error'");

        assert_eq!(
            kernel_err_diag.labels.len(),
            1,
            "kernel-error diagnostic should carry exactly 1 DiagnosticLabel, \
             got {}: {:?}",
            kernel_err_diag.labels.len(),
            kernel_err_diag.labels
        );
        assert_eq!(
            kernel_err_diag.labels[0].span, realization_span,
            "kernel-error label span should equal the supplied realization_span \
             {:?}, got {:?}",
            realization_span, kernel_err_diag.labels[0].span
        );
    }

    // ── per-op dispatch routing tests (step-7 #3436) ──────────────────────────
    //
    // These tests drive the multi-handle reshape of `execute_realization_ops`
    // landing in step-8: instead of a single `&mut dyn GeometryKernel`, the
    // helper takes a `&mut BTreeMap<String, Box<dyn GeometryKernel>>` keyed on
    // kernel name, a borrowed `&BTreeMap<String, &CapabilityDescriptor>`
    // dispatch registry, and a `&str` default-kernel name. For each op the
    // helper calls `dispatcher::dispatch(registry, op, BRep, {BRep})`, routes
    // the op to `kernels[plan.kernel]` (falling back to the default name when
    // the plan's kernel is absent from the map), or emits a `NoKernelChain`
    // diagnostic + sets `kernel_error_out` when dispatch returns `None`.

    /// Recording kernel: delegates the full `GeometryKernel` surface to a
    /// `MockGeometryKernel` and additionally pushes its own `name` onto a
    /// shared `Arc<Mutex<Vec<String>>>` on every `execute` /
    /// `execute_with_history` call. Lets the routing tests assert *which*
    /// kernel in the map received the op call — proof that per-op dispatch
    /// indexed into the named entry rather than the default.
    struct NamedRecordingKernel {
        name: String,
        inner: reify_test_support::mocks::MockGeometryKernel,
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl reify_ir::GeometryKernel for NamedRecordingKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.log.lock().unwrap().push(self.name.clone());
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }
    }

    /// Two BRep kernels — `"aaa"` (lex-min) and `"default"` — both supporting
    /// `(PrimitiveBox, BRep)`. `dispatch(registry, PrimitiveBox, BRep, {BRep})`
    /// must pick `"aaa"` by lex-min tie-break (BTreeMap iteration order). The
    /// recording kernel under `"aaa"` captures the `execute` call, proving the
    /// op was routed to the dispatcher-named kernel — NOT the default.
    ///
    /// RED before step-8: `execute_realization_ops` still has the
    /// single-kernel `&mut dyn GeometryKernel` first parameter, so this test
    /// fails to compile until step-8 reshapes the signature to take
    /// `&mut BTreeMap<String, Box<dyn GeometryKernel>>` +
    /// `&BTreeMap<String, &CapabilityDescriptor>` + `&str` default name.
    #[test]
    fn execute_realization_ops_routes_to_dispatcher_picked_kernel() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "aaa".to_string(),
            Box::new(NamedRecordingKernel {
                name: "aaa".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "default".to_string(),
            Box::new(NamedRecordingKernel {
                name: "default".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let desc_a = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("aaa".to_string(), &desc_a);
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["aaa".to_string()],
            "the op must be routed to the dispatcher-picked kernel (lex-min = \"aaa\"), \
             not the default — got call log {:?}",
            calls
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "expected one handle pushed from the dispatched kernel"
        );
    }

    /// Behavior-preserved: with only the default kernel in the map (and a
    /// registry naming it for the op), `execute_realization_ops` must run the
    /// op on the default kernel — exactly the v0.2 single-kernel path.
    ///
    /// RED before step-8: same signature change as
    /// `execute_realization_ops_routes_to_dispatcher_picked_kernel` above.
    #[test]
    fn execute_realization_ops_routes_to_default_when_only_default_registered() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "default".to_string(),
            Box::new(NamedRecordingKernel {
                name: "default".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["default".to_string()],
            "single-kernel-in-map: op must run on the default kernel; got log {:?}",
            calls,
        );
        assert_eq!(state.step_handles.len(), 1, "expected one handle pushed");
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "behavior-preserved single-default path must not emit error diagnostics; got {:?}",
            errors,
        );
    }

    // ── cross-kernel conversion executor tests (task 4050 step-7) ─────────────
    //
    // These drive the multi-stage conversion executor + the Mesh→BRep dispatch
    // fallback landing in step-8. RED before step-8: `run_demand` calls
    // `Engine::execute_realization_ops` with the not-yet-existing `demanded_repr`
    // parameter, so the whole `mod tests` build fails to compile until step-8
    // grows that parameter, wires the `dispatch(.., demanded_repr, ..).or_else(
    // BRep)` fallback, and replaces the `Some(_) =>` deferred-error arm with the
    // tessellate→ingest cross-kernel handoff.

    /// occt-like counting kernel: `execute` / `query` / `export` delegate to an
    /// inner [`MockGeometryKernel`] (so `PrimitiveBox` → BRep solid handles),
    /// and `tessellate` bumps a shared counter before returning a trivial
    /// single-triangle [`Mesh`] — the BRep→Mesh source projection the conversion
    /// executor drives for each prior-stage input handle.
    struct CountingTessellateKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        tessellate_count: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl reify_ir::GeometryKernel for CountingTessellateKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            *self.tessellate_count.lock().unwrap() += 1;
            Ok(reify_ir::Mesh {
                vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                indices: vec![0, 1, 2],
                normals: None,
            })
        }
    }

    /// manifold-like counting kernel: `ingest_mesh` bumps a shared counter and
    /// returns a fresh handle (the BRep→Mesh target projection), and `execute`
    /// bumps a shared counter (the final cross-kernel `BooleanUnion` op runs
    /// here). `query` / `export` / `tessellate` delegate to an inner
    /// [`MockGeometryKernel`]; only the union is ever routed here in the
    /// fixtures, so the `execute` counter is the `BooleanUnion`-on-Manifold
    /// count.
    struct CountingManifoldKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        execute_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for CountingManifoldKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.execute_count.lock().unwrap() += 1;
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.inner.tessellate(handle, tolerance)
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// openvdb-like counting kernel: `ingest_mesh` bumps a shared counter and
    /// returns a fresh handle (the Mesh→Voxel target projection via voxelising ingest),
    /// `execute` bumps a shared counter (the final cross-kernel `BooleanUnion` op runs
    /// here), and `tessellate` returns `Err(TessError::TessellationFailed)` mirroring the
    /// real OpenVDB stub — the real kernel cannot tessellate Voxel handles back to Mesh.
    /// `query` / `export` delegate to an inner [`MockGeometryKernel`].
    struct CountingVoxelizerKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        execute_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for CountingVoxelizerKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.execute_count.lock().unwrap() += 1;
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            // Mirrors the real OpenVDB stub: Voxel handles cannot be tessellated
            // back to Mesh via this kernel — the executor must NOT call this.
            Err(reify_ir::TessError::TessellationFailed(
                "openvdb stub: tessellate not supported".into(),
            ))
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// step-7(A) CONVERSION PATH (RED). With `demanded_repr = Mesh`, the
    /// dispatcher routes the terminal `BooleanUnion` to the Mesh-capable
    /// `"manifold"` kernel, preceded by a single BRep→Mesh conversion stage
    /// carried by `"occt"`. The executor must, for each of the union's two BRep
    /// input handles, `occt.tessellate` → Mesh then `manifold.ingest_mesh` →
    /// handle, substitute the converted handles, and run the union on
    /// `"manifold"`. Asserts the per-kernel call counts (2 / 2 / 1), the
    /// terminal `KernelId::Manifold` handle, and `produced_repr == Mesh`.
    #[test]
    fn execute_realization_ops_conversion_path_tessellates_and_ingests_cross_kernel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Shared call counters, read back after the call via the Arc clones.
        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh); manifold:
        // (BooleanUnion, Mesh). For demanded = Mesh / available = {BRep} the
        // dispatcher yields plan { kernel: "manifold", conversions:
        // [(Occt, BRep, Mesh)] } for the union.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            None,
            None,
        );

        // The cross-kernel handoff must succeed: no error diagnostics, no
        // kernel_error_out.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "cross-kernel conversion must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "cross-kernel conversion must leave kernel_error_out None, got {:?}",
            state.kernel_error_out
        );

        // (a) occt.tessellate fires once per BooleanUnion input handle = 2.
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "occt.tessellate must be called once per union input handle (2)"
        );
        // (b) manifold.ingest_mesh fires once per converted input = 2.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "manifold.ingest_mesh must be called once per converted input (2)"
        );
        // (c) manifold runs the final BooleanUnion exactly once.
        assert_eq!(
            *union_count.lock().unwrap(),
            1,
            "manifold must run the final BooleanUnion exactly once"
        );

        // The terminal pushed handle is a Manifold handle (plan.kernel).
        let terminal = state
            .step_handles
            .last()
            .expect("a terminal handle must be pushed on success");
        assert_eq!(
            terminal.kernel,
            KernelId::Manifold,
            "terminal handle must be tagged KernelId::Manifold, got {:?}",
            terminal.kernel
        );

        // produced_repr surfaced as Mesh (plan_output_repr of the union on
        // manifold).
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "produced_repr_out must be Mesh for the cross-kernel realization"
        );
    }

    /// step-3 TWO-STAGE BRep→Voxel EXECUTOR (RED). With `demanded_repr = Voxel`
    /// and kernels `"occt"` (CountingTessellateKernel) + `"openvdb"`
    /// (CountingVoxelizerKernel), the two-stage chain
    /// `[(occt,BRep,Mesh),(openvdb,Mesh,Voxel)]` must run EXACTLY ONCE per
    /// op-input parent: `occt.tessellate` × 2 → Mesh, `openvdb.ingest_mesh`
    /// × 2 → Voxel handle; then the union runs on `"openvdb"` once.
    ///
    /// RED: the current per-stage executor re-processes stage-2
    /// `(openvdb,Mesh,Voxel)` by calling `openvdb.tessellate(brep_pid)` →
    /// `TessError::TessellationFailed` → conversion-error diagnostic, so the
    /// no-error / ingest==2 / terminal assertions fail.
    #[test]
    fn execute_realization_ops_conversion_path_two_stage_brep_to_voxel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Shared call counters, read back after the call via the Arc clones.
        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "openvdb".to_string(),
            Box::new(CountingVoxelizerKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 2000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Mesh);
        // openvdb: (BooleanUnion, Voxel) + (Convert{Mesh}, Voxel).
        // For demanded = Voxel / available = {BRep} the dispatcher yields plan:
        // { kernel: "openvdb", conversions: [(Occt,BRep,Mesh),(OpenVdb,Mesh,Voxel)] }.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![
                (Operation::BooleanUnion, ReprKind::Voxel),
                (
                    Operation::Convert {
                        from: ReprKind::Mesh,
                    },
                    ReprKind::Voxel,
                ),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("MyDesign", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Voxel,
            None,
            None,
        );

        // The two-stage conversion must succeed: no error diagnostics, no
        // kernel_error_out.
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "two-stage BRep→Voxel conversion must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "two-stage BRep→Voxel conversion must leave kernel_error_out None, got {:?}",
            state.kernel_error_out
        );

        // (a) occt.tessellate fires once per BooleanUnion input handle = 2.
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "occt.tessellate must be called once per union input handle (2)"
        );
        // (b) openvdb.ingest_mesh fires once per converted input = 2.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "openvdb.ingest_mesh must be called once per converted input (2)"
        );
        // (c) openvdb runs the final BooleanUnion exactly once.
        assert_eq!(
            *union_count.lock().unwrap(),
            1,
            "openvdb must run the final BooleanUnion exactly once"
        );

        // The terminal pushed handle is an OpenVdb handle (plan.kernel).
        let terminal = state
            .step_handles
            .last()
            .expect("a terminal handle must be pushed on success");
        assert_eq!(
            terminal.kernel,
            KernelId::OpenVdb,
            "terminal handle must be tagged KernelId::OpenVdb, got {:?}",
            terminal.kernel
        );

        // produced_repr surfaced as Voxel (plan_output_repr of the union on openvdb).
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Voxel),
            "produced_repr_out must be Voxel for the two-stage BRep→Voxel realization"
        );
    }

    /// Amendment (suggestion 4): NEGATIVE — unsupported conversion crossing
    /// degrades gracefully (no kernel work performed).
    ///
    /// Exercises the Phase-1 validation gate: when the dispatcher produces a
    /// chain containing a crossing that `v03_conversion_projection` classifies
    /// as `None` (e.g. a direct `BRep→Voxel` stage, which is not one of the
    /// two supported crossings), the executor must emit exactly one
    /// `Error`-severity diagnostic and must perform zero kernel work —
    /// `ingest_mesh` and `execute` (for the final op) must never be called.
    ///
    /// Scenario: "occt" registers `(Convert{from:BRep}, Voxel)` — a
    /// single-step BRep→Voxel crossing.  The dispatcher BFS finds a plan
    /// `{kernel:"openvdb", conversions:[(Occt,BRep,Voxel)]}`.  Phase 1 calls
    /// `v03_conversion_projection(BRep, Voxel)` → `None` → `conversion_error`.
    #[test]
    fn execute_realization_ops_conversion_path_unsupported_crossing_degrades_gracefully() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        // "occt": produces BRep primitives and claims a direct BRep→Voxel
        // Convert edge (unsupported crossing in v0.3-β).
        kernels.insert("occt".to_string(), Box::new(MockGeometryKernel::new()));
        // "openvdb": counts ingest_mesh + execute calls so the test can assert
        // they never fire on the error path.
        kernels.insert(
            "openvdb".to_string(),
            Box::new(CountingVoxelizerKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 4000,
            }),
        );

        // occt: (PrimitiveBox, BRep) + (Convert{BRep}, Voxel) — the direct
        // BRep→Voxel crossing is not one of the two β-supported shapes.
        // openvdb: (BooleanUnion, Voxel) only — no Convert capability.
        //
        // Dispatcher BFS for demanded=Voxel / available={BRep}:
        //   pop(BRep): expand via occt's (Convert{BRep},Voxel) → (Voxel,[occt,BRep,Voxel])
        //   pop(Voxel): openvdb supports (BooleanUnion,Voxel) → plan found.
        // Phase 1: v03_conversion_projection(BRep,Voxel) = None → error.
        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Voxel,
                ),
            ],
        };
        let desc_openvdb = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Voxel)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("openvdb".to_string(), &desc_openvdb);

        // Two BRep primitives + one BooleanUnion consuming them.
        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("BadConv", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("BadConv"),
            SourceSpan::new(0, 0),
            ReprKind::Voxel,
            None,
            None,
        );

        // Must emit at least one Error diagnostic (the unsupported crossing).
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            !errors.is_empty(),
            "an unsupported BRep→Voxel crossing must emit an Error diagnostic, \
             got no errors (diagnostics: {:?})",
            state.diagnostics,
        );
        // Pin that the error originated from the Phase-1 classification gate
        // (v03_conversion_projection(BRep,Voxel) = None), not from a None
        // dispatch plan or some other unrelated code path.  The gate message
        // always contains "not executable in v0.3-β"; if the error comes from
        // elsewhere (e.g. dispatch returns None / NoKernelChain path) the test
        // would still pass the non-empty check above, but for the wrong reason.
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("not executable in v0.3-\u{03b2}")),
            "the Error diagnostic must originate from the Phase-1 classification \
             gate (message must contain 'not executable in v0.3-β'); \
             got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>(),
        );

        // No kernel work must have been performed after the Phase-1 error.
        assert_eq!(
            *ingest_count.lock().unwrap(),
            0,
            "ingest_mesh must not be called when the conversion stage is unsupported"
        );
        assert_eq!(
            *union_count.lock().unwrap(),
            0,
            "BooleanUnion must not run when the conversion stage is unsupported"
        );
    }

    /// step-7(B) FALLBACK CONTROL (RED) — pins design_decision 3. With
    /// `demanded_repr = Mesh` but a registry that has NO Mesh-capable kernel for
    /// the op (occt supports only `(PrimitiveBox, BRep)`), a lone PrimitiveBox
    /// realization must NOT error: the Mesh dispatch returns `None`, the
    /// executor falls back to a BRep dispatch, and the op runs on occt producing
    /// a BRep handle. Without the fallback this would hit the strict
    /// no-kernel-chain error arm and regress every Stl/Obj primitive export.
    #[test]
    fn execute_realization_ops_mesh_demand_falls_back_to_brep_when_no_mesh_kernel() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        // No Mesh-capable kernel for the op: a Mesh demand can't be satisfied and
        // must fall back to BRep rather than error.
        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("Lone", 0);
        let mut state = DispatchTestState::default();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Lone"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            None,
            None,
        );

        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "the Mesh→BRep fallback must not emit error diagnostics, got: {:?}",
            errors
        );
        assert!(
            state.kernel_error_out.is_none(),
            "the Mesh→BRep fallback must not set kernel_error_out, got {:?}",
            state.kernel_error_out
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "the fallback must produce exactly one BRep handle from occt"
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "the fallback realization's produced_repr must be BRep"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the fallback path must not tessellate (no conversion stage runs)"
        );
    }

    /// step-9 (RED): a NAMED Mesh-demanding conversion realization must cache
    /// its terminal handle at `(entity, Mesh, tol)` so a second identical build
    /// hits the cache short-circuit — `dispatch_count == 0`, the cached Manifold
    /// terminal handle returned, `produced_repr == Mesh`, and the occt/manifold
    /// call counters UNCHANGED (the whole realization short-circuits).
    ///
    /// RED before step-10: `cache_repr` is pinned to `ReprKind::BRep`, so the
    /// post-loop INSERT keys the genuinely-Mesh terminal at the BRep slot and
    /// the cache-hit short-circuit (which also keys on the pinned BRep) reports
    /// `produced_repr == BRep` for the second build instead of `Mesh`.
    #[test]
    fn execute_realization_ops_mesh_realization_caches_and_hits_at_mesh_key() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryKernel, KernelId, Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── First build: cold cache, full cross-kernel conversion. ──────────
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert!(
            state.dispatch_count > 0,
            "first (cold-cache) build must dispatch, got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "first build produced_repr"
        );
        let terminal_1 = *state
            .step_handles
            .last()
            .expect("first build must push a terminal handle");
        assert_eq!(terminal_1.kernel, KernelId::Manifold);
        let tess_after_1 = *tess_count.lock().unwrap();
        let ingest_after_1 = *ingest_count.lock().unwrap();
        let union_after_1 = *union_count.lock().unwrap();
        assert_eq!((tess_after_1, ingest_after_1, union_after_1), (2, 2, 1));

        // ── Reset the per-build instrumentation the way production does. ─────
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        // ── Second build: identical inputs, SAME cache → full short-circuit. ─
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second build must hit the cache short-circuit (no dispatch), got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::Mesh),
            "second build must report the Mesh terminal repr from the cache, not BRep"
        );
        let terminal_2 = *state
            .step_handles
            .last()
            .expect("second build must push the cached terminal handle");
        assert_eq!(
            terminal_2, terminal_1,
            "second build must return the cached Manifold terminal handle"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            tess_after_1,
            "tessellate must be untouched on the cache hit"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            ingest_after_1,
            "ingest_mesh must be untouched on the cache hit"
        );
        assert_eq!(
            *union_count.lock().unwrap(),
            union_after_1,
            "the boolean union must be untouched on the cache hit"
        );
    }

    /// step-9 control: a NAMED BRep-demanding realization still caches + hits at
    /// `(entity, BRep, tol)` and reports `produced_repr == BRep`. This is the
    /// backward-compat guard — it passes both before and after the step-10
    /// `cache_repr` unpin, so it pins that the BRep path is unaffected.
    #[test]
    fn execute_realization_ops_brep_realization_caches_and_hits_at_brep_key() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("Solid", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Solid"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );
        assert!(state.dispatch_count > 0, "first build must dispatch");
        assert_eq!(state.produced_repr_out, Some(ReprKind::BRep));
        let terminal_1 = *state.step_handles.last().expect("a terminal handle");

        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Solid"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second BRep build must hit the cache short-circuit"
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "second BRep build must report BRep from the cache"
        );
        assert_eq!(
            *state.step_handles.last().expect("a terminal handle"),
            terminal_1,
            "second BRep build must return the cached terminal handle"
        );
    }

    /// Amendment (reviewer_comprehensive #1, perf regression): a NAMED
    /// Mesh-demanding realization whose registry has NO Mesh-capable terminal
    /// kernel falls back to a BRep dispatch (design_decision 3), RESOLVES to
    /// BRep, and caches its terminal at `(entity, BRep, tol)`. A second
    /// identical Mesh-demanding build must STILL hit that cache — via the BRep
    /// fallback probe — so `dispatch_count == 0`, the cached BRep terminal
    /// handle is returned, `produced_repr == BRep`, and `tessellate` stays at 0.
    ///
    /// This pins the fix for the regression where the cache_repr unpin keyed the
    /// lookup at Mesh while the fell-back terminal was stored at BRep: without
    /// the BRep fallback probe the second build's Mesh lookup would miss the
    /// BRep entry and recompute the whole realization on every rebuild — the
    /// dominant occt-only Stl/Obj production export path.
    #[test]
    fn execute_realization_ops_mesh_demand_resolved_brep_hits_cache_via_brep_fallback() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, GeometryKernel, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );

        // No Mesh-capable kernel for the op: a Mesh demand resolves only via the
        // BRep fallback (design_decision 3) — the occt-only production config.
        let desc_occt = CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveBox, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let realization_id = RealizationNodeId::new("FellBack", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── First (cold) build: Mesh demand falls back to BRep, caches at BRep. ─
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("FellBack"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert!(state.dispatch_count > 0, "first build must dispatch");
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "a Mesh demand with no Mesh kernel must resolve BRep (fallback)"
        );
        let terminal_1 = *state.step_handles.last().expect("a terminal handle");
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the fallback path must not tessellate"
        );

        // ── Reset the per-build instrumentation the way production does. ────────
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();

        // ── Second build: SAME Mesh demand → BRep fallback probe must HIT. ──────
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("FellBack"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        assert_eq!(
            state.dispatch_count, 0,
            "second Mesh build must hit the cache via the BRep fallback probe, got {}",
            state.dispatch_count
        );
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "the fallback cache-hit must report the resolved BRep repr, not Mesh"
        );
        assert_eq!(
            *state.step_handles.last().expect("a terminal handle"),
            terminal_1,
            "the fallback cache-hit must return the cached BRep terminal handle"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            0,
            "the cache hit must not tessellate"
        );
    }

    /// step-11 (RED): intermediate caching + cross-realization reuse. After one
    /// successful Mesh-demanding conversion realization (the step-7(A) fixture),
    /// each BRep→Mesh intermediate produced by the conversion executor must be
    /// present in the [`RealizationCache`] at `(intermediate_entity, Mesh,
    /// per_stage_tol, NO_OPTIONS)`, where `intermediate_entity` is the
    /// per-input cache-key entity (`"{entity}#conv-step{idx}"` — the input's
    /// local step index makes it distinct-per-input AND stable across identical
    /// rebuilds) and `per_stage_tol = per_stage_tolerance_for_plan(&plan, tol)`
    /// for the single BRep→Mesh stage (`tol × 0.8`).
    ///
    /// A SECOND realization with the same entity + ops + tol but ANONYMOUS (no
    /// name, so the whole-realization terminal cache short-circuit cannot fire —
    /// it is gated on `realization_name.is_some()`) must reach the conversion
    /// executor again and REUSE both cached intermediates: occt.tessellate and
    /// manifold.ingest_mesh stay at the first realization's counts (2 / 2).
    ///
    /// RED before step-12: the conversion executor neither inserts intermediates
    /// into the cache nor consults it before tessellating, so the presence
    /// lookups miss (first assertion fails) and the anonymous second realization
    /// re-tessellates + re-ingests (counts climb to 4 / 4).
    #[test]
    fn execute_realization_ops_conversion_intermediates_cache_and_reuse() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryHandleId, GeometryKernel, KernelId,
            Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let union_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(CountingManifoldKernel {
                inner: MockGeometryKernel::new(),
                ingest_count: std::sync::Arc::clone(&ingest_count),
                execute_count: std::sync::Arc::clone(&union_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();

        // ── Realization 1: named, cold cache, full cross-kernel conversion. ──
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "realization 1 errors: {:?}", errors);
        let tess_after_1 = *tess_count.lock().unwrap();
        let ingest_after_1 = *ingest_count.lock().unwrap();
        assert_eq!(
            (tess_after_1, ingest_after_1, *union_count.lock().unwrap()),
            (2, 2, 1),
            "realization 1 must tessellate 2 inputs, ingest 2, run 1 union"
        );

        // The per-stage tolerance the executor used for the single BRep→Mesh
        // stage (one conversion ⇒ `tol × 0.8`).
        let per_stage_tol = per_stage_tolerance_for_plan(
            &DispatchPlan {
                kernel: "manifold".to_string(),
                conversions: vec![(KernelId::Occt, ReprKind::BRep, ReprKind::Mesh)],
            },
            tol,
        );

        // Both intermediates are cached at `("Cross#conv-step{0,1}", Mesh,
        // per_stage_tol, NO_OPTIONS)` — 2 distinct keys (the conversion source
        // provenance is the input's local step index), each holding a genuinely-
        // Mesh Manifold handle (the `manifold.ingest_mesh` result: ids 1000 /
        // 1001 in tessellate order). The key format MUST match the executor's
        // `conversion_intermediate_entity_id` (step-12).
        let cached_0 = state.realization_cache.lookup(
            "Cross#conv-step0",
            ReprKind::Mesh,
            per_stage_tol,
            NO_OPTIONS,
        );
        assert!(
            cached_0.is_some(),
            "intermediate for input step 0 must be cached at (Cross#conv-step0, Mesh, per_stage_tol)"
        );
        let cached_0 = *cached_0.unwrap();
        assert_eq!(
            cached_0.kernel,
            KernelId::Manifold,
            "intermediate handle must be tagged Manifold (target kernel)"
        );
        assert_eq!(cached_0.id, GeometryHandleId(1000));

        let cached_1 = state.realization_cache.lookup(
            "Cross#conv-step1",
            ReprKind::Mesh,
            per_stage_tol,
            NO_OPTIONS,
        );
        assert!(
            cached_1.is_some(),
            "intermediate for input step 1 must be cached at (Cross#conv-step1, Mesh, per_stage_tol)"
        );
        let cached_1 = *cached_1.unwrap();
        assert_eq!(cached_1.kernel, KernelId::Manifold);
        assert_eq!(cached_1.id, GeometryHandleId(1001));

        // ── Realization 2: same entity + ops + tol, ANONYMOUS (no name) so the
        //    whole-realization terminal short-circuit does NOT fire — the
        //    conversion executor runs again and must REUSE both cached
        //    intermediates rather than re-tessellate/re-ingest. ──
        state.dispatch_count = 0;
        state.produced_repr_out = None;
        state.reset_attribute_tables();
        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            None,
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );
        let errors: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_core::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "realization 2 errors: {:?}", errors);
        assert_eq!(
            *tess_count.lock().unwrap(),
            tess_after_1,
            "the anonymous re-realization must REUSE cached intermediates — no extra tessellate"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            ingest_after_1,
            "the anonymous re-realization must REUSE cached intermediates — no extra ingest_mesh"
        );
    }

    /// manifold-like mock whose `ingest_mesh` SUCCEEDS (counting + fresh ids, so
    /// the conversion executor produces and caches intermediates) but whose
    /// `execute` (the final `BooleanUnion`) FAILS — driving the realization into
    /// the rollback branch AFTER at least one intermediate was inserted. Used by
    /// step-13 to pin atomic intermediate-cache rollback.
    struct FailingUnionManifoldKernel {
        ingest_count: std::sync::Arc<std::sync::Mutex<usize>>,
        next_ingest_id: u64,
    }

    impl reify_ir::GeometryKernel for FailingUnionManifoldKernel {
        fn execute(
            &mut self,
            _op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            Err(reify_ir::GeometryError::OperationFailed(
                "simulated union failure".into(),
            ))
        }

        fn query(
            &self,
            _q: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            Err(reify_ir::QueryError::QueryFailed(
                "should not reach: execute always fails".into(),
            ))
        }

        fn export(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _format: reify_ir::ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            Err(reify_ir::ExportError::FormatError(
                "should not reach: execute always fails".into(),
            ))
        }

        fn tessellate(
            &self,
            _handle: reify_ir::GeometryHandleId,
            _tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            Err(reify_ir::TessError::TessellationFailed(
                "should not reach: execute always fails".into(),
            ))
        }

        fn ingest_mesh(
            &mut self,
            _mesh: &reify_ir::Mesh,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            *self.ingest_count.lock().unwrap() += 1;
            let id = reify_ir::GeometryHandleId(self.next_ingest_id);
            self.next_ingest_id += 1;
            Ok(reify_ir::GeometryHandle { id, repr: None })
        }
    }

    /// step-13 (RED): atomic intermediate-cache rollback. A Mesh-demanding
    /// conversion realization whose final `BooleanUnion` execute FAILS (after
    /// both BRep→Mesh intermediates were tessellated, ingested, and cached) must
    /// roll back ATOMICALLY: (i) `step_handles` truncated back to `handle_start`
    /// (no terminal handle leaked), and (ii) every intermediate cache entry the
    /// realization inserted is REMOVED, so a later lookup misses rather than
    /// returning a handle from a realization that never completed.
    ///
    /// RED before step-14: step-12 inserts the intermediates but the
    /// `rolled_back` branch does not yet remove them, so the post-failure
    /// lookups still HIT.
    #[test]
    fn execute_realization_ops_failed_conversion_rolls_back_intermediate_cache() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{
            CapabilityDescriptor, CompiledExpr, GeometryHandleId, GeometryKernel, KernelId,
            Operation, ReprKind,
        };
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let tess_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let ingest_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));

        let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "occt".to_string(),
            Box::new(CountingTessellateKernel {
                inner: MockGeometryKernel::new(),
                tessellate_count: std::sync::Arc::clone(&tess_count),
            }),
        );
        kernels.insert(
            "manifold".to_string(),
            Box::new(FailingUnionManifoldKernel {
                ingest_count: std::sync::Arc::clone(&ingest_count),
                next_ingest_id: 1000,
            }),
        );

        let desc_occt = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (
                    Operation::Convert {
                        from: ReprKind::BRep,
                    },
                    ReprKind::Mesh,
                ),
            ],
        };
        let desc_manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), &desc_occt);
        registry.insert("manifold".to_string(), &desc_manifold);

        let ops = vec![
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                args: vec![
                    ("width".into(), mm_lit(10.0)),
                    ("height".into(), mm_lit(20.0)),
                    ("depth".into(), mm_lit(5.0)),
                ],
            },
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(1),
            },
        ];

        let realization_id = RealizationNodeId::new("Cross", 0);
        let tol = 0.001;
        let mut state = DispatchTestState::default();
        // Pre-seed a sentinel so we can assert truncation went back to exactly
        // the pre-call length (handle_start = 1), not merely "emptied".
        let sentinel = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(0xCAFE),
        };
        state.step_handles.push(sentinel);

        state.run_demand(
            &mut kernels,
            &registry,
            "occt",
            &ops,
            &realization_id,
            Some("Cross"),
            SourceSpan::new(0, 0),
            ReprKind::Mesh,
            Some(tol),
            None,
        );

        // The realization must have FAILED at the union execute, AFTER both
        // intermediates were produced (proving the rollback is non-vacuous).
        assert!(
            state.kernel_error_out.is_some(),
            "the failing union must surface a kernel error"
        );
        assert_eq!(
            *tess_count.lock().unwrap(),
            2,
            "both inputs must have been tessellated before the union failed"
        );
        assert_eq!(
            *ingest_count.lock().unwrap(),
            2,
            "both intermediates must have been ingested (and cached) before the union failed"
        );

        // (i) step_handles truncated back to handle_start — only the sentinel
        //     survives; no occt primitive handles and no terminal handle leaked.
        assert_eq!(
            state.step_handles.len(),
            1,
            "step_handles must truncate back to the pre-call length of 1"
        );
        assert_eq!(
            state.step_handles[0], sentinel,
            "the pre-existing sentinel must be preserved unchanged"
        );

        // (ii) the intermediate cache entries inserted during the failed
        //      realization must be GONE.
        let per_stage_tol = per_stage_tolerance_for_plan(
            &DispatchPlan {
                kernel: "manifold".to_string(),
                conversions: vec![(KernelId::Occt, ReprKind::BRep, ReprKind::Mesh)],
            },
            tol,
        );
        assert!(
            state
                .realization_cache
                .lookup(
                    "Cross#conv-step0",
                    ReprKind::Mesh,
                    per_stage_tol,
                    NO_OPTIONS
                )
                .is_none(),
            "intermediate step-0 must be rolled out of the cache on realization failure"
        );
        assert!(
            state
                .realization_cache
                .lookup(
                    "Cross#conv-step1",
                    ReprKind::Mesh,
                    per_stage_tol,
                    NO_OPTIONS
                )
                .is_none(),
            "intermediate step-1 must be rolled out of the cache on realization failure"
        );
    }

    /// When the registry claims no kernel for the op (dispatch returns
    /// `None`), `execute_realization_ops` must emit a
    /// `DiagnosticCode::NoKernelChain` error diagnostic, set
    /// `kernel_error_out` so the caller can mark the realization Failed, and
    /// truncate `step_handles` back to its pre-call length.
    ///
    /// RED before step-8: routing + dispatch + NoKernelChain wiring all land
    /// in step-8.
    #[test]
    fn execute_realization_ops_emits_no_kernel_chain_diagnostic_when_dispatch_returns_none() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "default".to_string(),
            Box::new(MockGeometryKernel::new()) as Box<dyn reify_ir::GeometryKernel>,
        );

        // Registry deliberately does NOT support PrimitiveBox/BRep: every
        // descriptor in the map only supports BooleanUnion/Mesh, so
        // `dispatch(registry, PrimitiveBox, BRep, {BRep})` returns `None`.
        let desc_d = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("default".to_string(), &desc_d);

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        let mut state = DispatchTestState::default();
        state.run(
            &mut kernels,
            &registry,
            "default",
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // A NoKernelChain error diagnostic must be emitted.
        let no_chain: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::NoKernelChain))
            .collect();
        assert_eq!(
            no_chain.len(),
            1,
            "expected exactly one NoKernelChain diagnostic when the registry has no \
             kernel for the op; got {} diagnostics total: {:?}",
            no_chain.len(),
            state.diagnostics
        );
        assert!(
            matches!(no_chain[0].severity, reify_core::Severity::Error),
            "NoKernelChain must be an Error-severity diagnostic; got {:?}",
            no_chain[0].severity,
        );

        // Realization must surface as Failed via the caller-write kernel_error_out
        // out-param (the same channel `mark_realization_failed` consumes for
        // kernel errors today).
        assert!(
            state.kernel_error_out.is_some(),
            "unroutable op must set kernel_error_out so the caller can mark the \
             realization NodeId as Failed; got None"
        );

        // step_handles must be truncated to its pre-call length: no real handle
        // was produced.
        assert!(
            state.step_handles.is_empty(),
            "unroutable op must leave step_handles truncated to handle_start; got {:?}",
            state.step_handles,
        );
    }

    /// Step-13 (task ε / 3436) RED: the backward-compat None-fallback arm of
    /// [`Engine::execute_realization_ops`] must capture a synthetic
    /// `ReprKind::BRep` into the `produced_repr_out` channel for the executor-
    /// write invariant (step-10) to remain TOTAL across BOTH construction
    /// paths — `Engine::new(_, Some(kernel))` (which wraps the caller-supplied
    /// kernel under the synthetic [`Engine::DEFAULT_KERNEL_NAME`] sentinel and
    /// leaves the inventory registry deliberately out of sync with the kernels
    /// map) AND `with_registered_kernels` (which loads one kernel per
    /// inventory registration so dispatch always finds coverage).
    ///
    /// Pins the production gap the reviewer identified on
    /// `tests/multi_handle_engine_dispatch.rs::executor_writes_produced_repr_brep_on_build_snapshot`:
    /// that integration test passes incidentally when the local build has
    /// `cfg(has_occt)` (OCCT in the registry → dispatch returns
    /// `Some(plan{kernel:"occt"})` → 0-conversion arm falls back to the
    /// DEFAULT_KERNEL_NAME-keyed mock → `last_plan` is `Some` → post-loop
    /// `plan_output_repr` reads OCCT's `(PrimitiveBox, BRep)` support → writes
    /// `BRep`), but FAILS in stub-mode builds where the registry is empty and
    /// the None-fallback arm leaves `last_plan = None`, so the post-loop guard
    /// `if let (Some(plan), Some(op)) = (last_plan.as_ref(), last_operation)`
    /// short-circuits and `produced_repr_out` is never written.
    ///
    /// **Pre-corruption idiom**: this unit test pre-seeds `produced_repr_out =
    /// Some(ReprKind::Mesh)` before calling `execute_realization_ops`, exactly
    /// like the integration test pre-corrupts the snapshot graph node to
    /// `ReprKind::Mesh` before calling `build_snapshot()`. `Mesh` is the
    /// baseline-impossible value in v0.3-ε (the BRep baseline produces only
    /// BRep handles), so any later read of `BRep` here can only come from a
    /// step-14 fallback-arm write of `Some(ReprKind::BRep)`. A naïve
    /// `produced_repr_out == Some(BRep)` assertion against the construction-
    /// time `None` default would pass with or without the step-14 fix.
    ///
    /// **Why this fixture isolates the gap from OCCT availability**: the
    /// registry constructed below has NO `(PrimitiveBox, BRep)` support
    /// regardless of build profile — it carries only `(BooleanUnion, Mesh)`,
    /// a coverage that cannot satisfy the BRep-baseline query triple. The
    /// `assert!(dispatch(...).is_none())` sanity check below pins this
    /// invariant directly so a future registry change that accidentally
    /// covers `(PrimitiveBox, BRep)` would surface here rather than masking
    /// the fallback-arm exercise.
    ///
    /// RED before step-14: `last_produced_repr` does not yet exist, so the
    /// post-loop write key still reads `last_plan` — which is `None` in the
    /// fallback arm — and assertion (iii) below fires.
    #[test]
    fn execute_realization_ops_writes_produced_repr_brep_in_none_fallback_backward_compat() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{DiagnosticCode, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // (a) Registry that does NOT cover `(PrimitiveBox, BRep)`. The lone
        //     descriptor's `supports` list is `[(BooleanUnion, Mesh)]` — a
        //     valid `CapabilityDescriptor` (non-empty `supports`) that cannot
        //     answer the BRep-baseline dispatcher query for a Box op.
        let desc_none_match = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc_none_match);

        // (i) Sanity check: dispatch returns `None` for `(PrimitiveBox, BRep,
        //     {BRep})` against this registry, confirming the test reaches the
        //     None arm of the per-op match in `execute_realization_ops`.
        let available_set: std::collections::HashSet<ReprKind> = {
            let mut s = std::collections::HashSet::new();
            s.insert(ReprKind::BRep);
            s
        };
        assert!(
            dispatch(
                &registry,
                Operation::PrimitiveBox,
                ReprKind::BRep,
                &available_set,
                None,
            )
            .is_none(),
            "test invariant: synthetic registry must yield dispatch() == None for \
             (PrimitiveBox, BRep, {{BRep}}) so the executor reaches the backward-compat \
             fallback arm. If this fires, the registry was accidentally given coverage \
             for (PrimitiveBox, BRep)"
        );

        // (b) Single recording mock kernel keyed under
        //     `Engine::DEFAULT_KERNEL_NAME` — the synthetic sentinel that
        //     `Engine::new(_, Some(kernel))` / `with_prelude` wrap the caller-
        //     supplied kernel under (engine_admin.rs:197).
        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(NamedRecordingKernel {
                name: Engine::DEFAULT_KERNEL_NAME.to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(10.0)),
                ("height".into(), mm_lit(20.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        }];

        // (d) Pre-corrupt `produced_repr_out` to `Some(ReprKind::Mesh)` — a
        //     baseline-impossible value. Any later read of `BRep` can only
        //     come from the step-14 fallback-arm write of
        //     `Some(ReprKind::BRep)`; the construction-time `None` default
        //     would also let the assertion fail loudly if step-14 instead
        //     left the channel untouched. Mirrors the pre-corruption idiom in
        //     `tests/multi_handle_engine_dispatch.rs::executor_writes_produced_repr_brep_on_build_snapshot`.
        //
        //     Constructed via struct-update from `Default::default()` rather
        //     than a post-`default()` field reassignment to avoid the clippy
        //     `field_reassign_with_default` lint — the only field overridden
        //     from default is `produced_repr_out`, so the struct-init form
        //     stays readable.
        let mut state = DispatchTestState {
            produced_repr_out: Some(ReprKind::Mesh),
            ..DispatchTestState::default()
        };

        // (c) `default_kernel_name = Engine::DEFAULT_KERNEL_NAME` — the
        //     sentinel comparison `default_kernel_name ==
        //     Engine::DEFAULT_KERNEL_NAME` inside the None arm gates the
        //     fallback vs strict-mode behaviour (engine_build.rs:2379).
        state.run(
            &mut kernels,
            &registry,
            Engine::DEFAULT_KERNEL_NAME,
            &ops,
            None,
            SourceSpan::new(0, 0),
            None,
        );

        // (ii) The recording mock kernel must have captured the call, proving
        //      the fallback arm executed the op on the synthetic default
        //      (rather than emitting NoKernelChain and breaking out of the
        //      loop without executing).
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![Engine::DEFAULT_KERNEL_NAME.to_string()],
            "fallback arm must execute the op on the kernel registered under \
             Engine::DEFAULT_KERNEL_NAME; got call log {:?}",
            calls
        );
        assert_eq!(
            state.step_handles.len(),
            1,
            "expected one handle pushed from the fallback-routed default kernel"
        );

        // No NoKernelChain diagnostic must be emitted: the sentinel-gated
        // fallback arm is the backward-compat success path, NOT the strict-
        // mode missing-coverage error path the `no_kernel_chain` test pins.
        let no_chain: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::NoKernelChain))
            .collect();
        assert!(
            no_chain.is_empty(),
            "backward-compat None-fallback arm (default_kernel_name == \
             Engine::DEFAULT_KERNEL_NAME && kernels.contains_key(default_kernel_name)) \
             must NOT emit a NoKernelChain diagnostic — that diagnostic belongs to the \
             strict-mode arm only; got {:?}",
            no_chain
        );
        // Realization must NOT be marked Failed: kernel_error_out stays None
        // on the fallback success path (no error to surface to the caller).
        assert!(
            state.kernel_error_out.is_none(),
            "backward-compat fallback success must leave kernel_error_out untouched; \
             got {:?}",
            state.kernel_error_out
        );

        // (iii) The produced_repr_out channel must now carry
        //       `Some(ReprKind::BRep)` — overwriting the pre-corrupted
        //       `Some(ReprKind::Mesh)`. RED before step-14: the post-loop
        //       write guard `if let (Some(plan), Some(op)) =
        //       (last_plan.as_ref(), last_operation)` short-circuits because
        //       the fallback arm never sets `last_plan`, so the pre-corrupted
        //       Mesh value survives and this assertion fires. Step-14
        //       introduces a parallel `last_produced_repr` channel that the
        //       fallback arm sets to `Some(BRep)` (the v0.2 single-kernel
        //       invariant) and rewrites the post-loop write to consult it.
        assert_eq!(
            state.produced_repr_out,
            Some(ReprKind::BRep),
            "executor must write produced_repr = BRep through the None-fallback \
             backward-compat arm so the executor-write invariant (step-10) remains \
             TOTAL across both construction paths; got {:?}. If this fires after \
             step-14 lands, check that `last_produced_repr` is set in the None arm \
             (default_kernel_name == Engine::DEFAULT_KERNEL_NAME && \
             kernels.contains_key(default_kernel_name)) and that the post-loop write \
             consults it.",
            state.produced_repr_out
        );
    }

    // ── pragma-steering seam tests (task #3443, step S3) ─────────────────────

    /// Pragma-steering at the execute_realization_ops seam: when
    /// `prefer_kernel=Some("occt")` is supplied, the op routes to "occt" even
    /// though lex-min would pick "manifold" (m < o). A sibling call with
    /// `prefer_kernel=None` confirms lex-min routing to "manifold".
    ///
    /// Registry: `{"manifold", "occt"}` both supporting `(BooleanUnion, BRep)`.
    /// Available = `{BRep}` (direct dispatch). Kernels are `NamedRecordingKernel`
    /// instances so the test can read back which kernel's `execute()` fired.
    ///
    /// RED until S4 adds `prefer_kernel: Option<&str>` to `DispatchTestState::run`
    /// and threads it through `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_pragma_steers_to_preferred_kernel() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::Type;
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Both kernels support (PrimitiveBox, BRep) and (BooleanUnion, BRep) so
        // both primitives AND the union can route to either kernel.  Lex-min
        // picks "manifold" (m < o) for every op; prefer_kernel=Some("occt")
        // must override the terminal union.
        let desc = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &desc);
        registry.insert("occt".to_string(), &desc);

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(NamedRecordingKernel {
                name: "manifold".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );
        kernels.insert(
            "occt".to_string(),
            Box::new(NamedRecordingKernel {
                name: "occt".to_string(),
                inner: MockGeometryKernel::new(),
                log: std::sync::Arc::clone(&log),
            }),
        );

        // One PrimitiveBox followed by a BooleanUnion of step 0 with itself.
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(0),
            },
        ];

        // ── No pragma: lex-min "manifold" must be picked for every op. ──────
        let mut state_none = DispatchTestState::default();
        state_none.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            // RED: this 8th argument does not exist until S4 adds prefer_kernel
            // to DispatchTestState::run.
            None,
        );
        let calls_none = log.lock().unwrap().clone();
        assert!(
            calls_none
                .iter()
                .all(|k| k == "manifold"),
            "no pragma: every op must route to lex-min 'manifold'; got: {calls_none:?}",
        );

        // Reset log and re-use kernels for the pragma run.
        log.lock().unwrap().clear();

        // ── pragma "occt": union must be routed to "occt". ──────────────────
        let mut state_occt = DispatchTestState::default();
        state_occt.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            // RED: same — prefer_kernel param does not exist yet.
            Some("occt"),
        );
        let calls_occt = log.lock().unwrap().clone();
        // The union (last op, index 1) must be on "occt"; primitives can be on
        // either (lex-min still applies to them since they're not the pragma-
        // steered terminal op).
        assert!(
            calls_occt.last().map(|s| s.as_str()) == Some("occt"),
            "prefer_kernel=Some(\"occt\"): the terminal op (union) must route to \
             'occt', not lex-min 'manifold'; calls: {calls_occt:?}",
        );
    }

    // ── pragma-unsatisfiable diagnostic seam tests (task #3443, step S5) ───────

    /// `execute_realization_ops` must emit a `Severity::Warning` diagnostic with
    /// code `KernelPragmaUnsatisfiable` when `prefer_kernel` names a kernel that
    /// is absent from the registry (or present but not supporting the demanded
    /// `(op, demanded)` pair), and must STILL route the op via lex-min fallback
    /// (no `kernel_error_out`, one handle produced).
    ///
    /// Two scenarios:
    ///
    /// - **Unsatisfiable** (`prefer_kernel=Some("occt")`, "occt" absent): one
    ///   `KernelPragmaUnsatisfiable` warning; op routed to lex-min "manifold";
    ///   `kernel_error_out` is `None`; `step_handles.len() == 1`.
    /// - **Satisfiable** (`prefer_kernel=Some("manifold")`, "manifold" present
    ///   and supporting): zero `KernelPragmaUnsatisfiable` diagnostics.
    ///
    /// RED until S6 wires `kernel_pragma_unsatisfiable_diagnostic` into the
    /// per-op dispatch site in `execute_realization_ops`.
    #[test]
    fn execute_realization_ops_emits_kernel_pragma_unsatisfiable_and_falls_through() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::{DiagnosticCode, Severity, Type};
        use reify_ir::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind};
        use reify_test_support::mocks::MockGeometryKernel;

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        // Registry: only "manifold" supports (PrimitiveBox, BRep) and
        // (BooleanUnion, BRep). "occt" is deliberately absent — so
        // prefer_kernel=Some("occt") is unsatisfiable.
        let desc = CapabilityDescriptor {
            supports: vec![
                (Operation::PrimitiveBox, ReprKind::BRep),
                (Operation::BooleanUnion, ReprKind::BRep),
            ],
        };
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert("manifold".to_string(), &desc);

        let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "manifold".to_string(),
            Box::new(MockGeometryKernel::new()) as Box<dyn reify_ir::GeometryKernel>,
        );

        // Two ops: one PrimitiveBox followed by a BooleanUnion.
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
                left: GeomRef::Step(0),
                right: GeomRef::Step(0),
            },
        ];

        // ── Unsatisfiable pragma: "occt" is absent from the registry. ────────
        let mut state_unsat = DispatchTestState::default();
        state_unsat.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("occt"),
        );

        // (i) Exactly one KernelPragmaUnsatisfiable Warning must be emitted.
        // RED: execute_realization_ops does not yet call
        // kernel_pragma_unsatisfiable_diagnostic (that wiring is S6's job).
        let unsat_diags: Vec<_> = state_unsat
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert_eq!(
            unsat_diags.len(),
            1,
            "unsatisfiable pragma must emit exactly ONE KernelPragmaUnsatisfiable \
             warning; got {} (all diagnostics: {:?})",
            unsat_diags.len(),
            state_unsat.diagnostics,
        );
        assert!(
            matches!(unsat_diags[0].severity, Severity::Warning),
            "KernelPragmaUnsatisfiable must be Warning-severity; got {:?}",
            unsat_diags[0].severity,
        );

        // (ii) Op STILL routes via lex-min "manifold" fall-through — no error.
        assert!(
            state_unsat.kernel_error_out.is_none(),
            "unsatisfiable pragma must fall through (lex-min routes the op); \
             kernel_error_out should remain None, got {:?}",
            state_unsat.kernel_error_out,
        );
        assert_eq!(
            state_unsat.step_handles.len(),
            ops.len(),
            "unsatisfiable pragma: all ops must produce handles via lex-min; \
             expected {}, got {:?}",
            ops.len(),
            state_unsat.step_handles,
        );

        // ── Satisfiable pragma: "manifold" is present and supports the ops. ──
        let mut state_sat = DispatchTestState::default();
        state_sat.run(
            &mut kernels,
            &registry,
            "manifold",
            &ops,
            None,
            SourceSpan::new(0, 0),
            Some("manifold"),
        );

        // NO KernelPragmaUnsatisfiable diagnostic when the pragma is satisfiable.
        let sat_unsat_diags: Vec<_> = state_sat
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::KernelPragmaUnsatisfiable))
            .collect();
        assert!(
            sat_unsat_diags.is_empty(),
            "satisfiable pragma must NOT emit KernelPragmaUnsatisfiable; \
             got {:?}",
            sat_unsat_diags,
        );
    }

    // ── effective_tessellation_tolerance unit tests ──────────────────────────

    /// When `module.default_tolerance` is `Some(v)`, the helper returns `v`
    /// (in SI metres) verbatim — the module-level `#precision` pragma value
    /// overrides the engine's hardcoded default.
    #[test]
    fn effective_tessellation_tolerance_uses_module_default_when_set() {
        use reify_core::ModulePath;
        use reify_test_support::builders::CompiledModuleBuilder;

        let mut module = CompiledModuleBuilder::new(ModulePath::single("t")).build();
        module.default_tolerance = Some(0.005);

        assert_eq!(
            Engine::effective_tessellation_tolerance(&module),
            0.005,
            "effective_tessellation_tolerance must return module.default_tolerance \
             when it is Some(_)"
        );
    }

    /// When `module.default_tolerance` is `None`, the helper falls back to
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE` — preserving v0.1 behaviour
    /// for modules without a `#precision` pragma.
    #[test]
    fn effective_tessellation_tolerance_falls_back_to_default_when_none() {
        use reify_core::ModulePath;
        use reify_test_support::builders::CompiledModuleBuilder;

        let module = CompiledModuleBuilder::new(ModulePath::single("t")).build();
        assert!(
            module.default_tolerance.is_none(),
            "fresh module from CompiledModuleBuilder should have default_tolerance == None"
        );

        assert_eq!(
            Engine::effective_tessellation_tolerance(&module),
            Engine::DEFAULT_TESSELLATION_TOLERANCE,
            "effective_tessellation_tolerance must fall back to \
             Engine::DEFAULT_TESSELLATION_TOLERANCE when default_tolerance is None"
        );
    }

    // ── End-to-end #precision threading: field → kernel.tessellate ───────────
    //
    // The unit tests above pin `effective_tessellation_tolerance` in isolation,
    // but a regression that decoupled `default_tolerance` from the actual
    // `kernel.tessellate(...)` call site (e.g. someone reverting that line back
    // to the hardcoded constant) would slip through. The two tests below close
    // that gap by driving `tessellate_realizations` with a recording stub kernel
    // that captures every `tolerance` argument.

    /// Recording stub kernel: delegates the full `GeometryKernel` surface to a
    /// `MockGeometryKernel` and only intercepts `tessellate` to capture every
    /// `tolerance` argument into a shared Vec the test can read back after the
    /// engine takes ownership. Delegating (rather than reimplementing the
    /// trait) keeps this stub consistent with how the rest of this file's
    /// tests construct kernels and avoids drift if `MockGeometryKernel` gains
    /// new behaviour.
    struct RecordingTessellationKernel {
        inner: reify_test_support::mocks::MockGeometryKernel,
        recorded_tolerances: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
    }

    impl reify_ir::GeometryKernel for RecordingTessellationKernel {
        fn execute(
            &mut self,
            op: &reify_ir::GeometryOp,
        ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_ir::GeometryQuery,
        ) -> Result<reify_ir::Value, reify_ir::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: reify_ir::GeometryHandleId,
            format: reify_ir::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_ir::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_ir::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
            self.recorded_tolerances.lock().unwrap().push(tolerance);
            self.inner.tessellate(handle, tolerance)
        }
    }

    /// Build a CompiledModule with one Box-primitive realization, suitable for
    /// driving `tessellate_realizations`. Uses the same builder pattern as the
    /// fixture in `geometry_error_handling.rs::module_with_box_realization`.
    fn module_with_one_box_realization() -> reify_compiler::CompiledModule {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{ModulePath, Type};
        use reify_ir::CompiledExpr;
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};

        let e = "TestShape";
        let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());

        let box_op = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".into(), mm_lit(80.0)),
                ("height".into(), mm_lit(100.0)),
                ("depth".into(), mm_lit(5.0)),
            ],
        };

        let template = TopologyTemplateBuilder::new(e)
            .param(e, "width", Type::length(), Some(mm_lit(80.0)))
            .param(e, "height", Type::length(), Some(mm_lit(100.0)))
            .param(e, "depth", Type::length(), Some(mm_lit(5.0)))
            .realization(e, 0, vec![box_op])
            .build();

        CompiledModuleBuilder::new(ModulePath::single("test_precision_threading"))
            .template(template)
            .build()
    }

    /// End-to-end: when `module.default_tolerance == Some(0.005)`, the value
    /// passed to `kernel.tessellate(...)` must be exactly `0.005`. Pins the
    /// `kernel.tessellate(last_handle, Self::effective_tessellation_tolerance(module))`
    /// call site against a regression that re-introduces the hardcoded
    /// `Self::DEFAULT_TESSELLATION_TOLERANCE`.
    #[test]
    fn tessellate_realizations_threads_module_default_tolerance_into_kernel() {
        use reify_test_support::MockConstraintChecker;
        use std::sync::{Arc, Mutex};

        let mut module = module_with_one_box_realization();
        module.default_tolerance = Some(0.005);

        let recorded: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let kernel = RecordingTessellationKernel {
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            recorded_tolerances: Arc::clone(&recorded),
        };
        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), Some(Box::new(kernel)));

        let _ = engine.tessellate_realizations(&module);

        let tolerances = recorded.lock().unwrap().clone();
        assert_eq!(
            tolerances.len(),
            1,
            "expected exactly 1 tessellate call (one realization), got {}: {:?}",
            tolerances.len(),
            tolerances
        );
        assert_eq!(
            tolerances[0], 0.005,
            "kernel.tessellate must receive module.default_tolerance verbatim, got {}",
            tolerances[0]
        );
    }

    // ── parent_handles_for_op unit tests ─────────────────────────────────────

    /// Pins the per-variant-family parent extraction semantics of
    /// `parent_handles_for_op`. All variant families are covered in a single
    /// table; the `label` field doubles as the assertion failure message and
    /// as documentation for each exclusion rationale (path/spine, guide,
    /// plane — the three reference-geometry exclusion contracts).
    ///
    /// Rust's exhaustive `match` in `parent_handles_for_op` catches any new
    /// `GeometryOp` variant at compile time, so one representative per arm
    /// family is enough to guard against misclassification.
    #[test]
    fn parent_handles_for_op_returns_expected_handles_per_variant_family() {
        use reify_ir::Value;
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        struct Case {
            op: GeometryOp,
            expected: Vec<GeometryHandleId>,
            label: &'static str,
        }

        let cases: Vec<Case> = vec![
            // ── Primitives ────────────────────────────────────────────────────
            Case {
                op: GeometryOp::Box {
                    width: Value::Real(0.01),
                    height: Value::Real(0.02),
                    depth: Value::Real(0.005),
                },
                expected: vec![],
                label: "Box → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Cylinder {
                    radius: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Cylinder → empty (primitive, no parents)",
            },
            // ── Curve constructors ────────────────────────────────────────────
            Case {
                op: GeometryOp::LineSegment {
                    x1: 0.0,
                    y1: 0.0,
                    z1: 0.0,
                    x2: 1.0,
                    y2: 0.0,
                    z2: 0.0,
                },
                expected: vec![],
                label: "LineSegment → empty (curve constructor, no parents)",
            },
            // ── Pipe ──────────────────────────────────────────────────────────
            Case {
                op: GeometryOp::Pipe {
                    path: GeometryHandleId(30),
                    radius: Value::Real(0.005),
                },
                expected: vec![],
                label: "Pipe → empty (kernel-internal circle profile, no user-facing parent)",
            },
            // ── Boolean ops ───────────────────────────────────────────────────
            Case {
                op: GeometryOp::Union {
                    left: GeometryHandleId(1),
                    right: GeometryHandleId(2),
                },
                expected: vec![GeometryHandleId(1), GeometryHandleId(2)],
                label: "Union → [left, right] in left-then-right order",
            },
            Case {
                op: GeometryOp::Difference {
                    left: GeometryHandleId(3),
                    right: GeometryHandleId(4),
                },
                expected: vec![GeometryHandleId(3), GeometryHandleId(4)],
                label: "Difference → [left, right]",
            },
            Case {
                op: GeometryOp::Intersection {
                    left: GeometryHandleId(5),
                    right: GeometryHandleId(6),
                },
                expected: vec![GeometryHandleId(5), GeometryHandleId(6)],
                label: "Intersection → [left, right]",
            },
            // ── Single-target shape-mods ──────────────────────────────────────
            Case {
                op: GeometryOp::Fillet {
                    target: GeometryHandleId(7),
                    edges: vec![],
                    radius: Value::Real(0.001),
                },
                expected: vec![GeometryHandleId(7)],
                label: "Fillet → [target]",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: GeometryHandleId(82),
                    edges: vec![],
                    distance: Value::Real(0.001),
                },
                expected: vec![GeometryHandleId(82)],
                label: "Chamfer → [target]",
            },
            Case {
                op: GeometryOp::Translate {
                    target: GeometryHandleId(80),
                    dx: 0.01,
                    dy: 0.0,
                    dz: 0.0,
                },
                expected: vec![GeometryHandleId(80)],
                label: "Translate → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::LinearPattern {
                    target: GeometryHandleId(81),
                    direction: [1.0, 0.0, 0.0],
                    count: 3,
                    spacing: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(81)],
                label: "LinearPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::Thicken {
                    target: GeometryHandleId(83),
                    offset: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(83)],
                label: "Thicken → [target]",
            },
            Case {
                op: GeometryOp::OffsetSolid {
                    target: GeometryHandleId(85),
                    distance: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(85)],
                label: "OffsetSolid → [target]",
            },
            Case {
                op: GeometryOp::Shell {
                    target: GeometryHandleId(84),
                    thickness: Value::Real(0.002),
                    faces_to_remove: vec![0],
                    open_face_handles: vec![],
                },
                expected: vec![GeometryHandleId(84)],
                label: "Shell → [target]",
            },
            Case {
                op: GeometryOp::ZoneSlab {
                    target: GeometryHandleId(90),
                    width: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(90)],
                label: "ZoneSlab → [target]",
            },
            Case {
                op: GeometryOp::Draft {
                    target: GeometryHandleId(70),
                    faces: vec![],
                    angle: Value::Real(0.1),
                    plane: GeometryHandleId(71),
                },
                expected: vec![GeometryHandleId(70)],
                // Draft's `plane` is a reference geometry / constraint, not a
                // parent whose sub-shapes propagate — analogous to SweepGuided's
                // guide.
                label: "Draft → [target] only; plane excluded (reference constraint, not a parent)",
            },
            // ── Single-profile sweep ops (path / spine excluded) ──────────────
            Case {
                op: GeometryOp::Extrude {
                    profile: GeometryHandleId(85),
                    distance: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(85)],
                label: "Extrude → [profile] (single-profile sweep)",
            },
            Case {
                op: GeometryOp::ExtrudeSymmetric {
                    profile: GeometryHandleId(50),
                    distance: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(50)],
                label: "ExtrudeSymmetric → [profile]",
            },
            Case {
                op: GeometryOp::Revolve {
                    profile: GeometryHandleId(60),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    angle_rad: std::f64::consts::PI,
                },
                expected: vec![GeometryHandleId(60)],
                label: "Revolve → [profile] (axis fields are scalars, not parent handles)",
            },
            Case {
                op: GeometryOp::Sweep {
                    profile: GeometryHandleId(20),
                    path: GeometryHandleId(21),
                },
                expected: vec![GeometryHandleId(20)],
                // Path/spine is a route, not a parent whose sub-shapes propagate
                // into the result — mirrors populate_attribute_history semantics
                // (engine_build.rs:103-114).
                label: "Sweep → [profile] only; path excluded (spine is not a parent)",
            },
            Case {
                op: GeometryOp::SweepGuided {
                    profile: GeometryHandleId(40),
                    path: GeometryHandleId(41),
                    guide: GeometryHandleId(42),
                },
                expected: vec![GeometryHandleId(40)],
                label: "SweepGuided → [profile] only; both path and guide excluded (guide is an auxiliary constraint wire, not a parent)",
            },
            // ── Multi-profile loft ops (guides excluded) ───────────────────────
            Case {
                op: GeometryOp::Loft {
                    profiles: vec![
                        GeometryHandleId(10),
                        GeometryHandleId(11),
                        GeometryHandleId(12),
                    ],
                },
                expected: vec![
                    GeometryHandleId(10),
                    GeometryHandleId(11),
                    GeometryHandleId(12),
                ],
                label: "Loft → all profiles in input order (multi-profile, ordering preserved)",
            },
            Case {
                op: GeometryOp::LoftGuided {
                    profiles: vec![
                        GeometryHandleId(20),
                        GeometryHandleId(21),
                        GeometryHandleId(22),
                    ],
                    guides: vec![GeometryHandleId(30), GeometryHandleId(31)],
                },
                expected: vec![
                    GeometryHandleId(20),
                    GeometryHandleId(21),
                    GeometryHandleId(22),
                ],
                // Most error-prone exclusion: a regression that appended guides to
                // the parent list would be silently missed without this case.
                label: "LoftGuided → profiles only; guides excluded (constraints, not parents)",
            },
            // ── Remaining primitives (task 4671 step-3: full 47-variant coverage) ─
            Case {
                op: GeometryOp::Sphere { radius: Value::Real(0.005) },
                expected: vec![],
                label: "Sphere → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Tube {
                    outer_r: Value::Real(0.01),
                    inner_r: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Tube → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Cone {
                    bottom_radius: Value::Real(0.01),
                    top_radius: Value::Real(0.005),
                    height: Value::Real(0.02),
                },
                expected: vec![],
                label: "Cone → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Wedge {
                    width: Value::Real(0.020),
                    depth: Value::Real(0.010),
                    height: Value::Real(0.015),
                    top_width: Value::Real(0.005),
                },
                expected: vec![],
                label: "Wedge → empty (primitive, no parents)",
            },
            Case {
                op: GeometryOp::Torus {
                    major_radius: Value::Real(0.02),
                    minor_radius: Value::Real(0.005),
                },
                expected: vec![],
                label: "Torus → empty (primitive, no parents)",
            },
            // ── Remaining curve constructors ──────────────────────────────────
            Case {
                op: GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.01,
                    start_angle: 0.0,
                    end_angle: 1.57,
                    axis: [0.0, 0.0, 1.0],
                },
                expected: vec![],
                label: "Arc → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::Helix {
                    radius: 0.01,
                    pitch: 0.005,
                    height: 0.05,
                },
                expected: vec![],
                label: "Helix → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::InterpCurve {
                    points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: vec![],
                label: "InterpCurve → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::BezierCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: vec![],
                label: "BezierCurve → empty (curve constructor, no parents)",
            },
            Case {
                op: GeometryOp::NurbsCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                    weights: vec![1.0, 1.0],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    degree: 1,
                },
                expected: vec![],
                label: "NurbsCurve → empty (curve constructor, no parents)",
            },
            // ── Profile face producers ─────────────────────────────────────────
            Case {
                op: GeometryOp::RectangleProfile {
                    width: Value::Real(0.02),
                    height: Value::Real(0.01),
                },
                expected: vec![],
                label: "RectangleProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::CircleProfile { radius: Value::Real(0.008) },
                expected: vec![],
                label: "CircleProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::PolygonProfile {
                    points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]],
                },
                expected: vec![],
                label: "PolygonProfile → empty (profile producer, no parents)",
            },
            Case {
                op: GeometryOp::EllipseProfile {
                    semi_major: Value::Real(0.010),
                    semi_minor: Value::Real(0.005),
                },
                expected: vec![],
                label: "EllipseProfile → empty (profile producer, no parents)",
            },
            // ── Remaining single-target shape-mods ────────────────────────────
            Case {
                op: GeometryOp::ChamferAsymmetric {
                    target: GeometryHandleId(91),
                    edges: vec![],
                    d1: Value::Real(0.001),
                    d2: Value::Real(0.002),
                },
                expected: vec![GeometryHandleId(91)],
                label: "ChamferAsymmetric → [target]",
            },
            Case {
                op: GeometryOp::Rotate {
                    target: GeometryHandleId(92),
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: vec![GeometryHandleId(92)],
                label: "Rotate → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::Scale {
                    target: GeometryHandleId(93),
                    factor: 2.0,
                },
                expected: vec![GeometryHandleId(93)],
                label: "Scale → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::RotateAround {
                    target: GeometryHandleId(94),
                    point: [0.0, 0.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: vec![GeometryHandleId(94)],
                label: "RotateAround → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::ApplyTransform {
                    target: GeometryHandleId(95),
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: vec![GeometryHandleId(95)],
                label: "ApplyTransform → [target] (single-target transform)",
            },
            Case {
                op: GeometryOp::CircularPattern {
                    target: GeometryHandleId(96),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    count: 4,
                    angle: Value::Real(1.57),
                },
                expected: vec![GeometryHandleId(96)],
                label: "CircularPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::Mirror {
                    target: GeometryHandleId(97),
                    plane_origin: [0.0, 0.0, 0.0],
                    plane_normal: [1.0, 0.0, 0.0],
                },
                expected: vec![GeometryHandleId(97)],
                label: "Mirror → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::LinearPattern2D {
                    target: GeometryHandleId(98),
                    direction1: [1.0, 0.0, 0.0],
                    count1: 3,
                    spacing1: Value::Real(0.01),
                    direction2: [0.0, 1.0, 0.0],
                    count2: 3,
                    spacing2: Value::Real(0.01),
                },
                expected: vec![GeometryHandleId(98)],
                label: "LinearPattern2D → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::ArbitraryPattern {
                    target: GeometryHandleId(99),
                    transforms: vec![[0.0, 0.0, 0.0]],
                },
                expected: vec![GeometryHandleId(99)],
                label: "ArbitraryPattern → [target] (single-target pattern)",
            },
            Case {
                op: GeometryOp::OffsetCurve {
                    target: GeometryHandleId(100),
                    distance: Value::Real(0.002),
                    reference: None,
                    direction: None,
                },
                expected: vec![GeometryHandleId(100)],
                label: "OffsetCurve → [target]; reference is a constraint surface, not a parent",
            },
        ];

        for case in &cases {
            assert_eq!(
                parent_handles_for_op(&case.op).as_slice(),
                case.expected.as_slice(),
                "parent_handles_for_op mismatch: {}",
                case.label,
            );
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear exactly once in the cases table (DD-3 model — adding a variant
        // forces a RED test-time failure before it reaches unreachable!() in production).
        let seen: HashSet<GeometryOpDiscriminants> =
            cases.iter().map(|c| GeometryOpDiscriminants::from(&c.op)).collect();
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "parent_handles_for_op coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── substitute_op_parents unit tests ─────────────────────────────────────

    /// Characterizes the per-variant-family parent-handle substitution semantics
    /// of `substitute_op_parents`. For every non-Split variant (47 total):
    /// builds an op with known handle ids, applies `substitute_op_parents` with
    /// a mapping that remaps those ids, and asserts that only the PARENT fields
    /// are rewritten — non-parent fields (Pipe.path, Sweep.path, SweepGuided.path
    /// + .guide, Draft.plane, OffsetCurve.reference, LoftGuided.guides) are
    ///   deliberately placed in the map but must NOT be rewritten. Handles absent
    ///   from the map are left as-is (tested via Union left absent from map).
    ///
    /// All expected values are hardcoded independently of the L1 table, so
    /// full 47-variant coverage gives full validation of the table's
    /// `parent_role` column for this function.
    ///
    /// Stays GREEN against the current per-variant fn; the coverage-completeness
    /// assertion turns RED if a new variant is added and not covered here.
    #[test]
    fn substitute_op_parents_rewrites_parents_per_variant_family() {
        use std::collections::HashMap;
        use reify_ir::Value;
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        let h = GeometryHandleId;
        let mut seen: HashSet<GeometryOpDiscriminants> = HashSet::new();

        fn make_map(
            pairs: &[(u64, u64)],
        ) -> HashMap<GeometryHandleId, GeometryHandleId> {
            pairs.iter().map(|&(s, d)| (GeometryHandleId(s), GeometryHandleId(d))).collect()
        }

        // ── None-role: primitives — scalar fields only, nothing to substitute ─
        let no_handles = make_map(&[(999, 9999)]); // map with irrelevant entries

        let mut op = GeometryOp::Box { width: Value::Real(1.0), height: Value::Real(1.0), depth: Value::Real(1.0) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles); // must not panic

        let mut op = GeometryOp::Cylinder { radius: Value::Real(0.005), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Sphere { radius: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Tube { outer_r: Value::Real(0.01), inner_r: Value::Real(0.005), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Cone { bottom_radius: Value::Real(0.01), top_radius: Value::Real(0.0), height: Value::Real(0.02) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Wedge { width: Value::Real(0.02), depth: Value::Real(0.01), height: Value::Real(0.015), top_width: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Torus { major_radius: Value::Real(0.02), minor_radius: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: curve constructors ─────────────────────────────────────

        let mut op = GeometryOp::LineSegment { x1: 0.0, y1: 0.0, z1: 0.0, x2: 1.0, y2: 0.0, z2: 0.0 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Arc { center: [0.0; 3], radius: 0.01, start_angle: 0.0, end_angle: 1.57, axis: [0.0, 0.0, 1.0] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::Helix { radius: 0.01, pitch: 0.005, height: 0.05 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::InterpCurve { points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::BezierCurve { control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::NurbsCurve { control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], weights: vec![1.0, 1.0], knots: vec![0.0, 0.0, 1.0, 1.0], degree: 1 };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: profile face producers ────────────────────────────────

        let mut op = GeometryOp::RectangleProfile { width: Value::Real(0.02), height: Value::Real(0.01) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::CircleProfile { radius: Value::Real(0.008) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::PolygonProfile { points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01]] };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        let mut op = GeometryOp::EllipseProfile { semi_major: Value::Real(0.01), semi_minor: Value::Real(0.005) };
        seen.insert(GeometryOpDiscriminants::from(&op));
        substitute_op_parents(&mut op, &no_handles);

        // ── None-role: Pipe — path IS in the map but must NOT be remapped ────
        {
            let mut op = GeometryOp::Pipe { path: h(30), radius: Value::Real(0.005) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(30, 300)]));
            match &op {
                GeometryOp::Pipe { path, .. } => assert_eq!(
                    *path, h(30),
                    "Pipe.path must NOT be substituted (kernel-internal profile, not a user-facing parent)"
                ),
                _ => panic!("op must still be Pipe"),
            }
        }

        // ── Pair: both left and right are parents ─────────────────────────────
        {
            let mut op = GeometryOp::Union { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Union { left, right } => {
                    assert_eq!(*left, h(101), "Union.left must be remapped");
                    assert_eq!(*right, h(102), "Union.right must be remapped");
                }
                _ => panic!("op must still be Union"),
            }

            // Absent-from-map: right is NOT in the map, must stay as-is
            let mut op = GeometryOp::Union { left: h(3), right: h(4) };
            substitute_op_parents(&mut op, &make_map(&[(3, 103)])); // 4 absent
            match &op {
                GeometryOp::Union { left, right } => {
                    assert_eq!(*left, h(103), "Union.left must be remapped");
                    assert_eq!(*right, h(4), "Union.right absent from map must stay as-is");
                }
                _ => panic!("op must still be Union"),
            }
        }
        {
            let mut op = GeometryOp::Difference { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Difference { left, right } => {
                    assert_eq!(*left, h(101), "Difference.left remapped");
                    assert_eq!(*right, h(102), "Difference.right remapped");
                }
                _ => panic!("op must still be Difference"),
            }
        }
        {
            let mut op = GeometryOp::Intersection { left: h(1), right: h(2) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(1, 101), (2, 102)]));
            match &op {
                GeometryOp::Intersection { left, right } => {
                    assert_eq!(*left, h(101), "Intersection.left remapped");
                    assert_eq!(*right, h(102), "Intersection.right remapped");
                }
                _ => panic!("op must still be Intersection"),
            }
        }

        // ── SingleTarget: target is the sole parent ──────────────────────────
        macro_rules! check_single_target {
            ($op:expr, $target_id:expr, $new_id:expr, $label:literal) => {{
                let disc = GeometryOpDiscriminants::from(&$op);
                seen.insert(disc);
                let mut op = $op;
                substitute_op_parents(&mut op, &make_map(&[($target_id, $new_id)]));
                assert_eq!(
                    parent_handles_for_op(&op).as_slice(),
                    &[GeometryHandleId($new_id)],
                    "SingleTarget {}: target must be remapped",
                    $label
                );
            }};
        }

        check_single_target!(
            GeometryOp::Fillet { target: h(10), edges: vec![], radius: Value::Real(0.001) },
            10, 110, "Fillet"
        );
        check_single_target!(
            GeometryOp::Chamfer { target: h(10), edges: vec![], distance: Value::Real(0.001) },
            10, 110, "Chamfer"
        );
        check_single_target!(
            GeometryOp::ChamferAsymmetric { target: h(10), edges: vec![], d1: Value::Real(0.001), d2: Value::Real(0.002) },
            10, 110, "ChamferAsymmetric"
        );
        check_single_target!(
            GeometryOp::Translate { target: h(10), dx: 0.0, dy: 0.0, dz: 0.01 },
            10, 110, "Translate"
        );
        check_single_target!(
            GeometryOp::Rotate { target: h(10), axis: [0.0, 0.0, 1.0], angle_rad: 0.5 },
            10, 110, "Rotate"
        );
        check_single_target!(
            GeometryOp::Scale { target: h(10), factor: 2.0 },
            10, 110, "Scale"
        );
        check_single_target!(
            GeometryOp::RotateAround { target: h(10), point: [0.0; 3], axis: [0.0, 0.0, 1.0], angle_rad: 0.5 },
            10, 110, "RotateAround"
        );
        check_single_target!(
            GeometryOp::ApplyTransform { target: h(10), rotation: [1.0, 0.0, 0.0, 0.0], translation: [0.0; 3] },
            10, 110, "ApplyTransform"
        );
        check_single_target!(
            GeometryOp::LinearPattern { target: h(10), direction: [1.0, 0.0, 0.0], count: 3, spacing: Value::Real(0.01) },
            10, 110, "LinearPattern"
        );
        check_single_target!(
            GeometryOp::CircularPattern { target: h(10), axis_origin: [0.0; 3], axis_dir: [0.0, 0.0, 1.0], count: 4, angle: Value::Real(1.57) },
            10, 110, "CircularPattern"
        );
        check_single_target!(
            GeometryOp::Mirror { target: h(10), plane_origin: [0.0; 3], plane_normal: [1.0, 0.0, 0.0] },
            10, 110, "Mirror"
        );
        check_single_target!(
            GeometryOp::LinearPattern2D { target: h(10), direction1: [1.0, 0.0, 0.0], count1: 3, spacing1: Value::Real(0.01), direction2: [0.0, 1.0, 0.0], count2: 3, spacing2: Value::Real(0.01) },
            10, 110, "LinearPattern2D"
        );
        check_single_target!(
            GeometryOp::ArbitraryPattern { target: h(10), transforms: vec![[0.0; 3]] },
            10, 110, "ArbitraryPattern"
        );
        check_single_target!(
            GeometryOp::Thicken { target: h(10), offset: Value::Real(0.002) },
            10, 110, "Thicken"
        );
        check_single_target!(
            GeometryOp::OffsetSolid { target: h(10), distance: Value::Real(0.002) },
            10, 110, "OffsetSolid"
        );
        check_single_target!(
            GeometryOp::Shell { target: h(10), thickness: Value::Real(0.002), faces_to_remove: vec![0], open_face_handles: vec![] },
            10, 110, "Shell"
        );
        check_single_target!(
            GeometryOp::ZoneSlab { target: h(10), width: Value::Real(0.002) },
            10, 110, "ZoneSlab"
        );

        // Draft.plane is a constraint, not a parent — must NOT be remapped
        {
            let mut op = GeometryOp::Draft { target: h(10), faces: vec![], angle: Value::Real(0.1), plane: h(20) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::Draft { target, plane, .. } => {
                    assert_eq!(*target, h(110), "Draft.target must be remapped");
                    assert_eq!(*plane, h(20), "Draft.plane must NOT be remapped (reference constraint)");
                }
                _ => panic!("op must still be Draft"),
            }
        }
        // OffsetCurve.reference is a constraint surface, not a parent
        {
            let mut op = GeometryOp::OffsetCurve { target: h(10), distance: Value::Real(0.002), reference: Some(h(20)), direction: None };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::OffsetCurve { target, reference, .. } => {
                    assert_eq!(*target, h(110), "OffsetCurve.target must be remapped");
                    assert_eq!(*reference, Some(h(20)), "OffsetCurve.reference must NOT be remapped (constraint surface)");
                }
                _ => panic!("op must still be OffsetCurve"),
            }
        }

        // ── SingleProfile: profile only; path/guide excluded ─────────────────
        {
            let mut op = GeometryOp::Extrude { profile: h(10), distance: Value::Real(0.01) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::Extrude { profile, .. } => assert_eq!(*profile, h(110), "Extrude.profile remapped"),
                _ => panic!("op must still be Extrude"),
            }
        }
        {
            let mut op = GeometryOp::ExtrudeSymmetric { profile: h(10), distance: Value::Real(0.01) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::ExtrudeSymmetric { profile, .. } => assert_eq!(*profile, h(110), "ExtrudeSymmetric.profile remapped"),
                _ => panic!("op must still be ExtrudeSymmetric"),
            }
        }
        {
            let mut op = GeometryOp::Revolve { profile: h(10), axis_origin: [0.0; 3], axis_dir: [0.0, 0.0, 1.0], angle_rad: 1.0 };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110)]));
            match &op {
                GeometryOp::Revolve { profile, .. } => assert_eq!(*profile, h(110), "Revolve.profile remapped"),
                _ => panic!("op must still be Revolve"),
            }
        }
        // Sweep.path is a route, not a parent — must NOT be remapped
        {
            let mut op = GeometryOp::Sweep { profile: h(10), path: h(20) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220)]));
            match &op {
                GeometryOp::Sweep { profile, path } => {
                    assert_eq!(*profile, h(110), "Sweep.profile must be remapped");
                    assert_eq!(*path, h(20), "Sweep.path must NOT be remapped (spine is not a parent)");
                }
                _ => panic!("op must still be Sweep"),
            }
        }
        // SweepGuided.path and .guide are both excluded
        {
            let mut op = GeometryOp::SweepGuided { profile: h(10), path: h(20), guide: h(30) };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (20, 220), (30, 330)]));
            match &op {
                GeometryOp::SweepGuided { profile, path, guide } => {
                    assert_eq!(*profile, h(110), "SweepGuided.profile must be remapped");
                    assert_eq!(*path, h(20), "SweepGuided.path must NOT be remapped");
                    assert_eq!(*guide, h(30), "SweepGuided.guide must NOT be remapped (auxiliary constraint)");
                }
                _ => panic!("op must still be SweepGuided"),
            }
        }

        // ── VariadicProfiles: every profile remapped; guides excluded ─────────
        {
            let mut op = GeometryOp::Loft { profiles: vec![h(10), h(11), h(12)] };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (11, 111), (12, 112)]));
            match &op {
                GeometryOp::Loft { profiles } => assert_eq!(
                    profiles.as_slice(),
                    &[h(110), h(111), h(112)],
                    "Loft: all profiles must be remapped"
                ),
                _ => panic!("op must still be Loft"),
            }
        }
        // LoftGuided.guides must NOT be remapped
        {
            let mut op = GeometryOp::LoftGuided { profiles: vec![h(10), h(11)], guides: vec![h(30), h(31)] };
            seen.insert(GeometryOpDiscriminants::from(&op));
            substitute_op_parents(&mut op, &make_map(&[(10, 110), (11, 111), (30, 330), (31, 331)]));
            match &op {
                GeometryOp::LoftGuided { profiles, guides } => {
                    assert_eq!(profiles.as_slice(), &[h(110), h(111)], "LoftGuided: profiles must be remapped");
                    assert_eq!(guides.as_slice(), &[h(30), h(31)], "LoftGuided: guides must NOT be remapped");
                }
                _ => panic!("op must still be LoftGuided"),
            }
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear in the cases above (DD-3 model).
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "substitute_op_parents coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── compute_demanded_tols unit tests ─────────────────────────────────────

    /// Pins the new return type of `compute_demanded_tols`:
    /// `Vec<Vec<Option<f64>>>` indexed `[template_idx][realization_idx]`
    /// rather than `HashMap<(String, String), Option<f64>>`.
    ///
    /// Two sub-scenarios:
    ///
    /// (a) **Shape + all-None**: module with two templates — template `A`
    ///     (1 realization, entity "EntityA") and template `B` (2 realizations,
    ///     entities "EntityB_0" / "EntityB_1"), no tolerance contributors →
    ///     outer length == 2, inner lengths [1, 2], all cells `None`.
    ///
    /// (b) **Positive-path / positional alignment**: same module, but
    ///     `active_tolerance_scope` is seeded so EntityA → `Some(1e-5)` and
    ///     EntityB_0 → `Some(2e-5)`, while EntityB_1 is left unset.
    ///     Asserts that `result[0][0] == Some(1e-5)`,
    ///     `result[1][0] == Some(2e-5)`, and `result[1][1] == None` —
    ///     pinning correct positional alignment plus that an
    ///     `active_tolerance_scope` entry surfaces through the chain as
    ///     `Some(_)`.  Note: `demanded_tolerance_for_output` already
    ///     incorporates `active_tolerance_for` internally (the purpose_bound
    ///     path in `combine_demanded_tolerance`), so the seeded scope entry
    ///     surfaces via that function directly — no `.or_else` fallback is
    ///     required or present in the production code.
    #[test]
    fn compute_demanded_tols_returns_positionally_indexed_vec_of_vec() {
        use reify_core::ModulePath;
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let checker = MockConstraintChecker::new();
        // `mut` required for the positive-path sub-scenario where we seed
        // `active_tolerance_scope` directly (crate-internal field).
        let mut engine = crate::Engine::new(Box::new(checker), None);

        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization("EntityA", 0, vec![])
            .build();
        // Use distinct entity refs for B's two realizations so we can set one
        // scope entry and leave the other unset, pinning positional alignment.
        let template_b = TopologyTemplateBuilder::new("EntityB")
            .realization("EntityB_0", 0, vec![])
            .realization("EntityB_1", 1, vec![])
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_tols"))
            .template(template_a)
            .template(template_b)
            .build();

        // ── (a) shape + all-None ─────────────────────────────────────────────
        let result: Vec<Vec<Option<f64>>> = engine.compute_demanded_tols(&module);

        assert_eq!(
            result.len(),
            2,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result[0].len(), 1, "template A has 1 realization");
        assert_eq!(result[1].len(), 2, "template B has 2 realizations");
        assert!(
            result[0][0].is_none(),
            "no tolerance contributor → None for template A realization 0"
        );
        assert!(
            result[1][0].is_none(),
            "no tolerance contributor → None for template B realization 0"
        );
        assert!(
            result[1][1].is_none(),
            "no tolerance contributor → None for template B realization 1"
        );

        // ── (b) positive-path: active-tolerance contributor surfaces, positional alignment ──
        //
        // Seed `active_tolerance_scope` (crate-private field, directly
        // accessible from `mod tests` within the same crate) so that
        // `active_tolerance_for("EntityA")` and `active_tolerance_for("EntityB_0")`
        // return `Some`.  `demanded_tolerance_for_output` incorporates
        // `active_tolerance_for` as its purpose_bound path inside
        // `combine_demanded_tolerance`, so the seeded scope entries surface
        // as `Some(_)` through that function directly.  This test pins
        // (i) that the entry surfaces as `Some(_)` via the production path,
        // and (ii) correct positional alignment.
        engine
            .active_tolerance_scope
            .insert("EntityA".to_string(), 1e-5_f64);
        engine
            .active_tolerance_scope
            .insert("EntityB_0".to_string(), 2e-5_f64);
        // "EntityB_1" is intentionally left unset → result[1][1] stays None.

        let positive: Vec<Vec<Option<f64>>> = engine.compute_demanded_tols(&module);

        assert_eq!(
            positive[0][0],
            Some(1e-5),
            "EntityA scope → Some(1e-5) at [template_idx=0][r_idx=0]; \
             priority chain must surface it rather than return None"
        );
        assert_eq!(
            positive[1][0],
            Some(2e-5),
            "EntityB_0 scope → Some(2e-5) at [template_idx=1][r_idx=0]; \
             positional alignment: first realization must map to inner index 0"
        );
        assert!(
            positive[1][1].is_none(),
            "EntityB_1 unset → None at [template_idx=1][r_idx=1]; \
             positional alignment: second realization must map to inner index 1"
        );
    }

    // ── geometry_op_to_operation unit tests ──────────────────────────────────

    /// Pins the `GeometryOp` → `Operation` total mapping (task ε / 3436,
    /// PRD §8 step-3/4). Each entry constructs a representative `GeometryOp`
    /// (argument values are immaterial — the mapping is purely on variant
    /// kind, mirroring `parent_handles_for_op`'s table) and asserts the
    /// dispatcher-classifier output.
    ///
    /// Coverage spans every variant family — Primitives, Curves, Pipe,
    /// Booleans, single-target Modify/Transform/Pattern, single-profile
    /// Sweep, multi-profile Loft. Rust's exhaustive `match` inside
    /// `geometry_op_to_operation` makes a new `GeometryOp` variant fail to
    /// compile at the helper site, so the helper itself guards against
    /// missing arms — this test pins the chosen `Operation` per arm.
    ///
    /// RED before step-4 impl: `geometry_op_to_operation` does not exist yet.
    #[test]
    fn geometry_op_to_operation_maps_every_variant_family() {
        use reify_ir::{Operation, Value};
        use reify_ir::geometry::GeometryOpDiscriminants;
        use strum::IntoEnumIterator;

        let h = |id| GeometryHandleId(id);
        let r = |v| Value::Real(v);

        struct Case {
            op: GeometryOp,
            expected: Operation,
            label: &'static str,
        }

        let cases: Vec<Case> = vec![
            // Primitives
            Case {
                op: GeometryOp::Box {
                    width: r(0.01),
                    height: r(0.01),
                    depth: r(0.01),
                },
                expected: Operation::PrimitiveBox,
                label: "Box → PrimitiveBox",
            },
            Case {
                op: GeometryOp::Cylinder {
                    radius: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveCylinder,
                label: "Cylinder → PrimitiveCylinder",
            },
            Case {
                op: GeometryOp::Sphere { radius: r(0.005) },
                expected: Operation::PrimitiveSphere,
                label: "Sphere → PrimitiveSphere",
            },
            Case {
                op: GeometryOp::Tube {
                    outer_r: r(0.01),
                    inner_r: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveTube,
                label: "Tube → PrimitiveTube",
            },
            Case {
                op: GeometryOp::Cone {
                    bottom_radius: r(0.01),
                    top_radius: r(0.005),
                    height: r(0.02),
                },
                expected: Operation::PrimitiveCone,
                label: "Cone → PrimitiveCone",
            },
            Case {
                op: GeometryOp::Wedge {
                    width: r(0.020),
                    depth: r(0.010),
                    height: r(0.015),
                    top_width: r(0.005),
                },
                expected: Operation::PrimitiveWedge,
                label: "Wedge → PrimitiveWedge",
            },
            Case {
                op: GeometryOp::Torus {
                    major_radius: r(0.02),
                    minor_radius: r(0.005),
                },
                expected: Operation::PrimitiveTorus,
                label: "Torus → PrimitiveTorus",
            },
            // Booleans
            Case {
                op: GeometryOp::Union {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanUnion,
                label: "Union → BooleanUnion",
            },
            Case {
                op: GeometryOp::Difference {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanDifference,
                label: "Difference → BooleanDifference",
            },
            Case {
                op: GeometryOp::Intersection {
                    left: h(1),
                    right: h(2),
                },
                expected: Operation::BooleanIntersection,
                label: "Intersection → BooleanIntersection",
            },
            // Modify
            Case {
                op: GeometryOp::Fillet {
                    target: h(1),
                    edges: vec![],
                    radius: r(0.001),
                },
                expected: Operation::ModifyFillet,
                label: "Fillet → ModifyFillet",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: h(1),
                    edges: vec![],
                    distance: r(0.001),
                },
                expected: Operation::ModifyChamfer,
                label: "Chamfer → ModifyChamfer",
            },
            Case {
                op: GeometryOp::Shell {
                    target: h(1),
                    thickness: r(0.001),
                    faces_to_remove: vec![0],
                    open_face_handles: vec![],
                },
                expected: Operation::ModifyShell,
                label: "Shell → ModifyShell",
            },
            Case {
                op: GeometryOp::Draft {
                    target: h(1),
                    faces: vec![],
                    angle: r(0.1),
                    plane: h(2),
                },
                expected: Operation::ModifyDraft,
                label: "Draft → ModifyDraft",
            },
            Case {
                op: GeometryOp::Thicken {
                    target: h(1),
                    offset: r(0.001),
                },
                expected: Operation::ModifyThicken,
                label: "Thicken → ModifyThicken",
            },
            Case {
                op: GeometryOp::ZoneSlab {
                    target: h(1),
                    width: r(0.002),
                },
                expected: Operation::ModifyZoneSlab,
                label: "ZoneSlab → ModifyZoneSlab",
            },
            Case {
                op: GeometryOp::OffsetSolid {
                    target: h(1),
                    distance: r(0.002),
                },
                expected: Operation::ModifyOffsetSolid,
                label: "OffsetSolid → ModifyOffsetSolid",
            },
            Case {
                op: GeometryOp::OffsetCurve {
                    target: h(1),
                    distance: r(0.002),
                    reference: None,
                    direction: None,
                },
                expected: Operation::ModifyOffsetCurve,
                label: "OffsetCurve → ModifyOffsetCurve",
            },
            // Transform
            Case {
                op: GeometryOp::Translate {
                    target: h(1),
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.01,
                },
                expected: Operation::TransformTranslate,
                label: "Translate → TransformTranslate",
            },
            Case {
                op: GeometryOp::Rotate {
                    target: h(1),
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: Operation::TransformRotate,
                label: "Rotate → TransformRotate",
            },
            Case {
                op: GeometryOp::Scale {
                    target: h(1),
                    factor: 2.0,
                },
                expected: Operation::TransformScale,
                label: "Scale → TransformScale",
            },
            Case {
                op: GeometryOp::RotateAround {
                    target: h(1),
                    point: [0.0, 0.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    angle_rad: 0.5,
                },
                expected: Operation::TransformRotateAround,
                label: "RotateAround → TransformRotateAround",
            },
            // Pattern
            Case {
                op: GeometryOp::LinearPattern {
                    target: h(1),
                    direction: [1.0, 0.0, 0.0],
                    count: 3,
                    spacing: r(0.01),
                },
                expected: Operation::PatternLinear,
                label: "LinearPattern → PatternLinear",
            },
            Case {
                op: GeometryOp::CircularPattern {
                    target: h(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    count: 4,
                    angle: r(1.57),
                },
                expected: Operation::PatternCircular,
                label: "CircularPattern → PatternCircular",
            },
            Case {
                op: GeometryOp::Mirror {
                    target: h(1),
                    plane_origin: [0.0, 0.0, 0.0],
                    plane_normal: [1.0, 0.0, 0.0],
                },
                expected: Operation::PatternMirror,
                label: "Mirror → PatternMirror",
            },
            Case {
                op: GeometryOp::LinearPattern2D {
                    target: h(1),
                    direction1: [1.0, 0.0, 0.0],
                    count1: 3,
                    spacing1: r(0.01),
                    direction2: [0.0, 1.0, 0.0],
                    count2: 3,
                    spacing2: r(0.01),
                },
                expected: Operation::PatternLinear2D,
                label: "LinearPattern2D → PatternLinear2D",
            },
            Case {
                op: GeometryOp::ArbitraryPattern {
                    target: h(1),
                    transforms: vec![[0.0, 0.0, 0.0]],
                },
                expected: Operation::PatternArbitrary,
                label: "ArbitraryPattern → PatternArbitrary",
            },
            // Sweep (single-profile)
            Case {
                op: GeometryOp::Extrude {
                    profile: h(1),
                    distance: r(0.01),
                },
                expected: Operation::SweepExtrude,
                label: "Extrude → SweepExtrude",
            },
            Case {
                op: GeometryOp::ExtrudeSymmetric {
                    profile: h(1),
                    distance: r(0.01),
                },
                expected: Operation::SweepExtrudeSymmetric,
                label: "ExtrudeSymmetric → SweepExtrudeSymmetric",
            },
            Case {
                op: GeometryOp::Revolve {
                    profile: h(1),
                    axis_origin: [0.0, 0.0, 0.0],
                    axis_dir: [0.0, 0.0, 1.0],
                    angle_rad: 1.0,
                },
                expected: Operation::SweepRevolve,
                label: "Revolve → SweepRevolve",
            },
            Case {
                op: GeometryOp::Sweep {
                    profile: h(1),
                    path: h(2),
                },
                expected: Operation::SweepSweep,
                label: "Sweep → SweepSweep",
            },
            Case {
                op: GeometryOp::SweepGuided {
                    profile: h(1),
                    path: h(2),
                    guide: h(3),
                },
                expected: Operation::SweepSweepGuided,
                label: "SweepGuided → SweepSweepGuided",
            },
            Case {
                op: GeometryOp::Pipe {
                    path: h(1),
                    radius: r(0.005),
                },
                expected: Operation::SweepPipe,
                label: "Pipe → SweepPipe",
            },
            // Loft (multi-profile)
            Case {
                op: GeometryOp::Loft {
                    profiles: vec![h(1), h(2)],
                },
                expected: Operation::SweepLoft,
                label: "Loft → SweepLoft",
            },
            Case {
                op: GeometryOp::LoftGuided {
                    profiles: vec![h(1), h(2)],
                    guides: vec![h(3)],
                },
                expected: Operation::SweepLoftGuided,
                label: "LoftGuided → SweepLoftGuided",
            },
            // Curves
            Case {
                op: GeometryOp::LineSegment {
                    x1: 0.0,
                    y1: 0.0,
                    z1: 0.0,
                    x2: 1.0,
                    y2: 0.0,
                    z2: 0.0,
                },
                expected: Operation::CurveLineSegment,
                label: "LineSegment → CurveLineSegment",
            },
            Case {
                op: GeometryOp::Arc {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.01,
                    start_angle: 0.0,
                    end_angle: 1.57,
                    axis: [0.0, 0.0, 1.0],
                },
                expected: Operation::CurveArc,
                label: "Arc → CurveArc",
            },
            Case {
                op: GeometryOp::Helix {
                    radius: 0.01,
                    pitch: 0.005,
                    height: 0.05,
                },
                expected: Operation::CurveHelix,
                label: "Helix → CurveHelix",
            },
            Case {
                op: GeometryOp::InterpCurve {
                    points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: Operation::CurveInterpCurve,
                label: "InterpCurve → CurveInterpCurve",
            },
            Case {
                op: GeometryOp::BezierCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                },
                expected: Operation::CurveBezierCurve,
                label: "BezierCurve → CurveBezierCurve",
            },
            Case {
                op: GeometryOp::NurbsCurve {
                    control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                    weights: vec![1.0, 1.0],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    degree: 1,
                },
                expected: Operation::CurveNurbsCurve,
                label: "NurbsCurve → CurveNurbsCurve",
            },
            // Profiles (task-4160)
            Case {
                op: GeometryOp::RectangleProfile {
                    width: r(0.02),
                    height: r(0.01),
                },
                expected: Operation::ProfileRectangle,
                label: "RectangleProfile → ProfileRectangle",
            },
            Case {
                op: GeometryOp::CircleProfile { radius: r(0.008) },
                expected: Operation::ProfileCircle,
                label: "CircleProfile → ProfileCircle",
            },
            // Profiles (task-4161)
            Case {
                op: GeometryOp::PolygonProfile {
                    points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]],
                },
                expected: Operation::ProfilePolygon,
                label: "PolygonProfile → ProfilePolygon",
            },
            Case {
                op: GeometryOp::EllipseProfile {
                    semi_major: r(0.010),
                    semi_minor: r(0.005),
                },
                expected: Operation::ProfileEllipse,
                label: "EllipseProfile → ProfileEllipse",
            },
            // Previously missing from coverage (task 4671 step-1):
            Case {
                op: GeometryOp::ChamferAsymmetric {
                    target: h(1),
                    edges: vec![],
                    d1: r(0.001),
                    d2: r(0.002),
                },
                expected: Operation::ModifyChamfer,
                label: "ChamferAsymmetric → ModifyChamfer (reuses the ModifyChamfer capability)",
            },
            Case {
                op: GeometryOp::ApplyTransform {
                    target: h(1),
                    rotation: [1.0, 0.0, 0.0, 0.0],
                    translation: [0.0, 0.0, 0.0],
                },
                expected: Operation::TransformApplyTransform,
                label: "ApplyTransform → TransformApplyTransform",
            },
        ];

        for case in &cases {
            let got = geometry_op_to_operation(&case.op);
            assert_eq!(got, case.expected, "{} (got {got:?})", case.label);
        }

        // Coverage-completeness assertion: every non-Split GeometryOpDiscriminants
        // must appear exactly once in the cases table. Adding a new variant and
        // forgetting to add it here turns this into a RED test-time failure before
        // it could ever reach an unreachable!() in production (DD-3 model).
        let seen: HashSet<GeometryOpDiscriminants> =
            cases.iter().map(|c| GeometryOpDiscriminants::from(&c.op)).collect();
        let all_non_split: HashSet<GeometryOpDiscriminants> = GeometryOpDiscriminants::iter()
            .filter(|d| *d != GeometryOpDiscriminants::Split)
            .collect();
        assert_eq!(
            seen,
            all_non_split,
            "geometry_op_to_operation coverage gap — missing discriminants: {:?}",
            all_non_split.difference(&seen).collect::<Vec<_>>()
        );
    }

    // ── plan_output_repr unit tests ──────────────────────────────────────────

    /// Pins the `plan_output_repr` produced-repr derivation helper
    /// (task ε / 3436, PRD §8 step-5/6).
    ///
    /// The helper takes a borrowed-view registry, a [`DispatchPlan`] (whose
    /// `kernel` field names the chosen kernel), and an [`Operation`], and
    /// returns the `ReprKind` that kernel produces for `op` — i.e. the second
    /// element of the matching entry in `descriptor.supports`. This is the
    /// value `execute_realization_ops` (step-10) will write into the
    /// realization graph node's `produced_repr` field.
    ///
    /// Two synthetic kernels exercise both reprs the v0.3 dispatcher recognises:
    /// (a) a BRep-native kernel supporting `(BooleanUnion, BRep)` → `BRep`,
    /// (b) a Mesh-native kernel supporting `(BooleanUnion, Mesh)` → `Mesh`.
    /// Each plan names exactly one kernel and contains zero conversions
    /// (the ε baseline; non-empty chains are deferred to ζ/η/θ).
    ///
    /// A third sub-case pins the `None` fallback when the named kernel does
    /// not support `op` for any repr — defensible against an invariant
    /// violation where dispatch is given an inconsistent registry.
    ///
    /// RED before step-6 impl: `plan_output_repr` does not exist yet.
    #[test]
    fn plan_output_repr_returns_kernel_descriptor_output_repr() {
        // (a) BRep-native kernel.
        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut brep_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        brep_registry.insert("occt".to_string(), &occt);
        let brep_plan = DispatchPlan {
            kernel: "occt".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&brep_registry, &brep_plan, Operation::BooleanUnion),
            Some(ReprKind::BRep),
            "occt supports (BooleanUnion, BRep) → plan_output_repr must return BRep",
        );

        // (b) Mesh-native kernel.
        let manifold = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
        };
        let mut mesh_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        mesh_registry.insert("manifold".to_string(), &manifold);
        let mesh_plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&mesh_registry, &mesh_plan, Operation::BooleanUnion),
            Some(ReprKind::Mesh),
            "manifold supports (BooleanUnion, Mesh) → plan_output_repr must return Mesh",
        );

        // (c) Defensive fallback: plan names a kernel whose descriptor has
        // no entry for the requested op. plan_output_repr must return None
        // so the caller (execute_realization_ops in step-10) can surface a
        // diagnostic rather than fabricate a repr.
        let empty = CapabilityDescriptor { supports: vec![] };
        let mut empty_registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        empty_registry.insert("empty".to_string(), &empty);
        let empty_plan = DispatchPlan {
            kernel: "empty".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&empty_registry, &empty_plan, Operation::BooleanUnion),
            None,
            "kernel with no matching supports entry → plan_output_repr must return None",
        );

        // (d) Plan kernel missing from registry — also None (defensive).
        let mut occt_only: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        occt_only.insert("occt".to_string(), &occt);
        let missing_plan = DispatchPlan {
            kernel: "manifold".to_string(),
            conversions: vec![],
        };
        assert_eq!(
            plan_output_repr(&occt_only, &missing_plan, Operation::BooleanUnion),
            None,
            "plan.kernel absent from registry → plan_output_repr must return None",
        );
    }

    // ── compute_tessellation_budgets unit tests ───────────────────────────────

    /// Pins the return type of `compute_tessellation_budgets`:
    /// `Vec<Vec<f64>>` indexed `[template_idx][realization_idx]`.
    ///
    /// Two sub-scenarios share the same module fixture (1 template `EntityA`,
    /// 1 realization) and registry `{occt: [(BooleanUnion, BRep)]}`:
    ///
    /// (a) **No demanded tol / fallback path**: `demanded_tols[0][0]` is
    ///     `None` (no tolerance contributor) → helper falls back to
    ///     `effective_tessellation_tolerance(module)` (default `1e-4`) and
    ///     routes it through the v0.2 single-kernel registry which yields a
    ///     0-conversion plan → budget equals the fallback value.
    ///
    /// (b) **Seeded active-tolerance scope / Some-branch**: `EntityA` is
    ///     inserted into `active_tolerance_scope` with value `5e-7`.
    ///     Asserts (i) `demanded_b[0][0] == Some(5e-7)` — the scope entry
    ///     surfaces through the chain — and (ii) `budgets_b[0][0] == 5e-7`
    ///     bit-exactly — the v0.2 0-conversion DispatchPlan passes the
    ///     demand through `compute_realization_tolerance_budget` unchanged.
    #[test]
    fn compute_tessellation_budgets_returns_positionally_indexed_vec_of_vec() {
        use reify_core::ModulePath;
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let checker = MockConstraintChecker::new();
        // `mut` required for sub-scenario (b) where we seed
        // `active_tolerance_scope` directly (crate-private field, accessible
        // from `mod tests` within the same crate).
        let mut engine = crate::Engine::new(Box::new(checker), None);

        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization("EntityA", 0, vec![])
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_budgets"))
            .template(template_a)
            .build();

        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        registry.insert("occt".to_string(), occt);

        // ── (a) no demanded tol → fallback path ─────────────────────────────
        let demanded = engine.compute_demanded_tols(&module);
        let budgets: Vec<Vec<f64>> =
            engine.compute_tessellation_budgets(&module, &demanded, &registry);

        assert_eq!(
            budgets.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(budgets[0].len(), 1, "template A has 1 realization");
        assert_eq!(
            budgets[0][0],
            Engine::effective_tessellation_tolerance(&module),
            "no demanded tol → falls back to module default; 0-conversion DispatchPlan \
             passes it through bit-exactly",
        );

        // ── (b) seeded active-tolerance scope → Some-branch ─────────────────
        //
        // Seed `active_tolerance_scope` (crate-private field) so that
        // `active_tolerance_for("EntityA")` returns `Some(5e-7)`.  This
        // drives `compute_demanded_tols` into `Some(5e-7)`, which in turn
        // drives `compute_tessellation_budgets` into the
        // `compute_realization_tolerance_budget` Some-branch.  Under the v0.2
        // single-kernel registry the dispatcher returns a 0-conversion
        // DispatchPlan, so `per_stage_tolerance_for_plan` passes the demanded
        // tolerance through unchanged — budget == demanded bit-exactly.
        engine
            .active_tolerance_scope
            .insert("EntityA".to_string(), 5e-7_f64);

        let demanded_b = engine.compute_demanded_tols(&module);
        let budgets_b: Vec<Vec<f64>> =
            engine.compute_tessellation_budgets(&module, &demanded_b, &registry);

        assert_eq!(
            demanded_b[0][0],
            Some(5e-7),
            "EntityA scope entry must surface as Some(5e-7) in demanded_tols[0][0] \
             (precondition for the Some-branch budget assertion below)",
        );
        assert_eq!(
            budgets_b[0][0], 5e-7,
            "0-conversion DispatchPlan: compute_realization_tolerance_budget must \
             pass the demanded tolerance through unchanged (bit-exact). \
             Demand: 5e-7",
        );
    }

    // ── compute_realization_tolerance_budget unit tests ───────────────────────

    /// Pins the new 3-arg signature of `compute_realization_tolerance_budget`:
    /// the caller supplies the `&HashSet<ReprKind>` rather than the helper
    /// synthesising it from `BUDGET_QUERY_TRIPLE_V02.2` on every call.
    ///
    /// Fixture: single-kernel registry with `{(BooleanUnion, BRep)}`, demand
    /// `1e-6`, available `{BRep}`. The v0.2 single-kernel registry yields a
    /// 0-conversion `DispatchPlan`, so `per_stage_tolerance_for_plan` returns
    /// the demanded tolerance bit-exactly.
    #[test]
    fn compute_realization_tolerance_budget_accepts_caller_supplied_available_set() {
        use reify_test_support::MockConstraintChecker;

        let checker = MockConstraintChecker::new();
        let engine = crate::Engine::new(Box::new(checker), None);

        let occt = CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        };
        let mut single: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
        single.insert("occt".to_string(), occt);
        let registry_borrowed: BTreeMap<String, &CapabilityDescriptor> =
            single.iter().map(|(k, v)| (k.clone(), v)).collect();

        // Derive `available` from the same const that production code uses so a
        // future change to `BUDGET_QUERY_TRIPLE_V02.2` is caught here automatically.
        let available: HashSet<ReprKind> =
            Engine::BUDGET_QUERY_TRIPLE_V02.2.iter().copied().collect();
        // Verify the public helper returns the identical set — every external
        // consumer greps `budget_available_set`, so this folds the helper's
        // coverage into the same test that pins the const's contents.
        assert_eq!(
            Engine::budget_available_set(),
            available,
            "budget_available_set() must match BUDGET_QUERY_TRIPLE_V02.2 exactly; \
             if this fails, update all `budget_available_set` consumers",
        );
        let demand = 1e-6_f64;

        assert_eq!(
            engine.compute_realization_tolerance_budget(&registry_borrowed, &available, demand),
            demand,
            "single-kernel registry yields a 0-conversion DispatchPlan; \
             per_stage_tolerance_for_plan on an empty chain must return demanded_tol \
             bit-exactly. Demand: {demand}",
        );
    }

    /// End-to-end fallback: when `module.default_tolerance == None`, the value
    /// passed to `kernel.tessellate(...)` must be exactly
    /// `Engine::DEFAULT_TESSELLATION_TOLERANCE`. Pins the same call site for
    /// the no-pragma path.
    #[test]
    fn tessellate_realizations_falls_back_to_default_tolerance_in_kernel() {
        use reify_test_support::MockConstraintChecker;
        use std::sync::{Arc, Mutex};

        let module = module_with_one_box_realization();
        assert!(
            module.default_tolerance.is_none(),
            "fixture must start with default_tolerance == None"
        );

        let recorded: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
        let kernel = RecordingTessellationKernel {
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            recorded_tolerances: Arc::clone(&recorded),
        };
        let checker = MockConstraintChecker::new();
        let mut engine = crate::Engine::new(Box::new(checker), Some(Box::new(kernel)));

        let _ = engine.tessellate_realizations(&module);

        let tolerances = recorded.lock().unwrap().clone();
        assert_eq!(
            tolerances.len(),
            1,
            "expected exactly 1 tessellate call (one realization), got {}: {:?}",
            tolerances.len(),
            tolerances
        );
        assert_eq!(
            tolerances[0],
            Engine::DEFAULT_TESSELLATION_TOLERANCE,
            "kernel.tessellate must receive Engine::DEFAULT_TESSELLATION_TOLERANCE \
             when default_tolerance is None, got {}",
            tolerances[0]
        );
    }

    // ── tessellate_from_values fail-fast indexing tests ───────────────────────

    /// Pins that an out-of-bounds `demanded_tols` lookup in
    /// `tessellate_from_values` is a panic, not a silent `None` fallback.
    ///
    /// Passes `demanded_tols = &[]` (empty slice) with a 1-template /
    /// 1-realization module.  After step 6 replaces the defensive
    /// `.get(t_idx).and_then(...).unwrap_or(None)` with direct slice indexing
    /// `demanded_tols[t_idx][r_idx]`, the first realization triggers an OOB
    /// panic.  Currently RED: the call returns silently because
    /// `demanded_tols.get(0)` returns `None` and `.unwrap_or(None)` swallows
    /// the missing entry.
    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn tessellate_from_values_panics_on_oob_demanded_tols_lookup() {
        use reify_test_support::mocks::MockGeometryKernel;

        let module = module_with_one_box_realization();
        // Task ε (3436): wrap the mock kernel into the new multi-handle map
        // under the synthetic default-kernel name. `default_kernel_name` is
        // threaded through as the resolution key the helper indexes by.
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        geometry_kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(MockGeometryKernel::new()),
        );
        let mut values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut realization_cache: RealizationCache<KernelHandle> = RealizationCache::new();

        // `demanded_tols = &[]` is the OOB trigger: the producer would have
        // generated `&[vec![None]]` for a 1-template/1-realization module, but
        // passing an empty slice causes `demanded_tols[0][...]` to panic.
        // `tessellation_budgets` is correctly shaped so we can confirm the
        // panic originates at the demanded_tol lookup, not the budget lookup.
        let desc = dispatch_test_descriptor_all_brep();
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc);
        let mut achieved_repr_tol = std::collections::BTreeMap::new();
        Engine::tessellate_from_values(
            &mut geometry_kernels,
            &registry,
            Some(Engine::DEFAULT_KERNEL_NAME),
            &module,
            &mut values,
            &functions,
            &mut diagnostics,
            &meta_map,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &mut swept_kind_table,
            &mut realization_cache,
            &[],               // ← OOB: empty demanded_tols
            &[vec![1e-4_f64]], // correctly shaped tessellation_budgets
            &mut 0usize,
            false,
            &mut achieved_repr_tol,
            None,              // unified_pass: LegacyMultiPass (no schedule)
            &std::collections::HashSet::new(), // realization_read_cells: empty
        );
    }

    /// Pins that an out-of-bounds `tessellation_budgets` lookup in
    /// `tessellate_from_values` is a panic, not a silent module-pragma fallback.
    ///
    /// Passes `tessellation_budgets = &[]` (empty slice) with a 1-template /
    /// 1-realization module and correctly-shaped `demanded_tols = &[vec![None]]`.
    /// After step 8 replaces the defensive `.get(t_idx).and_then(...).unwrap_or_else(...)`
    /// with direct slice indexing `tessellation_budgets[t_idx][r_idx]`, control
    /// reaches the budget lookup and panics.  Currently RED: the call returns
    /// silently with `budget = effective_tessellation_tolerance(module)` via the
    /// `unwrap_or_else` fallback.
    #[test]
    #[should_panic(expected = "index out of bounds: the len is 0 but the index is 0")]
    fn tessellate_from_values_panics_on_oob_tessellation_budgets_lookup() {
        use reify_test_support::mocks::MockGeometryKernel;

        let module = module_with_one_box_realization();
        // Task ε (3436): wrap the mock kernel into the multi-handle map under
        // the synthetic default-kernel name (sibling test mirror).
        let mut geometry_kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
        geometry_kernels.insert(
            Engine::DEFAULT_KERNEL_NAME.to_string(),
            Box::new(MockGeometryKernel::new()),
        );
        let mut values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut realization_cache: RealizationCache<KernelHandle> = RealizationCache::new();

        // `demanded_tols` is correctly shaped; `tessellation_budgets = &[]` is
        // the OOB trigger.  The Box primitive in module_with_one_box_realization
        // produces at least one handle after `execute_realization_ops`, so
        // the `if step_handles.len() > handle_start` guard at line 1276 is true
        // and execution reaches the budget lookup.
        let desc = dispatch_test_descriptor_all_brep();
        let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
        registry.insert(Engine::DEFAULT_KERNEL_NAME.to_string(), &desc);
        let mut achieved_repr_tol = std::collections::BTreeMap::new();
        Engine::tessellate_from_values(
            &mut geometry_kernels,
            &registry,
            Some(Engine::DEFAULT_KERNEL_NAME),
            &module,
            &mut values,
            &functions,
            &mut diagnostics,
            &meta_map,
            &mut feature_tag_table,
            &mut topology_attribute_table,
            &mut swept_kind_table,
            &mut realization_cache,
            &[vec![None]], // correctly shaped demanded_tols
            &[],           // ← OOB: empty tessellation_budgets
            &mut 0usize,
            false,
            &mut achieved_repr_tol,
            None,          // unified_pass: LegacyMultiPass (no schedule)
            &std::collections::HashSet::new(), // realization_read_cells: empty
        );
    }

    // ── collect_centroids_with_failure_summary unit tests ─────────────────────

    /// All handles produce kernel query errors → exactly one coalesced warning
    /// naming the count, the realization_id, and the first error message.
    #[test]
    fn collect_centroids_with_failure_summary_coalesces_query_errors() {
        use reify_core::Severity;
        use reify_ir::Role;
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let attr0 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr1 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: Vec::new(),
        };
        let h0 = GeometryHandleId(101);
        let h1 = GeometryHandleId(102);
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h0, &attr0), (h1, &attr1)];

        // No centroid fixtures → query() returns QueryError::QueryFailed for both handles.
        let kernel = MockGeometryKernel::new();

        // Capture the actual error text the kernel will produce for h0 so that
        // the assertion below is decoupled from MockGeometryKernel's exact message
        // format — a mock cleanup won't break this test.
        let expected_first_err = kernel
            .query(&GeometryQuery::Centroid(h0))
            .unwrap_err()
            .to_string();

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        assert!(
            centroids.is_empty(),
            "expected no successful centroids when all queries fail, got: {centroids:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 coalesced warning, got {}: {diagnostics:?}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic must be a Warning, got: {diag:?}"
        );
        assert!(
            diag.message
                .contains("topology-attribute centroid query failed for 2 handle(s)"),
            "message must contain the count phrase, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("TestEntity#realization[0]"),
            "message must contain the realization_id display form, got: {}",
            diag.message
        );
        // Assert the first error's text is preserved verbatim using the sentinel
        // captured above — decoupled from the mock's internal format.
        assert!(
            diag.message
                .contains(&format!("(first: {expected_first_err}")),
            "message must embed the first error text, got: {}",
            diag.message
        );
    }

    /// Both handles produce `Ok(Value::Real(0.0))` from the kernel, which
    /// `parse_xyz_value` rejects as a non-string value → exactly one coalesced
    /// parse-fail warning, no query-fail warning.
    #[test]
    fn collect_centroids_with_failure_summary_coalesces_parse_errors() {
        use reify_core::Severity;
        use reify_ir::{Role, Value};
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let attr0 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr1 = TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: Vec::new(),
        };
        let h0 = GeometryHandleId(101);
        let h1 = GeometryHandleId(102);
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h0, &attr0), (h1, &attr1)];

        // Value::Real is not a string → parse_xyz_value returns Err for both.
        let kernel = MockGeometryKernel::new()
            .with_centroid_result(h0, Value::Real(0.0))
            .with_centroid_result(h1, Value::Real(0.0));

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        assert!(
            centroids.is_empty(),
            "expected no successful centroids when all parses fail, got: {centroids:?}"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 coalesced parse-fail warning, got {}: {diagnostics:?}",
            diagnostics.len()
        );
        let diag = &diagnostics[0];
        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic must be a Warning, got: {diag:?}"
        );
        assert!(
            diag.message
                .contains("topology-attribute centroid parse failed for 2 handle(s)"),
            "message must contain the parse-fail count phrase, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("TestEntity#realization[0]"),
            "message must contain the realization_id display form, got: {}",
            diag.message
        );
        // Assert that the first-error text is present and contains the locally-
        // owned query label ("local_index_reassignment_centroid" is defined in
        // engine_build.rs and passed to parse_xyz_value — stable regardless of
        // how QueryError formats its Display prefix).
        assert!(
            diag.message.contains("(first: "),
            "message must embed the first parse-error text, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("local_index_reassignment_centroid"),
            "first-error text must name the query label, got: {}",
            diag.message
        );
    }

    /// Mixed failure classes: one query-error handle (201), one parse-error
    /// handle (202), one success handle (203). Asserts:
    ///   - centroids map has exactly the success handle's xyz
    ///   - exactly two warnings: one per failure class
    ///   - each warning names the FIRST handle of its class (201 / 202)
    ///   - the parse-fail warning does NOT appear in the query-fail warning
    ///     and vice-versa (classes are separated)
    #[test]
    fn collect_centroids_with_failure_summary_separates_failure_classes_and_preserves_first_message()
     {
        use reify_core::Severity;
        use reify_ir::{Role, Value};
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new("TestEntity", 0);
        let feature_id = FeatureId::from(&realization_id);

        let make_attr = |local_index: u32| TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Side,
            local_index,
            user_label: None,
            mod_history: Vec::new(),
        };
        let attr0 = make_attr(0); // handle 201 — no kernel fixture → query Err
        let attr1 = make_attr(1); // handle 202 — Real(0.0) → parse Err
        let attr2 = make_attr(2); // handle 203 — valid xyz JSON → success

        let h_err = GeometryHandleId(201);
        let h_parse = GeometryHandleId(202);
        let h_ok = GeometryHandleId(203);

        // Construct in deterministic order so "first message" is well-defined.
        let realization_attrs: Vec<(GeometryHandleId, &TopologyAttribute)> =
            vec![(h_err, &attr0), (h_parse, &attr1), (h_ok, &attr2)];

        let kernel = MockGeometryKernel::new()
            // h_err: no fixture → returns QueryError::QueryFailed("no mock result …")
            .with_centroid_result(h_parse, Value::Real(0.0))
            .with_centroid_result(
                h_ok,
                Value::String("{\"x\":1.5,\"y\":2.5,\"z\":3.5}".into()),
            );

        // Capture the actual error text for h_err so the assertion below is
        // decoupled from MockGeometryKernel's exact message format.
        let expected_query_err = kernel
            .query(&GeometryQuery::Centroid(h_err))
            .unwrap_err()
            .to_string();

        let (centroids, diagnostics) =
            collect_centroids_with_failure_summary(&realization_attrs, &kernel, &realization_id);

        // Success handle returns the parsed xyz.
        assert_eq!(
            centroids.len(),
            1,
            "exactly one successful centroid expected, got: {centroids:?}"
        );
        assert_eq!(
            centroids.get(&h_ok),
            Some(&[1.5_f64, 2.5, 3.5]),
            "centroids map must hold the success handle's xyz"
        );

        // Two warnings — one per failure class.
        assert_eq!(
            diagnostics.len(),
            2,
            "expected exactly 2 warnings (one per failure class), got {}: {diagnostics:?}",
            diagnostics.len()
        );
        assert!(
            diagnostics.iter().all(|d| d.severity == Severity::Warning),
            "all diagnostics must be Warnings, got: {diagnostics:?}"
        );

        // Find the query-fail warning and the parse-fail warning.
        let query_warn = diagnostics
            .iter()
            .find(|d| d.message.contains("centroid query failed"))
            .expect("must have a query-fail warning");
        let parse_warn = diagnostics
            .iter()
            .find(|d| d.message.contains("centroid parse failed"))
            .expect("must have a parse-fail warning");

        // Query-fail warning: count=1, first error text matches sentinel
        // captured from the kernel before the call — decoupled from mock format.
        assert!(
            query_warn
                .message
                .contains("centroid query failed for 1 handle(s)"),
            "query-fail count must be 1, got: {}",
            query_warn.message
        );
        assert!(
            query_warn
                .message
                .contains(&format!("(first: {expected_query_err}")),
            "query-fail first must contain the captured error text, got: {}",
            query_warn.message
        );

        // Parse-fail warning: count=1, first-error text names the locally-owned
        // query label ("local_index_reassignment_centroid") — stable regardless
        // of how QueryError formats its Display prefix.
        assert!(
            parse_warn
                .message
                .contains("centroid parse failed for 1 handle(s)"),
            "parse-fail count must be 1, got: {}",
            parse_warn.message
        );
        assert!(
            parse_warn.message.contains("(first: "),
            "parse-fail message must embed the first error text, got: {}",
            parse_warn.message
        );
        assert!(
            parse_warn
                .message
                .contains("local_index_reassignment_centroid"),
            "parse-fail first-error must name the query label, got: {}",
            parse_warn.message
        );
    }

    // ── Task 4349: cross-kernel GeometryHandleId collision regression tests ─────

    /// Shared scaffolding for the two cross-kernel `GeometryHandleId` collision
    /// regression tests (task 4349).
    ///
    /// Builds `DispatchTestState`, pre-seeds the realization cache with
    /// `{Occt, GeometryHandleId(1)}`, calls the `pre_seed` closure (so each
    /// test can populate its own table at `GeometryHandleId(1)` to simulate the
    /// colliding sibling op), then drives the cache-hit short-circuit via
    /// `state.run_demand` with `operations=&[]`. Returns the state after the
    /// call for post-condition assertions.
    fn run_cross_kernel_cache_hit_short_circuit(
        entity_name: &str,
        pre_seed: impl FnOnce(&mut DispatchTestState, &RealizationNodeId),
    ) -> DispatchTestState {
        use reify_test_support::mocks::MockGeometryKernel;

        let realization_id = RealizationNodeId::new(entity_name, 0);
        let tol = 1e-4_f64;

        let desc = dispatch_test_descriptor_all_brep();
        let mut kernels = dispatch_test_kernels(Box::new(MockGeometryKernel::new()));
        let registry = dispatch_test_single_default_registry(&desc);

        let mut state = DispatchTestState::default();

        // Pre-seed the cache: the prior build stored {Occt, GeometryHandleId(1)}
        // as the terminal handle for this entity.
        state.realization_cache.insert(
            &realization_id.entity,
            ReprKind::BRep,
            tol,
            NO_OPTIONS,
            KernelHandle {
                kernel: KernelId::Occt,
                id: GeometryHandleId(1),
            },
        );

        // Allow each test to pre-seed its specific table before the run.
        pre_seed(&mut state, &realization_id);

        // Drive the cache-hit short-circuit: operations=&[] ensures the function
        // returns BEFORE the op loop. demanded_tol=Some(tol) and
        // realization_name=Some("part") together enable the cache probe path.
        state.run_demand(
            &mut kernels,
            &registry,
            "default",
            &[], // empty ops — cache-hit fires before the op loop
            &realization_id,
            Some("part"),
            SourceSpan::new(0, 0),
            ReprKind::BRep,
            Some(tol),
            None,
        );

        state
    }

    /// Regression test for cross-kernel `GeometryHandleId` collision at the
    /// cache-hit short-circuit — `feature_tag_table` path (task 4349).
    ///
    /// # Background
    ///
    /// OCCT and Manifold both mint `GeometryHandleId(1)` for their first
    /// geometry handle (each kernel's counter starts at 1). Within a single
    /// build a Manifold op can record
    /// `feature_tag_table.record(GeometryHandleId(1), tag)` while a later
    /// cache-hit short-circuit returns the cached `{Occt, GeometryHandleId(1)}`
    /// from a prior build. The former
    /// `debug_assert!(feature_tag_table.lookup(cached_handle.id).is_none())`
    /// fires because key `GeometryHandleId(1)` is occupied by the Manifold
    /// entry — two distinct `KernelHandle`s collapsing onto one kernel-blind key.
    ///
    /// After the fix the assert is replaced with
    /// `feature_tag_table.remove(cached_handle.id)`, so the cached handle reads
    /// `None` from the table — satisfying the #3226 spec ("a cache-served handle
    /// has no entries in those tables on the second build") even when a
    /// cross-kernel sibling left a colliding numeric id.
    ///
    /// # Invariant (build-mode independent)
    ///
    /// After the cache-hit short-circuit, `feature_tag_table.lookup(id)` for
    /// the cached handle must return `None` — regardless of whether the build
    /// is debug or release.  In debug builds the former `debug_assert!` also
    /// panicked before the fix, but the meaningful guarantee is the `None`
    /// post-condition, which holds in both build modes.
    #[test]
    fn cache_hit_short_circuit_tolerates_cross_kernel_feature_tag_id_collision() {
        use reify_ir::StepKind;

        let state = run_cross_kernel_cache_hit_short_circuit("CrossKernelEntity", |state, _| {
            // Pre-seed feature_tag_table with a colliding entry at GeometryHandleId(1),
            // simulating a cross-kernel sibling op (e.g. Manifold) that recorded its
            // first handle's tag earlier in this same build.
            state.feature_tag_table.record(
                GeometryHandleId(1),
                FeatureTag {
                    source_span: SourceSpan::new(0, 0),
                    step_kind: StepKind::Primitive,
                    sub_index: 0,
                },
            );
        });

        // Post-condition: the cached handle must read None from feature_tag_table
        // (#3226 spec: a cache-served handle has no entries in those tables).
        assert!(
            state
                .feature_tag_table
                .lookup(GeometryHandleId(1))
                .is_none(),
            "feature_tag_table must have no entry for the cached handle id after \
             cache-hit short-circuit: cross-kernel sibling's colliding entry must \
             be removed (not left behind as a foreign kernel's tag)"
        );
    }

    /// Regression test for cross-kernel `GeometryHandleId` collision at the
    /// cache-hit short-circuit — `topology_attribute_table` path (task 4349).
    ///
    /// Symmetric to `cache_hit_short_circuit_tolerates_cross_kernel_feature_tag_id_collision`
    /// but pre-seeds ONLY `topology_attribute_table` (leaving `feature_tag_table`
    /// empty). After step-2's fix the first check (`feature_tag_table.remove`)
    /// is a no-op on the empty table and execution reaches the SECOND
    /// `debug_assert!(topology_attribute_table.lookup(cached_handle.id).is_none())`
    /// which then fires because `GeometryHandleId(1)` is occupied by the
    /// sibling attribute entry.
    ///
    /// # Invariant (build-mode independent)
    ///
    /// After the cache-hit short-circuit, `topology_attribute_table.lookup(id)`
    /// for the cached handle must return `None` — regardless of debug vs release
    /// build mode.  The `None` post-condition is the meaningful guarantee; in
    /// debug builds the former `debug_assert!` also fired before the fix, but
    /// the test's value is not limited to that panic path.
    #[test]
    fn cache_hit_short_circuit_tolerates_cross_kernel_topology_attribute_id_collision() {
        use reify_ir::{FeatureId, Role};

        let state = run_cross_kernel_cache_hit_short_circuit(
            "CrossKernelEntity2",
            |state, realization_id| {
                // Pre-seed ONLY topology_attribute_table (not feature_tag_table) at
                // GeometryHandleId(1), simulating a cross-kernel sibling Mesh op that
                // recorded its first handle's attribute earlier in this same build.
                // feature_tag_table stays empty → the first check (now a remove, step-2)
                // is a no-op and execution reaches the SECOND assert for topology.
                state.topology_attribute_table.record(
                    GeometryHandleId(1),
                    TopologyAttribute {
                        feature_id: FeatureId::from(realization_id),
                        role: Role::Side,
                        local_index: 0,
                        user_label: None,
                        mod_history: Vec::new(),
                    },
                );
            },
        );

        // Post-condition: the cached handle must read None from topology_attribute_table.
        assert!(
            state
                .topology_attribute_table
                .lookup(GeometryHandleId(1))
                .is_none(),
            "topology_attribute_table must have no entry for the cached handle id \
             after cache-hit short-circuit: cross-kernel sibling's colliding entry \
             must be removed (not left behind as a foreign kernel's attribute)"
        );
    }

    /// Regression test for cross-kernel `GeometryHandleId` collision at the
    /// cache-hit short-circuit — both tables seeded simultaneously (task 4349).
    ///
    /// # Background
    ///
    /// In a realistic cross-kernel build a sibling op typically records BOTH a
    /// feature tag and a topology attribute for its handle.  This test seeds
    /// both `feature_tag_table` and `topology_attribute_table` at
    /// `GeometryHandleId(1)` before the cache-hit short-circuit fires, ensuring
    /// that neither eviction is accidentally gated on the other: both `remove`
    /// calls are independent and both must leave `None` at the colliding id.
    ///
    /// # Invariant (build-mode independent)
    ///
    /// After the cache-hit short-circuit, both
    /// `feature_tag_table.lookup(GeometryHandleId(1))` and
    /// `topology_attribute_table.lookup(GeometryHandleId(1))` must return
    /// `None`.  This holds in debug and release builds alike.
    #[test]
    fn cache_hit_short_circuit_tolerates_cross_kernel_both_tables_id_collision() {
        use reify_ir::{FeatureId, Role, StepKind};

        let state = run_cross_kernel_cache_hit_short_circuit(
            "CrossKernelEntityBoth",
            |state, realization_id| {
                // Pre-seed BOTH tables at GeometryHandleId(1) simultaneously,
                // simulating a sibling op that recorded both a feature tag and a
                // topology attribute for its first handle in the same build.
                state.feature_tag_table.record(
                    GeometryHandleId(1),
                    FeatureTag {
                        source_span: SourceSpan::new(0, 0),
                        step_kind: StepKind::Primitive,
                        sub_index: 0,
                    },
                );
                state.topology_attribute_table.record(
                    GeometryHandleId(1),
                    TopologyAttribute {
                        feature_id: FeatureId::from(realization_id),
                        role: Role::Side,
                        local_index: 0,
                        user_label: None,
                        mod_history: Vec::new(),
                    },
                );
            },
        );

        // Both evictions are independent: neither is gated on the other.
        assert!(
            state
                .feature_tag_table
                .lookup(GeometryHandleId(1))
                .is_none(),
            "feature_tag_table must have no entry for the cached handle id after \
             cache-hit short-circuit (both-tables case)"
        );
        assert!(
            state
                .topology_attribute_table
                .lookup(GeometryHandleId(1))
                .is_none(),
            "topology_attribute_table must have no entry for the cached handle id \
             after cache-hit short-circuit (both-tables case)"
        );
    }

    // ── step-1 (task 4538): pass-ordering regression test ─────────────────────

    /// Regression guard (task 4538): `run_post_processes` must populate `mp`
    /// with real mass-props when the body is a selector-produced
    /// `Value::GeometryHandle`.
    ///
    /// Before task 4538 this test would have failed: `post_process_body_mass_props`
    /// ran before the selector passes, so `sel_body` was still `Value::Undef` when
    /// the mass-props pass read it → body arg had no geometry handle → all three
    /// geometric fields (`mass`/`com`/`inertia`) stayed `Value::Undef`.
    /// The reorder (step-2) placed the selector passes first; this test now guards
    /// the corrected order — a future re-reordering would immediately fail here.
    ///
    /// A SANITY PRECONDITION (`post_process_topology_selectors` on a clone)
    /// verifies that the selector expression is correctly constructed; a RED
    /// failure in the MAIN assertion is therefore unambiguously about ordering.
    ///
    /// Template:
    ///   `sel_body` = `single(edges(s))` — a selector-produced geometry handle
    ///   `mp`       = `body_mass_props(sel_body, rho)` — reads sel_body
    ///
    /// MockGeometryKernel:
    ///   `extract_edges(parent_id)` → `[edge_id]`    (one edge → single() unwraps)
    ///   `Volume(edge_id)` → `Real(3.0)`              (mass = 2000 × 3 = 6000)
    ///   `CenterOfMass(edge_id, 2000.0)` → JSON CoM
    ///   `InertiaTensor(edge_id, 2000.0)` → nested list inertia
    #[test]
    fn run_post_processes_selector_produced_body_gets_real_mass_props() {
        use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type, ValueCellId};
        use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};
        use reify_test_support::{builders::TopologyTemplateBuilder, mocks::MockGeometryKernel};

        // ── geometry-handle fixture IDs ───────────────────────────────────────
        let parent_id = GeometryHandleId(100);
        let edge_id = GeometryHandleId(101);
        let parent_rr = RealizationNodeId::new("Design", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];

        // ── value-cell IDs ────────────────────────────────────────────────────
        let s_cell = ValueCellId::new("Design", "s");
        let sel_body_cell = ValueCellId::new("Design", "sel_body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mp_cell = ValueCellId::new("Design", "mp");

        // ── local helper: build a one-arg FunctionCall CompiledExpr ──────────
        //
        // Follows the pattern of `call_expr` in dynamics_ops::tests:674 and
        // `topology_selector_call_one_value_ref` in geometry_ops::tests:11837.
        fn one_arg_call(fn_name: &str, arg: CompiledExpr, result_type: Type) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(arg.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![arg],
                },
                result_type,
                content_hash,
            }
        }

        // ── local helper: build a two-arg FunctionCall CompiledExpr ──────────
        fn two_arg_call(
            fn_name: &str,
            a1: CompiledExpr,
            a2: CompiledExpr,
            result_type: Type,
        ) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(a1.content_hash)
                .combine(a2.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![a1, a2],
                },
                result_type,
                content_hash,
            }
        }

        // ── default_expr for sel_body: single(edges(s)) ──────────────────────
        //
        // `edges(s)` → FunctionCall("edges", [ValueRef(s_cell, Geometry)])
        //   `try_eval_topology_selector` returns Value::Selector(Edge, All)
        //
        // `single(edges(s))` → FunctionCall("single", [edges_expr])
        //   `try_eval_resolve_selector` matches the `single` arm (geometry_ops
        //   :3031), resolves the inner selector via `resolve_selector_to_list`,
        //   gets a one-element list, and unwraps the sole Value::GeometryHandle.
        //
        // Note: the inner arg is a bare FunctionCall (not wrapped in
        // ResolveSelector) — handled by the "Defensive" arm at geometry_ops:3037.
        let s_vref = CompiledExpr::value_ref(s_cell.clone(), Type::Geometry);
        let edges_expr = one_arg_call("edges", s_vref, Type::List(Box::new(Type::Geometry)));
        let single_edges_expr = one_arg_call("single", edges_expr, Type::Geometry);

        // ── default_expr for mp: body_mass_props(sel_body, rho) ──────────────
        //
        // Mirrors the call_expr helper in dynamics_ops::tests:674.  The body
        // arg is a ValueRef to sel_body — which starts as Undef (no
        // GeometryHandle) and is patched to a GeometryHandle by the selector
        // pass if the ordering is correct.
        let sel_body_vref = CompiledExpr::value_ref(sel_body_cell.clone(), Type::Geometry);
        let rho_vref = CompiledExpr::value_ref(rho_cell.clone(), Type::dimensionless_scalar());
        let mp_expr = two_arg_call(
            "body_mass_props",
            sel_body_vref,
            rho_vref,
            Type::StructureRef("MassProperties".to_string()),
        );

        // ── TopologyTemplate: two Let cells ──────────────────────────────────
        //
        // `sel_body` — post_process_topology_selectors patches this Undef →
        //              GeometryHandle{edge_id} via try_eval_resolve_selector
        // `mp`       — post_process_body_mass_props reads sel_body and
        //              assembles the MassProperties instance
        //
        // `s` and `rho` are seeded directly in the ValueMap; the selector and
        // mass-props passes read them from `values` without needing template
        // cells (only cells with default_expr are iterated by the passes).
        let template = TopologyTemplateBuilder::new("Design")
            .let_binding(
                "Design",
                "sel_body",
                Type::Geometry,
                single_edges_expr.clone(),
            )
            .let_binding(
                "Design",
                "mp",
                Type::StructureRef("MassProperties".to_string()),
                mp_expr,
            )
            .build();

        // ── initial ValueMap ──────────────────────────────────────────────────
        // sel_body and mp start as Undef (the pure eval_expr left them there);
        // the post-process passes must promote them to real values.
        let mut values = ValueMap::new();
        values.insert(
            s_cell.clone(),
            Value::GeometryHandle {
                realization_ref: parent_rr.clone(),
                upstream_values_hash: parent_hash,
                kernel_handle: parent_id,
            },
        );
        values.insert(sel_body_cell.clone(), Value::Undef);
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );
        values.insert(mp_cell.clone(), Value::Undef);

        // ── MockGeometryKernel fixture ────────────────────────────────────────
        // Volume = 3.0 m³ → expected mass = 2000.0 × 3.0 = 6000.0 kg
        // CoM injected as JSON; inertia as nested list with distinct diagonal.
        let injected_com = Value::String("{\"x\":0.01,\"y\":0.02,\"z\":0.03}".to_string());
        let injected_inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_edges(parent_id, vec![edge_id])
            .with_volume_result(edge_id, Value::Real(3.0))
            .with_center_of_mass_result(edge_id, 2000.0, injected_com)
            .with_inertia_tensor_result(edge_id, 2000.0, injected_inertia);

        let named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let table = TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // ── SANITY PRECONDITION ───────────────────────────────────────────────
        //
        // Run `post_process_topology_selectors` alone on a fresh clone to
        // confirm that the single(edges(s)) expression is correctly built and
        // resolves to a Value::GeometryHandle{edge_id}. If this assertion fires,
        // the bug is in the selector expression itself; if it passes, any
        // failure in the MAIN assertion below is unambiguously about ordering.
        {
            let mut values_clone = values.clone();
            let mut kernel2 =
                MockGeometryKernel::new().with_extracted_edges(parent_id, vec![edge_id]);
            let mut diags2: Vec<Diagnostic> = Vec::new();
            Engine::post_process_topology_selectors(
                &template,
                &named_steps,
                &mut values_clone,
                &mut kernel2 as &mut dyn GeometryKernel,
                &table,
                &mut diags2,
            );
            let patched = values_clone
                .get(&sel_body_cell)
                .expect("sel_body must be present after post_process_topology_selectors");
            assert!(
                matches!(
                    patched,
                    Value::GeometryHandle { kernel_handle, .. }
                        if *kernel_handle == edge_id
                ),
                "SANITY: post_process_topology_selectors must patch sel_body to \
                 GeometryHandle{{kernel_handle: {edge_id:?}}}; got: {patched:?}"
            );
        }

        // ── MAIN ASSERTION ────────────────────────────────────────────────────
        //
        // Before task 4538 (post_process_body_mass_props ran BEFORE selectors):
        //   body_mass_props read sel_body = Undef → no handle → mp's mass/
        //   com/inertia fields stayed Undef — this assertion would have failed.
        //
        // Fixed order (task 4538 / step-2):
        //   post_process_topology_selectors runs first → sel_body becomes
        //   GeometryHandle{edge_id} → body_mass_props queries the kernel →
        //   mass = density × Volume = 2000.0 × 3.0 = 6000.0.
        Engine::run_post_processes(
            &template,
            &named_steps,
            &mut values,
            &functions,
            &meta_map,
            &mut kernel as &mut dyn GeometryKernel,
            &table,
            &SweptKindTable::default(),
            &mut diagnostics,
        );

        let mp_val = values
            .get(&mp_cell)
            .expect("mp must be present in values after run_post_processes");
        let data = match mp_val {
            Value::StructureInstance(d) => d,
            other => panic!(
                "mp must be a MassProperties StructureInstance after \
                 run_post_processes; got {other:?}"
            ),
        };
        assert_eq!(data.type_name, "MassProperties");

        // `mass` must not be Undef: this is the ordering-contract assertion.
        // On the RED path the selector pass hasn't run yet so there is no
        // GeometryHandle → mass stays Undef.
        let mass_field = data
            .fields
            .get("mass")
            .expect("MassProperties must have a `mass` field");
        assert!(
            !matches!(mass_field, Value::Undef),
            "ordering regression: `post_process_body_mass_props` ran before the \
             selector passes populated `sel_body` (task 4538 fix). \
             Expected mass = density × volume = 2000.0 × 3.0 = 6000.0; \
             got: {mass_field:?}"
        );
        let mass = match mass_field {
            Value::Scalar { si_value, .. } => *si_value,
            Value::Real(m) => *m,
            other => panic!("mass must be a numeric Scalar or Real; got {other:?}"),
        };
        assert!(
            (mass - 6000.0_f64).abs() < 1e-9,
            "mass = density × volume = 2000.0 × 3.0 = 6000.0; got {mass}"
        );

        // CoM and inertia: assert non-Undef (real kernel values).
        let com_field = data
            .fields
            .get("com")
            .expect("MassProperties must have a `com` field");
        assert!(
            !matches!(com_field, Value::Undef),
            "com must not be Undef after run_post_processes; got {com_field:?}"
        );

        let inertia_field = data
            .fields
            .get("inertia")
            .expect("MassProperties must have an `inertia` field");
        let inertia = crate::dynamics_psd::inertia_3x3_from_value(inertia_field)
            .expect("inertia must parse as 3×3 via inertia_3x3_from_value");
        assert!(
            (inertia[0][0] - 1.0).abs() < 1e-9,
            "inertia[0][0] must be 1.0; got {}",
            inertia[0][0]
        );
        assert!(
            (inertia[1][1] - 2.0).abs() < 1e-9,
            "inertia[1][1] must be 2.0; got {}",
            inertia[1][1]
        );
        assert!(
            (inertia[2][2] - 3.0).abs() < 1e-9,
            "inertia[2][2] must be 3.0; got {}",
            inertia[2][2]
        );

        // Explicit density arg → no E_DynamicsNoDensity error.
        assert!(
            diagnostics.iter().all(|d| {
                !matches!(d.code, Some(reify_core::DiagnosticCode::DynamicsNoDensity))
            }),
            "explicit density must not emit E_DynamicsNoDensity; \
             diagnostics: {diagnostics:?}"
        );
    }

    /// Regression guard (task 4538, direct-body path): a body cell that already
    /// holds a `Value::GeometryHandle` before `run_post_processes` runs must
    /// still produce real mass-props in the new (last) ordering.
    ///
    /// The reorder moved `post_process_body_mass_props` to the end of
    /// `run_post_processes`; this test confirms the common pre-existing case
    /// (a directly let-bound body, not produced by a selector) is unaffected —
    /// real values arrive regardless of whether mass-props runs first or last
    /// relative to the selector passes.
    #[test]
    fn run_post_processes_direct_body_gets_real_mass_props() {
        use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type, ValueCellId};
        use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};
        use reify_test_support::{builders::TopologyTemplateBuilder, mocks::MockGeometryKernel};

        let body_id = GeometryHandleId(200);
        let body_rr = RealizationNodeId::new("Design", 0);
        let body_hash: [u8; 32] = [0xBBu8; 32];

        let body_cell = ValueCellId::new("Design", "body");
        let rho_cell = ValueCellId::new("Design", "rho");
        let mp_cell = ValueCellId::new("Design", "mp");

        // ── two-arg helper (mirrors the one in the selector-produced test) ────
        fn two_arg_call(
            fn_name: &str,
            a1: CompiledExpr,
            a2: CompiledExpr,
            result_type: Type,
        ) -> CompiledExpr {
            let content_hash = ContentHash::of(&[reify_ir::TAG_FUNCTION_CALL])
                .combine(ContentHash::of_str(fn_name))
                .combine(a1.content_hash)
                .combine(a2.content_hash);
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: fn_name.to_string(),
                        qualified_name: fn_name.to_string(),
                    },
                    args: vec![a1, a2],
                },
                result_type,
                content_hash,
            }
        }

        // ── default_expr for mp: body_mass_props(body, rho) ──────────────────
        let body_vref = CompiledExpr::value_ref(body_cell.clone(), Type::Geometry);
        let rho_vref = CompiledExpr::value_ref(rho_cell.clone(), Type::dimensionless_scalar());
        let mp_expr = two_arg_call(
            "body_mass_props",
            body_vref,
            rho_vref,
            Type::StructureRef("MassProperties".to_string()),
        );

        // Only `mp` needs a template cell; body and rho are seeded in the
        // ValueMap and read directly by `post_process_body_mass_props`.
        let template = TopologyTemplateBuilder::new("Design")
            .let_binding(
                "Design",
                "mp",
                Type::StructureRef("MassProperties".to_string()),
                mp_expr,
            )
            .build();

        let mut values = ValueMap::new();
        values.insert(
            body_cell.clone(),
            Value::GeometryHandle {
                realization_ref: body_rr,
                upstream_values_hash: body_hash,
                kernel_handle: body_id,
            },
        );
        values.insert(
            rho_cell.clone(),
            Value::Scalar {
                si_value: 2000.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
        );
        values.insert(mp_cell.clone(), Value::Undef);

        // Volume = 5.0 m³ → expected mass = 2000.0 × 5.0 = 10000.0 kg
        let injected_com = Value::String("{\"x\":0.1,\"y\":0.2,\"z\":0.3}".to_string());
        let injected_inertia = Value::List(vec![
            Value::List(vec![Value::Real(4.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(5.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(6.0)]),
        ]);
        let mut kernel = MockGeometryKernel::new()
            .with_volume_result(body_id, Value::Real(5.0))
            .with_center_of_mass_result(body_id, 2000.0, injected_com)
            .with_inertia_tensor_result(body_id, 2000.0, injected_inertia);

        let named_steps: HashMap<String, KernelHandle> = HashMap::new();
        let functions: Vec<CompiledFunction> = Vec::new();
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let table = TopologyAttributeTable::default();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        Engine::run_post_processes(
            &template,
            &named_steps,
            &mut values,
            &functions,
            &meta_map,
            &mut kernel as &mut dyn GeometryKernel,
            &table,
            &SweptKindTable::default(),
            &mut diagnostics,
        );

        let mp_val = values
            .get(&mp_cell)
            .expect("mp must be present after run_post_processes");
        let data = match mp_val {
            Value::StructureInstance(d) => d,
            other => panic!(
                "direct body: mp must be a MassProperties StructureInstance; \
                 got {other:?}"
            ),
        };
        assert_eq!(data.type_name, "MassProperties");

        let mass_field = data
            .fields
            .get("mass")
            .expect("MassProperties must have a `mass` field");
        let mass = match mass_field {
            Value::Scalar { si_value, .. } => *si_value,
            Value::Real(m) => *m,
            other => panic!("direct body: mass must be a numeric Scalar or Real; got {other:?}"),
        };
        assert!(
            (mass - 10_000.0_f64).abs() < 1e-9,
            "direct body: mass = density × volume = 2000.0 × 5.0 = 10000.0; \
             got {mass}"
        );

        let com_field = data
            .fields
            .get("com")
            .expect("MassProperties must have a `com` field");
        assert!(
            !matches!(com_field, Value::Undef),
            "direct body: com must not be Undef after run_post_processes; \
             got {com_field:?}"
        );
    }
}

// ── populate_attribute_history LocalFeature unit tests (step-3, RED) ────────

/// Tests for `populate_attribute_history` with `AttributeHistory::LocalFeature`.
///
/// RED: `AttributeHistory::LocalFeature` variant and the dispatch arm in
/// `populate_attribute_history` do not exist yet. Tests compile after step-4.
#[cfg(test)]
mod populate_local_feature_tests {
    use reify_ir::{
        AttributeHistory, FeatureId, GeometryHandleId, GeometryOp, HistoryRecord,
        LocalFeatureOpHistoryRecords, ModEntry, QueryError, Role, TopologyAttribute,
        TopologyAttributeTable, Value,
    };
    use reify_test_support::mocks::MockGeometryKernel;

    use super::populate_attribute_history;

    fn fillet_fid() -> FeatureId {
        FeatureId::new("Fillet#realization[0]")
    }

    fn make_attr(fid: &FeatureId, role: Role, local_index: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: fid.clone(),
            role,
            local_index,
            user_label: None,
            mod_history: vec![],
        }
    }

    fn hrec(parent_subshape_index: u32, result_subshape_index: u32) -> HistoryRecord {
        HistoryRecord {
            parent_index: 0,
            parent_subshape_index,
            result_subshape_index,
        }
    }

    // -----------------------------------------------------------------------
    // Fillet: face_generated cross-kind split (1 parent edge → 2 result faces)
    // -----------------------------------------------------------------------
    #[test]
    fn fillet_local_feature_dispatches_and_propagates_face_generated_split() {
        // Handles
        let target = GeometryHandleId(1);
        let result = GeometryHandleId(100);
        let parent_face = GeometryHandleId(10);
        let parent_edge = GeometryHandleId(20);
        let parent_vertex = GeometryHandleId(30);
        let result_face_a = GeometryHandleId(110);
        let result_face_b = GeometryHandleId(111);
        let result_edge = GeometryHandleId(120);

        // Mock: target extraction + result extraction
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(target, vec![parent_face])
            .with_extracted_edges(target, vec![parent_edge])
            .with_extracted_vertices(target, vec![parent_vertex])
            .with_extracted_faces(result, vec![result_face_a, result_face_b])
            .with_extracted_edges(result, vec![result_edge]);

        // Seed: parent_edge has an attribute (its Role::NewEdge propagates to 2 result faces)
        let fid = FeatureId::new("Box#realization[0]");
        let splitting_fid = fillet_fid();
        let mut table = TopologyAttributeTable::default();
        table.record(parent_face, make_attr(&fid, Role::Side, 0));
        table.record(parent_edge, make_attr(&fid, Role::NewEdge, 5));

        // History: one parent edge → two result faces (cross-kind split)
        let history = LocalFeatureOpHistoryRecords {
            face_generated: vec![hrec(0, 0), hrec(0, 1)],
            ..Default::default()
        };
        let attr_history = AttributeHistory::LocalFeature(history);

        let geom_op = GeometryOp::Fillet {
            target,
            edges: vec![],
            radius: Value::Real(0.001),
        };

        populate_attribute_history(
            &mut table,
            &mut kernel,
            &splitting_fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect("fillet LocalFeature dispatch should succeed");

        // Both result faces inherit parent_edge's attr + split ModEntry
        for (handle, expected_split_index) in [(result_face_a, 0u32), (result_face_b, 1u32)] {
            let attr = table
                .lookup(handle)
                .unwrap_or_else(|| panic!("{handle:?} must have attr after fillet propagation"));
            assert_eq!(attr.feature_id, fid);
            assert_eq!(attr.role, Role::NewEdge);
            assert_eq!(attr.local_index, 5);
            assert_eq!(
                attr.mod_history,
                vec![ModEntry {
                    splitting_feature_id: splitting_fid.clone(),
                    split_index: expected_split_index,
                }],
                "result face {handle:?} must have split ModEntry at index {expected_split_index}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Chamfer: edge_generated cross-kind pass-through (1 parent edge → 1 result edge)
    // -----------------------------------------------------------------------
    #[test]
    fn chamfer_local_feature_dispatches_and_propagates_edge_modified_passthrough() {
        let target = GeometryHandleId(2);
        let result = GeometryHandleId(200);
        let parent_face = GeometryHandleId(11);
        let parent_edge = GeometryHandleId(21);
        let parent_vertex = GeometryHandleId(31);
        let result_face = GeometryHandleId(210);
        let result_edge = GeometryHandleId(220);

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(target, vec![parent_face])
            .with_extracted_edges(target, vec![parent_edge])
            .with_extracted_vertices(target, vec![parent_vertex])
            .with_extracted_faces(result, vec![result_face])
            .with_extracted_edges(result, vec![result_edge]);

        let fid = FeatureId::new("Box#realization[0]");
        let splitting_fid = fillet_fid();
        let mut table = TopologyAttributeTable::default();
        table.record(parent_edge, make_attr(&fid, Role::NewEdge, 3));

        // edge_modified: 1 parent edge → 1 result edge (pure pass-through)
        let history = LocalFeatureOpHistoryRecords {
            edge_modified: vec![hrec(0, 0)],
            ..Default::default()
        };
        let attr_history = AttributeHistory::LocalFeature(history);

        let geom_op = GeometryOp::Chamfer {
            target,
            edges: vec![],
            distance: Value::Real(0.001),
        };

        populate_attribute_history(
            &mut table,
            &mut kernel,
            &splitting_fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect("chamfer LocalFeature dispatch should succeed");

        let attr = table
            .lookup(result_edge)
            .expect("result_edge must have attr after chamfer propagation");
        assert_eq!(attr.feature_id, fid);
        assert_eq!(attr.role, Role::NewEdge);
        assert_eq!(attr.local_index, 3);
        assert!(
            attr.mod_history.is_empty(),
            "1→1 pass-through must not add ModEntry; got {:?}",
            attr.mod_history
        );
    }

    // -----------------------------------------------------------------------
    // Guard: non-Fillet/Chamfer GeometryOp with AttributeHistory::LocalFeature
    //        must return Err(QueryError::QueryFailed).
    // -----------------------------------------------------------------------
    #[test]
    fn local_feature_with_non_fillet_chamfer_geom_op_returns_query_failed() {
        let profile = GeometryHandleId(4);
        let result = GeometryHandleId(300);

        // Empty mock — the error must fire before any kernel extraction.
        let mut kernel = MockGeometryKernel::new();
        let mut table = TopologyAttributeTable::default();
        let fid = fillet_fid();

        let attr_history = AttributeHistory::LocalFeature(LocalFeatureOpHistoryRecords::default());

        // Use Extrude (not Fillet/Chamfer) as the mismatched op.
        let geom_op = GeometryOp::Extrude {
            profile,
            distance: Value::Real(0.01),
        };

        let err = populate_attribute_history(
            &mut table,
            &mut kernel,
            &fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect_err("non-Fillet/Chamfer op with LocalFeature history must return QueryFailed");

        match err {
            QueryError::QueryFailed(_) => {}
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }
}

// ── dispatch_volume_mesh unit tests ──────────────────────────────────────────

#[cfg(test)]
mod dispatch_volume_mesh_tests {
    use super::*;
    use reify_ir::{ElementOrderTag, GeometryError, VolumeMesh};
    use reify_solver_elastic::{
        Mesh2d, Mesh2dError, Mesh2dReport, SweepError, SweepParams, SweptMesh3d,
    };

    fn make_empty_volume_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    fn make_swept_mesh(layers: usize) -> SweptMesh3d {
        use reify_solver_elastic::SweptConnectivity;
        SweptMesh3d {
            vertices: vec![],
            connectivity: SweptConnectivity::Wedge { indices: vec![] },
            layers,
        }
    }

    fn make_mesh2d_report() -> Mesh2dReport {
        Mesh2dReport {
            mesh: Mesh2d::Triangle {
                vertices: vec![],
                indices: vec![],
            },
            recombine_attempted: false,
            recombine_quality_ok: true,
        }
    }

    fn extrude_kind() -> crate::sweep_classifier::SweptKind {
        use reify_ir::Value;
        crate::sweep_classifier::SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        }
    }

    // ── Step-1: compile-time surface pin ─────────────────────────────────

    /// Compile-time surface pin: names `VolumeMeshOutcome::Tet` and `::Swept`,
    /// and verifies `dispatch_volume_mesh`'s generic signature including the
    /// new `ops`/`handles` slice parameters.  A rename, signature drift, or
    /// removal of either variant breaks compilation before any behavioural test.
    // ── Step-3: force_tet short-circuit ──────────────────────────────────

    #[test]
    fn dispatch_force_tet_always_calls_tet_path_regardless_of_swept_kind() {
        let kind = extrude_kind();
        let result = dispatch_volume_mesh(
            Some(&kind),
            true, // force_tet
            true, // require_hex_wedge (should be ignored when force_tet)
            &[],  // ops — not reached before force_tet short-circuit
            &[],  // handles
            |_swept| unreachable!("gmsh_2d must not be called when force_tet=true"),
            |_params, _mesh| unreachable!("sweep_step must not be called when force_tet=true"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result, Ok(VolumeMeshOutcome::Tet(_))),
            "force_tet=true must return Tet regardless of swept_kind; got {result:?}"
        );
    }

    // ── Step-5: None swept_kind + !require_hex_wedge → tet fallback ──────

    #[test]
    fn dispatch_no_swept_kind_returns_tet_when_not_require_hex_wedge() {
        let result = dispatch_volume_mesh(
            None,
            false, // force_tet
            false, // require_hex_wedge
            &[],   // ops
            &[],   // handles
            |_swept| unreachable!("gmsh_2d must not be called when swept_kind=None"),
            |_params, _mesh| unreachable!("sweep_step must not be called when swept_kind=None"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result, Ok(VolumeMeshOutcome::Tet(_))),
            "swept_kind=None + require_hex_wedge=false must return Tet; got {result:?}"
        );
    }

    // ── Step-7: None swept_kind + require_hex_wedge → error ──────────────

    #[test]
    fn dispatch_no_swept_kind_errors_when_require_hex_wedge() {
        let result = dispatch_volume_mesh(
            None,
            false, // force_tet
            true,  // require_hex_wedge
            &[],   // ops
            &[],   // handles
            |_swept| unreachable!("gmsh_2d must not be called"),
            |_params, _mesh| unreachable!("sweep_step must not be called"),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("body not swept"),
                    "error message must contain \"body not swept\"; got: {msg}"
                );
            }
            other => panic!("expected Err(OperationFailed(\"body not swept\")), got {other:?}"),
        }
    }

    // ── Step-9: Some(swept) happy path → Swept ───────────────────────────
    // Also asserts that the SweepParams delivered to sweep_step match the
    // SweptKind's fields — pins the params contract that dispatch advertises.

    #[test]
    fn dispatch_swept_kind_happy_path_returns_swept_and_pins_sweep_params() {
        let kind = extrude_kind(); // Extrude { axis: [0,0,1], length: Value::length(0.01) }
        let result = dispatch_volume_mesh(
            Some(&kind),
            false, // force_tet
            false, // require_hex_wedge
            &[],   // ops — Extrude arm does not need them
            &[],   // handles
            |_swept| Ok(make_mesh2d_report()),
            |params, _mesh| {
                // Assert that the SweepParams delivered to sweep_step are correct
                // for the Extrude arm (axis forwarded verbatim, length = 0.01).
                match params {
                    SweepParams::Extrude { axis, length } => {
                        assert_eq!(*axis, [0.0, 0.0, 1.0], "axis must be [0,0,1]");
                        assert!(
                            (length - 0.01).abs() < 1e-12,
                            "length must be 0.01; got {length}"
                        );
                    }
                    other => panic!("expected SweepParams::Extrude, got {other:?}"),
                }
                Ok(make_swept_mesh(2))
            },
            || unreachable!("tet_path must not be called on the swept happy path"),
        );
        match result {
            Ok(VolumeMeshOutcome::Swept(mesh3d)) => {
                assert_eq!(
                    mesh3d.layers, 2,
                    "swept mesh must have the layers returned by sweep_step"
                );
            }
            other => panic!("expected Ok(Swept(mesh3d)) with layers=2, got {other:?}"),
        }
    }

    // ── Step-11: swept failure + !require_hex_wedge → tet fallback ───────

    #[test]
    fn dispatch_swept_failure_falls_back_to_tet_when_not_require_hex_wedge() {
        let kind = extrude_kind();

        // Subcase A: gmsh_2d fails
        let result_a = dispatch_volume_mesh(
            Some(&kind),
            false,
            false,
            &[],
            &[], // ops, handles
            |_swept| Err(Mesh2dError::DegenerateBoundary),
            |_params, _mesh| unreachable!("sweep_step must not be called when gmsh_2d fails"),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result_a, Ok(VolumeMeshOutcome::Tet(_))),
            "gmsh_2d failure + require_hex_wedge=false must fall back to Tet; got {result_a:?}"
        );

        // Subcase B: sweep_step fails
        let result_b = dispatch_volume_mesh(
            Some(&kind),
            false,
            false,
            &[],
            &[], // ops, handles
            |_swept| Ok(make_mesh2d_report()),
            |_params, _mesh| Err(SweepError::DegenerateAxis),
            || Ok(make_empty_volume_mesh()),
        );
        assert!(
            matches!(result_b, Ok(VolumeMeshOutcome::Tet(_))),
            "sweep_step failure + require_hex_wedge=false must fall back to Tet; got {result_b:?}"
        );
    }

    // ── Step-13: swept failure + require_hex_wedge → error ───────────────

    #[test]
    fn dispatch_swept_failure_errors_when_require_hex_wedge() {
        let kind = extrude_kind();

        // Subcase A: gmsh_2d fails
        let result_a = dispatch_volume_mesh(
            Some(&kind),
            false,
            true, // require_hex_wedge
            &[],
            &[], // ops, handles
            |_swept| Err(Mesh2dError::DegenerateBoundary),
            |_params, _mesh| unreachable!("sweep_step must not be called when gmsh_2d fails"),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result_a {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("swept hex/wedge path failed"),
                    "subcase A error must contain \"swept hex/wedge path failed\"; got: {msg}"
                );
            }
            other => panic!("subcase A: expected Err(OperationFailed), got {other:?}"),
        }

        // Subcase B: sweep_step fails
        let result_b = dispatch_volume_mesh(
            Some(&kind),
            false,
            true, // require_hex_wedge
            &[],
            &[], // ops, handles
            |_swept| Ok(make_mesh2d_report()),
            |_params, _mesh| Err(SweepError::DegenerateMagnitude),
            || unreachable!("tet_path must not be called when require_hex_wedge errors"),
        );
        match result_b {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("swept hex/wedge path failed"),
                    "subcase B error must contain \"swept hex/wedge path failed\"; got: {msg}"
                );
            }
            other => panic!("subcase B: expected Err(OperationFailed), got {other:?}"),
        }
    }

    #[allow(dead_code, unreachable_code)]
    fn _surface_pin() {
        // Name both variants — a rename or variant removal breaks compilation.
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Tet(todo!()); // ptodo:allow exhaustiveness/stub arm - not tracked debt
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Swept(todo!()); // ptodo:allow exhaustiveness/stub arm - not tracked debt
        // Verify the full signature including the new ops/handles slice parameters
        // via function-item to function-pointer coercion.
        type DispatchVolumeMeshFn = fn(
            Option<&SweptKind>,
            bool,
            bool,
            &[GeometryOp],
            &[GeometryHandleId],
            fn(&SweptKind) -> Result<Mesh2dReport, Mesh2dError>,
            fn(&SweepParams, &Mesh2d) -> Result<SweptMesh3d, SweepError>,
            fn() -> Result<VolumeMesh, GeometryError>,
        ) -> Result<VolumeMeshOutcome, GeometryError>;
        let _: DispatchVolumeMeshFn = dispatch_volume_mesh::<_, _, _>;
    }
}

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
// Task 2947 follow-up (integration test): once this helper is wired into the
// engine's realization pipeline by VolumeMesh realization wiring (task 2947,
// pending at time of writing), add an end-to-end test that runs a P2 elastic
// solve on a scene with at least two qualifying swept bodies and asserts exactly
// one `Severity::Info` diagnostic per body (not zero, not two). The unit tests
// below exercise the helper's contract but cannot verify the one-shot guarantee
// at the call-site level.
//
// Previously cited task 2989 (volume-mesh integration); 2989 closed without
// wiring this helper, so the live blocker is now 2947.
#[allow(dead_code)] // production wiring blocked on task 2947 (VolumeMesh realization wiring, pending at time of writing)
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
mod p2_substitution_diagnostic_tests {
    use super::*;
    use reify_core::Severity;
    use reify_ir::ElementOrderTag;

    fn extrude_kind() -> crate::sweep_classifier::SweptKind {
        use reify_ir::Value;
        crate::sweep_classifier::SweptKind::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: Value::length(0.01),
        }
    }

    #[test]
    fn p2_substitution_happy_path_extrude_emits_info_diagnostic() {
        let kind = extrude_kind();
        let result = p2_substitution_diagnostic(
            Some(&kind),
            false, // force_tet
            ElementOrderTag::P2,
            "B1",
        );
        let diag = result.expect("expected Some(Diagnostic) for qualifying body with P2");
        assert_eq!(
            diag.severity,
            Severity::Info,
            "diagnostic must have Info severity"
        );
        assert_eq!(
            diag.message,
            "Body B1 qualified for hex/wedge meshing; P1 hex used despite `element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet.",
            "diagnostic message must match PRD wording verbatim"
        );
    }

    /// Suppression cases: each of the three gating conditions independently
    /// disables diagnostic emission and returns `None`.
    ///
    /// (a) element_order = P1 — no substitution happening, nothing to warn about.
    /// (b) force_tet = true — hex/wedge was suppressed by the caller; PRD states
    ///     "Diagnostic is suppressed under `force_tet = true`".
    /// (c) swept_kind = None — body doesn't qualify for hex/wedge promotion.
    #[test]
    fn p2_substitution_suppression_cases_return_none() {
        let kind = extrude_kind();

        // (a) P1 element order — no substitution, no diagnostic.
        assert!(
            p2_substitution_diagnostic(Some(&kind), false, ElementOrderTag::P1, "B_P1").is_none(),
            "(a) element_order=P1 must return None"
        );

        // (b) force_tet=true — hex/wedge suppressed; diagnostic must not fire.
        assert!(
            p2_substitution_diagnostic(Some(&kind), true, ElementOrderTag::P2, "B_ForceTet")
                .is_none(),
            "(b) force_tet=true must return None"
        );

        // (c) swept_kind=None — body not hex/wedge-eligible; diagnostic must not fire.
        assert!(
            p2_substitution_diagnostic(None, false, ElementOrderTag::P2, "B_NoSweep").is_none(),
            "(c) swept_kind=None must return None"
        );
    }

    /// Variant invariance: Revolve and SweepLinear swept-body types both emit
    /// the info diagnostic when the other conditions are met.
    ///
    /// This pins that the helper does NOT gate on a specific `SweptKind` variant
    /// — any future refactor that accidentally adds a variant-specific branch
    /// (e.g. only emitting for Extrude) will break this test.
    #[test]
    fn p2_substitution_variant_invariance_revolve_and_sweep_linear_emit() {
        use std::f64::consts::FRAC_PI_2;

        let revolve_kind = crate::sweep_classifier::SweptKind::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: FRAC_PI_2,
        };

        let sweep_linear_kind = crate::sweep_classifier::SweptKind::SweepLinear {
            profile: GeometryHandleId(0),
            path: GeometryHandleId(1),
        };

        // Compute expected message per PRD task #10 — identical wording for all
        // variants (only the body label differs). Using a closure rather than a
        // const so we can substitute the label while keeping the format string
        // in one place; any future drift in `p2_substitution_diagnostic`'s
        // wording will fail both assertions simultaneously.
        let expected_msg = |label: &str| -> String {
            format!(
                "Body {label} qualified for hex/wedge meshing; P1 hex used despite \
`element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet."
            )
        };

        // Revolve variant.
        let revolve_result = p2_substitution_diagnostic(
            Some(&revolve_kind),
            false,
            ElementOrderTag::P2,
            "RevolvedDisc",
        );
        let revolve_diag =
            revolve_result.expect("Revolve variant must emit Some(Diagnostic) with P2");
        assert_eq!(revolve_diag.severity, Severity::Info);
        assert_eq!(
            revolve_diag.message,
            expected_msg("RevolvedDisc"),
            "Revolve diagnostic must match PRD wording verbatim"
        );

        // SweepLinear variant.
        let sweep_result = p2_substitution_diagnostic(
            Some(&sweep_linear_kind),
            false,
            ElementOrderTag::P2,
            "SweptBar",
        );
        let sweep_diag =
            sweep_result.expect("SweepLinear variant must emit Some(Diagnostic) with P2");
        assert_eq!(sweep_diag.severity, Severity::Info);
        assert_eq!(
            sweep_diag.message,
            expected_msg("SweptBar"),
            "SweepLinear diagnostic must match PRD wording verbatim"
        );
    }
}

// ── build_mixed_region_mesh unit tests (T12 layer B) ──────────────────────────

#[cfg(test)]
mod mixed_region_tests {
    use super::*;
    use reify_ir::{ElementOrderTag, VolumeMesh};
    use reify_shell_extract::{MidSurfaceMesh, ShellTetInterface};

    /// Small shell mesh: 3 vertices, 1 triangle, thickness len 3. Vertex 0 sits
    /// at the origin (the unique nearest vertex to `location = [0,0,0]`).
    fn make_shell_mesh() -> MidSurfaceMesh {
        MidSurfaceMesh {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            thickness: vec![0.1, 0.1, 0.1],
        }
    }

    /// Tet mesh for interface tying: a through-thickness triple straddling the
    /// origin along +z (top z=+1, mid z=0, bot z=−1) plus a far 4th node that
    /// is excluded from the 3 nearest to `location`.
    fn make_tie_tet_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 1.0, // node 0 — top (z = +1)
                0.0, 0.0, 0.0, // node 1 — mid (z =  0, at location)
                0.0, 0.0, -1.0, // node 2 — bot (z = −1)
                9.0, 9.0, 9.0, // node 3 — far (not among the 3 nearest)
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    /// Small P1 tet mesh: 4 vertices = 1 tet (placed a unit above the shell).
    fn make_p1_tet_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 1.0, // node 0
                1.0, 0.0, 1.0, // node 1
                0.0, 1.0, 1.0, // node 2
                0.0, 0.0, 2.0, // node 3
            ],
            tet_indices: vec![0, 1, 2, 3],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    // ── Step 9: unified-mesh merge (no MPCs) ─────────────────────────────────

    /// Merging a shell mesh and a tet mesh with no interfaces concatenates the
    /// node lists (shell first, tet appended as f64), emits one `Shell` element
    /// per triangle and one `Tet` element per tet (connectivity offset by the
    /// shell node count), and produces no MPC rows.
    #[test]
    fn build_mixed_region_mesh_merges_shell_then_tet_nodes_and_elements() {
        let shell = make_shell_mesh();
        let tet = make_p1_tet_mesh();
        let result = build_mixed_region_mesh(&shell, &tet, &[])
            .expect("merge with no interfaces should succeed");

        let n_shell = shell.vertices.len(); // 3
        let n_tet = tet.vertices.len() / 3; // 4
        assert_eq!(
            result.nodes.len(),
            n_shell + n_tet,
            "merged node count = shell vertices + tet vertices"
        );
        // Shell nodes preserved verbatim, first.
        assert_eq!(result.nodes[0], [0.0, 0.0, 0.0]);
        assert_eq!(result.nodes[1], [1.0, 0.0, 0.0]);
        assert_eq!(result.nodes[2], [0.0, 1.0, 0.0]);
        // Tet vertices appended (f32 → f64) after the shell nodes.
        assert_eq!(result.nodes[n_shell], [0.0, 0.0, 1.0]);
        assert_eq!(result.nodes[n_shell + 3], [0.0, 0.0, 2.0]);

        // Elements: 1 shell triangle + 1 tet.
        assert_eq!(result.elements.len(), 2, "one shell + one tet element");
        let shell_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Shell)
            .collect();
        assert_eq!(shell_elems.len(), 1, "one shell element");
        assert_eq!(
            shell_elems[0].connectivity,
            vec![0usize, 1, 2],
            "shell connectivity = triangle vertex indices"
        );
        let tet_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Tet)
            .collect();
        assert_eq!(tet_elems.len(), 1, "one tet element");
        assert_eq!(
            tet_elems[0].connectivity,
            vec![n_shell, n_shell + 1, n_shell + 2, n_shell + 3],
            "tet connectivity offset by n_shell_nodes"
        );

        // No interfaces → no MPC rows.
        assert!(result.mpc_rows.is_empty(), "no interfaces → no MPC rows");
    }

    /// Empty shell + empty tet + no interfaces → an all-empty `MixedRegionMesh`.
    #[test]
    fn build_mixed_region_mesh_on_empty_inputs_is_all_empty() {
        let empty_shell = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let empty_tet = VolumeMesh {
            vertices: vec![],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let result = build_mixed_region_mesh(&empty_shell, &empty_tet, &[])
            .expect("empty merge should succeed");
        assert!(result.nodes.is_empty(), "no nodes");
        assert!(result.elements.is_empty(), "no elements");
        assert!(result.mpc_rows.is_empty(), "no MPC rows");
    }

    // ── Step 11: interface MPC wiring (D=6 layout) ───────────────────────────

    /// One interface ties the nearest shell vertex to the resolved tet
    /// top/mid/bot triple, producing `MpcRow::shell_tet_tying`'s 6 rows under
    /// the global D=6 DOF layout (`6·node + axis`, shell nodes first, tet nodes
    /// offset by `n_shell`).
    #[test]
    fn build_mixed_region_mesh_wires_interface_mpc_rows_in_d6_layout() {
        let shell = make_shell_mesh();
        let tet = make_tie_tet_mesh();
        let n_shell = shell.vertices.len(); // 3
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let result = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&interface))
            .expect("interface wiring should succeed");

        // shell_tet_tying emits exactly 6 rows.
        assert_eq!(result.mpc_rows.len(), 6, "one interface → 6 MPC rows");

        // Resolved tie nodes (unified indices) under the fixture geometry:
        //   shell tie node = nearest shell vertex to [0,0,0] = shell node 0.
        //   tet top/mid/bot = tet locals 0/1/2 (z = +1/0/−1) → unified 3/4/5.
        let shell_n = 0usize;
        let tet_mid = n_shell + 1; // 4

        // The three displacement-matching rows: u_shell_a − u_tet_mid_a = 0,
        // pivot at the shell disp DOF (6·shell_n + a) with coeffs [1, −1] and
        // the tet-mid disp DOF (6·tet_mid + a) as the second term.
        for a in 0..3 {
            let shell_disp = 6 * shell_n + a; // 0,1,2
            let tet_mid_dof = 6 * tet_mid + a; // 24,25,26
            let row = result
                .mpc_rows
                .iter()
                .find(|r| r.dofs == vec![shell_disp, tet_mid_dof])
                .unwrap_or_else(|| {
                    panic!(
                        "missing displacement row for axis {a}: dofs [{shell_disp}, {tet_mid_dof}]"
                    )
                });
            assert_eq!(
                row.coeffs,
                vec![1.0, -1.0],
                "displacement-matching row coeffs for axis {a}"
            );
            assert_eq!(row.rhs, 0.0, "homogeneous tie (rhs = 0) for axis {a}");
        }

        // D=6 sanity: every DOF index lies in the unified 6·node space.
        let n_total = result.nodes.len(); // 7
        for row in &result.mpc_rows {
            for &d in &row.dofs {
                assert!(
                    d < 6 * n_total,
                    "DOF {d} must lie within the D=6 space (6 · {n_total} nodes)"
                );
            }
        }

        // The rotation rows must reference the shell tie node's rotation DOFs
        // (6·shell_n + 3..5) — confirming the shell side contributes rotations.
        let shell_rot: Vec<usize> = (3..6).map(|axis| 6 * shell_n + axis).collect(); // [3,4,5]
        assert!(
            result
                .mpc_rows
                .iter()
                .any(|r| r.dofs.iter().any(|d| shell_rot.contains(d))),
            "a rotation row must reference a shell rotation DOF (6·n + 3..5)"
        );
    }

    /// An interface whose tet side has no nodes cannot resolve its tie nodes →
    /// `InterfaceResolutionFailed` tagged with the interface index.
    #[test]
    fn build_mixed_region_mesh_errors_when_interface_tie_nodes_unresolvable() {
        let shell = make_shell_mesh();
        let empty_tet = VolumeMesh {
            vertices: vec![],
            tet_indices: vec![],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let err = build_mixed_region_mesh(&shell, &empty_tet, std::slice::from_ref(&interface))
            .expect_err("an interface against an empty tet mesh must fail to resolve");
        assert_eq!(
            err,
            MixedRegionError::InterfaceResolutionFailed { interface_index: 0 },
            "error must name the offending interface index"
        );
    }

    // ── Amendment: P2 chunking + additional error-path coverage ──────────────

    /// A P2 tet (10 nodes/element) is chunked by 10 and offset by `n_shell`,
    /// exercising the `ElementOrderTag::P2` branch of `nodes_per_tet` that the
    /// P1 fixtures leave uncovered. Two tets confirm both the chunk size and the
    /// per-element offset.
    #[test]
    fn build_mixed_region_mesh_chunks_p2_tet_by_ten_nodes() {
        let shell = make_shell_mesh();
        let n_shell = shell.vertices.len(); // 3
        // 20 tet vertices = two P2 tets; the positions are irrelevant to the
        // connectivity chunking under test (kept clear of any interface).
        let mut vertices = Vec::new();
        for i in 0..20 {
            vertices.push(i as f32);
            vertices.push(0.0);
            vertices.push(5.0);
        }
        let tet = VolumeMesh {
            vertices,
            tet_indices: (0..20u32).collect(),
            element_order: ElementOrderTag::P2,
            normals: None,
        };

        let result = build_mixed_region_mesh(&shell, &tet, &[])
            .expect("P2 merge with no interfaces should succeed");

        assert_eq!(result.nodes.len(), n_shell + 20, "3 shell + 20 tet nodes");
        let tet_elems: Vec<&UnifiedElement> = result
            .elements
            .iter()
            .filter(|e| e.kind == UnifiedElementKind::Tet)
            .collect();
        assert_eq!(tet_elems.len(), 2, "20 P2 indices → two 10-node tets");
        assert_eq!(
            tet_elems[0].connectivity,
            (0..10).map(|m| n_shell + m).collect::<Vec<usize>>(),
            "first P2 tet: local nodes 0..10 offset by n_shell",
        );
        assert_eq!(
            tet_elems[1].connectivity,
            (10..20).map(|m| n_shell + m).collect::<Vec<usize>>(),
            "second P2 tet: local nodes 10..20 offset by n_shell",
        );
    }

    /// An interface against an empty-shell + non-empty-tet mesh cannot resolve
    /// its shell tie node (`nearest_node_index` over zero shell nodes is `None`)
    /// → `InterfaceResolutionFailed`. Complements the empty-tet case by covering
    /// the shell-side `None` branch.
    #[test]
    fn build_mixed_region_mesh_errors_when_shell_side_empty() {
        let empty_shell = MidSurfaceMesh {
            vertices: vec![],
            triangles: vec![],
            thickness: vec![],
        };
        let tet = make_tie_tet_mesh();
        let interface = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };

        let err = build_mixed_region_mesh(&empty_shell, &tet, std::slice::from_ref(&interface))
            .expect_err("an interface against an empty shell mesh must fail to resolve");
        assert_eq!(
            err,
            MixedRegionError::InterfaceResolutionFailed { interface_index: 0 },
            "empty shell side → no candidate tie node",
        );
    }

    /// An interface whose geometry violates `MpcRow::shell_tet_tying`'s
    /// preconditions (non-unit `normal`, or non-positive `thickness`) is
    /// surfaced as `InvalidInterfaceGeometry` instead of panicking in the
    /// downstream assertion. Guards the seam's direct-call contract — the
    /// partition layer normally guarantees these invariants, but the seam is
    /// `pub(crate)` and reachable with an arbitrary `ShellTetInterface`.
    #[test]
    fn build_mixed_region_mesh_errors_on_invalid_interface_geometry() {
        let shell = make_shell_mesh();
        let tet = make_tie_tet_mesh();

        // Case 1: non-unit normal (|n| = 2) — would trip the unit-normal assert.
        let non_unit = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 2.0],
            thickness: 0.1,
            location: [0.0, 0.0, 0.0],
        };
        let err = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&non_unit))
            .expect_err("a non-unit interface normal must be rejected");
        assert_eq!(
            err,
            MixedRegionError::InvalidInterfaceGeometry { interface_index: 0 },
            "non-unit normal → InvalidInterfaceGeometry",
        );

        // Case 2: non-positive thickness — would trip the positive-thickness assert.
        let bad_thickness = ShellTetInterface {
            shell_region: 0,
            tet_region: 1,
            normal: [0.0, 0.0, 1.0],
            thickness: 0.0,
            location: [0.0, 0.0, 0.0],
        };
        let err = build_mixed_region_mesh(&shell, &tet, std::slice::from_ref(&bad_thickness))
            .expect_err("a non-positive interface thickness must be rejected");
        assert_eq!(
            err,
            MixedRegionError::InvalidInterfaceGeometry { interface_index: 0 },
            "non-positive thickness → InvalidInterfaceGeometry",
        );
    }

    // ── op_accepts_repr / classify_op_input_reprs unit tests (task 4049) ────────

    /// Pins the `(Operation, ReprKind)` input-repr classifier table for the
    /// consumer-demand backward pass (PRD §3a.4, task 4049).
    ///
    /// Asserts the following classifier contract:
    ///
    /// - Boolean* and Transform* and Pattern* accept BOTH BRep and Mesh.
    /// - Modify* (Fillet/Chamfer/Shell/Draft/Thicken) and Sweep* (8 variants)
    ///   accept BRep but NOT Mesh.
    /// - `Operation::Convert { from: ReprKind::BRep }` is classified (accepts
    ///   at least one repr).
    ///
    /// RED before step-2 impl: `op_accepts_repr` / `classify_op_input_reprs`
    /// do not exist yet.
    #[test]
    fn op_accepts_repr_classifier_table() {
        use reify_ir::{Operation, ReprKind};

        // ── Boolean* ─────────────────────────────────────────────────────────
        for bool_op in [
            Operation::BooleanUnion,
            Operation::BooleanDifference,
            Operation::BooleanIntersection,
        ] {
            assert!(
                op_accepts_repr(&bool_op, ReprKind::BRep),
                "{bool_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&bool_op, ReprKind::Mesh),
                "{bool_op:?} must accept Mesh"
            );
        }

        // ── Modify* — BRep-only consumer ─────────────────────────────────────
        for mod_op in [
            Operation::ModifyFillet,
            Operation::ModifyChamfer,
            Operation::ModifyShell,
            Operation::ModifyDraft,
            Operation::ModifyThicken,
            Operation::ModifyZoneSlab,
            Operation::ModifyOffsetSolid,
        ] {
            assert!(
                op_accepts_repr(&mod_op, ReprKind::BRep),
                "{mod_op:?} must accept BRep"
            );
            assert!(
                !op_accepts_repr(&mod_op, ReprKind::Mesh),
                "{mod_op:?} must NOT accept Mesh (BRep-only consumer)"
            );
        }

        // ── Sweep* — BRep-only consumer ──────────────────────────────────────
        for sweep_op in [
            Operation::SweepLoft,
            Operation::SweepExtrude,
            Operation::SweepRevolve,
            Operation::SweepSweep,
            Operation::SweepExtrudeSymmetric,
            Operation::SweepSweepGuided,
            Operation::SweepLoftGuided,
            Operation::SweepPipe,
        ] {
            assert!(
                op_accepts_repr(&sweep_op, ReprKind::BRep),
                "{sweep_op:?} must accept BRep"
            );
            assert!(
                !op_accepts_repr(&sweep_op, ReprKind::Mesh),
                "{sweep_op:?} must NOT accept Mesh (BRep-only consumer)"
            );
        }

        // ── Transform* ───────────────────────────────────────────────────────
        for transform_op in [
            Operation::TransformTranslate,
            Operation::TransformRotate,
            Operation::TransformScale,
            Operation::TransformRotateAround,
        ] {
            assert!(
                op_accepts_repr(&transform_op, ReprKind::BRep),
                "{transform_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&transform_op, ReprKind::Mesh),
                "{transform_op:?} must accept Mesh"
            );
        }

        // ── Pattern* ─────────────────────────────────────────────────────────
        for pattern_op in [
            Operation::PatternLinear,
            Operation::PatternCircular,
            Operation::PatternMirror,
            Operation::PatternLinear2D,
            Operation::PatternArbitrary,
        ] {
            assert!(
                op_accepts_repr(&pattern_op, ReprKind::BRep),
                "{pattern_op:?} must accept BRep"
            );
            assert!(
                op_accepts_repr(&pattern_op, ReprKind::Mesh),
                "{pattern_op:?} must accept Mesh"
            );
        }

        // ── Convert — classified (accepts at least one repr) ─────────────────
        let convert_op = Operation::Convert {
            from: ReprKind::BRep,
        };
        assert!(
            classify_op_input_reprs(&convert_op).is_some(),
            "Convert{{from:BRep}} must be classified (Some)"
        );
    }

    /// Backward-pass tests "a" and "b" for `compute_demanded_reprs`
    /// (PRD §3a.4, task 4049).
    ///
    /// Fixture A (test a): mesh-terminal BooleanUnion → Mesh demand.
    /// One template with three named realizations:
    ///   realization "a" — Primitive Box (producer)
    ///   realization "b" — Primitive Box (producer)
    ///   realization "u" — Boolean{Union, left:Sub("a"), right:Sub("b")} (terminal)
    /// ExportFormat::Stl (mesh sink) → demand[0][2] == Mesh.
    ///
    /// Fixture B (test b): union-then-Fillet → BRep on union, Mesh on fillet.
    /// Extends fixture A by adding:
    ///   realization "f" — Modify{Fillet, target:Sub("u")} (terminal, mesh sink)
    /// demand[0][2] (union) == BRep (its consumer Fillet is BRep-only).
    /// demand[0][3] (fillet) == Mesh (terminal, mesh sink).
    ///
    /// Also asserts shape alignment with compute_demanded_tols (same
    /// [t_idx][r_idx] outer/inner lengths).
    ///
    /// RED before step-6: `compute_demanded_reprs` does not exist.
    #[test]
    fn compute_demanded_reprs_mesh_terminal_and_fillet_consumer() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        // Shared primitive op used as a leaf source.
        let prim_box = || CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };

        // ── Fixture A: single template, three realizations (a, b, u) ─────────
        let template_a = TopologyTemplateBuilder::new("EntityA")
            .realization_named("EntityA_a", 0, "a", vec![prim_box()])
            .realization_named("EntityA_b", 1, "b", vec![prim_box()])
            .realization_named(
                "EntityA_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .build();
        let module_a = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_a"))
            .template(template_a)
            .build();

        // ── Test a: mesh sink → terminal BooleanUnion demands Mesh ───────────
        let result_a = engine.compute_demanded_reprs(&module_a, ExportFormat::Stl);
        assert_eq!(
            result_a.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result_a[0].len(), 3, "template has 3 realizations");
        // demanded_tols alignment: same shape
        assert_eq!(
            result_a.len(),
            engine.compute_demanded_tols(&module_a).len(),
            "outer length must match compute_demanded_tols"
        );
        assert_eq!(
            result_a[0].len(),
            engine.compute_demanded_tols(&module_a)[0].len(),
            "inner length must match compute_demanded_tols"
        );
        assert_eq!(
            result_a[0][2],
            ReprKind::Mesh,
            "terminal BooleanUnion under Stl (mesh sink) must demand Mesh"
        );

        // ── Fixture B: extend with Fillet consuming the union ─────────────────
        let template_b = TopologyTemplateBuilder::new("EntityB")
            .realization_named("EntityB_a", 0, "a", vec![prim_box()])
            .realization_named("EntityB_b", 1, "b", vec![prim_box()])
            .realization_named(
                "EntityB_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .realization_named(
                "EntityB_f",
                3,
                "f",
                vec![CompiledGeometryOp::Modify {
                    kind: ModifyKind::Fillet,
                    target: GeomRef::Sub("u".to_string()),
                    args: vec![],
                }],
            )
            .build();
        let module_b = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_b"))
            .template(template_b)
            .build();

        // ── Test b ────────────────────────────────────────────────────────────
        let result_b = engine.compute_demanded_reprs(&module_b, ExportFormat::Stl);
        assert_eq!(
            result_b.len(),
            1,
            "outer Vec must have one entry per template"
        );
        assert_eq!(result_b[0].len(), 4, "template has 4 realizations");
        assert_eq!(
            result_b[0][2],
            ReprKind::BRep,
            "BooleanUnion whose consumer (Fillet) is BRep-only must demand BRep"
        );
        assert_eq!(
            result_b[0][3],
            ReprKind::Mesh,
            "terminal Fillet under Stl (mesh sink) must demand Mesh"
        );
    }

    /// Conservative-default test (task 4049 test "c", PRD §3a.4).
    ///
    /// Fixture: one template with two named realizations:
    ///   realization "a" — Primitive Box (producer)
    ///   realization "consumer" — Boolean{Union, left:Sub("a"), right:Sub("missing")}
    ///
    /// "missing" names no realization → unresolvable downstream reference.
    /// This exercises the PRD §3a.4 "Default-rule conservatism" trigger
    /// (downstream realization absent from graph snapshot), which is lumped with
    /// the unclassified-op trigger in the shared conservative code path.
    ///
    /// Expected: realization "a" demands BRep (conservative), and a
    /// `tracing::debug!` event is emitted naming the unresolved reference.
    ///
    /// RED before step-8: step-6 skips unresolved refs without emitting the
    /// debug log, and realization "a" is seen as terminal → incorrectly gets
    /// Mesh demand rather than BRep.
    #[test]
    fn compute_demanded_reprs_conservative_on_unresolved_sub() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CapturingSubscriberBuilder, CompiledModuleBuilder, MockConstraintChecker,
            TopologyTemplateBuilder, prime_tracing_callsite_cache,
        };

        prime_tracing_callsite_cache();

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        let prim_box = CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };
        // "missing" is not the name of any realization → unresolved reference.
        let template = TopologyTemplateBuilder::new("EntityC")
            .realization_named("EntityC_a", 0, "a", vec![prim_box])
            .realization_named(
                "EntityC_consumer",
                1,
                "consumer",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("missing".to_string()),
                }],
            )
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_c"))
            .template(template)
            .build();

        let (subscriber, capture) = CapturingSubscriberBuilder::new(tracing::Level::DEBUG)
            .target_prefix("reify_eval::demanded_reprs")
            .build();

        let result = tracing::subscriber::with_default(subscriber, || {
            engine.compute_demanded_reprs(&module, ExportFormat::Stl)
        });

        // Realization "a" has an unresolved downstream consumer → conservative BRep.
        assert_eq!(
            result[0][0],
            ReprKind::BRep,
            "realization 'a' has an unresolved downstream ref 'missing'; \
             must demand BRep (conservative)"
        );

        // A debug event must have been emitted naming the unresolved reference.
        assert!(
            capture.count() >= 1,
            "expected at least one DEBUG event for the unresolved 'missing' reference; \
             got {count}",
            count = capture.count()
        );
        let msgs = capture.messages();
        assert!(
            msgs.iter().any(|m| m.contains("missing")),
            "DEBUG message must mention the unresolved reference name 'missing'; \
             messages: {msgs:?}"
        );
    }

    /// Strum-iterate completeness test (task 4049 test "d", PRD §9 Q10).
    ///
    /// Iterates ALL current `Operation` variants via `strum::IntoEnumIterator`
    /// and asserts every one has an explicit classifier entry. This is the
    /// standing forcing function: a future `Operation` variant auto-appears in
    /// `Operation::iter()` (via the `EnumIter` derive added in pre-1) and
    /// fails this test until consciously classified, making silent omission
    /// impossible.
    ///
    /// Mirrors `compute_demanded_reprs_mesh_terminal_and_fillet_consumer` for
    /// `ExportFormat::ThreeMF` (task 4286 step-5).  The terminal realization
    /// must demand `ReprKind::Mesh` for any mesh-sink format.
    ///
    /// RED before step-6: `ExportFormat::ThreeMF` does not exist.
    #[test]
    fn compute_demanded_reprs_three_mf_demands_mesh() {
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
        use reify_core::ModulePath;
        use reify_ir::{ExportFormat, ReprKind};
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };

        let engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);

        let prim_box = || CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        };

        // One template: Box × Box → BooleanUnion (terminal).
        let template = TopologyTemplateBuilder::new("T3mf")
            .realization_named("T3mf_a", 0, "a", vec![prim_box()])
            .realization_named("T3mf_b", 1, "b", vec![prim_box()])
            .realization_named(
                "T3mf_u",
                2,
                "u",
                vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a".to_string()),
                    right: GeomRef::Sub("b".to_string()),
                }],
            )
            .build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test_demanded_reprs_3mf"))
            .template(template)
            .build();

        let result = engine.compute_demanded_reprs(&module, ExportFormat::ThreeMF);
        assert_eq!(result.len(), 1, "one template → one outer entry");
        assert_eq!(result[0].len(), 3, "three realizations");
        assert_eq!(
            result[0][2],
            ReprKind::Mesh,
            "terminal BooleanUnion under ThreeMF (mesh sink) must demand Mesh"
        );
    }

    /// RED before step-4: `PrimitiveBox/Cylinder/Sphere/Tube` and
    /// `CurveLineSegment/Arc/Helix/InterpCurve/BezierCurve/NurbsCurve`
    /// hit the `_ => None` catch-all in step-2's impl.
    #[test]
    fn classify_op_all_variants_are_classified() {
        use reify_ir::Operation;
        use strum::IntoEnumIterator;

        for op in Operation::iter() {
            assert!(
                classify_op_input_reprs(&op).is_some(),
                "Operation::{op:?} has no explicit classifier entry — \
                 classify it BRep-vs-Mesh per PRD §3a.4 (task 4049)"
            );
        }
    }
}

// ── post_process_mechanism_mass_props unit tests (task 4472 step-7) ───────────
//
// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
// The test calls it directly to verify the engine pass iterates values and
// writes `derived_mass_props` back into mechanism cells.

#[cfg(test)]
mod post_process_mechanism_mass_props_tests {
    use std::collections::BTreeMap;

    use reify_core::identity::ValueCellId;
    use reify_core::{RealizationNodeId, Severity};
    use reify_ir::{GeometryHandleId, Value, ValueMap};
    use reify_test_support::mocks::MockGeometryKernel;

    use super::Engine;

    /// Fixed kernel handle for the geometry-backed body in these tests.
    const HANDLE_ID: GeometryHandleId = GeometryHandleId(77);

    /// Build a minimal mechanism `Value::Map`: kind="mechanism", bodies=[body]
    /// where body.solid is the given `solid_value`.
    fn one_body_mechanism(solid_value: Value) -> Value {
        let mut body = BTreeMap::new();
        body.insert(Value::String("id".to_string()), Value::Int(0));
        body.insert(Value::String("solid".to_string()), solid_value);

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("bodies".to_string()),
            Value::List(vec![Value::Map(body)]),
        );
        Value::Map(mech)
    }

    /// Build a `Value::GeometryHandle` for `HANDLE_ID`.
    fn geometry_handle() -> Value {
        Value::GeometryHandle {
            realization_ref: RealizationNodeId::new("Design", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: HANDLE_ID,
        }
    }

    /// Build a `MockGeometryKernel` with injected Volume / CenterOfMass /
    /// InertiaTensor replies for `HANDLE_ID` at the water-default density
    /// (1000.0 kg/m³). Volume=6.0 → mass=6000.0 kg.
    fn mock_kernel() -> MockGeometryKernel {
        let inertia = Value::List(vec![
            Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
            Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
        ]);
        MockGeometryKernel::new()
            .with_volume_result(HANDLE_ID, Value::Real(6.0))
            .with_center_of_mass_result(
                HANDLE_ID,
                1000.0,
                Value::String("{\"x\":0.1,\"y\":0.2,\"z\":0.3}".to_string()),
            )
            .with_inertia_tensor_result(HANDLE_ID, 1000.0, inertia)
    }

    /// The engine pass must iterate over a ValueMap containing a mechanism cell,
    /// call `derive_mechanism_mass_props`, and write the patched mechanism back
    /// into the ValueMap so that values.get(cell_id) yields a mechanism whose
    /// first body carries `derived_mass_props`.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_writes_derived_back_into_value_map() {
        let cell_id = ValueCellId::new("Design", "mech");
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), one_body_mechanism(geometry_handle()));

        let kernel = mock_kernel();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // The mechanism cell must now hold a patched value.
        let patched = values
            .get(&cell_id)
            .expect("mechanism cell must still be present after pass");

        // Extract the first body from the patched mechanism.
        let mech_map = match patched {
            Value::Map(m) => m,
            other => panic!("mechanism cell must be a Map, got {other:?}"),
        };
        let bodies = match mech_map.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            _ => panic!("patched mechanism missing bodies"),
        };
        assert_eq!(bodies.len(), 1, "must have exactly one body");
        let body_map = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("body must be a Map, got {other:?}"),
        };

        // The body must carry `derived_mass_props` (additive write).
        let derived = body_map
            .get(&Value::String("derived_mass_props".to_string()))
            .expect("body must carry derived_mass_props after engine pass");

        // Must be a MassProperties StructureInstance.
        let data = match derived {
            Value::StructureInstance(d) => d,
            other => panic!("derived_mass_props must be a StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "MassProperties",
            "derived_mass_props type_name must be MassProperties"
        );

        // The original `solid` GeometryHandle must still be present — additive write.
        assert!(
            body_map.contains_key(&Value::String("solid".to_string())),
            "body must still carry `solid` after additive pass"
        );

        // No diagnostics expected on the success path.
        assert!(
            diags.is_empty(),
            "no diagnostics expected on success path; got: {diags:?}"
        );
    }

    /// A ValueMap cell that is NOT a mechanism (e.g. a plain Real) must be
    /// left untouched by the pass.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_leaves_non_mechanism_cells_untouched() {
        let cell_id = ValueCellId::new("Design", "x");
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), Value::Real(42.0));

        let kernel = mock_kernel();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // Cell must still hold its original value.
        assert_eq!(
            values.get(&cell_id),
            Some(&Value::Real(42.0)),
            "non-mechanism cell must be left untouched"
        );
        assert!(diags.is_empty(), "no diagnostics for non-mechanism cells");
    }

    /// A geometry-backed body whose kernel query fails (no injected results) must
    /// cause the pass to skip that body (emit a Warning), and since no body was
    /// patched the mechanism cell is left with its original value unchanged.
    ///
    /// RED: `Engine::post_process_mechanism_mass_props` does not exist yet.
    #[test]
    fn post_process_mechanism_mass_props_emits_warning_on_kernel_failure() {
        let cell_id = ValueCellId::new("Design", "mech");
        let original = one_body_mechanism(geometry_handle());
        let mut values = ValueMap::new();
        values.insert(cell_id.clone(), original.clone());

        // Bare kernel — no replies injected, so Volume query will fail.
        let kernel = MockGeometryKernel::new();
        let mut diags = Vec::new();

        Engine::post_process_mechanism_mass_props(&mut values, &kernel, &mut diags);

        // Cell must be unchanged (no body was patched).
        assert_eq!(
            values.get(&cell_id),
            Some(&original),
            "mechanism cell must be unchanged when kernel fails for all bodies"
        );

        // A Warning diagnostic must be emitted.
        assert!(
            diags.iter().any(|d| d.severity == Severity::Warning),
            "must emit a Warning when kernel query fails; got: {diags:?}"
        );
    }
}

// ── diagnose_topology_correspondence_drops unit tests (task 4545 step-3) ─────
//
// RED: `diagnose_topology_correspondence_drops` does not exist yet.
// These tests drive the pure helper over hand-built AttributeHistory values
// to verify the expected Warning diagnostics (one per non-zero counter).
// No OCCT kernel is required — all counters are plain u32 fields.

#[cfg(test)]
mod diagnose_topology_correspondence_drops_tests {
    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::{
        AttributeHistory, BooleanOpHistoryRecords, LocalFeatureOpHistoryRecords,
        LoftOpHistoryRecords, SweepOpHistoryRecords,
    };

    use super::diagnose_topology_correspondence_drops;

    /// Helper: call the helper and return the collected diagnostics.
    fn run(history: &AttributeHistory) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        diagnose_topology_correspondence_drops(history, "test-context", &mut diags);
        diags
    }

    /// Boolean silent_drop_count > 0 → exactly one Warning with
    /// TopologyCorrespondenceDropped and the count in the message.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn boolean_silent_drop_emits_one_warning() {
        let history = AttributeHistory::Boolean(BooleanOpHistoryRecords {
            silent_drop_count: 3,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic; got: {diags:?}"
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        assert!(
            d.message.contains("silent_drop_count=3"),
            "message should contain 'silent_drop_count=3'; got: {:?}",
            d.message
        );
        assert!(
            d.message.to_lowercase().contains("bool")
                || d.message.to_lowercase().contains("boolean"),
            "message should name the op kind; got: {:?}",
            d.message
        );
    }

    /// Boolean silent_drop_count == 0 → no diagnostics.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn boolean_silent_drop_zero_emits_nothing() {
        let history = AttributeHistory::Boolean(BooleanOpHistoryRecords {
            silent_drop_count: 0,
            ..Default::default()
        });
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for zero count; got: {diags:?}"
        );
    }

    /// Extrude with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings, each with the code and the respective count.
    /// Also verifies the op_kind label is "extrude" and that each message
    /// pins the counter name alongside the count (not just a bare digit).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn extrude_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Extrude(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        // Op-kind label must be present.
        assert!(
            messages.iter().any(|m| m.contains("extrude")),
            "op_kind 'extrude' not found in any message; messages: {messages:?}"
        );
        // Each counter must be reported as `counter_name=count` — not just a
        // bare digit — so the association between name and value is pinned.
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// Revolve with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings with op_kind "revolve" and counter_name=count
    /// tokens in the messages.
    #[test]
    fn revolve_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Revolve(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("revolve")),
            "op_kind 'revolve' not found in any message; messages: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// Sweep with all three non-zero SweepOpHistoryRecords counters →
    /// exactly three Warnings with op_kind "sweep" and counter_name=count
    /// tokens in the messages.
    #[test]
    fn sweep_three_nonzero_counters_emits_three_warnings() {
        let history = AttributeHistory::Sweep(SweepOpHistoryRecords {
            silent_drop_count: 1,
            unsynthesized_profile_edge_count: 2,
            duplicate_parent_subshape_index_count: 4,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(diags.len(), 3, "expected 3 diagnostics; got: {diags:?}");
        for d in &diags {
            assert_eq!(d.severity, Severity::Warning);
            assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        }
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("sweep")),
            "op_kind 'sweep' not found in any message; messages: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("silent_drop_count=1")),
            "silent_drop_count=1 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unsynthesized_profile_edge_count=2")),
            "unsynthesized_profile_edge_count=2 not found in any message; messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("duplicate_parent_subshape_index_count=4")),
            "duplicate_parent_subshape_index_count=4 not found in any message; messages: {messages:?}"
        );
    }

    /// LocalFeature silent_drop_count > 0 → exactly one Warning with the code
    /// and count 5.
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn local_feature_silent_drop_emits_one_warning() {
        let history = AttributeHistory::LocalFeature(LocalFeatureOpHistoryRecords {
            silent_drop_count: 5,
            ..Default::default()
        });
        let diags = run(&history);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly one diagnostic; got: {diags:?}"
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code, Some(DiagnosticCode::TopologyCorrespondenceDropped));
        assert!(
            d.message.contains("silent_drop_count=5"),
            "message should contain 'silent_drop_count=5'; got: {:?}",
            d.message
        );
    }

    /// Loft → no diagnostics (LoftOpHistoryRecords has no counters by design).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn loft_emits_nothing() {
        let history = AttributeHistory::Loft(LoftOpHistoryRecords::default());
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for Loft; got: {diags:?}"
        );
    }

    /// AttributeHistory::None → no diagnostics (zero-cost no-op).
    ///
    /// RED until step-4 adds the helper.
    #[test]
    fn none_emits_nothing() {
        let history = AttributeHistory::None;
        let diags = run(&history);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for None; got: {diags:?}"
        );
    }
}
