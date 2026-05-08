//! Global sparse-matrix assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #9. This module
//! scatters per-element [`crate::assembly::ElementStiffness`] dense matrices
//! into a global sparse stiffness matrix `K` of size `3N × 3N` (N = global
//! node count) using `faer-rs` CSR triplet builders.

use faer::sparse::{SparseRowMat, Triplet};

use super::ElementStiffness;

/// One element's contribution to the global system.
///
/// `connectivity` lists the global node IDs of the element's local nodes in
/// the same order as the rows/columns of `k_e` — that is, the local DOF index
/// `3 * a + α` (axis `α ∈ {0, 1, 2}`) maps to global DOF
/// `3 * connectivity[a] + α`.
///
/// The `id` field is descriptive metadata used in panic messages (e.g. to
/// name the offending element in a contract violation) and is *not* used
/// internally as a sort key in any [`AssemblyMode`]. Callers requiring a
/// canonical iteration order in [`AssemblyMode::Deterministic`] must sort
/// the slice themselves before passing it in.
pub struct AssemblyElement<'a> {
    /// Element ID (descriptive metadata; surfaces in panic messages).
    pub id: usize,
    /// Global node IDs — `connectivity.len() * 3 == k_e.n_dofs`.
    pub connectivity: &'a [usize],
    /// Per-element stiffness matrix.
    pub k_e: &'a ElementStiffness,
}

/// How [`assemble_global_stiffness`] iterates over `elements` when scattering
/// per-element triplets into the global system.
///
/// # `Deterministic`
///
/// Single-threaded, slice-order accumulation. The triplet emission order is
/// exactly the iteration order of the input slice. faer's CSR builder sums
/// duplicate `(row, col)` pairs in the order it encounters them, so the
/// global `K[i][j]` summation order is fully determined by the slice's
/// iteration order. Bit-stable across runs **and across machines**.
///
/// # `Parallel { threads }`
///
/// Multi-threaded scatter via `std::thread::scope`. The element slice is
/// partitioned into `threads` chunks; each thread accumulates a local
/// `Vec<Triplet>` in slice order; after join the per-thread Vecs are
/// concatenated in **thread-spawn order (0, 1, 2, …)** before being handed
/// to faer. This gives bit-stability for any *fixed* thread count, but the
/// summation order — and hence the LSB of shared-DOF sums — varies across
/// thread counts. Cross-thread-count equivalence is bounded by
/// `O(ulp · max|K_e[i][j]|)`, far below the FEA tolerance band.
///
/// `Parallel { threads: 0 }` is rejected with a panic at function entry —
/// auto-falling-back to single-threaded would silently mask caller bugs
/// (e.g. a misread config defaulting `threads` to 0). The "tiny problems
/// fall back to single-threaded under 10K DOF" policy lives in the
/// `ElasticOptions` resolution layer (PRD task #16), not in this primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyMode {
    /// Single-threaded, slice-order accumulation.
    Deterministic,
    /// Multi-threaded scatter with fixed-thread-id-order merge. `threads`
    /// must be `>= 1`; passing `0` panics.
    Parallel {
        /// Worker thread count.
        threads: usize,
    },
}

/// Scatter per-element stiffness matrices into a global `3N × 3N` sparse
/// stiffness matrix.
///
/// `n_nodes` is the global node count; the returned matrix has
/// `3 * n_nodes` rows and columns. `elements` is the slice of element
/// contributions (see [`AssemblyElement`]); each contribution emits a full
/// dense `(a, b, α, β)` block of `9 · k_e.n_local²` triplets, and faer's
/// CSR builder sums duplicates that share a `(row, col)` pair.
///
/// See [`AssemblyMode`] for the iteration / merge contract per mode.
pub fn assemble_global_stiffness(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
    mode: AssemblyMode,
) -> SparseRowMat<usize, f64> {
    // Empty-input fast path: faer accepts a zero-triplet input cleanly and
    // returns the all-zero sparse matrix of the requested shape. Wiring the
    // mode and element-slice contract checks (steps 7/8) in this skeleton
    // would force step-1's empty-input test to over-specify; those checks
    // land in their own RED/GREEN cycle.
    let _ = (elements, mode);
    let triplets: Vec<Triplet<usize, usize, f64>> = Vec::new();
    SparseRowMat::try_new_from_triplets(3 * n_nodes, 3 * n_nodes, &triplets)
        .expect("zero-triplet build cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::tet::element_stiffness_p1;
    use crate::constitutive::IsotropicElastic;

    /// Steel-like dimensionless material reused across the global-assembly
    /// tests. Mirrors the convention from `assembly::tests::dimensionless_steel_like`
    /// and `tet::tests::dimensionless_steel_like` so K_e numerics stay in
    /// O(1) range for human-readable failure messages.
    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>` as a plain `f64`,
    /// returning `0.0` if the entry is not stored. Lets test code densify
    /// the global K with one read per `(row, col)` regardless of whether
    /// the assembly path bothered to store explicit zero entries.
    fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }

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

    /// Single P1 element with identity connectivity `[0,1,2,3]` → K_global
    /// equals K_e bit-for-bit at every entry.
    ///
    /// Pins the DOF-mapping rule:
    /// `K_global[3*conn[a]+α][3*conn[b]+β] = K_e[3*a+α][3*b+β]`. With
    /// identity connectivity the rule degenerates to identity, so the
    /// densified 12×12 must match K_e exactly. A future regression that
    /// transposes the row/col mapping (or shifts axis-major vs node-major
    /// indexing) will surface here.
    #[test]
    fn single_p1_element_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        let connectivity = [0usize, 1, 2, 3];
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(4, &[element], AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 12);
        assert_eq!(k.ncols(), 12);

        for i in 0..12 {
            for j in 0..12 {
                let actual = read(&k, i, j);
                let expected = k_e.get(i, j);
                // Bit-for-bit: identity mapping ⇒ no FP-summation reordering.
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] = {actual} but K_e[{i}][{j}] = {expected}",
                );
            }
        }
    }
}
