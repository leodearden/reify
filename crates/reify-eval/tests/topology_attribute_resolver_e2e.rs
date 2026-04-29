//! End-to-end integration test for the v0.2 persistent-naming-v2
//! attribute-based selector resolver (task 2652, PRD task 2).
//!
//! Pure-Rust unit coverage lives inside `topology_attribute_resolver.rs`'s
//! `#[cfg(test)] mod tests` block. This file's purpose is to prove the
//! public crate-root API (`reify_eval::resolve_unique_by_attribute` plus
//! the `AttributeQuery` / `AttributeResolution` re-exports) compiles and
//! dispatches against real OCCT-allocated `GeometryHandleId`s, with a
//! `TopologyAttributeTable` that was populated by the production
//! `seed_primitive_attributes` path rather than hand-built.
//!
//! Pattern after `topology_attribute_e2e.rs` and
//! `topology_attribute_primitives_direct.rs`: same `OCCT_AVAILABLE` gate,
//! same 10mm-cube `BOX_SIDE_M` constant, same "extract face/edge handles
//! ONCE and reuse" discipline (each `extract_*` allocates fresh kernel
//! handle ids).
//!
//! Three sub-cases per the step-17 plan:
//!
//! (a) Role/local_index match against a real Box face. Confirms the
//!     resolver picks the handle the seeder wrote for `(Role::Side,
//!     local_index = 3)` with no diagnostic emitted.
//!
//! (b) User-label preference rule end-to-end. After seeding, manually
//!     overwrite face 0's attribute with `user_label = Some("manual")`
//!     so the user_label match (face 0) and the role/idx match (face 3)
//!     point at different handles. The resolver must return face 0,
//!     pinning PRD line 62 against the real handle space.
//!
//! (c) Imported-geometry fallback. Querying against an unallocated
//!     handle id that the kernel never minted (`GeometryHandleId(99999)`)
//!     returns `FallbackToComputed` because no candidate carries an
//!     entry in the table.

use reify_eval::{
    AttributeQuery, AttributeResolution, resolve_unique_by_attribute, seed_primitive_attributes,
};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    FeatureId, GeometryHandleId, GeometryOp, RealizationNodeId, Role, SourceSpan,
    TopologyAttribute, TopologyAttributeTable, Value,
};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

fn ten_mm_box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

fn body_realization_feature_id() -> FeatureId {
    FeatureId::from(&RealizationNodeId::new("Body", 0))
}

#[test]
fn resolver_dispatches_against_seeded_box_attributes() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // ─── Build a 10mm cube and pre-extract face/edge handles ─────────
    // Each `extract_*` allocates fresh handle ids, so we extract once
    // and reuse the same vectors for both seeding and resolver lookups.
    let mut kernel = OcctKernelHandle::spawn();
    let box_op = ten_mm_box_op();
    let box_id = kernel
        .execute(&box_op)
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a 10mm box must have exactly 6 faces in TopExp order"
    );

    // ─── Seed via the production path ────────────────────────────────
    // `seed_primitive_attributes` stamps `Role::Side` with
    // construction-order `local_index` 0..5 on every face for a Box.
    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes(
        &mut table,
        &mut kernel,
        &face_handles,
        &edge_handles,
        &feature_id,
        &box_op,
    )
    .expect("seed_primitive_attributes(Box) should succeed");

    // Sanity: each face has the expected (Role::Side, local_index = i)
    // entry. This pins the seeding contract the resolver depends on.
    for (idx, &face_id) in face_handles.iter().enumerate() {
        let attr = table
            .lookup(face_id)
            .expect("box face must have a seeded TopologyAttribute");
        assert_eq!(attr.role, Role::Side);
        assert_eq!(attr.local_index, idx as u32);
        assert_eq!(attr.user_label, None);
    }

    // ─── (a) Role/local_index match returns Resolved ────────────────
    let span = SourceSpan::empty(0);
    let query_role_idx = AttributeQuery {
        user_label: None,
        role_and_index: Some((Role::Side, 3)),
        feature_id: None,
    };
    let mut diagnostics = Vec::new();
    let result_a = resolve_unique_by_attribute(
        &table,
        &face_handles,
        &query_role_idx,
        span,
        &mut diagnostics,
    );
    assert_eq!(
        result_a,
        AttributeResolution::Resolved(face_handles[3]),
        "(Role::Side, 3) should resolve to face_handles[3]"
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic on a unique role/idx match (got {:?})",
        diagnostics
    );

    // ─── (b) user_label preference rule end-to-end ───────────────────
    // Manually overwrite face 0 with `user_label = Some("manual")`.
    // The role/idx branch would still point at face 3 for
    // `(Role::Side, 3)`, but PRD line 62 says a unique user_label match
    // wins — the resolver must return face 0.
    let original_face_0 = table
        .lookup(face_handles[0])
        .expect("face 0 must already be seeded")
        .clone();
    table.record(
        face_handles[0],
        TopologyAttribute {
            user_label: Some("manual".to_string()),
            ..original_face_0
        },
    );

    let query_label = AttributeQuery {
        user_label: Some("manual".to_string()),
        role_and_index: Some((Role::Side, 3)),
        feature_id: None,
    };
    let mut diagnostics = Vec::new();
    let result_b =
        resolve_unique_by_attribute(&table, &face_handles, &query_label, span, &mut diagnostics);
    assert_eq!(
        result_b,
        AttributeResolution::Resolved(face_handles[0]),
        "user_label=\"manual\" must win over (Role::Side, 3) per PRD line 62 \
         (face 0 has the label, face 3 has the role/idx)"
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic on a unique user_label match (got {:?})",
        diagnostics
    );

    // ─── (c) Imported-geometry fallback against a real handle space ──
    // `GeometryHandleId(99999)` is not minted by the kernel for this
    // box (the kernel allocates ids sequentially starting from 1, and
    // the box only produces a handful of faces and edges). It therefore
    // has no entry in the table, so `resolve_unique_by_attribute`
    // returns `FallbackToComputed` — the imported-geometry signal —
    // without emitting a diagnostic.
    let unallocated = [GeometryHandleId(99999)];
    let query_any = AttributeQuery {
        user_label: Some("anything".to_string()),
        role_and_index: Some((Role::Side, 0)),
        feature_id: None,
    };
    let mut diagnostics = Vec::new();
    let result_c =
        resolve_unique_by_attribute(&table, &unallocated, &query_any, span, &mut diagnostics);
    assert_eq!(
        result_c,
        AttributeResolution::FallbackToComputed,
        "an unallocated handle has no attribute entry → FallbackToComputed \
         (the imported-geometry signal per PRD line 68)"
    );
    assert!(
        diagnostics.is_empty(),
        "fallback emits no diagnostic — it is an expected path for imported \
         geometry, not a failure (got {:?})",
        diagnostics
    );
}
