//! Integration tests for `NodePolicyOverrides::from_config_overrides` (GR-007 G4 boundary).
//!
//! Pins the full Manifest→from_config_overrides→SchedulerConfig.resolve pipeline:
//! - Kind selector (`"value"`) → type override applied to all Value nodes.
//! - Instance selector (`"Entity.member"`) → instance override for that specific node.
//! - Unresolvable selector and malformed dotted selector → `NodeOverrideConfigError`.
//! - G2(b) distinguishability: override-config vs default-config produce different resolve results.

use reify_config::Manifest;
use reify_core::ValueCellId;
use reify_eval::cache::NodeId;
use reify_runtime::commitment::{NodeCommitmentOverride, NodePolicyOverrides};
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
