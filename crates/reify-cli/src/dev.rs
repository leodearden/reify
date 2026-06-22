//! `reify dev` subcommand group (GR-038 ε integration gate).
//!
//! Provides `reify dev inspect-node <node-id>`, which prints a node's
//! kind, declared traits, derived priority, derived policy, and active
//! instance/type overrides by routing through the α/β/γ/δ chain end-to-end.

use std::process::ExitCode;

use reify_eval::cache::NodeId;
use reify_ir::NodeTraits;

/// Parse a `Kind(inner)` node-id string into a [`NodeId`].
///
/// Accepted grammar:
/// - `Value(Entity.member)` — exactly one `.`, non-empty entity and member
/// - `Compute(Entity)` or `Compute(Entity[index])` — optional `[u32]` suffix, index defaults to 0
/// - `Constraint(...)`, `Realization(...)`, `Resolution(...)` — same as Compute
///
/// Returns `Err(String)` with a user-facing message for every malformed form.
pub fn parse_node_id(_s: &str) -> Result<NodeId, String> {
    // Stub: will be implemented in step-2.
    Err(format!("parse_node_id stub: not yet implemented"))
}

/// Format [`NodeTraits`] as a human-readable string for CLI output.
///
/// Renders each set flag by name in canonical order
/// (`IMMEDIATE`, `WARM_STARTABLE`, `PROGRESSIVE`, `COMMITTABLE`),
/// separated by ` | `. Returns `"(none)"` when the set is empty.
pub fn format_node_traits(_t: NodeTraits) -> String {
    // Stub: will be implemented in step-4.
    String::new()
}

/// Build the full inspection block for a node.
///
/// Resolves the node's kind, traits, priority, and policy through the
/// α/β/γ/δ chain (empty maps → kind-derived defaults) and renders
/// a documented multi-line block.
pub fn render_inspection(_node_id: &NodeId) -> String {
    // Stub: will be implemented in step-6.
    String::new()
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
fn cmd_inspect_node(_args: &[String]) -> ExitCode {
    // Stub: will be implemented in step-8.
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    // Unit tests added in step-1 (parse_node_id), step-3 (format_node_traits),
    // step-5 (render_inspection).
}
