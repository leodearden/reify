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
