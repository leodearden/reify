//! Cross-crate integration test exercising the real `manifold3d`-backed
//! `ManifoldKernel` end-to-end: ingestion → chained Union/Difference/
//! Intersection → tessellation.
//!
//! # Why this test exists
//!
//! The in-crate `mod tests` unit tests in `src/kernel.rs` validate each
//! boolean arm and `tessellate` in isolation. This integration binary
//! pins three additional concerns:
//!
//! 1. The factory entry point in `register.rs` (specifically
//!    [`manifold_factory_for_test`]) is reachable from outside the crate
//!    when the `test-fixtures` feature is active. A regression that drops
//!    the self-dev-dep activation in `Cargo.toml:61` would break this
//!    test at compile time with an actionable message (see the
//!    `compile_error!` guard below).
//! 2. The kernel correctly chains operations: each output handle from
//!    one boolean arm is a valid input handle for the next.
//! 3. Tessellating a derived (not just ingested) `Manifold` returns a
//!    non-empty mesh — exercises the round-trip
//!    `from_mesh_f64 → boolean → to_mesh_f64` path the production
//!    pipeline relies on.
//!
//! # Compile-time feature guard
//!
//! Mirrors the now-deleted `tests/common/mod.rs` pattern, scoped to this
//! single test binary. If a future Cargo.toml refactor accidentally drops
//! the `features = ["test-fixtures"]` activation on the self-dev-dep,
//! this guard fires at compile time with an explanatory message rather
//! than producing a confusing "unknown function" error from the missing
//! `manifold_factory_for_test` symbol.

#[cfg(not(feature = "test-fixtures"))]
compile_error!(
    "boolean_ops_integration.rs requires the `test-fixtures` feature. \
     The self-dev-dep in crates/reify-kernel-manifold/Cargo.toml:61 should \
     activate this feature for ALL integration test binaries — if you are \
     seeing this error, that activation has been dropped. Restore it via \
     `reify-kernel-manifold = { path = \".\", features = [\"test-fixtures\"] }` \
     in [dev-dependencies]."
);

use reify_kernel_manifold::{kernel::ManifoldKernel, register::manifold_factory_for_test};
use reify_types::{GeometryHandleId, GeometryKernel, GeometryOp, Mesh};

/// Closed unit cube as a `reify_types::Mesh` with right-hand-rule outward
/// normals (so the Manifold is well-oriented). Mirrors the helper in
/// `src/kernel.rs:mod tests`.
fn unit_cube_mesh(offset: [f32; 3]) -> Mesh {
    let [dx, dy, dz] = offset;
    Mesh {
        vertices: vec![
            0.0 + dx, 0.0 + dy, 0.0 + dz, // 0
            1.0 + dx, 0.0 + dy, 0.0 + dz, // 1
            1.0 + dx, 1.0 + dy, 0.0 + dz, // 2
            0.0 + dx, 1.0 + dy, 0.0 + dz, // 3
            0.0 + dx, 0.0 + dy, 1.0 + dz, // 4
            1.0 + dx, 0.0 + dy, 1.0 + dz, // 5
            1.0 + dx, 1.0 + dy, 1.0 + dz, // 6
            0.0 + dx, 1.0 + dy, 1.0 + dz, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z)
            0, 2, 1,  0, 3, 2,
            // +Z top
            4, 5, 6,  4, 6, 7,
            // -Y front
            0, 1, 5,  0, 5, 4,
            // +Y back
            3, 7, 6,  3, 6, 2,
            // -X left
            0, 4, 7,  0, 7, 3,
            // +X right
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

/// Round-trip integration: factory → ingest two cubes → chain three
/// boolean ops → tessellate the final result.
///
/// Geometric setup: two unit cubes overlapping by 0.5 on the x axis.
///   - `a = [0,1]³`
///   - `b = [0.5, 1.5] × [0,1] × [0,1]`
///   - `u = a ∪ b` (~1.5×1×1 hull)
///   - `d = a - b` (the [0, 0.5] x-slab of `a`)
///   - `result = u ∩ a` (= `a`, since `a ⊂ u`)
///
/// We tessellate `result` and pin the structural mesh contract: at least
/// one vertex, at least one triangle, vertex count divisible by 3 (xyz),
/// index count divisible by 3 (triangles). We deliberately do NOT pin the
/// vertex count exactly — Manifold may merge co-planar faces or simplify
/// the boundary; the `Manifold::merge` semantics are the manifold-csg
/// crate's invariant, not a property this Reify integration test should
/// double-pin.
#[test]
fn boolean_ops_round_trip_via_factory_and_geometry_kernel_trait_object() {
    let mut kernel: ManifoldKernel = manifold_factory_for_test();

    let a: GeometryHandleId = kernel.store_mesh_for_test(&unit_cube_mesh([0.0, 0.0, 0.0]));
    let b: GeometryHandleId = kernel.store_mesh_for_test(&unit_cube_mesh([0.5, 0.0, 0.0]));

    let u = kernel
        .execute(&GeometryOp::Union {
            left: a,
            right: b,
        })
        .expect("Union of two valid cubes must succeed");

    let _d = kernel
        .execute(&GeometryOp::Difference {
            left: a,
            right: b,
        })
        .expect("Difference of two valid cubes must succeed");

    let result = kernel
        .execute(&GeometryOp::Intersection {
            left: u.id,
            right: a,
        })
        .expect("Intersection of u and a must succeed (a ⊂ u, so result = a)");

    let mesh = kernel
        .tessellate(result.id, 0.0)
        .expect("tessellate of derived Manifold must succeed");

    assert!(
        !mesh.vertices.is_empty(),
        "tessellated chained-boolean mesh must have at least one vertex",
    );
    assert!(
        !mesh.indices.is_empty(),
        "tessellated chained-boolean mesh must have at least one triangle",
    );
    assert_eq!(
        mesh.vertices.len() % 3,
        0,
        "tessellated mesh vertices must come in xyz triplets",
    );
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "tessellated mesh indices must come in triangle triplets",
    );
}
