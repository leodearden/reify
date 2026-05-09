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
