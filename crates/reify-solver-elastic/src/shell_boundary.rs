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
            unimplemented!("(Shell, Pinned) — step-6")
        }
        (SupportBodyKind::Tet, SupportKind::Fixed) => {
            unimplemented!("(Tet, Fixed) — step-8")
        }
        (SupportBodyKind::Tet, SupportKind::Pinned) => {
            unimplemented!("(Tet, Pinned) — step-10")
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
}
