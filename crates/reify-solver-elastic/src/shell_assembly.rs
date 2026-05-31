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
//! # `shell_element_stiffness` is bare MITC3; the MITC3+ sibling is below
//!
//! [`shell_element_stiffness`] is the bare-MITC3 baseline (edge-midpoint
//! assumed shear, no bubble). The genuine flat-facet **MITC3+** element of
//! Bathe & Lee 2014 lives in the sibling [`shell_element_stiffness_mitc3_plus`]
//! (task 3392); the two are kept side by side so the shear-locking improvement
//! is measurable against the bare baseline.
//!
//! ## What the cubic bubble does — and does not — do on a flat facet
//!
//! The "+" of MITC3+ is a cubic rotation bubble `f_b = ξη(1−ξ−η)`. On a flat,
//! constant-Jacobian facet this bubble is **inert in BOTH coupling blocks**:
//!
//! - **Bending** (task 3349): the bending cross-coupling `K_NB^bend =
//!   ∫ B_b_nodal^T · D · B_b_bubble dA` is identically zero because `f_b`
//!   vanishes on all three edges, so `∫∫ ∂f_b/∂x dA = ∮ f_b · n_x ds = 0` by the
//!   divergence theorem (likewise ∂/∂y). This is the correct-but-narrow result:
//!   it kills a *bending* bubble on a flat facet only.
//! - **Transverse shear** (esc-3392 corrected resolution; DD#2 retracted): the
//!   shear cross-coupling `K_NB^shear` is *also* identically zero on a flat
//!   facet — re-deriving the flat-facet covariant-shear kinematics shows the
//!   bubble does not enter the assumed shear by value or by gradient there.
//!
//! It does **not** follow that MITC3+ needs curved geometry. On a flat facet the
//! genuine MITC3+ shear-locking cure lives entirely in the **nodal** assumed
//! transverse-shear field: the bare three-node covariant shear is sampled at six
//! *interior* tying points A–F and re-interpolated via Eq. 9
//! ([`crate::elements::mitc3_plus::Mitc3Plus::interpolate_assumed_shear_mitc3_plus`]),
//! a softer field than the bare edge-midpoint Eq. 5. The bubble (carried through
//! the retained 20×20 skeleton + 2×2 condensation, a no-op here since `K_NB = 0`)
//! enriches bending only and becomes live in shear/membrane on the curved
//! director substrate of task 4065. See the doc comment on
//! [`shell_element_stiffness_mitc3_plus`] for the full block breakdown.

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Local mid-surface coordinate frame for a MITC3 shell element.
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

impl ShellFrame {
    /// Returns the local-to-global rotation matrix for this shell element.
    ///
    /// # Convention
    ///
    /// The returned 3×3 matrix is the *local-to-global* rotation:
    /// - `result[i][j]` is the *i-th global component of the j-th local basis vector*
    ///   (equivalently: the j-th column of `result` is the j-th local basis vector expressed
    ///   in global coordinates).
    /// - A local-frame displacement vector `v_local` maps to global via `v_global = frame · v_local`.
    /// - A local-frame rank-2 stress tensor maps to global via `σ_global = frame · σ_local · frameᵀ`.
    ///
    /// This is the **transpose** of `self.r`, which stores the *global-to-local* rotation
    /// (rows = local basis vectors in global coordinates, so `R · v_global = v_local`).
    /// Transposing gives the local-to-global direction: `result[i][j] = self.r[j][i]`.
    ///
    /// # Relation to `ElasticResult.frame`
    ///
    /// Matches the `ElasticResult.frame` local-to-global convention documented in
    /// `crates/reify-compiler/stdlib/solver_elastic.ri:276–294`.  The future
    /// `to_global(stress, frame)` helper (T18-T20) can use this directly as
    /// `σ_global = frame · σ_local · frameᵀ` without any transpose step at the call site.
    ///
    /// See also [`build_shell_frame`] for how `self.r` is constructed.
    pub fn local_to_global(&self) -> [[f64; 3]; 3] {
        // Transpose: result[i][j] = self.r[j][i].
        std::array::from_fn(|i| std::array::from_fn(|j| self.r[j][i]))
    }
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
    assert!(len_n > 1e-30, "degenerate shell element: collinear nodes");
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
///
/// Validity is enforced by [`IsotropicElastic::debug_assert_valid`] — the
/// single source of truth for Poisson-ratio bounds in this crate (`-1 < ν < 0.5`).
pub fn plane_stress_d(material: &IsotropicElastic) -> [[f64; 3]; 3] {
    material.debug_assert_valid();
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let factor = e / (1.0 - nu * nu);
    [
        [factor, nu * factor, 0.0],
        [nu * factor, factor, 0.0],
        [0.0, 0.0, factor * (1.0 - nu) / 2.0],
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

// --- Element-size constants (full-path initializers; no `use`, so the shared
// helpers below can size their array signatures without a redundant import). ---

/// Total element DOFs (18), used by the shared local→global / symmetrize helpers.
const NDOF_ELEM: usize = crate::elements::mitc3_plus::Mitc3Plus::N_DOFS;
/// DOFs per node (6).
const NDP_ELEM: usize = crate::elements::mitc3_plus::Mitc3Plus::N_DOFS_PER_NODE;
/// Lagrangian nodes per element (3).
const NN_ELEM: usize = crate::elements::mitc3_plus::Mitc3Plus::N_NODES;

/// Accumulate the membrane block `Bₘᵀ·(t·D)·Bₘ·A` into the nodal DOFs of `k`.
///
/// Shared verbatim by the bare-MITC3 [`shell_element_stiffness`] and the MITC3+
/// [`shell_element_stiffness_mitc3_plus`]: membrane is identical in both
/// formulations and touches only the 18 nodal DOFs, so this is generic over the
/// matrix size `N ∈ {18 (bare), 20 (MITC3+ uncondensed)}`. Sharing one body
/// makes the "membrane is bit-identical to bare MITC3" guarantee structural
/// rather than a property of two copies staying in lockstep.
///
/// `t_dpl` is the pre-scaled `t · D_pl`; `dn` are the constant shape gradients.
#[inline]
#[allow(clippy::needless_range_loop)]
fn accumulate_membrane_k<const N: usize>(
    k: &mut [[f64; N]; N],
    dn: &[[f64; 2]; NN_ELEM],
    t_dpl: &[[f64; 3]; 3],
    area: f64,
) {
    for ni in 0..NN_ELEM {
        for nj in 0..NN_ELEM {
            let doi = [NDP_ELEM * ni, NDP_ELEM * ni + 1];
            let doj = [NDP_ELEM * nj, NDP_ELEM * nj + 1];
            let bmi = [[dn[ni][0], 0.0], [0.0, dn[ni][1]], [dn[ni][1], dn[ni][0]]];
            let bmj = [[dn[nj][0], 0.0], [0.0, dn[nj][1]], [dn[nj][1], dn[nj][0]]];
            for a in 0..2 {
                for b in 0..2 {
                    let mut v = 0.0;
                    for rr in 0..3 {
                        for s in 0..3 {
                            v += bmi[rr][a] * t_dpl[rr][s] * bmj[s][b];
                        }
                    }
                    k[doi[a]][doj[b]] += v * area;
                }
            }
        }
    }
}

/// Accumulate the nodal bending block `B_bᵀ·(t³/12·D)·B_b·A` into the rotational
/// nodal DOFs of `k`.
///
/// Shared verbatim by both element variants (the MITC3+ nodal bending K_NN is
/// bit-identical to bare MITC3); generic over `N ∈ {18, 20}` for the same reason
/// as [`accumulate_membrane_k`]. `t3_dpl` is the pre-scaled `t³/12 · D_pl`.
///
/// This assembles the **nodal** bending only; the MITC3+ bubble bending
/// self-term `K_BB^bend` is variant-specific and stays in
/// [`shell_element_stiffness_mitc3_plus`].
#[inline]
#[allow(clippy::needless_range_loop)]
fn accumulate_bending_k_nodal<const N: usize>(
    k: &mut [[f64; N]; N],
    dn: &[[f64; 2]; NN_ELEM],
    t3_dpl: &[[f64; 3]; 3],
    area: f64,
) {
    for ni in 0..NN_ELEM {
        for nj in 0..NN_ELEM {
            let doi = [NDP_ELEM * ni + 3, NDP_ELEM * ni + 4];
            let doj = [NDP_ELEM * nj + 3, NDP_ELEM * nj + 4];
            let bbi = [[0.0, -dn[ni][0]], [dn[ni][1], 0.0], [dn[ni][0], -dn[ni][1]]];
            let bbj = [[0.0, -dn[nj][0]], [dn[nj][1], 0.0], [dn[nj][0], -dn[nj][1]]];
            for a in 0..2 {
                for b in 0..2 {
                    let mut v = 0.0;
                    for rr in 0..3 {
                        for s in 0..3 {
                            v += bbi[rr][a] * t3_dpl[rr][s] * bbj[s][b];
                        }
                    }
                    k[doi[a]][doj[b]] += v * area;
                }
            }
        }
    }
}

/// Average the upper and lower triangles of an 18×18 local matrix in place.
///
/// Each B^T·D·B contribution is symmetric in form, so the two triangles agree to
/// within floating-point rounding; averaging (rather than copying one triangle)
/// minimises the residual asymmetry. Shared by both element variants.
#[inline]
#[allow(clippy::needless_range_loop)]
fn symmetrize_in_place(k_loc: &mut [[f64; NDOF_ELEM]; NDOF_ELEM]) {
    for a in 0..NDOF_ELEM {
        for b in (a + 1)..NDOF_ELEM {
            let m = 0.5 * (k_loc[a][b] + k_loc[b][a]);
            k_loc[a][b] = m;
            k_loc[b][a] = m;
        }
    }
}

/// Rotate a local-frame 18×18 element matrix into the global frame:
/// `K_glob[a..a+3, b..b+3] = Rᵀ · K_loc[a..a+3, b..b+3] · R`.
///
/// `T = blkdiag(R, …, R)` over the 2·N_NODES three-DOF blocks (a displacement
/// triple and a rotation triple per node). Shared verbatim by both element
/// variants — the only thing that differs between bare MITC3 and MITC3+ is the
/// transverse-shear treatment that produced `k_loc`, not this rotation.
#[inline]
#[allow(clippy::needless_range_loop)]
fn rotate_local_to_global(
    k_loc: &[[f64; NDOF_ELEM]; NDOF_ELEM],
    r: &[[f64; 3]; 3],
) -> [[f64; NDOF_ELEM]; NDOF_ELEM] {
    let n_blocks = 2 * NN_ELEM; // displacement triple + rotation triple per node
    let mut k_glob = [[0.0_f64; NDOF_ELEM]; NDOF_ELEM];
    for bi in 0..n_blocks {
        for bj in 0..n_blocks {
            let row_off = 3 * bi;
            let col_off = 3 * bj;
            let mut sub = [[0.0_f64; 3]; 3];
            for p in 0..3 {
                for q in 0..3 {
                    sub[p][q] = k_loc[row_off + p][col_off + q];
                }
            }
            let rt_sub = mat3_mul(
                &[
                    [r[0][0], r[1][0], r[2][0]],
                    [r[0][1], r[1][1], r[2][1]],
                    [r[0][2], r[1][2], r[2][2]],
                ],
                &sub,
            );
            let rt_sub_r = mat3_mul(&rt_sub, r);
            for p in 0..3 {
                for q in 0..3 {
                    k_glob[row_off + p][col_off + q] = rt_sub_r[p][q];
                }
            }
        }
    }
    k_glob
}

/// Compute the 18×18 element stiffness matrix for a MITC3 shell element.
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `thickness` is the constant shell thickness `t`.
/// `material` is the isotropic linear-elastic constitutive law.
///
/// Returns an [`ElementStiffness`] with `n_dofs = 18`. DOF ordering is
/// `6 · node_idx + i` with `i ∈ {0..5}` for `(u_x, u_y, u_z, θ_x, θ_y, θ_z)`.
///
/// **Drilling singularity.** The local drilling rotation — rotation about the
/// element normal — carries zero stiffness by construction (pure MITC3, no
/// Allman/Hughes enrichment).  In the *local* frame this is the `θ_z` DOF
/// (i=5 in each node's rotation triple), i.e. row/column 5, 11, 17 of K_local
/// are zero.  After rotation to global via R^T·K_local·R the singular
/// direction is **not** global `θ_z` unless the shell is xy-aligned.  For a
/// tilted element, the zero-stiffness eigenvector in each node's
/// three-component rotation sub-block is `R\[2\]` — the local frame's normal
/// axis expressed in global coordinates.  Derivation: `K_local · R · v = 0
/// ⇒ R · v ∥ ê_z ⇒ v ∝ Rᵀ · ê_z`, which is exactly the third row of `R`,
/// i.e. `e3 = R\[2\]`.
///
/// The global sparse-assembly consumer (PRD T#11) must either:
/// (a) constrain each node's rotation about the per-element local normal
///     `R\[2\]` explicitly, or
/// (b) add Allman/Hughes drilling stabilization at the assembly layer.
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
    use crate::elements::mitc3_plus::{Mitc3Plus, TyingShears};
    assert!(
        thickness > 0.0,
        "shell_element_stiffness: thickness must be positive, got {thickness}"
    );
    // Element-size constant — avoid hard-coding 18 throughout. (Per-node and
    // node counts now live in the shared assembly helpers.)
    const NDOF: usize = Mitc3Plus::N_DOFS; // 18 total DOFs

    let frame = build_shell_frame(nodes);
    let r = frame.r; // rotation matrix: row i = local basis eᵢ in global coords
    let area = frame.area;
    let t = thickness;
    let d_pl = plane_stress_d(material);

    // Shear modulus G and transverse-shear D scalar: κ·G
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let g = e / (2.0 * (1.0 + nu));
    let kappa_g = KAPPA * g;

    // Shared kinematics: local 2D coords, shape gradients, B_cov, J2⁻ᵀ, det2.
    let kin = crate::shell_kinematics::shell_kinematics(nodes, &frame);
    let dn = kin.dn;

    // --- 18×18 K_local (assembled in local frame) ---
    let mut k_loc = [[0.0_f64; NDOF]; NDOF];

    // ---- Membrane K ----
    // B_m is 3×9 (rows: ε_xx, ε_yy, γ_xy; per-node 2-col blocks map (u_x, u_y) →
    // local DOFs 6i+{0,1}). Assembled by the shared helper, so it is structurally
    // bit-identical to the MITC3+ membrane block.
    let t_dpl = {
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                td[i][j] = t * d_pl[i][j];
            }
        }
        td
    };
    accumulate_membrane_k(&mut k_loc, &dn, &t_dpl, area);

    // ---- Bending K (nodal) ----
    // B_b is 3×9 (rows: κ_xx, κ_yy, 2κ_xy; per-node 2-col blocks map (θ_x, θ_y) →
    // local DOFs 6i+{3,4}). Assembled by the shared helper, so it is structurally
    // bit-identical to the MITC3+ nodal bending block.
    let t3_12_dpl = {
        let factor = t * t * t / 12.0;
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                td[i][j] = factor * d_pl[i][j];
            }
        }
        td
    };
    accumulate_bending_k_nodal(&mut k_loc, &dn, &t3_12_dpl, area);

    // ---- Transverse-shear K (step 10, implemented here) ----
    // MITC3 assumed-strain interpolation.
    // Physical DOFs per node for shear: u_z (6n+2), θ_x (6n+3), θ_y (6n+4).
    let det2 = kin.det2;
    let inv_t = kin.jac2_inv_t;

    // MITC3 assumed-strain: covariant shear B-matrix from shared kinematics helper.
    // See shell_kinematics::shell_kinematics for the single source of truth.
    let b_cov = kin.b_cov_at_tying_points;
    let tying_pts = Mitc3Plus.tying_points();

    // For each quadrature point (= tying point, weight=1/6, det2 is Jacobian),
    // compute the MITC3 projected B_s_phys (2×18) and accumulate K_s.
    // The MITC3 interpolation is linear: for each DOF column d, the projected
    // covariant strain is interpolate_assumed_shear(sampled_for_column_d, qp).
    // We handle this column-by-column for all 18 DOFs, building B_s_phys[2][18].

    let qp_weight = 1.0 / 6.0; // each of A, B, C has weight 1/6 (sum=1/2=ref-tri area)

    for qp in tying_pts.iter() {
        // Build projected covariant B_s at this quadrature point (2×NDOF).
        let mut b_s_cov_qp = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            // For column `dof`, the covariant strain at each tying point is b_cov[tp][comp][dof].
            // TyingShears flat fields: gamma_<comp>_at_<point> = b_cov[tp][component][dof].
            let sampled = TyingShears {
                gamma_xi_zeta_at_a: b_cov[0][0][dof],
                gamma_eta_zeta_at_b: b_cov[1][1][dof],
                gamma_xi_zeta_at_c: b_cov[2][0][dof],
                gamma_eta_zeta_at_c: b_cov[2][1][dof],
            };
            let projected = Mitc3Plus.interpolate_assumed_shear(sampled, qp.coord);
            b_s_cov_qp[0][dof] = projected.gamma_xi_zeta;
            b_s_cov_qp[1][dof] = projected.gamma_eta_zeta;
        }

        // Convert covariant to physical: b_s_phys = J2⁻ᵀ · b_s_cov
        let mut b_s_phys = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            b_s_phys[0][dof] = inv_t[0][0] * b_s_cov_qp[0][dof] + inv_t[0][1] * b_s_cov_qp[1][dof];
            b_s_phys[1][dof] = inv_t[1][0] * b_s_cov_qp[0][dof] + inv_t[1][1] * b_s_cov_qp[1][dof];
        }

        // Accumulate K_s += B_sᵀ · (κ·G·t) · B_s · det2 · weight
        let scale = kappa_g * t * det2 * qp_weight;
        for a in 0..NDOF {
            for b in 0..NDOF {
                let v = (b_s_phys[0][a] * b_s_phys[0][b] + b_s_phys[1][a] * b_s_phys[1][b]) * scale;
                k_loc[a][b] += v;
            }
        }
    }

    // ---- Symmetrize, then rotate local → global (shared helpers) ----
    // Each B^T·D·B contribution is symmetric in form; averaging the triangles
    // minimises residual asymmetry before the Rᵀ·K·R block rotation.
    symmetrize_in_place(&mut k_loc);
    let k_glob = rotate_local_to_global(&k_loc, &r);

    // Pack into ElementStiffness
    let mut k_e = ElementStiffness::zeros(NDOF);
    for i in 0..NDOF {
        for j in 0..NDOF {
            k_e.data[i * NDOF + j] = k_glob[i][j];
        }
    }
    k_e
}

/// Compute the 18×18 element stiffness for the genuine flat-facet **MITC3+**
/// shell element (Lee, Lee & Bathe 2014) — the transverse-shear-locking cure
/// and sibling of the bare-MITC3 [`shell_element_stiffness`].
///
/// `nodes`, `thickness`, `material`, the DOF ordering, the drilling-rotation
/// singularity, and the local→global rotation are all exactly as documented on
/// [`shell_element_stiffness`]; only the transverse-shear treatment differs.
///
/// # Formulation (element-local, flat facet, constant Jacobian)
///
/// The section rotations are enriched by a cubic bubble `f_b = ξη(1−ξ−η)` tied
/// to an internal centroid node, adding 2 internal rotational DOFs
/// `(Δβ_x, Δβ_y)` → a 20×20 *uncondensed* local matrix, statically condensed:
///
/// ```text
/// K* = K_NN − K_NB · K_BB⁻¹ · K_BN        (18×18)
/// ```
///
/// ## Where the shear-locking cure lives (corrected — esc-3392)
///
/// On a flat, constant-Jacobian facet the cubic bubble is **inert in transverse
/// shear**: the nodal↔bubble shear coupling `K_NB^shear ≡ 0` (the
/// divergence-theorem result of task 3349, re-derived for the flat-facet shear
/// field — DD#2 retracted). The genuine MITC3+ shear-locking cure here lives
/// **entirely in the nodal assumed transverse-shear field**: the bare three-node
/// (DISP3) covariant shear is sampled at the six *interior* tying points A–F and
/// re-interpolated via the MITC3+ assumed field (Eq. 9,
/// [`Mitc3Plus::interpolate_assumed_shear_mitc3_plus`]). That field differs from
/// bare MITC3's edge-midpoint Eq. 5 by O(1) structural terms, so it is a softer,
/// patch-consistent, rigid-safe field — and `K*` is **not** bit-identical to
/// [`shell_element_stiffness`].
///
/// ## Block contents
///
/// - **K_NN** — the 18 nodal DOFs: membrane + nodal bending (both assembled by
///   the shared [`accumulate_membrane_k`] / [`accumulate_bending_k_nodal`]
///   helpers, so bit-identical to bare MITC3 *by construction*) + the **MITC3+
///   interior-tying assumed-shear** block (the A–F / Eq. 9 field; softer than and
///   distinct from bare MITC3's edge-midpoint block). This is what `K*` reduces
///   to (see K_NB below).
/// - **K_BB** — the bubble bending self-stiffness `∫ ∇f_b·D_b·∇f_b`; SPD, so the
///   2×2 inverse used by the condensation is well-posed.
/// - **K_NB / K_BN** — bubble↔nodal coupling, **identically zero on a flat
///   facet** in *both* bending (`∫_T ∇f_b dA = ∮ f_b·n ds = 0`, divergence
///   theorem) *and* shear (bubble out of the shear field). The static
///   condensation correction `K_NB·K_BB⁻¹·K_BN` is therefore zero and
///   `K* = K_NN` exactly. The 20×20 skeleton + 2×2 condensation are retained for
///   faithfulness to the formulation and as the substrate the curved/director
///   element (task 4065) activates — there `K_NB ≠ 0` and the bubble does work.
///
/// ## Patch-test consistency
///
/// A *constant* transverse-shear state (e.g. uniform `θ_y`, `w = 0`) samples to a
/// constant covariant shear at all six interior points; Eq. 9 reproduces a
/// constant field exactly (`ĉ = 0`), so the nodal assumed-shear block reproduces
/// the constant state and the membrane/bending patch tests are inherited from
/// bare MITC3. Rigid-body modes give identically-zero covariant shear ⇒ the
/// assumed field vanishes ⇒ the 6 rigid null modes are preserved.
#[allow(clippy::needless_range_loop)]
pub fn shell_element_stiffness_mitc3_plus(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
) -> ElementStiffness {
    use crate::elements::mitc3_plus::{Mitc3Plus, ShearStrain};
    assert!(
        thickness > 0.0,
        "shell_element_stiffness_mitc3_plus: thickness must be positive, got {thickness}"
    );
    const NDOF: usize = Mitc3Plus::N_DOFS; // 18 nodal DOFs
    const NU: usize = Mitc3Plus::N_DOFS_UNCONDENSED; // 20 = 18 + 2 bubble DOFs
    const BX: usize = NDOF; // 18 = Δβ_x bubble column
    const BY: usize = NDOF + 1; // 19 = Δβ_y bubble column
    // (Per-node / node counts live in the shared assembly helpers.)

    let frame = build_shell_frame(nodes);
    let r = frame.r;
    let area = frame.area;
    let t = thickness;
    let d_pl = plane_stress_d(material);

    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let g = e / (2.0 * (1.0 + nu));
    let kappa_g = KAPPA * g;

    let kin = crate::shell_kinematics::shell_kinematics(nodes, &frame);
    let dn = kin.dn;
    let det2 = kin.det2;
    let inv_t = kin.jac2_inv_t;

    // --- 20×20 uncondensed local matrix ---
    let mut k = [[0.0_f64; NU]; NU];

    // ---- Membrane K (nodal only; the bubble does not enter membrane) ----
    // Shared helper → bit-identical to bare MITC3's membrane block by construction.
    let t_dpl = {
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                td[i][j] = t * d_pl[i][j];
            }
        }
        td
    };
    accumulate_membrane_k(&mut k, &dn, &t_dpl, area);

    // ---- Bending K ----
    let t3_dpl = {
        let factor = t * t * t / 12.0;
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                td[i][j] = factor * d_pl[i][j];
            }
        }
        td
    };
    // Nodal bending K_NN — shared helper → bit-identical to bare MITC3.
    accumulate_bending_k_nodal(&mut k, &dn, &t3_dpl, area);

    // Interior tying points + their reference quadrature weight (weights sum to
    // the reference-triangle area 1/2; `× det2` maps to physical area).
    let interior = Mitc3Plus.interior_tying_points();
    let w_ref = 0.5 / (interior.len() as f64);

    // Bubble bending self-term K_BB^bend. The nodal↔bubble bending coupling is
    // identically zero on a flat facet (divergence theorem; see doc above), so
    // it is intentionally not assembled.
    //
    // INTENTIONALLY UNUSED NUMERIC VALUE: because K_NB ≡ 0 here (see the
    // condensation site below), the static condensation is a no-op and K* = K_NN
    // — so K_BB's only consumer is the `det_bb > 0` SPD `debug_assert`. The
    // six-interior-point sum below is therefore NOT an exact quadrature rule for
    // the degree-4 integrand ∇f_b·D_b·∇f_b; it only needs to preserve positive
    // definiteness (sign), which it does. Task 4065 — where the curved/director
    // substrate makes K_NB ≠ 0 and K_BB's value actually feeds K* — must replace
    // this with an exact rule before trusting the numeric block.
    for tp in interior.iter() {
        let g_ref = Mitc3Plus.bubble_grad_at(tp.coord);
        // physical bubble gradient = J2⁻ᵀ · ∇_ref f_b
        let fbx = inv_t[0][0] * g_ref[0] + inv_t[0][1] * g_ref[1];
        let fby = inv_t[1][0] * g_ref[0] + inv_t[1][1] * g_ref[1];
        // bubble bending B columns (Δβ_x, Δβ_y), same κ convention as nodal θ_x/θ_y
        let bb = [[0.0, -fbx], [fby, 0.0], [fbx, -fby]];
        for a in 0..2 {
            for b in 0..2 {
                let mut v = 0.0;
                for rr in 0..3 {
                    for s in 0..3 {
                        v += bb[rr][a] * t3_dpl[rr][s] * bb[s][b];
                    }
                }
                let ca = if a == 0 { BX } else { BY };
                let cb = if b == 0 { BX } else { BY };
                k[ca][cb] += v * w_ref * det2;
            }
        }
    }

    // ---- MITC3+ transverse-shear K (Lee, Lee & Bathe 2014, Eq. 9) ----
    // On a flat facet the cubic bubble is INERT in transverse shear
    // (K_NB^shear ≡ 0; DD#2 retracted — esc-3392 corrected resolution). The
    // shear-locking cure lives entirely in the NODAL assumed field: sample the
    // bare three-node (DISP3) covariant transverse shear at the six interior
    // tying points A-F and re-interpolate via Eq. 9 — a softer field than bare
    // MITC3's edge-midpoint Eq. 5 (the difference has O(1) structural terms, so
    // K* is NOT bit-identical to bare MITC3). The bubble enters BENDING only
    // (K_BB^bend above), so the shear block touches the 18 nodal DOFs only and
    // the bubble columns (18, 19) of the shear stay zero ⇒ K_NB = 0.
    let mut b_tp = [[[0.0_f64; NDOF]; 2]; Mitc3Plus::N_INTERIOR_TYING_POINTS];
    for (kk, tp) in interior.iter().enumerate() {
        b_tp[kk] = Mitc3Plus.covariant_shear_b_nodal(tp.coord);
    }

    // The assumed covariant field (Eq. 9) is linear in (r,s), so the shear
    // energy integrand is quadratic; the symmetric 3-point rule at the interior
    // A, B, C orbit (= interior[0..3], each weight 1/6) integrates it exactly.
    let quad_pts = [interior[0].coord, interior[1].coord, interior[2].coord];
    let qp_weight = 1.0 / 6.0;
    for qp in quad_pts {
        // Assumed covariant shear B (2 × NDOF) at qp, column by column via Eq. 9.
        let mut b_cov_qp = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            let samples: [ShearStrain; Mitc3Plus::N_INTERIOR_TYING_POINTS] = [
                ShearStrain { gamma_xi_zeta: b_tp[0][0][dof], gamma_eta_zeta: b_tp[0][1][dof] },
                ShearStrain { gamma_xi_zeta: b_tp[1][0][dof], gamma_eta_zeta: b_tp[1][1][dof] },
                ShearStrain { gamma_xi_zeta: b_tp[2][0][dof], gamma_eta_zeta: b_tp[2][1][dof] },
                ShearStrain { gamma_xi_zeta: b_tp[3][0][dof], gamma_eta_zeta: b_tp[3][1][dof] },
                ShearStrain { gamma_xi_zeta: b_tp[4][0][dof], gamma_eta_zeta: b_tp[4][1][dof] },
                ShearStrain { gamma_xi_zeta: b_tp[5][0][dof], gamma_eta_zeta: b_tp[5][1][dof] },
            ];
            let proj = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, qp);
            b_cov_qp[0][dof] = proj.gamma_xi_zeta;
            b_cov_qp[1][dof] = proj.gamma_eta_zeta;
        }
        // covariant → physical via J2⁻ᵀ
        let mut b_phys = [[0.0_f64; NDOF]; 2];
        for dof in 0..NDOF {
            b_phys[0][dof] = inv_t[0][0] * b_cov_qp[0][dof] + inv_t[0][1] * b_cov_qp[1][dof];
            b_phys[1][dof] = inv_t[1][0] * b_cov_qp[0][dof] + inv_t[1][1] * b_cov_qp[1][dof];
        }
        let scale = kappa_g * t * det2 * qp_weight;
        for a in 0..NDOF {
            for b in 0..NDOF {
                k[a][b] += (b_phys[0][a] * b_phys[0][b] + b_phys[1][a] * b_phys[1][b]) * scale;
            }
        }
    }

    // ---- Static condensation of the 2 bubble DOFs ----
    // K* = K_NN − K_NB · K_BB⁻¹ · K_BN, closed-form 2×2 inverse of K_BB.
    //
    // PROVABLY A NO-OP ON A FLAT FACET: the bubble↔nodal coupling K_NB is never
    // assembled — membrane/bending touch only nodal DOFs, the bubble bending
    // self-term writes only the (BX,BY) block, and the bubble is inert in shear
    // (K_NB^shear ≡ 0; DD#2 retracted). So K_NB ≡ 0 (bit-zero), the correction
    // K_NB·K_BB⁻¹·K_BN is identically zero, and K* = K_NN exactly. The full 2×2
    // condensation is retained as faithful scaffolding for task 4065, where the
    // curved/director substrate makes K_NB ≠ 0 and the bubble does work.
    debug_assert!(
        (0..NDOF).all(|i| {
            k[i][BX].to_bits() == 0
                && k[i][BY].to_bits() == 0
                && k[BX][i].to_bits() == 0
                && k[BY][i].to_bits() == 0
        }),
        "MITC3+ flat-facet invariant: K_NB/K_BN must be bit-zero so condensation is a no-op (K* = K_NN)"
    );
    let k_bb = [[k[BX][BX], k[BX][BY]], [k[BY][BX], k[BY][BY]]];
    let det_bb = k_bb[0][0] * k_bb[1][1] - k_bb[0][1] * k_bb[1][0];
    debug_assert!(
        det_bb > 0.0,
        "MITC3+ bubble block K_BB must be SPD (det = {det_bb})"
    );
    let inv_bb = [
        [k_bb[1][1] / det_bb, -k_bb[0][1] / det_bb],
        [-k_bb[1][0] / det_bb, k_bb[0][0] / det_bb],
    ];
    let mut k_loc = [[0.0_f64; NDOF]; NDOF];
    for i in 0..NDOF {
        let nb_i = [k[i][BX], k[i][BY]]; // K_NB row i
        for j in 0..NDOF {
            let bn_j = [k[BX][j], k[BY][j]]; // K_BN col j
            let mut corr = 0.0;
            for p in 0..2 {
                for q in 0..2 {
                    corr += nb_i[p] * inv_bb[p][q] * bn_j[q];
                }
            }
            k_loc[i][j] = k[i][j] - corr;
        }
    }

    // ---- Symmetrize, then rotate local → global (shared helpers, identical to
    // the bare path — only the transverse-shear treatment differs) ----
    symmetrize_in_place(&mut k_loc);
    let k_glob = rotate_local_to_global(&k_loc, &r);

    let mut k_e = ElementStiffness::zeros(NDOF);
    for i in 0..NDOF {
        for j in 0..NDOF {
            k_e.data[i * NDOF + j] = k_glob[i][j];
        }
    }
    k_e
}

/// Compute the 18×18 element stiffness for the **degenerated (continuum-based)
/// shell** element (task 4068): per-node directors + a varying element Jacobian,
/// carrying the MITC3+ assumed transverse-shear field (task 3392).
///
/// `nodes` are the three mid-surface vertex positions (global coords),
/// `directors` the per-node unit directors `V_i` (provenance-agnostic — supplied
/// explicitly by the caller; see [`crate::elements::degenerate_shell`] for the
/// neighbour-averaged fallback), `thicknesses` the per-node thicknesses `t_i`,
/// and `material` the isotropic linear-elastic law. DOF ordering, the drilling
/// singularity, and the [`ElementStiffness`] container are exactly as documented
/// on [`shell_element_stiffness`].
///
/// # Formulation (Ahmad/Bathe degenerated shell)
///
/// The element integrates `K = ∫_V Bᵀ D B dV` directly over the reference
/// triangle × `[-1, 1]`:
///
/// ```text
/// K = Σ_qp  w_inplane · w_ζ · ( B_mbᵀ D_pl B_mb  +  B_sᵀ (κG) B_s ) · det(J)
/// ```
///
/// - **B_mb** (3×18) — the membrane+bending strain–displacement operator from
///   the director-fibre displacement field pushed through `J⁻¹`
///   ([`crate::elements::degenerate_shell::degenerate_membrane_bending_b`]). The
///   through-thickness `ζ`-dependence makes a single operator carry *both*
///   membrane (ζ⁰) and bending (ζ²) — no separate `t` / `t³/12` split.
/// - **B_s** (2×18) — the carried MITC3+ interior-tying assumed transverse-shear
///   field, covariant→physical-mapped against the **varying** `J`
///   ([`crate::elements::degenerate_shell::degenerate_transverse_shear_b`]).
/// - **D_pl** (3×3) — the per-point local-lamina plane-stress law
///   ([`plane_stress_d`]); both B-matrices express strain in the same per-point
///   lamina frame, so the constitutive law applies without a separate rotation.
///
/// Because both B-matrices map **global** DOFs to lamina-frame strains, `K` is
/// assembled directly in the global frame — there is no `Rᵀ·K·R` step (unlike the
/// flat-facet siblings, which build `K` in a local frame first).
///
/// ## Quadrature
///
/// - In-plane: the symmetric 3-point interior-tying orbit (`interior[0..3]`,
///   weight `1/6` each) — the same rule [`shell_element_stiffness_mitc3_plus`]
///   uses for its shear, so it integrates the quadratic assumed-shear energy
///   exactly and reduces to the flat MITC3+ shear quadrature when flat.
/// - Through-thickness: 2-point Gauss in `ζ` (nodes `±1/√3`, weight `1`), exact
///   for the degree-≤2 `ζ`-integrand of a linear-director shell. (On a flat facet
///   `J` is `ζ`-invariant and this reproduces the closed-form `t` / `t³/12`
///   thickness integrals exactly — the flat-reduction anchor.)
///
/// # Retained MITC3+ skeleton
///
/// The 20×20 uncondensed skeleton + 2×2 bubble static condensation of
/// [`shell_element_stiffness_mitc3_plus`] is retained: the nodal block `K_NN` is
/// the degenerate integral above, the bubble bending self-term `K_BB` is carried
/// for a well-posed (SPD) condensation, and the nodal↔bubble coupling `K_NB` is
/// **zero** here (the carried nodal B-matrices do not write the bubble columns —
/// activating the bubble on the director substrate is task 4065's ANS-membrane).
/// So the closed-form condensation is a no-op and `K* = K_NN`, structurally
/// identical to the flat MITC3+ path.
#[allow(clippy::needless_range_loop)]
pub fn shell_element_stiffness_degenerate(
    nodes: &[[f64; 3]; 3],
    directors: &[crate::elements::degenerate_shell::Director; 3],
    thicknesses: &[f64; 3],
    material: &IsotropicElastic,
) -> ElementStiffness {
    use crate::elements::degenerate_shell::{
        ShellRefCoord3, degenerate_jacobian, degenerate_membrane_bending_b,
        degenerate_transverse_shear_b,
    };
    use crate::elements::mitc3_plus::Mitc3Plus;

    for (i, &t) in thicknesses.iter().enumerate() {
        assert!(
            t > 0.0,
            "shell_element_stiffness_degenerate: thickness[{i}] must be positive, got {t}"
        );
    }
    const NDOF: usize = Mitc3Plus::N_DOFS; // 18 nodal DOFs
    const NU: usize = Mitc3Plus::N_DOFS_UNCONDENSED; // 20 = 18 + 2 bubble DOFs
    const BX: usize = NDOF; // 18 = Δβ_x bubble column
    const BY: usize = NDOF + 1; // 19 = Δβ_y bubble column

    let d_pl = plane_stress_d(material);
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let g = e / (2.0 * (1.0 + nu));
    let kappa_g = KAPPA * g;

    // --- 20×20 uncondensed local matrix (nodal 18 + 2 bubble) ---
    let mut k = [[0.0_f64; NU]; NU];

    // ---- Nodal K_NN: numerically integrate membrane+bending + transverse shear
    // over the reference triangle × [-1, 1]. In-plane: the 3-point interior-tying
    // orbit (weight 1/6); through-thickness: 2-point Gauss in ζ (±1/√3, weight 1).
    let interior = Mitc3Plus.interior_tying_points();
    let inplane = [interior[0].coord, interior[1].coord, interior[2].coord];
    let w_inplane = 1.0 / 6.0;
    let zeta_node = 1.0 / 3.0_f64.sqrt();
    let zeta_gauss = [-zeta_node, zeta_node];
    let w_zeta = 1.0_f64;

    for ip in inplane.iter() {
        for &zeta in zeta_gauss.iter() {
            let c3 = ShellRefCoord3::new(ip.xi, ip.eta, zeta);
            let (_jm, det) = degenerate_jacobian(nodes, directors, thicknesses, c3);
            let b_mb = degenerate_membrane_bending_b(nodes, directors, thicknesses, c3);
            let b_s = degenerate_transverse_shear_b(nodes, directors, thicknesses, c3);
            let scale = w_inplane * w_zeta * det;

            // D_pl · B_mb (3×18), reused across the symmetric outer product.
            let mut db = [[0.0_f64; NDOF]; 3];
            for r in 0..3 {
                for col in 0..NDOF {
                    db[r][col] = d_pl[r][0] * b_mb[0][col]
                        + d_pl[r][1] * b_mb[1][col]
                        + d_pl[r][2] * b_mb[2][col];
                }
            }
            for a in 0..NDOF {
                for b in 0..NDOF {
                    // membrane+bending: B_mbᵀ · (D_pl · B_mb)
                    let mut v = b_mb[0][a] * db[0][b]
                        + b_mb[1][a] * db[1][b]
                        + b_mb[2][a] * db[2][b];
                    // transverse shear: B_sᵀ · (κG·I₂) · B_s
                    v += kappa_g * (b_s[0][a] * b_s[0][b] + b_s[1][a] * b_s[1][b]);
                    k[a][b] += v * scale;
                }
            }
        }
    }

    // ---- Bubble bending self-term K_BB (retained 3392 skeleton) ----
    // Computed from the flat mid-surface kinematics (three nodes are always
    // coplanar, so build_shell_frame is well-posed). On a flat facet this equals
    // the MITC3+ K_BB exactly; on a curved-director patch it is a well-posed SPD
    // block. Either way K_NB ≡ 0 (the nodal B-matrices above never touch the
    // bubble columns), so K_BB is condensed away — its only role here is a
    // well-posed (SPD) static condensation. Bubble activation (K_NB ≠ 0) is the
    // ANS-membrane work of task 4065.
    let frame = build_shell_frame(nodes);
    let kin = crate::shell_kinematics::shell_kinematics(nodes, &frame);
    let inv_t = kin.jac2_inv_t;
    let det2 = kin.det2;
    let t_avg = (thicknesses[0] + thicknesses[1] + thicknesses[2]) / 3.0;
    let t3_dpl = {
        let factor = t_avg * t_avg * t_avg / 12.0;
        let mut td = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                td[i][j] = factor * d_pl[i][j];
            }
        }
        td
    };
    let w_ref = 0.5 / (interior.len() as f64);
    for tp in interior.iter() {
        let g_ref = Mitc3Plus.bubble_grad_at(tp.coord);
        // physical bubble gradient = J2⁻ᵀ · ∇_ref f_b
        let fbx = inv_t[0][0] * g_ref[0] + inv_t[0][1] * g_ref[1];
        let fby = inv_t[1][0] * g_ref[0] + inv_t[1][1] * g_ref[1];
        let bb = [[0.0, -fbx], [fby, 0.0], [fbx, -fby]];
        for a in 0..2 {
            for b in 0..2 {
                let mut v = 0.0;
                for rr in 0..3 {
                    for s in 0..3 {
                        v += bb[rr][a] * t3_dpl[rr][s] * bb[s][b];
                    }
                }
                let ca = if a == 0 { BX } else { BY };
                let cb = if b == 0 { BX } else { BY };
                k[ca][cb] += v * w_ref * det2;
            }
        }
    }

    // ---- Static condensation of the 2 bubble DOFs (no-op here: K_NB ≡ 0) ----
    // K* = K_NN − K_NB · K_BB⁻¹ · K_BN. The nodal B-matrices never write the
    // bubble columns, so K_NB/K_BN are bit-zero and the correction vanishes
    // (K* = K_NN). The full 2×2 condensation is retained as faithful scaffolding
    // for task 4065, where the bubble couples (K_NB ≠ 0) and does work.
    debug_assert!(
        (0..NDOF).all(|i| {
            k[i][BX].to_bits() == 0
                && k[i][BY].to_bits() == 0
                && k[BX][i].to_bits() == 0
                && k[BY][i].to_bits() == 0
        }),
        "degenerate flat-skeleton invariant: K_NB/K_BN must be bit-zero so condensation is a no-op (K* = K_NN)"
    );
    let k_bb = [[k[BX][BX], k[BX][BY]], [k[BY][BX], k[BY][BY]]];
    let det_bb = k_bb[0][0] * k_bb[1][1] - k_bb[0][1] * k_bb[1][0];
    debug_assert!(
        det_bb > 0.0,
        "degenerate bubble block K_BB must be SPD (det = {det_bb})"
    );
    let inv_bb = [
        [k_bb[1][1] / det_bb, -k_bb[0][1] / det_bb],
        [-k_bb[1][0] / det_bb, k_bb[0][0] / det_bb],
    ];
    let mut k_loc = [[0.0_f64; NDOF]; NDOF];
    for i in 0..NDOF {
        let nb_i = [k[i][BX], k[i][BY]]; // K_NB row i
        for j in 0..NDOF {
            let bn_j = [k[BX][j], k[BY][j]]; // K_BN col j
            let mut corr = 0.0;
            for p in 0..2 {
                for q in 0..2 {
                    corr += nb_i[p] * inv_bb[p][q] * bn_j[q];
                }
            }
            k_loc[i][j] = k[i][j] - corr;
        }
    }

    // ---- Symmetrize and pack ----
    // The degenerate B-matrices map global DOFs to lamina-frame strains, so K is
    // already in the global frame — there is NO local→global rotation (unlike the
    // flat-facet siblings). Each Bᵀ·D·B contribution is symmetric in form;
    // averaging the triangles minimises residual asymmetry before packing.
    symmetrize_in_place(&mut k_loc);

    let mut k_e = ElementStiffness::zeros(NDOF);
    for i in 0..NDOF {
        for j in 0..NDOF {
            k_e.data[i * NDOF + j] = k_loc[i][j];
        }
    }
    k_e
}

/// Rotation matrix Q = Ry(45°) · Rz(30°) used as a shared test fixture by
/// both `shell_assembly` and `shell_boundary` tests.
///
/// The orientation was chosen so that no entry of Q is zero: the drilling
/// singularity mixes across all rotational DOFs, and every diagonal entry of
/// the rotated K is nonzero — avoiding the axis-aligned singularity of an
/// xy-plane `UNIT_TRI`.
///
/// **Single source of truth.** Both test modules import this function.  Edit
/// the rotation here and the change propagates to `shell_boundary` automatically.
#[cfg(test)]
pub(crate) fn tilted_q_for_shell_tests() -> [[f64; 3]; 3] {
    let cos30 = (30.0_f64.to_radians()).cos();
    let sin30 = (30.0_f64.to_radians()).sin();
    let cos45 = (45.0_f64.to_radians()).cos();
    let sin45 = (45.0_f64.to_radians()).sin();
    let rz: [[f64; 3]; 3] = [[cos30, -sin30, 0.0], [sin30, cos30, 0.0], [0.0, 0.0, 1.0]];
    let ry: [[f64; 3]; 3] = [[cos45, 0.0, sin45], [0.0, 1.0, 0.0], [-sin45, 0.0, cos45]];
    mat3_mul(&ry, &rz)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
#[allow(clippy::identity_op)] // explicit `ndp * node + dof` form mirrors the DOF layout
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;
    use crate::elements::mitc3_plus::Mitc3Plus;

    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    /// Assert that an N×N matrix is entry-wise finite and symmetric.
    ///
    /// Symmetry tolerance: `|D[i][j] − D[j][i]| < 1e-9 · max(|D[i][j]|, |D[j][i]|, 1)`.
    fn assert_symmetric_finite<const N: usize>(d: &[[f64; N]; N]) {
        for i in 0..N {
            for j in 0..N {
                assert!(
                    d[i][j].is_finite(),
                    "D[{i}][{j}] = {} is not finite",
                    d[i][j]
                );
                let lhs = d[i][j];
                let rhs = d[j][i];
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

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

    const WIDE_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];

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
            d[0][0] * eps[0] + d[0][1] * eps[1],
            d[1][0] * eps[0] + d[1][1] * eps[1],
            0.0,
        ];
        let area = 0.5_f64;
        let u_analytical = 0.5 * (eps[0] * d_eps[0] + eps[1] * d_eps[1]) * t * area;

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
                let dx = [
                    nodes[node][0] - c[0],
                    nodes[node][1] - c[1],
                    nodes[node][2] - c[2],
                ];
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

    // ====================================================================
    // MITC3+ element (Lee, Lee & Bathe 2014): genuine flat-facet shell with
    // a rotation-bubble + interior-tying assumed transverse-shear field and
    // static condensation of the 2 internal bubble DOFs. Implemented as a
    // sibling of the bare-MITC3 `shell_element_stiffness` above so the bare
    // path stays available as the comparison baseline (see plan task 3392).
    // ====================================================================

    // --- MITC3+ shape + rigid-body null space (task 3392 step-7) ---

    #[test]
    fn mitc3_plus_stiffness_is_18x18_symmetric_finite_with_rigid_body_null_space() {
        let mat = steel_like();
        let t = 0.05_f64;
        let k = shell_element_stiffness_mitc3_plus(&UNIT_TRI, t, &mat);

        // Shape: after static condensation of the 2 bubble DOFs the element
        // matrix is the standard 18×18 (324 entries), same size as bare MITC3.
        assert_eq!(
            k.n_dofs,
            Mitc3Plus::N_DOFS,
            "MITC3+ must condense to N_DOFS = 18"
        );
        assert_eq!(
            k.data.len(),
            Mitc3Plus::N_DOFS * Mitc3Plus::N_DOFS,
            "MITC3+ data length must be 18×18 = 324"
        );

        // Symmetric within fp tolerance and entrywise finite.
        for i in 0..Mitc3Plus::N_DOFS {
            for j in 0..Mitc3Plus::N_DOFS {
                let kij = k.get(i, j);
                let kji = k.get(j, i);
                assert!(kij.is_finite(), "MITC3+ K[{i}][{j}] = {kij} is not finite");
                let scale = kij.abs().max(kji.abs()).max(1.0);
                assert!(
                    (kij - kji).abs() < 1e-9 * scale,
                    "MITC3+ asymmetry at ({i},{j}): {kij} vs {kji}",
                );
            }
        }

        // Rigid-body null space: the 3 rigid translations and 3 rigid rotations
        // must annihilate K (within a k_max-relative tolerance). Static
        // condensation preserves these modes because rigid modes are strain-free
        // (K·u_rigid = 0 in every block, including K_BN). Mirrors the bare-MITC3
        // null-space tests above.
        let k_max = k.data.iter().copied().fold(0.0_f64, |a, x| a.max(x.abs()));
        let tol = 1e-9 * k_max.max(1.0);

        // (a) Rigid translations along each global axis.
        for axis in 0..3 {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                u[Mitc3Plus::N_DOFS_PER_NODE * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < tol,
                "MITC3+ translation axis {axis}: linf(K·u) = {}, tol = {tol}",
                linf(&ku),
            );
        }

        // (b) Rigid rotations about the centroid: u_i = ω × (x_i − c), θ_i = ω.
        let c = [1.0 / 3.0_f64, 1.0 / 3.0, 0.0_f64];
        let omega = [[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for &w in &omega {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                let dx = [
                    UNIT_TRI[node][0] - c[0],
                    UNIT_TRI[node][1] - c[1],
                    UNIT_TRI[node][2] - c[2],
                ];
                let ndp = Mitc3Plus::N_DOFS_PER_NODE;
                u[ndp * node + 0] = w[1] * dx[2] - w[2] * dx[1];
                u[ndp * node + 1] = w[2] * dx[0] - w[0] * dx[2];
                u[ndp * node + 2] = w[0] * dx[1] - w[1] * dx[0];
                u[ndp * node + 3] = w[0];
                u[ndp * node + 4] = w[1];
                u[ndp * node + 5] = w[2];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < tol,
                "MITC3+ rotation ω={w:?}: linf(K·u) = {}, tol = {tol}",
                linf(&ku),
            );
        }
    }

    // --- MITC3+ patch tests + isotropy + no-spurious-modes (task 3392 step-9) ---

    #[test]
    fn mitc3_plus_membrane_patch_test_matches_analytical_energy() {
        // Uniform in-plane strain u_x = a·x, u_y = b·y → ε = (a, b, 0). The
        // bubble does not enter membrane and a constant membrane state does not
        // excite it, so MITC3+ reproduces the analytical membrane energy exactly.
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.01_f64;
        let b = -0.005_f64;
        let k = shell_element_stiffness_mitc3_plus(&UNIT_TRI, t, &mat);

        const NDP: usize = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        u[NDP * 1] = a; // node1 at x=1 → u_x = a
        u[NDP * 2 + 1] = b; // node2 at y=1 → u_y = b

        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let d = plane_stress_d(&mat);
        let eps = [a, b, 0.0_f64];
        let d_eps = [
            d[0][0] * eps[0] + d[0][1] * eps[1],
            d[1][0] * eps[0] + d[1][1] * eps[1],
            0.0,
        ];
        let area = 0.5_f64;
        let u_analytical = 0.5 * (eps[0] * d_eps[0] + eps[1] * d_eps[1]) * t * area;
        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "MITC3+ membrane patch: U_K={u_k}, U_analytical={u_analytical}",
        );
    }

    #[test]
    fn mitc3_plus_shear_patch_test_uniform_theta_y_matches_analytical_energy() {
        // Uniform θ_y = α → uniform covariant transverse-shear state. A constant
        // covariant shear samples identically at all six interior tying points,
        // so the MITC3+ assumed field (Eq. 9) has twist ĉ = 0 and reproduces the
        // constant state exactly; the nodal assumed-shear block therefore yields
        // the analytical shear energy 0.5·α²·κ·G·t·A.
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.003_f64;
        let k = shell_element_stiffness_mitc3_plus(&UNIT_TRI, t, &mat);

        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            u[Mitc3Plus::N_DOFS_PER_NODE * node + 4] = alpha; // θ_y
        }
        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let gmod = e / (2.0 * (1.0 + nu));
        let kappa = 5.0_f64 / 6.0;
        let area = 0.5_f64;
        let u_analytical = 0.5 * alpha * alpha * kappa * gmod * t * area;
        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "MITC3+ shear patch: U_K={u_k}, U_analytical={u_analytical}",
        );
    }

    #[test]
    fn mitc3_plus_strain_energy_invariant_under_global_rigid_rotation() {
        // Frame covariance / isotropy: ½uᵀKu invariant under a global rotation Q
        // of nodes + DOFs (MITC3+ is built in the local frame and block-rotated,
        // exactly as bare MITC3). Mirrors the bare-MITC3 frame-covariance test.
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.01_f64;
        let b = -0.005_f64;
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u_orig = [0.0_f64; Mitc3Plus::N_DOFS];
        u_orig[ndp * 1] = a;
        u_orig[ndp * 2 + 1] = b;
        let k_orig = shell_element_stiffness_mitc3_plus(&UNIT_TRI, t, &mat);
        let ku_orig = matvec(&k_orig, &u_orig);
        let u_k_orig: f64 = 0.5
            * ku_orig
                .iter()
                .zip(u_orig.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();

        let q = super::tilted_q_for_shell_tests();
        let mut rot_nodes = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                rot_nodes[ni][i] = q[i][0] * node[0] + q[i][1] * node[1] + q[i][2] * node[2];
            }
        }
        let mut u_rot = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            for triple in 0..2 {
                let off = ndp * node + 3 * triple;
                let v = [u_orig[off], u_orig[off + 1], u_orig[off + 2]];
                for i in 0..3 {
                    u_rot[off + i] = q[i][0] * v[0] + q[i][1] * v[1] + q[i][2] * v[2];
                }
            }
        }
        let k_rot = shell_element_stiffness_mitc3_plus(&rot_nodes, t, &mat);
        let ku_rot = matvec(&k_rot, &u_rot);
        let u_k_rot: f64 = 0.5
            * ku_rot
                .iter()
                .zip(u_rot.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();
        let scale = u_k_orig.abs().max(1.0);
        assert!(
            (u_k_orig - u_k_rot).abs() < 1e-9 * scale,
            "MITC3+ frame covariance: U_orig={u_k_orig}, U_rot={u_k_rot}",
        );
    }

    #[test]
    fn mitc3_plus_strain_modes_have_strictly_positive_energy() {
        // No-spurious-modes proxy: representative membrane, bending, and shear
        // strain modes each carry strictly positive energy. K* is PSD (the
        // Schur complement of the PSD uncondensed matrix w.r.t. the SPD bubble
        // block) and these modes lie outside the rigid-body/drilling null space.
        let mat = steel_like();
        let t = 0.05_f64;
        let k = shell_element_stiffness_mitc3_plus(&UNIT_TRI, t, &mat);
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;

        let mut u_m = [0.0_f64; Mitc3Plus::N_DOFS]; // membrane: u_x at node1
        u_m[ndp] = 1.0;
        let mut u_b = [0.0_f64; Mitc3Plus::N_DOFS]; // bending: θ_y at node1
        u_b[ndp + 4] = 1.0;
        let mut u_s = [0.0_f64; Mitc3Plus::N_DOFS]; // shear: u_z at node1
        u_s[ndp + 2] = 1.0;

        for (name, u) in [("membrane", u_m), ("bending", u_b), ("shear", u_s)] {
            let ku = matvec(&k, &u);
            let energy: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();
            assert!(
                energy > 1e-12,
                "MITC3+ {name} strain-mode energy = {energy}, expected strictly > 0",
            );
        }
    }

    // --- MITC3+ core mechanism: not bit-identical + shear-softening (task 3392 step-11) ---

    /// A distorted (non-right-isosceles) flat triangle in the xy-plane, to
    /// exercise the MITC3+ mechanism on a non-trivial constant Jacobian.
    const DISTORTED_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.3, 0.0, 0.0], [0.4, 0.9, 0.0]];

    #[test]
    fn mitc3_plus_is_not_bit_identical_to_bare_mitc3() {
        // MITC3+ swaps bare MITC3's edge-midpoint Eq. 5 assumed-shear field for
        // the interior-tying Eq. 9 field (six interior points A–F) — a different,
        // softer transverse-shear block. THAT swapped shear block (NOT a live
        // shear bubble: K_NB^shear ≡ 0 here, so condensation is a no-op and
        // K* = K_NN) makes at least one entry of K_mitc3+ differ from bare MITC3
        // by more than fp tolerance. Refutes the old "flat-facet bubble enrichment
        // is bit-identical to bare MITC3" claim (task 3349).
        let mat = steel_like();
        let t = 0.05_f64;
        for nodes in [&UNIT_TRI, &DISTORTED_TRI] {
            let k_bare = shell_element_stiffness(nodes, t, &mat);
            let k_plus = shell_element_stiffness_mitc3_plus(nodes, t, &mat);
            let k_max = k_bare
                .data
                .iter()
                .copied()
                .fold(0.0_f64, |a, x| a.max(x.abs()));
            let tol = 1e-9 * k_max.max(1.0);
            let max_diff = k_bare
                .data
                .iter()
                .zip(k_plus.data.iter())
                .fold(0.0_f64, |acc, (a, b)| acc.max((a - b).abs()));
            assert!(
                max_diff > tol,
                "MITC3+ must differ from bare MITC3 (max|ΔK| = {max_diff}, tol = {tol})",
            );
        }
    }

    #[test]
    fn mitc3_plus_softens_shear_dominated_modes_relative_to_bare() {
        // MITC3+ swaps bare MITC3's edge-midpoint Eq. 5 shear block for the
        // interior-tying Eq. 9 field (condensation is a no-op here: K_NB ≡ 0 ⇒
        // K* = K_NN). The Eq. 9 field under-represents the parasitic linear shear
        // these bending/shear-dominated modes induce, so it stores ≤ the energy of
        // the Eq. 5 block for the modes exercised below — the shear-locking-relief
        // gate. NOTE: this per-mode inequality is an EMPIRICALLY OBSERVED bound for
        // these specific modes, NOT a Schur-complement / general-theorem guarantee
        // (there is no proof that the Eq. 9 block is energy-≤ the Eq. 5 block for
        // every conceivable mode); it would need re-checking if the assumed field
        // is ever retuned.
        let mat = steel_like();
        let t = 0.05_f64;
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;

        // A battery of bending/shear-dominated nodal modes. θ_x at node1 induces a
        // linearly-varying covariant shear; the Eq. 9 interior-tying field (twist
        // ĉ ≠ 0) under-represents it relative to bare MITC3's edge-midpoint Eq. 5
        // field, giving strictly lower stored energy for ≥ 1 of these modes.
        let mut m1 = [0.0_f64; Mitc3Plus::N_DOFS];
        m1[ndp + 3] = 1.0; // θ_x at node1
        let mut m2 = [0.0_f64; Mitc3Plus::N_DOFS];
        m2[2 * ndp + 4] = 1.0; // θ_y at node2
        let mut m3 = [0.0_f64; Mitc3Plus::N_DOFS];
        m3[ndp + 2] = 1.0; // u_z node1
        m3[2 * ndp + 3] = 0.5; // θ_x node2
        m3[ndp + 4] = -0.7; // θ_y node1
        let modes = [m1, m2, m3];

        let energy = |k: &ElementStiffness, u: &[f64; Mitc3Plus::N_DOFS]| -> f64 {
            0.5 * matvec(k, u).iter().zip(u.iter()).map(|(a, b)| a * b).sum::<f64>()
        };

        for nodes in [&UNIT_TRI, &DISTORTED_TRI] {
            let k_bare = shell_element_stiffness(nodes, t, &mat);
            let k_plus = shell_element_stiffness_mitc3_plus(nodes, t, &mat);
            let mut any_strictly_softer = false;
            for u in &modes {
                let e_bare = energy(&k_bare, u);
                let e_plus = energy(&k_plus, u);
                let scale = e_bare.abs().max(1.0);
                assert!(
                    e_plus <= e_bare + 1e-9 * scale,
                    "MITC3+ must not stiffen: e_plus={e_plus} > e_bare={e_bare}",
                );
                if (e_bare - e_plus) / scale > 1e-6 {
                    any_strictly_softer = true;
                }
            }
            assert!(
                any_strictly_softer,
                "MITC3+ must be strictly softer than bare for ≥1 shear/bending mode \
                 (nodes = {nodes:?})",
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

    /// Verifies the bending+shear energy partition by checking that K-energy at `t` and `2t`
    /// both match the analytical `c_b·t³ + c_s·t` formula, and that the measured ratio
    /// `U(2t)/U(t)` matches the analytical ratio.
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
            let uk1: f64 = 0.5
                * ku1
                    .iter()
                    .zip(u.iter())
                    .map(|(ki, ui)| ki * ui)
                    .sum::<f64>();
            let uk2: f64 = 0.5
                * ku2
                    .iter()
                    .zip(u.iter())
                    .map(|(ki, ui)| ki * ui)
                    .sum::<f64>();

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

    // --- Fixture invariants (step 1) ---

    /// Verify that `tilted_q_for_shell_tests()` returns a proper rotation matrix:
    /// each row is a unit vector, rows are mutually orthogonal, and det = +1.
    #[test]
    fn tilted_q_for_shell_tests_returns_orthonormal_rotation() {
        let q = super::tilted_q_for_shell_tests();

        // (a) Each row has unit Euclidean norm.
        for (i, row) in q.iter().enumerate() {
            let norm_sq: f64 = row.iter().map(|x| x * x).sum();
            assert!(
                (norm_sq - 1.0).abs() < 1e-12,
                "row {i}: ||row||^2 = {norm_sq} (expected 1.0)"
            );
        }

        // (b) All pairs of distinct rows are mutually orthogonal.
        for i in 0..3 {
            for j in (i + 1)..3 {
                let dot: f64 = (0..3).map(|k| q[i][k] * q[j][k]).sum();
                assert!(
                    dot.abs() < 1e-12,
                    "rows {i} and {j}: dot = {dot} (expected 0.0)"
                );
            }
        }

        // (c) det(Q) = +1 (right-handed / proper rotation).
        let det = q[0][0] * (q[1][1] * q[2][2] - q[1][2] * q[2][1])
            - q[0][1] * (q[1][0] * q[2][2] - q[1][2] * q[2][0])
            + q[0][2] * (q[1][0] * q[2][1] - q[1][1] * q[2][0]);
        assert!((det - 1.0).abs() < 1e-12, "det(Q) = {det} (expected +1.0)");
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
        let u_k_orig: f64 = 0.5
            * ku_orig
                .iter()
                .zip(u_orig.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();

        // Global rotation Q = Ry(45°) · Rz(30°) — shared fixture, single source of
        // truth in `super::tilted_q_for_shell_tests`.
        let q = super::tilted_q_for_shell_tests();

        // Rotate nodes
        let mut rot_nodes = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                rot_nodes[ni][i] = q[i][0] * node[0] + q[i][1] * node[1] + q[i][2] * node[2];
            }
        }

        // Rotate DOFs: each (u triple) and (θ triple) by Q
        let ndp = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u_rot = [0.0_f64; Mitc3Plus::N_DOFS];
        for node in 0..Mitc3Plus::N_NODES {
            for triple in 0..2 {
                // 0=displacements, 1=rotations
                let off = ndp * node + 3 * triple;
                let v = [u_orig[off], u_orig[off + 1], u_orig[off + 2]];
                for i in 0..3 {
                    u_rot[off + i] = q[i][0] * v[0] + q[i][1] * v[1] + q[i][2] * v[2];
                }
            }
        }

        let k_rot = shell_element_stiffness(&rot_nodes, t, &mat);
        let ku_rot = matvec(&k_rot, &u_rot);
        let u_k_rot: f64 = 0.5
            * ku_rot
                .iter()
                .zip(u_rot.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();

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
        // Curvature κ_xx = -α (uniform), MITC3 projects γ_xz to constant α/2.
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
                assert!(dot.abs() < 1e-12, "rows {i} · {j} = {dot}, expected 0",);
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

    // --- Tilted-frame block rotation test ---

    #[test]
    fn shell_tilted_frame_block_rotation_matches_q_kflat_qt() {
        // # Why this test exists
        //
        // `shell_element_stiffness` assembles K in a local mid-surface frame and
        // rotates each 3×3 sub-block to global via:
        //
        //   K_global[a..a+3, b..b+3] = Rᵀ · K_loc[a..a+3, b..b+3] · R
        //
        // where `R = frame.r` has rows = local basis vectors expressed in global
        // coordinates (`x_local = R · x_global`).
        //
        // All four existing patch tests (membrane, shear, bending, thickness) use
        // UNIT_TRI in the xy-plane, giving R = I, so the rotation block is a no-op.
        // The existing `shell_strain_energy_invariant_under_global_rigid_rotation`
        // checks only the scalar ½ uᵀ K u: because K is symmetric, a transpose flip
        // (R · sub · Rᵀ instead of Rᵀ · sub · R) leaves energy invariant for any
        // symmetric K and is therefore invisible to that test.
        //
        // # Why the equality K_tilted[block] = Q · K_flat[block] · Qᵀ holds
        //
        // For the tilted triangle Q·UNIT_TRI, `build_shell_frame` produces
        //   e1_tilted = Q·e_x,  e2_tilted = Q·e_y,  e3_tilted = Q·e_z
        // so R_tilted[i][j] = Q[j][i], i.e. R_tilted = Qᵀ.
        //
        // The local 2D coordinates used to build K_local are
        //   x_loc = R_tilted · (Q·node) = Qᵀ·Q·node = node
        // so K_local_tilted ≡ K_local_flat (up to float rounding).
        //
        // Therefore:
        //   K_global_flat   = Iᵀ · K_local · I             = K_local
        //   K_global_tilted = (Qᵀ)ᵀ · K_local · Qᵀ        = Q · K_local · Qᵀ
        //                   = Q · K_global_flat · Qᵀ
        //
        // # What bugs this catches that the energy invariant misses
        //
        // If the production loop applied R · sub · Rᵀ (transpose flip), the tilted
        // result would be Qᵀ · K_local · Q ≠ Q · K_local · Qᵀ for the generic
        // Q = Ry(45°)·Rz(30°) used here (no zero entries in any row/column).
        // A sign flip on any row or column of R propagates to a specific 3×3 block
        // entry and is equally caught by per-entry comparison.

        let mat = steel_like();
        let t = 0.05_f64;

        // --- Step 1: flat (xy-plane) stiffness; R_flat = I, so K_flat = K_local ---
        let k_flat = shell_element_stiffness(&UNIT_TRI, t, &mat);

        // --- Step 2: build Q = Ry(45°) · Rz(30°) — shared fixture ---
        let q = super::tilted_q_for_shell_tests();

        // --- Step 3: tilt the triangle nodes by Q ---
        let mut tilted_nodes = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                tilted_nodes[ni][i] = q[i][0] * node[0] + q[i][1] * node[1] + q[i][2] * node[2];
            }
        }

        // --- Step 4: stiffness for tilted triangle ---
        let k_tilted = shell_element_stiffness(&tilted_nodes, t, &mat);

        // --- Step 5: pre-compute Qᵀ ---
        let qt = [
            [q[0][0], q[1][0], q[2][0]],
            [q[0][1], q[1][1], q[2][1]],
            [q[0][2], q[1][2], q[2][2]],
        ];

        // --- Step 6: per-block assertion: K_tilted[bi,bj] == Q · K_flat[bi,bj] · Qᵀ ---
        // n_blocks = 2 * N_NODES = 6 (displacement triple + rotation triple per node),
        // mirroring the rotation-block loop in shell_element_stiffness (lines 464-486).
        let n_blocks = 2 * Mitc3Plus::N_NODES; // 6
        for bi in 0..n_blocks {
            for bj in 0..n_blocks {
                // Extract 3×3 sub-block from k_flat
                let mut sub_flat = [[0.0_f64; 3]; 3];
                for p in 0..3 {
                    for qq in 0..3 {
                        sub_flat[p][qq] = k_flat.get(3 * bi + p, 3 * bj + qq);
                    }
                }
                // expected = Q · sub_flat · Qᵀ
                let q_sub = mat3_mul(&q, &sub_flat);
                let expected_block = mat3_mul(&q_sub, &qt);

                // Compare entry-by-entry with relative tolerance 1e-9 · max(|expected|, 1.0)
                for p in 0..3 {
                    for qq in 0..3 {
                        let actual = k_tilted.get(3 * bi + p, 3 * bj + qq);
                        let expected = expected_block[p][qq];
                        let tol = 1e-9 * expected.abs().max(1.0);
                        assert!(
                            (actual - expected).abs() < tol,
                            "block ({bi},{bj}) entry ({p},{qq}): \
                             actual={actual:.6e}, expected={expected:.6e}, \
                             diff={:.6e}",
                            (actual - expected).abs(),
                        );
                    }
                }
            }
        }
    }

    // --- plane_stress_d auxetic ν-range tests ---

    #[test]
    fn plane_stress_d_accepts_auxetic_poisson_ratio() {
        // ν = -0.3 is inside the physical PD range (-1, 0.5).
        // D_pl = E/(1-ν²) · [[1, ν, 0], [ν, 1, 0], [0, 0, (1-ν)/2]]
        // All three diagonal entries are positive and finite for ν ∈ (-1, 0.5).
        let e = 1.0_f64;
        let nu = -0.3_f64;
        let mat = IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: nu,
        };

        let d = plane_stress_d(&mat);

        // Finite and symmetric.
        assert_symmetric_finite(&d);

        // Closed-form entries.
        let factor = e / (1.0 - nu * nu);
        let g = e / (2.0 * (1.0 + nu));
        let tol = 1e-9 * factor.abs().max(1.0);
        assert!((d[0][0] - factor).abs() < tol, "D[0][0] = {}", d[0][0]);
        assert!((d[1][1] - factor).abs() < tol, "D[1][1] = {}", d[1][1]);
        assert!((d[0][1] - nu * factor).abs() < tol, "D[0][1] = {}", d[0][1]);
        assert!((d[1][0] - nu * factor).abs() < tol, "D[1][0] = {}", d[1][0]);
        assert!((d[2][2] - g).abs() < tol, "D[2][2] = {} (expected G = {g})", d[2][2]);
        assert!(d[2][2] > 0.0, "shear term D[2][2] = {} should be positive", d[2][2]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn plane_stress_d_panics_at_incompressible_limit() {
        plane_stress_d(&IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.5,
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn plane_stress_d_panics_at_auxetic_limit() {
        plane_stress_d(&IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: -1.0,
        });
    }

    #[test]
    #[should_panic(expected = "degenerate shell element: p0 == p1")]
    fn build_shell_frame_panics_on_zero_edge_p0_eq_p1() {
        // p0 == p1 → len01 = 0 → first degenerate-frame assert fires.
        build_shell_frame(&[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }

    #[test]
    #[should_panic(expected = "degenerate shell element: collinear nodes")]
    fn build_shell_frame_panics_on_collinear_nodes() {
        // Three collinear points on the x-axis: len01=1 (first assert passes),
        // cross product = (1,0,0)×(2,0,0) = (0,0,0) → len_n = 0 → second assert fires.
        build_shell_frame(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    }

    // --- ShellFrame::local_to_global contract test (task 3159 step-1) ---

    /// `ShellFrame::local_to_global()` must return the transpose of `self.r`.
    ///
    /// `frame.r` rows = local basis vectors in global coordinates (global→local map).
    /// The local-to-global matrix is therefore `rᵀ`: `result[i][j] = r[j][i]`.
    ///
    /// Also verified: each column of `local_to_global()` (= each row of `r`) has unit
    /// norm within 1e-12, confirming the orthonormality contract is preserved after
    /// transposition.
    #[test]
    fn shell_frame_local_to_global_is_transpose_of_r() {
        let frame = build_shell_frame(&WIDE_TRI);
        let ltg = frame.local_to_global();

        // Transpose identity: ltg[i][j] == frame.r[j][i] for all i, j.
        for i in 0..3 {
            for j in 0..3 {
                let expected = frame.r[j][i];
                let got = ltg[i][j];
                assert!(
                    (got - expected).abs() < 1e-12,
                    "local_to_global()[{i}][{j}] = {got}, expected frame.r[{j}][{i}] = {expected}",
                );
            }
        }

        // Column orthonormality: each column of ltg (= each row of r) has unit norm.
        for col in 0..3 {
            let norm_sq = ltg[0][col] * ltg[0][col]
                + ltg[1][col] * ltg[1][col]
                + ltg[2][col] * ltg[2][col];
            assert!(
                (norm_sq - 1.0).abs() < 1e-12,
                "local_to_global() column {col} norm² = {norm_sq}, expected 1.0",
            );
        }
    }

    // ====================================================================
    // Degenerate (continuum-based) shell element (task 4068): per-node
    // directors + a varying Jacobian, carrying the MITC3+ assumed transverse
    // shear (task 3392). Tested on a CURVED patch (non-coplanar directors) so
    // the varying J is exercised — states the flat constant-J element cannot
    // reach. Sibling of `shell_element_stiffness_mitc3_plus` above.
    // ====================================================================

    /// Non-coplanar (radially tilted) per-node unit directors on the UNIT_TRI
    /// mid-surface: V_0 = +z, V_1 and V_2 tilted 30° outward. Non-parallel ⇒ the
    /// degenerate Jacobian varies in ζ and (ξ,η) — a curved patch whose curvature
    /// is carried by the directors (the mid-surface itself stays planar, which
    /// keeps the rigid-mode arithmetic clean while still exercising the varying J).
    fn curved_directors() -> [[f64; 3]; 3] {
        let c30 = 30.0_f64.to_radians().cos();
        let s30 = 30.0_f64.to_radians().sin();
        [[0.0, 0.0, 1.0], [s30, 0.0, c30], [0.0, s30, c30]]
    }

    #[test]
    fn degenerate_stiffness_is_18x18_symmetric_finite_with_rigid_body_null_space() {
        let mat = steel_like();
        let t = 0.05_f64;
        let directors = curved_directors();
        let thicknesses = [t; 3];
        let k = shell_element_stiffness_degenerate(&UNIT_TRI, &directors, &thicknesses, &mat);

        // (i) Shape: 18×18 after static condensation of the 2 bubble DOFs.
        assert_eq!(
            k.n_dofs,
            Mitc3Plus::N_DOFS,
            "degenerate element must condense to N_DOFS = 18"
        );
        assert_eq!(
            k.data.len(),
            Mitc3Plus::N_DOFS * Mitc3Plus::N_DOFS,
            "degenerate data length must be 18×18 = 324"
        );

        // (ii) Entrywise finite + symmetric to 1e-9 relative.
        for i in 0..Mitc3Plus::N_DOFS {
            for j in 0..Mitc3Plus::N_DOFS {
                let kij = k.get(i, j);
                let kji = k.get(j, i);
                assert!(kij.is_finite(), "degenerate K[{i}][{j}] = {kij} is not finite");
                let scale = kij.abs().max(kji.abs()).max(1.0);
                assert!(
                    (kij - kji).abs() < 1e-9 * scale,
                    "degenerate asymmetry at ({i},{j}): {kij} vs {kji}",
                );
            }
        }

        // (iii) Rigid-body null space on the CURVED patch. The degenerate
        // rigid-rotation mode u_i = ω×(x_i−c), θ_i = ω is strain-free for ANY
        // directors: u_h(ξ,η,ζ) ≡ ω×(X−c), so the velocity gradient is skew(ω)
        // (symmetric strain 0) and the geometry-free covariant shear coincides
        // with flat MITC3+'s (already proven to annihilate this mode).
        let k_max = k.data.iter().copied().fold(0.0_f64, |a, x| a.max(x.abs()));
        let tol = 1e-9 * k_max.max(1.0);

        // (a) Rigid translations along each global axis.
        for axis in 0..3 {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                u[Mitc3Plus::N_DOFS_PER_NODE * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < tol,
                "degenerate translation axis {axis}: linf(K·u) = {}, tol = {tol}",
                linf(&ku),
            );
        }

        // (b) Rigid rotations about the mid-surface centroid.
        let c = [1.0 / 3.0_f64, 1.0 / 3.0, 0.0_f64];
        let omega = [[1.0_f64, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for &w in &omega {
            let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
            for node in 0..Mitc3Plus::N_NODES {
                let dx = [
                    UNIT_TRI[node][0] - c[0],
                    UNIT_TRI[node][1] - c[1],
                    UNIT_TRI[node][2] - c[2],
                ];
                let ndp = Mitc3Plus::N_DOFS_PER_NODE;
                u[ndp * node + 0] = w[1] * dx[2] - w[2] * dx[1];
                u[ndp * node + 1] = w[2] * dx[0] - w[0] * dx[2];
                u[ndp * node + 2] = w[0] * dx[1] - w[1] * dx[0];
                u[ndp * node + 3] = w[0];
                u[ndp * node + 4] = w[1];
                u[ndp * node + 5] = w[2];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < tol,
                "degenerate rotation ω={w:?}: linf(K·u) = {}, tol = {tol}",
                linf(&ku),
            );
        }
    }

    // --- Degenerate patch tests (task 4068 step-13): the standard degenerated-
    // shell consistency acceptance tests, on a FLAT patch (directors ∥ facet
    // normal, uniform thickness) so the closed-form analytical energy is exact.
    // Mirror the MITC3+ membrane patch test and the bare-MITC3 bending patch
    // test — driving the degenerate element with constant in-plane strain and
    // with a constant-curvature (linear-θ) field, respectively.

    /// Flat-patch directors (∥ +z facet normal) + uniform thickness — the
    /// constant-Jacobian configuration in which the degenerate element's
    /// 2-point-Gauss-in-ζ through-thickness integral reproduces the closed-form
    /// `t` (membrane) and `t³/12` (bending) factors exactly.
    fn flat_patch_directors() -> [[f64; 3]; 3] {
        [[0.0, 0.0, 1.0]; 3]
    }

    #[test]
    fn degenerate_membrane_patch_test_matches_analytical_energy() {
        // Uniform in-plane strain u_x = a·x, u_y = b·y → ε = (a, b, 0). The
        // degenerate membrane B reproduces the constant strain and the in-plane
        // rule × 2-pt Gauss in ζ integrates the (constant) membrane integrand
        // exactly, so ½uᵀKu equals the analytical membrane energy.
        let mat = steel_like();
        let t = 0.05_f64;
        let a = 0.01_f64;
        let b = -0.005_f64;
        let directors = flat_patch_directors();
        let thicknesses = [t; 3];
        let k = shell_element_stiffness_degenerate(&UNIT_TRI, &directors, &thicknesses, &mat);

        const NDP: usize = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        u[NDP] = a; // node1 at x=1 → u_x = a
        u[NDP * 2 + 1] = b; // node2 at y=1 → u_y = b

        let ku = matvec(&k, &u);
        let u_k: f64 = 0.5 * ku.iter().zip(u.iter()).map(|(ki, ui)| ki * ui).sum::<f64>();

        let d = plane_stress_d(&mat);
        let eps = [a, b, 0.0_f64];
        let d_eps = [
            d[0][0] * eps[0] + d[0][1] * eps[1],
            d[1][0] * eps[0] + d[1][1] * eps[1],
            0.0,
        ];
        let area = 0.5_f64;
        let u_analytical = 0.5 * (eps[0] * d_eps[0] + eps[1] * d_eps[1]) * t * area;
        let scale = u_analytical.abs().max(1.0);
        assert!(
            (u_k - u_analytical).abs() < 1e-9 * scale,
            "degenerate membrane patch: U_K={u_k}, U_analytical={u_analytical}",
        );
    }

    #[test]
    fn degenerate_bending_patch_test_linear_theta_y_matches_analytical_total_energy() {
        // Constant-curvature field θ_y(node_i) = α·x_i (node0→0, node1→α,
        // node2→0): uniform curvature κ_xx = α plus the carried MITC3+
        // assumed-shear projection of the linear physical shear γ_xz = α·x to the
        // constant α/2 (verified: the MITC3+ interior-tying field, like bare
        // MITC3's edge field, averages a linear shear to its midpoint value). So
        //   U_total = ½·α²·D_pl[0][0]·(t³/12)·A  +  ½·(α/2)²·κ·G·t·A
        // and the degenerate element (which integrates bending via 2-pt Gauss in
        // ζ, exact for the ζ² integrand) reproduces it. On this flat patch the
        // element is numerically identical to flat MITC3+ for this mode.
        let mat = steel_like();
        let t = 0.05_f64;
        let alpha = 0.002_f64;
        let directors = flat_patch_directors();
        let thicknesses = [t; 3];
        let k = shell_element_stiffness_degenerate(&UNIT_TRI, &directors, &thicknesses, &mat);

        const NDP: usize = Mitc3Plus::N_DOFS_PER_NODE;
        let mut u = [0.0_f64; Mitc3Plus::N_DOFS];
        u[NDP + 4] = alpha; // θ_y = α at node1 (x=1) → θ_y = α·x

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
            "degenerate bending patch: U_K={u_k}, U_analytical={u_analytical} \
             (bending={u_bending}, shear={u_shear}), rel_err={}",
            (u_k - u_analytical).abs() / scale,
        );
    }
}
