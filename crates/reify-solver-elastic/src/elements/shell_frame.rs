//! Element-level shell primitives shared by every shell element, shell
//! assembly and shell stress recovery: the local mid-surface frame, the
//! plane-stress constitutive matrix, the CST membrane node-pair block and the
//! block-diagonal frame rotation.
//!
//! It imports nothing above `constitutive`, so the shell modules import
//! downward from here (PRD `docs/prds/v0_6/solver-crates-layering.md` D4).

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
    /// Matches the local-to-global convention documented on the `frame` field of
    /// `ElasticResult` in `crates/reify-compiler/stdlib/solver_elastic.ri`.  The future
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
/// Baked in as a crate-private constant — it is a property of the through-thickness
/// shape function, not of the material. See design decision in `plan.json`.
pub(crate) const KAPPA: f64 = 5.0 / 6.0;

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
pub(crate) fn mat3_mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
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

/// Constant-strain-triangle membrane strain-displacement node-pair block:
/// the 2×2 `Bₘᵢᵀ·(t·D_pl)·Bₘⱼ` (force/displacement, **before** the area
/// weight), indexed `[a][b]` for in-plane components `a, b ∈ {u_x, u_y}`.
///
/// With per-node constant shape gradient `dn = [∂N/∂x, ∂N/∂y]` the membrane
/// B-matrix column block is `Bₘ = [[dn_x, 0], [0, dn_y], [dn_y, dn_x]]` (rows =
/// Voigt strains `[ε_xx, ε_yy, γ_xy]`). This is the single strain-displacement
/// core shared — bit-identically — by the MITC3 / MITC3+ shell membrane block
/// (per-node DOF stride 6) and the dedicated 3-DOF/node CST membrane element
/// ([`crate::elements::membrane_cst::element_stiffness_membrane_cst`], stride 3).
/// Both callers multiply the returned block by the element area and scatter it
/// at their own per-node DOF stride, so the inner triple-product is identical
/// regardless of element kind.
#[inline]
#[allow(clippy::needless_range_loop)]
pub(crate) fn membrane_node_pair_block(
    dn_i: [f64; 2],
    dn_j: [f64; 2],
    t_dpl: &[[f64; 3]; 3],
) -> [[f64; 2]; 2] {
    let bmi = [[dn_i[0], 0.0], [0.0, dn_i[1]], [dn_i[1], dn_i[0]]];
    let bmj = [[dn_j[0], 0.0], [0.0, dn_j[1]], [dn_j[1], dn_j[0]]];
    let mut blk = [[0.0_f64; 2]; 2];
    for a in 0..2 {
        for b in 0..2 {
            let mut v = 0.0;
            for rr in 0..3 {
                for s in 0..3 {
                    v += bmi[rr][a] * t_dpl[rr][s] * bmj[s][b];
                }
            }
            blk[a][b] = v;
        }
    }
    blk
}

/// Rotate a local-frame element matrix into the global frame by a `blockdiag(R)`
/// congruence: `K_glob[3a..3a+3, 3b..3b+3] = Rᵀ · K_loc[3a.., 3b..] · R` over each
/// 3-DOF nodal block.
///
/// `N` is the matrix dimension and must be a multiple of 3; the block count is
/// `N / 3` and `T = blkdiag(R, …, R)` over those blocks. Shared by the MITC3
/// shell (`N = 18` ⇒ six displacement/rotation triples) and the CST membrane
/// (`N = 9` ⇒ three translation triples), so the block-rotation arithmetic lives
/// in exactly one place. Each per-block `Rᵀ·sub·R` uses the same `mat3_mul`
/// order as the original per-element code, so callers stay bit-identical.
#[inline]
#[allow(clippy::needless_range_loop)]
pub(crate) fn rotate_blockdiag<const N: usize>(
    k_loc: &[[f64; N]; N],
    r: &[[f64; 3]; 3],
) -> [[f64; N]; N] {
    let n_blocks = N / 3;
    // Rᵀ (local→global), built once and reused across every nodal block.
    let rt = [
        [r[0][0], r[1][0], r[2][0]],
        [r[0][1], r[1][1], r[2][1]],
        [r[0][2], r[1][2], r[2][2]],
    ];
    let mut k_glob = [[0.0_f64; N]; N];
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
            let rt_sub = mat3_mul(&rt, &sub);
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

#[cfg(test)]
pub(crate) mod test_support {
    use super::mat3_mul;

    /// Rotation Q = Ry(45°) · Rz(30°), the generic tilted frame shared by the
    /// shell unit-test modules.
    ///
    /// The orientation was chosen so that no entry of Q is zero: the drilling
    /// singularity mixes across all rotational DOFs, and every diagonal entry of
    /// the rotated K is nonzero — avoiding the axis-aligned singularity of an
    /// xy-plane `UNIT_TRI`.
    ///
    /// **Single source of truth.** Every shell test module that needs a tilted
    /// frame imports this function, so an edit to the rotation here reaches all
    /// of them.
    pub(crate) fn tilted_q_for_shell_tests() -> [[f64; 3]; 3] {
        let cos30 = (30.0_f64.to_radians()).cos();
        let sin30 = (30.0_f64.to_radians()).sin();
        let cos45 = (45.0_f64.to_radians()).cos();
        let sin45 = (45.0_f64.to_radians()).sin();
        let rz: [[f64; 3]; 3] = [[cos30, -sin30, 0.0], [sin30, cos30, 0.0], [0.0, 0.0, 1.0]];
        let ry: [[f64; 3]; 3] = [[cos45, 0.0, sin45], [0.0, 1.0, 0.0], [-sin45, 0.0, cos45]];
        mat3_mul(&ry, &rz)
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

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

    const WIDE_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];

    // --- Fixture invariants (step 1) ---

    /// Verify that `tilted_q_for_shell_tests()` returns a proper rotation matrix:
    /// each row is a unit vector, rows are mutually orthogonal, and det = +1.
    #[test]
    fn tilted_q_for_shell_tests_returns_orthonormal_rotation() {
        let q = test_support::tilted_q_for_shell_tests();

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

    // --- plane_stress_d test (step 5) ---

    #[test]
    fn plane_stress_d_matches_isotropic_formula_for_steel_like() {
        let mat = IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        };
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
}
