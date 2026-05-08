//! Shell-element stiffness assembly for the Reissner-Mindlin MITC3 shell.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` task T6.
//!
//! # Overview
//!
//! Computes the per-element 18×18 stiffness matrix for a three-node
//! Reissner-Mindlin shell element under a constant-thickness isotropic
//! linear-elastic constitutive law. Through-thickness integration is
//! closed-form; element K is assembled in a local mid-surface frame and then
//! rotated into the global frame so it is ready for the global sparse-assembly
//! consumer (PRD T#11). Output is a [`crate::assembly::ElementStiffness`]
//! with `n_dofs = 18`.
//!
//! # Shear-locking mitigation: MITC3 assumed-strain field
//!
//! Transverse-shear locking is eliminated via the mixed interpolation of
//! tensorial components (MITC) technique: covariant shear strains are sampled
//! at three edge-midpoint tying points (A, B, C) and blended via the affine
//! formula from `Mitc3Plus::interpolate_assumed_shear`. This corresponds to
//! the **MITC3** formulation (Bathe & Dvorkin 1985).
//!
//! # Deferred: MITC3+ cubic-bubble enrichment
//!
//! The MITC3+ element (Bathe & Lee 2014) adds a deviatoric cubic-bubble
//! rotation field to the covariant-shear sampling at the tying points, which
//! further reduces residual locking on curved or twisted geometries. That
//! enrichment is **not** wired here — the covariant shears at tying points
//! are computed from the standard three-node linear rotation field only.
//! The patch tests included pass because they exercise constant or affine
//! fields that are insensitive to the bubble. The '+' enrichment is tracked
//! as a follow-up task (PRD v0.4 T8 / curved-geometry accuracy).

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Local mid-surface coordinate frame for a MITC3+ shell element.
///
/// `r[i][j]` is the j-th global component of local basis vector `eᵢ`:
/// - `r[0]` = `e1` (along edge p0→p1, in-plane)
/// - `r[1]` = `e2` (in-plane, perpendicular to e1)
/// - `r[2]` = `e3` (outward normal, right-handed)
///
/// The transform `x_local = R · x_global` maps global vectors to local.
/// `origin` is the first node `p0`.
pub struct ShellFrame {
    /// Origin of the local frame (physical position of node 0).
    pub origin: [f64; 3],
    /// 3×3 rotation matrix: rows are the local basis vectors in global coords.
    pub r: [[f64; 3]; 3],
    /// Area of the physical triangle `= 0.5 · |(p1−p0) × (p2−p0)|`.
    pub area: f64,
}

/// Build the local mid-surface frame for a three-node shell element.
///
/// # Frame construction
///
/// - `e1 = (p1 − p0) / |p1 − p0|`
/// - `n = (p1 − p0) × (p2 − p0)` (unnormalized right-handed normal)
/// - `area = 0.5 · |n|`
/// - `e3 = n / |n|` (unit normal)
/// - `e2 = e3 × e1` (in-plane, orthogonal to e1)
///
/// The resulting frame is right-handed and orthonormal.
pub fn build_shell_frame(nodes: &[[f64; 3]; 3]) -> ShellFrame {
    let p0 = nodes[0];
    let p1 = nodes[1];
    let p2 = nodes[2];

    // Edge vectors
    let d01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let d02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

    // e1: normalize d01
    let len01 = (d01[0] * d01[0] + d01[1] * d01[1] + d01[2] * d01[2]).sqrt();
    assert!(len01 > 1e-30, "degenerate shell element: p0 == p1");
    let e1 = [d01[0] / len01, d01[1] / len01, d01[2] / len01];

    // Normal (cross product d01 × d02)
    let cx = d01[1] * d02[2] - d01[2] * d02[1];
    let cy = d01[2] * d02[0] - d01[0] * d02[2];
    let cz = d01[0] * d02[1] - d01[1] * d02[0];
    let len_n = (cx * cx + cy * cy + cz * cz).sqrt();
    debug_assert!(len_n > 1e-30, "degenerate shell element: collinear nodes");
    let area = 0.5 * len_n;

    // e3: unit normal
    let e3 = [cx / len_n, cy / len_n, cz / len_n];

    // e2 = e3 × e1
    let e2 = [
        e3[1] * e1[2] - e3[2] * e1[1],
        e3[2] * e1[0] - e3[0] * e1[2],
        e3[0] * e1[1] - e3[1] * e1[0],
    ];

    ShellFrame {
        origin: p0,
        r: [e1, e2, e3],
        area,
    }
}

/// Shear-correction factor κ = 5/6 (Reissner standard for rectangular cross-section).
///
/// Baked in as a private constant — it is a property of the through-thickness
/// shape function, not of the material. See design decision in `plan.json`.
const KAPPA: f64 = 5.0 / 6.0;

/// Plane-stress 3×3 constitutive matrix for membrane and bending.
///
/// Voigt order: `[ε_xx, ε_yy, γ_xy]` (engineering shear strain).
///
/// ```text
/// D_pl = E/(1−ν²) · ⎡ 1    ν    0        ⎤
///                    ⎢ ν    1    0        ⎥
///                    ⎣ 0    0    (1−ν)/2  ⎦
/// ```
///
/// The shear term `(1−ν)/2 · E/(1−ν²) = E/(2(1+ν)) = G` uses the engineering
/// shear strain convention, consistent with `IsotropicElastic::d_matrix`.
pub fn plane_stress_d(material: &IsotropicElastic) -> [[f64; 3]; 3] {
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    debug_assert!(
        (0.0..0.5).contains(&nu),
        "poisson_ratio must satisfy 0 ≤ ν < 0.5, got {nu}",
    );
    let factor = e / (1.0 - nu * nu);
    [
        [factor,        nu * factor,  0.0],
        [nu * factor,  factor,        0.0],
        [0.0,           0.0,           factor * (1.0 - nu) / 2.0],
    ]
}

/// Inline 3×3 matrix multiply: C = A · B.
#[inline]
fn mat3_mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut c = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                c[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    c
}

/// Compute the 18×18 element stiffness matrix for a MITC3+ shell element.
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `thickness` is the constant shell thickness `t`.
/// `material` is the isotropic linear-elastic constitutive law.
///
/// Returns an [`ElementStiffness`] with `n_dofs = 18`. DOF ordering is
/// `6 · node_idx + i` with `i ∈ {0..5}` for `(u_x, u_y, u_z, θ_x, θ_y, θ_z)`.
///
/// The drilling rotation `θ_z` (i=5) carries **zero stiffness** by
/// construction: pure MITC3 has no in-plane rotational stiffness. Every
/// drilling row and column of the returned matrix is zero, producing a zero
/// pivot on each drilling DOF in the global assembled system. The global
/// sparse-assembly consumer (PRD T#11) is responsible for handling these
/// singular directions — either by constraining drilling DOFs explicitly or
/// by adding an artificial Allman/Hughes drilling stiffness at the assembly
/// layer.
///
/// # Contributions
///
/// K = K_membrane + K_bending + K_shear, assembled in local mid-surface frame
/// then rotated into global: `K_global[a..a+3, b..b+3] = Rᵀ · K_local[...] · R`.
#[allow(clippy::needless_range_loop)]
pub fn shell_element_stiffness(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
) -> ElementStiffness {
    use crate::elements::mitc3_plus::{Mitc3Plus, ShearStrain, TyingShears};
    assert!(thickness > 0.0, "shell_element_stiffness: thickness must be positive, got {thickness}");
    // Element-size constants — avoid hard-coding 18/6/3 throughout.
    const NDOF: usize = Mitc3Plus::N_DOFS;        // 18 total DOFs
    const NDP:  usize = Mitc3Plus::N_DOFS_PER_NODE; // 6 DOFs per node
    const NN:   usize = Mitc3Plus::N_NODES;        // 3 nodes

    let frame = build_shell_frame(nodes);
    let r = frame.r;   // rotation matrix: row i = local basis eᵢ in global coords
    let area = frame.area;
    let t = thickness;
    let d_pl = plane_stress_d(material);

    // Shear modulus G and transverse-shear D scalar: κ·G
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let g = e / (2.0 * (1.0 + nu));
    let kappa_g = KAPPA * g;

    // --- Local 2D coordinates of nodes (x_loc = R · (p_i - p0)) ---
    // e3 component is zero for a flat triangle (by construction of R).
    let mut xloc = [[0.0_f64; 2]; 3]; // [node][local_x, local_y]
    for i in 0..3 {
        let d = [
            nodes[i][0] - frame.origin[0],
            nodes[i][1] - frame.origin[1],
            nodes[i][2] - frame.origin[2],
        ];
        xloc[i][0] = r[0][0]*d[0] + r[0][1]*d[1] + r[0][2]*d[2]; // x_loc
        xloc[i][1] = r[1][0]*d[0] + r[1][1]*d[1] + r[1][2]*d[2]; // y_loc
    }

    // 2D shape gradients via the standard triangle formula.
    // For nodes (x0,y0),(x1,y1),(x2,y2) with signed area A_signed = area (positive):
    //   ∂N_i/∂x = (y_j - y_k) / (2·A_signed)
    //   ∂N_i/∂y = (x_k - x_j) / (2·A_signed)
    // cyclic: i→j→k = 0→1→2→0
    let two_a = 2.0 * area;
    let x = [xloc[0][0], xloc[1][0], xloc[2][0]];
    let y = [xloc[0][1], xloc[1][1], xloc[2][1]];

    // dN[i] = [dN_i/dx, dN_i/dy] in local frame
    let dn = [
        [(y[1] - y[2]) / two_a, (x[2] - x[1]) / two_a],
        [(y[2] - y[0]) / two_a, (x[0] - x[2]) / two_a],
        [(y[0] - y[1]) / two_a, (x[1] - x[0]) / two_a],
    ];

    // --- 18×18 K_local (assembled in local frame) ---
    let mut k_loc = [[0.0_f64; NDOF]; NDOF];

    // ---- Membrane K (step 8) ----
    // B_m is 3×9 (rows: ε_xx, ε_yy, γ_xy; cols: u_x_0,u_y_0, u_x_1,u_y_1, u_x_2,u_y_2)
    // Per node i, the 2-col block in B_m is:
    //   row 0 (ε_xx):  [dN_i/dx, 0      ]
    //   row 1 (ε_yy):  [0,       dN_i/dy]
    //   row 2 (γ_xy):  [dN_i/dy, dN_i/dx]
    //
    // Global DOFs for in-plane: node i → local DOFs 6i+0 (u_x), 6i+1 (u_y)
    // K_m[a][b] += Σ_r Σ_s B_m[r][col_a] · (t·D_pl)[r][s] · B_m[s][col_b] · area
    // (1-point rule, integrand constant)
    let t_dpl = {
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 { for j in 0..3 { td[i][j] = t * d_pl[i][j]; } }
        td
    };
    for ni in 0..NN {
        for nj in 0..NN {
            // B_m columns for node i (2 cols) × B_m columns for node j (2 cols)
            // col offsets within the 9-col membrane sub-block: 2·n
            // but in local K, DOF = NDP·n + {0,1}
            let doi = [NDP*ni, NDP*ni+1]; // local DOF indices for (u_x, u_y) of node i
            let doj = [NDP*nj, NDP*nj+1];
            // B_m sub-block for node i (3×2):
            let bmi = [[dn[ni][0], 0.0], [0.0, dn[ni][1]], [dn[ni][1], dn[ni][0]]];
            let bmj = [[dn[nj][0], 0.0], [0.0, dn[nj][1]], [dn[nj][1], dn[nj][0]]];
            // K_m sub-block (2×2) for (node_i, node_j):
            // K_m_ij[a][b] = Σ_r Σ_s bmi[r][a] · t_dpl[r][s] · bmj[s][b] · area
            for a in 0..2 {
                for b in 0..2 {
                    let mut v = 0.0;
                    for r in 0..3 {
                        for s in 0..3 {
                            v += bmi[r][a] * t_dpl[r][s] * bmj[s][b];
                        }
                    }
                    k_loc[doi[a]][doj[b]] += v * area;
                }
            }
        }
    }

    // ---- Bending K (step 12, implemented here together) ----
    // B_b is 3×9 (rows: κ_xx, κ_yy, 2κ_xy; mapping per-node (θ_x, θ_y))
    // Per node i, 2-col block:
    //   row 0 (κ_xx = -∂θ_y/∂x): [0,        -dN_i/dx]
    //   row 1 (κ_yy = +∂θ_x/∂y): [+dN_i/dy,  0      ]
    //   row 2 (2κ_xy = ∂θ_x/∂x - ∂θ_y/∂y): [+dN_i/dx, -dN_i/dy]
    //
    // Global DOFs for rotations: node i → 6i+3 (θ_x), 6i+4 (θ_y)
    // K_b = B_bᵀ · (t³/12 · D_pl) · B_b · area
    let t3_12_dpl = {
        let factor = t * t * t / 12.0;
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 { for j in 0..3 { td[i][j] = factor * d_pl[i][j]; } }
        td
    };
    for ni in 0..NN {
        for nj in 0..NN {
            let doi = [NDP*ni+3, NDP*ni+4]; // θ_x, θ_y DOF indices for node i
            let doj = [NDP*nj+3, NDP*nj+4];
            // B_b sub-block (3×2) for node i:
            let bbi = [[0.0, -dn[ni][0]], [dn[ni][1], 0.0], [dn[ni][0], -dn[ni][1]]];
            let bbj = [[0.0, -dn[nj][0]], [dn[nj][1], 0.0], [dn[nj][0], -dn[nj][1]]];
            for a in 0..2 {
                for b in 0..2 {
                    let mut v = 0.0;
                    for r in 0..3 {
                        for s in 0..3 {
                            v += bbi[r][a] * t3_12_dpl[r][s] * bbj[s][b];
                        }
                    }
                    k_loc[doi[a]][doj[b]] += v * area;
                }
            }
        }
    }

    // ---- Transverse-shear K (step 10, implemented here) ----
    // MITC3+ assumed-strain interpolation.
    // Physical DOFs per node for shear: u_z (6n+2), θ_x (6n+3), θ_y (6n+4).
    //
    // Local 2D Jacobian from reference (ξ,η) to local (x_loc, y_loc):
    //   J2 = [[∂x/∂ξ, ∂x/∂η], [∂y/∂ξ, ∂y/∂η]]
    //      = [[x1-x0, x2-x0], [y1-y0, y2-y0]]
    // Inverse (for 2×2): J2⁻¹ = (1/det) · [[d, -b], [-c, a]] for [[a,b],[c,d]]
    //
    // Covariant → physical transform: γ_phys = J2⁻ᵀ · γ_cov
    let jac2 = [
        [x[1] - x[0], x[2] - x[0]],
        [y[1] - y[0], y[2] - y[0]],
    ];
    let det2 = jac2[0][0]*jac2[1][1] - jac2[0][1]*jac2[1][0];
    // J2⁻ᵀ: (J2⁻¹)ᵀ — maps covariant (ξ,η) components to physical (x,y)
    // J2⁻¹ = (1/det) · [[jac2[1][1], -jac2[0][1]], [-jac2[1][0], jac2[0][0]]]
    // J2⁻ᵀ[i][j] = J2⁻¹[j][i]
    let inv_t = [
        [ jac2[1][1] / det2, -jac2[1][0] / det2],
        [-jac2[0][1] / det2,  jac2[0][0] / det2],
    ];

    // Covariant shear at a tying point (ξ_t, η_t):
    // γ_ξζ = Σ_i (∂N_i/∂ξ · u_z_i + N_i · θ_y_i)
    // γ_ηζ = Σ_i (∂N_i/∂η · u_z_i - N_i · θ_x_i)
    //
    // For a given DOF vector u, the contributions per node are in columns
    // (u_z=DOF2, θ_x=DOF3, θ_y=DOF4).
    // We build B_s rows as: [γ_cov_xi, γ_cov_eta] × 18-DOF columns.
    //
    // But since we need to evaluate B_s at multiple quadrature points and
    // also sample at tying points, we use the full MITC3+ pipeline:
    //   1. Sample covariant strains at A, B, C.
    //   2. Interpolate via Mitc3Plus::interpolate_assumed_shear.
    //   3. Convert to physical via J2⁻ᵀ.
    //   4. Accumulate K_s += B_sᵀ · (κ·G·t·I₂) · B_s · det2 · w_q.
    //
    // Quadrature: 3-point edge-midpoint (A,B,C) with weight 1/6 each.
    // det2 is the Jacobian determinant (reference → local), weight = 1/6.
    // The 3 quadrature points coincide with the MITC3+ tying points.

    let tying_pts = Mitc3Plus.tying_points();
    // tying_pts = [A=(0.5,0), B=(0,0.5), C=(0.5,0.5)]

    // Build per-node covariant shear row contributions at each tying point.
    // B_cov_at_tp[tp_idx][row][dof] where row in {xi_zeta, eta_zeta}, dof in 0..18.
    // But we only need the per-node block: node n contributes to DOFs {6n+2, 6n+3, 6n+4}.

    // For each quadrature point (= tying point), compute the MITC3+ projected
    // covariant shear B_s_cov (2×18), then apply J2⁻ᵀ to get B_s_phys (2×18).

    // For the assumed-strain, we first build the 3×(DOFs for u_z,θ_x,θ_y)
    // covariant strains at each tying point.
    // covariant strain at tying point tp = (xi_t, eta_t):
    //   γ_ξζ = Σ_i dn_ref[i][0] * u_z_i + N_i(tp) * θ_y_i
    //   γ_ηζ = Σ_i dn_ref[i][1] * u_z_i - N_i(tp) * θ_x_i

    // We treat this as a linear operator on the 18-DOF vector and build
    // a 2×18 matrix B_cov_tp for each tying point. Then we assemble
    // TyingShears from {at_a: B_cov_tp[0], at_b: B_cov_tp[1], at_c: B_cov_tp[2]}.
    // Actually the interpolation formula takes scalar inputs, so we must
    // handle the linearity differently: we propagate the whole B matrix.

    // B_cov[tp][component][dof]: covariant shear B-matrix at each tying point
    let mut b_cov = [[[0.0_f64; NDOF]; 2]; 3]; // [tp][cov_component][dof]
    for (tp_idx, tp) in tying_pts.iter().enumerate() {
        // Use Mitc3Plus's canonical shape functions — single source of truth
        // for the reference-triangle layout (ξ, η) → [N_0, N_1, N_2].
        let n_at_tp = Mitc3Plus.shape_at(tp.coord);
        // shape_grad_at returns the constant ∂N/∂ξ and ∂N/∂η for each node:
        // ∇N_0=(−1,−1), ∇N_1=(1,0), ∇N_2=(0,1)
        let dn_ref_tp = Mitc3Plus.shape_grad_at(tp.coord);
        for node in 0..NN {
            let dof_uz = NDP * node + 2;
            let dof_tx = NDP * node + 3;
            let dof_ty = NDP * node + 4;
            // γ_ξζ contribution from this node: dn_ref[node][0]*u_z + N*θ_y
            b_cov[tp_idx][0][dof_uz] += dn_ref_tp[node][0];
            b_cov[tp_idx][0][dof_ty] += n_at_tp[node];
            // γ_ηζ contribution from this node: dn_ref[node][1]*u_z - N*θ_x
            b_cov[tp_idx][1][dof_uz] += dn_ref_tp[node][1];
            b_cov[tp_idx][1][dof_tx] -= n_at_tp[node];
        }
    }

    // For each quadrature point (= tying point, weight=1/6, det2 is Jacobian),
    // compute the MITC3+ projected B_s_phys (2×18) and accumulate K_s.
    // The MITC3+ interpolation is linear: for each DOF column d, the projected
    // covariant strain is interpolate_assumed_shear(sampled_for_column_d, qp).
    // We handle this column-by-column for all 18 DOFs, building B_s_phys[2][18].

    let qp_weight = 1.0 / 6.0; // each of A, B, C has weight 1/6 (sum=1/2=ref-tri area)

    for qp in tying_pts.iter() {
        // Build projected covariant B_s at this quadrature point (2×NDOF).
        let mut b_s_cov_qp = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            // For column `dof`, the covariant strain at each tying point is b_cov[tp][comp][dof].
            let sampled = TyingShears {
                at_a: ShearStrain {
                    gamma_xi_zeta: b_cov[0][0][dof],
                    gamma_eta_zeta: b_cov[0][1][dof],
                },
                at_b: ShearStrain {
                    gamma_xi_zeta: b_cov[1][0][dof],
                    gamma_eta_zeta: b_cov[1][1][dof],
                },
                at_c: ShearStrain {
                    gamma_xi_zeta: b_cov[2][0][dof],
                    gamma_eta_zeta: b_cov[2][1][dof],
                },
            };
            let projected = Mitc3Plus.interpolate_assumed_shear(sampled, qp.coord);
            b_s_cov_qp[0][dof] = projected.gamma_xi_zeta;
            b_s_cov_qp[1][dof] = projected.gamma_eta_zeta;
        }

        // Convert covariant to physical: b_s_phys = J2⁻ᵀ · b_s_cov
        let mut b_s_phys = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            b_s_phys[0][dof] = inv_t[0][0]*b_s_cov_qp[0][dof] + inv_t[0][1]*b_s_cov_qp[1][dof];
            b_s_phys[1][dof] = inv_t[1][0]*b_s_cov_qp[0][dof] + inv_t[1][1]*b_s_cov_qp[1][dof];
        }

        // Accumulate K_s += B_sᵀ · (κ·G·t) · B_s · det2 · weight
        let scale = kappa_g * t * det2 * qp_weight;
        for a in 0..NDOF {
            for b in 0..NDOF {
                let v = (b_s_phys[0][a] * b_s_phys[0][b]
                       + b_s_phys[1][a] * b_s_phys[1][b])
                      * scale;
                k_loc[a][b] += v;
            }
        }
    }

    // ---- Symmetrize K_local (average upper and lower triangle) ----
    // Each contribution (B_mᵀ D B_m, B_bᵀ D B_b, B_sᵀ D_s B_s) is
    // intrinsically symmetric in form, so the upper and lower entries
    // agree to within floating-point rounding. Averaging both triangles
    // (rather than discarding the lower) minimises the residual asymmetry.
    for a in 0..NDOF {
        for b in (a + 1)..NDOF {
            let m = 0.5 * (k_loc[a][b] + k_loc[b][a]);
            k_loc[a][b] = m;
            k_loc[b][a] = m;
        }
    }

    // ---- Local → Global rotation: K_global[a..a+3, b..b+3] = Rᵀ · K_loc[a..a+3, b..b+3] · R ----
    // T = blkdiag(R, R, R, R, R, R) — 2·NN blocks of 3×3 (two 3-DOF triples per node).
    // Apply Rᵀ on left, R on right, for each pair of 3-DOF blocks.
    let n_blocks = 2 * NN; // 6 = displacement triple + rotation triple per node
    let mut k_glob = [[0.0_f64; NDOF]; NDOF];
    for bi in 0..n_blocks { // block row index
        for bj in 0..n_blocks { // block col index
            let row_off = 3 * bi;
            let col_off = 3 * bj;
            // Extract 3×3 sub-block from k_loc
            let mut sub = [[0.0_f64; 3]; 3];
            for p in 0..3 {
                for q in 0..3 {
                    sub[p][q] = k_loc[row_off + p][col_off + q];
                }
            }
            // Rᵀ · sub · R
            let rt_sub = mat3_mul(&[[r[0][0],r[1][0],r[2][0]],[r[0][1],r[1][1],r[2][1]],[r[0][2],r[1][2],r[2][2]]], &sub);
            let rt_sub_r = mat3_mul(&rt_sub, &r);
            for p in 0..3 {
                for q in 0..3 {
                    k_glob[row_off + p][col_off + q] = rt_sub_r[p][q];
                }
            }
        }
    }

    // Pack into ElementStiffness
    let mut k_e = ElementStiffness::zeros(NDOF);
    for i in 0..NDOF {
        for j in 0..NDOF {
            k_e.data[i * NDOF + j] = k_glob[i][j];
        }
    }
    k_e
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use crate::constitutive::IsotropicElastic;
    use crate::elements::mitc3_plus::Mitc3Plus;
    use super::*;

    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    const UNIT_TRI: [[f64; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    /// Compute K · u for an 18-DOF stiffness matrix.
    fn matvec(k: &ElementStiffness, u: &[f64; Mitc3Plus::N_DOFS]) -> [f64; Mitc3Plus::N_DOFS] {
        let mut out = [0.0_f64; Mitc3Plus::N_DOFS];
        for i in 0..Mitc3Plus::N_DOFS {
            for j in 0..Mitc3Plus::N_DOFS {
                out[i] += k.get(i, j) * u[j];
            }
        }
        out
    }

    /// L∞ norm of a fixed-size slice.
    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    const WIDE_TRI: [[f64; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 3.0, 0.0],
    ];

    #[test]
    fn shell_element_stiffness_returns_18_by_18_for_unit_triangle() {
        let k = shell_element_stiffness(&UNIT_TRI, 0.05, &steel_like());
        assert_eq!(k.n_dofs, Mitc3Plus::N_DOFS);
        assert_eq!(k.data.len(), Mitc3Plus::N_DOFS * Mitc3Plus::N_DOFS);
    }

    // --- Membrane patch test (step 7) ---

    #[test]
    fn shell_membrane_patch_test_uniform_in_plane_strain_matches_analytical_energy() {
        // Triangle in xy-plane. Linear u_x = a·x, u_y = b·y, all other DOFs zero.
        // Membrane strain: ε_xx=a, ε_yy=b, γ_xy=0. Area A=0.5.
        // U_analytical = 0.5 · [a, b, 0] · D_pl · [a, b, 0]ᵀ · t · A
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.01_f64;
        let b = -0.005_f64;
        let nodes = UNIT_TRI; // p0=(0,0,0), p1=(1,0,0), p2=(0,1,0)
        let k = shell_element_stiffness(&nodes, t, &mat);

        // Build 18-DOF displacement vector: DOF layout NDP·node + i
        // u_x at node i => DOF NDP·i+0; u_y at node i => DOF NDP·i+1
        const NDP: usize = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        // node 0: x=0,y=0 → u_x=0, u_y=0
        // node 1: x=1,y=0 → u_x=a, u_y=0
        u[NDP * 1 + 0] = a * 1.0;
        // node 2: x=0,y=1 → u_x=0, u_y=b
        u[NDP * 2 + 1] = b * 1.0;

        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let d = plane_stress_d(&mat);
        let eps = [a, b, 0.0_f64];
        let d_eps: [f64; 3] = [
            d[0][0]*eps[0] + d[0][1]*eps[1],
            d[1][0]*eps[0] + d[1][1]*eps[1],
            0.0,
        ];
        let area = 0.5_f64;
        let u_analytical = 0.5 * (eps[0]*d_eps[0] + eps[1]*d_eps[1]) * t * area;

        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "U_K={u_k}, U_analytical={u_analytical}, rel_err={}",
            (u_k - u_analytical).abs() / scale,
        );
    }

    // --- Symmetry test (step 13) ---

    #[test]
    fn shell_element_stiffness_is_symmetric_within_fp_tolerance() {
        let k = shell_element_stiffness(&UNIT_TRI, 0.05, &steel_like());
        for i in 0..18 {
            for j in 0..18 {
                let kij = k.get(i, j);
                let kji = k.get(j, i);
                let scale = kij.abs().max(kji.abs()).max(1.0);
                assert!(
                    (kij - kji).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): K[i][j]={kij}, K[j][i]={kji}",
                );
            }
        }
    }

    // --- Rigid-body translation null-space (step 15) ---

    #[test]
    fn shell_has_rigid_body_translation_null_space() {
        let k = shell_element_stiffness(&UNIT_TRI, 0.05, &steel_like());
        // Tolerance relative to the maximum absolute K entry: floating-point
        // cancellation in the partition-of-unity sum scales with K_max, so an
        // absolute 1e-9 is too strict when E=200 GPa (K entries ~5e9).  Use
        // the same relative-scale pattern as the rotation null-space test.
        let k_max = k.data.iter().copied().fold(0.0_f64, |a, x| a.max(x.abs()));
        let tol = 1e-9 * k_max.max(1.0);
        for axis in 0..3 {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                u[Mitc3Plus::N_DOFS_PER_NODE * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < tol,
                "axis {axis}: linf(K·u_translation) = {}, tol = {tol}",
                linf(&ku),
            );
        }
    }

    // --- Rigid-body rotation null-space (step 17) ---

    #[test]
    fn shell_has_rigid_body_rotation_null_space() {
        // Centroid of unit triangle
        let c = [1.0 / 3.0_f64, 1.0 / 3.0, 0.0_f64];
        let nodes = UNIT_TRI;
        let k = shell_element_stiffness(&nodes, 0.05, &steel_like());

        // For each axis ω ∈ {e_x, e_y, e_z}, build 18-DOF rigid rotation mode.
        // Displacement: u_i = ω × (x_i - c); rotation: θ_i = ω.
        let omega = [[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for &w in &omega {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                let dx = [nodes[node][0] - c[0], nodes[node][1] - c[1], nodes[node][2] - c[2]];
                // u_i = ω × dx
                let ux = w[1] * dx[2] - w[2] * dx[1];
                let uy = w[2] * dx[0] - w[0] * dx[2];
                let uz = w[0] * dx[1] - w[1] * dx[0];
                let ndp = Mitc3Plus::N_DOFS_PER_NODE;
                u[ndp * node + 0] = ux;
                u[ndp * node + 1] = uy;
                u[ndp * node + 2] = uz;
                // θ_i = ω
                u[ndp * node + 3] = w[0];
                u[ndp * node + 4] = w[1];
                u[ndp * node + 5] = w[2];
            }
            let ku = matvec(&k, &u);
            let norm_ku = linf(&ku);
            // Tolerance relative to max absolute entry of K × |u| components
            let ku_scale = k.data.iter().copied().fold(0.0_f64, |a, x| a.max(x.abs()));
            let tol = 1e-9 * ku_scale.max(1.0);
            assert!(
                norm_ku < tol,
                "ω={w:?}: linf(K·u_rotation) = {norm_ku}, tol = {tol}",
            );
        }
    }

    // --- Thickness scaling (step 19) ---

    #[test]
    fn shell_thickness_scaling_membrane_mode_doubles_with_t() {
        let mat = steel_like();
        let a = 0.01_f64;
        let b = -0.005_f64;
        let t = 0.05_f64;
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        u[ndp * 1 + 0] = a;
        u[ndp * 2 + 1] = b;

        let k1 = shell_element_stiffness(&UNIT_TRI, t, &mat);
        let k2 = shell_element_stiffness(&UNIT_TRI, 2.0 * t, &mat);
        let ku1 = matvec(&k1, &u);
        let ku2 = matvec(&k2, &u);

        for i in 0..Mitc3Plus::N_DOFS {
            let scale = ku1[i].abs().max(1.0);
            assert!(
                (ku2[i] - 2.0 * ku1[i]).abs() < 1e-9 * scale,
                "membrane scaling at DOF {i}: 2·K(t)·u = {}, K(2t)·u = {}",
                2.0 * ku1[i],
                ku2[i],
            );
        }
    }

    #[test]
    fn shell_thickness_scaling_shear_mode_doubles_with_t() {
        let mat = steel_like();
        let alpha = 0.003_f64;
        let t = 0.05_f64;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            u[Mitc3Plus::N_DOFS_PER_NODE * node + 4] = alpha;
        }

        let k1 = shell_element_stiffness(&UNIT_TRI, t, &mat);
        let k2 = shell_element_stiffness(&UNIT_TRI, 2.0 * t, &mat);
        let ku1 = matvec(&k1, &u);
        let ku2 = matvec(&k2, &u);

        for i in 0..Mitc3Plus::N_DOFS {
            let scale = ku1[i].abs().max(1.0);
            assert!(
                (ku2[i] - 2.0 * ku1[i]).abs() < 1e-9 * scale,
                "shear scaling at DOF {i}: 2·K(t)·u = {}, K(2t)·u = {}",
                2.0 * ku1[i],
                ku2[i],
            );
        }
    }

    // --- Bending t³ thickness scaling (amendment: suggestion 4) ---

    /// Verify bending stiffness scales as t³: K_b(2t)·u = 8·K_b(t)·u.
    ///
    /// Strategy: Use a pure-curvature mode where the MITC3 projected shear is
    /// exactly zero. For the unit triangle with all θ_x uniform (non-zero) and
    /// all other DOFs zero, the curvature κ_yy = ∂θ_x/∂y = 0 (constant) and
    /// κ_xy = ∂θ_x/∂x = 0 (constant), BUT we need ∂θ_x/∂y ≠ 0 for a bending
    /// mode. A cleaner construction: set θ_x at node 2 only (node 2 is at y=1
    /// for the unit triangle), leaving nodes 0 and 1 at θ_x=0. This gives
    /// uniform κ_yy = -α (from ∂θ_x/∂y · dN_2/dy = α · 1 = α) with zero
    /// curvature in the other directions.
    ///
    /// The covariant shear for this mode: γ_ηζ = Σ_i dn_ref[i][1]*0 - N_i*θ_x_i.
    /// At tying point A=(½,0): N = [½,½,0], γ_ηζ = -N_2·α = 0.
    /// At tying point B=(0,½): N = [½,0,½], γ_ηζ = -N_2·α = -α/2.
    /// At tying point C=(½,½): N = [0,½,½], γ_ηζ = -N_2·α = -α/2.
    /// After MITC3 interpolation, the projected shear is non-zero.
    ///
    /// To isolate bending t³ from shear t scaling, we compare the *ratio*
    /// K(2t)·u / K(t)·u component-wise and assert it equals 8 (= 2³) for the
    /// DOFs where bending dominates, or verify the total strain energy scales
    /// correctly. We use the cleaner energy approach: fix α small enough that
    /// shear is negligible compared to bending, and assert U(2t)/U(t) ≈ 8.
    ///
    /// Cleaner: we use a DOF pattern where the MITC3 shear projection is
    /// exactly zero. For uniform θ_x = β (constant) at all nodes:
    ///   γ_ηζ at A, B, C = -N_total · β = -1 · β (N sums to 1 everywhere).
    /// Not zero. So there is always some shear for a pure-rotation mode.
    ///
    /// Instead, we isolate the bending t³ scaling by asserting that the
    /// energy ratio U_K(2t) / U_K(t) approaches 8 in the limit where bending
    /// dominates (large t). For steel-like material, with the curvature-only
    /// bending mode (θ_y = α·x, all else zero), bending energy ~ t³ and shear
    /// energy ~ t. At larger t, bending dominates and the ratio approaches 8.
    /// We test at t = 1.0 (very thick) where t³/12·D·A >> κ·G·t·A·(α/2)².
    #[test]
    fn shell_thickness_scaling_bending_mode_scales_as_t_cubed() {
        // Use the bending-patch mode: θ_y(node_i) = α·x_i (node1→α, others→0).
        // Energy: U = 0.5·α²·D_pl[0][0]·(t³/12)·A + 0.5·(α/2)²·κ·G·t·A
        //           = C_b·t³ + C_s·t
        // For t large enough, C_b·t³ >> C_s·t and the ratio U(2t)/U(t) → 8.
        //
        // Direct algebraic test: assert U(2t)/U(t) = (8·C_b·t³ + 2·C_s·t) / (C_b·t³ + C_s·t)
        // matches the ratio measured from K. We verify K(2t)·u entries scale
        // correctly against the analytical formula, which is the cleanest check.
        let mat = steel_like();
        let alpha = 0.002_f64;
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        u[ndp * 1 + 4] = alpha; // θ_y at node 1 (x=1)

        let d = plane_stress_d(&mat);
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g = e / (2.0 * (1.0 + nu));
        let kappa = 5.0_f64 / 6.0;
        let area = 0.5_f64;

        for &t in &[0.01_f64, 0.05, 0.1, 0.5] {
            let k1 = shell_element_stiffness(&UNIT_TRI, t, &mat);
            let k2 = shell_element_stiffness(&UNIT_TRI, 2.0 * t, &mat);

            let ku1 = matvec(&k1, &u);
            let ku2 = matvec(&k2, &u);
            let uk1: f64 = 0.5 * ku1.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();
            let uk2: f64 = 0.5 * ku2.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

            // Analytical energies: C_b·t³ + C_s·t and C_b·(2t)³ + C_s·(2t)
            let c_b = 0.5 * alpha * alpha * d[0][0] * (1.0 / 12.0) * area;
            let c_s = 0.5 * (alpha / 2.0) * (alpha / 2.0) * kappa * g * area;
            let u_anal1 = c_b * t.powi(3) + c_s * t;
            let u_anal2 = c_b * (2.0 * t).powi(3) + c_s * (2.0 * t);

            // K-energy must match the analytical formula at both t and 2t.
            let scale1 = u_anal1.abs().max(1e-30);
            let scale2 = u_anal2.abs().max(1e-30);
            assert!(
                (uk1 - u_anal1).abs() < 1e-9 * scale1,
                "t={t}: U_K={uk1}, U_anal={u_anal1}",
            );
            assert!(
                (uk2 - u_anal2).abs() < 1e-9 * scale2,
                "t={t}: U_K(2t)={uk2}, U_anal(2t)={u_anal2}",
            );

            // The ratio U(2t)/U(t) = (8·C_b·t³ + 2·C_s·t) / (C_b·t³ + C_s·t).
            // This lies strictly between 2 (shear-dominated) and 8 (bending-dominated).
            // We verify the measured ratio matches the analytical ratio.
            let ratio_anal = u_anal2 / u_anal1;
            let ratio_meas = uk2 / uk1;
            let scale_r = ratio_anal.abs().max(1.0);
            assert!(
                (ratio_meas - ratio_anal).abs() < 1e-6 * scale_r,
                "t={t}: ratio U(2t)/U(t) measured={ratio_meas} vs analytical={ratio_anal}",
            );
        }
    }

    // --- Frame covariance (step 21) ---

    #[test]
    fn shell_strain_energy_invariant_under_global_rigid_rotation() {
        // Membrane mode on xy-plane triangle.
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.01_f64;
        let b = -0.005_f64;
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u_orig = [0.0_f64; Mitc3Plus::N_DOFS];
        u_orig[ndp * 1 + 0] = a;
        u_orig[ndp * 2 + 1] = b;
        let k_orig = shell_element_stiffness(&UNIT_TRI, t, &mat);
        let ku_orig = matvec(&k_orig, &u_orig);
        let u_k_orig: f64 = 0.5 * ku_orig.iter().zip(u_orig.iter()).map(|(a, b)| a * b).sum::<f64>();

        // Global rotation Q: 30° about z, then 45° about y.
        let cos30 = (30.0_f64.to_radians()).cos();
        let sin30 = (30.0_f64.to_radians()).sin();
        let cos45 = (45.0_f64.to_radians()).cos();
        let sin45 = (45.0_f64.to_radians()).sin();
        // Rz(30°)
        let rz = [[cos30, -sin30, 0.0], [sin30, cos30, 0.0], [0.0, 0.0, 1.0]];
        // Ry(45°)
        let ry = [[cos45, 0.0, sin45], [0.0, 1.0, 0.0], [-sin45, 0.0, cos45]];
        // Q = Ry · Rz
        let q = mat3_mul(&ry, &rz);

        // Rotate nodes
        let mut rot_nodes = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                rot_nodes[ni][i] = q[i][0]*node[0] + q[i][1]*node[1] + q[i][2]*node[2];
            }
        }

        // Rotate DOFs: each (u triple) and (θ triple) by Q
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u_rot = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            for triple in 0..2 { // 0=displacements, 1=rotations
                let off = ndp * node + 3 * triple;
                let v = [u_orig[off], u_orig[off+1], u_orig[off+2]];
                for i in 0..3 {
                    u_rot[off + i] = q[i][0]*v[0] + q[i][1]*v[1] + q[i][2]*v[2];
                }
            }
        }

        let k_rot = shell_element_stiffness(&rot_nodes, t, &mat);
        let ku_rot = matvec(&k_rot, &u_rot);
        let u_k_rot: f64 = 0.5 * ku_rot.iter().zip(u_rot.iter()).map(|(a, b)| a * b).sum::<f64>();

        let scale = u_k_orig.abs().max(1.0);
        assert!(
            (u_k_orig - u_k_rot).abs() < 1e-9 * scale,
            "frame covariance: U_orig={u_k_orig}, U_rot={u_k_rot}, diff={}",
            (u_k_orig - u_k_rot).abs(),
        );
    }

    // --- Transverse-shear patch test (step 9) ---

    #[test]
    fn shell_transverse_shear_patch_test_uniform_theta_y_matches_analytical_energy() {
        // Uniform θ_y = α at all nodes → uniform γ_xz = α, γ_yz = 0.
        // U_analytical = 0.5 · α² · κ · G · t · A.
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.003_f64;
        let k = shell_element_stiffness(&UNIT_TRI, t, &mat);

        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            u[Mitc3Plus::N_DOFS_PER_NODE * node + 4] = alpha; // θ_y
        }

        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g = e / (2.0 * (1.0 + nu));
        let kappa = 5.0_f64 / 6.0;
        let area = 0.5_f64;
        let u_analytical = 0.5 * alpha * alpha * kappa * g * t * area;

        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "U_K={u_k}, U_analytical={u_analytical}, rel_err={}",
            (u_k - u_analytical).abs() / scale,
        );
    }

    // --- Bending patch test (step 11) ---

    #[test]
    fn shell_bending_patch_test_linear_theta_y_matches_analytical_total_energy() {
        // θ_y(node_i) = α · x_i: node0→0, node1→α, node2→0.
        // Curvature κ_xx = -α (uniform), MITC3+ projects γ_xz to constant α/2.
        // U_total = 0.5·α²·D_pl[0][0]·(t³/12)·A + 0.5·(α/2)²·κ·G·t·A.
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.002_f64;
        let k = shell_element_stiffness(&UNIT_TRI, t, &mat);

        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        // node0 at x=0: θ_y = 0
        // node1 at x=1: θ_y = α
        u[ndp * 1 + 4] = alpha * 1.0;
        // node2 at x=0: θ_y = 0

        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let d = plane_stress_d(&mat);
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g = e / (2.0 * (1.0 + nu));
        let kappa = 5.0_f64 / 6.0;
        let area = 0.5_f64;
        let u_bending = 0.5 * alpha * alpha * d[0][0] * (t * t * t / 12.0) * area;
        let u_shear = 0.5 * (alpha / 2.0) * (alpha / 2.0) * kappa * g * t * area;
        let u_analytical = u_bending + u_shear;

        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "U_K={u_k}, U_analytical={u_analytical} (bending={u_bending}, shear={u_shear}), rel_err={}",
            (u_k - u_analytical).abs() / scale,
        );
    }

    // --- plane_stress_d test (step 5) ---

    #[test]
    fn plane_stress_d_matches_isotropic_formula_for_steel_like() {
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let d = plane_stress_d(&mat);
        let factor = e / (1.0 - nu * nu);
        let tol = 1e-9 * factor.abs();
        // d[0][0] = d[1][1] = E/(1-ν²)
        assert!((d[0][0] - factor).abs() < tol, "d[0][0] = {}", d[0][0]);
        assert!((d[1][1] - factor).abs() < tol, "d[1][1] = {}", d[1][1]);
        // d[0][1] = d[1][0] = ν·E/(1-ν²)
        assert!((d[0][1] - nu * factor).abs() < tol, "d[0][1] = {}", d[0][1]);
        assert!((d[1][0] - nu * factor).abs() < tol, "d[1][0] = {}", d[1][0]);
        // d[2][2] = E/(2(1+ν))
        let g = e / (2.0 * (1.0 + nu));
        assert!((d[2][2] - g).abs() < tol, "d[2][2] = {}", d[2][2]);
        // Off-diagonal block entries are zero
        for (i, j) in [(0, 2), (1, 2), (2, 0), (2, 1)] {
            assert!(d[i][j].abs() < tol, "d[{i}][{j}] = {}", d[i][j]);
        }
    }

    // --- ShellFrame tests (step 3) ---

    #[test]
    fn build_shell_frame_returns_orthonormal_rotation() {
        let frame = build_shell_frame(&WIDE_TRI);
        let r = frame.r;
        // Each row has unit norm.
        for i in 0..3 {
            let norm_sq = r[i][0] * r[i][0] + r[i][1] * r[i][1] + r[i][2] * r[i][2];
            assert!(
                (norm_sq - 1.0).abs() < 1e-12,
                "row {i} norm² = {norm_sq}, expected 1.0",
            );
        }
        // Rows are mutually orthogonal.
        for i in 0..3 {
            for j in (i + 1)..3 {
                let dot = r[i][0] * r[j][0] + r[i][1] * r[j][1] + r[i][2] * r[j][2];
                assert!(
                    dot.abs() < 1e-12,
                    "rows {i} · {j} = {dot}, expected 0",
                );
            }
        }
    }

    #[test]
    fn build_shell_frame_normal_is_perpendicular_to_in_plane_edges() {
        let frame = build_shell_frame(&WIDE_TRI);
        let n = frame.r[2]; // e3 = normal
        let p0 = WIDE_TRI[0];
        let p1 = WIDE_TRI[1];
        let p2 = WIDE_TRI[2];
        let e01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let dot01 = n[0] * e01[0] + n[1] * e01[1] + n[2] * e01[2];
        let dot02 = n[0] * e02[0] + n[1] * e02[1] + n[2] * e02[2];
        assert!(dot01.abs() < 1e-12, "n · e01 = {dot01}, expected 0");
        assert!(dot02.abs() < 1e-12, "n · e02 = {dot02}, expected 0");
    }

    #[test]
    fn build_shell_frame_area_matches_half_cross_product_norm() {
        let frame = build_shell_frame(&WIDE_TRI);
        // For nodes (0,0,0), (2,0,0), (0,3,0):
        // cross = (2,0,0) × (0,3,0) = (0,0,6) → |cross| = 6 → area = 3.
        let expected_area = 3.0_f64;
        assert!(
            (frame.area - expected_area).abs() < 1e-12,
            "area = {}, expected {expected_area}",
            frame.area,
        );
    }

    #[test]
    #[should_panic(expected = "degenerate shell element: p0 == p1")]
    fn build_shell_frame_panics_on_zero_edge_p0_eq_p1() {
        // p0 == p1 → len01 = 0 → first degenerate-frame assert fires.
        build_shell_frame(&[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }
}
