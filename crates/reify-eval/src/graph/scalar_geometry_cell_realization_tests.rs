//! Pins `EvaluationGraph::scalar_geometry_cell_realization`, the one home of
//! the rule mapping a scalar geometry cell to the realization whose handle it
//! holds (task #9846). Both graph-backed mints, the graph mint and
//! `InstanceMintScope`, resolve through it.

use reify_core::{RealizationNodeId, ValueCellId};
use reify_test_support::parse_and_compile;

use super::EvaluationGraph;
use super::geometry_cell_link_fixture::{
    GEOMETRY_LIST_MEMBERS, SCALAR_GEOMETRY_MEMBERS, SRC, assert_some_realization_links,
};

#[test]
fn a_scalar_geometry_let_resolves_to_its_own_realization() {
    let module = parse_and_compile(SRC);
    let graph = EvaluationGraph::from_templates(&module.templates);

    for member in SCALAR_GEOMETRY_MEMBERS {
        let own_realization = module
            .templates
            .iter()
            .flat_map(|t| &t.realizations)
            .find(|r| r.name.as_deref() == Some(member))
            .map(|r| r.id.clone())
            .unwrap_or_else(|| panic!("SRC must compile a realization named `{member}`"));
        let cell = ValueCellId::new("T", member);

        let realization = graph
            .scalar_geometry_cell_realization(&cell)
            .unwrap_or_else(|| panic!("the scalar geometry let `{cell}` must resolve"));

        assert_eq!(
            realization.id, own_realization,
            "`{cell}` must resolve to its OWN realization"
        );
        assert_eq!(realization.geometry_cell.as_ref(), Some(&cell));
    }
}

#[test]
fn a_geometry_list_cell_resolves_to_no_realization() {
    let graph = EvaluationGraph::from_templates(&parse_and_compile(SRC).templates);

    for member in GEOMETRY_LIST_MEMBERS {
        let cell = ValueCellId::new("T", member);
        assert_some_realization_links(&graph, &cell);
        assert!(
            graph.scalar_geometry_cell_realization(&cell).is_none(),
            "`{cell}` is a List<Geometry> cell: its value is never one element's \
             handle, so it must resolve to no realization"
        );
    }
}

#[test]
fn a_cell_absent_from_the_graph_resolves_to_no_realization() {
    let graph = EvaluationGraph::from_templates(&parse_and_compile(SRC).templates);

    assert!(
        graph
            .scalar_geometry_cell_realization(&ValueCellId::new("T", "missing"))
            .is_none()
    );
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "is linked by two realizations")]
fn a_scalar_geometry_cell_linked_by_two_realizations_panics_in_a_debug_build() {
    let mut graph = EvaluationGraph::from_templates(&parse_and_compile(SRC).templates);
    let cell = ValueCellId::new("T", "a");
    let mut second = graph
        .scalar_geometry_cell_realization(&cell)
        .expect("precondition: `T.a` must resolve")
        .clone();
    second.id = RealizationNodeId::new("T", u32::MAX);
    graph.realizations.insert(second.id.clone(), second);

    graph.scalar_geometry_cell_realization(&cell);
}
