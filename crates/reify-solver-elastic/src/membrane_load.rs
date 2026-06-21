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

use crate::assembly::bar::MIN_BAR_LENGTH;
use crate::assembly::{
    AssemblyElement, AssemblyMode, BarSection, ElementStiffness, assemble_global_stiffness,
};
use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination, apply_point_load};
use crate::constitutive::IsotropicElastic;
use crate::form_find::MemberKind;
use crate::geometric_stiffness::{MembranePrestress, bar_tangent_stiffness, membrane_tangent_stiffness};
use crate::shell_assembly::{build_shell_frame, plane_stress_d};
use crate::shell_kinematics::shell_kinematics;
use crate::solver::{CgSolverOptions, SolverMode, solve_cg};
use crate::tensegrity_load::BarMember;

/// Diagonal magnitude seeded at an orphan fixed node's DOFs so the Dirichlet
/// row-elimination has a stored diagonal to overwrite.
///
/// Physically inert: a grounded DOF belongs to a fixed support, so
/// `apply_dirichlet_row_elimination` unconditionally sets its diagonal to `1.0`
/// and pins its displacement to `0` regardless of what is seeded here. A unit
/// value keeps the pre-elimination matrix well-conditioned and survives any
/// sparse-builder zero-pruning. (Verbatim T3b discipline.)
const GROUNDING_DIAGONAL: f64 = 1.0;

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
    nodes: &[[f64; 3]],
    bar_members: &[BarMember],
    membrane_patches: &[MembranePatch],
    loads: &[[f64; 3]],
    fixed_nodes: &[usize],
    options: &MembraneLoadOptions,
) -> Result<MembraneLoadSolve, MembraneLoadError> {
    let n_nodes = nodes.len();
    let n_patches = membrane_patches.len();

    // ---- Up-front validation (never panic; never silently mis-solve) -------
    // The per-node external load vector must cover every node exactly.
    if loads.len() != n_nodes {
        return Err(MembraneLoadError::DimensionMismatch);
    }
    // Every bar endpoint and every patch corner must be an in-range node index
    // (else the assembly below would index out of bounds).
    for member in bar_members {
        let (j, k) = member.nodes;
        if j >= n_nodes || k >= n_nodes {
            return Err(MembraneLoadError::DimensionMismatch);
        }
    }
    for patch in membrane_patches {
        let (a, b, c) = patch.nodes;
        if a >= n_nodes || b >= n_nodes || c >= n_nodes {
            return Err(MembraneLoadError::DimensionMismatch);
        }
    }
    // Every support index must be in range; record the anchored set in one pass.
    let mut is_fixed = vec![false; n_nodes];
    for &node in fixed_nodes {
        if node >= n_nodes {
            return Err(MembraneLoadError::DimensionMismatch);
        }
        is_fixed[node] = true;
    }
    // There must be at least one free DOF: an all-anchored (or node-less) problem
    // has nothing to solve for.
    if n_nodes == 0 || is_fixed.iter().all(|&f| f) {
        return Err(MembraneLoadError::EmptyFreeSet);
    }
    // A free node touched by no bar and no patch has zero incident stiffness — a
    // singular / rigid-body tangent mode whose DOFs reach the global tangent with
    // no stored diagonal, tripping the inner CG Jacobi preconditioner's
    // unconditional missing-diagonal assert (a panic the trampoline cannot
    // catch). Reject it up-front as SingularSystem. Anchored orphans are fine —
    // they are pinned by Dirichlet BCs and grounded by the stabiliser below.
    // Endpoints/corners were range-checked above, so indexing `touched` is
    // in-bounds.
    let mut touched = vec![false; n_nodes];
    for member in bar_members {
        let (j, k) = member.nodes;
        touched[j] = true;
        touched[k] = true;
    }
    for patch in membrane_patches {
        let (a, b, c) = patch.nodes;
        touched[a] = true;
        touched[b] = true;
        touched[c] = true;
    }
    for node in 0..n_nodes {
        if !is_fixed[node] && !touched[node] {
            return Err(MembraneLoadError::SingularSystem);
        }
    }

    // Tension-only active set: start with every bar + patch active and drop any
    // active cable whose total force goes compressive AND any active patch whose
    // minimum principal stress goes compressive, re-solving until a pass drops
    // nothing. The drop is monotone (a slack cable/patch is never re-added), so
    // the active set strictly shrinks and the loop terminates in at most
    // `#cables + #patches` passes. K_g is held *linear-about-prestress*: every
    // pass builds K_g from the fixed form-found `member.prestress` / `σ₀·t`, never
    // the load-updated state (PRD §5/§10), so the converged post-drop deflection
    // equals the reduced linear system with the slack elements removed.
    let n_members = bar_members.len();
    let mut active_bars = vec![true; n_members];
    let mut active_patches = vec![true; n_patches];
    let mut member_slack = vec![false; n_members];
    let mut surface_slack = vec![false; n_patches];
    let mut iterations = 0usize;

    let displacements = loop {
        iterations += 1;
        let (disp, conv) = solve_active_pass(
            nodes,
            bar_members,
            membrane_patches,
            &active_bars,
            &active_patches,
            loads,
            fixed_nodes,
            &is_fixed,
            options,
        )?;
        // A pass whose inner CG did not converge ⇒ a singular / ill-conditioned
        // reduced tangent system. Surface it rather than a silently-wrong field.
        if !conv {
            return Err(MembraneLoadError::SingularSystem);
        }

        let mut dropped_any = false;
        // Drop any active cable that has gone compressive (struts carry
        // compression and are never dropped).
        for (i, member) in bar_members.iter().enumerate() {
            if !active_bars[i] || member.kind != MemberKind::Cable {
                continue;
            }
            let total = member.prestress + member_force_delta(nodes, member, &disp);
            if total < -options.slack_tol {
                active_bars[i] = false;
                member_slack[i] = true;
                dropped_any = true;
            }
        }
        // Drop any active membrane patch whose minimum principal stress of the
        // total stress σ_total = σ₀·I + Δσ has gone compressive (a wrinkled/slack
        // membrane carries no compression — the 2-D analogue of the slack cable).
        for (i, patch) in membrane_patches.iter().enumerate() {
            if !active_patches[i] {
                continue;
            }
            let dsig = patch_stress_delta(nodes, patch, &disp);
            let total = [
                [patch.prestress + dsig[0][0], dsig[0][1]],
                [dsig[1][0], patch.prestress + dsig[1][1]],
            ];
            if principal_stresses_2x2(total)[0] < -options.slack_tol {
                active_patches[i] = false;
                surface_slack[i] = true;
                dropped_any = true;
            }
        }

        if !dropped_any {
            // Fixed point: no active cable/patch went slack this pass.
            break disp;
        }

        // §11 Q5 defensive cap. Drop-only monotonicity guarantees a fixed point in
        // ≤ #cables + #patches passes, so reaching the cap means a bug (or a future
        // non-monotone reactivation policy) is cycling — surface the diagnostic
        // instead of spinning. Checked only after a pass that dropped an element,
        // so a problem whose natural count equals the cap still converges.
        if iterations >= options.max_active_set_iters {
            return Err(MembraneLoadError::ActiveSetDidNotConverge { iterations });
        }
    };

    // Final per-line-member forces: slack cables report 0 with delta = −prestress
    // (their total force fell to zero); active members report prestress + dN on the
    // converged displacement field.
    let mut member_forces = vec![0.0_f64; n_members];
    let mut member_force_deltas = vec![0.0_f64; n_members];
    for (i, member) in bar_members.iter().enumerate() {
        if member_slack[i] {
            member_forces[i] = 0.0;
            member_force_deltas[i] = -member.prestress;
        } else {
            let dn = member_force_delta(nodes, member, &displacements);
            member_force_deltas[i] = dn;
            member_forces[i] = member.prestress + dn;
        }
    }

    // Final per-patch stresses: slack patches carry nothing — total stress 0, so
    // the reported delta is −σ₀·I (cancelling the prestress) and both principals
    // are 0 (the 2-D analogue of T3b's slack-cable 0-force / −prestress-delta
    // report). Active patches report the recovered Δσ + principals of
    // σ_total = σ₀·I + Δσ on the converged displacement field (real f64 by
    // construction — the G6 field-population invariant).
    let mut surface_stress_deltas = Vec::with_capacity(n_patches);
    let mut surface_principal_stresses = Vec::with_capacity(n_patches);
    for (i, patch) in membrane_patches.iter().enumerate() {
        if surface_slack[i] {
            surface_stress_deltas.push([[-patch.prestress, 0.0], [0.0, -patch.prestress]]);
            surface_principal_stresses.push([0.0, 0.0]);
        } else {
            let dsig = patch_stress_delta(nodes, patch, &displacements);
            let total = [
                [patch.prestress + dsig[0][0], dsig[0][1]],
                [dsig[1][0], patch.prestress + dsig[1][1]],
            ];
            surface_stress_deltas.push(dsig);
            surface_principal_stresses.push(principal_stresses_2x2(total));
        }
    }

    Ok(MembraneLoadSolve {
        displacements,
        member_forces,
        member_force_deltas,
        member_slack,
        surface_stress_deltas,
        surface_principal_stresses,
        surface_slack,
        active_set_iterations: iterations,
        converged: true,
    })
}

/// One linear solve over the currently-active bars + patches.
///
/// Builds the global tangent stiffness from each active patch's
/// [`membrane_tangent_stiffness`] and each active bar's [`bar_tangent_stiffness`]
/// (both `K_t = K_e + K_g`, scattered through the unchanged
/// [`assemble_global_stiffness`] into ONE system), applies the per-node external
/// loads, pins every `fixed_nodes` support in all three axes via homogeneous
/// Dirichlet BCs, and solves the reduced system with CG. Returns the per-node
/// displacement field and the CG convergence flag. The tension-only active set
/// calls it once per pass with a shrinking active set.
///
/// Returns [`MembraneLoadError::SingularSystem`] if any free (non-`is_fixed`) node
/// is touched by no currently-active bar or patch — a zero-stiffness, rigid-body
/// tangent mode (the *dynamic* case where the active set just dropped a node's
/// last element) that would otherwise panic the inner CG Jacobi preconditioner on
/// a missing diagonal. Unconnected *fixed* nodes are not orphans: they are pinned
/// by Dirichlet BCs and grounded by the stabiliser below.
#[allow(clippy::too_many_arguments)]
fn solve_active_pass(
    nodes: &[[f64; 3]],
    bar_members: &[BarMember],
    membrane_patches: &[MembranePatch],
    active_bars: &[bool],
    active_patches: &[bool],
    loads: &[[f64; 3]],
    fixed_nodes: &[usize],
    is_fixed: &[bool],
    options: &MembraneLoadOptions,
) -> Result<(Vec<[f64; 3]>, bool), MembraneLoadError> {
    let n_nodes = nodes.len();

    // Per-active-element connectivity + tangent stiffness, plus the 1-node
    // grounding stabilisers appended below. `conns` is heterogeneous (3-corner
    // patches, 2-node bars, 1-node grounders), so it owns `Vec<usize>`
    // connectivity; both Vecs outlive the `AssemblyElement` borrows.
    let mut conns: Vec<Vec<usize>> = Vec::new();
    let mut k_mats: Vec<ElementStiffness> = Vec::new();
    let mut connected = vec![false; n_nodes];
    for (p, patch) in membrane_patches.iter().enumerate() {
        if !active_patches[p] {
            continue;
        }
        let (a, b, c) = patch.nodes;
        connected[a] = true;
        connected[b] = true;
        connected[c] = true;
        conns.push(vec![a, b, c]);
        k_mats.push(membrane_tangent_stiffness(
            &[nodes[a], nodes[b], nodes[c]],
            patch.thickness,
            &patch.material,
            &MembranePrestress::isotropic(patch.prestress * patch.thickness),
        ));
    }
    for (m, member) in bar_members.iter().enumerate() {
        if !active_bars[m] {
            continue;
        }
        let (j, k) = member.nodes;
        connected[j] = true;
        connected[k] = true;
        conns.push(vec![j, k]);
        k_mats.push(bar_tangent_stiffness(
            &[nodes[j], nodes[k]],
            &member.section,
            member.prestress,
        ));
    }

    // Per-pass free-orphan guard (dynamic). A free node touched by no currently-
    // ACTIVE bar or patch carries zero incident stiffness this pass — a singular /
    // rigid-body tangent mode. This is the case a mid-solve active-set drop creates
    // (a node connected at problem setup, so it clears the up-front guard, but
    // disconnected here). Catch it before `assemble_global_stiffness` / `solve_cg`,
    // whose Jacobi preconditioner asserts on the missing diagonal and would panic.
    for node in 0..n_nodes {
        if !connected[node] && !is_fixed[node] {
            return Err(MembraneLoadError::SingularSystem);
        }
    }

    // Grounding stabilisers for orphan FIXED nodes — support nodes that no active
    // bar/patch touches (e.g. a dropped patch's anchors). `apply_dirichlet_row_
    // elimination` requires a stored diagonal at every constrained DOF, but an
    // orphan node contributes no stiffness, so the BC pass would otherwise panic on
    // a missing `K[i][i]`. Each gets a 1-node element seeding a diagonal at its
    // three DOFs — physically inert (row-elimination overwrites it with 1.0 and
    // pins the displacement to 0).
    for &node in fixed_nodes {
        if !connected[node] {
            let mut ground = ElementStiffness::zeros(3);
            ground.data[0] = GROUNDING_DIAGONAL; // (0, 0)
            ground.data[4] = GROUNDING_DIAGONAL; // (1, 1)
            ground.data[8] = GROUNDING_DIAGONAL; // (2, 2)
            conns.push(vec![node]);
            k_mats.push(ground);
        }
    }

    let elements: Vec<AssemblyElement<'_>> = conns
        .iter()
        .zip(k_mats.iter())
        .enumerate()
        .map(|(id, (conn, kt))| AssemblyElement {
            id,
            connectivity: conn.as_slice(),
            k_e: kt,
        })
        .collect();

    let mut k_global = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

    // External nodal loads.
    let mut f = vec![0.0_f64; 3 * n_nodes];
    for (node, &force) in loads.iter().enumerate() {
        apply_point_load(&mut f, node, force);
    }

    // Each fixed support node → 3 homogeneous Dirichlet BCs (all axes pinned).
    let mut bcs: Vec<DirichletBc> = Vec::with_capacity(3 * fixed_nodes.len());
    for &node in fixed_nodes {
        for axis in 0..3 {
            bcs.push(DirichletBc { dof: 3 * node + axis, value: 0.0 });
        }
    }
    apply_dirichlet_row_elimination(&mut k_global, &mut f, &bcs);

    let result = solve_cg(&k_global, &f, options.cg.clone(), SolverMode::Deterministic);

    // Scatter the flat displacement vector into per-node [x, y, z].
    let u = result.u();
    let mut displacements = vec![[0.0_f64; 3]; n_nodes];
    for (node, d) in displacements.iter_mut().enumerate() {
        *d = [u[3 * node], u[3 * node + 1], u[3 * node + 2]];
    }

    Ok((displacements, result.converged))
}

/// Constant in-plane membrane stress delta `Δσ` (symmetric 2×2, local frame) for
/// `patch` evaluated on a displacement field. Gathers the patch's 9-DOF global
/// nodal displacement and defers to [`membrane_stress_delta`].
fn patch_stress_delta(
    nodes: &[[f64; 3]],
    patch: &MembranePatch,
    displacements: &[[f64; 3]],
) -> [[f64; 2]; 2] {
    let (a, b, c) = patch.nodes;
    let u9 = [
        displacements[a][0], displacements[a][1], displacements[a][2],
        displacements[b][0], displacements[b][1], displacements[b][2],
        displacements[c][0], displacements[c][1], displacements[c][2],
    ];
    membrane_stress_delta(&[nodes[a], nodes[b], nodes[c]], &patch.material, &u9)
}

/// Axial force delta `dN` for `member` evaluated on a displacement field.
///
/// Gathers the member's two nodal displacements into the local 6-vector and
/// defers to [`bar_axial_force_delta`]. The total member force is
/// `prestress + dN`. Ported from T3b's `tensegrity_load` line-member half.
fn member_force_delta(nodes: &[[f64; 3]], member: &BarMember, displacements: &[[f64; 3]]) -> f64 {
    let (j, k) = member.nodes;
    let u_local = [
        displacements[j][0],
        displacements[j][1],
        displacements[j][2],
        displacements[k][0],
        displacements[k][1],
        displacements[k][2],
    ];
    bar_axial_force_delta(&[nodes[j], nodes[k]], &member.section, &u_local)
}

/// First-order axial member-force delta for a 2-node bar/cable element.
///
/// With unit direction cosine `c = (node1 − node0) / L`, cross-section `(E, A)`,
/// and element nodal displacement `u_local = [u0x,u0y,u0z, u1x,u1y,u1z]`,
///
/// ```text
/// dN = (E·A / L) · c · (u1 − u0)
/// ```
///
/// the axial projection of the relative tip displacement: a purely transverse
/// relative displacement and a rigid-body translation both contribute nothing.
/// The total member force is `N = prestress + dN`. Ported verbatim from T3b
/// (`tensegrity_load::bar_axial_force_delta`), sharing the `MIN_BAR_LENGTH`
/// degeneracy-guard convention of `element_stiffness_bar_p1`.
fn bar_axial_force_delta(
    phys_nodes: &[[f64; 3]; 2],
    section: &BarSection,
    u_local: &[f64; 6],
) -> f64 {
    debug_assert!(
        section.youngs_modulus.is_finite() && section.youngs_modulus > 0.0,
        "youngs_modulus must be finite and positive, got {}",
        section.youngs_modulus,
    );
    debug_assert!(
        section.area.is_finite() && section.area > 0.0,
        "area must be finite and positive, got {}",
        section.area,
    );

    let r = [
        phys_nodes[1][0] - phys_nodes[0][0],
        phys_nodes[1][1] - phys_nodes[0][1],
        phys_nodes[1][2] - phys_nodes[0][2],
    ];
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();

    debug_assert!(
        l > MIN_BAR_LENGTH,
        "degenerate bar: L = {} (must be > {})",
        l,
        MIN_BAR_LENGTH,
    );

    let c = [r[0] / l, r[1] / l, r[2] / l];
    // Relative tip displacement du = u1 − u0 (the only part that strains the bar).
    let du = [
        u_local[3] - u_local[0],
        u_local[4] - u_local[1],
        u_local[5] - u_local[2],
    ];
    let c_dot_du = c[0] * du[0] + c[1] * du[1] + c[2] * du[2];
    section.youngs_modulus * section.area / l * c_dot_du
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
    nodes: &[[f64; 3]; 3],
    material: &IsotropicElastic,
    u_local_global: &[f64; 9],
) -> [[f64; 2]; 2] {
    // Build the local mid-surface frame + constant local shape gradients once.
    // These are the *same* primitives the ζ CST element K_e uses
    // (`element_stiffness_membrane_cst`), so the strain recovered here is
    // consistent with the assembled stiffness. `build_shell_frame` also guards a
    // degenerate (collinear/zero-edge) triangle.
    let frame = build_shell_frame(nodes);
    let dn = shell_kinematics(nodes, &frame).dn;
    let r = &frame.r;

    // Constant in-plane strain ε = [εxx, εyy, γxy] = Σᵢ Bᵢ·uᵢ_local, where each
    // node's global displacement is rotated into the local frame
    // (u_local = R·u_global; the origin offset cancels for a displacement) and
    // only the in-plane (x, y) components feed the CST strain-displacement
    // matrix Bᵢ = [[dn_ix, 0], [0, dn_iy], [dn_iy, dn_ix]]:
    //   Bᵢ·[ulx, uly] = [dn_ix·ulx, dn_iy·uly, dn_iy·ulx + dn_ix·uly].
    let mut eps = [0.0_f64; 3];
    for i in 0..3 {
        let ug = [
            u_local_global[3 * i],
            u_local_global[3 * i + 1],
            u_local_global[3 * i + 2],
        ];
        // Local in-plane displacement components (rows e1, e2 of R).
        let ulx = r[0][0] * ug[0] + r[0][1] * ug[1] + r[0][2] * ug[2];
        let uly = r[1][0] * ug[0] + r[1][1] * ug[1] + r[1][2] * ug[2];
        let (dnx, dny) = (dn[i][0], dn[i][1]);
        eps[0] += dnx * ulx;
        eps[1] += dny * uly;
        eps[2] += dny * ulx + dnx * uly;
    }

    // Δσ_voigt = D_pl·ε (plane stress), Voigt order [σxx, σyy, σxy] — the exact
    // companion of the t·D_pl used by the element K_e (thickness-independent: a
    // stress, Pa). Map Voigt → the symmetric 2×2 [[σxx, σxy], [σxy, σyy]].
    let d = plane_stress_d(material);
    let sxx = d[0][0] * eps[0] + d[0][1] * eps[1] + d[0][2] * eps[2];
    let syy = d[1][0] * eps[0] + d[1][1] * eps[1] + d[1][2] * eps[2];
    let sxy = d[2][0] * eps[0] + d[2][1] * eps[1] + d[2][2] * eps[2];
    [[sxx, sxy], [sxy, syy]]
}

/// Principal stresses `[min, max]` of a symmetric 2×2 stress tensor
/// `[[a, c], [c, b]]`.
///
/// Closed-form symmetric-2×2 eigenvalues: `(a+b)/2 ± sqrt(((a−b)/2)² + c²)`,
/// returned sorted `[min, max]`. Used by the tension-only active set's
/// membrane-slack test (a patch is slack when its minimum principal stress goes
/// compressive).
pub fn principal_stresses_2x2(s: [[f64; 2]; 2]) -> [f64; 2] {
    let a = s[0][0];
    let b = s[1][1];
    // Symmetric off-diagonal: average the two stored entries so a slightly
    // off-symmetric input is treated as its symmetric part (the membrane stress
    // tensors are symmetric by construction, so this is a no-op there).
    let c = 0.5 * (s[0][1] + s[1][0]);
    let mean = 0.5 * (a + b);
    let half_diff = 0.5 * (a - b);
    let radius = (half_diff * half_diff + c * c).sqrt();
    [mean - radius, mean + radius]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::test_support::assert_close;

    /// ν = 0 plane-stress material ⇒ closed-form `D_pl = diag(E, E, E/2)`, so the
    /// recovered delta has the hand-checkable form `σxx = E·εxx`, `σyy = E·εyy`,
    /// `σxy = (E/2)·γxy` (no ν cross-coupling). Same material the ζ CST element
    /// patch test uses.
    fn nu_zero_material(e: f64) -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: 0.0,
        }
    }

    /// Unit triangle in the xy-plane: `R = I`, `dn = [(-1,-1), (1,0), (0,1)]`.
    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    /// Apply a 3×3 rotation `q` to a global 3-vector (tilt a flat triangle / its
    /// displacement field out of the xy-plane).
    fn apply_q(q: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
        [
            q[0][0] * v[0] + q[0][1] * v[1] + q[0][2] * v[2],
            q[1][0] * v[0] + q[1][1] * v[1] + q[1][2] * v[2],
            q[2][0] * v[0] + q[2][1] * v[1] + q[2][2] * v[2],
        ]
    }

    /// Entrywise-close assertion for a symmetric 2×2 stress tensor.
    fn assert_tensor2_close(got: [[f64; 2]; 2], want: [[f64; 2]; 2], tol: f64, label: &str) {
        for i in 0..2 {
            for j in 0..2 {
                assert_close(got[i][j], want[i][j], tol, &format!("{label}[{i}][{j}]"));
            }
        }
    }

    // (a) zero displacement ⇒ Δσ is identically zero.
    #[test]
    fn membrane_stress_delta_zero_disp_is_zero() {
        let mat = nu_zero_material(1000.0);
        let ds = membrane_stress_delta(&UNIT_TRI, &mat, &[0.0; 9]);
        assert_tensor2_close(ds, [[0.0; 2]; 2], 1e-12, "Δσ(zero u)");
    }

    // (b) Constant-strain patch test on the flat unit triangle. The linear field
    //     `u_x = εxx·x + γ·y`, `u_y = εyy·y` has constant strain
    //     `ε = [εxx, εyy, γ]`; with ν = 0 the recovery is
    //     `Δσ = [[E·εxx, (E/2)·γ], [(E/2)·γ, E·εyy]]`. The recovery is EXACT for a
    //     constant strain (it lives in the CST space — the same identity ζ's
    //     element_stiffness_membrane_cst patch test validates), so the
    //     hand-computed closed form is matched at 1e-12. With E = 1000,
    //     εxx = 1e-3, εyy = 2e-3, γ = 3e-3 ⇒ Δσ = [[1.0, 1.5], [1.5, 2.0]].
    #[test]
    fn membrane_stress_delta_constant_strain_patch_test() {
        let e = 1000.0_f64;
        let mat = nu_zero_material(e);
        let (exx, eyy, gam) = (0.001_f64, 0.002_f64, 0.003_f64);
        // Nodal global displacement (R = I ⇒ local == global xy):
        //   u0 = (0, 0), u1 = (εxx, 0), u2 = (γ, εyy).
        let u = [0.0, 0.0, 0.0, exx, 0.0, 0.0, gam, eyy, 0.0];
        let ds = membrane_stress_delta(&UNIT_TRI, &mat, &u);
        let want = [[e * exx, 0.5 * e * gam], [0.5 * e * gam, e * eyy]];
        assert_tensor2_close(ds, want, 1e-12, "Δσ(patch)");
        // Pin the hand numbers so a wrong D/strain wiring is obvious.
        assert_tensor2_close(want, [[1.0, 1.5], [1.5, 2.0]], 1e-12, "want hand-values");
    }

    // (c) A tilted (out-of-xy-plane) triangle carrying the rotated constant-strain
    //     field recovers the SAME local Δσ — exercising the `frame.r` global→local
    //     rotation. Tilting the nodes by Q gives frame `R' = Qᵀ`; rotating the
    //     global displacement by the same Q makes `u_i_local' = Qᵀ·Q·u_i = u_i`,
    //     so the local strain — and Δσ — are identical to the flat case.
    #[test]
    fn membrane_stress_delta_tilted_recovers_same_local_delta() {
        let e = 1000.0_f64;
        let mat = nu_zero_material(e);
        let (exx, eyy, gam) = (0.001_f64, 0.002_f64, 0.003_f64);
        let q = crate::shell_assembly::tilted_q_for_shell_tests();
        let tilted = [
            apply_q(&q, UNIT_TRI[0]),
            apply_q(&q, UNIT_TRI[1]),
            apply_q(&q, UNIT_TRI[2]),
        ];
        // Global displacement at each node = Q · (flat global displacement).
        let u0 = apply_q(&q, [0.0, 0.0, 0.0]);
        let u1 = apply_q(&q, [exx, 0.0, 0.0]);
        let u2 = apply_q(&q, [gam, eyy, 0.0]);
        let u = [
            u0[0], u0[1], u0[2], u1[0], u1[1], u1[2], u2[0], u2[1], u2[2],
        ];
        let ds = membrane_stress_delta(&tilted, &mat, &u);
        // Same local Δσ as the flat patch test (rotation introduces only rounding).
        assert_tensor2_close(ds, [[1.0, 1.5], [1.5, 2.0]], 1e-9, "Δσ(tilted)==Δσ(flat)");
    }

    // (d) principal_stresses_2x2 on known symmetric 2×2 tensors (eigenvalues
    //     hand-checked), returned sorted `[min, max]`.
    #[test]
    fn principal_stresses_2x2_hand_checked() {
        // [[3, 1], [1, 3]] ⇒ 3 ± 1 = {2, 4}.
        let p = principal_stresses_2x2([[3.0, 1.0], [1.0, 3.0]]);
        assert_close(p[0], 2.0, 1e-12, "min eig [[3,1],[1,3]]");
        assert_close(p[1], 4.0, 1e-12, "max eig [[3,1],[1,3]]");
        // Diagonal [[2, 0], [0, 5]] ⇒ {2, 5} (already sorted by axis).
        let p = principal_stresses_2x2([[2.0, 0.0], [0.0, 5.0]]);
        assert_close(p[0], 2.0, 1e-12, "min eig diag(2,5)");
        assert_close(p[1], 5.0, 1e-12, "max eig diag(2,5)");
        // [[1, 2], [2, 1]] ⇒ 1 ± 2 = {−1, 3}: a compressive min principal (the
        // membrane-slack trigger the active set keys on).
        let p = principal_stresses_2x2([[1.0, 2.0], [2.0, 1.0]]);
        assert_close(p[0], -1.0, 1e-12, "min eig [[1,2],[2,1]] (compressive)");
        assert_close(p[1], 3.0, 1e-12, "max eig [[1,2],[2,1]]");
    }

    // ---- step-5: up-front validation / orphan guards (mirror T3b) ----------

    use crate::assembly::BarSection;
    use crate::form_find::MemberKind;
    use crate::solver::CgSolverOptions;

    /// A flat unit-triangle membrane patch on the given corner indices.
    fn patch(a: usize, b: usize, c: usize) -> MembranePatch {
        MembranePatch {
            nodes: (a, b, c),
            thickness: 0.01,
            material: nu_zero_material(1.0e6),
            prestress: 1000.0,
        }
    }

    /// A cable [`BarMember`] joining `j`–`k` (the line-member half is wired in
    /// step-8; here it only exercises the step-6 range check).
    fn cable(j: usize, k: usize) -> BarMember {
        BarMember {
            nodes: (j, k),
            kind: MemberKind::Cable,
            section: BarSection {
                youngs_modulus: 200.0e9,
                area: 1.0e-4,
            },
            prestress: 5_000.0,
        }
    }

    /// Tight inner-CG guard options with a caller-chosen active-set cap.
    fn guard_options(max_active_set_iters: usize) -> MembraneLoadOptions {
        MembraneLoadOptions {
            max_active_set_iters,
            cg: CgSolverOptions {
                tolerance: 1.0e-12,
                max_iter: 1000,
            },
            slack_tol: 0.0,
        }
    }

    /// Unit-triangle node set (corners 0,1,2) shared by the guard problems.
    const TRI_NODES: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    // (a) `loads.len()` must equal `nodes.len()`: a short loads vector is a
    //     DimensionMismatch (validated up-front, not silently under-applied).
    #[test]
    fn dimension_mismatch_loads_length() {
        let nodes = TRI_NODES.to_vec();
        let patches = vec![patch(0, 1, 2)];
        let loads = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]; // 2 ≠ 3 nodes
        let fixed = vec![0, 1];
        let result =
            membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(64));
        assert!(
            matches!(result, Err(MembraneLoadError::DimensionMismatch)),
            "loads.len() != nodes.len() must be DimensionMismatch, got {result:?}",
        );
    }

    // (b) A patch corner index outside `0..nodes.len()` is a DimensionMismatch,
    //     caught up-front before any `nodes[idx]` assembly indexing (which would
    //     otherwise panic).
    #[test]
    fn dimension_mismatch_patch_corner_out_of_range() {
        let nodes = TRI_NODES.to_vec();
        let patches = vec![patch(0, 1, 5)]; // corner 5 ∉ 0..3
        let loads = vec![[0.0, 0.0, 0.0]; 3];
        let fixed = vec![0, 1];
        let result =
            membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(64));
        assert!(
            matches!(result, Err(MembraneLoadError::DimensionMismatch)),
            "out-of-range patch corner must be DimensionMismatch, got {result:?}",
        );
    }

    // (b′) A bar endpoint outside `0..nodes.len()` is likewise a DimensionMismatch
    //      (the step-6 range check covers bars + patches symmetrically).
    #[test]
    fn dimension_mismatch_bar_endpoint_out_of_range() {
        let nodes = TRI_NODES.to_vec();
        let patches = vec![patch(0, 1, 2)];
        let bars = vec![cable(0, 9)]; // endpoint 9 ∉ 0..3
        let loads = vec![[0.0, 0.0, 0.0]; 3];
        let fixed = vec![0, 1];
        let result =
            membrane_load_analysis(&nodes, &bars, &patches, &loads, &fixed, &guard_options(64));
        assert!(
            matches!(result, Err(MembraneLoadError::DimensionMismatch)),
            "out-of-range bar endpoint must be DimensionMismatch, got {result:?}",
        );
    }

    // (c) Every node fixed ⇒ no free DOF to solve for ⇒ EmptyFreeSet.
    #[test]
    fn empty_free_set_all_nodes_fixed() {
        let nodes = TRI_NODES.to_vec();
        let patches = vec![patch(0, 1, 2)];
        let loads = vec![[0.0, 0.0, 0.0]; 3];
        let fixed = vec![0, 1, 2]; // all (every) node anchored
        let result =
            membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(64));
        assert!(
            matches!(result, Err(MembraneLoadError::EmptyFreeSet)),
            "all-fixed problem must be EmptyFreeSet, got {result:?}",
        );
    }

    // (d) A free node referenced by no patch and no bar and absent from
    //     `fixed_nodes` has zero incident stiffness — its DOFs reach the CG
    //     Jacobi preconditioner with no stored diagonal and would PANIC. The
    //     kernel must reject it up-front as SingularSystem (never panic).
    //     Topology: patch (0,1,2) with nodes 0,1 fixed (node 2 free + touched);
    //     node 3 a FREE ORPHAN at (5,5,0) touched by nothing.
    #[test]
    fn singular_system_free_orphan_node() {
        let nodes = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [5.0, 5.0, 0.0], // FREE ORPHAN — no patch/bar, not fixed
        ];
        let patches = vec![patch(0, 1, 2)];
        let loads = vec![[0.0, 0.0, 0.0]; 4];
        let fixed = vec![0, 1]; // node 3 deliberately left free
        let result =
            membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(64));
        assert!(
            matches!(result, Err(MembraneLoadError::SingularSystem)),
            "a free node with no incident patch/bar must be SingularSystem (not a panic), \
             got {result:?}",
        );
    }

    // ---- step-11: active-set iteration cap (§11 Q5 defensive guard) ---------

    /// A constructed kernel problem: `(nodes, patches, loads, fixed_nodes)`.
    type KernelProblem = (Vec<[f64; 3]>, Vec<MembranePatch>, Vec<[f64; 3]>, Vec<usize>);

    /// A two-patch slack configuration (mirrors the kernel golden
    /// `membrane_slack_active_set_drop`): the free center node `F = 2` is held by a
    /// left patch `A = (2,0,1)` and a right patch `B = (2,3,4)`, symmetric about the
    /// x-axis. A `+x` in-plane load STRETCHES `A` (stays taut) and COMPRESSES `B`
    /// (its minimum principal stress goes below zero ⇒ slack). The natural
    /// active-set count is exactly TWO passes: pass 1 drops `B`, pass 2 confirms the
    /// fixed point.
    fn slack_two_patch_problem() -> KernelProblem {
        let (sigma, t, e, p) = (1.0_f64, 1.0_f64, 100.0_f64, 5.0_f64);
        let nodes = vec![
            [-1.0, 0.5, 0.0],  // 0 — A anchor
            [-1.0, -0.5, 0.0], // 1 — A anchor
            [0.0, 0.0, 0.0],   // 2 — F (free center)
            [1.0, 0.5, 0.0],   // 3 — B anchor
            [1.0, -0.5, 0.0],  // 4 — B anchor
        ];
        let mk = |a, b, c| MembranePatch {
            nodes: (a, b, c),
            thickness: t,
            material: nu_zero_material(e),
            prestress: sigma,
        };
        let patches = vec![mk(2, 0, 1), mk(2, 3, 4)];
        let mut loads = vec![[0.0, 0.0, 0.0]; nodes.len()];
        loads[2] = [p, 0.0, 0.0];
        let fixed = vec![0, 1, 3, 4];
        (nodes, patches, loads, fixed)
    }

    // (step-11) Capping `max_active_set_iters` below the natural drop count of a
    // slackening problem surfaces `ActiveSetDidNotConverge { iterations }` (the PRD
    // §11 Q5 defensive guard) rather than returning a half-converged field. The
    // two-patch problem naturally needs TWO passes (drop B, then confirm the fixed
    // point); capping at ONE ⇒ the guard fires right after pass 1's drop with
    // `iterations: 1`.
    #[test]
    fn active_set_cap_below_natural_count_does_not_converge() {
        let (nodes, patches, loads, fixed) = slack_two_patch_problem();
        // Cap 1 < natural count 2 ⇒ the guard fires after the first dropping pass.
        let result =
            membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(1));
        assert!(
            matches!(
                result,
                Err(MembraneLoadError::ActiveSetDidNotConverge { iterations: 1 }),
            ),
            "cap below the natural drop count must surface \
             ActiveSetDidNotConverge {{ iterations: 1 }}, got {result:?}",
        );
    }

    // (step-11, control) A cap AT (or above) the natural count lets the SAME problem
    // converge — proving the error above is the §11 Q5 cap firing on an
    // otherwise-feasible problem (natural count 2), not an unrelated infeasibility.
    // Pass 1 drops B, pass 2 confirms the fixed point ⇒ converged, iterations == 2,
    // patch B slack.
    #[test]
    fn active_set_cap_at_natural_count_converges() {
        let (nodes, patches, loads, fixed) = slack_two_patch_problem();
        let solve = membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed, &guard_options(2))
            .expect("cap == natural count (2) must converge");
        assert!(solve.converged, "solve converges at cap == natural count");
        assert_eq!(solve.active_set_iterations, 2, "natural count is two passes");
        assert_eq!(
            solve.surface_slack,
            vec![false, true],
            "patch B slackens, patch A stays taut",
        );
    }
}
