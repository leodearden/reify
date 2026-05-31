//! Degenerated (continuum-based) shell substrate: per-node directors and a
//! varying element Jacobian, carrying the MITC3+ assumed transverse-shear field.
//!
//! # References
//!
//! - Ahmad, S., Irons, B. M. & Zienkiewicz, O. C. (1970). "Analysis of thick
//!   and thin shell structures by curved finite elements." *Int. J. Numer.
//!   Methods Eng.*, 2(3), 419–451. — the original *degenerated solid* shell.
//! - Bathe, K.-J. (2014). *Finite Element Procedures*, 2nd ed., §5.4.2 — the
//!   continuum-based (degenerated) shell kinematics used here.
//! - Lee, Y., Lee, P.-S. & Bathe, K.-J. (2014). "The MITC3+ shell element and
//!   its performance." *Computers & Structures*, 138, 12–23. — the assumed
//!   transverse-shear field this substrate *carries* (task 3392 owns it).
//!
//! # Geometry map
//!
//! The element interpolates a mid-surface plus a per-node *director* fibre:
//!
//! ```text
//! X(ξ, η, ζ) = Σ_i N_i(ξ, η) · x_i  +  (ζ / 2) · Σ_i N_i(ξ, η) · t_i · V_i
//! ```
//!
//! where `N_i` are the three linear triangle shape functions
//! ([`crate::elements::mitc3_plus::Mitc3Plus::shape_at`]), `x_i` are the
//! mid-surface vertex positions, `t_i` the nodal thicknesses, `V_i` the
//! per-node **unit directors** (vertex normals), and `ζ ∈ [-1, 1]` the
//! through-thickness natural coordinate (`ζ = +1` top surface, `ζ = -1`
//! bottom).
//!
//! # Why a degenerate substrate (the varying-Jacobian deliverable)
//!
//! On a flat facet with all directors parallel to the facet normal, the 3×3
//! Jacobian `J = ∂X/∂(ξ,η,ζ)` is **invariant** in `ζ` and the element reduces
//! to the flat MITC3+ of task 3392. When the directors tilt (curved geometry),
//! the `(ζ/2) Σ ∇N_i t_i V_i` term makes `J` **vary** across the element —
//! that director-tilt-induced variation IS the varying Jacobian, and it
//! recovers the intra-element membrane–bending coupling a single flat facet
//! cannot represent.
//!
//! # Director provenance (cross-PRD seam G4)
//!
//! The element *consumes* explicit per-node directors (provenance-agnostic).
//! This module additionally ships a neighbour-averaged facet-normal fallback
//! for meshes without extraction-supplied vertex normals; curved benchmarks
//! supply analytic (e.g. radial) directors as the extraction stand-in. Actual
//! voxel-extraction wiring is deferred to integration (tasks 4065 / 4069).
//!
//! # Scope
//!
//! This module owns the *substrate*: directors, the geometry map, the varying
//! Jacobian, the membrane+bending strain–displacement operator, and the
//! covariant→physical re-expression of the carried MITC3+ shear field. The
//! transverse-shear *formulation* itself is task 3392's; ANS-membrane is task
//! 4065's. The element stiffness assembled from these pieces lives beside its
//! flat-facet sibling in [`crate::shell_assembly`].

use crate::elements::mitc3_plus::{Mitc3Plus, ShellReferenceCoord};
use crate::shell_assembly::build_shell_frame;

/// A 3D degenerate-shell reference coordinate `(ξ, η, ζ)`.
///
/// The in-plane pair `(ξ, η)` lives on the **unit reference triangle** with
/// vertices `(0,0)`, `(1,0)`, `(0,1)` — identical to
/// [`crate::elements::mitc3_plus::ShellReferenceCoord`] — so the linear
/// triangle shape functions apply unchanged. The through-thickness coordinate
/// `ζ ∈ [-1, 1]` runs from the bottom surface (`ζ = -1`) through the
/// mid-surface (`ζ = 0`) to the top surface (`ζ = +1`).
///
/// This is the 3D analogue of the 2D `ShellReferenceCoord`; the extra `ζ` is
/// what lets the degenerate element vary its Jacobian through the thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellRefCoord3 {
    pub xi: f64,
    pub eta: f64,
    pub zeta: f64,
}

impl ShellRefCoord3 {
    /// Construct a 3D degenerate-shell reference coordinate.
    pub const fn new(xi: f64, eta: f64, zeta: f64) -> Self {
        Self { xi, eta, zeta }
    }

    /// The in-plane `(ξ, η)` projection as a 2D [`ShellReferenceCoord`].
    pub const fn in_plane(&self) -> ShellReferenceCoord {
        ShellReferenceCoord::new(self.xi, self.eta)
    }
}

/// Physical position `X(ξ, η, ζ)` of the degenerate-shell geometry map
///
/// ```text
/// X = Σ_i N_i(ξ,η) · x_i  +  (ζ/2) · Σ_i N_i(ξ,η) · t_i · V_i
/// ```
///
/// where `N_i` are the three linear triangle shape functions
/// ([`Mitc3Plus::shape_at`]), `x_i = nodes[i]` the mid-surface vertices,
/// `t_i = thicknesses[i]` the nodal thicknesses, and `V_i = directors[i]` the
/// unit directors. At `ζ = 0` this is the pure mid-surface interpolation
/// `Σ N_i x_i`; at `ζ = ±1` it reaches the top/bottom fibre endpoints.
pub fn degenerate_position(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [f64; 3] {
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let half_zeta = 0.5 * coord.zeta;
    let mut x = [0.0_f64; 3];
    for i in 0..Mitc3Plus::N_NODES {
        let fibre = half_zeta * thicknesses[i];
        for k in 0..3 {
            x[k] += n[i] * (nodes[i][k] + fibre * directors[i][k]);
        }
    }
    x
}

/// Degenerate-shell Jacobian `J = ∂X/∂(ξ,η,ζ)` and its determinant at `coord`.
///
/// `matrix[row][col] = ∂X_row/∂ξ_col` (the `elements::Jacobian` convention),
/// with columns
///
/// ```text
/// J[:,0] = ∂X/∂ξ = Σ_i (∂N_i/∂ξ) · (x_i + (ζ/2) t_i V_i)
/// J[:,1] = ∂X/∂η = Σ_i (∂N_i/∂η) · (x_i + (ζ/2) t_i V_i)
/// J[:,2] = ∂X/∂ζ = Σ_i N_i · (t_i/2) V_i
/// ```
///
/// The `(ζ/2) Σ ∇N_i t_i V_i` contribution to the first two columns is the
/// **director-tilt term**: it vanishes when the directors are parallel (flat
/// facet, `Σ∇N_i = 0`) — making `J` invariant in `ζ` — and is non-zero once
/// the directors tilt, which is exactly how a curved patch acquires a Jacobian
/// that varies through the thickness. The determinant reuses
/// [`crate::elements::Jacobian::from_matrix`] for a shared cofactor convention.
pub fn degenerate_jacobian(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> ([[f64; 3]; 3], f64) {
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let dn = Mitc3Plus.shape_grad_at(coord.in_plane());
    let half_zeta = 0.5 * coord.zeta;

    let mut m = [[0.0_f64; 3]; 3];
    for i in 0..Mitc3Plus::N_NODES {
        let half_t = 0.5 * thicknesses[i];
        // Fibre point p_i = x_i + (ζ/2) t_i V_i feeds the in-plane columns.
        let mut p_i = [0.0_f64; 3];
        for k in 0..3 {
            p_i[k] = nodes[i][k] + half_zeta * thicknesses[i] * directors[i][k];
        }
        for row in 0..3 {
            // ∂X/∂ξ and ∂X/∂η from the fibre point.
            m[row][0] += dn[i][0] * p_i[row];
            m[row][1] += dn[i][1] * p_i[row];
            // ∂X/∂ζ = Σ N_i (t_i/2) V_i.
            m[row][2] += n[i] * half_t * directors[i][row];
        }
    }
    let det = crate::elements::Jacobian::from_matrix(m).det;
    (m, det)
}

/// Inverse of a 3×3 matrix via the adjugate (cofactor) method, returned with
/// its determinant.
///
/// Used to push reference gradients into physical gradients: a reference
/// gradient column `g_ref = [∂/∂ξ, ∂/∂η, ∂/∂ζ]ᵀ` maps to the physical gradient
/// `g_phys = Jᵀ⁻¹ · g_ref` (i.e. `(J⁻¹)ᵀ · g_ref`). The determinant is returned
/// alongside so callers can guard against a singular Jacobian without a second
/// pass. Shares the cofactor convention of
/// [`crate::elements::Jacobian::from_matrix`].
pub fn mat3_inverse(m: &[[f64; 3]; 3]) -> ([[f64; 3]; 3], f64) {
    // Cofactors C[i][j] = (−1)^(i+j) · minor(i,j).
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = -(m[1][0] * m[2][2] - m[1][2] * m[2][0]);
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let c10 = -(m[0][1] * m[2][2] - m[0][2] * m[2][1]);
    let c11 = m[0][0] * m[2][2] - m[0][2] * m[2][0];
    let c12 = -(m[0][0] * m[2][1] - m[0][1] * m[2][0]);
    let c20 = m[0][1] * m[1][2] - m[0][2] * m[1][1];
    let c21 = -(m[0][0] * m[1][2] - m[0][2] * m[1][0]);
    let c22 = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    let inv_det = 1.0 / det;
    // inverse = adj/det = (cofactorᵀ)/det → inv[i][j] = C[j][i]/det.
    let inv = [
        [c00 * inv_det, c10 * inv_det, c20 * inv_det],
        [c01 * inv_det, c11 * inv_det, c21 * inv_det],
        [c02 * inv_det, c12 * inv_det, c22 * inv_det],
    ];
    (inv, det)
}

/// Push a reference gradient `g_ref = [∂/∂ξ, ∂/∂η, ∂/∂ζ]ᵀ` into the physical
/// gradient `Jᵀ⁻¹ · g_ref`, given `j_inv = J⁻¹`. `(Jᵀ⁻¹)[i][k] = J⁻¹[k][i]`.
#[inline]
fn jinv_t_mul(j_inv: &[[f64; 3]; 3], g_ref: &[f64; 3]) -> [f64; 3] {
    let mut g = [0.0_f64; 3];
    for i in 0..3 {
        for k in 0..3 {
            g[i] += j_inv[k][i] * g_ref[k];
        }
    }
    g
}

/// Per-point local **lamina frame** (rows `e1, e2, e3`) for the degenerate
/// element, built from the interpolated director and the Jacobian's first
/// in-plane column.
///
/// - `e3 = normalize(Σ N_i V_i)` — the interpolated director (lamina normal).
/// - `e1 = normalize(g1 − (g1·e3) e3)` — `g1 = ∂X/∂ξ` (J column 0), projected
///   into the plane ⊥ e3.
/// - `e2 = e3 × e1`.
///
/// On a flat facet (directors ∥ facet normal, `g1` in-plane) this reduces
/// exactly to [`crate::shell_assembly::build_shell_frame`], so `plane_stress_d`
/// — expressed in this lamina frame — applies and the flat reduction is exact.
fn lamina_frame(j: &[[f64; 3]; 3], n: &[f64; 3], directors: &[Director; 3]) -> [[f64; 3]; 3] {
    // e3 = normalized interpolated director.
    let mut e3 = [0.0_f64; 3];
    for i in 0..Mitc3Plus::N_NODES {
        for k in 0..3 {
            e3[k] += n[i] * directors[i][k];
        }
    }
    let l3 = (e3[0] * e3[0] + e3[1] * e3[1] + e3[2] * e3[2]).sqrt();
    for c in e3.iter_mut() {
        *c /= l3;
    }
    // g1 = J column 0 = ∂X/∂ξ.
    let g1 = [j[0][0], j[1][0], j[2][0]];
    let dot = g1[0] * e3[0] + g1[1] * e3[1] + g1[2] * e3[2];
    let mut e1 = [g1[0] - dot * e3[0], g1[1] - dot * e3[1], g1[2] - dot * e3[2]];
    let l1 = (e1[0] * e1[0] + e1[1] * e1[1] + e1[2] * e1[2]).sqrt();
    for c in e1.iter_mut() {
        *c /= l1;
    }
    // e2 = e3 × e1.
    let e2 = [
        e3[1] * e1[2] - e3[2] * e1[1],
        e3[2] * e1[0] - e3[0] * e1[2],
        e3[0] * e1[1] - e3[1] * e1[0],
    ];
    [e1, e2, e3]
}

/// In-plane lamina Voigt strain `[ε'₁₁, ε'₂₂, 2ε'₁₂]` from a physical velocity
/// gradient `h = ∂u/∂x` and the lamina frame `q` (rows `e1, e2, e3`).
///
/// Symmetrises `h` into the small strain `ε = ½(h + hᵀ)`, rotates into the
/// lamina frame (`ε'_pq = e_pᵀ ε e_q`), and returns the two in-plane normal
/// components plus the engineering in-plane shear.
fn inplane_lamina_strain(h: &[[f64; 3]; 3], q: &[[f64; 3]; 3]) -> [f64; 3] {
    let mut eps = [[0.0_f64; 3]; 3];
    for a in 0..3 {
        for b in 0..3 {
            eps[a][b] = 0.5 * (h[a][b] + h[b][a]);
        }
    }
    let strain_pq = |p: usize, qq: usize| -> f64 {
        let mut s = 0.0;
        for a in 0..3 {
            for b in 0..3 {
                s += q[p][a] * eps[a][b] * q[qq][b];
            }
        }
        s
    };
    [strain_pq(0, 0), strain_pq(1, 1), 2.0 * strain_pq(0, 1)]
}

/// Membrane+bending strain–displacement matrix `B` (3 in-plane lamina strain
/// rows `[ε'₁₁, ε'₂₂, 2ε'₁₂]` × 18 DOF columns) at `coord`.
///
/// # Kinematics
///
/// The degenerate displacement field is
///
/// ```text
/// u(ξ,η,ζ) = Σ_i N_i u_i  +  (ζ t_i/2) Σ_i N_i (θ_i × V_i)
/// ```
///
/// so the physical velocity gradient `H = ∂u/∂x` has, per node `i`:
/// - a translation part `H[a][j] += (∇ₓN_i)[j] · u_{i,a}` with
///   `∇ₓN_i = Jᵀ⁻¹ · [∂N_i/∂ξ, ∂N_i/∂η, 0]ᵀ`;
/// - a rotation part `H[a][j] += (∇ₓφ_i)[j] · (θ_i × V_i)_a` with
///   `φ_i = N_i (ζ t_i/2)`, `∇ₓφ_i = Jᵀ⁻¹ · [∂N_i/∂ξ·(ζt_i/2), ∂N_i/∂η·(ζt_i/2),
///   N_i·(t_i/2)]ᵀ`, and `(θ_i × V_i) = −skew(V_i)·θ_i`.
///
/// `H` is symmetrised and projected into the per-point lamina frame
/// ([`lamina_frame`]) so the plane-stress constitutive law applies. DOF
/// ordering is identical to [`crate::elements::mitc3_plus::Mitc3Plus`]:
/// `6·node + {u_x,u_y,u_z,θ_x,θ_y,θ_z}`.
pub fn degenerate_membrane_bending_b(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 18]; 3] {
    let (j, _det) = degenerate_jacobian(nodes, directors, thicknesses, coord);
    let (j_inv, _) = mat3_inverse(&j);
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let dn = Mitc3Plus.shape_grad_at(coord.in_plane());
    let q = lamina_frame(&j, &n, directors);

    let half_zeta = 0.5 * coord.zeta;
    let mut b = [[0.0_f64; 18]; 3];
    for i in 0..Mitc3Plus::N_NODES {
        let half_t = 0.5 * thicknesses[i];
        let zt = half_zeta * thicknesses[i];
        // Physical gradient of N_i (ζ-independent) and of φ_i = N_i·(ζ t_i/2).
        let g_n = jinv_t_mul(&j_inv, &[dn[i][0], dn[i][1], 0.0]);
        let g_phi = jinv_t_mul(&j_inv, &[dn[i][0] * zt, dn[i][1] * zt, n[i] * half_t]);
        // C_i = −skew(V_i): (θ × V) = C_i · θ.
        let v = directors[i];
        let c_i = [
            [0.0, v[2], -v[1]],
            [-v[2], 0.0, v[0]],
            [v[1], -v[0], 0.0],
        ];

        // Translation DOFs (a = 0,1,2): H has row `a` equal to g_n.
        for a in 0..3 {
            let mut h = [[0.0_f64; 3]; 3];
            h[a] = g_n;
            let e = inplane_lamina_strain(&h, &q);
            let col = 6 * i + a;
            for r in 0..3 {
                b[r][col] = e[r];
            }
        }
        // Rotation DOFs (cc = 0,1,2 → θ_x,θ_y,θ_z): H[a][j] = C_i[a][cc]·g_phi[j].
        for cc in 0..3 {
            let mut h = [[0.0_f64; 3]; 3];
            for a in 0..3 {
                for jj in 0..3 {
                    h[a][jj] = c_i[a][cc] * g_phi[jj];
                }
            }
            let e = inplane_lamina_strain(&h, &q);
            let col = 6 * i + 3 + cc;
            for r in 0..3 {
                b[r][col] = e[r];
            }
        }
    }
    b
}

/// A per-node shell **director**: the unit vector along the through-thickness
/// fibre at a mesh vertex (the `V_i` of the degenerate-shell geometry map
/// `X = Σ N_i x_i + (ζ/2) Σ N_i t_i V_i`).
///
/// Represented as a bare `[f64; 3]` so it interoperates directly with node
/// positions and the cross-product helpers. The element treats it as a unit
/// vector; callers supplying explicit (extraction- or analytically-derived)
/// directors own the unit-norm invariant. The [`directors_from_facets`]
/// fallback always returns unit-norm directors.
pub type Director = [f64; 3];

/// Neighbour-averaged facet-normal **director fallback** for meshes without
/// extraction-supplied per-vertex normals.
///
/// For each triangle in `connectivity` the unit facet normal is taken from
/// [`build_shell_frame`] (cross-product convention `n = (p1−p0) × (p2−p0)`,
/// normalized — `e3` of the local frame), so the sign convention is shared
/// with the rest of the shell pipeline. Each facet normal is accumulated into
/// its three vertices and every vertex director is the normalized sum of the
/// unit normals of its incident facets. Returns one unit-norm [`Director`] per
/// entry of `nodes`, in node order.
///
/// This is the *default* director source when nothing better is available; the
/// element consumes explicit directors (provenance-agnostic), and curved
/// benchmarks instead supply analytic vertex normals (the extraction
/// stand-in). A node with no incident facet, or whose incident normals exactly
/// cancel, falls back to `+z` so the result is always unit-norm.
pub fn directors_from_facets(nodes: &[[f64; 3]], connectivity: &[[usize; 3]]) -> Vec<Director> {
    let mut acc = vec![[0.0_f64; 3]; nodes.len()];
    for conn in connectivity {
        let tri = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
        // e3 of the local frame = unit facet normal, sign-consistent with the
        // membrane/bending/shear pipeline.
        let n = build_shell_frame(&tri).r[2];
        for &v in conn.iter() {
            acc[v][0] += n[0];
            acc[v][1] += n[1];
            acc[v][2] += n[2];
        }
    }
    for d in acc.iter_mut() {
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len > 1e-30 {
            d[0] /= len;
            d[1] /= len;
            d[2] /= len;
        } else {
            *d = [0.0, 0.0, 1.0];
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Unit-normal of triangle `[p0, p1, p2]` via the `build_shell_frame`
    /// cross-product convention `n = (p1−p0) × (p2−p0)`, normalized. Used by
    /// the tests to independently reproduce expected facet normals.
    fn facet_unit_normal(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3]) -> [f64; 3] {
        let d01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let d02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            d01[1] * d02[2] - d01[2] * d02[1],
            d01[2] * d02[0] - d01[0] * d02[2],
            d01[0] * d02[1] - d01[1] * d02[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        [n[0] / len, n[1] / len, n[2] / len]
    }

    fn norm(v: [f64; 3]) -> f64 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    /// (i) Single flat triangle: every node director equals the unit facet
    /// normal. A triangle in the xy-plane has facet normal (0,0,1).
    #[test]
    fn directors_from_facets_single_flat_triangle_all_equal_facet_normal() {
        let nodes = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let connectivity = vec![[0_usize, 1, 2]];
        let dirs: Vec<Director> = directors_from_facets(&nodes, &connectivity);

        assert_eq!(dirs.len(), 3, "one director per node");
        let n = facet_unit_normal(nodes[0], nodes[1], nodes[2]);
        assert!(
            (n[0]).abs() < TOL && (n[1]).abs() < TOL && (n[2] - 1.0).abs() < TOL,
            "facet normal must be +z, got {n:?}",
        );
        for (i, d) in dirs.iter().enumerate() {
            for k in 0..3 {
                assert!(
                    (d[k] - n[k]).abs() < TOL,
                    "director[{i}][{k}] = {}, expected facet normal {}",
                    d[k],
                    n[k],
                );
            }
        }
    }

    /// (ii) Two facets meeting at a shared edge (symmetric tent, 90° fold): the
    /// shared-vertex director is the normalized sum of the two facet normals.
    ///
    /// Layout (ridge along y from node 0 to node 1):
    ///   node 0 = (0,0,0), node 1 = (0,2,0)  — shared ridge
    ///   node 2 = (−1,1,1) — apex of facet A = [0,1,2], unit normal (1,0,1)/√2
    ///   node 3 = (1,1,1)  — apex of facet B = [1,0,3], unit normal (−1,0,1)/√2
    /// Sum of unit normals = (0,0,√2) → normalized (0,0,1) at the shared nodes.
    #[test]
    fn directors_from_facets_shared_edge_is_normalized_sum_of_facet_normals() {
        let nodes = vec![
            [0.0, 0.0, 0.0],  // 0 shared
            [0.0, 2.0, 0.0],  // 1 shared
            [-1.0, 1.0, 1.0], // 2 facet-A apex
            [1.0, 1.0, 1.0],  // 3 facet-B apex
        ];
        // Reverse the shared edge on facet B so both normals point +z (outward).
        let connectivity = vec![[0_usize, 1, 2], [1_usize, 0, 3]];
        let dirs: Vec<Director> = directors_from_facets(&nodes, &connectivity);
        assert_eq!(dirs.len(), 4);

        let n_a = facet_unit_normal(nodes[0], nodes[1], nodes[2]);
        let n_b = facet_unit_normal(nodes[1], nodes[0], nodes[3]);
        // Sanity: the hand-computed unit normals.
        let inv_sqrt2 = 1.0 / 2.0_f64.sqrt();
        assert!((n_a[0] - inv_sqrt2).abs() < TOL && n_a[1].abs() < TOL && (n_a[2] - inv_sqrt2).abs() < TOL);
        assert!((n_b[0] + inv_sqrt2).abs() < TOL && n_b[1].abs() < TOL && (n_b[2] - inv_sqrt2).abs() < TOL);

        // Shared nodes 0 and 1: normalized(n_a + n_b) = (0,0,1).
        for &shared in &[0_usize, 1] {
            let d = dirs[shared];
            assert!(
                d[0].abs() < TOL && d[1].abs() < TOL && (d[2] - 1.0).abs() < TOL,
                "shared director[{shared}] = {d:?}, expected (0,0,1)",
            );
        }
        // Non-shared nodes keep their single facet normal.
        for k in 0..3 {
            assert!((dirs[2][k] - n_a[k]).abs() < TOL, "node 2 dir mismatch");
            assert!((dirs[3][k] - n_b[k]).abs() < TOL, "node 3 dir mismatch");
        }
    }

    /// (iii) Every director is unit-norm, including at shared vertices where
    /// several facet normals are accumulated.
    #[test]
    fn directors_from_facets_are_always_unit_norm() {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [-1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let connectivity = vec![[0_usize, 1, 2], [1_usize, 0, 3]];
        let dirs = directors_from_facets(&nodes, &connectivity);
        for (i, d) in dirs.iter().enumerate() {
            assert!(
                (norm(*d) - 1.0).abs() < TOL,
                "director[{i}] = {d:?} has norm {}, expected 1.0",
                norm(*d),
            );
        }
    }

    // ── step-3: degenerate-shell geometry map ───────────────────────────────

    /// Tilted-director fixture with clean closed-form fibre offsets.
    ///
    /// Mid-surface nodes (0,0,0),(2,0,0),(0,2,0); directors V_0=+z,
    /// V_1=(1,0,1)/√2, V_2=(0,1,1)/√2; thicknesses chosen so `(t_i/2)·V_i` is a
    /// clean vector: t_0=0.5 → (0,0,0.25); t_1=t_2=2√2 → (1,0,1) and (0,1,1).
    fn tilted_fixture() -> ([[f64; 3]; 3], [Director; 3], [f64; 3]) {
        let inv_sqrt2 = 1.0 / 2.0_f64.sqrt();
        let nodes = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        let directors = [
            [0.0, 0.0, 1.0],
            [inv_sqrt2, 0.0, inv_sqrt2],
            [0.0, inv_sqrt2, inv_sqrt2],
        ];
        let thicknesses = [0.5, 2.0 * 2.0_f64.sqrt(), 2.0 * 2.0_f64.sqrt()];
        (nodes, directors, thicknesses)
    }

    fn assert_pt(got: [f64; 3], want: [f64; 3], ctx: &str) {
        for k in 0..3 {
            assert!(
                (got[k] - want[k]).abs() < 1e-9,
                "{ctx}: component {k} = {}, expected {}",
                got[k],
                want[k],
            );
        }
    }

    /// At ζ=0 the geometry map returns the pure mid-surface interpolation
    /// Σ N_i x_i — independent of the directors — equalling each vertex at the
    /// reference vertices and the node centroid at the reference centroid.
    #[test]
    fn degenerate_position_at_zeta_zero_is_midsurface() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        // Reference vertices map to the physical vertices.
        let ref_vtx = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)];
        for (i, &(xi, eta)) in ref_vtx.iter().enumerate() {
            let x = degenerate_position(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            assert_pt(x, nodes[i], &format!("vertex {i} @ ζ=0"));
        }
        // Reference centroid → node centroid (2/3, 2/3, 0).
        let xc = degenerate_position(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
        );
        assert_pt(xc, [2.0 / 3.0, 2.0 / 3.0, 0.0], "centroid @ ζ=0");
    }

    /// At a reference vertex with ζ=±1 the map returns the top/bottom fibre
    /// endpoints x_i ± (t_i/2) V_i. Covers both the flat (+z) director at node 0
    /// and the tilted directors at nodes 1, 2.
    #[test]
    fn degenerate_position_at_zeta_pm1_is_fibre_endpoint() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        // (node index, (xi, eta), top X, bottom X)
        let cases = [
            (0_usize, (0.0, 0.0), [0.0, 0.0, 0.25], [0.0, 0.0, -0.25]),
            (1_usize, (1.0, 0.0), [3.0, 0.0, 1.0], [1.0, 0.0, -1.0]),
            (2_usize, (0.0, 1.0), [0.0, 3.0, 1.0], [0.0, 1.0, -1.0]),
        ];
        for (i, (xi, eta), top, bottom) in cases {
            let xt = degenerate_position(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 1.0),
            );
            assert_pt(xt, top, &format!("node {i} top (ζ=+1)"));
            let xb = degenerate_position(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, -1.0),
            );
            assert_pt(xb, bottom, &format!("node {i} bottom (ζ=−1)"));
        }
    }

    /// Interior point with ζ≠0 exercises the full formula
    /// X = Σ N_i x_i + (ζ/2) Σ N_i t_i V_i. At the centroid with ζ=+1 the
    /// hand-computed result is (1, 1, 0.75) (see plan step-3 arithmetic).
    #[test]
    fn degenerate_position_interior_point_matches_full_formula() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        let x = degenerate_position(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 1.0),
        );
        assert_pt(x, [1.0, 1.0, 0.75], "centroid @ ζ=+1");
    }

    /// Flat case: planar nodes, all directors +z, uniform thickness. The fibre
    /// offset is purely ±(t/2) in z, so top/bottom surfaces are the mid-surface
    /// shifted in z and nothing tilts.
    #[test]
    fn degenerate_position_flat_case_is_pure_z_offset() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let directors = [[0.0, 0.0, 1.0]; 3];
        let t = 0.2;
        let thicknesses = [t; 3];
        let probe = (0.25, 0.35);
        let mid = degenerate_position(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(probe.0, probe.1, 0.0),
        );
        let top = degenerate_position(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(probe.0, probe.1, 1.0),
        );
        // Mid-surface is in-plane (z=0); top is shifted +t/2 in z only.
        assert!((mid[2]).abs() < 1e-12, "flat mid z must be 0");
        assert_pt(top, [mid[0], mid[1], mid[2] + t / 2.0], "flat top");
    }

    // ── step-5: varying Jacobian (headline property) ────────────────────────

    /// Max absolute entrywise difference between two 3×3 matrices.
    fn mat3_max_abs_diff(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> f64 {
        let mut m = 0.0_f64;
        for i in 0..3 {
            for j in 0..3 {
                m = m.max((a[i][j] - b[i][j]).abs());
            }
        }
        m
    }

    /// FLAT case: planar triangle, directors ∥ facet normal, uniform thickness.
    /// The director-tilt term Σ ∇N_i V_i vanishes (Σ∇N_i = 0, V_i constant), so
    /// J is INVARIANT in ζ and det(J) = (2A)·(t/2) = A·t.
    #[test]
    fn degenerate_jacobian_flat_case_is_zeta_invariant_with_closed_form_det() {
        // WIDE_TRI: area A = 0.5·|(2,0,0)×(0,3,0)| = 3.
        let nodes = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];
        let directors = [[0.0, 0.0, 1.0]; 3];
        let t = 0.2;
        let thicknesses = [t; 3];
        let area = 3.0_f64;

        let (j_minus, det_minus) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, -0.5),
        );
        let (j_plus, det_plus) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 0.5),
        );

        // (i) ζ-invariance of the whole matrix.
        assert!(
            mat3_max_abs_diff(&j_minus, &j_plus) < 1e-12,
            "flat J must be invariant in ζ; max diff = {}",
            mat3_max_abs_diff(&j_minus, &j_plus),
        );
        // det = A·t (closed form), at both ζ samples.
        let want_det = area * t;
        assert!((det_minus - want_det).abs() < 1e-12, "det@-0.5 = {det_minus}, want {want_det}");
        assert!((det_plus - want_det).abs() < 1e-12, "det@+0.5 = {det_plus}, want {want_det}");
        // The closed-form J itself: columns (2,0,0), (0,3,0), (0,0,t/2).
        let want_j = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, t / 2.0]];
        assert!(mat3_max_abs_diff(&j_plus, &want_j) < 1e-12, "flat J = {j_plus:?}");
    }

    /// CURVED case: non-parallel (tilted) directors make the director-tilt term
    /// non-zero, so J genuinely VARIES with ζ AND in (ξ,η).
    #[test]
    fn degenerate_jacobian_curved_case_varies_in_zeta_and_in_plane() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        // Non-parallel directors (radial-like tilt).
        let c30 = 30.0_f64.to_radians().cos();
        let s30 = 30.0_f64.to_radians().sin();
        let directors = [[0.0, 0.0, 1.0], [s30, 0.0, c30], [0.0, s30, c30]];
        let thicknesses = [0.3; 3];

        // (ii.a) varies in ζ at a fixed in-plane point.
        let (j_m, _) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, -0.5),
        );
        let (j_p, _) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 0.5),
        );
        assert!(
            mat3_max_abs_diff(&j_m, &j_p) > 1e-6,
            "curved J must vary in ζ; max diff = {}",
            mat3_max_abs_diff(&j_m, &j_p),
        );

        // (ii.b) varies in (ξ,η) at a fixed ζ (via the dX/dζ = Σ N_i (t/2) V_i
        // column, which depends on the shape-function values).
        let (j_a, _) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(0.2, 0.2, 0.5),
        );
        let (j_b, _) = degenerate_jacobian(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(0.5, 0.3, 0.5),
        );
        assert!(
            mat3_max_abs_diff(&j_a, &j_b) > 1e-6,
            "curved J must vary in (ξ,η); max diff = {}",
            mat3_max_abs_diff(&j_a, &j_b),
        );
    }

    // ── step-7: membrane/bending strain-displacement B ──────────────────────

    /// Contract a 3×18 B-matrix with an 18-DOF vector → 3-component strain.
    fn b_times_u(b: &[[f64; 18]; 3], u: &[f64; 18]) -> [f64; 3] {
        let mut e = [0.0_f64; 3];
        for (r, row) in b.iter().enumerate() {
            for (c, &bc) in row.iter().enumerate() {
                e[r] += bc * u[c];
            }
        }
        e
    }

    /// Flat patch in the xy-plane: directors +z, uniform thickness. The lamina
    /// frame is global xy, so the in-plane strain components are global.
    fn flat_patch() -> ([[f64; 3]; 3], [Director; 3], [f64; 3]) {
        let nodes = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.5, 0.0]];
        let directors = [[0.0, 0.0, 1.0]; 3];
        let thicknesses = [0.25; 3];
        (nodes, directors, thicknesses)
    }

    /// (i) A rigid-body translation yields ZERO membrane/bending strain at every
    /// integration point (Σ ∇N_i = 0).
    #[test]
    fn degenerate_b_rigid_translation_yields_zero_strain() {
        let (nodes, directors, thicknesses) = flat_patch();
        // Uniform translation (0.7, −0.3, 0.4) at all nodes, rotations zero.
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i] = 0.7;
            u[6 * i + 1] = -0.3;
            u[6 * i + 2] = 0.4;
        }
        for &zeta in &[-0.6, 0.0, 0.6] {
            let b = degenerate_membrane_bending_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(0.3, 0.3, zeta),
            );
            let e = b_times_u(&b, &u);
            for (r, &er) in e.iter().enumerate() {
                assert!(er.abs() < 1e-12, "rigid translation strain[{r}] = {er} @ ζ={zeta}");
            }
        }
    }

    /// (ii) A constant in-plane stretch field u_x = a·x, u_y = b·y yields the
    /// constant membrane strain [a, b, 0] (lamina = global xy on the flat patch).
    #[test]
    fn degenerate_b_constant_stretch_yields_constant_membrane_strain() {
        let (nodes, directors, thicknesses) = flat_patch();
        let a = 0.01_f64;
        let b = -0.004_f64;
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i] = a * nodes[i][0]; // u_x = a·x
            u[6 * i + 1] = b * nodes[i][1]; // u_y = b·y
        }
        for &zeta in &[-0.6, 0.0, 0.6] {
            let bm = degenerate_membrane_bending_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(0.25, 0.4, zeta),
            );
            let e = b_times_u(&bm, &u);
            assert!((e[0] - a).abs() < 1e-12, "ε_xx = {} expected {a} @ ζ={zeta}", e[0]);
            assert!((e[1] - b).abs() < 1e-12, "ε_yy = {} expected {b} @ ζ={zeta}", e[1]);
            assert!(e[2].abs() < 1e-12, "γ_xy = {} expected 0 @ ζ={zeta}", e[2]);
        }
    }

    /// (iii) Column sparsity matches the 18-DOF (6-per-node) layout: the matrix
    /// is 3×18; the drilling rotation about the director (col 6i+5 on a +z-flat
    /// patch) produces no in-plane strain at any ζ; and at the mid-surface
    /// (ζ=0) the out-of-plane translation column (6i+2) produces no membrane
    /// strain.
    #[test]
    fn degenerate_b_column_sparsity_matches_layout() {
        let (nodes, directors, thicknesses) = flat_patch();

        // Drilling (θ_z) column zero at several ζ.
        for &zeta in &[-0.7, 0.0, 0.7] {
            let b = degenerate_membrane_bending_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(0.3, 0.3, zeta),
            );
            assert_eq!(b.len(), 3, "B must have 3 strain rows");
            assert_eq!(b[0].len(), 18, "B must have 18 DOF columns");
            for i in 0..3 {
                let drill = 6 * i + 5;
                for r in 0..3 {
                    assert!(
                        b[r][drill].abs() < 1e-12,
                        "drilling col {drill} row {r} = {} must be 0 @ ζ={zeta}",
                        b[r][drill],
                    );
                }
            }
        }

        // Out-of-plane translation (u_z) column zero at the mid-surface.
        let b0 = degenerate_membrane_bending_b(
            &nodes,
            &directors,
            &thicknesses,
            ShellRefCoord3::new(0.3, 0.3, 0.0),
        );
        for i in 0..3 {
            let uz = 6 * i + 2;
            for r in 0..3 {
                assert!(
                    b0[r][uz].abs() < 1e-12,
                    "u_z col {uz} row {r} = {} must be 0 at ζ=0",
                    b0[r][uz],
                );
            }
        }
    }
}
