//! PRD task #13 — quality-threshold calibration regression-guard suite.
//!
//! This integration-test binary exercises the (`elasticity_morph` +
//! `quality_check`) pair against three procedural parametric fixtures
//! (box, plate-with-hole, L-bracket) and asserts the "morph rejected only
//! when from-scratch is materially better" rule that calibrates
//! [`MorphOptions::default()`].
//!
//! Helper modules are pulled in via `#[path = …]` so Cargo does NOT compile
//! them as standalone integration-test binaries — only this file is. See
//! Cargo book §"Integration tests" and the plan's design-decisions for
//! background.
//!
//! Provenance: task #2950.

#[path = "calibration/fixtures.rs"]
mod fixtures;

#[path = "calibration/sweep.rs"]
mod sweep;

/// Smoke test: helper modules are wired in correctly and expose the
/// `MODULE_OK` sentinel constants. Fails to compile while either helper
/// module is missing — pins the file layout the task spec requires.
#[test]
fn calibration_helper_modules_are_wired_in() {
    assert!(fixtures::MODULE_OK);
    assert!(sweep::MODULE_OK);
}
