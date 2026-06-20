//! Membrane (surface-element) load analysis with a tension-only active set
//! (Tensegrity-membrane η, layer M2).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11 (task η). This is the
//! pure numeric kernel behind the dedicated `solver::membrane_load` ComputeNode
//! target: given a form-found pavilion (node coordinates, per-line-member and
//! per-patch prestress, bar/cable sections + a shared membrane section), external
//! nodal loads, and a set of fixed support nodes, it assembles the combined
//! tangent stiffness `K_t = K_e + K_g` for **both** the bar/cable members
//! ([`crate::bar_tangent_stiffness`]) and the CST membrane patches
//! ([`crate::membrane_tangent_stiffness`]), solves the linear system via the
//! existing CG path, and reports nodal deflections plus per-member force deltas
//! and per-patch membrane-stress deltas. A tension-only active-set wrapper drops
//! both slack cables (cables whose total force goes compressive) and slack
//! membrane patches (patches whose minimum principal stress goes compressive)
//! and re-solves to a fixed point.
//!
//! # Method
//!
//! Bars and flat CST membranes both expose three translational DOF per node with
//! the same `3·node + axis` DOF layout, so they scatter through the unchanged
//! [`crate::assemble_global_stiffness`] into **one** global SPD system — the
//! "pavilion under load" is one combined solve. External loads are applied with
//! [`crate::apply_point_load`]; each fixed node expands to three homogeneous
//! Dirichlet BCs applied via [`crate::apply_dirichlet_row_elimination`]; the
//! reduced system is solved with [`crate::solve_cg`].
//!
//! The line-member force delta is `dNᵢ = (Eᵢ Aᵢ / Lᵢ) · cᵢ · (u_k − u_j)` and the
//! total member force is `Nᵢ = prestressᵢ + dNᵢ` — the verbatim T3b
//! (`tensegrity_load`) bar delta. Each membrane patch's in-plane stress delta
//! `Δσ` is recovered by [`membrane_stress_delta`] (a constant-strain recovery),
//! and the patch's total stress `σ_total = σ₀·I + Δσ` feeds the slack test.
//!
//! The tension-only active set drops any active cable whose total force is
//! compressive (`Nᵢ < −slack_tol`) and any active membrane patch whose minimum
//! principal stress is compressive (`min eig(σ_total) < −slack_tol`), then
//! re-solves; the drop is monotone (a dropped cable/patch is never re-added
//! within a solve), so the active set strictly shrinks and the loop terminates in
//! at most `#cables + #patches` passes. The geometric stiffness `K_g` is held
//! *linear-about-prestress* (it uses the fixed form-found `σ₀` / `N`, not the
//! load-updated state, per PRD §5/§10), so the converged post-drop deflection is
//! exactly the reduced linear system with the slack elements removed.
//!
//! # Scope
//!
//! Load analysis on a supplied form-found geometry + prestress only, with a
//! single shared membrane section broadcast across patches (the trampoline's v1
//! decision). Re-running form-finding, geometrically-nonlinear / force-updated
//! `K_g`, and per-patch heterogeneous fabrics are out of scope (PRD §10 future
//! work).

use crate::constitutive::IsotropicElastic;
use crate::solver::CgSolverOptions;
use crate::tensegrity_load::BarMember;

/// A single flat three-node CST membrane patch in a membrane load problem.
///
/// The surface-element analogue of [`BarMember`]: it carries its three corner
/// node indices, constant thickness, isotropic material, and the form-found
/// isotropic in-plane prestress `σ₀` (stress, tension positive). The kernel keeps
/// a per-patch material/thickness so heterogeneous fabrics are a clean additive
/// extension; the v1 trampoline broadcasts a single shared section across all
/// patches.
pub struct MembranePatch {
    /// Global node indices `(n0, n1, n2)` of the patch's three corners.
    pub nodes: (usize, usize, usize),
    /// Constant membrane thickness `t` (used both for `K_e` and to scale the
    /// prestress into the resultant `N = σ₀·t` for `K_g`).
    pub thickness: f64,
    /// Isotropic linear-elastic material (Young's modulus + Poisson ratio).
    pub material: IsotropicElastic,
    /// Form-found isotropic in-plane prestress `σ₀` (stress, tension positive).
    /// Seeds the geometric stiffness `K_g` (via `N = σ₀·t`) and the slack test.
    pub prestress: f64,
}

/// Tuning knobs for [`membrane_load_analysis`].
#[derive(Debug, Clone)]
pub struct MembraneLoadOptions {
    /// Hard cap on tension-only active-set passes. Drop-only monotonicity
    /// guarantees a fixed point in at most `#cables + #patches` passes, so
    /// exceeding this cap surfaces [`MembraneLoadError::ActiveSetDidNotConverge`]
    /// (the PRD §11 Q5 defensive guard) rather than spinning.
    pub max_active_set_iters: usize,
    /// Inner linear-solve (CG) options used for each active-set pass.
    pub cg: CgSolverOptions,
    /// Slack tolerance: an active cable is dropped when its total force is
    /// `< −slack_tol`, and an active patch is dropped when its minimum principal
    /// stress is `< −slack_tol`. A small positive value tolerates floating-point
    /// noise around zero tension; `0.0` drops strictly compressive elements.
    pub slack_tol: f64,
}

impl Default for MembraneLoadOptions {
    fn default() -> Self {
        Self {
            // Comfortably above any monotone active-set count; the kernel also
            // bounds itself by `#cables + #patches`. Lowering this below the
            // natural count is how the §11 Q5 guard is exercised deterministically.
            max_active_set_iters: 64,
            cg: CgSolverOptions::default(),
            slack_tol: 0.0,
        }
    }
}

/// Reason a membrane load solve is infeasible. Surfaced by the trampoline as an
/// `E_MembraneLoadInfeasible` diagnostic (PRD §11 contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembraneLoadError {
    /// Input arrays disagree in length (e.g. `loads.len() != nodes.len()`), or a
    /// bar endpoint / patch corner / support node index is out of range for the
    /// node set.
    DimensionMismatch,
    /// Every node is fixed — there is no free DOF to solve for.
    EmptyFreeSet,
    /// The assembled tangent system was singular (a free node touched by no
    /// active bar or patch), or the inner CG solve failed to converge.
    SingularSystem,
    /// The tension-only active set did not reach a fixed point within
    /// `max_active_set_iters` passes (PRD §11 Q5 defensive guard).
    ActiveSetDidNotConverge {
        /// Number of active-set passes performed before hitting the cap.
        iterations: usize,
    },
}

/// Result of a membrane load solve.
#[derive(Debug, Clone)]
pub struct MembraneLoadSolve {
    /// Per-node displacement `u` (length = node count), in original node order.
    /// Fixed support nodes are exactly zero.
    pub displacements: Vec<[f64; 3]>,
    /// Per-line-member total axial force `Nᵢ = prestressᵢ + dNᵢ`, in input
    /// `bar_members` order. Slack (dropped) cables report `0.0`.
    pub member_forces: Vec<f64>,
    /// Per-line-member force delta `dNᵢ` from the applied load, in input
    /// `bar_members` order. Slack (dropped) cables report `−prestressᵢ`.
    pub member_force_deltas: Vec<f64>,
    /// Per-line-member slack mask, in input `bar_members` order — `true` iff the
    /// member is a cable that the tension-only active set dropped.
    pub member_slack: Vec<bool>,
    /// Per-patch in-plane stress delta `Δσ` (symmetric 2×2, element local frame),
    /// in input `membrane_patches` order.
    pub surface_stress_deltas: Vec<[[f64; 2]; 2]>,
    /// Per-patch principal stresses `[min, max]` of the total stress
    /// `σ_total = σ₀·I + Δσ`, in input `membrane_patches` order.
    pub surface_principal_stresses: Vec<[f64; 2]>,
    /// Per-patch slack mask, in input `membrane_patches` order — `true` iff the
    /// patch went compressive (min principal stress `< −slack_tol`) and the
    /// tension-only active set dropped it.
    pub surface_slack: Vec<bool>,
    /// Number of tension-only active-set passes performed before the fixed point
    /// (all elements active ⇒ `1`).
    pub active_set_iterations: usize,
    /// Whether the solve converged: the inner CG converged on every pass and the
    /// active set reached a fixed point within the iteration cap.
    pub converged: bool,
}

/// Solve the membrane load-analysis problem.
///
/// `nodes` are the form-found node coordinates; `bar_members` are the bar/cable
/// line members; `membrane_patches` are the CST membrane patches; `loads` is the
/// per-node external force (length must equal `nodes.len()`); `fixed_nodes` lists
/// the support node indices (each pinned in all three axes); `options` tunes the
/// inner CG solve and the active-set cap.
///
/// Returns the solved [`MembraneLoadSolve`] on success, or a
/// [`MembraneLoadError`] describing why the input is infeasible.
///
/// # Errors
///
/// - [`MembraneLoadError::DimensionMismatch`] — `loads.len() != nodes.len()`, or
///   a bar endpoint / patch corner / support index lies outside `0..nodes.len()`.
/// - [`MembraneLoadError::EmptyFreeSet`] — every node is anchored.
/// - [`MembraneLoadError::SingularSystem`] — an inner CG pass failed to converge,
///   or a free node has no incident bar or patch.
/// - [`MembraneLoadError::ActiveSetDidNotConverge`] — the tension-only active set
///   did not reach a fixed point within `options.max_active_set_iters` passes.
pub fn membrane_load_analysis(
    _nodes: &[[f64; 3]],
    _bar_members: &[BarMember],
    _membrane_patches: &[MembranePatch],
    _loads: &[[f64; 3]],
    _fixed_nodes: &[usize],
    _options: &MembraneLoadOptions,
) -> Result<MembraneLoadSolve, MembraneLoadError> {
    // pre-1 scaffold placeholder — the real single-pass core, validation guards,
    // bar coupling, and active-set loop land in steps 4 / 6 / 8 / 10 / 12.
    Err(MembraneLoadError::SingularSystem)
}

/// Recover the constant in-plane membrane stress delta `Δσ` (symmetric 2×2, in
/// the element local frame) for a flat three-node CST membrane patch under a
/// nodal displacement field.
///
/// `nodes` are the three physical corner positions (global coords); `material`
/// is the isotropic plane-stress law; `u_local_global` is the patch's 9-DOF
/// global nodal displacement `[u0x,u0y,u0z, u1x,u1y,u1z, u2x,u2y,u2z]`.
///
/// Built from the same primitives the ζ CST element uses: the local frame +
/// constant local shape gradients, the global→local displacement rotation, the
/// constant in-plane strain `ε = Σᵢ Bᵢ·uᵢ_local`, and `Δσ = plane_stress_d·ε`
/// (Voigt → 2×2). The recovery is **exact** for a constant-strain field. The
/// returned delta is thickness-independent (it is a stress, Pa).
pub fn membrane_stress_delta(
    _nodes: &[[f64; 3]; 3],
    _material: &IsotropicElastic,
    _u_local_global: &[f64; 9],
) -> [[f64; 2]; 2] {
    // pre-1 scaffold placeholder — the constant-strain recovery lands in step-2.
    [[0.0; 2]; 2]
}

/// Principal stresses `[min, max]` of a symmetric 2×2 stress tensor
/// `[[a, c], [c, b]]`.
///
/// Closed-form symmetric-2×2 eigenvalues: `(a+b)/2 ± sqrt(((a−b)/2)² + c²)`,
/// returned sorted `[min, max]`. Used by the tension-only active set's
/// membrane-slack test (a patch is slack when its minimum principal stress goes
/// compressive).
pub fn principal_stresses_2x2(_s: [[f64; 2]; 2]) -> [f64; 2] {
    // pre-1 scaffold placeholder — the closed-form eigenvalues land in step-2.
    [0.0, 0.0]
}
