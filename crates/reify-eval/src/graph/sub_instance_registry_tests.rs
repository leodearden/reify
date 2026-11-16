//! The sub-instance template registry `EvaluationGraph` keeps agrees with the
//! cells stamped into `value_cells`.

use reify_compiler::ValueCellKind;
use reify_core::{ContentHash, Type, ValueCellId};

use super::{CollectionSubInfo, EvaluationGraph, ValueCellNode};

fn cell(entity: &str, member: &str) -> ValueCellNode {
    ValueCellNode {
        id: ValueCellId::new(entity, member),
        kind: ValueCellKind::Param,
        cell_type: Type::length(),
        default_expr: None,
        content_hash: ContentHash::of_str(member),
    }
}

#[test]
fn insert_sub_instance_cell_records_the_template_being_stamped_over_a_stale_one() {
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.value_cells.remove(&ValueCellId::new("P.s[0]", "d"));

    graph.insert_sub_instance_cell("Screw", cell("P.s[0]", "d"));

    assert_eq!(
        graph.sub_instance_template("P.s[0]"),
        Some("Screw"),
        "the registry must name the template the slot's cells are stamped from",
    );
}

#[test]
fn insert_sub_instance_cell_keeps_one_slot_template_across_its_cells() {
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "len"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[1]", "d"));

    assert_eq!(graph.sub_instance_template("P.s[0]"), Some("Bolt"));
    assert_eq!(graph.sub_instance_template("P.s[1]"), Some("Bolt"));
    for (entity, member) in [("P.s[0]", "d"), ("P.s[0]", "len"), ("P.s[1]", "d")] {
        assert!(
            graph
                .value_cells
                .contains_key(&ValueCellId::new(entity, member)),
            "`{entity}.{member}` must be stamped",
        );
    }
    assert_eq!(graph.sub_instance_template("P.s[2]"), None);
}

fn bolts_collection(members: &[&str]) -> CollectionSubInfo {
    CollectionSubInfo {
        parent_entity: "P".to_string(),
        sub_name: "s".to_string(),
        structure_name: "Bolt".to_string(),
        count_cell: ValueCellId::new("P", "__count_s"),
        child_value_cells: members
            .iter()
            .map(|member| {
                (
                    member.to_string(),
                    ValueCellKind::Param,
                    Type::length(),
                    None,
                )
            })
            .collect(),
    }
}

#[test]
fn remove_collection_slots_drops_the_slots_cells_and_forgets_the_slots() {
    let bolts = bolts_collection(&["d", "len"]);
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "len"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[1]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[2]", "d"));

    let removed = graph.remove_collection_slots(&bolts, 0..2);

    let expected = vec![
        ValueCellId::new("P.s[0]", "d"),
        ValueCellId::new("P.s[0]", "len"),
        ValueCellId::new("P.s[1]", "d"),
        ValueCellId::new("P.s[1]", "len"),
    ];
    assert_eq!(removed, expected, "every shape member's id, slot by slot");
    for id in &removed {
        assert!(
            !graph.value_cells.contains_key(id),
            "`{id}` must leave value_cells",
        );
    }
    assert_eq!(graph.sub_instance_template("P.s[0]"), None);
    assert_eq!(graph.sub_instance_template("P.s[1]"), None);
    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new("P.s[2]", "d")),
        "the slot outside the range must survive",
    );
    assert_eq!(graph.sub_instance_template("P.s[2]"), Some("Bolt"));
}

#[test]
fn a_removed_slot_restamped_from_another_template_records_the_new_one() {
    let bolts = bolts_collection(&["d"]);
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.remove_collection_slots(&bolts, 0..1);

    graph.insert_sub_instance_cell("Screw", cell("P.s[0]", "d"));

    assert_eq!(graph.sub_instance_template("P.s[0]"), Some("Screw"));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a removed slot of `P.s` still holds a cell")]
fn remove_collection_slots_refuses_to_forget_a_slot_holding_a_cell_outside_its_shape() {
    let bolts = bolts_collection(&["d"]);
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "stray"));

    graph.remove_collection_slots(&bolts, 0..1);
}
