//! CST **membrane** geometric stiffness `K_g`, in-plane prestress resultant, and
//! per-element tangent `K_t = K_e + K_g` for `reify-solver-elastic`.
//!
//! The 2-D surface-element analogue of the bar `K_g`
//! (`geometric_stiffness/bar.rs`). Under an in-plane membrane prestress
//! resultant `S` (2×2, force/length, element local frame) the geometric
//! stiffness stiffens **only** the transverse/local-normal DOF:
//!
//! ```text
//! K_g_loc[3i+2][3j+2] = A · (dn_i · S · dn_j)
//! ```
//!
//! while the in-plane DOFs carry zero geometric stiffness — the 2-D analogue of
//! the bar's axial null. For an isotropic resultant `N = σ·t` this reduces to
//! `N·A·(∇N·∇N)`, the σt-scaled cotangent-Laplacian (consistent with the §4
//! NFDM reduction). See PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 + D1/D2,
//! task ζ.
//!
//! The elastic stiffness `K_e` lives in
//! [`crate::elements::membrane_cst::element_stiffness_membrane_cst`].

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// In-plane membrane stress **resultant** (force/length) in the element local
/// frame, carried as a 2×2 tensor `S` indexed `resultant[i][j]`.
///
/// Mirrors [`crate::geometric_stiffness::InitialStress3`]: an isotropic
/// constructor ([`Self::isotropic`]) and a [`Self::zero`] trivial input, with
/// the kernel symmetrising `S` implicitly via the double sum (no `i ≤ j`
/// shortcut), so a slightly off-symmetric input yields the `K_g` of
/// `0.5·(S + Sᵀ)` rather than panicking.
///
/// Isotropic-first per PRD D1; the general 2×2 resultant keeps the anisotropic
/// warp/weft extension (task ε) a drop-in without re-architecting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MembranePrestress {
    /// 2×2 in-plane stress resultant `S` (force/length), element local frame.
    pub resultant: [[f64; 2]; 2],
}

impl MembranePrestress {
    /// Isotropic in-plane prestress resultant `N`: `S = N·I₂`
    /// (`[[N, 0], [0, N]]`). For `N = σ·t` this is a uniform membrane
    /// pretension of stress `σ` through thickness `t`.
    pub fn isotropic(n: f64) -> Self {
        Self {
            resultant: [[n, 0.0], [0.0, n]],
        }
    }

    /// Zero prestress — the trivial input that pins
    /// `geometric_element_stiffness_membrane_cst == 0` entrywise.
    pub const fn zero() -> Self {
        Self {
            resultant: [[0.0; 2]; 2],
        }
    }
}

/// Compute the 9×9 membrane geometric stiffness `K_g` for a flat 3-node CST
/// membrane under an in-plane prestress resultant.
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `prestress` is the in-plane stress resultant `S` (local frame). Returns an
/// [`ElementStiffness`] with `n_dofs = 9`, row-major, sharing the `3·node +
/// axis` DOF layout of [`ElementStiffness`].
pub fn geometric_element_stiffness_membrane_cst(
    nodes: &[[f64; 3]; 3],
    prestress: &MembranePrestress,
) -> ElementStiffness {
    todo!("geometric_element_stiffness_membrane_cst: implemented in S6")
}

/// Compute the per-element membrane tangent stiffness `K_t = K_e + K_g`.
///
/// A flat membrane's transverse stiffness comes **entirely** from `K_g`
/// (`K_e` is transversely singular), so a transverse/pressure solve requires
/// `K_t`. Mirrors [`crate::geometric_stiffness::bar_tangent_stiffness`].
pub fn membrane_tangent_stiffness(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
    prestress: &MembranePrestress,
) -> ElementStiffness {
    todo!("membrane_tangent_stiffness: implemented in S8")
}
