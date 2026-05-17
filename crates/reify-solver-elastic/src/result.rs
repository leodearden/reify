//! Per-element stress and nodal-stress gradient recovery for tetrahedral
//! P1 FEA.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #13.
//!
//! # Scope
//!
//! P1-only stress recovery for v0.3. The engine integration layer
//! (PRD §16) wraps the recovered nodal field as
//! `Field<Point3<Length>, Tensor<2,3,Pressure>>`; this crate ships the
//! Rust math primitives in plain `f64` types, mirroring the pattern in
//! `shell_result.rs` for shells.
//!
//! # Public surface
//!
//! - [`element_stress_p1`] — per-element constant Cauchy stress
//!   `σ_e = D · B · u_e` returned as a 3×3 symmetric tensor (Voigt is
//!   internal to the multiplication).
//! - [`tet_volume_p1`] — `|det J| / 6` from the affine map.
//! - [`recover_nodal_stress_p1`] + [`StressElement`] — volume-weighted
//!   averaging across incident elements, producing a continuous nodal
//!   stress field interpolatable via the same P1 shape functions.

use crate::constitutive::IsotropicElastic;
use crate::elements::{ReferenceCoord, ReferenceElement, tet_p1::TetP1};
use crate::math::{MIN_JACOBIAN_DET, inverse_transpose_3x3};

/// Compute the constant per-element Cauchy stress tensor for a P1
/// tetrahedron: `σ_e = D · B(p) · u_e`.
///
/// Returns a 3×3 symmetric tensor in the consumer-facing form
/// `[[σxx, σxy, σxz], [σxy, σyy, σyz], [σxz, σyz, σzz]]`. Voigt is
/// internal to the `D · B` multiplication; consumers
/// (`von_mises`, `principal_stresses`, …) want full tensor form.
///
/// # Algorithm
///
/// 1. Compute the forward Jacobian `J_ij = Σ_k phys_nodes[k][i] ·
///    grad_ref[k][j]` at the reference centroid via
///    [`TetP1::shape_grad_at`]. P1 gradients are constant per element
///    (any reference coord works).
/// 2. Compute `J⁻ᵀ` via [`crate::math::inverse_transpose_3x3`].
/// 3. Push reference gradients to physical: `∇x N_i = J⁻ᵀ · ∇ξ N_i`.
/// 4. Build the 6×12 strain-displacement matrix `B` with the same
///    engineering-shear Voigt convention as
///    `assembly/tet.rs:208-222`.
/// 5. Compute `ε_voigt = B · u_e` (engineering strain, length 6).
/// 6. Compute `σ_voigt = D · ε_voigt` via
///    [`IsotropicElastic::d_matrix`].
/// 7. Unpack to the symmetric 3×3 tensor.
///
/// # Voigt convention
///
/// Strain order: `[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]` with engineering
/// shear (`γ = 2ε`). Stress order: `[σ_xx, σ_yy, σ_zz, σ_xy, σ_yz,
/// σ_xz]`. Drift from this convention would break the patch test in
/// `step-11`; see `crate::constitutive::IsotropicElastic` and
/// `crate::assembly::tet` for the full convention rationale. The
/// uniaxial-strain patch test
/// (`element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal`)
/// pins the layout: a `u(x) = (a·x, 0, 0)` field round-trips to
/// `σ = diag((λ+2μ)·a, λ·a, λ·a)` exactly.
///
/// # Preconditions
///
/// The tet must be non-degenerate (`det J != 0`); see
/// [`crate::math::inverse_transpose_3x3`].
#[allow(clippy::needless_range_loop)]
pub fn element_stress_p1(
    phys_nodes: &[[f64; 3]; 4],
    material: &IsotropicElastic,
    u_e: &[f64; 12],
) -> [[f64; 3]; 3] {
    // Reference gradients (constant for P1 — any reference coord works).
    let grads_ref = TetP1.shape_grad_at(ReferenceCoord::new(0.25, 0.25, 0.25));

    // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..4 {
        for i in 0..3 {
            for j in 0..3 {
                j_mat[i][j] += phys_nodes[k][i] * grads_ref[k][j];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);
    // Degenerate-element guard (debug-only). Mirrors the convention used
    // by `assembly/tet.rs:182` for the same primitive: `det.is_normal()`
    // catches ±0, ±∞, NaN, and subnormals; the absolute-value floor
    // catches the merely-tiny case where dividing by `det` in
    // `inverse_transpose_3x3` would inflate FP error into `σ_e`.
    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate tet in element_stress_p1: |det J| = {} (must be > {} \
         and finite — see PRD task #21 for the future diagnostic path)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );
    let j_inv_t = inverse_transpose_3x3(&j_mat, det);

    // Push to physical gradients: ∇x N_i = J⁻ᵀ · ∇ξ N_i.
    let mut grads_phys = [[0.0_f64; 3]; 4];
    for i in 0..4 {
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..3 {
                s += j_inv_t[r][c] * grads_ref[i][c];
            }
            grads_phys[i][r] = s;
        }
    }

    // Build B and compute ε_voigt = B · u_e in one fused loop.
    // B is 6×12; row layout matches `assembly/tet.rs:208-222`:
    //   row 0 ε_xx ← ∂N/∂x for u_x
    //   row 1 ε_yy ← ∂N/∂y for u_y
    //   row 2 ε_zz ← ∂N/∂z for u_z
    //   row 3 γ_xy ← ∂N/∂y for u_x  +  ∂N/∂x for u_y
    //   row 4 γ_yz ← ∂N/∂z for u_y  +  ∂N/∂y for u_z
    //   row 5 γ_xz ← ∂N/∂z for u_x  +  ∂N/∂x for u_z
    let mut eps = [0.0_f64; 6];
    for i in 0..4 {
        let (gx, gy, gz) = (grads_phys[i][0], grads_phys[i][1], grads_phys[i][2]);
        let (ux, uy, uz) = (u_e[3 * i], u_e[3 * i + 1], u_e[3 * i + 2]);
        eps[0] += gx * ux;
        eps[1] += gy * uy;
        eps[2] += gz * uz;
        eps[3] += gy * ux + gx * uy;
        eps[4] += gz * uy + gy * uz;
        eps[5] += gz * ux + gx * uz;
    }

    // σ_voigt = D · ε_voigt.
    let d_mat = material.d_matrix();
    let mut sigma_voigt = [0.0_f64; 6];
    for i in 0..6 {
        let mut s = 0.0;
        for j in 0..6 {
            s += d_mat[i][j] * eps[j];
        }
        sigma_voigt[i] = s;
    }

    // Unpack to symmetric 3×3 tensor.
    [
        [sigma_voigt[0], sigma_voigt[3], sigma_voigt[5]],
        [sigma_voigt[3], sigma_voigt[1], sigma_voigt[4]],
        [sigma_voigt[5], sigma_voigt[4], sigma_voigt[2]],
    ]
}

/// Compute the volume of a P1 tetrahedron from its physical vertex
/// positions: `V = |det M| / 6`, where
/// `M = [v_1 − v_0 | v_2 − v_0 | v_3 − v_0]` is the 3×3 Jacobian of the
/// reference→physical affine map.
///
/// The `.abs()` choice mirrors `crate::assembly::tet`'s `det.abs()`
/// usage (see `assembly/tet.rs:244`): a left-handed (mirror-flipped)
/// node ordering yields `det J < 0` but the same physical volume, so
/// taking `|det J|` keeps `V > 0` for any valid (non-degenerate) tet.
///
/// # Preconditions
///
/// The tet must be non-degenerate. A degenerate (zero-volume) tet
/// returns exactly `0.0`; diagnosing that condition is PRD task #21's
/// job.
pub fn tet_volume_p1(phys_nodes: &[[f64; 3]; 4]) -> f64 {
    let v0 = phys_nodes[0];
    let mut m = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        m[i][0] = phys_nodes[1][i] - v0[i];
        m[i][1] = phys_nodes[2][i] - v0[i];
        m[i][2] = phys_nodes[3][i] - v0[i];
    }
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    det.abs() / 6.0
}

/// Per-element stress contribution for [`recover_nodal_stress_p1`].
///
/// Borrows the connectivity slice from the parent mesh; carries the
/// element's constant Cauchy stress and volume by value. Mirrors the
/// lifetime-borrowed-slice layout of
/// [`crate::assembly::AssemblyElement`] and
/// [`crate::interpolation::LocatableTet`].
#[derive(Debug, Clone, Copy)]
pub struct StressElement<'a> {
    /// Global node indices, in element-local order. For P1 tets, this
    /// has length 4 — but the recovery algorithm is connectivity-shape
    /// agnostic and accepts any length (e.g. 10 for P2 in a future
    /// extension).
    pub connectivity: &'a [usize],
    /// Constant per-element Cauchy stress tensor (from
    /// [`element_stress_p1`]).
    pub stress: [[f64; 3]; 3],
    /// Element volume (from [`tet_volume_p1`]).
    pub volume: f64,
}

/// Recover a continuous nodal stress field from per-element constant
/// stresses via volume-weighted simple averaging.
///
/// For each node `n`, the recovered stress is
///
/// ```text
/// σ_n = (Σ_{e incident to n} V_e · σ_e) / (Σ_{e incident to n} V_e)
/// ```
///
/// where the sum runs over every element whose connectivity includes
/// `n`. Nodes incident to no element yield the zero tensor (the only
/// reasonable default; flagging this as an error is PRD task #21's job).
///
/// # Algorithm choice
///
/// PRD §13 allows either Zienkiewicz–Zhu patch recovery or simple
/// averaging; volume-weighted simple averaging is bit-deterministic, has
/// no per-patch least-squares system, and is trivially parallelisable.
/// Z-Z is deferred to a v0.4+ task (design decision recorded in
/// `.task/plan.json`). The unequal-volume two-element test
/// (`recover_nodal_stress_volume_weighted_average_two_unequal_volume_elements`)
/// pins the weighting: σ_A=diag(100,0,0)·V=1 and σ_B=diag(0,200,0)·V=3
/// at a shared node round-trip to (1·σ_A + 3·σ_B)/4 = diag(25,150,0).
/// The canonical FE patch test
/// (`recover_nodal_stress_uniform_strain_patch_test_yields_constant_field_across_two_element_fan`)
/// verifies the full pipeline (`element_stress_p1` → recovery →
/// uniform σ at every node) preserves uniform strain across element
/// boundaries.
///
/// # Engine wrapping
///
/// The engine integration layer (PRD §16) wraps this output as a
/// `Field<Point3<Length>, Tensor<2,3,Pressure>>` by composing it with
/// [`crate::interpolation::interpolate_p1_at_point`] (component-wise on
/// the tensor): given a query point, locate the containing element via
/// [`crate::interpolation::locate_element_p1`], pull out the four
/// recovered nodal tensors, and linearly interpolate.
pub fn recover_nodal_stress_p1(
    n_nodes: usize,
    elements: &[StressElement<'_>],
) -> Vec<[[f64; 3]; 3]> {
    let mut accum = vec![[[0.0_f64; 3]; 3]; n_nodes];
    let mut weights = vec![0.0_f64; n_nodes];

    for el in elements {
        for &node in el.connectivity {
            // Bounds guard (debug-only): turn a generic Rust slice OOB
            // into a domain-specific panic message. Release builds rely
            // on the `accum[node]` indexing below to enforce the same
            // invariant; PRD task #21 (diagnostics) will own the
            // production-ready validation path.
            debug_assert!(
                node < n_nodes,
                "connectivity index {node} >= n_nodes {n_nodes} in recover_nodal_stress_p1",
            );
            for (acc_cell, &stress_cell) in accum[node]
                .iter_mut()
                .flatten()
                .zip(el.stress.iter().flatten())
            {
                *acc_cell += el.volume * stress_cell;
            }
            weights[node] += el.volume;
        }
    }

    for (node_accum, &weight) in accum.iter_mut().zip(weights.iter()) {
        if weight > 0.0 {
            for cell in node_accum.iter_mut().flatten() {
                *cell /= weight;
            }
        }
        // else: leave as zero (no incident elements).
    }

    accum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    /// Canonical unit reference tet: vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference-tet volume `1/6`.
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    #[test]
    fn recover_nodal_stress_uniform_strain_patch_test_yields_constant_field_across_two_element_fan()
    {
        // The canonical FE patch test:
        //   Two unit P1 tets sharing a face, apply a uniform linear
        //   displacement field u(x) = a · x ê_x ⇒ ε_xx = a everywhere,
        //   uniform stress. Recovery must preserve uniformity (recovered
        //   nodal field is constant across all nodes) and equal the
        //   analytical D · [a, 0, 0, 0, 0, 0]ᵀ.
        let a = 0.01_f64;
        let mat = dimensionless_steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let lambda_plus_two_mu = lambda + two_mu;

        // Two-element fan sharing the face (x ∈ [0,1], y + z ≤ 1 on
        // x = 1)... simpler: split the unit cube along its diagonal into
        // two tets sharing a face. Use the canonical unit tet for tet0,
        // and a mirrored tet for tet1 that shares face {1,2,3}.
        //
        // tet0: [v0, v1, v2, v3] = [(0,0,0), (1,0,0), (0,1,0), (0,0,1)]
        //   ⇒ shares face on the plane x + y + z = 1, namely the
        //   triangle (1,0,0)–(0,1,0)–(0,0,1).
        // tet1: shares that face; the fourth vertex sits on the other
        //   side of that plane, e.g. (1,1,1).
        let nodes = [
            [0.0_f64, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0],     // 1
            [0.0, 1.0, 0.0],     // 2
            [0.0, 0.0, 1.0],     // 3
            [1.0, 1.0, 1.0],     // 4
        ];
        let tet0_nodes = [nodes[0], nodes[1], nodes[2], nodes[3]];
        let tet1_nodes = [nodes[1], nodes[2], nodes[3], nodes[4]];
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];

        // Uniform displacement u(x) = (a·x, 0, 0).
        let mut u0 = [0.0_f64; 12];
        for (i, x) in tet0_nodes.iter().enumerate() {
            u0[3 * i] = a * x[0];
        }
        let mut u1 = [0.0_f64; 12];
        for (i, x) in tet1_nodes.iter().enumerate() {
            u1[3 * i] = a * x[0];
        }

        let stress0 = element_stress_p1(&tet0_nodes, &mat, &u0);
        let stress1 = element_stress_p1(&tet1_nodes, &mat, &u1);

        let element0 = StressElement {
            connectivity: &conn0,
            stress: stress0,
            volume: tet_volume_p1(&tet0_nodes),
        };
        let element1 = StressElement {
            connectivity: &conn1,
            stress: stress1,
            volume: tet_volume_p1(&tet1_nodes),
        };

        let nodal = recover_nodal_stress_p1(5, &[element0, element1]);

        // Expected analytical stress: σ_xx = (λ+2μ)·a, σ_yy = σ_zz = λ·a.
        let expected = [
            [lambda_plus_two_mu * a, 0.0, 0.0],
            [0.0, lambda * a, 0.0],
            [0.0, 0.0, lambda * a],
        ];

        // (a) Uniform across all 5 nodes (within 1e-9 relative tol on the
        //     largest expected entry). (b) Equal to the analytical value.
        let scale = expected[0][0].abs().max(1.0);
        for (n, t) in nodal.iter().enumerate() {
            for i in 0..3 {
                for j in 0..3 {
                    assert!(
                        (t[i][j] - expected[i][j]).abs() < 1e-9 * scale,
                        "node {n} σ[{i}][{j}] = {} expected {} (uniform-strain patch test)",
                        t[i][j],
                        expected[i][j],
                    );
                }
            }
        }
    }

    #[test]
    fn recover_nodal_stress_volume_weighted_average_two_unequal_volume_elements() {
        // Two elements share node 0; element A also touches [1,2,3],
        // element B also touches [4,5,6]. Pin the volume-weighted-average
        // behaviour against unequal volumes:
        //   σ_A = diag(100, 0, 0), V_A = 1.0
        //   σ_B = diag(0, 200, 0), V_B = 3.0
        // ⇒ recovered σ_0 = (1·σ_A + 3·σ_B) / 4 = diag(25, 150, 0).
        let stress_a = [[100.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let stress_b = [[0.0, 0.0, 0.0], [0.0, 200.0, 0.0], [0.0, 0.0, 0.0]];
        let conn_a = [0_usize, 1, 2, 3];
        let conn_b = [0_usize, 4, 5, 6];
        let element_a = StressElement {
            connectivity: &conn_a,
            stress: stress_a,
            volume: 1.0,
        };
        let element_b = StressElement {
            connectivity: &conn_b,
            stress: stress_b,
            volume: 3.0,
        };

        let nodal = recover_nodal_stress_p1(7, &[element_a, element_b]);

        // Shared node 0: weighted average.
        let expected_0 = [[25.0, 0.0, 0.0], [0.0, 150.0, 0.0], [0.0, 0.0, 0.0]];
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (nodal[0][i][j] - expected_0[i][j]).abs() < 1e-12,
                    "node 0 σ[{i}][{j}] = {} expected {}",
                    nodal[0][i][j],
                    expected_0[i][j],
                );
            }
        }
        // Node 1 is only in A → recovers σ_A.
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (nodal[1][i][j] - stress_a[i][j]).abs() < 1e-12,
                    "node 1 (only in A) σ[{i}][{j}] = {} expected σ_A = {}",
                    nodal[1][i][j],
                    stress_a[i][j],
                );
            }
        }
        // Node 4 is only in B → recovers σ_B.
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (nodal[4][i][j] - stress_b[i][j]).abs() < 1e-12,
                    "node 4 (only in B) σ[{i}][{j}] = {} expected σ_B = {}",
                    nodal[4][i][j],
                    stress_b[i][j],
                );
            }
        }
    }

    #[test]
    fn recover_nodal_stress_single_element_returns_element_stress_at_each_node() {
        // One element with a non-trivial diagonal stress and unit volume.
        // The volume-weighted average across one incident element is
        // just that element's own stress at every node it touches.
        let stress = [[100.0, 0.0, 0.0], [0.0, 50.0, 0.0], [0.0, 0.0, 25.0]];
        let connectivity = [0_usize, 1, 2, 3];
        let element = StressElement {
            connectivity: &connectivity,
            stress,
            volume: 1.0 / 6.0,
        };

        let nodal = recover_nodal_stress_p1(4, &[element]);

        assert_eq!(nodal.len(), 4, "n_nodes=4 ⇒ output length 4");
        for (n, t) in nodal.iter().enumerate() {
            for i in 0..3 {
                for j in 0..3 {
                    assert!(
                        (t[i][j] - stress[i][j]).abs() < 1e-12,
                        "node {n} σ[{i}][{j}] = {} expected {}",
                        t[i][j],
                        stress[i][j],
                    );
                }
            }
        }
    }

    #[test]
    fn tet_volume_p1_unit_tet_returns_one_sixth_and_scales_cubically_under_uniform_doubling() {
        // Unit tet: V = 1/6.
        let v_unit = tet_volume_p1(&UNIT_TET_P1);
        assert!(
            (v_unit - 1.0 / 6.0).abs() < 1e-12,
            "V(unit_tet) = {v_unit} expected 1/6",
        );

        // Edge-doubled tet: V = 8/6 (V scales as L³).
        let scaled: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let v_scaled = tet_volume_p1(&scaled);
        assert!(
            (v_scaled - 8.0 / 6.0).abs() < 1e-12,
            "V(scaled_tet) = {v_scaled} expected 8/6",
        );

        // Left-handed ordering (swap nodes 1 and 2): det J flips sign,
        // but |det J|/6 is unchanged. Pin that .abs() is taken.
        let flipped: [[f64; 3]; 4] = [
            UNIT_TET_P1[0],
            UNIT_TET_P1[2],
            UNIT_TET_P1[1],
            UNIT_TET_P1[3],
        ];
        let v_flipped = tet_volume_p1(&flipped);
        assert!(
            (v_flipped - 1.0 / 6.0).abs() < 1e-12,
            "V(flipped_tet) = {v_flipped} expected 1/6 (|.| takes care of left-handed)",
        );
    }

    #[test]
    fn element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal() {
        // Linear displacement u(x) = (a·x, 0, 0) ⇒ ε_xx = a, all other
        // strain components 0. Expect σ_xx = (λ+2μ)·a, σ_yy = σ_zz = λ·a,
        // off-diagonals 0. Pins the B-matrix orientation against
        // assembly/tet.rs's convention.
        let a = 0.01_f64;
        let mat = dimensionless_steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let lambda_plus_two_mu = lambda + two_mu;

        let mut u_e = [0.0_f64; 12];
        for (i, x) in UNIT_TET_P1.iter().enumerate() {
            u_e[3 * i] = a * x[0];
            // u_y, u_z stay 0
        }

        let stress = element_stress_p1(&UNIT_TET_P1, &mat, &u_e);

        let exp_xx = lambda_plus_two_mu * a;
        let exp_yy = lambda * a;
        let exp_zz = lambda * a;

        let scale_xx = exp_xx.abs().max(1.0);
        let scale_yy = exp_yy.abs().max(1.0);
        assert!(
            (stress[0][0] - exp_xx).abs() < 1e-9 * scale_xx,
            "σ_xx = {} expected (λ+2μ)·a = {exp_xx}",
            stress[0][0],
        );
        assert!(
            (stress[1][1] - exp_yy).abs() < 1e-9 * scale_yy,
            "σ_yy = {} expected λ·a = {exp_yy}",
            stress[1][1],
        );
        assert!(
            (stress[2][2] - exp_zz).abs() < 1e-9 * scale_yy,
            "σ_zz = {} expected λ·a = {exp_zz}",
            stress[2][2],
        );
        // Off-diagonals must vanish (within 1e-9 of the largest σ entry).
        let scale_off = exp_xx.abs().max(1.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert!(
                stress[i][j].abs() < 1e-9 * scale_off,
                "σ[{i}][{j}] = {} expected 0",
                stress[i][j],
            );
        }
    }

    #[test]
    fn element_stress_p1_zero_displacement_yields_zero_stress() {
        // Regression guard: an off-by-one that leaks the D-matrix
        // diagonal into the result for ε = 0 would surface here.
        let mat = dimensionless_steel_like();
        let stress = element_stress_p1(&UNIT_TET_P1, &mat, &[0.0_f64; 12]);
        for (i, row) in stress.iter().enumerate() {
            for (j, &sij) in row.iter().enumerate() {
                assert_eq!(
                    sij, 0.0,
                    "zero-displacement σ[{i}][{j}] = {sij} expected 0.0",
                );
            }
        }
    }
}
