//! Reference-element primitives for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #7.
//!
//! # Canonical reference element
//!
//! All elements in this module are defined on the **unit reference
//! tetrahedron** with vertices at `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` in
//! `(ξ, η, ζ)` coordinates. The reference-tet volume is `1/6`.

pub mod tet_p1;
pub mod tet_p2;

/// A point in the reference-tetrahedron's `(ξ, η, ζ)` coordinate space.
///
/// The unit reference tet has vertices at `(0,0,0), (1,0,0), (0,1,0),
/// (0,0,1)`; barycentric coordinates are `(1-ξ-η-ζ, ξ, η, ζ)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceCoord {
    pub xi: f64,
    pub eta: f64,
    pub zeta: f64,
}

impl ReferenceCoord {
    /// Construct a reference-coordinate triple.
    pub const fn new(xi: f64, eta: f64, zeta: f64) -> Self {
        Self { xi, eta, zeta }
    }
}

/// Reference→physical Jacobian of an element at a single reference
/// coordinate.
///
/// `matrix[i][j] = ∂x_i / ∂ξ_j` where `x` is the physical coordinate and
/// `ξ` the reference coordinate. `det` is the determinant of `matrix`.
///
/// This is the **forward** map only. The inverse / transpose-inverse map
/// (`Jᵀ⁻¹`) needed to push reference gradients into physical gradients
/// for stiffness assembly is the consumer's responsibility (PRD task #8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jacobian {
    pub matrix: [[f64; 3]; 3],
    pub det: f64,
}

impl Jacobian {
    /// Construct from a 3×3 matrix; computes the determinant via cofactor
    /// expansion.
    pub fn from_matrix(matrix: [[f64; 3]; 3]) -> Self {
        let det = matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
            - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
            + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
        Self { matrix, det }
    }
}

/// A quadrature point: a reference-coordinate location and its weight.
///
/// Weights sum to the reference-tet volume `1/6` for every rule defined
/// in this crate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadraturePoint {
    pub coord: ReferenceCoord,
    pub weight: f64,
}

/// Reference-tetrahedron Lagrangian element trait.
///
/// Implementors expose:
/// - the number of nodes (`N_NODES`),
/// - the shape functions evaluated at a reference coordinate
///   (`shape_at`), returning a `Vec<f64>` of length `N_NODES`,
/// - the shape-function gradients in reference coordinates
///   (`shape_grad_at`), returning a `Vec<[f64; 3]>` of length `N_NODES`,
/// - a Gauss quadrature rule (`quad_points`) covering the reference tet.
///
/// The default `jacobian` method composes `shape_grad_at` with
/// caller-supplied physical-node coordinates to produce the
/// reference→physical Jacobian (forward map only; inverse / Jᵀ⁻¹
/// mapping for physical-gradient assembly is the consumer's
/// responsibility — see PRD task #8).
pub trait ReferenceElement {
    /// Number of Lagrangian nodes per element (e.g., 4 for P1, 10 for P2).
    const N_NODES: usize;

    /// Shape-function values `[N_0, …, N_{N-1}]` at the given reference
    /// coordinate. The returned `Vec` has length `N_NODES`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64>;

    /// Shape-function gradients in reference coordinates,
    /// `[∇N_0, …, ∇N_{N-1}]`, where each gradient is `[∂N/∂ξ, ∂N/∂η,
    /// ∂N/∂ζ]`. The returned `Vec` has length `N_NODES`.
    fn shape_grad_at(&self, coord: ReferenceCoord) -> Vec<[f64; 3]>;

    /// Gauss quadrature rule for integration over the reference tet.
    ///
    /// Weights sum to the reference-tet volume `1/6`.
    fn quad_points(&self) -> &'static [QuadraturePoint];

    /// Reference→physical Jacobian at `ref_coord`.
    ///
    /// Computes `J_ij = Σ_k phys_nodes[k][i] · shape_grad_at(ref_coord)[k][j]`.
    ///
    /// `phys_nodes.len()` must equal `Self::N_NODES` and the entries must
    /// be ordered to match the canonical reference-vertex ordering pinned
    /// in the implementing element module:
    ///
    /// - **`TetP1`** — vertices in `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`
    ///   order.
    /// - **`TetP2`** — same vertex order followed by the 6 edge midpoints
    ///   in canonical Hughes/Gmsh order `(0,1), (1,2), (2,0), (0,3),
    ///   (1,3), (2,3)`.
    fn jacobian(&self, phys_nodes: &[[f64; 3]], ref_coord: ReferenceCoord) -> Jacobian {
        debug_assert_eq!(
            phys_nodes.len(),
            Self::N_NODES,
            "phys_nodes.len() must equal Self::N_NODES",
        );
        let grads = self.shape_grad_at(ref_coord);
        debug_assert_eq!(
            grads.len(),
            Self::N_NODES,
            "shape_grad_at must return N_NODES rows",
        );
        let mut m = [[0.0_f64; 3]; 3];
        for k in 0..Self::N_NODES {
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += phys_nodes[k][i] * grads[k][j];
                }
            }
        }
        Jacobian::from_matrix(m)
    }
}

#[cfg(test)]
mod tests {
    use crate::{QuadraturePoint, ReferenceCoord, ReferenceElement, TetP1, TetP2};

    #[test]
    fn reference_coord_constructor_exposes_components() {
        let c = ReferenceCoord::new(0.1, 0.2, 0.3);
        assert_eq!(c.xi, 0.1);
        assert_eq!(c.eta, 0.2);
        assert_eq!(c.zeta, 0.3);
    }

    #[test]
    fn quadrature_point_carries_coord_and_weight() {
        let q = QuadraturePoint {
            coord: ReferenceCoord::new(0.25, 0.25, 0.25),
            weight: 1.0 / 6.0,
        };
        assert_eq!(q.coord.xi, 0.25);
        assert_eq!(q.coord.eta, 0.25);
        assert_eq!(q.coord.zeta, 0.25);
        assert_eq!(q.weight, 1.0 / 6.0);
    }

    #[test]
    fn tet_p1_implements_reference_element_with_four_nodes() {
        assert_eq!(<TetP1 as ReferenceElement>::N_NODES, 4);
    }

    #[test]
    fn tet_p2_implements_reference_element_with_ten_nodes() {
        assert_eq!(<TetP2 as ReferenceElement>::N_NODES, 10);
    }
}
