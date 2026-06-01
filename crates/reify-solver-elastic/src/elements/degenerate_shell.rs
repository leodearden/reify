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
//!
//! # Transverse shear: exact kinematics and the in-plane-metric improvement
//!
//! The carried MITC3+ assumed-strain *locking treatment* (Lee–Lee–Bathe Eq. 9
//! interior tying, task 3392's deliverable) is reused **verbatim**; only the
//! per-tying-point covariant *samples* are re-derived from the true degenerate
//! kinematics ([`degenerate_exact_covariant_shear_b`]) — the exact covariant
//! shear `γ_αζ = g_α·u_,ζ + g_ζ·u_,α` rather than task 3392's flat global-DOF
//! field. This is the only form that is simultaneously frame-objective (rotates
//! as a tensor under rigid 3D rotation) and rigid-body-compatible — see
//! esc-4068-127.
//!
//! A consequence (esc-4068-134): the exact covariant shear carries the in-plane
//! metric on the rotation term — its `γ_αζ[θ]` coefficient is `|g_α|·h_i`, the
//! genuine standard-MITC3 value — whereas task 3392's
//! [`Mitc3Plus::covariant_shear_b_nodal`] writes the bare `h_i` with no metric.
//! On a UNIT-metric orthogonal flat triangle (`|g_α| = 1`) the two coincide
//! bit-for-bit, so this element reduces exactly to flat MITC3+ there (the
//! flat-reduction anchors). On a general (non-unit / skew) flat triangle the
//! degenerate element computes the physically-correct standard-MITC3 transverse
//! shear, which differs from 3392's simplified flat-facet value by `|g_α|` — a
//! strict **improvement** (part of the step-21 curved-benchmark gain), pinned
//! positively by the non-unit constant-shear patch test, NOT a regression.

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
// G-allow: degenerate-shell (MITC3+) position interpolation, tasks #4068/#4069; reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests.
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
// G-allow: degenerate-shell (MITC3+) Jacobian-inverse helper, tasks #4068/#4069; reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests.
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
    // Degeneracy guard: a valid degenerate-shell element Jacobian has
    // |det| ~ area·(t/2) (O(1e-2…1) for physical meshes). A determinant at/below
    // this floor signals a collapsed or inverted element, which would otherwise
    // yield a silent inf/NaN inverse that propagates into B and the stiffness.
    // debug-only (no release-build cost), matching the project convention that
    // contract violations surface a diagnostic.
    debug_assert!(
        det.is_finite() && det.abs() > 1e-12,
        "mat3_inverse: near-singular Jacobian (det = {det}); a degenerate or inverted \
         shell element corrupts the stiffness — check mesh geometry / directors",
    );
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
    // The interpolated director is a convex combination of unit directors, so
    // |e3| ∈ (0, 1]; it collapses to ~0 only if incident directors nearly cancel
    // (an over-curved/degenerate element), which would leave the lamina normal
    // undefined and divide by ~0 below.
    debug_assert!(
        l3 > 1e-12,
        "lamina_frame: interpolated director magnitude {l3} ≈ 0 — incident directors \
         nearly cancel (over-curved/degenerate element); lamina normal is undefined",
    );
    for c in e3.iter_mut() {
        *c /= l3;
    }
    // g1 = J column 0 = ∂X/∂ξ.
    let g1 = [j[0][0], j[1][0], j[2][0]];
    let dot = g1[0] * e3[0] + g1[1] * e3[1] + g1[2] * e3[2];
    let mut e1 = [g1[0] - dot * e3[0], g1[1] - dot * e3[1], g1[2] - dot * e3[2]];
    let l1 = (e1[0] * e1[0] + e1[1] * e1[1] + e1[2] * e1[2]).sqrt();
    // |e1| ≈ 0 only if J column 0 (g1 = ∂X/∂ξ) is parallel to the director e3 —
    // a degenerate element whose in-plane tangent has collapsed onto the normal;
    // the in-plane lamina axis would be undefined and divide by ~0 below.
    debug_assert!(
        l1 > 1e-12,
        "lamina_frame: in-plane axis magnitude {l1} ≈ 0 — J column 0 is parallel to the \
         director (degenerate element); lamina in-plane axis is undefined",
    );
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

/// Dot product of two 3-vectors. Shared by the exact-covariant strain builders
/// ([`degenerate_exact_covariant_membrane_b`], [`degenerate_exact_covariant_shear_b`]).
#[inline]
fn dot3(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Columns of `C_i = −skew(V_i)` for a per-node director `v`, i.e.
/// `C_i[:, c] = ∂(θ_i × V_i)/∂θ_c`. Returned as `[col0, col1, col2]`.
///
/// Single source of truth for the skew-symmetric rotation→displacement convention
/// shared by the exact-covariant membrane ([`degenerate_exact_covariant_membrane_b`])
/// and transverse-shear ([`degenerate_exact_covariant_shear_b`]) B builders.
#[inline]
fn director_rotation_columns(v: &Director) -> [[f64; 3]; 3] {
    [
        [0.0, -v[2], v[1]], // C_i[:,0] = (0, −V_z, V_y)
        [v[2], 0.0, -v[0]], // C_i[:,1] = (V_z, 0, −V_x)
        [-v[1], v[0], 0.0], // C_i[:,2] = (−V_y, V_x, 0)
    ]
}

/// In-plane contravariant **lamina projection** at `coord`: the `2×2` block
/// `m2 = (q·J⁻ᵀ)[0..2, 0..2]` and the through-thickness factor
/// `s3 = (q·J⁻ᵀ)[2][2] = e'_3·g^ζ = 1/|g_ζ|`, with the lamina frame `q`
/// ([`lamina_frame`]) and the degenerate Jacobian `J` ([`degenerate_jacobian`]) at
/// `coord`. `m2` is the in-plane contravariant projection `e'_p·g^i`
/// (`p, i ∈ {1, 2}`) that maps a covariant in-plane tensor into the physical lamina
/// frame; `s3` strips the `g_ζ` metric the exact covariant shear carries.
///
/// Single source of truth for the covariant→physical map shared by the ANS membrane
/// ([`degenerate_assumed_membrane_b`], which uses `m2`) and the ANS transverse-shear
/// ([`degenerate_transverse_shear_b`], which uses both `m2` and `s3`) fields.
fn lamina_contravariant_projection(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> ([[f64; 2]; 2], f64) {
    let (j, _det) = degenerate_jacobian(nodes, directors, thicknesses, coord);
    let (j_inv, _) = mat3_inverse(&j);
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let q = lamina_frame(&j, &n, directors);
    // (q · J⁻ᵀ)[a][b] = Σ_k q[a][k] · J⁻ᵀ[k][b] = Σ_k q[a][k] · J⁻¹[b][k].
    let qjit = |a: usize, b: usize| -> f64 { (0..3).map(|k| q[a][k] * j_inv[b][k]).sum() };
    let m2 = [[qjit(0, 0), qjit(0, 1)], [qjit(1, 0), qjit(1, 1)]];
    let s3 = qjit(2, 2); // through-thickness normalization e'_3·g^ζ = 1/|g_ζ|
    (m2, s3)
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
        // `cc` is the COLUMN index of `c_i` (and drives `col`); enumerate-over-rows
        // would read `c_i[cc]` (a row) and transpose the kinematics.
        #[allow(clippy::needless_range_loop)]
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

/// Rotation-bubble membrane+bending enrichment columns (`3 × 2`) at `coord`.
///
/// Returns the 2 enrichment columns `[Δβ_x, Δβ_y]` of the cubic rotation bubble
/// `f_b = 27·ξ·η·(1−ξ−η)` (the "+" in MITC3+, Lee–Lee–Bathe 2014 Eq. 7) for the
/// degenerate-substrate **bending** strain `[ε'₁₁, ε'₂₂, 2ε'₁₂]`.
///
/// # Kinematics
///
/// Mirrors the **rotation block** of [`degenerate_membrane_bending_b`] with
/// `f_b`/`∇f_b` substituted for `N_i`/`∇N_i` and the interpolated lamina normal
/// `e₃ = Σ N_i V_i` (normalized; row 2 of [`lamina_frame`]) as the representative
/// director `V_repr`:
///
/// ```text
/// φ_b = f_b(ξ,η) · (ζ t_repr / 2)       t_repr = (t₀+t₁+t₂)/3
/// ∇_x φ_b = J⁻ᵀ · [∂f_b/∂ξ·ζt_r/2, ∂f_b/∂η·ζt_r/2, f_b·t_r/2]ᵀ
/// H[a][jj] = C_repr[a][c] · (∇_x φ_b)[jj]      c ∈ {0, 1}  (Δβ_x, Δβ_y)
/// B_bubble[r][c] = inplane_lamina_strain(H, q)[r]
/// ```
///
/// where `C_repr = −skew(V_repr)`.  Dropping `c = 2` omits the drilling DOF
/// (θ_z), which is sterile for the in-plane strain since `C_repr[:,2] = 0` when
/// `V_repr = e₃`.
///
/// # Flat-inertness
///
/// The caller [`crate::shell_assembly::degenerate_stiffness_core`] short-circuits
/// before invoking this function when all directors are parallel, so flat patches
/// are bit-exact inert (K_NB stays zero) without relying on any integral
/// cancellation here.
// G-allow: degenerate-shell rotation-bubble B-matrix helper, task #4065; called from
// degenerate_stiffness_core on the bubble-active path; orphan-safe (fn-pointer
// registration the orphan audit cannot trace); guarded by the bubble coupling test.
pub fn degenerate_bubble_bending_b(
    jm: &[[f64; 3]; 3], // pre-computed degenerate_jacobian at this quadrature point
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 2]; 3] {
    // Jacobian already computed by the caller (degenerate_stiffness_core) at this
    // quadrature point — reuse it rather than recomputing.  This avoids one full
    // 3×3 Jacobian evaluation (3D interpolation + outer products) per quadrature
    // point on the curved path.
    let (j_inv, _) = mat3_inverse(jm);
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let g_fb_ref = Mitc3Plus.bubble_grad_at(coord.in_plane());
    let f_b_val = Mitc3Plus.bubble_at(coord.in_plane());
    let q = lamina_frame(jm, &n, directors);

    // Representative director = lamina normal e₃ (row 2 of the lamina frame).
    let v_repr = q[2];
    // C_repr = −skew(V_repr): same convention as degenerate_membrane_bending_b.
    let c_repr = [
        [0.0, v_repr[2], -v_repr[1]],
        [-v_repr[2], 0.0, v_repr[0]],
        [v_repr[1], -v_repr[0], 0.0],
    ];

    // Representative thickness (nodal average); t_repr/2 is the half-thickness.
    let t_repr = (thicknesses[0] + thicknesses[1] + thicknesses[2]) / 3.0;
    let half_t_repr = 0.5 * t_repr;
    let zt_repr = 0.5 * coord.zeta * t_repr; // ζ · t_repr / 2

    // 3D reference gradient of φ_b = f_b · (ζ t_repr / 2):
    // [∂f_b/∂ξ·zt, ∂f_b/∂η·zt, f_b·t_repr/2] → pushed to physical by J⁻ᵀ.
    let g_phi_b = jinv_t_mul(
        &j_inv,
        &[g_fb_ref[0] * zt_repr, g_fb_ref[1] * zt_repr, f_b_val * half_t_repr],
    );

    // 2 bubble enrichment columns: cc=0 (Δβ_x) and cc=1 (Δβ_y); skip cc=2 (drilling).
    let mut b = [[0.0_f64; 2]; 3];
    #[allow(clippy::needless_range_loop)]
    for cc in 0..2 {
        let mut h = [[0.0_f64; 3]; 3];
        for a in 0..3 {
            for jj in 0..3 {
                h[a][jj] = c_repr[a][cc] * g_phi_b[jj];
            }
        }
        let e = inplane_lamina_strain(&h, &q);
        for r in 0..3 {
            b[r][cc] = e[r];
        }
    }
    b
}

/// Exact degenerate-kinematics **covariant** in-plane MEMBRANE strain B-matrix
/// (`3 × 18`) at the 3D reference coordinate `coord`, before any assumed-strain
/// re-interpolation.
///
/// Rows are the covariant membrane components `(ε_ξξ, ε_ηη, 2ε_ξη)` (engineering
/// in-plane shear in the third row); columns the 18 nodal DOFs
/// (`6·node + {u_x,u_y,u_z,θ_x,θ_y,θ_z}`).
///
/// # Kinematics (the actual covariant strain)
///
/// With the degenerate in-plane base vectors `g_α = ∂X/∂ξ_α` (α ∈ {ξ, η}, the
/// first two columns of [`degenerate_jacobian`]) and the displacement field
/// `u = Σ_i N_i u_i + (ζ t_i/2) Σ_i N_i (θ_i × V_i)`, the covariant membrane
/// strain is the genuine in-plane `ε_αβ`:
///
/// ```text
/// ε_αβ = ½ (g_α · u_,β + g_β · u_,α)        (α, β ∈ {ξ, η})
/// u_,α = Σ_i N_i,α u_i + (ζ t_i/2) Σ_i N_i,α (θ_i × V_i)
/// ```
///
/// reading translation `u_i` (DOFs 0–2) and ALL THREE rotations `θ_i` (DOFs 3–5)
/// via `(θ_i × V_i) = C_i·θ_i`, `C_i = −skew(V_i)` — structurally mirroring
/// [`degenerate_exact_covariant_shear_b`]. Because it IS the covariant strain a
/// rigid-body mode gives an identically-zero `ε_αβ`, and the field rotates as a
/// tensor under a rigid 3D rotation.
///
/// At the mid-surface (`ζ = 0`) the rotation contribution vanishes (the `ζ t/2`
/// factor), so the covariant MEMBRANE strain is translation-only and — for a
/// linear triangle with constant `∇N_i` and `ζ`-independent in-plane base vectors
/// — element-CONSTANT in `(ξ, η)`. [`degenerate_assumed_membrane_b`] samples this
/// at the mid-surface to build the assumed-natural-strain membrane field.
fn degenerate_exact_covariant_membrane_b(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 18]; 3] {
    let (j, _det) = degenerate_jacobian(nodes, directors, thicknesses, coord);
    // In-plane covariant base vectors = first two columns of J.
    let g_xi = [j[0][0], j[1][0], j[2][0]];
    let g_eta = [j[0][1], j[1][1], j[2][1]];
    let dn = Mitc3Plus.shape_grad_at(coord.in_plane());
    let half_zeta = 0.5 * coord.zeta;

    let mut b = [[0.0_f64; 18]; 3];
    for i in 0..Mitc3Plus::N_NODES {
        let zt = half_zeta * thicknesses[i]; // ζ·t_i/2
        // Translation DOFs (a = 0,1,2): u_,α = N_i,α e_a, so
        //   ε_ξξ  col(6i+a) = g_ξ · (N_i,ξ e_a) = N_i,ξ · (g_ξ)_a
        //   ε_ηη  col       = N_i,η · (g_η)_a
        //   2ε_ξη col       = N_i,η · (g_ξ)_a + N_i,ξ · (g_η)_a
        for a in 0..3 {
            let col = 6 * i + a;
            b[0][col] = dn[i][0] * g_xi[a];
            b[1][col] = dn[i][1] * g_eta[a];
            b[2][col] = dn[i][1] * g_xi[a] + dn[i][0] * g_eta[a];
        }
        // Rotation DOFs (c = 0,1,2 → θ_x,θ_y,θ_z): u_,α[θ_c] = (ζ t_i/2) N_i,α C_i[:,c],
        // C_i = −skew(V_i). So
        //   ε_ξξ  col(6i+3+c) = g_ξ · (zt N_i,ξ C_i[:,c]) = zt N_i,ξ (g_ξ·C_i[:,c])
        //   ε_ηη  col         = zt N_i,η (g_η·C_i[:,c])
        //   2ε_ξη col         = zt N_i,η (g_ξ·C_i[:,c]) + zt N_i,ξ (g_η·C_i[:,c])
        let c_cols = director_rotation_columns(&directors[i]);
        for (c, cc) in c_cols.iter().enumerate() {
            let g_xi_cc = dot3(&g_xi, cc);
            let g_eta_cc = dot3(&g_eta, cc);
            let col = 6 * i + 3 + c;
            b[0][col] = zt * dn[i][0] * g_xi_cc;
            b[1][col] = zt * dn[i][1] * g_eta_cc;
            b[2][col] = zt * dn[i][1] * g_xi_cc + zt * dn[i][0] * g_eta_cc;
        }
    }
    b
}

/// Assumed **covariant** transverse-shear B-matrix (`2 × 18`) at the in-plane
/// reference coordinate `coord`, carried VERBATIM from the MITC3+ formulation
/// (task 3392) using the **flat-facet** (global `u_z/θ_x/θ_y`) kinematics.
///
/// Rows are the covariant components `(γ_ξζ, γ_ηζ)`; columns the 18 nodal DOFs
/// (`6·node + {u_x,u_y,u_z,θ_x,θ_y,θ_z}`). Construction — identical to the inline
/// assembly in [`crate::shell_assembly::shell_element_stiffness_mitc3_plus`]:
///
/// 1. sample the bare three-node (DISP3) covariant shear
///    ([`Mitc3Plus::covariant_shear_b_nodal`]) at the six interior tying points
///    ([`Mitc3Plus::interior_tying_points`]);
/// 2. re-interpolate each DOF column via
///    [`Mitc3Plus::interpolate_assumed_shear_mitc3_plus`] (Lee–Lee–Bathe Eq. 9).
///
/// This field is **geometry-free** (pure natural coordinates): it is *exactly*
/// task 3392's assumed transverse-shear field — no new shear formulation is
/// introduced here.
///
/// # Role: a documented flat-equivalence reference, NOT the stiffness path
///
/// `Mitc3Plus::covariant_shear_b_nodal` hard-codes the flat-facet Reissner–Mindlin
/// kinematics — it reads the *global* `u_z` (DOF 2), `θ_x` (DOF 3), `θ_y` (DOF 4)
/// and assumes the shell normal is the global `+z`; it carries neither `θ_z` nor
/// the per-node director `V_i`. On a curved (tilted-director) patch no linear
/// covariant→physical map of this flat field can simultaneously satisfy the
/// rigid-body null-space and 3D-rotation isotropy (esc-4068-127). It is therefore
/// **not** the field the stiffness uses; [`degenerate_transverse_shear_b`] builds
/// the covariant shear from the *exact* degenerate kinematics instead. This
/// function is a test-only (`#[cfg(test)]`) witness that, when flat, the exact
/// kinematics coincide with task 3392's carried DISP3 covariant field, used by
/// the constant-state and flat-reduction tests below. "No new shear field" holds
/// by construction: it invokes only the task-3392 primitives
/// (`covariant_shear_b_nodal` + `interpolate_assumed_shear_mitc3_plus`), so there
/// is no second reconstruction to drift against.
#[cfg(test)]
fn degenerate_assumed_covariant_shear_b(coord: ShellReferenceCoord) -> [[f64; 18]; 2] {
    use crate::elements::mitc3_plus::ShearStrain;
    let interior = Mitc3Plus.interior_tying_points();
    // (1) Sample the bare-DISP3 covariant shear at the six interior tying points.
    let mut b_tp = [[[0.0_f64; 18]; 2]; Mitc3Plus::N_INTERIOR_TYING_POINTS];
    for (k, tp) in interior.iter().enumerate() {
        b_tp[k] = Mitc3Plus.covariant_shear_b_nodal(tp.coord);
    }
    // (2) Re-interpolate each DOF column via Eq. 9 at `coord`.
    let mut b = [[0.0_f64; 18]; 2];
    for dof in 0..Mitc3Plus::N_DOFS {
        let samples: [ShearStrain; Mitc3Plus::N_INTERIOR_TYING_POINTS] = [
            ShearStrain { gamma_xi_zeta: b_tp[0][0][dof], gamma_eta_zeta: b_tp[0][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[1][0][dof], gamma_eta_zeta: b_tp[1][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[2][0][dof], gamma_eta_zeta: b_tp[2][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[3][0][dof], gamma_eta_zeta: b_tp[3][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[4][0][dof], gamma_eta_zeta: b_tp[4][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[5][0][dof], gamma_eta_zeta: b_tp[5][1][dof] },
        ];
        let proj = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, coord);
        b[0][dof] = proj.gamma_xi_zeta;
        b[1][dof] = proj.gamma_eta_zeta;
    }
    b
}

/// Exact degenerate-kinematics **covariant** transverse-shear B-matrix (`2 × 18`)
/// at the 3D reference coordinate `coord`, before the MITC3+ tying interpolation.
///
/// Rows are the covariant components `(γ_ξζ, γ_ηζ)`; columns the 18 nodal DOFs
/// (`6·node + {u_x,u_y,u_z,θ_x,θ_y,θ_z}`).
///
/// # Kinematics (the actual covariant strain)
///
/// With the degenerate base vectors `g_α = ∂X/∂ξ_α` and `g_ζ = ∂X/∂ζ` (the
/// columns of [`degenerate_jacobian`]) and the displacement field
/// `u = Σ_i N_i u_i + (ζ t_i/2) Σ_i N_i (θ_i × V_i)`, the (engineering) covariant
/// transverse shear is the genuine `2·ε_αζ`:
///
/// ```text
/// γ_αζ = g_α · u_,ζ + g_ζ · u_,α        (α ∈ {ξ, η})
/// u_,ζ = Σ_i N_i (t_i/2)(θ_i × V_i)                     (translations contribute 0)
/// u_,α = Σ_i N_i,α u_i + (ζ t_i/2) Σ_i N_i,α (θ_i × V_i)
/// ```
///
/// reading translation `u_i` (DOFs 0–2) and ALL THREE rotations `θ_i` (DOFs 3–5)
/// via `(θ_i × V_i) = C_i·θ_i`, `C_i = −skew(V_i)` — exactly mirroring the
/// membrane/bending construction in [`degenerate_membrane_bending_b`]. Because it
/// IS the covariant strain, a rigid-body mode (which has zero strain) gives an
/// identically-zero `γ_αζ`, and the field rotates as a tensor under a rigid 3D
/// rotation — the two properties no linear remap of the flat global-DOF field
/// could provide together (esc-4068-127).
///
/// On a flat facet (`V_i ∥ +z`, planar nodes) this reduces — after the
/// through-thickness `1/|g_ζ|` and in-plane `J⁻ᵀ` map of
/// [`degenerate_transverse_shear_b`] — to task 3392's carried DISP3 covariant
/// field, so the flat reduction is preserved.
fn degenerate_exact_covariant_shear_b(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 18]; 2] {
    let (j, _det) = degenerate_jacobian(nodes, directors, thicknesses, coord);
    // Covariant base vectors = columns of J.
    let g_xi = [j[0][0], j[1][0], j[2][0]];
    let g_eta = [j[0][1], j[1][1], j[2][1]];
    let g_zeta = [j[0][2], j[1][2], j[2][2]];
    let n = Mitc3Plus.shape_at(coord.in_plane());
    let dn = Mitc3Plus.shape_grad_at(coord.in_plane());
    let half_zeta = 0.5 * coord.zeta;

    let mut b = [[0.0_f64; 18]; 2];
    for i in 0..Mitc3Plus::N_NODES {
        let half_t = 0.5 * thicknesses[i];
        let zt = half_zeta * thicknesses[i]; // ζ·t_i/2
        // Translation DOFs (a = 0,1,2): u_,α = N_i,α e_a, u_,ζ = 0, so
        // γ_αζ col(6i+a) = g_ζ · (N_i,α e_a) = N_i,α · (g_ζ)_a.
        for a in 0..3 {
            b[0][6 * i + a] = dn[i][0] * g_zeta[a]; // α = ξ
            b[1][6 * i + a] = dn[i][1] * g_zeta[a]; // α = η
        }
        // Rotation DOFs (c = 0,1,2 → θ_x,θ_y,θ_z): (θ × V) = C_i·θ, C_i = −skew(V_i),
        // so ∂(θ×V)/∂θ_c = C_i[:,c]. Then
        //   γ_αζ col(6i+3+c) = g_α · (N_i (t_i/2) C_i[:,c])
        //                    + g_ζ · (ζ t_i/2 · N_i,α · C_i[:,c]).
        let c_cols = director_rotation_columns(&directors[i]);
        for (c, cc) in c_cols.iter().enumerate() {
            let g_xi_cc = dot3(&g_xi, cc);
            let g_eta_cc = dot3(&g_eta, cc);
            let g_zeta_cc = dot3(&g_zeta, cc);
            b[0][6 * i + 3 + c] = n[i] * half_t * g_xi_cc + zt * dn[i][0] * g_zeta_cc;
            b[1][6 * i + 3 + c] = n[i] * half_t * g_eta_cc + zt * dn[i][1] * g_zeta_cc;
        }
    }
    b
}

/// Physical (lamina) transverse-shear B-matrix (`2 × 18`) at `coord`: the EXACT
/// degenerate-kinematics covariant shear ([`degenerate_exact_covariant_shear_b`])
/// run through the MITC3+ interior-tying assumed-strain interpolation (task 3392's
/// locking cure, carried verbatim) and mapped covariant→physical against the
/// now-**varying** Jacobian.
///
/// Rows are the physical lamina transverse-shear components `(γ_1ζ', γ_2ζ')`
/// (lamina axes `e1, e2` against the director `e3`); columns the 18 nodal DOFs.
///
/// # Pipeline
///
/// 1. **Exact covariant samples.** Sample the exact covariant shear B
///    ([`degenerate_exact_covariant_shear_b`]) at the six interior tying points
///    ([`Mitc3Plus::interior_tying_points`]), all at the current integration `ζ`.
/// 2. **Carried MITC3+ locking cure (verbatim).** Re-interpolate each DOF column
///    via [`Mitc3Plus::interpolate_assumed_shear_mitc3_plus`] (Lee–Lee–Bathe
///    Eq. 9) to the in-plane `(ξ, η)`. This is task 3392's assumed-strain field
///    UNCHANGED — only the per-tying-point covariant *samples* now come from the
///    true curved kinematics instead of the flat global-DOF field.
/// 3. **Covariant→physical map.** Map the assumed covariant pair to the physical
///    lamina shear with the in-plane `2×2` sub-block of `J_lam⁻ᵀ = q·J⁻ᵀ` plus the
///    through-thickness normalization scalar:
///
///    ```text
///    [γ_1ζ'; γ_2ζ'] = s₃ · (J_lam⁻ᵀ)[0..2, 0..2] · [γ_ξζ; γ_ηζ]
///    ```
///
///    Here `q` (rows `e1,e2,e3`; [`lamina_frame`]) gives `J_lam = q·J`, so
///    `J_lam⁻ᵀ = q·J⁻ᵀ`; `m2 = (q·J⁻ᵀ)[0..2,0..2]` is the in-plane contravariant
///    projection `e'_p·g^i` (`p,i ∈ {1,2}`), and `s₃ = (q·J⁻ᵀ)[2][2] = e'_3·g^ζ`
///    is the through-thickness factor `1/|g_ζ|` that strips the `g_ζ` metric the
///    exact covariant field carries (the flat-facet DISP3 field of task 3392 was
///    pre-normalized, so it needed only the in-plane block; the exact field needs
///    both). Every factor is built from the rotated geometry, so the product is
///    invariant under a rigid 3D rotation (frame-objective) and vanishes for a
///    rigid mode (zero covariant strain).
///
/// # Flat reduction
///
/// On a flat facet `J` is `ζ`-invariant, `q` reduces to
/// [`crate::shell_assembly::build_shell_frame`], `s₃ = 2/t = 1/|g_ζ|`, and the
/// exact covariant samples × `s₃·m2` reduce to task 3392's
/// `jac2_inv_t · covariant_shear_b_nodal` flat map at every `ζ`.
pub fn degenerate_transverse_shear_b(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 18]; 2] {
    use crate::elements::mitc3_plus::ShearStrain;

    // (1) Sample the EXACT covariant shear B at the six interior tying points, at
    // the current integration ζ (the tying is in-plane; ζ rides along).
    let interior = Mitc3Plus.interior_tying_points();
    let mut b_tp = [[[0.0_f64; 18]; 2]; Mitc3Plus::N_INTERIOR_TYING_POINTS];
    for (k, tp) in interior.iter().enumerate() {
        let c3 = ShellRefCoord3::new(tp.coord.xi, tp.coord.eta, coord.zeta);
        b_tp[k] = degenerate_exact_covariant_shear_b(nodes, directors, thicknesses, c3);
    }

    // (2) Carried MITC3+ Eq. 9 assumed-strain re-interpolation, column by column.
    let mut b_cov = [[0.0_f64; 18]; 2];
    for dof in 0..18 {
        let samples: [ShearStrain; Mitc3Plus::N_INTERIOR_TYING_POINTS] = [
            ShearStrain { gamma_xi_zeta: b_tp[0][0][dof], gamma_eta_zeta: b_tp[0][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[1][0][dof], gamma_eta_zeta: b_tp[1][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[2][0][dof], gamma_eta_zeta: b_tp[2][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[3][0][dof], gamma_eta_zeta: b_tp[3][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[4][0][dof], gamma_eta_zeta: b_tp[4][1][dof] },
            ShearStrain { gamma_xi_zeta: b_tp[5][0][dof], gamma_eta_zeta: b_tp[5][1][dof] },
        ];
        let proj = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, coord.in_plane());
        b_cov[0][dof] = proj.gamma_xi_zeta;
        b_cov[1][dof] = proj.gamma_eta_zeta;
    }

    // (3) Covariant→physical map at the varying J: s₃ · m2 · b_cov.
    let (m2, s3) = lamina_contravariant_projection(nodes, directors, thicknesses, coord);

    let mut b_phys = [[0.0_f64; 18]; 2];
    for dof in 0..18 {
        b_phys[0][dof] = s3 * (m2[0][0] * b_cov[0][dof] + m2[0][1] * b_cov[1][dof]);
        b_phys[1][dof] = s3 * (m2[1][0] * b_cov[0][dof] + m2[1][1] * b_cov[1][dof]);
    }
    b_phys
}

/// Physical (lamina) assumed-natural-strain **membrane** B-matrix (`3 × 18`) at
/// the mid-surface above the in-plane reference coordinate `coord` (the `ζ`
/// component of `coord` is ignored — the membrane is a mid-surface quantity).
///
/// Rows are the physical lamina membrane strains `(ε'₁₁, ε'₂₂, 2ε'₁₂)` (lamina
/// axes `e1, e2`); columns the 18 nodal DOFs. This is the assumed-natural-strain
/// MEMBRANE field that cures membrane locking on the curved (varying-J)
/// substrate — the membrane analogue of [`degenerate_transverse_shear_b`].
///
/// # Pipeline (mirrors the transverse-shear ANS pipeline)
///
/// 1. **Exact covariant sample.** Sample the exact covariant membrane strain
///    ([`degenerate_exact_covariant_membrane_b`]) once, at the element centroid
///    on the mid-surface `ζ = 0`.
/// 2. **Constant-state-consistent assumed field.** For a linear triangle the
///    covariant membrane natural strain is element-CONSTANT at the mid-surface
///    (constant `∇N_i`, `ζ`-independent in-plane base vectors, and the rotation
///    term carries a `ζ` factor that vanishes at `ζ = 0`). The constant-state-
///    consistent assumed field is therefore just that common constant — a single
///    collocation sample reproduces any constant covariant state exactly (the
///    step-3 consistency prerequisite) and is the membrane analogue of the
///    constant-reproduction property of `interpolate_assumed_shear_mitc3_plus`.
///    (Sampling all six interior tying points and averaging would yield the
///    identical constant; the single evaluation just avoids five redundant
///    [`degenerate_jacobian`] builds.)
/// 3. **Covariant→physical lamina map.** Map the (constant) covariant membrane
///    tensor to the physical lamina membrane strain with the in-plane `2×2`
///    sub-block `m2 = (q·J⁻ᵀ)[0..2, 0..2]` of the lamina contravariant projection
///    (`q` from [`lamina_frame`]), via the rank-2 tensor transformation
///    `ε'_pq = Σ_{a,b} m2[p][a] m2[q][b] ε_ab` (`a, b ∈ {ξ, η}`).
///
/// # The locking cure (esc-4068-127 mechanism, membrane analogue)
///
/// The displacement-based membrane ([`degenerate_membrane_bending_b`] at `ζ = 0`)
/// projects the full velocity gradient into the lamina frame, so on a curved
/// (tilted-director) element the OUT-OF-PLANE component of the lamina axes `e_p`
/// (the director `e3` is not the surface normal there) leaks director-rotation
/// energy into the membrane strain — the parasitic membrane lock. The ANS field
/// instead maps the genuine covariant membrane tensor through the in-plane
/// contravariant projection `m2`, which contracts `e_p` onto the tangent plane
/// and filters that parasitic coupling. Because `m2 = q·J⁻ᵀ` co-rotates with the
/// configuration and the covariant strain vanishes for any rigid mode, the field
/// is frame-objective and rigid-body-compatible by construction.
///
/// # Flat reduction
///
/// On a flat facet the lamina axes lie in the tangent plane (`e3` ∥ facet normal),
/// so the in-plane projection is exact and the ANS membrane B equals the
/// displacement-based membrane B at `ζ = 0` (the flat-inertness anchor, step-5).
pub fn degenerate_assumed_membrane_b(
    nodes: &[[f64; 3]; 3],
    directors: &[Director; 3],
    thicknesses: &[f64; 3],
    coord: ShellRefCoord3,
) -> [[f64; 18]; 3] {
    // The membrane is a mid-surface quantity: evaluate everything at ζ = 0 above
    // the requested in-plane location.
    let mid = ShellRefCoord3::new(coord.xi, coord.eta, 0.0);

    // (1)+(2) Sample the exact covariant membrane strain ONCE, at the element
    // centroid on the mid-surface (ζ = 0). For a linear triangle this covariant
    // membrane natural strain is element-CONSTANT at ζ = 0 (constant ∇N_i,
    // ζ-independent in-plane base vectors, and the rotation term carries a ζ
    // factor that vanishes at ζ = 0), so a single collocation sample IS the
    // constant-state-consistent assumed field: it reproduces any constant
    // covariant state exactly (the step-3 consistency prerequisite) — the membrane
    // analogue of the constant-reproduction property of
    // interpolate_assumed_shear_mitc3_plus. Sampling all six interior tying points
    // and averaging would yield the identical constant, so the single evaluation
    // just avoids five redundant degenerate_jacobian builds.
    let centroid = ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
    let b_cov = degenerate_exact_covariant_membrane_b(nodes, directors, thicknesses, centroid);

    // (3) Covariant→physical lamina map at (ξ, η, 0): ε'_pq = Σ m2[p][a] m2[q][b] ε_ab.
    // (s3 is the transverse-shear through-thickness factor — unused by the membrane.)
    let (m2, _s3) = lamina_contravariant_projection(nodes, directors, thicknesses, mid);

    let mut b_phys = [[0.0_f64; 18]; 3];
    for dof in 0..18 {
        let e_xx = b_cov[0][dof];
        let e_yy = b_cov[1][dof];
        let e_xy = 0.5 * b_cov[2][dof]; // tensor ε_ξη from engineering 2ε_ξη
        // ε'_11 = Σ_{a,b} m2[0][a] m2[0][b] ε_ab
        b_phys[0][dof] = m2[0][0] * m2[0][0] * e_xx
            + m2[0][1] * m2[0][1] * e_yy
            + 2.0 * m2[0][0] * m2[0][1] * e_xy;
        // ε'_22
        b_phys[1][dof] = m2[1][0] * m2[1][0] * e_xx
            + m2[1][1] * m2[1][1] * e_yy
            + 2.0 * m2[1][0] * m2[1][1] * e_xy;
        // 2ε'_12
        let e12 = m2[0][0] * m2[1][0] * e_xx
            + m2[0][1] * m2[1][1] * e_yy
            + (m2[0][0] * m2[1][1] + m2[0][1] * m2[1][0]) * e_xy;
        b_phys[2][dof] = 2.0 * e12;
    }
    b_phys
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
// G-allow: degenerate-shell (MITC3+) default director source, tasks #4068/#4069; reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests.
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
                #[allow(clippy::needless_range_loop)] // `r` is used in the assertion message
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
            #[allow(clippy::needless_range_loop)] // `r` is used in the assertion message
            for r in 0..3 {
                assert!(
                    b0[r][uz].abs() < 1e-12,
                    "u_z col {uz} row {r} = {} must be 0 at ζ=0",
                    b0[r][uz],
                );
            }
        }
    }

    // ── step-9: carry MITC3+ assumed transverse-shear onto the varying J ─────

    /// Contract a 2×18 transverse-shear B-matrix with an 18-DOF vector → the
    /// 2-component `(γ_ξζ, γ_ηζ)` (covariant) or `(γ_1ζ', γ_2ζ')` (physical)
    /// shear, depending on which B is passed.
    fn shear_b_times_u(b: &[[f64; 18]; 2], u: &[f64; 18]) -> [f64; 2] {
        let mut e = [0.0_f64; 2];
        for (r, row) in b.iter().enumerate() {
            for (c, &bc) in row.iter().enumerate() {
                e[r] += bc * u[c];
            }
        }
        e
    }

    /// (i) Patch-safety prerequisite: the carried assumed **covariant** shear
    /// field reproduces a CONSTANT covariant transverse-shear state exactly at
    /// every probe. Mirrors
    /// `covariant_shear_b_nodal_plus_eq9_reproduces_constant_shear_state`: a
    /// uniform `θ_y = α` (with `w = 0`, `θ_x = 0`) is the constant covariant
    /// state `(γ_ξζ = α, γ_ηζ = 0)`, so Eq. 9 must return `(α, 0)` at any `(ξ,η)`.
    #[test]
    fn degenerate_assumed_covariant_shear_reproduces_constant_state() {
        let alpha = 0.7_f64;
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i + 4] = alpha; // θ_y at every node
        }
        for p in [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.2, 0.3),
            ShellReferenceCoord::new(0.5, 0.25),
        ] {
            let b = degenerate_assumed_covariant_shear_b(p);
            let e = shear_b_times_u(&b, &u);
            assert!((e[0] - alpha).abs() < TOL, "γ_ξζ at {p:?} = {} expected {alpha}", e[0]);
            assert!(e[1].abs() < TOL, "γ_ηζ at {p:?} = {} expected 0", e[1]);
        }
    }

    /// (ii) FLAT-facet reduction on a UNIT-METRIC orthogonal triangle: the
    /// varying-J covariant→physical shear map equals task 3392's constant `J2⁻ᵀ`
    /// map. The reference triangle `[(0,0),(1,0),(0,1)]` in the xy-plane has its
    /// first edge along +x (so `build_shell_frame`'s lamina frame is the global
    /// xyz frame) AND `g_ξ = (1,0,0)`, `g_η = (0,1,0)` — i.e. `|g_ξ| = |g_η| = 1`
    /// and `g_ξ ⊥ g_η`. With directors +z and uniform thickness, the physical
    /// transverse-shear B from [`degenerate_transverse_shear_b`] equals the
    /// carried covariant field mapped by `shell_kinematics(...).jac2_inv_t` — at
    /// every ζ (a flat facet has a ζ-invariant J) — to 1e-12 (measured: max diff
    /// 0.0).
    ///
    /// # Why the UNIT-metric triangle (esc-4068-134)
    ///
    /// The exact degenerate kinematics ([`degenerate_exact_covariant_shear_b`])
    /// carry the in-plane metric on the rotation term: the covariant `γ_αζ[θ]`
    /// coefficient is `|g_α|·h_i`, the genuine standard-MITC3 value. Task 3392's
    /// [`Mitc3Plus::covariant_shear_b_nodal`] writes the bare `h_i` with **no**
    /// metric, and the energy applies `jac2_inv_t` (the contravariant metric
    /// `g^{αβ}`), so 3392's physical rotation shear is `h_i/|g_α|` — metric-
    /// SIMPLIFIED, exact only where `|g_α| = 1`. The two therefore agree bit-for-
    /// bit **only** on a unit-metric orthogonal flat triangle (here, the unit
    /// reference triangle, where iteration-6 measured max diff 0.0). On a general
    /// (non-unit / skew) flat triangle the degenerate element computes the
    /// physically-correct standard-MITC3 shear, differing from 3392's simplified
    /// value by the in-plane metric `|g_α|` — a strict IMPROVEMENT, pinned
    /// positively by the non-unit constant-shear PATCH test in `shell_assembly`.
    /// "Carry MITC3+ shear" stays satisfied: the assumed-strain Eq. 9 LOCKING
    /// treatment (3392's deliverable) is carried verbatim; only the per-tying-
    /// point covariant SAMPLES use true curved kinematics.
    #[test]
    fn degenerate_transverse_shear_b_flat_reduces_to_mitc3_plus_j2_inv_t() {
        // UNIT reference triangle: |g_ξ| = |g_η| = 1, g_ξ ⊥ g_η — the one flat
        // geometry where 3392's metric-simplified shear equals the exact one.
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let directors = [[0.0, 0.0, 1.0]; 3];
        let thicknesses = [0.25; 3];

        // 3392 covariant→physical map: J2⁻ᵀ from the shared shell kinematics.
        let frame = build_shell_frame(&nodes);
        let kin = crate::shell_kinematics::shell_kinematics(&nodes, &frame);
        let inv_t = kin.jac2_inv_t;

        let (pxi, peta) = (0.25_f64, 0.35_f64);
        let b_cov = degenerate_assumed_covariant_shear_b(ShellReferenceCoord::new(pxi, peta));
        // Reference physical B = J2⁻ᵀ · b_cov (3392's constant-J flat map).
        let mut b_ref = [[0.0_f64; 18]; 2];
        for dof in 0..18 {
            b_ref[0][dof] = inv_t[0][0] * b_cov[0][dof] + inv_t[0][1] * b_cov[1][dof];
            b_ref[1][dof] = inv_t[1][0] * b_cov[0][dof] + inv_t[1][1] * b_cov[1][dof];
        }

        // Varying-J degenerate map at several ζ (flat ⇒ ζ-invariant ⇒ equal to
        // the constant-J 3392 map at all ζ).
        for &zeta in &[-0.6, 0.0, 0.6] {
            let b_deg = degenerate_transverse_shear_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(pxi, peta, zeta),
            );
            for r in 0..2 {
                for c in 0..18 {
                    assert!(
                        (b_deg[r][c] - b_ref[r][c]).abs() < 1e-12,
                        "flat shear B[{r}][{c}] @ ζ={zeta} = {} ≠ 3392 J2⁻ᵀ map {}",
                        b_deg[r][c],
                        b_ref[r][c],
                    );
                }
            }
        }
    }

    // ── amendment (reviewer #1): curved-patch rigid rotation & frame objectivity ─

    /// Cross product `a × b`.
    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// Apply a 3×3 matrix to a 3-vector (`m · v`).
    fn matvec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    /// A fixed *proper* rigid rotation `R = Rz(γ)·Ry(β)·Rx(α)` (det = +1), in
    /// closed form (ZYX Euler, α=0.3, β=−0.4, γ=0.5 rad). Rotates the whole patch
    /// (nodes, directors, DOFs) for the frame-objectivity test.
    fn rot_matrix() -> [[f64; 3]; 3] {
        let (a, b, c) = (0.3_f64, -0.4_f64, 0.5_f64);
        let (sa, ca) = (a.sin(), a.cos());
        let (sb, cb) = (b.sin(), b.cos());
        let (sc, cc) = (c.sin(), c.cos());
        [
            [cb * cc, sa * sb * cc - ca * sc, ca * sb * cc + sa * sc],
            [cb * sc, sa * sb * sc + ca * cc, ca * sb * sc - sa * cc],
            [-sb, sa * cb, ca * cb],
        ]
    }

    /// Headline esc-4068-127 property, part 1 — **rigid-body compatibility on a
    /// CURVED patch.** A consistent infinitesimal rigid rotation of the degenerate
    /// element is `u_i = ω × x_i`, `θ_i = ω` (each director co-rotates as
    /// `θ_i × V_i = ω × V_i`), which reproduces the exact rigid field
    /// `u(X) = ω × X` at every fibre point. The membrane/bending operator
    /// (`ε = sym ∂u/∂x = sym skew(ω) = 0`) AND the exact-covariant transverse shear
    /// (`γ_αζ = g_α·(ω×g_ζ) + g_ζ·(ω×g_α) = 0` by the scalar triple product) must
    /// both annihilate it at every `(ξ,η,ζ)`. This is exactly why the flat
    /// global-DOF shear field of task 3392 was rejected on curved geometry — and
    /// the most error-prone claim of the module — so it is verified directly on the
    /// tilted-director fixture (not just on a flat facet).
    #[test]
    fn degenerate_b_rigid_rotation_on_curved_patch_yields_zero_strain() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        let omega = [0.02_f64, -0.05, 0.03];
        let mut u = [0.0_f64; 18];
        for (i, x_i) in nodes.iter().enumerate() {
            let t = cross(omega, *x_i); // translation u_i = ω × x_i
            u[6 * i] = t[0];
            u[6 * i + 1] = t[1];
            u[6 * i + 2] = t[2];
            u[6 * i + 3] = omega[0]; // rotation θ_i = ω
            u[6 * i + 4] = omega[1];
            u[6 * i + 5] = omega[2];
        }
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.2, 0.3), (0.5, 0.25)] {
            for &zeta in &[-0.6_f64, 0.0, 0.6] {
                let c = ShellRefCoord3::new(xi, eta, zeta);
                let e_mb = b_times_u(
                    &degenerate_membrane_bending_b(&nodes, &directors, &thicknesses, c),
                    &u,
                );
                for (r, &er) in e_mb.iter().enumerate() {
                    assert!(
                        er.abs() < 1e-9,
                        "rigid-rotation membrane/bending strain[{r}] = {er} @ ({xi},{eta},{zeta})",
                    );
                }
                let e_s = shear_b_times_u(
                    &degenerate_transverse_shear_b(&nodes, &directors, &thicknesses, c),
                    &u,
                );
                for (r, &er) in e_s.iter().enumerate() {
                    assert!(
                        er.abs() < 1e-9,
                        "rigid-rotation transverse shear[{r}] = {er} @ ({xi},{eta},{zeta})",
                    );
                }
            }
        }
    }

    /// Headline esc-4068-127 property, part 2 — **frame objectivity.** Rotating the
    /// whole configuration (nodes, directors, AND the DOF vector — translations by
    /// `R`, rotation pseudo-vectors by `R`) by a fixed rigid `R` leaves the lamina
    /// strains unchanged: the strain tensor and the lamina frame co-rotate
    /// together (`q → qRᵀ`, `J⁻ᵀ → RJ⁻ᵀ`, so `q·J⁻ᵀ` and every dot-product are
    /// invariant), hence the lamina Voigt components `[ε'₁₁, ε'₂₂, 2ε'₁₂]` and the
    /// physical transverse shear `(γ'_1ζ, γ'_2ζ)` are invariant. An arbitrary
    /// (non-rigid) DOF field is used so the strains are genuinely non-zero — a
    /// non-objective formulation would differ by O(strain), ~1e7× the tolerance.
    #[test]
    fn degenerate_b_is_frame_objective_under_rigid_rotation() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        // Arbitrary small DOF field (6 per node) to exercise real, non-zero strain.
        let u: [f64; 18] = [
            0.010, -0.020, 0.015, 0.030, -0.010, 0.020, // node 0
            -0.005, 0.012, -0.008, -0.020, 0.025, -0.015, // node 1
            0.018, -0.006, 0.022, 0.010, -0.030, 0.005, // node 2
        ];
        let r = rot_matrix();
        let nodes_r = [matvec(&r, nodes[0]), matvec(&r, nodes[1]), matvec(&r, nodes[2])];
        let directors_r =
            [matvec(&r, directors[0]), matvec(&r, directors[1]), matvec(&r, directors[2])];
        // Rotate DOFs: block-diagonal R on each node's (uᵢ) and (θᵢ).
        let mut u_r = [0.0_f64; 18];
        for i in 0..3 {
            let ut = matvec(&r, [u[6 * i], u[6 * i + 1], u[6 * i + 2]]);
            let tt = matvec(&r, [u[6 * i + 3], u[6 * i + 4], u[6 * i + 5]]);
            u_r[6 * i] = ut[0];
            u_r[6 * i + 1] = ut[1];
            u_r[6 * i + 2] = ut[2];
            u_r[6 * i + 3] = tt[0];
            u_r[6 * i + 4] = tt[1];
            u_r[6 * i + 5] = tt[2];
        }
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.25, 0.4), (0.5, 0.2)] {
            for &zeta in &[-0.6_f64, 0.0, 0.6] {
                let c = ShellRefCoord3::new(xi, eta, zeta);
                // Membrane/bending lamina strains: invariant under the frame rotation.
                let e0 = b_times_u(
                    &degenerate_membrane_bending_b(&nodes, &directors, &thicknesses, c),
                    &u,
                );
                let e1 = b_times_u(
                    &degenerate_membrane_bending_b(&nodes_r, &directors_r, &thicknesses, c),
                    &u_r,
                );
                for (k, (&a, &b)) in e0.iter().zip(e1.iter()).enumerate() {
                    assert!(
                        (a - b).abs() < 1e-9,
                        "membrane/bending strain[{k}] not frame-objective: {a} vs {b} @ ({xi},{eta},{zeta})",
                    );
                }
                // Physical transverse shear: invariant under the frame rotation.
                let s0 = shear_b_times_u(
                    &degenerate_transverse_shear_b(&nodes, &directors, &thicknesses, c),
                    &u,
                );
                let s1 = shear_b_times_u(
                    &degenerate_transverse_shear_b(&nodes_r, &directors_r, &thicknesses, c),
                    &u_r,
                );
                for (k, (&a, &b)) in s0.iter().zip(s1.iter()).enumerate() {
                    assert!(
                        (a - b).abs() < 1e-9,
                        "transverse shear[{k}] not frame-objective: {a} vs {b} @ ({xi},{eta},{zeta})",
                    );
                }
            }
        }
    }

    // ── task 4069 step-1/2: exact covariant MEMBRANE strain B ────────────────

    /// (i) On the flat patch, a constant in-plane stretch `u_x = a·x`, `u_y = b·y`
    /// yields the constant COVARIANT membrane strain at `ζ=0`. The covariant strain
    /// carries the un-normalized base-vector metric: with `g_ξ = x₁−x₀ = (2,0,0)`,
    /// `g_η = x₂−x₀ = (0,1.5,0)`, `u_,ξ = (2a,0,0)`, `u_,η = (0,1.5b,0)`,
    ///   `ε_ξξ = g_ξ·u_,ξ = 4a`, `ε_ηη = g_η·u_,η = 2.25b`, `2ε_ξη = 0`.
    /// Evaluated at several `(ξ,η)` — the covariant membrane strain of a linear
    /// triangle is element-constant at the mid-surface. Mirrors
    /// `degenerate_b_constant_stretch_yields_constant_membrane_strain` (covariant
    /// rather than physical-lamina components).
    #[test]
    fn degenerate_exact_covariant_membrane_b_constant_stretch_at_zeta_zero() {
        let (nodes, directors, thicknesses) = flat_patch();
        let a = 0.01_f64;
        let b = -0.004_f64;
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i] = a * nodes[i][0]; // u_x = a·x
            u[6 * i + 1] = b * nodes[i][1]; // u_y = b·y
        }
        // g_ξ = (2,0,0), g_η = (0,1.5,0); u_,ξ = (2a,0,0), u_,η = (0,1.5b,0).
        let want = [4.0 * a, 2.25 * b, 0.0];
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.2, 0.3), (0.5, 0.25)] {
            let bm = degenerate_exact_covariant_membrane_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            let e = b_times_u(&bm, &u);
            for r in 0..3 {
                assert!(
                    (e[r] - want[r]).abs() < TOL,
                    "covariant membrane strain[{r}] at ({xi},{eta}) = {} expected {}",
                    e[r],
                    want[r],
                );
            }
        }
    }

    /// (ii) A rigid translation yields ZERO covariant membrane strain
    /// (`Σ ∇N_i = 0`, so `u_,ξ = u_,η = 0`).
    #[test]
    fn degenerate_exact_covariant_membrane_b_rigid_translation_yields_zero() {
        let (nodes, directors, thicknesses) = flat_patch();
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i] = 0.7;
            u[6 * i + 1] = -0.3;
            u[6 * i + 2] = 0.4;
        }
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.25, 0.4)] {
            let bm = degenerate_exact_covariant_membrane_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            let e = b_times_u(&bm, &u);
            for (r, &er) in e.iter().enumerate() {
                assert!(
                    er.abs() < TOL,
                    "rigid translation covariant membrane strain[{r}] = {er}",
                );
            }
        }
    }

    // ── task 4069 step-3/4: assumed (ANS) membrane B — constant-state prereq ──

    /// Constant-state reproduction (the consistency / patch prerequisite). On the
    /// flat patch (a constant-Jacobian configuration, lamina = global xy) an affine
    /// in-plane field `u_x = a·x + p·y`, `u_y = q·x + b·y` drives a CONSTANT
    /// covariant membrane state, which maps to the constant physical lamina strain
    /// `[a, b, p+q]`. The assumed-natural-strain membrane field
    /// [`degenerate_assumed_membrane_b`] must reproduce it EXACTLY at every `(ξ,η)`
    /// to `TOL = 1e-12`. Mirrors
    /// `degenerate_assumed_covariant_shear_reproduces_constant_state`.
    #[test]
    fn degenerate_assumed_membrane_reproduces_constant_state() {
        let (nodes, directors, thicknesses) = flat_patch();
        let (a, b, p, q) = (0.012_f64, -0.006, 0.004, -0.002);
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            let (x, y) = (nodes[i][0], nodes[i][1]);
            u[6 * i] = a * x + p * y; // u_x = a·x + p·y
            u[6 * i + 1] = q * x + b * y; // u_y = q·x + b·y
        }
        let want = [a, b, p + q];
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.2, 0.3), (0.5, 0.25), (0.1, 0.6)] {
            let bm = degenerate_assumed_membrane_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            let e = b_times_u(&bm, &u);
            for r in 0..3 {
                assert!(
                    (e[r] - want[r]).abs() < TOL,
                    "ANS membrane constant-state strain[{r}] at ({xi},{eta}) = {} expected {}",
                    e[r],
                    want[r],
                );
            }
        }
    }

    // ── task 4069 step-5/6: flat inertness ───────────────────────────────────

    /// Flat-inertness anchor. On a flat facet (planar nodes, directors ∥ +z,
    /// uniform thickness) the assumed-natural-strain membrane B must equal the
    /// membrane part of the displacement-based [`degenerate_membrane_bending_b`]
    /// evaluated at `ζ = 0`, ENTRY-for-entry to 1e-12, at several `(ξ,η)`. This
    /// pins the requirement that the ANS membrane field stays inert when the
    /// element is flat — it must not spuriously soften a flat patch (whose
    /// displacement membrane already carries no parasitic lock). Mirrors
    /// `degenerate_transverse_shear_b_flat_reduces_to_mitc3_plus_j2_inv_t`.
    ///
    /// On a flat facet the lamina axes lie in the tangent plane, so the covariant
    /// in-plane projection `m2` is exact and the ANS membrane equals the direct
    /// lamina projection of the displacement gradient; the rotation columns are
    /// zero in both (the ANS covariant membrane is translation-only at `ζ = 0`,
    /// and the displacement membrane's through-thickness rotation gradient lies
    /// along the flat normal, contributing no in-plane strain).
    #[test]
    fn degenerate_assumed_membrane_b_flat_reduces_to_displacement_membrane() {
        let (nodes, directors, thicknesses) = flat_patch();
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.2, 0.3), (0.5, 0.25), (0.1, 0.6)] {
            let b_ans = degenerate_assumed_membrane_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            // Membrane part of the displacement-based B = the operator at ζ = 0.
            let b_disp = degenerate_membrane_bending_b(
                &nodes,
                &directors,
                &thicknesses,
                ShellRefCoord3::new(xi, eta, 0.0),
            );
            for r in 0..3 {
                for c in 0..18 {
                    assert!(
                        (b_ans[r][c] - b_disp[r][c]).abs() < 1e-12,
                        "flat ANS membrane B[{r}][{c}] @ ({xi},{eta}) = {} ≠ displacement {}",
                        b_ans[r][c],
                        b_disp[r][c],
                    );
                }
            }
        }
    }

    // ── task 4069 step-7/8: rigid-body compatibility & frame objectivity ─────

    /// (a) Rigid-body compatibility on the CURVED tilted fixture. A consistent
    /// rigid rotation `u_i = ω×x_i`, `θ_i = ω` is strain-free, so the ANS membrane
    /// field must annihilate it (`<1e-9`) at several `(ξ,η)`. The covariant
    /// membrane strain IS the genuine strain (zero for any rigid mode), so this
    /// holds by construction — verified directly on tilted directors. Mirrors
    /// `degenerate_b_rigid_rotation_on_curved_patch_yields_zero_strain`.
    #[test]
    fn degenerate_assumed_membrane_b_rigid_rotation_on_curved_patch_yields_zero() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        let omega = [0.02_f64, -0.05, 0.03];
        let mut u = [0.0_f64; 18];
        for (i, x_i) in nodes.iter().enumerate() {
            let t = cross(omega, *x_i);
            u[6 * i] = t[0];
            u[6 * i + 1] = t[1];
            u[6 * i + 2] = t[2];
            u[6 * i + 3] = omega[0];
            u[6 * i + 4] = omega[1];
            u[6 * i + 5] = omega[2];
        }
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.2, 0.3), (0.5, 0.25)] {
            let e = b_times_u(
                &degenerate_assumed_membrane_b(
                    &nodes,
                    &directors,
                    &thicknesses,
                    ShellRefCoord3::new(xi, eta, 0.0),
                ),
                &u,
            );
            for (r, &er) in e.iter().enumerate() {
                assert!(
                    er.abs() < 1e-9,
                    "rigid-rotation ANS membrane strain[{r}] = {er} @ ({xi},{eta})",
                );
            }
        }
    }

    /// (b) Frame objectivity on the CURVED tilted fixture. Rotating the whole
    /// configuration (nodes, directors, DOFs) by a fixed proper `R` leaves the ANS
    /// membrane lamina strains unchanged (`<1e-9`): the covariant strain
    /// (`g_a·u_,b`) and the covariant→physical map `m2 = q·J⁻ᵀ` co-rotate
    /// together. An arbitrary non-rigid DOF field is used so the strains are
    /// genuinely non-zero — a non-objective map would differ by O(strain). Mirrors
    /// `degenerate_b_is_frame_objective_under_rigid_rotation`.
    #[test]
    fn degenerate_assumed_membrane_b_is_frame_objective_under_rigid_rotation() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        let u: [f64; 18] = [
            0.010, -0.020, 0.015, 0.030, -0.010, 0.020, // node 0
            -0.005, 0.012, -0.008, -0.020, 0.025, -0.015, // node 1
            0.018, -0.006, 0.022, 0.010, -0.030, 0.005, // node 2
        ];
        let r = rot_matrix();
        let nodes_r = [matvec(&r, nodes[0]), matvec(&r, nodes[1]), matvec(&r, nodes[2])];
        let directors_r =
            [matvec(&r, directors[0]), matvec(&r, directors[1]), matvec(&r, directors[2])];
        let mut u_r = [0.0_f64; 18];
        for i in 0..3 {
            let ut = matvec(&r, [u[6 * i], u[6 * i + 1], u[6 * i + 2]]);
            let tt = matvec(&r, [u[6 * i + 3], u[6 * i + 4], u[6 * i + 5]]);
            u_r[6 * i] = ut[0];
            u_r[6 * i + 1] = ut[1];
            u_r[6 * i + 2] = ut[2];
            u_r[6 * i + 3] = tt[0];
            u_r[6 * i + 4] = tt[1];
            u_r[6 * i + 5] = tt[2];
        }
        let mut max_strain = 0.0_f64;
        for &(xi, eta) in &[(1.0 / 3.0, 1.0 / 3.0), (0.25, 0.4), (0.5, 0.2)] {
            let c = ShellRefCoord3::new(xi, eta, 0.0);
            let e0 = b_times_u(
                &degenerate_assumed_membrane_b(&nodes, &directors, &thicknesses, c),
                &u,
            );
            let e1 = b_times_u(
                &degenerate_assumed_membrane_b(&nodes_r, &directors_r, &thicknesses, c),
                &u_r,
            );
            for (k, (&a, &b)) in e0.iter().zip(e1.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-9,
                    "ANS membrane strain[{k}] not frame-objective: {a} vs {b} @ ({xi},{eta})",
                );
                max_strain = max_strain.max(a.abs());
            }
        }
        assert!(
            max_strain > 1e-6,
            "non-rigid membrane strain magnitude {max_strain} too small to test objectivity",
        );
    }

    // ── task 4069 step-9/10: parasitic-membrane-reduction witness ────────────

    /// Parasitic-membrane-reduction witness (the locking-cure purpose). On the
    /// curved tilted fixture under a curvature-coupled bending mode (uniform
    /// director rotation, NO translation — an inextensional-type mode), the ANS
    /// membrane strain magnitude `‖ANS·u‖` must be STRICTLY LESS than the
    /// displacement-based membrane strain magnitude `‖disp·u‖` at the in-plane
    /// integration points. RELATIVE inequality only — no absolute tolerance, hence
    /// no numeric-bound premise.
    ///
    /// The displacement membrane spuriously couples director rotation into
    /// membrane strain on a curved element (the parasitic lock); the ANS membrane,
    /// being translation-only in covariant form at the mid-surface, carries none
    /// of it (‖ANS·u‖ = 0 for a translation-free mode), so the inequality is
    /// strict with ample margin.
    #[test]
    fn degenerate_assumed_membrane_b_filters_parasitic_membrane_on_curved_patch() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        // Pure director-rotation mode (bending): θ_i = ω at every node, u_i = 0.
        let omega = [0.01_f64, -0.02, 0.015];
        let mut u = [0.0_f64; 18];
        for i in 0..3 {
            u[6 * i + 3] = omega[0];
            u[6 * i + 4] = omega[1];
            u[6 * i + 5] = omega[2];
        }
        let interior = Mitc3Plus.interior_tying_points();
        let mag = |e: [f64; 3]| (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
        for tp in interior.iter().take(3) {
            let c = ShellRefCoord3::new(tp.coord.xi, tp.coord.eta, 0.0);
            let ans = mag(b_times_u(
                &degenerate_assumed_membrane_b(&nodes, &directors, &thicknesses, c),
                &u,
            ));
            let disp = mag(b_times_u(
                &degenerate_membrane_bending_b(&nodes, &directors, &thicknesses, c),
                &u,
            ));
            assert!(
                disp > 1e-6,
                "displacement membrane strain {disp} must be a meaningful parasitic lock \
                 @ ({},{})",
                tp.coord.xi,
                tp.coord.eta,
            );
            // The ANS covariant membrane is translation-only at ζ=0, so a
            // translation-free director-rotation mode produces EXACTLY zero ANS
            // membrane strain — assert that machine-zero directly (the real
            // property under test). It implies a fortiori the relative locking-cure
            // inequality ans < disp, since disp > 1e-6.
            assert!(
                ans < 1e-12,
                "ANS membrane strain {ans} must be ~machine-zero for a translation-free \
                 director-rotation mode @ ({},{}) — the parasitic lock is filtered entirely",
                tp.coord.xi,
                tp.coord.eta,
            );
        }
    }

    /// Quantitative filtering on a curved element with a MIXED translation+rotation
    /// mode — a NON-trivial companion to the pure-rotation witness above (where the
    /// ANS strain is structurally zero). The translation part is a genuine in-plane
    /// stretch (nonzero membrane strain); the rotation part is a uniform director
    /// rotation (the parasitic bending mode). Because the B operators are linear and
    /// the ANS covariant membrane is translation-only at ζ=0, this pins the locking
    /// cure with a nonzero ANS witness, without any fragile magnitude ordering of
    /// summed strain vectors:
    ///
    /// - the ANS membrane strain is genuinely NONZERO (it captures the real stretch
    ///   — the filter does not over-soften the membrane);
    /// - adding the director rotation leaves the ANS membrane strain EXACTLY
    ///   unchanged (the parasitic rotation is filtered to machine precision) — the
    ///   cure;
    /// - the displacement membrane strain DOES change when the rotation is added
    ///   (it carries the parasitic membrane the ANS field removes) — the disease.
    #[test]
    fn degenerate_assumed_membrane_b_keeps_stretch_filters_rotation_on_curved_patch() {
        let (nodes, directors, thicknesses) = tilted_fixture();
        // Translation part: an isotropic in-plane stretch u_i = s·(x_i, y_i, 0) — a
        // genuine membrane mode (nonzero ANS membrane strain).
        let s = 1.0e-3_f64;
        let mut u_trans = [0.0_f64; 18];
        for i in 0..3 {
            u_trans[6 * i] = s * nodes[i][0];
            u_trans[6 * i + 1] = s * nodes[i][1];
        }
        // Mixed mode: add a uniform director rotation θ_i = ω (the parasitic bending
        // mode) on top of the stretch.
        let omega = [0.01_f64, -0.02, 0.015];
        let mut u_mixed = u_trans;
        for i in 0..3 {
            u_mixed[6 * i + 3] = omega[0];
            u_mixed[6 * i + 4] = omega[1];
            u_mixed[6 * i + 5] = omega[2];
        }
        let mag = |e: [f64; 3]| (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
        let diff = |a: [f64; 3], b: [f64; 3]| mag([a[0] - b[0], a[1] - b[1], a[2] - b[2]]);
        let interior = Mitc3Plus.interior_tying_points();
        for tp in interior.iter().take(3) {
            let c = ShellRefCoord3::new(tp.coord.xi, tp.coord.eta, 0.0);
            let ans_b = degenerate_assumed_membrane_b(&nodes, &directors, &thicknesses, c);
            let disp_b = degenerate_membrane_bending_b(&nodes, &directors, &thicknesses, c);
            let ans_mixed = b_times_u(&ans_b, &u_mixed);
            let ans_trans = b_times_u(&ans_b, &u_trans);
            let disp_mixed = b_times_u(&disp_b, &u_mixed);
            let disp_trans = b_times_u(&disp_b, &u_trans);
            // (a) ANS captures the genuine stretch — it is not over-filtered.
            assert!(
                mag(ans_mixed) > 1e-9,
                "ANS membrane strain {} must be genuinely nonzero for a stretch mode @ ({},{})",
                mag(ans_mixed),
                tp.coord.xi,
                tp.coord.eta,
            );
            // (b) The director rotation contributes EXACTLY nothing to the ANS
            // membrane (translation-only covariant field at ζ=0) — the cure.
            assert!(
                diff(ans_mixed, ans_trans) < 1e-12,
                "ANS membrane must be unchanged by the director rotation (filtered \
                 exactly); changed by {} @ ({},{})",
                diff(ans_mixed, ans_trans),
                tp.coord.xi,
                tp.coord.eta,
            );
            // (c) The displacement membrane DOES carry the parasitic rotation — the
            // lock the ANS field removes.
            assert!(
                diff(disp_mixed, disp_trans) > 1e-9,
                "displacement membrane must carry the parasitic rotation strain (the \
                 lock); changed by only {} @ ({},{})",
                diff(disp_mixed, disp_trans),
                tp.coord.xi,
                tp.coord.eta,
            );
        }
    }
}
