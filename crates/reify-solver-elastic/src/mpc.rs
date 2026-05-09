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

use faer::sparse::SparseRowMat;

/// Apply multi-point constraints to the global stiffness matrix `K` and load
/// vector `f` via row-elimination, in place.
///
/// For each `MpcRow` in `rows` (in slice order), the function substitutes the
/// pivot DOF `p = dofs[0]` using:
///
/// ```text
///     u[p] = (rhs − Σᵢ>0 coeffs[i] · u[dofs[i]]) / coeffs[0]
/// ```
///
/// letting `c0 = coeffs[0]`, `αᵢ = −coeffs[i] / c0` for `i > 0`, and
/// `β = rhs / c0`.  The four-step elimination is:
///
/// 1. **Column-into-RHS + redistribution** — for every row `j ≠ p`:
///    - read original `K[j][p]` (before any zeroing);
///    - `f[j] -= K[j][p] · β` (inhomogeneous RHS contribution);
///    - for each `i > 0`: `K[j][dofs[i]] += K[j][p] · αᵢ` (redistribute the
///      eliminated column into the other constraint DOFs);
///    - zero the stored `K[j][p]` entry.
/// 2. **Zero pivot row `p`** — set all stored values in row `p` to `0.0`.
/// 3. **Set pivot equation in row `p`** — `K[p][p] = 1`; for each `i > 0`,
///    `K[p][dofs[i]] = −αᵢ`; the rest of row `p` stays zero.
/// 4. **Pin RHS** — `f[p] = β`.
///
/// The sparsity pattern of `K` is not changed — only stored values are
/// overwritten.  Every `(j, dofs[i])` entry that step 1 writes, and every
/// `(p, dofs[i])` entry that step 3 writes, **must already be stored** in K.
/// Missing entries are detected and the function panics with a descriptive
/// message naming the missing `(row, col)` pair.  Downstream MPC-aware
/// assembly is responsible for pre-allocating these entries; test fixtures
/// use `try_new_from_triplets` with explicit zero entries at each required
/// position.
///
/// # Inhomogeneous constraints
///
/// When `rhs ≠ 0`, the pivot displacement is `β = rhs / coeffs[0]` rather
/// than zero.  The column-into-RHS subtraction `f[j] -= K_before[j][p] · β`
/// runs unconditionally for all `j ≠ p`, encoding the inhomogeneous constraint
/// into the free-DOF load vector so that the solved displacements satisfy the
/// original constraint.  `f[p] = β` pins the pivot directly.  There is no
/// short-circuit on `β == 0`; a homogeneous constraint with `rhs = 0` simply
/// subtracts zero, which is bit-identical to not subtracting at all.  This
/// mirrors the inhomogeneous BC path in `apply_dirichlet_row_elimination`.
///
/// # Empty slice
///
/// An empty `rows` slice is a perfect identity operation — no stored value in
/// K is touched, no `f[j]` changes.
///
/// # Order-independence
///
/// For MPC rows with **disjoint pivot DOFs**, applying them in any order
/// produces bit-identical K and tolerance-equal f.  The mechanism mirrors
/// Dirichlet: MPC₁'s row-zero on row `p₁` zeros `K[p₁][p₂]`, so when MPC₂
/// later reads `K[p₂][p₁]` for its column-into-RHS step it reads the
/// still-original value (MPC₁'s column zeroing happens row-by-row, not
/// column-by-column, so `K[p₂][p₁]` is unaffected until MPC₂ touches it).
///
/// # Panics
///
/// - `f.len() != k.nrows()` — load vector and matrix dimension mismatch.
/// - `k.nrows() != k.ncols()` — K must be square.
/// - Any `dof` in any `MpcRow` is `>= k.nrows()` — DOF out of range.
/// - Any redistribution target `K[j][dofs[i]]` or pivot-row target
///   `K[p][dofs[i]]` has no stored entry in K — sparsity precondition
///   violated (see above).
pub fn apply_mpc_row_elimination(
    k: &mut SparseRowMat<usize, f64>,
    f: &mut [f64],
    rows: &[MpcRow],
) {
    // --- Contract checks ---
    assert_eq!(
        f.len(),
        k.nrows(),
        "apply_mpc_row_elimination: f.len() = {} but k.nrows() = {}; expected f.len() == k.nrows()",
        f.len(),
        k.nrows(),
    );
    assert_eq!(
        k.nrows(),
        k.ncols(),
        "apply_mpc_row_elimination: k must be square: k.nrows() = {} but k.ncols() = {}",
        k.nrows(),
        k.ncols(),
    );
    for row in rows {
        for &dof in &row.dofs {
            assert!(
                dof < k.nrows(),
                "MpcRow has dof = {} but k.nrows() = {}; DOF index out of range",
                dof,
                k.nrows(),
            );
        }
    }

    // Debug-only: duplicate pivot DOFs produce undefined output. Surface eagerly.
    #[cfg(debug_assertions)]
    {
        let mut pivots: Vec<usize> = rows.iter().map(|r| r.dofs[0]).collect();
        pivots.sort_unstable();
        for w in pivots.windows(2) {
            assert_ne!(
                w[0], w[1],
                "duplicate MpcRow pivot {} in rows slice; duplicate pivots produce \
                 undefined output — deduplicate before calling apply_mpc_row_elimination",
                w[0],
            );
        }
    }

    for mpc in rows {
        let p = mpc.dofs[0];
        let c0 = mpc.coeffs[0];
        let beta = mpc.rhs / c0;
        // αᵢ = −coeffs[i] / c0 for i > 0
        let alphas: Vec<f64> = mpc.coeffs[1..].iter().map(|&c| -c / c0).collect();
        let other_dofs = &mpc.dofs[1..];

        let (sym, vals) = k.parts_mut();
        let row_ptr = sym.row_ptr();
        let col_idx = sym.col_idx();
        let n = sym.nrows();

        // Step 2: zero pivot row p entirely (before the fused loop so column
        // entries in row p are zeroed; step 3 will write the pivot equation back).
        vals[row_ptr[p]..row_ptr[p + 1]].fill(0.0);

        // Fused steps 1 + partial 3: scan every row j.
        //
        // For j ≠ p:
        //   - locate K[j][p] in CSR;
        //   - read its value BEFORE zeroing (capture-then-update-then-zero);
        //   - f[j] -= captured · β (step 1 RHS);
        //   - for each i: locate K[j][dofs[i+1]] and add captured · αᵢ (step 1 redistribution);
        //   - zero K[j][p] (column p elimination).
        //
        // For j == p: the row was already zeroed in step 2; step 3 fills it after
        // this loop.  We skip the column-p scan for j == p since step 2 zeroed
        // K[p][p] already.
        for j in 0..n {
            if j == p {
                continue;
            }
            let start = row_ptr[j];
            let end = row_ptr[j + 1];
            // Find K[j][p].
            let mut kjp = 0.0_f64;
            let mut kjp_idx: Option<usize> = None;
            for idx in start..end {
                if col_idx[idx] == p {
                    kjp = vals[idx];
                    kjp_idx = Some(idx);
                    break;
                }
            }
            if kjp == 0.0 {
                // Entry not stored or is structural zero — redistribution is zero, skip.
                // (If the entry isn't stored at all, no write is needed.)
                if let Some(idx) = kjp_idx {
                    vals[idx] = 0.0;
                }
                continue;
            }
            // f[j] -= K[j][p] · β
            f[j] -= kjp * beta;
            // For each i > 0: K[j][dofs[i]] += K[j][p] · αᵢ
            for (i, (&di, &ai)) in other_dofs.iter().zip(alphas.iter()).enumerate() {
                let mut found = false;
                for idx in start..end {
                    if col_idx[idx] == di {
                        vals[idx] += kjp * ai;
                        found = true;
                        break;
                    }
                }
                assert!(
                    found,
                    "MpcRow apply: missing K[{}][{}] entry — required for redistribution \
                     K[j][dofs[{}]] += K[j][p]·α; ensure assembly pre-allocates this entry",
                    j, di, i + 1,
                );
            }
            // Zero K[j][p] (column p eliminated for this row).
            if let Some(idx) = kjp_idx {
                vals[idx] = 0.0;
            }
        }

        // Step 3: write pivot equation in row p.
        // K[p][p] = 1; K[p][dofs[i]] = -αᵢ for i > 0.
        let start_p = row_ptr[p];
        let end_p = row_ptr[p + 1];
        // Set diagonal K[p][p] = 1.
        let mut diag_found = false;
        for idx in start_p..end_p {
            if col_idx[idx] == p {
                vals[idx] = 1.0;
                diag_found = true;
                break;
            }
        }
        assert!(
            diag_found,
            "MpcRow apply: missing K[{p}][{p}] diagonal entry — required to set pivot \
             equation K[p][p] = 1; ensure assembly pre-allocates the diagonal",
        );
        // Set K[p][dofs[i]] = -αᵢ.
        for (i, (&di, &ai)) in other_dofs.iter().zip(alphas.iter()).enumerate() {
            let mut found = false;
            for idx in start_p..end_p {
                if col_idx[idx] == di {
                    vals[idx] = -ai;
                    found = true;
                    break;
                }
            }
            assert!(
                found,
                "MpcRow apply: missing K[{p}][{}] entry — required to set pivot equation \
                 K[p][dofs[{}]] = -αᵢ; ensure assembly pre-allocates this entry",
                di, i + 1,
            );
        }

        // Step 4: pin RHS.
        f[p] = beta;
    }
}

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

impl MpcRow {
    /// Construct a validated `MpcRow` from DOF indices, coefficients, and RHS.
    ///
    /// # Panics
    ///
    /// - `dofs.len() != coeffs.len()` — lengths must match element-wise.
    /// - `dofs.is_empty()` — at least one DOF is required.
    /// - `coeffs[0] == 0.0` or `!coeffs[0].is_finite()` — the pivot coefficient
    ///   (`coeffs[0]`) must be non-zero and finite so that
    ///   `u[dofs[0]] = (rhs − Σᵢ>0 coeffs[i]·u[dofs[i]]) / coeffs[0]`
    ///   is well-defined. See the module-level doc on the pivot convention.
    /// - `rhs` is not finite — `rhs` must be a finite number.
    pub fn new(dofs: Vec<usize>, coeffs: Vec<f64>, rhs: f64) -> Self {
        assert_eq!(
            dofs.len(),
            coeffs.len(),
            "MpcRow::new: dofs.len() = {} but coeffs.len() = {}; expected equal lengths",
            dofs.len(),
            coeffs.len(),
        );
        assert!(
            !dofs.is_empty(),
            "MpcRow::new: at least one DOF is required",
        );
        assert!(
            coeffs[0] != 0.0,
            "MpcRow::new: pivot coefficient coeffs[0] must be non-zero; got {}",
            coeffs[0],
        );
        assert!(
            coeffs[0].is_finite(),
            "MpcRow::new: pivot coefficient coeffs[0] must be finite; got {}",
            coeffs[0],
        );
        assert!(
            rhs.is_finite(),
            "MpcRow::new: rhs must be finite; got {}",
            rhs,
        );
        MpcRow { dofs, coeffs, rhs }
    }
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

    // -----------------------------------------------------------------------
    // Step 1 (RED): MpcRow::new constructor contract tests
    // -----------------------------------------------------------------------

    /// `MpcRow::new` must panic when `dofs.len() != coeffs.len()`.
    #[test]
    #[should_panic(expected = "dofs.len()")]
    fn mpc_row_new_panics_on_length_mismatch() {
        // len 2 vs len 1 — must panic
        let _ = MpcRow::new(vec![1, 2], vec![1.0], 0.0);
    }

    /// `MpcRow::new` must panic when `coeffs[0]` (the pivot coefficient) is zero.
    #[test]
    #[should_panic(expected = "pivot")]
    fn mpc_row_new_panics_on_zero_pivot_coefficient() {
        // zero pivot — must panic
        let _ = MpcRow::new(vec![3, 7], vec![0.0, 1.0], 0.0);
    }

    /// `MpcRow::new` constructs and the fields round-trip exactly.
    #[test]
    fn mpc_row_new_round_trips_dofs_coeffs_rhs() {
        let row = MpcRow::new(vec![3, 7, 11], vec![1.0, -0.5, 0.5], 0.25);
        assert_eq!(row.dofs, vec![3, 7, 11]);
        assert_eq!(row.coeffs, vec![1.0, -0.5, 0.5]);
        assert_eq!(row.rhs.to_bits(), 0.25_f64.to_bits());
    }

    // -----------------------------------------------------------------------
    // Step 3 (RED): apply_mpc_row_elimination — homogeneous single MPC
    // -----------------------------------------------------------------------

    /// Build a fully-dense 5×5 sparse K and apply a single homogeneous MpcRow
    /// with pivot p=0 and other DOFs d1=2, d2=4.
    ///
    /// Asserts:
    /// (a) pivot row 0 reads: K[0][0]=1, K[0][2]=-α_1=-0.5, K[0][4]=-α_2=+0.5,
    ///     all other entries in row 0 are zero.
    /// (b) for j in {1,2,3,4}: K[j][0] is zeroed (column p eliminated);
    ///     K[j][2] == K_before[j][2] + K_before[j][0]·α_1 (bit-identical);
    ///     K[j][4] == K_before[j][4] + K_before[j][0]·α_2 (bit-identical).
    /// (c) f[0] = β = 0 (homogeneous → rhs/c0 = 0/2 = 0).
    /// (d) f[j] for j in {1,2,3,4} bit-identical to before (β=0 → no subtract).
    /// (e) all other K entries (rows 1..5, cols not in {0,2,4}) bit-identical to snapshot.
    ///
    /// RED: `apply_mpc_row_elimination` is not yet defined.
    #[test]
    fn single_homogeneous_mpc_zeros_pivot_row_redistributes_column_and_pins_pivot_to_constraint() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        // Build a fully-dense 5×5 K: K[i][j] = (i*5 + j + 1) as f64
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, (i * 5 + j + 1) as f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=5).map(|i| i as f64).collect();

        // Snapshot K and f
        let k_before: Vec<Vec<f64>> =
            (0..n).map(|i| (0..n).map(|j| read_k(&k, i, j)).collect()).collect();
        let f_before = f.clone();

        // MpcRow: pivot p=0, d1=2, d2=4; coeffs=[2.0, -1.0, 1.0], rhs=0
        // α_1 = -(-1.0)/2.0 = 0.5, α_2 = -(1.0)/2.0 = -0.5, β = 0/2 = 0
        let row = MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 0.0);
        let alpha_1 = 0.5_f64;  // -coeffs[1]/c0
        let alpha_2 = -0.5_f64; // -coeffs[2]/c0

        apply_mpc_row_elimination(&mut k, &mut f, &[row]);

        let p = 0usize;
        let d1 = 2usize;
        let d2 = 4usize;

        // (a) Pivot row
        assert_eq!(read_k(&k, p, p).to_bits(), 1.0_f64.to_bits(), "K[0][0] must be 1.0");
        assert_eq!(
            read_k(&k, p, d1).to_bits(),
            (-alpha_1).to_bits(),
            "K[0][2] must be -α_1 = {}",
            -alpha_1
        );
        assert_eq!(
            read_k(&k, p, d2).to_bits(),
            (-alpha_2).to_bits(),
            "K[0][4] must be -α_2 = {}",
            -alpha_2
        );
        for j in 0..n {
            if j != p && j != d1 && j != d2 {
                assert_eq!(read_k(&k, p, j), 0.0, "K[0][{j}] should be 0 in pivot row");
            }
        }

        // (b) Non-pivot rows
        for j in 1..n {
            assert_eq!(read_k(&k, j, p), 0.0, "K[{j}][0] should be 0 (column p eliminated)");
            let expected_d1 = k_before[j][d1] + k_before[j][p] * alpha_1;
            assert_eq!(
                read_k(&k, j, d1).to_bits(),
                expected_d1.to_bits(),
                "K[{j}][{d1}] mismatch: got {}, expected {}",
                read_k(&k, j, d1),
                expected_d1,
            );
            let expected_d2 = k_before[j][d2] + k_before[j][p] * alpha_2;
            assert_eq!(
                read_k(&k, j, d2).to_bits(),
                expected_d2.to_bits(),
                "K[{j}][{d2}] mismatch: got {}, expected {}",
                read_k(&k, j, d2),
                expected_d2,
            );
        }

        // (c) f[0] = β = 0
        assert_eq!(f[p].to_bits(), 0.0_f64.to_bits(), "f[0] must be β=0");

        // (d) f[j] for j≠0 bit-identical (β=0 → no subtract)
        for j in 1..n {
            assert_eq!(
                f[j].to_bits(),
                f_before[j].to_bits(),
                "f[{j}] should be unchanged (homogeneous β=0)"
            );
        }

        // (e) Other K entries (rows 1..5, cols not in {0, 2, 4}) bit-identical
        for j in 1..n {
            for col in 0..n {
                if col != p && col != d1 && col != d2 {
                    assert_eq!(
                        read_k(&k, j, col).to_bits(),
                        k_before[j][col].to_bits(),
                        "K[{j}][{col}] should be unchanged"
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 5 (RED): inhomogeneous MPC subtracts column into RHS and pins β
    // -----------------------------------------------------------------------

    /// Inhomogeneous MpcRow (rhs=1.5, β=0.75) subtracts `K_before[j][p]·β`
    /// from `f[j]` for all `j ≠ p` and pins `f[p] = β`.
    ///
    /// Same 5×5 fully-dense fixture as step-3.  The K redistribution terms
    /// (αᵢ depend only on coeffs, not rhs) are identical to the homogeneous case.
    ///
    /// RED if any short-circuit on β==0 suppresses the `f[j] -= ... · β` path.
    #[test]
    fn inhomogeneous_mpc_subtracts_column_into_rhs_and_pins_pivot_to_beta() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, (i * 5 + j + 1) as f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=5).map(|i| i as f64).collect();

        let k_before: Vec<Vec<f64>> =
            (0..n).map(|i| (0..n).map(|j| read_k(&k, i, j)).collect()).collect();
        let f_before = f.clone();

        // rhs=1.5 → β=1.5/2.0=0.75, α_1=0.5, α_2=-0.5 (same as step-3)
        let row = MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 1.5);
        let beta = 0.75_f64;
        let alpha_1 = 0.5_f64;
        let alpha_2 = -0.5_f64;

        apply_mpc_row_elimination(&mut k, &mut f, &[row]);

        let p = 0usize;
        let d1 = 2usize;
        let d2 = 4usize;

        // (a) f[0] = β = 0.75 exactly
        assert_eq!(f[p].to_bits(), beta.to_bits(), "f[0] must be β={beta}");

        // (b) f[j] for j≠0 must be f_before[j] - K_before[j][0] · β (single-summand)
        for j in 1..n {
            let expected = f_before[j] - k_before[j][p] * beta;
            assert_eq!(
                f[j].to_bits(),
                expected.to_bits(),
                "f[{j}]: expected {expected} (f_before={} - K[{j}][0]={} · β={beta}), got {}",
                f_before[j],
                k_before[j][p],
                f[j],
            );
        }

        // (c) Pivot row: K[0][0]=1, K[0][2]=-α_1=-0.5, K[0][4]=-α_2=+0.5, rest zero
        assert_eq!(read_k(&k, p, p).to_bits(), 1.0_f64.to_bits(), "K[0][0] must be 1.0");
        assert_eq!(
            read_k(&k, p, d1).to_bits(),
            (-alpha_1).to_bits(),
            "K[0][2] must be -α_1={}", -alpha_1
        );
        assert_eq!(
            read_k(&k, p, d2).to_bits(),
            (-alpha_2).to_bits(),
            "K[0][4] must be -α_2={}", -alpha_2
        );
        for j in 0..n {
            if j != p && j != d1 && j != d2 {
                assert_eq!(read_k(&k, p, j), 0.0, "K[0][{j}] should be 0 in pivot row");
            }
        }

        // (d) Same K redistribution as homogeneous (αᵢ don't depend on rhs)
        for j in 1..n {
            assert_eq!(read_k(&k, j, p), 0.0, "K[{j}][0] should be 0 (column p eliminated)");
            let expected_d1 = k_before[j][d1] + k_before[j][p] * alpha_1;
            assert_eq!(
                read_k(&k, j, d1).to_bits(),
                expected_d1.to_bits(),
                "K[{j}][{d1}] mismatch",
            );
            let expected_d2 = k_before[j][d2] + k_before[j][p] * alpha_2;
            assert_eq!(
                read_k(&k, j, d2).to_bits(),
                expected_d2.to_bits(),
                "K[{j}][{d2}] mismatch",
            );
        }
    }

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>`, returning 0.0 if
    /// the entry is not explicitly stored.
    fn read_k(k: &faer::sparse::SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }
}
