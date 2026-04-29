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

use crate::eval_builtin;

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
        "sweep" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE any `eval_builtin("snapshot", ...)`
            // delegation; mirrors snapshot.rs's snapshot arm validation):
            //   args.len() == 4                                → arity guard
            //   args[0] is Map with kind="mechanism"           → mechanism guard
            //   is_driving_joint(args[1])                      → joint kind guard
            //   args[2] is Value::Range matching the joint's   → range guard
            //     dimension and SI-finite
            //   args[3] is Value::Int(n) with n >= 0           → steps guard
            // Errored-mechanism short-circuit and >=2-step interpolation
            // arms are layered on in subsequent steps.
            if args.len() != 4 {
                return Some(Value::Undef);
            }
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }
            let expected_dim = match driving_joint_kind(&args[1]) {
                Some(d) => d,
                None => return Some(Value::Undef),
            };
            if validate_range_with_dimension(&args[2], expected_dim).is_none() {
                return Some(Value::Undef);
            }
            let steps = match &args[3] {
                Value::Int(n) if *n >= 0 => *n,
                _ => return Some(Value::Undef),
            };
            if steps == 0 {
                return Some(Value::List(vec![]));
            }
            // steps == 1 is wired in step-14; for now it falls through
            // to the >=2 arm where (steps - 1) == 0 would divide by
            // zero — so guard with Undef.
            if steps == 1 {
                return Some(Value::Undef);
            }
            // Range bounds in SI units; validation above guarantees
            // both are present and finite.
            let (lo_si, up_si) = match &args[2] {
                Value::Range {
                    lower: Some(lo),
                    upper: Some(up),
                    ..
                } => match (lo.as_f64(), up.as_f64()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return Some(Value::Undef),
                },
                _ => return Some(Value::Undef),
            };
            // Linearly-interpolated motion values, evenly spaced from
            // range.lower (i=0) to range.upper (i=steps-1). Each
            // interpolated f64 is wrapped back into a dimensioned
            // Value::length / Value::angle per joint kind, then bound
            // and passed to snapshot() — keeping the FK walk and
            // unbound-joint midpoint fallback in one place.
            let mut snapshots = Vec::with_capacity(steps as usize);
            for i in 0..steps {
                let t = (i as f64) / ((steps - 1) as f64);
                let v_si = lo_si + t * (up_si - lo_si);
                let wrapped = match wrap_value_for_driving_joint(&args[1], v_si) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                };
                let binding = eval_builtin("bind", &[args[1].clone(), wrapped]);
                if binding.is_undef() {
                    return Some(Value::Undef);
                }
                let snap = eval_builtin(
                    "snapshot",
                    &[args[0].clone(), Value::List(vec![binding])],
                );
                if snap.is_undef() {
                    return Some(Value::Undef);
                }
                snapshots.push(snap);
            }
            Value::List(snapshots)
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

/// Wrap an interpolated f64 (in SI units) back into a dimensioned
/// `Value` based on the driving joint's kind.
///
/// - `prismatic` → `Value::length(v_si)` (metres)
/// - `revolute`  → `Value::angle(v_si)`  (radians)
///
/// Returns `None` for any other shape (coupling, fixed, world, non-Map).
/// Couplings and fixed joints never reach this helper because
/// `driving_joint_kind` rejects them upstream — the `None` branch is
/// pure defense-in-depth.
///
/// Mirrors `wrap_midpoint_for_joint` in snapshot.rs (the `prismatic` and
/// `revolute` arms specifically); the coupling arm is omitted because
/// couplings are rejected at the `dim`/`sweep` boundary by
/// `driving_joint_kind`.  A future refactor could promote both helpers
/// to a shared `stdlib/helpers.rs` utility — see snapshot.rs:597-599.
fn wrap_value_for_driving_joint(joint: &Value, v_si: f64) -> Option<Value> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" => Some(Value::length(v_si)),
        "revolute" => Some(Value::angle(v_si)),
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

    // ── sweep(m, joint, range, steps): degenerate inputs ──────────────────

    /// Helper: build a 1-body mechanism with a prismatic +X joint
    /// (range 0..1m), parent=world, identity pose. Returns the
    /// mechanism Map and the joint so tests can use both.
    fn make_one_body_prismatic_mechanism() -> (Value, Value) {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("a".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j.clone()]);
        (m1, j)
    }

    /// `sweep(m, j, range, Int(0))` returns the empty `Value::List`.
    /// Pins the degenerate-input semantic so callers can pipe
    /// `sweep(...).map(...)` without a defensive `if steps > 0`
    /// guard.
    #[test]
    fn sweep_steps_zero_returns_empty_list() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let result = eval_builtin(
            "sweep",
            &[m, j, length_range_0_to_1m(), Value::Int(0)],
        );
        assert_eq!(
            result,
            Value::List(vec![]),
            "sweep with steps==0 should yield an empty List"
        );
    }

    // ── sweep(): headline acceptance test (PRD task 5) ────────────────────

    /// Decompose a `Value::Transform` into its translation `[tx, ty, tz]`
    /// (SI metres). Mirrors the pattern in
    /// `snapshot.rs::tests::decompose_transform_for_assert`.
    fn translation_of_transform(t: &Value) -> [f64; 3] {
        let trans = match t {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Value::Transform, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(c) if c.len() == 3 => c,
            other => panic!("expected Value::Vector len=3, got {:?}", other),
        };
        let read = |v: &Value| -> f64 {
            match v {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected numeric component, got {:?}", other),
            }
        };
        [read(&comps[0]), read(&comps[1]), read(&comps[2])]
    }

    /// Extract a snapshot's body 0 world translation (assumes id=0 sits
    /// at index 0 of the bodies list).
    fn body_0_translation(snapshot: &Value) -> [f64; 3] {
        let smap = match snapshot {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body
            .get(&Value::String("world_transform".to_string()))
            .expect("body record must carry world_transform");
        translation_of_transform(wt)
    }

    /// Headline acceptance test (PRD task 5): 11 evenly-spaced
    /// snapshots over a 0..1m prismatic +X joint. The i-th snapshot's
    /// body sits at world translation (i/10, 0, 0). Endpoints match
    /// `snapshot(m, [bind(j, range.lower)])` and `snapshot(m, [bind(j,
    /// range.upper)])` per spec §13.4.
    #[test]
    fn sweep_eleven_steps_evenly_spaced_first_last_match_snapshot() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();
        let result = eval_builtin(
            "sweep",
            &[m.clone(), j.clone(), r.clone(), Value::Int(11)],
        );
        let list = match result {
            Value::List(l) => l,
            other => panic!("expected Value::List, got {:?}", other),
        };
        assert_eq!(list.len(), 11, "sweep with steps=11 should produce 11 snapshots");

        // Each snapshot is a Snapshot Map with kind="snapshot"; body 0
        // sits at world translation (i/10, 0, 0).
        for i in 0..=10 {
            let snap = &list[i];
            let smap = match snap {
                Value::Map(m) => m,
                other => panic!("snap[{}] should be a Map, got {:?}", i, other),
            };
            assert_eq!(
                smap.get(&Value::String("kind".to_string())),
                Some(&Value::String("snapshot".to_string())),
                "snap[{}].kind should be 'snapshot'",
                i
            );
            let [tx, ty, tz] = body_0_translation(snap);
            let expected_x = (i as f64) / 10.0;
            assert!(
                (tx - expected_x).abs() < 1e-12,
                "snap[{}] body translation x should be {}, got {}",
                i,
                expected_x,
                tx
            );
            assert!(ty.abs() < 1e-12, "snap[{}] body translation y should be 0, got {}", i, ty);
            assert!(tz.abs() < 1e-12, "snap[{}] body translation z should be 0, got {}", i, tz);
        }

        // First snapshot equals snapshot(m, [bind(j, length(0.0))]).
        let bind_lo = eval_builtin("bind", &[j.clone(), Value::length(0.0)]);
        let snap_lo = eval_builtin(
            "snapshot",
            &[m.clone(), Value::List(vec![bind_lo])],
        );
        assert_eq!(
            list[0], snap_lo,
            "first sweep element should equal snapshot(m, [bind(j, length(0))])"
        );

        // Last snapshot equals snapshot(m, [bind(j, length(1.0))]).
        let bind_hi = eval_builtin("bind", &[j.clone(), Value::length(1.0)]);
        let snap_hi = eval_builtin("snapshot", &[m, Value::List(vec![bind_hi])]);
        assert_eq!(
            list[10], snap_hi,
            "last sweep element should equal snapshot(m, [bind(j, length(1))])"
        );
    }

    // ── Errored-mechanism short-circuit ───────────────────────────────────

    fn axis_y_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    /// `sweep()` on an errored Mechanism returns `Value::Undef` — not a
    /// partial List of pre-error snapshots. Mirrors
    /// `snapshot_on_errored_mechanism_returns_undef` in snapshot.rs:
    /// chained sweeps must reckon with the upstream error before
    /// getting a plausible-looking List back.
    #[test]
    fn sweep_on_errored_mechanism_returns_undef() {
        // Build an errored mechanism via parent-conflict — same recipe as
        // snapshot.rs::snapshot_on_errored_mechanism_returns_undef.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b]);
        // Sanity: the setup actually produced an errored mechanism.
        match &errored {
            Value::Map(m) => assert_eq!(
                m.get(&Value::String("error".to_string())),
                Some(&Value::String("closed_chain".to_string())),
                "setup precondition: errored mechanism has error='closed_chain'"
            ),
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        // sweep() on the errored mechanism must yield Undef even
        // though the pre-error bodies list contains a fully-formed
        // body record.
        assert!(
            eval_builtin(
                "sweep",
                &[errored, j_x, angle_range_0_to_pi(), Value::Int(11)]
            )
            .is_undef(),
            "sweep() on errored mechanism must yield Undef"
        );
    }
}
