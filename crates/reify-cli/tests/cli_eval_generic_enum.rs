//! CLI smoke test for `reify eval` on the generic-enum example (task ε #4033,
//! step-7; PRD docs/prds/v0_6/generic-data-carrying-enums.md §8).
//!
//! Pins the literal `reify eval` report at the binary surface:
//!   `Demo.bore = 0.005 m`   (§1 signal: `Ok { value: 5mm }` default, unwrapped)
//!   `Demo.total = 0.003 m`  (recursive Tree<Length> leaves, 1mm + 2mm, summed
//!                            via nested two-arm match — INV-5 end-to-end)
//!
//! Mirrors the DCE ζ precedent `cli_eval_data_carrying_enum.rs` — uses
//! `common::run_subcommand("eval", &common::example_path("m6_generic_enum.ri"))`.
//! The reify-cli eval path needs no production change (task ε "no change expected").

mod common;

/// `reify eval examples/m6_generic_enum.ri` exits 0 and reports the SI-normalized
/// bore (0.005 m = 5mm, the Ok-arm default) and total (0.003 m = 1mm + 2mm Tree
/// leaves).
///
/// This test pins the **rendered string form** as it appears in the binary's
/// stdout. The companion integration tests `bore_ok_default_is_5mm` /
/// `tree_sum_total_is_3mm` in `crates/reify-eval/tests/generic_enum_erasure_e2e.rs`
/// pin the same values as **numeric SI assertions**
/// (`|si_value − 0.005| < 1e-12` / `|si_value − 0.003| < 1e-12`). The two are
/// complementary: the numeric assertions are formatter-agnostic; this one is
/// stable as long as the display format doesn't change. If the eval reporter's
/// output format changes, update the `contains` strings here; the numeric
/// integration tests will remain stable.
#[test]
fn eval_generic_enum_reports_bore_and_total() {
    let path = common::example_path("m6_generic_enum.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for m6_generic_enum.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Demo.bore = 0.005 m"),
        "stdout should contain 'Demo.bore = 0.005 m' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("Demo.total = 0.003 m"),
        "stdout should contain 'Demo.total = 0.003 m' (INV-5 Tree<Length> sum)\nstdout: {stdout}"
    );
}
