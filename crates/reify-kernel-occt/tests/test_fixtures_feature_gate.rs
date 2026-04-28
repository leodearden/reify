//! Smoke test that pins the public surface exposed by the `test-fixtures` cargo
//! feature.
//!
//! If this file ever stops compiling, either the feature was removed from
//! `Cargo.toml`, or one of the gated helpers was renamed/removed. All 8
//! `store_*_for_test` methods must be reachable and callable under the feature.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use std::collections::HashSet;

use reify_kernel_occt::OcctKernel;

/// Verify that all 8 `store_*_for_test` fixture helpers are callable and each
/// returns a distinct `GeometryHandleId`.
///
/// Distinctness proves that each helper executed `kernel.store(...)` and
/// incremented the handle counter — i.e. no helper was silently no-op'd by a
/// missing cfg gate.
#[test]
fn all_eight_test_fixtures_callable_under_feature() {
    let mut kernel = OcctKernel::new();

    let id0 = kernel.store_circle_face_for_test(0.005, 0.0);
    let id1 = kernel.store_nonmanifold_compound_for_test();
    let id2 = kernel.store_malformed_solid_for_test();
    let id3 = kernel.store_nonorientable_shell_for_test();
    let id4 = kernel.store_closed_shell_for_test();
    let id5 = kernel.store_edge_for_test();
    let id6 = kernel.store_vertex_for_test();
    let id7 = kernel.store_compsolid_for_test();

    let ids = [id0, id1, id2, id3, id4, id5, id6, id7];

    let mut seen = HashSet::new();
    for id in ids {
        assert!(
            seen.insert(id),
            "duplicate GeometryHandleId {id:?} — a helper may not have stored its shape"
        );
    }
    assert_eq!(seen.len(), 8, "expected 8 distinct handles, got {}", seen.len());
}
