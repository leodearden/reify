//! Task #4679 — kernel-pin version-based enforcement integration tests (arm 3).
//!
//! Exercises `Engine::with_registered_kernels_and_manifest` against the LIVE
//! registry to verify that arm 3 (`KernelVersionMismatch`) fires end-to-end:
//! adapter VERSION const → registry → `kernel_pin_diagnostics` → ERROR.
//!
//! Both tests gate assertions on `reify_kernel_occt::OCCT_AVAILABLE` so the
//! file is non-vacuously green in both real and stub builds.

use reify_config::Manifest;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::Engine;

/// Test A (mismatch): pinning `occt = "0.0.0-mismatch-test"` while the real
/// adapter has `OCCT_KERNEL_VERSION = "7.9.3"` must produce exactly one
/// `Severity::Error` / `DiagnosticCode::KernelVersionMismatch` diagnostic
/// naming "occt".
///
/// In stub builds (OCCT not linked) the kernel is absent from the registry
/// so arm 3 cannot fire — asserts zero `KernelVersionMismatch` errors for
/// "occt" in that branch (non-vacuous contrapositive).
#[test]
fn with_manifest_version_mismatch_produces_error() {
    let manifest = Manifest::from_toml_str("[kernels]\nocct = \"0.0.0-mismatch-test\"\n")
        .expect("valid manifest");

    let (_engine, diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    let mismatch_errors: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::KernelVersionMismatch)
                && d.message.contains("occt")
        })
        .collect();

    if reify_kernel_occt::OCCT_AVAILABLE {
        assert_eq!(
            mismatch_errors.len(),
            1,
            "expected exactly one KernelVersionMismatch error for \"occt\" \
             when pinned version mismatches adapter VERSION; got diags={diags:?}"
        );
    } else {
        assert!(
            mismatch_errors.is_empty(),
            "OCCT is not registered (stub-mode build) — no KernelVersionMismatch \
             for \"occt\" should be present; got diags={diags:?}"
        );
    }
}

/// Test B (match): pinning `occt = OCCT_KERNEL_VERSION` must produce zero
/// `KernelVersionMismatch` diagnostics for "occt" in both build modes.
///
/// References the adapter const directly so the assertion is self-consistent
/// with no literal drift (the const value is 7.9.3 today but the test
/// adapts automatically if the pin is bumped).
#[test]
fn with_manifest_version_match_produces_no_mismatch_error() {
    let pinned_version = reify_kernel_occt::OCCT_KERNEL_VERSION;
    let toml = format!("[kernels]\nocct = \"{pinned_version}\"\n");
    let manifest = Manifest::from_toml_str(&toml).expect("valid manifest");

    let (_engine, diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    let occt_mismatch_errors: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::KernelVersionMismatch)
                && d.message.contains("occt")
        })
        .collect();

    assert!(
        occt_mismatch_errors.is_empty(),
        "pinning occt at its adapter VERSION must not produce a \
         KernelVersionMismatch error; got diags={diags:?}"
    );
}
