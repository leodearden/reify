//! End-to-end B7 tests for the GD&T check-time legality diagnostics (task 4475 β).
//!
//! Exercises `reify check` over committed examples/tolerancing/ fixtures:
//! - `gdt_illegal_modifier.ri`  — Flatness(MMC): error on stderr + non-zero exit.
//! - `gdt_legality_rfs.ri`      — all-legal / RFS callouts: silent + exit 0.
//! - `gdt_removed_2018.ri`      — Concentricity: removed-2018 warning on stderr.
//!
//! Step-9 RED: fails because fixtures and cmd_check exit-code wiring are absent.
//! Step-10 GREEN: add fixtures + wire GdtIllegalModifier → non-zero exit.

use crate::common;

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

// ── task 5748 / esc-5748-7: no double-print on sub-path (c)'s build branch ───

/// D2 invariant lock for `cmd_check` sub-path (c) (`--purpose`) on a
/// GEOMETRY-BEARING module: a `GdtIllegalModifier` error must be printed
/// EXACTLY ONCE.
///
/// Task 5748's D1 item 2 routes a geometry-bearing `--purpose` module through
/// `realize_for_check` instead of `eval`. That realization internally calls
/// `Engine::check`, which already ends with
/// `diagnostics.extend(self.run_gdt_check_passes(module, &values))`
/// (engine_constraints.rs) — so the legality pass has ALREADY contributed its
/// diagnostics by the time `cmd_check` folds in its own
/// `engine.run_gdt_check_passes(...)` call. `run_gdt_check_passes` is a pure
/// function of `(module, values)`, so a bare `extend` there yields two
/// byte-identical error lines, violating D2's "no diagnostic that would appear
/// in `check()`'s own diagnostics today is ever printed twice".
///
/// The pre-existing `check_purpose_gdt_illegal_modifier_exits_failure` above
/// cannot catch this: `contains("error:")` / `contains("MMC")` are both still
/// true when the line appears twice.
///
/// Note the assertion is on the ERROR TEXT, not on a substring like "MMC" that
/// also occurs in the fixture-independent parts of the report.
#[test]
fn check_purpose_gdt_illegal_modifier_on_geometry_module_prints_once() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=FlatnessMmcPurposeGeometry",
        &common::fixture_path("gdt_illegal_modifier_purpose_geometry.ri"),
    ]);

    let needle = "`Flatness` is an RFS-only tolerance characteristic";
    assert!(
        stderr.contains(needle),
        "the GdtIllegalModifier error must still reach stderr on the geometry-bearing \
         `--purpose` path.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let occurrences = stderr.matches(needle).count();
    assert_eq!(
        occurrences,
        1,
        "the GD&T legality pass contributes its diagnostics ONCE via the realization's \
         internal `Engine::check` and once more via `cmd_check`'s own fold-in; D2 \
         requires exactly one printed line, which `strip_diagnostics_reproduced_by` \
         achieves by withdrawing the realization's copy before the fold-in — got \
         {occurrences}.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The GdtIllegalModifier escalation still fires (unchanged by the merge —
    // membership is a union, so no exit code can move).
    assert!(
        !status.success(),
        "reify check --purpose must still exit non-zero for a GdtIllegalModifier.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// Two DISTINCT illegal callouts of the same characteristic must print TWO
/// lines on the geometry-bearing `--purpose` path (task 5748 amendment,
/// reviewer_comprehensive `correctness` suggestion).
///
/// `illegal_modifier_error` formats its message from the characteristic name
/// alone and anchors the location in a label, so two `Flatness`+MMC callouts
/// are two byte-identical messages at two different spans. Sub-path (c)'s
/// `used_build` arm is the only place where the build list is BOTH the
/// self-dedup subject and the merge seed, so a text-only dedup key collapsed
/// them into one printed line — silently losing the second callout's location,
/// which is the ONLY thing that distinguishes the two reports.
///
/// This is the counterpart of
/// `check_purpose_gdt_illegal_modifier_on_geometry_module_prints_once`: that
/// test pins that the legality pass's two RUNS (the realization's internal
/// `Engine::check` and `cmd_check`'s own fold-in) yield one line; this one pins
/// that two distinct CALLOUTS yield two. Both must hold at once, and no local
/// dedup key can separate the two cases — the span is deliberately NOT part of
/// `DiagKey` (measured: the duplicated `mirror(...)` 'ox' error carries two
/// different spans for one problem). That is why `cmd_check` withdraws the
/// realization's copy of the pass wholesale via
/// `strip_diagnostics_reproduced_by` and lets its own run be the single source,
/// instead of deduping the two runs against each other.
///
/// Kernel-independent: the legality pass is a static lint over post-eval
/// values, so it produces the same two callouts with or without OCCT.
#[test]
fn check_purpose_gdt_two_illegal_callouts_on_geometry_module_print_twice() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=TwoFlatnessMmcPurposeGeometry",
        &common::fixture_path("gdt_two_illegal_modifiers_purpose_geometry.ri"),
    ]);

    let needle = "`Flatness` is an RFS-only tolerance characteristic";
    let occurrences = stderr.matches(needle).count();
    assert_eq!(
        occurrences, 2,
        "the module declares TWO illegal Flatness callouts; each is a separate \
         report anchored at its own span, so both must reach stderr. Collapsing \
         them to one is not deduplication — it is dropping a finding.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // The escalation still fires, exactly as for the single-callout twin.
    assert!(
        !status.success(),
        "reify check --purpose must still exit non-zero for a GdtIllegalModifier.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
