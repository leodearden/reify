//! The analytical references and reference fixtures shared by this crate's
//! analytical-validation binaries: the Timoshenko cantilever tip deflection
//! and its face-mean measurement, P1 nodal stress recovery, and the Lamé
//! thick-walled-cylinder reference with its quarter-annulus P1 mesh,
//! symmetry / plane-strain BCs, inner-pressure loads, solve-and-recover
//! driver and its inner-ring max-von-Mises measurement.
//!
//! Not a test binary. It builds on `fea_harness`, so a consumer declares both
//! `#[path = "common/fea_harness.rs"] mod fea_harness;` and
//! `#[path = "common/analytical_fixtures.rs"] mod analytical_fixtures;` at its crate root.

use reify_solver_elastic::{
    DirichletBc, FaceOrder, IsotropicElastic, StressElement, apply_traction_load,
    element_stress_p1, recover_nodal_stress_p1, tet_volume_p1,
};

use crate::fea_harness::{
    gather_u_p1, kuhn_split_hex_to_six_tets, solve_p1_pipeline, von_mises_of_tensor,
};

/// Mean transverse (y) displacement over the `end` nodes ≈ the neutral-axis tip
/// deflection (the quantity 1-D beam theory predicts). Returned as a magnitude.
pub fn mean_tip_deflection(u: &[f64], end: &[usize]) -> f64 {
    let s: f64 = end.iter().map(|&n| u[n * 3 + 1]).sum();
    (s / end.len() as f64).abs()
}

/// Timoshenko cantilever tip deflection under an end shear `F`:
/// `δ = FL³/(3EI) + FL/(G·A·k_s)` with `I = b·h³/12`, `A = b·h`,
/// `G = E/(2(1+ν))`, shear-correction `k_s = 5/6`.
pub fn timoshenko_tip_deflection(f: f64, l: f64, h: f64, b: f64, mat: &IsotropicElastic) -> f64 {
    let g = mat.youngs_modulus / (2.0 * (1.0 + mat.poisson_ratio));
    let i_bending = b * h.powi(3) / 12.0;
    let area = b * h;
    let k_s = 5.0 / 6.0;
    f * l.powi(3) / (3.0 * mat.youngs_modulus * i_bending) + f * l / (g * area * k_s)
}

/// Recover the continuous nodal stress field of a P1 tet mesh: per-element
/// constant Cauchy stress (`element_stress_p1`) volume-weighted into nodal
/// tensors (`recover_nodal_stress_p1`).
pub fn recover_nodal_p1(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    u: &[f64],
    mat: &IsotropicElastic,
) -> Vec<[[f64; 3]; 3]> {
    let elems: Vec<StressElement<'_>> = conns
        .iter()
        .map(|conn| {
            let en = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
            StressElement {
                connectivity: conn.as_slice(),
                stress: element_stress_p1(&en, mat, &gather_u_p1(u, conn)),
                volume: tet_volume_p1(&en),
            }
        })
        .collect();
    recover_nodal_stress_p1(nodes.len(), &elems)
}

/// Build a structured P1 tet mesh for a quarter-annulus
/// `a ≤ r ≤ b`, `θ ∈ [0, π/2]`, `z ∈ [0, l]`
/// with `nr × ntheta × nz` hex cells (Kuhn-split into 6 tets each).
///
/// # Node indexing
///
/// `node(ir, iθ, iz) = iz·(nθ+1)·(nr+1) + iθ·(nr+1) + ir`  (radial fastest).
///
/// Physical positions: `x = r_ir·cos(θ_iθ)`, `y = r_ir·sin(θ_iθ)`, `z = z_iz`
/// where `r_ir = a + ir·(b−a)/nr`, `θ_iθ = iθ·π/(2·nθ)`, `z_iz = iz·l/nz`.
///
/// Corner ordering per hex cell matches
/// [`crate::fea_harness::box_p1_mesh`]'s `c[0..8]` (radial → x,
/// angular → y, axial → z), so the existing `kuhn_split_hex_to_six_tets` yields
/// positive-volume tets on a right-handed cylindrical grid.
///
/// # Returns
///
/// `(nodes, tet_connectivity, inner_p1_faces)` — `inner_p1_faces` are the
/// `2·nθ·nz` P1 triangles on the inner surface (`r = a`).
#[allow(clippy::type_complexity)]
pub fn annular_p1_mesh(
    a: f64, b: f64, l: f64,
    nr: usize, ntheta: usize, nz: usize,
) -> (Vec<[f64; 3]>, Vec<[usize; 4]>, Vec<[usize; 3]>) {
    use std::f64::consts::PI;

    let nnr = nr + 1;
    let nntheta = ntheta + 1;
    let nnz = nz + 1;

    let mut nodes = Vec::with_capacity(nnr * nntheta * nnz);
    for iz in 0..nnz {
        for itheta in 0..nntheta {
            for ir in 0..nnr {
                let r = a + ir as f64 * (b - a) / nr as f64;
                let theta = itheta as f64 * PI / (2.0 * ntheta as f64);
                let z = iz as f64 * l / nz as f64;
                nodes.push([r * theta.cos(), r * theta.sin(), z]);
            }
        }
    }

    let node_idx = |ir: usize, itheta: usize, iz: usize| -> usize {
        iz * nntheta * nnr + itheta * nnr + ir
    };

    let mut connectivity = Vec::with_capacity(6 * nr * ntheta * nz);
    let mut inner_faces = Vec::with_capacity(2 * ntheta * nz);

    for iz in 0..nz {
        for itheta in 0..ntheta {
            for ir in 0..nr {
                // 8 hex corners in box_p1_mesh order (radial→x, angular→y, axial→z)
                let c = [
                    node_idx(ir,     itheta,     iz),
                    node_idx(ir + 1, itheta,     iz),
                    node_idx(ir + 1, itheta + 1, iz),
                    node_idx(ir,     itheta + 1, iz),
                    node_idx(ir,     itheta,     iz + 1),
                    node_idx(ir + 1, itheta,     iz + 1),
                    node_idx(ir + 1, itheta + 1, iz + 1),
                    node_idx(ir,     itheta + 1, iz + 1),
                ];
                for tet in kuhn_split_hex_to_six_tets(c) {
                    connectivity.push(tet);
                }
                // Inner surface (ir=0): two Kuhn triangles on the low-r (low-x) face.
                // Face quad {c0,c3,c7,c4}; the triangles wholly in this face are
                // (c0,c3,c7) and (c0,c4,c7) — verified against the Kuhn split.
                if ir == 0 {
                    inner_faces.push([c[0], c[3], c[7]]);
                    inner_faces.push([c[0], c[4], c[7]]);
                }
            }
        }
    }

    (nodes, connectivity, inner_faces)
}

/// Lamé thick-walled cylinder plane-strain analytical stress components.
///
/// For a cylinder `a ≤ r ≤ b` with inner pressure `P_i` and zero outer pressure:
/// - `σ_r(r) = P_i·a²/(b²−a²)·(1 − b²/r²)`
/// - `σ_θ(r) = P_i·a²/(b²−a²)·(1 + b²/r²)`
/// - `σ_z(r) = ν·(σ_r+σ_θ)`  [plane-strain constraint, ε_z=0]
///
/// Returns `(σ_r, σ_θ, σ_z)`.
pub fn lame_stresses(r: f64, a: f64, b: f64, p_i: f64, nu: f64) -> (f64, f64, f64) {
    let denom = b * b - a * a;
    let c = p_i * a * a / denom;
    let sr = c * (1.0 - b * b / (r * r));
    let st = c * (1.0 + b * b / (r * r));
    let sz = nu * (sr + st);
    (sr, st, sz)
}

/// von Mises stress for the Lamé plane-strain cylinder at radius `r`.
///
/// The stress state is purely diagonal (principal stresses = `(σ_r, σ_θ, σ_z)`)
/// with no shear, so von Mises reduces to the principal-stress formula.
pub fn lame_von_mises(r: f64, a: f64, b: f64, p_i: f64, nu: f64) -> f64 {
    let (sr, st, sz) = lame_stresses(r, a, b, p_i, nu);
    let s = [[sr, 0.0, 0.0], [0.0, st, 0.0], [0.0, 0.0, sz]];
    von_mises_of_tensor(&s)
}

/// Build Dirichlet BCs for the quarter-annulus symmetry planes and
/// plane-strain z-faces.
///
/// Per-axis constraints (not full 3-DOF clamp):
/// - `u_y = 0` on the `θ=0` face (`y ≈ 0`) — symmetry BC
/// - `u_x = 0` on the `θ=π/2` face (`x ≈ 0`) — symmetry BC
/// - `u_z = 0` on `z = 0` and `z = l` — plane-strain constraint (ε_z = 0)
///
/// Shared edge/corner nodes may receive multiple BCs that are
/// deduplicated by `dedup_bcs` before row elimination.
pub fn annular_symmetry_plane_strain_bcs(
    nodes: &[[f64; 3]],
    l: f64,
    tol: f64,
) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, &p) in nodes.iter().enumerate() {
        // θ=0 face: y ≈ 0 → u_y = 0 (DOF 1)
        if p[1].abs() < tol {
            bcs.push(DirichletBc { dof: node * 3 + 1, value: 0.0 });
        }
        // θ=π/2 face: x ≈ 0 → u_x = 0 (DOF 0)
        if p[0].abs() < tol {
            bcs.push(DirichletBc { dof: node * 3, value: 0.0 });
        }
        // z=0 face: u_z = 0 (DOF 2)
        if p[2].abs() < tol {
            bcs.push(DirichletBc { dof: node * 3 + 2, value: 0.0 });
        }
        // z=l face: u_z = 0 (DOF 2)
        if (p[2] - l).abs() < tol {
            bcs.push(DirichletBc { dof: node * 3 + 2, value: 0.0 });
        }
    }
    bcs
}

/// Shared implementation for P1-tri, P2-tri and P1-quad inner-pressure load
/// assembly.
///
/// For each face in `inner_faces`:
/// 1. Build `face_phys` from the node indices (every node of the face).
/// 2. Compute the centroid radial direction `r̂ = (cx, cy, 0) / ||(cx,cy)||`
///    from the **first 3 corner nodes** (`face[0..3]`), whatever the face's
///    node count.
/// 3. Apply traction `t = P_i · r̂` via `apply_traction_load` using `face_order`.
///
/// `face_order` selects the face quadrature and must match the face arity:
/// `FaceOrder::P1Tri` (3-node triangle), `FaceOrder::P2Tri` (6-node triangle)
/// or `FaceOrder::P1Quad` (4-node quad).  Returns nonzero `(dof, value)` pairs.
pub fn inner_pressure_loads_core<F: AsRef<[usize]>>(
    inner_faces: &[F],
    nodes: &[[f64; 3]],
    p_i: f64,
    face_order: FaceOrder,
) -> Vec<(usize, f64)> {
    let ndof = 3 * nodes.len();
    let mut f = vec![0.0_f64; ndof];
    for face in inner_faces {
        let face = face.as_ref();
        let face_phys: Vec<[f64; 3]> = face.iter().map(|&n| nodes[n]).collect();
        // Centroid radial unit direction from the first 3 corner nodes
        let cx = (face_phys[0][0] + face_phys[1][0] + face_phys[2][0]) / 3.0;
        let cy = (face_phys[0][1] + face_phys[1][1] + face_phys[2][1]) / 3.0;
        let r_c = (cx * cx + cy * cy).sqrt();
        let traction = [p_i * cx / r_c, p_i * cy / r_c, 0.0];
        apply_traction_load(&mut f, face_order, face, &face_phys, traction);
    }
    // Lower to (dof, value) pairs; drop exact zeros
    f.iter().enumerate()
        .filter(|(_, v)| v.abs() > 1e-15)
        .map(|(i, v)| (i, *v))
        .collect()
}

/// Assemble inner-pressure nodal loads for P1 inner-surface triangles.
///
/// Per-face centroid radial traction (`t = P_i · r̂`), assembled via
/// [`apply_traction_load`] with `FaceOrder::P1Tri` and lowered to
/// `(dof, value)` pairs for `solve_p1_pipeline`.
fn inner_pressure_loads(
    inner_faces: &[[usize; 3]],
    nodes: &[[f64; 3]],
    p_i: f64,
) -> Vec<(usize, f64)> {
    inner_pressure_loads_core(inner_faces, nodes, p_i, FaceOrder::P1Tri)
}

/// Build the annular P1 mesh, solve, recover nodal stress, and return the
/// maximum von Mises stress over inner-ring nodes (r ≈ a).
///
/// Shared so that every P1-tet cylinder comparison exercises the same code
/// path and none of them can diverge silently.
///
/// Returns `(nodes, sigma, max_vm_inner)`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn cylinder_p1_solve_and_recover(
    a: f64, b: f64, l: f64,
    nr: usize, ntheta: usize, nz: usize,
    p_i: f64, mat: &IsotropicElastic, tol_geom: f64,
) -> (Vec<[f64; 3]>, Vec<[[f64; 3]; 3]>, f64) {
    let (nodes, conns, inner_faces) = annular_p1_mesh(a, b, l, nr, ntheta, nz);
    let bcs = annular_symmetry_plane_strain_bcs(&nodes, l, tol_geom);
    let loads = inner_pressure_loads(&inner_faces, &nodes, p_i);
    let u = solve_p1_pipeline(&nodes, &conns, bcs, &loads, mat);
    let sigma = recover_nodal_p1(&nodes, &conns, &u, mat);
    let max_vm = max_inner_ring_von_mises(&nodes, &sigma, a, tol_geom);
    (nodes, sigma, max_vm)
}

/// Maximum nodal von Mises stress over the inner-ring nodes of a cylinder
/// mesh: the nodes whose xy-plane radius is within `tol_geom` of `a`.
///
/// `sigma[i]` is the recovered stress tensor at `nodes[i]`.
///
/// # Panics
///
/// Panics when no node lies on the ring, which is always a fixture error: a
/// wrong `a`, too tight a `tol_geom`, or a mesh with no node at r = a.
pub fn max_inner_ring_von_mises(
    nodes: &[[f64; 3]],
    sigma: &[[[f64; 3]; 3]],
    a: f64,
    tol_geom: f64,
) -> f64 {
    nodes.iter().enumerate()
        .filter(|(_, p)| ((p[0]*p[0]+p[1]*p[1]).sqrt() - a).abs() < tol_geom)
        .map(|(ni, _)| von_mises_of_tensor(&sigma[ni]))
        .reduce(f64::max)
        .unwrap_or_else(|| {
            panic!(
                "max_inner_ring_von_mises: no node within tol_geom={tol_geom} of r={a} (n_nodes={})",
                nodes.len()
            )
        })
}
