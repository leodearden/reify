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
//! Scope of this task (#2574):
//! - `GeometryOp::Box` — face entries seeded; edge entries arrive in step-7/8.
//! - All other variants are intentional no-ops; the dispatch is widened in
//!   subsequent steps.
//!
//! Variants intentionally deferred:
//! - `GeometryOp::Tube` — composed via `boolean_cut` at the kernel layer; its
//!   per-result attribute attachment lands with task 8 (booleans) or a Tube-
//!   specific follow-up.
//! - `GeometryOp::Cone` / `GeometryOp::Torus` — not yet present in
//!   `GeometryOp` (no FFI, no compiler `PrimitiveKind`); these primitives
//!   will be added end-to-end as a separate task before their seeding arms
//!   are wired here.
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

use reify_types::{
    CapKind, FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, QueryError,
    Role, TopologyAttribute, TopologyAttributeTable, Value,
};

/// Tolerance for cylinder cap-vs-side classification by face-normal
/// z-component. A face whose normal satisfies `nz > 1.0 - eps` is the
/// top cap; `nz < -1.0 + eps` is the bottom cap; otherwise the face
/// is a side. Picked to match the project's general geometric-tolerance
/// convention (1e-6) used in `OcctKernel`'s assertions and in the
/// `topology_selectors` filters.
const NORMAL_Z_EPSILON: f64 = 1.0e-6;

/// Seed per-face/per-edge `TopologyAttribute` records for a primitive
/// constructor's result handle.
///
/// Inputs:
/// - `table`: the table to write entries into.
/// - `kernel`: kept on the signature for arms that need geometric queries
///   (e.g. Cylinder uses `GeometryQuery::FaceNormal` to classify caps).
///   Box / Sphere arms touch only `face_handles` / `edge_handles` and never
///   call into the kernel.
/// - `face_handles`, `edge_handles`: TopExp-ordered handle vectors the
///   caller has pre-extracted (typically via `kernel.extract_faces(...)` /
///   `extract_edges(...)` on the primitive's result handle, immediately
///   after `kernel.execute(&op)` returns `Ok(handle)`). Each call to
///   `extract_*` allocates fresh handle ids, so callers must reuse the
///   same vectors for downstream lookups (tests, propagation).
/// - `feature_id`: the originating feature for every entry written here.
/// - `op`: matched on by variant. Non-primitive variants are silent
///   no-ops, leaving `table` unchanged. This lets
///   `Engine::execute_realization_ops` invoke the seeder unconditionally
///   on every kernel-success path without per-op gating.
///
/// Returns `Err(QueryError)` only when a primitive arm needs a kernel
/// query (Cylinder's `FaceNormal`) and the kernel reports an error.
/// Callers should treat this as auxiliary-metadata failure (warn and
/// continue) rather than a primary geometry failure.
pub fn seed_primitive_attributes(
    table: &mut TopologyAttributeTable,
    kernel: &mut dyn GeometryKernel,
    face_handles: &[GeometryHandleId],
    edge_handles: &[GeometryHandleId],
    feature_id: &FeatureId,
    op: &GeometryOp,
) -> Result<(), QueryError> {
    // Suppress unused-variable warning while only face arms are wired —
    // edge arms (step-8) consume `edge_handles`.
    let _ = edge_handles;
    match op {
        // Box and Sphere have byte-identical face-seeding semantics — every
        // face gets Role::Side with construction-order local_index. The
        // shared helper avoids per-arm drift if the invariant changes.
        GeometryOp::Box { .. } | GeometryOp::Sphere { .. } => {
            record_all_faces_as_side(table, face_handles, feature_id);
            Ok(())
        }
        GeometryOp::Cylinder { .. } => {
            // A cylinder emits 3 faces in OCCT's TopExp order: side, top
            // cap, bottom cap (order varies by OCCT version). Classify
            // each via `GeometryQuery::FaceNormal`'s z-component. Each
            // role appears exactly once, so local_index is always 0.
            for &face_id in face_handles.iter() {
                let normal_value = kernel.query(&GeometryQuery::FaceNormal(face_id))?;
                let nz = parse_normal_z(&normal_value)?;
                let role = classify_cylinder_face_role(nz);
                table.record(
                    face_id,
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
        // All other variants are intentional no-ops at this step. Subsequent
        // steps widen the dispatch to cover Sphere (step-6), and edge
        // seeding (step-8). Sweep / local-feature / boolean variants land
        // in tasks 5, 7, 8 of the PRD.
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
