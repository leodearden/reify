//! Test-only mesh fixtures shared between in-crate `mod tests` and
//! cross-crate integration tests under `tests/`.
//!
//! Gated on `cfg(any(test, feature = "test-fixtures"))` so the module is
//! reachable both from the lib's own `cfg(test)` build (in-crate unit
//! tests in `kernel.rs`) and from cross-crate integration test binaries
//! that pick up the `test-fixtures` feature via the self-dev-dep in
//! `Cargo.toml`. The `ingest_mesh` production trait method on
//! [`crate::kernel::ManifoldKernel`] is now the canonical ingestion path
//! (no longer gated); these fixtures remain gated because they are
//! test-only inputs not needed in production link closures.
//!
//! # Why a shared module rather than two copies
//!
//! `unit_cube_mesh` was previously duplicated verbatim between
//! `src/kernel.rs:mod tests` and `tests/boolean_ops_integration.rs`.
//! The two copies can drift — e.g. if a future test fixture grows new
//! face winding requirements or per-vertex attributes for an
//! attribute-propagation test, both copies must be kept in lock-step
//! manually. Extracting once into this module gives a single source of
//! truth without widening the production surface (the module is gated
//! behind test-fixtures so it never reaches production link closures).

use reify_ir::Mesh;
use crate::kernel::manifold_from_reify_mesh;

/// Closed unit cube as a `reify_types::Mesh`: 8 vertices, 12 outward-
/// facing triangles. Used by the boolean-op tests in this crate to
/// populate input handles via
/// [`reify_ir::GeometryKernel::ingest_mesh`].
///
/// Vertices are in the unit `[0, 1]³` corner-block; the `offset`
/// parameter shifts the cube by `(dx, dy, dz)` so two cubes can be made
/// to overlap (e.g. `unit_cube_mesh([0.5, 0.0, 0.0])` overlaps the
/// origin-anchored cube by 0.5 in x).
///
/// Triangle winding follows right-hand-rule outward normals so the
/// resulting Manifold is well-oriented and Boolean operations succeed.
pub fn unit_cube_mesh(offset: [f32; 3]) -> Mesh {
    let [dx, dy, dz] = offset;
    Mesh {
        vertices: vec![
            // 0..7 → (x, y, z) for the 8 cube corners
            0.0 + dx,
            0.0 + dy,
            0.0 + dz, // 0
            1.0 + dx,
            0.0 + dy,
            0.0 + dz, // 1
            1.0 + dx,
            1.0 + dy,
            0.0 + dz, // 2
            0.0 + dx,
            1.0 + dy,
            0.0 + dz, // 3
            0.0 + dx,
            0.0 + dy,
            1.0 + dz, // 4
            1.0 + dx,
            0.0 + dy,
            1.0 + dz, // 5
            1.0 + dx,
            1.0 + dy,
            1.0 + dz, // 6
            0.0 + dx,
            1.0 + dy,
            1.0 + dz, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z, so CW from +Z view)
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

/// Closed unit cube as a `manifold3d::Manifold` ready for boolean operations.
///
/// Delegates to [`unit_cube_mesh`] for the cube geometry, then calls
/// [`crate::kernel::manifold_from_reify_mesh`] (the same shared helper used by
/// the production `ingest_mesh` path) to do the f32→f64/u32→u64 conversion and
/// construct the `Manifold`. This ensures the fixture exercises the real
/// ingestion conversion and prevents the two callers from drifting independently.
///
/// Panics if the cube geometry is not a valid closed orientable manifold —
/// that would indicate a regression in [`unit_cube_mesh`]'s winding, not a
/// caller error.
///
/// Used by `union_meshgl64_exposes_provenance_and_merge_pairing_invariant` in
/// `kernel.rs` to build a union result whose `MeshGL64` carries multi-parent
/// provenance. This is the exact egress path that task 3525 (persistent-naming-v2
/// PRD task 9) will walk.
pub fn unit_cube_manifold(offset: [f32; 3]) -> manifold3d::Manifold {
    let mesh = unit_cube_mesh(offset);
    manifold_from_reify_mesh(&mesh)
        .expect(
            "unit_cube_manifold: unit_cube_mesh must be a valid closed orientable manifold",
        )
}
