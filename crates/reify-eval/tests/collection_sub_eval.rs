//! Collection sub-structure evaluation tests (task 64).
//!
//! Tests for evaluating collection sub-components (`sub bolts : List<Bolt>`),
//! count-based elaboration, and count re-elaboration.

use reify_eval::graph::EvaluationGraph;
use reify_eval::Engine;
use reify_test_support::builders::value_ref_typed;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
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

// ─── step-9: Engine.eval() with collection count=4 ───

#[test]
fn eval_collection_sub_produces_instances() {
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

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Verify count cell evaluated to Int(4)
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    let count_val = result.values.get(&count_id);
    assert_eq!(
        count_val,
        Some(&Value::Int(4)),
        "count cell should evaluate to Int(4)"
    );

    // Verify 4 instances have diameter = 0.01 (10mm)
    for i in 0..4 {
        let scoped_id = ValueCellId::new(&format!("Parent.bolts[{}]", i), "diameter");
        let val = result.values.get(&scoped_id);
        assert_eq!(
            val,
            Some(&Value::length(0.01)),
            "bolts[{}].diameter should be 10mm = 0.01m",
            i
        );
    }

    // No bolts[4]
    let no_instance = ValueCellId::new("Parent.bolts[4]", "diameter");
    assert!(
        result.values.get(&no_instance).is_none(),
        "should not have bolts[4]"
    );
}

// ─── step-11: count Undef means no instances ───

#[test]
fn eval_collection_sub_undef_count_no_instances() {
    // Bolt template: param diameter : Scalar = 10mm
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .build();

    // Parent template: count cell depends on an Undef param (no default)
    // __count_bolts = ValueRef(Parent.n), but n has no default -> Undef
    let count_expr = value_ref_typed("Parent", "n", Type::Int);
    let parent = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "n", Type::Int, None) // no default -> Undef
        .let_binding("Parent", "__count_bolts", Type::Int, count_expr)
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component(
            "bolts",
            "Bolt",
            ValueCellId::new("Parent", "__count_bolts"),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Count cell should be Undef
    let count_id = ValueCellId::new("Parent", "__count_bolts");
    let count_val = result.values.get(&count_id);
    assert_eq!(
        count_val,
        Some(&Value::Undef),
        "count cell should be Undef when param n has no default"
    );

    // No instances should exist
    for i in 0..4 {
        let scoped_id = ValueCellId::new(&format!("Parent.bolts[{}]", i), "diameter");
        assert!(
            result.values.get(&scoped_id).is_none(),
            "bolts[{}] should not exist when count is Undef",
            i
        );
    }
}
