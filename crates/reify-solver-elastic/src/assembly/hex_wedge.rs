//! Element-stiffness assembly for P1 hexahedra and P1 wedges.
//!
//! Each public entry point is a thin fixed-node-count wrapper around the
//! generic integration kernel [`crate::assembly::tet::element_stiffness_generic`],
//! mirroring the `element_stiffness_p1` / `element_stiffness_p2` pattern in
//! [`crate::assembly::tet`] for tetrahedra.

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Compute the 24×24 element stiffness for a P1 (trilinear) hexahedron.
///
/// `phys_nodes` are the 8 vertex positions in canonical Hughes/Gmsh hex8 order
/// (same sign-pattern ordering as `crate::elements::hex_p1::VERTEX_SIGNS`):
///
/// | node | ξ  | η  | ζ  |
/// |------|----|----|----|
/// | 0    | −1 | −1 | −1 |
/// | 1    | +1 | −1 | −1 |
/// | 2    | +1 | +1 | −1 |
/// | 3    | −1 | +1 | −1 |
/// | 4    | −1 | −1 | +1 |
/// | 5    | +1 | −1 | +1 |
/// | 6    | +1 | +1 | +1 |
/// | 7    | −1 | +1 | +1 |
///
/// **Quadrature**: 2×2×2 Gauss-Legendre rule (8 points), degree-3-per-axis exact
/// — sufficient for constant-strain modes on a constant-Jacobian hex.
///
/// See [`crate::assembly::tet::element_stiffness_generic`] for the BᵀDB integrand
/// and [`IsotropicElastic::d_matrix`] for the engineering-strain Voigt convention
/// (shear-block diagonal = μ, not 2μ).
pub fn element_stiffness_hex_p1(
    phys_nodes: &[[f64; 3]; 8],
    material: &IsotropicElastic,
) -> ElementStiffness {
    crate::assembly::tet::element_stiffness_generic(
        &crate::elements::hex_p1::HexP1,
        &phys_nodes[..],
        material,
    )
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::test_support::scaled_unit_hex_phys_nodes;
    use crate::constitutive::IsotropicElastic;

    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// K · u for a flat-row-major K of size n × n.
    fn matvec(k: &crate::assembly::ElementStiffness, u: &[f64]) -> Vec<f64> {
        assert_eq!(k.n_dofs, u.len());
        let n = k.n_dofs;
        let mut out = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                out[i] += k.get(i, j) * u[j];
            }
        }
        out
    }

    /// L∞ norm of a slice.
    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    /// Compute U_K = 0.5 · u' K u and U_analytical = 0.5 · ε' D ε · V.
    fn strain_energies(
        k: &crate::assembly::ElementStiffness,
        u: &[f64],
        eps_voigt: &[f64; 6],
        d: &[[f64; 6]; 6],
        volume: f64,
    ) -> (f64, f64) {
        let ku = matvec(k, u);
        let mut u_dot_ku = 0.0;
        for i in 0..u.len() {
            u_dot_ku += u[i] * ku[i];
        }
        let u_k = 0.5 * u_dot_ku;

        let mut d_eps = [0.0_f64; 6];
        for i in 0..6 {
            for j in 0..6 {
                d_eps[i] += d[i][j] * eps_voigt[j];
            }
        }
        let mut eps_dot_d_eps = 0.0;
        for i in 0..6 {
            eps_dot_d_eps += eps_voigt[i] * d_eps[i];
        }
        (u_k, 0.5 * eps_dot_d_eps * volume)
    }

    // ── (a) Output dimensions ────────────────────────────────────────────────

    #[test]
    fn hex_p1_returns_24_by_24_stiffness() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);
        assert_eq!(k.n_dofs, 24, "hex K_e must be 24×24 (8 nodes × 3 axes)");
        assert_eq!(k.data.len(), 576, "hex K_e data must have 576 entries");
    }

    // ── (b) Symmetry ─────────────────────────────────────────────────────────

    #[test]
    fn hex_p1_is_symmetric() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);
        for i in 0..24 {
            for j in 0..24 {
                let lhs = k.get(i, j);
                let rhs = k.get(j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): K[i][j]={lhs} vs K[j][i]={rhs}",
                );
            }
        }
    }

    // ── (c) Rigid-body translation null space ────────────────────────────────

    #[test]
    fn hex_p1_has_rigid_body_translation_null_space() {
        // u[3·k + axis] = 1 for all 8 nodes is a rigid-body translation;
        // K·u must vanish for each axis.
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);
        for axis in 0..3 {
            let mut u = vec![0.0; 24];
            for node in 0..8 {
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

    // ── (d) Rigid-body rotation null space ──────────────────────────────────

    #[test]
    fn hex_p1_has_rigid_body_rotation_null_space() {
        // Centroid of hex on [−1,1]³ is the origin (by symmetry).
        // For each ω ∈ {ê_x, ê_y, ê_z}, build u_i = ω × x_i. This is a
        // linear displacement field that lives in the hex8 basis exactly,
        // producing zero strain, so K·u must vanish.
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);
        for axis in 0..3 {
            let mut omega = [0.0_f64; 3];
            omega[axis] = 1.0;
            let mut u = vec![0.0; 24];
            for (node, x) in phys.iter().enumerate() {
                // u_i = ω × x_i  (centroid = origin)
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

    // ── (e) Strain-energy patch test — normal-strain mode ───────────────────
    //   FAILS on the zeros stub (U_K = 0 vs U_analytical > 0).

    #[test]
    fn hex_p1_strain_energy_patch_test_matches_normal_strain_mode() {
        // u(x) = diag(a,b,c)·x ⇒ ε = [a,b,c,0,0,0] (constant).
        // Shape functions reproduce linear fields exactly, so
        // U_K = 0.5 uᵀKu must equal 0.5 εᵀDε·V with V = 8 (cube [−1,1]³).
        let (a, b, c) = (0.01, -0.005, 0.003);
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);

        let mut u = vec![0.0; 24];
        for (node_idx, x) in phys.iter().enumerate() {
            u[3 * node_idx] = a * x[0];
            u[3 * node_idx + 1] = b * x[1];
            u[3 * node_idx + 2] = c * x[2];
        }
        let eps_voigt = [a, b, c, 0.0, 0.0, 0.0];
        let volume = 8.0; // (2s)³ = 8 for s=1

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    // ── (f) Strain-energy patch test — full 6-component mode ────────────────
    //   FAILS on the zeros stub (U_K = 0 vs U_analytical > 0).

    #[test]
    fn hex_p1_strain_energy_patch_test_matches_full_six_component_strain() {
        // u(x) = A·x with A symmetric, all 6 Voigt entries distinct.
        // A_xx=a, A_yy=b, A_zz=c, A_xy=A_yx=d/2, A_yz=A_zy=e/2, A_xz=A_zx=f/2.
        // ε_voigt = [a, b, c, d, e, f].
        let (a, b, c, d, e_v, f) = (0.01, -0.005, 0.003, 0.002, -0.001, 0.0007);
        let big_a = [
            [a, d / 2.0, f / 2.0],
            [d / 2.0, b, e_v / 2.0],
            [f / 2.0, e_v / 2.0, c],
        ];
        let mat = dimensionless_steel_like();
        let d_mat = mat.d_matrix();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);

        let mut u = vec![0.0; 24];
        for (node_idx, x) in phys.iter().enumerate() {
            for i in 0..3 {
                let mut s = 0.0;
                for j in 0..3 {
                    s += big_a[i][j] * x[j];
                }
                u[3 * node_idx + i] = s;
            }
        }
        let eps_voigt = [a, b, c, d, e_v, f];
        let volume = 8.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d_mat, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    // ── (g) Volume scaling ───────────────────────────────────────────────────

    #[test]
    fn hex_p1_volume_scaling_doubles_stiffness_when_edge_length_doubles() {
        // K ∝ L: B ∝ 1/L, dV ∝ L³ ⇒ BᵀDB·dV ∝ L.
        // Doubling all node coords (s=1 → s=2) must double every K_e entry.
        let mat = dimensionless_steel_like();
        let k_unit = element_stiffness_hex_p1(&scaled_unit_hex_phys_nodes(1.0), &mat);
        let k_scaled = element_stiffness_hex_p1(&scaled_unit_hex_phys_nodes(2.0), &mat);

        for i in 0..24 {
            for j in 0..24 {
                let unit = k_unit.get(i, j);
                let got = k_scaled.get(i, j);
                let expected = 2.0 * unit;
                let scale = expected.abs().max(unit.abs()).max(1.0);
                assert!(
                    (got - expected).abs() < 1e-9 * scale,
                    "K_scaled[{i}][{j}] = {got} (expected 2·K_unit = {expected})",
                );
            }
        }
    }

    // ── (h) Left-handed orientation patch test ───────────────────────────────
    //   FAILS on the zeros stub (U_K = 0 vs U_analytical > 0).

    #[test]
    fn hex_p1_strain_energy_patch_test_holds_on_left_handed_fixture() {
        // Swap nodes 0 ↔ 6 (opposite-corner pair: both signs differ in all three
        // coordinates) to produce a left-handed hex with det J < 0 at all 8
        // Gauss points. The generic integrator uses det.abs() so the energy must
        // still equal U_analytical. Mirrors p1_strain_energy_patch_test_holds_on_left_handed_fixture
        // in tet.rs.
        //
        // Physical volume of the swapped element: for the canonical hex on [−1,1]³
        // with nodes 0↔6 swapped, det J = −(1 + ηζ + ξζ + ξη)/2 (derived via the
        // matrix-determinant lemma).  At the 8 Gauss points ±1/√3 this is either
        // −1 (at (g,g,g) and (−g,−g,−g)) or −1/3 (at the other 6), so
        // ∫|det J| dV = 2·1 + 6·(1/3) = 4.  The 2×2×2 rule integrates this
        // degree-2 polynomial exactly, confirming V_physical = 4 (not 8).
        let (a, b, c) = (0.01, -0.005, 0.003);
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();

        let mut phys = scaled_unit_hex_phys_nodes(1.0);
        phys.swap(0, 6); // flip orientation
        let k = element_stiffness_hex_p1(&phys, &mat);

        let mut u = vec![0.0; 24];
        for (node_idx, x) in phys.iter().enumerate() {
            u[3 * node_idx] = a * x[0];
            u[3 * node_idx + 1] = b * x[1];
            u[3 * node_idx + 2] = c * x[2];
        }
        let eps_voigt = [a, b, c, 0.0, 0.0, 0.0];
        // Physical volume of the swapped element = 4 (see comment above).
        let volume = 4.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
        assert!(u_k > 0.0, "expected U_K > 0 on physical strain, got {u_k}");
    }
}
