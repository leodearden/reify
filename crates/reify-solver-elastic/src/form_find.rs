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
