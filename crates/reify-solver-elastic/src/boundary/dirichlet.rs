//! Dirichlet (prescribed-displacement) boundary condition application via
//! row-elimination for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #10.
//!
//! # Algorithm
//!
//! Given a sparse SPD global stiffness matrix `K` (`n × n`) and load vector
//! `f` (length `n`), and a list of constraints `bcs = [(i₁, u₁), (i₂, u₂), …]`,
//! apply each `(i, u_i)` in slice order:
//!
//! 1. **Column-into-RHS:** for every row `j ≠ i`, `f[j] -= K[j][i] · u_i`.
//!    This step **must** run before row/col zeroing because it reads the
//!    still-original `K[j][i]` values. Omitting it makes inhomogeneous BCs
//!    (`u_i ≠ 0`) silently wrong.
//! 2. **Zero row `i`:** set every stored value in row `i` to `0.0`.
//! 3. **Zero column `i`:** for every row `j ≠ i`, find the (≤1) stored entry
//!    where `col_idx == i` and set it to `0.0`.
//! 4. **Set diagonal:** set `K[i][i] = 1.0`. Panics if no explicit diagonal
//!    entry exists (FEA-assembled K always has one per Task 2916).
//! 5. **Pin RHS:** `f[i] = u_i`.
//!
//! # Symmetry preservation
//!
//! The algorithm zeros both row `i` and column `i` (not just one), so a
//! symmetric input K remains symmetric after elimination. Setting `K[i][i] = 1.0`
//! preserves a positive eigenvalue at the constrained DOF (SPD is retained).
//! The unconstrained block is the original SPD submatrix with rows/cols `i`
//! deleted — still SPD by Cauchy interlacing. The
//! `multiple_bcs_preserve_k_symmetry_within_fp_tolerance` test pins this
//! invariant as a regression guard.
//!
//! # Order-independence
//!
//! When two BCs target distinct DOFs `k₁` and `k₂`, applying them in either
//! order produces bit-identical K and tolerance-equal f. The mechanism: BC₁'s
//! row-zero pass zeros `K[k₁][k₂]`, so when BC₂ later runs
//! `f[k₁] -= K[k₁][k₂] · u₂`, the subtract is `-= 0.0`, leaving
//! `f[k₁] = u₁` intact. The contract holds for *distinct* DOFs only —
//! duplicate DOFs in `bcs` are caller error and are not guarded against.
//! The `multiple_bcs_are_order_independent_within_fp_tolerance` test pins this.
//!
//! # References
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` task #10.

use faer::sparse::SparseRowMat;

/// A prescribed-displacement boundary condition at a single degree of freedom.
///
/// `dof` is the 0-based DOF index in the global system (row and column in K).
/// `value` is the prescribed displacement `u_i`; use `0.0` for homogeneous
/// (zero-displacement) constraints.
///
/// The pair `(dof, value)` is consumed by
/// [`apply_dirichlet_row_elimination`], which performs in-place row-elimination
/// on the global K and f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirichletBc {
    /// Global DOF index (0-based row/column in K).
    pub dof: usize,
    /// Prescribed displacement value (`u_i`). Use `0.0` for homogeneous.
    pub value: f64,
}

/// Apply Dirichlet boundary conditions to the global stiffness `K` and load
/// vector `f` via row-elimination, in place.
///
/// `bcs` is a slice of `(DOF, prescribed-value)` pairs. For each
/// `DirichletBc { dof: i, value: u_i }` in slice order the function:
///
/// 1. Subtracts `K[j][i] · u_i` from `f[j]` for every row `j ≠ i` (column-
///    into-RHS, preserving the inhomogeneous solution for unconstrained DOFs).
/// 2. Zeros every stored value in row `i`.
/// 3. Zeros the stored `K[j][i]` entry for every row `j ≠ i`.
/// 4. Sets `K[i][i] = 1.0` (unit diagonal at the constrained DOF).
/// 5. Sets `f[i] = u_i`.
///
/// The sparsity pattern of `K` is not changed — only stored values are
/// overwritten.  Callers may therefore reuse the same K/f allocation across
/// multiple solves.
///
/// # Symmetry preservation
///
/// Row-elimination zeros both row `i` and column `i`, so symmetric K stays
/// symmetric.  Setting the diagonal to `1.0` keeps K positive-definite on the
/// constrained block.  See the module-level doc for the SPD proof sketch.
///
/// # Order-independence
///
/// For distinct DOF indices the result is order-independent: K is bit-identical
/// and f is tolerance-equal regardless of the slice order of `bcs`.  See the
/// module-level doc for the mechanism.
///
/// # Panics
///
/// - `f.len() != k.nrows()` — the load vector length must equal the matrix
///   dimension.
/// - `k.nrows() != k.ncols()` — K must be square.
/// - `bc.dof >= k.nrows()` for any `bc` in `bcs` — DOF index out of range;
///   the panic message names the offending dof and the matrix dimension.
/// - No explicit diagonal entry `K[bc.dof][bc.dof]` stored — all
///   FEA-assembled K matrices satisfy this (per Task 2916); a missing diagonal
///   indicates a non-FEA-assembled input.
pub fn apply_dirichlet_row_elimination(
    k: &mut SparseRowMat<usize, f64>,
    f: &mut [f64],
    bcs: &[DirichletBc],
) {
    if bcs.is_empty() {
        return;
    }
    todo!(
        "apply_dirichlet_row_elimination: non-empty BC list not yet implemented \
         (step-4 will replace this todo!)"
    )
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::{DirichletBc, apply_dirichlet_row_elimination};

    use faer::sparse::SparseRowMat;

    use crate::assembly::{
        AssemblyElement, AssemblyMode, assemble_global_stiffness,
    };
    use crate::assembly::tet::element_stiffness_p1;
    use crate::constitutive::IsotropicElastic;

    /// Steel-like dimensionless material reused across boundary tests.
    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1_NODES: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>` as a plain `f64`,
    /// returning `0.0` if the entry is not stored. Lets test code densify
    /// K without caring whether the BC algorithm left explicit zero entries.
    fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }

    /// Assemble K for the single unit P1 tet (identity connectivity).
    fn single_p1_k() -> SparseRowMat<usize, f64> {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1_NODES, &mat);
        let connectivity = [0usize, 1, 2, 3];
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        assemble_global_stiffness(4, &[element], AssemblyMode::Deterministic)
    }

    /// Assemble K for a two-element shared-face mesh (15 × 15).
    ///
    /// Same mesh as `assembly::global::tests::two_p1_elements_sharing_face_*`:
    /// - Element 0: connectivity `[0, 1, 2, 3]`, E = 1.0
    /// - Element 1: connectivity `[1, 2, 3, 4]`, E = 1.0 (same material)
    /// - n_nodes = 5  →  K is 15 × 15
    fn two_element_shared_face_k() -> SparseRowMat<usize, f64> {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1_NODES, &mat);
        let conn0 = [0usize, 1, 2, 3];
        let conn1 = [1usize, 2, 3, 4];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn0,
                k_e: &k_e,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn1,
                k_e: &k_e,
            },
        ];
        assemble_global_stiffness(5, &elements, AssemblyMode::Deterministic)
    }

    // -----------------------------------------------------------------------
    // Step 1 — empty BC list is a no-op
    // -----------------------------------------------------------------------

    /// Empty BC list → K and f are bit-identical to their pre-call snapshots.
    ///
    /// Pins the no-op contract: passing `bcs = &[]` must be a perfect
    /// identity operation — no stored value in K is touched, no `f[j]`
    /// changes.  Regression guard for future refactors that, for example,
    /// allocate and write a scratch buffer unconditionally.
    #[test]
    fn apply_dirichlet_bcs_with_empty_slice_leaves_k_and_f_unchanged() {
        let mut k = single_p1_k();
        let mut f: Vec<f64> = (0..12).map(|i| i as f64).collect();

        // Snapshot K (densified) and f before the call.
        let k_before: Vec<Vec<f64>> = (0..12)
            .map(|i| (0..12).map(|j| read(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        // Apply empty BC list — must be a no-op.
        apply_dirichlet_row_elimination(&mut k, &mut f, &[]);

        // Verify bit-exact identity.
        for i in 0..12 {
            for j in 0..12 {
                let after = read(&k, i, j);
                assert_eq!(
                    after.to_bits(),
                    k_before[i][j].to_bits(),
                    "K[{i}][{j}] changed after empty-BC call: was {}, now {}",
                    k_before[i][j],
                    after,
                );
            }
            assert_eq!(
                f[i].to_bits(),
                f_before[i].to_bits(),
                "f[{i}] changed after empty-BC call: was {}, now {}",
                f_before[i],
                f[i],
            );
        }
    }
}
