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
//! 1. The `test-fixtures`-gated API surface — concretely
//!    [`reify_kernel_manifold::test_fixtures::unit_cube_mesh`] and
//!    [`reify_kernel_manifold::test_fixtures::manifold_factory_for_test`] —
//!    is reachable from outside the crate when the feature is active.
//!    A regression that drops the self-dev-dep activation in
//!    `Cargo.toml:61` would break this test at compile time with an
//!    actionable message (see the `compile_error!` guard below).
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
//! `unit_cube_mesh` / `test_fixtures` API surface.

#[cfg(not(feature = "test-fixtures"))]
compile_error!(
    "boolean_ops_integration.rs requires the `test-fixtures` feature. \
     The self-dev-dep in crates/reify-kernel-manifold/Cargo.toml:61 should \
     activate this feature for ALL integration test binaries — if you are \
     seeing this error, that activation has been dropped. Restore it via \
     `reify-kernel-manifold = { path = \".\", features = [\"test-fixtures\"] }` \
     in [dev-dependencies]."
);

use reify_kernel_manifold::{kernel::ManifoldKernel, test_fixtures::unit_cube_mesh};
use reify_ir::{GeometryHandleId, GeometryKernel, GeometryOp};

/// Round-trip integration: construct kernel → ingest two cubes → chain
/// three boolean ops → tessellate the final result.
///
/// Geometric setup: two unit cubes overlapping by 0.5 on the x axis.
///   - `a = [0,1]³`
///   - `b = [0.5, 1.5] × [0,1] × [0,1]`
///   - `u = a ∪ b` (~1.5×1×1 hull)
///   - `d = a - b` (the [0, 0.5] x-slab of `a`)
///   - `result = u ∩ d` (= `d`, since `d ⊂ u`)
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
    // `ManifoldKernel::new()` is the production factory shape (its
    // boxed sibling `manifold_factory()` calls `::new()` then `Box::new`).
    // `ingest_mesh` is now a production trait method on `GeometryKernel`, so
    // we could use either the concrete type or the boxed factory here; we use
    // the concrete type for clarity.
    let mut kernel = ManifoldKernel::new();

    let a: GeometryHandleId = kernel
        .ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]))
        .expect("unit_cube_mesh fixture must be a valid manifold")
        .id;
    let b: GeometryHandleId = kernel
        .ingest_mesh(&unit_cube_mesh([0.5, 0.0, 0.0]))
        .expect("unit_cube_mesh fixture must be a valid manifold")
        .id;

    let u = kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("Union of two valid cubes must succeed");

    let d = kernel
        .execute(&GeometryOp::Difference { left: a, right: b })
        .expect("Difference of two valid cubes must succeed");

    let result = kernel
        .execute(&GeometryOp::Intersection {
            left: u.id,
            right: d.id,
        })
        .expect("Intersection of u and d must succeed (d ⊂ u, so result = d)");

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

    // Geometric sanity: result = u ∩ d where d = a - b = [0, 0.5] × [0,1]².
    // The tessellated mesh's x-extent must be capped at ≈ 0.5 (d's slab),
    // NOT at ≈ 1.5 (u's full x-extent). This pins that Intersection actually
    // clipped to `d` — a silent bug that returned `u` instead would produce
    // max_x ≈ 1.5 and fail here.
    //
    // Vertices are stored as a flat [x0,y0,z0, x1,y1,z1, ...] f32 array.
    // Tolerance 1e-4 absorbs manifold-csg floating-point rounding at the
    // shared x=0.5 face.
    let max_x = mesh
        .vertices
        .chunks(3)
        .map(|v| v[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_x <= 0.5 + 1e-4,
        "max x-coordinate of u ∩ d must be ≤ 0.5 (d's x-slab bound); got {max_x:.6} — \
         if this is ~1.5, Intersection returned u instead of d",
    );
    // Lower-bound pin: result must actually fill d's full slab up to x=0.5,
    // not collapse to a degenerate sliver near x=0. Catches a hypothetical
    // regression where Intersection returns a near-empty result that still
    // passes the nonempty-mesh and upper-bound checks.
    assert!(
        max_x >= 0.5 - 1e-4,
        "max x-coordinate of u ∩ d must reach d's far face at 0.5; got {max_x:.6} — \
         if max_x ≪ 0.5, Intersection produced a degenerate sliver",
    );
}
