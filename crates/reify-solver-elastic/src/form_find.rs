//! Force-Density (FD) form-finding kernel — anchored case (Tensegrity T1a).
//!
//! # PRD reference
//!
//! `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1a. This is the
//! pure numeric kernel behind the `solver::form_find` ComputeNode target: given
//! a tensegrity's node coordinates, member connectivity (struts then cables),
//! per-member force densities `q`, and a set of anchored node indices, it solves
//! the reduced linear Force-Density system for the free-node coordinates.
//!
//! # Method (anchored case)
//!
//! For `m` members over `N` nodes, the branch-node connectivity matrix `C` is
//! `m×N` with `+1` at the start node `j` and `−1` at the end node `k` of each
//! member. With `Q = diag(q)`, the force-density stiffness is `D = Cᵀ Q C`
//! (`N×N`). Partitioning node indices into free `F` and anchored `A`, the
//! prestress-only equilibrium (no external load) is
//!
//! ```text
//!     D_ff · X_f = − D_fa · X_a
//! ```
//!
//! solved per coordinate axis. All three axes share the same `D_ff` factor and
//! are solved together as an `|F|×3` right-hand side.
//!
//! # Sign convention
//!
//! Cables carry tension (`q > 0`), struts carry compression (`q < 0`). See the
//! validation guards in [`form_find_anchored`] for the enforced contract; the
//! free-standing eigenvalue/ratio form-finding variant is deferred to T1b.
//!
//! # Scope
//!
//! Anchored, explicit-`q`, no-load form-finding only. Force-density *ratio*
//! auto-scaling, the free-standing (unanchored) eigenvalue case, external loads,
//! and stability/buckling analysis are out of scope (tracked as T1b/T2/T3).

/// Member type tag. Determines the enforced sign of the member's force density:
/// cables carry tension (`q > 0`), struts carry compression (`q < 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// Compression member (force density `q < 0`).
    Strut,
    /// Tension member (force density `q > 0`).
    Cable,
}

/// Reason an anchored form-find solve is infeasible. Surfaced by the trampoline
/// as an `E_FormFindInfeasible` diagnostic (PRD §8.1 contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFindError {
    /// A member's force density violates its kind's sign contract
    /// (a cable with `q ≤ 0` or a strut with `q ≥ 0`).
    SignViolation,
    /// The reduced force-density stiffness `D_ff` is singular or
    /// ill-conditioned (e.g. a free node with no path to any anchor).
    SingularReducedStiffness,
    /// Every node is anchored — there is no free node to solve for.
    EmptyFreeSet,
    /// Input arrays disagree in length (`members`, `kinds`, `q`).
    DimensionMismatch,
}

/// Result of an anchored Force-Density form-find solve.
#[derive(Debug, Clone)]
pub struct FormFindSolve {
    /// Solved node coordinates in original node order (anchors unchanged,
    /// free nodes at their equilibrium positions).
    pub nodes: Vec<[f64; 3]>,
    /// Per-member axial force `Nᵢ = qᵢ · Lᵢ` on the solved geometry, in
    /// struts-then-cables member order.
    pub member_forces: Vec<f64>,
    /// Echo of the input force densities (struts-then-cables order).
    pub force_densities: Vec<f64>,
    /// Whether the solve succeeded (non-singular `D_ff`).
    pub converged: bool,
}

/// Solve the anchored Force-Density form-finding problem.
///
/// `nodes` are the node coordinates (free-node entries are an unused initial
/// guess; anchor coordinates are read here). `members` are `(start, end)` index
/// pairs in struts-then-cables order, `kinds` tags each member, `q` is the
/// per-member force density (same order), and `anchors` lists the anchored node
/// indices.
///
/// Returns the solved [`FormFindSolve`] on success, or a [`FormFindError`]
/// describing why the input is infeasible.
pub fn form_find_anchored(
    nodes: &[[f64; 3]],
    members: &[(usize, usize)],
    kinds: &[MemberKind],
    q: &[f64],
    anchors: &[usize],
) -> Result<FormFindSolve, FormFindError> {
    // pre-1 scaffold: public API only, no behavior yet. step-2/4/6 implement the
    // FD solve, member forces, and validation guards respectively. The stub
    // returns an error so kernel unit tests fail RED on behavior, not on a
    // missing symbol.
    let _ = (nodes, members, kinds, q, anchors);
    Err(FormFindError::DimensionMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for the analytic FD-identity goldens. The reduced linear solve
    /// reproduces these exact identities to ~1e-13; 1e-9 leaves ~4 orders of
    /// margin while still catching a wrong solve.
    const TOL: f64 = 1e-9;

    /// Max absolute componentwise difference between two 3-vectors.
    fn max_coord_err(a: [f64; 3], b: [f64; 3]) -> f64 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max)
    }

    // (a) A single free node cabled to 4 anchors with equal force density solves
    // to the (unweighted) centroid of the anchors — the weighted-centroid FD
    // identity x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ with all qᵢ equal. Anchors are placed
    // symmetrically in x,y so the centroid is (0, 0, 0.5).
    #[test]
    fn single_free_node_equal_q_solves_to_anchor_centroid() {
        let nodes = vec![
            [0.3, 0.2, 0.4], // free node 0 — deliberately off-solution
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [1.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("equal-q anchored cable net must be feasible");

        let expected = [0.0, 0.0, 0.5];
        assert!(
            max_coord_err(solve.nodes[0], expected) < TOL,
            "nodes[0] = {:?}, expected anchor centroid {:?}",
            solve.nodes[0],
            expected,
        );
    }

    // (b) Unequal force densities give the *weighted* centroid
    // x_f = Σ qᵢ x_{aᵢ} / Σ qᵢ. Same geometry as (a) but q = [2,1,1,1]; the
    // expected point is computed from the identity rather than hard-coded.
    #[test]
    fn single_free_node_unequal_q_solves_to_weighted_centroid() {
        let anchor_pts = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let nodes = vec![
            [0.3, 0.2, 0.4], // free node 0
            anchor_pts[0],
            anchor_pts[1],
            anchor_pts[2],
            anchor_pts[3],
        ];
        let members = [(0, 1), (0, 2), (0, 3), (0, 4)];
        let kinds = [MemberKind::Cable; 4];
        let q = [2.0, 1.0, 1.0, 1.0];
        let anchors = [1, 2, 3, 4];

        // Analytic weighted centroid Σ qᵢ x_i / Σ qᵢ.
        let qsum: f64 = q.iter().sum();
        let mut expected = [0.0_f64; 3];
        for (w, p) in q.iter().zip(anchor_pts.iter()) {
            for (e, c) in expected.iter_mut().zip(p.iter()) {
                *e += w * c;
            }
        }
        for e in expected.iter_mut() {
            *e /= qsum;
        }

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("unequal-q anchored cable net must be feasible");

        assert!(
            max_coord_err(solve.nodes[0], expected) < TOL,
            "nodes[0] = {:?}, expected weighted centroid {:?}",
            solve.nodes[0],
            expected,
        );
    }

    // (c) Two free nodes in a uniform-tension chain
    // anchor(x=0) — node0 — node1 — anchor(x=3), all cables q=1. The interior
    // nodes settle to evenly-spaced positions x0=1, x1=2. This exercises the
    // off-diagonal D_ff coupling: the node0–node1 cable couples the two free
    // equations, so a diagonal-only solve would get this wrong.
    #[test]
    fn two_free_node_chain_solves_to_uniform_spacing() {
        let nodes = vec![
            [0.5, 0.0, 0.0], // free node 0 — off-solution
            [2.5, 0.0, 0.0], // free node 1 — off-solution
            [0.0, 0.0, 0.0], // anchor at x=0
            [3.0, 0.0, 0.0], // anchor at x=3
        ];
        let members = [(2, 0), (0, 1), (1, 3)];
        let kinds = [MemberKind::Cable; 3];
        let q = [1.0, 1.0, 1.0];
        let anchors = [2, 3];

        let solve = form_find_anchored(&nodes, &members, &kinds, &q, &anchors)
            .expect("uniform-tension chain must be feasible");

        assert!(
            max_coord_err(solve.nodes[0], [1.0, 0.0, 0.0]) < TOL,
            "free node 0 = {:?}, expected (1,0,0)",
            solve.nodes[0],
        );
        assert!(
            max_coord_err(solve.nodes[1], [2.0, 0.0, 0.0]) < TOL,
            "free node 1 = {:?}, expected (2,0,0)",
            solve.nodes[1],
        );
    }
}
