//! Element-stiffness assembly for the P1 (linear) wedge (triangular prism).

use super::ElementStiffness;
use crate::constitutive::IsotropicElastic;
use crate::material_field::MaterialField;

/// Compute the 18×18 element stiffness for a P1 (linear) wedge (prism).
///
/// `phys_nodes` are the 6 vertex positions in canonical Gmsh PRI6 order:
/// bottom face (ζ = −1) first in barycentric order (L₀, L₁, L₂), then top
/// face (ζ = +1) in the same cyclic order.
///
/// | node | bary | ζ  | ref coords    |
/// |------|------|----|---------------|
/// | 0    | L₀   | −1 | `(0, 0, −1)`  |
/// | 1    | L₁   | −1 | `(1, 0, −1)`  |
/// | 2    | L₂   | −1 | `(0, 1, −1)`  |
/// | 3    | L₀   | +1 | `(0, 0, +1)`  |
/// | 4    | L₁   | +1 | `(1, 0, +1)`  |
/// | 5    | L₂   | +1 | `(0, 1, +1)`  |
///
/// **Quadrature**: 3×2 tensor-product rule (6 points) — 3-point triangle ×
/// 2-point Gauss-Legendre on `[-1, +1]` — exact for degree-2-in-triangle ×
/// degree-3-in-line integrands, sufficient for constant-strain modes on a
/// constant-Jacobian wedge.
///
/// See [`crate::assembly::tet::element_stiffness_generic`] for the BᵀDB
/// integrand and [`IsotropicElastic::d_matrix`] for the engineering-strain
/// Voigt convention (shear-block diagonal = μ, not 2μ).
pub fn element_stiffness_wedge_p1(
    phys_nodes: &[[f64; 3]; 6],
    material: &IsotropicElastic,
) -> ElementStiffness {
    crate::assembly::tet::element_stiffness_generic(
        &crate::elements::wedge_p1::WedgeP1,
        &phys_nodes[..],
        material,
    )
}

/// Compute the 18×18 element stiffness for a P1 wedge with per-element
/// material sampled from a [`MaterialField`].
///
/// Centroid is the mean of all 6 corner phys-nodes (top + bottom
/// triangular faces). Samples `field.material_at(centroid)` once,
/// computes `d_global`, and dispatches to
/// `element_stiffness_generic_with_d_global`.
///
/// # Foundation β contract (PRD §C4)
///
/// When `field` is a constant lift of an identity-frame isotropic
/// material, the returned `K_e` is **bitwise** equal to
/// `element_stiffness_wedge_p1(phys_nodes, &iso)`.
pub fn element_stiffness_wedge_p1_with_field<F: MaterialField>(
    phys_nodes: &[[f64; 3]; 6],
    field: &F,
) -> ElementStiffness {
    let mut c = [0.0_f64; 3];
    for n in phys_nodes {
        c[0] += n[0];
        c[1] += n[1];
        c[2] += n[2];
    }
    let inv6 = 1.0 / 6.0;
    let centroid = [inv6 * c[0], inv6 * c[1], inv6 * c[2]];
    let mat = field.material_at(centroid);
    let d_global = mat.d_matrix_global();
    crate::assembly::tet::element_stiffness_generic_with_d_global(
        &crate::elements::wedge_p1::WedgeP1,
        &phys_nodes[..],
        &d_global,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::test_support::{
        self, ElementStiffnessTestSpec, dimensionless_steel_like, scaled_unit_wedge_phys_nodes,
    };

    #[test]
    fn wedge_p1_returns_18_by_18_stiffness() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_wedge_phys_nodes(1.0);
        let k = element_stiffness_wedge_p1(&phys, &mat);
        assert_eq!(k.n_dofs, 18, "wedge K_e must be 18×18 (6 nodes × 3 axes)");
        assert_eq!(k.data.len(), 324, "wedge K_e data must have 324 entries");
    }

    #[test]
    fn wedge_p1_behavioral_pins() {
        // Tests (b)–(h): symmetry, rigid-body null spaces, patch tests (normal
        // strain + full 6-component), volume scaling, and left-handed orientation.
        test_support::run_element_stiffness_tests(
            &|nodes, mat| {
                let arr: &[[f64; 3]; 6] = nodes.try_into().unwrap();
                element_stiffness_wedge_p1(arr, mat)
            },
            &|s| scaled_unit_wedge_phys_nodes(s).to_vec(),
            ElementStiffnessTestSpec {
                n_nodes: 6,
                vol_ref: 1.0,
                // Centroid of the unit reference prism is (1/3, 1/3, 0).
                centroid: [1.0 / 3.0, 1.0 / 3.0, 0.0],
                // Swap bottom-face nodes 1↔2 to flip orientation. For the
                // bottom-face swap of an identity-mapped reference prism,
                // det J(ζ) = ζ (see the production test
                // `wedge_p1::tests::jacobian_negative_det_for_swapped_bottom_face_nodes`
                // for the J derivation), so the 2-point Gauss-Legendre line
                // rule integrates |ζ| to 2·(1/√3), giving an effective
                // quadrature volume of (1/2)·(2/√3) = 1/√3.
                swap_pair: (1, 2),
                vol_swapped: 1.0_f64 / 3.0_f64.sqrt(),
            },
        );
    }
}
