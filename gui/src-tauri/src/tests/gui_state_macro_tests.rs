// `gui_state!` macro — keyed classification (L5 plan steps 1/2).
//
// Exercises the `diffed keyed(...)` field arm in isolation, using a minimal
// `MiniState`/`MiniDelta` invocation, before the real `GuiState` migration
// (step 8). See `gui_state_schema.rs` for the macro definition.
//
// Field DSL note: the macro takes an explicit `changed=<ident>` parameter
// (in addition to the plan-described `removed=<ident>`) naming the delta's
// "changed" field. Plain `macro_rules!` cannot synthesize a new identifier
// like `changed_items` by concatenating a literal prefix onto `$name` — that
// requires an external crate (e.g. `paste`), which the task's design
// decisions rule out ("no new crate"). Explicit naming mirrors the
// already-explicit `removed=`/event-name params and design decision 3's
// precedent for irregular names. See esc-5034-1.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MiniItem {
    id: String,
    val: i64,
}

fn item(id: &str, val: i64) -> MiniItem {
    MiniItem {
        id: id.to_string(),
        val,
    }
}

crate::gui_state_schema::gui_state! {
    state=MiniState, delta=MiniDelta, diff_fn=diff_mini, events_fn=mini_events;
    diffed keyed(key=id, item="item", update="item-update", remove="item-removed", changed=changed_items, removed=removed_item_ids)
    items: Vec<MiniItem>,
}

#[test]
fn diff_mini_reports_changed_and_added_items_keeps_unchanged_out() {
    let old = MiniState {
        items: vec![item("a", 1), item("unchanged", 7)],
    };
    let new = MiniState {
        items: vec![item("a", 2), item("unchanged", 7), item("b", 3)],
    };

    let delta = diff_mini(&old, &new);

    assert_eq!(delta.changed_items, vec![item("a", 2), item("b", 3)]);
    assert!(delta.removed_item_ids.is_empty());
}

#[test]
fn diff_mini_reports_dropped_keys_as_removed_item_ids() {
    let old = MiniState {
        items: vec![item("a", 1), item("b", 2)],
    };
    let new = MiniState {
        items: vec![item("a", 1)],
    };

    let delta = diff_mini(&old, &new);

    assert!(delta.changed_items.is_empty());
    assert_eq!(delta.removed_item_ids, vec!["b".to_string()]);
}

#[test]
fn mini_events_emits_update_per_change_then_removed_per_drop() {
    let old = MiniState {
        items: vec![item("a", 1), item("b", 2)],
    };
    let new = MiniState {
        items: vec![item("a", 99)],
    };

    let delta = diff_mini(&old, &new);
    let events = mini_events(&delta);

    assert_eq!(
        events,
        vec![
            (
                "item-update".to_string(),
                serde_json::to_value(item("a", 99)).unwrap(),
            ),
            (
                "item-removed".to_string(),
                serde_json::Value::String("b".to_string()),
            ),
        ]
    );
}

#[test]
fn mini_delta_full_marks_every_item_changed_with_no_removals() {
    let state = MiniState {
        items: vec![item("a", 1), item("b", 2)],
    };

    let delta = MiniDelta::full(&state);

    assert_eq!(delta.changed_items, state.items);
    assert!(delta.removed_item_ids.is_empty());
}
