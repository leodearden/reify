//! Mechanism builder stdlib (task 2528).
//!
//! Implements the v0.1 `mechanism().body(...)` builder per
//! `docs/prds/kinematic-constraints.md` task 3 and `docs/reify-stdlib-reference.md` §13.2.
//!
//! Mechanism state is encoded as a `Value::Map` with the shape:
//! `{ "kind": "mechanism", "bodies": List(body_record...), "joint_parents": Map(joint→parent), "next_id": Int(N) }`.
//! On error the Map additionally carries `error`, `error_path1`, `error_path2`,
//! and `error_message` fields. See plan §"Mechanism Map shape".
//!
//! Diagnostic emission via `EvalResult.diagnostics` is deferred to the
//! snapshot/eval-pipeline integration (`DiagnosticCode::KinematicClosedChain`
//! and `DiagnosticCode::MechanismDuplicateSolid` are reserved in
//! `reify-types/src/diagnostics.rs` for that future integration).

use reify_types::Value;

/// Evaluate a mechanism stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_mechanism(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;
    use std::collections::BTreeMap;

    // ── mechanism() constructor: happy path ────────────────────────────────

    /// `mechanism()` returns a `Value::Map` with the four canonical fields
    /// (`kind = "mechanism"`, empty `bodies` list, empty `joint_parents` map,
    /// `next_id = 0`). Pins the empty-Mechanism shape so subsequent `body()`
    /// builders can rely on these fields existing.
    #[test]
    fn mechanism_returns_empty_map() {
        let result = eval_builtin("mechanism", &[]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("mechanism".to_string())),
            "kind field should be 'mechanism'"
        );
        assert_eq!(
            map.get(&Value::String("bodies".to_string())),
            Some(&Value::List(vec![])),
            "bodies field should be an empty List"
        );
        assert_eq!(
            map.get(&Value::String("joint_parents".to_string())),
            Some(&Value::Map(BTreeMap::new())),
            "joint_parents field should be an empty Map"
        );
        assert_eq!(
            map.get(&Value::String("next_id".to_string())),
            Some(&Value::Int(0)),
            "next_id field should be Int(0)"
        );
    }

    /// `mechanism(...)` with any non-zero arg count returns `Value::Undef`,
    /// matching the stdlib convention for wrong-arity constructors.
    #[test]
    fn mechanism_with_args_returns_undef() {
        assert!(eval_builtin("mechanism", &[Value::Int(0)]).is_undef());
        assert!(eval_builtin("mechanism", &[Value::Int(0), Value::Int(1)]).is_undef());
        assert!(eval_builtin("mechanism", &[Value::Real(1.0)]).is_undef());
    }
}
