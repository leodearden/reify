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

use crate::types::GuiState;

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

/// The acceptance harness (forward direction): every field reflected off a
/// fully-populated `GuiState` must be either classified in
/// `classification_table()` or named on `known_stale_allowlist()`. This is
/// the check that fails the build when a new, unclassified field is added
/// to `GuiState` without updating this lint.
#[test]
fn every_gui_state_field_is_classified_or_allowlisted() {
    let state = fully_populated_gui_state();
    let keys: Vec<String> = serde_json::to_value(&state)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();

    assert_eq!(
        check_field_coverage(&keys, &classification_table(), &known_stale_allowlist()),
        Ok(()),
        "every GuiState field must be classified or on the known-stale allowlist"
    );
}

/// Classifies each `GuiState` field with how its value reaches the frontend
/// today (PRD §1 coverage matrix). The `Diffed` set mirrors `StateDelta`
/// (diff.rs); the `Emitter` payloads name the real Tauri event(s) (main.rs);
/// `demand_prune_measurement` is observability-only (task 4532/4741).
fn classification_table() -> BTreeMap<&'static str, SyncMechanism> {
    let mut table = BTreeMap::new();
    table.insert("meshes", SyncMechanism::Diffed);
    table.insert("values", SyncMechanism::Diffed);
    table.insert("constraints", SyncMechanism::Diffed);
    table.insert("tessellation_diagnostics", SyncMechanism::Diffed);
    table.insert("compile_diagnostics", SyncMechanism::Diffed);
    table.insert(
        "files",
        SyncMechanism::Emitter("file-changed/file-removed"),
    );
    table.insert(
        "fea_diagnostics",
        SyncMechanism::Emitter("fea-diagnostics-changed"),
    );
    table.insert(
        "demand_prune_measurement",
        SyncMechanism::FullReloadOnly(
            "observability-only; read via reify-debug MCP engine_state_json \
             (commands.rs:262), no UI reader — task 4741",
        ),
    );
    table
}

/// Fields known to be stale (not wired to any live sync mechanism today),
/// each mapped to a non-empty reference naming the task that clears it —
/// L2 clears the four tensegrity/display fields, L3 clears `fea_convergence`.
fn known_stale_allowlist() -> BTreeMap<&'static str, &'static str> {
    let mut allowlist = BTreeMap::new();
    allowlist.insert("tensegrity_wires", "cleared by L2");
    allowlist.insert("tensegrity_surfaces", "cleared by L2");
    allowlist.insert("display_panes", "cleared by L2");
    allowlist.insert("display_appearance", "cleared by L2");
    allowlist.insert("fea_convergence", "cleared by L3");
    allowlist
}

/// A `GuiState` fixture with every field populated so all 13 serde keys
/// reflect — including `fea_convergence`, which carries
/// `#[serde(skip_serializing_if = "Option::is_none")]` (types.rs) and so
/// would otherwise vanish from the reflected set (the gotcha caught by
/// `fixture_reflects_every_classified_and_allowlisted_field`).
fn fully_populated_gui_state() -> GuiState {
    use crate::types::{DemandPruneMeasurementDto, FeaConvergenceInfo, WouldPruneByKindDto};

    GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: Some(DemandPruneMeasurementDto {
            eval_set_size: 0,
            observed_retained: 0,
            would_prune: WouldPruneByKindDto {
                value: 0,
                constraint: 0,
                realization: 0,
                resolution: 0,
                compute: 0,
            },
        }),
        display_panes: vec![],
        display_appearance: vec![],
        fea_diagnostics: vec![],
        fea_convergence: Some(FeaConvergenceInfo {
            converged: true,
            reason: None,
        }),
    }
}

/// Reverse coverage: every key named in `classification_table()` or
/// `known_stale_allowlist()` must actually appear in the reflected key set.
/// This catches (a) a field silently dropped from the reflected set by
/// `skip_serializing_if` while still sitting in the table/allowlist, and
/// (b) a stale table/allowlist entry for a field that no longer exists —
/// neither of which the forward check (`every_gui_state_field_is_classified_or_allowlisted`)
/// can see, since a missing key never trips it.
#[test]
fn fixture_reflects_every_classified_and_allowlisted_field() {
    use std::collections::BTreeSet;

    let state = fully_populated_gui_state();
    let reflected: BTreeSet<String> = serde_json::to_value(&state)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();

    let table = classification_table();
    let allowlist = known_stale_allowlist();
    let expected: BTreeSet<String> = table
        .keys()
        .chain(allowlist.keys())
        .map(|k| k.to_string())
        .collect();

    assert_eq!(
        reflected, expected,
        "the fixture must reflect exactly the fields named in the classification table and allowlist"
    );

    for (field, reference) in &allowlist {
        assert!(
            !reference.is_empty(),
            "allowlist entry for '{field}' must name a non-empty clearing-task reference"
        );
    }
}
