//! Per-op auto-population of `TopologyAttribute` records for primitive
//! constructors (v0.2 persistent-naming-v2, decomposition-plan task 6).
//!
//! Cross-references:
//! - PRD docs/prds/v0_2/persistent-naming-v2.md "Decomposition plan" task 6.
//! - Sibling module [`crate::topology_attribute_propagation`], which carries
//!   the *propagation* phase: copying parent attributes onto result handles
//!   AFTER a constructive op (boolean fuse / cut / common, sweep, fillet).
//!   This module covers the *seeding* phase — originating attributes for the
//!   leaves of the feature tree (primitives), which have no parent.
//!
//! Scope of this task (#2574), extended by task #4156 (Cone):
//! - `GeometryOp::Box` / `GeometryOp::Sphere` — every face seeded
//!   `Role::Side`; every edge seeded `Role::NewEdge`. `local_index` is
//!   the construction-order (TopExp) position within `(feature_id, role)`.
//! - `GeometryOp::Cylinder` / `GeometryOp::Cone` — faces classified into
//!   `Cap(Top)`, `Cap(Bottom)`, or `Side` via `GeometryQuery::FaceNormal`'s
//!   z-component; every edge seeded `Role::NewEdge`. A pointed cone
//!   (top_radius == 0) emits only 2 faces (no top cap), so `Cap(Top)` count
//!   is 0 in that case.
//! - All other variants are intentional no-ops; the dispatch is widened in
//!   subsequent tasks.
//!
//! ## Edge-role convention (PRD line 66, construction-order tiebreak)
//!
//! All edges of a primitive constructor receive `Role::NewEdge` with
//! `local_index` equal to their position in `kernel.extract_edges(handle)`'s
//! TopExp-ordered output. Per PRD line 66 the local-index ordering should be
//! "deterministic geometric ordering with construction-order tiebreak only
//! for genuine geometric ties"; primitives have no per-edge geometric
//! distinguisher (a box's 12 edges are all axis-aligned, a cylinder's
//! cap circles are isomorphic to each other, etc.), so construction-order
//! is the canonical ordering. Cap-vs-side edge classification (e.g. for
//! cylinders the cap-circle edges are different from the seam) is left to
//! later refinement tasks if/when selector vocabulary requires it.
//!
//! Variants intentionally deferred:
//! - `GeometryOp::Tube` — composed via `boolean_cut` at the kernel layer; its
//!   per-result attribute attachment lands with task 8 (booleans) or a Tube-
//!   specific follow-up.
//! - `GeometryOp::Torus` — not yet present in `GeometryOp` (no FFI, no
//!   compiler `PrimitiveKind`); will be added end-to-end as a separate task.
//! - Sweep / local-feature / boolean variants — tasks 5, 7, 8.
//!
//! ## Why pre-extracted face/edge handle slices?
//!
//! `kernel.extract_faces(handle)` / `extract_edges(handle)` allocate fresh
//! `GeometryHandleId`s on each call (the kernel does not dedupe by face-
//! equality). To make `table.lookup(face)` work for the same handle vector
//! the caller observes, the caller must extract once and reuse those vectors
//! both for seeding (here) and for any later lookup (tests, downstream
//! propagation). This mirrors
//! [`crate::topology_attribute_propagation::propagate_attributes_via_brepalgoapi_history`]'s
//! pre-extracted-vectors discipline — see that module's doc-comment for
//! the original rationale.
//!
//! ## Per-attribute invariants (task 1, PRD lines 52-66)
//! - `feature_id` is the seeded primitive's `FeatureId` (derived from its
//!   `RealizationNodeId` via `FeatureId::from(&realization_id)`).
//! - `user_label` is always `None` here. The user-facing `name = "..."`
//!   syntax (PRD line 50) is absorbed by later authoring layers.
//! - `mod_history` is always empty. Splits/labels arrive in tasks 3-4.
//!
//! ## Shared-FeatureId limitation across primitives in one realization
//!
//! Every primitive op inside a single realization currently shares one
//! `FeatureId` (derived once from the realization's `RealizationNodeId`
//! by `Engine::execute_realization_ops`). When a realization contains
//! multiple primitives — e.g.
//! `let body = box(...); let lid = cylinder(...); union(body, lid)` —
//! both primitives' seeded entries carry the same `feature_id`, so any
//! reverse lookup keyed on `(feature_id, role, local_index)` will be
//! ambiguous between them (lookup-by-`GeometryHandleId` is unaffected
//! and remains the contract this seeder upholds).
//!
//! This is intentional for task 6 (#2574): the `RealizationNodeId` is
//! the only feature-identity granularity that has landed so far, and
//! widening it to a per-let-binding `FeatureId` (e.g.
//! `Body#realization[0]/let[body]`) requires the AST → IR threading
//! that PRD §6.5 sketches but tasks 9-10 (selector vocabulary +
//! `feature_tag_table` retirement) are intended to deliver. The
//! primitive seeder will follow that thread once it lands; until then,
//! the shared-FeatureId contract is the documented status quo.

use std::collections::HashMap;

use reify_ir::{
    AxisSign, CapKind, FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery,
    QueryError, Role, TopologyAttribute, TopologyAttributeTable, Value,
};

/// Tolerance for cylinder cap-vs-side classification by face-normal
/// z-component. A face whose normal satisfies `nz > 1.0 - eps` is the
/// top cap; `nz < -1.0 + eps` is the bottom cap; otherwise the face
/// is a side. Picked to match the project's general geometric-tolerance
/// convention (1e-6) used in `OcctKernel`'s assertions and in the
/// `topology_selectors` filters.
const NORMAL_Z_EPSILON: f64 = 1.0e-6;

/// Extract the primitive's faces/edges/vertices from `kernel` and seed
/// `TopologyAttribute` records for each, in one call.
///
/// Convenience wrapper for callers (e.g. `Engine::execute_realization_ops`)
/// that don't already have pre-extracted face/edge/vertex handle vectors and
/// don't need to reuse them downstream. For seedable primitive variants
/// (`Box`, `Cylinder`, `Sphere`), this calls `kernel.extract_faces` /
/// `kernel.extract_edges` and delegates to [`seed_primitive_attributes`].
/// For `GeometryOp::Box` specifically, it also calls
/// `kernel.extract_vertices` and passes the resulting handles through so
/// that `record_box_corner_vertices` populates the 8 `CornerVertex` entries.
/// For `Cylinder` / `Sphere`, vertex extraction is skipped at zero cost
/// (no analytic vertices per PRD §2 Q-MM2-1).
/// For non-seedable variants, returns `Ok(())` without calling into the
/// kernel — the dispatch by op kind happens here so non-primitive ops
/// (Translate / boolean / sweep / …) pay zero kernel overhead per op.
///
/// `Engine::execute_realization_ops` cannot use the underlying
/// [`seed_primitive_attributes`] directly because the kernel is not
/// available to the engine as a `&mut dyn GeometryKernel` at the same
/// time as borrows on `step_handles` / `feature_tag_table`; this wrapper
/// brackets all kernel borrows in one synchronous call.
///
/// Errors: same contract as [`seed_primitive_attributes`] — callers
/// should treat any `Err(QueryError)` as auxiliary-metadata failure
/// (warn and continue) rather than primary geometry failure.
///
/// Visibility: `pub` — widened from `pub(crate)` in task 3633 (step-4)
/// so that the integration test
/// `topology_attribute_primitives_direct::seed_primitive_attributes_for_handle_box_extracts_and_seeds_vertices_too`
/// (which lives in `tests/` and links against the crate as an external
/// consumer) can call it directly. The function is a thin wrapper whose
/// contract is already documented; there is no meaningful privacy boundary
/// worth preserving here.
pub fn seed_primitive_attributes_for_handle(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    result_handle: GeometryHandleId,
    feature_id: &FeatureId,
    op: &GeometryOp,
) -> Result<(), QueryError> {
    if !is_seedable_primitive(op) {
        // Non-seedable variants — skip the extract_* calls entirely so the
        // engine pays zero kernel overhead per non-primitive op. The
        // closed-extension contract pins in `tests` (steps 9-10) verify
        // every non-primitive variant returns Ok(()) without a kernel
        // touch when invoked through `seed_primitive_attributes`; this
        // wrapper enforces the same invariant for the engine path.
        return Ok(());
    }
    let face_handles = kernel.extract_faces(result_handle)?;
    let edge_handles = kernel.extract_edges(result_handle)?;
    // Extract vertices only for Box ops — vertex enumeration includes shape
    // exploration and is more expensive than face/edge extraction.
    // Cylinder/Sphere have no analytic vertices (PRD §2 Q-MM2-1), so their
    // vertex slices stay empty at zero kernel cost.
    let vertex_handles: Vec<GeometryHandleId> = if matches!(op, GeometryOp::Box { .. }) {
        kernel.extract_vertices(result_handle)?
    } else {
        Vec::new()
    };
    seed_primitive_attributes(
        table,
        kernel,
        &face_handles,
        &edge_handles,
        &vertex_handles,
        feature_id,
        op,
    )
}

/// Returns `true` for `GeometryOp` variants that this seeder originates
/// attribute records for. Non-seedable variants are intentional no-ops —
/// see the module docstring for the deferred-task accounting.
fn is_seedable_primitive(op: &GeometryOp) -> bool {
    matches!(
        op,
        GeometryOp::Box { .. }
            | GeometryOp::Cylinder { .. }
            | GeometryOp::Sphere { .. }
            | GeometryOp::Cone { .. }
    )
}

/// Seed per-face/per-edge/per-vertex `TopologyAttribute` records for a
/// primitive constructor's result handle.
///
/// Inputs:
/// - `table`: the table to write entries into.
/// - `kernel`: kept on the signature for arms that need geometric queries
///   (Cylinder uses `GeometryQuery::FaceNormal` to classify caps; Box uses
///   `GeometryQuery::BoundingBox` on each vertex to classify corners).
///   Sphere arms touch only `face_handles` / `edge_handles` and never call
///   into the kernel.
/// - `face_handles`, `edge_handles`, `vertex_handles`: TopExp-ordered handle
///   vectors the caller has pre-extracted (typically via
///   `kernel.extract_faces(...)` / `extract_edges(...)` /
///   `extract_vertices(...)` on the primitive's result handle, immediately
///   after `kernel.execute(&op)` returns `Ok(handle)`). Each call to
///   `extract_*` allocates fresh handle ids, so callers must reuse the
///   same vectors for downstream lookups (tests, propagation).
/// - `feature_id`: the originating feature for every entry written here.
/// - `op`: matched on by variant. Non-primitive variants are silent
///   no-ops, leaving `table` unchanged. This lets
///   `Engine::execute_realization_ops` invoke the seeder unconditionally
///   on every kernel-success path without per-op gating.
///
/// Vertex seeding:
/// - `GeometryOp::Box`: each vertex in `vertex_handles` is classified by
///   the BoundingBox sign of its `(xmin, ymin, zmin)` coordinates (Pos iff
///   coord >= 0.0). The three-axis sign triple maps to `Role::CornerVertex
///   { x, y, z }` with `local_index = pack_sign_bits(x, y, z)` (0..7,
///   deterministic across builds — same Role payload → same local_index).
/// - `GeometryOp::Cylinder` / `GeometryOp::Sphere`: `vertex_handles` is
///   ignored (no analytic vertices per PRD §2 Q-MM2-1).
///
/// Returns `Err(QueryError)` only when a primitive arm needs a kernel
/// query (Cylinder's `FaceNormal`, or Box's vertex `BoundingBox`) and the
/// kernel reports an error.
/// Callers should treat this as auxiliary-metadata failure (warn and
/// continue) rather than a primary geometry failure.
pub fn seed_primitive_attributes(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    face_handles: &[GeometryHandleId],
    edge_handles: &[GeometryHandleId],
    vertex_handles: &[GeometryHandleId],
    feature_id: &FeatureId,
    op: &GeometryOp,
) -> Result<(), QueryError> {
    match op {
        GeometryOp::Box { .. } => {
            record_all_faces_as_side(table, face_handles, feature_id);
            record_all_edges_as_new_edge(table, edge_handles, feature_id);
            record_box_corner_vertices(table, kernel, vertex_handles, feature_id)?;
            Ok(())
        }
        GeometryOp::Sphere { .. } => {
            // Sphere has byte-identical face-seeding semantics to Box — every
            // face gets Role::Side with construction-order local_index — but
            // no analytic vertices per PRD §2 Q-MM2-1, so vertex_handles
            // is intentionally ignored.
            record_all_faces_as_side(table, face_handles, feature_id);
            record_all_edges_as_new_edge(table, edge_handles, feature_id);
            Ok(())
        }
        GeometryOp::Cylinder { .. } | GeometryOp::Cone { .. } => {
            // A cylinder emits 3 faces in OCCT's TopExp order: side, top
            // cap, bottom cap (order varies by OCCT version). Classify
            // each via `GeometryQuery::FaceNormal`'s z-component.
            //
            // A cone (frustum) similarly emits 3 faces (slanted side +
            // top cap + bottom cap). A pointed cone (top_radius == 0)
            // emits 2 faces (slanted side + bottom cap, no top face).
            // The FaceNormal-z classification handles both transparently:
            // nz ≈ +1 → Cap(Top), nz ≈ -1 → Cap(Bottom), |nz| ≈ 0 → Side.
            //
            // Limitation (Cone only): this assumes the lateral face has a
            // near-horizontal normal (|nz| well below 1 - 1e-6).  A very
            // steep frustum — large radius delta over a small height — drives
            // the slanted-face normal toward the z-axis, risking
            // misclassification of the Side face as Cap(Top/Bottom).
            // Steep cones are uncommon in practice and out of scope for this
            // task; a future improvement should classify by planar-vs-conical
            // surface type (BRep surface type query) rather than an absolute
            // nz threshold to handle the degenerate regime correctly.
            //
            // For the canonical 3-face case each role appears exactly
            // once and `local_index` is 0 for every entry. Per-role
            // counters preserve that invariant while remaining safe
            // against degenerate kernel outputs (e.g. an unusual face
            // split or a future OCCT version that emits more than one
            // face with the same classification): each subsequent face
            // of the same role gets the next sequential `local_index`,
            // mirroring the construction-order discipline used for
            // Box/Sphere `Role::Side`. This guarantees the seeder never
            // writes two rows with identical `(feature_id, role,
            // local_index)`, keeping reverse lookups unambiguous.
            let mut role_counts: HashMap<Role, u32> = HashMap::new();
            for &face_id in face_handles.iter() {
                let normal_value = kernel.query(&GeometryQuery::FaceNormal(face_id))?;
                let nz = parse_normal_z(&normal_value)?;
                let role = classify_cylinder_face_role(nz);
                let local_index = {
                    let counter = role_counts.entry(role).or_insert(0);
                    let assigned = *counter;
                    *counter += 1;
                    assigned
                };
                table.record(
                    face_id,
                    TopologyAttribute {
                        feature_id: feature_id.clone(),
                        role,
                        local_index,
                        user_label: None,
                        mod_history: Vec::new(),
                    },
                );
            }
            record_all_edges_as_new_edge(table, edge_handles, feature_id);
            Ok(())
        }
        // All other variants are intentional no-ops. Per-op auto-population
        // for sweep / local-feature / boolean variants lands in PRD tasks 5,
        // 7, 8 respectively. The `_ => Ok(())` arm is the closed-extension
        // contract pin tested by the unit tests in step-9/10: the seeder
        // never calls into the kernel for non-primitive variants.
        _ => Ok(()),
    }
}

/// Record every supplied face handle as `Role::Side` with construction-
/// order `local_index` and the task-1 default metadata
/// (`user_label = None`, `mod_history = Vec::new()`).
///
/// Shared by the Box and Sphere arms — neither primitive has a meaningful
/// "Cap" classification (Box's 6 faces are all axis-aligned sides; Sphere
/// has no caps), so all faces are uniformly `Role::Side`. PRD line 66
/// permits construction-order tiebreak (TopExp order) for genuine
/// geometric ties.
fn record_all_faces_as_side(
    table: &mut TopologyAttributeTable,
    face_handles: &[GeometryHandleId],
    feature_id: &FeatureId,
) {
    for (idx, &face_id) in face_handles.iter().enumerate() {
        table.record(
            face_id,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::Side,
                local_index: idx as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
}

/// Record every supplied edge handle as `Role::NewEdge` with construction-
/// order `local_index` and the task-1 default metadata
/// (`user_label = None`, `mod_history = Vec::new()`).
///
/// Shared by the Box, Cylinder, and Sphere arms — all primitive constructors
/// classify their construction edges uniformly as `NewEdge`. PRD line 66
/// permits construction-order tiebreak (TopExp order) for genuine geometric
/// ties, which is the case here (a primitive's edges have no per-edge
/// geometric distinguisher in the current selector vocabulary).
fn record_all_edges_as_new_edge(
    table: &mut TopologyAttributeTable,
    edge_handles: &[GeometryHandleId],
    feature_id: &FeatureId,
) {
    for (idx, &edge_id) in edge_handles.iter().enumerate() {
        table.record(
            edge_id,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::NewEdge,
                local_index: idx as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
}

/// Classify a cylinder face by its normal's z-component.
///
/// `|nz - 1| < eps` → top cap. `|nz + 1| < eps` → bottom cap. Otherwise
/// the face is a side. The constant comes from `NORMAL_Z_EPSILON`.
fn classify_cylinder_face_role(nz: f64) -> Role {
    if nz > 1.0 - NORMAL_Z_EPSILON {
        Role::Cap(CapKind::Top)
    } else if nz < -1.0 + NORMAL_Z_EPSILON {
        Role::Cap(CapKind::Bottom)
    } else {
        Role::Side
    }
}

/// Extract the z-component of the JSON-encoded `{"x":..,"y":..,"z":..}`
/// payload that `GeometryQuery::FaceNormal` returns.
///
/// Mirrors the parsing convention used by `topology_selectors::parse_xyz_value`
/// — kept inline here to avoid widening that helper's visibility for a
/// single-component read.
fn parse_normal_z(value: &Value) -> Result<f64, QueryError> {
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "FaceNormal returned non-string value: {other:?}"
            )));
        }
    };
    // Strip outer braces and split on commas — the kernel emits a flat
    // `{"x":NUM,"y":NUM,"z":NUM}` shape with no nested objects.
    let inner = s
        .trim()
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| {
            QueryError::QueryFailed(format!("FaceNormal returned malformed JSON Point3: {s:?}"))
        })?;
    for part in inner.split(',') {
        let mut kv = part.splitn(2, ':');
        let key = kv
            .next()
            .ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "FaceNormal returned malformed JSON Point3 (missing key): {s:?}"
                ))
            })?
            .trim()
            .trim_matches('"');
        let val = kv
            .next()
            .ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "FaceNormal returned malformed JSON Point3 (missing value): {s:?}"
                ))
            })?
            .trim();
        if key == "z" {
            return val.parse::<f64>().map_err(|_| {
                QueryError::QueryFailed(format!(
                    "FaceNormal z-component is not a valid f64: {val:?} (full payload {s:?})"
                ))
            });
        }
    }
    Err(QueryError::QueryFailed(format!(
        "FaceNormal payload missing z-component: {s:?}"
    )))
}

/// Seed `Role::CornerVertex { x, y, z }` entries for each vertex of a
/// Box primitive.
///
/// Each vertex's bounding box `(xmin, ymin, zmin)` encodes its position
/// (for a vertex `xmin == xmax`, etc.). Origin-centered box convention:
/// `AxisSign::Pos` iff coord >= 0.0, `AxisSign::Neg` iff coord < 0.0.
/// `local_index` = `pack_sign_bits(x, y, z)` — 0..7, deterministic across
/// builds.
fn record_box_corner_vertices(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    vertex_handles: &[GeometryHandleId],
    feature_id: &FeatureId,
) -> Result<(), QueryError> {
    for &vertex_id in vertex_handles {
        let bbox_value = kernel.query(&GeometryQuery::BoundingBox(vertex_id))?;
        let (xmin, ymin, zmin) = parse_bbox_xyz_min(&bbox_value)?;
        let x = if xmin >= 0.0 {
            AxisSign::Pos
        } else {
            AxisSign::Neg
        };
        let y = if ymin >= 0.0 {
            AxisSign::Pos
        } else {
            AxisSign::Neg
        };
        let z = if zmin >= 0.0 {
            AxisSign::Pos
        } else {
            AxisSign::Neg
        };
        table.record(
            vertex_id,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::CornerVertex { x, y, z },
                local_index: pack_sign_bits(x, y, z),
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }
    Ok(())
}

/// Map a (x, y, z) AxisSign triple to a unique index in 0..7.
///
/// Formula: `(x_pos_bit << 2) | (y_pos_bit << 1) | z_pos_bit`
/// where Pos=1, Neg=0. This is deterministic: same Role payload →
/// same local_index across builds. X-major ordering matches the
/// conventional `(x,y,z)` reading order.
fn pack_sign_bits(x: AxisSign, y: AxisSign, z: AxisSign) -> u32 {
    let xb = u32::from(x == AxisSign::Pos);
    let yb = u32::from(y == AxisSign::Pos);
    let zb = u32::from(z == AxisSign::Pos);
    (xb << 2) | (yb << 1) | zb
}

/// Extract `(xmin, ymin, zmin)` from a `GeometryQuery::BoundingBox`
/// response payload.
///
/// The kernel emits a flat JSON string of the form:
/// `{"xmin":NUM,"ymin":NUM,"zmin":NUM,"xmax":NUM,"ymax":NUM,"zmax":NUM}`.
/// For a vertex, `xmin == xmax` (degenerate bbox), so reading the `*min`
/// triple is sufficient to recover the vertex position.
///
/// Mirrors the `parse_normal_z` pattern — strip braces, split on commas,
/// parse each `"key":NUM` pair. Same error-wording convention for diagnostic
/// consistency.
pub(crate) fn parse_bbox_xyz_min(value: &Value) -> Result<(f64, f64, f64), QueryError> {
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "BoundingBox returned non-string value: {other:?}"
            )));
        }
    };
    let inner = s
        .trim()
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| {
            QueryError::QueryFailed(format!("BoundingBox returned malformed JSON: {s:?}"))
        })?;
    let mut xmin: Option<f64> = None;
    let mut ymin: Option<f64> = None;
    let mut zmin: Option<f64> = None;
    for part in inner.split(',') {
        let mut kv = part.splitn(2, ':');
        let key = kv
            .next()
            .ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "BoundingBox returned malformed JSON (missing key): {s:?}"
                ))
            })?
            .trim()
            .trim_matches('"');
        let val = kv
            .next()
            .ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "BoundingBox returned malformed JSON (missing value): {s:?}"
                ))
            })?
            .trim();
        match key {
            "xmin" => {
                xmin = Some(val.parse::<f64>().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "BoundingBox xmin is not a valid f64: {val:?} (full payload {s:?})"
                    ))
                })?)
            }
            "ymin" => {
                ymin = Some(val.parse::<f64>().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "BoundingBox ymin is not a valid f64: {val:?} (full payload {s:?})"
                    ))
                })?)
            }
            "zmin" => {
                zmin = Some(val.parse::<f64>().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "BoundingBox zmin is not a valid f64: {val:?} (full payload {s:?})"
                    ))
                })?)
            }
            // xmax/ymax/zmax and any other keys tolerated silently
            _ => {}
        }
    }
    let xmin = xmin.ok_or_else(|| {
        QueryError::QueryFailed(format!("BoundingBox payload missing xmin: {s:?}"))
    })?;
    let ymin = ymin.ok_or_else(|| {
        QueryError::QueryFailed(format!("BoundingBox payload missing ymin: {s:?}"))
    })?;
    let zmin = zmin.ok_or_else(|| {
        QueryError::QueryFailed(format!("BoundingBox payload missing zmin: {s:?}"))
    })?;
    Ok((xmin, ymin, zmin))
}

#[cfg(test)]
mod tests {
    //! Pure no-OCCT unit tests for the seeder dispatch.
    //!
    //! These tests pin the closed-extension contract: every non-seedable
    //! `GeometryOp` variant must return `Ok(())` without ever calling into
    //! the kernel. They use a `MockKernel` that returns
    //! `Err(QueryError::QueryFailed("..."))` from every method — if the
    //! seeder's dispatch ever falls through into the kernel for one of
    //! these variants, the test fails on the kernel-call error rather
    //! than silently passing. The OCCT-gated integration tests in
    //! `tests/topology_attribute_primitives_direct.rs` cover the
    //! seedable branches; this module is the unit-level safety net for
    //! the no-op fall-through arm.
    //!
    //! Why each variant in step-9/10 (and not just one): the dispatch is
    //! a `match` with explicit arms for `Box`/`Sphere`/`Cylinder` and a
    //! single `_ => Ok(())` catch-all. A pin per variant kind protects
    //! against a future refactor that introduces an unintended catch-all
    //! that branches based on op shape (e.g. accidentally treating
    //! `Tube` as a primitive because it has a `radius` field).
    use super::*;
    use reify_ir::{
        ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryQuery,
        Mesh, TessError,
    };

    /// In-test `GeometryKernel` that errors from every method. Used to
    /// prove that `seed_primitive_attributes` does not call into the
    /// kernel for non-primitive variants — every test in this module
    /// passes the same `MockKernel` and asserts the seeder returns
    /// `Ok(())`. If the seeder reached the kernel, the wrapping `expect`
    /// would surface the synthetic error message.
    struct MockKernel;

    impl GeometryKernel for MockKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            unreachable!("seed_primitive_attributes must not call kernel.execute")
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed(
                "MockKernel::query should not be called for non-primitive ops".into(),
            ))
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            unreachable!("seed_primitive_attributes must not call kernel.export")
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            unreachable!("seed_primitive_attributes must not call kernel.tessellate")
        }

        fn extract_edges(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Err(QueryError::QueryFailed(
                "MockKernel::extract_edges should not be called for non-primitive ops".into(),
            ))
        }

        fn extract_faces(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Err(QueryError::QueryFailed(
                "MockKernel::extract_faces should not be called for non-primitive ops".into(),
            ))
        }
    }

    fn feature_id() -> FeatureId {
        FeatureId::new("Body#realization[0]")
    }

    /// Helper: invoke the seeder with a `MockKernel`, assert it returns
    /// `Ok(())`, and assert the table is unchanged. Panics on seeder
    /// error so each per-variant call site stays compact.
    #[track_caller]
    fn assert_seeds_nothing(op: &GeometryOp) {
        let mut kernel = MockKernel;
        let mut table = TopologyAttributeTable::default();
        // The face/edge slices must be empty — the dispatch is supposed
        // to never read them for non-seedable variants. Passing empty
        // slices means an accidental read would still not produce any
        // entries (rather than seeding garbage), but a fall-through into
        // the kernel for a Cylinder-shaped variant would still surface
        // via the kernel's `MockKernel::query` error.
        seed_primitive_attributes(&mut table, &mut kernel, &[], &[], &[], &feature_id(), op)
            .expect("seed_primitive_attributes must return Ok(()) for non-seedable variants");
        assert!(
            table.is_empty(),
            "seed_primitive_attributes must not write any entries for non-seedable variants; \
             table contains {} entries",
            table.len()
        );
    }

    /// Stand-in `GeometryHandleId` for op variants that need a target id.
    /// Picked deterministically so the test reports the same id across runs;
    /// the value is never resolved by the mock kernel.
    fn fake_target() -> GeometryHandleId {
        GeometryHandleId(7)
    }

    // ─── step-9 — explicit Translate + Union sanity checks ───────────────────
    //
    // The plan calls these out by name as the contract pins
    // `Engine::execute_realization_ops` will rely on (mixed primitive +
    // non-primitive op streams must produce no spurious entries for the
    // non-primitive ops). They live here separately from the
    // closed-extension matrix below so a regression on either cites the
    // step-9 test name in the failure output.

    #[test]
    fn seed_returns_ok_and_writes_no_entries_for_non_primitive_op() {
        // Translate is the canonical "auxiliary op the engine threads
        // through the same loop as a primitive constructor" — its
        // attribute attachment is the propagation phase (task 7), not
        // the seeding phase (this task). The dispatch must see Translate,
        // fall through, and leave the table untouched.
        assert_seeds_nothing(&GeometryOp::Translate {
            target: fake_target(),
            dx: 1.0,
            dy: 0.0,
            dz: 0.0,
        });
    }

    #[test]
    fn seed_returns_ok_and_writes_no_entries_for_boolean_op() {
        // Boolean ops are task 8's scope. The seeder must be a no-op for
        // them — the engine relies on this so it can call the seeder
        // unconditionally on every kernel-success path without per-op
        // gating. (Task 8 will add the boolean-op propagation hook in a
        // separate code path that runs alongside this seeder.)
        assert_seeds_nothing(&GeometryOp::Union {
            left: fake_target(),
            right: GeometryHandleId(8),
        });
    }

    // ─── step-10 — closed-extension contract pins per variant kind ───────────
    //
    // Per the design decision noted on the dispatch's `_ => Ok(())` arm:
    // non-primitive variants are intentional no-ops. Per-op auto-population
    // for them lands in PRD tasks 5 (sweeps), 7 (local features), and
    // 8 (booleans). These pins exercise one representative variant per kind
    // so an accidental dispatch widening (e.g. a future generic "shape with
    // radius field" handler) is caught at the unit-test layer rather than
    // surfacing as a downstream selector mis-resolution.

    #[test]
    fn seed_returns_ok_for_sweep_kind_extrude() {
        // Extrude is the canonical sweep — task 5's scope. Per-op routing
        // for sweeps will record cap (start/end) faces and side faces with
        // distinct `Role` values; until then the seeder must leave the
        // table untouched.
        assert_seeds_nothing(&GeometryOp::Extrude {
            profile: fake_target(),
            distance: Value::Real(1.0),
        });
    }

    #[test]
    fn seed_returns_ok_for_sweep_kind_pipe() {
        assert_seeds_nothing(&GeometryOp::Pipe {
            path: fake_target(),
            radius: Value::Real(1.0),
        });
    }

    #[test]
    fn seed_returns_ok_for_boolean_kind_difference() {
        // Sibling pin to the Union test above — Difference and Intersection
        // share the boolean-op no-op contract until task 8.
        assert_seeds_nothing(&GeometryOp::Difference {
            left: fake_target(),
            right: GeometryHandleId(8),
        });
    }

    #[test]
    fn seed_returns_ok_for_boolean_kind_intersection() {
        assert_seeds_nothing(&GeometryOp::Intersection {
            left: fake_target(),
            right: GeometryHandleId(8),
        });
    }

    #[test]
    fn seed_returns_ok_for_modify_kind_fillet() {
        // Fillet is the canonical local feature — task 7's scope.
        assert_seeds_nothing(&GeometryOp::Fillet {
            target: fake_target(),
            radius: Value::Real(0.001),
        });
    }

    #[test]
    fn seed_returns_ok_for_modify_kind_chamfer() {
        assert_seeds_nothing(&GeometryOp::Chamfer {
            target: fake_target(),
            distance: Value::Real(0.001),
        });
    }

    #[test]
    fn seed_returns_ok_for_transform_kind_rotate() {
        // Transforms are pure rigid-body; the v0.2 attribute model treats
        // them as identity for face/edge identity. Task 7's propagation
        // path will copy attributes through; the seeder is a no-op here.
        assert_seeds_nothing(&GeometryOp::Rotate {
            target: fake_target(),
            axis: [0.0, 0.0, 1.0],
            angle_rad: 0.5,
        });
    }

    #[test]
    fn seed_returns_ok_for_transform_kind_scale() {
        assert_seeds_nothing(&GeometryOp::Scale {
            target: fake_target(),
            factor: 2.0,
        });
    }

    #[test]
    fn seed_returns_ok_for_pattern_kind_linear_pattern() {
        // Patterns synthesize multiple copies; per-copy attribute attachment
        // is task 7's scope (each instance becomes its own feature subtree).
        assert_seeds_nothing(&GeometryOp::LinearPattern {
            target: fake_target(),
            direction: [1.0, 0.0, 0.0],
            count: 3,
            spacing: Value::Real(0.01),
        });
    }

    #[test]
    fn seed_returns_ok_for_pattern_kind_circular_pattern() {
        assert_seeds_nothing(&GeometryOp::CircularPattern {
            target: fake_target(),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            count: 4,
            angle: Value::Real(std::f64::consts::FRAC_PI_2),
        });
    }

    #[test]
    fn seed_returns_ok_for_curve_kind_line_segment() {
        // Curve constructors produce wires whose attributes are the source
        // for sweep ops — but the wire itself doesn't carry face/edge
        // attributes in the v0.2 model. The seeder is therefore a no-op.
        assert_seeds_nothing(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 1.0,
            y2: 0.0,
            z2: 0.0,
        });
    }

    #[test]
    fn seed_returns_ok_for_curve_kind_arc() {
        assert_seeds_nothing(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        });
    }

    #[test]
    fn seed_returns_ok_for_tube_kind() {
        // Tube is composed via boolean_cut at the kernel layer; its
        // attribute attachment depends on task 8's boolean propagation
        // (or a Tube-specific compound classifier). Defer to task 8.
        // This pin guarantees Tube is not accidentally swept into the
        // primitive seeding arm just because it shares fields like
        // `outer_r` / `inner_r` with the cylinder family.
        assert_seeds_nothing(&GeometryOp::Tube {
            outer_r: Value::Real(0.005),
            inner_r: Value::Real(0.003),
            height: Value::Real(0.010),
        });
    }

    // ─── step-9 — Wedge generic seeding (task-4158) ──────────────────────────
    //
    // RED until step-10 adds:
    //   - GeometryOp::Wedge to `is_seedable_primitive`
    //   - A Wedge arm in `seed_primitive_attributes` using the generic
    //     `record_all_faces_as_side` / `record_all_edges_as_new_edge` helpers.

    fn wedge_op() -> GeometryOp {
        GeometryOp::Wedge {
            width: Value::Real(0.020),
            depth: Value::Real(0.010),
            height: Value::Real(0.015),
            top_width: Value::Real(0.005),
        }
    }

    /// `is_seedable_primitive` must return `true` for a Wedge op.
    ///
    /// RED until step-10 adds `GeometryOp::Wedge { .. }` to the OR-list in
    /// `is_seedable_primitive`.
    #[test]
    fn is_seedable_primitive_wedge_returns_true() {
        assert!(
            is_seedable_primitive(&wedge_op()),
            "GeometryOp::Wedge must be recognised as a seedable primitive"
        );
    }

    /// `seed_primitive_attributes` must write exactly N_faces + N_edges entries
    /// for a Wedge, all faces as `Role::Side` and all edges as `Role::NewEdge`.
    ///
    /// Uses pre-supplied fake `GeometryHandleId` values — no kernel call
    /// needed (the Wedge arm only uses the provided handles, not the kernel).
    ///
    /// RED until step-10 adds the Wedge arm that calls
    /// `record_all_faces_as_side` + `record_all_edges_as_new_edge`.
    #[test]
    fn seed_primitive_attributes_wedge_records_role_side_and_new_edge() {
        let face_handles: Vec<GeometryHandleId> = (10..16).map(GeometryHandleId).collect(); // 6 fake faces
        let edge_handles: Vec<GeometryHandleId> = (20..32).map(GeometryHandleId).collect(); // 12 fake edges

        let fid = feature_id();
        let mut table = TopologyAttributeTable::default();
        let mut kernel = MockKernel;

        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &[], // no vertex seeding for wedge
            &fid,
            &wedge_op(),
        )
        .expect("seed_primitive_attributes for wedge must return Ok(())");

        assert_eq!(
            table.len(),
            face_handles.len() + edge_handles.len(),
            "wedge: must record one entry per face + one per edge"
        );

        for (idx, &fh) in face_handles.iter().enumerate() {
            let attr = table
                .lookup(fh)
                .unwrap_or_else(|| panic!("wedge face #{idx} (handle {:?}) must have an entry", fh));
            assert_eq!(
                attr.role,
                Role::Side,
                "wedge face #{idx} must be Role::Side (no Cap classification)"
            );
            assert_eq!(
                attr.local_index, idx as u32,
                "wedge face #{idx} local_index must be construction-order position"
            );
        }

        for (idx, &eh) in edge_handles.iter().enumerate() {
            let attr = table
                .lookup(eh)
                .unwrap_or_else(|| panic!("wedge edge #{idx} (handle {:?}) must have an entry", eh));
            assert_eq!(
                attr.role,
                Role::NewEdge,
                "wedge edge #{idx} must be Role::NewEdge"
            );
            assert_eq!(
                attr.local_index, idx as u32,
                "wedge edge #{idx} local_index must be construction-order position"
            );
        }
    }
}
