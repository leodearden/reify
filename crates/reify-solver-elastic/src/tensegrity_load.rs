//! Tensegrity load analysis with a tension-only active set (Tensegrity T3b).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` §6 / Tier-3 leaf T3b. This is the
//! pure numeric kernel behind the dedicated `solver::tensegrity_load`
//! ComputeNode target: given a form-found geometry, per-member prestress, bar/
//! cable sections, external nodal loads, and a set of fixed support nodes, it
//! assembles the per-member tangent stiffness `K_t = K_e + K_g`, solves the
//! linear system via the existing CG path, and reports nodal deflections plus
//! per-member force deltas. A tension-only active-set wrapper drops slack
//! cables (cables whose total force goes compressive) and re-solves to a fixed
//! point.
//!
//! # Method
//!
//! For each active member the per-member tangent stiffness
//! [`crate::bar_tangent_stiffness`] is scattered into the global sparse
//! stiffness via [`crate::assemble_global_stiffness`]; external loads are
//! applied with [`crate::apply_point_load`]; each fixed node expands to three
//! homogeneous Dirichlet BCs applied via
//! [`crate::apply_dirichlet_row_elimination`]; the reduced system is solved
//! with [`crate::solve_cg`]. This mirrors the single-bar
//! `tests/bar_axial_deflection.rs` pattern, generalised to `N` members and
//! supports.
//!
//! The member-force delta is `dNᵢ = (Eᵢ Aᵢ / Lᵢ) · cᵢ · (u_k − u_j)` and the
//! total member force is `Nᵢ = prestressᵢ + dNᵢ`. The tension-only active set
//! drops any active cable whose total force is compressive (`Nᵢ < −slack_tol`)
//! and re-solves; the drop is monotone (a dropped cable is never re-added
//! within a solve), so the active set strictly shrinks and the loop terminates
//! in at most `#cables` passes. The geometric stiffness `K_g` is held
//! *linear-about-prestress* (it uses the fixed form-found `N`, not the
//! load-updated force, per PRD §10), so the converged post-drop deflection is
//! exactly the reduced linear system with the slack cable removed.
//!
//! # Scope
//!
//! Load analysis on a supplied form-found geometry + prestress only. Re-running
//! form-finding, geometrically-nonlinear / force-updated `K_g`, and per-member
//! section marshalling beyond a single shared `(E, A)` are out of scope (PRD
//! §10 future work / the trampoline's v1 shared-section decision).

use crate::assembly::bar::MIN_BAR_LENGTH;
use crate::assembly::{
    AssemblyElement, AssemblyMode, BarSection, ElementStiffness, assemble_global_stiffness,
};
use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination, apply_point_load};
use crate::form_find::MemberKind;
use crate::geometric_stiffness::bar_tangent_stiffness;
use crate::solver::{CgSolverOptions, SolverMode, solve_cg};

/// A single pin-jointed bar or cable member in a tensegrity load problem.
///
/// The kernel keeps a general per-member [`BarSection`] so per-member section
/// marshalling is a clean additive extension; the v1 trampoline broadcasts a
/// single shared `(E, A)` across members.
pub struct BarMember {
    /// Global node indices `(start, end)` of the member's two endpoints.
    pub nodes: (usize, usize),
    /// Member kind tag. Only [`MemberKind::Cable`] members may be dropped
    /// (slackened) by the tension-only active set; [`MemberKind::Strut`]
    /// members carry compression and are never dropped.
    pub kind: MemberKind,
    /// Cross-section properties (`E`, `A`) for the elastic/tangent stiffness.
    pub section: BarSection,
    /// Pre-existing form-found member force `N` (tension positive, compression
    /// negative). Seeds the geometric stiffness `K_g` and the slack test.
    pub prestress: f64,
}

/// Tuning knobs for [`tensegrity_load_analysis`].
#[derive(Debug, Clone)]
pub struct TensegrityLoadOptions {
    /// Hard cap on tension-only active-set passes. Drop-only monotonicity
    /// guarantees a fixed point in at most `#cables` passes, so exceeding this
    /// cap surfaces [`TensegrityLoadError::ActiveSetDidNotConverge`] (the PRD
    /// §11 Q5 defensive guard) rather than spinning.
    pub max_active_set_iters: usize,
    /// Inner linear-solve (CG) options used for each active-set pass.
    pub cg: CgSolverOptions,
    /// Slack tolerance: an active cable is dropped (marked slack) when its
    /// total force is `< −slack_tol`. A small positive value tolerates
    /// floating-point noise around zero tension; `0.0` drops strictly
    /// compressive cables.
    pub slack_tol: f64,
}

impl Default for TensegrityLoadOptions {
    fn default() -> Self {
        Self {
            // Comfortably above any monotone active-set count; the kernel also
            // bounds itself by `#cables`. Lowering this below the natural count
            // is how the §11 Q5 guard is exercised deterministically.
            max_active_set_iters: 64,
            cg: CgSolverOptions::default(),
            slack_tol: 0.0,
        }
    }
}

/// Reason a tensegrity load solve is infeasible. Surfaced by the trampoline as
/// an `E_TensegrityLoadInfeasible` diagnostic (PRD §8.1 contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensegrityLoadError {
    /// Input arrays disagree in length (e.g. `loads.len() != nodes.len()`), or a
    /// member/load/support node index is out of range for the node set.
    DimensionMismatch,
    /// Every node is fixed — there is no free DOF to solve for.
    EmptyFreeSet,
    /// The assembled tangent system was singular or the inner CG solve failed
    /// to converge.
    SingularSystem,
    /// The tension-only active set did not reach a fixed point within
    /// `max_active_set_iters` passes (PRD §11 Q5 defensive guard).
    ActiveSetDidNotConverge {
        /// Number of active-set passes performed before hitting the cap.
        iterations: usize,
    },
}

/// Result of a tensegrity load solve.
#[derive(Debug, Clone)]
pub struct TensegrityLoadSolve {
    /// Per-node displacement `u` (length = node count), in original node order.
    /// Fixed support nodes are exactly zero.
    pub displacements: Vec<[f64; 3]>,
    /// Per-member total axial force `Nᵢ = prestressᵢ + dNᵢ`, in input member
    /// order. Slack (dropped) cables report `0.0`.
    pub member_forces: Vec<f64>,
    /// Per-member force delta `dNᵢ` from the applied load, in input member
    /// order. Slack (dropped) cables report `−prestressᵢ` (their total force
    /// fell to `0`).
    pub member_force_deltas: Vec<f64>,
    /// Per-member slack mask, in input member order — `true` iff the member is
    /// a cable that the tension-only active set dropped.
    pub slack: Vec<bool>,
    /// Number of tension-only active-set passes performed before the fixed
    /// point (all members active ⇒ `1`).
    pub active_set_iterations: usize,
    /// Whether the solve converged: the inner CG converged on every pass and
    /// the active set reached a fixed point within the iteration cap.
    pub converged: bool,
}

/// Solve the tensegrity load-analysis problem.
///
/// `nodes` are the form-found node coordinates; `members` are the bar/cable
/// members (each carrying its node pair, kind, section, and prestress `N`);
/// `loads` is the per-node external force (length must equal `nodes.len()`);
/// `fixed_nodes` lists the support node indices (each pinned in all three
/// axes); `options` tunes the inner CG solve and the active-set cap.
///
/// Returns the solved [`TensegrityLoadSolve`] on success, or a
/// [`TensegrityLoadError`] describing why the input is infeasible.
///
/// **Stub** (Tensegrity T3b prerequisite): the public surface compiles and is
/// nameable from in-crate and `tests/` integration tests, but the behaviour is
/// not yet implemented — every call returns
/// [`TensegrityLoadError::DimensionMismatch`], so the behavioural tests added
/// in later steps start RED. The real implementation lands incrementally in the
/// TDD steps that follow.
pub fn tensegrity_load_analysis(
    nodes: &[[f64; 3]],
    members: &[BarMember],
    loads: &[[f64; 3]],
    fixed_nodes: &[usize],
    options: &TensegrityLoadOptions,
) -> Result<TensegrityLoadSolve, TensegrityLoadError> {
    let n_members = members.len();

    // All members active. The tension-only active-set drop loop is added in a
    // later step; for now this is a single linear pass over every member.
    let active = vec![true; n_members];

    let (displacements, converged) =
        solve_active_pass(nodes, members, &active, loads, fixed_nodes, options)?;

    // Per-member total force N_i = prestress_i + dN_i (every member active here,
    // so no slack zeroing yet).
    let mut member_forces = vec![0.0_f64; n_members];
    let mut member_force_deltas = vec![0.0_f64; n_members];
    for (i, member) in members.iter().enumerate() {
        let (j, k) = member.nodes;
        let u_local = [
            displacements[j][0],
            displacements[j][1],
            displacements[j][2],
            displacements[k][0],
            displacements[k][1],
            displacements[k][2],
        ];
        let dn = bar_axial_force_delta(&[nodes[j], nodes[k]], &member.section, &u_local);
        member_force_deltas[i] = dn;
        member_forces[i] = member.prestress + dn;
    }

    Ok(TensegrityLoadSolve {
        displacements,
        member_forces,
        member_force_deltas,
        slack: vec![false; n_members],
        active_set_iterations: 1,
        converged,
    })
}

/// One linear solve over the currently-active members.
///
/// Builds the global tangent stiffness from each active member's
/// `K_t = K_e + K_g` ([`bar_tangent_stiffness`]), applies the per-node external
/// loads, pins every `fixed_nodes` support in all three axes via homogeneous
/// Dirichlet BCs, and solves the reduced system with CG. Returns the per-node
/// displacement field and the CG convergence flag. This is the `bar_axial_
/// deflection.rs` assemble→BC→solve pattern generalised to `N` members and
/// supports; the tension-only active set (a later step) calls it once per pass
/// with a shrinking active set.
fn solve_active_pass(
    nodes: &[[f64; 3]],
    members: &[BarMember],
    active: &[bool],
    loads: &[[f64; 3]],
    fixed_nodes: &[usize],
    options: &TensegrityLoadOptions,
) -> Result<(Vec<[f64; 3]>, bool), TensegrityLoadError> {
    let n_nodes = nodes.len();

    // Per-active-member connectivity + tangent stiffness. Both Vecs outlive the
    // `AssemblyElement` borrows below.
    let mut conns: Vec<[usize; 2]> = Vec::new();
    let mut k_mats: Vec<ElementStiffness> = Vec::new();
    for (m, member) in members.iter().enumerate() {
        if !active[m] {
            continue;
        }
        let (j, k) = member.nodes;
        conns.push([j, k]);
        k_mats.push(bar_tangent_stiffness(
            &[nodes[j], nodes[k]],
            &member.section,
            member.prestress,
        ));
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

    let mut k_global =
        assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

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

/// First-order axial member-force delta for a 2-node bar/cable element.
///
/// With unit direction cosine `c = (node1 − node0) / L`, cross-section `(E, A)`,
/// and element nodal displacement `u_local = [u0x,u0y,u0z, u1x,u1y,u1z]`, the
/// linearised change in axial force is
///
/// ```text
/// dN = (E·A / L) · c · (u1 − u0)
/// ```
///
/// where `u1 − u0` is the relative tip displacement. This is the axial
/// component of `K_e · u_local` projected back onto the bar direction: a purely
/// transverse relative displacement contributes nothing, and a rigid-body
/// translation (`u1 = u0`) contributes nothing. The total member force is
/// `N = prestress + dN`.
///
/// Uses the same unit-direction normalisation and `MIN_BAR_LENGTH` degeneracy
/// guard convention as [`crate::assembly::bar::element_stiffness_bar_p1`].
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::test_support::assert_close;

    // (1) x-aligned bar with an axial relative tip displacement: the force
    //     delta is dN = (EA/L)·du. c = (1,0,0), du = u1 − u0 = (du,0,0), so
    //     c·du = du and dN = (EA/L)·du.
    #[test]
    fn axial_force_delta_x_aligned_axial_disp() {
        let e = 2.0e11_f64;
        let a = 1.5e-4_f64;
        let l = 3.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        let du = 0.01_f64; // node 1 displaced +du along x
        let u_local = [0.0, 0.0, 0.0, du, 0.0, 0.0];
        let dn = bar_axial_force_delta(&nodes, &section, &u_local);
        let expected = e * a / l * du; // (EA/L)·du
        assert_close(dn, expected, 1e-12, "dN = (EA/L)·du for axial disp");
    }

    // (2) x-aligned bar with a purely transverse relative displacement: the
    //     axial projection is zero, so dN = 0 (to first order K_e carries no
    //     transverse force).
    #[test]
    fn axial_force_delta_x_aligned_transverse_disp_is_zero() {
        let e = 2.0e11_f64;
        let a = 1.5e-4_f64;
        let l = 3.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        // node 1 displaced in y and z only — no component along c.
        let u_local = [0.0, 0.0, 0.0, 0.0, 0.02, -0.03];
        let dn = bar_axial_force_delta(&nodes, &section, &u_local);
        assert_close(dn, 0.0, 1e-9, "dN = 0 for purely transverse disp");
    }

    // (3) oblique 45° bar projects the relative displacement onto the unit
    //     direction cosine c = (1/√2, 1/√2, 0): an x-only tip displacement du
    //     contributes c·du = du/√2, so dN = (EA/L)·du/√2. With E=A=1, d=2
    //     (L=2√2) and du=1 this is exactly 1/4.
    #[test]
    fn axial_force_delta_oblique_45deg_projects_onto_cosine() {
        let d = 2.0_f64;
        let l = d * 2.0_f64.sqrt();
        let e = 1.0_f64;
        let a = 1.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [d, d, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        let u_local = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0]; // node 1 disp (1,0,0)
        let dn = bar_axial_force_delta(&nodes, &section, &u_local);
        let expected = e * a / l * (1.0 / 2.0_f64.sqrt()); // (EA/L)·(c·du)
        assert_close(dn, expected, 1e-12, "oblique dN projects onto cosine");
        assert_close(expected, 0.25, 1e-12, "sanity: expected == 1/4");
    }

    // (4) rigid-body translation: both nodes displaced by the same vector, so
    //     the relative displacement u1 − u0 = 0 and dN = 0 regardless of the
    //     bar orientation. Guards against using absolute (not relative) disp.
    #[test]
    fn axial_force_delta_rigid_translation_is_zero() {
        let nodes = [[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]]; // L = 5
        let section = BarSection { youngs_modulus: 1.0e6, area: 0.01 };
        let t = [0.07_f64, -0.02, 0.05];
        let u_local = [t[0], t[1], t[2], t[0], t[1], t[2]];
        let dn = bar_axial_force_delta(&nodes, &section, &u_local);
        assert_close(dn, 0.0, 1e-9, "dN = 0 under rigid-body translation");
    }
}
