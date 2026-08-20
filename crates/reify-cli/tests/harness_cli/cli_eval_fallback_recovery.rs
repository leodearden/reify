//! CLI smoke test for `reify eval` on the fallback-recovery example (task
//! 3986, result-and-fallback PRD §8 Phase 4 task δ — THE integration gate /
//! primary user-observable signal).
//!
//! Pins the literal `reify eval` report at the binary surface (§1 signal,
//! SI-normalized): valid input (raw="12mm") recovers to 12mm/12mm/true;
//! unparseable input (raw="garbage") falls back to 6mm/4mm/false.
//!
//! Mirrors `cli_eval_data_carrying_enum.rs` — uses
//! `common::run_subcommand("eval", &common::example_path("m6_fallback_recovery.ri"))`.
//! The reify-cli eval path needs no production change (PRD §8 δ "no change
//! expected").
//!
//! Complementary to the numeric integration test
//! `crates/reify-eval/tests/fallback_recovery_e2e.rs`: that test pins the
//! same six value cells as a formatter-agnostic numeric assertion; this one
//! pins the rendered string form as it appears in the binary's stdout. If the
//! eval reporter's output format changes, update the `contains` strings here;
//! the numeric integration test will remain stable.

use crate::common;

/// `reify eval examples/m6_fallback_recovery.ri` exits 0 and reports the
/// SI-normalized recovered values for both the valid and unparseable cases.
#[test]
fn eval_fallback_recovery_reports_recovered_values() {
    let path = common::example_path("m6_fallback_recovery.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for m6_fallback_recovery.ri\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Valid input: raw="12mm" parses to some(12mm).
    assert!(
        stdout.contains("MountValid.bore = 0.012 m"),
        "stdout should contain 'MountValid.bore = 0.012 m' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountValid.pin = 0.012 m"),
        "stdout should contain 'MountValid.pin = 0.012 m' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountValid.has_bore = true"),
        "stdout should contain 'MountValid.has_bore = true' (§1 signal)\nstdout: {stdout}"
    );

    // Unparseable input: raw="garbage" parses to none.
    assert!(
        stdout.contains("MountBad.bore = 0.006 m"),
        "stdout should contain 'MountBad.bore = 0.006 m' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountBad.pin = 0.004 m"),
        "stdout should contain 'MountBad.pin = 0.004 m' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountBad.has_bore = false"),
        "stdout should contain 'MountBad.has_bore = false' (§1 signal)\nstdout: {stdout}"
    );
}
