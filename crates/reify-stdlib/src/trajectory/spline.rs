//! Pure-Rust spline math for the trajectory stdlib module.
//!
//! Implements interpolating cubic and quintic B-splines used by
//! `piecewise_polynomial` / `evaluate_profile*` / `profile_duration`.
//!
//! This module has no `reify_types` dependency — all inputs and outputs are
//! plain `f64` / `Vec<f64>`.  Value marshalling lives in `mod.rs`.
