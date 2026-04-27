use reify_types::Value;

mod helpers;

/// Public re-export of the shared complex-phase helper, so reify-expr's method
/// path can call the same implementation used by the stdlib builtin path.
pub use helpers::complex_phase;

#[cfg(test)]
#[macro_use]
mod test_macros;

mod analysis;
mod complex;
mod geometry;
mod joints;
mod linalg;
mod matrix;
mod numeric;
mod orientation;
mod trig;

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    if let Some(v) = numeric::eval_numeric(name, args) {
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
