//! CLI smoke test for `reify eval` on the data-carrying-enum example (ζ #3946, step-5).
//!
//! Pins the literal `reify eval` report at the binary surface:
//!   `Widget.area = 0.0002 m^2`  (SI-normalized §1 signal: 20mm × 10mm = 200 mm²)
//!   `Widget.outline = Shape::Rect` (default variant)
//!
//! Mirrors the δ precedent `cli_check_variant_construction.rs` — uses
//! `common::run_subcommand("eval", &common::example_path("m6_data_carrying_enum.ri"))`.
//! The reify-cli eval path needs no production change (PRD §1 "no change expected").

mod common;

/// `reify eval examples/m6_data_carrying_enum.ri` exits 0 and reports the
/// SI-normalized area (0.0002 m^2 = 20mm × 10mm) and the Rect outline.
///
/// This test pins the **rendered string form** as it appears in the binary's stdout.
/// The companion integration test `rect_default_area_is_200mm2` in
/// `crates/reify-eval/tests/m6_data_carrying_enum.rs` pins the same value as a
/// **numeric SI assertion** (`|si_value − 0.0002| < 1e-12`).  The two are
/// complementary: the numeric assertion is formatter-agnostic; this one is stable
/// as long as the display format doesn't change.  If the eval reporter's output
/// format changes, update the `contains` string here; the numeric integration test
/// will remain stable.
#[test]
fn eval_data_carrying_enum_reports_area() {
    let path = common::example_path("m6_data_carrying_enum.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval should exit 0 for m6_data_carrying_enum.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Widget.area = 0.0002 m^2"),
        "stdout should contain 'Widget.area = 0.0002 m^2' (§1 signal)\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("Widget.outline = Shape::Rect"),
        "stdout should contain 'Widget.outline = Shape::Rect'\nstdout: {stdout}"
    );
}
