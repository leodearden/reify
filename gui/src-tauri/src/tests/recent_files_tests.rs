use std::path::{Path, PathBuf};

use crate::recent_files::{RECENT_FILES_LIMIT, RecentEntry, list_recent, record_recent};

/// A real `.ri` file named `name` inside `dir`, returned with its canonical path.
fn touch(dir: &Path, name: &str) -> (PathBuf, String) {
    let path = dir.join(name);
    std::fs::write(&path, "// recent\n").unwrap();
    let canonical = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    (path, canonical)
}

fn entry(path: &str, opened_at: u64) -> RecentEntry {
    RecentEntry {
        path: path.to_string(),
        opened_at,
    }
}

#[test]
fn a_missing_store_lists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(list_recent(&dir.path().join("recent-files.json")), vec![]);
}

#[test]
fn a_recorded_entry_round_trips_through_disk_under_its_canonical_path() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("recent-files.json");
    let (a, a_key) = touch(dir.path(), "a.ri");

    let recorded = record_recent(&store, a.to_str().unwrap(), 1000).expect("record succeeds");

    assert_eq!(recorded, vec![entry(&a_key, 1000)]);
    assert_eq!(list_recent(&store), recorded);
}

#[test]
fn re_recording_a_path_moves_it_to_the_front_without_duplicating_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("recent-files.json");
    let (a, a_key) = touch(dir.path(), "a.ri");
    let (b, b_key) = touch(dir.path(), "b.ri");

    record_recent(&store, a.to_str().unwrap(), 1000).unwrap();
    record_recent(&store, b.to_str().unwrap(), 2000).unwrap();
    let recorded = record_recent(&store, a.to_str().unwrap(), 3000).unwrap();

    assert_eq!(recorded, vec![entry(&a_key, 3000), entry(&b_key, 2000)]);
    assert_eq!(list_recent(&store), recorded);
}

#[test]
fn only_the_newest_entries_up_to_the_limit_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("recent-files.json");
    let total = RECENT_FILES_LIMIT + 2;
    let keys: Vec<String> = (0..total)
        .map(|i| {
            let (path, key) = touch(dir.path(), &format!("f{i}.ri"));
            record_recent(&store, path.to_str().unwrap(), i as u64).unwrap();
            key
        })
        .collect();

    let expected: Vec<RecentEntry> = (0..total)
        .rev()
        .take(RECENT_FILES_LIMIT)
        .map(|i| entry(&keys[i], i as u64))
        .collect();
    assert_eq!(list_recent(&store), expected);
}

#[test]
fn recording_creates_the_store_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir
        .path()
        .join("app-data")
        .join("nested")
        .join("recent-files.json");
    let (a, a_key) = touch(dir.path(), "a.ri");

    record_recent(&store, a.to_str().unwrap(), 7).expect("record creates the parent dir");

    assert_eq!(list_recent(&store), vec![entry(&a_key, 7)]);
}

#[test]
fn a_corrupt_store_lists_nothing_and_is_replaced_by_the_next_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("recent-files.json");
    std::fs::write(&store, b"\x00not json{").unwrap();
    let (a, a_key) = touch(dir.path(), "a.ri");

    assert_eq!(list_recent(&store), vec![]);
    record_recent(&store, a.to_str().unwrap(), 5).expect("record replaces a corrupt store");

    assert_eq!(list_recent(&store), vec![entry(&a_key, 5)]);
}

#[test]
fn the_wire_shape_is_path_and_opened_at() {
    assert_eq!(
        serde_json::to_value(entry("/d/a.ri", 42)).unwrap(),
        serde_json::json!({ "path": "/d/a.ri", "opened_at": 42 })
    );
}
