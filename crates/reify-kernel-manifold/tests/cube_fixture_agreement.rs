//! Executable drift guard between this crate's offset-taking
//! `test_fixtures::unit_cube_mesh` and the workspace-canonical box fixtures in
//! [`reify_test_support::fixtures`].
//!
//! # Why this file exists
//!
//! Task #6387 hoisted the box/cube fixtures into `reify_test_support::fixtures`
//! so the workspace has ONE definition of "a box". This crate's copy is the one
//! deliberate exception: `src/test_fixtures.rs` is gated on
//! `cfg(any(test, feature = "test-fixtures"))`, and the `test-fixtures` arm is
//! reached from the crate's PLAIN library artifact (that is the whole point of
//! the self-dev-dep in `Cargo.toml`), where `reify-test-support` — a
//! `[dev-dependencies]` entry here — is simply not linkable. Delegating to the
//! canonical fixture from that function is an unresolved-crate error, not a
//! style preference. See the "Judgment call (task #6387)" section on
//! `test_fixtures::unit_cube_mesh` for the full argument.
//!
//! Because the copy stays, an assertion is the only thing standing between the
//! two definitions and silent drift — precisely the failure class #6387 was
//! filed against (a fixture corrected in one copy and not the others makes
//! independent guards disagree about what "a box" is).
//!
//! # Why exact equality is sound here
//!
//! The offset case uses DYADIC components (`0.5`, `0.25`, `-2.0`). Every cube
//! coordinate is `0.0f32` or `1.0f32`, so `0.0 + off` and `1.0 + off` are
//! exactly representable in f32 and incur no rounding. `assert_eq!` on the raw
//! floats is therefore a statement about the fixture, not about arithmetic —
//! do NOT weaken it to a tolerance.

use reify_kernel_manifold::test_fixtures::unit_cube_mesh;
use reify_test_support::fixtures::{
    prismatic_box_mesh, unit_cube_mesh as canonical_unit_cube_mesh,
};

/// The zero-offset local cube is bit-identical to the canonical one.
#[test]
fn zero_offset_local_cube_is_bit_identical_to_canonical() {
    let local = unit_cube_mesh([0.0, 0.0, 0.0]);
    let canonical = canonical_unit_cube_mesh();

    assert_eq!(
        local.vertices, canonical.vertices,
        "reify_kernel_manifold::test_fixtures::unit_cube_mesh([0,0,0]) has drifted from \
         reify_test_support::fixtures::unit_cube_mesh() in its vertex buffer"
    );
    assert_eq!(
        local.indices, canonical.indices,
        "reify_kernel_manifold::test_fixtures::unit_cube_mesh([0,0,0]) has drifted from \
         reify_test_support::fixtures::unit_cube_mesh() in its index buffer (winding is \
         load-bearing: both fixtures are vetted OUTWARD)"
    );
    assert!(
        local.normals.is_none(),
        "local unit_cube_mesh grew a normals buffer; the canonical fixture has none"
    );
    assert!(
        canonical.normals.is_none(),
        "canonical unit_cube_mesh grew a normals buffer; the local fixture has none"
    );
}

/// A non-zero offset translates every vertex and leaves the topology alone.
#[test]
fn offset_translates_vertices_exactly_and_preserves_indices() {
    // Dyadic on purpose — see the module docs. `0.0f32 + off` and `1.0f32 + off`
    // are exactly representable for each of these, so equality is exact.
    let off = [0.5f32, 0.25, -2.0];
    let shifted = unit_cube_mesh(off);
    let canonical = canonical_unit_cube_mesh();

    assert_eq!(
        shifted.vertices.len(),
        canonical.vertices.len(),
        "offsetting must not change the vertex count"
    );
    for i in 0..canonical.vertices.len() / 3 {
        for k in 0..3 {
            assert_eq!(
                shifted.vertices[3 * i + k],
                canonical.vertices[3 * i + k] + off[k],
                "vertex {i} component {k}: offset cube must be the canonical cube \
                 translated by {off:?}"
            );
        }
    }
    assert_eq!(
        shifted.indices, canonical.indices,
        "offsetting must not renumber or re-wind any triangle"
    );
}

/// The canonical unit cube is exactly the 1x1x1 specialisation of the canonical
/// prismatic box — pinning `unit_cube_mesh` as a specialisation rather than an
/// independent second definition.
#[test]
fn canonical_unit_cube_is_the_unit_prismatic_box() {
    let cube = canonical_unit_cube_mesh();
    let box_111 = prismatic_box_mesh(1.0, 1.0, 1.0);

    assert_eq!(cube.vertices, box_111.vertices);
    assert_eq!(cube.indices, box_111.indices);
    assert!(cube.normals.is_none() && box_111.normals.is_none());
}
