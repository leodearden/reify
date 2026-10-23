//! CLI integration tests for `reify dev inspect-node` (GR-038 ε integration gate).
//!
//! These tests focus on CLI-specific behaviour: exit code, stdout/stderr routing,
//! determinism, and that the id printed on the `node:` line is accepted back as
//! input. Exact field-content assertions live in the unit tests in
//! `crates/reify-cli/src/dev.rs`; one representative content check per test is
//! kept here to verify end-to-end routing from the binary.

use std::collections::BTreeSet;

use reify_core::{
    ComputeNodeId, ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId,
};
use reify_eval::cache::NodeId;

use crate::common;

/// (a) `reify dev inspect-node foo#computation[0]` exits 0 and stdout contains
/// at least the kind field (verifying the output routes to stdout).
#[test]
fn inspect_compute_foo_succeeds() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "foo#computation[0]"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Compute"),
        "missing 'kind: Compute' in stdout;\nstdout: {stdout}"
    );
    assert!(stderr.is_empty(), "expected empty stderr on success;\nstderr: {stderr}");
}

/// (b) Determinism: two identical runs produce byte-identical stdout.
#[test]
fn inspect_compute_foo_deterministic() {
    let (_, stdout1, _) = common::run_with_args(&["dev", "inspect-node", "foo#computation[0]"]);
    let (_, stdout2, _) = common::run_with_args(&["dev", "inspect-node", "foo#computation[0]"]);
    assert_eq!(
        stdout1, stdout2,
        "runs produced different stdout — output is non-deterministic"
    );
}

/// (c) Kind coverage — Value node exits 0 and output routes to stdout.
#[test]
fn inspect_value_b_w() {
    let (status, stdout, stderr) = common::run_with_args(&["dev", "inspect-node", "B.w"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("kind: Value"), "missing 'kind: Value';\nstdout: {stdout}");
}

/// (c) Kind coverage — Constraint node exits 0 and output routes to stdout.
#[test]
fn inspect_constraint_a() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "A#constraint[0]"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Constraint"),
        "missing 'kind: Constraint';\nstdout: {stdout}"
    );
}

/// (c) Kind coverage — Realization node exits 0 and output routes to stdout.
#[test]
fn inspect_realization() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "R#realization[0]"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Realization"),
        "missing 'kind: Realization';\nstdout: {stdout}"
    );
}

/// (c) Kind coverage — Resolution node exits 0 and output routes to stdout.
#[test]
fn inspect_resolution() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "S#resolution[0]"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Resolution"),
        "missing 'kind: Resolution';\nstdout: {stdout}"
    );
}

/// Exhaustive (no `_` arm), so a sixth `NodeId` variant fails to compile here
/// until the round-trip table below covers it.
fn node_id_variant_index(id: &NodeId) -> usize {
    match id {
        NodeId::Value(_) => 0,
        NodeId::Constraint(_) => 1,
        NodeId::Realization(_) => 2,
        NodeId::Resolution(_) => 3,
        NodeId::Compute(_) => 4,
    }
}

fn printed_node_id(stdout: &str) -> &str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("node: "))
        .unwrap_or_else(|| panic!("no 'node: ' line in stdout;\nstdout: {stdout}"))
}

/// (B11) The id printed on the `node:` line is valid input: it parses back to
/// the inspected id, and pasting it back in reproduces the output exactly.
#[test]
fn printed_node_id_parses_back_to_the_same_node_id_for_every_kind() {
    let ids = [
        NodeId::Value(ValueCellId::new("Bracket", "width")),
        NodeId::Value(ValueCellId::new("Widget", r#"vents["intake"]"#)),
        NodeId::Constraint(ConstraintNodeId::new("forall_connect:v@Rig.bolts#3", 0)),
        NodeId::Realization(RealizationNodeId::new("box-2658-smoke", 1)),
        NodeId::Resolution(ResolutionNodeId::new("A", 0)),
        NodeId::Compute(ComputeNodeId::new("Bracket", 4)),
    ];
    let kinds: BTreeSet<usize> = ids.iter().map(node_id_variant_index).collect();
    assert_eq!(kinds, (0..5).collect(), "the table must cover every kind");

    for id in ids {
        let input = id.to_string();
        let (status, first, stderr) = common::run_with_args(&["dev", "inspect-node", &input]);
        assert!(
            status.success(),
            "inspecting {input:?} must succeed;\nstdout: {first}\nstderr: {stderr}"
        );
        let printed = printed_node_id(&first);
        assert_eq!(
            printed.parse::<NodeId>(),
            Ok(id.clone()),
            "the printed id {printed:?} must name the inspected node"
        );

        let (status, second, stderr) = common::run_with_args(&["dev", "inspect-node", printed]);
        assert!(
            status.success(),
            "re-inspecting the printed id {printed:?} must succeed;\nstderr: {stderr}"
        );
        assert_eq!(
            second, first,
            "pasting the printed id {printed:?} back must reproduce the output"
        );
    }
}

/// (PRD §12 Q3) The legacy `Kind(inner)` grammar is gone: it is refused with
/// the typed parse error rather than read as some node.
#[test]
fn inspect_rejects_legacy_kind_inner_form() {
    let legacy = "Compute(foo)";
    let (status, stdout, stderr) = common::run_with_args(&["dev", "inspect-node", legacy]);
    assert!(
        !status.success(),
        "expected exit FAILURE for the legacy form;\nstdout: {stdout}\nstderr: {stderr}"
    );
    let typed = legacy.parse::<NodeId>().unwrap_err().to_string();
    assert!(
        stderr.contains(&typed),
        "stderr must carry the typed parse error {typed:?};\nstderr: {stderr}"
    );
}

/// (d) Error path — malformed node-id exits FAILURE with an error message on stderr.
#[test]
fn inspect_malformed_node_id_exits_failure() {
    let (status, _stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "NotAKind(x)"]);
    assert!(
        !status.success(),
        "expected exit FAILURE for unknown kind;\nstderr: {stderr}"
    );
    assert!(
        !stderr.is_empty(),
        "expected an error message on stderr;\nstderr: {stderr}"
    );
}

/// (d) Error path — missing node-id arg prints usage and exits FAILURE.
#[test]
fn inspect_missing_node_id_exits_failure() {
    let (status, _stdout, stderr) = common::run_with_args(&["dev", "inspect-node"]);
    assert!(
        !status.success(),
        "expected exit FAILURE when node-id is missing;\nstderr: {stderr}"
    );
    assert!(
        !stderr.is_empty(),
        "expected usage message on stderr;\nstderr: {stderr}"
    );
}

/// (d) Error path — extra positional arguments after node-id exit FAILURE.
#[test]
fn inspect_extra_args_exits_failure() {
    let (status, _stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "foo#computation[0]", "garbage"]);
    assert!(
        !status.success(),
        "expected exit FAILURE for extra argument;\nstderr: {stderr}"
    );
    assert!(
        !stderr.is_empty(),
        "expected error message on stderr;\nstderr: {stderr}"
    );
}

/// (d) Error path — unknown dev subcommand exits FAILURE.
#[test]
fn unknown_dev_subcommand_exits_failure() {
    let (status, _stdout, stderr) = common::run_with_args(&["dev", "frobnicate"]);
    assert!(
        !status.success(),
        "expected exit FAILURE for unknown dev subcommand;\nstderr: {stderr}"
    );
    assert!(
        !stderr.is_empty(),
        "expected error message on stderr;\nstderr: {stderr}"
    );
}
