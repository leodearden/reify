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

use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

/// Evaluate a sweep stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_sweep(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "dim" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE constructing the SweepDim Map):
            //   args.len() == 3                        → arity guard
            //   is_driving_joint(args[0])              → joint kind
            //                                            (coupling/fixed/
            //                                            world/non-Map all
            //                                            rejected per §13.4)
            //   args[1] is Value::Range with both
            //     lower/upper bounds present, both
            //     SI-finite, and dimension == joint
            //     kind's expected (LENGTH for
            //     prismatic, ANGLE for revolute)        → range guard
            //   args[2] is Value::Int(n) with n >= 0   → steps guard
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            let expected_dim = match driving_joint_kind(&args[0]) {
                Some(d) => d,
                None => return Some(Value::Undef),
            };
            if validate_range_with_dimension(&args[1], expected_dim).is_none() {
                return Some(Value::Undef);
            }
            match &args[2] {
                Value::Int(n) if *n >= 0 => {}
                _ => return Some(Value::Undef),
            }
            make_sweep_dim(args[0].clone(), args[1].clone(), args[2].clone())
        }
        _ => return None,
    })
}

/// Map a driving joint Value to its expected motion-variable dimension.
///
/// Returns:
/// - `Some(LENGTH)` for prismatic joints
/// - `Some(ANGLE)`  for revolute  joints
/// - `None` for any other shape: coupling joints, fixed joints, the
///   world sentinel, non-joint Maps, and non-Map values.
///
/// Couplings are rejected per §13.4 ("Couplings cannot appear in sweep
/// dims — their motion is derived from the driving joint that is
/// already being swept").  Fixed joints (0-DOF) have no motion variable
/// to sweep over.  The combined predicate-plus-dimension is the
/// uniform driving-joint test for both `dim` and `sweep`.
fn driving_joint_kind(v: &Value) -> Option<DimensionVector> {
    let map = match v {
        Value::Map(m) => m,
        _ => return None,
    };
    match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => match s.as_str() {
            "prismatic" => Some(DimensionVector::LENGTH),
            "revolute" => Some(DimensionVector::ANGLE),
            _ => None,
        },
        _ => None,
    }
}

/// Validate a range value: must be `Value::Range` with both bounds
/// present, both sharing `expected_dim`, both SI-finite. Mirrors
/// `validate_range` in joints.rs (which checks dimension only); this
/// variant additionally enforces finite SI values, since sweep
/// interpolation relies on `as_f64()` returning finite numbers.
fn validate_range_with_dimension(value: &Value, expected_dim: DimensionVector) -> Option<()> {
    match value {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => {
            if lo.dimension() != expected_dim || up.dimension() != expected_dim {
                return None;
            }
            let lo_si = lo.as_f64()?;
            let up_si = up.as_f64()?;
            if !lo_si.is_finite() || !up_si.is_finite() {
                return None;
            }
            Some(())
        }
        _ => None,
    }
}

/// Build a SweepDim `Value::Map` with the standard four-key layout:
/// `kind`, `joint`, `range`, `steps` (alphabetical, matching `BTreeMap`
/// iteration). Mirrors `make_binding` in snapshot.rs and `make_joint` in
/// joints.rs — the kind-discriminated Map convention used across the
/// stdlib value types.
fn make_sweep_dim(joint: Value, range: Value, steps: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("sweep_dim".to_string()),
    );
    m.insert(Value::String("joint".to_string()), joint);
    m.insert(Value::String("range".to_string()), range);
    m.insert(Value::String("steps".to_string()), steps);
    Value::Map(m)
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

    fn axis_z_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    fn length_range_0_to_1m() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn angle_range_0_to_pi() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
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

    // ── dim() input validation: full surface returns Undef ────────────────
    //
    // Validation allow-list (matches `eval_sweep::dim` arm — wired in step-4):
    //   args.len() == 3                        → arity guard
    //   is_driving_joint(args[0])              → joint kind guard (only
    //                                            prismatic/revolute; rejects
    //                                            coupling, fixed, world,
    //                                            non-Map). Couplings are
    //                                            rejected per §13.4 — their
    //                                            motion is derived from the
    //                                            driving joint already swept.
    //                                            Fixed joints have no motion
    //                                            variable to sweep.
    //   args[1] is Value::Range with both
    //     lower/upper bounds present and
    //     dimension == joint kind's expected   → range/dimension guard
    //   args[2] is Value::Int(n) with n >= 0   → steps guard
    // Any guard failure returns `Value::Undef` BEFORE Map construction.

    /// `dim()` with an arity outside {3} returns Undef.
    #[test]
    fn dim_wrong_arity_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // 0, 1, 2, 4 args
        assert!(eval_builtin("dim", &[]).is_undef());
        assert!(eval_builtin("dim", std::slice::from_ref(&j)).is_undef());
        assert!(eval_builtin("dim", &[j.clone(), r.clone()]).is_undef());
        assert!(eval_builtin("dim", &[j, r, n.clone(), n]).is_undef());
    }

    /// `dim(non_driving_joint, range, steps)` returns Undef when args[0]
    /// is not a prismatic/revolute joint. Covers Real, String, world
    /// sentinel, fixed joint, and coupling joint — couplings are
    /// rejected per §13.4 ("Couplings cannot appear in sweep dims —
    /// their motion is derived from the driving joint that is already
    /// being swept"). Fixed joints have no motion variable.
    #[test]
    fn dim_non_driving_joint_returns_undef() {
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // Real (not a Map at all)
        assert!(eval_builtin("dim", &[Value::Real(1.0), r.clone(), n.clone()]).is_undef());

        // String (not a Map at all)
        assert!(
            eval_builtin("dim", &[Value::String("not a joint".to_string()), r.clone(), n.clone()])
                .is_undef()
        );

        // world sentinel — Map with kind="world", not a joint
        assert!(eval_builtin("dim", &[eval_builtin("world", &[]), r.clone(), n.clone()]).is_undef());

        // fixed joint — kind="fixed" has no motion variable
        let fixed = eval_builtin("fixed", &[]);
        assert!(eval_builtin("dim", &[fixed, r.clone(), n.clone()]).is_undef());

        // coupling joint — derived from a driving joint, can't be swept
        let parent = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        assert!(eval_builtin("dim", &[coupling, r, n]).is_undef());
    }

    /// `dim(joint, non_range, steps)` returns Undef when args[1] is not a
    /// Value::Range with both bounds present.
    #[test]
    fn dim_non_range_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let n = Value::Int(11);

        // Real
        assert!(eval_builtin("dim", &[j.clone(), Value::Real(1.0), n.clone()]).is_undef());

        // String
        assert!(
            eval_builtin("dim", &[j.clone(), Value::String("nope".to_string()), n.clone()])
                .is_undef()
        );

        // Map (not a Range)
        let mech = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("dim", &[j, mech, n]).is_undef());
    }

    /// `dim(joint, range, non_int_steps)` returns Undef when args[2] is
    /// not a Value::Int.
    #[test]
    fn dim_non_int_steps_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();

        // Real
        assert!(eval_builtin("dim", &[j.clone(), r.clone(), Value::Real(11.0)]).is_undef());

        // String
        assert!(
            eval_builtin("dim", &[j, r, Value::String("eleven".to_string())]).is_undef()
        );
    }

    /// `dim(joint, range, Int(-1))` returns Undef — negative step counts
    /// are rejected. (Int(0) is valid: `sweep` returns the empty list.)
    #[test]
    fn dim_negative_steps_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        assert!(eval_builtin("dim", &[j, r, Value::Int(-1)]).is_undef());
    }

    /// `dim(joint, range, steps)` returns Undef when the range's
    /// dimension does not match the joint kind:
    /// - prismatic joint requires LENGTH range
    /// - revolute  joint requires ANGLE  range
    /// Mirrors `validate_range`'s pattern from joints.rs.
    #[test]
    fn dim_dimension_mismatch_returns_undef() {
        // Prismatic joint + angle range → Undef.
        let j_pris = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        assert!(
            eval_builtin("dim", &[j_pris, angle_range_0_to_pi(), Value::Int(11)]).is_undef()
        );

        // Revolute joint + length range → Undef.
        let j_rev = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        assert!(
            eval_builtin("dim", &[j_rev, length_range_0_to_1m(), Value::Int(11)]).is_undef()
        );
    }
}
