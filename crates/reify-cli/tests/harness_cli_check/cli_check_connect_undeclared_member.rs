//! End-to-end `reify check` gate for a DOTTED connect endpoint that names a
//! member the resolved sub's child does not declare at all (task #7880).
//!
//! Before #7880 the probe compiled clean and `reify check` reported the
//! connection's compat constraint Satisfied over a port that does not exist.
//! The compiler-level pins (declared non-port members, declared-later children,
//! match-arm clusters) live in
//! `reify-compiler/tests/harness_modules_ports/connect_compile_tests.rs`.

use crate::common;

/// `connect motor.nonexistent -> coupler.bore` -> rejected as an undefined port.
#[test]
fn check_connect_undeclared_sub_member_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("connect_undeclared_sub_member.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for an undeclared sub member.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("undefined port 'motor.nonexistent' in connect statement"),
        "stderr should report the undefined port, got: {stderr}"
    );
    assert!(
        !stdout.contains("All constraints satisfied"),
        "stdout must not report the connect as satisfied, got: {stdout}"
    );
}
