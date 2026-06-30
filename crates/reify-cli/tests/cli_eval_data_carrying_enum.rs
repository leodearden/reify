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
