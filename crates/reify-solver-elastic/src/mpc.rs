//! Multi-point constraint (MPC) types for the structural-analysis solver.
//!
//! # PRD reference
//!
//! See `docs/prds/v0_4/structural-analysis-shells.md` tasks **T10 / T11**.
//! Task T11 (this commit) ships the global mixed-element assembler and
//! the typed `MpcRow` placeholder that T10 will populate.
//!
//! # Constraint form
//!
//! Each `MpcRow` represents a single linear equality constraint
//!
//! ```text
//!     Σᵢ coeffs[i] · u[dofs[i]] = rhs
//! ```
//!
//! over the global displacement vector `u`. A typical MPC connects
//! `n ≥ 2` DOFs at distinct global indices; e.g. a shell-tet rotation
//! ↔ tet-displacement-gradient tying constraint at one through-thickness
//! sampling point produces one `MpcRow` (with the shell rotation DOF
//! plus the displacement DOFs of the tet nodes spanned by the
//! through-thickness offset).
//!
//! # Application strategy
//!
//! T10 will apply MPCs **post-assembly via row-elimination**, reusing
//! Task 2917's Dirichlet plumbing in `crate::boundary::dirichlet`.
//! Concretely: each row of K (and the corresponding entry of f) is
//! eliminated by substituting `u[dofs[0]] = (rhs − Σᵢ>0 coeffs[i] ·
//! u[dofs[i]]) / coeffs[0]` (or any alternative pivot DOF with non-zero
//! coefficient), then the substituted equation is plugged back into K's
//! other rows. The KKT-style penalty / Lagrange-multiplier alternative is
//! out of scope; row-elimination matches the v0.3 Dirichlet code path
//! and avoids growing the linear system.
//!
//! # T11 / T10 split
//!
//! - **T11 (this commit)** — ship the `MpcRow` placeholder type and the
//!   `pub mod mpc;` declaration so the file the orchestrator's
//!   file-list expects exists, the type is callable from downstream
//!   crates, and the round-trip contract on the public fields is locked.
//! - **T10 (Task 3020, pending)** — populate construction methods (e.g.
//!   `MpcRow::shell_tet_tying(shell_node, tet_nodes, offset, ...)`) and
//!   the row-elimination application function. T10's edits are
//!   insertion-only on the public surface of this module.
//!
//! `assemble_global_stiffness` does **not** take MPCs as input — MPCs
//! are applied post-assembly. See the design decision in the task plan
//! for the rationale.

/// One linear multi-point constraint row of the form
/// `Σᵢ coeffs[i] · u[dofs[i]] = rhs`.
///
/// `dofs` and `coeffs` must agree in length. Constructors that enforce
/// this invariant are deferred to T10 (Task 3020); for now consumers
/// build via struct-literal initialization. The `Debug` / `Clone` /
/// `PartialEq` derives are needed for downstream test assertions and
/// caller-side bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct MpcRow {
    /// Global DOF indices participating in this constraint. Order is
    /// significant only insofar as it matches `coeffs` element-wise;
    /// the constraint equation itself is symmetric in summation order.
    pub dofs: Vec<usize>,
    /// Coefficients corresponding to `dofs` element-wise. Must have the
    /// same length as `dofs`.
    pub coeffs: Vec<f64>,
    /// Right-hand side scalar. For homogeneous constraints (e.g.
    /// shell-tet tying with no imposed offset) this is `0.0`.
    pub rhs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only smoke that `MpcRow` is reachable and struct-literal
    /// constructible with the documented field shape.
    ///
    /// Once Task 3020 (T10) adds real constructors / validators (e.g.
    /// `MpcRow::shell_tet_tying`, length-equality assertions), the
    /// behavioural tests live alongside that logic. This test exists
    /// solely to lock that the public-field shape is the one downstream
    /// crates will compile against — no behaviour to assert until T10
    /// owns it.
    #[test]
    fn mpc_row_type_compiles_with_documented_field_shape() {
        let _: MpcRow = MpcRow {
            dofs: vec![3, 7, 11],
            coeffs: vec![1.0, -0.5, 0.5],
            rhs: 0.0,
        };
    }
}
