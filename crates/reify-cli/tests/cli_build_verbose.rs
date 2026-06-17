//! Integration tests for `reify build --verbose` kernel-provenance output
//! (task 4248, piece 2).
//!
//! OCCT-gated: skipped when `reify_kernel_occt::OCCT_AVAILABLE` is false.
//!
//! RED today: `cmd_build` has no `--verbose` handling; no `kernel:` line is
//! printed and the no-`-o` invocation exits non-zero (missing -o guard).

mod common;

use tempfile::TempDir;

/// `reify build <bracket> -o <out.step> --verbose` must:
/// - exit 0
/// - stdout contains `kernel: occt` (provenance for the BRep build)
/// - stdout contains `Wrote` (file was written)
/// - out.step is written and non-empty
/// - stdout does NOT contain `kernel: manifold` (G3 regression guard)
#[test]
fn build_verbose_with_output_reports_occt_kernel() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping build_verbose_with_output_reports_occt_kernel: \
             OCCT unavailable (cfg(has_occt) not set)"
        );
        return;
    }

    let dir = TempDir::new().expect("failed to create temp dir");
    let out_path = dir.path().join("out.step");
    let out_str = out_path.to_str().expect("temp path is not valid UTF-8");

    let bracket = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) =
        common::run_with_args(&["build", &bracket, "-o", out_str, "--verbose"]);

    assert!(
        status.success(),
        "build --verbose with -o must exit 0; stderr: {stderr}"
    );
    assert!(
        stdout.contains("kernel: occt"),
        "stdout must contain 'kernel: occt' for a BRep bracket build; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout must contain 'Wrote' confirming file was written; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("kernel: manifold"),
        "stdout must NOT contain 'kernel: manifold' — Manifold must not hijack BRep builds \
         (G3 regression guard); got:\n{stdout}"
    );
    assert!(
        out_path.exists(),
        "output file must be created at {out_str}"
    );
    assert!(
        out_path.metadata().map(|m| m.len()).unwrap_or(0) > 0,
        "output file must be non-empty"
    );
}

/// `reify build <bracket> --verbose` (NO -o) must:
/// - exit 0 (proves -o is relaxed under --verbose)
/// - stdout contains `kernel: occt`
/// - stdout does NOT contain `kernel: manifold`
#[test]
fn build_verbose_without_output_flag_exits_zero() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping build_verbose_without_output_flag_exits_zero: \
             OCCT unavailable (cfg(has_occt) not set)"
        );
        return;
    }

    let bracket = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = common::run_with_args(&["build", &bracket, "--verbose"]);

    assert!(
        status.success(),
        "build --verbose without -o must exit 0 (the -o requirement is relaxed under \
         --verbose); stderr: {stderr}"
    );
    assert!(
        stdout.contains("kernel: occt"),
        "stdout must contain 'kernel: occt' for a BRep bracket build (no -o); got:\n{stdout}"
    );
    assert!(
        !stdout.contains("kernel: manifold"),
        "stdout must NOT contain 'kernel: manifold' (G3 guard); got:\n{stdout}"
    );
}
