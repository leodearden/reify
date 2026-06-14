//! CLI integration gate for PRD §5 BT7 — `RepresentationWithin` Satisfied + zero
//! exit (consumer-boundary half not shipped by γ, task-4199).
//!
//! ## PRD §5 BT1–BT9 canonical test homes
//!
//! | BT   | Description                                          | Home file                                       |
//! |------|------------------------------------------------------|-------------------------------------------------|
//! | BT1  | golden-equivalence (A1)                              | `purpose_compile_tests.rs:2076`                 |
//! | BT2  | AllGeometryDetermined (A2)                           | `purpose_compile_tests.rs:1992`                 |
//! | BT3  | scope / arg diagnostics (A3)                         | `purpose_compile_tests.rs:2162/2214/2252`       |
//! | BT4  | intrinsic CLI Satisfied/Violated (A4)                | `cli_determinacy_intrinsics.rs`                 |
//! | BT5  | deviation monotonicity (B1/B2)                       | `achieved_repr_tol.rs`                          |
//! | BT6  | RW Violated + non-zero exit (C3)                     | `representation_within_assertion.rs` + `cli_representation_within.rs` |
//! | BT7  | **RW Satisfied + zero exit (C3)**                    | **this file** (`cli_determinacy_gate.rs`)       |
//! | BT8  | RW Indeterminate (C1)                                | `representation_within_assertion.rs` bt8 + `cli_representation_within.rs` |
//! | BT9  | budget regression (C2)                               | `representation_within_assertion.rs` c2 + `tolerance_scope.rs` / `tolerance_combine.rs` |
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
}
