//! CLI integration tests for `reify check` with a geometric `Conforms`
//! constraint — the η GD&T geometric-conformance pass (task 4480 η, PRD v0_6
//! C3/C5).
//!
//! ## OCCT-gated tests (step-15 RED / step-16 GREEN)
//!
//! A `Conforms` callout with an EXPLICIT `actual` binding is measured at check
//! time: `Engine::measure_gdt_conformance` runs a `GeometryQuery::MaxDeviation`
//! of the realized `actual` geometry against the tolerance callout's nominal
//! `feature`, and overrides the scalar verdict with a geometric one.
//!
//! - B1 `gdt_conformance_violated.ri`: the `actual` cube is shifted +0.5 mm from
//!   the nominal — a 0.5 mm deviation that EXCEEDS the 0.1 mm tolerance zone.
//!   Under OCCT, `reify check` must exit non-zero and print "VIOLATED", with the
//!   measured-magnitude + zone-width diagnostic on stderr.
//! - B2 `gdt_conformance_satisfied.ri`: the `actual` cube is coincident with the
//!   nominal (0 mm shift) — a ~0 mm deviation WITHIN the 0.1 mm zone. Exits 0,
//!   Satisfied, never "VIOLATED".
//! - B3 stub mode (no OCCT): both files exit 0 with the conformance check
//!   `Indeterminate` — no kernel to measure deviation → C1 graceful degradation,
//!   never a false Violated.
//!
//! These mirror `crates/reify-cli/tests/cli_representation_within.rs` (the
//! sibling RepresentationWithin OCCT-gated harness).

mod common;

/// Extract the measured deviation (in mm) from the η VIOLATED diagnostic:
/// "Conforms VIOLATED: measured deviation 0.5000 mm exceeds the 0.1000 mm …".
fn parse_deviation_mm(stderr: &str) -> Option<f64> {
    let after = stderr.split("measured deviation ").nth(1)?;
    let num = after.split(" mm").next()?;
    num.trim().parse::<f64>().ok()
}

/// B1 (signal): `reify check examples/gdt_conformance_violated.ri` on a cube
/// whose as-built `actual` is shifted +0.5 mm from the nominal `feature` (a
/// 0.5 mm deviation) against a 0.1 mm tolerance zone exits non-zero (FAILURE)
/// and prints "VIOLATED" when OCCT is available, with the measured-magnitude +
/// zone-width diagnostic on stderr.
///
/// Stub-mode (no OCCT): the same command exits 0 — the conformance check is
/// `Indeterminate` (no kernel → deviation cannot be measured → C1 graceful
/// degradation, never a false Violated).
#[test]
fn check_gdt_conformance_violated_under_occt() {
    let path = common::example_path("gdt_conformance_violated.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → Indeterminate → exit 0, never "VIOLATED".
        assert!(
            status.success(),
            "stub mode: reify check gdt_conformance_violated.ri should exit 0 \
             (Conforms is Indeterminate without OCCT — C1 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stdout.contains("VIOLATED"),
            "stub mode: stdout must not contain 'VIOLATED' \
             (Indeterminate, not Violated).\nstdout: {stdout}"
        );
        eprintln!(
            "skipping VIOLATED assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the build-before-check → measure_gdt_conformance pipeline
    // must fire. 0.5 mm deviation ≫ 0.1 mm zone → Violated → FAILURE.
    assert!(
        !status.success(),
        "OCCT mode: reify check gdt_conformance_violated.ri should exit non-zero \
         (0.5 mm deviation exceeds the 0.1 mm zone → Violated → FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "OCCT mode: stdout must contain 'VIOLATED' (geometric Conforms fires: \
         0.5 mm measured deviation exceeds the 0.1 mm tolerance zone).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // The η magnitude/zone diagnostic lands on stderr.
    assert!(
        stderr.contains("measured deviation") && stderr.contains("0.1000 mm tolerance zone"),
        "OCCT mode: stderr must carry the η diagnostic with the measured \
         magnitude and the 0.1000 mm zone width.\nstderr: {stderr}"
    );
    // Loosely pin the magnitude to the constructed 0.5 mm deviation, tolerant of
    // f32-quantization on planar faces (≤ 0.01 mm; max_deviation_query.rs S1).
    let dev_mm = parse_deviation_mm(&stderr)
        .unwrap_or_else(|| panic!("could not parse deviation from diagnostic.\nstderr: {stderr}"));
    assert!(
        (dev_mm - 0.5).abs() < 0.05,
        "OCCT mode: measured deviation should be ~0.5 mm (constructed), got {dev_mm} mm.\n\
         stderr: {stderr}"
    );
}

/// B2: `reify check examples/gdt_conformance_satisfied.ri` on a cube whose
/// as-built `actual` is coincident with the nominal `feature` (0 mm shift, a
/// ~0 mm deviation) against a 0.1 mm tolerance zone exits 0 and never prints
/// "VIOLATED".
///
/// Exit 0 + no "VIOLATED" holds in BOTH modes: under OCCT the verdict is
/// Satisfied; in stub mode it is Indeterminate (C1). Under OCCT we additionally
/// assert the Satisfied headline.
#[test]
fn check_gdt_conformance_satisfied_exits_zero() {
    let path = common::example_path("gdt_conformance_satisfied.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check gdt_conformance_satisfied.ri should exit 0 \
         (~0 mm deviation within the 0.1 mm zone → Satisfied; Indeterminate \
         without OCCT — either way exit 0).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout must not contain 'VIOLATED' (a coincident twin is within the \
         tolerance zone).\nstdout: {stdout}\nstderr: {stderr}"
    );

    if reify_kernel_occt::OCCT_AVAILABLE {
        // Under OCCT the deviation is measured (~0 mm) → Satisfied headline.
        assert!(
            stdout.contains("All constraints satisfied"),
            "OCCT mode: stdout should report 'All constraints satisfied' \
             (geometric Conforms measured within the zone).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
    }
}
