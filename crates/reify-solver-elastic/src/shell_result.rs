// shell_result.rs — Rust runtime container for the structured shell stress
// result (PRD task T16, `docs/prds/v0_4/structural-analysis-shells.md` §
// "Stress through thickness").
//
// Sibling to the stdlib-level `ShellStress` structure_def declared at
// `crates/reify-compiler/stdlib/solver_elastic.ri:366` (std/solver/elastic).
// Both definitions must stay shape-aligned (top/mid/bottom); if a future task
// adds a fourth layer, update both sides together. Engine-integration tasks
// T18-T20 will add a cross-assertion once they consume both sides. This
// file ships the data-only contract (define the shape + tet constructor);
// engine-integration tasks T18-T20 are responsible for actually populating
// these fields from the MITC3 kernel and wiring the `to_global(stress,
// frame)` dispatch helper.

use crate::constitutive::IsotropicElastic;
use crate::shell_assembly::{build_shell_frame, plane_stress_d};
use reify_ir::Value;

/// Per-element Cauchy stress tensors in the local mid-surface frame for a
/// MITC3 shell element.
///
/// Each field is a full 3×3 symmetric stress tensor in the element's local
/// coordinate frame (e₁, e₂, e₃):
/// - index 0 ↔ e₁ (first in-plane direction)
/// - index 1 ↔ e₂ (second in-plane direction)
/// - index 2 ↔ e₃ (out-of-plane / thickness direction)
///
/// # Through-thickness layers
///
/// - `top`    — stress at z = +t/2 (outer fibre in the local-e₃ direction).
/// - `mid`    — stress at z = 0 (neutral plane / mid-surface).
/// - `bottom` — stress at z = −t/2 (inner fibre, opposite to `top`).
///
/// In-plane components (indices [0][0], [1][1], [0][1]/[1][0]) vary linearly
/// through thickness (membrane + bending).  Transverse-shear components
/// ([0][2]/[2][0], [1][2]/[2][1]) are uniform across the three layers
/// (Reissner-Mindlin first-order, κ = 5/6 correction folded in).
/// σ_zz ([2][2]) is zero (plane-stress assumption).
///
/// # To-global transform (T18-T20)
///
/// Use [`crate::shell_assembly::ShellFrame::local_to_global`] to obtain the
/// local-to-global rotation matrix F, then `σ_global = F · σ_local · Fᵀ` for each layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellElementStress {
    /// Stress tensor at z = +t/2 (top / outer fibre), local frame.
    pub top: [[f64; 3]; 3],
    /// Stress tensor at z = 0 (mid-surface / neutral plane), local frame.
    pub mid: [[f64; 3]; 3],
    /// Stress tensor at z = −t/2 (bottom / inner fibre), local frame.
    pub bottom: [[f64; 3]; 3],
}

/// Recover the Cauchy stress tensors at the top, mid, and bottom surfaces of a
/// MITC3 shell element in the element's local coordinate frame.
///
/// # Arguments
///
/// - `nodes` — three physical vertex positions in global coordinates.
/// - `thickness` — constant shell thickness `t > 0`.
/// - `material` — isotropic linear-elastic constitutive law.
/// - `u_global` — 18-DOF global displacement vector; DOF layout is
///   `6·node + i` with i ∈ {0..5} for `(u_x, u_y, u_z, θ_x, θ_y, θ_z)`.
///
/// # Returns
///
/// A [`ShellElementStress`] with in-plane stress varying linearly through
/// thickness (membrane + bending at z = ±t/2 and 0) and transverse-shear
/// uniform across layers (Reissner-Mindlin, κ = 5/6).  σ_zz = 0 everywhere.
///
/// # Panics
///
/// Panics if `nodes` are degenerate (same as [`build_shell_frame`]).
#[allow(clippy::needless_range_loop)]
pub fn shell_element_stress(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
    u_global: &[f64; 18],
) -> ShellElementStress {
    use crate::elements::mitc3_plus::{Mitc3Plus, TyingShears};

    let frame = build_shell_frame(nodes);
    let r = frame.r; // rows = local basis in global coords: R · v_global = v_local
    let t = thickness;

    // --- Rotate 18 global DOFs → local frame (6 blocks of 3 DOFs) ---
    // Block order per node: translations (6n+0..2) then rotations (6n+3..5).
    let mut u_loc = [0.0_f64; 18];
    let n_nodes = Mitc3Plus::N_NODES; // 3
    let ndp = Mitc3Plus::N_DOFS_PER_NODE; // 6
    for node in 0..n_nodes {
        for triple in 0..2 {
            let off = ndp * node + 3 * triple;
            let vg = [u_global[off], u_global[off + 1], u_global[off + 2]];
            for i in 0..3 {
                u_loc[off + i] = r[i][0] * vg[0] + r[i][1] * vg[1] + r[i][2] * vg[2];
            }
        }
    }

    // Shared kinematics: local 2D coords, shape gradients, B_cov, J2⁻ᵀ.
    let kin = crate::shell_kinematics::shell_kinematics(nodes, &frame);
    let dn = kin.dn;

    // --- Membrane Voigt strain: ε = [ε_xx, ε_yy, γ_xy] ---
    let mut eps = [0.0_f64; 3];
    for i in 0..n_nodes {
        let ux = u_loc[ndp * i]; // u_x in local frame
        let uy = u_loc[ndp * i + 1]; // u_y in local frame
        eps[0] += dn[i][0] * ux; // ε_xx
        eps[1] += dn[i][1] * uy; // ε_yy
        eps[2] += dn[i][1] * ux + dn[i][0] * uy; // γ_xy
    }

    // --- Plane-stress Voigt stress from membrane strain ---
    let d_pl = plane_stress_d(material);
    let mut sv_mem = [0.0_f64; 3]; // σ_voigt_membrane
    for p in 0..3 {
        for q in 0..3 {
            sv_mem[p] += d_pl[p][q] * eps[q];
        }
    }

    // --- Curvature Voigt vector: κ = [κ_xx, κ_yy, 2κ_xy] from rotation DOFs ---
    // κ_xx = −∂θ_y/∂x, κ_yy = +∂θ_x/∂y, 2κ_xy = ∂θ_x/∂x − ∂θ_y/∂y
    // (matches bending B-matrix convention in shell_assembly.rs)
    let mut kappa = [0.0_f64; 3];
    for i in 0..n_nodes {
        let tx = u_loc[ndp * i + 3]; // θ_x in local frame
        let ty = u_loc[ndp * i + 4]; // θ_y in local frame
        kappa[0] += -dn[i][0] * ty; // κ_xx = -∂θ_y/∂x
        kappa[1] += dn[i][1] * tx; // κ_yy = +∂θ_x/∂y
        kappa[2] += dn[i][0] * tx - dn[i][1] * ty; // 2κ_xy
    }

    // --- Bending Voigt stress: σ_bending = D_pl · κ ---
    let mut sv_bend = [0.0_f64; 3]; // σ_voigt_bending (per unit z)
    for p in 0..3 {
        for q in 0..3 {
            sv_bend[p] += d_pl[p][q] * kappa[q];
        }
    }

    // --- MITC3 transverse-shear recovery ---
    // Covariant shear B-matrix from shared kinematics helper — single source of truth.
    // See shell_kinematics::shell_kinematics for the construction.
    let b_cov = kin.b_cov_at_tying_points;

    // Sample covariant shears at each tying point from u_loc.
    let mut g_cov_tp = [[0.0_f64; 2]; 3]; // [tp][xi/eta]
    for tp_idx in 0..3 {
        for comp in 0..2 {
            for dof in 0..18 {
                g_cov_tp[tp_idx][comp] += b_cov[tp_idx][comp][dof] * u_loc[dof];
            }
        }
    }

    // Project covariant shears at centroid (ξ=1/3, η=1/3) via MITC3.
    let centroid = crate::elements::mitc3_plus::ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0);
    let sampled = TyingShears {
        gamma_xi_zeta_at_a: g_cov_tp[0][0],
        gamma_eta_zeta_at_b: g_cov_tp[1][1],
        gamma_xi_zeta_at_c: g_cov_tp[2][0],
        gamma_eta_zeta_at_c: g_cov_tp[2][1],
    };
    let g_cov_ctr = Mitc3Plus.interpolate_assumed_shear(sampled, centroid);

    // Covariant → physical: γ_phys = J2⁻ᵀ · γ_cov (J2⁻ᵀ from shared kinematics)
    let inv_t = kin.jac2_inv_t;
    let g_phys_xz = inv_t[0][0] * g_cov_ctr.gamma_xi_zeta + inv_t[0][1] * g_cov_ctr.gamma_eta_zeta;
    let g_phys_yz = inv_t[1][0] * g_cov_ctr.gamma_xi_zeta + inv_t[1][1] * g_cov_ctr.gamma_eta_zeta;

    // Shear stresses: σ_xz = κ·G·γ_xz_phys, σ_yz = κ·G·γ_yz_phys (κ = 5/6)
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let g_mod = e / (2.0 * (1.0 + nu));
    const KAPPA: f64 = 5.0 / 6.0;
    let s_xz = KAPPA * g_mod * g_phys_xz;
    let s_yz = KAPPA * g_mod * g_phys_yz;

    // --- Assemble per-layer 3×3 stress tensors ---
    // In-plane: σ_voigt(z) = sv_mem + z·sv_bend; transverse-shear uniform.
    let make_layer = |z: f64| -> [[f64; 3]; 3] {
        let sv0 = sv_mem[0] + z * sv_bend[0]; // σ_xx(z)
        let sv1 = sv_mem[1] + z * sv_bend[1]; // σ_yy(z)
        let sv2 = sv_mem[2] + z * sv_bend[2]; // σ_xy(z) (= γ_xy(z)·G in Voigt)
        [[sv0, sv2, s_xz], [sv2, sv1, s_yz], [s_xz, s_yz, 0.0]]
    };

    ShellElementStress {
        top: make_layer(t / 2.0),
        mid: make_layer(0.0),
        bottom: make_layer(-t / 2.0),
    }
}

/// Structured shell stress result carrying per-integration-layer stress
/// channels.
///
/// # Channels
///
/// - `top`    — top-surface stress (outer fibre in the element's local-z).
/// - `mid`    — mid-surface (neutral-plane) stress. For tet results all three
///   channels are equal (no through-thickness gradient).
/// - `bottom` — bottom-surface stress (inner fibre opposite to `top`).
///
/// The per-element local-to-global rotation frame lives on `ElasticResult`
/// (as `frame : Real` placeholder for `Field<Point3<Length>, Matrix<3,3,Real>>`),
/// not on `ShellStress`. All three channels share the same per-element
/// rotation, so keeping `frame` at the `ElasticResult` level avoids
/// duplicating it across channels.
///
/// # Note on `Eq`
///
/// `PartialEq` is derived; `Eq` cannot be derived because `Value` contains
/// `f64`, which does not implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellStress {
    pub top: Value,
    pub mid: Value,
    pub bottom: Value,
}

impl ShellStress {
    /// Canonical tet-result constructor. Sets `top == mid == bottom == field`
    /// (no through-thickness stress variation for solid elements).
    ///
    /// Engine-integration tasks T18-T20 call this for every tet-element result
    /// when packaging the solver output. For shell elements they use direct
    /// struct initialisation with distinct per-layer fields.
    pub fn homogeneous(field: Value) -> Self {
        Self {
            top: field.clone(),
            mid: field.clone(),
            bottom: field,
        }
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)] // index variables drive parallel-array indexing
#[allow(clippy::identity_op)] // explicit `6 * node + dof` form mirrors the DOF layout
mod tests {
    use super::*;
    use reify_ir::Value;
    use crate::assembly::ElementStiffness;
    use crate::shell_assembly::shell_element_stiffness;

    /// Compute K · u for an 18-DOF stiffness matrix.
    fn matvec(k: &ElementStiffness, u: &[f64; 18]) -> [f64; 18] {
        let mut out = [0.0_f64; 18];
        for i in 0..18 {
            for j in 0..18 {
                out[i] += k.get(i, j) * u[j];
            }
        }
        out
    }

    // Helpers shared across shell_element_stress tests.
    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    /// Pure membrane mode (u_x at node 1 = a, u_y at node 2 = b, all rotations zero)
    /// should produce uniform stress through thickness with no curvature contribution.
    ///
    /// For UNIT_TRI, local = global frame, dN_1/dx = 1, dN_2/dy = 1, all other
    /// relevant gradients are zero.  Voigt strain = [a, b, 0], so:
    ///   σ_voigt = D_pl · [a, b, 0]
    ///
    /// Asserted: top == mid == bottom; σ_xx ≈ σ_voigt[0]; σ_yy ≈ σ_voigt[1];
    /// σ_xy = 0 (since γ_xy = 0); all σ_xz/σ_yz/σ_zz = 0.
    #[test]
    fn shell_element_stress_pure_membrane_mode_yields_uniform_through_thickness() {
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.001_f64;
        let b = -0.0005_f64;

        let mut u = [0.0_f64; 18];
        u[6 * 1 + 0] = a; // u_x at node 1
        u[6 * 2 + 1] = b; // u_y at node 2

        let s = shell_element_stress(&UNIT_TRI, t, &mat, &u);

        // Analytical stress via plane-stress D-matrix.
        let d = plane_stress_d(&mat);
        let sv0 = d[0][0] * a + d[0][1] * b; // σ_xx
        let sv1 = d[1][0] * a + d[1][1] * b; // σ_yy

        let scale = sv0.abs().max(sv1.abs()).max(1.0);
        let tol = 1e-9 * scale;

        // top, mid, bottom must be equal (no through-thickness gradient).
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (s.top[i][j] - s.mid[i][j]).abs() < tol,
                    "top[{i}][{j}] = {} ≠ mid[{i}][{j}] = {}",
                    s.top[i][j],
                    s.mid[i][j],
                );
                assert!(
                    (s.top[i][j] - s.bottom[i][j]).abs() < tol,
                    "top[{i}][{j}] = {} ≠ bottom[{i}][{j}] = {}",
                    s.top[i][j],
                    s.bottom[i][j],
                );
            }
        }

        // In-plane normal components.
        assert!(
            (s.top[0][0] - sv0).abs() < tol,
            "σ_xx = {}, expected {sv0}",
            s.top[0][0]
        );
        assert!(
            (s.top[1][1] - sv1).abs() < tol,
            "σ_yy = {}, expected {sv1}",
            s.top[1][1]
        );

        // In-plane shear and transverse components must be zero.
        assert!(
            s.top[0][1].abs() < tol,
            "σ_xy = {}, expected 0",
            s.top[0][1]
        );
        assert!(
            s.top[1][0].abs() < tol,
            "σ_yx = {}, expected 0",
            s.top[1][0]
        );
        assert!(
            s.top[0][2].abs() < tol,
            "σ_xz = {}, expected 0",
            s.top[0][2]
        );
        assert!(
            s.top[2][0].abs() < tol,
            "σ_zx = {}, expected 0",
            s.top[2][0]
        );
        assert!(
            s.top[1][2].abs() < tol,
            "σ_yz = {}, expected 0",
            s.top[1][2]
        );
        assert!(
            s.top[2][1].abs() < tol,
            "σ_zy = {}, expected 0",
            s.top[2][1]
        );
        assert!(
            s.top[2][2].abs() < tol,
            "σ_zz = {}, expected 0",
            s.top[2][2]
        );
    }

    /// Single-node θ_y = α DOF state on UNIT_TRI produces in-plane stress that is
    /// LINEAR through thickness: anti-symmetric top vs bottom, zero in-plane at mid.
    ///
    /// For UNIT_TRI, dN_1/dx = 1, so κ_xx = −∂θ_y/∂x = −α·dN_1/dx = −α,
    /// κ_yy = 2κ_xy = 0.  Analytical per-layer in-plane Voigt stress:
    ///   σ_voigt(z) = z · D_pl · [−α, 0, 0]
    ///
    /// **NOT a Kirchhoff/MITC3+ pure-bending kinematic.** A single-node θ_y also
    /// induces a non-zero MITC3+-projected transverse shear at the centroid, which
    /// this test intentionally does NOT assert.  That behaviour is pinned separately
    /// by `shell_element_stress_uniform_theta_y_yields_constant_transverse_shear`.
    ///
    /// Asserted: top[0][0] ≈ −(t/2)·α·D_pl[0][0]; bottom = −top (in-plane);
    /// mid in-plane block ≈ 0; top[0][1] ≈ 0 (no in-plane shear).
    #[test]
    fn single_node_theta_y_yields_linear_in_plane_stress_through_thickness() {
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.002_f64;

        let mut u = [0.0_f64; 18];
        u[6 * 1 + 4] = alpha; // θ_y at node 1; all translations and other rotations zero

        let s = shell_element_stress(&UNIT_TRI, t, &mat, &u);
        let d = plane_stress_d(&mat);

        // κ_xx = −α (only dN_1/dx = 1 contributes), κ_yy = 0, 2κ_xy = 0.
        // σ_bending_voigt = D_pl · [−α, 0, 0]
        let sb0 = d[0][0] * (-alpha); // σ_xx per unit z
        let sb1 = d[1][0] * (-alpha); // σ_yy per unit z (= D[0][1]·(−α))

        let scale = (sb0 * t / 2.0).abs().max((sb1 * t / 2.0).abs()).max(1.0);
        let tol = 1e-9 * scale;

        // top: z = +t/2
        assert!(
            (s.top[0][0] - sb0 * (t / 2.0)).abs() < tol,
            "top σ_xx = {}, expected {}",
            s.top[0][0],
            sb0 * (t / 2.0)
        );
        assert!(
            (s.top[1][1] - sb1 * (t / 2.0)).abs() < tol,
            "top σ_yy = {}, expected {}",
            s.top[1][1],
            sb1 * (t / 2.0)
        );
        assert!(
            s.top[0][1].abs() < tol,
            "top σ_xy = {}, expected 0",
            s.top[0][1]
        );
        assert!(
            s.top[1][0].abs() < tol,
            "top σ_yx = {}, expected 0",
            s.top[1][0]
        );

        // mid: z = 0 → in-plane stress = 0
        assert!(
            s.mid[0][0].abs() < tol,
            "mid σ_xx = {}, expected 0",
            s.mid[0][0]
        );
        assert!(
            s.mid[1][1].abs() < tol,
            "mid σ_yy = {}, expected 0",
            s.mid[1][1]
        );
        assert!(
            s.mid[0][1].abs() < tol,
            "mid σ_xy = {}, expected 0",
            s.mid[0][1]
        );

        // bottom: z = −t/2 → anti-symmetric vs top
        assert!(
            (s.bottom[0][0] + s.top[0][0]).abs() < tol,
            "bottom σ_xx + top σ_xx = {} ≠ 0",
            s.bottom[0][0] + s.top[0][0]
        );
        assert!(
            (s.bottom[1][1] + s.top[1][1]).abs() < tol,
            "bottom σ_yy + top σ_yy = {} ≠ 0",
            s.bottom[1][1] + s.top[1][1]
        );
    }

    /// Uniform θ_y = α at all nodes should produce uniform transverse shear
    /// σ_xz = κ·G·α through the thickness, with σ_yz = 0 and all in-plane
    /// components zero (partition-of-unity cancels the curvature gradient).
    ///
    /// For UNIT_TRI, jac2 = identity, so covariant = physical.  MITC3 projected
    /// shear at centroid: γ_ξζ = α (all N_i·α sum to 1·α), γ_ηζ = 0.
    /// Therefore σ_xz = (5/6)·G·α, uniform across top/mid/bottom.
    #[test]
    fn shell_element_stress_uniform_theta_y_yields_constant_transverse_shear() {
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.003_f64;

        let mut u = [0.0_f64; 18];
        for n in 0..3 {
            u[6 * n + 4] = alpha; // uniform θ_y at all nodes
        }

        let s = shell_element_stress(&UNIT_TRI, t, &mat, &u);

        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g_mod = e / (2.0 * (1.0 + nu));
        let kappa = 5.0_f64 / 6.0;
        let expected_sxz = kappa * g_mod * alpha;

        let scale = expected_sxz.abs().max(1.0);
        let tol = 1e-9 * scale;
        let tol_abs = 1e-9 * (mat.youngs_modulus * alpha * t).max(1.0); // for near-zero checks

        // σ_xz = (5/6)·G·α, uniform across all three layers.
        for (name, layer) in [("top", s.top), ("mid", s.mid), ("bottom", s.bottom)] {
            assert!(
                (layer[0][2] - expected_sxz).abs() < tol,
                "{name} σ_xz = {}, expected {expected_sxz}",
                layer[0][2]
            );
            assert!(
                (layer[2][0] - expected_sxz).abs() < tol,
                "{name} σ_zx = {}, expected {expected_sxz}",
                layer[2][0]
            );
            // σ_yz = 0
            assert!(
                layer[1][2].abs() < tol,
                "{name} σ_yz = {}, expected 0",
                layer[1][2]
            );
            assert!(
                layer[2][1].abs() < tol,
                "{name} σ_zy = {}, expected 0",
                layer[2][1]
            );
            // σ_zz = 0
            assert!(
                layer[2][2].abs() < tol_abs,
                "{name} σ_zz = {}, expected 0",
                layer[2][2]
            );
            // In-plane block = 0 (uniform θ_y → zero curvature via partition of unity).
            assert!(
                layer[0][0].abs() < tol_abs,
                "{name} σ_xx = {}, expected 0",
                layer[0][0]
            );
            assert!(
                layer[1][1].abs() < tol_abs,
                "{name} σ_yy = {}, expected 0",
                layer[1][1]
            );
            assert!(
                layer[0][1].abs() < tol_abs,
                "{name} σ_xy = {}, expected 0",
                layer[0][1]
            );
        }
    }

    /// `ShellStress::homogeneous(field)` is the canonical tet-result constructor.
    /// It must set all three stress channels to the same field value.
    ///
    /// This test pins the tet-result population contract:
    ///   result.top    == input field
    ///   result.mid    == input field
    ///   result.bottom == input field
    #[test]
    fn shell_stress_homogeneous_replicates_field_across_channels() {
        let field = Value::Real(42.0);
        let result = ShellStress::homogeneous(field.clone());

        assert_eq!(
            result.top, field,
            "homogeneous: top should equal the input field"
        );
        assert_eq!(
            result.mid, field,
            "homogeneous: mid should equal the input field"
        );
        assert_eq!(
            result.bottom, field,
            "homogeneous: bottom should equal the input field"
        );
    }

    /// Explicit construction must preserve distinct per-channel values, proving
    /// that `ShellStress` can represent the fully differentiated per-layer
    /// stress distribution produced by the MITC3 shell kernel.
    ///
    /// This test pins the explicit/per-channel shape needed for shell results:
    /// each of top/mid/bottom round-trips through the struct unchanged.
    #[test]
    fn shell_stress_explicit_construction_preserves_per_channel_values() {
        let top = Value::Real(1.0);
        let mid = Value::Real(2.0);
        let bottom = Value::Real(3.0);

        let result = ShellStress {
            top: top.clone(),
            mid: mid.clone(),
            bottom: bottom.clone(),
        };

        assert_eq!(result.top, top, "explicit: top round-trips");
        assert_eq!(result.mid, mid, "explicit: mid round-trips");
        assert_eq!(result.bottom, bottom, "explicit: bottom round-trips");
    }

    /// Locks the membrane path of `shell_element_stiffness` and `shell_element_stress`
    /// together: `B_mᵀ · σ_voigt_membrane(u) · area · t` (the membrane internal-force
    /// residual computed via the stress kernel) must equal the in-plane components
    /// of `K · u` (the same residual computed via the stiffness kernel) for any DOF
    /// vector `u`. Variational consistency: `K_m = ∫ B_mᵀ D B_m dV ⇒
    /// K_m · u_inplane = B_mᵀ · (D · ε(u_inplane)) · area · t = B_mᵀ · σ_voigt · area · t`.
    ///
    /// Run on UNIT_TRI (R = identity ⇒ K_global = K_local, u_loc = u, σ_local = σ_global).
    /// Drives the test with 5 deterministic pseudo-random 18-DOF vectors so a
    /// future divergence in either kernel's kinematics — sign flip, tying-point
    /// re-ordering, dn formula change — surfaces here.
    ///
    /// For UNIT_TRI the in-plane K rows (6n+0, 6n+1) receive contributions only
    /// from the membrane block: bending activates {θ_x, θ_y} DOFs and shear
    /// activates {u_z, θ_x, θ_y} DOFs — neither writes into the u_x / u_y rows.
    #[test]
    fn shell_membrane_residual_locks_stiffness_and_stress_paths() {
        let mat = steel_like();
        let t = 0.05_f64;
        let area = 0.5_f64; // UNIT_TRI area

        // Build K once.
        let k = shell_element_stiffness(&UNIT_TRI, t, &mat);

        // dn for UNIT_TRI (two_a = 1): [[-1,-1],[1,0],[0,1]]
        // These are the closed-form P1 shape-function gradients in local coords.
        let dn: [[f64; 2]; 3] = [[-1.0_f64, -1.0], [1.0, 0.0], [0.0, 1.0]];

        // Deterministic LCG (Knuth multiplicative): maps to small displacements ~1e-3
        // so elastic regime applies and FP scales stay sane.
        let mut lcg: u64 = 0xDEAD_BEEF_u64;
        let mut next_val = || -> f64 {
            lcg = lcg
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let bits = (lcg >> 11) as f64;
            (bits / (1u64 << 53) as f64 - 0.5) * 2e-3
        };

        for trial in 0..5_usize {
            let mut u = [0.0_f64; 18];
            for dof in 0..18 {
                u[dof] = next_val();
            }

            // K · u (stiffness path)
            let ku = matvec(&k, &u);

            // Stress at mid-surface (z=0): pure membrane stress, bending = 0.
            let s = shell_element_stress(&UNIT_TRI, t, &mat, &u);
            let sv_mem = [s.mid[0][0], s.mid[1][1], s.mid[0][1]];

            // For each node, B_mᵀ · σ_voigt · area · t must equal K·u at in-plane DOFs.
            // B_m sub-block for node n (3×2):
            //   [ dn[n][0],    0    ]
            //   [    0   ,  dn[n][1] ]
            //   [ dn[n][1],  dn[n][0] ]
            // B_mᵀ · σ_voigt:
            //   x-component: dn[n][0]*σ_xx + dn[n][1]*σ_xy
            //   y-component: dn[n][1]*σ_yy + dn[n][0]*σ_xy
            for n in 0..3 {
                let resid_x = (dn[n][0] * sv_mem[0] + dn[n][1] * sv_mem[2]) * area * t;
                let resid_y = (dn[n][1] * sv_mem[1] + dn[n][0] * sv_mem[2]) * area * t;

                let ku_x = ku[6 * n];
                let ku_y = ku[6 * n + 1];

                let scale_x = resid_x.abs().max(ku_x.abs()).max(1e-30);
                let scale_y = resid_y.abs().max(ku_y.abs()).max(1e-30);

                assert!(
                    (ku_x - resid_x).abs() < 1e-9 * scale_x,
                    "trial={trial} node={n}: K·u[x]={ku_x:.6e}, \
                     B_mᵀ·σ·A·t={resid_x:.6e}, diff={:.6e}",
                    (ku_x - resid_x).abs()
                );
                assert!(
                    (ku_y - resid_y).abs() < 1e-9 * scale_y,
                    "trial={trial} node={n}: K·u[y]={ku_y:.6e}, \
                     B_mᵀ·σ·A·t={resid_y:.6e}, diff={:.6e}",
                    (ku_y - resid_y).abs()
                );
            }
        }
    }

    /// Zero-DOF regression guard (relocated from lib.rs doctest and strengthened).
    ///
    /// With all 18 DOFs set to zero, every strain/stress accumulation in
    /// `shell_element_stress` reduces to a sum of `coeff * 0.0` terms, so each
    /// output component must be bit-exact 0.0 (no floating-point rounding).
    ///
    /// Asserts every component of all three layers (`top`, `mid`, `bottom`) via
    /// the derived `PartialEq`, catching σ_xx/σ_yy/σ_xy/σ_xz/σ_yz/σ_zz
    /// regressions — not just [0][0] as the old lib.rs doctest did.
    #[test]
    fn shell_element_stress_zero_dofs_yields_all_zero_stress() {
        let s = shell_element_stress(&UNIT_TRI, 0.05, &steel_like(), &[0.0_f64; 18]);
        assert_eq!(s.top, [[0.0_f64; 3]; 3], "zero-DOF top layer must be all 0.0");
        assert_eq!(s.mid, [[0.0_f64; 3]; 3], "zero-DOF mid layer must be all 0.0");
        assert_eq!(s.bottom, [[0.0_f64; 3]; 3], "zero-DOF bottom layer must be all 0.0");
    }
}
