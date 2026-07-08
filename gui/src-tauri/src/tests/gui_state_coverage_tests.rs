// INV-GUI-1 interim enforcement: every `GuiState` field must be either
// classified with a sync mechanism or named on the known-stale allowlist.
// See docs/prds/v0_6/gui-state-sync.md §8 L1.
//
// "Warn-mode" (task title; docs/invariants.md's fail-closed rollout: contract
// spec -> warn-mode corpus sweep -> fix bulk producers -> flip to enforce)
// names the ALLOWLIST's tolerance of pre-existing debt, not the check's
// severity: gui-state-sync.md §2 is explicit that "a field that is neither
// classified nor on an explicit shrinking known-stale allowlist -> the lint
// fails". Every test below that calls `check_field_coverage` hard-fails
// `cargo test` (assert_eq!/expect) the moment a field is neither classified
// nor allowlisted — promotion to `enforce` status happens at L5, when the
// derive makes an unclassified field a compile error instead.
//
// This lint is interim scaffolding — L5 (per the PRD) retires it wholesale
// once the derive-based mechanism lands. Everything it needs (checker fn,
// classification table, allowlist, fixture, tests) lives in this one file so
// deletion is a single `rm` + removing its `mod` line from tests/mod.rs.
//
// Tests are progressively added across steps 1–10 of the plan. This file is
// intentionally sparse at pre-1 — content grows with each step.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::diff::StateDelta;
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

/// Reflects the serde-JSON object keys of any `Serialize` value. The single
/// definition site for the reflection idiom, shared by `reflected_keys`
/// (`GuiState`) and the `StateDelta` cross-check below — so both stay in
/// sync if the reflection approach ever changes.
fn serde_keys<T: Serialize>(value: &T) -> BTreeSet<String> {
    serde_json::to_value(value)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

/// Reflects the serde-JSON object keys of a `GuiState` value — the field
/// names that actually appear on the wire. For a field carrying
/// `#[serde(skip_serializing_if = ...)]` (e.g. `fea_convergence`), this
/// excludes the key entirely when the field is left at its skipped value.
/// Shared by the forward and reverse coverage checks below.
fn reflected_keys(state: &GuiState) -> BTreeSet<String> {
    serde_keys(state)
}

/// The acceptance harness (forward direction): every field reflected off a
/// fully-populated `GuiState` must be either classified in
/// `classification_table()` or named on `known_stale_allowlist()`. This is
/// the check that fails the build when a new, unclassified field is added
/// to `GuiState` without updating this lint.
#[test]
fn every_gui_state_field_is_classified_or_allowlisted() {
    let state = fully_populated_gui_state();
    let keys: Vec<String> = reflected_keys(&state).into_iter().collect();

    assert_eq!(
        check_field_coverage(&keys, &classification_table(), &known_stale_allowlist()),
        Ok(()),
        "every GuiState field must be classified or on the known-stale allowlist"
    );
}

/// A hardcoded tripwire on `GuiState`'s total field count, independent of
/// `classification_table()`/`known_stale_allowlist()`. Adding (or removing) a
/// field to/from `GuiState` changes how many keys `reflected_keys` produces
/// for a fully-populated fixture, so this goes red and forces a deliberate
/// bump here — plus a look at whether the new field needs classifying —
/// rather than silently drifting.
///
/// This does NOT, on its own, catch a field that both (a) carries
/// `#[serde(skip_serializing_if = ...)]` and (b) is left at its skipped
/// value in `fully_populated_gui_state()`: such a field never reflects, so
/// the count would not move. That residual gap is why `fully_populated_gui_state`'s
/// doc comment requires every `Option`/`skip_serializing_if` field to be set
/// to `Some(...)`.
const EXPECTED_GUI_STATE_FIELD_COUNT: usize = 13;

#[test]
fn fully_populated_fixture_has_expected_field_count() {
    let reflected = reflected_keys(&fully_populated_gui_state());
    assert_eq!(
        reflected.len(),
        EXPECTED_GUI_STATE_FIELD_COUNT,
        "GuiState's reflected field count changed (now {reflected:?}) — update \
         EXPECTED_GUI_STATE_FIELD_COUNT and classify the new/removed field in \
         classification_table() or known_stale_allowlist()"
    );
}

/// Cross-checks the `Diffed` classification against `StateDelta`'s real
/// field set (diff.rs) instead of trusting the classification table's
/// comment. Each `Diffed` `GuiState` field is expected to have a
/// `changed_<field>` counterpart on `StateDelta`, per the naming convention
/// both `StateDelta::full` and `diff_gui_state` follow — so a `Diffed` entry
/// can't silently drift out of sync with the actual diff channel.
#[test]
fn diffed_classification_matches_state_delta_fields() {
    let delta = StateDelta::full(&fully_populated_gui_state());
    let delta_keys = serde_keys(&delta);

    for (field, mechanism) in classification_table() {
        if mechanism == SyncMechanism::Diffed {
            let expected_key = format!("changed_{field}");
            assert!(
                delta_keys.contains(&expected_key),
                "GuiState field '{field}' is classified Diffed but StateDelta has no \
                 '{expected_key}' field — diff.rs may have drifted from this classification"
            );
        }
    }
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
    table.insert("tensegrity_wires", SyncMechanism::Diffed);
    table.insert("tensegrity_surfaces", SyncMechanism::Diffed);
    table.insert("display_panes", SyncMechanism::Diffed);
    table.insert("display_appearance", SyncMechanism::Diffed);
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

/// True if `reference` cites a task in this repo's canonical `#NNNN` form
/// (CLAUDE.md's TODO-citation convention: a bare `#` followed by at least
/// one ASCII digit) — e.g. `"cleared by L2 (#5031)"`. A PRD-relative label
/// like `"cleared by L2"` alone does not count: CLAUDE.md calls out
/// PRD-relative indices as a `malformed-cite` shape.
fn cites_task(reference: &str) -> bool {
    reference
        .find('#')
        .is_some_and(|i| reference[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
}

/// Validates a clearing-task reference cites a live task in the canonical
/// `#NNNN` form at construction, so the known-stale ledger's "every entry
/// names its clearing task" guarantee is structural (enforced wherever the
/// reference is built, and re-checked by every test that constructs
/// `known_stale_allowlist()`) rather than merely a non-empty string.
fn clearing_task_reference(reference: &'static str) -> &'static str {
    assert!(
        cites_task(reference),
        "clearing-task reference '{reference}' must cite a live task in the \
         canonical #NNNN form (CLAUDE.md's TODO-citation convention)"
    );
    reference
}

/// Fields known to be stale (not wired to any live sync mechanism today),
/// each mapped to a reference citing the live task that clears it — L2
/// (#5031, "stopgap: four list fields -> StateDelta") cleared the four
/// tensegrity/display fields by classifying them `Diffed` above; L3 (#5032,
/// "stopgap: fea-convergence-changed emitter") clears `fea_convergence`.
/// `clearing_task_reference` enforces the `#NNNN` citation structurally, not
/// just non-emptiness.
fn known_stale_allowlist() -> BTreeMap<&'static str, &'static str> {
    let mut allowlist = BTreeMap::new();
    allowlist.insert(
        "fea_convergence",
        clearing_task_reference("cleared by L3 (#5032)"),
    );
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
    let state = fully_populated_gui_state();
    let reflected = reflected_keys(&state);

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
}

/// The warn-mode debt ledger: `currently_unwired_fields` must report exactly
/// the two fields not live on a param edit today (PRD §1) — the one
/// allowlisted stale field (`fea_convergence`) plus the one `FullReloadOnly`
/// field (`demand_prune_measurement`). L2 (#5031) shrank this from six to two
/// by classifying the four tensegrity/display fields `Diffed`.
#[test]
fn warn_mode_report_lists_the_two_remaining_unwired_fields() {
    let fields = currently_unwired_fields(&classification_table(), &known_stale_allowlist());
    assert_eq!(
        fields,
        vec![
            "demand_prune_measurement".to_string(),
            "fea_convergence".to_string(),
        ]
    );
}

/// The sorted warn-mode debt ledger: every `allowlist` field plus every
/// `table` field classified `FullReloadOnly` — i.e. every field not live on
/// a param edit today. L2/L3 shrink this list as they clear allowlist
/// entries and wire live sync mechanisms.
///
/// Collected through a `BTreeSet` so a field that is (transiently) both
/// allowlisted AND classified `FullReloadOnly` is reported once, not twice.
fn currently_unwired_fields(
    table: &BTreeMap<&'static str, SyncMechanism>,
    allowlist: &BTreeMap<&'static str, &'static str>,
) -> Vec<String> {
    let mut fields: BTreeSet<String> = allowlist.keys().map(|k| k.to_string()).collect();
    fields.extend(
        table
            .iter()
            .filter(|(_, mechanism)| matches!(mechanism, SyncMechanism::FullReloadOnly(_)))
            .map(|(k, _)| k.to_string()),
    );
    fields.into_iter().collect()
}
