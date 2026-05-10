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

/// Stub: placeholder implementation — `N_NODES`, `shape_at`,
/// `shape_grad_at`, and `quad_points` will be filled in step-2/4/6.
impl ReferenceElement for WedgeP1 {
    const N_NODES: usize = 0; // STUB — will become 6 in step-2

    fn shape_at(&self, _coord: ReferenceCoord) -> Vec<f64> {
        vec![] // STUB
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        vec![] // STUB
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        &[] // STUB
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
}
