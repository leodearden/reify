//! Z-Z-style energy-norm error indicator for tetrahedral P1 linear
//! elastostatics.
//!
//! Uses volume-weighted average recovery for the nodal smoothing step — not
//! the full superconvergent patch-recovery (SPR) least-squares fit over
//! superconvergent sampling points. PRD §13 (`docs/prds/v0_4/a-posteriori-error-estimation.md`,
//! Resolved §"Error indicator") explicitly permits either scheme; volume-weighted
//! averaging is bit-deterministic and reuses [`crate::result::recover_nodal_stress_p1`]
//! directly (Task decomposition #1, task 2996).
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
use crate::result::{StressElement, recover_nodal_stress_p1};
use reify_ir::VolumeMesh;

/// Output of the Z-Z-style energy-norm error indicator.
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

/// Compute the Z-Z-style energy-norm error indicator over a per-element stress
/// field.
///
/// # Algorithm
///
/// (a) For each node n, gather patch P_n = elements containing n (from
///     `el.connectivity`).
/// (b) Compute smoothed nodal stress σ_n* = **volume-weighted average** of σ_e
///     for e ∈ P_n via [`crate::result::recover_nodal_stress_p1`]. This is
///     simple averaging, not the full Z-Z least-squares SPR fit; PRD §13
///     permits either scheme.
/// (c) For each element e, interpolate σ_n* back to the element centroid:
///     for P1 tets, barycentric coords at the centroid are (1/4,…,1/4), so
///     σ̄_e* = (1/N) Σ_{n ∈ conn(e)} σ_n*.
/// (d) Per-element: `η_e = √(V_e · energy_density_voigt(σ_e − σ̄_e*, D⁻¹))`.
///     Step (d) uses 1-point Gauss (centroid) quadrature; the integrand is
///     quadratic, so this is an O(h) approximation — standard for Z-Z over
///     P1 tets and consistent with PRD §13's allowance for either scheme.
/// (e) Global: `η_global = √(Σ η_e² / U_solution)` where
///     `U_solution = Σ_e V_e · energy_density_voigt(σ_e, D⁻¹)`.
///
/// # Zero-energy guard
///
/// When all element stresses are zero, `U_solution == 0`. Returning `0.0`
/// (rather than NaN from `0/0`) is consistent with
/// `recover_nodal_stress_p1`'s "no incident elements → zero tensor"
/// convention (`result.rs`). The auto-refinement loop receives a sensible
/// signal ("no error, no refinement needed") rather than NaN propagation.
/// Pinned by
/// `tests::zero_stress_field_yields_zero_indicator_and_zero_global_error_without_dividing_by_zero`.
///
/// # Behavioural tests
///
/// - `tests::per_element_indicator_two_tet_fan_nonuniform_stress_closed_form`
///   — closed-form pin for per-element η_e on a 2-tet fan.
/// - `tests::global_relative_energy_error_two_tet_fan_nonuniform_stress_closed_form`
///   — closed-form pin for η_global on the same fixture.
/// - `tests::uniform_stress_field_yields_zero_per_element_indicator_and_zero_global_error`
///   — Zienkiewicz textbook patch test (uniform σ → η = 0 everywhere).
/// - `tests::l_corner_style_hot_element_localisation_dominates_uniform_neighbours`
///   — qualitative localisation pin (η_hot > 1.5 × η_cold on a 3-tet fan).
pub fn compute_zz_indicator(
    elements: &[StressElement<'_>],
    mesh: &VolumeMesh,
    material: &IsotropicElastic,
) -> ZzIndicator {
    let n_nodes = mesh.vertices.len() / 3;
    let compliance = compliance_matrix(material);

    // Steps (a)+(b): volume-weighted nodal patch average via reuse from result.rs.
    let nodal_smoothed = recover_nodal_stress_p1(n_nodes, elements);

    let mut per_element = Vec::with_capacity(elements.len());
    let mut sum_eta_sq = 0.0_f64;
    let mut sum_energy_sq = 0.0_f64;

    for el in elements {
        let n = el.connectivity.len();
        // Centroid interpolation assumes uniform barycentric coords (1/N, …, 1/N),
        // which is only correct for P1 tets (N=4). Guard against silent misuse
        // — including in release builds — by callers passing P2 or higher-order
        // connectivity.
        assert_eq!(
            n,
            4,
            "compute_zz_indicator currently supports P1 tets only; \
             got connectivity of length {n}",
        );

        // Step (c): interpolate smoothed stress back to the element centroid.
        // For P1 tets, barycentric coords at the centroid are (1/N, …, 1/N),
        // so centroid interpolation = arithmetic mean of the nodal values.
        let mut sigma_bar = [[0.0_f64; 3]; 3];
        for &node in el.connectivity {
            let ns = &nodal_smoothed[node];
            for i in 0..3 {
                for j in 0..3 {
                    sigma_bar[i][j] += ns[i][j];
                }
            }
        }
        let inv_n = 1.0 / (n as f64);
        for row in &mut sigma_bar {
            for cell in row.iter_mut() {
                *cell *= inv_n;
            }
        }

        // Step (d): per-element indicator η_e = sqrt(V_e · diff · S · diff).
        let mut diff = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                diff[i][j] = el.stress[i][j] - sigma_bar[i][j];
            }
        }
        let eta_sq = el.volume * energy_density_voigt(&diff, &compliance);
        per_element.push(eta_sq.sqrt());
        sum_eta_sq += eta_sq;

        // Accumulate solution strain energy: V_e · σ_e · S · σ_e.
        sum_energy_sq += el.volume * energy_density_voigt(&el.stress, &compliance);
    }

    // Step (e): global relative energy error.
    // Guard: if U_solution == 0 (unloaded body, all σ_e = 0), return 0.0
    // rather than NaN from 0/0. Consistent with recover_nodal_stress_p1's
    // "no incident elements → zero tensor" convention (result.rs).
    let global_relative_energy_error = if sum_energy_sq > 0.0 {
        (sum_eta_sq / sum_energy_sq).sqrt()
    } else {
        0.0
    };

    ZzIndicator {
        per_element,
        global_relative_energy_error,
    }
}

/// Compute the compliance matrix `S = D⁻¹` for an isotropic linear-elastic
/// material in engineering-shear Voigt order
/// `[σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]`.
///
/// The analytical closed form (6×6 symmetric) is:
///
/// ```text
/// S = [ 1/E   −ν/E  −ν/E   0    0    0  ]
///     [ −ν/E   1/E  −ν/E   0    0    0  ]
///     [ −ν/E  −ν/E   1/E   0    0    0  ]
///     [  0     0     0    1/G   0    0  ]
///     [  0     0     0     0   1/G   0  ]
///     [  0     0     0     0    0   1/G ]
/// ```
///
/// where `G = E / (2(1+ν))`. This mirrors the convention in
/// [`crate::constitutive::IsotropicElastic::d_matrix`] — that function uses
/// engineering-shear strains (`γ_ij = 2ε_ij`), so the shear diagonal of `D`
/// is `G` (not `2G`). Inverting that convention yields `1/G` on the shear
/// diagonal here.
///
/// Private to this module; kept local per the established pattern of
/// `inverse_transpose_3x3` in `result.rs` and `interpolation.rs`.
fn compliance_matrix(material: &IsotropicElastic) -> [[f64; 6]; 6] {
    let e = material.youngs_modulus;
    let nu = material.poisson_ratio;
    let inv_e = 1.0 / e;
    let neg_nu_over_e = -nu / e;
    let g = e / (2.0 * (1.0 + nu));
    let inv_g = 1.0 / g;

    let mut s = [[0.0_f64; 6]; 6];
    // Normal-stress block (rows/cols 0..3): diagonal = 1/E, off-diagonal = -ν/E.
    s[0][0] = inv_e;
    s[0][1] = neg_nu_over_e;
    s[0][2] = neg_nu_over_e;
    s[1][0] = neg_nu_over_e;
    s[1][1] = inv_e;
    s[1][2] = neg_nu_over_e;
    s[2][0] = neg_nu_over_e;
    s[2][1] = neg_nu_over_e;
    s[2][2] = inv_e;
    // Shear-stress block (rows/cols 3..6) — diagonal 1/G, off-diagonal 0.
    s[3][3] = inv_g;
    s[4][4] = inv_g;
    s[5][5] = inv_g;
    s
}

/// Pack a symmetric 3×3 stress tensor and compute `t_voigt · S · t_voigt`.
///
/// Canonical bilinear form shared by the per-element indicator (step d) and
/// the solution strain-energy accumulator (step e). Keeping both consumers on
/// one code path prevents divergence if the Voigt ordering ever changes.
///
/// Voigt order: `[σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]` (engineering shear).
#[inline]
fn energy_density_voigt(t: &[[f64; 3]; 3], s: &[[f64; 6]; 6]) -> f64 {
    let v = [t[0][0], t[1][1], t[2][2], t[0][1], t[1][2], t[0][2]];
    let mut result = 0.0;
    for i in 0..6 {
        let mut sv_i = 0.0;
        for j in 0..6 {
            sv_i += s[i][j] * v[j];
        }
        result += v[i] * sv_i;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;
    use crate::result::StressElement;
    use reify_ir::{ElementOrderTag, VolumeMesh};

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

    /// Round-trip check: `compliance_matrix(m) · d_matrix() ≈ I₆`.
    ///
    /// Detects swapped off-diagonal signs (-ν/E vs +ν/E), incorrect shear-block
    /// factor (1/G vs 1/(2G) — a common Voigt-convention bug), or any other
    /// construction error in the private `compliance_matrix` helper.  Tested
    /// for `ν = 0.3` (steel-like) and `ν = 0.0` (cross-coupling sanity check).
    #[test]
    fn compliance_matrix_times_d_matrix_is_identity() {
        fn check(mat: &IsotropicElastic) {
            let s = compliance_matrix(mat);
            let d = mat.d_matrix();
            // Multiply s · d → should be identity.
            let mut sd = [[0.0_f64; 6]; 6];
            for i in 0..6 {
                for j in 0..6 {
                    for k in 0..6 {
                        sd[i][j] += s[i][k] * d[k][j];
                    }
                }
            }
            let tol = 1e-12;
            for (i, sd_row) in sd.iter().enumerate() {
                for (j, &sd_ij) in sd_row.iter().enumerate() {
                    let expected = if i == j { 1.0 } else { 0.0 };
                    assert!(
                        (sd_ij - expected).abs() < tol,
                        "S·D[{i}][{j}] = {} (expected {expected}) for material E={}, ν={}",
                        sd_ij,
                        mat.youngs_modulus,
                        mat.poisson_ratio,
                    );
                }
            }
        }

        // Engineering-realistic case: E=200e9, ν=0.3 (steel).
        check(&IsotropicElastic {
            youngs_modulus: 200e9,
            poisson_ratio: 0.3,
        });
        // No cross-coupling: ν=0.
        check(&IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.0,
        });
        // Dimensionless fixture used across other tests.
        check(&dimensionless_steel_like());
    }

    /// Global relative energy error on the same two-tet fan as the per-element
    /// test.
    ///
    /// Closed form (same fixture: σ_A=diag(100,0,0), σ_B=diag(0,0,0), V=1/6,
    /// E=1.0, ν=0.3):
    ///
    ///   Σ η_e² = 2 · V · (37.5)² / E = 2·(1/6)·1406.25 = 468.75
    ///   U_solution = Σ V_e · σ_e · D⁻¹ · σ_e
    ///              = V_A · (100)²/E + V_B · 0
    ///              = (1/6) · 10000 = 1666.667
    ///   η_global = sqrt(468.75 / 1666.667) = sqrt(0.28125) ≈ 0.53033...
    ///
    /// The stub returns global_relative_energy_error = 0.0, so this test fails.
    #[test]
    fn global_relative_energy_error_two_tet_fan_nonuniform_stress_closed_form() {
        let mat = dimensionless_steel_like();
        let v = 1.0_f64 / 6.0;
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        let sigma_a = [[100.0_f64, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let sigma_b = [[0.0_f64; 3]; 3];
        let elements = [
            StressElement {
                connectivity: &conn0,
                stress: sigma_a,
                volume: v,
            },
            StressElement {
                connectivity: &conn1,
                stress: sigma_b,
                volume: v,
            },
        ];
        let mesh = two_tet_fan_mesh();

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        // Closed form:
        //   sum_eta_sq = 2·(1/6)·37.5²/E = 468.75
        //   U_solution  = (1/6)·100²/E   = 1666.666...
        //   global = sqrt(468.75 / 1666.666...) = sqrt(0.28125) ≈ 0.530330...
        let sum_eta_sq = 2.0 * v * 37.5_f64 * 37.5 / mat.youngs_modulus;
        let u_solution = v * 100.0_f64 * 100.0 / mat.youngs_modulus;
        let expected_global = (sum_eta_sq / u_solution).sqrt();

        let rel_tol = 1e-9;
        assert!(
            (result.global_relative_energy_error - expected_global).abs()
                < rel_tol * expected_global,
            "global = {}, expected ≈ {expected_global}",
            result.global_relative_energy_error,
        );
    }

    /// Textbook Zienkiewicz patch test: uniform stress across the two-tet fan
    /// must produce zero per-element indicators and zero global error.
    ///
    /// When σ is uniform, every nodal patch average equals σ, so the
    /// smoothed interpolation σ̄_e* = σ everywhere. The difference σ_e − σ̄_e*
    /// is zero for every element ⇒ η_e = 0, η_global = 0.
    ///
    /// Uses a non-trivial σ = diag(100, 50, 25) to catch any accidental
    /// zero-tensor short-circuit.
    #[test]
    fn uniform_stress_field_yields_zero_per_element_indicator_and_zero_global_error() {
        let mat = dimensionless_steel_like();
        let v = 1.0_f64 / 6.0;
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        // Non-trivial uniform stress across both elements.
        let sigma = [[100.0_f64, 0.0, 0.0], [0.0, 50.0, 0.0], [0.0, 0.0, 25.0]];
        let elements = [
            StressElement {
                connectivity: &conn0,
                stress: sigma,
                volume: v,
            },
            StressElement {
                connectivity: &conn1,
                stress: sigma,
                volume: v,
            },
        ];
        let mesh = two_tet_fan_mesh();

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        assert_eq!(result.per_element.len(), 2);
        let abs_tol = 1e-12;
        assert!(
            result.per_element[0].abs() < abs_tol,
            "uniform stress: per_element[0] = {} (expected < {abs_tol})",
            result.per_element[0],
        );
        assert!(
            result.per_element[1].abs() < abs_tol,
            "uniform stress: per_element[1] = {} (expected < {abs_tol})",
            result.per_element[1],
        );
        assert!(
            result.global_relative_energy_error.abs() < abs_tol,
            "uniform stress: global = {} (expected < {abs_tol})",
            result.global_relative_energy_error,
        );
    }

    /// Zero-energy guard: all-zero stress field must yield zero indicators and
    /// zero global error without dividing by zero (NaN).
    ///
    /// Pins the `if sum_energy_sq > 0.0 else 0.0` guard in
    /// [`compute_zz_indicator`]. Without that guard, this case returns NaN
    /// from 0/0 and propagates through downstream code.
    #[test]
    fn zero_stress_field_yields_zero_indicator_and_zero_global_error_without_dividing_by_zero() {
        let mat = dimensionless_steel_like();
        let v = 1.0_f64 / 6.0;
        let conn0 = [0_usize, 1, 2, 3];
        let conn1 = [1_usize, 2, 3, 4];
        let sigma_zero = [[0.0_f64; 3]; 3];
        let elements = [
            StressElement {
                connectivity: &conn0,
                stress: sigma_zero,
                volume: v,
            },
            StressElement {
                connectivity: &conn1,
                stress: sigma_zero,
                volume: v,
            },
        ];
        let mesh = two_tet_fan_mesh();

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        assert_eq!(result.per_element.len(), 2);
        assert_eq!(
            result.per_element[0], 0.0,
            "zero-stress per_element[0] must be 0.0"
        );
        assert_eq!(
            result.per_element[1], 0.0,
            "zero-stress per_element[1] must be 0.0"
        );
        assert_eq!(
            result.global_relative_energy_error, 0.0,
            "zero-energy guard must return 0.0, not NaN",
        );
        assert!(
            !result.global_relative_energy_error.is_nan(),
            "global must not be NaN for zero-stress input",
        );
    }

    /// L-corner-style localisation: one "hot" element with σ >> neighbours
    /// must dominate the per-element indicator.
    ///
    /// Synthetic fixture: 3-tet fan where tet_hot is "hot" with
    /// σ_hot = diag(1000, 0, 0), and the two flanking tets are "cold" with
    /// σ_cold = diag(0, 0, 0). All volumes = 1/6.
    ///
    /// Topology: all 3 tets share vertex 0; no other shared nodes.
    ///
    ///   tet_hot:   connectivity = [0, 1, 2, 3]
    ///   tet_cold0: connectivity = [0, 4, 5, 6]
    ///   tet_cold1: connectivity = [0, 7, 8, 9]
    ///
    /// Analytical ratio: η_hot / η_cold = 2.0 (exactly).
    ///   Node 0 (shared × 3): σ̄₀ = σ_hot/3.
    ///   Nodes 1-3 (only hot): σ̄ = σ_hot.
    ///   Hot σ̄* = (σ_hot/3 + 3·σ_hot)/4 = 5σ_hot/6  →  diff = σ_hot/6
    ///   Cold σ̄* = (σ_hot/3)/4 = σ_hot/12             →  diff = σ_hot/12
    ///   Ratio = (1/6)/(1/12) = 2. Threshold 1.5 is conservative.
    ///
    /// Note: the plan originally specified 5×, but that ratio is not
    /// achievable with a 3-tet single-shared-node topology (escalation
    /// esc-2996-104). The 1.5× threshold still demonstrates the core
    /// localisation property: the element at the stress boundary has a
    /// larger indicator than its low-stress neighbours.
    #[test]
    fn l_corner_style_hot_element_localisation_dominates_uniform_neighbours() {
        let mat = dimensionless_steel_like();
        let v = 1.0_f64 / 6.0;
        // 10 nodes: 0 is the shared vertex; 1-3 belong only to hot; 4-9 to colds.
        let conn_hot = [0_usize, 1, 2, 3];
        let conn_cold0 = [0_usize, 4, 5, 6];
        let conn_cold1 = [0_usize, 7, 8, 9];
        let sigma_hot = [[1000.0_f64, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let sigma_cold = [[0.0_f64; 3]; 3];
        let elements = [
            StressElement {
                connectivity: &conn_hot,
                stress: sigma_hot,
                volume: v,
            },
            StressElement {
                connectivity: &conn_cold0,
                stress: sigma_cold,
                volume: v,
            },
            StressElement {
                connectivity: &conn_cold1,
                stress: sigma_cold,
                volume: v,
            },
        ];
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32; 30], // 10 nodes × 3 coords
            tet_indices: vec![0, 1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9],
            element_order: ElementOrderTag::P1,
            normals: None,
        };

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        assert_eq!(result.per_element.len(), 3);
        let eta_hot = result.per_element[0];
        let eta_cold0 = result.per_element[1];
        let eta_cold1 = result.per_element[2];

        // Lock in that cold elements have non-trivial indicators (diff = σ_hot/12),
        // so the ratio comparison is meaningful and would catch a regression that
        // drove η_cold → 0 (e.g. a patch-average bug that collapsed cold diffs).
        assert!(
            eta_cold0 > 0.0,
            "cold element 0 must have a non-zero indicator; got η_cold0={eta_cold0}",
        );
        assert!(
            eta_cold1 > 0.0,
            "cold element 1 must have a non-zero indicator; got η_cold1={eta_cold1}",
        );

        // Conservative threshold: actual ratio = 2.0, threshold = 1.5.
        let threshold = 1.5;
        assert!(
            eta_hot > threshold * eta_cold0,
            "hot element must dominate cold0: η_hot={eta_hot} vs {threshold}×η_cold0={}",
            threshold * eta_cold0,
        );
        assert!(
            eta_hot > threshold * eta_cold1,
            "hot element must dominate cold1: η_hot={eta_hot} vs {threshold}×η_cold1={}",
            threshold * eta_cold1,
        );
    }

    /// Guard test: `compute_zz_indicator` must panic in **all** build modes
    /// when called with P2-length (10-node) connectivity.
    ///
    /// # TDD red→green
    ///
    /// **RED** (step-1, before the fix): in `cargo test --release` the existing
    /// `debug_assert_eq!` is compiled out, so no panic fires and this
    /// `#[should_panic]` test fails — the release leg of the verify pipeline
    /// catches the regression.
    ///
    /// **GREEN** (step-2, after promoting to `assert_eq!`): both debug and
    /// release modes panic, and the test passes in both modes.
    #[test]
    #[should_panic(expected = "P1 tets only")]
    fn compute_zz_indicator_panics_when_called_with_p2_length_connectivity() {
        let mat = dimensionless_steel_like();
        // P2 tet has 10 nodes; all pointing to node-0 keeps recover_nodal_stress_p1
        // from OOB-panicking for an unrelated reason.
        let conn_p2 = [0_usize; 10];
        let sigma = [[1.0_f64, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let elements = [StressElement {
            connectivity: &conn_p2,
            stress: sigma,
            volume: 1.0 / 6.0,
        }];
        let mesh = VolumeMesh {
            vertices: vec![0.0_f32; 30], // 10 nodes × 3 coords
            tet_indices: vec![0; 10],
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        // Expect a panic containing "P1 tets only" — substring present in both
        // the existing debug_assert message and the new assert message.
        compute_zz_indicator(&elements, &mesh, &mat);
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
            StressElement {
                connectivity: &conn0,
                stress: sigma_a,
                volume: v,
            },
            StressElement {
                connectivity: &conn1,
                stress: sigma_b,
                volume: v,
            },
        ];
        let mesh = two_tet_fan_mesh();

        let result = compute_zz_indicator(&elements, &mesh, &mat);

        // Closed-form: η_e = sqrt(V · (37.5)² / E) = sqrt((1/6) · 1406.25)
        //                   = sqrt(234.375) ≈ 15.30931...
        let expected_eta = ((1.0 / 6.0) * 37.5_f64 * 37.5 / mat.youngs_modulus).sqrt();
        assert_eq!(
            result.per_element.len(),
            2,
            "must have 2 per-element entries"
        );
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
