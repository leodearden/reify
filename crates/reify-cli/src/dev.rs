//! `reify dev` subcommand group (GR-038 ε integration gate).
//!
//! Provides `reify dev inspect-node <node-id>`, which prints a node's
//! kind, declared traits, derived priority, derived policy, and active
//! instance/type overrides by routing through the α/β/γ/δ chain end-to-end.

use std::process::ExitCode;

use reify_core::{
    ComputeNodeId, ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId,
};
use reify_eval::cache::NodeId;
use reify_ir::{NodeKind, NodeTraits, NodeTraitsMap};
use reify_runtime::commitment::{NodeCommitmentOverride, NodePolicyOverrides};
use reify_runtime::{traits_to_priority, Priority};

/// Parse a `Kind(inner)` node-id string into a [`NodeId`].
///
/// Accepted grammar:
/// - `Value(Entity.member)` — exactly one `.`, non-empty entity and member
/// - `Compute(Entity)` or `Compute(Entity[index])` — optional `[u32]` suffix, index defaults to 0
/// - `Constraint(...)`, `Realization(...)`, `Resolution(...)` — same as Compute
///
/// Returns `Err(String)` with a user-facing message for every malformed form.
pub fn parse_node_id(s: &str) -> Result<NodeId, String> {
    // Grammar: Kind(inner)
    // Split at the first '(' and require a trailing ')'.
    let (kind_str, rest) = s
        .split_once('(')
        .ok_or_else(|| format!("invalid node-id {:?}: expected 'Kind(inner)' form", s))?;

    let inner = rest
        .strip_suffix(')')
        .ok_or_else(|| format!("invalid node-id {:?}: missing closing ')'", s))?;

    match kind_str {
        "Value" => {
            // Value(Entity.member) — exactly one '.', non-empty halves.
            let (entity, member) = inner.split_once('.').ok_or_else(|| {
                format!(
                    "invalid Value node-id {:?}: expected 'Value(Entity.member)'",
                    s
                )
            })?;
            if entity.is_empty() || member.is_empty() {
                return Err(format!(
                    "invalid Value node-id {:?}: entity and member must be non-empty",
                    s
                ));
            }
            // Reject multiple dots in the member part.
            if member.contains('.') {
                return Err(format!(
                    "invalid Value node-id {:?}: only one '.' separator allowed",
                    s
                ));
            }
            Ok(NodeId::Value(ValueCellId::new(entity, member)))
        }
        "Compute" => {
            let (entity, index) = parse_entity_with_optional_index(s, inner)?;
            Ok(NodeId::Compute(ComputeNodeId::new(entity, index)))
        }
        "Constraint" => {
            let (entity, index) = parse_entity_with_optional_index(s, inner)?;
            Ok(NodeId::Constraint(ConstraintNodeId::new(entity, index)))
        }
        "Realization" => {
            let (entity, index) = parse_entity_with_optional_index(s, inner)?;
            Ok(NodeId::Realization(RealizationNodeId::new(entity, index)))
        }
        "Resolution" => {
            let (entity, index) = parse_entity_with_optional_index(s, inner)?;
            Ok(NodeId::Resolution(ResolutionNodeId::new(entity, index)))
        }
        other => Err(format!(
            "unknown node kind {:?}: expected one of Value, Compute, Constraint, Realization, Resolution",
            other
        )),
    }
}

/// Parse `entity` or `entity[index]` from the inner part of a node-id.
///
/// Returns `(entity_str, index)`. Index defaults to 0 when the `[n]` suffix is absent.
fn parse_entity_with_optional_index<'a>(
    full: &str,
    inner: &'a str,
) -> Result<(&'a str, u32), String> {
    if inner.is_empty() {
        return Err(format!(
            "invalid node-id {:?}: entity name must be non-empty",
            full
        ));
    }
    if let Some((entity, idx_str)) = inner.split_once('[') {
        // Expect idx_str to end with ']'.
        let idx_str = idx_str.strip_suffix(']').ok_or_else(|| {
            format!(
                "invalid node-id {:?}: index bracket '[' opened but ']' not found",
                full
            )
        })?;
        if entity.is_empty() {
            return Err(format!(
                "invalid node-id {:?}: entity name must be non-empty",
                full
            ));
        }
        let index = idx_str.parse::<u32>().map_err(|_| {
            format!(
                "invalid node-id {:?}: index {:?} is not a valid u32",
                full, idx_str
            )
        })?;
        Ok((entity, index))
    } else {
        Ok((inner, 0))
    }
}

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

    // Instance and type override slots (both absent for empty config → "(none)").
    // Expose them explicitly so the output documents the full precedence chain.
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

/// Entry point for `reify dev <subcommand> [args...]`.
pub fn cmd_dev(args: &[String]) -> ExitCode {
    // Stub: will be implemented in step-8.
    match args.first().map(String::as_str) {
        Some("inspect-node") => cmd_inspect_node(&args[1..]),
        Some(other) => {
            eprintln!("Unknown dev subcommand: {}", other);
            eprintln!("Usage: reify dev inspect-node <node-id>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("Usage: reify dev inspect-node <node-id>");
            ExitCode::FAILURE
        }
    }
}

/// Entry point for `reify dev inspect-node <node-id>`.
fn cmd_inspect_node(args: &[String]) -> ExitCode {
    let node_id_str = match args.first() {
        Some(s) => s,
        None => {
            eprintln!("Usage: reify dev inspect-node <node-id>");
            eprintln!("  e.g.: reify dev inspect-node Compute(foo)");
            return ExitCode::FAILURE;
        }
    };
    match parse_node_id(node_id_str) {
        Ok(node_id) => {
            println!("{}", render_inspection(&node_id));
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("error: {msg}");
            eprintln!("Usage: reify dev inspect-node <node-id>");
            eprintln!("  e.g.: reify dev inspect-node Compute(foo)");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{
        ComputeNodeId, ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId,
    };
    use reify_eval::cache::NodeId;

    // ── render_inspection (step-5 RED) ───────────────────────────────────────

    fn contains(output: &str, needle: &str) -> bool {
        output.contains(needle)
    }

    #[test]
    fn render_compute_foo() {
        let node_id = parse_node_id("Compute(foo)").unwrap();
        let out = render_inspection(&node_id);
        assert!(contains(&out, "kind: Compute"), "missing 'kind: Compute' in:\n{out}");
        assert!(
            contains(&out, "declared traits: WARM_STARTABLE | COMMITTABLE"),
            "missing traits in:\n{out}"
        );
        assert!(
            contains(&out, "derived priority: P1Slow"),
            "missing priority in:\n{out}"
        );
        assert!(
            contains(&out, "derived policy: CommitIfSlow"),
            "missing policy in:\n{out}"
        );
        assert!(
            contains(&out, "instance override: (none)"),
            "missing instance override line in:\n{out}"
        );
        assert!(
            contains(&out, "type override: (none)"),
            "missing type override line in:\n{out}"
        );
    }

    #[test]
    fn render_value_b_w() {
        let node_id = parse_node_id("Value(B.w)").unwrap();
        let out = render_inspection(&node_id);
        assert!(contains(&out, "kind: Value"), "missing 'kind: Value' in:\n{out}");
        assert!(
            contains(&out, "declared traits: IMMEDIATE"),
            "missing 'declared traits: IMMEDIATE' in:\n{out}"
        );
        assert!(
            contains(&out, "derived priority: P1Fast"),
            "missing 'derived priority: P1Fast' in:\n{out}"
        );
        assert!(
            contains(&out, "derived policy: AlwaysCancelWhenStale"),
            "missing 'derived policy: AlwaysCancelWhenStale' in:\n{out}"
        );
    }

    #[test]
    fn render_constraint_a() {
        let node_id = parse_node_id("Constraint(A)").unwrap();
        let out = render_inspection(&node_id);
        assert!(
            contains(&out, "declared traits: (none)"),
            "missing 'declared traits: (none)' in:\n{out}"
        );
        assert!(
            contains(&out, "derived priority: P3Speculative"),
            "missing 'derived priority: P3Speculative' in:\n{out}"
        );
        assert!(
            contains(&out, "derived policy: AlwaysCancelWhenStale"),
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

    // ── parse_node_id (step-1 RED) ────────────────────────────────────────────

    #[test]
    fn parse_compute_no_index() {
        let result = parse_node_id("Compute(foo)");
        assert_eq!(result, Ok(NodeId::Compute(ComputeNodeId::new("foo", 0))));
    }

    #[test]
    fn parse_compute_with_index() {
        let result = parse_node_id("Compute(foo[3])");
        assert_eq!(result, Ok(NodeId::Compute(ComputeNodeId::new("foo", 3))));
    }

    #[test]
    fn parse_value() {
        let result = parse_node_id("Value(Bracket.width)");
        assert_eq!(
            result,
            Ok(NodeId::Value(ValueCellId::new("Bracket", "width")))
        );
    }

    #[test]
    fn parse_constraint_with_index() {
        let result = parse_node_id("Constraint(A[2])");
        assert_eq!(
            result,
            Ok(NodeId::Constraint(ConstraintNodeId::new("A", 2)))
        );
    }

    #[test]
    fn parse_realization_no_index() {
        let result = parse_node_id("Realization(R)");
        assert_eq!(
            result,
            Ok(NodeId::Realization(RealizationNodeId::new("R", 0)))
        );
    }

    #[test]
    fn parse_resolution_no_index() {
        let result = parse_node_id("Resolution(S)");
        assert_eq!(
            result,
            Ok(NodeId::Resolution(ResolutionNodeId::new("S", 0)))
        );
    }

    // Error cases ──────────────────────────────────────────────────────────────

    #[test]
    fn parse_bare_word_is_err() {
        assert!(parse_node_id("foo").is_err());
    }

    #[test]
    fn parse_unknown_kind_is_err() {
        assert!(parse_node_id("Bogus(x)").is_err());
    }

    #[test]
    fn parse_value_no_dot_is_err() {
        assert!(parse_node_id("Value(no_dot)").is_err());
    }

    #[test]
    fn parse_empty_inner_is_err() {
        assert!(parse_node_id("Compute()").is_err());
    }
}
