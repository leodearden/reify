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
//!
//! # Relationship to the workspace-canonical fixtures
//!
//! Since task #6387 the workspace's canonical box/cube fixtures live in
//! `reify_test_support::fixtures` — `prismatic_box_mesh`,
//! `unit_cube_mesh`, `unwelded_prismatic_box_mesh` and the curved
//! `tessellated_cylinder_mesh`, together with the derived
//! `F32_STORAGE_REL` tolerance. Prefer those in any test that CAN reach
//! them. This module is the deliberate exception rather than a fourth
//! copy: see the "Judgment call (task #6387)" section on
//! [`unit_cube_mesh`] for the link constraint that forces it, and
//! `tests/cube_fixture_agreement.rs` for the executable guard that keeps
//! the two definitions in agreement.

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
///
/// # Judgment call (task #6387)
///
/// This function keeps its own box literal and deliberately does NOT
/// delegate to `reify_test_support::fixtures::unit_cube_mesh`, even
/// though that is now the workspace-canonical copy. Three reasons, in
/// order of how binding they are:
///
/// 1. **It would not compile.** This module is gated on
///    `cfg(any(test, feature = "test-fixtures"))`, and the
///    `test-fixtures` arm is reached from the crate's PLAIN library
///    artifact — that is exactly what the self-dev-dep in `Cargo.toml`
///    exists for, since cross-crate `tests/` binaries are separate
///    compilation units that do not inherit `cfg(test)`.
///    `reify-test-support` is only a `[dev-dependencies]` entry here, so
///    it is absent from that rlib's link closure. Referencing it from
///    this function is an unresolved-crate error, not a style
///    preference.
/// 2. **The fix would be worse than the duplication.** Making it resolve
///    means promoting `reify-test-support` to a normal, feature-gated
///    dependency of a kernel ADAPTER crate, dragging `reify-compiler`
///    and `reify-syntax` into the adapter's graph. That cuts against the
///    deliberately-inverted adapter -> eval layering this crate's own
///    manifest comments protect, and is a production-adjacent
///    dependency-graph change well outside a test-fixture dedup.
/// 3. **The divergence is watched, not forgotten.**
///    `crates/reify-kernel-manifold/tests/cube_fixture_agreement.rs` is
///    the executable guard: it asserts this fixture is bit-identical to
///    the canonical one at zero offset, and an exact per-component
///    translation of it at a dyadic offset. Edit either copy in
///    isolation and that test reds immediately.
///
/// So the goal #6387 was filed for — kill DRIFT, not bytes — is met here
/// by an assertion rather than by deletion.
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
