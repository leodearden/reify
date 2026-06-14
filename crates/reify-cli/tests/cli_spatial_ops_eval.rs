//! End-to-end CLI tests for the std.fields ε spatial-op constructors (task 4223 ε).
//!
//! Tests `reify check` and `reify eval` against the canonical worked example
//! `examples/fields/spatial_ops.ri`, which exercises all four ops (B6/B7/B8):
//!   constant_field — uniform field sampled at multiple points
//!   clamp_field    — over-range Pressure input clamped to bound (B7)
//!   remap_field    — linear remap
//!   threshold      — Bool field sampling true/false (B8)
//!
//! Do NOT assert stderr is empty — a benign W_MODULE_DECL_MISSING warning appears
//! on stderr for every file that omits a top-of-file `module` declaration (the
//! entire examples corpus). This matches the pattern in cli_generics_eval.rs.

mod common;

/// B6/B7/B8: `reify check examples/fields/spatial_ops.ri` exits 0 and reports
/// "All constraints satisfied." proving all four spatial-op constructor fns work.
///
/// `reify eval` must also exit 0 and contain key value substrings:
///   - the constant-field value "42" (from constant_field(42.0) sampled at 0.0/1.0)
///   - the clamped pressure value (200MPa in SI-base ≈ 2e8 Pa → contains "200000000")
///   - the boolean true/false results from threshold
///
/// RED until step-8 creates examples/fields/spatial_ops.ri: `reify check`
/// fails to load the file → non-zero exit / no "All constraints satisfied."
#[test]
fn eval_spatial_ops_example() {
    let path = common::example_path("fields/spatial_ops.ri");

    // `reify check` must exit 0 AND report all constraints satisfied.
    let (status, stdout, stderr) = common::run_subcommand("check", &path);
    assert!(
        status.success(),
        "reify check fields/spatial_ops.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.';\ngot: {stdout}\nstderr: {stderr}"
    );

    // `reify eval` must exit 0.
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);
    assert!(
        status.success(),
        "reify eval fields/spatial_ops.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // B6: constant_field(42.0) sampled at any point → 42.0 (Real).
    assert!(
        stdout.contains("42"),
        "stdout should contain '42' (constant_field(42.0) value);\ngot: {stdout}\nstderr: {stderr}"
    );
}
