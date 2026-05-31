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

use crate::sparse_util::find_in_row;

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
/// overwritten.  Every `(j, dofs[i])` entry that step 1 could write, and
/// every `(p, dofs[i])` entry that step 3 writes, **must already be stored**
/// in K.  The function asserts this eagerly: if `K[j][p]` is stored (even as
/// a structural zero), all redistribution targets `K[j][dofs[i]]` are
/// looked up and their absence panics with a descriptive message naming the
/// missing `(row, col)` pair — regardless of whether `K[j][p]` is zero.
/// (Only truly absent `K[j][p]` entries — those with no stored slot — skip
/// the redistribution check, since they can never contribute to the write.)
/// Downstream MPC-aware assembly is responsible for pre-allocating these
/// entries; test fixtures use `try_new_from_triplets` with explicit zero
/// entries at each required position.
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
/// # End-to-end recovery property
///
/// After applying MPCs and solving the eliminated system `K_after · u = f_after`,
/// the original constraint `Σᵢ coeffs[i] · u[dofs[i]] = rhs` is satisfied to
/// FP tolerance for each `MpcRow` in the input slice.  The mechanism: the pivot
/// row equation `u[p] = β + Σᵢ>0 αᵢ · u[dofs[i]]` is directly encoded into
/// `K_after`'s row `p` and `f_after[p]`, so the solve recovers `u[p]`
/// consistent with the constraint by construction.  The
/// `shell_tet_tying_constraints_compose_with_apply_mpc_row_elimination_to_satisfy_constraint_after_solve`
/// test is the regression pin for this invariant — paralleling
/// `dirichlet_bc_elimination_satisfies_original_equilibrium_at_free_dofs` in
/// `apply_dirichlet_row_elimination`.
///
/// # Empty slice
///
/// An empty `rows` slice is a perfect identity operation — no stored value in
/// K is touched, no `f[j]` changes.
///
/// # Order-independence
///
/// For MPC rows with **pairwise-disjoint full DOF sets** (no two rows share
/// any DOF index, pivot or otherwise), applying them in any order produces
/// bit-identical K and tolerance-equal f.  When the full DOF sets are
/// disjoint, each row's redistribution writes touch disjoint columns, so no
/// row's update can affect the input values read by any other row.
///
/// Note: disjoint *pivots alone* are not sufficient — if two rows share a
/// non-pivot DOF, one row's redistribution can modify a column entry that
/// the other row then reads, making the result order-dependent.
///
/// # Panics
///
/// - `f.len() != k.nrows()` — load vector and matrix dimension mismatch.
/// - `k.nrows() != k.ncols()` — K must be square.
/// - Any `dof` in any `MpcRow` is `>= k.nrows()` — DOF out of range.
/// - Any redistribution target `K[j][dofs[i]]` or pivot-row target
///   `K[p][dofs[i]]` has no stored entry in K — sparsity precondition
///   violated (see above).
/// - In debug builds, panics if `K` has unsorted (or duplicate) column
///   indices within any row — `col_idx[start..end]` must be strictly
///   increasing. **Release builds silently produce wrong results** (binary
///   search on unsorted data returns unspecified Ok/Err — sort col_idx, e.g.
///   via `try_new_from_triplets`, before calling).
pub fn apply_mpc_row_elimination(k: &mut SparseRowMat<usize, f64>, f: &mut [f64], rows: &[MpcRow]) {
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

    // Debug-only: walk all rows and assert strictly-increasing col_idx.
    // binary_search on an unsorted slice returns an unspecified result —
    // a wrong Ok/Err corrupts K[j][p] redistribution writes or f[j]
    // subtracts silently. Surface it eagerly here before any per-MpcRow work.
    // O(nnz) total per call, paid only in debug builds. Asserting strictly
    // increasing (`<`) also catches duplicate (row, col) entries.
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
                    "apply_mpc_row_elimination: col_idx is unsorted within row {j}: \
                     adjacent entries {w0} >= {w1}; col_idx[start..end] must be strictly \
                     increasing (faer try_new_from_triplets guarantees this; ensure \
                     col_idx is sorted before calling apply_mpc_row_elimination)",
                    w0 = w[0],
                    w1 = w[1],
                );
            }
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
        //   - if the entry has no stored slot, skip (nothing to redistribute or zero);
        //   - if the entry IS stored (even as a structural zero), run the
        //     redistribution-target lookup eagerly — missing targets are caught here
        //     before a future non-zero K[j][p] would trigger a seemingly-unprovoked panic;
        //   - read its value BEFORE zeroing (capture-then-update-then-zero);
        //   - f[j] -= captured · β (step 1 RHS, skipped when captured == 0);
        //   - for each i: locate K[j][dofs[i+1]] (assert found) and add captured · αᵢ
        //     (step 1 redistribution, skipped when captured == 0);
        //   - zero K[j][p] (column p elimination).
        //
        // For j == p: the row was already zeroed in step 2; step 3 fills it after
        // this loop.  We skip the column-p scan for j == p since step 2 zeroed
        // K[p][p] already.
        // CSR col_idx is sorted within each row (faer SymbolicSparseRowMat soft
        // invariant); binary_search is O(log nnz_per_row).
        for j in 0..n {
            if j == p {
                continue;
            }
            let start = row_ptr[j];
            let end = row_ptr[j + 1];
            // Find K[j][p].  None → no stored entry → skip entirely.
            let Some(kjp_store_idx) = find_in_row(col_idx, start, end, p) else {
                continue;
            };
            let kjp = vals[kjp_store_idx];
            // K[j][p] IS stored (possibly as a structural zero).  Run the
            // redistribution-target lookup regardless of kjp's value so that
            // missing sparsity entries are caught eagerly.
            if kjp != 0.0 {
                // f[j] -= K[j][p] · β
                f[j] -= kjp * beta;
            }
            // For each i > 0: K[j][dofs[i]] += K[j][p] · αᵢ (assert target exists)
            for (i, (&di, &ai)) in other_dofs.iter().zip(alphas.iter()).enumerate() {
                match find_in_row(col_idx, start, end, di) {
                    Some(idx) => {
                        if kjp != 0.0 {
                            vals[idx] += kjp * ai;
                        }
                    }
                    None => panic!(
                        "MpcRow apply: missing K[{}][{}] entry — required for redistribution \
                         K[j][dofs[{}]] += K[j][p]·α; ensure assembly pre-allocates this entry",
                        j,
                        di,
                        i + 1,
                    ),
                }
            }
            // Zero K[j][p] (column p eliminated for this row).
            vals[kjp_store_idx] = 0.0;
        }

        // Step 3: write pivot equation in row p.
        // K[p][p] = 1; K[p][dofs[i]] = -αᵢ for i > 0.
        let start_p = row_ptr[p];
        let end_p = row_ptr[p + 1];
        // Set diagonal K[p][p] = 1.
        match find_in_row(col_idx, start_p, end_p, p) {
            Some(idx) => vals[idx] = 1.0,
            None => panic!(
                "MpcRow apply: missing K[{p}][{p}] diagonal entry — required to set pivot \
                 equation K[p][p] = 1; ensure assembly pre-allocates the diagonal",
            ),
        }
        // Set K[p][dofs[i]] = -αᵢ.
        for (i, (&di, &ai)) in other_dofs.iter().zip(alphas.iter()).enumerate() {
            match find_in_row(col_idx, start_p, end_p, di) {
                Some(idx) => vals[idx] = -ai,
                None => panic!(
                    "MpcRow apply: missing K[{p}][{}] entry — required to set pivot equation \
                     K[p][dofs[{}]] = -αᵢ; ensure assembly pre-allocates this entry",
                    di,
                    i + 1,
                ),
            }
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

    /// Build the canonical 6 `MpcRow`s for a shell/tet junction constraint.
    ///
    /// Returns 6 rows:
    ///
    /// **Three displacement-matching rows** (mid-surface tying), one per axis `a`:
    /// ```text
    ///     u_shell_a − u_tet_mid_a = 0
    /// ```
    /// Pivot is `shell_disp_dofs[a]` with coefficient `+1.0`.
    ///
    /// **Three rotation/gradient rows**, derived from
    /// `(u_tet_top − u_tet_bot) = (θ × n) · h`:
    /// ```text
    ///     −ε_abc · θ_b · n_c · h + (u_top_a − u_bot_a) = 0   for each axis a
    /// ```
    /// The pivot is the shell-rotation DOF with the largest-magnitude rotational
    /// coefficient.  If both rotational coefficients for axis `a` are `< 1e-12`
    /// in absolute value (the "drilling" axis — parallel to the normal), the
    /// row degenerates to the tet-only constraint `u_top_a − u_bot_a = 0`
    /// with pivot at `tet_top_dofs[a]`.
    ///
    /// # Panics
    ///
    /// - `thickness <= 0.0` — shell thickness must be positive.
    /// - `normal` is not a unit vector (magnitude outside `1.0 ± 1e-9`).
    pub fn shell_tet_tying(
        shell_disp_dofs: [usize; 3],
        shell_rot_dofs: [usize; 3],
        tet_top_dofs: [usize; 3],
        tet_mid_dofs: [usize; 3],
        tet_bot_dofs: [usize; 3],
        normal: [f64; 3],
        thickness: f64,
    ) -> Vec<MpcRow> {
        assert!(
            thickness > 0.0,
            "MpcRow::shell_tet_tying: thickness must be positive; got {thickness}",
        );
        let norm_sq = normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2];
        let norm = norm_sq.sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-9,
            "MpcRow::shell_tet_tying: normal must be a unit vector; |normal| = {norm} (expected 1.0 ± 1e-9)",
        );

        let h = thickness;
        let mut rows = Vec::with_capacity(6);

        // ── Three displacement-matching rows (mid-surface tying) ────────────
        for a in 0..3 {
            rows.push(MpcRow::new(
                vec![shell_disp_dofs[a], tet_mid_dofs[a]],
                vec![1.0, -1.0],
                0.0,
            ));
        }

        // ── Three rotation/gradient rows ────────────────────────────────────
        // For each output axis a, compute the two rotational coefficients:
        //   coeff_rot[b] = −ε_{a,b,c} · n_c · h   (c = remaining index from {0,1,2}\{a,b})
        //
        // Precomputed per-axis (b1, b2 are the two non-a rotation indices;
        // sign1/sign2 are the Levi-Civita signs ε_{a,b1,c1} and ε_{a,b2,c2}):
        //
        //   a=0: b1=1,c1=2,ε_012=+1 → coeff_b1 = −n[2]·h
        //        b2=2,c2=1,ε_021=−1 → coeff_b2 = +n[1]·h
        //   a=1: b1=0,c1=2,ε_102=−1 → coeff_b1 = +n[2]·h
        //        b2=2,c2=0,ε_120=+1 → coeff_b2 = −n[0]·h
        //   a=2: b1=0,c1=1,ε_201=+1 → coeff_b1 = −n[1]·h
        //        b2=1,c2=0,ε_210=−1 → coeff_b2 = +n[0]·h
        //
        // (sign: ε_012=+1, ε_102=−1, ε_120=+1, ε_201=+1, ε_021=−1, ε_210=−1)
        let rot_data: [(usize, usize, f64, usize, usize, f64); 3] = [
            // (b1, c1, sign1, b2, c2, sign2) — sign_i = ε_{a,bi,ci}
            (1, 2, 1.0, 2, 1, -1.0), // a=0: ε_012=+1, ε_021=−1
            (0, 2, -1.0, 2, 0, 1.0), // a=1: ε_102=−1, ε_120=+1
            (0, 1, 1.0, 1, 0, -1.0), // a=2: ε_201=+1, ε_210=−1
        ];

        for (a, &(b1, c1, sign1, b2, c2, sign2)) in rot_data.iter().enumerate() {
            let coeff_b1 = -sign1 * normal[c1] * h; // −ε_{a,b1,c1} · n_c1 · h
            let coeff_b2 = -sign2 * normal[c2] * h; // −ε_{a,b2,c2} · n_c2 · h

            const DRILLING_EPS: f64 = 1e-12;
            let abs1 = coeff_b1.abs();
            let abs2 = coeff_b2.abs();

            if abs1 < DRILLING_EPS && abs2 < DRILLING_EPS {
                // Drilling axis — both rotational coefficients vanish.
                // Fallback: tet-only u_top_a - u_bot_a = 0.
                rows.push(MpcRow::new(
                    vec![tet_top_dofs[a], tet_bot_dofs[a]],
                    vec![1.0, -1.0],
                    0.0,
                ));
            } else {
                // Pick the rotation DOF with the larger-magnitude coefficient as pivot.
                let (pivot_b, pivot_coeff, other_b, other_coeff) = if abs1 >= abs2 {
                    (b1, coeff_b1, b2, coeff_b2)
                } else {
                    (b2, coeff_b2, b1, coeff_b1)
                };

                if other_coeff.abs() < DRILLING_EPS {
                    // One rotational DOF is essentially zero — two-term row.
                    rows.push(MpcRow::new(
                        vec![shell_rot_dofs[pivot_b], tet_top_dofs[a], tet_bot_dofs[a]],
                        vec![pivot_coeff, 1.0, -1.0],
                        0.0,
                    ));
                } else {
                    // Both rotational DOFs contribute — four-term row.
                    rows.push(MpcRow::new(
                        vec![
                            shell_rot_dofs[pivot_b],
                            shell_rot_dofs[other_b],
                            tet_top_dofs[a],
                            tet_bot_dofs[a],
                        ],
                        vec![pivot_coeff, other_coeff, 1.0, -1.0],
                        0.0,
                    ));
                }
            }
        }

        rows
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

    /// `MpcRow::new` must panic when `dofs` is empty (zero DOFs).
    #[test]
    #[should_panic(expected = "at least one")]
    fn mpc_row_new_panics_on_empty_dofs() {
        // len-equality passes (0 == 0), but is_empty check fires.
        let _ = MpcRow::new(vec![], vec![], 0.0);
    }

    /// `MpcRow::new` must panic when `coeffs[0]` is NaN.
    ///
    /// NaN passes the `!= 0.0` guard (IEEE 754: `NaN == 0.0` is false, so
    /// `NaN != 0.0` is true), so this test pins that the `is_finite` guard
    /// is the one that catches NaN.  Reordering those two pivot-coefficient
    /// asserts would silently break this contract.
    #[test]
    #[should_panic(expected = "finite")]
    fn mpc_row_new_panics_on_nan_pivot() {
        let _ = MpcRow::new(vec![1], vec![f64::NAN], 0.0);
    }

    /// `MpcRow::new` must panic when `rhs` is infinite.
    #[test]
    #[should_panic(expected = "rhs")]
    fn mpc_row_new_panics_on_infinite_rhs() {
        let _ = MpcRow::new(vec![1], vec![1.0], f64::INFINITY);
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
    #[allow(clippy::needless_range_loop)] // explicit indexing reads parallel matrices
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
        let k_before: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read_k(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        // MpcRow: pivot p=0, d1=2, d2=4; coeffs=[2.0, -1.0, 1.0], rhs=0
        // α_1 = -(-1.0)/2.0 = 0.5, α_2 = -(1.0)/2.0 = -0.5, β = 0/2 = 0
        let row = MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 0.0);
        let alpha_1 = 0.5_f64; // -coeffs[1]/c0
        let alpha_2 = -0.5_f64; // -coeffs[2]/c0

        apply_mpc_row_elimination(&mut k, &mut f, &[row]);

        let p = 0usize;
        let d1 = 2usize;
        let d2 = 4usize;

        // (a) Pivot row
        assert_eq!(
            read_k(&k, p, p).to_bits(),
            1.0_f64.to_bits(),
            "K[0][0] must be 1.0"
        );
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
            assert_eq!(
                read_k(&k, j, p),
                0.0,
                "K[{j}][0] should be 0 (column p eliminated)"
            );
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
    #[allow(clippy::needless_range_loop)] // explicit indexing reads parallel matrices
    fn inhomogeneous_mpc_subtracts_column_into_rhs_and_pins_pivot_to_beta() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, (i * 5 + j + 1) as f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=5).map(|i| i as f64).collect();

        let k_before: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read_k(&k, i, j)).collect())
            .collect();
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
        assert_eq!(
            read_k(&k, p, p).to_bits(),
            1.0_f64.to_bits(),
            "K[0][0] must be 1.0"
        );
        assert_eq!(
            read_k(&k, p, d1).to_bits(),
            (-alpha_1).to_bits(),
            "K[0][2] must be -α_1={}",
            -alpha_1
        );
        assert_eq!(
            read_k(&k, p, d2).to_bits(),
            (-alpha_2).to_bits(),
            "K[0][4] must be -α_2={}",
            -alpha_2
        );
        for j in 0..n {
            if j != p && j != d1 && j != d2 {
                assert_eq!(read_k(&k, p, j), 0.0, "K[0][{j}] should be 0 in pivot row");
            }
        }

        // (d) Same K redistribution as homogeneous (αᵢ don't depend on rhs)
        for j in 1..n {
            assert_eq!(
                read_k(&k, j, p),
                0.0,
                "K[{j}][0] should be 0 (column p eliminated)"
            );
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

    // -----------------------------------------------------------------------
    // Step 7 (RED): multiple MPCs with disjoint pivots are order-independent
    // -----------------------------------------------------------------------

    /// Applying two MPCs with fully-disjoint DOF sets in either order produces
    /// bit-identical K and tolerance-equal f.
    ///
    /// mpc_a: pivot 0, others {2,4}, rhs=0.   → DOF set {0,2,4}
    /// mpc_b: pivot 1, others {3},   rhs=0.5. → DOF set {1,3}
    /// {0,2,4} ∩ {1,3} = ∅ — the precondition for order-independence stated in
    /// the module doc (see `apply_mpc_row_elimination` doc, lines 121-129).
    #[test]
    fn multiple_mpcs_with_disjoint_dof_sets_are_order_independent_within_fp_tolerance() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        let make_k = || -> SparseRowMat<usize, f64> {
            let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
                .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, (i * 5 + j + 1) as f64)))
                .collect();
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap()
        };
        let make_f = || -> Vec<f64> { (1..=5).map(|i| i as f64).collect() };

        // Precondition: mpc_a.dofs ∩ mpc_b.dofs = {0,2,4} ∩ {1,3} = ∅.
        // Fully-disjoint DOF sets (not merely disjoint pivots) are required for
        // order-independence; see `apply_mpc_row_elimination` doc lines 121-129.
        let mpc_a = MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 0.0);
        let mpc_b = MpcRow::new(vec![1, 3], vec![3.0, -2.0], 0.5);

        // Pipeline A: a then b
        let mut k_a = make_k();
        let mut f_a = make_f();
        apply_mpc_row_elimination(&mut k_a, &mut f_a, &[mpc_a.clone(), mpc_b.clone()]);

        // Pipeline B: b then a
        let mut k_b = make_k();
        let mut f_b = make_f();
        apply_mpc_row_elimination(&mut k_b, &mut f_b, &[mpc_b, mpc_a]);

        // K must be bit-identical
        for i in 0..n {
            for j in 0..n {
                let ka = read_k(&k_a, i, j);
                let kb = read_k(&k_b, i, j);
                assert_eq!(
                    ka.to_bits(),
                    kb.to_bits(),
                    "K[{i}][{j}]: forward={ka} reverse={kb}",
                );
            }
        }

        // f must agree within FP tolerance
        for j in 0..n {
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
    // Step 9 (RED): contract violation panics
    // -----------------------------------------------------------------------

    /// Out-of-range DOF panics with a message naming the dof.
    #[test]
    #[should_panic(expected = "MpcRow")]
    fn apply_mpc_panics_on_out_of_range_dof() {
        use faer::sparse::{SparseRowMat, Triplet};
        let n = 5usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, 1.0_f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f = vec![0.0_f64; n];
        // DOF 99 is out of range for a 5-DOF system.
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 99], vec![1.0, -1.0], 0.0)],
        );
    }

    /// f.len() != k.nrows() panics.
    #[test]
    #[should_panic(expected = "f.len()")]
    fn apply_mpc_panics_on_f_length_mismatch() {
        use faer::sparse::{SparseRowMat, Triplet};
        let n = 5usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, 1.0_f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f = vec![0.0_f64; 3]; // wrong length
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 1], vec![1.0, -1.0], 0.0)],
        );
    }

    /// Missing redistribution target entry panics naming the missing (row, col).
    #[test]
    #[should_panic(expected = "missing")]
    fn apply_mpc_panics_on_missing_redistribution_target_entry() {
        use faer::sparse::{SparseRowMat, Triplet};
        // 3×3 sparse K: K[0][0]=1, K[0][1]=2, K[1][0]=3, K[1][1]=4, K[2][2]=5
        // Deliberately missing K[0][2], K[2][0], K[2][1].
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            Triplet::new(0, 0, 1.0),
            Triplet::new(0, 1, 2.0),
            Triplet::new(1, 0, 3.0),
            Triplet::new(1, 1, 4.0),
            Triplet::new(2, 2, 5.0),
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(3, 3, &triplets).unwrap();
        let mut f = vec![0.0_f64; 3];
        // Pivot=0, other DOF=2 — K[j][2] is missing for j in {0,1}
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 2], vec![1.0, 2.0], 0.0)],
        );
    }

    /// Non-square K panics.
    #[test]
    #[should_panic(expected = "k must be square")]
    fn apply_mpc_panics_on_non_square_k() {
        use faer::sparse::{SparseRowMat, Triplet};
        // 3×4 non-square
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            Triplet::new(0, 0, 1.0),
            Triplet::new(1, 1, 1.0),
            Triplet::new(2, 2, 1.0),
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(3, 4, &triplets).unwrap();
        let mut f = vec![0.0_f64; 3];
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 1], vec![1.0, -1.0], 0.0)],
        );
    }

    // -----------------------------------------------------------------------
    // Step 11: empty-slice no-op contract
    // -----------------------------------------------------------------------

    /// Calling `apply_mpc_row_elimination` with an empty slice leaves K and f
    /// bit-identical to their pre-call snapshots.
    ///
    /// Regression guard for future refactors that might allocate scratch
    /// buffers unconditionally.
    #[test]
    #[allow(clippy::needless_range_loop)] // explicit indexing reads parallel matrices
    fn apply_mpc_with_empty_slice_leaves_k_and_f_unchanged() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 4usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| (0..n).map(move |j| Triplet::new(i, j, (i * n + j + 1) as f64)))
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=n).map(|i| i as f64).collect();

        let k_before: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read_k(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        apply_mpc_row_elimination(&mut k, &mut f, &[]);

        for i in 0..n {
            for j in 0..n {
                assert_eq!(
                    read_k(&k, i, j).to_bits(),
                    k_before[i][j].to_bits(),
                    "K[{i}][{j}] changed after empty-MPC call: was {}, now {}",
                    k_before[i][j],
                    read_k(&k, i, j),
                );
            }
            assert_eq!(
                f[i].to_bits(),
                f_before[i].to_bits(),
                "f[{i}] changed after empty-MPC call",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step 13 (RED): MpcRow::shell_tet_tying canonical constraint rows
    // -----------------------------------------------------------------------

    /// `shell_tet_tying` with normal=[0,0,1] and h=1 produces exactly 6
    /// canonical constraint rows.
    ///
    /// DOF layout:
    ///   shell_disp = [0, 1, 2], shell_rot = [3, 4, 5]
    ///   tet_top    = [6, 7, 8], tet_mid   = [9, 10, 11], tet_bot = [12, 13, 14]
    ///
    /// Expected rows:
    ///   (0) disp x:  dofs=[0,  9], coeffs=[+1, -1], rhs=0
    ///   (1) disp y:  dofs=[1, 10], coeffs=[+1, -1], rhs=0
    ///   (2) disp z:  dofs=[2, 11], coeffs=[+1, -1], rhs=0
    ///   (3) rot/grad x: dofs=[4,  6, 12], coeffs=[-1, +1, -1], rhs=0
    ///       (pivot shell_rot[1]=4; constraint -θ_y·h + (u_top_x-u_bot_x)=0)
    ///   (4) rot/grad y: dofs=[3,  7, 13], coeffs=[+1, +1, -1], rhs=0
    ///       (pivot shell_rot[0]=3; constraint +θ_x·h + (u_top_y-u_bot_y)=0)
    ///   (5) drilling z: dofs=[8, 14], coeffs=[+1, -1], rhs=0
    ///       (tet-only fallback; both rotational coefficients zero for axis‖normal)
    ///
    /// RED: `MpcRow::shell_tet_tying` is not yet defined.
    #[test]
    fn shell_tet_tying_with_z_normal_produces_six_canonical_constraint_rows() {
        let rows = MpcRow::shell_tet_tying(
            [0, 1, 2],       // shell_disp
            [3, 4, 5],       // shell_rot
            [6, 7, 8],       // tet_top
            [9, 10, 11],     // tet_mid
            [12, 13, 14],    // tet_bot
            [0.0, 0.0, 1.0], // normal = z
            1.0,             // h = 1
        );
        assert_eq!(rows.len(), 6, "shell_tet_tying must produce 6 rows");

        // (0) displacement x
        assert_eq!(rows[0].dofs, vec![0, 9], "row 0 dofs");
        assert_eq!(rows[0].coeffs, vec![1.0, -1.0], "row 0 coeffs");
        assert_eq!(rows[0].rhs.to_bits(), 0.0_f64.to_bits(), "row 0 rhs");

        // (1) displacement y
        assert_eq!(rows[1].dofs, vec![1, 10], "row 1 dofs");
        assert_eq!(rows[1].coeffs, vec![1.0, -1.0], "row 1 coeffs");
        assert_eq!(rows[1].rhs.to_bits(), 0.0_f64.to_bits(), "row 1 rhs");

        // (2) displacement z
        assert_eq!(rows[2].dofs, vec![2, 11], "row 2 dofs");
        assert_eq!(rows[2].coeffs, vec![1.0, -1.0], "row 2 coeffs");
        assert_eq!(rows[2].rhs.to_bits(), 0.0_f64.to_bits(), "row 2 rhs");

        // (3) rot/grad axis x: -θ_y·1 + (u_top_x - u_bot_x) = 0
        // pivot is shell_rot[1]=4, coeff=-1
        assert_eq!(rows[3].dofs, vec![4, 6, 12], "row 3 dofs");
        assert_eq!(rows[3].coeffs, vec![-1.0, 1.0, -1.0], "row 3 coeffs");
        assert_eq!(rows[3].rhs.to_bits(), 0.0_f64.to_bits(), "row 3 rhs");

        // (4) rot/grad axis y: +θ_x·1 + (u_top_y - u_bot_y) = 0
        // pivot is shell_rot[0]=3, coeff=+1
        assert_eq!(rows[4].dofs, vec![3, 7, 13], "row 4 dofs");
        assert_eq!(rows[4].coeffs, vec![1.0, 1.0, -1.0], "row 4 coeffs");
        assert_eq!(rows[4].rhs.to_bits(), 0.0_f64.to_bits(), "row 4 rhs");

        // (5) drilling z: tet-only u_top_z - u_bot_z = 0 (fallback)
        assert_eq!(rows[5].dofs, vec![8, 14], "row 5 dofs");
        assert_eq!(rows[5].coeffs, vec![1.0, -1.0], "row 5 coeffs");
        assert_eq!(rows[5].rhs.to_bits(), 0.0_f64.to_bits(), "row 5 rhs");
    }

    /// With normal=[1,0,0] (x-normal), the drilling axis is x, and the
    /// rotational constraint coefficients are different.
    ///
    /// For axis a=0 (x, parallel to normal): both rotational coefficients
    /// are zero → tet-only fallback: u_top_x - u_bot_x = 0, pivot=tet_top[0]=6.
    ///
    /// For axis a=1 (y): cross-product formula gives coefficients for θ_z·n_x·h,
    /// so the pivot is shell_rot[2] (the largest-magnitude rotational coeff).
    /// ε_102 = -ε_012 = -1, so for output axis 1: c_for_θ_2 = -ε_120·n_0·h = +1·1·h = h
    /// → coeff = +h for shell_rot[2].
    ///
    /// For axis a=2 (z): ε_201·n_1·h → c_for_θ_1 = -ε_210·n_0·h = +1·1·h = h
    /// → pivot is shell_rot[1] with coeff +h... let's verify via the test.
    #[test]
    fn shell_tet_tying_with_x_normal_swaps_drilling_axis() {
        let rows = MpcRow::shell_tet_tying(
            [0, 1, 2],       // shell_disp
            [3, 4, 5],       // shell_rot
            [6, 7, 8],       // tet_top
            [9, 10, 11],     // tet_mid
            [12, 13, 14],    // tet_bot
            [1.0, 0.0, 0.0], // normal = x
            1.0,
        );
        assert_eq!(rows.len(), 6, "must produce 6 rows");

        // Displacement rows unchanged regardless of normal
        assert_eq!(rows[0].dofs, vec![0, 9]);
        assert_eq!(rows[1].dofs, vec![1, 10]);
        assert_eq!(rows[2].dofs, vec![2, 11]);

        // Row 3 — axis 0 is the drilling axis (parallel to normal=[1,0,0]):
        // tet-only fallback u_top_x - u_bot_x = 0, pivot = tet_top[0]=6
        assert_eq!(
            rows[3].dofs,
            vec![6, 12],
            "row 3: drilling fallback for axis 0"
        );
        assert_eq!(rows[3].coeffs, vec![1.0, -1.0], "row 3: coeffs");

        // Row 4 — axis 1 (y): pivot = shell_rot[2]=5 (b2=2, largest |coeff| for n_x=1)
        // rot_data[1]: (b1=0,c1=2,sign1=-1), (b2=2,c2=0,sign2=+1)
        // coeff_b1 = -(-1)·n[2]·h = 0;  coeff_b2 = -(+1)·n[0]·h = -1.0
        // → two-term row: pivot shell_rot[2]=5, coeffs=[-1, +1, -1]
        assert_eq!(rows[4].dofs, vec![5, 7, 13], "row 4: dofs");
        assert_eq!(rows[4].coeffs, vec![-1.0, 1.0, -1.0], "row 4: coeffs");
        assert_eq!(rows[4].rhs.to_bits(), 0.0_f64.to_bits(), "row 4 rhs=0");

        // Row 5 — axis 2 (z): pivot = shell_rot[1]=4 (b2=1, largest |coeff| for n_x=1)
        // rot_data[2]: (b1=0,c1=1,sign1=+1), (b2=1,c2=0,sign2=-1)
        // coeff_b1 = -(+1)·n[1]·h = 0;  coeff_b2 = -(-1)·n[0]·h = +1.0
        // → two-term row: pivot shell_rot[1]=4, coeffs=[+1, +1, -1]
        assert_eq!(rows[5].dofs, vec![4, 8, 14], "row 5: dofs");
        assert_eq!(rows[5].coeffs, vec![1.0, 1.0, -1.0], "row 5: coeffs");
        assert_eq!(rows[5].rhs.to_bits(), 0.0_f64.to_bits(), "row 5 rhs=0");
    }

    // -----------------------------------------------------------------------
    // Oblique-normal four-term branch coverage
    // -----------------------------------------------------------------------

    /// Tests the four-term rotation-row branch in `MpcRow::shell_tet_tying`
    /// (lines 484-495), which is unreachable by the existing z-/x-normal tests.
    ///
    /// With normal = [1/√3, 1/√3, 1/√3] and h = 1, every axis has both
    /// rotational coefficients with magnitude 1/√3 > DRILLING_EPS, so all
    /// three rotation/gradient rows (rows[3..6]) route into the four-term branch.
    ///
    /// Hand-computed expected values (coeff_bi = −ε_{a,bi,ci}·n_ci·h):
    ///
    /// | row | a | rot_data tuple               | coeff_b1 | coeff_b2 | pivot_b | other_b |
    /// |-----|---|------------------------------|----------|----------|---------|---------|
    /// |  3  | 0 | (1,2,+1, 2,1,-1)             | −1/√3    | +1/√3    | b1=1    | b2=2    |
    /// |  4  | 1 | (0,2,-1, 2,0,+1)             | +1/√3    | −1/√3    | b1=0    | b2=2    |
    /// |  5  | 2 | (0,1,+1, 1,0,-1)             | −1/√3    | +1/√3    | b1=0    | b2=1    |
    ///
    /// Because |coeff_b1| == |coeff_b2| for all axes, the `abs1 >= abs2` tie-break
    /// selects b1 as pivot. This test pins the sign distinction between pivot_coeff
    /// and other_coeff — a swap would still be self-consistent under row-elimination
    /// but would flip the dofs[0]/coeffs[0] relationship.
    ///
    /// The implementation computes `coeff = -sign * normal[c] * h`. With
    /// sign ∈ {+1.0, −1.0}, h = 1.0, and normal[c] = inv_sqrt3 = 1.0/√3,
    /// IEEE 754 gives exact bit-identical values, so `.to_bits()` equality holds.
    #[test]
    #[allow(clippy::needless_range_loop)] // `a` used both as index and in assertion messages
    fn shell_tet_tying_with_oblique_normal_produces_three_four_term_rotation_rows() {
        let inv_sqrt3 = 1.0_f64 / 3.0_f64.sqrt();
        let rows = MpcRow::shell_tet_tying(
            [0, 1, 2],                         // shell_disp
            [3, 4, 5],                         // shell_rot
            [6, 7, 8],                         // tet_top
            [9, 10, 11],                       // tet_mid
            [12, 13, 14],                      // tet_bot
            [inv_sqrt3, inv_sqrt3, inv_sqrt3], // oblique unit normal
            1.0,                               // h = 1
        );
        assert_eq!(rows.len(), 6, "shell_tet_tying must produce 6 rows");

        // ── Displacement rows (sanity check) ────────────────────────────────
        for a in 0..3 {
            assert_eq!(rows[a].dofs, vec![a, 9 + a], "disp row {a} dofs");
            assert_eq!(rows[a].coeffs, vec![1.0, -1.0], "disp row {a} coeffs");
            assert_eq!(rows[a].rhs.to_bits(), 0.0_f64.to_bits(), "disp row {a} rhs");
        }

        // ── Rotation row 3 (a=0): rot_data (1,2,+1, 2,1,-1) ────────────────
        // coeff_b1 = -(+1)·n[2]·h = -inv_sqrt3   → pivot_b=1 (|abs1|>=|abs2|)
        // coeff_b2 = -(-1)·n[1]·h = +inv_sqrt3   → other_b=2
        // dofs = [shell_rot[1]=4, shell_rot[2]=5, tet_top[0]=6, tet_bot[0]=12]
        assert_eq!(rows[3].dofs, vec![4, 5, 6, 12], "row 3 dofs");
        assert_eq!(rows[3].coeffs.len(), 4, "row 3 must be four-term");
        assert_eq!(
            rows[3].coeffs[0].to_bits(),
            (-inv_sqrt3).to_bits(),
            "row 3 pivot_coeff"
        );
        assert_eq!(
            rows[3].coeffs[1].to_bits(),
            inv_sqrt3.to_bits(),
            "row 3 other_coeff"
        );
        assert_eq!(
            rows[3].coeffs[2].to_bits(),
            1.0_f64.to_bits(),
            "row 3 tet_top coeff"
        );
        assert_eq!(
            rows[3].coeffs[3].to_bits(),
            (-1.0_f64).to_bits(),
            "row 3 tet_bot coeff"
        );
        assert_eq!(rows[3].rhs.to_bits(), 0.0_f64.to_bits(), "row 3 rhs");

        // ── Rotation row 4 (a=1): rot_data (0,2,-1, 2,0,+1) ────────────────
        // coeff_b1 = -(-1)·n[2]·h = +inv_sqrt3   → pivot_b=0 (|abs1|>=|abs2|)
        // coeff_b2 = -(+1)·n[0]·h = -inv_sqrt3   → other_b=2
        // dofs = [shell_rot[0]=3, shell_rot[2]=5, tet_top[1]=7, tet_bot[1]=13]
        assert_eq!(rows[4].dofs, vec![3, 5, 7, 13], "row 4 dofs");
        assert_eq!(rows[4].coeffs.len(), 4, "row 4 must be four-term");
        assert_eq!(
            rows[4].coeffs[0].to_bits(),
            inv_sqrt3.to_bits(),
            "row 4 pivot_coeff"
        );
        assert_eq!(
            rows[4].coeffs[1].to_bits(),
            (-inv_sqrt3).to_bits(),
            "row 4 other_coeff"
        );
        assert_eq!(
            rows[4].coeffs[2].to_bits(),
            1.0_f64.to_bits(),
            "row 4 tet_top coeff"
        );
        assert_eq!(
            rows[4].coeffs[3].to_bits(),
            (-1.0_f64).to_bits(),
            "row 4 tet_bot coeff"
        );
        assert_eq!(rows[4].rhs.to_bits(), 0.0_f64.to_bits(), "row 4 rhs");

        // ── Rotation row 5 (a=2): rot_data (0,1,+1, 1,0,-1) ────────────────
        // coeff_b1 = -(+1)·n[1]·h = -inv_sqrt3   → pivot_b=0 (|abs1|>=|abs2|)
        // coeff_b2 = -(-1)·n[0]·h = +inv_sqrt3   → other_b=1
        // dofs = [shell_rot[0]=3, shell_rot[1]=4, tet_top[2]=8, tet_bot[2]=14]
        assert_eq!(rows[5].dofs, vec![3, 4, 8, 14], "row 5 dofs");
        assert_eq!(rows[5].coeffs.len(), 4, "row 5 must be four-term");
        assert_eq!(
            rows[5].coeffs[0].to_bits(),
            (-inv_sqrt3).to_bits(),
            "row 5 pivot_coeff"
        );
        assert_eq!(
            rows[5].coeffs[1].to_bits(),
            inv_sqrt3.to_bits(),
            "row 5 other_coeff"
        );
        assert_eq!(
            rows[5].coeffs[2].to_bits(),
            1.0_f64.to_bits(),
            "row 5 tet_top coeff"
        );
        assert_eq!(
            rows[5].coeffs[3].to_bits(),
            (-1.0_f64).to_bits(),
            "row 5 tet_bot coeff"
        );
        assert_eq!(rows[5].rhs.to_bits(), 0.0_f64.to_bits(), "row 5 rhs");
    }

    // -----------------------------------------------------------------------
    // Step 15 (RED): end-to-end shell_tet_tying + apply + solve
    // -----------------------------------------------------------------------

    /// Composing `shell_tet_tying` with `apply_mpc_row_elimination` and solving
    /// the eliminated system must satisfy every MPC constraint to FP tolerance.
    ///
    /// Setup: 15×15 fully-dense K with SPD-like values, f = [1..15].
    /// DOF layout: shell_disp=[0,1,2], shell_rot=[3,4,5], tet_top=[6,7,8],
    ///   tet_mid=[9,10,11], tet_bot=[12,13,14]. Normal=[0,0,1], h=1.
    ///
    /// The 6 MPC pivots are {0,1,2,4,3,8} — all distinct.
    /// After elimination and dense-LU solve, for each MpcRow:
    ///   |Σᵢ row.coeffs[i] · u[row.dofs[i]] − row.rhs| < 1e-9.
    #[test]
    fn shell_tet_tying_constraints_compose_with_apply_mpc_row_elimination_to_satisfy_constraint_after_solve()
     {
        use faer::linalg::solvers::Solve;
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 15usize;

        // SPD-like K: K[i][j] = if i==j { 10.0 } else { 0.1 + 0.05*(i+j) as f64 }
        // Fully dense so all redistribution targets are pre-allocated.
        let triplets: Vec<Triplet<usize, usize, f64>> = (0..n)
            .flat_map(|i| {
                (0..n).map(move |j| {
                    let v = if i == j {
                        10.0
                    } else {
                        0.1 + 0.05 * (i + j) as f64
                    };
                    Triplet::new(i, j, v)
                })
            })
            .collect();
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=n).map(|i| i as f64).collect();

        // Build the 6 MPC rows for z-normal, h=1
        let mpc_rows = MpcRow::shell_tet_tying(
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [9, 10, 11],
            [12, 13, 14],
            [0.0, 0.0, 1.0],
            1.0,
        );
        assert_eq!(mpc_rows.len(), 6);

        apply_mpc_row_elimination(&mut k, &mut f, &mpc_rows);

        // Dense LU solve: K_after · u = f_after
        let k_dense = k.to_dense();
        let plu = k_dense.partial_piv_lu();
        let mut rhs = faer::Mat::<f64>::from_fn(n, 1, |i, _| f[i]);
        plu.solve_in_place(&mut rhs);
        let u = rhs.col_as_slice(0_usize);

        // Verify each constraint Σ coeffs[i] · u[dofs[i]] = rhs
        for (k_idx, row) in mpc_rows.iter().enumerate() {
            let residual: f64 = row
                .coeffs
                .iter()
                .zip(row.dofs.iter())
                .map(|(&c, &d)| c * u[d])
                .sum::<f64>()
                - row.rhs;
            assert!(
                residual.abs() < 1e-9,
                "MpcRow {k_idx} constraint not satisfied: residual = {residual:.2e}",
            );
        }
    }

    // -----------------------------------------------------------------------
    // Step 1: binary_search regression — sparse CSR with non-trivial offsets
    // -----------------------------------------------------------------------

    /// Applies `apply_mpc_row_elimination` to a SPARSE 5×5 CSR where target
    /// columns land at non-trivial positions within per-row stored-slot ranges.
    ///
    /// Fixture (MPC: pivot=0, dofs=[0,2,4], coeffs=[2.0,-1.0,1.0], rhs=0.0):
    ///   Row 0 (pivot): cols [0,2,3,4] — diagonal at offset 0; K[0][2] at offset 1,
    ///                  K[0][4] at offset 3. Extra col 3 ensures pivot row zeroing
    ///                  and pivot-equation write happen at non-zero slice offsets.
    ///   Row 1:         cols [0,1,2,4] — K[1][0] at offset 0; K[1][2] pre-allocated
    ///                  (offset 2) so this row exercises redistribution normally.
    ///   Row 2:         cols [2,4]     — K[2][0] absent → Err arm (skip entirely).
    ///   Row 3:         cols [0,2,4]   — K[3][0] at offset 0, K[3][2] at offset 1,
    ///                  K[3][4] at offset 2 (all redistribution targets present).
    ///   Row 4:         cols [0,2,4]   — mirrors row 3.
    ///
    /// Pins:
    /// - Absolute-index computation `start + rel`: for row 3, start=10, rel=1 → idx=11
    ///   for K[3][2]. If `rel` is used alone (without adding `start`), the write hits
    ///   vals[1] = K[0][2] — silently wrong but not caught by a fully-dense fixture.
    /// - Err-arm skip at row 2: binary_search returns Err(0) for col 0 in [2,4];
    ///   row 2 must remain bit-identical to its pre-call snapshot.
    /// - Pivot-row writes at non-zero offsets within row 0's slot range [0..4].
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn apply_mpc_to_sparse_csr_with_target_at_row_range_boundaries_redistributes_correctly() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        // Build sparse K — rows store only a subset of columns.
        // Column indices within each row are sorted (faer invariant).
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            // Row 0 (pivot p=0): cols [0,2,3,4]
            Triplet::new(0, 0, 5.0),
            Triplet::new(0, 2, 0.5),
            Triplet::new(0, 3, 0.3),
            Triplet::new(0, 4, 0.7),
            // Row 1: cols [0,1,2,4] — K[1][2] pre-allocated to avoid panic
            Triplet::new(1, 0, 1.0),
            Triplet::new(1, 1, 4.0),
            Triplet::new(1, 2, 0.2),
            Triplet::new(1, 4, 0.6),
            // Row 2: cols [2,4] — K[2][0] absent → Err path
            Triplet::new(2, 2, 3.0),
            Triplet::new(2, 4, 0.4),
            // Row 3: cols [0,2,4] — all redistribution targets present
            Triplet::new(3, 0, 2.0),
            Triplet::new(3, 2, 1.5),
            Triplet::new(3, 4, 0.9),
            // Row 4: cols [0,2,4]
            Triplet::new(4, 0, 0.8),
            Triplet::new(4, 2, 1.2),
            Triplet::new(4, 4, 6.0),
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f: Vec<f64> = (1..=5).map(|i| i as f64).collect();

        // Snapshot K and f before applying the MPC.
        let k_before: Vec<Vec<f64>> = (0..n)
            .map(|i| (0..n).map(|j| read_k(&k, i, j)).collect())
            .collect();
        let f_before = f.clone();

        // MPC: pivot p=0, other dofs d1=2, d2=4; coeffs=[2.0, -1.0, 1.0], rhs=0.0
        // α_1 = -coeffs[1]/c0 = -(-1.0)/2.0 = 0.5
        // α_2 = -coeffs[2]/c0 = -(1.0)/2.0  = -0.5
        // β   = rhs/c0         = 0.0/2.0     = 0.0
        let row = MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 0.0);
        let alpha_1 = 0.5_f64;
        let alpha_2 = -0.5_f64;
        let beta = 0.0_f64;

        apply_mpc_row_elimination(&mut k, &mut f, &[row]);

        let p = 0usize;
        let d1 = 2usize;
        let d2 = 4usize;

        // ── Pivot row assertions ─────────────────────────────────────────────
        // Step 2 zeroes row 0; step 3 writes K[0][0]=1, K[0][2]=-α_1, K[0][4]=-α_2.
        assert_eq!(
            read_k(&k, 0, 0).to_bits(),
            1.0_f64.to_bits(),
            "K[0][0] must be 1.0 (pivot diagonal)"
        );
        assert_eq!(
            read_k(&k, 0, d1).to_bits(),
            (-alpha_1).to_bits(),
            "K[0][2] must be -α_1 = {}",
            -alpha_1
        );
        assert_eq!(
            read_k(&k, 0, d2).to_bits(),
            (-alpha_2).to_bits(),
            "K[0][4] must be -α_2 = {}",
            -alpha_2
        );
        // K[0][1] not stored; K[0][3] was 0.3 but step-2 zero clears it.
        assert_eq!(read_k(&k, 0, 1), 0.0, "K[0][1] must be 0.0");
        assert_eq!(
            read_k(&k, 0, 3),
            0.0,
            "K[0][3] must be 0.0 (step-2 cleared)"
        );

        // ── Row 2: Err-arm skip (K[2][0] absent) ────────────────────────────
        for col in 0..n {
            assert_eq!(
                read_k(&k, 2, col).to_bits(),
                k_before[2][col].to_bits(),
                "K[2][{col}] must be bit-identical (Err arm: K[2][0] absent)"
            );
        }
        assert_eq!(
            f[2].to_bits(),
            f_before[2].to_bits(),
            "f[2] must be bit-identical (no K[2][0] entry, no redistribution)"
        );

        // ── Row 3: redistribution at non-zero slot offsets ──────────────────
        // CSR layout: row 3 starts at slot 10 (4+4+2=10), so col 0 → slot 10,
        // col 2 → slot 11, col 4 → slot 12.  binary_search returns rel∈{0,1,2};
        // absolute index = start(10) + rel.  Using `rel` alone would corrupt row 0.
        assert_eq!(
            read_k(&k, 3, p),
            0.0,
            "K[3][0] must be 0.0 (column p eliminated)"
        );
        let expected_32 = k_before[3][d1] + k_before[3][p] * alpha_1;
        assert_eq!(
            read_k(&k, 3, d1).to_bits(),
            expected_32.to_bits(),
            "K[3][2]: expected {expected_32} = {} + {} * {}",
            k_before[3][d1],
            k_before[3][p],
            alpha_1,
        );
        let expected_34 = k_before[3][d2] + k_before[3][p] * alpha_2;
        assert_eq!(
            read_k(&k, 3, d2).to_bits(),
            expected_34.to_bits(),
            "K[3][4]: expected {expected_34} = {} + {} * {}",
            k_before[3][d2],
            k_before[3][p],
            alpha_2,
        );

        // ── f unchanged (β = 0 → no subtract from f[j]) ─────────────────────
        for j in 1..n {
            assert_eq!(
                f[j].to_bits(),
                f_before[j].to_bits(),
                "f[{j}] must be unchanged (β=0.0, homogeneous MPC)"
            );
        }
        assert_eq!(f[p].to_bits(), beta.to_bits(), "f[0] must be β=0.0");
    }

    // -----------------------------------------------------------------------
    // Debug-only: sorted col_idx assertion
    // -----------------------------------------------------------------------

    /// Debug-only: `apply_mpc_row_elimination` panics in debug builds when
    /// any row of `K` has unsorted column indices.
    ///
    /// Fixture: 3×3 sparse K where row 0 has col_idx `[2, 0]` — out of
    /// order.  Bypasses `try_new_from_triplets` (which sorts on construction)
    /// by using `SymbolicSparseRowMat::new_unsorted_checked` +
    /// `SparseRowMat::new`, so the deliberate sort violation reaches
    /// `apply_mpc_row_elimination` intact.
    ///
    /// The new `#[cfg(debug_assertions)]` sorted-col_idx walk at function
    /// entry must fire before any `binary_search` work, producing a panic
    /// message that contains "unsorted".
    ///
    /// Gated by `#[cfg(debug_assertions)]` so `cargo test --release` does
    /// not false-fail — same pattern as the debug-gated `#[should_panic]`
    /// tests in `shell_boundary.rs`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unsorted")]
    fn apply_mpc_panics_in_debug_when_col_idx_unsorted_within_row() {
        use faer::sparse::SymbolicSparseRowMat;

        // Build a 3×3 CSR with row 0 having col_idx [2, 0] — unsorted.
        //   row_ptr = [0, 2, 3, 4]:
        //     row 0 → slots 0..2  (col_idx = [2, 0], out of order)
        //     row 1 → slots 2..3  (col_idx = [1])
        //     row 2 → slots 3..4  (col_idx = [2])
        //   vals = [1.0, 2.0, 3.0, 4.0]
        // The debug walk fires on row 0 (adjacent entries 2 ≥ 0) at function
        // entry, before any per-MpcRow work begins.
        let symbolic = SymbolicSparseRowMat::<usize>::new_unsorted_checked(
            3_usize,
            3_usize,
            vec![0_usize, 2, 3, 4],
            None,
            vec![2_usize, 0, 1, 2],
        );
        let mut k = SparseRowMat::<usize, f64>::new(symbolic, vec![1.0, 2.0, 3.0, 4.0]);
        let mut f = vec![0.0_f64; 3];
        // MpcRow: pivot dof=0, other dof=1; the debug check fires at entry
        // before binary_search is ever called.
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 1], vec![1.0, -1.0], 0.0)],
        );
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
    fn apply_mpc_panics_in_debug_when_col_idx_unsorted_on_later_row() {
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
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 1], vec![1.0, -1.0], 0.0)],
        );
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
    fn apply_mpc_panics_in_debug_when_col_idx_has_duplicate_in_row() {
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
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 1], vec![1.0, -1.0], 0.0)],
        );
    }

    /// `apply_mpc_row_elimination` panics with "missing" when `K[j][p]` IS stored
    /// but the redistribution target `K[j][dofs[1]]` is absent.
    ///
    /// Fixture: row 1 has cols [0,1,4] → K[1][0] is stored (at the first slot
    /// of row 1's range, i.e. a non-zero buffer offset), but K[1][2] is absent.
    /// The MPC's redistribution step for j=1 requires K[1][2] → panics with
    /// the "missing" message whether the inner loop is linear or binary_search.
    ///
    /// Regression: a binary_search refactor that treated `Err` as "found but at
    /// index 0" would silently corrupt K instead of panicking.
    #[test]
    #[should_panic(expected = "missing")]
    fn apply_mpc_to_sparse_csr_panics_when_redistribution_target_at_non_zero_offset_is_absent() {
        use faer::sparse::{SparseRowMat, Triplet};

        let n = 5usize;
        let triplets: Vec<Triplet<usize, usize, f64>> = vec![
            // Row 0 (pivot p=0): all required entries present
            Triplet::new(0, 0, 5.0),
            Triplet::new(0, 2, 0.5),
            Triplet::new(0, 4, 0.7),
            // Row 1: K[1][0] stored at buffer offset 3 (after 3 row-0 entries),
            // but K[1][2] intentionally absent → redistribution target missing.
            Triplet::new(1, 0, 1.0),
            Triplet::new(1, 1, 4.0),
            Triplet::new(1, 4, 0.6),
            // Remaining rows (never reached after row 1 panics).
            Triplet::new(2, 2, 3.0),
            Triplet::new(2, 4, 0.4),
            Triplet::new(3, 0, 2.0),
            Triplet::new(3, 2, 1.5),
            Triplet::new(3, 4, 0.9),
            Triplet::new(4, 0, 0.8),
            Triplet::new(4, 4, 6.0),
        ];
        let mut k: SparseRowMat<usize, f64> =
            SparseRowMat::try_new_from_triplets(n, n, &triplets).unwrap();
        let mut f = vec![0.0_f64; n];
        // K[1][0]=1.0 is stored → kjp_idx is found → redistribution attempts
        // K[1][2] → absent → panic("MpcRow apply: missing K[1][2] entry …")
        apply_mpc_row_elimination(
            &mut k,
            &mut f,
            &[MpcRow::new(vec![0, 2, 4], vec![2.0, -1.0, 1.0], 0.0)],
        );
    }

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>`, returning 0.0 if
    /// the entry is not explicitly stored.
    fn read_k(k: &faer::sparse::SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }
}
