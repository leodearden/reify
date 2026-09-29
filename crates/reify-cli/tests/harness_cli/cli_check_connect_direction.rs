//! End-to-end `reify check` gate for connect port-direction checking on DOTTED
//! sub-port endpoints (task #7175).
//!
//! `reify check` compiles + constraint-checks (no geometry eval), so the
//! compile-time direction diagnostic emitted by `crate::connect` carries the
//! user-observable signal. The dotted In -> In fixture must exit non-zero with
//! the diagnostic on stderr; the bare In -> In fixture is the control (green
//! before this task, and pinned here so the two forms are held to one wording);
//! the direction-correct dotted fixture pins that resolving dotted endpoints
//! does not turn well-formed assemblies red.
//!
//! The CLI surfaces diagnostic MESSAGE text, so these assertions match message
//! substrings — the constraint-literal assertions (that the connection stops
//! reading as "checked and fine") live in the compiler unit tests
//! (`reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs`).

use crate::common;

/// `connect e1.p -> e2.p` with both `Leaf.p : in T` -> rejected.
#[test]
fn check_connect_dotted_direction_mismatch_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("connect_direction_dotted_mismatch.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for a dotted In -> In connect.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("incompatible port directions"),
        "stderr should report the incompatible directions, got: {stderr}"
    );
}

/// The control: `connect a -> b` with both `in` on the OWN entity -> rejected,
/// with the same wording the dotted case above is held to.
#[test]
fn check_connect_bare_direction_mismatch_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("connect_direction_bare_mismatch.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for a bare In -> In connect.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("incompatible port directions"),
        "stderr should report the incompatible directions, got: {stderr}"
    );
}

/// `connect a.p -> b.q` with `Src.p : out` and `Dst.q : in` -> checks clean.
#[test]
fn check_connect_dotted_direction_ok_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("connect_direction_dotted_ok.ri"),
    );

    assert!(
        status.success(),
        "reify check should exit 0 for a direction-correct dotted connect.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
}
