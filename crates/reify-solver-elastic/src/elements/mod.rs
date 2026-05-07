//! Reference-element primitives for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #7.

pub mod tet_p1;
pub mod tet_p2;

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
