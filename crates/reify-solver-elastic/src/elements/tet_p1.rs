//! First-order tetrahedron (P1) reference element.
//!
//! Linear (constant-strain) tet with 4 nodes located at the reference
//! vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`. Shape functions are the
//! barycentric coordinates `[1-ξ-η-ζ, ξ, η, ζ]`; gradients are constant.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// First-order Lagrangian tetrahedron.
pub struct TetP1;

impl ReferenceElement for TetP1 {
    const N_NODES: usize = 4;

    fn shape_at(&self, _coord: ReferenceCoord) -> Vec<f64> {
        todo!("P1 shape functions — task 2914 step-4")
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        todo!("P1 shape-function gradients — task 2914 step-6")
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        todo!("P1 quadrature rule — task 2914 step-8")
    }
}
