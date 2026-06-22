//! CLI integration tests for `reify dev inspect-node` (GR-038 ε integration gate).
//!
//! These tests focus on CLI-specific behaviour: exit code, stdout/stderr routing,
//! and determinism. Exact field-content assertions live in the unit tests in
//! `crates/reify-cli/src/dev.rs`; one representative content check per test is
//! kept here to verify end-to-end routing from the binary.

mod common;

/// (a) `reify dev inspect-node Compute(foo)` exits 0 and stdout contains
/// at least the kind field (verifying the output routes to stdout).
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
        "missing 'kind: Compute' in stdout;\nstdout: {stdout}"
    );
    assert!(stderr.is_empty(), "expected empty stderr on success;\nstderr: {stderr}");
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

/// (c) Kind coverage — Value node exits 0 and output routes to stdout.
#[test]
fn inspect_value_b_w() {
    let (status, stdout, stderr) =
        common::run_with_args(&["dev", "inspect-node", "Value(B.w)"]);
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
        common::run_with_args(&["dev", "inspect-node", "Constraint(A)"]);
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
        common::run_with_args(&["dev", "inspect-node", "Realization(R)"]);
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
        common::run_with_args(&["dev", "inspect-node", "Resolution(S)"]);
    assert!(
        status.success(),
        "expected exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("kind: Resolution"),
        "missing 'kind: Resolution';\nstdout: {stdout}"
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
        common::run_with_args(&["dev", "inspect-node", "Compute(foo)", "garbage"]);
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
