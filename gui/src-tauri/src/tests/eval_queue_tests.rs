//! Tests for [`crate::eval_queue`].

use crate::eval_queue::{EditIdentity, EditLedger, EditOrder};

// ── Edit identity: which queued edit a newer one makes redundant ─────────────

const EPOCH: u64 = 7;

fn at(seq: u64) -> EditOrder {
    EditOrder { epoch: EPOCH, seq }
}

fn preview(cell: &str, seq: u64) -> EditIdentity {
    EditIdentity::preview(cell, at(seq))
}

fn commit(cell: &str, seq: u64) -> EditIdentity {
    EditIdentity::commit(cell, at(seq))
}

fn editor(path: &str, seq: u64) -> EditIdentity {
    EditIdentity::editor_source(path, at(seq))
}

fn disk(path: &str) -> EditIdentity {
    EditIdentity::disk_source(path)
}

/// The wire shape `bridge.ts` sends as a command argument.
#[test]
fn an_edit_order_deserializes_from_the_frontend_stamp() {
    let order: EditOrder = serde_json::from_value(serde_json::json!({
        "epoch": 9_007_199_254_740_991_u64,
        "seq": 3,
    }))
    .expect("an {epoch, seq} object must deserialize");
    assert_eq!(
        order,
        EditOrder {
            epoch: 9_007_199_254_740_991,
            seq: 3
        }
    );
}

#[test]
fn a_newer_preview_supersedes_an_older_one_of_its_cell_but_not_the_reverse() {
    assert!(preview("A", 2).supersedes(&preview("A", 1)));
    assert!(!preview("A", 1).supersedes(&preview("A", 2)));
}

#[test]
fn edits_of_one_cell_never_supersede_edits_of_another() {
    for newer in [preview("A", 9), commit("A", 9)] {
        for older in [preview("B", 1), commit("B", 1)] {
            assert!(
                !newer.supersedes(&older),
                "{newer:?} must not supersede {older:?}"
            );
        }
    }
}

#[test]
fn a_commit_supersedes_older_previews_and_older_commits_of_its_cell() {
    assert!(commit("A", 5).supersedes(&preview("A", 4)));
    assert!(commit("A", 5).supersedes(&commit("A", 4)));
}

/// INV-GUI-3: a transient frame must never drop a durable write.
#[test]
fn a_preview_never_supersedes_a_commit_however_new() {
    assert!(!preview("A", 100).supersedes(&commit("A", 1)));
}

#[test]
fn editor_sources_coalesce_per_path_and_never_with_disk_sources() {
    assert!(editor("p.ri", 2).supersedes(&editor("p.ri", 1)));
    assert!(!editor("p.ri", 2).supersedes(&editor("q.ri", 1)));
    assert!(!editor("p.ri", 2).supersedes(&disk("p.ri")));
    assert!(!disk("p.ri").supersedes(&editor("p.ri", 1)));
}

/// A disk reload carries no order: the reload reads the file when it runs, so
/// whichever arrives later is the one worth running.
#[test]
fn a_disk_reload_supersedes_a_queued_reload_of_the_same_file() {
    assert!(disk("p.ri").supersedes(&disk("p.ri")));
    assert!(!disk("p.ri").supersedes(&disk("q.ri")));
}

/// A page reload restarts `seq` under a new epoch, and epochs are compared only
/// for equality, so across epochs the arriving edit wins in either direction.
#[test]
fn across_epochs_the_arriving_edit_wins() {
    let reloaded = |epoch, seq| EditIdentity::preview("A", EditOrder { epoch, seq });
    assert!(reloaded(EPOCH + 1, 1).supersedes(&preview("A", 50)));
    assert!(reloaded(EPOCH - 1, 1).supersedes(&preview("A", 50)));
}

#[test]
fn the_ledger_refuses_a_preview_older_than_one_already_admitted() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&preview("A", 5)));
    assert!(!ledger.admit(&preview("A", 4)), "a late preview must be refused");
    assert!(ledger.admit(&preview("A", 6)));
}

#[test]
fn an_admitted_commit_refuses_older_previews_and_older_commits() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&commit("A", 5)));
    assert!(!ledger.admit(&preview("A", 4)));
    assert!(!ledger.admit(&commit("A", 4)));
}

/// INV-GUI-3 again: a late durable write is never dropped for a newer preview.
#[test]
fn a_commit_is_admitted_even_behind_a_newer_preview() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&preview("A", 5)));
    assert!(ledger.admit(&commit("A", 4)));
}

#[test]
fn the_ledger_admits_other_epochs_other_cells_and_every_disk_reload() {
    let mut ledger = EditLedger::default();
    assert!(ledger.admit(&commit("A", 5)));
    let other_epoch = EditOrder {
        epoch: EPOCH + 1,
        seq: 1,
    };
    assert!(ledger.admit(&EditIdentity::preview("A", other_epoch)));
    assert!(ledger.admit(&EditIdentity::commit("A", other_epoch)));
    assert!(ledger.admit(&preview("B", 1)));
    assert!(ledger.admit(&disk("p.ri")));
    assert!(ledger.admit(&disk("p.ri")));
}
