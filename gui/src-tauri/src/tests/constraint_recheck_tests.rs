//! Pins the constraint re-check in `surface_geometry_derived_cells`: once a pass
//! surfaces geometry-derived cells it may upgrade an Indeterminate verdict those
//! values settle, but never a geometric Conforms, whose verdict only the
//! measure pass may give (C1).

use super::test_helpers::{
    find_moi_principal_constraint, rigid_mass_props_session, visible_realization_keys,
};
use crate::types::{ConstraintData, GuiState};

/// A `: Rigid` body (so the PD constraint is Indeterminate until the re-check)
/// carrying a geometric Conforms that no kernel here can measure.
const RIGID_GEOMETRIC_CONFORMS_SRC: &str = r#"
structure def GdtRigidProbe : Rigid {
    param depth : Length = 300mm
    param geometry : Solid = box(100mm, 100mm, depth)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    param tol : Flatness = Flatness(tolerance_value: 0.1mm, feature: geometry)
    constraint Conforms(tolerance: tol, measured_deviation: 0mm, feature_departure: 0mm, actual: geometry)
}
"#;

/// Drive `source` through every surface that runs the re-check: load, the
/// frontend's selective posture with two cache-sourced rebuilds, and an FEA
/// case switch. Each state is paired with the name of the pass that built it.
fn gui_states_across_recheck_paths(
    source: &str,
    module_name: &str,
) -> Vec<(&'static str, GuiState)> {
    let mut session = rigid_mass_props_session();
    let loaded = session
        .load_from_source(source, module_name)
        .expect("fixture should load");
    let keys = visible_realization_keys(&loaded);
    assert!(
        !keys.is_empty(),
        "the fixture must render a realization, else sync_demand is a no-op"
    );
    session.sync_demand(&keys);
    let rebuilt = session
        .build_gui_state()
        .expect("first selective rebuild should succeed");
    let rebuilt_again = session
        .build_gui_state()
        .expect("second selective rebuild should succeed");
    let switched = session
        .set_active_fea_case("overload")
        .expect("FEA case switch should succeed");
    vec![
        ("load", loaded),
        ("selective rebuild", rebuilt),
        ("second selective rebuild", rebuilt_again),
        ("FEA case switch", switched),
    ]
}

fn find_conforms<'a>(state: &'a GuiState, ctx: &str) -> &'a ConstraintData {
    let by_label = |c: &&ConstraintData| {
        c.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Conforms"))
    };
    let by_expression = |c: &&ConstraintData| c.expression.contains("effective_tolerance_zone");
    state
        .constraints
        .iter()
        .find(by_label)
        .or_else(|| state.constraints.iter().find(by_expression))
        .unwrap_or_else(|| {
            panic!(
                "[{ctx}] no Conforms constraint; have: {:?}",
                state
                    .constraints
                    .iter()
                    .map(|c| (&c.node_id, &c.label, &c.expression, &c.status))
                    .collect::<Vec<_>>()
            )
        })
}

fn assert_recheck_ran_but_conforms_stayed_unmeasured(state: &GuiState, ctx: &str) {
    let pd = find_moi_principal_constraint(state);
    assert_eq!(
        pd.status, "satisfied",
        "[{ctx}] non-vacuity: only the re-check settles the PD constraint, so it must \
         have run in this pass"
    );
    let conforms = find_conforms(state, ctx);
    assert_eq!(
        conforms.status, "indeterminate",
        "[{ctx}] an unmeasured geometric Conforms must stay indeterminate (C1); got {conforms:?}"
    );
}

#[test]
fn geometric_conforms_is_never_upgraded_by_the_post_geometry_recheck() {
    for (ctx, state) in
        gui_states_across_recheck_paths(RIGID_GEOMETRIC_CONFORMS_SRC, "gdt_rigid_probe")
    {
        assert_recheck_ran_but_conforms_stayed_unmeasured(&state, ctx);
    }
}
