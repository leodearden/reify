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
    match (body, kind) {
        (SupportBodyKind::Shell, SupportKind::Fixed) => {
            // 6 DOFs per node: u_x, u_y, u_z, θ_x, θ_y, θ_z  (offsets 0..6)
            let bcs = nodes
                .iter()
                .flat_map(|&n| {
                    (0..6).map(move |i| DirichletBc {
                        dof: 6 * n + i,
                        value: 0.0,
                    })
                })
                .collect();
            (bcs, SupportCompatibility::Ok)
        }
        (SupportBodyKind::Shell, SupportKind::Pinned) => {
            // 3 translational DOFs per node: u_x, u_y, u_z (offsets 0..3).
            // Rotational DOFs (offsets 3..6) are intentionally left free.
            let bcs = nodes
                .iter()
                .flat_map(|&n| {
                    (0..3).map(move |i| DirichletBc {
                        dof: 6 * n + i,
                        value: 0.0,
                    })
                })
                .collect();
            (bcs, SupportCompatibility::Ok)
        }
        (SupportBodyKind::Tet, SupportKind::Fixed) => {
            // 3 DOFs per node: u_x, u_y, u_z (offsets 0..3). 3-stride.
            let bcs = nodes
                .iter()
                .flat_map(|&n| {
                    (0..3).map(move |i| DirichletBc {
                        dof: 3 * n + i,
                        value: 0.0,
                    })
                })
                .collect();
            (bcs, SupportCompatibility::Ok)
        }
        (SupportBodyKind::Tet, SupportKind::Pinned) => {
            // Tet nodes have no rotational DOFs, so Pinned is bit-identical to Fixed.
            // We surface the user-intent mismatch via the compat tag.
            let bcs = nodes
                .iter()
                .flat_map(|&n| {
                    (0..3).map(move |i| DirichletBc {
                        dof: 3 * n + i,
                        value: 0.0,
                    })
                })
                .collect();
            (bcs, SupportCompatibility::PinnedOnTetEquivalentToFixed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Step 1: enum smoke tests — all three types, all variants, all derives
    // ------------------------------------------------------------------

    #[test]
    fn enum_types_exist_with_correct_variants() {
        // SupportKind
        let fixed = SupportKind::Fixed;
        let pinned = SupportKind::Pinned;
        // Copy: assign without move
        let fixed_copy = fixed;
        assert_eq!(fixed_copy, fixed, "SupportKind must be PartialEq");
        assert_ne!(fixed, pinned, "Fixed != Pinned");
        // Debug
        let _ = format!("{:?}", fixed);
        let _ = format!("{:?}", pinned);

        // SupportBodyKind
        let shell = SupportBodyKind::Shell;
        let tet = SupportBodyKind::Tet;
        let shell_copy = shell;
        assert_eq!(shell_copy, shell, "SupportBodyKind must be PartialEq");
        assert_ne!(shell, tet, "Shell != Tet");
        let _ = format!("{:?}", shell);
        let _ = format!("{:?}", tet);

        // SupportCompatibility
        let ok = SupportCompatibility::Ok;
        let equiv = SupportCompatibility::PinnedOnTetEquivalentToFixed;
        let ok_copy = ok;
        assert_eq!(ok_copy, ok, "SupportCompatibility must be PartialEq");
        assert_ne!(ok, equiv, "Ok != PinnedOnTetEquivalentToFixed");
        let _ = format!("{:?}", ok);
        let _ = format!("{:?}", equiv);
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
}
