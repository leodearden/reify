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

use crate::gui_state_schema::gui_state;

// `pub(crate)` (not private) so the `pub` fields the macro generates on
// `MiniDelta` don't warn `private_interfaces` — mirrors how every real
// `GuiState` item type (`MeshData`, `ValueData`, ...) is `pub`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MiniItem {
    id: String,
    val: i64,
}

fn item(id: &str, val: i64) -> MiniItem {
    MiniItem {
        id: id.to_string(),
        val,
    }
}

gui_state! {
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

// --- `diffed whole` + `full_reload_only` classifications (L5 plan step 3/4) ---
//
// A richer invocation alongside the keyed-only one above: a keyed field
// (exercising ordering against the whole field's event), a `diffed whole`
// field, and a `full_reload_only` field that must NOT appear on the
// generated delta at all.

gui_state! {
    state=MiniState2, delta=MiniDelta2, diff_fn=diff_mini2, events_fn=mini_events2;
    diffed keyed(key=id, item="item", update="item-update", remove="item-removed", changed=changed_items, removed=removed_item_ids)
    items: Vec<MiniItem>,
    diffed whole(event="note-update", item_type="note-update", item_id="notes", changed=changed_notes)
    notes: Vec<String>,
    full_reload_only("test-only channel")
    blob: Option<String>,
}

#[test]
fn diff_whole_field_is_some_new_when_changed_and_none_when_equal() {
    let old = MiniState2 {
        items: vec![],
        notes: vec!["a".to_string()],
        blob: None,
    };
    let changed = MiniState2 {
        items: vec![],
        notes: vec!["a".to_string(), "b".to_string()],
        blob: None,
    };
    let unchanged = MiniState2 {
        items: vec![],
        notes: vec!["a".to_string()],
        blob: None,
    };

    assert_eq!(
        diff_mini2(&old, &changed).changed_notes,
        Some(vec!["a".to_string(), "b".to_string()])
    );
    assert_eq!(diff_mini2(&old, &unchanged).changed_notes, None);
}

#[test]
fn full_marks_whole_field_some_when_non_empty_and_none_when_empty() {
    let populated = MiniState2 {
        items: vec![],
        notes: vec!["a".to_string()],
        blob: None,
    };
    let empty = MiniState2 {
        items: vec![],
        notes: vec![],
        blob: None,
    };

    assert_eq!(
        MiniDelta2::full(&populated).changed_notes,
        Some(vec!["a".to_string()])
    );
    assert_eq!(MiniDelta2::full(&empty).changed_notes, None);
}

#[test]
fn events_emit_one_whole_update_after_all_keyed_update_and_removed_events() {
    let old = MiniState2 {
        items: vec![item("a", 1), item("b", 2)],
        notes: vec!["x".to_string()],
        blob: None,
    };
    let new = MiniState2 {
        items: vec![item("a", 99)],
        notes: vec!["x".to_string(), "y".to_string()],
        blob: None,
    };

    let delta = diff_mini2(&old, &new);
    let events = mini_events2(&delta);

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
            (
                "note-update".to_string(),
                serde_json::to_value(vec!["x".to_string(), "y".to_string()]).unwrap(),
            ),
        ]
    );
}

#[test]
fn full_reload_only_field_generates_no_delta_field_and_no_event() {
    // If the macro generated a `blob` field on MiniDelta2, this literal
    // (naming only the keyed + whole fields) would fail to compile with a
    // "missing field"/"no field `blob`" error — the absence of that error
    // IS the assertion that `full_reload_only` contributes nothing to the
    // delta.
    let delta = MiniDelta2 {
        changed_items: vec![],
        removed_item_ids: vec![],
        changed_notes: None,
    };
    let events = mini_events2(&delta);
    assert!(events.is_empty());

    // `blob` is still a plain, ordinarily-readable/writable state field —
    // only the delta/diff/events side is excluded.
    let state = MiniState2 {
        items: vec![],
        notes: vec![],
        blob: Some("test-only channel value".to_string()),
    };
    assert_eq!(state.blob, Some("test-only channel value".to_string()));
}
