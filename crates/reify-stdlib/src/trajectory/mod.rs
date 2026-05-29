//! Trajectory stdlib module — `piecewise_polynomial` ctor and evaluator
//! intrinsics (evaluate_profile / _dot / _ddot, profile_duration).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.1, §11 Phase 1 β.

use reify_ir::Value;

mod gcode_import;
mod spline;

/// Evaluate a trajectory stdlib function by name.
///
/// Returns `Some(Value)` for known function names, or `None` for unknown names
/// so that `eval_builtin` can fall through to the next module.
///
/// Phase β: all recognized names unconditionally return `Some(Value::Undef)`.
/// The pure-Rust spline math is implemented in the `spline` submodule but is
/// not yet wired to the Value API.  Full marshalling (parsing a
/// `PiecewisePolynomialProfile` from `Value::StructureInstance`, dispatching on
/// the `BoundaryCondition` SIR type-tag, emitting `Value::List<Value::Real>`
/// per joint) is deferred to a later phase (γ/η/θ per the β PRD scope
/// boundary).  Callers that see `Value::Undef` here should treat it as a
/// "not yet implemented" stub, not a computation result.
pub(crate) fn eval_trajectory(name: &str, _args: &[Value]) -> Option<Value> {
    match name {
        "piecewise_polynomial"
        | "evaluate_profile"
        | "evaluate_profile_dot"
        | "evaluate_profile_ddot"
        | "profile_duration" => Some(Value::Undef),
        _ => None,
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
