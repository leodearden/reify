//! Batch-sweep stdlib for forward kinematics (task 2529).
//!
//! Implements the v0.1 `dim()` / `sweep()` / `sweep_grid()` builtins per
//! `docs/prds/kinematic-constraints.md` task 5 and `docs/reify-stdlib-reference.md` §13.4.
//!
//! Both `sweep` and `sweep_grid` delegate to the existing `snapshot()` builtin
//! (task 2535) — they construct interpolated bindings lists from per-joint
//! ranges and steps, then call `eval_builtin("snapshot", ...)` once per
//! result element.  Joints absent from the bindings list automatically fall
//! back to range midpoint via `snapshot()`'s existing fallback chain.
//!
//! Surface:
//!   - `dim(joint, range, steps)`             → SweepDim Map
//!   - `sweep(m, joint, range, steps)`        → List<Snapshot>
//!   - `sweep_grid(m, dims_list)`             → List<Snapshot>

use reify_types::Value;

/// Evaluate a sweep stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_sweep(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    // ── Joint / range fixtures (mirror the per-module duplication
    //    convention noted in snapshot.rs:597-599). ────────────────────────

    fn axis_x_unit() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    fn length_range_0_to_1m() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    // ── dim(joint, range, steps): happy path ─────────────────────────────

    /// `dim(joint, range, steps)` returns a `Value::Map` with shape
    /// `{kind="sweep_dim", joint=<input joint>, range=<input range>, steps=<input steps>}`.
    /// Pins the SweepDim shape so subsequent `sweep_grid` steps can rely on
    /// these four canonical fields existing.
    #[test]
    fn dim_returns_sweep_dim_map_with_kind_joint_range_steps() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        let n = Value::Int(11);
        let result = eval_builtin("dim", &[j.clone(), r.clone(), n.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map sweep_dim record, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("sweep_dim".to_string())),
            "kind field should be 'sweep_dim'"
        );
        assert_eq!(
            map.get(&Value::String("joint".to_string())),
            Some(&j),
            "joint field should be the input joint verbatim"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&r),
            "range field should be the input range verbatim"
        );
        assert_eq!(
            map.get(&Value::String("steps".to_string())),
            Some(&n),
            "steps field should be the input Int verbatim"
        );
    }
}
