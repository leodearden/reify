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
}
