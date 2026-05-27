//! Element-stiffness assembly for the P1 (trilinear) hexahedron.

use super::ElementStiffness;
use crate::constitutive::IsotropicElastic;
use crate::material_field::MaterialField;

/// Compute the 24×24 element stiffness for a P1 (trilinear) hexahedron.
///
/// `phys_nodes` are the 8 vertex positions in canonical Hughes/Gmsh hex8 order
/// (same sign-pattern ordering as `crate::elements::hex_p1::VERTEX_SIGNS`):
///
/// | node | ξ  | η  | ζ  |
/// |------|----|----|----|
/// | 0    | −1 | −1 | −1 |
/// | 1    | +1 | −1 | −1 |
/// | 2    | +1 | +1 | −1 |
/// | 3    | −1 | +1 | −1 |
/// | 4    | −1 | −1 | +1 |
/// | 5    | +1 | −1 | +1 |
/// | 6    | +1 | +1 | +1 |
/// | 7    | −1 | +1 | +1 |
///
/// **Quadrature**: 2×2×2 Gauss-Legendre rule (8 points), degree-3-per-axis exact
/// — sufficient for constant-strain modes on a constant-Jacobian hex.
///
/// See [`crate::assembly::tet::element_stiffness_generic`] for the BᵀDB integrand
/// and [`IsotropicElastic::d_matrix`] for the engineering-strain Voigt convention
/// (shear-block diagonal = μ, not 2μ).
pub fn element_stiffness_hex_p1(
    phys_nodes: &[[f64; 3]; 8],
    material: &IsotropicElastic,
) -> ElementStiffness {
    crate::assembly::tet::element_stiffness_generic(
        &crate::elements::hex_p1::HexP1,
        &phys_nodes[..],
        material,
    )
}

/// Compute the 24×24 element stiffness for a P1 hex with per-element
/// material sampled from a [`MaterialField`].
///
/// Centroid is the mean of all 8 corner phys-nodes (the canonical hex
/// centroid in reference coords `(0,0,0)` for the `[−1,1]³` cube).
/// Samples `field.material_at(centroid)` once, computes `d_global`,
/// and dispatches to `element_stiffness_generic_with_d_global`.
///
/// # Foundation β contract (PRD §C4)
///
/// When `field` is a constant lift of an identity-frame isotropic
/// material, the returned `K_e` is **bitwise** equal to
/// `element_stiffness_hex_p1(phys_nodes, &iso)`.
pub fn element_stiffness_hex_p1_with_field<F: MaterialField>(
    phys_nodes: &[[f64; 3]; 8],
    field: &F,
) -> ElementStiffness {
    let mut c = [0.0_f64; 3];
    for n in phys_nodes {
        c[0] += n[0];
        c[1] += n[1];
        c[2] += n[2];
    }
    let centroid = [0.125 * c[0], 0.125 * c[1], 0.125 * c[2]];
    let mat = field.material_at(centroid);
    let d_global = mat.d_matrix_global();
    crate::assembly::tet::element_stiffness_generic_with_d_global(
        &crate::elements::hex_p1::HexP1,
        &phys_nodes[..],
        &d_global,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::test_support::{
        self, ElementStiffnessTestSpec, dimensionless_steel_like, scaled_unit_hex_phys_nodes,
    };

    #[test]
    fn hex_p1_returns_24_by_24_stiffness() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k = element_stiffness_hex_p1(&phys, &mat);
        assert_eq!(k.n_dofs, 24, "hex K_e must be 24×24 (8 nodes × 3 axes)");
        assert_eq!(k.data.len(), 576, "hex K_e data must have 576 entries");
    }

    #[test]
    fn hex_p1_behavioral_pins() {
        // Tests (b)–(h): symmetry, rigid-body null spaces, patch tests (normal
        // strain + full 6-component), volume scaling, and left-handed orientation.
        test_support::run_element_stiffness_tests(
            &|nodes, mat| {
                let arr: &[[f64; 3]; 8] = nodes.try_into().unwrap();
                element_stiffness_hex_p1(arr, mat)
            },
            &|s| scaled_unit_hex_phys_nodes(s).to_vec(),
            ElementStiffnessTestSpec {
                n_nodes: 8,
                vol_ref: 8.0,
                // Centroid of [−1,1]³ is the origin.
                centroid: [0.0, 0.0, 0.0],
                // Swap opposite corners (0↔6) to flip orientation.
                swap_pair: (0, 6),
                vol_swapped: 4.0,
            },
        );
    }
}
