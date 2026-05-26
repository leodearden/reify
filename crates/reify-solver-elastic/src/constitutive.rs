//! Constitutive laws for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the isotropic linear-elastic 6×6 D-matrix used by element-stiffness
//! assembly. The Voigt component order is `[εxx, εyy, εzz, γxy, γyz, γxz]`
//! with **engineering shear strain** (`γ = 2ε`); see [`IsotropicElastic`] for
//! the convention details.
//!
//! Foundation α adds [`ConstitutiveLaw`], [`OrthotropicMaterial`],
//! [`TransverseIsotropicMaterial`], and [`rotate_voigt`].
//! See PRD `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C1/C2.

// ─────────────────────────────────────────────────────────────────────────────
// ConstitutiveLaw trait
// ─────────────────────────────────────────────────────────────────────────────

/// Common interface for linear-elastic constitutive laws.
///
/// # Contract (PRD §C1, `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md`)
///
/// `d_matrix_local` returns the 6×6 elasticity matrix `D` in the conformer's
/// **local material frame** using engineering-strain Voigt order
/// `[εxx, εyy, εzz, γxy, γyz, γxz]` with shear-block diagonal = G (not 2G).
/// Frame rotation (local → global) is handled separately by [`rotate_voigt`].
///
/// The returned matrix must be:
/// - **Symmetric**: `D[i][j] == D[j][i]` for all `i, j`.
/// - **Positive-definite**: all eigenvalues strictly positive (every valid
///   physical material satisfies this).
/// - **Entry-wise finite**: no `NaN` or `±inf`.
///
/// Validation of the PD invariants is performed via `debug_assert!` inside
/// each conformer's `d_matrix_local` implementation, mirroring the existing
/// [`IsotropicElastic::debug_assert_valid`] pattern.
pub trait ConstitutiveLaw {
    /// Return the 6×6 elasticity matrix in the conformer's local frame.
    ///
    /// See type-level documentation for the Voigt convention.
    fn d_matrix_local(&self) -> [[f64; 6]; 6];
}

/// Isotropic linear-elastic constitutive law parameterised by Young's
/// modulus `E` and Poisson's ratio `ν`.
///
/// # Voigt convention
///
/// The 6×6 matrix returned by [`IsotropicElastic::d_matrix`] maps a
/// **Voigt strain vector with engineering shear** to a Voigt stress vector,
///
/// ```text
/// ε = [ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]ᵀ          (γ_ij = 2 ε_ij)
/// σ = [σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]ᵀ
/// σ = D · ε
/// ```
///
/// Because shear strain enters as the engineering quantity `γ = 2ε`, the
/// shear-block diagonal of `D` is the shear modulus `μ = G = E / (2(1+ν))`
/// directly — **without** the additional factor of 2 that appears when
/// using tensorial shear strain. Consumers that build the
/// strain-displacement matrix `B` must match this convention by placing
/// `(∂N/∂y, ∂N/∂x, 0)` (no halving) in the row corresponding to `γ_xy`.
///
/// # Lamé form
///
/// Internally the D matrix is written in Lamé form. With
/// `factor = E / ((1+ν)(1−2ν))`,
///
/// ```text
/// λ      = factor · ν                  (Lamé first parameter)
/// 2μ     = factor · (1 − 2ν)           (twice the shear modulus)
/// μ      = factor · (1 − 2ν) / 2       (shear modulus G)
/// ```
///
/// then
///
/// ```text
/// D = [ λ+2μ   λ     λ     0   0   0
///       λ      λ+2μ  λ     0   0   0
///       λ      λ     λ+2μ  0   0   0
///       0      0     0     μ   0   0
///       0      0     0     0   μ   0
///       0      0     0     0   0   μ ]
/// ```
///
/// # Preconditions
///
/// `ν ∈ (-1, 0.5)` (open on both ends) — the mathematical range over which
/// the isotropic linear-elastic D matrix is positive-definite:
/// - `G = E / (2(1+ν)) > 0` requires `ν > -1` (auxetic lower limit).
/// - `K = E / (3(1-2ν)) > 0` requires `ν < 0.5` (incompressible upper limit).
///
/// The stdlib `ElasticMaterial` trait at
/// `crates/reify-compiler/stdlib/materials_fea.ri:94-103` keeps the stricter
/// policy bound `[0, 0.5)` to exclude auxetic materials from the user-facing
/// trait surface. This Rust struct accepts the full mathematical PD range;
/// compiler-side enforcement via `ElasticMaterial` keeps user-visible
/// constructions in the stricter range.
///
/// `youngs_modulus` must be positive (any consistent units — the D matrix is
/// linear in `E`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsotropicElastic {
    /// Young's modulus `E` (any consistent unit; the D matrix is linear in `E`).
    pub youngs_modulus: f64,
    /// Poisson's ratio `ν`. Must satisfy `-1 < ν < 0.5` (mathematical PD range).
    pub poisson_ratio: f64,
}

impl IsotropicElastic {
    /// Assert the contract `E > 0` and `-1 < ν < 0.5` in debug builds.
    ///
    /// This is the single source of truth for Poisson-ratio validation in this
    /// crate. Both [`Self::d_matrix`] and [`crate::shell_assembly::plane_stress_d`]
    /// delegate here rather than carrying inline checks. A future hardening pass
    /// (PRD task #21 diagnostics) may promote these to a fallible
    /// `IsotropicElastic::new(e, nu) -> Result<Self, ConstitutiveError>`.
    #[inline]
    pub(crate) fn debug_assert_valid(&self) {
        debug_assert!(
            self.youngs_modulus > 0.0,
            "IsotropicElastic.youngs_modulus must be positive, got {e}",
            e = self.youngs_modulus,
        );
        debug_assert!(
            self.poisson_ratio > -1.0 && self.poisson_ratio < 0.5,
            "IsotropicElastic.poisson_ratio must satisfy -1 < ν < 0.5 \
             (positive-definite isotropic D requires G = E/(2(1+ν)) > 0 and \
             K = E/(3(1-2ν)) > 0; ν ≤ -1 is the auxetic limit, ν ≥ 0.5 is the \
             incompressible limit), got {nu}",
            nu = self.poisson_ratio,
        );
    }

    /// Return the 6×6 elasticity matrix `D` in engineering-strain Voigt form.
    ///
    /// See the type-level documentation for the Voigt component order
    /// (`[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]`) and the rationale for the
    /// shear-block diagonal being `μ = G` (not `2G`).
    ///
    /// # Contract
    ///
    /// `youngs_modulus > 0` and `-1 < poisson_ratio < 0.5` (mathematical PD
    /// range). Validation is delegated to [`Self::debug_assert_valid`] — the
    /// single source of truth for this crate. The stdlib `ElasticMaterial`
    /// constructor enforces the stricter `[0, 0.5)` policy bound upstream
    /// (`crates/reify-compiler/stdlib/materials_fea.ri:94-103`), but this
    /// struct is publicly constructible, so we re-check the contract here in
    /// debug builds. A release-mode caller bypassing this gate is responsible
    /// for the resulting non-finite / garbage output.
    #[allow(clippy::needless_range_loop)]
    pub fn d_matrix(&self) -> [[f64; 6]; 6] {
        self.debug_assert_valid();
        let e = self.youngs_modulus;
        let nu = self.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let mu = 0.5 * two_mu;
        let lambda_plus_two_mu = lambda + two_mu;

        let mut d = [[0.0_f64; 6]; 6];
        // Normal-stress block (rows/cols 0..3).
        for i in 0..3 {
            for j in 0..3 {
                d[i][j] = if i == j { lambda_plus_two_mu } else { lambda };
            }
        }
        // Shear-stress block (rows/cols 3..6) — diagonal μ, off-diagonal 0.
        for k in 3..6 {
            d[k][k] = mu;
        }
        // Off-diagonal blocks are zero (initialised that way).
        d
    }
}

impl ConstitutiveLaw for IsotropicElastic {
    /// Delegate to [`IsotropicElastic::d_matrix`] — one-line forward so the
    /// trait surface reuses the established v0.3 isotropic D builder exactly.
    #[inline]
    fn d_matrix_local(&self) -> [[f64; 6]; 6] {
        self.d_matrix()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OrthotropicMaterial
// ─────────────────────────────────────────────────────────────────────────────

/// Nine-constant orthotropic linear-elastic law in engineering-strain Voigt order.
///
/// The Voigt order matches [`IsotropicElastic`]: `[εxx, εyy, εzz, γxy, γyz, γxz]`
/// with shear-block diagonal = G (not 2G).
///
/// # Symmetric Poisson convention
///
/// Only the *upper-triangle* Poisson ratios `ν12`, `ν13`, `ν23` are stored
/// (`νij` = strain in direction `j` per unit stress in direction `i`).
/// The *reciprocal* ratios used internally are derived from thermodynamic
/// symmetry: `νji = νij · Ej / Ei`.
///
/// # PRD reference
///
/// `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §C1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrthotropicMaterial {
    /// Young's modulus in the 1-direction (fibre/principal axis 1).
    pub e1: f64,
    /// Young's modulus in the 2-direction.
    pub e2: f64,
    /// Young's modulus in the 3-direction (through-thickness / build axis).
    pub e3: f64,
    /// Shear modulus in the 1-2 plane.  Corresponds to Voigt row/col 3 (γxy).
    pub g12: f64,
    /// Shear modulus in the 1-3 plane.  Corresponds to Voigt row/col 5 (γxz).
    pub g13: f64,
    /// Shear modulus in the 2-3 plane.  Corresponds to Voigt row/col 4 (γyz).
    pub g23: f64,
    /// Poisson's ratio ν12 (contraction in 2 per extension in 1).
    pub nu12: f64,
    /// Poisson's ratio ν13 (contraction in 3 per extension in 1).
    pub nu13: f64,
    /// Poisson's ratio ν23 (contraction in 3 per extension in 2).
    pub nu23: f64,
}

// ─── Private helper ──────────────────────────────────────────────────────────

/// Compute the orthotropic PD determinant Δ from the six input constants.
///
/// ```text
/// Δ = 1 − ν12·ν21 − ν23·ν32 − ν31·ν13 − 2·ν21·ν32·ν13   (PRD §C1)
/// ```
/// with the reciprocal ratios `νji = νij·Ej/Ei`.
///
/// This is the **single source of truth** for the Δ formula.  Both
/// [`OrthotropicMaterial::debug_assert_valid`] and
/// [`TransverseIsotropicMaterial::debug_assert_valid`] call it so the
/// formula cannot drift between the two validation paths.
#[inline]
fn orthotropic_delta(e1: f64, e2: f64, e3: f64, nu12: f64, nu13: f64, nu23: f64) -> f64 {
    let nu21 = nu12 * e2 / e1;
    let nu31 = nu13 * e3 / e1;
    let nu32 = nu23 * e3 / e2;
    1.0 - nu12 * nu21 - nu23 * nu32 - nu31 * nu13 - 2.0 * nu21 * nu32 * nu13
}

impl OrthotropicMaterial {
    /// Assert the PD contract in debug builds.
    ///
    /// Checks (necessary AND sufficient for SPD; Ting 1996 §5.3):
    /// 1. All six moduli `e1, e2, e3, g12, g13, g23 > 0`.
    /// 2. Pair-determinants `1 − ν12·ν21 > 0`, `1 − ν23·ν32 > 0`,
    ///    `1 − ν13·ν31 > 0` (each equivalent to `|νij| < sqrt(Ei/Ej)`).
    ///    A material can satisfy Δ > 0 while a pair-determinant is negative
    ///    (when moduli are highly asymmetric), making a diagonal D entry
    ///    negative and the matrix non-PD.
    /// 3. Determinant `Δ = 1 − ν12·ν21 − ν23·ν32 − ν31·ν13 − 2·ν21·ν32·ν13 > 0`
    ///    (PRD Contract C1) where `νji = νij·Ej/Ei`.
    ///
    /// Each `debug_assert!` message starts with `"OrthotropicMaterial"` so that
    /// `#[should_panic(expected = "OrthotropicMaterial")]` tests can pin exactly
    /// which conformer rejected.
    #[inline]
    pub(crate) fn debug_assert_valid(&self) {
        debug_assert!(
            self.e1 > 0.0,
            "OrthotropicMaterial.e1 must be positive, got {e}",
            e = self.e1,
        );
        debug_assert!(
            self.e2 > 0.0,
            "OrthotropicMaterial.e2 must be positive, got {e}",
            e = self.e2,
        );
        debug_assert!(
            self.e3 > 0.0,
            "OrthotropicMaterial.e3 must be positive, got {e}",
            e = self.e3,
        );
        debug_assert!(
            self.g12 > 0.0,
            "OrthotropicMaterial.g12 must be positive, got {g}",
            g = self.g12,
        );
        debug_assert!(
            self.g13 > 0.0,
            "OrthotropicMaterial.g13 must be positive, got {g}",
            g = self.g13,
        );
        debug_assert!(
            self.g23 > 0.0,
            "OrthotropicMaterial.g23 must be positive, got {g}",
            g = self.g23,
        );
        // Reciprocal Poisson ratios from thermodynamic symmetry (νji = νij·Ej/Ei).
        let nu21 = self.nu12 * self.e2 / self.e1;
        let nu31 = self.nu13 * self.e3 / self.e1;
        let nu32 = self.nu23 * self.e3 / self.e2;
        // ── Pair-determinant checks (check 2) ────────────────────────────────
        // These are necessary conditions for SPD that Δ alone cannot catch.
        // E.g. when |ν12| > sqrt(E1/E2), D33 = (1−ν12·ν21)·E3/Δ is negative
        // even if Δ > 0.
        debug_assert!(
            1.0 - self.nu12 * nu21 > 0.0,
            "OrthotropicMaterial: pair-determinant 1 − ν12·ν21 must be > 0 \
             (requires |ν12| < sqrt(E1/E2)), got 1 − ν12·ν21 = {val}; \
             ν12 = {nu12}, ν21 = {nu21}",
            val = 1.0 - self.nu12 * nu21,
            nu12 = self.nu12,
            nu21 = nu21,
        );
        debug_assert!(
            1.0 - self.nu23 * nu32 > 0.0,
            "OrthotropicMaterial: pair-determinant 1 − ν23·ν32 must be > 0 \
             (requires |ν23| < sqrt(E2/E3)), got 1 − ν23·ν32 = {val}; \
             ν23 = {nu23}, ν32 = {nu32}",
            val = 1.0 - self.nu23 * nu32,
            nu23 = self.nu23,
            nu32 = nu32,
        );
        debug_assert!(
            1.0 - self.nu13 * nu31 > 0.0,
            "OrthotropicMaterial: pair-determinant 1 − ν13·ν31 must be > 0 \
             (requires |ν13| < sqrt(E1/E3)), got 1 − ν13·ν31 = {val}; \
             ν13 = {nu13}, ν31 = {nu31}",
            val = 1.0 - self.nu13 * nu31,
            nu13 = self.nu13,
            nu31 = nu31,
        );
        // ── Full PD determinant (check 3) — via single-source helper ─────────
        let delta = orthotropic_delta(self.e1, self.e2, self.e3, self.nu12, self.nu13, self.nu23);
        debug_assert!(
            delta > 0.0,
            "OrthotropicMaterial: positive-definite constraint Δ = \
             1 − ν12·ν21 − ν23·ν32 − ν31·ν13 − 2·ν21·ν32·ν13 must be > 0, \
             got Δ = {delta} (PRD §C1)",
        );
    }

    /// Return the 6×6 elasticity matrix in engineering-strain Voigt order.
    ///
    /// Voigt order: `[εxx, εyy, εzz, γxy, γyz, γxz]` (engineering shear).
    /// Shear block: D[3][3]=g12 (γxy), D[4][4]=g23 (γyz), D[5][5]=g13 (γxz).
    ///
    /// Closed-form expressions (PRD §C1):
    /// ```text
    /// Δ  = 1 − ν12·ν21 − ν23·ν32 − ν31·ν13 − 2·ν21·ν32·ν13
    /// D11 = (1 − ν23·ν32)·E1/Δ
    /// D22 = (1 − ν13·ν31)·E2/Δ
    /// D33 = (1 − ν12·ν21)·E3/Δ
    /// D12 = (ν21 + ν23·ν31)·E1/Δ
    /// D13 = (ν31 + ν21·ν32)·E1/Δ
    /// D23 = (ν32 + ν12·ν31)·E2/Δ
    /// ```
    pub fn d_matrix_local(&self) -> [[f64; 6]; 6] {
        self.debug_assert_valid();

        let nu21 = self.nu12 * self.e2 / self.e1;
        let nu31 = self.nu13 * self.e3 / self.e1;
        let nu32 = self.nu23 * self.e3 / self.e2;
        let delta = 1.0
            - self.nu12 * nu21
            - self.nu23 * nu32
            - nu31 * self.nu13
            - 2.0 * nu21 * nu32 * self.nu13;

        let d11 = (1.0 - self.nu23 * nu32) * self.e1 / delta;
        let d22 = (1.0 - self.nu13 * nu31) * self.e2 / delta;
        let d33 = (1.0 - self.nu12 * nu21) * self.e3 / delta;
        let d12 = (nu21 + self.nu23 * nu31) * self.e1 / delta;
        let d13 = (nu31 + nu21 * nu32) * self.e1 / delta;
        let d23 = (nu32 + self.nu12 * nu31) * self.e2 / delta;

        let mut d = [[0.0_f64; 6]; 6];
        // Normal-stress block.
        d[0][0] = d11;
        d[1][1] = d22;
        d[2][2] = d33;
        d[0][1] = d12;
        d[1][0] = d12;
        d[0][2] = d13;
        d[2][0] = d13;
        d[1][2] = d23;
        d[2][1] = d23;
        // Shear block — D44=g12 (γxy), D55=g23 (γyz), D66=g13 (γxz).
        d[3][3] = self.g12;
        d[4][4] = self.g23;
        d[5][5] = self.g13;
        // Off-diagonal blocks are zero (initialised that way).
        d
    }
}

impl ConstitutiveLaw for OrthotropicMaterial {
    // Fully-qualified dispatch (not `self.d_matrix_local()`) — the trait method
    // and inherent method share a name; explicit qualification makes the dispatch
    // target stable.  If the inherent method is renamed/moved behind a feature
    // flag, this call will fail to compile instead of silently turning into
    // infinite recursion via trait-method fallback.
    #[inline]
    fn d_matrix_local(&self) -> [[f64; 6]; 6] {
        OrthotropicMaterial::d_matrix_local(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TransverseIsotropicMaterial
// ─────────────────────────────────────────────────────────────────────────────

/// Five-constant transversely-isotropic linear-elastic law.
///
/// The material is isotropic in the 1-2 (in-plane) directions and has a
/// distinct axial stiffness along the 3-direction (build/z axis). This is the
/// literature-standard FDM/composite simplification (cite PRD §Sketch).
///
/// # Relationship to `OrthotropicMaterial`
///
/// A `TransverseIsotropicMaterial` is a specialisation of `OrthotropicMaterial`
/// with:
/// - `E1 = E2 = e_in_plane`
/// - `E3 = e_axial`
/// - `ν12 = nu_in_plane`
/// - `ν13 = ν23 = nu_axial`
/// - `G12 = e_in_plane / (2·(1 + nu_in_plane))` (derived from in-plane isotropy)
/// - `G13 = G23 = g_axial`
///
/// # PRD reference
///
/// `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` §Sketch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransverseIsotropicMaterial {
    /// Young's modulus in the isotropic 1-2 plane.
    pub e_in_plane: f64,
    /// Young's modulus along the axial (3) direction.
    pub e_axial: f64,
    /// In-plane Poisson's ratio ν12.
    pub nu_in_plane: f64,
    /// Axial Poisson's ratio ν13 = ν23.
    pub nu_axial: f64,
    /// Out-of-plane (axial) shear modulus G13 = G23.
    pub g_axial: f64,
}

impl TransverseIsotropicMaterial {
    /// Assert the PD contract in debug builds.
    ///
    /// Delegates to the equivalent [`OrthotropicMaterial::debug_assert_valid`].
    /// Panic messages start with `"TransverseIsotropicMaterial"`.
    #[inline]
    pub(crate) fn debug_assert_valid(&self) {
        debug_assert!(
            self.e_in_plane > 0.0,
            "TransverseIsotropicMaterial.e_in_plane must be positive, got {e}",
            e = self.e_in_plane,
        );
        debug_assert!(
            self.e_axial > 0.0,
            "TransverseIsotropicMaterial.e_axial must be positive, got {e}",
            e = self.e_axial,
        );
        debug_assert!(
            self.g_axial > 0.0,
            "TransverseIsotropicMaterial.g_axial must be positive, got {g}",
            g = self.g_axial,
        );
        // g_in_plane must be positive before we can meaningfully check Δ.
        let g_in_plane = self.e_in_plane / (2.0 * (1.0 + self.nu_in_plane));
        debug_assert!(
            g_in_plane > 0.0,
            "TransverseIsotropicMaterial: g_in_plane = E_p/(2(1+ν_p)) must be positive; \
             got nu_in_plane = {nu}, g_in_plane = {g}",
            nu = self.nu_in_plane,
            g = g_in_plane,
        );
        // Full PD constraint via the single-source Δ formula (see `orthotropic_delta`).
        // Using it here instead of inlining prevents the formula from drifting relative
        // to `OrthotropicMaterial::debug_assert_valid`.
        // Note: the pair-determinant sub-checks (1 − νij·νji > 0) also fire in debug
        // builds when `d_matrix_local` delegates to `OrthotropicMaterial::d_matrix_local`.
        let delta = orthotropic_delta(
            self.e_in_plane, self.e_in_plane, self.e_axial,
            self.nu_in_plane, self.nu_axial, self.nu_axial,
        );
        debug_assert!(
            delta > 0.0,
            "TransverseIsotropicMaterial: positive-definite constraint Δ must be > 0, \
             got Δ = {delta} (PRD §C1)",
        );
    }

    /// Return the 6×6 elasticity matrix in engineering-strain Voigt order.
    ///
    /// Builds the equivalent [`OrthotropicMaterial`] and delegates to its
    /// `d_matrix_local`, ensuring numerical consistency between the two types.
    pub fn d_matrix_local(&self) -> [[f64; 6]; 6] {
        self.debug_assert_valid();
        let g_in_plane = self.e_in_plane / (2.0 * (1.0 + self.nu_in_plane));
        OrthotropicMaterial {
            e1: self.e_in_plane,
            e2: self.e_in_plane,
            e3: self.e_axial,
            g12: g_in_plane,
            g13: self.g_axial,
            g23: self.g_axial,
            nu12: self.nu_in_plane,
            nu13: self.nu_axial,
            nu23: self.nu_axial,
        }
        .d_matrix_local()
    }
}

impl ConstitutiveLaw for TransverseIsotropicMaterial {
    // Same name-shadowing concern as OrthotropicMaterial — fully-qualified.
    // `IsotropicElastic` is not affected: its inherent method is `d_matrix`
    // (not `d_matrix_local`), so there is no name collision there.
    #[inline]
    fn d_matrix_local(&self) -> [[f64; 6]; 6] {
        TransverseIsotropicMaterial::d_matrix_local(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// rotate_voigt
// ─────────────────────────────────────────────────────────────────────────────

/// Rotate a 6×6 Voigt elasticity matrix from a local material frame into a
/// global frame.
///
/// # Convention (single source of truth)
///
/// `rotation` is the **local → global** rotation R: it transforms vector
/// *components* as
///
/// ```text
/// v_global = R · v_local
/// ```
///
/// Equivalently, the **columns** of `rotation` are the three local basis
/// vectors `e₁, e₂, e₃` expressed in global coordinates. (The rows are the
/// global basis vectors expressed in local coordinates.)
///
/// ⚠ This is the **transpose** of the `ShellFrame.r` convention in
/// `shell_assembly.rs` (which stores rows = local basis in global coords,
/// i.e. `v_local = ShellFrame.r · v_global`, the global → local map).
/// Callers holding a `ShellFrame` must pass `transpose(frame.r)` here.
///
/// # Formula (PRD Contract C2)
///
/// ```text
/// D_global = T · D_local · Tᵀ
/// ```
///
/// where `T = M_σ(R)` is the 6×6 **Voigt-stress Bond matrix** for R. With the
/// chosen convention, `M_σ(R)` transforms stress *local → global*
/// (`σ_global = T · σ_local`); the matching engineering-strain transform is
/// `Tᵀ` (so `ε_local = Tᵀ · ε_global`), and the two combine into the formula
/// above.
///
/// # Worked sanity example
///
/// 90° active rotation of the material frame about z (local x → global y):
///
/// ```text
///       ⎡ 0  −1   0 ⎤
///   R = ⎢ 1   0   0 ⎥   (columns = local e₁,e₂,e₃ in global coords:
///       ⎣ 0   0   1 ⎦    e₁=[0,1,0]=ŷ, e₂=[−1,0,0]=−x̂, e₃=[0,0,1]=ẑ)
/// ```
///
/// For an orthotropic D with `D₁₁ ≠ D₂₂` (stiffness along local x vs y),
/// `rotate_voigt(&D, &R)` returns a D' with `D'₁₁ ≈ D₂₂` and `D'₂₂ ≈ D₁₁` —
/// the original stiffness along local x is now along global y (and vice
/// versa). Tests `rotate_voigt_90deg_about_z_swaps_d11_d22_for_orthotropic`
/// and `rotate_voigt_round_trip_with_inverse_restores_d_local` pin this.
///
/// # Voigt convention
///
/// Engineering-shear order `[εxx, εyy, εzz, γxy, γyz, γxz]`, consistent with
/// [`IsotropicElastic::d_matrix`] (shear-block diagonal = G, not 2G).
///
/// # T-matrix construction
///
/// Let the rows of `rotation` be `[l1,m1,n1]`, `[l2,m2,n2]`, `[l3,m3,n3]`
/// (i.e. `lᵢ = rotation[i][0]`, `mᵢ = rotation[i][1]`, `nᵢ = rotation[i][2]`).
/// Under the local → global convention above, row `i` of `rotation` is the
/// `i`-th global basis vector expressed in local coordinates, and these are
/// exactly the direction cosines the Bond formula expects.
///
/// Upper-left 3×3 block (row `i` of T, col `j`): `lᵢ²`, `mᵢ²`, `nᵢ²` (squares of direction cosines).
///
/// Upper-right 3×3 block (row `i`, col-groups `{12,23,13}`):
/// `2·lᵢ·mᵢ`, `2·mᵢ·nᵢ`, `2·lᵢ·nᵢ` (factor-of-2 for engineering-shear strain).
///
/// Lower-left 3×3 block (row `{12,23,13}`, col `j`):
/// `l₁·l₂`, `m₁·m₂`, `n₁·n₂`, etc. (products of direction cosines).
///
/// Lower-right 3×3 block (row `{12,23,13}`, col `{12,23,13}`):
/// `lᵢ·mⱼ + lⱼ·mᵢ`, etc. (sum-of-products for double-index pairs).
///
/// # Parameters
///
/// - `d_local`: the 6×6 D matrix in the material's local frame (from a
///   [`ConstitutiveLaw::d_matrix_local`] call).
/// - `rotation`: the 3×3 orthonormal local → global rotation. Columns are
///   the local basis vectors in global coordinates; rows are the global
///   basis vectors in local coordinates. See the ⚠ note about
///   `ShellFrame.r` (which is the transpose of this matrix).
///
/// # Returns
///
/// The 6×6 D matrix in the global frame. All stack-allocated; no heap
/// allocation.
#[allow(clippy::needless_range_loop)]
pub fn rotate_voigt(d_local: &[[f64; 6]; 6], rotation: &[[f64; 3]; 3]) -> [[f64; 6]; 6] {
    // Extract row direction-cosines.
    let [l1, m1, n1] = rotation[0];
    let [l2, m2, n2] = rotation[1];
    let [l3, m3, n3] = rotation[2];

    // Build the 6×6 transformation matrix T.
    // Voigt stress-transformation (Bond matrix) for engineering-shear convention.
    // Row/col index mapping:
    //   0=xx, 1=yy, 2=zz, 3=xy, 4=yz, 5=xz
    //
    // Upper-left (3×3): squares of direction cosines.
    // Upper-right (3×3): 2·lᵢ·mᵢ  2·mᵢ·nᵢ  2·lᵢ·nᵢ
    // Lower-left  (3×3): products l₁l₂ m₁m₂ n₁n₂ etc.
    // Lower-right (3×3): sum-of-product pairs.
    let mut t = [[0.0_f64; 6]; 6];

    // Row 0 (xx):  upper-left: l1² m1² n1²;  upper-right: 2l1m1 2m1n1 2l1n1
    t[0][0] = l1 * l1;
    t[0][1] = m1 * m1;
    t[0][2] = n1 * n1;
    t[0][3] = 2.0 * l1 * m1;
    t[0][4] = 2.0 * m1 * n1;
    t[0][5] = 2.0 * l1 * n1;

    // Row 1 (yy):  l2² m2² n2²;  2l2m2 2m2n2 2l2n2
    t[1][0] = l2 * l2;
    t[1][1] = m2 * m2;
    t[1][2] = n2 * n2;
    t[1][3] = 2.0 * l2 * m2;
    t[1][4] = 2.0 * m2 * n2;
    t[1][5] = 2.0 * l2 * n2;

    // Row 2 (zz):  l3² m3² n3²;  2l3m3 2m3n3 2l3n3
    t[2][0] = l3 * l3;
    t[2][1] = m3 * m3;
    t[2][2] = n3 * n3;
    t[2][3] = 2.0 * l3 * m3;
    t[2][4] = 2.0 * m3 * n3;
    t[2][5] = 2.0 * l3 * n3;

    // Row 3 (xy):  lower-left: l1l2 m1m2 n1n2;  lower-right: l1m2+l2m1  m1n2+m2n1  l1n2+l2n1
    t[3][0] = l1 * l2;
    t[3][1] = m1 * m2;
    t[3][2] = n1 * n2;
    t[3][3] = l1 * m2 + l2 * m1;
    t[3][4] = m1 * n2 + m2 * n1;
    t[3][5] = l1 * n2 + l2 * n1;

    // Row 4 (yz):  lower-left: l2l3 m2m3 n2n3;  lower-right: l2m3+l3m2  m2n3+m3n2  l2n3+l3n2
    t[4][0] = l2 * l3;
    t[4][1] = m2 * m3;
    t[4][2] = n2 * n3;
    t[4][3] = l2 * m3 + l3 * m2;
    t[4][4] = m2 * n3 + m3 * n2;
    t[4][5] = l2 * n3 + l3 * n2;

    // Row 5 (xz):  lower-left: l1l3 m1m3 n1n3;  lower-right: l1m3+l3m1  m1n3+m3n1  l1n3+l3n1
    t[5][0] = l1 * l3;
    t[5][1] = m1 * m3;
    t[5][2] = n1 * n3;
    t[5][3] = l1 * m3 + l3 * m1;
    t[5][4] = m1 * n3 + m3 * n1;
    t[5][5] = l1 * n3 + l3 * n1;

    // Compute D_global = T · D_local · Tᵀ  in two steps:
    //   tmp   = D_local · Tᵀ       →  tmp[i][j]  = Σ_k D[i][k] · T[j][k]
    //   D_global = T · tmp          →  out[i][j] = Σ_k T[i][k] · tmp[k][j]
    //
    // NOTE: the formula is T · D · Tᵀ (NOT Tᵀ · D · T).
    // Derivation sketch (engineering-shear Voigt convention):
    //   σ_global = M_σ · σ_local            (M_σ = T, stress Bond matrix for R)
    //   ε_eng_local = A · ε_eng_global       (A = H·M_σ·H⁻¹, H=diag(1,1,1,2,2,2))
    //   D_global = M_σ · D_local · A⁻¹
    // For orthogonal R: Bond matrices compose multiplicatively, so
    //   M_σ(R)·M_σ(Rᵀ) = M_σ(R·Rᵀ) = M_σ(I) = I  →  M_σ(R)⁻¹ = M_σ(Rᵀ).
    // The engineering-shear H-conjugate M_ε = H·M_σ·H⁻¹ is also orthogonal
    // (H is diagonal, orthogonal R keeps M_σ orthogonal up to H-scaling), so
    //   A⁻¹ = Aᵀ = (H·M_σ·H⁻¹)ᵀ = H·M_σᵀ·H⁻¹ = H·M_σ(Rᵀ)·H⁻¹ = Tᵀ.
    // → D_global = T · D_local · Tᵀ
    // Direction is empirically pinned by `rotate_voigt_round_trip_with_inverse_restores_d_local`
    // and `rotate_voigt_30deg_about_z_matches_lamina_transformation_with_correct_sign`.
    let mut tmp = [[0.0_f64; 6]; 6];
    for i in 0..6 {
        for j in 0..6 {
            for k in 0..6 {
                // tmp = D · Tᵀ: D[i][k] * T^T[k][j] = D[i][k] * T[j][k]
                tmp[i][j] += d_local[i][k] * t[j][k];
            }
        }
    }
    let mut d_global = [[0.0_f64; 6]; 6];
    for i in 0..6 {
        for j in 0..6 {
            for k in 0..6 {
                d_global[i][j] += t[i][k] * tmp[k][j];
            }
        }
    }
    d_global
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

    /// Multiply a 6×6 matrix by a 6-vector.
    fn matvec(d: &[[f64; 6]; 6], v: &[f64; 6]) -> [f64; 6] {
        let mut out = [0.0_f64; 6];
        for i in 0..6 {
            for j in 0..6 {
                out[i] += d[i][j] * v[j];
            }
        }
        out
    }

    /// Assert that an N×N matrix is entry-wise finite and symmetric.
    ///
    /// Symmetry tolerance: `|D[i][j] − D[j][i]| < 1e-9 · max(|D[i][j]|, |D[j][i]|, 1)`.
    fn assert_symmetric_finite<const N: usize>(d: &[[f64; N]; N]) {
        for i in 0..N {
            for j in 0..N {
                assert!(
                    d[i][j].is_finite(),
                    "D[{i}][{j}] = {} is not finite",
                    d[i][j]
                );
                let lhs = d[i][j];
                let rhs = d[j][i];
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    /// Steel-like reference: E = 200 GPa, ν = 0.3 (Pa, dimensionless).
    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    #[test]
    fn d_matrix_is_symmetric_for_steel_like_inputs() {
        assert_symmetric_finite(&steel_like().d_matrix());
    }

    #[test]
    fn d_matrix_hydrostatic_strain_yields_hydrostatic_stress_with_bulk_modulus() {
        // ε_v = 1e-4 in each normal slot; expect σ_xx = σ_yy = σ_zz and
        // trace(σ)/3 = K · trace(ε), K = E / (3 (1 − 2ν)).
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let bulk = e / (3.0 * (1.0 - 2.0 * nu));
        let eps_v = 1.0e-4;
        let strain = [eps_v, eps_v, eps_v, 0.0, 0.0, 0.0];

        let sigma = matvec(&mat.d_matrix(), &strain);

        let trace_sigma = sigma[0] + sigma[1] + sigma[2];
        let trace_eps = 3.0 * eps_v;
        let expected_mean = bulk * trace_eps;
        let actual_mean = trace_sigma / 3.0;
        assert!(
            (actual_mean - expected_mean).abs() < 1e-9 * expected_mean.abs(),
            "mean stress: got {actual_mean}, expected {expected_mean}",
        );

        // All three normal components equal under hydrostatic loading.
        let scale = sigma[0].abs().max(1.0);
        assert!((sigma[0] - sigma[1]).abs() < 1e-9 * scale);
        assert!((sigma[0] - sigma[2]).abs() < 1e-9 * scale);

        // No shear response under hydrostatic strain.
        for k in 3..6 {
            assert!(
                sigma[k].abs() < 1e-9 * scale,
                "shear leak at {k}: {}",
                sigma[k]
            );
        }
    }

    #[test]
    fn d_matrix_pure_shear_strain_yields_shear_stress_via_g() {
        // ε = (0, 0, 0, γ, 0, 0) → σ_xy = G·γ with G = E / (2(1+ν));
        // all other σ-components vanish.
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g = e / (2.0 * (1.0 + nu));
        let gamma = 2.5e-4;
        let strain = [0.0, 0.0, 0.0, gamma, 0.0, 0.0];

        let sigma = matvec(&mat.d_matrix(), &strain);

        let expected_shear = g * gamma;
        assert!(
            (sigma[3] - expected_shear).abs() < 1e-9 * expected_shear.abs(),
            "σ_xy: got {}, expected {expected_shear}",
            sigma[3],
        );

        // Other five components must vanish.
        let scale = sigma[3].abs().max(1.0);
        for (k, val) in sigma.iter().enumerate() {
            if k == 3 {
                continue;
            }
            assert!(val.abs() < 1e-9 * scale, "non-zero σ[{k}] = {val}");
        }
    }

    #[test]
    fn d_matrix_zero_poisson_limit_is_diagonal_with_e_and_e_over_two() {
        // ν = 0 ⇒ λ = 0, μ = E/2; the D matrix collapses to
        // diag(E, E, E, E/2, E/2, E/2).
        let e: f64 = 1.0;
        let mat = IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: 0.0,
        };
        let d = mat.d_matrix();
        for i in 0..6 {
            for j in 0..6 {
                let expected: f64 = if i == j {
                    if i < 3 { e } else { e / 2.0 }
                } else {
                    0.0
                };
                let scale = expected.abs().max(1.0);
                assert!(
                    (d[i][j] - expected).abs() < 1e-9 * scale,
                    "D[{i}][{j}] = {} (expected {expected})",
                    d[i][j],
                );
            }
        }
    }

    #[test]
    fn d_matrix_uniaxial_strain_recovers_lame_diagonal_and_off_diagonal() {
        // ε = (1, 0, 0, 0, 0, 0) ⇒ σ_xx = λ + 2μ, σ_yy = σ_zz = λ,
        // shears all zero.
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let lambda_plus_two_mu = lambda + two_mu;

        let strain = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let sigma = matvec(&mat.d_matrix(), &strain);

        assert!(
            (sigma[0] - lambda_plus_two_mu).abs() < 1e-9 * lambda_plus_two_mu.abs(),
            "σ_xx: got {}, expected λ+2μ = {lambda_plus_two_mu}",
            sigma[0],
        );
        assert!(
            (sigma[1] - lambda).abs() < 1e-9 * lambda.abs(),
            "σ_yy: got {}, expected λ = {lambda}",
            sigma[1],
        );
        assert!(
            (sigma[2] - lambda).abs() < 1e-9 * lambda.abs(),
            "σ_zz: got {}, expected λ = {lambda}",
            sigma[2],
        );
        for k in 3..6 {
            let scale = sigma[0].abs().max(1.0);
            assert!(
                sigma[k].abs() < 1e-9 * scale,
                "σ[{k}] should vanish, got {}",
                sigma[k]
            );
        }
    }

    // --- Auxetic (negative-ν) validity range tests ---

    #[test]
    fn d_matrix_accepts_auxetic_poisson_ratio_with_positive_bulk_and_shear_moduli() {
        // ν = -0.5 is inside the physical PD range (-1, 0.5).
        // K = E/(3(1−2ν)) = 1/(3·2) = 1/6 > 0;  G = E/(2(1+ν)) = 1/(2·0.5) = 1 > 0.
        let e = 1.0_f64;
        let nu = -0.5_f64;
        let mat = IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: nu,
        };

        let d = mat.d_matrix();

        // Finite and symmetric.
        assert_symmetric_finite(&d);

        // Hydrostatic strain → bulk modulus K > 0.
        let bulk = e / (3.0 * (1.0 - 2.0 * nu));
        assert!(bulk > 0.0, "K = {bulk} should be positive for ν = {nu}");
        let eps_v = 1.0e-4_f64;
        let strain_h = [eps_v, eps_v, eps_v, 0.0, 0.0, 0.0];
        let sigma_h = matvec(&d, &strain_h);
        let trace_sigma = sigma_h[0] + sigma_h[1] + sigma_h[2];
        let mean_stress = trace_sigma / 3.0;
        let expected_mean = bulk * (3.0 * eps_v);
        let scale = expected_mean.abs().max(1.0);
        assert!(
            (mean_stress - expected_mean).abs() < 1e-9 * scale,
            "mean stress: got {mean_stress}, expected {expected_mean}",
        );

        // Pure-shear strain → shear modulus G > 0.
        let g = e / (2.0 * (1.0 + nu));
        assert!(g > 0.0, "G = {g} should be positive for ν = {nu}");
        let gamma = 1.0e-4_f64;
        let strain_s = [0.0, 0.0, 0.0, gamma, 0.0, 0.0];
        let sigma_s = matvec(&d, &strain_s);
        let expected_shear = g * gamma;
        let scale_s = expected_shear.abs().max(1.0);
        assert!(
            (sigma_s[3] - expected_shear).abs() < 1e-9 * scale_s,
            "σ_xy: got {}, expected G·γ = {expected_shear}",
            sigma_s[3],
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn d_matrix_panics_at_incompressible_upper_limit() {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.5,
        }
        .d_matrix();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn d_matrix_panics_at_auxetic_lower_limit() {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: -1.0,
        }
        .d_matrix();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn d_matrix_panics_above_incompressible_limit() {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.6,
        }
        .d_matrix();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "poisson_ratio")]
    fn d_matrix_panics_below_auxetic_limit() {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: -1.5,
        }
        .d_matrix();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "youngs_modulus")]
    fn d_matrix_panics_when_youngs_modulus_is_zero() {
        IsotropicElastic {
            youngs_modulus: 0.0,
            poisson_ratio: 0.3,
        }
        .d_matrix();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "youngs_modulus")]
    fn d_matrix_panics_when_youngs_modulus_is_negative() {
        IsotropicElastic {
            youngs_modulus: -1.0,
            poisson_ratio: 0.3,
        }
        .d_matrix();
    }
}
