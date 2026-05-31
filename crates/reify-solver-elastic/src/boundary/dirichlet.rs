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

use crate::sparse_util::find_in_row;

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
/// The input `K` must have at most one stored entry per `(row, col)` pair —
/// the standard CSR uniqueness invariant satisfied by all faer
/// [`SparseRowMat`] matrices assembled via `try_new_from_triplets` and by
/// [`assemble_global_stiffness`].  The fused column-scan stops at the first
/// match per row; a matrix with duplicate column entries would leave one copy
/// un-zeroed and produce a silently wrong result.
///
/// # Symmetry preservation
///
/// Row-elimination zeros both row `i` and column `i`, so symmetric K stays
/// symmetric.  Setting the diagonal to `1.0` keeps K positive-definite on the
/// constrained block.  The unconstrained block is the original SPD submatrix
/// with rows/cols `i` deleted — still SPD by Cauchy interlacing.
/// The `multiple_bcs_preserve_k_symmetry_within_fp_tolerance` test is the
/// regression pin for this invariant: any refactor that accidentally zeros
/// only the row or only the column will produce a visibly asymmetric K on the
/// two-element shared-face mesh and trip the tolerance check.
///
/// # Order-independence
///
/// For **distinct** DOF indices the result is order-independent: K is
/// bit-identical and f is tolerance-equal regardless of the slice order of
/// `bcs`.  The mechanism: BC₁'s row-zero pass zeros `K[k₁][k₂]`, so when
/// BC₂ later runs `f[k₁] -= K[k₁][k₂] · u₂`, it reads `0.0` — leaving
/// `f[k₁] = u₁` intact.  K bit-identity follows because set operations
/// (writing 0.0 and 1.0) are idempotent regardless of which BC reaches a
/// shared entry first.
///
/// The `multiple_bcs_are_order_independent_within_fp_tolerance` test is the
/// regression pin.  Note: duplicate DOFs in `bcs` (the same `dof` appearing
/// twice) are caller error; the result is unspecified and, in debug builds,
/// caught by an explicit assertion.
///
/// # Complexity
///
/// O(nnz × |bcs|) where nnz is the number of stored entries in K. Each BC
/// drives one full row-scan to locate the column-i entries. For FEA matrices
/// (O(n) nnz, |bcs| ≪ n) this is dominated by the global solve cost and is
/// not a bottleneck in practice. At pinned-surface scale (|bcs| ~ O(n)),
/// a precomputed CSC mirror would reduce work to O(nnz_col_i) per BC; that
/// optimisation is left for a future pass when profiling warrants it.
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
/// - In debug builds, panics if `K` has unsorted (or duplicate) column
///   indices within any row — `col_idx[start..end]` must be strictly
///   increasing. **Release builds silently produce wrong results** (binary
///   search on unsorted data returns unspecified Ok/Err — sort col_idx, e.g.
///   via `try_new_from_triplets`, before calling).
pub fn apply_dirichlet_row_elimination(
    k: &mut SparseRowMat<usize, f64>,
    f: &mut [f64],
    bcs: &[DirichletBc],
) {
    // --- Contract checks (mirroring assemble_global_stiffness panic policy) ---
    assert_eq!(
        f.len(),
        k.nrows(),
        "f.len() = {} but k.nrows() = {}; expected f.len() == k.nrows()",
        f.len(),
        k.nrows(),
    );
    assert_eq!(
        k.nrows(),
        k.ncols(),
        "k must be square: k.nrows() = {} but k.ncols() = {}",
        k.nrows(),
        k.ncols(),
    );
    for bc in bcs {
        assert!(
            bc.dof < k.nrows(),
            "DirichletBc {{ dof: {} }} exceeds k.nrows() = {}; valid range is 0..{}",
            bc.dof,
            k.nrows(),
            k.nrows(),
        );
    }

    // Debug-only: duplicate DOF indices produce undefined output (a later
    // BC's row/col-zero overwrites an earlier BC's f[k] = u). Surface it
    // eagerly in debug builds via an O(m log m) sort-and-scan.
    #[cfg(debug_assertions)]
    {
        let mut dofs: Vec<usize> = bcs.iter().map(|bc| bc.dof).collect();
        dofs.sort_unstable();
        for w in dofs.windows(2) {
            assert_ne!(
                w[0], w[1],
                "duplicate DirichletBc dof {} in bcs slice; duplicate DOFs produce \
                 undefined output — deduplicate before calling \
                 apply_dirichlet_row_elimination",
                w[0],
            );
        }
    }

    // Debug-only: walk all rows and assert strictly-increasing col_idx.
    // binary_search on an unsorted slice returns an unspecified result
    // (Rust std: "If the slice is not sorted, the returned result is
    // unspecified and meaningless") — the diag_found assert below only
    // catches the diagonal-row miss; off-diagonal corruption (wrong K[j][i]
    // zeroed or wrong f[j] subtracted) is silent. Surface it eagerly here.
    // O(nnz) total per call, paid only in debug builds. Asserting strictly
    // increasing (`<`) also catches duplicate (row, col) entries, which
    // break find_in_row's binary_search uniqueness assumption.
    #[cfg(debug_assertions)]
    {
        let sym = k.symbolic();
        let row_ptr = sym.row_ptr();
        let col_idx = sym.col_idx();
        let n = sym.nrows();
        for j in 0..n {
            let start = row_ptr[j];
            let end = row_ptr[j + 1];
            for w in col_idx[start..end].windows(2) {
                assert!(
                    w[0] < w[1],
                    "apply_dirichlet_row_elimination: col_idx is unsorted within row {j}: \
                     adjacent entries {w0} >= {w1}; col_idx[start..end] must be strictly \
                     increasing (faer try_new_from_triplets guarantees this; ensure \
                     col_idx is sorted before calling apply_dirichlet_row_elimination)",
                    w0 = w[0],
                    w1 = w[1],
                );
            }
        }
    }

    for bc in bcs {
        let i = bc.dof;
        let u = bc.value;

        let (sym, vals) = k.parts_mut();
        let row_ptr = sym.row_ptr();
        let col_idx = sym.col_idx();
        let n = sym.nrows();

        // Step 2: zero row i — set every stored value in row i to 0.0.
        // Must precede the fused loop: the diagonal K[i][i] is zeroed here
        // and then unconditionally set to 1.0 inside the loop (step 4).
        // Running this fill after the loop would clobber that write.
        vals[row_ptr[i]..row_ptr[i + 1]].fill(0.0);

        // Fused steps 1, 3, and 4: single pass over all rows.
        // Complexity: O(nnz) per BC (scans all n rows once); see the function-level
        // `# Complexity` note for guidance on when this becomes a bottleneck.
        //
        // For each row j, locate the stored K[j][i] entry (at most one per
        // row in CSR — the uniqueness invariant required by this function):
        //
        // • j ≠ i: read K[j][i], subtract into f[j] (step 1), then zero the
        //   stored entry (step 3). Reading before writing preserves the
        //   still-original K[j][i] value for the subtraction. Step 2 above
        //   only zeroed row i, so K[j][i] for j ≠ i is unaffected here.
        //
        // • j == i: set K[i][i] = 1.0 (step 4; step 2 already zeroed it).
        //   f[i] is overwritten unconditionally by step 5, so the
        //   column-into-RHS term is skipped for the diagonal row.
        let mut diag_found = false;
        // CSR col_idx is sorted within each row (faer SymbolicSparseRowMat soft
        // invariant); binary_search is O(log nnz_per_row).
        for j in 0..n {
            let start = row_ptr[j];
            let end = row_ptr[j + 1];
            if let Some(idx) = find_in_row(col_idx, start, end, i) {
                if j == i {
                    // Diagonal: was zeroed by step 2; set to 1.0 (step 4).
                    vals[idx] = 1.0;
                    diag_found = true;
                } else {
                    // Off-diagonal column i entry:
                    // step 1 — read K[j][i], subtract into f[j] before zeroing;
                    // step 3 — zero the stored entry.
                    f[j] -= vals[idx] * u;
                    vals[idx] = 0.0;
                }
            }
        }

        assert!(
            diag_found,
            "DirichletBc {{ dof: {i} }} has no explicit diagonal entry K[{i}][{i}] — \
             the row-elimination algorithm requires a stored diagonal so K[i][i] can \
             be set to 1.0 in place. FEA-assembled K always has a diagonal entry per \
             Task 2916; a missing diagonal indicates the input K is not FEA-assembled.",
        );

        // Step 5: pin RHS — f[i] is overwritten with the prescribed value.
        f[i] = u;
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::{DirichletBc, apply_dirichlet_row_elimination};

    use faer::sparse::SparseRowMat;

    use crate::assembly::tet::element_stiffness_p1;
    use crate::assembly::{AssemblyElement, AssemblyMode, assemble_global_stiffness};
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
        // Both elements reuse the unit-tet k_e — the test only needs an SPD CSR with realistic shared-DOF coupling, not a physically consistent assembly.
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
    // Homogeneous BC: row/col-zeroing and diagonal set
    // -----------------------------------------------------------------------

    /// Homogeneous BC (`u = 0`) zeros row `i` and column `i` of K, sets
    /// `K[i][i] = 1.0`, and sets `f[i] = 0.0`, while leaving all other
    /// K entries and `f[j]` (j ≠ i) bit-for-bit identical.
    ///
    /// Pins the row/col-zeroing and diagonal-set arms of the algorithm
    /// without exercising the column-into-RHS path (which contributes zero
    /// when `u = 0`).  The non-trivial `f = [1.0, 2.0, …, 12.0]` ensures
    /// any accidental f-mutation at j ≠ i would surface immediately.
    #[test]
    fn homogeneous_bc_zeros_row_col_and_sets_unit_diagonal() {
        let mut k = single_p1_k();
        let mut f: Vec<f64> = (1..=12).map(|i| i as f64).collect();

        // Snapshot K and f before applying the BC.
        let k_before: Vec<Vec<f64>> = (0..12)
            .map(|i| (0..12).map(|j| read(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        let constrained_dof = 3usize;
        apply_dirichlet_row_elimination(
            &mut k,
            &mut f,
            &[DirichletBc {
                dof: constrained_dof,
                value: 0.0,
            }],
        );

        let d = constrained_dof;

        // (a) Row d must be all zeros except the diagonal.
        for j in 0..12 {
            if j != d {
                assert_eq!(
                    read(&k, d, j),
                    0.0,
                    "K[{d}][{j}] should be 0.0 after homogeneous BC, got {}",
                    read(&k, d, j),
                );
            }
        }

        // (b) Column d must be all zeros except the diagonal.
        for i in 0..12 {
            if i != d {
                assert_eq!(
                    read(&k, i, d),
                    0.0,
                    "K[{i}][{d}] should be 0.0 after homogeneous BC, got {}",
                    read(&k, i, d),
                );
            }
        }

        // (c) Diagonal K[d][d] must be exactly 1.0.
        assert_eq!(
            read(&k, d, d).to_bits(),
            1.0_f64.to_bits(),
            "K[{d}][{d}] should be 1.0 after homogeneous BC, got {}",
            read(&k, d, d),
        );

        // (d) Unconstrained block must be bit-for-bit identical.
        for i in 0..12 {
            for j in 0..12 {
                if i != d && j != d {
                    let after = read(&k, i, j);
                    assert_eq!(
                        after.to_bits(),
                        k_before[i][j].to_bits(),
                        "K[{i}][{j}] changed unexpectedly: was {}, now {}",
                        k_before[i][j],
                        after,
                    );
                }
            }
        }

        // (e) f[d] must be 0.0; f[j] for j ≠ d must be bit-identical to before
        //     (homogeneous BC subtracts K[j][d] * 0.0 = 0 from f[j]).
        assert_eq!(f[d], 0.0, "f[{d}] should be 0.0 after homogeneous BC",);
        for j in 0..12 {
            if j != d {
                assert_eq!(
                    f[j].to_bits(),
                    f_before[j].to_bits(),
                    "f[{j}] changed unexpectedly: was {}, now {}",
                    f_before[j],
                    f[j],
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Inhomogeneous BC: column-into-RHS subtraction
    // -----------------------------------------------------------------------

    /// Inhomogeneous BC (`u ≠ 0`) subtracts `K_before[j][i] · u` from `f[j]`
    /// for every unconstrained row `j ≠ i`, in addition to zeroing the
    /// constrained row/col and setting the diagonal.
    ///
    /// The assertion is analytic: for a single prescribed BC at DOF `d` with
    /// value `u`, the expected `f_after[j] = f_before[j] - k_before[j][d] * u`
    /// is computed from the pre-call snapshots.  This is a single-summand
    /// FP subtract (no reordering ambiguity), so bit-for-bit equality holds.
    #[test]
    fn inhomogeneous_bc_subtracts_column_into_rhs() {
        let mut k = single_p1_k();
        let mut f: Vec<f64> = (1..=12).map(|i| i as f64).collect();

        // Snapshot K and f before the call.
        let k_before: Vec<Vec<f64>> = (0..12)
            .map(|i| (0..12).map(|j| read(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        let d = 3usize;
        let u = 0.5_f64;
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: d, value: u }]);

        // (a) For every j ≠ d: f_after[j] == f_before[j] - K_before[j][d] * u.
        //     Single-summand subtraction — bit-for-bit equal (no reordering).
        for j in 0..12 {
            if j != d {
                let expected = f_before[j] - k_before[j][d] * u;
                assert_eq!(
                    f[j].to_bits(),
                    expected.to_bits(),
                    "f[{j}]: expected {expected} (f_before={} - K[{j}][{d}]={} * u={u}), \
                     got {}",
                    f_before[j],
                    k_before[j][d],
                    f[j],
                );
            }
        }

        // (b) f[d] must be pinned to u.
        assert_eq!(
            f[d], u,
            "f[{d}] should be {u} (prescribed value), got {}",
            f[d],
        );

        // (c) Row d zeroed, column d zeroed, diagonal = 1.0 (same as homogeneous).
        for j in 0..12 {
            if j != d {
                assert_eq!(read(&k, d, j), 0.0, "K[{d}][{j}] not zero");
                assert_eq!(read(&k, j, d), 0.0, "K[{j}][{d}] not zero");
            }
        }
        assert_eq!(
            read(&k, d, d).to_bits(),
            1.0_f64.to_bits(),
            "K[{d}][{d}] = {} (expected 1.0)",
            read(&k, d, d),
        );

        // (d) Unconstrained block must be bit-for-bit identical.
        for i in 0..12 {
            for j in 0..12 {
                if i != d && j != d {
                    let after = read(&k, i, j);
                    assert_eq!(
                        after.to_bits(),
                        k_before[i][j].to_bits(),
                        "K[{i}][{j}] changed unexpectedly",
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Multi-BC: SPD preservation regression
    // -----------------------------------------------------------------------

    /// Multi-BC row-elimination on a real FEA-assembled K preserves symmetry
    /// within FP tolerance.
    ///
    /// The algorithm zeros both row `i` and column `i` (not just one), so
    /// symmetric K stays symmetric.  A regression where, say, only the row is
    /// zeroed but not the column, or vice-versa, produces a visibly asymmetric
    /// K on a multi-element mesh and surfaces here.
    ///
    /// Tolerance: `|K_after[i][j] - K_after[j][i]| ≤ 1e-9 · max(|…|, 1)`,
    /// mirroring `global_k_is_symmetric_within_fp_tolerance` from Task 2916.
    #[test]
    fn multiple_bcs_preserve_k_symmetry_within_fp_tolerance() {
        let mut k = two_element_shared_face_k(); // 15 × 15
        let mut f: Vec<f64> = (0..15).map(|i| (i + 1) as f64 / 10.0).collect();

        let bcs = [
            DirichletBc { dof: 0, value: 0.0 }, // homogeneous
            DirichletBc {
                dof: 14,
                value: 0.001,
            }, // inhomogeneous
            DirichletBc {
                dof: 7,
                value: -0.002,
            }, // inhomogeneous
        ];
        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        // Check symmetry of K_after over the upper triangle.
        for i in 0..15 {
            for j in i..15 {
                let kij = read(&k, i, j);
                let kji = read(&k, j, i);
                let tol = 1e-9 * kij.abs().max(kji.abs()).max(1.0);
                let delta = (kij - kji).abs();
                assert!(
                    delta <= tol,
                    "K_after[{i}][{j}] = {kij} but K_after[{j}][{i}] = {kji}; \
                     |Δ| = {delta} > tol = {tol}",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Multi-BC: order-independence within FP tolerance
    // -----------------------------------------------------------------------

    /// Applying the same multi-BC list in two different orders produces
    /// bit-identical K and tolerance-equal f.
    ///
    /// K is bit-identical because the row/col zeroing is a set operation
    /// (not accumulate) — writing 0.0 twice gives the same result as once,
    /// regardless of order.  f is tolerance-equal rather than bit-identical
    /// because the column-into-RHS subtraction `f[j] -= K[j][i] * u` is
    /// applied at different K states in the two orderings; after BC₁'s
    /// row-zero zeros `K[k₁][k₂]`, BC₂'s column-into-RHS reads 0.0 from
    /// that entry, so the total f delta is O(ulp · max|f|), well below
    /// `1e-12 · max(|fa|, |fb|, 1)`.
    #[test]
    fn multiple_bcs_are_order_independent_within_fp_tolerance() {
        let bcs_forward = [
            DirichletBc { dof: 0, value: 0.0 }, // homogeneous
            DirichletBc {
                dof: 14,
                value: 0.001,
            }, // inhomogeneous
            DirichletBc {
                dof: 7,
                value: -0.002,
            }, // inhomogeneous
        ];
        let bcs_reverse = [
            DirichletBc {
                dof: 7,
                value: -0.002,
            },
            DirichletBc {
                dof: 14,
                value: 0.001,
            },
            DirichletBc { dof: 0, value: 0.0 },
        ];

        // Pipeline A: forward order.
        let mut k_a = two_element_shared_face_k();
        let mut f_a: Vec<f64> = (0..15).map(|i| (i + 1) as f64 / 10.0).collect();
        apply_dirichlet_row_elimination(&mut k_a, &mut f_a, &bcs_forward);

        // Pipeline B: reverse order.
        let mut k_b = two_element_shared_face_k();
        let mut f_b: Vec<f64> = (0..15).map(|i| (i + 1) as f64 / 10.0).collect();
        apply_dirichlet_row_elimination(&mut k_b, &mut f_b, &bcs_reverse);

        // K must be bit-identical across both orderings.
        for i in 0..15 {
            for j in 0..15 {
                let ka = read(&k_a, i, j);
                let kb = read(&k_b, i, j);
                assert_eq!(
                    ka.to_bits(),
                    kb.to_bits(),
                    "K[{i}][{j}]: forward={ka} but reverse={kb}",
                );
            }
        }

        // f must agree within FP tolerance.
        for j in 0..15 {
            let fa = f_a[j];
            let fb = f_b[j];
            let tol = 1e-12 * fa.abs().max(fb.abs()).max(1.0);
            let delta = (fa - fb).abs();
            assert!(
                delta <= tol,
                "f[{j}]: forward={fa} reverse={fb} |Δ|={delta} > tol={tol}",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Contract violations: panic with descriptive messages
    // -----------------------------------------------------------------------

    /// Out-of-range DOF index panics with a message naming the offending dof
    /// and the matrix dimension.
    ///
    /// The explicit `assert!(bc.dof < k.nrows(), …)` at function entry fires
    /// before any faer indexing occurs, giving a descriptive message rather
    /// than a raw slice-out-of-bounds panic.
    #[test]
    #[should_panic(expected = "DirichletBc")]
    fn out_of_range_dof_panics() {
        let mut k = single_p1_k(); // 12 × 12
        let mut f = vec![0.0_f64; 12];
        // DOF 99 is out of range for a 12-DOF system.
        apply_dirichlet_row_elimination(
            &mut k,
            &mut f,
            &[DirichletBc {
                dof: 99,
                value: 0.0,
            }],
        );
    }

    /// Mismatched f length panics with a message naming both lengths.
    ///
    /// The explicit `assert_eq!(f.len(), k.nrows(), …)` at function entry
    /// fires before any element access, giving a descriptive message that
    /// names both the actual and expected lengths.
    #[test]
    #[should_panic(expected = "f.len()")]
    fn f_length_mismatch_panics() {
        let mut k = single_p1_k(); // 12 × 12
        let mut f = vec![0.0_f64; 7]; // wrong length
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 0, value: 0.0 }]);
    }

    // -----------------------------------------------------------------------
    // End-to-end solve: equilibrium verification
    // -----------------------------------------------------------------------

    /// Applying BCs + solving the eliminated system recovers the correct
    /// equilibrium: original `K · u = f` is satisfied at all unconstrained
    /// DOFs after solving `K_after · u = f_after`.
    ///
    /// This end-to-end test verifies the *column-into-RHS* step by checking a
    /// property that only holds when the step has the correct sign and
    /// magnitude.  The setup is:
    ///
    /// - Single-tet P1 mesh (12 DOFs), E = 1, ν = 0.3, no body forces.
    /// - 7 BCs: fix node 0 (x, y, z), node 1 (y, z) and node 2 (z) to zero
    ///   (removes all 6 rigid body modes), plus an **inhomogeneous** BC at
    ///   node 1 x = 0.1 that drives the free DOFs via the column-into-RHS
    ///   term.
    ///
    /// After solving K_after · u = f_after, algebraic manipulation shows that
    /// for every unconstrained DOF j the equation reduces to
    /// `K_original[j, :] · u = f_original[j]`.  Here f_original = 0, so the
    /// assertion is `|K_original[j, :] · u| < tol`.
    ///
    /// A wrong sign in the column-into-RHS step produces an incorrect
    /// f_after, which in turn produces incorrect free-DOF displacements that
    /// violate the equilibrium condition above.
    ///
    /// Note: checking the free-DOF displacements against analytic values for
    /// a uniaxial-stretch scenario is deferred to the downstream PRD task #12
    /// (CG solver integration), which owns the full solve pipeline.
    #[test]
    fn dirichlet_bc_elimination_satisfies_original_equilibrium_at_free_dofs() {
        use faer::linalg::solvers::Solve;

        let mut k = single_p1_k();
        let n = k.nrows(); // 12

        // Snapshot K before modification (need the original entries to compute K_orig * u).
        let k_orig: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read(&k, i, j)).collect())
            .collect();

        // No external body forces.
        let f_original = vec![0.0_f64; n];

        // BCs: fix node 0 (DOFs 0,1,2), node 1 y (DOF 4), node 1 z (DOF 5),
        // node 2 z (DOF 8) — removes all 6 rigid body modes.
        // Plus inhomogeneous BC: node 1 x (DOF 3) = 0.1.
        let bcs = [
            DirichletBc { dof: 0, value: 0.0 },
            DirichletBc { dof: 1, value: 0.0 },
            DirichletBc { dof: 2, value: 0.0 },
            DirichletBc { dof: 3, value: 0.1 }, // inhomogeneous
            DirichletBc { dof: 4, value: 0.0 },
            DirichletBc { dof: 5, value: 0.0 },
            DirichletBc { dof: 8, value: 0.0 },
        ];
        let constrained: std::collections::HashSet<usize> = bcs.iter().map(|bc| bc.dof).collect();

        let mut f = f_original.clone();
        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        // Dense LU solve: K_after · u = f_after.
        // PartialPivLu works for any invertible matrix; the BC-eliminated K is
        // non-singular because the 7 constrained DOFs span all 6 rigid body modes.
        let k_dense = k.to_dense();
        let plu = k_dense.partial_piv_lu();
        let mut rhs = faer::Mat::<f64>::from_fn(n, 1, |i, _| f[i]);
        plu.solve_in_place(&mut rhs);
        let u = rhs.col_as_slice(0_usize); // &[f64] of length n

        // (a) Each prescribed DOF must match its prescribed value.
        for bc in &bcs {
            assert!(
                (u[bc.dof] - bc.value).abs() < 1e-12,
                "u[{}] = {} ≠ {} (prescribed value)",
                bc.dof,
                u[bc.dof],
                bc.value,
            );
        }

        // (b) For each unconstrained DOF j, the original equilibrium
        //     K_original[j, :] · u = f_original[j] = 0 must hold.
        //     This is the key invariant: a correct column-into-RHS step
        //     encodes the inhomogeneous BC contribution into f_after so that
        //     the solved u satisfies the ORIGINAL balance at free DOFs.
        //     A sign flip in column-into-RHS produces the wrong f_after →
        //     wrong free-DOF displacements → non-zero residual here.
        for j in 0..n {
            if constrained.contains(&j) {
                continue;
            }
            let ku_j: f64 = (0..n).map(|col| k_orig[j][col] * u[col]).sum();
            let residual = (ku_j - f_original[j]).abs();
            assert!(
                residual < 1e-12,
                "K_orig[{j}, :] · u = {ku_j} ≠ f_orig[{j}] = {}; \
                 column-into-RHS may have wrong sign or magnitude",
                f_original[j],
            );
        }
    }

    /// Missing diagonal entry panics with a message naming the dof.
    ///
    /// Synthesise a 3×3 CSR that intentionally omits the `(2, 2)` diagonal
    /// entry.  The `diag_found` guard inside the per-BC loop fires and
    /// panics with the expected message.
    #[test]
    #[should_panic(expected = "diagonal")]
    fn missing_diagonal_entry_panics() {
        use faer::sparse::SparseRowMat;
        use faer::sparse::Triplet;

        // Build a 3×3 matrix without the (2,2) diagonal entry:
        // K = | 1  0  0 |
        //     | 0  2  0 |
        //     | 0  0  0 |   ← no stored entry at (2,2)
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            Triplet::new(0, 0, 1.0),
            Triplet::new(1, 1, 2.0),
            // (2, 2) intentionally absent
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(3, 3, &triplets).expect("valid triplets");
        let mut f = vec![0.0_f64; 3];
        // Applying a BC at dof 2 should panic because K[2][2] has no stored entry.
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 2, value: 0.0 }]);
    }

    // -----------------------------------------------------------------------
    // Step 3: binary_search regression — sparse CSR with non-trivial offsets
    // -----------------------------------------------------------------------

    /// Applies `apply_dirichlet_row_elimination` to a SPARSE 4×4 CSR where
    /// the BC column `i=2` lands at non-trivial positions in per-row ranges.
    ///
    /// Fixture (BC: dof=2, value=0.5):
    ///   Row 0: cols [0,2] — K[0][2] at offset 1 (end of row-0 range).
    ///   Row 1: cols [1,3] — K[1][2] absent → Err arm → row 1 is bit-identical.
    ///   Row 2: cols [0,2,3] — K[2][2] diagonal at offset 1 (middle of range).
    ///   Row 3: cols [2,3] — K[3][2] at offset 0 (start of row-3 range).
    ///
    /// CSR slot layout (try_new_from_triplets with sorted cols):
    ///   Row 0: slots [0..2]  → col_idx=[0,2]
    ///   Row 1: slots [2..4]  → col_idx=[1,3]
    ///   Row 2: slots [4..7]  → col_idx=[0,2,3]
    ///   Row 3: slots [7..9]  → col_idx=[2,3]
    ///
    /// Pins:
    /// - `start + rel` at offset 1 (row 0: start=0, rel=1 → slot 1 for K[0][2]).
    /// - `start + rel` at offset 0 (row 3: start=7, rel=0 → slot 7 for K[3][2]).
    ///   Using `rel` alone would give slot 0 — corrupting K[0][0] silently.
    /// - Err-arm skip (row 1: binary_search([1,3], 2) → Err(1) → no change).
    /// - Diagonal at non-boundary offset (row 2: start=4, rel=1 → slot 5).
    #[test]
    fn apply_dirichlet_to_sparse_csr_with_target_at_row_range_boundaries_eliminates_column_correctly()
     {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 4usize;
        // Build sparse K — only a subset of columns stored per row.
        // Sorted column indices within each row (faer invariant).
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            // Row 0: cols [0,2]   — K[0][2] at offset 1 within row-0 range
            Triplet::new(0, 0, 1.0),
            Triplet::new(0, 2, 2.0),
            // Row 1: cols [1,3]   — K[1][2] intentionally absent
            Triplet::new(1, 1, 3.0),
            Triplet::new(1, 3, 4.0),
            // Row 2: cols [0,2,3] — K[2][2] diagonal at offset 1 within row-2 range
            Triplet::new(2, 0, 5.0),
            Triplet::new(2, 2, 6.0),
            Triplet::new(2, 3, 7.0),
            // Row 3: cols [2,3]   — K[3][2] at offset 0 within row-3 range
            Triplet::new(3, 2, 8.0),
            Triplet::new(3, 3, 9.0),
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];

        // Snapshot K and f before the BC call.
        let k_before: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        // Apply inhomogeneous BC at dof=2, value=0.5.
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 2, value: 0.5 }]);
        let u = 0.5_f64;

        // ── Row 0: K[0][2] zeroed; f[0] adjusted ─────────────────────────────
        // K[0][2] was at slot offset 1 in row 0's range (start=0, rel=1 → idx=1).
        // Using `rel` alone (=1) would correctly index idx=1 here, but for row 3
        // (start=7, rel=0) bare `rel` gives idx=0, corrupting K[0][0].
        assert_eq!(
            read(&k, 0, 2),
            0.0,
            "K[0][2] must be 0.0 (column i=2 eliminated)"
        );
        assert_eq!(
            read(&k, 0, 0).to_bits(),
            k_before[0][0].to_bits(),
            "K[0][0] must be bit-identical (not in BC column or row)"
        );
        let expected_f0 = f_before[0] - k_before[0][2] * u;
        assert_eq!(
            f[0].to_bits(),
            expected_f0.to_bits(),
            "f[0]: expected {expected_f0} = {} - {} * {u}",
            f_before[0],
            k_before[0][2],
        );

        // ── Row 1: bit-identical to pre-call snapshot (Err arm, K[1][2] absent) ─
        for col in 0..n {
            assert_eq!(
                read(&k, 1, col).to_bits(),
                k_before[1][col].to_bits(),
                "K[1][{col}] must be bit-identical (K[1][2] absent → Err arm)"
            );
        }
        assert_eq!(
            f[1].to_bits(),
            f_before[1].to_bits(),
            "f[1] must be bit-identical (no K[1][2] entry)"
        );

        // ── Row 2: diagonal set; row zeroed by step-2 ────────────────────────
        // Step-2 zeros all of row 2 before the per-row scan; the diagonal arm
        // then writes K[2][2]=1.0 at offset 1 of row-2's stored range (slot 5).
        assert_eq!(
            read(&k, 2, 2).to_bits(),
            1.0_f64.to_bits(),
            "K[2][2] must be 1.0 (BC diagonal)"
        );
        assert_eq!(
            read(&k, 2, 0),
            0.0,
            "K[2][0] must be 0.0 (zeroed by step-2)"
        );
        assert_eq!(
            read(&k, 2, 3),
            0.0,
            "K[2][3] must be 0.0 (zeroed by step-2)"
        );
        assert_eq!(f[2].to_bits(), u.to_bits(), "f[2] must be u=0.5 (pinned)");

        // ── Row 3: K[3][2] zeroed; f[3] adjusted ────────────────────────────
        // K[3][2] is at slot offset 0 in row 3's range (start=7, rel=0 → idx=7).
        // A buggy impl using `rel` (=0) instead of `start + rel` (=7) would
        // silently write to vals[0] = K[0][0] and leave K[3][2] intact.
        assert_eq!(
            read(&k, 3, 2),
            0.0,
            "K[3][2] must be 0.0 (column i=2 eliminated)"
        );
        assert_eq!(
            read(&k, 3, 3).to_bits(),
            k_before[3][3].to_bits(),
            "K[3][3] must be bit-identical (not in BC column)"
        );
        let expected_f3 = f_before[3] - k_before[3][2] * u;
        assert_eq!(
            f[3].to_bits(),
            expected_f3.to_bits(),
            "f[3]: expected {expected_f3} = {} - {} * {u}",
            f_before[3],
            k_before[3][2],
        );
    }

    // -----------------------------------------------------------------------
    // Empty BC list: no-op contract
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Debug-only: sorted col_idx assertion
    // -----------------------------------------------------------------------

    /// Debug-only: `apply_dirichlet_row_elimination` panics in debug builds
    /// when any row of `K` has unsorted column indices.
    ///
    /// Fixture: 3×3 sparse K where row 0 has col_idx `[2, 0]` — out of
    /// order.  Bypasses `try_new_from_triplets` (which sorts on construction)
    /// by using `SymbolicSparseRowMat::new_unsorted_checked` +
    /// `SparseRowMat::new`, so the deliberate sort violation reaches
    /// `apply_dirichlet_row_elimination` intact.
    ///
    /// The new `#[cfg(debug_assertions)]` sorted-col_idx walk at function
    /// entry must fire before any `binary_search` work, producing a panic
    /// message that contains "unsorted".
    ///
    /// Gated by `#[cfg(debug_assertions)]` so `cargo test --release` does
    /// not false-fail — `debug_assert!` is elided in release builds, meaning
    /// no panic would be raised and `#[should_panic]` would falsely trip.
    /// Same pattern as the debug-gated `#[should_panic]` tests in
    /// `shell_boundary.rs`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unsorted")]
    fn apply_dirichlet_panics_in_debug_when_col_idx_unsorted_within_row() {
        use faer::sparse::SymbolicSparseRowMat;

        // Build a 3×3 CSR with row 0 having col_idx [2, 0] — unsorted.
        //   row_ptr = [0, 2, 3, 4]:
        //     row 0 → slots 0..2  (col_idx = [2, 0], out of order)
        //     row 1 → slots 2..3  (col_idx = [1])
        //     row 2 → slots 3..4  (col_idx = [2])
        //   vals = [1.0, 2.0, 3.0, 4.0]
        // K has explicit diagonals at rows 1 and 2 but not row 0.  The debug
        // walk fires on row 0 (adjacent entries 2 ≥ 0) before any BC work.
        let symbolic = SymbolicSparseRowMat::<usize>::new_unsorted_checked(
            3_usize,
            3_usize,
            vec![0_usize, 2, 3, 4],
            None,
            vec![2_usize, 0, 1, 2],
        );
        let mut k = SparseRowMat::<usize, f64>::new(symbolic, vec![1.0, 2.0, 3.0, 4.0]);
        let mut f = vec![0.0_f64; 3];
        // BC at dof=0; the sorted-col_idx debug check runs at function entry
        // and fires on row 0 before any binary_search work.
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 0, value: 0.0 }]);
    }

    /// Debug-only: sorted col_idx assertion fires on a later row, not just
    /// row 0 — verifies the walk covers all rows, not just the first.
    ///
    /// Fixture: 3×3 CSR where row 2 has col_idx `[2, 0]` (out of order).
    /// Rows 0 and 1 are sorted, so only the walk reaching row 2 triggers
    /// the assertion.  Regression guard for a hypothetical bug that only
    /// checked windows on the first row.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unsorted")]
    fn apply_dirichlet_panics_in_debug_when_col_idx_unsorted_on_later_row() {
        use faer::sparse::SymbolicSparseRowMat;

        // row_ptr = [0, 1, 2, 4]:
        //   row 0 → slot 0     (col_idx = [0])  — sorted
        //   row 1 → slot 1     (col_idx = [1])  — sorted
        //   row 2 → slots 2..4 (col_idx = [2, 0]) — UNSORTED
        let symbolic = SymbolicSparseRowMat::<usize>::new_unsorted_checked(
            3_usize,
            3_usize,
            vec![0_usize, 1, 2, 4],
            None,
            vec![0_usize, 1, 2, 0],
        );
        let mut k = SparseRowMat::<usize, f64>::new(symbolic, vec![1.0, 2.0, 3.0, 4.0]);
        let mut f = vec![0.0_f64; 3];
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 0, value: 0.0 }]);
    }

    /// Debug-only: the strictly-increasing (`<`) assertion catches duplicate
    /// column indices within a row, not merely out-of-order pairs.
    ///
    /// Fixture: 3×3 CSR where row 0 has col_idx `[0, 0]` — a duplicate
    /// column, which breaks `find_in_row`'s binary_search uniqueness
    /// assumption and is caught by `w[0] < w[1]` (0 < 0 is false).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unsorted")]
    fn apply_dirichlet_panics_in_debug_when_col_idx_has_duplicate_in_row() {
        use faer::sparse::SymbolicSparseRowMat;

        // row_ptr = [0, 2, 3, 4]:
        //   row 0 → slots 0..2 (col_idx = [0, 0]) — DUPLICATE column
        //   row 1 → slot 2     (col_idx = [1])
        //   row 2 → slot 3     (col_idx = [2])
        let symbolic = SymbolicSparseRowMat::<usize>::new_unsorted_checked(
            3_usize,
            3_usize,
            vec![0_usize, 2, 3, 4],
            None,
            vec![0_usize, 0, 1, 2],
        );
        let mut k = SparseRowMat::<usize, f64>::new(symbolic, vec![1.0, 2.0, 3.0, 4.0]);
        let mut f = vec![0.0_f64; 3];
        apply_dirichlet_row_elimination(&mut k, &mut f, &[DirichletBc { dof: 0, value: 0.0 }]);
    }

    // -----------------------------------------------------------------------
    // Empty BC list: no-op contract
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
