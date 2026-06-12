//! Constant-strain-triangle (CST) **membrane** element for `reify-solver-elastic`.
//!
//! A dedicated 3-DOF/node (translation-only) flat membrane triangle — the 2-D
//! surface-element analogue of the pin-jointed bar (`assembly/bar.rs` `K_e` +
//! `geometric_stiffness/bar.rs` `K_g`). See PRD
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 + D2, task ζ.
//!
//! The elastic stiffness `K_e` reuses the shell's patch-test-validated CST
//! membrane block (`shell_assembly::membrane_node_pair_block`), so the membrane
//! `K_e` is the *same* validated `Bₘᵀ(t·D_pl)Bₘ` triple-product as the MITC3
//! shell — a structural guarantee, not two copies kept in lockstep. The element
//! is assembled in the local mid-surface frame (`build_shell_frame` +
//! `shell_kinematics`) then block-rotated to global by `blockdiag(R)` over the
//! three three-DOF nodal blocks. A flat membrane has no rotational DOFs, so the
//! shell's drilling/SPD-suppression machinery does not apply.
//!
//! The companion geometric-stiffness kernel `K_g` and the per-element tangent
//! `K_t = K_e + K_g` live in [`crate::geometric_stiffness::membrane`].

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Compute the 9×9 elastic stiffness `K_e` for a flat 3-node CST membrane
/// element (3 translational DOF/node, DOF layout `3·node + axis`).
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `thickness` is the constant membrane thickness `t`. `material` is the
/// isotropic linear-elastic constitutive law (plane stress).
///
/// Returns an [`ElementStiffness`] with `n_dofs = 9`, row-major, assemblable
/// through the unchanged [`crate::assemble_global_stiffness`] scatter
/// (`dofs_per_node = 9 / 3 = 3`).
pub fn element_stiffness_membrane_cst(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
) -> ElementStiffness {
    todo!("element_stiffness_membrane_cst: implemented in S2/S4")
}
