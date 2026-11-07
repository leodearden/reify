//! Integration tests for `NodePolicyOverrides::from_config_overrides` (GR-007 G4 boundary).
//!
//! Pins the full Manifest→from_config_overrides→NodePolicyOverrides::resolve_with_traits pipeline:
//! - Kind selector (`"value"`) → type override applied to all Value nodes.
//! - Instance selector (`"Entity.member"`) → instance override for that specific node.
//! - Unresolvable selector and malformed dotted selector → `NodeOverrideConfigError`.
//! - G2(b) distinguishability: override-config vs default-config produce different resolve results.

use reify_config::{Manifest, NodeCommitmentPolicy, NodePolicyOverride};
use reify_core::ValueCellId;
use reify_eval::cache::NodeId;
use reify_ir::{NodeKind, NodeTraits};
use reify_runtime::commitment::{NodeCommitmentOverride, NodeOverrideConfigError, NodePolicyOverrides};

/// The traits a node carries absent a `NodeTraitMap` entry — what level 4
/// actually sees for it. This file's claims are all about the config→overrides
/// boundary rather than about traits, so every node is resolved with its own
/// kind defaults instead of a hardcoded set: a `Value` node bearing
/// `COMMITTABLE` (real defaults: `IMMEDIATE`) is a combination production never
/// produces, and asserting on it reads as contradicting the Q-3 note in
/// `commitment.rs`.
fn kind_default_traits(node: &NodeId) -> NodeTraits {
    NodeKind::from(node).default_traits()
}

// --- G4 boundary + G2(b) distinguishability ---

#[test]
fn kind_selector_value_overrides_all_value_nodes() {
    // `only_run_on_final_inputs` is the one policy no level-4 default can
    // produce, so both assertions below distinguish "the override applied" from
    // "the node fell through". The `always_cancel_when_stale` spelling is
    // covered where it belongs, in reify-config's own
    // `two_node_overrides_entries_round_trip`.
    let toml = "\
[[node_overrides]]
node_id_pattern = \"value\"
commitment_policy = \"only_run_on_final_inputs\"
";
    let manifest = Manifest::from_toml_str(toml).expect("manifest must parse");
    let overrides = NodePolicyOverrides::from_config_overrides(manifest.node_overrides())
        .expect("from_config_overrides must succeed for kind selector");

    // Value node → overridden to OnlyRunOnFinalInputs
    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        overrides.resolve_with_traits(&value_node, kind_default_traits(&value_node)),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "Value kind selector must override all Value nodes"
    );

    // Constraint node → not overridden → its own level-4 default (kind isolation)
    let constraint_node = NodeId::Constraint(reify_core::ConstraintNodeId::new("Bracket", 0));
    assert_eq!(
        overrides.resolve_with_traits(&constraint_node, kind_default_traits(&constraint_node)),
        NodeCommitmentOverride::AlwaysCancelWhenStale,
        "kind selector for Value must not affect Constraint nodes"
    );
}

#[test]
fn g2b_default_config_resolves_to_the_kind_traits_default() {
    // G2(b) distinguishability: with no config entries the same node resolves
    // differently — to its kind+traits default instead of the override above.
    let default_config = NodePolicyOverrides::default();
    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    assert_eq!(
        default_config.resolve_with_traits(&value_node, kind_default_traits(&value_node)),
        NodeCommitmentOverride::AlwaysCancelWhenStale,
        "with no overrides a Value node must fall through to its IMMEDIATE default"
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
        overrides.resolve_with_traits(&width, kind_default_traits(&width)),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "instance selector must override the named node"
    );

    // A sibling node falls through to its level-4 default (instance isolation).
    let height = NodeId::Value(ValueCellId::new("Bracket", "height"));
    assert_eq!(
        overrides.resolve_with_traits(&height, kind_default_traits(&height)),
        NodeCommitmentOverride::AlwaysCancelWhenStale,
        "instance selector must not affect sibling nodes"
    );
}

#[test]
fn config_kind_override_wins_regardless_of_node_traits() {
    // The kind-selector half of the boundary: a kind entry materialises into
    // the level-2 `set_type` map, so level 4 is never consulted and the answer
    // is traits-independent. The instance half of the same claim is already
    // pinned as a unit by
    // `resolve_with_traits_respects_instance_then_type_precedence`; this is the
    // config path that has no such coverage.
    //
    // The trait sets below are deliberately synthetic — spanning both branches
    // of the COMMITTABLE-presence test plus the node's real defaults — because
    // traits-independence is the claim. The level-4 branches themselves are
    // owned by `resolve_with_traits_consults_default_overrides_at_level_4`.
    let entry = NodePolicyOverride {
        node_id_pattern: "value".into(),
        commitment_policy: NodeCommitmentPolicy::OnlyRunOnFinalInputs,
    };
    let overrides =
        NodePolicyOverrides::from_config_overrides(&[entry]).expect("kind selector must succeed");

    let value_node = NodeId::Value(ValueCellId::new("Bracket", "width"));
    for traits in [
        NodeTraits::empty(),
        NodeTraits::COMMITTABLE,
        kind_default_traits(&value_node),
    ] {
        assert_eq!(
            overrides.resolve_with_traits(&value_node, traits),
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
            "{traits:?}: a config kind override is level 2 and must win regardless of traits"
        );
    }
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
        overrides.resolve_with_traits(&value_node, kind_default_traits(&value_node)),
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
        overrides.resolve_with_traits(&width, kind_default_traits(&width)),
        NodeCommitmentOverride::OnlyRunOnFinalInputs,
        "last duplicate instance selector must win"
    );
}
