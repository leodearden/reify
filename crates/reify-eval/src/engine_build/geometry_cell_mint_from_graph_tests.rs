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
use crate::DefEnv;
use crate::graph::EvaluationGraph;
use crate::graph::geometry_cell_link_fixture::{
    GEOMETRY_LIST_MEMBERS, SCALAR_GEOMETRY_MEMBERS, SRC, TEMPLATE, assert_some_realization_links,
    compiled_realization_named,
};

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
    let graph = EvaluationGraph::from_env(DefEnv::of_module(&module, &[]));

    for member in GEOMETRY_LIST_MEMBERS {
        let cell = ValueCellId::new(TEMPLATE, member);
        assert_some_realization_links(&graph, &cell);
        assert_eq!(
            mint_from_graph(&cell, &graph),
            None,
            "`{cell}` is a List<Geometry> cell: it must never receive a scalar \
             handle minted from one of its elements"
        );
    }

    for member in SCALAR_GEOMETRY_MEMBERS {
        let cell = ValueCellId::new(TEMPLATE, member);
        let own_realization = compiled_realization_named(&module, member);
        match mint_from_graph(&cell, &graph) {
            Some(Value::GeometryHandle {
                realization_ref,
                kernel_handle: None,
                ..
            }) => assert_eq!(
                realization_ref, own_realization,
                "control: the scalar let `{cell}` must mint a handle for its OWN realization"
            ),
            other => panic!(
                "control: the scalar geometry let `{cell}` must still mint a symbolic handle; \
                 got {other:?}"
            ),
        }
    }
}
