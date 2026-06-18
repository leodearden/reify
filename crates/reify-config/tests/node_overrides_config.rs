//! Integration tests for the `[[node_overrides]]` array-of-tables in `reify.toml`.
//!
//! Covers the public contract for task 3464 (GR-007 config ingestion):
//! - Empty manifest and manifest with no `[[node_overrides]]` → empty slice.
//! - Two `[[node_overrides]]` entries (kind selector + instance selector) round-trip.
//! - Empty / whitespace-only `node_id_pattern` → typed `ManifestError::EmptyNodeOverridePattern`.
//! - Unknown `commitment_policy` value → `ManifestError::Parse(_)` (serde rejects it).
//! - Unknown key inside `[[node_overrides]]` → `ManifestError::Parse(_)` (deny_unknown_fields).

use reify_config::{Manifest, ManifestError, NodeCommitmentPolicy, NodePolicyOverride};

// --- happy-path round-trip ---

#[test]
fn empty_manifest_has_no_node_overrides() {
    let manifest = Manifest::from_toml_str("").expect("empty manifest must parse");
    assert!(
        manifest.node_overrides().is_empty(),
        "empty manifest must have no node_overrides"
    );
}

#[test]
fn manifest_without_node_overrides_section_has_empty_slice() {
    let manifest = Manifest::from_toml_str("[kernels]\nocct = \"7.7.0\"\n")
        .expect("manifest without [[node_overrides]] must parse");
    assert!(
        manifest.node_overrides().is_empty(),
        "manifest without [[node_overrides]] must have no node_overrides"
    );
}

#[test]
fn two_node_overrides_entries_round_trip() {
    let toml = "\
[[node_overrides]]
node_id_pattern = \"value\"
commitment_policy = \"always_cancel_when_stale\"

[[node_overrides]]
node_id_pattern = \"Bracket.width\"
commitment_policy = \"only_run_on_final_inputs\"
";
    let manifest = Manifest::from_toml_str(toml).expect("two [[node_overrides]] must parse");
    let entries: &[NodePolicyOverride] = manifest.node_overrides();
    assert_eq!(entries.len(), 2, "must have exactly two node_overrides entries");

    assert_eq!(entries[0].node_id_pattern, "value");
    assert_eq!(
        entries[0].commitment_policy,
        NodeCommitmentPolicy::AlwaysCancelWhenStale
    );

    assert_eq!(entries[1].node_id_pattern, "Bracket.width");
    assert_eq!(
        entries[1].commitment_policy,
        NodeCommitmentPolicy::OnlyRunOnFinalInputs
    );
}

#[test]
fn commit_if_slow_policy_round_trips() {
    let toml = "\
[[node_overrides]]
node_id_pattern = \"compute\"
commitment_policy = \"commit_if_slow\"
";
    let manifest = Manifest::from_toml_str(toml).expect("commit_if_slow must parse");
    let entries = manifest.node_overrides();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].commitment_policy, NodeCommitmentPolicy::CommitIfSlow);
}

// --- validation / strict-schema cases (appended in step-3 after step-2 GREEN) ---
