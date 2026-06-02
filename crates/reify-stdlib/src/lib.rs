// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` for once-per-session OOB-warning
// suppression, which is excluded from `PartialEq`/`Ord`/`Hash`/`content_hash`
// but still triggers the `mutable_key_type` lint on every `BTreeMap<Value, _>`.
#![allow(clippy::mutable_key_type)]

use reify_ir::Value;

mod helpers;

/// Public re-export of the shared complex-phase helper, so reify-expr's method
/// path can call the same implementation used by the stdlib builtin path.
pub use helpers::complex_phase;

/// Public re-export of the tolerance stack-up error classifier.
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push `Severity::Error` diagnostics into the `EvalContext` sink when a
/// stackup builtin returns `Value::Undef`.
pub use stackup::diagnose as stackup_diagnose;

/// Public re-export of the multi-load-case FEA error classifier.
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push `Severity::Error` diagnostics into the `EvalContext` sink when
/// `linear_combine` returns `Value::Undef` for a task-#10 failure mode
/// (empty/unknown-case weights or incompatible meshes).
pub use fea::diagnose as fea_diagnose;

/// Public re-export of the affine-constructor diagnostic classifier (task β).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push a `Severity::Warning` into the `EvalContext` sink when `affine_scale`
/// returns `Value::Undef` for a zero (degenerate, det=0) or dimensioned scale
/// factor. Fires only on the `Value::Undef` path, like `stackup_diagnose` /
/// `fea_diagnose`.
pub use geometry::diagnose as geometry_diagnose;

/// Public re-export of the PRB-flexure diagnostic classifier (task 3871).
///
/// Called by `crates/reify-expr/src/lib.rs` at the builtin fallthrough arm to
/// push the §5.3 / §1 flexure diagnostics (`W_FlexureYielding`,
/// `W_FlexurePrbOutOfRange`, `E_FlexureGeometryInvalid`,
/// `W_FlexureFatigueCheckMissing`) into the `EvalContext` sink. Unlike
/// `stackup_diagnose` / `fea_diagnose` (which fire only on a `Value::Undef`
/// result), `flexure_diagnose` runs on BOTH the success and `Undef` paths —
/// the yielding / out-of-range warnings fire on a successfully constructed joint.
pub use flexures::flexure_diagnose;

/// Public re-export of the von Mises scalar kernel for cross-crate reuse.
///
/// Called by `crates/reify-expr/src/field_reductions.rs` in the
/// `project_von_mises_sampled` helper to project each 9-float stride-1 window
/// of a Sampled tensor field to a scalar von Mises value during field
/// reduction (max/min/argmax/argmin of a VonMises-derived field). Avoids
/// duplicating the formula inlined at
/// `crates/reify-eval/src/compute_targets/elastic_static.rs:667`.
pub use analysis::compute_von_mises_3x3;
/// Public re-export of the impulse-shaper math (`ImpulseTrain` + its residual /
/// convolution API) and the `Shaper`→`ImpulseTrain` marshalling boundary.
///
/// `reify-eval/src/trajectory_ops.rs` calls [`build_train_for_shaper`] to turn a
/// `Shaper` `Value` into an [`impulse_shaper::ImpulseTrain`] and then sweeps
/// [`impulse_shaper::ImpulseTrain::residual_vibration`] across a frequency band
/// for the engine-side robustness metric (task ζ). It reads the swept damping
/// ratio ζ via [`shaper_damping_ratio`] — the same single-source reader
/// `build_train_for_shaper` builds the train with — so the sweep evaluates the
/// train at exactly the ζ it was constructed from.
pub use trajectory::impulse_shaper;
pub use trajectory::input_shape::{build_train_for_shaper, shaper_damping_ratio};

#[cfg(test)]
#[macro_use]
mod test_macros;

#[cfg(test)]
mod test_fixtures;

mod analysis;
mod complex;
mod construct;
mod fea;
mod flexures;
mod geometry;
mod joints;
mod linalg;
mod list;
mod loads;
pub mod dynamics;
pub mod loop_closure;
pub mod loop_closure_solver;
pub mod loop_closure_value;
mod matrix;
mod mechanism;
pub mod modal;
mod numeric;
mod orientation;
mod snapshot;
mod stackup;
mod supports;
mod sweep;
mod tensegrity;
mod trajectory;
mod trig;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    if let Some(v) = numeric::eval_numeric(name, args) {
        return v;
    }
    if let Some(v) = list::eval_list(name, args) {
        return v;
    }
    if let Some(v) = trig::eval_trig(name, args) {
        return v;
    }
    if let Some(v) = linalg::eval_linalg(name, args) {
        return v;
    }
    if let Some(v) = complex::eval_complex(name, args) {
        return v;
    }
    if let Some(v) = orientation::eval_orientation(name, args) {
        return v;
    }
    if let Some(v) = geometry::eval_geometry(name, args) {
        return v;
    }
    if let Some(v) = matrix::eval_matrix(name, args) {
        return v;
    }
    if let Some(v) = analysis::eval_analysis(name, args) {
        return v;
    }
    if let Some(v) = joints::eval_joints(name, args) {
        return v;
    }
    if let Some(v) = flexures::eval_flexures(name, args) {
        return v;
    }
    if let Some(v) = loads::eval_loads(name, args) {
        return v;
    }
    if let Some(v) = fea::eval_fea(name, args) {
        return v;
    }
    if let Some(v) = supports::eval_supports(name, args) {
        return v;
    }
    if let Some(v) = mechanism::eval_mechanism(name, args) {
        return v;
    }
    if let Some(v) = snapshot::eval_snapshot(name, args) {
        return v;
    }
    if let Some(v) = dynamics::eval::eval_dynamics(name, args) {
        return v;
    }
    if let Some(v) = stackup::eval_stackup(name, args) {
        return v;
    }
    if let Some(v) = sweep::eval_sweep(name, args) {
        return v;
    }
    if let Some(v) = trajectory::eval_trajectory(name, args) {
        return v;
    }
    if let Some(v) = tensegrity::eval_tensegrity(name, args) {
        return v;
    }
    Value::Undef
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_function_returns_undef() {
        assert!(eval_builtin("foo", &[Value::Real(1.0)]).is_undef());
    }
}
