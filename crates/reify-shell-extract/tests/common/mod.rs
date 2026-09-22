//! Analytic slab-SDF fixtures shared by this crate's integration tests.
//!
//! Extracted from `medial_alignment_invariance.rs` (task #6566) so the
//! resolution-window sweep can reuse the builders rather than copy them. The
//! crate already carries several near-identical private `slab_sdf_3d` copies in
//! its unit-test modules (`medial.rs`, `mesher.rs`, `mid_surface.rs`,
//! `segmentation.rs`, `pruning.rs`, …); these are the integration-side
//! builders, and they are shared rather than duplicated a further time.
//!
//! # Grid-layout discipline
//!
//! `SampledField::data` is row-major with axis 0 OUTERMOST
//! (`data[i*ny*nz + j*nz + k]`), which is what `sample_at_index` reads. The
//! builder below follows `medial.rs`'s own `slab_sdf_3d` fixture. Note the
//! reify-eval-side slab fixtures (`shell_solve.rs::build_slab_sdf`,
//! `realization_read_api.rs::slab_field`) flatten z-outermost and are
//! transposed relative to this convention — do not copy those.

// Each Rust integration-test file is its own crate and takes only the helpers
// it needs, so an unused-here helper is expected rather than dead.
#![allow(dead_code)]

use reify_ir::value::{InterpolationKind, SampledField, SampledGridKind};
use std::sync::atomic::AtomicBool;

/// The eight sub-voxel alignments swept by every alignment test here: the
/// medial surface placed at `offset × h` past a sample plane. `0.0` is the
/// coincident alignment that #7527 fixes; the other seven already worked and
/// are kept so a regression in either direction is visible.
pub const SUB_VOXEL_OFFSETS: [f64; 8] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// Build an isotropic `voxel_count³` Regular3D [`SampledField`] whose value at
/// each grid point is `phi(x, y, z)`, with grid point `(i, j, k)` at world
/// `(bounds_min + i·h, bounds_min + j·h, bounds_min + k·h)`.
pub fn regular3d_field(
    name: &str,
    h: f64,
    voxel_count: usize,
    bounds_min: f64,
    phi: impl Fn(f64, f64, f64) -> f64,
) -> SampledField {
    let n = voxel_count;
    let axis_grid: Vec<f64> = (0..n).map(|i| bounds_min + (i as f64) * h).collect();
    let bounds_max = axis_grid[n - 1];

    // Row-major flat layout: data[i*n*n + j*n + k] at index (i,j,k).
    let mut data = Vec::with_capacity(n * n * n);
    for &x in &axis_grid {
        for &y in &axis_grid {
            for &z in &axis_grid {
                data.push(phi(x, y, z));
            }
        }
    }

    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![bounds_min, bounds_min, bounds_min],
        bounds_max: vec![bounds_max, bounds_max, bounds_max],
        spacing: vec![h, h, h],
        axis_grids: vec![axis_grid.clone(), axis_grid.clone(), axis_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Analytic Z-slab `φ = |z − mid_plane_world| − thickness/2`: a wall of the
/// given thickness whose medial surface is exactly the plane
/// `z = mid_plane_world`, placeable anywhere relative to the sample planes.
pub fn slab_field(
    mid_plane_world: f64,
    thickness: f64,
    h: f64,
    voxel_count: usize,
    bounds_min: f64,
) -> SampledField {
    regular3d_field(
        &format!("slab-mid{mid_plane_world}-t{thickness}"),
        h,
        voxel_count,
        bounds_min,
        |_x, _y, z| (z - mid_plane_world).abs() - 0.5 * thickness,
    )
}

/// Index of the sample plane nearest `world` on an axis starting at
/// `bounds_min` with spacing `h`.
pub fn nearest_index(world: f64, bounds_min: f64, h: f64) -> i32 {
    ((world - bounds_min) / h).round() as i32
}
