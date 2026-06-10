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

/// Diagonal magnitude seeded at an orphan fixed node's DOFs so the Dirichlet
/// row-elimination has a stored diagonal to overwrite.
///
/// The value is physically inert: a grounded DOF belongs to a fixed support, so
/// `apply_dirichlet_row_elimination` unconditionally sets its diagonal to `1.0`
/// and pins its displacement to `0` regardless of what is seeded here. A unit
/// value (rather than a near-zero epsilon) keeps the pre-elimination matrix
/// well-conditioned and survives any sparse-builder zero-pruning.
const GROUNDING_DIAGONAL: f64 = 1.0;

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

    // Tension-only active set: start with every member active and drop any
    // active cable whose total force goes compressive, re-solving until a pass
    // drops nothing. The drop is monotone (a slack cable is never re-added), so
    // the active set strictly shrinks and the loop terminates in at most
    // `#cables` passes. K_g is held *linear-about-prestress*: every pass builds
    // K_g from the fixed form-found `member.prestress`, never the load-updated
    // force (PRD §10), so the converged post-drop deflection equals the reduced
    // linear system with the slack cables removed.
    let mut active = vec![true; n_members];
    let mut slack = vec![false; n_members];
    let mut iterations = 0usize;

    let (displacements, converged) = loop {
        iterations += 1;
        let (disp, conv) =
            solve_active_pass(nodes, members, &active, loads, fixed_nodes, options)?;

        // Drop any active cable that has gone compressive (struts carry
        // compression and are never dropped).
        let mut dropped_any = false;
        for (i, member) in members.iter().enumerate() {
            if !active[i] || member.kind != MemberKind::Cable {
                continue;
            }
            let total = member.prestress + member_force_delta(nodes, member, &disp);
            if total < -options.slack_tol {
                active[i] = false;
                slack[i] = true;
                dropped_any = true;
            }
        }

        if !dropped_any {
            // Fixed point: no active cable went slack this pass.
            break (disp, conv);
        }
    };

    // Final per-member forces: slack cables report 0 with delta = −prestress
    // (their total force fell to zero); active members report prestress + dN on
    // the converged displacement field.
    let mut member_forces = vec![0.0_f64; n_members];
    let mut member_force_deltas = vec![0.0_f64; n_members];
    for (i, member) in members.iter().enumerate() {
        if slack[i] {
            member_forces[i] = 0.0;
            member_force_deltas[i] = -member.prestress;
        } else {
            let dn = member_force_delta(nodes, member, &displacements);
            member_force_deltas[i] = dn;
            member_forces[i] = member.prestress + dn;
        }
    }

    Ok(TensegrityLoadSolve {
        displacements,
        member_forces,
        member_force_deltas,
        slack,
        active_set_iterations: iterations,
        converged,
    })
}

/// Axial force delta `dN` for `member` evaluated on a displacement field.
///
/// Gathers the member's two nodal displacements into the local 6-vector and
/// defers to [`bar_axial_force_delta`]. The total member force is
/// `prestress + dN`.
fn member_force_delta(
    nodes: &[[f64; 3]],
    member: &BarMember,
    displacements: &[[f64; 3]],
) -> f64 {
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

    // Per-active-member connectivity + tangent stiffness, plus the 1-node
    // grounding stabilisers appended below. `conns` is heterogeneous (2-node
    // bars and 1-node grounders), so it owns `Vec<usize>` connectivity; both
    // Vecs outlive the `AssemblyElement` borrows further down.
    let mut conns: Vec<Vec<usize>> = Vec::new();
    let mut k_mats: Vec<ElementStiffness> = Vec::new();
    let mut connected = vec![false; n_nodes];
    for (m, member) in members.iter().enumerate() {
        if !active[m] {
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

    // Grounding stabilisers for *orphan* fixed nodes — support nodes that no
    // active member touches. This happens when a fixed node's only members are
    // all dropped as slack (e.g. the far anchor of a collinear cable string once
    // the relieved cable is removed). `apply_dirichlet_row_elimination` requires
    // a stored diagonal at every constrained DOF (the FEA-assembled-K invariant,
    // Task 2916), but an orphan node contributes no stiffness entries, so the BC
    // pass would otherwise panic on a missing `K[i][i]`. Each orphan fixed node
    // gets a 1-node element seeding a diagonal at its three DOFs. The magnitude
    // is physically inert: row-elimination overwrites every fixed DOF's diagonal
    // with 1.0 and pins its displacement to 0, so this only guarantees the
    // diagonal exists — it changes no force or displacement in the solve.
    for &node in fixed_nodes {
        if node < n_nodes && !connected[node] {
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

    // ---- Validation / active-set guard tests (PRD §11 Q5) -----------------

    /// Build a cable [`BarMember`] joining `j`–`k`. Shared by the guard tests.
    fn cable(j: usize, k: usize, e: f64, a: f64, prestress: f64) -> BarMember {
        BarMember {
            nodes: (j, k),
            kind: MemberKind::Cable,
            section: BarSection { youngs_modulus: e, area: a },
            prestress,
        }
    }

    /// Tight inner-CG guard options with a caller-chosen active-set cap. The
    /// guard problems are tiny (≤ 3 nodes), so CG converges in a few iterations.
    fn guard_options(max_active_set_iters: usize) -> TensegrityLoadOptions {
        TensegrityLoadOptions {
            max_active_set_iters,
            cg: CgSolverOptions { tolerance: 1.0e-12, max_iter: 1000 },
            slack_tol: 0.0,
        }
    }

    // (a) `loads.len()` must equal `nodes.len()`: a short loads vector is a
    //     DimensionMismatch. The kernel must validate up-front rather than
    //     silently dropping the missing per-node forces.
    #[test]
    fn dimension_mismatch_loads_length() {
        let nodes = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [4.0, 0.0, 0.0]];
        let members = vec![
            cable(0, 1, 200.0e9, 1.0e-4, 5_000.0),
            cable(1, 2, 200.0e9, 1.0e-4, 5_000.0),
        ];
        let loads = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]; // 2 ≠ 3 nodes
        let fixed_nodes = vec![0, 2];
        let result =
            tensegrity_load_analysis(&nodes, &members, &loads, &fixed_nodes, &guard_options(64));
        assert!(
            matches!(result, Err(TensegrityLoadError::DimensionMismatch)),
            "loads.len() != nodes.len() must be DimensionMismatch, got {result:?}",
        );
    }

    // (a′) A member referencing a node index outside `0..nodes.len()` is a
    //      DimensionMismatch, caught up-front before any assembly indexing.
    #[test]
    fn dimension_mismatch_member_node_index_out_of_range() {
        let nodes = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let members = vec![cable(0, 5, 200.0e9, 1.0e-4, 5_000.0)]; // node 5 ∉ 0..2
        let loads = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let fixed_nodes = vec![0];
        let result =
            tensegrity_load_analysis(&nodes, &members, &loads, &fixed_nodes, &guard_options(64));
        assert!(
            matches!(result, Err(TensegrityLoadError::DimensionMismatch)),
            "out-of-range member node index must be DimensionMismatch, got {result:?}",
        );
    }

    // (b) Every node fixed ⇒ no free DOF to solve for ⇒ EmptyFreeSet.
    #[test]
    fn empty_free_set_all_nodes_fixed() {
        let nodes = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let members = vec![cable(0, 1, 200.0e9, 1.0e-4, 5_000.0)];
        let loads = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let fixed_nodes = vec![0, 1]; // both (all) nodes anchored
        let result =
            tensegrity_load_analysis(&nodes, &members, &loads, &fixed_nodes, &guard_options(64));
        assert!(
            matches!(result, Err(TensegrityLoadError::EmptyFreeSet)),
            "all-fixed problem must be EmptyFreeSet, got {result:?}",
        );
    }

    // (c) The §11 Q5 active-set cap. The slackening collinear-cable problem
    //     needs two passes (drop, then confirm the fixed point); capping at one
    //     pass trips the ActiveSetDidNotConverge{iterations} guard instead of
    //     letting the loop reach its natural fixed point.
    #[test]
    fn active_set_did_not_converge_when_cap_below_natural_count() {
        let l = 2.0_f64;
        let e = 200.0e9_f64;
        let a = 1.0e-4_f64;
        let n0 = 5_000.0_f64;
        let p = 3.0 * n0;
        let nodes = vec![[0.0, 0.0, 0.0], [l, 0.0, 0.0], [2.0 * l, 0.0, 0.0]];
        let members = vec![cable(0, 1, e, a, n0), cable(1, 2, e, a, n0)];
        let loads = vec![[0.0, 0.0, 0.0], [p, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let fixed_nodes = vec![0, 2];
        let result = tensegrity_load_analysis(
            &nodes,
            &members,
            &loads,
            &fixed_nodes,
            &guard_options(1), // natural count is 2 ⇒ a cap of 1 trips the guard
        );
        assert!(
            matches!(
                result,
                Err(TensegrityLoadError::ActiveSetDidNotConverge { iterations: 1 })
            ),
            "cap below natural count must be ActiveSetDidNotConverge{{ iterations: 1 }}, \
             got {result:?}",
        );
    }
}
