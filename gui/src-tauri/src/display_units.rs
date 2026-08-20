//! Thin re-export of the per-cell display-unit ladder registry.
//!
//! The registry (task #5199) relocated to `reify_core::display_units`
//! (task #5232) so it is reachable from reify-ir/reify-expr/reify-lsp, not
//! just the GUI. Re-exporting it here keeps
//! `reify_gui::display_units::{DimensionLadder, UnitOption, unit_ladders}`
//! resolving unchanged for `main.rs`'s `get_unit_ladders` Tauri command and
//! the Parameters panel's unit picker.
//!
//! PRD display-unit-preference §5's auto-scaling policy travels *with* the
//! table (task #5236): `DimensionLadder::auto_scaled` and its
//! [`AutoScaleChoice`] result are re-exported alongside the data, so this
//! module stays a complete view of the registry module rather than a
//! data-only slice a GUI-side consumer would have to reach past.

pub use reify_core::display_units::{
    AutoScale, AutoScaleChoice, DimensionLadder, UnitOption, unit_ladders,
};

#[cfg(test)]
mod tests {
    //! Compat smoke test for the GUI-facing re-export (task #5232). The
    //! ladder registry now lives in `reify_core::display_units`; this pins
    //! that `reify_gui::display_units::{unit_ladders, DimensionLadder,
    //! UnitOption, AutoScale}` still resolves — with the new `§4`
    //! `derived_unit_name` / `§5` `auto_scale` fields reachable — so
    //! `main.rs`'s `get_unit_ladders` Tauri command and the Parameters
    //! panel's unit picker keep working. The data-level invariants are
    //! owned by reify-core's own tests; this only guards the re-export
    //! surface (a broken/renamed re-export would fail to compile or fail
    //! here rather than silently break the command).

    use super::{AutoScale, AutoScaleChoice, DimensionLadder, UnitOption, unit_ladders};

    #[test]
    fn reexport_exposes_all_curated_dimensions_with_new_fields() {
        let ladders: Vec<DimensionLadder> = unit_ladders();
        assert!(
            !ladders.is_empty(),
            "re-exported unit_ladders() must not be empty"
        );

        // All 10 curated dimensions (the original 7 plus Force/Energy/Power)
        // must be reachable through the re-export.
        for dimension in [
            "Length", "Area", "Volume", "Angle", "Mass", "Pressure", "Density", "Force", "Energy",
            "Power",
        ] {
            let l = ladders
                .iter()
                .find(|l| l.dimension == dimension)
                .unwrap_or_else(|| panic!("re-export missing ladder for {dimension:?}"));

            // The new §4/§5 fields must be reachable through the re-export.
            assert!(
                !l.derived_unit_name.is_empty(),
                "{dimension:?} derived_unit_name empty through re-export"
            );
            let _auto_scale: &Option<AutoScale> = &l.auto_scale;

            // §5's auto-scaling policy travels with the table (task #5236):
            // both the `AutoScaleChoice` result type and the
            // `DimensionLadder::auto_scaled` inherent method must resolve
            // through the shim, so a GUI-side consumer can apply the policy
            // without reaching past `reify_gui::display_units`. Only the
            // re-export surface is guarded here — which variant comes back is
            // reify-core's own invariant, tested there.
            assert!(
                matches!(
                    l.auto_scaled(1.0),
                    AutoScaleChoice::Static
                        | AutoScaleChoice::Rung(_)
                        | AutoScaleChoice::Engineering { .. }
                ),
                "{dimension:?} auto_scaled() unreachable through re-export"
            );

            // The relocated UnitOption type is also reachable through the re-export.
            let first: &UnitOption = l
                .units
                .first()
                .unwrap_or_else(|| panic!("{dimension:?} ladder has no units"));
            assert!(
                !first.label.is_empty(),
                "{dimension:?} first unit has empty label"
            );
        }
    }
}
