// INV-GUI-1 interim enforcement (warn-mode): every `GuiState` field must be
// either classified with a sync mechanism or named on the known-stale
// allowlist. See docs/prds/v0_6/gui-state-sync.md §8 L1.
//
// This lint is interim scaffolding — L5 (per the PRD) retires it wholesale
// once the derive-based mechanism lands. Everything it needs (checker fn,
// classification table, allowlist, fixture, tests) lives in this one file so
// deletion is a single `rm` + removing its `mod` line from tests/mod.rs.
//
// Tests are progressively added across steps 1–10 of the plan. This file is
// intentionally sparse at pre-1 — content grows with each step.

use std::collections::BTreeMap;

/// How a `GuiState` field's value reaches the frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncMechanism {
    /// Carried on the per-field `StateDelta` diff channel (diff.rs).
    Diffed,
    /// Pushed via a dedicated Tauri event; payload names the event(s).
    Emitter(&'static str),
    /// Not live on a param edit; only visible via a full state reload or
    /// out-of-band observability channel. Payload documents how it's read.
    FullReloadOnly(&'static str),
}

/// Checks that every reflected `GuiState` field key is either classified in
/// `table` or named on the known-stale `allowlist`. Returns the sorted list
/// of offending keys (present in neither) as `Err`, or `Ok(())` if none.
fn check_field_coverage(
    keys: &[String],
    table: &BTreeMap<&'static str, SyncMechanism>,
    allowlist: &BTreeMap<&'static str, &'static str>,
) -> Result<(), Vec<String>> {
    let mut offending: Vec<String> = keys
        .iter()
        .filter(|key| !table.contains_key(key.as_str()) && !allowlist.contains_key(key.as_str()))
        .cloned()
        .collect();
    offending.sort();
    if offending.is_empty() {
        Ok(())
    } else {
        Err(offending)
    }
}

#[test]
fn check_field_coverage_rejects_unknown_key() {
    let mut table: BTreeMap<&'static str, SyncMechanism> = BTreeMap::new();
    table.insert("meshes", SyncMechanism::Diffed);
    let allowlist: BTreeMap<&'static str, &'static str> = BTreeMap::new();

    let keys = vec!["meshes".to_string(), "ghost_field".to_string()];
    let result = check_field_coverage(&keys, &table, &allowlist);

    let err = result.expect_err("ghost_field is neither classified nor allowlisted");
    assert!(
        err.contains(&"ghost_field".to_string()),
        "expected offending keys {err:?} to contain 'ghost_field'"
    );
    assert!(
        !err.contains(&"meshes".to_string()),
        "classified key 'meshes' must not be reported as offending: {err:?}"
    );
}

#[test]
fn check_field_coverage_honors_allowlist() {
    let mut table: BTreeMap<&'static str, SyncMechanism> = BTreeMap::new();
    table.insert("meshes", SyncMechanism::Diffed);
    let mut allowlist: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    allowlist.insert("tensegrity_wires", "L2 stopgap");

    let keys = vec!["meshes".to_string(), "tensegrity_wires".to_string()];
    assert_eq!(
        check_field_coverage(&keys, &table, &allowlist),
        Ok(()),
        "a key present on the allowlist must be accepted"
    );

    let keys_with_ghost = vec![
        "meshes".to_string(),
        "tensegrity_wires".to_string(),
        "ghost".to_string(),
    ];
    let err = check_field_coverage(&keys_with_ghost, &table, &allowlist)
        .expect_err("'ghost' is in neither the table nor the allowlist");
    assert_eq!(err, vec!["ghost".to_string()]);
}
