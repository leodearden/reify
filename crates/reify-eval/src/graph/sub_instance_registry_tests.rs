//! The sub-instance template registry `EvaluationGraph` keeps agrees with the
//! cells stamped into `value_cells`.

use reify_compiler::ValueCellKind;
use reify_core::{ContentHash, Type, ValueCellId};

use super::{EvaluationGraph, ValueCellNode};

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

#[test]
fn remove_sub_instance_cells_drops_the_slot_cells_and_forgets_the_slot() {
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "len"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[1]", "d"));

    let removed = graph.remove_sub_instance_cells("P.s[0]", ["d", "len", "absent"]);

    let expected = vec![
        ValueCellId::new("P.s[0]", "d"),
        ValueCellId::new("P.s[0]", "len"),
        ValueCellId::new("P.s[0]", "absent"),
    ];
    assert_eq!(removed, expected, "every member's id, in members order");
    for id in &removed {
        assert!(
            !graph.value_cells.contains_key(id),
            "`{id}` must leave value_cells",
        );
    }
    assert_eq!(graph.sub_instance_template("P.s[0]"), None);
    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new("P.s[1]", "d")),
        "the untouched slot's cell must survive",
    );
    assert_eq!(graph.sub_instance_template("P.s[1]"), Some("Bolt"));
}

#[test]
fn a_removed_slot_restamped_from_another_template_records_the_new_one() {
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.remove_sub_instance_cells("P.s[0]", ["d"]);

    graph.insert_sub_instance_cell("Screw", cell("P.s[0]", "d"));

    assert_eq!(graph.sub_instance_template("P.s[0]"), Some("Screw"));
}
