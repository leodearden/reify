//! CLI integration tests for `reify dev inspect-node` (GR-038 ε integration gate).
//!
//! RED on base because main()'s dispatcher has no `"dev"` arm →
//! "Unknown command: dev" + FAILURE.

mod common;

/// (a) `reify dev inspect-node Compute(foo)` exits 0 and stdout contains
/// the expected 5-field block.
#[test]
fn inspect_compute_foo_succeeds() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "Compute(foo)"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Compute"),
        "missing 'kind: Compute';\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("declared traits: WARM_STARTABLE | COMMITTABLE"),
        "missing traits;\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived priority: P1Slow"),
        "missing priority;\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived policy: CommitIfSlow"),
        "missing policy;\nstdout: {stdout}"
    );
}

/// (b) Determinism: two identical runs produce byte-identical stdout.
#[test]
fn inspect_compute_foo_deterministic() {
    let (_, stdout1, _) = common::run_with_args(&["dev", "inspect-node", "Compute(foo)"]);
    let (_, stdout2, _) = common::run_with_args(&["dev", "inspect-node", "Compute(foo)"]);
    assert_eq!(
        stdout1, stdout2,
        "runs produced different stdout — output is non-deterministic"
    );
}

/// (c) Kind coverage — Value node.
#[test]
fn inspect_value_b_w() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "Value(B.w)"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("kind: Value"), "missing 'kind: Value';\nstdout: {stdout}");
    assert!(
        stdout.contains("declared traits: IMMEDIATE"),
        "missing 'declared traits: IMMEDIATE';\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived priority: P1Fast"),
        "missing 'derived priority: P1Fast';\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived policy: AlwaysCancelWhenStale"),
        "missing 'derived policy: AlwaysCancelWhenStale';\nstdout: {stdout}"
    );
}

/// (c) Kind coverage — Constraint node.
#[test]
fn inspect_constraint_a() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "Constraint(A)"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("declared traits: (none)"),
        "missing 'declared traits: (none)';\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived priority: P3Speculative"),
        "missing 'derived priority: P3Speculative';\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("derived policy: AlwaysCancelWhenStale"),
        "missing 'derived policy: AlwaysCancelWhenStale';\nstdout: {stdout}"
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
