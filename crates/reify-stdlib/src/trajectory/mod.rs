//! Trajectory stdlib module — `piecewise_polynomial` ctor and evaluator
//! intrinsics (evaluate_profile / _dot / _ddot, profile_duration).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.1, §11 Phase 1 β.

use reify_ir::Value;

mod gcode_import;
mod impulse_shaper;
mod sampling;
mod simulate;
mod spline;
mod tots;

/// Evaluate a trajectory stdlib function by name.
///
/// Returns `Some(Value)` for known function names, or `None` for unknown names
/// so that `eval_builtin` can fall through to the next module.
///
/// `gcode_import` (task ο) is fully wired: it marshals its arguments through the
/// pure [`gcode_import::lower_gcode`] layer and returns a real `Value::List` of
/// profile records (or `Value::Undef` on bad args / a hard parse error). See
/// [`gcode_import::eval_gcode_import`] for the argument contract.
///
/// `gcode_import_lower` is an internal delegate intrinsic: the stdlib `.ri`
/// declaration of `gcode_import` shadows the same-named `eval_builtin` entry
/// (the compiler's `resolve_function_overload` returns `Resolved` → `UserFunctionCall`
/// for any fn with a `.ri` body, so the evaluator runs the body rather than
/// reaching `eval_builtin`). The body therefore delegates via a *distinct* name —
/// `gcode_import_lower` — which has no `.ri` declaration and thus resolves
/// `NoUserFunctions` → `FunctionCall` → `eval_builtin` → here. Both names route
/// to the single `eval_gcode_import` implementation. The original `"gcode_import"`
/// name is kept so that the Rust eval-boundary tests in `mod.rs::tests` that call
/// `eval_builtin("gcode_import", …)` directly remain green with zero churn.
///
/// The Phase β spline intrinsics still unconditionally return `Some(Value::Undef)`:
/// the pure-Rust spline math is implemented in the `spline` submodule but is
/// not yet wired to the Value API.  Full marshalling (parsing a
/// `PiecewisePolynomialProfile` from `Value::StructureInstance`, dispatching on
/// the `BoundaryCondition` SIR type-tag, emitting `Value::List<Value::Real>`
/// per joint) is deferred to a later phase (γ/η/θ per the β PRD scope
/// boundary).  Callers that see `Value::Undef` from one of those names should
/// treat it as a "not yet implemented" stub, not a computation result.
pub(crate) fn eval_trajectory(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "gcode_import" | "gcode_import_lower" => Some(gcode_import::eval_gcode_import(args)),
        "piecewise_polynomial"
        | "evaluate_profile"
        | "evaluate_profile_dot"
        | "evaluate_profile_ddot"
        | "profile_duration" => Some(Value::Undef),
        _ => None,
    }
}

/// Analytic reference polynomials shared by sibling submodule tests.
///
/// `spline.rs` and `sampling.rs` both test against the same closed-form cubic
/// `p(t) = 1 + 2t - 0.5t² + 0.3t³` and quintic
/// `q(t) = 1 + t + t² + t³ - 0.5t⁴ + 0.1t⁵`.  Defining them once here
/// prevents the two copies from drifting independently.
#[cfg(test)]
pub(crate) mod test_polynomials {
    /// Cubic polynomial `p(t) = 1 + 2t - 0.5t² + 0.3t³`.
    pub(crate) fn cubic_p(t: f64) -> f64 {
        1.0 + 2.0 * t - 0.5 * t * t + 0.3 * t * t * t
    }
    /// First derivative `p'(t) = 2 - t + 0.9t²`.
    pub(crate) fn cubic_dp(t: f64) -> f64 {
        2.0 - t + 0.9 * t * t
    }
    /// Second derivative `p''(t) = -1 + 1.8t`.
    pub(crate) fn cubic_ddp(t: f64) -> f64 {
        -1.0 + 1.8 * t
    }

    /// Quintic polynomial `q(t) = 1 + t + t² + t³ - 0.5t⁴ + 0.1t⁵`.
    pub(crate) fn quintic_q(t: f64) -> f64 {
        1.0 + t + t * t + t * t * t - 0.5 * t.powi(4) + 0.1 * t.powi(5)
    }
    /// First derivative `q'(t) = 1 + 2t + 3t² - 2t³ + 0.5t⁴`.
    pub(crate) fn quintic_dq(t: f64) -> f64 {
        1.0 + 2.0 * t + 3.0 * t * t - 2.0 * t * t * t + 0.5 * t.powi(4)
    }
    /// Second derivative `q''(t) = 2 + 6t - 6t² + 2t³`.
    pub(crate) fn quintic_ddq(t: f64) -> f64 {
        2.0 + 6.0 * t - 6.0 * t * t + 2.0 * t * t * t
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a 100-line Marlin g-code fixture: 100 contiguous `G1` moves with
    /// no non-motion splitters, so lowering yields a single profile.
    fn marlin_100_line_fixture() -> String {
        let mut s = String::new();
        for i in 0..100 {
            s.push_str(&format!("G1 X{i} Y{i}\n"));
        }
        s
    }

    /// Build a `MarlinDialect` dialect value as the eval path receives it: a
    /// `Value::StructureInstance` whose `type_name` is `"MarlinDialect"` (the
    /// `gcode_import` arm dispatches on this name without a StructureRegistry).
    fn marlin_dialect_value() -> Value {
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "MarlinDialect".to_string(),
            version: 0,
            fields: PersistentMap::default(),
        }))
    }

    /// `gcode_import(<100-line Marlin source>, MarlinDialect)` evaluates to a
    /// non-empty `Value::List` (one entry per lowered motion profile).
    #[test]
    fn gcode_import_marlin_fixture_returns_nonempty_list() {
        let result = eval_builtin(
            "gcode_import",
            &[Value::String(marlin_100_line_fixture()), marlin_dialect_value()],
        );
        match result {
            Value::List(items) => {
                assert!(!items.is_empty(), "expected >= 1 profile, got an empty list")
            }
            other => panic!("expected Value::List from gcode_import, got {other:?}"),
        }
    }

    /// Wrong arity, a non-String source, or a non-StructureInstance dialect
    /// each return `Value::Undef` (the stdlib bad-args convention).
    #[test]
    fn gcode_import_bad_args_return_undef() {
        let dialect = marlin_dialect_value();
        let src = Value::String("G1 X10".to_string());

        // Wrong arity: 0, 1, and 3 args.
        assert!(eval_builtin("gcode_import", &[]).is_undef());
        assert!(eval_builtin("gcode_import", std::slice::from_ref(&src)).is_undef());
        assert!(
            eval_builtin(
                "gcode_import",
                &[src.clone(), dialect.clone(), Value::Int(0)]
            )
            .is_undef()
        );

        // Non-String source.
        assert!(eval_builtin("gcode_import", &[Value::Int(5), dialect.clone()]).is_undef());

        // Non-StructureInstance dialect.
        assert!(eval_builtin("gcode_import", &[src.clone(), Value::Int(7)]).is_undef());
    }
}
