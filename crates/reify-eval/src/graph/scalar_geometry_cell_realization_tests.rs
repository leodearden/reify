//! Pins `EvaluationGraph::scalar_geometry_cell_realization`, the one home of
//! the rule mapping a scalar geometry cell to the realization whose handle it
//! holds (task #9846). Both symbolic geometry mints resolve through it.

use reify_core::ValueCellId;
use reify_test_support::parse_and_compile;

use super::EvaluationGraph;

const SRC: &str = r#"structure S {
    let a = box(10mm, 10mm, 10mm)
    let one = generate(1, |i| cylinder(5mm, 20mm))
    let holes = generate(3, |i| cylinder(5mm, 20mm))
}"#;

#[test]
fn a_scalar_geometry_let_resolves_to_its_own_realization() {
    let module = parse_and_compile(SRC);
    let graph = EvaluationGraph::from_templates(&module.templates);
    let a_realization = module
        .templates
        .iter()
        .flat_map(|t| &t.realizations)
        .find(|r| r.name.as_deref() == Some("a"))
        .map(|r| r.id.clone())
        .expect("SRC must compile a realization named `a`");
    let cell = ValueCellId::new("S", "a");

    let realization = graph
        .scalar_geometry_cell_realization(&cell)
        .expect("the scalar geometry let `a` must resolve to a realization");

    assert_eq!(
        realization.id, a_realization,
        "`a` must resolve to its OWN realization"
    );
    assert_eq!(realization.geometry_cell.as_ref(), Some(&cell));
}

#[test]
fn a_geometry_list_cell_resolves_to_no_realization() {
    let graph = EvaluationGraph::from_templates(&parse_and_compile(SRC).templates);

    for member in ["one", "holes"] {
        let cell = ValueCellId::new("S", member);
        assert!(
            graph
                .realizations
                .values()
                .any(|r| r.geometry_cell.as_ref() == Some(&cell)),
            "precondition: some realization must link `{cell}` (task #5385's list \
             link), or this test is vacuous"
        );
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
            .scalar_geometry_cell_realization(&ValueCellId::new("S", "missing"))
            .is_none()
    );
}
