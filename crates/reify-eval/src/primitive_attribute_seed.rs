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
    FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, QueryError, Role, TopologyAttribute,
    TopologyAttributeTable,
};

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
    // Suppress unused-variable warnings while only the Box arm is wired —
    // the Cylinder arm (step-4) and edge arms (step-8) consume them.
    let _ = kernel;
    let _ = edge_handles;
    match op {
        GeometryOp::Box { .. } => {
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
            Ok(())
        }
        // All other variants are intentional no-ops at this step. Subsequent
        // steps widen the dispatch to cover Cylinder (step-4), Sphere
        // (step-6), and edge seeding (step-8). Sweep / local-feature /
        // boolean variants land in tasks 5, 7, 8 of the PRD.
        _ => Ok(()),
    }
}
