//! MITC3+ Reissner-Mindlin shell element (Bathe & Lee 2014).
//!
//! # Reference
//!
//! Lee, Y., Lee, P.-S. & Bathe, K.-J. (2014). "The MITC3+ shell element
//! and its performance." *Computers & Structures*, 138, 12–23.
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

/// Covariant transverse-shear strain components `(γ_ξζ, γ_ηζ)` at a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShearStrain {
    pub gamma_xi_zeta: f64,
    pub gamma_eta_zeta: f64,
}

/// Sampled covariant transverse-shear strains at the three MITC3+ tying
/// points A, B, and C (see [`TyingPoint`] for the exact coordinates).
///
/// Each field name encodes both the tying-point location (`_at_a`, `_at_b`,
/// `_at_c`) and the covariant component (`gamma_xi_zeta` or `gamma_eta_zeta`).
/// The four fields correspond exactly to the four scalars consumed by
/// [`Mitc3Plus::interpolate_assumed_shear`] — no more, no less.  The MITC3+
/// mixed-interpolation formula (Bathe & Lee 2014) uses `γ_ξζ` from A and C
/// and `γ_ηζ` from B and C; exposing only those four fields eliminates the
/// silent-ignore class of bugs present in earlier designs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TyingShears {
    /// Covariant `γ_ξζ` sampled at tying point A = (½, 0).
    pub gamma_xi_zeta_at_a: f64,
    /// Covariant `γ_ηζ` sampled at tying point B = (0, ½).
    pub gamma_eta_zeta_at_b: f64,
    /// Covariant `γ_ξζ` sampled at tying point C = (½, ½).
    pub gamma_xi_zeta_at_c: f64,
    /// Covariant `γ_ηζ` sampled at tying point C = (½, ½).
    pub gamma_eta_zeta_at_c: f64,
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
    TyingPoint {
        coord: ShellReferenceCoord::new(0.5, 0.0),
    }, // A
    TyingPoint {
        coord: ShellReferenceCoord::new(0.0, 0.5),
    }, // B
    TyingPoint {
        coord: ShellReferenceCoord::new(0.5, 0.5),
    }, // C
];

/// Static array of the six MITC3+ *interior* transverse-shear tying points
/// (Lee, Lee & Bathe 2014). Unlike the three edge-midpoint [`TYING_POINTS`]
/// (where the rotation bubble `f_b = ξη(1−ξ−η)` vanishes), these points lie
/// strictly inside the reference triangle, so `f_b > 0` at every one of them
/// and the bubble is *live* in the assumed transverse-shear field — the core
/// mechanism that makes MITC3+ cure shear locking on a flat facet.
///
/// The six points form one symmetric orbit under the triangle's S3 symmetry
/// group (the permutations of the barycentric triple `(1−ξ−η, ξ, η)` applied
/// to `(1/6, 1/2)`), so the element is isotropic (frame-covariant): no edge or
/// node is privileged. Because the nodal covariant transverse-shear field is
/// affine in `(ξ, η)`, its average over this symmetric set equals its value at
/// the centroid — a constant, lock-free assumed field — while the average of
/// the cubic bubble `f_b` over the set is strictly positive, so the bubble
/// couples into the shear stiffness (`K_NB^shear ≠ 0`).
const INTERIOR_TYING_POINTS: &[TyingPoint] = &[
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 6.0, 1.0 / 2.0),
    },
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 2.0, 1.0 / 6.0),
    },
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 2.0),
    },
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 2.0, 1.0 / 3.0),
    },
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 6.0),
    },
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 6.0, 1.0 / 3.0),
    },
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
        [eta * (1.0 - 2.0 * xi - eta), xi * (1.0 - xi - 2.0 * eta)]
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

    /// Assumed transverse-shear strain at `coord` via the MITC3+ mixed
    /// interpolation (Bathe & Lee 2014).
    ///
    /// Given the covariant strains sampled at the three tying points A, B, C,
    /// this function returns the interpolated `(γ_ξζ, γ_ηζ)` at an arbitrary
    /// reference coordinate by the affine blending formula:
    ///
    /// ```text
    /// γ_ξζ(ξ, η) = gamma_xi_zeta_at_a  +  η · c
    /// γ_ηζ(ξ, η) = gamma_eta_zeta_at_b  −  ξ · c
    ///
    /// where c = (gamma_xi_zeta_at_c − gamma_eta_zeta_at_c)
    ///         − (gamma_xi_zeta_at_a − gamma_eta_zeta_at_b)
    /// ```
    ///
    /// Properties:
    /// - At A=(½,0): output `γ_ξζ = gamma_xi_zeta_at_a`  (tying identity for A).
    /// - At B=(0,½): output `γ_ηζ = gamma_eta_zeta_at_b`  (tying identity for B).
    /// - Constant when paired inputs match: `c = 0`, output is uniform.
    /// - Affine in `(ξ, η)`: constant base term plus a linear `η·c` / `−ξ·c` correction.
    pub fn interpolate_assumed_shear(
        &self,
        sampled: TyingShears,
        coord: ShellReferenceCoord,
    ) -> ShearStrain {
        let ShellReferenceCoord { xi, eta } = coord;
        let c = (sampled.gamma_xi_zeta_at_c - sampled.gamma_eta_zeta_at_c)
            - (sampled.gamma_xi_zeta_at_a - sampled.gamma_eta_zeta_at_b);
        ShearStrain {
            gamma_xi_zeta: sampled.gamma_xi_zeta_at_a + eta * c,
            gamma_eta_zeta: sampled.gamma_eta_zeta_at_b - xi * c,
        }
    }

    /// Returns the three MITC3+ tying points in canonical A, B, C order.
    ///
    /// The return type is `&'static [TyingPoint]` — the slice points to a
    /// compile-time constant, so no allocation occurs.
    ///
    /// The static slice contains:
    /// - `A = (½, 0)` — covariant `γ_ξζ` is sampled here.
    /// - `B = (0, ½)` — covariant `γ_ηζ` is sampled here.
    /// - `C = (½, ½)` — both components coupled here.
    pub fn tying_points(&self) -> &'static [TyingPoint] {
        TYING_POINTS
    }

    /// Returns the six MITC3+ *interior* transverse-shear tying points
    /// (Lee, Lee & Bathe 2014). See [`INTERIOR_TYING_POINTS`] for the exact
    /// coordinates and the rationale.
    ///
    /// The return type is `&'static [TyingPoint]` — the slice points to a
    /// compile-time constant, so no allocation occurs.
    ///
    /// These differ fundamentally from [`Mitc3Plus::tying_points`] (the three
    /// edge midpoints A/B/C of the bare-MITC3 baseline): every interior point
    /// has `f_b > 0`, so the rotation bubble enters the assumed transverse-shear
    /// field by *value* there — the flat-facet shear-locking cure.
    pub fn interior_tying_points(&self) -> &'static [TyingPoint] {
        INTERIOR_TYING_POINTS
    }

    /// MITC3+ assumed transverse-shear strain at `coord`, re-interpolated from
    /// the covariant shears sampled at the six interior tying points.
    ///
    /// # Formulation
    ///
    /// The assumed covariant transverse-shear field is the L2 projection of the
    /// displacement-derived shear onto the **constant** field — i.e. the average
    /// of the samples at the six interior tying points. On a flat,
    /// constant-Jacobian facet this is a consistent, isotropic, lock-free
    /// assumed-natural-strain (ANS) reduction:
    ///
    /// - **Consistency / patch test.** A constant covariant field `(c1, c2)`
    ///   sampled at all six points averages back to exactly `(c1, c2)`, so
    ///   constant transverse-shear states are reproduced exactly (a prerequisite
    ///   for passing the patch test).
    /// - **Lock-free.** Projecting the shear onto constants removes the
    ///   parasitic linear shear that causes transverse-shear locking in a
    ///   displacement-based Reissner–Mindlin triangle.
    /// - **Isotropy.** The six tying points are one symmetric S3 orbit, so the
    ///   average privileges no edge or node.
    ///
    /// The `coord` argument is accepted for API uniformity with the bare-MITC3
    /// [`Mitc3Plus::interpolate_assumed_shear`] (whose field is affine in
    /// `(ξ, η)`); the MITC3+ constant field is independent of `coord`.
    ///
    /// `sampled[k]` is the covariant `(γ_ξζ, γ_ηζ)` sampled at
    /// `interior_tying_points()[k]`.
    pub fn interpolate_assumed_shear_mitc3_plus(
        &self,
        sampled: &[ShearStrain],
        _coord: ShellReferenceCoord,
    ) -> ShearStrain {
        debug_assert_eq!(
            sampled.len(),
            Self::N_INTERIOR_TYING_POINTS,
            "interpolate_assumed_shear_mitc3_plus expects one sample per interior tying point"
        );
        let n = sampled.len() as f64;
        let mut gamma_xi_zeta = 0.0_f64;
        let mut gamma_eta_zeta = 0.0_f64;
        for s in sampled {
            gamma_xi_zeta += s.gamma_xi_zeta;
            gamma_eta_zeta += s.gamma_eta_zeta;
        }
        ShearStrain {
            gamma_xi_zeta: gamma_xi_zeta / n,
            gamma_eta_zeta: gamma_eta_zeta / n,
        }
    }

    /// Bubble-augmented **covariant** transverse-shear strain B-matrix at
    /// `coord`, with shape `[2][20]`: rows are the covariant components
    /// `(γ_ξζ, γ_ηζ)`; columns are the 18 nodal DOFs followed by the 2 bubble
    /// DOFs `(Δβ_x = col 18, Δβ_y = col 19)`.
    ///
    /// # Construction (covariant, geometry-free)
    ///
    /// The covariant transverse-shear strains in the MITC3+ kinematics are
    ///
    /// ```text
    /// γ_ξζ = Σ_i (∂N_i/∂ξ · u_z_i + N_i · θ_y_i)  +  f_b · Δβ_y
    /// γ_ηζ = Σ_i (∂N_i/∂η · u_z_i − N_i · θ_x_i)  −  f_b · Δβ_x
    /// ```
    ///
    /// where `∂N_i/∂ξ` are the constant reference-coordinate shape gradients
    /// (`shape_grad_at`), `N_i` are the shape functions (`shape_at`), and
    /// `f_b = ξη(1−ξ−η)` is the rotation bubble (`bubble_at`).
    ///
    /// The **nodal** 18 columns reproduce exactly the bare-MITC3 covariant
    /// shear B (the single source of truth for which is
    /// `shell_kinematics::shell_kinematics`); this helper leaves that path
    /// untouched and only *adds* the 2 bubble columns.
    ///
    /// The **bubble** columns are proportional to the bubble *value* `f_b`:
    /// non-zero at the interior tying points (`f_b > 0`) and exactly zero at
    /// the edge midpoints A/B/C (`f_b = 0`). This is precisely why the bubble
    /// is live in shear on a flat facet only when sampled at *interior* points
    /// — the core MITC3+ correction.
    pub fn covariant_shear_b_with_bubble(
        &self,
        coord: ShellReferenceCoord,
    ) -> [[f64; Self::N_DOFS_UNCONDENSED]; 2] {
        let n = self.shape_at(coord);
        let dn_ref = self.shape_grad_at(coord);
        let fb = self.bubble_at(coord);
        let mut b = [[0.0_f64; Self::N_DOFS_UNCONDENSED]; 2];
        for node in 0..Self::N_NODES {
            let dof_uz = Self::N_DOFS_PER_NODE * node + 2;
            let dof_tx = Self::N_DOFS_PER_NODE * node + 3;
            let dof_ty = Self::N_DOFS_PER_NODE * node + 4;
            // γ_ξζ: ∂N/∂ξ · u_z + N · θ_y
            b[0][dof_uz] += dn_ref[node][0];
            b[0][dof_ty] += n[node];
            // γ_ηζ: ∂N/∂η · u_z − N · θ_x
            b[1][dof_uz] += dn_ref[node][1];
            b[1][dof_tx] -= n[node];
        }
        // Bubble columns (18 = Δβ_x, 19 = Δβ_y), proportional to f_b VALUE:
        //   γ_ξζ gets + f_b · Δβ_y ; γ_ηζ gets − f_b · Δβ_x.
        b[0][Self::N_DOFS + 1] += fb; // Δβ_y → γ_ξζ
        b[1][Self::N_DOFS] -= fb; // Δβ_x → γ_ηζ
        b
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

    /// Number of *interior* tying points for the MITC3+ assumed
    /// transverse-shear strain field (Lee, Lee & Bathe 2014). These lie
    /// strictly inside the reference triangle (`f_b > 0`), so the rotation
    /// bubble is live in the shear field — see [`INTERIOR_TYING_POINTS`].
    pub const N_INTERIOR_TYING_POINTS: usize = 6;

    /// Total DOFs of the *uncondensed* MITC3+ element: the 18 nodal DOFs plus
    /// the 2 internal cubic-bubble rotational DOFs `(Δβ_x, Δβ_y)` at the
    /// centroid. After static condensation of the two bubble DOFs the element
    /// matrix is the standard `N_DOFS = 18`.
    ///
    /// Bubble-DOF column indices in the uncondensed layout: `18 = Δβ_x`,
    /// `19 = Δβ_y`.
    pub const N_DOFS_UNCONDENSED: usize = Self::N_DOFS + 2;
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
            ShellReferenceCoord::new(0.5, 0.0),   // mid of edge v0-v1 (η=0)
            ShellReferenceCoord::new(0.0, 0.5),   // mid of edge v0-v2 (ξ=0)
            ShellReferenceCoord::new(0.5, 0.5),   // mid of edge v1-v2 (ξ+η=1)
            ShellReferenceCoord::new(0.25, 0.0),  // quarter of edge v0-v1
            ShellReferenceCoord::new(0.0, 0.25),  // quarter of edge v0-v2
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
    fn interior_tying_points_returns_six_strictly_interior_points() {
        let tp = Mitc3Plus.interior_tying_points();
        assert_eq!(
            tp.len(),
            Mitc3Plus::N_INTERIOR_TYING_POINTS,
            "interior_tying_points() must return N_INTERIOR_TYING_POINTS entries"
        );
        assert_eq!(
            Mitc3Plus::N_INTERIOR_TYING_POINTS,
            6,
            "MITC3+ (Lee-Lee-Bathe 2014) uses six interior transverse-shear tying points"
        );
        for (i, p) in tp.iter().enumerate() {
            let ShellReferenceCoord { xi, eta } = p.coord;
            // Strictly interior: 0 < ξ, 0 < η, ξ+η < 1.
            assert!(
                xi > 0.0 && eta > 0.0 && (xi + eta) < 1.0,
                "interior_tying_points()[{i}] = {:?} must be strictly interior \
                 (0<ξ, 0<η, ξ+η<1)",
                p.coord,
            );
            // Equivalently, the cubic bubble is strictly positive there — the
            // bubble is LIVE in the transverse-shear field at these points,
            // unlike the edge midpoints A/B/C where f_b = 0.
            let fb = Mitc3Plus.bubble_at(p.coord);
            assert!(
                fb > 1e-9,
                "bubble_at(interior_tying_points()[{i}]) = {fb} must be > 0 \
                 (bubble live in shear at interior tying points)",
            );
        }
    }

    #[test]
    fn interpolate_assumed_shear_mitc3_plus_reproduces_constant_covariant_field() {
        // Consistency / patch-test prerequisite: when the covariant shears at
        // all six interior tying points correspond to a CONSTANT covariant
        // field (γ_ξζ, γ_ηζ) = (c1, c2), the MITC3+ assumed-strain
        // interpolation must return exactly (c1, c2) at any reference coord.
        let c1 = 0.37_f64;
        let c2 = -0.21_f64;
        let samples = [ShearStrain {
            gamma_xi_zeta: c1,
            gamma_eta_zeta: c2,
        }; 6];
        assert_eq!(samples.len(), Mitc3Plus::N_INTERIOR_TYING_POINTS);
        let probes = [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0), // centroid
            ShellReferenceCoord::new(0.2, 0.3),             // off-center interior
            ShellReferenceCoord::new(0.5, 0.1),             // another off-center
        ];
        for p in probes {
            let out = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, p);
            assert!(
                (out.gamma_xi_zeta - c1).abs() < TOL,
                "at {:?}: gamma_xi_zeta = {}, expected {c1}",
                p,
                out.gamma_xi_zeta,
            );
            assert!(
                (out.gamma_eta_zeta - c2).abs() < TOL,
                "at {:?}: gamma_eta_zeta = {}, expected {c2}",
                p,
                out.gamma_eta_zeta,
            );
        }
    }

    #[test]
    fn covariant_shear_b_with_bubble_columns_track_fb_and_match_nodal_mapping() {
        let bx = Mitc3Plus::N_DOFS; // 18 = Δβ_x column
        let by = Mitc3Plus::N_DOFS + 1; // 19 = Δβ_y column

        // (a) At each interior tying point the bubble is LIVE: the bubble
        // columns equal ±f_b (non-zero), encoding the rotation bubble entering
        // the transverse-shear field by VALUE.
        for p in Mitc3Plus.interior_tying_points().iter() {
            let fb = Mitc3Plus.bubble_at(p.coord);
            assert!(fb > 1e-9, "precondition: interior point {:?} has f_b>0", p.coord);
            let b = Mitc3Plus.covariant_shear_b_with_bubble(p.coord);
            // γ_ξζ (row 0): Δβ_x col = 0, Δβ_y col = +f_b
            assert!(b[0][bx].abs() < TOL, "γ_ξζ Δβ_x col must be 0");
            assert!(
                (b[0][by] - fb).abs() < TOL,
                "γ_ξζ Δβ_y col = {}, expected +f_b = {fb}",
                b[0][by],
            );
            // γ_ηζ (row 1): Δβ_x col = −f_b, Δβ_y col = 0
            assert!(
                (b[1][bx] + fb).abs() < TOL,
                "γ_ηζ Δβ_x col = {}, expected −f_b = {}",
                b[1][bx],
                -fb,
            );
            assert!(b[1][by].abs() < TOL, "γ_ηζ Δβ_y col must be 0");
            assert!(
                b[0][by].abs() > 1e-9 && b[1][bx].abs() > 1e-9,
                "bubble must be live (non-zero) in shear at interior point {:?}",
                p.coord,
            );
        }

        // (b) At the edge midpoints A/B/C, f_b = 0, so the bubble columns are
        // exactly zero — the bubble is DEAD in shear there (the trap MITC3+
        // avoids by sampling interior points instead).
        for p in Mitc3Plus.tying_points().iter() {
            let fb = Mitc3Plus.bubble_at(p.coord);
            assert!(fb.abs() < TOL, "precondition: edge midpoint {:?} has f_b=0", p.coord);
            let b = Mitc3Plus.covariant_shear_b_with_bubble(p.coord);
            assert!(
                b[0][bx].abs() < TOL
                    && b[0][by].abs() < TOL
                    && b[1][bx].abs() < TOL
                    && b[1][by].abs() < TOL,
                "all bubble columns must vanish at edge midpoint {:?}",
                p.coord,
            );
        }

        // (c) The nodal 18 columns reproduce the standard covariant shear B
        // (the w, θ_x, θ_y mapping; in-plane and drilling DOFs untouched).
        let probe = ShellReferenceCoord::new(0.25, 0.35);
        let n = Mitc3Plus.shape_at(probe);
        let dn_ref = Mitc3Plus.shape_grad_at(probe);
        let b = Mitc3Plus.covariant_shear_b_with_bubble(probe);
        for node in 0..Mitc3Plus::N_NODES {
            let uz = Mitc3Plus::N_DOFS_PER_NODE * node + 2;
            let tx = Mitc3Plus::N_DOFS_PER_NODE * node + 3;
            let ty = Mitc3Plus::N_DOFS_PER_NODE * node + 4;
            // γ_ξζ: u_z → ∂N/∂ξ, θ_y → +N, θ_x → 0
            assert!((b[0][uz] - dn_ref[node][0]).abs() < TOL);
            assert!((b[0][ty] - n[node]).abs() < TOL);
            assert!(b[0][tx].abs() < TOL);
            // γ_ηζ: u_z → ∂N/∂η, θ_x → −N, θ_y → 0
            assert!((b[1][uz] - dn_ref[node][1]).abs() < TOL);
            assert!((b[1][tx] + n[node]).abs() < TOL);
            assert!(b[1][ty].abs() < TOL);
            // In-plane (u_x, u_y) and drilling (θ_z) columns are identically zero.
            for loc in [0_usize, 1, 5] {
                let c = Mitc3Plus::N_DOFS_PER_NODE * node + loc;
                assert!(
                    b[0][c].abs() < TOL && b[1][c].abs() < TOL,
                    "non-shear DOF column {c} must be 0",
                );
            }
        }
    }

    #[test]
    fn interpolate_assumed_shear_satisfies_c_tying_identity() {
        // At C = (½, ½), the MITC3+ assumed-strain formula guarantees:
        //
        //   γ_ξζ_out − γ_ηζ_out  =  gamma_xi_zeta_at_c − gamma_eta_zeta_at_c
        //
        // This identity pins the `c` parameter:
        //   c = (gamma_xi_zeta_at_c − gamma_eta_zeta_at_c)
        //     − (gamma_xi_zeta_at_a − gamma_eta_zeta_at_b)
        //
        // A sign flip or swapped terms in `c` would not be caught by the
        // A/B tying tests alone, because those identities evaluate at
        // η=0 and ξ=0 respectively (the `η·c` and `ξ·c` cross-terms vanish).
        let sampled = TyingShears {
            gamma_xi_zeta_at_a: 0.5,
            gamma_eta_zeta_at_b: 0.8,
            gamma_xi_zeta_at_c: 0.3,
            gamma_eta_zeta_at_c: 0.4,
        };
        let c_coord = ShellReferenceCoord::new(0.5, 0.5);
        let out_c = Mitc3Plus.interpolate_assumed_shear(sampled, c_coord);
        let lhs = out_c.gamma_xi_zeta - out_c.gamma_eta_zeta;
        let rhs = sampled.gamma_xi_zeta_at_c - sampled.gamma_eta_zeta_at_c;
        assert!(
            (lhs - rhs).abs() < TOL,
            "at C: γ_ξζ − γ_ηζ = {lhs}, expected {rhs}",
        );
    }

    #[test]
    fn interpolate_assumed_shear_reproduces_gamma_xi_zeta_at_a_and_gamma_eta_zeta_at_b() {
        let sampled = TyingShears {
            gamma_xi_zeta_at_a: 0.5,
            gamma_eta_zeta_at_b: 0.8,
            gamma_xi_zeta_at_c: 0.3,
            gamma_eta_zeta_at_c: 0.4,
        };
        // At tying point A = (½, 0): γ_ξζ output must equal gamma_xi_zeta_at_a.
        let a = ShellReferenceCoord::new(0.5, 0.0);
        let out_a = Mitc3Plus.interpolate_assumed_shear(sampled, a);
        assert!(
            (out_a.gamma_xi_zeta - sampled.gamma_xi_zeta_at_a).abs() < TOL,
            "at A: gamma_xi_zeta = {}, expected {}",
            out_a.gamma_xi_zeta,
            sampled.gamma_xi_zeta_at_a,
        );
        // At tying point B = (0, ½): γ_ηζ output must equal gamma_eta_zeta_at_b.
        let b = ShellReferenceCoord::new(0.0, 0.5);
        let out_b = Mitc3Plus.interpolate_assumed_shear(sampled, b);
        assert!(
            (out_b.gamma_eta_zeta - sampled.gamma_eta_zeta_at_b).abs() < TOL,
            "at B: gamma_eta_zeta = {}, expected {}",
            out_b.gamma_eta_zeta,
            sampled.gamma_eta_zeta_at_b,
        );
    }

    #[test]
    fn interpolate_assumed_shear_is_constant_when_paired_tying_inputs_match() {
        // c = (gamma_xi_zeta_at_c - gamma_eta_zeta_at_c)
        //   - (gamma_xi_zeta_at_a - gamma_eta_zeta_at_b)
        // Setting the paired inputs gamma_xi_zeta_at_a == gamma_xi_zeta_at_c and
        //                            gamma_eta_zeta_at_b == gamma_eta_zeta_at_c gives c = 0.
        let gx = 0.7_f64;
        let ge = -0.4_f64;
        let sampled = TyingShears {
            gamma_xi_zeta_at_a: gx,
            gamma_eta_zeta_at_b: ge,
            gamma_xi_zeta_at_c: gx,
            gamma_eta_zeta_at_c: ge,
        };
        let probes = [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.5, 0.0),
            ShellReferenceCoord::new(0.0, 0.5),
            ShellReferenceCoord::new(0.5, 0.5),
            ShellReferenceCoord::new(0.2, 0.3),
        ];
        for p in probes.iter() {
            let out = Mitc3Plus.interpolate_assumed_shear(sampled, *p);
            assert!(
                (out.gamma_xi_zeta - gx).abs() < TOL,
                "at {:?}: gamma_xi_zeta = {}, expected {}",
                p,
                out.gamma_xi_zeta,
                gx,
            );
            assert!(
                (out.gamma_eta_zeta - ge).abs() < TOL,
                "at {:?}: gamma_eta_zeta = {}, expected {}",
                p,
                out.gamma_eta_zeta,
                ge,
            );
        }
    }

    #[test]
    fn interpolate_assumed_shear_is_affine_in_reference_coords() {
        let sampled = TyingShears {
            gamma_xi_zeta_at_a: 1.0,
            gamma_eta_zeta_at_b: 1.0,
            gamma_xi_zeta_at_c: 0.5,
            gamma_eta_zeta_at_c: 0.5,
        };
        let p1 = ShellReferenceCoord::new(0.1, 0.2);
        let p2 = ShellReferenceCoord::new(0.4, 0.3);
        let pm = ShellReferenceCoord::new(0.25, 0.25); // midpoint of p1 and p2
        let r1 = Mitc3Plus.interpolate_assumed_shear(sampled, p1);
        let r2 = Mitc3Plus.interpolate_assumed_shear(sampled, p2);
        let rm = Mitc3Plus.interpolate_assumed_shear(sampled, pm);
        let mid_xi = 0.5 * (r1.gamma_xi_zeta + r2.gamma_xi_zeta);
        let mid_eta = 0.5 * (r1.gamma_eta_zeta + r2.gamma_eta_zeta);
        assert!(
            (rm.gamma_xi_zeta - mid_xi).abs() < TOL,
            "linearity: gamma_xi_zeta at midpoint = {}, expected {}",
            rm.gamma_xi_zeta,
            mid_xi,
        );
        assert!(
            (rm.gamma_eta_zeta - mid_eta).abs() < TOL,
            "linearity: gamma_eta_zeta at midpoint = {}, expected {}",
            rm.gamma_eta_zeta,
            mid_eta,
        );
    }

    #[test]
    fn bubble_grad_vanishes_at_centroid() {
        // Centroid is the unique interior maximum of f_b, so ∇f_b = 0 there.
        let g = Mitc3Plus.bubble_grad_at(ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0));
        for (k, gk) in g.iter().enumerate() {
            assert!(
                gk.abs() < TOL,
                "bubble_grad_at(centroid)[{k}] = {gk}, expected 0",
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
            for (k, rk) in row.iter().enumerate() {
                sum[k] += rk;
            }
        }
        for (k, s) in sum.iter().enumerate() {
            assert!(s.abs() < TOL, "Σ_i ∇N_i[{k}] = {s}, expected 0");
        }
    }
}
