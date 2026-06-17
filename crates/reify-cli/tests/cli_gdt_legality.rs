//! End-to-end B7 tests for the GD&T check-time legality diagnostics (task 4475 β).
//!
//! Exercises `reify check` over committed examples/tolerancing/ fixtures:
//! - `gdt_illegal_modifier.ri`  — Flatness(MMC): error on stderr + non-zero exit.
//! - `gdt_legality_rfs.ri`      — all-legal / RFS callouts: silent + exit 0.
//! - `gdt_removed_2018.ri`      — Concentricity: removed-2018 warning on stderr.
//!
//! Step-9 RED: fails because fixtures and cmd_check exit-code wiring are absent.
//! Step-10 GREEN: add fixtures + wire GdtIllegalModifier → non-zero exit.

mod common;

/// B7-A: a `Flatness(material_condition: MMC, ...)` callout must produce an error
/// on stderr and cause `reify check` to exit non-zero.
///
/// Fails until the fixture exists and cmd_check wires GdtIllegalModifier → failure.
#[test]
fn check_gdt_illegal_modifier_exits_failure_with_error_on_stderr() {
    let path = common::example_path("tolerancing/gdt_illegal_modifier.ri");
    let (status, _stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        !status.success(),
        "reify check must exit non-zero for a GdtIllegalModifier callout.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr must contain 'error:' for a GdtIllegalModifier diagnostic.\nstderr: {stderr}"
    );
    // The diagnostic message must mention the illegal modifier concept.
    assert!(
        stderr.contains("MMC") || stderr.contains("LMC") || stderr.contains("material"),
        "stderr must reference the illegal material condition modifier.\nstderr: {stderr}"
    );
}

/// B7-B: an all-RFS / all-legal fixture must produce no GD&T legality errors and
/// exit 0.
///
/// Fails until the fixture exists (the exit-code check passes once the error is absent).
#[test]
fn check_gdt_legality_rfs_exits_success_with_no_error() {
    let path = common::example_path("tolerancing/gdt_legality_rfs.ri");
    let (status, _stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check must exit 0 for an all-RFS / all-legal GD&T fixture.\nstderr: {stderr}"
    );
    // No GdtIllegalModifier error must appear.
    assert!(
        !stderr.contains("GdtIllegalModifier") && !stderr.contains("RFS-only"),
        "stderr must not contain a GdtIllegalModifier error for an all-legal fixture.\nstderr: {stderr}"
    );
}

// ── B7 --purpose path wiring (task 4589 step-3 RED / step-4 GREEN) ───────────

/// B7-D: `reify check --purpose mfg_ready=FlatnessMmcPurpose` over a fixture
/// with an illegal MMC modifier must exit non-zero, emit an error on stderr,
/// and still report the purpose constraint as satisfied in stdout.
///
/// The purpose constraint is `subject.width > 0mm` (default 80mm → satisfied),
/// so the ONLY source of a non-zero exit is the GD&T escalation, not a constraint
/// violation or a purpose-activation failure.
///
/// RED: the `--purpose` branch does not call `run_gdt_check_passes` yet, so it
/// exits 0 with no GdtIllegalModifier diagnostic (task 4589 step-4 fixes this).
#[test]
fn check_purpose_gdt_illegal_modifier_exits_failure() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=FlatnessMmcPurpose",
        &common::fixture_path("gdt_illegal_modifier_purpose.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --purpose must exit non-zero when a GdtIllegalModifier is present.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr must contain 'error:' for the GdtIllegalModifier diagnostic.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("MMC") || stderr.contains("material"),
        "stderr must reference the illegal material condition modifier.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // The purpose constraint itself is satisfied — the exit is ONLY the GDT escalation.
    assert!(
        stdout.contains("purpose:mfg_ready@"),
        "stdout must contain the purpose-injected constraint id prefix 'purpose:mfg_ready@'.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout must report 'All constraints satisfied.' (the purpose constraint is satisfied).\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// B7-E (over-escalation guard): `reify check --purpose mfg_ready=AllLegalGdtPurpose`
/// over an all-legal GDT fixture must exit 0 with no GdtIllegalModifier error.
///
/// Ensures that adding the GDT pass to the `--purpose` branch does not cause
/// false positives for legal callouts (Position/MMC/Cylindrical, StraightnessOfAxis/MMC).
///
/// This test must stay GREEN across step-4.
#[test]
fn check_purpose_gdt_legality_rfs_exits_success() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=AllLegalGdtPurpose",
        &common::fixture_path("gdt_legality_rfs_purpose.ri"),
    ]);

    assert!(
        status.success(),
        "reify check --purpose must exit 0 for an all-legal GDT fixture.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:") && !stderr.contains("RFS-only"),
        "stderr must not contain a GdtIllegalModifier error for an all-legal GDT fixture.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// B7-F (warning non-fatal on `--purpose` path): `reify check --purpose` over a
/// fixture with a Concentricity callout must exit 0 (warnings are non-fatal) while
/// still emitting a GdtRemoved2018 warning on stderr.
///
/// Mirrors B7-C for the `--purpose` branch, confirming that `run_gdt_check_passes`
/// wires the full legality pass (including removed-characteristic detection) without
/// escalating non-error diagnostics to FAILURE.
#[test]
fn check_purpose_gdt_removed_2018_warning_nonfatal() {
    let (status, _stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=ConcentricityPurpose",
        &common::fixture_path("gdt_removed_2018_purpose.ri"),
    ]);

    assert!(
        status.success(),
        "reify check --purpose must exit 0 for a GdtRemoved2018 warning (warnings are non-fatal).\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("warning:"),
        "stderr must contain 'warning:' for the GdtRemoved2018 diagnostic.\nstderr: {stderr}"
    );
    // The warning must name at least one replacement characteristic, mirroring B7-C.
    assert!(
        stderr.contains("Position") || stderr.contains("Profile") || stderr.contains("Runout"),
        "GdtRemoved2018 warning must name replacement characteristics.\nstderr: {stderr}"
    );
}

/// B7-C: a `Concentricity(...)` callout must produce a removed-2018 warning on
/// stderr. The exit code is 0 (warnings are non-fatal).
///
/// Fails until the fixture exists.
#[test]
fn check_gdt_removed_2018_emits_warning_on_stderr() {
    let path = common::example_path("tolerancing/gdt_removed_2018.ri");
    let (status, _stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check must exit 0 for a GdtRemoved2018 warning (warnings are non-fatal).\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("warning:"),
        "stderr must contain 'warning:' for a GdtRemoved2018 diagnostic.\nstderr: {stderr}"
    );
    // The warning must mention at least one replacement characteristic.
    assert!(
        stderr.contains("Position") || stderr.contains("Profile") || stderr.contains("Runout"),
        "GdtRemoved2018 warning must name replacement characteristics.\nstderr: {stderr}"
    );
}
