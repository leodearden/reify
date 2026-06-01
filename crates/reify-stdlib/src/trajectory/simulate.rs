//! Pure-Rust forward-pass simulator — `simulate_trajectory_core`.
//!
//! Implements the pure-Rust f64 CORE of the forward-pass simulator described
//! in `docs/prds/v0_3/trajectory-input-shaping.md` §6.1, §11 Phase 3 θ.
//!
//! # Scope / pure-Rust layer
//!
//! Like the sibling submodules (`spline.rs`, `sampling.rs`, `impulse_shaper.rs`,
//! `gcode_import.rs`) and `modal/transient.rs`, this module is a **pure-Rust
//! f64 layer** — all inputs and outputs are plain Rust types with no
//! `reify_ir::Value` dependency.
//!
//! The following are **deferred** to the downstream Value-wiring task (π
//! ComputeNode trampoline / dedicated dispatch):
//! - `eval_trajectory` match-arm dispatch wiring
//! - Value marshalling (EndEffectorTrack Value construction)
//! - FK-snapshot integration (Value-level `snapshot`/`end_effector_pose`)
//! - `.ri` accessor bodies (`end_effector_track`, `deviation_from_nominal`,
//!   `peak_deviation`) — currently stub TODO(θ) bodies in trajectory.ri
//!
//! # Dead-code suppression
//!
//! The public(crate) API here is tested at the pure-function level ahead of
//! the π consumer that will wire it to the Value layer.  Suppress the lint
//! rather than adding a premature marshalling layer.
#![allow(dead_code)]

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── step-1: RED — modal_aware_dt ─────────────────────────────────────────

    /// (a) 0.5/f_max governs (high f_max, long duration).
    #[test]
    fn modal_aware_dt_governed_by_frequency() {
        // f_max=100 Hz → 0.5/100=0.005 s; duration/1000 = 10.0/1000 = 0.01 s
        // → min is 0.005
        let dt = modal_aware_dt(&[50.0, 100.0], 10.0);
        assert!((dt - 0.005).abs() < 1e-14, "expected 0.005, got {dt}");
    }

    /// (b) duration/1000 governs (low f_max, short duration).
    #[test]
    fn modal_aware_dt_governed_by_duration() {
        // f_max=1 Hz → 0.5/1=0.5 s; duration/1000 = 0.1/1000 = 0.0001 s
        // → min is 0.0001
        let dt = modal_aware_dt(&[0.5, 1.0], 0.1);
        assert!((dt - 0.0001).abs() < 1e-18, "expected 0.0001, got {dt}");
    }

    /// (c) single-mode input.
    #[test]
    fn modal_aware_dt_single_mode() {
        // f_max=20 Hz → 0.5/20=0.025 s; duration/1000 = 5.0/1000=0.005 s
        // → min is 0.005
        let dt = modal_aware_dt(&[20.0], 5.0);
        assert!((dt - 0.005).abs() < 1e-16, "expected 0.005, got {dt}");
    }

    /// (d) empty modes → falls back to duration/1000.
    #[test]
    fn modal_aware_dt_empty_modes_falls_back_to_duration() {
        let dt = modal_aware_dt(&[], 2.0);
        assert!((dt - 0.002).abs() < 1e-16, "expected 0.002, got {dt}");
    }

    /// (e) non-positive/non-finite frequencies are ignored; result stays finite > 0.
    #[test]
    fn modal_aware_dt_ignores_bad_frequencies() {
        // Only valid freq is 10.0 Hz → 0.5/10=0.05; duration/1000=1.0/1000=0.001
        // → min is 0.001
        let dt = modal_aware_dt(&[0.0, -5.0, f64::NAN, f64::INFINITY, 10.0], 1.0);
        assert!(dt > 0.0 && dt.is_finite(), "dt must be finite and positive, got {dt}");
        assert!((dt - 0.001).abs() < 1e-15, "expected 0.001, got {dt}");
    }
}
