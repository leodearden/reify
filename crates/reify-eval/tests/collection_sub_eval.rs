//! Collection sub-structure evaluation tests (task 64).
//!
//! Tests for evaluating collection sub-components (`sub bolts : List<Bolt>`),
//! count-based elaboration, and count re-elaboration.

use reify_eval::graph::EvaluationGraph;
use reify_test_support::builders::value_ref_typed;
use reify_test_support::TopologyTemplateBuilder;
use reify_types::*;

// ─── step-7: collection sub elaboration in from_templates ───

#[test]
fn from_templates_creates_collection_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: param n=4, count_cell __count_bolts = n, collection sub bolts
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(4), Type::Int)),
        )
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component(
            "bolts",
            "Bolt",
            ValueCellId::new("Parent", "__count_bolts"),
        )
        .build();

    let graph = EvaluationGraph::from_templates(&[parent, bolt]);

    // Verify 4 scoped instances exist: Parent.bolts[0].diameter through Parent.bolts[3].diameter
    for i in 0..4 {
        let scoped_entity = format!("Parent.bolts[{}]", i);
        let scoped_id = ValueCellId::new(&scoped_entity, "diameter");
        assert!(
            graph.value_cells.contains_key(&scoped_id),
            "expected scoped value cell {} to exist in graph",
            scoped_id,
        );
    }

    // Verify no instance [4] exists
    let no_instance = ValueCellId::new("Parent.bolts[4]", "diameter");
    assert!(
        !graph.value_cells.contains_key(&no_instance),
        "should not have bolts[4]",
    );
}
