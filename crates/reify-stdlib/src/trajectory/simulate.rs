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
