//! CLI integration tests for `reify check` with a `DFMRule` module that
//! triggers thickness violations (task 4638, lazy-OpenVDB DFM thickness path).
//!
//! ## Gate structure
//!
//! Tests are split by compile-time `cfg(has_openvdb)` and a runtime
//! `reify_kernel_occt::OCCT_AVAILABLE` check:
//!
//! - `#[cfg(has_openvdb)]` + `OCCT_AVAILABLE`: positive assertions —
//!   `W_DFM_MIN_WALL` and `W_DFM_MIN_FEATURE` (Warning fixture) or
//!   `E_DFM_MIN_WALL` and `E_DFM_MIN_FEATURE` (Error fixture) in stderr.
//!   **RED** until step-6 wires `ensure_openvdb_kernel` into `cmd_check`.
//!
//! - `#[cfg(has_openvdb)]` + `!OCCT_AVAILABLE` (runtime): graceful
//!   degradation — no thickness diagnostics, exit 0 (C1/D5).
//!
//! - `#[cfg(not(has_openvdb))]`: stub-mode — no thickness diagnostics,
//!   exit 0 (C1/D5 graceful degradation, `ensure_openvdb_kernel` returns false).
//!
//! These mirror `cli_dfm_overhang.rs` (the sibling DFM routing harness).

mod common;

/// `cfg(has_openvdb)` + OCCT: `reify check fixtures/dfm_thickness.ri` on a
/// DFMRule(Warning) module with a thin-plate subject (box(14mm, 14mm, 1mm))
/// whose wall thickness (~1mm) is below the Subtracting process's
/// `min_feature_size = 2mm` threshold.
///
/// Under has_openvdb+OCCT: exits 0 (Warning severity → non-fatal) and stderr
/// contains both `W_DFM_MIN_WALL` and `W_DFM_MIN_FEATURE`.
///
/// Stub mode (no OCCT at runtime): exits 0, no thickness diagnostics (C1/D5).
///
/// **RED** until step-6 wires `ensure_openvdb_kernel` into `cmd_check`'s DFM
/// arm after `engine.build()` and before `engine.check()` — without that call
/// `realize_solid_sdf` returns `None` → Indeterminate → no diagnostic.
#[cfg(has_openvdb)]
#[test]
fn check_dfm_thickness_emits_w_dfm_min_wall_and_feature_under_openvdb_and_occt() {
    let path = common::fixture_path("dfm_thickness.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // has_openvdb compiled in but OCCT absent at runtime:
        // ensure_openvdb_kernel returns false → realize_solid_sdf None →
        // Indeterminate → no diagnostic, exit 0 (C1/D5).
        assert!(
            status.success(),
            "stub mode (no OCCT): reify check dfm_thickness.ri should exit 0 \
             (thickness Indeterminate without OCCT — C1/D5 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("W_DFM_MIN_WALL"),
            "stub mode (no OCCT): stderr must not contain 'W_DFM_MIN_WALL' \
             (C1: no kernel → realize_solid_sdf None → Indeterminate).\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("W_DFM_MIN_FEATURE"),
            "stub mode (no OCCT): stderr must not contain 'W_DFM_MIN_FEATURE' \
             (C1: no kernel → realize_solid_sdf None → Indeterminate).\nstderr: {stderr}"
        );
        eprintln!(
            "skipping thickness positive assertions: OCCT unavailable \
             (has_openvdb set but OCCT_AVAILABLE=false — OCCT stub-mode build)"
        );
        return;
    }

    // OpenVDB + OCCT both present: the lazy `ensure_openvdb_kernel` call
    // wired by step-6 must have populated the engine with the OpenVDB kernel.
    // The thin plate's wall (~1mm, box 14×14×1mm) is below min_feature_size (2mm) →
    // both W_DFM_MIN_WALL and W_DFM_MIN_FEATURE on stderr.
    // Warning severity → non-fatal → exit 0.
    assert!(
        status.success(),
        "OCCT+OpenVDB mode: reify check dfm_thickness.ri should exit 0 \
         (DFMRule Warning severity → non-fatal → exit 0).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("W_DFM_MIN_WALL"),
        "OCCT+OpenVDB mode: stderr must contain 'W_DFM_MIN_WALL' \
         (box(14×14×1mm) wall ~1mm < 2mm min_feature_size threshold).\n\
         stderr: {stderr}"
    );
    assert!(
        stderr.contains("W_DFM_MIN_FEATURE"),
        "OCCT+OpenVDB mode: stderr must contain 'W_DFM_MIN_FEATURE' \
         (box(14×14×1mm) feature ~1mm < 2mm min_feature_size threshold).\n\
         stderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_MIN_WALL"),
        "OCCT+OpenVDB mode: stderr must NOT contain 'E_DFM_MIN_WALL' \
         (severity is Warning, not Error).\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_MIN_FEATURE"),
        "OCCT+OpenVDB mode: stderr must NOT contain 'E_DFM_MIN_FEATURE' \
         (severity is Warning, not Error).\nstderr: {stderr}"
    );
}

/// `cfg(has_openvdb)` + OCCT: `reify check fixtures/dfm_thickness_error.ri`
/// on a DFMRule(Error) module (box(14mm, 14mm, 1mm) thin plate) exits
/// non-zero and emits both `E_DFM_MIN_WALL` and `E_DFM_MIN_FEATURE` when
/// OpenVDB and OCCT are present.
///
/// Stub mode (no OCCT at runtime): exits 0 (C1/D5 graceful degradation).
///
/// **RED** until step-6 wires `ensure_openvdb_kernel` into `cmd_check`.
#[cfg(has_openvdb)]
#[test]
fn check_dfm_thickness_error_emits_e_dfm_min_wall_and_feature_under_openvdb_and_occt() {
    let path = common::fixture_path("dfm_thickness_error.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub: OCCT absent → no kernel → realize_solid_sdf None →
        // Indeterminate → no diagnostic, exit 0 (C1/D5).
        assert!(
            status.success(),
            "stub mode (no OCCT): reify check dfm_thickness_error.ri should exit 0 \
             (C1/D5 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_DFM_MIN_WALL"),
            "stub mode (no OCCT): stderr must not contain 'E_DFM_MIN_WALL'.\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_DFM_MIN_FEATURE"),
            "stub mode (no OCCT): stderr must not contain 'E_DFM_MIN_FEATURE'.\nstderr: {stderr}"
        );
        eprintln!(
            "skipping thickness error positive assertions: OCCT unavailable \
             (has_openvdb set but OCCT_AVAILABLE=false — OCCT stub-mode build)"
        );
        return;
    }

    // OpenVDB + OCCT: thin plate wall (~1mm, box 14×14×1mm) < min_feature_size (2mm) →
    // E_DFM_MIN_WALL + E_DFM_MIN_FEATURE.
    // DFMSeverity.Error → cmd_check escalates to FAILURE (non-zero exit).
    assert!(
        !status.success(),
        "OCCT+OpenVDB mode: reify check dfm_thickness_error.ri should exit non-zero \
         (DFMSeverity.Error → cmd_check escalates to FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_DFM_MIN_WALL"),
        "OCCT+OpenVDB mode: stderr must contain 'E_DFM_MIN_WALL' \
         (box(14×14×1mm) wall ~1mm < 2mm min_feature_size threshold).\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_DFM_MIN_FEATURE"),
        "OCCT+OpenVDB mode: stderr must contain 'E_DFM_MIN_FEATURE' \
         (box(14×14×1mm) feature ~1mm < 2mm min_feature_size threshold).\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_MIN_WALL"),
        "OCCT+OpenVDB mode: stderr must NOT contain 'W_DFM_MIN_WALL' \
         (severity is Error, not Warning).\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_MIN_FEATURE"),
        "OCCT+OpenVDB mode: stderr must NOT contain 'W_DFM_MIN_FEATURE' \
         (severity is Error, not Warning).\nstderr: {stderr}"
    );
}

/// `cfg(not(has_openvdb))` stub: `reify check fixtures/dfm_thickness.ri` exits
/// 0 and emits no thickness diagnostics when OpenVDB is absent from the build
/// (C1/D5 graceful degradation — `ensure_openvdb_kernel` returns false).
#[cfg(not(has_openvdb))]
#[test]
fn check_dfm_thickness_no_openvdb_no_thickness_diag() {
    let path = common::fixture_path("dfm_thickness.ri");
    let (status, _stdout, stderr) = common::run_subcommand("check", &path);

    eprintln!(
        "has_openvdb not set — stub-mode: ensure_openvdb_kernel no-ops → \
         Indeterminate → no thickness diagnostic (C1/D5)"
    );
    assert!(
        status.success(),
        "stub mode (no has_openvdb): reify check dfm_thickness.ri should exit 0 \
         (C1/D5 graceful degradation — thickness Indeterminate).\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_MIN_WALL"),
        "stub mode (no has_openvdb): stderr must not contain 'W_DFM_MIN_WALL'.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_MIN_FEATURE"),
        "stub mode (no has_openvdb): stderr must not contain 'W_DFM_MIN_FEATURE'.\nstderr: {stderr}"
    );
}

/// `cfg(not(has_openvdb))` stub: `reify check fixtures/dfm_thickness_error.ri`
/// exits 0 and emits no thickness diagnostics when OpenVDB is absent (C1/D5).
#[cfg(not(has_openvdb))]
#[test]
fn check_dfm_thickness_error_no_openvdb_no_thickness_diag() {
    let path = common::fixture_path("dfm_thickness_error.ri");
    let (status, _stdout, stderr) = common::run_subcommand("check", &path);

    eprintln!(
        "has_openvdb not set — stub-mode: ensure_openvdb_kernel no-ops → \
         Indeterminate → no thickness diagnostic, exit 0 (C1/D5)"
    );
    assert!(
        status.success(),
        "stub mode (no has_openvdb): reify check dfm_thickness_error.ri should exit 0 \
         (C1/D5 graceful degradation — thickness Indeterminate).\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_MIN_WALL"),
        "stub mode (no has_openvdb): stderr must not contain 'E_DFM_MIN_WALL'.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_MIN_FEATURE"),
        "stub mode (no has_openvdb): stderr must not contain 'E_DFM_MIN_FEATURE'.\nstderr: {stderr}"
    );
}
