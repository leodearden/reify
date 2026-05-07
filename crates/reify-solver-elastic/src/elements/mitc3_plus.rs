//! MITC3+ Reissner-Mindlin shell element (Bathe & Lee 2014).
//!
//! # Reference
//!
//! Bathe, K.-J. & Lee, P.-S. (2014). "Towards improving the MITC9 shell
//! element." *Computers & Structures*, 81, 477–489; and Bathe, K.-J. &
//! Lee, P.-S. (2011). "An improvement of the MITC shell elements."
//! *Computers & Structures*, 89, 1413–1422.
//!
//! # Element description
//!
//! Three-node triangular shell element parameterized on a 2D mid-surface
//! reference triangle with vertices `(0,0)`, `(1,0)`, `(0,1)` in local
//! `(ξ, η)` coordinates.  Each node carries 6 DOFs (3 displacement + 3
//! rotation), giving 18 DOFs per element.
//!
//! The "+" distinguishes MITC3+ from plain MITC3: the rotation field is
//! enriched by a deviatoric cubic bubble `f_b(ξ,η) = ξ·η·(1−ξ−η)` that
//! eliminates spurious transverse-shear locking without additional DOFs.
//! Transverse-shear strains are interpolated from values sampled at the
//! three canonical edge-midpoint tying points A=(½,0), B=(0,½), C=(½,½).

/// A point in the local 2D mid-surface reference-triangle `(ξ, η)` space.
///
/// The unit reference triangle has vertices at `(0,0)`, `(1,0)`, `(0,1)`.
/// Barycentric coordinates are `(1−ξ−η, ξ, η)`.
///
/// This is a 2D type distinct from the 3D `ReferenceCoord` used by the
/// tetrahedral elements; shell elements are parameterised in the local
/// mid-surface plane only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellReferenceCoord {
    pub xi: f64,
    pub eta: f64,
}

impl ShellReferenceCoord {
    /// Construct a 2D shell reference-coordinate pair.
    pub const fn new(xi: f64, eta: f64) -> Self {
        Self { xi, eta }
    }
}

/// An edge-midpoint tying point used for the assumed transverse-shear strain
/// interpolation in MITC3+ (Bathe & Lee 2014).
///
/// Three tying points A, B, C sit at the edge midpoints of the reference
/// triangle.  The covariant transverse-shear strains are sampled at these
/// points and blended to form the assumed strain field used in the element
/// stiffness matrix (T6 concern).
///
/// - **A** = `(½, 0)` — midpoint of the `v_0–v_1` edge; `γ_ξζ` sampled here.
/// - **B** = `(0, ½)` — midpoint of the `v_0–v_2` edge; `γ_ηζ` sampled here.
/// - **C** = `(½, ½)` — midpoint of the `v_1–v_2` edge; both components coupled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TyingPoint {
    pub coord: ShellReferenceCoord,
}

/// Static array of the three MITC3+ tying points in canonical A, B, C order.
const TYING_POINTS: &[TyingPoint] = &[
    TyingPoint { coord: ShellReferenceCoord::new(0.5, 0.0) }, // A
    TyingPoint { coord: ShellReferenceCoord::new(0.0, 0.5) }, // B
    TyingPoint { coord: ShellReferenceCoord::new(0.5, 0.5) }, // C
];

/// MITC3+ Reissner-Mindlin triangular shell element.
///
/// Three-node element on the reference triangle with vertices `(0,0)`,
/// `(1,0)`, `(0,1)`. Each node carries 6 DOFs (3 displacements + 3
/// rotations), totalling `N_DOFS = 18` per element.
pub struct Mitc3Plus;

impl Mitc3Plus {
    /// Gradient of the cubic bubble enrichment at `coord`.
    ///
    /// Returns `[∂f_b/∂ξ, ∂f_b/∂η]` derived from `f_b = ξ·η·(1−ξ−η)` by
    /// the product rule:
    ///
    /// ```text
    /// ∂f_b/∂ξ = η·(1 − 2ξ − η)
    /// ∂f_b/∂η = ξ·(1 − ξ − 2η)
    /// ```
    ///
    /// Both components vanish at the centroid `(1/3, 1/3)` — the unique
    /// interior maximum of the bubble function.
    pub fn bubble_grad_at(&self, coord: ShellReferenceCoord) -> [f64; 2] {
        let ShellReferenceCoord { xi, eta } = coord;
        [
            eta * (1.0 - 2.0 * xi - eta),
            xi * (1.0 - xi - 2.0 * eta),
        ]
    }

    /// Cubic bubble enrichment at `coord`.
    ///
    /// Returns `f_b(ξ, η) = ξ · η · (1 − ξ − η)`.
    ///
    /// This is the "+" in MITC3+ (Bathe & Lee 2014): the deviatoric bubble
    /// enriches only the rotation field, not the displacement field.  It
    /// vanishes on every edge of the reference triangle — on the edge `η=0`
    /// the `η` factor is zero; on `ξ=0` the `ξ` factor is zero; on
    /// `ξ+η=1` the `(1−ξ−η)` factor is zero — so the enrichment does not
    /// introduce additional DOFs at nodes or edges.
    pub fn bubble_at(&self, coord: ShellReferenceCoord) -> f64 {
        let ShellReferenceCoord { xi, eta } = coord;
        xi * eta * (1.0 - xi - eta)
    }

    /// Shape-function gradients in reference coordinates at `coord`.
    ///
    /// Returns `[∇N_0, ∇N_1, ∇N_2]` where each gradient is
    /// `[∂N/∂ξ, ∂N/∂η]`.  The gradients are constant — independent of
    /// `coord` — because the P1 shape functions are linear:
    ///
    /// ```text
    /// ∇N_0 = (−1, −1),  ∇N_1 = (1, 0),  ∇N_2 = (0, 1)
    /// ```
    ///
    /// The `coord` argument is accepted for API uniformity with
    /// `bubble_grad_at`, which **is** coordinate-dependent.
    pub fn shape_grad_at(&self, _coord: ShellReferenceCoord) -> [[f64; 2]; 3] {
        [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]]
    }

    /// Standard 3-node Lagrangian shape functions at `coord`.
    ///
    /// Returns `[1 − ξ − η, ξ, η]` — i.e. the barycentric coordinates of
    /// reference vertex `v_i` in the canonical ordering `(0,0)`, `(1,0)`,
    /// `(0,1)`.  These are the displacement shapes and the base-rotation
    /// shapes; the bubble enrichment (rotation only) is exposed separately
    /// via `bubble_at` / `bubble_grad_at`.
    pub fn shape_at(&self, coord: ShellReferenceCoord) -> [f64; 3] {
        let ShellReferenceCoord { xi, eta } = coord;
        [1.0 - xi - eta, xi, eta]
    }

    /// Returns the three MITC3+ tying points in canonical A, B, C order.
    ///
    /// The static slice contains:
    /// - `A = (½, 0)` — covariant `γ_ξζ` is sampled here.
    /// - `B = (0, ½)` — covariant `γ_ηζ` is sampled here.
    /// - `C = (½, ½)` — both components coupled here.
    pub fn tying_points(&self) -> &'static [TyingPoint] {
        TYING_POINTS
    }

    /// Number of Lagrangian nodes.
    pub const N_NODES: usize = 3;

    /// DOFs per node (3 displacement + 3 rotation).
    pub const N_DOFS_PER_NODE: usize = 6;

    /// Total DOFs per element: `N_NODES × N_DOFS_PER_NODE = 18`.
    pub const N_DOFS: usize = Self::N_NODES * Self::N_DOFS_PER_NODE;

    /// Number of edge-midpoint tying points for the assumed transverse-shear
    /// strain interpolation (A, B, C in Bathe & Lee 2014 notation).
    pub const N_TYING_POINTS: usize = 3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mitc3_plus_dof_constants_match_18_dof_specification() {
        assert_eq!(Mitc3Plus::N_NODES, 3);
        assert_eq!(Mitc3Plus::N_DOFS_PER_NODE, 6);
        assert_eq!(Mitc3Plus::N_DOFS, 18);
        assert_eq!(Mitc3Plus::N_TYING_POINTS, 3);
    }

    const TOL: f64 = 1e-12;

    /// Reference vertices v_0, v_1, v_2 in canonical ordering.
    const REF_VERTICES: [ShellReferenceCoord; 3] = [
        ShellReferenceCoord::new(0.0, 0.0),
        ShellReferenceCoord::new(1.0, 0.0),
        ShellReferenceCoord::new(0.0, 1.0),
    ];

    #[test]
    fn shell_reference_coord_constructor_pins_xi_eta_fields() {
        let coord = ShellReferenceCoord::new(0.3, 0.4);
        assert_eq!(coord.xi, 0.3);
        assert_eq!(coord.eta, 0.4);
    }

    #[test]
    fn shape_at_satisfies_kronecker_delta_at_reference_vertices() {
        for (i, v) in REF_VERTICES.iter().enumerate() {
            let n = Mitc3Plus.shape_at(*v);
            assert_eq!(n.len(), 3, "shape_at must return N_NODES=3 entries");
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
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.2, 0.3),
        ];
        for p in &probes {
            let sum: f64 = Mitc3Plus.shape_at(*p).iter().sum();
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ N_i({:?}) = {sum}, expected 1.0",
                p,
            );
        }
    }

    #[test]
    fn shape_grad_at_returns_canonical_constant_gradients() {
        let probes = [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.0, 0.0),
            ShellReferenceCoord::new(0.1, 0.2),
        ];
        let expected: [[f64; 2]; 3] = [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]];
        for p in probes {
            let g = Mitc3Plus.shape_grad_at(p);
            assert_eq!(g.len(), 3, "shape_grad_at must return N_NODES=3 rows");
            for (i, row) in g.iter().enumerate() {
                for k in 0..2 {
                    assert!(
                        (row[k] - expected[i][k]).abs() < TOL,
                        "∇N_{i}({:?})[{k}] = {} expected {}",
                        p,
                        row[k],
                        expected[i][k],
                    );
                }
            }
        }
    }

    #[test]
    fn bubble_vanishes_at_three_reference_vertices() {
        for v in REF_VERTICES.iter() {
            let b = Mitc3Plus.bubble_at(*v);
            assert!(b.abs() < TOL, "bubble_at({:?}) = {b}, expected 0", v);
        }
    }

    #[test]
    fn bubble_vanishes_on_three_reference_edges() {
        // Edge midpoints A, B, C and some quarter-edge probes — each lies on
        // one of the three edges of the reference triangle, where one
        // barycentric coordinate is zero, so the bubble must vanish.
        let edge_probes = [
            ShellReferenceCoord::new(0.5, 0.0),  // mid of edge v0-v1 (η=0)
            ShellReferenceCoord::new(0.0, 0.5),  // mid of edge v0-v2 (ξ=0)
            ShellReferenceCoord::new(0.5, 0.5),  // mid of edge v1-v2 (ξ+η=1)
            ShellReferenceCoord::new(0.25, 0.0), // quarter of edge v0-v1
            ShellReferenceCoord::new(0.0, 0.25), // quarter of edge v0-v2
            ShellReferenceCoord::new(0.75, 0.25), // on edge v1-v2
        ];
        for p in edge_probes.iter() {
            let b = Mitc3Plus.bubble_at(*p);
            assert!(b.abs() < TOL, "bubble_at({:?}) = {b}, expected 0 (edge)", p);
        }
    }

    #[test]
    fn bubble_grad_matches_analytic_form_at_probes() {
        // Closed form: ∂f_b/∂ξ = η(1 − 2ξ − η), ∂f_b/∂η = ξ(1 − ξ − 2η)
        let probes = [
            ShellReferenceCoord::new(0.1, 0.2),
            ShellReferenceCoord::new(0.4, 0.3),
        ];
        for p in probes.iter() {
            let ShellReferenceCoord { xi, eta } = *p;
            let expected = [eta * (1.0 - 2.0 * xi - eta), xi * (1.0 - xi - 2.0 * eta)];
            let g = Mitc3Plus.bubble_grad_at(*p);
            for k in 0..2 {
                assert!(
                    (g[k] - expected[k]).abs() < TOL,
                    "bubble_grad_at({:?})[{k}] = {}, expected {}",
                    p,
                    g[k],
                    expected[k],
                );
            }
        }
    }

    #[test]
    fn tying_points_returns_three_canonical_edge_midpoints_in_a_b_c_order() {
        let tp = Mitc3Plus.tying_points();
        assert_eq!(tp.len(), 3, "must return exactly 3 tying points");
        // A = (½, 0), B = (0, ½), C = (½, ½)
        let expected_coords = [
            ShellReferenceCoord::new(0.5, 0.0),
            ShellReferenceCoord::new(0.0, 0.5),
            ShellReferenceCoord::new(0.5, 0.5),
        ];
        for (i, (tp_i, &exp)) in tp.iter().zip(expected_coords.iter()).enumerate() {
            assert!(
                (tp_i.coord.xi - exp.xi).abs() < TOL && (tp_i.coord.eta - exp.eta).abs() < TOL,
                "tying_points()[{i}].coord = {:?}, expected {:?}",
                tp_i.coord,
                exp,
            );
        }
    }

    #[test]
    fn tying_points_returned_slice_is_static() {
        let _: &'static [TyingPoint] = Mitc3Plus.tying_points();
    }

    #[test]
    fn bubble_grad_vanishes_at_centroid() {
        // Centroid is the unique interior maximum of f_b, so ∇f_b = 0 there.
        let g = Mitc3Plus.bubble_grad_at(ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0));
        for k in 0..2 {
            assert!(
                g[k].abs() < TOL,
                "bubble_grad_at(centroid)[{k}] = {}, expected 0",
                g[k],
            );
        }
    }

    #[test]
    fn bubble_equals_one_twenty_seventh_at_centroid() {
        // Centroid (1/3, 1/3): f_b = (1/3)·(1/3)·(1 − 2/3) = 1/27.
        let b = Mitc3Plus.bubble_at(ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0));
        let expected = 1.0 / 27.0;
        assert!(
            (b - expected).abs() < TOL,
            "bubble_at(centroid) = {b}, expected {expected}",
        );
    }

    #[test]
    fn shape_grad_at_sum_is_zero_partition_of_unity_consequence() {
        let g = Mitc3Plus.shape_grad_at(ShellReferenceCoord::new(0.1, 0.2));
        let mut sum = [0.0_f64; 2];
        for row in g {
            for k in 0..2 {
                sum[k] += row[k];
            }
        }
        for k in 0..2 {
            assert!((sum[k]).abs() < TOL, "Σ_i ∇N_i[{k}] = {}, expected 0", sum[k]);
        }
    }
}
