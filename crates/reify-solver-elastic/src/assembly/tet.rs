//! Tetrahedral element-stiffness assembly (P1 and P2).
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8.
//!
//! # Formula
//!
//! For each tetrahedral element with reference→physical Jacobian `J(ξ)`,
//! the element-local stiffness is
//!
//! ```text
//! K_e = ∫_Ω̂ Bᵀ(ξ) D B(ξ) |det J(ξ)| dξ
//! ```
//!
//! integrated over the reference tet `Ω̂` via Gauss quadrature
//! (`element.quad_points()`). `D` is the 6×6 isotropic-elastic constitutive
//! matrix from [`crate::constitutive::IsotropicElastic`].
//!
//! # Strain-displacement matrix `B`
//!
//! `B` is a `6 × 3N` matrix that maps element-nodal displacements
//! `[u₁ₓ, u₁ᵧ, u₁ᵤ, u₂ₓ, …]ᵀ` to the **engineering-strain Voigt vector**
//! `[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]ᵀ`. The shear rows hold the
//! engineering shear strain `γ_ij = 2 ε_ij`; this convention is what lets
//! the constitutive shear-block diagonal be `μ` directly (rather than
//! `2μ`) — see [`crate::constitutive::IsotropicElastic`] for the matching
//! `D`-matrix derivation.
//!
//! For each node `i`, the three columns of `B` at DOF indices
//! `3i+0`, `3i+1`, `3i+2` (axes `x, y, z`) are:
//!
//! ```text
//!                 col 3i+0   col 3i+1   col 3i+2
//!                 (u_x)      (u_y)      (u_z)
//!               ┌                                ┐
//!  row 0 ε_xx   │ ∂N_i/∂x   0          0        │
//!  row 1 ε_yy   │ 0         ∂N_i/∂y    0        │
//!  row 2 ε_zz   │ 0         0          ∂N_i/∂z  │
//!  row 3 γ_xy   │ ∂N_i/∂y   ∂N_i/∂x    0        │
//!  row 4 γ_yz   │ 0         ∂N_i/∂z    ∂N_i/∂y  │
//!  row 5 γ_xz   │ ∂N_i/∂z   0          ∂N_i/∂x  │
//!               └                                ┘
//! ```
//!
//! Read row-by-row: each Voigt component picks up contributions from the
//! three displacement axes of node `i` according to the symmetric strain
//! tensor `ε_ij = ½ (∂u_i/∂x_j + ∂u_j/∂x_i)`, doubled in the shear rows
//! because `γ_ij = 2 ε_ij`.
//!
//! Physical-frame gradients are obtained from reference gradients via
//! `∇_x N_i = J⁻ᵀ ∇_ξ N_i`. The 3×3 inverse-transpose is computed via
//! [`crate::math::inverse_transpose_3x3`] — no external linear-algebra
//! dependency.
//!
//! # DOF ordering
//!
//! `K_e` is indexed `(3·node_idx + axis, 3·node_idx + axis)` with
//! `axis ∈ {0, 1, 2}` for `(u_x, u_y, u_z)`. See the
//! [`crate::assembly::ElementStiffness`] doc for the row-major storage
//! contract.

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;
use crate::elements::{ReferenceElement, tet_p1::TetP1, tet_p2::TetP2};
use crate::math::{MIN_JACOBIAN_DET, inverse_transpose_3x3};

/// Generic element-stiffness assembly: `K_e = ∫ BᵀDB |det J| dV` integrated
/// via the element's Gauss quadrature rule.
///
/// `phys_nodes.len()` must equal `E::N_NODES`; the resulting matrix is
/// `n_dofs × n_dofs` with `n_dofs = 3 · N_NODES`.
///
/// One-line delegate to [`element_stiffness_generic_with_d_global`] —
/// preserves the legacy `IsotropicElastic`-taking entry point exactly so
/// every existing call site keeps its bit-identical numerical behaviour
/// (foundation β C4 contract: no possibility of a reordered FP
/// accumulation when the D matrix path stays unchanged for the legacy
/// callers).
#[inline]
pub(crate) fn element_stiffness_generic<E: ReferenceElement>(
    element: &E,
    phys_nodes: &[[f64; 3]],
    material: &IsotropicElastic,
) -> ElementStiffness {
    element_stiffness_generic_with_d_global(element, phys_nodes, &material.d_matrix())
}

/// D-agnostic element-stiffness assembly primitive — the inner
/// `K_e = ∫ BᵀD_globalB |det J| dV` loop with the 6×6 D matrix passed
/// in directly rather than derived from an `IsotropicElastic`.
///
/// `phys_nodes.len()` must equal `E::N_NODES`; the resulting matrix is
/// `n_dofs × n_dofs` with `n_dofs = 3 · N_NODES`.
///
/// Uses `det.abs()` for the volume measure so mirror-flipped
/// (left-handed) node orderings still produce a non-negative strain-energy
/// integrand. Right-handed elements have `det J > 0` and
/// `det.abs() == det`.
///
/// # Foundation β role (PRD §C4)
///
/// This is the **single source of truth** for the BᵀDB inner loop. The
/// legacy `element_stiffness_generic(element, phys, &material)` delegates
/// to this function via `&material.d_matrix()`, and the per-shape
/// `element_stiffness_*_with_field` entry points (steps 10/12) delegate
/// via `&material_field.material_at(centroid).d_matrix_global()`. Both
/// paths share the same FP accumulation order, so the C4 bit-identity
/// contract reduces to "identity-frame rotate_voigt is bitwise-no-op"
/// (pinned by `rotate_voigt_identity_frame_is_bitwise_no_op` from
/// foundation α) plus this delegation.
#[allow(clippy::needless_range_loop)]
pub(crate) fn element_stiffness_generic_with_d_global<E: ReferenceElement>(
    element: &E,
    phys_nodes: &[[f64; 3]],
    d_mat: &[[f64; 6]; 6],
) -> ElementStiffness {
    assert_eq!(
        phys_nodes.len(),
        E::N_NODES,
        "phys_nodes.len() must equal E::N_NODES",
    );
    let n = E::N_NODES;
    let n_dofs = 3 * n;
    let mut k_e = ElementStiffness::zeros(n_dofs);

    // Reusable scratch buffers (one allocation per call, not per q-point).
    let mut b_cols: Vec<[f64; 6]> = vec![[0.0_f64; 6]; n_dofs];
    let mut db_cols: Vec<[f64; 6]> = vec![[0.0_f64; 6]; n_dofs];
    let mut grads_phys: Vec<[f64; 3]> = vec![[0.0_f64; 3]; n];

    for q in element.quad_points() {
        // Reference gradients ∇_ξ N_i at this q-point.
        let grads_ref = element.shape_grad_at(q.coord);
        debug_assert_eq!(grads_ref.len(), n);

        // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
        // (Inlined here rather than calling `element.jacobian(...)` so we
        // don't re-allocate `grads_ref` inside the trait default impl.)
        let mut j_mat = [[0.0_f64; 3]; 3];
        for k in 0..n {
            for i in 0..3 {
                for jj in 0..3 {
                    j_mat[i][jj] += phys_nodes[k][i] * grads_ref[k][jj];
                }
            }
        }
        let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
            - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
            + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);
        // Degenerate-element guard. `det.is_normal()` catches ±0, ±∞, NaN,
        // and subnormals; the absolute-value floor (`MIN_JACOBIAN_DET`)
        // catches the merely-tiny case where division by `det` in
        // `inverse_transpose_3x3` would inflate FP error to dominate the
        // final `K_e`. Both conditions trip a `debug_assert!` rather than
        // silently propagating `NaN` / `±∞`. PRD task #21 (diagnostics)
        // will replace this with a mesh-scale-aware degeneracy detector
        // and proper error reporting.
        debug_assert!(
            det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
            "degenerate element: |det J| = {} at quad point {:?} (must be > {} \
             and finite — see PRD task #21 for the future diagnostic path)",
            det.abs(),
            q.coord,
            MIN_JACOBIAN_DET,
        );
        let j_inv_t = inverse_transpose_3x3(&j_mat, det);

        // Push reference gradients to physical: ∇_x N_i = J⁻ᵀ · ∇_ξ N_i.
        for i in 0..n {
            for r in 0..3 {
                let mut s = 0.0;
                for c in 0..3 {
                    s += j_inv_t[r][c] * grads_ref[i][c];
                }
                grads_phys[i][r] = s;
            }
        }

        // Build B columns: b_cols[3i+α][m] = B[m][3i+α].
        // Reset all entries (previous q-point's values are stale).
        for col in b_cols.iter_mut() {
            *col = [0.0; 6];
        }
        for i in 0..n {
            let (gx, gy, gz) = (grads_phys[i][0], grads_phys[i][1], grads_phys[i][2]);
            // α = 0 (u_x): nonzero in rows 0 (ε_xx), 3 (γ_xy), 5 (γ_xz)
            b_cols[3 * i][0] = gx;
            b_cols[3 * i][3] = gy;
            b_cols[3 * i][5] = gz;
            // α = 1 (u_y): nonzero in rows 1 (ε_yy), 3 (γ_xy), 4 (γ_yz)
            b_cols[3 * i + 1][1] = gy;
            b_cols[3 * i + 1][3] = gx;
            b_cols[3 * i + 1][4] = gz;
            // α = 2 (u_z): nonzero in rows 2 (ε_zz), 4 (γ_yz), 5 (γ_xz)
            b_cols[3 * i + 2][2] = gz;
            b_cols[3 * i + 2][4] = gy;
            b_cols[3 * i + 2][5] = gx;
        }

        // db_cols[a][m] = (D · B)[m][a] = Σ_n D[m][n] · B[n][a]
        //               = Σ_n D[m][n] · b_cols[a][n].
        for a in 0..n_dofs {
            for m in 0..6 {
                let mut s = 0.0;
                for n_idx in 0..6 {
                    s += d_mat[m][n_idx] * b_cols[a][n_idx];
                }
                db_cols[a][m] = s;
            }
        }

        // K[a][b] += Σ_m B[m][a] · (DB)[m][b] · |det J| · w
        //         = Σ_m b_cols[a][m] · db_cols[b][m] · factor.
        //
        // BᵀDB is symmetric whenever D is (which the isotropic-elastic D
        // is by construction), so we accumulate only the upper triangle
        // (b ≥ a) here and mirror once after the q-point loop. This both
        // halves the inner-loop ops and guarantees `K_e` is bit-for-bit
        // symmetric (no FP drift from differing summation orders).
        let factor = det.abs() * q.weight;
        for a in 0..n_dofs {
            for b in a..n_dofs {
                let mut s = 0.0;
                for m in 0..6 {
                    s += b_cols[a][m] * db_cols[b][m];
                }
                k_e.add(a, b, s * factor);
            }
        }
    }

    // Mirror upper triangle into lower triangle. Direct `data` access
    // because `ElementStiffness::data` is `pub` and we need a true store
    // (not an `add`) — copying after the q-point sum is finished is an
    // O(n_dofs²) tail with no inner-loop work, dominated by the BᵀDB
    // accumulation above.
    for a in 0..n_dofs {
        for b in (a + 1)..n_dofs {
            let v = k_e.data[a * n_dofs + b];
            k_e.data[b * n_dofs + a] = v;
        }
    }

    k_e
}

/// Compute the 12×12 element stiffness for a P1 (linear) tetrahedron.
///
/// `phys_nodes` are the 4 vertex positions in canonical order
/// matching `TetP1::N_NODES = 4` and the reference vertex layout
/// `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`.
///
/// Quadrature: P1 uses a 1-point centroid rule (degree-1 exact); for
/// affine geometry that's exact for the constant-`B` integrand a P1
/// element produces.
pub fn element_stiffness_p1(
    phys_nodes: &[[f64; 3]; 4],
    material: &IsotropicElastic,
) -> ElementStiffness {
    element_stiffness_generic(&TetP1, &phys_nodes[..], material)
}

/// Compute the 30×30 element stiffness for a P2 (quadratic) tetrahedron.
///
/// `phys_nodes` are the 10 nodal positions in canonical Hughes/Gmsh order:
/// the 4 reference vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` followed
/// by the 6 edge-midpoint nodes in `crate::elements::tet_p2::EDGES` order
/// `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.
///
/// # Quadrature
///
/// Uses the 4-point Stroud rule from [`TetP2::quad_points`] (degree-2
/// exact). For **straight-edge** P2 elements the geometric Jacobian is
/// constant per element, so the BᵀDB integrand is degree-2 in reference
/// coordinates and Stroud integrates it exactly — see the rationale in
/// `crates/reify-solver-elastic/src/elements/tet_p2.rs:31-36`.
///
/// **Curved-edge** P2 (where the edge-midpoint nodes are nudged off the
/// straight midpoint to follow a curved boundary) yields a non-constant
/// Jacobian and would need the 11-point degree-4 rule; that case is
/// deferred to v0.4+ per the crate-level scope note in `lib.rs:19-21`.
pub fn element_stiffness_p2(
    phys_nodes: &[[f64; 3]; 10],
    material: &IsotropicElastic,
) -> ElementStiffness {
    element_stiffness_generic(&TetP2, &phys_nodes[..], material)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::hex::element_stiffness_hex_p1;
    use crate::assembly::test_support::{
        dimensionless_steel_like, linf, matvec, scaled_p2_phys_nodes,
        scaled_unit_hex_phys_nodes, scaled_unit_wedge_phys_nodes, strain_energies,
    };
    use crate::assembly::wedge::element_stiffness_wedge_p1;
    use crate::elements::{hex_p1::HexP1, wedge_p1::WedgeP1};

    /// Canonical unit reference tet: vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference-tet volume 1/6.
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    // ── Step 7 RED: D-agnostic primitive bit-identity regression ────────────

    /// `element_stiffness_generic_with_d_global(element, phys, &d_mat)` must
    /// return a `Vec<f64>` bitwise equal to the existing
    /// `element_stiffness_*(phys, &material)` path for all four shapes.
    /// This is the load-bearing pin for the C4 contract: the new path
    /// shares the BᵀDB inner loop verbatim with the legacy path.
    #[test]
    fn element_stiffness_generic_with_d_global_matches_isotropic_path_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let d_mat = mat.d_matrix();

        // TetP1 row
        {
            let legacy = element_stiffness_p1(&UNIT_TET_P1, &mat);
            let via_d = element_stiffness_generic_with_d_global(&TetP1, &UNIT_TET_P1[..], &d_mat);
            assert_eq!(via_d.n_dofs, legacy.n_dofs, "TetP1 n_dofs mismatch");
            for (i, (got, want)) in via_d.data.iter().zip(legacy.data.iter()).enumerate() {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "TetP1: K[{i}] = {got} must equal {want} bitwise (D-agnostic primitive must share inner loop)",
                );
            }
        }
        // TetP2 row
        {
            let phys = scaled_p2_phys_nodes(1.0);
            let legacy = element_stiffness_p2(&phys, &mat);
            let via_d = element_stiffness_generic_with_d_global(&TetP2, &phys[..], &d_mat);
            assert_eq!(via_d.n_dofs, legacy.n_dofs, "TetP2 n_dofs mismatch");
            for (i, (got, want)) in via_d.data.iter().zip(legacy.data.iter()).enumerate() {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "TetP2: K[{i}] = {got} must equal {want} bitwise",
                );
            }
        }
        // HexP1 row
        {
            let phys = scaled_unit_hex_phys_nodes(1.0);
            let legacy = element_stiffness_hex_p1(&phys, &mat);
            let via_d = element_stiffness_generic_with_d_global(&HexP1, &phys[..], &d_mat);
            assert_eq!(via_d.n_dofs, legacy.n_dofs, "HexP1 n_dofs mismatch");
            for (i, (got, want)) in via_d.data.iter().zip(legacy.data.iter()).enumerate() {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "HexP1: K[{i}] = {got} must equal {want} bitwise",
                );
            }
        }
        // WedgeP1 row
        {
            let phys = scaled_unit_wedge_phys_nodes(1.0);
            let legacy = element_stiffness_wedge_p1(&phys, &mat);
            let via_d = element_stiffness_generic_with_d_global(&WedgeP1, &phys[..], &d_mat);
            assert_eq!(via_d.n_dofs, legacy.n_dofs, "WedgeP1 n_dofs mismatch");
            for (i, (got, want)) in via_d.data.iter().zip(legacy.data.iter()).enumerate() {
                assert_eq!(
                    got.to_bits(),
                    want.to_bits(),
                    "WedgeP1: K[{i}] = {got} must equal {want} bitwise",
                );
            }
        }
    }

    #[test]
    fn p1_returns_12_by_12_stiffness() {
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        assert_eq!(k.n_dofs, 12);
        assert_eq!(k.data.len(), 144);
    }

    #[test]
    fn p1_is_symmetric() {
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for i in 0..12 {
            for j in 0..12 {
                let lhs = k.get(i, j);
                let rhs = k.get(j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-10 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn p1_has_rigid_body_translation_null_space() {
        // For each axis ∈ {0, 1, 2}, the 12-vector u with
        // u[3·k + axis] = 1 ∀k is a uniform translation; K·u must vanish.
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut u = vec![0.0; 12];
            for node in 0..4 {
                u[3 * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-9,
                "axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p1_has_rigid_body_rotation_null_space() {
        // For each axis ω ∈ {ê_x, ê_y, ê_z}, build u_i = ω × x_i (using
        // node phys-coords). Such infinitesimal rotations produce zero
        // strain and must lie in K's kernel.
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut omega = [0.0_f64; 3];
            omega[axis] = 1.0;
            let mut u = vec![0.0; 12];
            for (node, x) in UNIT_TET_P1.iter().enumerate() {
                // ω × x  =  (ω_y x_z − ω_z x_y, ω_z x_x − ω_x x_z, ω_x x_y − ω_y x_x)
                u[3 * node] = omega[1] * x[2] - omega[2] * x[1];
                u[3 * node + 1] = omega[2] * x[0] - omega[0] * x[2];
                u[3 * node + 2] = omega[0] * x[1] - omega[1] * x[0];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-9,
                "ω-axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p1_strain_energy_patch_test_matches_normal_strain_mode() {
        // Linear displacement u(x) = A·x with A = diag(a, b, c); the
        // resulting strain field is constant (ε_xx = a, ε_yy = b, ε_zz = c,
        // shears zero), so for a P1 tet with linear shapes the FE
        // strain energy must equal the analytical 0.5 εᵀDε V exactly
        // (modulo FP).
        let (a, b, c) = (0.01, -0.005, 0.003);
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();
        let k = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let mut u = vec![0.0; 12];
        for (node_idx, x) in UNIT_TET_P1.iter().enumerate() {
            // (A · x)[axis] = A_axis_axis · x[axis] for diagonal A
            u[3 * node_idx] = a * x[0];
            u[3 * node_idx + 1] = b * x[1];
            u[3 * node_idx + 2] = c * x[2];
        }
        let eps_voigt = [a, b, c, 0.0, 0.0, 0.0];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    #[test]
    fn p1_strain_energy_patch_test_matches_pure_shear_mode() {
        // Linear displacement u_x = (s/2) y, u_y = (s/2) x, u_z = 0
        // ⇒ ε_xx = ε_yy = ε_zz = 0, ε_xy = s/2, γ_xy = 2 ε_xy = s.
        // ε_voigt = [0, 0, 0, s, 0, 0].
        let s = 0.004;
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();
        let k = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let mut u = vec![0.0; 12];
        for (node_idx, x) in UNIT_TET_P1.iter().enumerate() {
            u[3 * node_idx] = 0.5 * s * x[1];
            u[3 * node_idx + 1] = 0.5 * s * x[0];
            u[3 * node_idx + 2] = 0.0;
        }
        let eps_voigt = [0.0, 0.0, 0.0, s, 0.0, 0.0];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    #[test]
    fn p2_returns_30_by_30_stiffness() {
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        assert_eq!(k.n_dofs, 30);
        assert_eq!(k.data.len(), 900);
    }

    #[test]
    fn p2_is_symmetric() {
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        for i in 0..30 {
            for j in 0..30 {
                let lhs = k.get(i, j);
                let rhs = k.get(j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn p2_has_rigid_body_translation_null_space() {
        // u[3·k + axis] = 1 for all 10 nodes is a rigid-body translation;
        // K·u must vanish.
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut u = vec![0.0; 30];
            for node in 0..10 {
                u[3 * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-8,
                "axis {axis}: ‖K·u‖_∞ = {} (expected <1e-8)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p2_has_rigid_body_rotation_null_space() {
        // Build u_i = ω × (x_i − c) about the centroid c = (0.25, 0.25, 0.25)
        // for each ω ∈ {ê_x, ê_y, ê_z}. Linear-in-x displacements live in
        // the P2 basis exactly, so rigid rotations sit in K's kernel.
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        let centroid = [0.25_f64, 0.25, 0.25];
        for axis in 0..3 {
            let mut omega = [0.0_f64; 3];
            omega[axis] = 1.0;
            let mut u = vec![0.0; 30];
            for (node, x) in phys.iter().enumerate() {
                let r = [x[0] - centroid[0], x[1] - centroid[1], x[2] - centroid[2]];
                u[3 * node] = omega[1] * r[2] - omega[2] * r[1];
                u[3 * node + 1] = omega[2] * r[0] - omega[0] * r[2];
                u[3 * node + 2] = omega[0] * r[1] - omega[1] * r[0];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-8,
                "ω-axis {axis}: ‖K·u‖_∞ = {} (expected <1e-8)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p2_strain_energy_patch_test_matches_full_six_component_strain() {
        // u(x) = A·x with A symmetric ⇒ pure-strain (no rotation), with
        // ε_ij = ½(A_ij + A_ji) = A_ij. In Voigt:
        //   ε_voigt = [A_xx, A_yy, A_zz, 2 A_xy, 2 A_yz, 2 A_xz]
        // Pick all 6 entries distinct so every Voigt component is exercised.
        // Working in terms of the desired Voigt entries (a, b, c, d, e, f):
        //   A_xx = a, A_yy = b, A_zz = c,
        //   A_xy = A_yx = d/2, A_yz = A_zy = e/2, A_xz = A_zx = f/2.
        let (a, b, c, d, e_v, f) = (0.01, -0.005, 0.003, 0.002, -0.001, 0.0007);
        let big_a = [
            [a, d / 2.0, f / 2.0],
            [d / 2.0, b, e_v / 2.0],
            [f / 2.0, e_v / 2.0, c],
        ];
        let mat = dimensionless_steel_like();
        let d_mat = mat.d_matrix();
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &mat);

        let mut u = vec![0.0; 30];
        for (node_idx, x) in phys.iter().enumerate() {
            // u_i = (A · x)[i] = Σ_j A[i][j] · x[j]
            for i in 0..3 {
                let mut s = 0.0;
                for j in 0..3 {
                    s += big_a[i][j] * x[j];
                }
                u[3 * node_idx + i] = s;
            }
        }
        let eps_voigt = [a, b, c, d, e_v, f];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d_mat, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    #[test]
    fn p1_volume_scaling_doubles_stiffness_when_edge_length_doubles() {
        // K ∝ L for isotropic linear-elastic affine maps: B ∝ 1/L
        // (gradients scale inversely with mesh size), and dV ∝ L³, so
        // BᵀDB·dV ∝ L. Doubling all node coordinates from the unit tet
        // therefore exactly doubles every entry of K_e.
        let mat = dimensionless_steel_like();
        let k_unit = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let scaled: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let k_scaled = element_stiffness_p1(&scaled, &mat);

        for i in 0..12 {
            for j in 0..12 {
                let unit: f64 = k_unit.get(i, j);
                let got: f64 = k_scaled.get(i, j);
                let expected: f64 = 2.0 * unit;
                let scale = expected.abs().max(unit.abs()).max(1.0);
                assert!(
                    (got - expected).abs() < 1e-10 * scale,
                    "K_scaled[{i}][{j}] = {got} (expected 2·K_unit = {expected})",
                );
            }
        }
    }

    #[test]
    fn p2_volume_scaling_doubles_stiffness_when_edge_length_doubles() {
        // P2 mirror of the P1 volume-scaling test. P2 has a different
        // shape-gradient pattern and uses a 4-point Stroud quadrature, so
        // the same K ∝ L scaling is the more meaningful regression check
        // here: a bug in the inverse-Jacobian × reference-gradient pipeline
        // could pass the patch tests on a unit tet (which they all use)
        // while still failing on a uniformly scaled tet.
        let mat = dimensionless_steel_like();
        let k_unit = element_stiffness_p2(&scaled_p2_phys_nodes(1.0), &mat);
        let k_scaled = element_stiffness_p2(&scaled_p2_phys_nodes(2.0), &mat);

        for i in 0..30 {
            for j in 0..30 {
                let unit: f64 = k_unit.get(i, j);
                let got: f64 = k_scaled.get(i, j);
                let expected: f64 = 2.0 * unit;
                let scale = expected.abs().max(unit.abs()).max(1.0);
                assert!(
                    (got - expected).abs() < 1e-9 * scale,
                    "K_scaled[{i}][{j}] = {got} (expected 2·K_unit = {expected})",
                );
            }
        }
    }

    #[test]
    fn p1_strain_energy_patch_test_holds_on_left_handed_fixture() {
        // Exercises the `det.abs()` branch in element_stiffness_generic.
        // Swapping two vertices of the unit tet flips its orientation so
        // det(J) is negative. Strain energy is a scalar invariant of the
        // node ordering once the displacement field is expressed in the
        // matching DOF order, so U_K must still equal 0.5·εᵀDε·V — if
        // `.abs()` were dropped (or det used directly) the integrand
        // would carry the negative sign and U_K would come back as
        // −U_analytical, which this assertion rejects.
        let (a, b, c) = (0.01, -0.005, 0.003);
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();

        // Swap nodes 1 ↔ 2 to obtain a left-handed ordering.
        let flipped: [[f64; 3]; 4] = [
            UNIT_TET_P1[0],
            UNIT_TET_P1[2],
            UNIT_TET_P1[1],
            UNIT_TET_P1[3],
        ];
        let k = element_stiffness_p1(&flipped, &mat);

        let mut u = vec![0.0; 12];
        for (node_idx, x) in flipped.iter().enumerate() {
            u[3 * node_idx] = a * x[0];
            u[3 * node_idx + 1] = b * x[1];
            u[3 * node_idx + 2] = c * x[2];
        }
        let eps_voigt = [a, b, c, 0.0, 0.0, 0.0];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
        // Sanity: U_K is positive — guards against the failure mode where
        // `.abs()` is dropped and the patch-test difference happens to
        // round to zero by symmetry.
        assert!(u_k > 0.0, "expected U_K > 0 on physical strain, got {u_k}");
    }
}
