//! A file whose only import is `import std.prelude` checks and evaluates
//! through the production CLI with no unresolved-import warning (task #5502,
//! PRD stdlib-namespace ι, boundary #11). Input: the fixture
//! `tests/fixtures/stdlib_prelude_facade.ri`, which uses `Length`/`mm`,
//! `unwrap_or` and the `simulation_ready` purpose.

use crate::common;

const UNRESOLVED_IMPORT: &str = "not resolved by this entry point";

fn facade_fixture() -> String {
    common::fixture_path("stdlib_prelude_facade.ri")
}

fn assert_std_prelude_resolved(stdout: &str, stderr: &str) {
    assert!(
        !stdout.contains(UNRESOLVED_IMPORT) && !stderr.contains(UNRESOLVED_IMPORT),
        "`import std.prelude` must resolve.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_with_simulation_ready_passes_on_a_std_prelude_only_file() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Bracket",
        &facade_fixture(),
    ]);
    assert!(
        status.success(),
        "check should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "simulation_ready should be satisfied.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_std_prelude_resolved(&stdout, &stderr);
}

#[test]
fn eval_recovers_through_unwrap_or_on_a_std_prelude_only_file() {
    let (status, stdout, stderr) = common::run_with_args(&["eval", &facade_fixture()]);
    assert!(
        status.success(),
        "eval should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Bracket.bore = 0.006 m"),
        "unwrap_or should recover bore to 6mm.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_std_prelude_resolved(&stdout, &stderr);
}

#[test]
fn fix_imports_check_finds_nothing_missing_on_a_std_prelude_only_file() {
    let (status, stdout, stderr) =
        common::run_with_args(&["fix", "--imports", "--check", &facade_fixture()]);
    assert!(
        !stdout.contains("missing import") && !stderr.contains("missing import"),
        "std.prelude re-exports every std module the fixture uses.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        status.success(),
        "fix --imports --check should pass.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
