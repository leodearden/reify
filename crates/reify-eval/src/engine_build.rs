// Split from lib.rs (task 2032) — build methods.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use reify_compiler::CompiledModule;
use reify_solver_elastic::{
    Mesh2d, Mesh2dError, Mesh2dReport, SweepError, SweepParams, SweptMesh3d,
};
use reify_types::{
    AttributeHistory, CapabilityDescriptor, CompiledFunction, Diagnostic, DiagnosticLabel,
    ErrorRef, ExportFormat, FeatureId, FeatureTag, FeatureTagTable, Freshness, GeometryError,
    GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, LoftOpHistoryRecords, Mesh,
    Operation, RealizationNodeId, ReprKind, SourceSpan, SweepOpHistoryRecords, TopologyAttribute,
    TopologyAttributeTable, ValueMap, VersionId, VolumeMesh,
};

use crate::cache::{CacheStore, CachedResult, FAILED_REALIZATION_STUB_HANDLE, NodeCache, NodeId};
use crate::deps::DependencyTrace;
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
    populate_sweep_attributes,
};
use crate::{BuildResult, Engine, TessellateResult};

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
struct RealizationOutputs<'a> {
    step_handles: &'a mut Vec<GeometryHandleId>,
    named_steps: &'a mut HashMap<String, GeometryHandleId>,
    feature_tag_table: &'a mut FeatureTagTable,
    topology_attribute_table: &'a mut TopologyAttributeTable,
    swept_kind_table: &'a mut SweptKindTable,
}

impl<'a> RealizationOutputs<'a> {
    /// Positional constructor mirroring struct-declaration field order
    /// (tasks 3119 + 3133).  Call sites don't need to repeat field names;
    /// argument order is fixed by the struct definition.  Line count at
    /// each call site is unchanged from struct-literal form — the trade-off
    /// is fewer redundant identifiers vs. the named-field self-documentation
    /// of struct-literal syntax.
    fn new(
        step_handles: &'a mut Vec<GeometryHandleId>,
        named_steps: &'a mut HashMap<String, GeometryHandleId>,
        feature_tag_table: &'a mut FeatureTagTable,
        topology_attribute_table: &'a mut TopologyAttributeTable,
        swept_kind_table: &'a mut SweptKindTable,
    ) -> Self {
        Self {
            step_handles,
            named_steps,
            feature_tag_table,
            topology_attribute_table,
            swept_kind_table,
        }
    }
}

/// Task 3441: seed compound-key entries `<sub>.<member> → handle` from each
/// non-collection sub's completed snapshot in `module_named_steps`.  No
/// entries are produced for collection subs (the compile-side already blocks
/// collection-sub geometry composition) or for subs whose child template
/// hasn't been realised yet (forward-declared / recursive); those cases fall
/// through to the `geometry_ops.rs::resolve_geom_ref` "unresolvable
/// GeomRef::Sub" runtime error path.
///
/// Extracted (amendment) to the single source of truth for the three eval
/// loop sites (`build`, `build_snapshot`, `tessellate_from_values`) — keeps
/// any future change (e.g. handling collection subs, narrowing the snapshot,
/// emitting forward-ref diagnostics) a one-place edit instead of three.
///
/// **Same-template aliasing note (v0.1 limitation).** Because the registry
/// keys by sub's *structure* (`sub.structure_name`), two subs of the same
/// child template share a single set of handles — `sub a = Inner(); sub b =
/// Inner();` makes `a.body` and `b.body` resolve to identical kernel handles.
/// Likewise, per-instance args on a sub do NOT affect the cross-sub view (the
/// child template is realised once with default param values).  Pinned by
/// the `cross_sub_same_template_subs_share_handle*` regression tests in
/// `cross_sub_geometry_e2e.rs`.
fn seed_cross_sub_named_steps(
    template: &reify_compiler::TopologyTemplate,
    module_named_steps: &HashMap<String, HashMap<String, GeometryHandleId>>,
    named_steps: &mut HashMap<String, GeometryHandleId>,
) {
    for sub in &template.sub_components {
        if sub.is_collection {
            continue;
        }
        if let Some(child_snapshot) = module_named_steps.get(&sub.structure_name) {
            for (member, handle) in child_snapshot {
                named_steps.insert(format!("{}.{}", sub.name, member), *handle);
            }
        }
    }
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
    named_steps: HashMap<String, GeometryHandleId>,
    module_named_steps: &mut HashMap<String, HashMap<String, GeometryHandleId>>,
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
) -> Result<(), reify_types::QueryError> {
    match attribute_history {
        AttributeHistory::None => Ok(()),
        AttributeHistory::Extrude(history) => {
            let profile_handle = match geom_op {
                GeometryOp::Extrude { profile, .. } => *profile,
                _ => {
                    return Err(reify_types::QueryError::QueryFailed(format!(
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
                    return Err(reify_types::QueryError::QueryFailed(format!(
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
                    return Err(reify_types::QueryError::QueryFailed(format!(
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
                    return Err(reify_types::QueryError::QueryFailed(format!(
                        "AttributeHistory::Loft returned for non-Loft GeometryOp: {:?}",
                        geom_op
                    )));
                }
            };
            populate_loft_op(table, kernel, feature_id, profiles, result_handle, history)
        }
    }
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
) -> Result<Vec<Vec<u32>>, reify_types::QueryError> {
    let mut index_lists: Vec<Vec<u32>> = Vec::with_capacity(cap_face_indices.len());
    for &cap_idx in cap_face_indices {
        let cap_face_handle = result_faces
            .get(cap_idx as usize)
            .copied()
            .ok_or_else(|| {
                reify_types::QueryError::QueryFailed(format!(
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
) -> Result<
    (Vec<GeometryHandleId>, Vec<Vec<u32>>, Vec<Vec<u32>>),
    reify_types::QueryError,
> {
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
    Ok((result_vertices, start_cap_vertex_index_lists, end_cap_vertex_index_lists))
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
) -> Result<(), reify_types::QueryError> {
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
) -> Result<(), reify_types::QueryError> {
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
    match op {
        // Primitives — no parent handles.
        GeometryOp::Box { .. }
        | GeometryOp::Cylinder { .. }
        | GeometryOp::Sphere { .. }
        | GeometryOp::Tube { .. } => ParentHandles::Inline([z, z], 0),

        // Curve constructors — no parent handles.
        GeometryOp::LineSegment { .. }
        | GeometryOp::Arc { .. }
        | GeometryOp::Helix { .. }
        | GeometryOp::InterpCurve { .. }
        | GeometryOp::BezierCurve { .. }
        | GeometryOp::NurbsCurve { .. } => ParentHandles::Inline([z, z], 0),

        // Pipe — kernel-internal circle profile; no user-facing parent.
        GeometryOp::Pipe { .. } => ParentHandles::Inline([z, z], 0),

        // Boolean ops — both operands are parents.
        GeometryOp::Union { left, right }
        | GeometryOp::Difference { left, right }
        | GeometryOp::Intersection { left, right } => ParentHandles::Inline([*left, *right], 2),

        // Single-target shape-modifying ops — the target is the sole parent.
        GeometryOp::Fillet { target, .. }
        | GeometryOp::Chamfer { target, .. }
        | GeometryOp::Translate { target, .. }
        | GeometryOp::Rotate { target, .. }
        | GeometryOp::Scale { target, .. }
        | GeometryOp::RotateAround { target, .. }
        | GeometryOp::LinearPattern { target, .. }
        | GeometryOp::CircularPattern { target, .. }
        | GeometryOp::Mirror { target, .. }
        | GeometryOp::LinearPattern2D { target, .. }
        | GeometryOp::ArbitraryPattern { target, .. }
        // Draft's `plane` is a reference geometry / constraint, not a parent
        // whose sub-shapes propagate — analogous to SweepGuided's guide.
        | GeometryOp::Draft { target, .. }
        | GeometryOp::Thicken { target, .. }
        | GeometryOp::Shell { target, .. } => ParentHandles::Inline([*target, z], 1),

        // Single-profile sweep ops — profile only; path/spine excluded.
        // Per `populate_attribute_history` (engine_build.rs:103-114):
        // "the path/spine is not itself a parent".
        GeometryOp::Extrude { profile, .. }
        | GeometryOp::ExtrudeSymmetric { profile, .. }
        | GeometryOp::Revolve { profile, .. }
        | GeometryOp::Sweep { profile, .. }
        | GeometryOp::SweepGuided { profile, .. } => ParentHandles::Inline([*profile, z], 1),

        // Multi-profile loft ops — all profiles are parents; guides excluded.
        // Borrow the profiles vec directly to avoid a clone on every loft op.
        GeometryOp::Loft { profiles } => ParentHandles::Borrowed(profiles.as_slice()),
        GeometryOp::LoftGuided { profiles, .. } => ParentHandles::Borrowed(profiles.as_slice()),
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
    match op {
        // Primitives
        GeometryOp::Box { .. } => Operation::PrimitiveBox,
        GeometryOp::Cylinder { .. } => Operation::PrimitiveCylinder,
        GeometryOp::Sphere { .. } => Operation::PrimitiveSphere,
        GeometryOp::Tube { .. } => Operation::PrimitiveTube,

        // Booleans
        GeometryOp::Union { .. } => Operation::BooleanUnion,
        GeometryOp::Difference { .. } => Operation::BooleanDifference,
        GeometryOp::Intersection { .. } => Operation::BooleanIntersection,

        // Modify
        GeometryOp::Fillet { .. } => Operation::ModifyFillet,
        GeometryOp::Chamfer { .. } => Operation::ModifyChamfer,
        GeometryOp::Shell { .. } => Operation::ModifyShell,
        GeometryOp::Draft { .. } => Operation::ModifyDraft,
        GeometryOp::Thicken { .. } => Operation::ModifyThicken,

        // Transform
        GeometryOp::Translate { .. } => Operation::TransformTranslate,
        GeometryOp::Rotate { .. } => Operation::TransformRotate,
        GeometryOp::Scale { .. } => Operation::TransformScale,
        GeometryOp::RotateAround { .. } => Operation::TransformRotateAround,

        // Pattern
        GeometryOp::LinearPattern { .. } => Operation::PatternLinear,
        GeometryOp::CircularPattern { .. } => Operation::PatternCircular,
        GeometryOp::Mirror { .. } => Operation::PatternMirror,
        GeometryOp::LinearPattern2D { .. } => Operation::PatternLinear2D,
        GeometryOp::ArbitraryPattern { .. } => Operation::PatternArbitrary,

        // Sweep (single-profile + Pipe)
        GeometryOp::Extrude { .. } => Operation::SweepExtrude,
        GeometryOp::ExtrudeSymmetric { .. } => Operation::SweepExtrudeSymmetric,
        GeometryOp::Revolve { .. } => Operation::SweepRevolve,
        GeometryOp::Sweep { .. } => Operation::SweepSweep,
        GeometryOp::SweepGuided { .. } => Operation::SweepSweepGuided,
        GeometryOp::Pipe { .. } => Operation::SweepPipe,

        // Loft (multi-profile)
        GeometryOp::Loft { .. } => Operation::SweepLoft,
        GeometryOp::LoftGuided { .. } => Operation::SweepLoftGuided,

        // Curve constructors
        GeometryOp::LineSegment { .. } => Operation::CurveLineSegment,
        GeometryOp::Arc { .. } => Operation::CurveArc,
        GeometryOp::Helix { .. } => Operation::CurveHelix,
        GeometryOp::InterpCurve { .. } => Operation::CurveInterpCurve,
        GeometryOp::BezierCurve { .. } => Operation::CurveBezierCurve,
        GeometryOp::NurbsCurve { .. } => Operation::CurveNurbsCurve,
    }
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
#[allow(dead_code)]
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
        // Task ε (3436): resolve the engine's default kernel through the new
        // multi-handle map. Single-handle surfaces (export, post-process,
        // build pre-step-8) operate on this kernel; per-op dispatch routing
        // lands in step-8 inside `execute_realization_ops`.
        let default_kernel_name = self.default_kernel_name.clone();
        let geometry_output = if let Some(name) = default_kernel_name.as_deref()
            && let Some(kernel) = self.geometry_kernels.get_mut(name)
        {
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

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
            let mut module_named_steps: HashMap<String, HashMap<String, GeometryHandleId>> =
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
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
                seed_cross_sub_named_steps(template, &module_named_steps, &mut named_steps);
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
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
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
                        ),
                        &mut diagnostics,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                        &mut self.realization_cache,
                        demanded_tol,
                    );
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
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in `build` and
                // `tessellate_from_values` — keep all four call sites in
                // sync (follow-up: the broader build/build_snapshot
                // realization-loop duplication is tracked separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_ref(),
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
                    kernel.as_ref(),
                    &mut diagnostics,
                );
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_mut(),
                    &self.topology_attribute_table,
                    &mut diagnostics,
                );
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
        let geometry_output = if let Some(name) = default_kernel_name.as_deref()
            && let Some(kernel) = self.geometry_kernels.get_mut(name)
        {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let had_realization_ops = module
                .templates
                .iter()
                .flat_map(|t| &t.realizations)
                .any(|r| !r.operations.is_empty());

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
            let mut module_named_steps: HashMap<String, HashMap<String, GeometryHandleId>> =
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
                let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
                seed_cross_sub_named_steps(template, &module_named_steps, &mut named_steps);
                for (r_idx, realization) in template.realizations.iter().enumerate() {
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
                    Engine::execute_realization_ops(
                        kernel.as_mut(),
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
                        ),
                        &mut diagnostics,
                        &realization.id,
                        realization.name.as_deref(),
                        realization.span,
                        &mut kernel_error,
                        &mut self.realization_cache,
                        demanded_tol,
                    );
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
                // Task 2320: see `Engine::post_process_conformance_queries`
                // docstring for the full contract. Mirrored in
                // `build_snapshot` and `tessellate_from_values` — keep all
                // four call sites in sync (follow-up: the broader
                // build/build_snapshot realization-loop duplication is
                // tracked separately).
                Engine::post_process_conformance_queries(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_ref(),
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
                    kernel.as_ref(),
                    &mut diagnostics,
                );
                Engine::run_post_processes(
                    template,
                    &named_steps,
                    &mut values,
                    kernel.as_mut(),
                    &self.topology_attribute_table,
                    &mut diagnostics,
                );
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
            values,
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
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernels,
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
        match dispatch(registry, op, demanded, available) {
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
        default_kernel_name: Option<&str>,
        module: &CompiledModule,
        values: &mut ValueMap,
        functions: &[CompiledFunction],
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
        feature_tag_table: &mut FeatureTagTable,
        topology_attribute_table: &mut TopologyAttributeTable,
        swept_kind_table: &mut SweptKindTable,
        realization_cache: &mut RealizationCache<GeometryHandleId>,
        demanded_tols: &[Vec<Option<f64>>],
        tessellation_budgets: &[Vec<f64>],
    ) -> Vec<(String, Mesh)> {
        let mut meshes = Vec::new();

        // Task ε (3436): the engine's default kernel is fetched by name from
        // the multi-handle map; `None` matches the v0.2 "no kernel
        // configured" semantics.
        let kernel = match default_kernel_name.and_then(|n| geometry_kernels.get_mut(n)) {
            Some(k) => k,
            None => return meshes,
        };

        let mut step_handles: Vec<GeometryHandleId> = Vec::new();
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
        let mut module_named_steps: HashMap<String, HashMap<String, GeometryHandleId>> =
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
            let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
            seed_cross_sub_named_steps(template, &module_named_steps, &mut named_steps);
            for (r_idx, realization) in template.realizations.iter().enumerate() {
                let handle_start = step_handles.len();
                // Tessellate paths do not propagate kernel errors into
                // `Freshness::Failed` today (arch §9.1 wires that on the
                // build path only — see `Engine::build` / `Engine::build_snapshot`).
                // Pass `&mut None` so `execute_realization_ops` collects the
                // diagnostic but no caller acts on the kernel error here.
                let mut kernel_error: Option<ErrorRef> = None;
                // Task 3227 / 3297: direct positional index — no String clones,
                // no hashing. The producer (`compute_demanded_tols`) and this
                // consumer iterate the same `module.templates × realizations`
                // product unconditionally, so OOB is unambiguously an internal
                // bug; Rust's slice indexing panics with a precise OOB message
                // at runtime in both debug and release.
                let demanded_tol = demanded_tols[t_idx][r_idx];
                Engine::execute_realization_ops(
                    kernel.as_mut(),
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
                    ),
                    diagnostics,
                    &realization.id,
                    realization.name.as_deref(),
                    realization.span,
                    &mut kernel_error,
                    realization_cache,
                    demanded_tol,
                );

                // Tessellate this realization's final handle (if any new handles were produced)
                if step_handles.len() > handle_start {
                    let last_handle = step_handles[step_handles.len() - 1];
                    // Task 2874 step-12: forward the per-realization tessellation
                    // budget (precomputed by the caller via
                    // `Engine::compute_realization_tolerance_budget`) to the
                    // kernel instead of the unconditional module-pragma
                    // default. Task 3227 / 3297: direct positional index —
                    // no String clones, no hashing. The producer
                    // (`compute_tessellation_budgets`) and this consumer iterate
                    // the same `module.templates × realizations` product
                    // unconditionally, so OOB is unambiguously an internal
                    // bug; Rust's slice indexing panics with a precise OOB
                    // message at runtime in both debug and release.
                    let budget = tessellation_budgets[t_idx][r_idx];
                    match kernel.tessellate(last_handle, budget) {
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
            // Task 2320 amendment: mirrors the `build` / `build_snapshot`
            // wire-up so `TessellateResult.values` exposes the same
            // kernel-resolved `Bool` for conformance-query cells as
            // `BuildResult.values`. See
            // `Engine::post_process_conformance_queries` docstring.
            Engine::post_process_conformance_queries(
                template,
                &named_steps,
                values,
                kernel.as_ref(),
                diagnostics,
            );
            // Task 2531: see the build / build_snapshot wire-up. Tessellate
            // surface exposes the same kernel-resolved kinematic-query
            // values as the build surface so GUI overlays stay consistent.
            Engine::post_process_kinematic_queries(
                template,
                &named_steps,
                values,
                kernel.as_ref(),
                diagnostics,
            );
            Engine::run_post_processes(
                template,
                &named_steps,
                values,
                kernel.as_mut(),
                topology_attribute_table,
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
    /// **Internal-consistency invariant** (amendment): on cache-hit the
    /// helper `debug_assert!`s that neither `feature_tag_table` nor
    /// `topology_attribute_table` already carries an entry for the cached
    /// handle. Under the per-build reset contract above, that assertion
    /// must always hold (the table was just reset and only earlier
    /// realizations in the same build could have touched a different
    /// handle). The assertion fires loudly during development if a future
    /// refactor weakens the per-build reset or routes a cache-served
    /// handle through a path that ALSO populates the tables — both of
    /// which would silently regress attribute-query results for a cached
    /// handle. Production builds skip the check entirely
    /// (`debug_assertions` cfg).
    #[allow(clippy::too_many_arguments)]
    fn execute_realization_ops(
        kernel: &mut dyn GeometryKernel,
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
        realization_cache: &mut RealizationCache<GeometryHandleId>,
        demanded_tol: Option<f64>,
    ) {
        let RealizationOutputs {
            step_handles,
            named_steps,
            feature_tag_table,
            topology_attribute_table,
            swept_kind_table,
        } = outputs;
        let handle_start = step_handles.len();

        // Task 2874, step-8: cache-hit short-circuit. When the caller has
        // threaded a demanded tolerance AND the realization is named (the
        // `named_steps` contract requires a name to write into the map),
        // probe the per-engine `RealizationCache` at
        // `(entity_id, ReprKind::BRep, demanded_tol)`. On hit we push the
        // cached terminal handle, write `named_steps[name] = cached_handle`,
        // and return — preserving the post-condition the success path
        // establishes below. On miss (or when either guard is `None`) we
        // fall through to the kernel op loop, and step-6's post-success
        // insert at the bottom of the helper populates the cache for the
        // NEXT call. The lookup uses `RealizationCache`'s partial-order
        // "tighter satisfies looser" rule (`cached_tol ≤ requested_tol`),
        // so a tighter request automatically misses a looser cached entry
        // (see step-13's pin).
        if let (Some(tol), Some(name)) = (demanded_tol, realization_name)
            && let Some(&cached_handle) =
                realization_cache.lookup(&realization_id.entity, ReprKind::BRep, tol, NO_OPTIONS)
        {
            // Internal-consistency invariant (amendment): the per-build
            // reset of `feature_tag_table` / `topology_attribute_table` at
            // the top of build() / build_snapshot() / tessellate_*()
            // guarantees neither table can already carry an entry for the
            // cached handle on a clean cache-hit path. If a future refactor
            // weakens the reset or routes the cached handle through a path
            // that ALSO populates the tables, this debug_assert fires loudly
            // during development rather than silently regressing attribute-
            // query results for cached handles.
            debug_assert!(
                feature_tag_table.lookup(cached_handle).is_none(),
                "feature_tag_table already has an entry for cached handle \
                 {:?} on cache-hit short-circuit — per-build reset invariant \
                 violated",
                cached_handle,
            );
            debug_assert!(
                topology_attribute_table.lookup(cached_handle).is_none(),
                "topology_attribute_table already has an entry for cached \
                 handle {:?} on cache-hit short-circuit — per-build reset \
                 invariant violated",
                cached_handle,
            );
            step_handles.push(cached_handle);
            named_steps.insert(name.to_string(), cached_handle);
            return;
        }

        let mut had_failure = false;
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
        for (op_idx, op) in operations.iter().enumerate() {
            let geom_op = compile_geometry_op(
                op,
                values,
                &step_handles[handle_start..],
                functions,
                meta_map,
                named_steps,
                diagnostics,
            );
            match geom_op {
                Ok(geom_op) => {
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
                            step_handles.push(handle.id);
                            // Capture the compiled op parallel to step_handles for
                            // post-loop classification (task 2982). Cleared on
                            // rollback below. Pushed last in this arm so all the
                            // earlier `&geom_op` borrows above have already
                            // released — we move ownership rather than cloning.
                            realization_ops.push(geom_op);
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
                    step_handles.push(GeometryHandleId::INVALID);
                    had_failure = true;
                }
            }
        }
        // Discard intermediate handles from partially-failed realizations
        let rolled_back =
            had_failure || step_handles.len().saturating_sub(handle_start) < operations.len();
        if rolled_back {
            step_handles.truncate(handle_start);
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
            if let Some(kind) = classify_swept_body(&realization_ops, &step_handles[handle_start..])
                && let Some(&last) = step_handles[handle_start..].last()
            {
                swept_kind_table.record(last, kind);
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
                let (centroids, centroid_diags) = collect_centroids_with_failure_summary(
                    &realization_attrs,
                    kernel,
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
                    named_steps.insert(name.to_string(), last);
                }
                if let (Some(tol), Some(_name)) = (demanded_tol, realization_name) {
                    realization_cache.insert(
                        &realization_id.entity,
                        ReprKind::BRep,
                        tol,
                        NO_OPTIONS,
                        last,
                    );
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
        named_steps: &HashMap<String, GeometryHandleId>,
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
        named_steps: &HashMap<String, GeometryHandleId>,
        values: &mut ValueMap,
        kernel: &dyn GeometryKernel,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Iterate `values` directly without snapshotting (parallels the
        // `post_process_conformance_queries` sibling above). Safe because
        // none of the kinematic helpers chain — a later cell's dispatch
        // reads `args[0]` as a `ValueRef` to a Snapshot let-cell filled by
        // the regular `eval_expr` pass, never to another kinematic-query
        // cell, so an earlier patch in this loop cannot influence a later
        // dispatch's input.
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
    fn run_post_processes(
        template: &reify_compiler::TopologyTemplate,
        named_steps: &HashMap<String, GeometryHandleId>,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
        table: &TopologyAttributeTable,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        Engine::post_process_topology_selectors(template, named_steps, values, kernel, diagnostics);
        Engine::post_process_ad_hoc_selectors(
            template,
            named_steps,
            values,
            kernel,
            table,
            diagnostics,
        );
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
        named_steps: &HashMap<String, GeometryHandleId>,
        values: &mut ValueMap,
        kernel: &mut dyn GeometryKernel,
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
        named_steps: &HashMap<String, GeometryHandleId>,
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
        let state = self.eval_state.as_ref()?;

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
        self.feature_tag_table = FeatureTagTable::default();
        self.topology_attribute_table = TopologyAttributeTable::default();
        self.swept_kind_table = SweptKindTable::default();
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernels,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── execute_realization_ops unit tests ────────────────────────────────────

    /// Happy path: all operations compile and execute successfully.
    /// Appends exactly one handle and emits no diagnostics.
    #[test]
    fn execute_realization_ops_happy_path_appends_handle() {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        // Filter to error-severity only: the v0.2 topology-attribute seeder
        // (#2574) emits a Diagnostic::warning when extract_faces / extract_edges
        // fail (e.g. on a mock kernel without an extraction fixture). The
        // happy-path contract is "no Error diagnostics"; auxiliary-metadata
        // warnings are expected noise on mock kernels.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
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
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
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
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, diagnostics
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

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
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
            geometry_errors, 1,
            "expected exactly 1 geometry-error diagnostic, got {}: {:?}",
            geometry_errors, diagnostics
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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
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
            compile_failures, 1,
            "expected exactly 1 compile-error diagnostic, got {}: {:?}",
            compile_failures, diagnostics
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

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        // The Error diagnostic must contain the standard prefix (preserves
        // existing integration-test substring checks) AND the specific reason.
        let compile_err_diag = diagnostics
            .iter()
            .find(|d| {
                d.message.contains("failed to compile geometry operation")
                    && matches!(d.severity, reify_types::Severity::Error)
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "expected no error diagnostics, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder extract-failure per
        // successful primitive op). See the happy-path test for the rationale.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
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
        assert_eq!(step_handles.len(), 1, "expected one handle appended");
        let body_handle = named_steps.get("body").copied();
        assert!(
            body_handle.is_some(),
            "named_steps should contain 'body' after successful named realization"
        );
        assert_eq!(
            body_handle.unwrap(),
            step_handles[0],
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

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("bad"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        assert!(
            !named_steps.contains_key("bad"),
            "named_steps must NOT contain 'bad' after rollback; stale entries \
             would let later realizations resolve a name whose geometry was never \
             successfully produced"
        );
        // Verify rollback did happen (existing invariant)
        assert!(
            step_handles.is_empty(),
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

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

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        // First binding: let body = box(…)
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );
        // Snapshot via the contract-visible map entry, not by positional index,
        // so the snapshot stays correct if internal handle-slot layout changes.
        let h1 = named_steps["body"];

        // Second binding: let body = cylinder(…) — same name, different primitive
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &cyl_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );
        let h2 = named_steps["body"];

        // The kernel must have issued distinct handles so the test is non-trivial
        assert_ne!(
            h1, h2,
            "MockGeometryKernel must return distinct handles for distinct ops"
        );

        // Last-write-wins: named_steps["body"] must equal h2 (the cylinder binding)
        assert_eq!(
            named_steps.get("body").copied(),
            Some(h2),
            "shadowing contract: the second `let body` binding must overwrite \
             the first — named_steps[\"body\"] must be the handle from the \
             most-recent successful realization"
        );

        // Explicit anti-assertion: a first-write-wins regression must fail here
        assert_ne!(
            named_steps.get("body").copied(),
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
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "no errors expected for two valid realizations, got: {:?}",
            errors
        );
        // Pin the expected warning count: this test runs two successful
        // primitive ops (Box, then Cylinder) through the same `diagnostics`
        // Vec, so one seeder warning per op accumulates → 2 total.
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CompiledExpr, Type};

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

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();

        // First binding: let body = box(…) — succeeds, populates named_steps.
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &box_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );
        let h1 = named_steps["body"];
        // Filter to error-severity only: see comment in the happy-path test.
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "first realization must succeed cleanly, got: {:?}",
            errors
        );
        // Pin the expected warning count (one seeder failure for the
        // successful Box op). See the happy-path test for the rationale.
        let warnings_after_first: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
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
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &fail_ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            Some("body"),
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        // The failed shadow must NOT have overwritten the successful binding.
        assert_eq!(
            named_steps.get("body").copied(),
            Some(h1),
            "rollback guard: a failed shadow must not overwrite the previous \
             successful binding — named_steps[\"body\"] must still resolve to h1"
        );

        // The second call must have emitted a diagnostic (compile failure).
        assert!(
            !diagnostics.is_empty(),
            "expected a diagnostic from the failed second realization"
        );
        // Pin the warning count after the second call: the second op fails
        // before reaching `kernel.execute`, so the seeder is never invoked
        // and no NEW warning lands on top of the one from the first call.
        let warnings_after_second: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Warning))
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{Severity, SourceSpan};

        // Step(99) is out-of-bounds when step_handles is empty →
        // compile_geometry_op returns Err("unresolvable GeomRef::Step(99) …")
        let ops = vec![CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99),
            right: GeomRef::Step(99),
        }];

        let mut kernel = MockGeometryKernel::new();
        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        let realization_span = SourceSpan::new(100, 150);

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            realization_span,
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        // Find the compile-failure Error diagnostic.
        let compile_err_diag = diagnostics
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
        use reify_test_support::mocks::FailingMockGeometryKernel;
        use reify_types::{CompiledExpr, Severity, SourceSpan, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

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
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        let realization_span = SourceSpan::new(200, 250);

        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);
        Engine::execute_realization_ops(
            &mut kernel,
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            realization_span,
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        // Find the kernel-error Error diagnostic.
        let kernel_err_diag = diagnostics
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

    impl reify_types::GeometryKernel for NamedRecordingKernel {
        fn execute(
            &mut self,
            op: &reify_types::GeometryOp,
        ) -> Result<reify_types::GeometryHandle, reify_types::GeometryError> {
            self.log.lock().unwrap().push(self.name.clone());
            self.inner.execute(op)
        }

        fn query(
            &self,
            q: &reify_types::GeometryQuery,
        ) -> Result<reify_types::Value, reify_types::QueryError> {
            self.inner.query(q)
        }

        fn export(
            &self,
            handle: reify_types::GeometryHandleId,
            format: reify_types::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_types::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_types::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_types::Mesh, reify_types::TessError> {
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_types::GeometryKernel>> = BTreeMap::new();
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

        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);

        Engine::execute_realization_ops(
            &mut kernels,
            &registry,
            "default",
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
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
            step_handles.len(),
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{CapabilityDescriptor, CompiledExpr, Operation, ReprKind, Type};

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let mut kernels: BTreeMap<String, Box<dyn reify_types::GeometryKernel>> = BTreeMap::new();
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

        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);

        Engine::execute_realization_ops(
            &mut kernels,
            &registry,
            "default",
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut None,
            &mut RealizationCache::new(),
            None,
        );

        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["default".to_string()],
            "single-kernel-in-map: op must run on the default kernel; got log {:?}",
            calls,
        );
        assert_eq!(step_handles.len(), 1, "expected one handle pushed");
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reify_types::Severity::Error))
            .collect();
        assert!(
            errors.is_empty(),
            "behavior-preserved single-default path must not emit error diagnostics; got {:?}",
            errors,
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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{
            CapabilityDescriptor, CompiledExpr, DiagnosticCode, Operation, ReprKind, Type,
        };

        let mm_lit = |v: f64| CompiledExpr::literal(reify_test_support::mm(v), Type::length());

        let mut kernels: BTreeMap<String, Box<dyn reify_types::GeometryKernel>> = BTreeMap::new();
        kernels.insert(
            "default".to_string(),
            Box::new(MockGeometryKernel::new()) as Box<dyn reify_types::GeometryKernel>,
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

        let values = ValueMap::new();
        let functions: Vec<CompiledFunction> = vec![];
        let meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut step_handles: Vec<GeometryHandleId> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut named_steps: HashMap<String, GeometryHandleId> = HashMap::new();
        let mut feature_tag_table = FeatureTagTable::default();
        let mut topology_attribute_table = TopologyAttributeTable::default();
        let mut swept_kind_table = SweptKindTable::default();
        let mut kernel_error_out: Option<ErrorRef> = None;
        let test_realization_id = RealizationNodeId::new("TestEntity", 0);

        Engine::execute_realization_ops(
            &mut kernels,
            &registry,
            "default",
            &ops,
            &[],
            &values,
            &functions,
            &meta_map,
            RealizationOutputs::new(
                &mut step_handles,
                &mut named_steps,
                &mut feature_tag_table,
                &mut topology_attribute_table,
                &mut swept_kind_table,
            ),
            &mut diagnostics,
            &test_realization_id,
            None,
            SourceSpan::new(0, 0),
            &mut kernel_error_out,
            &mut RealizationCache::new(),
            None,
        );

        // A NoKernelChain error diagnostic must be emitted.
        let no_chain: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::NoKernelChain))
            .collect();
        assert_eq!(
            no_chain.len(),
            1,
            "expected exactly one NoKernelChain diagnostic when the registry has no \
             kernel for the op; got {} diagnostics total: {:?}",
            no_chain.len(),
            diagnostics
        );
        assert!(
            matches!(no_chain[0].severity, reify_types::Severity::Error),
            "NoKernelChain must be an Error-severity diagnostic; got {:?}",
            no_chain[0].severity,
        );

        // Realization must surface as Failed via the caller-write kernel_error_out
        // out-param (the same channel `mark_realization_failed` consumes for
        // kernel errors today).
        assert!(
            kernel_error_out.is_some(),
            "unroutable op must set kernel_error_out so the caller can mark the \
             realization NodeId as Failed; got None"
        );

        // step_handles must be truncated to its pre-call length: no real handle
        // was produced.
        assert!(
            step_handles.is_empty(),
            "unroutable op must leave step_handles truncated to handle_start; got {:?}",
            step_handles,
        );
    }

    // ── effective_tessellation_tolerance unit tests ──────────────────────────

    /// When `module.default_tolerance` is `Some(v)`, the helper returns `v`
    /// (in SI metres) verbatim — the module-level `#precision` pragma value
    /// overrides the engine's hardcoded default.
    #[test]
    fn effective_tessellation_tolerance_uses_module_default_when_set() {
        use reify_test_support::builders::CompiledModuleBuilder;
        use reify_types::ModulePath;

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
        use reify_test_support::builders::CompiledModuleBuilder;
        use reify_types::ModulePath;

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

    impl reify_types::GeometryKernel for RecordingTessellationKernel {
        fn execute(
            &mut self,
            op: &reify_types::GeometryOp,
        ) -> Result<reify_types::GeometryHandle, reify_types::GeometryError> {
            self.inner.execute(op)
        }

        fn query(
            &self,
            query: &reify_types::GeometryQuery,
        ) -> Result<reify_types::Value, reify_types::QueryError> {
            self.inner.query(query)
        }

        fn export(
            &self,
            handle: reify_types::GeometryHandleId,
            format: reify_types::ExportFormat,
            writer: &mut dyn std::io::Write,
        ) -> Result<(), reify_types::ExportError> {
            self.inner.export(handle, format, writer)
        }

        fn tessellate(
            &self,
            handle: reify_types::GeometryHandleId,
            tolerance: f64,
        ) -> Result<reify_types::Mesh, reify_types::TessError> {
            self.recorded_tolerances.lock().unwrap().push(tolerance);
            self.inner.tessellate(handle, tolerance)
        }
    }

    /// Build a CompiledModule with one Box-primitive realization, suitable for
    /// driving `tessellate_realizations`. Uses the same builder pattern as the
    /// fixture in `geometry_error_handling.rs::module_with_box_realization`.
    fn module_with_one_box_realization() -> reify_compiler::CompiledModule {
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};
        use reify_types::{CompiledExpr, ModulePath, Type};

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
        use reify_types::Value;

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
                    radius: Value::Real(0.001),
                },
                expected: vec![GeometryHandleId(7)],
                label: "Fillet → [target]",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: GeometryHandleId(82),
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
                op: GeometryOp::Shell {
                    target: GeometryHandleId(84),
                    thickness: Value::Real(0.002),
                    faces_to_remove: vec![0],
                },
                expected: vec![GeometryHandleId(84)],
                label: "Shell → [target]",
            },
            Case {
                op: GeometryOp::Draft {
                    target: GeometryHandleId(70),
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
        ];

        for case in &cases {
            assert_eq!(
                parent_handles_for_op(&case.op).as_slice(),
                case.expected.as_slice(),
                "parent_handles_for_op mismatch: {}",
                case.label,
            );
        }
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
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };
        use reify_types::ModulePath;

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
        use reify_types::{Operation, Value};

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
                    radius: r(0.001),
                },
                expected: Operation::ModifyFillet,
                label: "Fillet → ModifyFillet",
            },
            Case {
                op: GeometryOp::Chamfer {
                    target: h(1),
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
                },
                expected: Operation::ModifyShell,
                label: "Shell → ModifyShell",
            },
            Case {
                op: GeometryOp::Draft {
                    target: h(1),
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
        ];

        for Case {
            op,
            expected,
            label,
        } in cases
        {
            let got = geometry_op_to_operation(&op);
            assert_eq!(got, expected, "{label} (got {got:?})");
        }
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
        use reify_test_support::{
            CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder,
        };
        use reify_types::ModulePath;

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
        let mut realization_cache: RealizationCache<GeometryHandleId> = RealizationCache::new();

        // `demanded_tols = &[]` is the OOB trigger: the producer would have
        // generated `&[vec![None]]` for a 1-template/1-realization module, but
        // passing an empty slice causes `demanded_tols[0][...]` to panic.
        // `tessellation_budgets` is correctly shaped so we can confirm the
        // panic originates at the demanded_tol lookup, not the budget lookup.
        Engine::tessellate_from_values(
            &mut geometry_kernels,
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
        let mut realization_cache: RealizationCache<GeometryHandleId> = RealizationCache::new();

        // `demanded_tols` is correctly shaped; `tessellation_budgets = &[]` is
        // the OOB trigger.  The Box primitive in module_with_one_box_realization
        // produces at least one handle after `execute_realization_ops`, so
        // the `if step_handles.len() > handle_start` guard at line 1276 is true
        // and execution reaches the budget lookup.
        Engine::tessellate_from_values(
            &mut geometry_kernels,
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
        );
    }

    // ── collect_centroids_with_failure_summary unit tests ─────────────────────

    /// All handles produce kernel query errors → exactly one coalesced warning
    /// naming the count, the realization_id, and the first error message.
    #[test]
    fn collect_centroids_with_failure_summary_coalesces_query_errors() {
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{Role, Severity};

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{Role, Severity, Value};

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
        use reify_test_support::mocks::MockGeometryKernel;
        use reify_types::{Role, Severity, Value};

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
}

// ── dispatch_volume_mesh unit tests ──────────────────────────────────────────

#[cfg(test)]
mod dispatch_volume_mesh_tests {
    use super::*;
    use reify_solver_elastic::{
        Mesh2d, Mesh2dError, Mesh2dReport, SweepError, SweepParams, SweptMesh3d,
    };
    use reify_types::{ElementOrderTag, GeometryError, VolumeMesh};

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
        use reify_types::Value;
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
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Tet(todo!());
        let _: VolumeMeshOutcome = VolumeMeshOutcome::Swept(todo!());
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
    element_order: reify_types::ElementOrderTag,
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
    if element_order != reify_types::ElementOrderTag::P2 {
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
    use reify_types::{ElementOrderTag, Severity};

    fn extrude_kind() -> crate::sweep_classifier::SweptKind {
        use reify_types::Value;
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
