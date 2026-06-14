//! Topology-attribute propagation through BRepAlgoAPI history records
//! (v0.2 persistent-naming-v2, task 2590).
//!
//! After a constructive boolean op (Fuse / Cut / Common) the result shape
//! contains a mix of:
//!
//! - parent faces/edges that survived unchanged (Modified maps a parent
//!   sub-shape onto the equivalent result sub-shape);
//! - parent faces/edges that were split or transformed (also Modified, but
//!   1-to-many);
//! - newly-created faces/edges along the cut/seam (Generated, with an
//!   imaginary parent sub-shape — represented in our flat record format
//!   via a `parent_subshape_index` of the surviving parent boundary that
//!   sponsored the new sub-shape);
//! - parent faces/edges that disappeared (Deleted; no result entry).
//!
//! [`propagate_attributes_via_brepalgoapi_history`] takes the per-parent
//! attribute table populated by tasks 5-8 (or, in the foundational task 1
//! integration test, hand-seeded) and copies the parent attribute onto
//! each Modified/Generated result handle. Deleted records are skipped.
//!
//! Per v0.2 task-3: the parent's attribute is cloned and the parent-key
//! fields (`feature_id`, `role`, `local_index`, `user_label`) are
//! propagated unchanged. The `mod_history` postfix is augmented on
//! splits: when a parent has more than one same-kind result child
//! (Modified ∪ Generated, face vs edge counted independently), each
//! child is given a fresh `ModEntry { splitting_feature_id, split_index }`
//! appended to its inherited `mod_history`. Single-result parents
//! remain pure pass-through (mod_history unchanged). Per-op
//! transformation rules (e.g. "boolean cut's generated faces always
//! carry Role::NewEdge") are deferred to tasks 5-8, which will add
//! per-op variants of this helper.
//!
//! # See also
//!
//! [`crate::kernel_attribute_hook`] (file: `crates/reify-eval/src/kernel_attribute_hook.rs`)
//! covers mesh-Boolean attribute propagation via the
//! [`reify_types::KernelAttributeHook`] trait — PRD line 70's Manifold
//! `MeshGL` / `originalID` / `faceID` pattern. This module is the BRep-side
//! analogue: it covers `BRepAlgoAPI_*` Modified / Generated / Deleted
//! propagation only.

use std::collections::HashMap;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};
use reify_ir::{
    AxisSign, BooleanOpHistoryRecords, BooleanOpParents, CapKind, FeatureId, GeometryHandleId,
    HistoryRecord, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords, ModEntry, QueryError, Role,
    SweepOpHistoryRecords, TopologyAttribute, TopologyAttributeTable,
};

/// Propagate parent topology attributes onto the result of a `BRepAlgoAPI`
/// boolean operation, using the Modified / Generated / Deleted records the
/// algorithm exposes.
///
/// Inputs:
/// - `table`: the `TopologyAttributeTable` to update in place. Parent
///   entries are read; new entries are written for each Modified/Generated
///   result sub-shape whose corresponding parent had an attribute.
/// - `parents`: typed wrapper carrying the per-parent face/edge handle
///   slices in canonical TopExp order. Use [`BooleanOpParents::Binary`]
///   for binary booleans (fuse / cut / common), where `parent_index` 0 is
///   the left operand and 1 is the right operand (matching
///   [`HistoryRecord::parent_index`] semantics). Use
///   [`BooleanOpParents::NAry`] for multi-input fuse
///   (`BRepAlgoAPI_BuilderAlgo`).
/// - `result_face_handles`: the result shape's faces in canonical
///   TopExp order. Indexed by `record.result_subshape_index`. The
///   propagation writes entries to these handle ids.
/// - `result_edge_handles`: as above, but for edges.
/// - `history`: the records emitted by the FFI primitive
///   (`OcctKernelHandle::boolean_fuse_with_history`).
/// - `splitting_feature_id`: the FeatureId of the boolean op whose history
///   is being propagated. Stamped onto each `ModEntry` appended on splits;
///   single-result parents remain pure pass-through and never see this
///   value land in their `mod_history`.
///
/// Why pre-extracted vectors?
///
/// `kernel.extract_faces(handle)` / `extract_edges(handle)` allocate
/// fresh `GeometryHandleId`s on each call (the kernel does not dedupe
/// by face-equality). To make the parent attribute lookup by handle id
/// work, the caller must seed the table using the same handle vectors
/// it later passes to the propagation. Likewise, the result-face write
/// keys are the caller's chosen result-face ids — passing them in keeps
/// the function pure with respect to id allocation and lets a downstream
/// consumer (test, task-5-8 auto-population) use the same vectors to
/// inspect what was written.
///
/// Behaviour:
/// - For every Modified or Generated record (faces and edges), if the
///   parent sub-shape has an entry in `table`, clone it onto the
///   corresponding result sub-shape's handle. Parent-key fields
///   (`feature_id`, `role`, `local_index`, `user_label`) are preserved;
///   per-op transformation is task-5-8 scope per PRD.
/// - Modified/Generated children of a parent that has > 1 result
///   sub-shapes (across the same-kind Modified ∪ Generated union) inherit
///   the parent attribute's parent-key fields AND get a fresh
///   `ModEntry { splitting_feature_id: clone, split_index }` appended
///   to `mod_history`. Single-result parents remain pure pass-through
///   (mod_history unchanged). Records arriving in Modified-then-Generated
///   order yield `split_index` 0, 1, 2, … deterministically per parent.
/// - Deleted records are skipped: a deleted parent has no result entry
///   to write, and the parent's own table entry is left untouched (its
///   handle still resolves; task 4 will add diagnostics for accidental
///   rebinds).
///
/// Returns `Err(QueryError::QueryFailed)` if any record references an
/// out-of-bounds parent or result sub-shape index — the FFI primitive
/// guarantees in-range indices, so this is a defense-in-depth path
/// pinned by the unit tests below.
///
/// Cross-references PRD docs/prds/v0_2/persistent-naming-v2.md (a)+(c)+(d)
/// of decomposition-plan task 1 (lines 89-103).
pub fn propagate_attributes_via_brepalgoapi_history(
    table: &mut TopologyAttributeTable,
    parents: &BooleanOpParents<'_>,
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &BooleanOpHistoryRecords,
    splitting_feature_id: &FeatureId,
) -> Result<(), QueryError> {
    let parent_face_handles = parents.face_slices();
    let parent_edge_handles = parents.edge_slices();

    // Build a (parent_index, parent_subshape_index) → count map per kind
    // (faces vs edges) across the Modified ∪ Generated union. A count > 1
    // identifies a parent that was split, in which case each child needs
    // a fresh ModEntry appended to its mod_history.
    let face_child_counts =
        count_children_per_parent(&history.face_modified, &history.face_generated);
    let edge_child_counts =
        count_children_per_parent(&history.edge_modified, &history.edge_generated);

    // Per-parent split_index counters seeded at zero. Counts up as each
    // record for a split parent is propagated (Modified records first,
    // then Generated, in the same iteration order as the chain below).
    let mut face_split_counters: HashMap<(u8, u32), u32> = HashMap::new();
    let mut edge_split_counters: HashMap<(u8, u32), u32> = HashMap::new();

    let mut face_ctx = SplitContext::new(
        splitting_feature_id,
        &face_child_counts,
        &mut face_split_counters,
    );
    // Faces: Modified ∪ Generated.
    for record in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        propagate_one(
            table,
            parent_face_handles,
            result_face_handles,
            record,
            "face",
            &mut face_ctx,
        )?;
    }

    let mut edge_ctx = SplitContext::new(
        splitting_feature_id,
        &edge_child_counts,
        &mut edge_split_counters,
    );
    // Edges: Modified ∪ Generated.
    for record in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        propagate_one(
            table,
            parent_edge_handles,
            result_edge_handles,
            record,
            "edge",
            &mut edge_ctx,
        )?;
    }

    // Deleted records are intentionally skipped: no result sub-shape
    // exists to receive the attribute, and parents' existing table
    // entries remain valid (task 4 will add diagnostics).
    Ok(())
}

/// Propagate parent topology attributes onto the result of a `BRep` local-feature
/// operation (fillet / chamfer), using the Modified / Generated / Deleted records
/// the algorithm exposes.
///
/// Local features have **cross-kind** parent relationships that differ from boolean
/// ops. Each of the four streams is processed independently with its own parent map:
///
/// | Stream           | Parent slice         | Result slice        |
/// |------------------|----------------------|---------------------|
/// | `face_modified`  | `parent_face_handles`| `result_face_handles`|
/// | `face_generated` | `parent_edge_handles`| `result_face_handles`|
/// | `edge_modified`  | `parent_edge_handles`| `result_edge_handles`|
/// | `edge_generated` | `parent_vertex_handles`| `result_edge_handles`|
///
/// `parent_index` on every inner record is always `0` (one target shape).
///
/// Split detection works identically to
/// [`propagate_attributes_via_brepalgoapi_history`]: a parent with >1 same-stream
/// result children gets a fresh `ModEntry { splitting_feature_id, split_index }`
/// appended to `mod_history` on each child; single-result parents are pure
/// pass-through (mod_history unchanged). Each stream uses its own independent
/// count map and split-counter map so cross-stream index collisions do not occur.
///
/// Returns `Err(QueryError::QueryFailed)` if any record references an out-of-bounds
/// parent or result sub-shape index.
pub fn propagate_attributes_via_local_feature_history(
    table: &mut TopologyAttributeTable,
    parent_face_handles: &[GeometryHandleId],
    parent_edge_handles: &[GeometryHandleId],
    parent_vertex_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &LocalFeatureOpHistoryRecords,
    splitting_feature_id: &FeatureId,
) -> Result<(), QueryError> {
    // ---- stream 1: face_modified <- parent FACE -> result FACE ----
    {
        let counts = count_children_per_parent(&history.face_modified, &[]);
        let mut split_counters = std::collections::HashMap::new();
        let mut ctx = SplitContext::new(splitting_feature_id, &counts, &mut split_counters);
        for record in &history.face_modified {
            propagate_one(
                table,
                &[parent_face_handles],
                result_face_handles,
                record,
                "face_modified",
                &mut ctx,
            )?;
        }
    }

    // ---- stream 2: face_generated <- parent EDGE -> result FACE (cross-kind) ----
    {
        let counts = count_children_per_parent(&history.face_generated, &[]);
        let mut split_counters = std::collections::HashMap::new();
        let mut ctx = SplitContext::new(splitting_feature_id, &counts, &mut split_counters);
        for record in &history.face_generated {
            propagate_one(
                table,
                &[parent_edge_handles],
                result_face_handles,
                record,
                "face_generated",
                &mut ctx,
            )?;
        }
    }

    // ---- stream 3: edge_modified <- parent EDGE -> result EDGE ----
    {
        let counts = count_children_per_parent(&history.edge_modified, &[]);
        let mut split_counters = std::collections::HashMap::new();
        let mut ctx = SplitContext::new(splitting_feature_id, &counts, &mut split_counters);
        for record in &history.edge_modified {
            propagate_one(
                table,
                &[parent_edge_handles],
                result_edge_handles,
                record,
                "edge_modified",
                &mut ctx,
            )?;
        }
    }

    // ---- stream 4: edge_generated <- parent VERTEX -> result EDGE (cross-kind) ----
    {
        let counts = count_children_per_parent(&history.edge_generated, &[]);
        let mut split_counters = std::collections::HashMap::new();
        let mut ctx = SplitContext::new(splitting_feature_id, &counts, &mut split_counters);
        for record in &history.edge_generated {
            propagate_one(
                table,
                &[parent_vertex_handles],
                result_edge_handles,
                record,
                "edge_generated",
                &mut ctx,
            )?;
        }
    }

    Ok(())
}

/// Build a `(parent_index, parent_subshape_index) → count` map across the
/// concatenation of `records_modified` and `records_generated`.
///
/// Records are visited in `Modified` then `Generated` order within each
/// kind; this ordering determines `split_index` assignment downstream in
/// [`propagate_one`] (Modified records' children get the lower indices,
/// Generated records' children follow). The count map and propagator MUST
/// walk identical record streams in identical orders so that `split_index`
/// for child `i` equals child `i`'s position in the per-parent record
/// stream — both call sites use the same `chain(modified, generated)`
/// iterator over the same `BooleanOpHistoryRecords` to enforce this.
///
/// A parent appearing in this map with `count > 1` is a split — each of
/// its children is given a fresh `ModEntry { splitting_feature_id,
/// split_index }` appended to `mod_history`. `count == 1` means a pure
/// pass-through (mod_history unchanged).
///
/// Regression pin:
/// `propagate_split_combines_modified_and_generated_records_for_same_parent`
/// (parent appearing in BOTH Modified and Generated yields count == 2 with
/// Modified-record child at split_index 0, Generated-record child at 1).
fn count_children_per_parent(
    records_modified: &[HistoryRecord],
    records_generated: &[HistoryRecord],
) -> HashMap<(u8, u32), usize> {
    // Upper-bound capacity: at most one entry per record across the
    // chained iteration (an empty pair of slices reserves zero, matching
    // the pre-amendment behaviour for the default-history fast path).
    // Skips the inner reallocs that the previous `HashMap::new()` would
    // hit as the map grew through its default load-factor thresholds.
    let mut counts: HashMap<(u8, u32), usize> =
        HashMap::with_capacity(records_modified.len() + records_generated.len());
    for rec in records_modified.iter().chain(records_generated.iter()) {
        let key = (rec.parent_index, rec.parent_subshape_index);
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Bundles the three split-related inputs threaded through `propagate_one`
/// and `maybe_append_split_entry` per geometry kind (faces vs edges).
///
/// - `feature_id`: stamped onto each new `ModEntry` appended on a split.
/// - `child_counts`: maps `(parent_index, parent_subshape_index)` → count of
///   same-kind result children; count > 1 identifies a split parent.
/// - `split_counters`: accumulates the per-parent `split_index` across the
///   Modified ∪ Generated record stream so each sibling gets a distinct index.
struct SplitContext<'a> {
    feature_id: &'a FeatureId,
    child_counts: &'a HashMap<(u8, u32), usize>,
    split_counters: &'a mut HashMap<(u8, u32), u32>,
}

impl<'a> SplitContext<'a> {
    fn new(
        feature_id: &'a FeatureId,
        child_counts: &'a HashMap<(u8, u32), usize>,
        split_counters: &'a mut HashMap<(u8, u32), u32>,
    ) -> Self {
        Self {
            feature_id,
            child_counts,
            split_counters,
        }
    }
}

/// If the parent at `(parent_index, parent_subshape_index)` has more than
/// one same-kind result child (count > 1), append a fresh
/// `ModEntry { splitting_feature_id, split_index }` to `attr` and bump
/// the per-parent `split_index` counter for the next sibling.
///
/// **Regression pin (`propagate_skips_mod_entry_for_single_result_parent`):**
/// for single-result parents (count == 1) and parents absent from the
/// count map (count == 0; defensive — the populator builds the map over
/// the same record stream the propagator walks, so this branch is
/// unreachable in practice) this is a no-op. The v0.2 invariant is that
/// `mod_history` is only augmented on actual splits — pure pass-through
/// propagation preserves prior `mod_history` exactly, including
/// non-empty history accumulated by upstream split feeds (a parent that
/// was itself split by an earlier feature retains its accumulated postfix
/// when a later non-splitting op forwards it).
fn maybe_append_split_entry(
    attr: &mut TopologyAttribute,
    parent_key: (u8, u32),
    ctx: &mut SplitContext<'_>,
) {
    let count = ctx.child_counts.get(&parent_key).copied().unwrap_or(0);
    if count > 1 {
        let split_index = ctx.split_counters.entry(parent_key).or_insert(0);
        attr.mod_history.push(ModEntry {
            splitting_feature_id: ctx.feature_id.clone(),
            split_index: *split_index,
        });
        *split_index += 1;
    }
}

/// Look up the parent attribute via `record.parent_index` /
/// `record.parent_subshape_index`, and if present, clone it onto the
/// result sub-shape at `record.result_subshape_index`. When the parent
/// has more than one same-kind result child (`child_counts[(parent_index,
/// parent_subshape_index)] > 1`), append a fresh `ModEntry` to the
/// cloned attribute's `mod_history` before recording it.
///
/// Returns `Err(QueryError::QueryFailed)` if any index is out of range.
fn propagate_one(
    table: &mut TopologyAttributeTable,
    parent_handles: &[&[GeometryHandleId]],
    result_handles: &[GeometryHandleId],
    record: &HistoryRecord,
    kind: &str,
    ctx: &mut SplitContext<'_>,
) -> Result<(), QueryError> {
    let parent_idx = record.parent_index as usize;
    if parent_idx >= parent_handles.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has parent_index {parent_idx} \
             but only {} parents are tracked",
            parent_handles.len()
        )));
    }
    let parent_vec = parent_handles[parent_idx];
    let parent_subshape_idx = record.parent_subshape_index as usize;
    if parent_subshape_idx >= parent_vec.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has parent_subshape_index {} \
             but parent {} has only {} {kind}s",
            parent_subshape_idx,
            parent_idx,
            parent_vec.len()
        )));
    }
    let parent_handle = parent_vec[parent_subshape_idx];

    let result_subshape_idx = record.result_subshape_index as usize;
    if result_subshape_idx >= result_handles.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has result_subshape_index {} \
             but result has only {} {kind}s",
            result_subshape_idx,
            result_handles.len()
        )));
    }
    let result_handle = result_handles[result_subshape_idx];

    // If the parent had no attribute (e.g. tasks 5-8 only auto-populate
    // for some op kinds; task-1 tests hand-seed only faces), there's
    // nothing to clone — silently skip. The end-to-end test asserts
    // that explicitly-seeded parents propagate.
    if let Some(parent_attr) = table.lookup(parent_handle) {
        let mut attr_clone = parent_attr.clone();
        let parent_key = (record.parent_index, record.parent_subshape_index);
        maybe_append_split_entry(&mut attr_clone, parent_key, ctx);
        table.record(result_handle, attr_clone);
    }
    Ok(())
}

/// Originate topology attributes for a `BRepPrimAPI_MakePrism` (extrude)
/// result, given the per-op history records returned by
/// `OcctKernel::extrude_with_history`.
///
/// Inputs:
/// - `table`: the table to update in place. Result-face entries are
///   written for each cap-face index and each `face_generated` record.
/// - `feature_id`: the FeatureId attached to every entry written. Caller
///   constructs this from a `RealizationNodeId` via the existing
///   `From<&RealizationNodeId> for FeatureId` impl.
/// - `profile_face_handles` / `profile_edge_handles`: the profile shape's
///   faces / edges in canonical TopExp order. These are passed in for
///   defense-in-depth `parent_subshape_index` range validation; the
///   helper does not currently *read* the profile table (extrude
///   originates fresh attributes — propagation of pre-existing profile
///   attributes through `Modified` records is task 5b's loft / 6's
///   primitives concern, not 5a's).
/// - `result_face_handles` / `result_edge_handles`: the result shape's
///   faces / edges in canonical TopExp order. Indexed by
///   `start_cap_face_indices`, `end_cap_face_indices`, and the
///   `result_subshape_index` of `face_generated` records.
/// - `history`: the `SweepOpHistoryRecords` produced by the FFI.
///
/// Cap orientation contract (see `SweepOpHistoryRecords` doc):
///   `start_cap_face_indices` → `Role::Cap(CapKind::Top)`
///   `end_cap_face_indices` → `Role::Cap(CapKind::Bottom)`
///
/// The "Top from start" convention follows from the `make_prism(profile,
/// 0, 0, +dist)` call shape: the profile-as-placed (FirstShape) is the
/// face the user authored at the chosen Z origin and the swept-end
/// (LastShape) is at +dist. For a positive-Z extrude the `start_cap` is
/// the higher-Z face (`Top`) and the `end_cap` is the lower-Z face
/// (`Bottom`); see `SweepOpHistoryRecords` doc for cross-reference.
///
/// Local-index assignment:
///   - Caps are unique within their `(feature_id, role)` pair, so
///     each cap face gets `local_index = 0`.
///   - Side faces (`face_generated`) are assigned sequential 0-based
///     `local_index` in the order they appear in `history.face_generated`.
///     Per task-5a design decision, this 0-based ordering follows the
///     kernel's TopExp parent-edge enumeration (each parent edge sponsors
///     one side face; the records arrive in parent-edge order), so the
///     index is invariant across parameter edits that preserve profile
///     shape (the test in `engine_build_extrude_with_mock_history_*`
///     verifies this stability).
///
/// Edge attributes (e.g. `Role::NewEdge` for cap-to-side seam edges) are
/// **not** written by this helper. Edge-level attribution is deferred to
/// task 5b / 6 once the cap-edge / seam-edge classification rules are
/// finalised; the variant exists in `Role` for the type-system but is
/// not yet emitted by extrude population.
///
/// Returns `Err(QueryError::QueryFailed)` if any record references an
/// out-of-bounds result-face index, or if any cap-face index is out of
/// bounds in `result_face_handles`. The FFI primitive guarantees
/// in-range indices, so this is a defense-in-depth path pinned by the
/// step-11 unit tests.
#[allow(clippy::too_many_arguments)]
pub fn populate_extrude_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &SweepOpHistoryRecords,
    result_vertex_handles: &[GeometryHandleId],
    start_cap_vertex_index_lists: &[Vec<u32>],
    end_cap_vertex_index_lists: &[Vec<u32>],
) -> Result<(), QueryError> {
    // Caps: start → Top, end → Bottom; each cap is unique → local_index = 0.
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.start_cap_face_indices,
        Role::Cap(CapKind::Top),
        "extrude start cap",
    )?;
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.end_cap_face_indices,
        Role::Cap(CapKind::Bottom),
        "extrude end cap",
    )?;

    // Sides: each face_generated record → Role::Side with sequential
    // local_index in the order the records appear (mirrors parent-edge
    // TopExp ordering, stable across parameter edits).
    write_face_generated_attributes(
        table,
        feature_id,
        profile_face_handles,
        profile_edge_handles,
        result_face_handles,
        result_edge_handles,
        &history.face_generated,
        Role::Side,
        "extrude side",
    )?;

    // Cap vertices: start → Top, end → Bottom.
    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        start_cap_vertex_index_lists,
        CapKind::Top,
        "extrude start cap vertex",
    )?;
    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        end_cap_vertex_index_lists,
        CapKind::Bottom,
        "extrude end cap vertex",
    )?;

    Ok(())
}

/// Originate topology attributes for a `BRepPrimAPI_MakeRevol` (revolve)
/// result, given the per-op history records returned by
/// `OcctKernel::revolve_with_history`.
///
/// Mirrors [`populate_extrude_attributes`] but emits revolve-specific
/// roles:
///   - `start_cap_face_indices` → `Role::Cap(CapKind::Start)` (partial
///     revolutions only; empty for full-2π).
///   - `end_cap_face_indices` → `Role::Cap(CapKind::End)` (partial
///     revolutions only; empty for full-2π).
///   - `face_generated` → `Role::RevolvedFace` (NOT `Role::Side` — this
///     is the per-op distinguisher between extrude lateral faces and
///     revolve lateral faces, per task-5a design decisions).
///
/// `Role::AxisFace` is **not** emitted by this helper. The variant is
/// declared in `Role` for type-system completeness — selectors built
/// against the v0.2 vocabulary v2 (PRD line 102) need it stable — but
/// detection of "this face touches the revolve axis" requires geometric
/// analysis (face surface contains axis, or near-zero surface area)
/// that is deferred to a follow-up task per task-5a's documented scope.
///
/// **face_generated provenance (task 2636):** under a full 2π revolution,
/// OCCT's `Generated()` omits records for profile edges that are
/// perpendicular to the rotation axis (radial edges).  The C++ FFI layer
/// (`synthesize_full_revolution_radial_face_records` in occt_wrapper.cpp)
/// closes this gap by appending synthesized records for those edges and
/// stable-sorting the combined vector by `parent_subshape_index`.  The
/// synthesized records are byte-identical to OCCT-reported records in the
/// `SweepOpHistoryRecords` format, so this function processes them
/// transparently: both originate as `Role::RevolvedFace` entries with
/// sequential `local_index`.  The FFI-layer sort guarantees
/// `local_index == parent_subshape_index` for well-formed revolve sweeps
/// (same invariant as partial revolutions), ensuring selector portability
/// between the two cases.
///
/// Local-index assignment, parameter semantics, and out-of-range error
/// behaviour are identical to [`populate_extrude_attributes`]; see
/// that helper's doc-comment for the parameter contract.
#[allow(clippy::too_many_arguments)]
pub fn populate_revolve_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &SweepOpHistoryRecords,
    result_vertex_handles: &[GeometryHandleId],
    start_cap_vertex_index_lists: &[Vec<u32>],
    end_cap_vertex_index_lists: &[Vec<u32>],
) -> Result<(), QueryError> {
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.start_cap_face_indices,
        Role::Cap(CapKind::Start),
        "revolve start cap",
    )?;
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.end_cap_face_indices,
        Role::Cap(CapKind::End),
        "revolve end cap",
    )?;

    write_face_generated_attributes(
        table,
        feature_id,
        profile_face_handles,
        profile_edge_handles,
        result_face_handles,
        result_edge_handles,
        &history.face_generated,
        Role::RevolvedFace,
        "revolve revolved face",
    )?;

    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        start_cap_vertex_index_lists,
        CapKind::Start,
        "revolve start cap vertex",
    )?;
    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        end_cap_vertex_index_lists,
        CapKind::End,
        "revolve end cap vertex",
    )?;

    Ok(())
}

/// Originate topology attributes for a `BRepOffsetAPI_MakePipe` (sweep)
/// result, given the per-op history records returned by
/// `OcctKernel::sweep_with_history`.
///
/// Mirrors [`populate_extrude_attributes`] but emits sweep-specific roles:
///   - `start_cap_face_indices` → `Role::Cap(CapKind::Start)` (parametric
///     Start/End semantics matching the spine's parameter direction; NOT
///     extrude's gravitational Top/Bottom).
///   - `end_cap_face_indices` → `Role::Cap(CapKind::End)`.
///   - `face_generated` → `Role::SweptFace` (NOT `Role::Side` — this is
///     the per-op distinguisher between extrude lateral faces and sweep
///     lateral faces, per task-5b design decisions in geometry.rs).
///
/// Sweep is single-parent like extrude / revolve (the profile is the
/// operand whose sub-shapes propagate to the result; the path / spine is
/// not itself a parent), so this helper reuses `SweepOpHistoryRecords`
/// verbatim — `parent_index` in every record is `0`.
///
/// Edge attributes (e.g. `Role::NewEdge` for cap-to-side seam edges) are
/// **not** written by this helper, mirroring [`populate_extrude_attributes`]:
/// edge-level attribution is deferred until the cap-edge / seam-edge
/// classification rules are finalised.
///
/// Local-index assignment, parameter semantics, and out-of-range error
/// behaviour are identical to [`populate_extrude_attributes`]; see that
/// helper's doc-comment for the parameter contract.
#[allow(clippy::too_many_arguments)]
pub fn populate_sweep_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &SweepOpHistoryRecords,
    result_vertex_handles: &[GeometryHandleId],
    start_cap_vertex_index_lists: &[Vec<u32>],
    end_cap_vertex_index_lists: &[Vec<u32>],
) -> Result<(), QueryError> {
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.start_cap_face_indices,
        Role::Cap(CapKind::Start),
        "sweep start cap",
    )?;
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.end_cap_face_indices,
        Role::Cap(CapKind::End),
        "sweep end cap",
    )?;

    write_face_generated_attributes(
        table,
        feature_id,
        profile_face_handles,
        profile_edge_handles,
        result_face_handles,
        result_edge_handles,
        &history.face_generated,
        Role::SweptFace,
        "sweep swept face",
    )?;

    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        start_cap_vertex_index_lists,
        CapKind::Start,
        "sweep start cap vertex",
    )?;
    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        end_cap_vertex_index_lists,
        CapKind::End,
        "sweep end cap vertex",
    )?;

    Ok(())
}

/// Originate topology attributes for a `BRepOffsetAPI_ThruSections` (loft)
/// result, given the per-op history records returned by
/// `OcctKernel::loft_with_history`.
///
/// Loft is the **multi-parent** variant: `parent_index` in each
/// `face_generated` record denotes a section index in
/// `[0, profiles.len())`, and `parent_subshape_index` denotes the edge
/// index within that section's edge map. This helper validates both
/// indices against the per-section profile face/edge slices the caller
/// supplies.
///
/// Role assignments:
///   - `start_cap_face_indices` → `Role::Cap(CapKind::Start)` (first
///     profile section's cap under `is_solid=true`).
///   - `end_cap_face_indices` → `Role::Cap(CapKind::End)` (last
///     profile section's cap under `is_solid=true`).
///   - `face_generated` → `Role::LoftedFace` (NOT `Role::Side` /
///     `SweptFace` / `RevolvedFace` — per-op distinguisher per the
///     task-5a/5b design decisions in geometry.rs).
///
/// `local_index` increments **sequentially across all sections** in the
/// order the records appear in `face_generated` — sections are not
/// re-numbered per-section. The C++ wrapper emits records in section
/// order (section 0's edges first, then section 1's, ...), so the
/// resulting `local_index` is naturally stable for selector portability
/// when sections are added/removed at the end (head insertion
/// invalidates indices, matching the documented v0.2 caveat).
///
/// Edge attributes (e.g. `Role::NewEdge` for rail edges between
/// sections) are **not** written by this helper, mirroring
/// [`populate_extrude_attributes`]: edge-level attribution is deferred
/// until the cap-edge / seam-edge / rail-edge classification rules are
/// finalised in a follow-up task.
///
/// # Errors
///
/// Returns `QueryError::QueryFailed` if any `face_generated` record's
/// `parent_index` is `>= section_edge_handles_per_section.len()`, if its
/// `parent_subshape_index` is out of range for the addressed section's
/// edge slice, or if its `result_subshape_index` is out of range for
/// `result_face_handles`. Also returns `QueryError::QueryFailed` if any
/// cap-face index is out of range. The FFI primitive guarantees in-range
/// indices on success, so these are defense-in-depth paths pinned by the
/// step-9 unit tests.
#[allow(clippy::too_many_arguments)]
pub fn populate_loft_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    section_face_handles_per_section: &[Vec<GeometryHandleId>],
    section_edge_handles_per_section: &[Vec<GeometryHandleId>],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &LoftOpHistoryRecords,
    result_vertex_handles: &[GeometryHandleId],
    start_cap_vertex_index_lists: &[Vec<u32>],
    end_cap_vertex_index_lists: &[Vec<u32>],
) -> Result<(), QueryError> {
    // Pin the lockstep invariant: `engine_build.rs::populate_loft_op` builds
    // `section_faces` and `section_edges` in tandem (one push per profile) so
    // both slices always have `len() == profile_handles.len()`.
    // `write_loft_face_generated_attributes` range-checks `parent_index` against
    // `section_edge_handles_per_section.len()`; if the two families diverged, a
    // `parent_index` valid in the edge family could silently be out-of-range in
    // the face family (and vice versa once face-level writes land).
    debug_assert_eq!(
        section_face_handles_per_section.len(),
        section_edge_handles_per_section.len(),
        "loft section face/edge slice families must be built in lockstep \
         (engine_build.rs::populate_loft_op); write_loft_face_generated_attributes' \
         parent_index range check uses the edge-slice family"
    );
    // Both parameters are reserved at this public-API entry point rather than
    // being dropped inside the inner helpers:
    //   • `section_face_handles_per_section` — seam for future face-level
    //     Modified records (once the loft kernel emits per-section face maps)
    //   • `result_edge_handles` — seam for future rail/seam/cap-edge
    //     classification
    let _ = section_face_handles_per_section; // reserved for future face-level Modified records
    let _ = result_edge_handles; // reserved for future rail/seam/cap-edge classification

    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.start_cap_face_indices,
        Role::Cap(CapKind::Start),
        "loft start cap",
    )?;
    write_cap_attributes(
        table,
        feature_id,
        result_face_handles,
        &history.end_cap_face_indices,
        Role::Cap(CapKind::End),
        "loft end cap",
    )?;
    write_loft_face_generated_attributes(
        table,
        feature_id,
        section_edge_handles_per_section,
        result_face_handles,
        &history.face_generated,
    )?;

    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        start_cap_vertex_index_lists,
        CapKind::Start,
        "loft start cap vertex",
    )?;
    write_cap_vertex_attributes(
        table,
        feature_id,
        result_vertex_handles,
        end_cap_vertex_index_lists,
        CapKind::End,
        "loft end cap vertex",
    )?;

    Ok(())
}

/// Shared helper: write `(feature_id, role, local_index = 0)` to each
/// cap face index in `cap_indices`, validating that each index is in
/// range for `result_face_handles`.
fn write_cap_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    result_face_handles: &[GeometryHandleId],
    cap_indices: &[u32],
    role: Role,
    kind: &str,
) -> Result<(), QueryError> {
    for &idx in cap_indices {
        let idx_usize = idx as usize;
        if idx_usize >= result_face_handles.len() {
            return Err(QueryError::QueryFailed(format!(
                "{kind} face index {idx} is out of range \
                 for result face handles of len {}",
                result_face_handles.len()
            )));
        }
        let handle = result_face_handles[idx_usize];
        table.record(
            handle,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role,
                local_index: 0,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
    Ok(())
}

/// Shared helper: write `Role::CapCornerVertex { face }` entries for all
/// vertices belonging to cap faces.
///
/// `cap_vertex_index_lists` is a slice of `Vec<u32>`, one inner `Vec` per
/// cap face (mirrors `cap_face_indices` in [`write_cap_attributes`]).  Each
/// inner `Vec` is a list of indices into `result_vertex_handles` that belong
/// to that cap face's vertices.  `local_index` is assigned sequentially
/// within each inner Vec, resetting to 0 at the start of each new cap face's
/// vertex list.
///
/// Returns `Err(QueryError::QueryFailed)` if any index is out of range for
/// `result_vertex_handles` (defense-in-depth; the caller supplies
/// kernel-derived indices that are guaranteed in-range for well-formed ops).
fn write_cap_vertex_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    result_vertex_handles: &[GeometryHandleId],
    cap_vertex_index_lists: &[Vec<u32>],
    face: CapKind,
    kind: &str,
) -> Result<(), QueryError> {
    for cap_vertices in cap_vertex_index_lists {
        for (local_index, &vertex_idx) in cap_vertices.iter().enumerate() {
            let idx_usize = vertex_idx as usize;
            if idx_usize >= result_vertex_handles.len() {
                return Err(QueryError::QueryFailed(format!(
                    "{kind} vertex index {vertex_idx} is out of range \
                     for result vertex handles of len {}",
                    result_vertex_handles.len()
                )));
            }
            let handle = result_vertex_handles[idx_usize];
            table.record(
                handle,
                TopologyAttribute {
                    feature_id: feature_id.clone(),
                    role: Role::CapCornerVertex { face },
                    local_index: local_index as u32,
                    user_label: None,
                    mod_history: Vec::new(),
                },
            );
        }
    }
    Ok(())
}

/// Shared helper: write `(feature_id, role, local_index = sequential)`
/// to each `face_generated` record's `result_subshape_index`, validating
/// that each `parent_subshape_index` is in range for the profile slices
/// (defense-in-depth) and each `result_subshape_index` is in range for
/// `result_face_handles`. `local_index` increments per record in the
/// order they appear in `face_generated`.
#[allow(clippy::too_many_arguments)] // sweep helpers fan out parent + result slices for both faces and edges
fn write_face_generated_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    _result_edge_handles: &[GeometryHandleId],
    face_generated: &[HistoryRecord],
    role: Role,
    kind: &str,
) -> Result<(), QueryError> {
    let _ = profile_face_handles; // reserved for future face-level Modified checks
    for (sequential_idx, record) in face_generated.iter().enumerate() {
        // Defense-in-depth: parent_subshape_index in range over profile edges.
        // The kernel emits each side face from a parent profile edge sweep,
        // so parent_subshape_index points into the profile edge map.
        let parent_subshape_idx = record.parent_subshape_index as usize;
        if parent_subshape_idx >= profile_edge_handles.len() {
            return Err(QueryError::QueryFailed(format!(
                "{kind} face_generated record has parent_subshape_index {} \
                 but profile has only {} edges",
                parent_subshape_idx,
                profile_edge_handles.len()
            )));
        }

        let result_subshape_idx = record.result_subshape_index as usize;
        if result_subshape_idx >= result_face_handles.len() {
            return Err(QueryError::QueryFailed(format!(
                "{kind} face_generated record has result_subshape_index {} \
                 but result has only {} faces",
                result_subshape_idx,
                result_face_handles.len()
            )));
        }

        let handle = result_face_handles[result_subshape_idx];
        table.record(
            handle,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role,
                local_index: sequential_idx as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
    Ok(())
}

/// Multi-parent variant of [`write_face_generated_attributes`] for loft
/// (`BRepOffsetAPI_ThruSections`).  For each `face_generated` record:
///
///   1. Validate `parent_index` is in range for
///      `section_edge_handles_per_section.len()` (the number of loft
///      sections).  Returns `QueryFailed` mentioning "section" on
///      out-of-range.
///   2. Validate `parent_subshape_index` is in range for the addressed
///      section's edge slice (the kernel emits each lateral face from a
///      parent profile edge sweep, so the subshape index points into
///      the edge map).
///   3. Validate `result_subshape_index` is in range for
///      `result_face_handles`.
///   4. Write `(feature_id, Role::LoftedFace, local_index =
///      sequential_idx)` keyed by the result face handle.
///
/// `local_index` increments sequentially across all sections in the
/// order records appear in `face_generated` (section 0's edges first,
/// then section 1's, ...).
fn write_loft_face_generated_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    section_edge_handles_per_section: &[Vec<GeometryHandleId>],
    result_face_handles: &[GeometryHandleId],
    face_generated: &[HistoryRecord],
) -> Result<(), QueryError> {
    for (sequential_idx, record) in face_generated.iter().enumerate() {
        // Step 1: parent_index in range over section count.
        let parent_idx = record.parent_index as usize;
        if parent_idx >= section_edge_handles_per_section.len() {
            return Err(QueryError::QueryFailed(format!(
                "loft face_generated record has parent_index {} \
                 but loft has only {} section(s)",
                parent_idx,
                section_edge_handles_per_section.len()
            )));
        }

        // Step 2: parent_subshape_index in range over the addressed
        // section's edge slice.
        let parent_subshape_idx = record.parent_subshape_index as usize;
        let section_edges = &section_edge_handles_per_section[parent_idx];
        if parent_subshape_idx >= section_edges.len() {
            return Err(QueryError::QueryFailed(format!(
                "loft face_generated record has parent_subshape_index {} \
                 but section {} has only {} edges",
                parent_subshape_idx,
                parent_idx,
                section_edges.len()
            )));
        }

        // Step 3: result_subshape_index in range over result faces.
        let result_subshape_idx = record.result_subshape_index as usize;
        if result_subshape_idx >= result_face_handles.len() {
            return Err(QueryError::QueryFailed(format!(
                "loft face_generated record has result_subshape_index {} \
                 but result has only {} faces",
                result_subshape_idx,
                result_face_handles.len()
            )));
        }

        // Step 4: write the attribute, keyed by the result face handle.
        let handle = result_face_handles[result_subshape_idx];
        table.record(
            handle,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::LoftedFace,
                local_index: sequential_idx as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
    Ok(())
}

/// Kernel-epsilon-tight tolerance (1 nm, 1e-9 m) for the construction-time
/// local-index-reassignment fragility detector.
///
/// Squared-distance comparison vs `LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M *
/// LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M`: pairs of `(feature_id, role)`-peer
/// centroids closer than this are flagged as geometrically tied. Real CAD
/// designs almost never have features tied to that precision, so false
/// positives are minimal.
///
/// **Per-realization tolerance threading is deferred** to a follow-up task
/// (see #2654 design decisions); when that lands, this constant becomes the
/// default and the realization-specific tolerance overrides it at the call
/// site. Keeping it as a single named constant means that threading change
/// is one-line, not a 7-caller mechanical rewrite.
pub const LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M: f64 = 1e-9;

/// Stable sort key and human-readable name for `Role`, both decoupled from `Debug`.
///
/// Returns `(discriminant, human_name)` where `discriminant` drives deterministic
/// emission order and `human_name` is the stable wording used in diagnostic messages
/// (the contract for downstream diagnostic consumers). The Rust API guidelines mark
/// `Debug` as "not stable for serialization" — co-locating both values in one
/// exhaustive match eliminates the drift risk of two parallel match functions, where
/// a transposition (e.g. discriminant 4 assigned to `Side` while the name arm returns
/// "NewEdge" for `Side`) would be invisible to the compiler. New variants must be
/// appended (assigning the next discriminant) rather than inserted between existing ones.
fn role_sort_key(role: &Role) -> (u32, &'static str) {
    match role {
        Role::Cap(CapKind::Top) => (0, "Cap(Top)"),
        Role::Cap(CapKind::Bottom) => (1, "Cap(Bottom)"),
        Role::Cap(CapKind::Start) => (2, "Cap(Start)"),
        Role::Cap(CapKind::End) => (3, "Cap(End)"),
        Role::Side => (4, "Side"),
        Role::NewEdge => (5, "NewEdge"),
        Role::RevolvedFace => (6, "RevolvedFace"),
        Role::AxisFace => (7, "AxisFace"),
        Role::SweptFace => (8, "SweptFace"),
        Role::LoftedFace => (9, "LoftedFace"),
        Role::MidSurfaceFace => (10, "MidSurfaceFace"),
        Role::MidSurfaceEdge => (11, "MidSurfaceEdge"),
        Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Pos,
            z: AxisSign::Pos,
        } => (12, "CornerVertex(+x,+y,+z)"),
        Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Pos,
            z: AxisSign::Neg,
        } => (13, "CornerVertex(+x,+y,-z)"),
        Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Neg,
            z: AxisSign::Pos,
        } => (14, "CornerVertex(+x,-y,+z)"),
        Role::CornerVertex {
            x: AxisSign::Pos,
            y: AxisSign::Neg,
            z: AxisSign::Neg,
        } => (15, "CornerVertex(+x,-y,-z)"),
        Role::CornerVertex {
            x: AxisSign::Neg,
            y: AxisSign::Pos,
            z: AxisSign::Pos,
        } => (16, "CornerVertex(-x,+y,+z)"),
        Role::CornerVertex {
            x: AxisSign::Neg,
            y: AxisSign::Pos,
            z: AxisSign::Neg,
        } => (17, "CornerVertex(-x,+y,-z)"),
        Role::CornerVertex {
            x: AxisSign::Neg,
            y: AxisSign::Neg,
            z: AxisSign::Pos,
        } => (18, "CornerVertex(-x,-y,+z)"),
        Role::CornerVertex {
            x: AxisSign::Neg,
            y: AxisSign::Neg,
            z: AxisSign::Neg,
        } => (19, "CornerVertex(-x,-y,-z)"),
        Role::CapCornerVertex { face: CapKind::Top } => (20, "CapCornerVertex(Top)"),
        Role::CapCornerVertex {
            face: CapKind::Bottom,
        } => (21, "CapCornerVertex(Bottom)"),
        Role::CapCornerVertex {
            face: CapKind::Start,
        } => (22, "CapCornerVertex(Start)"),
        Role::CapCornerVertex { face: CapKind::End } => (23, "CapCornerVertex(End)"),
    }
}

/// Emit `TopologyAttributeLocalIndexReassigned` Warnings for groups of
/// topology-attribute entries whose centroids are geometrically tied within
/// `tol_m`, signalling that the kernel's enumeration order — and therefore
/// the `local_index` assignment — would arbitrarily shuffle under a future
/// edit (PRD `docs/prds/v0_2/persistent-naming-v2.md` line 72).
///
/// # Inputs
///
/// - `handles_with_attrs`: per-realization slice of `(GeometryHandleId,
///   &TopologyAttribute)` pairs. The caller (engine_build.rs::execute_realization_ops)
///   is responsible for scoping this to one realization at a time — typically
///   by filtering `topology_attribute_table.iter()` on
///   `attr.feature_id == realization_feature_id`. Passing the full table
///   would re-emit on every successive realization for prior-realization
///   entries that persist for the duration of `build()`.
/// - `centroids`: pre-queried centroid map (`HashMap<GeometryHandleId, [f64; 3]>`).
///   Computed at the call site via `kernel.query(GeometryQuery::Centroid(handle))`
///   so this helper stays pure-Rust (no `&mut dyn GeometryKernel` borrow).
///   Handles absent from this map are silently skipped — kernel-query failures
///   at the call site emit a Warning there and skip the handle, mirroring the
///   auxiliary-metadata-failure-must-not-regress-to-Failed convention used by
///   `seed_primitive_attributes_for_handle` and `populate_attribute_history`.
/// - `tol_m`: tolerance in meters. Pairs whose squared centroid distance
///   `<= tol_m * tol_m` are considered geometrically tied. Engine call site
///   uses [`LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M`] (a kernel-epsilon-tight
///   1 nm / 1e-9 m sentinel); per-realization tolerance threading is
///   deferred to a follow-up task.
/// - `realization_span`: span attached to the primary diagnostic label —
///   the source span of the realization being constructed. Detection runs at
///   realization-construction time, before any selector resolution, so the
///   label always points at the realization, not at a selector call site.
/// - `diagnostics`: appended in place; the helper never clears or reorders
///   pre-existing entries.
///
/// # Output
///
/// At most one diagnostic per `(feature_id, role)` group, carrying
/// `DiagnosticCode::TopologyAttributeLocalIndexReassigned`, severity Warning,
/// and naming the smallest pair of tied `local_index` values for
/// reproducible message wording.
///
/// # Filter rules
///
/// - Entries with non-empty `mod_history` are skipped — post-split clusters
///   are tracked through `ModEntry` and surfaced via
///   `TopologyAttributeAmbiguousAfterSplit` at resolve time per PRD line 72
///   ("not because of a split — splits are handled by mod_history"). Re-firing
///   here would double-warn the user about the same fragility.
/// - Singleton groups (one entry per `(feature_id, role)`) have no pairwise
///   comparison and are skipped.
///
/// # Tolerance semantics
///
/// Squared-distance comparison vs `tol_m * tol_m` to avoid an `sqrt` per pair
/// (mirroring the squared-distance idiom from
/// `selector_vocabulary_v2::extremal_by_centroid`). NaN / infinite centroid
/// components — should they ever arise from a degenerate kernel query — would
/// fail the squared-distance comparison naturally (NaN comparisons are false),
/// so no extra guard is needed.
///
/// # Single-source rule
///
/// This helper does NOT regress the realization to Failed under any condition:
/// it only appends Warnings. Auxiliary metadata MUST NOT regress to Failed —
/// the realization is primary, attribute fragility detection is supplementary.
pub fn detect_local_index_reassignment_diagnostics(
    handles_with_attrs: &[(GeometryHandleId, &TopologyAttribute)],
    centroids: &HashMap<GeometryHandleId, [f64; 3]>,
    tol_m: f64,
    realization_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Group entries by (feature_id, role). Skip post-split clusters
    // (mod_history non-empty) per PRD line 72 — those are tracked by
    // TopologyAttributeAmbiguousAfterSplit. Keys borrow `feature_id` from
    // the input slice (whose lifetime outlives this function) to avoid a
    // FeatureId clone per entry.
    let mut groups: HashMap<(&FeatureId, Role), Vec<(GeometryHandleId, u32)>> = HashMap::new();
    for (handle_id, attr) in handles_with_attrs.iter() {
        if !attr.mod_history.is_empty() {
            continue;
        }
        groups
            .entry((&attr.feature_id, attr.role))
            .or_default()
            .push((*handle_id, attr.local_index));
    }

    let tol_sq = tol_m * tol_m;

    // Iterate groups in deterministic (feature_id, role) order. HashMap
    // iteration order is unspecified — collect keys, sort, then walk.
    // sort_by_cached_key materializes (feature_id_string, role_discriminant)
    // once per key so to_string() is not called on every comparison.
    let mut group_keys: Vec<&(&FeatureId, Role)> = groups.keys().collect();
    group_keys.sort_by_cached_key(|k| (k.0.to_string(), role_sort_key(&k.1).0));

    for key in group_keys {
        let entries = &groups[key];
        if entries.len() < 2 {
            continue;
        }
        // Sort entries by local_index ascending so the smallest tied pair is
        // emitted first (deterministic message wording: "indices i and j" with
        // i < j and i is the smallest tied index in the group).
        let mut sorted = entries.clone();
        sorted.sort_by_key(|(_, local_index)| *local_index);

        // Pairwise comparison; stop after first tie (at most one diagnostic
        // per group). Handles absent from the centroid map are silently
        // skipped — kernel-query failure at the call site is reported there
        // and the helper just lacks data for that handle.
        'outer: for i in 0..sorted.len() {
            let (h_i, idx_i) = sorted[i];
            let Some(c_i) = centroids.get(&h_i) else {
                continue;
            };
            for &(h_j, idx_j) in sorted.iter().skip(i + 1) {
                let Some(c_j) = centroids.get(&h_j) else {
                    continue;
                };
                let dx = c_i[0] - c_j[0];
                let dy = c_i[1] - c_j[1];
                let dz = c_i[2] - c_j[2];
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq <= tol_sq {
                    let (feature_id, role) = key;
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "topology-attribute selector for (feature '{}', role '{}') has \
                             geometrically tied local_index assignments at indices {} and {}; \
                             selector resolution may shuffle after edits",
                            feature_id,
                            role_sort_key(role).1,
                            idx_i,
                            idx_j,
                        ))
                        .with_code(DiagnosticCode::TopologyAttributeLocalIndexReassigned)
                        .with_label(DiagnosticLabel::new(
                            realization_span,
                            "realization producing geometrically tied attributes",
                        )),
                    );
                    break 'outer;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests focused on the `Err(QueryError::QueryFailed(...))`
    //! defense-in-depth branches of [`propagate_one`]. The happy-path
    //! (parent → result attribute clone via Modified/Generated records)
    //! is fully covered by the PRD-line-93 single integration test in
    //! `tests/topology_attribute_e2e.rs`; duplicating it here would just
    //! double the maintenance surface as the propagation contract evolves.
    //!
    //! These error branches are pure given inputs, so they need no OCCT
    //! kernel — we hand-build a malformed `BooleanOpHistoryRecords` and
    //! check that each variant surfaces as `QueryFailed`.
    use reify_ir::{
        BooleanOpHistoryRecords, BooleanOpParents, CapKind, FeatureId, GeometryHandleId,
        HistoryRecord, LoftOpHistoryRecords, ModEntry, QueryError, Role, SweepOpHistoryRecords,
        TopologyAttribute, TopologyAttributeTable,
    };

    use super::{
        populate_extrude_attributes, populate_loft_attributes, populate_revolve_attributes,
        populate_sweep_attributes, propagate_attributes_via_brepalgoapi_history,
    };

    /// Synthetic FeatureId reused by every split-detection test as the
    /// `splitting_feature_id` parameter to
    /// `propagate_attributes_via_brepalgoapi_history`.
    fn fuse_feature_id() -> FeatureId {
        FeatureId::new("Fuse#realization[0]")
    }

    /// Build a `BooleanOpHistoryRecords` with `rec` as the sole
    /// `face_modified` entry and every other vector empty.
    fn history_with_single_face_modified(rec: HistoryRecord) -> BooleanOpHistoryRecords {
        BooleanOpHistoryRecords {
            face_modified: vec![rec],
            ..Default::default()
        }
    }

    /// Build a `BooleanOpHistoryRecords` with `rec` as the sole
    /// `edge_modified` entry and every other vector empty.
    fn history_with_single_edge_modified(rec: HistoryRecord) -> BooleanOpHistoryRecords {
        BooleanOpHistoryRecords {
            edge_modified: vec![rec],
            ..Default::default()
        }
    }

    /// Parent + result handle vectors for a 2-parent, 1-result layout
    /// — owned so the test fn can borrow slices into them without
    /// running afoul of intermediate-temporary lifetime issues.
    struct MinimalLayout {
        parent_faces: [Vec<GeometryHandleId>; 2],
        parent_edges: [Vec<GeometryHandleId>; 2],
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    }

    /// One face/edge per parent + one result face/edge — the minimum
    /// shape needed to exercise out-of-range index error paths without
    /// tripping earlier guards.
    fn minimal_parent_result_layout() -> MinimalLayout {
        MinimalLayout {
            parent_faces: [vec![GeometryHandleId(1)], vec![GeometryHandleId(2)]],
            parent_edges: [vec![GeometryHandleId(3)], vec![GeometryHandleId(4)]],
            result_faces: vec![GeometryHandleId(11)],
            result_edges: vec![GeometryHandleId(12)],
        }
    }

    /// One face/edge per parent + 3 result faces — used by split-detection
    /// tests that need a parent's records to point at multiple result
    /// sub-shapes (count > 1 ⇒ split).
    fn split_layout() -> MinimalLayout {
        MinimalLayout {
            parent_faces: [vec![GeometryHandleId(1)], vec![GeometryHandleId(2)]],
            parent_edges: [vec![GeometryHandleId(3)], vec![GeometryHandleId(4)]],
            result_faces: vec![
                GeometryHandleId(11),
                GeometryHandleId(12),
                GeometryHandleId(13),
            ],
            result_edges: vec![GeometryHandleId(15)],
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_parent_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // 5 >= 2 parents tracked.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 5,
            parent_subshape_index: 0,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for parent_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("parent_index 5"),
                    "error message should mention the offending parent_index, got {msg:?}",
                );
                assert!(
                    msg.contains("face record"),
                    "error message should identify face record, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_parent_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Parent 0 has only 1 face, so subshape 99 is out of range.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 99,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for parent_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("face record"),
                    "face-record error message should identify face kind, got {msg:?}",
                );
                assert!(
                    msg.contains("parent_subshape_index 99"),
                    "error message should mention the offending parent_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_result_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Result has only 1 face, so subshape 7 is out of range.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 0,
            result_subshape_index: 7,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for result_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("face record"),
                    "face-record error message should identify face kind, got {msg:?}",
                );
                assert!(
                    msg.contains("result_subshape_index 7"),
                    "error message should mention the offending result_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_edge_record_has_parent_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Edge equivalent of the parent_index check — confirms the kind
        // arg is threaded into the error message.
        let history = history_with_single_edge_modified(HistoryRecord {
            parent_index: 4,
            parent_subshape_index: 0,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for edge parent_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("edge record"),
                    "edge-record error message should identify edge kind, got {msg:?}",
                );
                assert!(
                    msg.contains("parent_index 4"),
                    "error message should mention the offending parent_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_edge_record_has_parent_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Parent 0 has only 1 edge, so subshape 99 is out of range.
        let history = history_with_single_edge_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 99,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for edge parent_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("edge record"),
                    "edge-record error message should identify edge kind, got {msg:?}",
                );
                assert!(
                    msg.contains("parent_subshape_index 99"),
                    "error message should mention the offending parent_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_edge_record_has_result_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Result has only 1 edge, so subshape 7 is out of range.
        let history = history_with_single_edge_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 0,
            result_subshape_index: 7,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect_err("expected QueryFailed for edge result_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("edge record"),
                    "edge-record error message should identify edge kind, got {msg:?}",
                );
                assert!(
                    msg.contains("result_subshape_index 7"),
                    "error message should mention the offending result_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_succeeds_silently_on_empty_history() {
        // No records — propagation is a no-op and must not error even
        // when parent/result handle slices are empty.
        let mut table = TopologyAttributeTable::default();
        let parents = BooleanOpParents::nary(&[], &[]);
        let result_handles: Vec<GeometryHandleId> = Vec::new();
        let history = BooleanOpHistoryRecords::default();

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &result_handles,
            &result_handles,
            &history,
            &fuse_feature_id(),
        )
        .expect("empty history should propagate without error");
        assert!(table.is_empty(), "no-op propagation must not write entries");
    }

    #[test]
    fn no_records_binary_succeeds() {
        // Smoke-test: Binary variant + empty history must succeed and leave
        // the table empty — exercises the Binary accessor path through
        // propagation without hitting any error branch.
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };
        let history = BooleanOpHistoryRecords::default();

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect("empty history with Binary parents should propagate without error");
        assert!(table.is_empty(), "no-op propagation must not write entries");
    }

    /// step-5 — split detection unions `face_modified` ∪ `face_generated`
    /// per parent, and the iteration order is Modified-then-Generated.
    ///
    /// Parent (0, 0) appears in BOTH `face_modified = [(0, 0, 1)]` AND
    /// `face_generated = [(0, 0, 2)]` — total count = 2 across the union
    /// ⇒ split. The Modified record is encountered first (per the existing
    /// `chain(face_modified, face_generated)` order), so result_face[1]
    /// gets `split_index = 0` and result_face[2] gets `split_index = 1`.
    /// Both children must carry a fresh ModEntry stamping the
    /// splitting_feature_id.
    ///
    /// Pins both (a) the union semantics for split detection and (b) the
    /// per-kind iteration order Modified→Generated.
    #[test]
    fn propagate_split_combines_modified_and_generated_records_for_same_parent() {
        let mut table = TopologyAttributeTable::default();
        let layout = split_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        let parent_handle = layout.parent_faces[0][0];
        let parent_feature_id = FeatureId::new("Parent#realization[0]");
        table.record(
            parent_handle,
            TopologyAttribute {
                feature_id: parent_feature_id.clone(),
                role: Role::Side,
                local_index: 3,
                user_label: None,
                mod_history: Vec::new(),
            },
        );

        // Same parent (0, 0) appears once in face_modified and once in
        // face_generated — count == 2 across the union ⇒ split.
        let history = BooleanOpHistoryRecords {
            face_modified: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 1,
            }],
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 2,
            }],
            ..Default::default()
        };

        let splitting = fuse_feature_id();
        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &splitting,
        )
        .expect("propagation should succeed for a well-formed cross-kind split");

        // (a) result_faces[1] — first child (Modified record), split_index=0.
        let attr_modified = table
            .lookup(layout.result_faces[1])
            .expect("Modified child should have a propagated entry");
        assert_eq!(
            attr_modified.mod_history,
            vec![ModEntry {
                splitting_feature_id: splitting.clone(),
                split_index: 0,
            }],
            "Modified record is iterated before Generated; split_index = 0"
        );

        // (b) result_faces[2] — second child (Generated record), split_index=1.
        let attr_generated = table
            .lookup(layout.result_faces[2])
            .expect("Generated child should have a propagated entry");
        assert_eq!(
            attr_generated.mod_history,
            vec![ModEntry {
                splitting_feature_id: splitting,
                split_index: 1,
            }],
            "Generated record follows Modified in iteration order; split_index = 1"
        );
    }

    /// step-3 — single-result parent must NOT receive a fresh ModEntry.
    ///
    /// The parent has exactly one same-kind result record (`face_modified`
    /// only, no `face_generated` for that parent), so the count is 1 — not
    /// a split. The propagator must clone the parent attribute pure
    /// pass-through, preserving any prior `mod_history` exactly. This pins
    /// the `count > 1` guard in `maybe_append_split_entry`.
    #[test]
    fn propagate_skips_mod_entry_for_single_result_parent() {
        let mut table = TopologyAttributeTable::default();
        let layout = split_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Seed the parent with a NON-empty mod_history — the regression
        // pin is "preserves prior mod_history; new ModEntry only on splits".
        let parent_handle = layout.parent_faces[0][0];
        let parent_feature_id = FeatureId::new("Parent#realization[0]");
        let prior_mod_history = vec![ModEntry {
            splitting_feature_id: FeatureId::new("Earlier"),
            split_index: 5,
        }];
        table.record(
            parent_handle,
            TopologyAttribute {
                feature_id: parent_feature_id.clone(),
                role: Role::Side,
                local_index: 7,
                user_label: None,
                mod_history: prior_mod_history.clone(),
            },
        );

        // Exactly ONE face_modified record for this parent; empty
        // face_generated. count((0,0)) = 1 ⇒ no split, no new ModEntry.
        let history = BooleanOpHistoryRecords {
            face_modified: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 1,
            }],
            ..Default::default()
        };

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &fuse_feature_id(),
        )
        .expect("propagation should succeed for a well-formed single-result history");

        let attr = table
            .lookup(layout.result_faces[1])
            .expect("result face 1 should have a propagated entry");
        assert_eq!(attr.feature_id, parent_feature_id);
        assert_eq!(attr.role, Role::Side);
        assert_eq!(attr.local_index, 7);
        assert!(attr.user_label.is_none());
        assert_eq!(
            attr.mod_history, prior_mod_history,
            "single-result parent (count==1) must propagate prior mod_history unchanged",
        );
    }

    /// step-1 (RED) — split parent with two `face_modified` records.
    ///
    /// A single parent face (parent 0, subshape 0) is mapped to TWO distinct
    /// result faces by the boolean op. Per the v0.2 invariant: each child
    /// inherits the parent's `(feature_id, role, local_index, user_label)`
    /// AND gets a fresh `ModEntry { splitting_feature_id, split_index }`
    /// appended to its `mod_history`. The first child (record 0) gets
    /// `split_index = 0`; the second (record 1) gets `split_index = 1`.
    #[test]
    fn propagate_appends_mod_entry_for_split_parent_with_two_face_modified_records() {
        let mut table = TopologyAttributeTable::default();
        let layout = split_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Seed the parent face with a non-empty user_label (so we can pin the
        // user_label is also propagated unchanged).
        let parent_handle = layout.parent_faces[0][0];
        let parent_feature_id = FeatureId::new("Parent#realization[0]");
        table.record(
            parent_handle,
            TopologyAttribute {
                feature_id: parent_feature_id.clone(),
                role: Role::Side,
                local_index: 7,
                user_label: Some("seam".to_string()),
                mod_history: Vec::new(),
            },
        );

        // Two face_modified records pointing the SAME parent at two distinct
        // result faces — the split signature.
        let history = BooleanOpHistoryRecords {
            face_modified: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 1,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 2,
                },
            ],
            ..Default::default()
        };

        let splitting = fuse_feature_id();
        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &splitting,
        )
        .expect("propagation should succeed for a well-formed split history");

        // (a) result_faces[1] — first child, split_index = 0.
        let attr_1 = table
            .lookup(layout.result_faces[1])
            .expect("result face 1 should have a propagated entry");
        assert_eq!(attr_1.feature_id, parent_feature_id);
        assert_eq!(attr_1.role, Role::Side);
        assert_eq!(attr_1.local_index, 7);
        assert_eq!(attr_1.user_label, Some("seam".to_string()));
        assert_eq!(
            attr_1.mod_history,
            vec![ModEntry {
                splitting_feature_id: splitting.clone(),
                split_index: 0,
            }],
            "first child of split parent must carry split_index=0"
        );

        // (b) result_faces[2] — second child, split_index = 1.
        let attr_2 = table
            .lookup(layout.result_faces[2])
            .expect("result face 2 should have a propagated entry");
        assert_eq!(attr_2.feature_id, parent_feature_id);
        assert_eq!(attr_2.role, Role::Side);
        assert_eq!(attr_2.local_index, 7);
        assert_eq!(attr_2.user_label, Some("seam".to_string()));
        assert_eq!(
            attr_2.mod_history,
            vec![ModEntry {
                splitting_feature_id: splitting,
                split_index: 1,
            }],
            "second child of split parent must carry split_index=1"
        );
    }

    // -- populate_extrude_attributes tests (task 5a, step-11) --
    //
    // The helper originates new attributes for an extrude result: cap faces
    // get `Role::Cap(CapKind::Top|Bottom)` with local_index 0; lateral faces
    // get `Role::Side` with sequential 0-based local_index in face_generated
    // order. Profile face/edge slices are passed in for defense-in-depth
    // index-range validation.

    /// Profile + result handle vectors for a 1-parent extrude layout.
    /// Owned so the test fn can borrow slices without temporary-lifetime
    /// issues.
    struct ExtrudeLayout {
        profile_faces: Vec<GeometryHandleId>,
        profile_edges: Vec<GeometryHandleId>,
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    }

    /// Layout for a rect-face extrude: 1 profile face, 4 profile edges,
    /// 9 result faces (indices 0..=8 → 5 = start cap, 6 = end cap, 7/8
    /// = side faces), 12 result edges.
    fn extrude_layout_for_step11() -> ExtrudeLayout {
        ExtrudeLayout {
            profile_faces: vec![GeometryHandleId(101)],
            profile_edges: vec![
                GeometryHandleId(201),
                GeometryHandleId(202),
                GeometryHandleId(203),
                GeometryHandleId(204),
            ],
            result_faces: (0..9).map(|i| GeometryHandleId(1000 + i)).collect(),
            result_edges: (0..12).map(|i| GeometryHandleId(2000 + i)).collect(),
        }
    }

    /// Synthetic SweepOpHistoryRecords matching the step-11 spec:
    /// start_cap = [5], end_cap = [6], face_generated = [(0,0,7), (0,1,8)],
    /// every other vector empty.
    fn step11_extrude_history() -> SweepOpHistoryRecords {
        SweepOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 7,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 8,
                },
            ],
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        }
    }

    #[test]
    fn populate_extrude_writes_cap_top_for_start_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-11 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[5])
            .expect("start_cap_face_indices[0] = 5 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Top));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_extrude_writes_cap_bottom_for_end_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-11 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[6])
            .expect("end_cap_face_indices[0] = 6 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Bottom));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_extrude_writes_side_with_sequential_local_index_for_face_generated() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-11 history is well-formed");

        let side_a = table
            .lookup(layout.result_faces[7])
            .expect("face_generated[0].result_subshape_index = 7 should have an entry");
        assert_eq!(side_a.role, Role::Side);
        assert_eq!(side_a.local_index, 0);
        assert_eq!(side_a.feature_id, feature_id);
        assert!(side_a.mod_history.is_empty());
        assert!(side_a.user_label.is_none());

        let side_b = table
            .lookup(layout.result_faces[8])
            .expect("face_generated[1].result_subshape_index = 8 should have an entry");
        assert_eq!(side_b.role, Role::Side);
        assert_eq!(side_b.local_index, 1);
        assert_eq!(side_b.feature_id, feature_id);
        assert!(side_b.mod_history.is_empty());
        assert!(side_b.user_label.is_none());
    }

    #[test]
    fn populate_extrude_does_not_write_to_result_face_indices_not_in_records() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-11 history is well-formed");

        // Only indices 5, 6, 7, 8 are referenced; 0..=4 must remain unkeyed.
        for unkeyed_idx in [0_usize, 1, 2, 3, 4] {
            assert!(
                table.lookup(layout.result_faces[unkeyed_idx]).is_none(),
                "result face index {unkeyed_idx} should have no attribute entry",
            );
        }
        assert_eq!(
            table.len(),
            4,
            "only the 2 cap faces and 2 side faces should be keyed",
        );
    }

    #[test]
    fn populate_extrude_returns_query_failed_when_start_cap_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![99], // result has only 9 faces.
            ..Default::default()
        };

        let err = populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range start_cap index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("99"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_extrude_returns_query_failed_when_face_generated_result_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 42, // > result faces (9).
            }],
            ..Default::default()
        };

        let err = populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range result_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("42"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_extrude_empty_history_is_a_noop() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords::default();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("empty history is a no-op");
        assert!(table.is_empty());
    }

    // -- populate_revolve_attributes tests (task 5a, step-13) --
    //
    // Mirrors the extrude helper but with revolve-specific role
    // assignments: `start_cap_face_indices` → `Cap(Start)`,
    // `end_cap_face_indices` → `Cap(End)`, `face_generated` →
    // `RevolvedFace` (NOT `Side`). Empty cap lists encode full-2π
    // revolutions (no cap faces).

    /// Layout for a half-rect-face revolve: 1 profile face, 4 profile
    /// edges (axis-side + far-side + 2 perpendiculars), 8 result faces
    /// (indices 0..=7), 16 result edges. Sized to fit step-13's
    /// (a) PARTIAL fixture (start=2, end=3, face_generated=4..=7).
    fn revolve_layout_for_step13() -> ExtrudeLayout {
        ExtrudeLayout {
            profile_faces: vec![GeometryHandleId(301)],
            profile_edges: vec![
                GeometryHandleId(401),
                GeometryHandleId(402),
                GeometryHandleId(403),
                GeometryHandleId(404),
            ],
            result_faces: (0..8).map(|i| GeometryHandleId(3000 + i)).collect(),
            result_edges: (0..16).map(|i| GeometryHandleId(4000 + i)).collect(),
        }
    }

    /// Synthetic SweepOpHistoryRecords matching the step-13 (a) PARTIAL
    /// fixture: start_cap = [2], end_cap = [3], face_generated =
    /// [(0,0,4), (0,1,5), (0,2,6), (0,3,7)].
    fn step13_partial_revolve_history() -> SweepOpHistoryRecords {
        SweepOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 4,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 5,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 2,
                    result_subshape_index: 6,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 3,
                    result_subshape_index: 7,
                },
            ],
            start_cap_face_indices: vec![2],
            end_cap_face_indices: vec![3],
            ..Default::default()
        }
    }

    #[test]
    fn populate_partial_revolve_writes_cap_start_and_cap_end_for_cap_indices() {
        let mut table = TopologyAttributeTable::default();
        let layout = revolve_layout_for_step13();
        let feature_id = FeatureId::new("Bowl#realization[0]");
        let history = step13_partial_revolve_history();

        populate_revolve_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-13 partial-revolve history is well-formed");

        let start_cap = table
            .lookup(layout.result_faces[2])
            .expect("start_cap_face_indices[0] = 2 should have an entry");
        assert_eq!(start_cap.role, Role::Cap(CapKind::Start));
        assert_eq!(start_cap.local_index, 0);
        assert_eq!(start_cap.feature_id, feature_id);
        assert!(start_cap.user_label.is_none());
        assert!(start_cap.mod_history.is_empty());

        let end_cap = table
            .lookup(layout.result_faces[3])
            .expect("end_cap_face_indices[0] = 3 should have an entry");
        assert_eq!(end_cap.role, Role::Cap(CapKind::End));
        assert_eq!(end_cap.local_index, 0);
        assert_eq!(end_cap.feature_id, feature_id);
        assert!(end_cap.user_label.is_none());
        assert!(end_cap.mod_history.is_empty());
    }

    #[test]
    fn populate_partial_revolve_writes_revolved_face_with_sequential_local_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = revolve_layout_for_step13();
        let feature_id = FeatureId::new("Bowl#realization[0]");
        let history = step13_partial_revolve_history();

        populate_revolve_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-13 partial-revolve history is well-formed");

        for (sequential_idx, result_face_idx) in [4_usize, 5, 6, 7].iter().enumerate() {
            let attr = table
                .lookup(layout.result_faces[*result_face_idx])
                .unwrap_or_else(|| {
                    panic!(
                        "face_generated[{sequential_idx}].result_subshape_index = \
                         {result_face_idx} should have an entry"
                    )
                });
            assert_eq!(
                attr.role,
                Role::RevolvedFace,
                "revolve face_generated must use Role::RevolvedFace not Role::Side",
            );
            assert_eq!(attr.local_index, sequential_idx as u32);
            assert_eq!(attr.feature_id, feature_id);
            assert!(attr.user_label.is_none());
            assert!(attr.mod_history.is_empty());
        }
    }

    #[test]
    fn populate_full_revolve_writes_only_revolved_face_no_caps() {
        // FULL-2π revolve fixture: empty cap lists, face_generated only.
        let mut table = TopologyAttributeTable::default();
        let layout = revolve_layout_for_step13();
        let feature_id = FeatureId::new("Bowl#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 0,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 1,
                },
            ],
            start_cap_face_indices: vec![],
            end_cap_face_indices: vec![],
            ..Default::default()
        };

        populate_revolve_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-13 full-revolve history is well-formed");

        assert_eq!(
            table.len(),
            2,
            "full-2π revolve has no caps; only the 2 revolved faces are keyed",
        );

        for (sequential_idx, result_face_idx) in [0_usize, 1].iter().enumerate() {
            let attr = table
                .lookup(layout.result_faces[*result_face_idx])
                .expect("expected revolved face entry");
            assert_eq!(attr.role, Role::RevolvedFace);
            assert_eq!(attr.local_index, sequential_idx as u32);
        }
    }

    #[test]
    fn populate_revolve_returns_query_failed_when_start_cap_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = revolve_layout_for_step13();
        let feature_id = FeatureId::new("Bowl#realization[0]");
        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![123], // result has only 8 faces.
            ..Default::default()
        };

        let err = populate_revolve_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range start_cap index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("123"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_revolve_returns_query_failed_when_face_generated_result_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = revolve_layout_for_step13();
        let feature_id = FeatureId::new("Bowl#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 256, // > result faces (8).
            }],
            ..Default::default()
        };

        let err = populate_revolve_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range result_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("256"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    // -- populate_sweep_attributes tests (task 5b / #2619, step-7) --
    //
    // Mirrors the extrude helper but with sweep-specific role assignments:
    // `start_cap_face_indices` → `Cap(Start)`, `end_cap_face_indices` →
    // `Cap(End)` (parametric Start/End semantics, NOT extrude's Top/Bottom),
    // `face_generated` → `SweptFace` (NOT `Side` — per-op distinguisher
    // per task-5a design decisions, mirrored for 5b in geometry.rs).
    // Sweep is single-parent like extrude/revolve so reuses
    // `SweepOpHistoryRecords` verbatim.

    /// Layout for a rect-face sweep: 1 profile face, 4 profile edges,
    /// 9 result faces, 12 result edges. Same shape as the extrude
    /// fixture; sweep produces an identical topology under a straight
    /// spine (rect profile + linear path → rect prism).
    fn sweep_layout_for_step7() -> ExtrudeLayout {
        ExtrudeLayout {
            profile_faces: vec![GeometryHandleId(501)],
            profile_edges: vec![
                GeometryHandleId(601),
                GeometryHandleId(602),
                GeometryHandleId(603),
                GeometryHandleId(604),
            ],
            result_faces: (0..9).map(|i| GeometryHandleId(5000 + i)).collect(),
            result_edges: (0..12).map(|i| GeometryHandleId(6000 + i)).collect(),
        }
    }

    /// Synthetic SweepOpHistoryRecords for the step-7 happy path:
    /// start_cap = [5], end_cap = [6], face_generated = [(0,0,7), (0,1,8)].
    fn step7_sweep_history() -> SweepOpHistoryRecords {
        SweepOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 7,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 8,
                },
            ],
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        }
    }

    #[test]
    fn populate_sweep_writes_cap_start_for_start_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = step7_sweep_history();

        populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-7 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[5])
            .expect("start_cap_face_indices[0] = 5 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Start));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_sweep_writes_cap_end_for_end_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = step7_sweep_history();

        populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-7 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[6])
            .expect("end_cap_face_indices[0] = 6 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::End));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_sweep_writes_swept_face_with_sequential_local_index_for_face_generated() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = step7_sweep_history();

        populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-7 history is well-formed");

        let side_a = table
            .lookup(layout.result_faces[7])
            .expect("face_generated[0].result_subshape_index = 7 should have an entry");
        assert_eq!(side_a.role, Role::SweptFace);
        assert_eq!(side_a.local_index, 0);
        assert_eq!(side_a.feature_id, feature_id);
        assert!(side_a.mod_history.is_empty());
        assert!(side_a.user_label.is_none());

        let side_b = table
            .lookup(layout.result_faces[8])
            .expect("face_generated[1].result_subshape_index = 8 should have an entry");
        assert_eq!(side_b.role, Role::SweptFace);
        assert_eq!(side_b.local_index, 1);
        assert_eq!(side_b.feature_id, feature_id);
        assert!(side_b.mod_history.is_empty());
        assert!(side_b.user_label.is_none());
    }

    #[test]
    fn populate_sweep_empty_history_is_a_noop() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = SweepOpHistoryRecords::default();

        populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("empty history is a no-op");
        assert!(table.is_empty());
    }

    #[test]
    fn populate_sweep_returns_query_failed_when_start_cap_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![99], // result has only 9 faces.
            ..Default::default()
        };

        let err = populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range start_cap index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("99"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_sweep_returns_query_failed_when_face_generated_result_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 42, // > result faces (9).
            }],
            ..Default::default()
        };

        let err = populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range result_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("42"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_sweep_returns_query_failed_when_parent_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = sweep_layout_for_step7();
        let feature_id = FeatureId::new("Pipe#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 99, // > profile edges (4).
                result_subshape_index: 7,
            }],
            ..Default::default()
        };

        let err = populate_sweep_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range parent_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("99"),
                    "error should mention out-of-range parent_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    // -- populate_loft_attributes tests (task 5b / #2619, step-9) --
    //
    // Loft is the multi-parent variant: `parent_index` denotes a section
    // index in `[0, profiles.len())`, and `parent_subshape_index` is the
    // edge index within that section's edge map. `populate_loft_attributes`
    // takes per-section profile face/edge slices (`&[Vec<GeometryHandleId>]`)
    // and validates each `face_generated` record's
    // `(parent_index, parent_subshape_index)` pair against the addressed
    // section. Caps reuse `Role::Cap(CapKind::Start)` / `Role::Cap(CapKind::End)`
    // (loft section 0 = start; loft section N-1 = end under `is_solid=true`).
    // `face_generated` records emit `Role::LoftedFace` (NOT `SweptFace` /
    // `RevolvedFace` / `Side` — per-op distinguisher per task-5a/5b design).
    // `local_index` is sequential across all sections in the order records
    // appear in `face_generated`.

    /// Layout for a 2-section loft: each section has 1 profile face + 2
    /// profile edges; result has 6 faces (cap_start + cap_end + 4 lateral)
    /// and 8 result edges. Sized for the step-9 happy-path fixture.
    struct LoftLayout {
        section_faces: Vec<Vec<GeometryHandleId>>,
        section_edges: Vec<Vec<GeometryHandleId>>,
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    }

    fn loft_layout_for_step9() -> LoftLayout {
        LoftLayout {
            // Two sections; each has 1 profile face and 2 profile edges.
            section_faces: vec![vec![GeometryHandleId(701)], vec![GeometryHandleId(702)]],
            section_edges: vec![
                vec![GeometryHandleId(801), GeometryHandleId(802)],
                vec![GeometryHandleId(803), GeometryHandleId(804)],
            ],
            // 6 result faces: indices 0/1 = caps Start/End, 2..=5 = lateral.
            result_faces: (0..6).map(|i| GeometryHandleId(7000 + i)).collect(),
            result_edges: (0..8).map(|i| GeometryHandleId(8000 + i)).collect(),
        }
    }

    /// Synthetic LoftOpHistoryRecords for the step-9 happy path:
    /// start_cap = [0], end_cap = [1], face_generated =
    /// [(0,0,2), (0,1,3), (1,0,4), (1,1,5)] (sequential across sections).
    fn step9_loft_history() -> LoftOpHistoryRecords {
        LoftOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 2,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 3,
                },
                HistoryRecord {
                    parent_index: 1,
                    parent_subshape_index: 0,
                    result_subshape_index: 4,
                },
                HistoryRecord {
                    parent_index: 1,
                    parent_subshape_index: 1,
                    result_subshape_index: 5,
                },
            ],
            start_cap_face_indices: vec![0],
            end_cap_face_indices: vec![1],
            ..Default::default()
        }
    }

    #[test]
    fn populate_loft_writes_cap_start_for_start_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = step9_loft_history();

        populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-9 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[0])
            .expect("start_cap_face_indices[0] = 0 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Start));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_loft_writes_cap_end_for_end_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = step9_loft_history();

        populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-9 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[1])
            .expect("end_cap_face_indices[0] = 1 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::End));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_loft_writes_lofted_face_with_sequential_local_index_across_sections() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = step9_loft_history();

        populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("step-9 history is well-formed");

        // local_index increments sequentially across all sections in the
        // order face_generated records appear (sections [0][0], [0][1],
        // [1][0], [1][1] → indices 0,1,2,3).
        for (sequential_idx, result_face_idx) in [2_usize, 3, 4, 5].iter().enumerate() {
            let attr = table
                .lookup(layout.result_faces[*result_face_idx])
                .unwrap_or_else(|| {
                    panic!(
                        "face_generated[{sequential_idx}].result_subshape_index = \
                         {result_face_idx} should have an entry"
                    )
                });
            assert_eq!(
                attr.role,
                Role::LoftedFace,
                "loft face_generated must use Role::LoftedFace not Role::Side/Sweep/Revolved",
            );
            assert_eq!(attr.local_index, sequential_idx as u32);
            assert_eq!(attr.feature_id, feature_id);
            assert!(attr.user_label.is_none());
            assert!(attr.mod_history.is_empty());
        }
    }

    #[test]
    fn populate_loft_empty_history_is_a_noop() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = LoftOpHistoryRecords::default();

        populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect("empty history is a no-op");
        assert!(table.is_empty());
    }

    #[test]
    fn populate_loft_returns_query_failed_when_parent_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = LoftOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 9, // > sections (2).
                parent_subshape_index: 0,
                result_subshape_index: 2,
            }],
            ..Default::default()
        };

        let err = populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range parent_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("9"),
                    "error should mention out-of-range parent_index, got {msg:?}",
                );
                assert!(
                    msg.to_lowercase().contains("section"),
                    "error should mention 'section', got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_loft_returns_query_failed_when_parent_subshape_index_out_of_range_for_section() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        // Section 0 has 2 edges; index 7 is out of range.
        let history = LoftOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 7,
                result_subshape_index: 2,
            }],
            ..Default::default()
        };

        let err = populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range parent_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("7"),
                    "error should mention out-of-range parent_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_loft_returns_query_failed_when_face_generated_result_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = LoftOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 99, // > result faces (6).
            }],
            ..Default::default()
        };

        let err = populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range result_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("99"),
                    "error should mention out-of-range result_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_loft_returns_query_failed_when_start_cap_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = loft_layout_for_step9();
        let feature_id = FeatureId::new("Loft#realization[0]");
        let history = LoftOpHistoryRecords {
            start_cap_face_indices: vec![123], // > result faces (6).
            ..Default::default()
        };

        let err = populate_loft_attributes(
            &mut table,
            &feature_id,
            &layout.section_faces,
            &layout.section_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
            &[],
            &[],
            &[],
        )
        .expect_err("expected QueryFailed for out-of-range start_cap index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("123"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn populate_loft_panics_on_asymmetric_face_edge_slice_counts() {
        // Parameterised regression test: the lockstep `debug_assert_eq!` at the top of
        // `populate_loft_attributes` fires for any (section_faces_count, section_edges_count)
        // asymmetry regardless of which side is longer.  Testing both directions and zero-cases
        // ensures a future refactor that weakens the check to `<=` or `>=` is caught.
        for (nfaces, nedges) in [(2_usize, 1_usize), (1, 2), (3, 0), (0, 3)] {
            let mut table = TopologyAttributeTable::default();
            let feature_id = FeatureId::new("Loft#realization[0]");
            let section_faces: Vec<Vec<GeometryHandleId>> = (0..nfaces)
                .map(|i| vec![GeometryHandleId(700_u64 + i as u64)])
                .collect();
            let section_edges: Vec<Vec<GeometryHandleId>> = (0..nedges)
                .map(|i| vec![GeometryHandleId(800_u64 + i as u64)])
                .collect();
            let result_faces = vec![GeometryHandleId(7000)];
            let result_edges: Vec<GeometryHandleId> = vec![];
            let history = LoftOpHistoryRecords::default();

            let call_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = populate_loft_attributes(
                    &mut table,
                    &feature_id,
                    &section_faces,
                    &section_edges,
                    &result_faces,
                    &result_edges,
                    &history,
                    &[],
                    &[],
                    &[],
                );
            }));
            assert!(
                call_result.is_err(),
                "expected lockstep panic for (faces={nfaces}, edges={nedges}) but none fired"
            );
            let payload = call_result.unwrap_err();
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-string panic payload>");
            assert!(
                msg.contains("loft section face/edge slice families must be built in lockstep"),
                "wrong panic for (faces={nfaces}, edges={nedges}): {msg:?}"
            );
        }
    }

    // --- detect_local_index_reassignment_diagnostics (PRD task 4 / #2654) ---
    //
    // The helper is pure Rust over (handles_with_attrs, centroid_map): no
    // OCCT kernel needed. Tests below construct synthetic input slices and
    // centroid maps, call the helper, and assert on the emitted diagnostics.
    mod detect_local_index_reassignment {
        use std::collections::HashMap;

        use reify_core::SourceSpan;
        use reify_ir::{CapKind, FeatureId, GeometryHandleId, ModEntry, Role, TopologyAttribute};

        use super::super::{
            LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M, detect_local_index_reassignment_diagnostics,
        };

        /// Synthetic span used by every detect_* test — pinning a stable
        /// (start, end) pair so message assertions can pattern-match against it.
        fn synthetic_span() -> SourceSpan {
            SourceSpan::new(10, 20)
        }

        /// Build a TopologyAttribute with the given (feature_id, role, local_index)
        /// and an empty mod_history (the non-split case detection covers).
        fn make_attr(feature: &str, role: Role, local_index: u32) -> TopologyAttribute {
            TopologyAttribute {
                feature_id: FeatureId::new(feature),
                role,
                local_index,
                user_label: None,
                mod_history: Vec::new(),
            }
        }

        /// Build a TopologyAttribute with a single ModEntry in mod_history
        /// (the post-split-cluster case detection must skip).
        fn make_attr_with_split(
            feature: &str,
            role: Role,
            local_index: u32,
            split_index: u32,
        ) -> TopologyAttribute {
            TopologyAttribute {
                feature_id: FeatureId::new(feature),
                role,
                local_index,
                user_label: None,
                mod_history: vec![ModEntry {
                    splitting_feature_id: FeatureId::new("Cut#realization[1]"),
                    split_index,
                }],
            }
        }

        #[test]
        fn detect_local_index_reassignment_emits_no_diagnostic_on_empty_input() {
            let centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert!(diagnostics.is_empty());
        }

        #[test]
        fn detect_local_index_reassignment_emits_no_diagnostic_for_singleton_role_group() {
            let attr = make_attr("F#realization[0]", Role::Side, 0);
            let h = GeometryHandleId(1);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            centroids.insert(h, [1.0, 2.0, 3.0]);
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[(h, &attr)],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            // Singleton group: no pairwise comparison to do → no diagnostic.
            assert!(diagnostics.is_empty());
        }

        #[test]
        fn detect_local_index_reassignment_emits_diagnostic_when_two_entries_have_tied_centroids() {
            use reify_core::Severity;
            let attr0 = make_attr("F#realization[0]", Role::Side, 0);
            let attr1 = make_attr("F#realization[0]", Role::Side, 1);
            let h0 = GeometryHandleId(1);
            let h1 = GeometryHandleId(2);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            // Identical centroids → squared distance == 0 ≤ tol_m^2.
            centroids.insert(h0, [1.0, 2.0, 3.0]);
            centroids.insert(h1, [1.0, 2.0, 3.0]);
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[(h0, &attr0), (h1, &attr1)],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
            let diag = &diagnostics[0];
            assert_eq!(diag.severity, Severity::Warning);
            assert_eq!(
                diag.code,
                Some(reify_core::DiagnosticCode::TopologyAttributeLocalIndexReassigned)
            );
            assert!(
                diag.message.contains("topology-attribute selector for"),
                "missing canonical prefix in message: {}",
                diag.message
            );
            assert!(
                diag.message.contains("F#realization[0]"),
                "missing feature_id in message: {}",
                diag.message
            );
            assert!(
                diag.message.contains("Side"),
                "missing role in message: {}",
                diag.message
            );
            assert!(
                diag.message
                    .contains("local_index assignments at indices 0 and 1"),
                "missing tied indices in message: {}",
                diag.message
            );
            // Label should span the realization span with text
            // "realization producing geometrically tied attributes".
            assert_eq!(diag.labels.len(), 1, "expected exactly one label");
            let label = &diag.labels[0];
            assert_eq!(label.span, synthetic_span());
            assert_eq!(
                label.message,
                "realization producing geometrically tied attributes"
            );
        }

        #[test]
        fn detect_local_index_reassignment_skips_entries_with_non_empty_mod_history() {
            // PRD line 72: "not because of a split — splits are handled by
            // mod_history". Post-split clusters are surfaced via
            // TopologyAttributeAmbiguousAfterSplit at resolve time; this helper
            // must NOT double-warn the user about the same fragility under a
            // different code.
            let attr0 = make_attr_with_split("F#realization[0]", Role::Side, 0, 0);
            let attr1 = make_attr_with_split("F#realization[0]", Role::Side, 1, 1);
            let h0 = GeometryHandleId(1);
            let h1 = GeometryHandleId(2);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            // Identical centroids — would normally trip the helper, but
            // mod_history non-empty must short-circuit the entry.
            centroids.insert(h0, [1.0, 2.0, 3.0]);
            centroids.insert(h1, [1.0, 2.0, 3.0]);
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[(h0, &attr0), (h1, &attr1)],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert!(
                diagnostics.is_empty(),
                "expected no diagnostic — split-cluster entries must be skipped, got: {diagnostics:?}"
            );
        }

        #[test]
        fn detect_local_index_reassignment_does_not_emit_when_centroids_are_well_separated() {
            // Same two-entry (feature_id, role) group as the tied-centroids
            // test, but centroids differ by [10.0, 0.0, 0.0] — Euclidean
            // distance 10 m, vastly above any reasonable tolerance. Kernel
            // enumeration order ties to their geometric distinctness, so
            // local_index assignment is stable across edits and there is
            // nothing to warn about. Regression guard for the squared-distance
            // threshold check.
            let attr0 = make_attr("F#realization[0]", Role::Side, 0);
            let attr1 = make_attr("F#realization[0]", Role::Side, 1);
            let h0 = GeometryHandleId(1);
            let h1 = GeometryHandleId(2);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            centroids.insert(h0, [0.0, 0.0, 0.0]);
            centroids.insert(h1, [10.0, 0.0, 0.0]);
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[(h0, &attr0), (h1, &attr1)],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert!(
                diagnostics.is_empty(),
                "expected no diagnostic — well-separated centroids pose no fragility, got: {diagnostics:?}"
            );
        }

        #[test]
        fn detect_local_index_reassignment_emits_at_most_one_diagnostic_per_role_group() {
            // Three entries in the same (feature_id, role) group with
            // local_index ∈ {0, 1, 2}, all sharing the SAME centroid. All
            // three pairs (0,1), (0,2), (1,2) would trigger detection — but
            // the helper must emit exactly ONE diagnostic per group, naming
            // the smallest tied pair (indices 0 and 1).
            let attr0 = make_attr("F#realization[0]", Role::Side, 0);
            let attr1 = make_attr("F#realization[0]", Role::Side, 1);
            let attr2 = make_attr("F#realization[0]", Role::Side, 2);
            let h0 = GeometryHandleId(1);
            let h1 = GeometryHandleId(2);
            let h2 = GeometryHandleId(3);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            centroids.insert(h0, [0.0, 0.0, 0.0]);
            centroids.insert(h1, [0.0, 0.0, 0.0]);
            centroids.insert(h2, [0.0, 0.0, 0.0]);
            let mut diagnostics = Vec::new();
            detect_local_index_reassignment_diagnostics(
                &[(h0, &attr0), (h1, &attr1), (h2, &attr2)],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "expected exactly one diagnostic per role group, got: {diagnostics:?}"
            );
            let diag = &diagnostics[0];
            assert!(
                diag.message
                    .contains("local_index assignments at indices 0 and 1"),
                "expected the smallest tied pair (0 and 1) to be named, got: {}",
                diag.message
            );
        }

        #[test]
        fn detect_local_index_reassignment_emits_diagnostics_in_deterministic_order_across_groups()
        {
            // Sub-case 1: two groups with different feature_ids — "A" < "B"
            // alphabetically, so group A's diagnostic must be emitted first.
            let attr_b0 = make_attr("B#realization[0]", Role::Side, 0);
            let attr_b1 = make_attr("B#realization[0]", Role::Side, 1);
            let attr_a0 = make_attr("A#realization[0]", Role::Side, 0);
            let attr_a1 = make_attr("A#realization[0]", Role::Side, 1);
            let hb0 = GeometryHandleId(20);
            let hb1 = GeometryHandleId(21);
            let ha0 = GeometryHandleId(22);
            let ha1 = GeometryHandleId(23);
            let mut centroids: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            // Give B-group a different centroid so the two groups are distinct.
            centroids.insert(hb0, [5.0, 0.0, 0.0]);
            centroids.insert(hb1, [5.0, 0.0, 0.0]);
            centroids.insert(ha0, [0.0, 0.0, 0.0]);
            centroids.insert(ha1, [0.0, 0.0, 0.0]);
            let mut diagnostics = Vec::new();
            // Supply inputs in B, A order (reversed) to exercise the sort.
            detect_local_index_reassignment_diagnostics(
                &[
                    (hb0, &attr_b0),
                    (hb1, &attr_b1),
                    (ha0, &attr_a0),
                    (ha1, &attr_a1),
                ],
                &centroids,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics,
            );
            assert_eq!(
                diagnostics.len(),
                2,
                "expected two diagnostics, got: {diagnostics:?}"
            );
            assert!(
                diagnostics[0].message.contains("A#realization[0]"),
                "first diagnostic should be for A (sorted first), got: {}",
                diagnostics[0].message
            );
            assert!(
                diagnostics[1].message.contains("B#realization[0]"),
                "second diagnostic should be for B (sorted second), got: {}",
                diagnostics[1].message
            );

            // Sub-case 2: same feature_id, two roles — Cap(Top) has discriminant
            // 0, Side has discriminant 4, so Cap diagnostic must come first.
            let attr_s0 = make_attr("F#realization[0]", Role::Side, 0);
            let attr_s1 = make_attr("F#realization[0]", Role::Side, 1);
            let attr_c0 = make_attr("F#realization[0]", Role::Cap(CapKind::Top), 0);
            let attr_c1 = make_attr("F#realization[0]", Role::Cap(CapKind::Top), 1);
            let hs0 = GeometryHandleId(30);
            let hs1 = GeometryHandleId(31);
            let hc0 = GeometryHandleId(32);
            let hc1 = GeometryHandleId(33);
            let mut centroids2: HashMap<GeometryHandleId, [f64; 3]> = HashMap::new();
            centroids2.insert(hs0, [0.0, 0.0, 0.0]);
            centroids2.insert(hs1, [0.0, 0.0, 0.0]);
            centroids2.insert(hc0, [0.0, 0.0, 0.0]);
            centroids2.insert(hc1, [0.0, 0.0, 0.0]);
            let mut diagnostics2 = Vec::new();
            // Supply Side entries before Cap entries to exercise role sort.
            detect_local_index_reassignment_diagnostics(
                &[
                    (hs0, &attr_s0),
                    (hs1, &attr_s1),
                    (hc0, &attr_c0),
                    (hc1, &attr_c1),
                ],
                &centroids2,
                LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M,
                synthetic_span(),
                &mut diagnostics2,
            );
            assert_eq!(
                diagnostics2.len(),
                2,
                "expected two diagnostics, got: {diagnostics2:?}"
            );
            assert!(
                diagnostics2[0].message.contains("Cap(Top)"),
                "first diagnostic should be Cap(Top) (lower discriminant), got: {}",
                diagnostics2[0].message
            );
            assert!(
                diagnostics2[1].message.contains("Side"),
                "second diagnostic should be Side (higher discriminant), got: {}",
                diagnostics2[1].message
            );
        }
    }

    // ── task-3633 step-5: cap-vertex emission tests (Phase C) ─────────────────
    //
    // These four synthetic tests pin the CapCornerVertex emission contract for
    // each of the four populate_* helpers.  They use fabricated handles and do
    // not require an OCCT kernel.  The new three vertex-related args
    // (result_vertex_handles, start_cap_vertex_index_lists,
    // end_cap_vertex_index_lists) are added at the END of each signature;
    // the tests fail to compile until step-6 widens the implementations.

    #[test]
    fn populate_extrude_attributes_emits_cap_corner_vertex_for_top_and_bottom() {
        let mut table = TopologyAttributeTable::default();
        let feature_id = FeatureId::new("Extrude#realization[0]");

        // 7 result faces: indices 5 = start cap (Top), 6 = end cap (Bottom).
        let result_faces: Vec<GeometryHandleId> =
            (0..7).map(|i| GeometryHandleId(1000 + i)).collect();
        let result_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(2000)];
        let profile_faces: Vec<GeometryHandleId> = vec![GeometryHandleId(3000)];
        let profile_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(3001)];

        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        };

        // 8 result vertices: 0..3 belong to the start cap, 4..7 to the end cap.
        let result_vertices: Vec<GeometryHandleId> =
            (0..8).map(|i| GeometryHandleId(4000 + i)).collect();
        let start_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![0, 1, 2, 3]];
        let end_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![4, 5, 6, 7]];

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            &history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        )
        .expect("well-formed extrude history + vertex lists should succeed");

        // Vertices 0..3 → CapCornerVertex { face: Top }, local_index 0..3.
        for i in 0u32..4 {
            let handle = GeometryHandleId(4000 + i as u64);
            let attr = table
                .lookup(handle)
                .unwrap_or_else(|| panic!("start-cap vertex #{i} must have an entry"));
            assert_eq!(
                attr.role,
                Role::CapCornerVertex { face: CapKind::Top },
                "start-cap vertex #{i} must be CapCornerVertex{{Top}}"
            );
            assert_eq!(
                attr.local_index, i,
                "start-cap vertex #{i} local_index must equal its per-cap position {i}"
            );
            assert_eq!(attr.feature_id, feature_id);
            assert!(attr.user_label.is_none());
            assert!(attr.mod_history.is_empty());
        }

        // Vertices 4..7 → CapCornerVertex { face: Bottom }, local_index 0..3.
        for i in 0u32..4 {
            let handle = GeometryHandleId(4004 + i as u64);
            let attr = table
                .lookup(handle)
                .unwrap_or_else(|| panic!("end-cap vertex #{i} must have an entry"));
            assert_eq!(
                attr.role,
                Role::CapCornerVertex {
                    face: CapKind::Bottom
                },
                "end-cap vertex #{i} must be CapCornerVertex{{Bottom}}"
            );
            assert_eq!(
                attr.local_index, i,
                "end-cap vertex #{i} local_index must equal its per-cap position {i}"
            );
            assert_eq!(attr.feature_id, feature_id);
            assert!(attr.user_label.is_none());
            assert!(attr.mod_history.is_empty());
        }
    }

    #[test]
    fn populate_revolve_attributes_emits_cap_corner_vertex_for_start_and_end() {
        let mut table = TopologyAttributeTable::default();
        let feature_id = FeatureId::new("Revolve#realization[0]");

        let result_faces: Vec<GeometryHandleId> =
            (0..7).map(|i| GeometryHandleId(5000 + i)).collect();
        let result_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(5100)];
        let profile_faces: Vec<GeometryHandleId> = vec![GeometryHandleId(5200)];
        let profile_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(5201)];

        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        };

        let result_vertices: Vec<GeometryHandleId> =
            (0..8).map(|i| GeometryHandleId(5300 + i)).collect();
        let start_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![0, 1, 2, 3]];
        let end_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![4, 5, 6, 7]];

        populate_revolve_attributes(
            &mut table,
            &feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            &history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        )
        .expect("well-formed revolve history + vertex lists should succeed");

        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(5300 + i as u64))
                .unwrap_or_else(|| panic!("start-cap vertex #{i} must have an entry"));
            assert_eq!(
                attr.role,
                Role::CapCornerVertex {
                    face: CapKind::Start
                }
            );
            assert_eq!(attr.local_index, i);
        }
        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(5304 + i as u64))
                .unwrap_or_else(|| panic!("end-cap vertex #{i} must have an entry"));
            assert_eq!(attr.role, Role::CapCornerVertex { face: CapKind::End });
            assert_eq!(attr.local_index, i);
        }
    }

    #[test]
    fn populate_sweep_attributes_emits_cap_corner_vertex_for_start_and_end() {
        let mut table = TopologyAttributeTable::default();
        let feature_id = FeatureId::new("Sweep#realization[0]");

        let result_faces: Vec<GeometryHandleId> =
            (0..7).map(|i| GeometryHandleId(6000 + i)).collect();
        let result_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(6100)];
        let profile_faces: Vec<GeometryHandleId> = vec![GeometryHandleId(6200)];
        let profile_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(6201)];

        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        };

        let result_vertices: Vec<GeometryHandleId> =
            (0..8).map(|i| GeometryHandleId(6300 + i)).collect();
        let start_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![0, 1, 2, 3]];
        let end_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![4, 5, 6, 7]];

        populate_sweep_attributes(
            &mut table,
            &feature_id,
            &profile_faces,
            &profile_edges,
            &result_faces,
            &result_edges,
            &history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        )
        .expect("well-formed sweep history + vertex lists should succeed");

        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(6300 + i as u64))
                .unwrap_or_else(|| panic!("start-cap vertex #{i} must have an entry"));
            assert_eq!(
                attr.role,
                Role::CapCornerVertex {
                    face: CapKind::Start
                }
            );
            assert_eq!(attr.local_index, i);
        }
        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(6304 + i as u64))
                .unwrap_or_else(|| panic!("end-cap vertex #{i} must have an entry"));
            assert_eq!(attr.role, Role::CapCornerVertex { face: CapKind::End });
            assert_eq!(attr.local_index, i);
        }
    }

    #[test]
    fn populate_loft_attributes_emits_cap_corner_vertex_for_start_and_end() {
        let mut table = TopologyAttributeTable::default();
        let feature_id = FeatureId::new("Loft#realization[0]");

        // Two sections; 7 result faces: 5 = start cap, 6 = end cap.
        let section_faces: Vec<Vec<GeometryHandleId>> =
            vec![vec![GeometryHandleId(7100)], vec![GeometryHandleId(7101)]];
        let section_edges: Vec<Vec<GeometryHandleId>> = vec![
            vec![GeometryHandleId(7200), GeometryHandleId(7201)],
            vec![GeometryHandleId(7202), GeometryHandleId(7203)],
        ];
        let result_faces: Vec<GeometryHandleId> =
            (0..7).map(|i| GeometryHandleId(7300 + i)).collect();
        let result_edges: Vec<GeometryHandleId> = vec![GeometryHandleId(7400)];

        let history = LoftOpHistoryRecords {
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        };

        let result_vertices: Vec<GeometryHandleId> =
            (0..8).map(|i| GeometryHandleId(7500 + i)).collect();
        let start_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![0, 1, 2, 3]];
        let end_cap_vertex_index_lists: Vec<Vec<u32>> = vec![vec![4, 5, 6, 7]];

        populate_loft_attributes(
            &mut table,
            &feature_id,
            &section_faces,
            &section_edges,
            &result_faces,
            &result_edges,
            &history,
            &result_vertices,
            &start_cap_vertex_index_lists,
            &end_cap_vertex_index_lists,
        )
        .expect("well-formed loft history + vertex lists should succeed");

        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(7500 + i as u64))
                .unwrap_or_else(|| panic!("start-cap vertex #{i} must have an entry"));
            assert_eq!(
                attr.role,
                Role::CapCornerVertex {
                    face: CapKind::Start
                }
            );
            assert_eq!(attr.local_index, i);
        }
        for i in 0u32..4 {
            let attr = table
                .lookup(GeometryHandleId(7504 + i as u64))
                .unwrap_or_else(|| panic!("end-cap vertex #{i} must have an entry"));
            assert_eq!(attr.role, Role::CapCornerVertex { face: CapKind::End });
            assert_eq!(attr.local_index, i);
        }
    }

    // -----------------------------------------------------------------------
    // propagate_attributes_via_local_feature_history unit tests (step-1, RED)
    //
    // Exercises the four per-stream cross-kind propagation paths that fillet
    // and chamfer history require:
    //   face_modified  <- parent FACE
    //   face_generated <- parent EDGE  (cross-kind)
    //   edge_modified  <- parent EDGE
    //   edge_generated <- parent VERTEX (cross-kind)
    //
    // All assertions are pure data-structure logic — no kernel required.
    // The function and its type argument do not yet exist; this is the RED state.
    // -----------------------------------------------------------------------

    mod local_feature_propagation {
        use reify_ir::{
            AxisSign, FeatureId, GeometryHandleId, HistoryRecord, LocalFeatureOpHistoryRecords,
            ModEntry, QueryError, Role, TopologyAttribute, TopologyAttributeTable,
        };

        use super::super::propagate_attributes_via_local_feature_history;

        /// Canonical fillet FeatureId reused by every test as splitting_feature_id.
        fn fillet_feature_id() -> FeatureId {
            FeatureId::new("Fillet#realization[0]")
        }

        /// Build a `TopologyAttribute` with the given role/feature_id and empty
        /// mod_history.
        fn make_attr(fid: &FeatureId, role: Role, local_index: u32) -> TopologyAttribute {
            TopologyAttribute {
                feature_id: fid.clone(),
                role,
                local_index,
                user_label: None,
                mod_history: vec![],
            }
        }

        /// Build a `HistoryRecord` with parent_index=0 (always 0 for local features).
        fn rec(parent_subshape_index: u32, result_subshape_index: u32) -> HistoryRecord {
            HistoryRecord {
                parent_index: 0,
                parent_subshape_index,
                result_subshape_index,
            }
        }

        // ------------------------------------------------------------------ (a)
        // face_modified 1→1: copies parent FACE attr verbatim; no ModEntry added.
        // ------------------------------------------------------------------ (a)
        #[test]
        fn face_modified_one_to_one_copies_parent_face_attr_with_empty_mod_history() {
            let fid = FeatureId::new("Box#realization[0]");
            let parent_face = GeometryHandleId(1);
            let result_face = GeometryHandleId(11);

            let mut table = TopologyAttributeTable::default();
            table.record(parent_face, make_attr(&fid, Role::Side, 0));

            let history = LocalFeatureOpHistoryRecords {
                face_modified: vec![rec(0, 0)],
                ..Default::default()
            };

            propagate_attributes_via_local_feature_history(
                &mut table,
                &[parent_face],       // parent_face_handles
                &[],                  // parent_edge_handles
                &[],                  // parent_vertex_handles
                &[result_face],       // result_face_handles
                &[],                  // result_edge_handles
                &history,
                &fillet_feature_id(),
            )
            .expect("well-formed 1→1 face_modified should succeed");

            let attr = table
                .lookup(result_face)
                .expect("result face must have an attribute");
            assert_eq!(attr.feature_id, fid, "feature_id must be inherited from parent");
            assert_eq!(attr.role, Role::Side, "role must be inherited from parent");
            assert_eq!(attr.local_index, 0, "local_index must be inherited from parent");
            assert!(
                attr.mod_history.is_empty(),
                "single-result pass-through must not add a ModEntry; got {:?}",
                attr.mod_history
            );
        }

        // ------------------------------------------------------------------ (b)
        // face_generated: one parent EDGE maps to 2 result faces (cross-kind split).
        // Both result faces inherit the EDGE attr; each gets a ModEntry with
        // split_index 0 then 1.
        // ------------------------------------------------------------------ (b)
        #[test]
        fn face_generated_cross_kind_edge_to_two_faces_appends_split_mod_entries() {
            let fid = FeatureId::new("Box#realization[0]");
            let parent_edge = GeometryHandleId(3);
            let result_face_a = GeometryHandleId(11);
            let result_face_b = GeometryHandleId(12);
            let splitting_fid = fillet_feature_id();

            let mut table = TopologyAttributeTable::default();
            table.record(parent_edge, make_attr(&fid, Role::NewEdge, 5));

            // One parent edge → two result faces = a split.
            let history = LocalFeatureOpHistoryRecords {
                face_generated: vec![rec(0, 0), rec(0, 1)],
                ..Default::default()
            };

            propagate_attributes_via_local_feature_history(
                &mut table,
                &[],                              // parent_face_handles (unused here)
                &[parent_edge],                   // parent_edge_handles
                &[],                              // parent_vertex_handles
                &[result_face_a, result_face_b],  // result_face_handles
                &[],                              // result_edge_handles
                &history,
                &splitting_fid,
            )
            .expect("well-formed face_generated cross-kind split should succeed");

            // Both result faces inherit the parent EDGE's attribute.
            for (handle, expected_split_index) in [(result_face_a, 0u32), (result_face_b, 1u32)] {
                let attr = table
                    .lookup(handle)
                    .unwrap_or_else(|| panic!("result face {:?} must have an attribute", handle));
                assert_eq!(attr.feature_id, fid, "feature_id inherited from parent edge");
                assert_eq!(attr.role, Role::NewEdge, "role inherited from parent edge");
                assert_eq!(attr.local_index, 5, "local_index inherited from parent edge");
                assert_eq!(
                    attr.mod_history.len(),
                    1,
                    "split must add exactly one ModEntry; got {:?}",
                    attr.mod_history
                );
                assert_eq!(
                    attr.mod_history[0],
                    ModEntry {
                        splitting_feature_id: splitting_fid.clone(),
                        split_index: expected_split_index,
                    },
                    "ModEntry must carry the fillet feature_id and split_index {}",
                    expected_split_index
                );
            }
        }

        // ------------------------------------------------------------------ (c)
        // edge_modified 1→1: copies parent EDGE attr verbatim; no ModEntry.
        // ------------------------------------------------------------------ (c)
        #[test]
        fn edge_modified_one_to_one_copies_parent_edge_attr_with_empty_mod_history() {
            let fid = FeatureId::new("Box#realization[0]");
            let parent_edge = GeometryHandleId(3);
            let result_edge = GeometryHandleId(21);

            let mut table = TopologyAttributeTable::default();
            table.record(parent_edge, make_attr(&fid, Role::NewEdge, 2));

            let history = LocalFeatureOpHistoryRecords {
                edge_modified: vec![rec(0, 0)],
                ..Default::default()
            };

            propagate_attributes_via_local_feature_history(
                &mut table,
                &[],              // parent_face_handles
                &[parent_edge],   // parent_edge_handles
                &[],              // parent_vertex_handles
                &[],              // result_face_handles
                &[result_edge],   // result_edge_handles
                &history,
                &fillet_feature_id(),
            )
            .expect("well-formed 1→1 edge_modified should succeed");

            let attr = table
                .lookup(result_edge)
                .expect("result edge must have an attribute");
            assert_eq!(attr.feature_id, fid);
            assert_eq!(attr.role, Role::NewEdge);
            assert_eq!(attr.local_index, 2);
            assert!(
                attr.mod_history.is_empty(),
                "single-result pass-through must not add a ModEntry; got {:?}",
                attr.mod_history
            );
        }

        // ------------------------------------------------------------------ (d)
        // edge_generated: one parent VERTEX maps to 2 result edges (cross-kind split).
        // Both result edges inherit the VERTEX attr; each gets a ModEntry with
        // split_index 0 then 1.
        // ------------------------------------------------------------------ (d)
        #[test]
        fn edge_generated_cross_kind_vertex_to_two_edges_appends_split_mod_entries() {
            let fid = FeatureId::new("Box#realization[0]");
            let parent_vertex = GeometryHandleId(5);
            let result_edge_a = GeometryHandleId(21);
            let result_edge_b = GeometryHandleId(22);
            let splitting_fid = fillet_feature_id();

            let mut table = TopologyAttributeTable::default();
            let corner_role = Role::CornerVertex { x: AxisSign::Pos, y: AxisSign::Pos, z: AxisSign::Pos };
            table.record(parent_vertex, make_attr(&fid, corner_role.clone(), 3));

            let history = LocalFeatureOpHistoryRecords {
                edge_generated: vec![rec(0, 0), rec(0, 1)],
                ..Default::default()
            };

            propagate_attributes_via_local_feature_history(
                &mut table,
                &[],                                // parent_face_handles
                &[],                                // parent_edge_handles
                &[parent_vertex],                   // parent_vertex_handles
                &[],                                // result_face_handles
                &[result_edge_a, result_edge_b],    // result_edge_handles
                &history,
                &splitting_fid,
            )
            .expect("well-formed edge_generated cross-kind split should succeed");

            for (handle, expected_split_index) in [(result_edge_a, 0u32), (result_edge_b, 1u32)] {
                let attr = table
                    .lookup(handle)
                    .unwrap_or_else(|| panic!("result edge {:?} must have an attribute", handle));
                assert_eq!(attr.feature_id, fid, "feature_id inherited from parent vertex");
                assert_eq!(attr.role, corner_role, "role inherited from parent vertex");
                assert_eq!(attr.local_index, 3, "local_index inherited from parent vertex");
                assert_eq!(
                    attr.mod_history.len(),
                    1,
                    "split must add exactly one ModEntry; got {:?}",
                    attr.mod_history
                );
                assert_eq!(
                    attr.mod_history[0],
                    ModEntry {
                        splitting_feature_id: splitting_fid.clone(),
                        split_index: expected_split_index,
                    }
                );
            }
        }

        // ------------------------------------------------------------------ (e)
        // A parent that already carries a non-empty mod_history is preserved on
        // single-child pass-through and only appended-to on a split.
        // ------------------------------------------------------------------ (e)
        #[test]
        fn prior_mod_history_preserved_on_passthrough_and_appended_on_split() {
            let fid = FeatureId::new("Box#realization[0]");
            let prior_fid = FeatureId::new("PriorOp#realization[0]");
            let prior_entry = ModEntry {
                splitting_feature_id: prior_fid.clone(),
                split_index: 7,
            };

            // Case 1: single-child pass-through — prior mod_history must survive unchanged.
            {
                let parent_face = GeometryHandleId(1);
                let result_face = GeometryHandleId(11);

                let mut table = TopologyAttributeTable::default();
                let mut attr = make_attr(&fid, Role::Side, 0);
                attr.mod_history.push(prior_entry.clone());
                table.record(parent_face, attr);

                let history = LocalFeatureOpHistoryRecords {
                    face_modified: vec![rec(0, 0)],
                    ..Default::default()
                };

                propagate_attributes_via_local_feature_history(
                    &mut table,
                    &[parent_face],
                    &[],
                    &[],
                    &[result_face],
                    &[],
                    &history,
                    &fillet_feature_id(),
                )
                .expect("single-child pass-through should succeed");

                let result_attr = table.lookup(result_face).expect("result must have attr");
                assert_eq!(
                    result_attr.mod_history,
                    vec![prior_entry.clone()],
                    "prior mod_history must be preserved on 1→1 pass-through"
                );
            }

            // Case 2: split (1 parent edge → 2 result faces) — prior mod_history
            // must be preserved on both children, with the new ModEntry appended.
            {
                let parent_edge = GeometryHandleId(3);
                let result_face_a = GeometryHandleId(11);
                let result_face_b = GeometryHandleId(12);
                let splitting_fid = fillet_feature_id();

                let mut table = TopologyAttributeTable::default();
                let mut attr = make_attr(&fid, Role::NewEdge, 0);
                attr.mod_history.push(prior_entry.clone());
                table.record(parent_edge, attr);

                let history = LocalFeatureOpHistoryRecords {
                    face_generated: vec![rec(0, 0), rec(0, 1)],
                    ..Default::default()
                };

                propagate_attributes_via_local_feature_history(
                    &mut table,
                    &[],
                    &[parent_edge],
                    &[],
                    &[result_face_a, result_face_b],
                    &[],
                    &history,
                    &splitting_fid,
                )
                .expect("split should succeed");

                for (handle, expected_split_index) in
                    [(result_face_a, 0u32), (result_face_b, 1u32)]
                {
                    let result_attr = table
                        .lookup(handle)
                        .unwrap_or_else(|| panic!("{:?} must have attr", handle));
                    assert_eq!(
                        result_attr.mod_history.len(),
                        2,
                        "prior entry + new ModEntry = 2 entries; got {:?}",
                        result_attr.mod_history
                    );
                    assert_eq!(
                        result_attr.mod_history[0],
                        prior_entry,
                        "prior entry must be at index 0"
                    );
                    assert_eq!(
                        result_attr.mod_history[1],
                        ModEntry {
                            splitting_feature_id: splitting_fid.clone(),
                            split_index: expected_split_index,
                        },
                        "new ModEntry must be at index 1 with split_index {}",
                        expected_split_index
                    );
                }
            }
        }

        // ------------------------------------------------------------------ (f)
        // An out-of-range parent subshape index returns Err(QueryError::QueryFailed).
        // ------------------------------------------------------------------ (f)
        #[test]
        fn out_of_range_face_modified_parent_subshape_index_returns_error() {
            let mut table = TopologyAttributeTable::default();

            let history = LocalFeatureOpHistoryRecords {
                // parent_subshape_index=99 but parent_face_handles has only 1 entry.
                face_modified: vec![HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 99,
                    result_subshape_index: 0,
                }],
                ..Default::default()
            };

            let err = propagate_attributes_via_local_feature_history(
                &mut table,
                &[GeometryHandleId(1)],  // only 1 parent face (index 0 valid, 99 is OOB)
                &[],
                &[],
                &[GeometryHandleId(11)],
                &[],
                &history,
                &fillet_feature_id(),
            )
            .expect_err("out-of-range parent_subshape_index should return QueryFailed");

            match err {
                QueryError::QueryFailed(msg) => {
                    assert!(
                        msg.contains("parent_subshape_index 99") || msg.contains("99"),
                        "error message should mention the offending index, got {msg:?}"
                    );
                }
                other => panic!("expected QueryError::QueryFailed, got {other:?}"),
            }
        }

        #[test]
        fn out_of_range_result_subshape_index_returns_error() {
            let mut table = TopologyAttributeTable::default();
            let parent_edge = GeometryHandleId(3);

            let history = LocalFeatureOpHistoryRecords {
                // result_subshape_index=7 but result_face_handles has only 1 entry.
                face_generated: vec![HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 7,
                }],
                ..Default::default()
            };

            let err = propagate_attributes_via_local_feature_history(
                &mut table,
                &[],
                &[parent_edge],
                &[],
                &[GeometryHandleId(11)],  // only 1 result face (index 0 valid, 7 is OOB)
                &[],
                &history,
                &fillet_feature_id(),
            )
            .expect_err("out-of-range result_subshape_index should return QueryFailed");

            match err {
                QueryError::QueryFailed(_) => {}
                other => panic!("expected QueryError::QueryFailed, got {other:?}"),
            }
        }
    }
}
