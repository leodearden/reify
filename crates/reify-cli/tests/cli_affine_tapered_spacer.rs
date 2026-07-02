//! CLI end-to-end gate for the AffineMap stdlib example (task 3965, PRD
//! `docs/prds/v0_6/affine-map-type.md` task η, "C-as-integration-gate").
//!
//! Runs the real `reify` binary against the committed
//! `examples/affine_tapered_spacer.ri` and asserts the two CLI-surface
//! signals tying tasks α–ζ together end-to-end:
//!
//! - `reify eval`: the three determinant cells print their exact closed-form
//!   values — OCCT-independent (always-on even on stub-mode CI runners).
//! - `reify build`: exit 0, "Wrote" in stdout, and the exported STEP file
//!   contains exactly 2 `MANIFOLD_SOLID_BREP(` entities (`body` +
//!   `body_composed`) — ACTIVE (the CLI binary links OCCT, per the
//!   sub-placement CLI-gate precedent in `cli_sub_placement_assembly.rs`).

mod common;

/// `reify eval examples/affine_tapered_spacer.ri` prints the three
/// determinant cells with their exact closed-form values and exits 0.
///
/// RED until the composed/rigid demonstrations are added to the example
/// (step-4): the `det_composed`/`det_rigid` cells do not exist yet.
#[test]
fn eval_tapered_spacer_prints_determinants() {
    let path = common::example_path("affine_tapered_spacer.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval affine_tapered_spacer.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("TaperedSpacer.det_z = 1.5"),
        "stdout should contain 'TaperedSpacer.det_z = 1.5'; got:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("TaperedSpacer.det_composed = 6"),
        "stdout should contain 'TaperedSpacer.det_composed = 6'; got:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("TaperedSpacer.det_rigid = 1.5"),
        "stdout should contain 'TaperedSpacer.det_rigid = 1.5'; got:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// `reify build examples/affine_tapered_spacer.ri -o out.step` exits 0 and
/// the exported STEP file contains exactly 2 `MANIFOLD_SOLID_BREP(` entities
/// (`body` + `body_composed`).
///
/// RED until `body_composed` is added to the example (step-4): only 1 solid
/// (`body`) exists before then.
#[test]
fn build_tapered_spacer_two_deformed_solids() {
    let result = common::run_build_at(&common::example_path("affine_tapered_spacer.ri"));

    assert!(
        result.status.success(),
        "reify build should exit 0 for affine_tapered_spacer.ri.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote'; got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "geometry file should be written for affine_tapered_spacer.ri"
    );

    let step_bytes =
        std::fs::read(&result.output_path).expect("failed to read exported STEP file");
    let step_str = String::from_utf8(step_bytes).expect("STEP output must be valid UTF-8");
    let solid_count = step_str.matches("MANIFOLD_SOLID_BREP(").count();
    assert_eq!(
        solid_count, 2,
        "exported STEP must contain exactly 2 solids (body + body_composed);\n\
         got {solid_count} MANIFOLD_SOLID_BREP entities."
    );
}
