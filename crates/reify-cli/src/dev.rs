//! `reify dev` subcommand group (GR-038 ε integration gate).
//!
//! Provides `reify dev inspect-node <node-id>`, which prints a node's
//! kind, declared traits, derived priority, derived policy, and the
//! instance/type override slots (always `(none)` today; see
//! `render_inspection`) by routing through the α/β/γ/δ chain end-to-end.
//! `<node-id>` is the id printed on the output's `node:` line (the node's
//! Display form), so an inspected id can be pasted straight back in.

use std::process::ExitCode;

use reify_core::identity::IndexedNodeKind;
use reify_eval::cache::NodeId;
use reify_ir::{NodeKind, NodeTraits, NodeTraitsMap};
use reify_runtime::commitment::{NodeCommitmentOverride, NodePolicyOverrides};
use reify_runtime::{traits_to_priority, Priority};

/// The node id the usage hint offers as an example.
const EXAMPLE_NODE_ID: &str = "Bracket#computation[0]";

/// Format [`NodeTraits`] as a human-readable string for CLI output.
///
/// Renders each set flag by name in canonical order
/// (`IMMEDIATE`, `WARM_STARTABLE`, `PROGRESSIVE`, `COMMITTABLE`),
/// separated by ` | `. Returns `"(none)"` when the set is empty.
pub fn format_node_traits(t: NodeTraits) -> String {
    // Canonical flag order: IMMEDIATE, WARM_STARTABLE, PROGRESSIVE, COMMITTABLE.
    let mut parts: Vec<&'static str> = Vec::new();
    if t.contains(NodeTraits::IMMEDIATE) {
        parts.push("IMMEDIATE");
    }
    if t.contains(NodeTraits::WARM_STARTABLE) {
        parts.push("WARM_STARTABLE");
    }
    if t.contains(NodeTraits::PROGRESSIVE) {
        parts.push("PROGRESSIVE");
    }
    if t.contains(NodeTraits::COMMITTABLE) {
        parts.push("COMMITTABLE");
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(" | ")
    }
}

/// Build the full inspection block for a node.
///
/// Resolves the node's kind, traits, priority, and policy through the
/// α/β/γ/δ chain (empty maps → kind-derived defaults) and renders
/// a documented multi-line block.
pub fn render_inspection(node_id: &NodeId) -> String {
    // α: kind from NodeId
    let kind: NodeKind = NodeKind::from(node_id);
    // β: resolve traits through empty NodeTraitsMap (falls through to kind defaults)
    let traits_map = NodeTraitsMap::<NodeId>::new();
    let traits = traits_map.resolve(node_id);
    // γ: derive priority from traits
    let priority: Priority = traits_to_priority(traits);
    // δ: derive commitment policy through NodePolicyOverrides (empty → default_overrides)
    let policy_overrides = NodePolicyOverrides::new();
    let policy: NodeCommitmentOverride = policy_overrides.resolve_with_traits(node_id, traits);

    // Instance and type override slots — pinned to "(none)". inspect-node never
    // loads reify.toml and no production path populates NodePolicyOverrides, so
    // both maps are always empty. `policy` above is the kind+traits default; no
    // scheduler applies it. Status: docs/notes/reify-toml-schema.md.
    let instance_override = "(none)";
    let type_override = "(none)";

    format!(
        "node: {node_id}\n\
         kind: {kind:?}\n\
         declared traits: {traits}\n\
         derived priority: {priority:?}\n\
         derived policy: {policy:?}\n\
         instance override: {instance_override}\n\
         type override: {type_override}",
        traits = format_node_traits(traits),
    )
}

/// Print the `inspect-node` usage, naming every accepted `<node-id>` form.
fn print_inspect_node_usage() {
    let indexed_forms =
        IndexedNodeKind::ALL.map(|kind| format!("<entity>#{}[<index>]", kind.marker()));
    eprintln!("Usage: reify dev inspect-node <node-id>");
    eprintln!(
        "  <node-id>: <entity>.<member> | {}",
        indexed_forms.join(" | ")
    );
    eprintln!("  e.g.: reify dev inspect-node '{EXAMPLE_NODE_ID}'");
}

/// Entry point for `reify dev <subcommand> [args...]`.
pub fn cmd_dev(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("inspect-node") => cmd_inspect_node(&args[1..]),
        Some(other) => {
            eprintln!("Unknown dev subcommand: {}", other);
            print_inspect_node_usage();
            ExitCode::FAILURE
        }
        None => {
            print_inspect_node_usage();
            ExitCode::FAILURE
        }
    }
}

/// Entry point for `reify dev inspect-node <node-id>`.
fn cmd_inspect_node(args: &[String]) -> ExitCode {
    let node_id_str = match args.first() {
        Some(s) => s,
        None => {
            print_inspect_node_usage();
            return ExitCode::FAILURE;
        }
    };
    if args.len() > 1 {
        eprintln!("error: unexpected extra arguments after <node-id>");
        print_inspect_node_usage();
        return ExitCode::FAILURE;
    }
    match node_id_str.parse::<NodeId>() {
        Ok(node_id) => {
            println!("{}", render_inspection(&node_id));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: invalid node id {node_id_str:?}: {e}");
            print_inspect_node_usage();
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{ComputeNodeId, ConstraintNodeId, ValueCellId};

    #[test]
    fn usage_example_node_id_is_accepted() {
        if let Err(e) = EXAMPLE_NODE_ID.parse::<NodeId>() {
            panic!("the usage example {EXAMPLE_NODE_ID:?} must be a valid node id: {e}");
        }
    }

    // ── render_inspection (step-5 RED) ───────────────────────────────────────

    #[test]
    fn render_compute_foo() {
        let node_id = NodeId::Compute(ComputeNodeId::new("foo", 0));
        let out = render_inspection(&node_id);
        assert!(out.contains("kind: Compute"), "missing 'kind: Compute' in:\n{out}");
        assert!(
            out.contains("declared traits: WARM_STARTABLE | COMMITTABLE"),
            "missing traits in:\n{out}"
        );
        assert!(
            out.contains("derived priority: P1Slow"),
            "missing priority in:\n{out}"
        );
        assert!(
            out.contains("derived policy: CommitIfSlow"),
            "missing policy in:\n{out}"
        );
        assert!(
            out.contains("instance override: (none)"),
            "missing instance override line in:\n{out}"
        );
        assert!(
            out.contains("type override: (none)"),
            "missing type override line in:\n{out}"
        );
    }

    #[test]
    fn render_value_b_w() {
        let node_id = NodeId::Value(ValueCellId::new("B", "w"));
        let out = render_inspection(&node_id);
        assert!(out.contains("kind: Value"), "missing 'kind: Value' in:\n{out}");
        assert!(
            out.contains("declared traits: IMMEDIATE"),
            "missing 'declared traits: IMMEDIATE' in:\n{out}"
        );
        assert!(
            out.contains("derived priority: P1Fast"),
            "missing 'derived priority: P1Fast' in:\n{out}"
        );
        assert!(
            out.contains("derived policy: AlwaysCancelWhenStale"),
            "missing 'derived policy: AlwaysCancelWhenStale' in:\n{out}"
        );
    }

    #[test]
    fn render_constraint_a() {
        let node_id = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let out = render_inspection(&node_id);
        assert!(
            out.contains("declared traits: (none)"),
            "missing 'declared traits: (none)' in:\n{out}"
        );
        assert!(
            out.contains("derived priority: P3Speculative"),
            "missing 'derived priority: P3Speculative' in:\n{out}"
        );
        assert!(
            out.contains("derived policy: AlwaysCancelWhenStale"),
            "missing 'derived policy: AlwaysCancelWhenStale' in:\n{out}"
        );
    }

    // ── format_node_traits (step-3 RED) ──────────────────────────────────────

    #[test]
    fn format_immediate() {
        assert_eq!(format_node_traits(NodeTraits::IMMEDIATE), "IMMEDIATE");
    }

    #[test]
    fn format_warm_startable_and_committable() {
        assert_eq!(
            format_node_traits(NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE),
            "WARM_STARTABLE | COMMITTABLE"
        );
    }

    #[test]
    fn format_empty_is_none() {
        assert_eq!(format_node_traits(NodeTraits::empty()), "(none)");
    }

    #[test]
    fn format_canonical_order_all_flags() {
        let all = NodeTraits::IMMEDIATE
            | NodeTraits::WARM_STARTABLE
            | NodeTraits::PROGRESSIVE
            | NodeTraits::COMMITTABLE;
        assert_eq!(
            format_node_traits(all),
            "IMMEDIATE | WARM_STARTABLE | PROGRESSIVE | COMMITTABLE"
        );
    }
}
