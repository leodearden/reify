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

#[test]
fn check_with_simulation_ready_passes_on_a_std_prelude_only_file() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "simulation_ready=Part",
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
    assert!(
        !stdout.contains(UNRESOLVED_IMPORT) && !stderr.contains(UNRESOLVED_IMPORT),
        "`import std.prelude` must resolve.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn eval_recovers_through_unwrap_or_on_a_std_prelude_only_file() {
    let (status, stdout, stderr) = common::run_with_args(&["eval", &facade_fixture()]);
    assert!(
        status.success(),
        "eval should succeed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Part.bore = 0.006 m"),
        "unwrap_or should recover bore to 6mm.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
