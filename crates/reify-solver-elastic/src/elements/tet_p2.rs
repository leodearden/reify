//! Second-order tetrahedron (P2) reference element.
//!
//! Quadratic Lagrangian tet with 10 nodes: 4 at the reference vertices
//! `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` and 6 at the midpoints of the
//! edges in the canonical Hughes/Gmsh ordering
//! `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

/// Second-order Lagrangian tetrahedron.
pub struct TetP2;

impl ReferenceElement for TetP2 {
    const N_NODES: usize = 10;

    fn shape_at(&self, _coord: ReferenceCoord) -> Vec<f64> {
        todo!("P2 shape functions — task 2914 step-12")
    }

    fn shape_grad_at(&self, _coord: ReferenceCoord) -> Vec<[f64; 3]> {
        todo!("P2 shape-function gradients — task 2914 step-14")
    }

    fn quad_points(&self) -> &'static [QuadraturePoint] {
        todo!("P2 quadrature rule — task 2914 step-16")
    }
}
