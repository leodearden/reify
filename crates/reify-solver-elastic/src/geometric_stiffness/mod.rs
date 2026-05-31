//! Element-level **geometric stiffness** `K_g` assembly for the
//! linear-buckling eigenproblem.
//!
//! See PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task γ. This module
//! ships:
//!
//! - [`geometric_element_stiffness_tet_p1`] — the P1 (linear, 4-node)
//!   tetrahedron geometric-stiffness kernel,
//! - stub entry points for shell / hex / wedge cells
//!   ([`geometric_element_stiffness_shell`],
//!   [`geometric_element_stiffness_hex_p1`],
//!   [`geometric_element_stiffness_wedge_p1`]) — these panic with a
//!   descriptive citation; the diagnostic-emitting trampoline path is
//!   task ζ's job (`E_BucklingShellNotImplemented` /
//!   `E_BucklingHexWedgeNotImplemented`).
//!
//! Global `K_g` is obtained by feeding the per-element matrices through the
//! existing [`crate::assembly::assemble_global_stiffness`] scatter — the
//! per-element `K_g` shares the [`ElementStiffness`](crate::assembly::ElementStiffness)
//! row-major shape and DOF-ordering contract of the elastic `K_e`, so no
//! separate global assembler is needed.
//!
//! # Formula
//!
//! For a 3D solid element with initial Cauchy stress `σ⁰`,
//!
//! ```text
//! K_g[3a+α, 3b+α] = ∫_Ω σ⁰_ij · (∂N_a/∂x_i) · (∂N_b/∂x_j) dV    α ∈ {0,1,2}
//! ```
//!
//! and off-axis (α ≠ β) entries are zero. Symmetry follows from `σ⁰` being
//! symmetric. For a constant-strain P1 tet with constant `σ⁰` the integrand
//! is constant over the element, so the integral collapses to
//! `(∇N_a · σ⁰ · ∇N_b) · V_e` per node pair `(a, b)`.

pub mod bar;
pub mod stubs;
pub mod tet;
pub mod tet_p2;

pub use bar::{bar_tangent_stiffness, geometric_element_stiffness_bar_p1};
pub use stubs::{
    geometric_element_stiffness_hex_p1, geometric_element_stiffness_shell,
    geometric_element_stiffness_wedge_p1,
};
pub use tet::geometric_element_stiffness_tet_p1;
pub use tet_p2::geometric_element_stiffness_tet_p2;

/// Constant 3×3 symmetric Cauchy stress in the global frame.
///
/// Components are indexed `sigma[i][j]` for `(i, j) ∈ {0,1,2}²`.
/// Callers are expected to supply a symmetric tensor; the
/// [`geometric_element_stiffness_tet_p1`] kernel symmetrises implicitly by
/// summing `σ_ij · g_i · g_j` over all `(i, j)` pairs (no `i ≤ j` shortcut),
/// so a slightly off-symmetric input yields the K_g of `0.5·(σ + σᵀ)` rather
/// than panicking — see the unit-test note on symmetric-input contract.
///
/// Use [`InitialStress3::uniaxial_z`] for uniform axial pre-stress
/// (the Euler-column buckling fixture), or build the array directly for
/// general stress fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitialStress3 {
    /// 3×3 symmetric Cauchy stress in the global frame.
    pub sigma: [[f64; 3]; 3],
}

impl InitialStress3 {
    /// Zero stress — the trivial input that pins
    /// `geometric_element_stiffness_tet_p1 == 0` per PRD §13 task γ
    /// observable signal (b).
    pub const fn zero() -> Self {
        Self {
            sigma: [[0.0; 3]; 3],
        }
    }

    /// Uniaxial stress along the global z-axis: `σ_zz = s`, all other
    /// components zero. Used by the Euler-column buckling fixture in
    /// `tests/kg_p1_tet.rs` — a negative `s` is compressive.
    pub const fn uniaxial_z(s: f64) -> Self {
        Self {
            sigma: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, s]],
        }
    }
}
