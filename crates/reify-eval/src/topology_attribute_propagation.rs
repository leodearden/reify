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

use std::collections::HashMap;

use reify_types::{
    BooleanOpHistoryRecords, BooleanOpParents, CapKind, FeatureId, GeometryHandleId, HistoryRecord,
    ModEntry, QueryError, Role, SweepOpHistoryRecords, TopologyAttribute, TopologyAttributeTable,
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
            splitting_feature_id,
            &face_child_counts,
            &mut face_split_counters,
        )?;
    }

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
            splitting_feature_id,
            &edge_child_counts,
            &mut edge_split_counters,
        )?;
    }

    // Deleted records are intentionally skipped: no result sub-shape
    // exists to receive the attribute, and parents' existing table
    // entries remain valid (task 4 will add diagnostics).
    Ok(())
}

/// Build a `(parent_index, parent_subshape_index) → count` map across the
/// concatenation of `records_modified` and `records_generated`.
///
/// Records are visited in `Modified` then `Generated` order within each
/// kind; this ordering matches the propagation walk, so counts here pin
/// the same per-parent record stream that determines `split_index`
/// assignment in [`propagate_one`].
///
/// A parent appearing in this map with `count > 1` is a split — each of
/// its children is given a fresh `ModEntry { splitting_feature_id,
/// split_index }` appended to `mod_history`. `count == 1` means a pure
/// pass-through (mod_history unchanged).
fn count_children_per_parent(
    records_modified: &[HistoryRecord],
    records_generated: &[HistoryRecord],
) -> HashMap<(u8, u32), usize> {
    let mut counts: HashMap<(u8, u32), usize> = HashMap::new();
    for rec in records_modified.iter().chain(records_generated.iter()) {
        let key = (rec.parent_index, rec.parent_subshape_index);
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// If the parent at `(parent_index, parent_subshape_index)` has more than
/// one same-kind result child (count > 1), append a fresh
/// `ModEntry { splitting_feature_id, split_index }` to `attr` and bump
/// the per-parent `split_index` counter for the next sibling. For
/// single-result parents (count == 1) this is a no-op — the v0.2 invariant
/// is that `mod_history` is only augmented on actual splits, preserving
/// prior history exactly for pure pass-through propagation.
fn maybe_append_split_entry(
    attr: &mut TopologyAttribute,
    parent_key: (u8, u32),
    child_counts: &HashMap<(u8, u32), usize>,
    split_counters: &mut HashMap<(u8, u32), u32>,
    splitting_feature_id: &FeatureId,
) {
    let count = child_counts.get(&parent_key).copied().unwrap_or(0);
    if count > 1 {
        let split_index = split_counters.entry(parent_key).or_insert(0);
        attr.mod_history.push(ModEntry {
            splitting_feature_id: splitting_feature_id.clone(),
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
#[allow(clippy::too_many_arguments)] // each arg threads a needed input; alternatives bundle them into a context struct only the caller would build
fn propagate_one(
    table: &mut TopologyAttributeTable,
    parent_handles: &[&[GeometryHandleId]],
    result_handles: &[GeometryHandleId],
    record: &HistoryRecord,
    kind: &str,
    splitting_feature_id: &FeatureId,
    child_counts: &HashMap<(u8, u32), usize>,
    split_counters: &mut HashMap<(u8, u32), u32>,
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
        maybe_append_split_entry(
            &mut attr_clone,
            parent_key,
            child_counts,
            split_counters,
            splitting_feature_id,
        );
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
pub fn populate_extrude_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &SweepOpHistoryRecords,
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
pub fn populate_revolve_attributes(
    table: &mut TopologyAttributeTable,
    feature_id: &FeatureId,
    profile_face_handles: &[GeometryHandleId],
    profile_edge_handles: &[GeometryHandleId],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &SweepOpHistoryRecords,
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
    use reify_types::{
        BooleanOpHistoryRecords, BooleanOpParents, CapKind, FeatureId, GeometryHandleId,
        HistoryRecord, ModEntry, QueryError, Role, SweepOpHistoryRecords, TopologyAttribute,
        TopologyAttributeTable,
    };

    use super::{
        populate_extrude_attributes, populate_revolve_attributes,
        propagate_attributes_via_brepalgoapi_history,
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
                    msg.contains("face"),
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
                    msg.contains("face"),
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
                    msg.contains("face"),
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
                    msg.contains("edge"),
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
                    msg.contains("edge"),
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
                    msg.contains("edge"),
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
}
