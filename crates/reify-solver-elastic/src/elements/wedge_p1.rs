//! First-order wedge/prism (P1 / wedge6) reference element.
//!
//! Linear 6-node triangular-prism element defined on the **reference prism**
//! = unit triangle × `[-1, +1]`:
//!
//! ```text
//! { (ξ, η, ζ) : ξ ≥ 0, η ≥ 0, ξ + η ≤ 1, ζ ∈ [-1, +1] }
//! ```
//!
//! Shape functions are tensor products of barycentric (triangle base) × linear
//! (sweep direction):
//!
//! ```text
//! N_i(ξ, η, ζ) = L_{a_i}(ξ, η) · (1 + s_i · ζ) / 2
//! ```
//!
//! where `L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η` are the barycentric functions
//! and `(a_i, s_i)` is the (barycentric-index, ζ-sign) pair for node `i`.
//!
//! Canonical Gmsh PRI6 node ordering — bottom face `(ζ = −1)` first, then top
//! face `(ζ = +1)` in the same cyclic barycentric order:
//!
//! | node | bary index | ζ sign | ref coords        |
//! |------|-----------|--------|-------------------|
//! | 0    | 0 (L₀)    | −1     | `(0, 0, −1)`      |
//! | 1    | 1 (L₁=ξ)  | −1     | `(1, 0, −1)`      |
//! | 2    | 2 (L₂=η)  | −1     | `(0, 1, −1)`      |
//! | 3    | 0 (L₀)    | +1     | `(0, 0, +1)`      |
//! | 4    | 1 (L₁=ξ)  | +1     | `(1, 0, +1)`      |
//! | 5    | 2 (L₂=η)  | +1     | `(0, 1, +1)`      |
//!
//! Right-handed orientation — this ordering produces `det J > 0` for an
//! unsheared prism. Reference-prism volume `= (1/2) × 2 = 1`.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// First-order Lagrangian triangular prism (wedge6).
pub struct WedgeP1;

/// `(barycentric-coord-index, ζ-sign)` for each of the 6 nodes in Gmsh
/// PRI6 ordering.
///
/// `bary_idx` ∈ {0, 1, 2} selects which barycentric function
/// (`L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η`) governs the triangle face.
/// `zeta_sign` ∈ {−1.0, +1.0} selects the sweep layer.
///
/// Single-source: used by both `shape_at` and `shape_grad_at` to prevent
/// per-method ordering drift — mirrors the `hex_p1::VERTEX_SIGNS` pattern.
pub(crate) const VERTEX_BARY_ZETA: [(usize, f64); 6] = [
    (0, -1.0), // node 0: L₀, ζ = −1  → (0, 0, −1)
    (1, -1.0), // node 1: L₁, ζ = −1  → (1, 0, −1)
    (2, -1.0), // node 2: L₂, ζ = −1  → (0, 1, −1)
    (0, 1.0),  // node 3: L₀, ζ = +1  → (0, 0, +1)
    (1, 1.0),  // node 4: L₁, ζ = +1  → (1, 0, +1)
    (2, 1.0),  // node 5: L₂, ζ = +1  → (0, 1, +1)
];

impl ReferenceElement for WedgeP1 {
    const N_NODES: usize = 6;

    /// Shape functions at `coord`.
    ///
    /// Returns `[N_0, …, N_5]` where
    /// `N_i(ξ, η, ζ) = L_{a_i}(ξ, η) · (1 + s_i · ζ) / 2`
    /// and `L_0 = 1−ξ−η`, `L_1 = ξ`, `L_2 = η`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64> {
        let ReferenceCoord { xi, eta, zeta } = coord;
        let lambda = [1.0 - xi - eta, xi, eta];
        let mut n = Vec::with_capacity(6);
        for &(a, s) in &VERTEX_BARY_ZETA {
            n.push(lambda[a] * (1.0 + s * zeta) / 2.0);
        }
        n
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        vec![] // STUB — will be filled in step-4
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        &[] // STUB — will be filled in step-6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Canonical reference-prism vertices in Gmsh PRI6 ordering.
    const REF_VERTICES: [ReferenceCoord; 6] = [
        ReferenceCoord::new(0.0, 0.0, -1.0), // v_0: L₀, ζ = −1
        ReferenceCoord::new(1.0, 0.0, -1.0), // v_1: L₁, ζ = −1
        ReferenceCoord::new(0.0, 1.0, -1.0), // v_2: L₂, ζ = −1
        ReferenceCoord::new(0.0, 0.0, 1.0),  // v_3: L₀, ζ = +1
        ReferenceCoord::new(1.0, 0.0, 1.0),  // v_4: L₁, ζ = +1
        ReferenceCoord::new(0.0, 1.0, 1.0),  // v_5: L₂, ζ = +1
    ];

    #[test]
    fn n_nodes_const_is_six() {
        assert_eq!(<WedgeP1 as ReferenceElement>::N_NODES, 6);
    }

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_six_reference_prism_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = WedgeP1.shape_at(*v);
            assert_eq!(n.len(), 6, "shape_at must return N_NODES=6 entries");
            for (j, &n_j) in n.iter().enumerate() {
                let expected = if i == j { 1.0_f64 } else { 0.0_f64 };
                assert!(
                    (n_j - expected).abs() < TOL,
                    "N_{j}({:?}) = {n_j}, expected {expected}",
                    v,
                );
            }
        }
    }

    #[test]
    fn shape_at_partition_of_unity_at_centroid_and_interior() {
        // Centroid of unit triangle: (1/3, 1/3); mid sweep: ζ = 0.
        // Interior probes: all satisfy ξ > 0, η > 0, ξ+η < 1, |ζ| < 1.
        let probes = [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0), // centroid
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
        ];
        for p in &probes {
            let sum: f64 = WedgeP1.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }

    // ── shape_grad_at tests ──────────────────────────────────────────────────

    #[test]
    fn shape_grad_at_returns_six_rows_each_with_three_components() {
        let probe = ReferenceCoord::new(0.2, 0.3, 0.5);
        let g = WedgeP1.shape_grad_at(probe);
        assert_eq!(g.len(), 6, "shape_grad_at must return N_NODES=6 rows");
        for row in &g {
            assert_eq!(row.len(), 3, "each gradient row must have 3 components");
        }
    }

    #[test]
    fn shape_grad_at_partition_of_unity_consequence() {
        // Σ_i ∇N_i = (0, 0, 0) — consequence of Σ N_i ≡ 1.
        let probes = [
            ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0),
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
        ];
        for p in &probes {
            let g = WedgeP1.shape_grad_at(*p);
            let mut sum = [0.0_f64; 3];
            for row in &g {
                for k in 0..3 {
                    sum[k] += row[k];
                }
            }
            for k in 0..3 {
                assert!(
                    sum[k].abs() < TOL,
                    "Σ_i ∇N_i({:?})[{k}] = {}, expected 0",
                    p,
                    sum[k],
                );
            }
        }
    }

    #[test]
    fn shape_grad_at_matches_simple_analytic_form_at_centroid() {
        // At centroid (1/3, 1/3, 0):
        //   λ = [1/3, 1/3, 1/3], (1 + s*0)/2 = 1/2 for all nodes.
        //   ∇L₀ = (-1,-1, 0), ∇L₁ = (1,0,0), ∇L₂ = (0,1,0).
        //   ∂N_i/∂ξ = ∇L_{a_i}[0] * 1/2
        //   ∂N_i/∂η = ∇L_{a_i}[1] * 1/2
        //   ∂N_i/∂ζ = λ[a_i] * s_i / 2  = (1/3) * s_i / 2
        //
        // Node ordering: (a, s) = (0,-1),(1,-1),(2,-1),(0,+1),(1,+1),(2,+1)
        // Expected [∂N/∂ξ, ∂N/∂η, ∂N/∂ζ] for each node:
        #[rustfmt::skip]
        let expected: [[f64; 3]; 6] = [
            [-0.5, -0.5, -1.0 / 6.0], // node 0: a=0,s=-1
            [ 0.5,  0.0, -1.0 / 6.0], // node 1: a=1,s=-1
            [ 0.0,  0.5, -1.0 / 6.0], // node 2: a=2,s=-1
            [-0.5, -0.5,  1.0 / 6.0], // node 3: a=0,s=+1
            [ 0.5,  0.0,  1.0 / 6.0], // node 4: a=1,s=+1
            [ 0.0,  0.5,  1.0 / 6.0], // node 5: a=2,s=+1
        ];
        let centroid = ReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
        let g = WedgeP1.shape_grad_at(centroid);
        for (i, (grad, exp)) in g.iter().zip(expected.iter()).enumerate() {
            for k in 0..3 {
                assert!(
                    (grad[k] - exp[k]).abs() < TOL,
                    "∇N_{i}(centroid)[{k}] = {}, expected {}",
                    grad[k],
                    exp[k],
                );
            }
        }
    }

    #[test]
    fn shape_grad_at_matches_finite_difference_oracle_at_off_centroid_probes() {
        // FD oracle: central-difference truncation O(h²) ≈ 1e-12 + roundoff
        // O(ε·|f|/h) ≈ 1e-10; 1e-9 comfortably above.
        //
        // Probes stay strictly inside the reference prism:
        //   ξ,η ≥ 0.1, ξ+η ≤ 0.7, |ζ| ≤ 0.7 — so coord ± h·ê_k
        //   stays inside the domain for h = 1e-6.
        const FD_H: f64 = 1e-6;
        const FD_TOL: f64 = 1e-9;

        let probes = [
            ReferenceCoord::new(0.2, 0.3, 0.5),
            ReferenceCoord::new(0.4, 0.2, -0.6),
            ReferenceCoord::new(0.1, 0.1, 0.7),
        ];

        for probe in &probes {
            let grad = WedgeP1.shape_grad_at(*probe);
            for k in 0..3 {
                let coord_plus = match k {
                    0 => ReferenceCoord::new(probe.xi + FD_H, probe.eta, probe.zeta),
                    1 => ReferenceCoord::new(probe.xi, probe.eta + FD_H, probe.zeta),
                    2 => ReferenceCoord::new(probe.xi, probe.eta, probe.zeta + FD_H),
                    _ => unreachable!(),
                };
                let coord_minus = match k {
                    0 => ReferenceCoord::new(probe.xi - FD_H, probe.eta, probe.zeta),
                    1 => ReferenceCoord::new(probe.xi, probe.eta - FD_H, probe.zeta),
                    2 => ReferenceCoord::new(probe.xi, probe.eta, probe.zeta - FD_H),
                    _ => unreachable!(),
                };
                let n_plus = WedgeP1.shape_at(coord_plus);
                let n_minus = WedgeP1.shape_at(coord_minus);
                for i in 0..6 {
                    let fd_approx = (n_plus[i] - n_minus[i]) / (2.0 * FD_H);
                    assert!(
                        (fd_approx - grad[i][k]).abs() < FD_TOL,
                        "FD∇N_{i}({:?})[{k}]: fd={fd_approx}, analytic={}, diff={}",
                        probe,
                        grad[i][k],
                        (fd_approx - grad[i][k]).abs(),
                    );
                }
            }
        }
    }
}
