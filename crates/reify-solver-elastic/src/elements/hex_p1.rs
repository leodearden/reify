//! First-order hexahedron (P1 / hex8) reference element.
//!
//! Implementation pending — test scaffold only at this stage.

use crate::elements::{QuadraturePoint, ReferenceCoord, ReferenceElement};

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Reference cube vertices `v_0..v_7` in canonical Hughes/Gmsh ordering:
    /// bottom face (ζ = −1) counter-clockwise when viewed from +ζ, then
    /// top face in the same cyclic order.  Matches `VERTEX_SIGNS` in the
    /// outer module.
    const REF_VERTICES: [ReferenceCoord; 8] = [
        ReferenceCoord::new(-1.0, -1.0, -1.0), // v_0
        ReferenceCoord::new( 1.0, -1.0, -1.0), // v_1
        ReferenceCoord::new( 1.0,  1.0, -1.0), // v_2
        ReferenceCoord::new(-1.0,  1.0, -1.0), // v_3
        ReferenceCoord::new(-1.0, -1.0,  1.0), // v_4
        ReferenceCoord::new( 1.0, -1.0,  1.0), // v_5
        ReferenceCoord::new( 1.0,  1.0,  1.0), // v_6
        ReferenceCoord::new(-1.0,  1.0,  1.0), // v_7
    ];

    #[test]
    fn n_nodes_const_is_eight() {
        assert_eq!(<HexP1 as ReferenceElement>::N_NODES, 8);
    }

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_eight_reference_cube_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = HexP1.shape_at(*v);
            assert_eq!(n.len(), 8, "shape_at must return N_NODES=8 entries");
            for (j, &n_j) in n.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
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
        let probes = [
            ReferenceCoord::new(0.0, 0.0, 0.0),
            ReferenceCoord::new(0.3, -0.4, 0.2),
            ReferenceCoord::new(0.5, 0.5, 0.5),
        ];
        for p in &probes {
            let sum: f64 = HexP1.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }
}
