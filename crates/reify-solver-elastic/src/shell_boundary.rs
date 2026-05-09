//! Shell-aware support boundary conditions (PRD v0.4 shells T8).
//!
//! Maps high-level support semantics onto DOF-level Dirichlet BCs:
//!
//! - [`SupportKind::Fixed`] clamps all relevant DOFs (6 on a shell node, 3 on
//!   a tet node).
//! - [`SupportKind::Pinned`] clamps only the 3 translational DOFs and leaves
//!   rotational DOFs free (meaningful only on shell nodes; on a tet node this
//!   is semantically equivalent to `Fixed` — see [`SupportCompatibility`]).
//!
//! The primary entry point is [`build_support_bcs`], which returns a
//! `(Vec<DirichletBc>, SupportCompatibility)` pair.  Feed the `Vec<DirichletBc>`
//! directly into [`crate::boundary::apply_dirichlet_row_elimination`].
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/shells.md` task T8 ("shell BC application —
//! rotation auto-clamp + PinnedSupport opt-out").
//! The stdlib-side constructor (`FixedSupport`, `PinnedSupport`) is documented
//! in `crates/reify-stdlib/src/supports.rs`.

/// Whether all DOFs are clamped (`Fixed`) or only translational DOFs
/// (`Pinned`, which leaves rotational DOFs free on a shell node).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportKind {
    /// Clamp all DOFs (6 per shell node, 3 per tet node).
    Fixed,
    /// Clamp only the 3 translational DOFs; leave rotational DOFs free.
    ///
    /// On a tet body (which has no rotational DOFs), this produces the same
    /// BCs as `Fixed` — see [`SupportCompatibility::PinnedOnTetEquivalentToFixed`].
    Pinned,
}

/// The element body type carrying the constrained nodes.
///
/// The body kind determines the DOF stride and which DOFs are translational
/// vs. rotational.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportBodyKind {
    /// MITC3+ shell element: 6 DOFs per node `(u_x, u_y, u_z, θ_x, θ_y, θ_z)`.
    Shell,
    /// P1/P2 tetrahedral element: 3 DOFs per node `(u_x, u_y, u_z)`.
    Tet,
}

use crate::boundary::DirichletBc;

/// Diagnostic tag returned alongside the BC list from [`build_support_bcs`].
///
/// `Ok` is the common case.  `PinnedOnTetEquivalentToFixed` surfaces a
/// user-intent mismatch without changing the solver behaviour: tet nodes
/// carry no rotational DOFs, so `PinnedSupport` and `FixedSupport` produce
/// identical Dirichlet BCs on a tet body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportCompatibility {
    /// No issue: the `(SupportKind, SupportBodyKind)` combination is
    /// semantically unambiguous.
    Ok,
    /// [`SupportKind::Pinned`] was applied to a [`SupportBodyKind::Tet`] body.
    ///
    /// Tet elements have no rotational DOFs, so `PinnedSupport` on a tet is
    /// bit-identical to `FixedSupport` on a tet. The BCs produced are valid;
    /// this tag signals a likely copy-paste mismatch between shell and tet
    /// bodies for downstream warning/logging.
    ///
    /// **Note on vacuous calls**: this tag is returned even when `nodes` is
    /// empty (no BCs are emitted). The tag encodes the *call signature* —
    /// the `(Pinned, Tet)` combination is always semantically ambiguous
    /// regardless of whether any nodes are constrained. Callers that
    /// interpret the tag as a per-constraint warning should suppress it when
    /// `bcs.is_empty()`.
    PinnedOnTetEquivalentToFixed,
}

/// Build the [`DirichletBc`] list for a set of support nodes.
///
/// # Parameters
///
/// - `nodes`: global node indices that are constrained.
/// - `kind`: [`SupportKind::Fixed`] clamps all DOFs; [`SupportKind::Pinned`]
///   clamps only the 3 translational DOFs (meaningful on shell bodies).
/// - `body`: [`SupportBodyKind::Shell`] uses a 6-DOF/node stride;
///   [`SupportBodyKind::Tet`] uses 3.
///
/// # Returns
///
/// A pair `(bcs, compat)` where:
/// - `bcs` is the flat list of [`DirichletBc`] pairs in node-major, DOF-minor order.
/// - `compat` is [`SupportCompatibility::Ok`] for all combinations except
///   `(Pinned, Tet)`, which returns
///   [`SupportCompatibility::PinnedOnTetEquivalentToFixed`].
///
/// All produced BCs have `value = 0.0` (homogeneous Dirichlet). Non-zero
/// prescribed displacements are handled by `DisplacementSupport` (separate task).
pub fn build_support_bcs(
    nodes: &[usize],
    kind: SupportKind,
    body: SupportBodyKind,
) -> (Vec<DirichletBc>, SupportCompatibility) {
    // Dispatch on (body, kind) to determine:
    //   stride    — DOFs per node (global DOF = stride * node + offset)
    //   dof_count — how many DOFs per node to clamp (offsets 0..dof_count)
    //   compat    — diagnostic tag returned alongside the BC list
    //
    // Shell: 6-stride (u_x, u_y, u_z, θ_x, θ_y, θ_z per node).
    //   Fixed  → clamp all 6 offsets.
    //   Pinned → clamp only the 3 translational offsets (0..3); rotational free.
    // Tet: 3-stride (u_x, u_y, u_z per node; no rotational DOFs).
    //   Fixed  → clamp all 3 offsets.
    //   Pinned → bit-identical to Fixed; tag signals user-intent mismatch.
    let (stride, dof_count, compat) = match (body, kind) {
        (SupportBodyKind::Shell, SupportKind::Fixed) => (6, 6, SupportCompatibility::Ok),
        (SupportBodyKind::Shell, SupportKind::Pinned) => (6, 3, SupportCompatibility::Ok),
        (SupportBodyKind::Tet, SupportKind::Fixed) => (3, 3, SupportCompatibility::Ok),
        (SupportBodyKind::Tet, SupportKind::Pinned) => {
            (3, 3, SupportCompatibility::PinnedOnTetEquivalentToFixed)
        }
    };
    let bcs = nodes
        .iter()
        .flat_map(|&n| {
            (0..dof_count).map(move |i| DirichletBc {
                dof: stride * n + i,
                value: 0.0,
            })
        })
        .collect();
    (bcs, compat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::apply_dirichlet_row_elimination;
    use crate::constitutive::IsotropicElastic;
    use crate::shell_assembly::shell_element_stiffness;
    use faer::sparse::{SparseRowMat, Triplet};

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>`, returning `0.0`
    /// if the entry is not stored (same pattern as `dirichlet.rs` tests).
    fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }

    /// Build Q = Ry(45°) · Rz(30°) (same rotation used by the covariance test
    /// in `shell_assembly.rs` to avoid drilling-singularity alignment).
    fn tilted_q() -> [[f64; 3]; 3] {
        let cos30 = (30.0_f64.to_radians()).cos();
        let sin30 = (30.0_f64.to_radians()).sin();
        let cos45 = (45.0_f64.to_radians()).cos();
        let sin45 = (45.0_f64.to_radians()).sin();
        let rz: [[f64; 3]; 3] = [[cos30, -sin30, 0.0], [sin30, cos30, 0.0], [0.0, 0.0, 1.0]];
        let ry: [[f64; 3]; 3] = [[cos45, 0.0, sin45], [0.0, 1.0, 0.0], [-sin45, 0.0, cos45]];
        let mut q = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    q[i][j] += ry[i][k] * rz[k][j];
                }
            }
        }
        q
    }

    /// Build the tilted triangle nodes from UNIT_TRI rotated by Q = Ry(45°)·Rz(30°).
    fn tilted_tri() -> [[f64; 3]; 3] {
        const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let q = tilted_q();
        let mut nodes = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                nodes[ni][i] = q[i][0] * node[0] + q[i][1] * node[1] + q[i][2] * node[2];
            }
        }
        nodes
    }

    /// Build a 18×18 `SparseRowMat` from the non-zero entries of a dense
    /// `shell_element_stiffness` output via faer triplets.
    ///
    /// Zero entries are skipped — this matches typical sparse-matrix
    /// construction conventions, avoids explicit-zero storage, and ensures
    /// `try_new_from_triplets` never sums duplicates or stores finite zeros
    /// that could mask NaN/inf in unused entries.
    fn shell_k_sparse(nodes: &[[f64; 3]; 3]) -> SparseRowMat<usize, f64> {
        let mat = IsotropicElastic {
            youngs_modulus: 210_000.0,
            poisson_ratio: 0.3,
        };
        let k_dense = shell_element_stiffness(nodes, 0.05, &mat);
        let mut triplets = Vec::new();
        for i in 0..18 {
            for j in 0..18 {
                let v = k_dense.get(i, j);
                if v != 0.0 {
                    triplets.push(Triplet::new(i, j, v));
                }
            }
        }
        SparseRowMat::try_new_from_triplets(18, 18, &triplets).expect("valid 18×18 triplets")
    }

    // ------------------------------------------------------------------
    // Step 3: build_support_bcs — (Shell, Fixed) returns 6 BCs/node
    // ------------------------------------------------------------------

    /// `build_support_bcs(&[2, 5], Fixed, Shell)` → 12 BCs at DOFs
    /// `[12,13,14,15,16,17, 30,31,32,33,34,35]`, all values 0.0, compat `Ok`.
    ///
    /// Also checks the empty-input case: `build_support_bcs(&[], Fixed, Shell)`
    /// returns `(Vec::new(), Ok)`.
    #[test]
    fn build_support_bcs_shell_fixed_emits_six_bcs_per_node() {
        let (bcs, compat) =
            build_support_bcs(&[2, 5], SupportKind::Fixed, SupportBodyKind::Shell);

        // 2 nodes × 6 DOFs each
        assert_eq!(bcs.len(), 12, "expected 12 BCs for 2 shell nodes (Fixed)");

        // DOFs: 6*2 + {0..5}, then 6*5 + {0..5}
        let expected_dofs: Vec<usize> = vec![12, 13, 14, 15, 16, 17, 30, 31, 32, 33, 34, 35];
        for (i, (bc, &exp_dof)) in bcs.iter().zip(expected_dofs.iter()).enumerate() {
            assert_eq!(bc.dof, exp_dof, "bcs[{i}].dof: expected {exp_dof}, got {}", bc.dof);
            assert_eq!(
                bc.value.to_bits(),
                0.0_f64.to_bits(),
                "bcs[{i}].value must be 0.0"
            );
        }
        assert_eq!(compat, SupportCompatibility::Ok, "compat must be Ok");

        // Empty-input case
        let (empty_bcs, empty_compat) =
            build_support_bcs(&[], SupportKind::Fixed, SupportBodyKind::Shell);
        assert!(empty_bcs.is_empty(), "empty nodes → empty BC list");
        assert_eq!(empty_compat, SupportCompatibility::Ok);
    }

    // ------------------------------------------------------------------
    // Step 5: build_support_bcs — (Shell, Pinned) returns 3 BCs/node
    // ------------------------------------------------------------------

    /// `build_support_bcs(&[0, 3], Pinned, Shell)` → 6 BCs at DOFs
    /// `[0, 1, 2, 18, 19, 20]` (translational only; rotational 3..6 absent).
    #[test]
    fn build_support_bcs_shell_pinned_emits_three_translational_bcs_per_node() {
        let (bcs, compat) =
            build_support_bcs(&[0, 3], SupportKind::Pinned, SupportBodyKind::Shell);

        // 2 nodes × 3 translational DOFs each
        assert_eq!(bcs.len(), 6, "expected 6 BCs for 2 shell nodes (Pinned)");

        // DOFs: 6*0 + {0,1,2}, then 6*3 + {0,1,2}
        let expected_dofs: Vec<usize> = vec![0, 1, 2, 18, 19, 20];
        for (i, (bc, &exp_dof)) in bcs.iter().zip(expected_dofs.iter()).enumerate() {
            assert_eq!(bc.dof, exp_dof, "bcs[{i}].dof: expected {exp_dof}, got {}", bc.dof);
            assert_eq!(
                bc.value.to_bits(),
                0.0_f64.to_bits(),
                "bcs[{i}].value must be 0.0"
            );
        }
        assert_eq!(compat, SupportCompatibility::Ok, "compat must be Ok");

        // Rotational DOFs must NOT be in the list
        let dofs: Vec<usize> = bcs.iter().map(|bc| bc.dof).collect();
        for rot_off in [3, 4, 5] {
            assert!(
                !dofs.contains(&rot_off),
                "rotational DOF {rot_off} must not appear in Pinned BC list"
            );
        }
    }

    // ------------------------------------------------------------------
    // Step 7: build_support_bcs — (Tet, Fixed) returns 3 BCs/node
    // ------------------------------------------------------------------

    /// `build_support_bcs(&[1, 4], Fixed, Tet)` → 6 BCs at DOFs
    /// `[3, 4, 5, 12, 13, 14]` (3-stride: `3*N + {0,1,2}` per node).
    #[test]
    fn build_support_bcs_tet_fixed_emits_three_bcs_per_node_with_3_stride() {
        let (bcs, compat) =
            build_support_bcs(&[1, 4], SupportKind::Fixed, SupportBodyKind::Tet);

        // 2 nodes × 3 DOFs each
        assert_eq!(bcs.len(), 6, "expected 6 BCs for 2 tet nodes (Fixed)");

        // DOFs: 3*1 + {0,1,2} = [3,4,5], then 3*4 + {0,1,2} = [12,13,14]
        let expected_dofs: Vec<usize> = vec![3, 4, 5, 12, 13, 14];
        for (i, (bc, &exp_dof)) in bcs.iter().zip(expected_dofs.iter()).enumerate() {
            assert_eq!(bc.dof, exp_dof, "bcs[{i}].dof: expected {exp_dof}, got {}", bc.dof);
            assert_eq!(
                bc.value.to_bits(),
                0.0_f64.to_bits(),
                "bcs[{i}].value must be 0.0"
            );
        }
        assert_eq!(compat, SupportCompatibility::Ok, "compat must be Ok for (Tet, Fixed)");
    }

    // ------------------------------------------------------------------
    // Step 9: build_support_bcs — (Tet, Pinned) bit-identical BCs to (Tet, Fixed)
    //         but compat = PinnedOnTetEquivalentToFixed
    // ------------------------------------------------------------------

    /// `build_support_bcs(nodes, Pinned, Tet)` produces a BC list
    /// bit-identical to `build_support_bcs(nodes, Fixed, Tet)` for the same
    /// `nodes`, but the compatibility tag is `PinnedOnTetEquivalentToFixed`.
    ///
    /// The empty-input case also returns that tag (the tag encodes the call
    /// signature, not whether BCs were emitted).
    #[test]
    fn build_support_bcs_tet_pinned_bcs_identical_to_fixed_but_different_compat() {
        let nodes = [0usize, 2, 7];

        let (pinned_bcs, pinned_compat) =
            build_support_bcs(&nodes, SupportKind::Pinned, SupportBodyKind::Tet);
        let (fixed_bcs, fixed_compat) =
            build_support_bcs(&nodes, SupportKind::Fixed, SupportBodyKind::Tet);

        // BC lists must be bit-identical in length and content
        assert_eq!(pinned_bcs.len(), fixed_bcs.len(), "BC count must match");
        for (i, (pb, fb)) in pinned_bcs.iter().zip(fixed_bcs.iter()).enumerate() {
            assert_eq!(pb.dof, fb.dof, "bcs[{i}].dof mismatch");
            assert_eq!(
                pb.value.to_bits(),
                fb.value.to_bits(),
                "bcs[{i}].value mismatch"
            );
        }

        // Compat tags must differ
        assert_eq!(
            fixed_compat,
            SupportCompatibility::Ok,
            "(Tet, Fixed) compat must be Ok"
        );
        assert_eq!(
            pinned_compat,
            SupportCompatibility::PinnedOnTetEquivalentToFixed,
            "(Tet, Pinned) compat must be PinnedOnTetEquivalentToFixed"
        );

        // Empty-input case: compat tag still applies
        let (empty_bcs, empty_compat) =
            build_support_bcs(&[], SupportKind::Pinned, SupportBodyKind::Tet);
        assert!(empty_bcs.is_empty());
        assert_eq!(empty_compat, SupportCompatibility::PinnedOnTetEquivalentToFixed);
    }

    // ------------------------------------------------------------------
    // Step 11: end-to-end (Shell, Fixed) — node 0, tilted triangle fixture
    // ------------------------------------------------------------------

    /// Feed `build_support_bcs(&[0], Fixed, Shell)` into
    /// `apply_dirichlet_row_elimination` on a 18-DOF shell K.
    ///
    /// Uses the tilted triangle (Q = Ry(45°)·Rz(30°)) so that the drilling
    /// null-direction mixes across all rotational DOFs and every diagonal
    /// entry is nonzero — avoiding the singularity of the xy-plane UNIT_TRI.
    #[test]
    fn e2e_shell_fixed_node0_apply_dirichlet_bcs_correct() {
        let nodes = tilted_tri();
        let mut k = shell_k_sparse(&nodes);
        let mut f: Vec<f64> = (1..=18).map(|i| i as f64).collect();

        // Snapshot K and f before BC application.
        let k_before: Vec<Vec<f64>> =
            (0..18).map(|i| (0..18).map(|j| read(&k, i, j)).collect()).collect();
        let f_before = f.clone();

        // Build Fixed BCs for node 0 (DOFs 0..6) and apply.
        let (bcs, compat) = build_support_bcs(&[0], SupportKind::Fixed, SupportBodyKind::Shell);
        assert_eq!(compat, SupportCompatibility::Ok);
        assert_eq!(bcs.len(), 6);
        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        // (a) f[0..6] must be 0.0 (prescribed value for homogeneous BCs).
        for i in 0..6 {
            assert_eq!(
                f[i].to_bits(),
                0.0_f64.to_bits(),
                "f[{i}] must be 0.0 after Fixed on node 0"
            );
        }

        // (b) Row i in 0..6: diagonal = 1.0, all off-diagonals = 0.0.
        for i in 0..6 {
            assert_eq!(read(&k, i, i), 1.0, "K[{i}][{i}] must be 1.0");
            for j in 0..18 {
                if j != i {
                    assert_eq!(
                        read(&k, i, j),
                        0.0,
                        "K[{i}][{j}] must be 0.0 (row {i} zeroed)"
                    );
                }
            }
        }

        // (c) Column i in 0..6: all off-diagonal entries zero.
        for i in 0..6 {
            for j in 0..18 {
                if j != i {
                    assert_eq!(
                        read(&k, j, i),
                        0.0,
                        "K[{j}][{i}] must be 0.0 (col {i} zeroed)"
                    );
                }
            }
        }

        // (d) f[6..18] bit-identical to snapshot (homogeneous BCs → no RHS change).
        for j in 6..18 {
            assert_eq!(
                f[j].to_bits(),
                f_before[j].to_bits(),
                "f[{j}] must be bit-identical (homogeneous BCs contribute 0 to col-into-RHS)"
            );
        }

        // Regression guard: K[6..18][6..18] sub-block is untouched by FixedSupport on node 0.
        for i in 6..18 {
            for j in 6..18 {
                assert_eq!(
                    read(&k, i, j).to_bits(),
                    k_before[i][j].to_bits(),
                    "K[{i}][{j}] in unconstrained sub-block must be unchanged"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Step 13: end-to-end (Shell, Pinned) — rotational DOFs NOT clamped
    // ------------------------------------------------------------------

    /// Feed `build_support_bcs(&[0], Pinned, Shell)` into
    /// `apply_dirichlet_row_elimination` on the same 18-DOF shell K.
    ///
    /// Key property: DOFs 3, 4, 5 (rotational DOFs of node 0) must NOT be
    /// clamped — the K rows/cols and f entries for those DOFs must be
    /// bit-identical to their pre-call snapshots.
    #[test]
    fn e2e_shell_pinned_node0_leaves_rotational_dofs_untouched() {
        let nodes = tilted_tri();
        let mut k = shell_k_sparse(&nodes);
        let mut f: Vec<f64> = (1..=18).map(|i| i as f64).collect();

        // Snapshot K and f before BC application.
        let k_before: Vec<Vec<f64>> =
            (0..18).map(|i| (0..18).map(|j| read(&k, i, j)).collect()).collect();
        let f_before = f.clone();

        // Build Pinned BCs for node 0 (DOFs 0..3 only) and apply.
        let (bcs, compat) = build_support_bcs(&[0], SupportKind::Pinned, SupportBodyKind::Shell);
        assert_eq!(compat, SupportCompatibility::Ok);
        assert_eq!(bcs.len(), 3, "Pinned on shell emits 3 BCs per node");

        apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

        // (a) f[0..3] must be 0.0 (translational BCs prescribed to zero).
        for i in 0..3 {
            assert_eq!(
                f[i].to_bits(),
                0.0_f64.to_bits(),
                "f[{i}] must be 0.0 after Pinned on node 0 (translational DOF)"
            );
        }

        // (b) Row i in 0..3: diagonal = 1.0, all off-diagonals = 0.0.
        for i in 0..3 {
            assert_eq!(read(&k, i, i), 1.0, "K[{i}][{i}] must be 1.0 (translational row)");
            for j in 0..18 {
                if j != i {
                    assert_eq!(
                        read(&k, i, j),
                        0.0,
                        "K[{i}][{j}] must be 0.0 (translational row {i} zeroed)"
                    );
                }
            }
        }

        // (c) Column i in 0..3: all off-diagonal entries zero.
        for i in 0..3 {
            for j in 0..18 {
                if j != i {
                    assert_eq!(
                        read(&k, j, i),
                        0.0,
                        "K[{j}][{i}] must be 0.0 (translational col {i} zeroed)"
                    );
                }
            }
        }

        // (d) Rotational rows 3..6 of K must be bit-identical to snapshot.
        //     Regression guard: Pinned must NOT clamp θ_x, θ_y, θ_z.
        for i in 3..6 {
            for j in 3..18 {
                assert_eq!(
                    read(&k, i, j).to_bits(),
                    k_before[i][j].to_bits(),
                    "K[{i}][{j}] rotational row must be unchanged by Pinned BCs"
                );
            }
            // Critical: diagonal must NOT be 1.0 (i.e. not clamped).
            assert_ne!(
                read(&k, i, i),
                1.0,
                "K[{i}][{i}] rotational diagonal must not be clobbered to 1.0"
            );
        }

        // (e) f[3..18] bit-identical to snapshot (no BC at rotational DOFs and
        //     homogeneous translational BCs → column-into-RHS contributes 0).
        for j in 3..18 {
            assert_eq!(
                f[j].to_bits(),
                f_before[j].to_bits(),
                "f[{j}] must be bit-identical after Pinned (no BC here)"
            );
        }
    }
}
