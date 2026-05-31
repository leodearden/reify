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

#[cfg(test)]
#[macro_use]
mod test_macros;

#[cfg(test)]
mod test_fixtures;

mod analysis;
mod complex;
mod fea;
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
