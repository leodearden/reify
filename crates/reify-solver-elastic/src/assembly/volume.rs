//! Realized-`VolumeMesh` → global stiffness assembly dispatcher.
//!
//! PRD `docs/prds/v0_3/hex-wedge-meshing.md` Addendum (2026-07-04), contract
//! C-3 (task 4986). [`assemble_volume_mesh_stiffness`] is the reusable
//! substrate that assembles a realized [`reify_ir::VolumeMesh`] — Tet (P1 or
//! P2), Hex, or Wedge — into a global stiffness matrix, dispatching on
//! [`reify_ir::VolumeConnectivity`] to the matching per-element stiffness
//! entry point and scattering via [`super::assemble_global_stiffness`].
//!
//! This exercises the hex/wedge element assembly FROM a realized `VolumeMesh`
//! (previously only exercised directly from `SweptMesh3d` —
//! `crate::sweep::tests::swept_hex_mesh_converts_to_volume_mesh_hex_connectivity`
//! / `swept_wedge_mesh_converts_to_volume_mesh_wedge_connectivity`, task 4986
//! steps 3-4). It is deliberately NOT wired into `reify-eval`'s
//! `elastic_static` cantilever trampoline — that production edge
//! (element-type demand, `dispatch_volume_mesh` args, e2e leaves) is task
//! 4746 (C-4/C-5).

use faer::sparse::SparseRowMat;
use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};

use super::hex::element_stiffness_hex_p1;
use super::wedge::element_stiffness_wedge_p1;
use super::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness, assemble_global_stiffness,
    element_stiffness,
};
use crate::constitutive::IsotropicElastic;

/// Assemble the global stiffness matrix for a realized [`VolumeMesh`].
///
/// Dispatches on `vm.connectivity()`:
/// - `Tet { order: P1, .. }` → `chunks_exact(4)` + `element_stiffness(ElementOrder::P1, ..)`
/// - `Tet { order: P2, .. }` → `chunks_exact(10)` + `element_stiffness(ElementOrder::P2, ..)`
/// - `Hex { .. }` → `chunks_exact(8)` + [`element_stiffness_hex_p1`]
/// - `Wedge { .. }` → `chunks_exact(6)` + [`element_stiffness_wedge_p1`]
///
/// and scatters every element via [`assemble_global_stiffness`] (`mode`
/// controls single- vs. multi-threaded accumulation).
///
/// Node coordinates widen from `vm.vertices` (`f32`, stride 3) to `f64` via
/// [`VolumeMesh::vertex_f64`].
///
/// # Honest degradation
///
/// Returns `None` — mirroring
/// `reify_eval::compute_targets::elastic_static::volume_mesh_to_solver_mesh`'s
/// degradation style (realization-read-api §3.2-5) — when:
/// - `vm.vertices.len()` is not a multiple of 3 (malformed vertex buffer);
/// - the connectivity's flat index buffer is empty, or its length is not a
///   multiple of the family's nodes-per-element stride
///   ([`VolumeMesh::nodes_per_element`]);
/// - any element index is out of range (`>= n_nodes`).
///
/// On success, returns `(K, n_nodes)`: the assembled `3·n_nodes × 3·n_nodes`
/// global stiffness matrix and the node count, so callers sizing Dirichlet
/// BCs / load vectors don't need to recompute `vm.vertices.len() / 3`.
pub fn assemble_volume_mesh_stiffness(
    vm: &VolumeMesh,
    material: &IsotropicElastic,
    mode: AssemblyMode,
) -> Option<(SparseRowMat<usize, f64>, usize)> {
    if !vm.vertices.len().is_multiple_of(3) {
        return None;
    }
    let n_nodes = vm.vertices.len() / 3;

    let stride = vm.nodes_per_element();
    let flat_indices: &[u32] = match vm.connectivity() {
        VolumeConnectivity::Tet { indices, .. } => indices,
        VolumeConnectivity::Hex { indices } => indices,
        VolumeConnectivity::Wedge { indices } => indices,
    };
    if flat_indices.is_empty() || !flat_indices.len().is_multiple_of(stride) {
        return None;
    }

    // Widen every vertex (stride 3) to an [f64; 3] node coordinate;
    // `vertex_f64` bounds-checks the index too (defensive — `n` is already
    // `< n_nodes` by construction here, this can only fail on internal
    // overflow, matching `volume_mesh_to_solver_mesh`'s use of the same
    // accessor).
    let mut coords: Vec<[f64; 3]> = Vec::with_capacity(n_nodes);
    for n in 0..n_nodes {
        coords.push(vm.vertex_f64(n as u32)?);
    }

    // Build one (connectivity, k_e) pair per element, reshaping the flat
    // index buffer into per-element node-id slices and computing k_e via
    // the matching element-stiffness entry point.
    let n_elements = flat_indices.len() / stride;
    let mut elements_data: Vec<(Vec<usize>, ElementStiffness)> = Vec::with_capacity(n_elements);
    for chunk in flat_indices.chunks_exact(stride) {
        let mut conn: Vec<usize> = Vec::with_capacity(stride);
        let mut phys_nodes: Vec<[f64; 3]> = Vec::with_capacity(stride);
        for &idx in chunk {
            let idx = idx as usize;
            if idx >= n_nodes {
                return None;
            }
            conn.push(idx);
            phys_nodes.push(coords[idx]);
        }
        let k_e = match vm.connectivity() {
            VolumeConnectivity::Tet { order, .. } => {
                let order = match order {
                    ElementOrderTag::P1 => ElementOrder::P1,
                    ElementOrderTag::P2 => ElementOrder::P2,
                };
                element_stiffness(order, &phys_nodes, material)
            }
            VolumeConnectivity::Hex { .. } => {
                let arr: [[f64; 3]; 8] = phys_nodes
                    .try_into()
                    .expect("stride checked above: Hex chunk has exactly 8 nodes");
                element_stiffness_hex_p1(&arr, material)
            }
            VolumeConnectivity::Wedge { .. } => {
                let arr: [[f64; 3]; 6] = phys_nodes
                    .try_into()
                    .expect("stride checked above: Wedge chunk has exactly 6 nodes");
                element_stiffness_wedge_p1(&arr, material)
            }
        };
        elements_data.push((conn, k_e));
    }

    let elements: Vec<AssemblyElement<'_>> = elements_data
        .iter()
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn,
            k_e,
        })
        .collect();

    let k = assemble_global_stiffness(n_nodes, &elements, mode);
    Some((k, n_nodes))
}
