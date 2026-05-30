//! `input_shape(profile, shaper)` dispatcher + Profile/Shaper `Value`
//! marshalling for the trajectory stdlib module (PRD
//! `docs/prds/v0_3/trajectory-input-shaping.md` §5.3, §11 Phase 2 task ζ).
//!
//! Two pieces live here:
//!
//! 1. [`build_train_for_shaper`] — the marshalling boundary that reads a
//!    `Shaper` [`Value::StructureInstance`] (ZVShaper / ZVDShaper / EIShaper /
//!    CascadedShaper) and constructs the corresponding
//!    [`super::impulse_shaper::ImpulseTrain`]. This is where the Hz→rad/s
//!    conversion (`ω_n = 2π·f`) happens — the pure `impulse_shaper` math is
//!    entirely in angular frequency (rad/s). Exposed (via the `reify_stdlib`
//!    re-export) so the engine-side band-sweep robustness metric in
//!    `reify-eval/src/trajectory_ops.rs` can reuse it.
//!
//! 2. [`eval_input_shape`] — the thin `eval_trajectory` dispatch arm that maps
//!    `(profile, shaper)` `Value` arguments to the shaped `Profile`, mirroring
//!    the `gcode_import` precedent (arity / `StructureInstance` arg-reading,
//!    bad-args → [`Value::Undef`]). Full command-waveform resampling to new
//!    waypoints is deferred to task θ; ζ returns a registry-free shaped-Profile
//!    stand-in that echoes the input profile (a valid `Shaper` is still
//!    required — an unrecognised shaper ⇒ `Value::Undef`).

use reify_ir::Value;

use super::impulse_shaper::ImpulseTrain;

/// Build the [`ImpulseTrain`] for a `Shaper` `Value::StructureInstance`.
///
/// STUB (prereq-1): always returns `None`. The real dispatch (ZVShaper /
/// ZVDShaper / EIShaper → impulse convolution; CascadedShaper → fold) is wired
/// in step-4.
///
/// `pub` (re-exported at the crate root as `reify_stdlib::build_train_for_shaper`)
/// so `reify-eval/src/trajectory_ops.rs` can reach it across the crate boundary.
pub fn build_train_for_shaper(_shaper: &Value) -> Option<ImpulseTrain> {
    None
}

/// Evaluate `input_shape(profile, shaper)`.
///
/// STUB (prereq-1): always returns [`Value::Undef`]. The real marshalling
/// (arity / `StructureInstance` guards, `build_train_for_shaper` dispatch,
/// shaped-Profile result) is wired in step-6.
///
/// `#[allow(dead_code)]`: this fn is implemented ahead of the `eval_trajectory`
/// registrar arm that calls it (step-6), so it is written-but-never-read in the
/// prereq-1/step-4 builds. Same "implemented ahead of wiring" suppression the
/// sibling `gcode_import` / `spline` / `impulse_shaper` modules use; removed
/// once the registrar arm lands.
#[allow(dead_code)]
pub(crate) fn eval_input_shape(_args: &[Value]) -> Value {
    Value::Undef
}
