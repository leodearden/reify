//! Procedural tetrahedral-mesh generators for the PRD task #13 calibration
//! suite. Each fixture takes parametric inputs and returns
//! `(VolumeMesh, surface_node_indices: Vec<u32>)` — connectivity is
//! deterministic across parameter values, so a morph from `param_0` to
//! `param_1` is a strict node-position update.

use reify_types::{ElementOrderTag, VolumeMesh};
use std::collections::HashMap;

/// Sentinel pin used by `tests/calibration.rs`'s smoke test to verify the
/// helper module is wired in before the procedural generators land.
pub const MODULE_OK: bool = true;

/// 6-tet decomposition of a unit hex with main diagonal between corners 0
/// (origin) and 6 (opposite corner). Each tet is right-handed for the
/// canonical CW-from-bottom corner ordering used in CFD/FEA codes
/// (z=0 face: 0→1→2→3; z=top face: 4→5→6→7).
///
/// The split is face-conforming: every quad face is bisected by the same
/// diagonal seen from either side, so adjacent hex cells share a matching
/// triangulation and the mesh has no T-junctions.
pub(crate) const HEX_TO_6TETS: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

/// Hollow box mesh: the outer cube `[0, outer]^3` minus the inner cavity
/// `[wall_thickness, outer-wall_thickness]^3`. Cells with ALL three cell
/// indices strictly interior (`{1, ..., n-2}`) are skipped (cavity); every
/// other cell is hex-decomposed into 6 right-handed tets via
/// [`HEX_TO_6TETS`].
///
/// ## Parameters
///
/// - `outer` — outer cube edge length.
/// - `wall_thickness` — uniform wall thickness on all six sides; must satisfy
///   `0 < 2 * wall_thickness < outer`.
/// - `n` — total cells per axis (≥ 3). The first and last cells per axis are
///   wall layers (each one cell of width `wall_thickness`); the middle
///   `n - 2` cells span the cavity (or, for `n == 3`, the cavity is a single
///   cell).
///
/// ## Returns
///
/// `(mesh, surface_node_indices)` where `surface_node_indices` lists every
/// vertex that sits on either an outer face or the inner-cavity face. These
/// are the nodes the calibration sweep prescribes when running
/// [`reify_mesh_morph::elasticity_morph`] from one wall-thickness to another.
///
/// Vertex positions at `i ∈ {1, n-1}` (the inner-cavity boundary planes)
/// are the only ones that move when `wall_thickness` is varied. All other
/// vertex positions are determined by `outer` and `n` alone, so the
/// connectivity is preserved across the whole calibration sweep.
///
/// Disconnected vertices (cavity-interior grid points used by no wall cell,
/// only emerging at `n ≥ 4`) are filtered out of the compact vertex array,
/// which keeps the output mesh well-conditioned for the elasticity solver
/// (no zero-row diagonals).
pub fn box_mesh(outer: f64, wall_thickness: f64, n: usize) -> (VolumeMesh, Vec<u32>) {
    assert!(
        n >= 3,
        "box_mesh requires n ≥ 3 (front-wall + cavity + back-wall)"
    );
    assert!(
        wall_thickness > 0.0 && 2.0 * wall_thickness < outer,
        "box_mesh requires 0 < 2 * wall_thickness < outer"
    );

    let na = n + 1; // grid points per axis
    let nc = n; // cells per axis

    // Axis positions: [0, wt, ..., outer-wt, outer]. The outer-wall layers are
    // exactly one cell of width `wall_thickness`; the cavity span is split
    // evenly across `n - 2` cells.
    let mut axis: Vec<f64> = Vec::with_capacity(na);
    axis.push(0.0);
    axis.push(wall_thickness);
    let interior_cavity_cells = n - 2;
    if interior_cavity_cells > 1 {
        let cavity_span = outer - 2.0 * wall_thickness;
        let cavity_step = cavity_span / interior_cavity_cells as f64;
        for j in 1..interior_cavity_cells {
            axis.push(wall_thickness + j as f64 * cavity_step);
        }
    }
    axis.push(outer - wall_thickness);
    axis.push(outer);
    debug_assert_eq!(axis.len(), na);

    // A hex cell is a "wall cell" iff at least one of its three cell indices
    // is on the outermost layer (0 or nc - 1).
    let is_wall_cell = |ci: usize, cj: usize, ck: usize| -> bool {
        ci == 0 || ci == nc - 1 || cj == 0 || cj == nc - 1 || ck == 0 || ck == nc - 1
    };

    // Two-pass build: collect every wall-cell vertex first (so disconnected
    // cavity-interior grid points are never emitted), then emit tets
    // referencing the compact reindexed vertex set.
    let mut compact: HashMap<(usize, usize, usize), u32> = HashMap::new();
    let mut vertices: Vec<f32> = Vec::new();
    let mut surface_indices: Vec<u32> = Vec::new();

    for ck in 0..nc {
        for cj in 0..nc {
            for ci in 0..nc {
                if !is_wall_cell(ci, cj, ck) {
                    continue;
                }
                for dk in 0..2 {
                    for dj in 0..2 {
                        for di in 0..2 {
                            let key = (ci + di, cj + dj, ck + dk);
                            compact.entry(key).or_insert_with(|| {
                                let new_idx = (vertices.len() / 3) as u32;
                                let (i, j, k) = key;
                                vertices.push(axis[i] as f32);
                                vertices.push(axis[j] as f32);
                                vertices.push(axis[k] as f32);

                                // Surface set = outer-cube boundary ∪
                                // inner-cavity boundary. on_inner only fires
                                // when on_outer is false, so a vertex that
                                // sits on both (e.g. (0, 1, 1) at a wall edge)
                                // is added once via the outer branch.
                                let on_outer = i == 0
                                    || i == na - 1
                                    || j == 0
                                    || j == na - 1
                                    || k == 0
                                    || k == na - 1;
                                let on_inner = !on_outer
                                    && (i == 1
                                        || i == na - 2
                                        || j == 1
                                        || j == na - 2
                                        || k == 1
                                        || k == na - 2);
                                if on_outer || on_inner {
                                    surface_indices.push(new_idx);
                                }

                                new_idx
                            });
                        }
                    }
                }
            }
        }
    }

    let mut tet_indices: Vec<u32> = Vec::new();
    for ck in 0..nc {
        for cj in 0..nc {
            for ci in 0..nc {
                if !is_wall_cell(ci, cj, ck) {
                    continue;
                }
                let c = [
                    compact[&(ci, cj, ck)],
                    compact[&(ci + 1, cj, ck)],
                    compact[&(ci + 1, cj + 1, ck)],
                    compact[&(ci, cj + 1, ck)],
                    compact[&(ci, cj, ck + 1)],
                    compact[&(ci + 1, cj, ck + 1)],
                    compact[&(ci + 1, cj + 1, ck + 1)],
                    compact[&(ci, cj + 1, ck + 1)],
                ];
                for tet in &HEX_TO_6TETS {
                    tet_indices.extend_from_slice(&[
                        c[tet[0]], c[tet[1]], c[tet[2]], c[tet[3]],
                    ]);
                }
            }
        }
    }

    let mesh = VolumeMesh {
        vertices,
        tet_indices,
        element_order: ElementOrderTag::P1,
        normals: None,
    };
    (mesh, surface_indices)
}
