//! The sub-instance template registry `EvaluationGraph` keeps agrees with the
//! cells stamped into `value_cells`.

use reify_compiler::ValueCellKind;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::MemberKey;

use super::cell_contributions::fixtures::rig_with_every_slot_kind;
use super::{
    CollectionSubInfo, EvaluationGraph, KeyedSubInfo, SubSlotRegistryMismatch, ValueCellNode,
};
use crate::DefEnv;

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
#[should_panic(
    expected = "a removed slot of `P.s` still holds a cell its collection's shape does not name: `P.s[0].stray`"
)]
fn remove_collection_slots_debug_asserts_no_forgotten_slot_keeps_a_cell_outside_its_shape() {
    let bolts = bolts_collection(&["d"]);
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "stray"));

    graph.remove_collection_slots(&bolts, 0..1);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(
    expected = "does not name: `P.s[0].stray`; `P.s[1].stray`; `P.s[2].stray`; `P.s[3].stray`"
)]
fn remove_collection_slots_names_every_stray_cell_in_sorted_order() {
    let bolts = bolts_collection(&["d"]);
    let mut graph = EvaluationGraph::default();
    for index in (0..4).rev() {
        let slot = format!("P.s[{index}]");
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "d"));
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "stray"));
    }

    graph.remove_collection_slots(&bolts, 0..4);
}

#[test]
fn remove_collection_slots_ignores_a_stray_cell_in_a_slot_it_keeps() {
    let bolts = bolts_collection(&["d"]);
    let mut graph = EvaluationGraph::default();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[1]", "d"));
    graph.insert_sub_instance_cell("Bolt", cell("P.s[1]", "stray"));

    graph.remove_collection_slots(&bolts, 0..1);

    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new("P.s[1]", "stray")),
        "a slot outside the removed range keeps every cell it holds",
    );
    assert_eq!(graph.sub_instance_template("P.s[1]"), Some("Bolt"));
}

fn graph_with_bolts_collection() -> EvaluationGraph {
    let mut graph = EvaluationGraph::default();
    graph.collection_subs.push(bolts_collection(&["d", "len"]));
    graph
}

#[test]
fn a_collection_slot_stamped_around_the_registry_is_a_mismatch() {
    let mut graph = graph_with_bolts_collection();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    let stray = cell("P.s[1]", "d");
    graph.value_cells.insert(stray.id.clone(), stray);

    assert_eq!(
        graph.sub_slot_registry_mismatches(),
        vec![SubSlotRegistryMismatch {
            slot: "P.s[1]".to_string(),
            sub_template: "Bolt".to_string(),
            registry_template: None,
        }],
    );
}

#[test]
fn a_collection_slot_registered_to_another_template_is_a_mismatch() {
    let mut graph = graph_with_bolts_collection();
    graph.insert_sub_instance_cell("Screw", cell("P.s[0]", "d"));

    let mismatches = graph.sub_slot_registry_mismatches();
    assert_eq!(
        mismatches,
        vec![SubSlotRegistryMismatch {
            slot: "P.s[0]".to_string(),
            sub_template: "Bolt".to_string(),
            registry_template: Some("Screw".to_string()),
        }],
    );
    assert_eq!(
        mismatches[0].to_string(),
        "`P.s[0]` (a slot of a `Bolt` sub) holds cells but the registry records `Screw`",
    );
}

#[test]
fn shrinking_then_regrowing_a_collection_leaves_no_mismatch() {
    let bolts = bolts_collection(&["d", "len"]);
    let mut graph = graph_with_bolts_collection();
    for index in 0..3 {
        let slot = format!("P.s[{index}]");
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "d"));
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "len"));
    }
    graph.remove_collection_slots(&bolts, 0..3);
    for index in 0..2 {
        let slot = format!("P.s[{index}]");
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "d"));
        graph.insert_sub_instance_cell("Bolt", cell(&slot, "len"));
    }

    assert_eq!(graph.sub_slot_registry_mismatches(), vec![]);
    assert_eq!(graph.sub_instance_template("P.s[1]"), Some("Bolt"));
    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new("P.s[1]", "d")),
        "the regrown slot must hold its cells",
    );
}

#[test]
fn an_empty_graph_has_no_mismatch() {
    let graph = graph_with_bolts_collection();

    assert_eq!(graph.sub_slot_registry_mismatches(), vec![]);
}

fn keyed_slot(parent: &str, sub_name: &str, key: &str) -> String {
    format!("{parent}.{}", MemberKey::new(key).path_segment(sub_name))
}

#[test]
fn a_keyed_slot_stamped_around_the_registry_is_a_mismatch() {
    let mut graph = EvaluationGraph::default();
    graph.keyed_subs.push(KeyedSubInfo {
        parent_entity: "P".to_string(),
        sub_name: "v".to_string(),
        structure_name: "Vent".to_string(),
        count_cell: None,
        member_keys: vec![MemberKey::new("intake"), MemberKey::new("exhaust")],
    });
    let intake = keyed_slot("P", "v", "intake");
    let stray = cell(&intake, "d");
    graph.value_cells.insert(stray.id.clone(), stray);

    assert_eq!(
        graph.sub_slot_registry_mismatches(),
        vec![SubSlotRegistryMismatch {
            slot: intake,
            sub_template: "Vent".to_string(),
            registry_template: None,
        }],
        "only the keyed slot holding a cell is checked",
    );
}

#[test]
fn from_templates_builds_a_registry_with_no_mismatch() {
    let module = rig_with_every_slot_kind();
    let mut graph = EvaluationGraph::from_env(DefEnv::of_module(&module, &[]));

    assert_eq!(graph.sub_slot_registry_mismatches(), vec![]);
    let keyed_a = keyed_slot("Rig", "keyed", "a");
    assert_eq!(
        graph.sub_instance_template(&keyed_a),
        Some("Bolt"),
        "anti-vacuity: from_env must register the keyed slot it stamps",
    );
    assert!(
        graph
            .value_cells
            .contains_key(&ValueCellId::new(&keyed_a, "len")),
        "anti-vacuity: from_env must stamp the keyed slot's cells",
    );

    let past_rack = graph
        .collection_subs
        .iter()
        .find(|col_sub| col_sub.parent_entity == "Rig" && col_sub.sub_name == "rack")
        .expect("the fixture registers `Rig.rack` as a collection sub")
        .slot_entity(2);
    let stray = cell(&past_rack, "len");
    graph.value_cells.insert(stray.id.clone(), stray);
    graph.insert_sub_instance_cell("Screw", cell(&keyed_a, "len"));

    let mut in_rig: Vec<SubSlotRegistryMismatch> = graph
        .sub_slot_registry_mismatches()
        .into_iter()
        .filter(|mismatch| mismatch.slot.starts_with("Rig."))
        .collect();
    in_rig.sort_by(|a, b| a.slot.cmp(&b.slot));
    assert_eq!(
        in_rig,
        vec![
            SubSlotRegistryMismatch {
                slot: keyed_a,
                sub_template: "Bolt".to_string(),
                registry_template: Some("Screw".to_string()),
            },
            SubSlotRegistryMismatch {
                slot: past_rack,
                sub_template: "Bolt".to_string(),
                registry_template: None,
            },
        ],
        "anti-vacuity: the check must see both the compiled collection and keyed slots",
    );
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(
    expected = "`P.s[1]` (a slot of a `Bolt` sub) holds cells but the registry records none"
)]
fn the_registry_assertion_names_the_unregistered_slot_and_its_template() {
    let mut graph = graph_with_bolts_collection();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));
    let stray = cell("P.s[1]", "d");
    graph.value_cells.insert(stray.id.clone(), stray);

    graph.debug_assert_sub_instance_registry_agrees();
}

#[test]
fn the_registry_assertion_accepts_an_agreeing_graph() {
    let mut graph = graph_with_bolts_collection();
    graph.insert_sub_instance_cell("Bolt", cell("P.s[0]", "d"));

    graph.debug_assert_sub_instance_registry_agrees();
}
