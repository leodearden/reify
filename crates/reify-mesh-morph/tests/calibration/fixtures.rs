//! Procedural tetrahedral-mesh generators for the PRD task #13 calibration
//! suite. Each fixture takes a parametric inputs and returns
//! `(VolumeMesh, surface_node_indices: Vec<u32>)` — connectivity is
//! deterministic across parameter values so a morph from `param_0` to
//! `param_1` is a strict node-position update.

/// Sentinel pin used by `tests/calibration.rs`'s smoke test to verify the
/// helper module is wired in before the procedural generators land.
pub const MODULE_OK: bool = true;
