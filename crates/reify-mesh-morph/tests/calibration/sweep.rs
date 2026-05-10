//! Sweep runner + `SweepReport` + materially-better-rule helper for the
//! PRD task #13 calibration suite. The runner drives a fixture across a
//! parameter range, runs `elasticity_morph` against the procedural
//! target-mesh's surface vertices, and compares morph quality against a
//! from-scratch remesh using `quality_check`.

/// Sentinel pin used by `tests/calibration.rs`'s smoke test to verify the
/// helper module is wired in before the sweep runner lands.
pub const MODULE_OK: bool = true;
