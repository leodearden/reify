//! Closed-mesh fixtures shared by this crate's unit tests and its `tests/`
//! integration binaries.
//!
//! # Why a module rather than a copy per test file
//!
//! `tests/*.rs` are separate compilation units and do NOT inherit the parent
//! crate's `cfg(test)`, so a fixture defined in a `#[cfg(test)] mod tests` is
//! unreachable from them. Before this module the 100 × 100 × 1 plate existed
//! twice — once in `mesh_to_voxel_options.rs`'s unit tests and once in
//! `tests/mesh_to_voxel_resolution_tests.rs` — each with its own copy of the
//! winding table and the units rationale. Gating on
//! `any(test, feature = "test-fixtures")` reaches both, following the same
//! pattern that already exposes `OpenVdbKernel::open_vdb_grid_for_test`
//! (`Cargo.toml`'s `test-fixtures` feature and self-dev-dep).
//!
//! # Units
//!
//! Every coordinate here is a model-space length in the mesh's own units
//! (SI metres, per [`reify_ir::Mesh::vertices`]). See
//! [`reify_ir::VoxelResolution`]'s "Units" section — the single normative
//! statement of what a length in a resolution request means — before reading a
//! millimetre figure into any fixture below.

use reify_ir::Mesh;

/// Closed axis-aligned box spanning `min`..`max`: 8 corner vertices and 12
/// outward-wound triangles.
///
/// The single winding table every fixture in this module is built from, so a
/// box and a plate cannot disagree about face orientation — `meshToVolume`
/// signs the interior from that orientation, and an inward-wound face inverts
/// the sign of everything downstream.
pub fn axis_aligned_box(min: [f32; 3], max: [f32; 3]) -> Mesh {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;

    #[rustfmt::skip]
    let vertices: Vec<f32> = vec![
        x0, y0, z0, // 0
        x1, y0, z0, // 1
        x1, y1, z0, // 2
        x0, y1, z0, // 3
        x0, y0, z1, // 4
        x1, y0, z1, // 5
        x1, y1, z1, // 6
        x0, y1, z1, // 7
    ];
    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        // Bottom (-Z)
        0, 2, 1,  0, 3, 2,
        // Top (+Z)
        4, 5, 6,  4, 6, 7,
        // Front (-Y)
        0, 1, 5,  0, 5, 4,
        // Back (+Y)
        2, 3, 7,  2, 7, 6,
        // Left (-X)
        0, 4, 7,  0, 7, 3,
        // Right (+X)
        1, 2, 6,  1, 6, 5,
    ];

    Mesh { vertices, indices, normals: None }
}

/// Closed box centred at the origin with the given half-extents, i.e. total
/// extents `2 * hx × 2 * hy × 2 * hz`.
pub fn box_mesh(hx: f32, hy: f32, hz: f32) -> Mesh {
    axis_aligned_box([-hx, -hy, -hz], [hx, hy, hz])
}

/// The shells PRD's motivating PROPORTIONS
/// (`docs/prds/v0_4/structural-analysis-shells.md`, "Background"): a thin
/// feature 1/100 of the part across, which the PRD states as a 1 mm feature in
/// a 100 mm part. Spans `x, y ∈ [0, 100]`, `z ∈ [0, 1]`.
///
/// The coordinates are the PRD's millimetre figures written at ×1000 so the
/// arithmetic in the tests stays in round numbers; the resolution policy is
/// scale-invariant (`h = longest/64`, `h = t/4` and the band rule are all
/// ratios), so every conclusion drawn on this body holds verbatim for the real
/// 0.1 × 0.1 × 0.001 m part with `MinFeature(0.001)`. See the module's "Units".
///
/// Deliberately NOT origin-centred: OpenVDB grids index from the world origin,
/// so at h = 0.25 the voxel planes fall at z = 0, 0.25, 0.5, 0.75, 1.0 and a
/// mid-plane probe lands exactly on a sample point.
pub fn plate_100x100x1() -> Mesh {
    axis_aligned_box([0.0, 0.0, 0.0], [100.0, 100.0, 1.0])
}
