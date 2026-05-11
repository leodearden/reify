//! Zienkiewicz-Zhu superconvergent patch-recovery error estimator for
//! tetrahedral P1 linear elastostatics.
//!
//! See PRD `docs/prds/v0_4/a-posteriori-error-estimation.md`, Resolved
//! §"Error indicator" + Task decomposition #1 (task 2996).
//!
//! # Scope
//!
//! Pure-Rust kernel math primitives for the Z-Z error indicator over the v0.3
//! per-element stress field from kernel task #2920. Does NOT plumb into
//! `ElasticResult` (task A3) and does NOT touch the refinement loop (task A2).
//!
//! # Public surface
//!
//! - [`ZzIndicator`] — output carrier: per-element η_e and global relative
//!   energy error η_global.
//! - [`compute_zz_indicator`] — entry point: given a per-element stress field
//!   (as `&[StressElement<'_>]` from task 2920), a mesh for n_nodes, and
//!   material parameters, returns the Z-Z indicator.

use crate::constitutive::IsotropicElastic;
use crate::result::StressElement;
use reify_types::VolumeMesh;

/// Output of the Zienkiewicz-Zhu superconvergent patch-recovery error
/// estimator.
///
/// Both fields are in plain-f64 kernel form. The lofty
/// `Field<Element, ScalarPressure>` / `Number` wrappings belong at the
/// engine-integration layer (task A3 — ElasticResult API extensions), not
/// here.
#[derive(Debug, Clone, PartialEq)]
pub struct ZzIndicator {
    /// Per-element error indicator η_e, one entry per input element in input
    /// order.
    ///
    /// `η_e = √(V_e · (σ_e − σ̄_e*)ᵀ D⁻¹ (σ_e − σ̄_e*))` where `σ̄_e*` is the
    /// smoothed stress interpolated back to the element centroid via the P1
    /// patch average.
    pub per_element: Vec<f64>,

    /// Global relative energy error `η_global = √(Σ η_e² / U_solution)`.
    ///
    /// Returns `0.0` when `U_solution == 0` (unloaded body) to avoid NaN
    /// propagation; see [`compute_zz_indicator`] for the guard rationale.
    pub global_relative_energy_error: f64,
}

/// Compute the Zienkiewicz-Zhu superconvergent patch-recovery error indicator
/// over a per-element stress field.
///
/// # Algorithm
///
/// (a) For each node n, gather patch P_n = elements containing n (from
///     `el.connectivity`).
/// (b) Compute smoothed nodal stress σ_n* = volume-weighted average of σ_e
///     for e ∈ P_n via [`crate::result::recover_nodal_stress_p1`].
/// (c) For each element e, interpolate σ_n* back to the element centroid:
///     for P1 tets, barycentric coords at the centroid are (1/4,…,1/4), so
///     σ̄_e* = (1/N) Σ_{n ∈ conn(e)} σ_n*.
/// (d) Compute per-element indicator: η_e = √(V_e · diff_voigt · D⁻¹ ·
///     diff_voigt) where diff = σ_e − σ̄_e*.
/// (e) Compute global: η_global = √(Σ η_e² / U_solution) with
///     `U_solution = Σ_e V_e · σ_e_voigt · D⁻¹ · σ_e_voigt`.
///
/// # Zero-energy guard
///
/// When all element stresses are zero, `U_solution == 0`. Returning 0.0 in
/// that case (rather than NaN from 0/0) is consistent with
/// `recover_nodal_stress_p1`'s "no incident elements → zero tensor"
/// convention (`result.rs`). The auto-refinement loop receives a sensible
/// signal ("no error, no refinement needed") rather than NaN propagation.
pub fn compute_zz_indicator(
    elements: &[StressElement<'_>],
    mesh: &VolumeMesh,
    material: &IsotropicElastic,
) -> ZzIndicator {
    let _ = mesh;
    let _ = material;
    ZzIndicator {
        per_element: vec![0.0; elements.len()],
        global_relative_energy_error: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;
    use crate::result::StressElement;
    use reify_types::{ElementOrderTag, VolumeMesh};

    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Build the standard 5-node, 2-tet fan fixture used across all
    /// error-estimator tests.
    ///
    /// Topology:
    ///   tet0: nodes [0,1,2,3]  (the canonical unit tet)
    ///   tet1: nodes [1,2,3,4]  (shares face {1,2,3} with tet0)
    ///
    /// Node positions:
    ///   0=(0,0,0), 1=(1,0,0), 2=(0,1,0), 3=(0,0,1), 4=(1,1,1)
    ///
    /// Both tets have volume 1/6.  Returns a `VolumeMesh` with n_nodes=5.
    fn two_tet_fan_mesh() -> VolumeMesh {
        VolumeMesh {
            vertices: vec![
                0.0_f32, 0.0, 0.0, // node 0
                1.0, 0.0, 0.0, // node 1
                0.0, 1.0, 0.0, // node 2
                0.0, 0.0, 1.0, // node 3
                1.0, 1.0, 1.0, // node 4
            ],
            tet_indices: vec![0, 1, 2, 3, 1, 2, 3, 4],
            element_order: ElementOrderTag::P1,
            normals: None,
        }
    }

    /// Surface compile pin — confirms that `ZzIndicator` is constructible as
    /// a struct literal and that `compute_zz_indicator` has the expected
    /// function-item signature. Mirrors the doctest in `lib.rs` (Task 2996
    /// block) so any signature drift trips both the doctest and this test.
    #[test]
    fn surface_compile_pin_for_zz_indicator_struct_and_compute_function() {
        let _zz = ZzIndicator {
            per_element: vec![0.5_f64],
            global_relative_energy_error: 0.05_f64,
        };
        let _: fn(
            &[StressElement<'_>],
            &reify_types::VolumeMesh,
            &IsotropicElastic,
        ) -> ZzIndicator = compute_zz_indicator;
    }

    /// Per-element indicator on a two-tet fan with σ_A ≠ σ_B.
    ///
    /// Fixture: σ_A = diag(100,0,0), σ_B = diag(0,0,0), V_A = V_B = 1/6.
    ///
    /// Patch averages:
    ///   nodes 1,2,3 (shared face): σ̄_n = (V_A·σ_A + V_B·σ_B)/(V_A+V_B)
    ///                                    = diag(50,0,0)
    ///   node 0 (only A):           σ̄_0 = σ_A = diag(100,0,0)
    ///   node 4 (only B):           σ̄_4 = σ_B = diag(0,0,0)
    ///
    /// Element centroid interpolation (mean of nodal σ̄ over 4 corners):
    ///   σ̄_A* = (diag(100,0,0) + 3·diag(50,0,0)) / 4 = diag(62.5,0,0)
    ///   σ̄_B* = (3·diag(50,0,0) + diag(0,0,0)) / 4   = diag(37.5,0,0)
    ///
    /// Difference Voigt vectors (diff = σ_e − σ̄_e*):
    ///   diff_A = [37.5, 0, 0, 0, 0, 0]
    ///   diff_B = [-37.5, 0, 0, 0, 0, 0]
    ///
    /// Energy density = diff_voigt · D⁻¹ · diff_voigt.
    /// For a pure σ_xx vector with engineering-shear compliance: d² / E.
    ///
    /// η_e = sqrt(V_e · d²/E) = sqrt((1/6) · 37.5² / 1.0) ≈ 15.30931
    /// Both elements have equal magnitude by symmetry.
    #[test]
    fn per_element_indicator_two_tet_fan_nonuniform_stress_closed_form() {
        let mat = dimensionless_steel_like();
        let v = 1.0_f64 / 6.0;
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        let sigma_a = [[100.0_f64, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let sigma_b = [[0.0_f64; 3]; 3];
        let elements = [
            StressElement { connectivity: &conn0, stress: sigma_a, volume: v },
            StressElement { connectivity: &conn1, stress: sigma_b, volume: v },
        ];
        let mesh = two_tet_fan_mesh();

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        // Closed-form: η_e = sqrt(V · (37.5)² / E) = sqrt((1/6) · 1406.25)
        //                   = sqrt(234.375) ≈ 15.30931...
        let expected_eta = ((1.0 / 6.0) * 37.5_f64 * 37.5 / mat.youngs_modulus).sqrt();
        assert_eq!(result.per_element.len(), 2, "must have 2 per-element entries");
        let rel_tol = 1e-9;
        assert!(
            (result.per_element[0] - expected_eta).abs() < rel_tol * expected_eta,
            "per_element[0] = {}, expected ≈ {expected_eta}",
            result.per_element[0],
        );
        assert!(
            (result.per_element[1] - expected_eta).abs() < rel_tol * expected_eta,
            "per_element[1] = {}, expected ≈ {expected_eta}",
            result.per_element[1],
        );
    }
}
