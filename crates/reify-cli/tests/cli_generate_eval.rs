//! End-to-end CLI test for the `generate` bolt-circle example (task 3994,
//! structural-query ζ).
//!
//! RED until step-12 creates `examples/generate_bolt_circle.ri`.
//!
//! This is the exit-0 end-to-end smoke. The structured golden (count == 4 and
//! the 4 point3 coordinates within tolerance) is verified separately in
//! `crates/reify-eval/tests/generate_eval.rs::generate_bolt_circle_example_golden`,
//! which has the compile→Engine::eval harness and avoids stdout-parse fragility.

mod common;

/// `reify eval examples/generate_bolt_circle.ri` exits 0 and stdout reports the
/// evaluated `positions` cell.
#[test]
fn eval_generate_bolt_circle_example_succeeds() {
    let path = common::example_path("generate_bolt_circle.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval generate_bolt_circle.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("positions"),
        "stdout should contain the 'positions' cell; got: {stdout}"
    );
}
