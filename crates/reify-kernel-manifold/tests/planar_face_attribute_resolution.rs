//! Capstone acceptance test for task-4262:
//! "extract_faces on a Manifold box must yield 6 stable, resolvable
//! planar-face handles, and `resolve_unique_by_attribute` must return
//! `Resolved` for each."
//!
//! # What this pins
//!
//! End-to-end through the real kernel + the real (unchanged) resolver:
//!
//! 1. **Coalescing** — `extract_faces` on a unit cube returns **6** handles
//!    (one per coalesced planar face), not 12 (one per mesh triangle).
//! 2. **Stability** — two calls with the same parent handle return an
//!    element-for-element identical `Vec<GeometryHandleId>` (per-parent
//!    memoization cache, step-4).
//! 3. **Resolvability** — with six distinct `(Role::Side, local_index)`
//!    attributes seeded into a `TopologyAttributeTable` (one per face),
//!    `resolve_unique_by_attribute` returns `AttributeResolution::Resolved`
//!    for each query with no diagnostics.
//!
//! Mirrors the seed-table-then-resolve pattern from
//! `reify-eval/tests/ad_hoc_selector_smoke_tests.rs` and the
//! `reify-eval/src/topology_attribute_resolver.rs` unit tests.
//!
//! # Test placement rationale
//!
//! `reify-kernel-manifold` dev-depends on `reify-eval` (Cargo.toml:46), so
//! this integration test binary can import both the real kernel and the real
//! resolver. `reify-eval` does NOT depend on the manifold adapter, so the test
//! cannot live there. Precedent: `tests/kernel_attribute_hook_integration.rs`
//! already combines `ManifoldKernel`, `TopologyAttributeTable` (reify-ir), and
//! reify-eval types in one test binary.
//!
//! # Feature gate
//!
//! Requires `features = ["test-fixtures"]` on the self-dev-dep so that
//! `reify_kernel_manifold::test_fixtures::unit_cube_mesh` is accessible. The
//! self-dev-dep in `Cargo.toml:55` activates it for all integration tests.

#[cfg(not(feature = "test-fixtures"))]
compile_error!(
    "planar_face_attribute_resolution.rs requires the `test-fixtures` feature. \
     The self-dev-dep in crates/reify-kernel-manifold/Cargo.toml should \
     activate this feature for ALL integration test binaries — restore it via \
     `reify-kernel-manifold = {{ path = \".\", features = [\"test-fixtures\"] }}` \
     in [dev-dependencies]."
);

use reify_core::{Diagnostic, SourceSpan};
use reify_eval::{AttributeQuery, AttributeResolution, resolve_unique_by_attribute};
use reify_ir::{
    FeatureId, GeometryKernel, Role, TopologyAttribute, TopologyAttributeTable,
};
use reify_kernel_manifold::{kernel::ManifoldKernel, test_fixtures::unit_cube_mesh};

/// Helper: ingest `unit_cube_mesh(offset)` and return the parent `GeometryHandleId`.
fn ingest(kernel: &mut ManifoldKernel, offset: [f32; 3]) -> reify_ir::GeometryHandleId {
    kernel
        .ingest_mesh(&unit_cube_mesh(offset))
        .expect("unit_cube_mesh fixture must produce a valid manifold")
        .id
}

/// End-to-end acceptance test for task-4262.
///
/// Verifies that after coplanar coalescing (step-2) and ID-stability
/// memoization (step-4):
///
/// - (1) `extract_faces` returns exactly **6** handles for a unit cube.
/// - (2) Two `extract_faces` calls on the same parent return **element-for-element
///   identical** vecs (stable IDs feeding the resolver).
/// - (3) `resolve_unique_by_attribute` returns `Resolved(faces[i])` for each
///   of six distinct `(Role::Side, local_index=i)` queries — one-handle-per-
///   face uniqueness guarantees no ambiguity and no multi-match degradation.
#[test]
fn extract_faces_6_stable_handles_resolve_uniquely_by_attribute() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    // (1) Coalesced planar faces: exactly 6 for a unit cube.
    let faces = kernel
        .extract_faces(handle)
        .expect("extract_faces must succeed on a stored unit cube");
    assert_eq!(
        faces.len(),
        6,
        "unit cube must yield 6 coalesced planar-face handles (BRep parity); got {:?}",
        faces,
    );

    // (2) ID stability: second call returns identical ids in identical order.
    let faces2 = kernel
        .extract_faces(handle)
        .expect("second extract_faces call must succeed");
    assert_eq!(
        faces,
        faces2,
        "extract_faces must return the same ids in the same order on every call \
         for the same parent handle (per-parent memoization); \
         first={:?}, second={:?}",
        faces,
        faces2,
    );

    // (3a) Seed a TopologyAttributeTable: one distinct (Role::Side, local_index)
    // attribute per coalesced planar face.  Using a single FeatureId for all
    // six because the resolver matches on (role, local_index) uniqueness within
    // the candidate set — feature_id acts as a filter, not a discriminator here.
    let feature_id = FeatureId::new("Box#realization[0]");
    let mut table = TopologyAttributeTable::default();
    for (i, &face_id) in faces.iter().enumerate() {
        table.record(
            face_id,
            TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::Side,
                local_index: i as u32,
                user_label: None,
                mod_history: Vec::new(),
            },
        );
    }

    // (3b) For each face, query by (Role::Side, local_index=i) and assert
    // the resolver returns Resolved(faces[i]) with no diagnostics.
    let span = SourceSpan::empty(0);
    for (i, &expected_handle) in faces.iter().enumerate() {
        let mut diags: Vec<Diagnostic> = Vec::new();
        let query = AttributeQuery {
            user_label: None,
            role_and_index: Some((Role::Side, i as u32)),
            feature_id: None,
        };
        let result = resolve_unique_by_attribute(&table, &faces, &query, span, &mut diags);
        assert_eq!(
            result,
            AttributeResolution::Resolved(expected_handle),
            "resolve_unique_by_attribute(Role::Side, local_index={i}) must return \
             Resolved(faces[{i}]={expected_handle:?}); got {result:?}",
        );
        assert!(
            diags.is_empty(),
            "no diagnostics expected for a unique attribute match \
             (face[{i}]); got {diags:?}",
        );
    }
}
