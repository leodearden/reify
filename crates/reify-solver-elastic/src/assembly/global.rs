//! Global sparse-matrix assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #9. This module
//! scatters per-element [`crate::assembly::ElementStiffness`] dense matrices
//! into a global sparse stiffness matrix `K` of size `3N × 3N` (N = global
//! node count) using `faer-rs` CSR triplet builders.

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty `elements` slice → `3N × 3N` all-zero sparse matrix.
    ///
    /// Pins the empty-input contract: the function returns a matrix whose
    /// dimensions match `3 * n_nodes`, and whose stored-entry count is zero
    /// (faer's CSR builder must accept a zero-triplet input cleanly).
    #[test]
    fn empty_elements_returns_zero_3n_by_3n_sparse_matrix() {
        // Compile-only construction of both `AssemblyMode` variants so a
        // future regression that drops one of the variants surfaces here.
        let _det = AssemblyMode::Deterministic;
        let _par = AssemblyMode::Parallel { threads: 1 };

        let n_nodes = 4;
        let k = assemble_global_stiffness(n_nodes, &[], AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 3 * n_nodes);
        assert_eq!(k.ncols(), 3 * n_nodes);
        assert_eq!(k.compute_nnz(), 0, "no triplets ⇒ zero stored entries");
    }
}
