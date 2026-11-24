//! Pins `EvaluationGraph::scalar_geometry_cell_realization`, the one home of
//! the rule mapping a scalar geometry cell to the realization whose handle it
//! holds (task #9846). Both graph-backed mints, the graph mint and
//! `InstanceMintScope`, resolve through it.

use reify_core::ValueCellId;
use reify_test_support::parse_and_compile;

use super::EvaluationGraph;
use super::geometry_cell_link_fixture::{
    GEOMETRY_LIST_MEMBERS, SCALAR_GEOMETRY_MEMBERS, SRC, TEMPLATE, assert_some_realization_links,
    compiled_realization_named,
};
use crate::DefEnv;

/// A top-level Solid param and a guarded same-name override: the compiler
/// names both realizations `body` and links both to the one cell `T.body`.
const GUARDED_SAME_NAME_PARAM_SRC: &str = r#"
    structure T {
        param size : Length = 10mm
        param on : Bool = true
        param body : Solid = box(size, 10mm, 10mm)
        where on {
            param body : Solid = cylinder(5mm, size)
        }
    }
"#;

#[test]
fn a_scalar_geometry_let_resolves_to_its_own_realization() {
    let module = parse_and_compile(SRC);
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&module, &[]));

    for member in SCALAR_GEOMETRY_MEMBERS {
        let own_realization = compiled_realization_named(&module, member);
        let cell = ValueCellId::new(TEMPLATE, member);

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
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&parse_and_compile(SRC), &[]));

    for member in GEOMETRY_LIST_MEMBERS {
        let cell = ValueCellId::new(TEMPLATE, member);
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
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&parse_and_compile(SRC), &[]));

    assert!(
        graph
            .scalar_geometry_cell_realization(&ValueCellId::new(TEMPLATE, "missing"))
            .is_none()
    );
}

/// Which of the two links answers is unspecified: `realizations` iterates in
/// hash order. Choosing a policy for this shape is a separate follow-up.
#[test]
fn a_scalar_geometry_cell_linked_by_a_guarded_same_name_param_resolves_to_one_of_its_links() {
    let graph = EvaluationGraph::from_env(DefEnv::of_module(
        &parse_and_compile(GUARDED_SAME_NAME_PARAM_SRC),
        &[],
    ));
    let cell = ValueCellId::new("T", "body");
    let linking: Vec<_> = graph
        .realizations
        .values()
        .filter(|r| r.geometry_cell.as_ref() == Some(&cell))
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(
        linking.len(),
        2,
        "precondition: the compiler links a top-level Solid param and its guarded \
         same-name override to the one cell `{cell}`; if this count changed, a \
         construction-time policy for the shape has landed and this test must be \
         revisited"
    );

    let realization = graph
        .scalar_geometry_cell_realization(&cell)
        .unwrap_or_else(|| panic!("the scalar geometry param `{cell}` must resolve"));

    assert!(
        linking.contains(&realization.id),
        "`{cell}` must resolve to one of its own links {linking:?}, got `{}`",
        realization.id
    );
    assert_eq!(realization.geometry_cell.as_ref(), Some(&cell));
}
