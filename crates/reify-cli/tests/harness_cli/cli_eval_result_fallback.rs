//! CLI smoke test for `reify eval` on the Result-fallback example (task
//! result-fallback Layer-B δ #4038, PRD docs/prds/v0_6/result-and-fallback.md
//! §4.3/§5 D6/§8.B task B-δ).
//!
//! Pins the literal `reify eval` report at the binary surface (SI-normalized):
//! valid input (raw="12mm") recovers to 0.012 m/0.012 m; unparseable input
//! (raw="garbage") falls back to 0.006 m/0.004 m.
//!
//! Mirrors `cli_eval_fallback_recovery.rs` (task 3986's Option-subject
//! sibling) — uses `common::run_subcommand("eval",
//! &common::example_path("m6_result_fallback.ri"))`.
//!
//! Complementary to the numeric integration test
//! `crates/reify-eval/tests/result_fallback_e2e.rs`: that test pins the same
//! four value cells as a formatter-agnostic numeric assertion; this one pins
//! the rendered string form as it appears in the binary's stdout.
//!
//! RED before step-5: `examples/m6_result_fallback.ri` does not exist yet, so
//! `reify eval` fails to load it.

use crate::common;

/// `reify eval examples/m6_result_fallback.ri` exits 0 and reports the
/// SI-normalized recovered values for both the valid and unparseable cases.
#[test]
fn eval_result_fallback_reports_recovered_values() {
    let path = common::example_path("m6_result_fallback.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for m6_result_fallback.ri\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Valid input: raw="12mm" parses to Ok(12mm).
    assert!(
        stdout.contains("Valid.bore = 0.012 m"),
        "stdout should contain 'Valid.bore = 0.012 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("Valid.pin = 0.012 m"),
        "stdout should contain 'Valid.pin = 0.012 m'\nstdout: {stdout}"
    );

    // Unparseable input: raw="garbage" parses to Err(..).
    assert!(
        stdout.contains("Bad.bore = 0.006 m"),
        "stdout should contain 'Bad.bore = 0.006 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("Bad.pin = 0.004 m"),
        "stdout should contain 'Bad.pin = 0.004 m'\nstdout: {stdout}"
    );
}
