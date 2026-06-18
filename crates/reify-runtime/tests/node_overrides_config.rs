//! Integration tests for `NodePolicyOverrides::from_config_overrides` (GR-007 G4 boundary).
//!
//! Pins the full Manifest→from_config_overrides→SchedulerConfig.resolve pipeline:
//! - Kind selector (`"value"`) → type override applied to all Value nodes.
//! - Instance selector (`"Entity.member"`) → instance override for that specific node.
//! - Unresolvable selector and malformed dotted selector → `NodeOverrideConfigError`.
//! - G2(b) distinguishability: override-config vs default-config produce different resolve results.

use reify_config::{Manifest, NodeCommitmentPolicy, NodePolicyOverride};
use reify_core::ValueCellId;
use reify_eval::cache::NodeId;
use reify_runtime::commitment::{NodeCommitmentOverride, NodeOverrideConfigError, NodePolicyOverrides};
use reify_runtime::concurrent::SchedulerConfig;

// --- G4 boundary + G2(b) distinguishability ---

#[test]
fn kind_selector_value_overrides_all_value_nodes() {
    let toml = "\
[[node_overrides]]
node_id_pattern = \"value\"
commitment_policy = \"always_cancel_when_stale\"
";
    let manifest = Manifest::from_toml_str(toml).expect("manifest must parse");
    let overrides = NodePolicyOverrides::from_config_overrides(manifest.node_overrides())
        .expect("from_config_overrides must succeed for kind selector");

    let config = SchedulerConfig {
        node_overrides: overrides,
        ..Default::default()
    };

    // Value node → overridden to AlwaysCancelWhenStale
    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        config.node_overrides.resolve(&value_node),
        NodeCommitmentOverride::AlwaysCancelWhenStale,
        "Value kind selector must override all Value nodes"
    );

    // Constraint node → not overridden → default CommitIfSlow (kind isolation)
    let constraint_node = NodeId::Constraint(reify_core::ConstraintNodeId::new("Bracket", 0));
    assert_eq!(
        config.node_overrides.resolve(&constraint_node),
        NodeCommitmentOverride::CommitIfSlow,
        "kind selector for Value must not affect Constraint nodes"
    );
}

#[test]
fn g2b_default_config_resolves_to_commit_if_slow() {
    // G2(b) distinguishability: default SchedulerConfig resolves the same node to CommitIfSlow.
    let default_config = SchedulerConfig::default();
    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        default_config.node_overrides.resolve(&value_node),
        NodeCommitmentOverride::CommitIfSlow,
        "default SchedulerConfig must resolve to CommitIfSlow (no overrides)"
    );
}

// --- instance selector ---

#[test]
fn instance_selector_overrides_exact_node_and_isolates_siblings() {
    let entry = NodePolicyOverride {
        node_id_pattern: "Bracket.width".into(),
        commitment_policy: NodeCommitmentPolicy::OnlyRunOnFinalInputs,
    };
    let overrides = NodePolicyOverrides::from_config_overrides(&[entry])
        .expect("instance selector must succeed");

    // The targeted node is overridden.
    let width = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        overrides.resolve(&width),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "instance selector must override the named node"
    );

    // A sibling node resolves to the default (instance isolation).
    let height = NodeId::Value(ValueCellId::new("Bracket", "height"));
    assert_eq!(
        overrides.resolve(&height),
        NodeCommitmentOverride::CommitIfSlow,
        "instance selector must not affect sibling nodes"
    );
}

// --- unresolvable selector errors ---

#[test]
fn bare_word_selector_returns_unresolvable_error() {
    let entry = NodePolicyOverride {
        node_id_pattern: "widget".into(),
        commitment_policy: NodeCommitmentPolicy::CommitIfSlow,
    };
    let err = NodePolicyOverrides::from_config_overrides(&[entry])
        .expect_err("bare non-kind word must be rejected");
    match err {
        NodeOverrideConfigError::UnresolvableSelector(pat) => {
            assert_eq!(pat, "widget");
        }
    }
}

#[test]
fn trailing_dot_selector_returns_unresolvable_error() {
    let entry = NodePolicyOverride {
        node_id_pattern: "Bracket.".into(),
        commitment_policy: NodeCommitmentPolicy::CommitIfSlow,
    };
    let err = NodePolicyOverrides::from_config_overrides(&[entry])
        .expect_err("trailing-dot selector must be rejected");
    match err {
        NodeOverrideConfigError::UnresolvableSelector(pat) => {
            assert_eq!(pat, "Bracket.");
        }
    }
}

#[test]
fn leading_dot_selector_returns_unresolvable_error() {
    let entry = NodePolicyOverride {
        node_id_pattern: ".width".into(),
        commitment_policy: NodeCommitmentPolicy::CommitIfSlow,
    };
    let err = NodePolicyOverrides::from_config_overrides(&[entry])
        .expect_err("leading-dot selector must be rejected");
    match err {
        NodeOverrideConfigError::UnresolvableSelector(pat) => {
            assert_eq!(pat, ".width");
        }
    }
}

#[test]
fn multi_dot_selector_returns_unresolvable_error() {
    // "a.b.c" must be rejected — it is not a valid single-dot Entity.member selector.
    // This pins the contract documented in the UnresolvableSelector doc comment.
    let entry = NodePolicyOverride {
        node_id_pattern: "a.b.c".into(),
        commitment_policy: NodeCommitmentPolicy::CommitIfSlow,
    };
    let err = NodePolicyOverrides::from_config_overrides(&[entry])
        .expect_err("multi-dot selector must be rejected");
    match err {
        NodeOverrideConfigError::UnresolvableSelector(pat) => {
            assert_eq!(pat, "a.b.c");
        }
    }
}

#[test]
fn duplicate_kind_selector_last_entry_wins() {
    // Two entries for the same kind: the second one overrides the first (last-write-wins).
    let entries = vec![
        NodePolicyOverride {
            node_id_pattern: "value".into(),
            commitment_policy: NodeCommitmentPolicy::AlwaysCancelWhenStale,
        },
        NodePolicyOverride {
            node_id_pattern: "value".into(),
            commitment_policy: NodeCommitmentPolicy::OnlyRunOnFinalInputs,
        },
    ];
    let overrides =
        NodePolicyOverrides::from_config_overrides(&entries).expect("duplicate entries must succeed");

    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        overrides.resolve(&value_node),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "last duplicate kind selector must win"
    );
}

#[test]
fn duplicate_instance_selector_last_entry_wins() {
    // Two entries for the same instance: the second one overrides the first (last-write-wins).
    let entries = vec![
        NodePolicyOverride {
            node_id_pattern: "Bracket.width".into(),
            commitment_policy: NodeCommitmentPolicy::AlwaysCancelWhenStale,
        },
        NodePolicyOverride {
            node_id_pattern: "Bracket.width".into(),
            commitment_policy: NodeCommitmentPolicy::OnlyRunOnFinalInputs,
        },
    ];
    let overrides =
        NodePolicyOverrides::from_config_overrides(&entries).expect("duplicate instance entries must succeed");

    let width = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        overrides.resolve(&width),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "last duplicate instance selector must win"
    );
}
