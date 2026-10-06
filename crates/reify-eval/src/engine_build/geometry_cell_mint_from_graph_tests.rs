//! `Engine::mint_symbolic_geometry_handle_for_cell_from_graph` answers only for
//! a SCALAR geometry cell (task #6460).
//!
//! The mint resolves a cell to its realization through
//! `RealizationNodeData.geometry_cell`. Since task #5385 that link is N:1 for a
//! geometry-list let, and for a length-1 list it looks exactly like a scalar
//! let's 1:1 link, so only the cell's shape can stop the mint writing one
//! element's scalar handle into a `List<Geometry>` cell.
//!
//! Driven directly rather than through `edit_param`, its sole caller: that path
//! reaches the mint only for a cell whose reeval is exactly `Value::Undef`, and
//! a geometry list reevaluates to `List([Undef; n])`, so a public-seam test
//! would pass whether or not the guard existed.

use std::collections::HashMap;

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};
use reify_test_support::parse_and_compile;

use super::Engine;
use crate::graph::EvaluationGraph;

const SRC: &str = r#"structure S {
    let a = box(10mm, 10mm, 10mm)
    let one = generate(1, |i| cylinder(5mm, 20mm))
    let holes = generate(3, |i| cylinder(5mm, 20mm))
}"#;

fn mint_from_graph(cell: &ValueCellId, graph: &EvaluationGraph) -> Option<Value> {
    Engine::mint_symbolic_geometry_handle_for_cell_from_graph(
        cell,
        graph,
        &ValueMap::new(),
        &[],
        &HashMap::new(),
    )
}

#[test]
fn from_graph_mint_never_writes_a_scalar_handle_into_a_geometry_list_cell() {
    let module = parse_and_compile(SRC);
    let graph = EvaluationGraph::from_templates(&module.templates);

    for member in ["one", "holes"] {
        let cell = ValueCellId::new("S", member);
        assert!(
            graph
                .realizations
                .values()
                .any(|r| r.geometry_cell.as_ref() == Some(&cell)),
            "precondition: some realization must link `{cell}` (task #5385's list \
             link), or the mint cannot reach the cell and this test is vacuous"
        );
        assert_eq!(
            mint_from_graph(&cell, &graph),
            None,
            "`{cell}` is a List<Geometry> cell: it must never receive a scalar \
             handle minted from one of its elements"
        );
    }

    let a_realization = module
        .templates
        .iter()
        .flat_map(|t| &t.realizations)
        .find(|r| r.name.as_deref() == Some("a"))
        .map(|r| r.id.clone())
        .expect("SRC must compile a realization named `a`");
    match mint_from_graph(&ValueCellId::new("S", "a"), &graph) {
        Some(Value::GeometryHandle {
            realization_ref,
            kernel_handle: None,
            ..
        }) => assert_eq!(
            realization_ref, a_realization,
            "control: the scalar let `a` must mint a handle for its OWN realization"
        ),
        other => panic!(
            "control: the scalar geometry let `a` must still mint a symbolic handle; \
             got {other:?}"
        ),
    }
}
