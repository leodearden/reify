//! CLI smoke test for `reify eval` on `match` over the PRELUDE `Result<T, E>`
//! — task β #4036, step-7 (PRD docs/prds/v0_6/result-and-fallback.md).
//!
//! Pins the literal `reify eval` report at the binary surface — the §1
//! user-observable signal: `match r { Ok { value: v } => v, Err { error: msg }
//! => 6mm }` reports `Widget.bore = 0.012 m` when `r` defaults to `Ok { value:
//! 12mm }`, and `Widget.bore = 0.006 m` when the default is switched to
//! `Err { error: "bad" }`.
//!
//! Mirrors `cli_eval_generic_enum.rs` (#4033) + `cli_check_result_prelude.rs`
//! (#4035) — uses `common::run_subcommand("eval", &common::fixture_path(..))`.
//! The reify-cli eval path needs no production change (task β "no change
//! expected"; see plan.json design_decisions).
//!
//! This test pins the **rendered string form**; the companion integration
//! tests `bore_is_12mm_when_r_defaults_to_ok` / `bore_is_6mm_when_r_defaults_to_err`
//! in `crates/reify-eval/tests/result_match_e2e.rs` pin the same values as
//! **numeric SI assertions** (`|si_value - 0.012| < 1e-12` /
//! `|si_value - 0.006| < 1e-12`, formatter-agnostic). The two are
//! complementary.
//!
//! Both fixtures (`result_match_bore_ok.ri` / `result_match_bore_err.ri`) ship
//! alongside this test file under `tests/fixtures/`, so these are GREEN pins
//! of the rendered `reify eval` output.

use crate::common;

/// `reify eval` on the `Ok { value: 12mm }`-default fixture exits 0 and
/// reports `Widget.bore = 0.012 m` (the Ok arm's `v` binder, unwrapped).
#[test]
fn eval_result_match_bore_ok_reports_12mm() {
    let (status, stdout, stderr) = common::run_subcommand(
        "eval",
        &common::fixture_path("result_match_bore_ok.ri"),
    );

    assert!(
        status.success(),
        "reify eval should exit 0 for result_match_bore_ok.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Widget.bore = 0.012 m"),
        "stdout should contain 'Widget.bore = 0.012 m' (§1 signal, Ok-arm default), got: {stdout}"
    );
}

/// Same fixture shape but `param r` defaults to `Err { error: "bad" }` —
/// `reify eval` exits 0 and reports `Widget.bore = 0.006 m` (the Err arm's
/// literal `6mm` fallback).
#[test]
fn eval_result_match_bore_err_reports_6mm() {
    let (status, stdout, stderr) = common::run_subcommand(
        "eval",
        &common::fixture_path("result_match_bore_err.ri"),
    );

    assert!(
        status.success(),
        "reify eval should exit 0 for result_match_bore_err.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Widget.bore = 0.006 m"),
        "stdout should contain 'Widget.bore = 0.006 m' (Err-arm fallback switch), got: {stdout}"
    );
}
