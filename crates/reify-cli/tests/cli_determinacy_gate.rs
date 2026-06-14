//! CLI integration gate for PRD §5 BT7 — `RepresentationWithin` Satisfied + zero
//! exit (consumer-boundary half not shipped by γ, task-4199).
//!
//! ## BT7 in the PRD §5 collection
//!
//! This file **owns BT7** (RepresentationWithin Satisfied + zero exit, C3) in the
//! PRD §5 BT1–BT9 suite.  For the full canonical cross-reference table mapping each
//! BT to its home file and test function, see:
//! `crates/reify-eval/tests/determinacy_integration_gate.rs` — the collected-view
//! file that δ (task-4200) adds to close the α↔γ integration seam.
//!
//! ## Why BT7 belongs here (not in γ)
//!
//! Task γ (task-4199) shipped the CLI Violated case in `cli_representation_within.rs`
//! (`check_representation_within_violated_under_occt`).  The Satisfied/zero-exit
//! case was left as a genuine gap — γ's CLI test file did not include it.
//! Task δ (task-4200) closes this gap by adding a fixture with fine precision so
//! the RepresentationWithin assertion is Satisfied (or Indeterminate under stub),
//! and asserting `exit 0` + `no "VIOLATED"` in stdout.
//!
//! The assertion is dual-mode-robust:
//! - Under OCCT: `#precision(0.1mm)` sphere deviation ≪ `1mm` bound → Satisfied → exit 0.
//! - Without OCCT: realization cannot run → `achieved_repr_tol` map stays empty →
//!   Indeterminate (C1 graceful degradation) → exit 0.
//!
//! Both modes satisfy the gate invariants: `exit 0` AND stdout does NOT contain "VIOLATED".

mod common;

/// BT7 consumer-boundary gate (PRD §5 BT7 / C3): `reify check` on a fine-precision
/// sphere with a `RepresentationWithin(subject, 1mm)` assertion must exit 0 and
/// must not print "VIOLATED" in stdout.
///
/// This is the missing consumer-boundary half of the Satisfied branch:
/// - Under OCCT: `#precision(0.1mm)` sphere → sampled deviation ≪ 1 mm → Satisfied → exit 0.
/// - Under stub (no OCCT): realization cannot run → Indeterminate (C1) → exit 0.
///
/// Numbers (`1 m` sphere, `0.1mm` deflection, `1mm` bound) mirror the shipped,
/// passing engine test `bt7_fine_sphere_tight_bound_yields_satisfied` in
/// `crates/reify-eval/tests/representation_within_assertion.rs`, so the numeric
/// premise is grounded in a validated reference result, not a guessed threshold.
///
/// RED until step-2 creates `fixtures/representation_within_satisfied.ri` — without
/// it `reify check` cannot load the file and exits non-zero, failing the assertion.
#[test]
fn check_representation_within_satisfied_exits_zero() {
    let path = common::fixture_path("representation_within_satisfied.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "BT7: reify check representation_within_satisfied.ri should exit 0.\n\
         Under OCCT: fine precision → deviation ≪ 1mm → Satisfied → exit 0.\n\
         Under stub: no realization → Indeterminate (C1) → exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "BT7: stdout must not contain 'VIOLATED' (fine precision → Satisfied or \
         Indeterminate, never Violated).\nstdout: {stdout}"
    );

    // Mode-gated assertion: confirm the RepresentationWithin constraint was
    // actually exercised (not just that the file loaded and didn't crash).
    //
    // Under stub (no OCCT): the achieved_repr_tol map stays empty → the
    // SphereCheck RepresentationWithin constraint is Indeterminate (C1 graceful
    // degradation).  `reify check` prints "  INDETERMINATE SphereCheck#constraint[0]"
    // to stdout.  If the RepresentationWithin constraint were removed from the
    // fixture, there would be no Indeterminate entry and "INDETERMINATE" would
    // not appear — so this assertion catches that regression.
    //
    // Under OCCT: fine-precision sphere deviation ≪ 1 mm → Satisfied.
    // `reify check` prints "All constraints satisfied." in the summary line.
    if reify_kernel_occt::OCCT_AVAILABLE {
        assert!(
            stdout.contains("All constraints satisfied."),
            "BT7 OCCT mode: stdout should contain 'All constraints satisfied.' \
             (fine sphere → RepresentationWithin Satisfied → exit 0 + no VIOLATED).\n\
             stdout: {stdout}"
        );
    } else {
        assert!(
            stdout.contains("INDETERMINATE"),
            "BT7 stub mode: stdout should contain 'INDETERMINATE' — RepresentationWithin \
             is Indeterminate without OCCT (C1 graceful degradation).  If this assertion \
             fails after removing RepresentationWithin from the fixture, the gate \
             correctly rejects the regression.\nstdout: {stdout}"
        );
    }
}
