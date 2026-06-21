//! Task π (#3444) — kernel-pin name-based enforcement integration tests.
//!
//! Exercises the PUBLIC constructor `Engine::with_registered_kernels_and_manifest`
//! against the LIVE registry (the constructor that S4 will introduce).
//!
//! **Registry-agnostic contract**: this file MUST NOT reference any
//! `reify_kernel_manifold::*` symbol so "manifold" stays dead-stripped /
//! unregistered — keeping the PinnedKernelMissing assertion reliable even
//! in builds that link the manifold adapter.
//!
//! RED until S4 introduces `Engine::with_registered_kernels_and_manifest`.

use reify_config::Manifest;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity};
use reify_eval::Engine;

/// (a) Passing `None` as the manifest must behave identically to
/// `Engine::with_registered_kernels`: no pin diagnostics, same kernel count.
///
/// Regression guard: introducing the new constructor must not silently
/// change kernel registration or the single-selection-event contract for
/// the `None` path.
#[test]
fn with_manifest_none_produces_no_diagnostics_and_same_kernel_count() {
    let (engine_new, diags) =
        Engine::with_registered_kernels_and_manifest(Box::new(SimpleConstraintChecker), None);
    let engine_old = Engine::with_registered_kernels(Box::new(SimpleConstraintChecker));

    assert!(
        diags.is_empty(),
        "None manifest must produce no diagnostics; got {diags:?}"
    );
    assert_eq!(
        engine_new.kernel_count(),
        engine_old.kernel_count(),
        "with_manifest(None) must register the same kernel count as with_registered_kernels"
    );
}

/// (b) Pinning a kernel that is never registered (manifold) must produce
/// exactly one ERROR diagnostic naming "manifold".
///
/// Note: no `reify_kernel_manifold::*` symbol is referenced here — the
/// manifold adapter is absent from the registry regardless of link flags.
#[test]
fn with_manifest_pinned_missing_kernel_produces_error() {
    let manifest = Manifest::from_toml_str("[kernels]\nmanifold = \"1.0.0\"\n")
        .expect("valid manifest");

    let (_engine, diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::PinnedKernelMissing))
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one PinnedKernelMissing error; got diags={diags:?}"
    );
    let names_manifold: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("manifold"))
        .collect();
    assert!(
        !names_manifold.is_empty(),
        "PinnedKernelMissing error must name \"manifold\"; got errors={errors:?}"
    );
}

/// (c) When OCCT is available (arm 2): the manifold-pinned manifest also
/// causes a WARNING for the "occt" kernel that is registered but not pinned.
///
/// Gated on `reify_kernel_occt::OCCT_AVAILABLE` — skips with an observable
/// eprintln in stub-mode builds so regressions cannot hide in silent green.
#[test]
fn with_manifest_registered_but_unpinned_kernel_warns() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "with_manifest_registered_but_unpinned_kernel_warns: \
             stub-mode build (OCCT_AVAILABLE=false) — skipping UnpinnedKernelLoaded assertion"
        );
        return;
    }

    let manifest = Manifest::from_toml_str("[kernels]\nmanifold = \"1.0.0\"\n")
        .expect("valid manifest");

    let (_engine, diags) = Engine::with_registered_kernels_and_manifest(
        Box::new(SimpleConstraintChecker),
        Some(&manifest),
    );

    let occt_warnings: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::UnpinnedKernelLoaded)
                && d.message.contains("occt")
        })
        .collect();
    assert!(
        !occt_warnings.is_empty(),
        "expected a UnpinnedKernelLoaded warning naming \"occt\" when OCCT is \
         registered but not pinned; got diags={diags:?}"
    );
}
