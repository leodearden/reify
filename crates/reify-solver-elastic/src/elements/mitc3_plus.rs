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

/// Perturbation `d` used to place the near-centroid interior tying points
/// D, E, F (Lee, Lee & Bathe 2014, Table 1). The paper fixes `d = 1/10000`;
/// `d = 0` collapses D, E, F onto the centroid and kills the twist term `ĉ`
/// (over-stiff element), so the small offset is essential.
const TYING_OFFSET_D: f64 = 1.0 / 10000.0;

/// Static array of the six MITC3+ *interior* transverse-shear tying points
/// (Lee, Lee & Bathe 2014, Table 1), in canonical order A, B, C, D, E, F.
///
/// The bare three-node (DISP3) covariant transverse shear is **sampled** at
/// these six points and re-interpolated via the assumed-strain field
/// [`Mitc3Plus::interpolate_assumed_shear_mitc3_plus`] (Eq. 9). This nodal
/// assumed field — distinct from bare-MITC3's edge-midpoint Eq. 5 — is the
/// MITC3+ shear-locking cure on a flat facet.
///
/// Two distinct symmetric orbits:
/// - **A, B, C** — the `(1/6, 1/6, 2/3)` barycentric orbit (`f₄ = 1/2`).
///   These also serve as the 3-point degree-2 quadrature rule used to
///   integrate the (quadratic) assumed-shear energy exactly.
/// - **D, E, F** — the near-centroid triple at `(1/3 ± d, …)` with
///   `d = TYING_OFFSET_D`, used only to form the twist coefficient `ĉ`.
///
/// **The cubic bubble does not enter the shear field on a flat facet.** Although
/// `f₄ > 0` at every interior point, the nodal↔bubble shear coupling is
/// identically zero on a flat, constant-Jacobian facet (`K_NB^shear ≡ 0`, the
/// divergence-theorem result of task 3349, re-derived for the flat-facet shear
/// field in the esc-3392 corrected resolution; DD#2 retracted). The bubble
/// enriches **bending** only and becomes live in transverse shear solely on the
/// curved director substrate (task 4065).
const INTERIOR_TYING_POINTS: &[TyingPoint] = &[
    // A = (1/6, 2/3)
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 6.0, 2.0 / 3.0),
    },
    // B = (2/3, 1/6)
    TyingPoint {
        coord: ShellReferenceCoord::new(2.0 / 3.0, 1.0 / 6.0),
    },
    // C = (1/6, 1/6)
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 6.0, 1.0 / 6.0),
    },
    // D = (1/3 + d, 1/3 − 2d)
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 3.0 + TYING_OFFSET_D, 1.0 / 3.0 - 2.0 * TYING_OFFSET_D),
    },
    // E = (1/3 − 2d, 1/3 + d)
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 3.0 - 2.0 * TYING_OFFSET_D, 1.0 / 3.0 + TYING_OFFSET_D),
    },
    // F = (1/3 + d, 1/3 + d)
    TyingPoint {
        coord: ShellReferenceCoord::new(1.0 / 3.0 + TYING_OFFSET_D, 1.0 / 3.0 + TYING_OFFSET_D),
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
    /// Returns `[∂f₄/∂ξ, ∂f₄/∂η]` derived from `f₄ = 27·ξ·η·(1−ξ−η)` by
    /// the product rule:
    ///
    /// ```text
    /// ∂f₄/∂ξ = 27·η·(1 − 2ξ − η)
    /// ∂f₄/∂η = 27·ξ·(1 − ξ − 2η)
    /// ```
    ///
    /// Both components vanish at the centroid `(1/3, 1/3)` — the unique
    /// interior maximum of the bubble function (where `f₄ = 1`).
    pub fn bubble_grad_at(&self, coord: ShellReferenceCoord) -> [f64; 2] {
        let ShellReferenceCoord { xi, eta } = coord;
        [
            27.0 * eta * (1.0 - 2.0 * xi - eta),
            27.0 * xi * (1.0 - xi - 2.0 * eta),
        ]
    }

    /// Cubic bubble enrichment at `coord`.
    ///
    /// Returns `f₄(ξ, η) = 27 · ξ · η · (1 − ξ − η)`.
    ///
    /// This is the "+" in MITC3+ (Lee, Lee & Bathe 2014, Eq. 7): the cubic
    /// bubble enriches the rotation field. The factor `27` normalises the
    /// bubble so that `f₄ = 1` at the centroid `(1/3, 1/3)`. It vanishes on
    /// every edge of the reference triangle — on the edge `η=0` the `η` factor
    /// is zero; on `ξ=0` the `ξ` factor is zero; on `ξ+η=1` the `(1−ξ−η)`
    /// factor is zero — so the enrichment does not introduce additional DOFs
    /// at nodes or edges.
    pub fn bubble_at(&self, coord: ShellReferenceCoord) -> f64 {
        let ShellReferenceCoord { xi, eta } = coord;
        27.0 * xi * eta * (1.0 - xi - eta)
    }

    /// Modified corner rotation shape functions at `coord` (Lee, Lee & Bathe
    /// 2014, Eq. 7): `f_i = h_i − (1/3)·f₄` for `i ∈ {0,1,2}`, where
    /// `h_i = shape_at` are the plain barycentric P1 functions and
    /// `f₄ = bubble_at` is the cubic bubble.
    ///
    /// These are the functions that interpolate the **rotation** field in the
    /// enriched MITC3+ displacement (Eq. 8); the **translation** field uses the
    /// plain `h_i`. The `−(1/3)f₄` correction preserves partition of unity with
    /// the bubble: `Σ_i f_i + f₄ = Σ_i h_i − f₄ + f₄ = 1`, so a constant
    /// rotation state (all nodal rotations equal, bubble DOF equal too) is
    /// represented exactly.
    pub fn rotation_shape_at(&self, coord: ShellReferenceCoord) -> [f64; 3] {
        let h = self.shape_at(coord);
        let third_f4 = self.bubble_at(coord) / 3.0;
        [h[0] - third_f4, h[1] - third_f4, h[2] - third_f4]
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
    /// edge midpoints A/B/C of the bare-MITC3 baseline): the bare DISP3 covariant
    /// shear is sampled at these six interior points and re-interpolated via
    /// Eq. 9, giving a softer nodal assumed field than bare MITC3's Eq. 5 — the
    /// flat-facet shear-locking cure. (The cubic bubble is inert in shear on a
    /// flat facet; see [`INTERIOR_TYING_POINTS`].)
    pub fn interior_tying_points(&self) -> &'static [TyingPoint] {
        INTERIOR_TYING_POINTS
    }

    /// MITC3+ assumed covariant transverse-shear field at `coord = (r, s)`,
    /// re-interpolated from the covariant shears sampled at the six interior
    /// tying points (Lee, Lee & Bathe 2014, Eq. 9).
    ///
    /// # Formulation
    ///
    /// With `sampled` indexed in the canonical order `[A, B, C, D, E, F]` of
    /// [`Mitc3Plus::interior_tying_points`], and writing `e_rt = γ_ξζ`,
    /// `e_st = γ_ηζ`:
    ///
    /// ```text
    /// ĉ      = e_rt^F − e_rt^D − e_st^F + e_st^E
    /// ê_rt   = (2/3)(e_rt^B − ½·e_st^B) + (1/3)(e_rt^C + e_st^C) + (1/3)·ĉ·(3s − 1)
    /// ê_st   = (2/3)(e_st^A − ½·e_rt^A) + (1/3)(e_rt^C + e_st^C) + (1/3)·ĉ·(1 − 3r)
    /// ```
    ///
    /// This is a tying-**collocation** re-interpolation: it samples the
    /// covariant shear at the six discrete interior points and re-interpolates,
    /// rather than forming a Galerkin / L2 projection. The samples are taken from
    /// the bare three-node (DISP3) covariant shear
    /// ([`Mitc3Plus::covariant_shear_b_nodal`]); on a flat facet the cubic bubble
    /// does not enter (it is inert in shear there — see [`INTERIOR_TYING_POINTS`]).
    ///
    /// Properties:
    /// - **Three-parameter field, linear in `(r, s)`** — two constants plus the
    ///   twist `ĉ`; `ê_rt` varies in `s` only (the `3s−1` term), `ê_st` in `r`
    ///   only (the `1−3r` term). It deliberately CANNOT reproduce an arbitrary
    ///   linear covariant field — that under-representation is the locking cure.
    /// - **Constant-state consistency.** For a constant covariant field
    ///   `(c1, c2)` sampled at all six points, `ĉ = 0` and
    ///   `ê_rt = (2/3)(c1 − c2/2) + (1/3)(c1 + c2) = c1`, `ê_st = c2`, so
    ///   constant transverse-shear states are reproduced exactly.
    ///
    /// `sampled[k]` is the covariant `(γ_ξζ, γ_ηζ)` sampled at
    /// `interior_tying_points()[k]`.
    pub fn interpolate_assumed_shear_mitc3_plus(
        &self,
        sampled: &[ShearStrain],
        coord: ShellReferenceCoord,
    ) -> ShearStrain {
        debug_assert_eq!(
            sampled.len(),
            Self::N_INTERIOR_TYING_POINTS,
            "interpolate_assumed_shear_mitc3_plus expects one sample per interior tying point"
        );
        let ShellReferenceCoord { xi: r, eta: s } = coord;
        let (a, b, c, d, e, f) = (
            sampled[0], sampled[1], sampled[2], sampled[3], sampled[4], sampled[5],
        );
        // Twist coefficient ĉ from the near-centroid triple D, E, F.
        let chat = f.gamma_xi_zeta - d.gamma_xi_zeta - f.gamma_eta_zeta + e.gamma_eta_zeta;
        let common = (1.0 / 3.0) * (c.gamma_xi_zeta + c.gamma_eta_zeta);
        ShearStrain {
            gamma_xi_zeta: (2.0 / 3.0) * (b.gamma_xi_zeta - 0.5 * b.gamma_eta_zeta)
                + common
                + (1.0 / 3.0) * chat * (3.0 * s - 1.0),
            gamma_eta_zeta: (2.0 / 3.0) * (a.gamma_eta_zeta - 0.5 * a.gamma_xi_zeta)
                + common
                + (1.0 / 3.0) * chat * (1.0 - 3.0 * r),
        }
    }

    /// **Covariant** transverse-shear strain B-matrix of the bare three-node
    /// (DISP3) displacement field at `coord`, with shape `[2][18]`: rows are the
    /// covariant components `(γ_ξζ, γ_ηζ)`; columns are the 18 nodal DOFs.
    ///
    /// # Construction (covariant, geometry-free)
    ///
    /// Both the transverse displacement and the section rotation use the plain
    /// three-node shape functions `h_i` (`shape_at`):
    ///
    /// ```text
    /// γ_ξζ = Σ_i (∂h_i/∂ξ · u_z_i + h_i · θ_y_i)
    /// γ_ηζ = Σ_i (∂h_i/∂η · u_z_i − h_i · θ_x_i)
    /// ```
    ///
    /// where `∂h_i/∂ξ` are the constant reference-coordinate shape gradients
    /// (`shape_grad_at`). This is identical to the covariant shear B built by
    /// [`crate::shell_kinematics`] for the bare-MITC3 edge-midpoint scheme;
    /// exposing it per-coordinate lets the MITC3+ assembly sample it at the six
    /// interior tying points.
    ///
    /// # Why the bubble does NOT appear here (flat facet)
    ///
    /// On a flat, constant-Jacobian facet the cubic rotation bubble is **inert**
    /// in transverse shear: its nodal↔bubble shear coupling `K_NB^shear ≡ 0`
    /// (the divergence-theorem result first proved for the bending bubble in task
    /// 3349, re-derived for the flat-facet shear field in the esc-3392 corrected
    /// resolution). The genuine MITC3+ shear-locking cure on a flat facet lives
    /// **entirely in the nodal assumed field**: these corner covariant shears are
    /// sampled at the six interior tying points A–F and re-interpolated via
    /// [`Mitc3Plus::interpolate_assumed_shear_mitc3_plus`] (Eq. 9), which is a
    /// softer field than bare MITC3's edge-midpoint Eq. 5. The cubic bubble
    /// enriches **bending** only and becomes live in transverse shear only on the
    /// curved director substrate (task 4065) — hence this B-matrix carries no
    /// bubble columns.
    pub fn covariant_shear_b_nodal(
        &self,
        coord: ShellReferenceCoord,
    ) -> [[f64; Self::N_DOFS]; 2] {
        let dn_ref = self.shape_grad_at(coord);
        let h = self.shape_at(coord); // plain corner functions (DISP3)
        let mut b = [[0.0_f64; Self::N_DOFS]; 2];
        for node in 0..Self::N_NODES {
            let dof_uz = Self::N_DOFS_PER_NODE * node + 2;
            let dof_tx = Self::N_DOFS_PER_NODE * node + 3;
            let dof_ty = Self::N_DOFS_PER_NODE * node + 4;
            // γ_ξζ: ∂h/∂ξ · u_z + h_i · θ_y
            b[0][dof_uz] += dn_ref[node][0];
            b[0][dof_ty] += h[node];
            // γ_ηζ: ∂h/∂η · u_z − h_i · θ_x
            b[1][dof_uz] += dn_ref[node][1];
            b[1][dof_tx] -= h[node];
        }
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
        // Closed form: ∂f₄/∂ξ = 27·η(1 − 2ξ − η), ∂f₄/∂η = 27·ξ(1 − ξ − 2η)
        let probes = [
            ShellReferenceCoord::new(0.1, 0.2),
            ShellReferenceCoord::new(0.4, 0.3),
        ];
        for p in probes.iter() {
            let ShellReferenceCoord { xi, eta } = *p;
            let expected = [
                27.0 * eta * (1.0 - 2.0 * xi - eta),
                27.0 * xi * (1.0 - xi - 2.0 * eta),
            ];
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
    fn interpolate_assumed_shear_mitc3_plus_has_three_parameter_linear_structure() {
        // CORRECTED (esc-3392, point 4): the previous test
        // `..._reproduces_linear_covariant_field` encoded a FALSE premise — the
        // genuine Eq. 9 field is a 3-PARAMETER field (two constants + one twist)
        // and CANNOT reproduce an arbitrary linear covariant field. That
        // under-representation IS the locking cure. Pin the genuine structure:
        //   ê_rt = const + (1/3)·ĉ·(3s − 1)   → depends on s only, linear in s
        //   ê_st = const + (1/3)·ĉ·(1 − 3r)   → depends on r only, linear in r
        // Use generic (non-constant) samples so the twist ĉ ≠ 0.
        let raw = [
            (0.1, 0.9),
            (0.3, 0.2),
            (0.5, -0.4),
            (0.7, 0.1),
            (-0.2, 0.6),
            (0.8, 0.3),
        ];
        let mut samples = [ShearStrain {
            gamma_xi_zeta: 0.0,
            gamma_eta_zeta: 0.0,
        }; 6];
        for (k, &(rt, st)) in raw.iter().enumerate() {
            samples[k] = ShearStrain {
                gamma_xi_zeta: rt,
                gamma_eta_zeta: st,
            };
        }
        let eval = |r: f64, s: f64| {
            Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, ShellReferenceCoord::new(r, s))
        };

        // (i) ê_rt is independent of r: same s, different r → identical ê_rt.
        let a = eval(0.1, 0.4);
        let b = eval(0.6, 0.4);
        assert!(
            (a.gamma_xi_zeta - b.gamma_xi_zeta).abs() < TOL,
            "ê_rt must be independent of r ({} vs {})",
            a.gamma_xi_zeta,
            b.gamma_xi_zeta,
        );
        // (ii) ê_st is independent of s: same r, different s → identical ê_st.
        let c = eval(0.3, 0.1);
        let d = eval(0.3, 0.5);
        assert!(
            (c.gamma_eta_zeta - d.gamma_eta_zeta).abs() < TOL,
            "ê_st must be independent of s ({} vs {})",
            c.gamma_eta_zeta,
            d.gamma_eta_zeta,
        );
        // (iii) ê_rt is LINEAR in s (midpoint equals the average of the ends).
        let s0 = eval(0.2, 0.0);
        let s1 = eval(0.2, 0.25);
        let s2 = eval(0.2, 0.5);
        let mid = 0.5 * (s0.gamma_xi_zeta + s2.gamma_xi_zeta);
        assert!(
            (s1.gamma_xi_zeta - mid).abs() < TOL,
            "ê_rt must be linear in s",
        );
        // (iv) and genuinely NON-constant in s (the twist term is active).
        assert!(
            (s0.gamma_xi_zeta - s2.gamma_xi_zeta).abs() > 1e-6,
            "ê_rt must genuinely vary in s (3-parameter field, ĉ ≠ 0)",
        );

        // (v) The false premise is explicitly refuted: the field does NOT
        // reproduce the arbitrary linear field e_rt = r sampled at A-F.
        let mut lin = [ShearStrain {
            gamma_xi_zeta: 0.0,
            gamma_eta_zeta: 0.0,
        }; 6];
        for (k, tp) in Mitc3Plus.interior_tying_points().iter().enumerate() {
            lin[k] = ShearStrain {
                gamma_xi_zeta: tp.coord.xi,
                gamma_eta_zeta: tp.coord.eta,
            };
        }
        // e_rt = r is linear in r; Eq. 9's ê_rt depends on s only, so at a probe
        // with r ≠ ê_rt it cannot match r. (It collapses to the constant 1/2.)
        let probe = ShellReferenceCoord::new(0.7, 0.1);
        let out = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&lin, probe);
        assert!(
            (out.gamma_xi_zeta - probe.xi).abs() > 1e-3,
            "Eq. 9 must NOT reproduce an arbitrary linear field (got {} for r={})",
            out.gamma_xi_zeta,
            probe.xi,
        );
    }

    #[test]
    fn covariant_shear_b_nodal_is_the_plain_corner_covariant_shear() {
        // CORRECTED (esc-3392, DD#2 retracted): on a flat facet the shear-locking
        // cure lives in the NODAL assumed field; the cubic bubble is inert in
        // shear. So the per-point covariant shear B uses the PLAIN corner
        // functions h_i (not the modified f_i = h_i − f₄/3) and carries NO bubble
        // columns: it is exactly the bare DISP3 covariant transverse shear.
        let probe = ShellReferenceCoord::new(0.25, 0.35);
        let h = Mitc3Plus.shape_at(probe);
        let dn_ref = Mitc3Plus.shape_grad_at(probe);
        let b = Mitc3Plus.covariant_shear_b_nodal(probe);
        for node in 0..Mitc3Plus::N_NODES {
            let uz = Mitc3Plus::N_DOFS_PER_NODE * node + 2;
            let tx = Mitc3Plus::N_DOFS_PER_NODE * node + 3;
            let ty = Mitc3Plus::N_DOFS_PER_NODE * node + 4;
            // γ_ξζ: u_z → ∂h/∂ξ, θ_y → +h_i, θ_x → 0
            assert!((b[0][uz] - dn_ref[node][0]).abs() < TOL);
            assert!((b[0][ty] - h[node]).abs() < TOL);
            assert!(b[0][tx].abs() < TOL);
            // γ_ηζ: u_z → ∂h/∂η, θ_x → −h_i, θ_y → 0
            assert!((b[1][uz] - dn_ref[node][1]).abs() < TOL);
            assert!((b[1][tx] + h[node]).abs() < TOL);
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
    fn covariant_shear_b_nodal_plus_eq9_reproduces_constant_shear_state() {
        // Patch-safety prerequisite (the property the value-coupled bubble broke):
        // a uniform θ_y = α with w = 0 is a CONSTANT covariant shear state
        // (e_rt = α, e_st = 0). Sampling the corner covariant shear at the six
        // interior tying points and re-interpolating via Eq. 9 must return the
        // SAME constant (α, 0) at any probe.
        let alpha = 0.7_f64;
        let tps = Mitc3Plus.interior_tying_points();
        let mut samples = [ShearStrain { gamma_xi_zeta: 0.0, gamma_eta_zeta: 0.0 }; 6];
        for (k, tp) in tps.iter().enumerate() {
            let b = Mitc3Plus.covariant_shear_b_nodal(tp.coord);
            // Contract the corner B with the uniform-θ_y DOF vector (θ_y = α at
            // every node, all other DOFs zero).
            let mut e_rt = 0.0;
            let mut e_st = 0.0;
            for node in 0..Mitc3Plus::N_NODES {
                let ty = Mitc3Plus::N_DOFS_PER_NODE * node + 4;
                e_rt += b[0][ty] * alpha;
                e_st += b[1][ty] * alpha;
            }
            samples[k] = ShearStrain { gamma_xi_zeta: e_rt, gamma_eta_zeta: e_st };
        }
        for p in [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.2, 0.3),
        ] {
            let out = Mitc3Plus.interpolate_assumed_shear_mitc3_plus(&samples, p);
            assert!(
                (out.gamma_xi_zeta - alpha).abs() < TOL,
                "constant e_rt at {:?}: {} expected {alpha}",
                p,
                out.gamma_xi_zeta,
            );
            assert!(
                out.gamma_eta_zeta.abs() < TOL,
                "constant e_st at {:?}: {} expected 0",
                p,
                out.gamma_eta_zeta,
            );
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
    fn bubble_equals_one_at_centroid() {
        // Centroid (1/3, 1/3): f₄ = 27·(1/3)·(1/3)·(1 − 2/3) = 27/27 = 1.
        let b = Mitc3Plus.bubble_at(ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0));
        let expected = 1.0;
        assert!(
            (b - expected).abs() < TOL,
            "bubble_at(centroid) = {b}, expected {expected}",
        );
    }

    #[test]
    fn rotation_shape_at_partition_of_unity_with_bubble() {
        // The modified corner functions f_i = h_i − f₄/3 satisfy
        // Σ_i f_i + f₄ = 1 (partition of unity with the bubble), so a constant
        // rotation (all nodal rotations equal, bubble DOF equal) is exact.
        let probes = [
            ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0),
            ShellReferenceCoord::new(0.2, 0.3),
            ShellReferenceCoord::new(1.0 / 6.0, 2.0 / 3.0),
        ];
        for p in probes {
            let f = Mitc3Plus.rotation_shape_at(p);
            let f4 = Mitc3Plus.bubble_at(p);
            let sum: f64 = f.iter().sum::<f64>() + f4;
            assert!(
                (sum - 1.0).abs() < TOL,
                "Σ f_i + f₄ at {:?} = {sum}, expected 1.0",
                p,
            );
            // On an edge f₄ = 0, so f_i must collapse to the plain h_i there.
            let h = Mitc3Plus.shape_at(p);
            for i in 0..3 {
                assert!((f[i] - (h[i] - f4 / 3.0)).abs() < TOL);
            }
        }
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
