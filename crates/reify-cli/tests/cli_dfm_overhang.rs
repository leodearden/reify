//! CLI integration tests for `reify check` with a `DFMRule` module that
//! triggers an overhang violation (task 4600, process-dfm routing).
//!
//! ## OCCT-gated tests (step-3 RED / step-4 GREEN; amend)
//!
//! Two tests cover the routing and exit-code contract:
//!
//! ### Warning severity (`check_dfm_overhang_emits_one_w_dfm_overhang_under_occt`)
//! `reify check <dfm-file>` must emit exactly one `W_DFM_OVERHANG` diagnostic
//! on stderr (PRD §7.2) and exit 0 when OCCT is present.
//!
//! - Fixture `dfm_overhang.ri`: FDM Adding conformer + `DFMRule(Warning)`.
//!   No `constraint` declarations → Warning is non-fatal → exit 0.
//!
//! ### Error severity (`check_dfm_overhang_error_emits_e_dfm_overhang_under_occt`)
//! `reify check <dfm-file>` must emit exactly one `E_DFM_OVERHANG` diagnostic
//! on stderr and exit non-zero when OCCT is present.
//!
//! - Fixture `dfm_overhang_error.ri`: same geometry, `DFMSeverity.Error`.
//!   `cmd_check` escalates any code-less Error-severity diagnostic to FAILURE.
//!
//! Stub mode (no OCCT): `measure_dfm_rules` C1 no-op guard fires
//! (`default_kernel_name` is None → no kernel → no handles → no diagnostic).
//! Both tests exit 0 and emit nothing (C1 graceful degradation).
//!
//! These mirror `crates/reify-cli/tests/cli_gdt_conformance.rs` and
//! `cli_representation_within.rs` (the sibling routing-change harnesses).

mod common;

/// OCCT-gated: `reify check fixtures/dfm_overhang.ri` on a DFMRule(Warning)
/// module whose box subject has a 90° downward face (violating `max_overhang_angle=0deg`)
/// exits 0 (Warning is non-fatal) and emits exactly one `W_DFM_OVERHANG` on
/// stderr when OCCT is available.
///
/// Stub-mode (no OCCT): the same command exits 0 — `measure_dfm_rules`' C1
/// no-op guard fires (no kernel → no handles → no diagnostic, never a false
/// positive W_DFM_OVERHANG).
///
/// RED until step-4 wires `has_dfm_rule` into `cmd_check`'s kernel-backed
/// `build(ExportFormat::Step)`-before-`check` arm, causing `realization_handles`
/// to be populated and `measure_dfm_rules` to fire.
#[test]
fn check_dfm_overhang_emits_one_w_dfm_overhang_under_occt() {
    let path = common::fixture_path("dfm_overhang.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no diagnostic.
        // Must exit 0 and must NOT emit W_DFM_OVERHANG.
        assert!(
            status.success(),
            "stub mode: reify check dfm_overhang.ri should exit 0 \
             (DFMRule Warning is Indeterminate without OCCT — C1 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("W_DFM_OVERHANG"),
            "stub mode: stderr must not contain 'W_DFM_OVERHANG' \
             (C1: no kernel → measure_dfm_rules no-op → no diagnostic).\n\
             stderr: {stderr}"
        );
        eprintln!(
            "skipping W_DFM_OVERHANG assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the build-before-check → measure_dfm_rules pipeline must
    // fire.  The box bottom face (normal -Z) dips 90° past the build plane;
    // max_overhang_angle=0deg means any downward-pointing face violates.
    // Warning severity → non-fatal → exit 0.  Exactly one W_DFM_OVERHANG on
    // stderr (validated by the passing engine test
    // overhang_warning_rule_emits_w_dfm_overhang in process_dfm_measure.rs).
    assert!(
        status.success(),
        "OCCT mode: reify check dfm_overhang.ri should exit 0 \
         (DFMRule Warning severity → non-fatal → exit 0).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        count,
        1,
        "OCCT mode: stderr must carry exactly one 'W_DFM_OVERHANG' diagnostic \
         (one box bottom face violates the 0-deg overhang limit).\n\
         got {count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_OVERHANG"),
        "OCCT mode: stderr must NOT contain 'E_DFM_OVERHANG' — severity is \
         Warning, not Error.\nstderr: {stderr}"
    );
}

/// OCCT-gated: `reify check fixtures/dfm_with_repr_within.ri` on a module
/// combining a DFMRule(Warning) with a RepresentationWithin assertion.
///
/// This exercises the single kernel-backed arm in `cmd_check` with TWO
/// kernel-dependent constraint kinds present simultaneously:
///
/// - `has_dfm_rule = true`  → `engine.build(ExportFormat::Step)` must fire
///   to populate `realization_handles` for `measure_dfm_rules`.
/// - `has_representation_within = true` → `set_capture_repr_tol(true)` +
///   `tessellate_realizations()` must fire to populate `achieved_repr_tol`.
///
/// Both side effects touch DISJOINT engine maps, so the combined module gets
/// correct verdicts for both kinds in one `check()` call.
///
/// Under OCCT: exactly one `W_DFM_OVERHANG` on stderr (box bottom face dips
/// 90° with max_overhang_angle=0deg); RepresentationWithin Satisfied (1m sphere
/// at #precision(0.1mm) ≪ 1mm bound); no `VIOLATED` on stdout; exit 0.
///
/// Stub mode (no OCCT): exit 0, no `W_DFM_OVERHANG`, no `VIOLATED` (C1).
#[test]
fn check_dfm_plus_repr_within_combined_arm() {
    let path = common::fixture_path("dfm_with_repr_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    // Both OCCT and stub: must exit 0.
    // DFM Warning + RepresentationWithin (Satisfied or Indeterminate) are non-fatal.
    assert!(
        status.success(),
        "combined DFM+RepresentationWithin module should exit 0 \
         (DFM Warning + Satisfied Repr → non-fatal).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // RepresentationWithin must be Satisfied (not VIOLATED) in both modes.
    assert!(
        !stdout.contains("VIOLATED"),
        "RepresentationWithin should be Satisfied (not VIOLATED) in both OCCT \
         and stub mode.\nstdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no DFM diagnostic;
        // tessellate skipped → RepresentationWithin Indeterminate.
        assert!(
            !stderr.contains("W_DFM_OVERHANG"),
            "stub mode: must not emit W_DFM_OVERHANG (C1 no-op).\nstderr: {stderr}"
        );
        eprintln!(
            "skipping DFM+RepresentationWithin combined OCCT assertions: \
             OCCT unavailable (stub-mode build)"
        );
        return;
    }

    // OCCT: build() + tessellate() sequence must both fire in the combined arm.

    // DFM: box bottom face (normal -Z, 90°) violates max_overhang_angle=0deg →
    // exactly one W_DFM_OVERHANG (Warning severity → non-fatal).
    let dfm_count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        dfm_count,
        1,
        "OCCT mode: exactly one W_DFM_OVERHANG expected from the DFMRule.\n\
         got {dfm_count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_OVERHANG"),
        "OCCT mode: no E_DFM_OVERHANG (severity is Warning, not Error).\n\
         stderr: {stderr}"
    );
}

/// OCCT-gated: `reify check fixtures/dfm_overhang_error.ri` on a DFMRule(Error)
/// module whose box subject has a 90° downward face (violating `max_overhang_angle=0deg`)
/// exits non-zero (Error severity → fatal → non-zero exit) and emits exactly one
/// `E_DFM_OVERHANG` on stderr when OCCT is available.
///
/// Stub-mode (no OCCT): the same command exits 0 — `measure_dfm_rules`' C1
/// no-op guard fires (no kernel → no handles → no diagnostic, never a false
/// positive E_DFM_OVERHANG).
#[test]
fn check_dfm_overhang_error_emits_e_dfm_overhang_under_occt() {
    let path = common::fixture_path("dfm_overhang_error.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no diagnostic.
        // Must exit 0 and must NOT emit E_DFM_OVERHANG.
        assert!(
            status.success(),
            "stub mode: reify check dfm_overhang_error.ri should exit 0 \
             (DFMRule Error is Indeterminate without OCCT — C1 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_DFM_OVERHANG"),
            "stub mode: stderr must not contain 'E_DFM_OVERHANG' \
             (C1: no kernel → measure_dfm_rules no-op → no diagnostic).\n\
             stderr: {stderr}"
        );
        eprintln!(
            "skipping E_DFM_OVERHANG assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the build-before-check → measure_dfm_rules pipeline must
    // fire.  DFMSeverity.Error → E_DFM_OVERHANG diagnostic → cmd_check escalates
    // any code-less Error-severity diagnostic to FAILURE.
    assert!(
        !status.success(),
        "OCCT mode: reify check dfm_overhang_error.ri should exit non-zero \
         (DFMSeverity.Error → cmd_check escalates to FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let count = stderr.matches("E_DFM_OVERHANG").count();
    assert_eq!(
        count,
        1,
        "OCCT mode: stderr must carry exactly one 'E_DFM_OVERHANG' diagnostic \
         (one box bottom face violates the 0-deg overhang limit).\n\
         got {count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_OVERHANG"),
        "OCCT mode: stderr must NOT contain 'W_DFM_OVERHANG' — severity is \
         Error, not Warning.\nstderr: {stderr}"
    );
}
