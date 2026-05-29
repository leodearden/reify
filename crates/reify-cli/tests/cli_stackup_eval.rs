//! End-to-end CLI tests for tolerance stack-up eval integration.
//!
//! Tests are RED until step-8 creates the example and fixture files.

mod common;

/// Test A: `reify eval examples/tolerance-stackup-rss.ri` succeeds (exit 0)
/// and stdout contains the worst-case and rss map keys.
///
/// A benign compiler Warning for the unknown stackup builtin names may appear
/// on stderr — we do NOT assert stderr is empty.
#[test]
fn eval_tolerance_stackup_rss_example_succeeds() {
    let path = common::example_path("tolerance-stackup-rss.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval tolerance-stackup-rss.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("worst_case_band"),
        "stdout should contain 'worst_case_band'; got: {stdout}"
    );
    assert!(
        stdout.contains("rss_sigma"),
        "stdout should contain 'rss_sigma'; got: {stdout}"
    );
}

/// Test B: `reify eval crates/reify-cli/tests/fixtures/stackup_empty_chain.ri`
/// exits non-zero and stderr contains "E_StackupEmptyChain".
#[test]
fn eval_stackup_empty_chain_fixture_fails_with_diagnostic() {
    let path = common::fixture_path("stackup_empty_chain.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "reify eval stackup_empty_chain.ri should exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_StackupEmptyChain"),
        "stderr should contain 'E_StackupEmptyChain'; got: {stderr}"
    );
}
