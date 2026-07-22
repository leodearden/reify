//! CLI smoke test for `reify eval` on the Result-recovery example (task
//! result-fallback Layer-B ε #4039, PRD docs/prds/v0_6/result-and-fallback.md
//! §1/§8.B task B-ε), the PRIMARY Layer-B user-observable leaf and B+H
//! integration gate.
//!
//! Pins the literal `reify eval` report at the binary surface (SI-normalized):
//! valid input (raw="12mm") recovers bore/bore_fb to 0.012 m and reports
//! diag = "ok"; unparseable input (raw="garbage") falls back to 0.006 m and
//! surfaces the real parse-failure reason as diag — the Err payload MESSAGE
//! that distinguishes Result (Layer B) from Option (Layer A), which carries
//! only presence/absence.
//!
//! Mirrors `cli_eval_result_fallback.rs` (task δ #4038's sibling) — uses
//! `common::run_subcommand("eval", &common::example_path("m6_result_recovery.ri"))`.
//!
//! Complementary to the numeric integration test
//! `crates/reify-eval/tests/result_recovery_e2e.rs`: that test pins the same
//! value cells as a formatter-agnostic numeric/String assertion; this one
//! pins the rendered string form as it appears in the binary's stdout.
//!
//! Failure mode: if `examples/m6_result_recovery.ri` lacks a `diag` cell,
//! the diag stdout assertions below fail with the actual stdout printed in
//! the panic message.

use crate::common;

/// `reify eval examples/m6_result_recovery.ri` exits 0 and reports the
/// SI-normalized recovered bore/bore_fb values plus the observable diag
/// message for both the valid and unparseable cases.
#[test]
fn eval_result_recovery_reports_recovered_values_and_diag() {
    let path = common::example_path("m6_result_recovery.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for m6_result_recovery.ri\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Valid input: raw="12mm" parses to Ok(12mm).
    assert!(
        stdout.contains("MountValid.bore = 0.012 m"),
        "stdout should contain 'MountValid.bore = 0.012 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountValid.bore_fb = 0.012 m"),
        "stdout should contain 'MountValid.bore_fb = 0.012 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountValid.diag = \"ok\""),
        "stdout should contain 'MountValid.diag = \"ok\"'\nstdout: {stdout}"
    );

    // Unparseable input: raw="garbage" parses to Err(..).
    assert!(
        stdout.contains("MountBad.bore = 0.006 m"),
        "stdout should contain 'MountBad.bore = 0.006 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountBad.bore_fb = 0.006 m"),
        "stdout should contain 'MountBad.bore_fb = 0.006 m'\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("MountBad.diag = \"could not parse 'garbage' as a length\""),
        "stdout should contain the observable Err message line\nstdout: {stdout}"
    );
}
