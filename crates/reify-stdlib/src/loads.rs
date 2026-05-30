//! FEA load constructors for the stdlib.
//!
//! Provides `gravity` as the only remaining name-dispatched constructor.  It
//! returns a `Value::Map` with a `kind` discriminator field, matching the
//! joints/coupling constructor pattern.
//!
//! Retired: `point_load` (SIR-α, task 3540 step-20), `pressure_load`
//! (SIR-β-load, task 3544 step-4), `traction_load` (FEA-2, task 2881
//! step-4), and `body_force` (FEA-2, task 2881 step-8) — all four replaced
//! by stdlib structure defs (`PointLoad` / `PressureLoad` / `TractionLoad` /
//! `BodyForce` in `fea_multi_case.ri`) that lower to `Value::StructureInstance`
//! via `CompiledExprKind::StructureInstanceCtor`.  `gravity` is retained as a
//! builtin because its 0-arg Earth-default and scalar→−Z constructor logic
//! cannot be replicated by a plain structure-def field bundle.
//!
//! Selector-target validation is delegated to
//! [`crate::helpers::validate_selector_target`].  Selector targets are
//! validated as a narrow placeholder set (`Value::Map` or `Value::String`)
//! until the topology-selector variants land — see that helper's doc-comment
//! for the full narrowed contract and PRD task 16 deadline reference.

use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{make_kind_map, validate_dimensioned_scalar, validate_dimensioned_vec3};

/// Earth standard gravity in m/s² (CGPM 1901 definition).
pub(crate) const EARTH_GRAVITY: f64 = 9.80665;

/// Canonical set of load kinds recognized by this module.
///
/// Analogous to `joints::JOINT_KINDS`.  Consumed by `is_load_value` and
/// guarded by the `load_kinds_all_dispatched_by_eval_loads` partition test to
/// prevent silent drift between this list and `eval_loads`'s dispatch arms.
///
/// Not yet referenced by any external caller — the FEA solver (PRD task 16)
/// will wire this up when it lands.
///
/// task 3540 (SIR-α wave-1, step-20): `"point_load"` retired here — the
/// `structure def PointLoad : Load { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_loads`'s arm is
/// removed in lockstep so this list and its partition guard stay in sync.
///
/// task 3544 (SIR-β-load, step-4): `"pressure_load"` retired here — the
/// `structure def PressureLoad : Load { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the same
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_loads`'s arm is
/// removed in lockstep so this list and its partition guard stay in sync.
///
/// task 2881 (FEA-2, step-4): `"traction_load"` retired here — the
/// `structure def TractionLoad : Load { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the same
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_loads`'s arm is
/// removed in lockstep so this list and its partition guard stay in sync.
///
/// task 2881 (FEA-2, step-8): `"body_force"` retired here — the
/// `structure def BodyForce : Load { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the same
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_loads`'s arm is
/// removed in lockstep so this list and its partition guard stay in sync.
/// LOAD_KINDS is now exactly `["gravity"]`.
#[allow(dead_code)]
pub(crate) const LOAD_KINDS: &[&str] = &["gravity"];

/// Returns `true` if `v` is a load `Value::Map` produced by this module —
/// i.e., a Map with a `kind` field whose value is one of `LOAD_KINDS`.
///
/// Analogous to `joints::is_joint_value`.  Used by the FEA solver (PRD
/// task 16) once it lands; not yet called from any external module.
#[allow(dead_code)]
pub(crate) fn is_load_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => m
            .get(&Value::String("kind".to_string()))
            .and_then(|k| {
                if let Value::String(s) = k {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .is_some_and(|s| LOAD_KINDS.contains(&s)),
        _ => false,
    }
}

/// Evaluate a loads stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_loads(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // task 3540 (SIR-α wave-1, step-20): `point_load` retired. The
        // `structure def PointLoad : Load { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `PointLoad(...)` evals to a `Value::StructureInstance`. Returning
        // `None` here makes `eval_builtin("point_load", ...)` fall through to
        // `Value::Undef` (the unknown-name contract) — the swap is observable
        // at the source level via the `PointLoad` ctor, not the snake_case
        // builtin. The `force`/`point` field shapes are preserved by the
        // structure-def per Q-SIR-4 (preserve-don't-redesign).
        //
        // task 3544 (SIR-β-load, step-4): `pressure_load` retired. The
        // `structure def PressureLoad : Load { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // same `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `PressureLoad(...)` evals to a `Value::StructureInstance`. Returning
        // `None` (via the wildcard arm below) makes
        // `eval_builtin("pressure_load", ...)` fall through to `Value::Undef`
        // (the unknown-name contract). The `magnitude`/`face`/`direction` field
        // shapes are preserved by the structure-def per Q-SIR-4.
        //
        // task 2881 (FEA-2, step-4): `traction_load` retired. The
        // `structure def TractionLoad : Load { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // same `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `TractionLoad(...)` evals to a `Value::StructureInstance`. Returning
        // `None` (via the wildcard arm below) makes
        // `eval_builtin("traction_load", ...)` fall through to `Value::Undef`
        // (the unknown-name contract). The `face`/`traction` field shapes are
        // preserved by the structure-def per Q-SIR-4.
        //
        // task 2881 (FEA-2, step-8): `body_force` retired. The
        // `structure def BodyForce : Load { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // same `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `BodyForce(...)` evals to a `Value::StructureInstance`. Returning
        // `None` (via the wildcard arm below) makes
        // `eval_builtin("body_force", ...)` fall through to `Value::Undef`
        // (the unknown-name contract). The `body`/`force_density` field shapes
        // are preserved by the structure-def per Q-SIR-4.
        "gravity" => {
            let accel_dim = DimensionVector::ACCELERATION;
            match args.len() {
                0 => {
                    // 0-arg form: Earth standard gravity in -Z direction.
                    let acceleration = Value::Vector(vec![
                        Value::Scalar {
                            si_value: 0.0,
                            dimension: accel_dim,
                        },
                        Value::Scalar {
                            si_value: 0.0,
                            dimension: accel_dim,
                        },
                        Value::Scalar {
                            si_value: -EARTH_GRAVITY,
                            dimension: accel_dim,
                        },
                    ]);
                    make_kind_map("gravity", vec![("acceleration", acceleration)])
                }
                1 => {
                    // Try scalar interpretation first: Scalar<Acceleration> → magnitude
                    // placed in -Z with sign flip.  Then try Vector3/Tensor/Point
                    // interpretation (same as point_load / traction_load / body_force):
                    // any 3-component dimensioned vector accepted and round-tripped
                    // unchanged (no sign flip, no Z-axis remap).
                    if let Some(magnitude) = validate_dimensioned_scalar(&args[0], accel_dim) {
                        let acceleration = Value::Vector(vec![
                            Value::Scalar {
                                si_value: 0.0,
                                dimension: accel_dim,
                            },
                            Value::Scalar {
                                si_value: 0.0,
                                dimension: accel_dim,
                            },
                            Value::Scalar {
                                si_value: -magnitude,
                                dimension: accel_dim,
                            },
                        ]);
                        make_kind_map("gravity", vec![("acceleration", acceleration)])
                    } else if validate_dimensioned_vec3(&args[0], accel_dim).is_some() {
                        make_kind_map("gravity", vec![("acceleration", args[0].clone())])
                    } else {
                        return Some(Value::Undef);
                    }
                }
                _ => return Some(Value::Undef),
            }
        }
        _ => return None,
    })
}

// ── Validators ───────────────────────────────────────────────────────────────
//
// task 3544 (SIR-β-load): `validate_pressure_direction` was removed here —
// it validated the `direction` argument to the `"pressure_load"` builtin arm
// (the `"normal"` sentinel OR a dimensionless Vector3). After step-4 retired
// the `"pressure_load"` arm from `eval_loads`, this function became dead code.
// The direction validation remains relevant for PressureLoad but now belongs to
// any future wave-2 solver that reads the `direction` field from the
// `Value::StructureInstance` produced by the stdlib structure def.

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_core::DimensionVector;
    use reify_ir::Value;
    use std::collections::BTreeMap;

    /// Build a simple opaque selector stub (Map with the given `kind`).
    fn selector_stub(kind: &str) -> Value {
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String(kind.to_string()),
            );
            m
        })
    }

    // ── point_load constructor: RETIRED (SIR-α wave-1, task 3540 step-20) ────
    //
    // The `point_load` name-dispatched builtin was retired in favour of the
    // `structure def PointLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PointLoad(...)` calls now lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval to a `Value::StructureInstance` (end-to-end coverage:
    // `crates/reify-eval/tests/structure_instance_e2e.rs::
    //  point_load_in_source_lowers_to_structure_instance`).
    //
    // The Rust API contract — `eval_builtin("point_load", ...)` returns
    // `Value::Undef` — is pinned by
    // `point_load_eval_builtin_returns_undef_post_retirement` above. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. Selector / dimensioned-
    // vector validation now lives on the structure-def's field contracts and
    // is exercised by the SIR-α boundary suite, not here.

    // ── pressure_load constructor: RETIRED (SIR-β-load, task 3544 step-4) ────
    //
    // The `pressure_load` name-dispatched builtin was retired in favour of the
    // `structure def PressureLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PressureLoad(...)` calls now lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval to a `Value::StructureInstance` (end-to-end coverage:
    // `crates/reify-eval/tests/pressure_load.rs::
    //  pressure_load_in_source_lowers_to_structure_instance`).
    //
    // The Rust API contract — `eval_builtin("pressure_load", ...)` returns
    // `Value::Undef` — is pinned by
    // `pressure_load_eval_builtin_returns_undef_post_retirement` below. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. Selector / dimensioned-
    // scalar validation now lives on the structure-def's field contracts and
    // is exercised by the SIR-β-load boundary suite, not here.

    // ── task 2881 step-3 (RED): post-retirement contract ─────────────────────
    //
    // After step-4 (FEA-2 stdlib swap), `traction_load` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def TractionLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `TractionLoad(...)` calls lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval into a `Value::StructureInstance`. The
    // `eval_builtin("traction_load", ...)` Rust API path returns `Value::Undef`
    // because the dispatch arm in `eval_loads` is removed.
    //
    // RED: this test currently fails because `eval_builtin("traction_load", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-4 retires
    // the arm and updates `LOAD_KINDS` so the partition guard stays green.

    #[test]
    fn traction_load_eval_builtin_returns_undef_post_retirement() {
        let selector = selector_stub("face_stub");
        let traction = make_scalar_vec3([2e6, 0.0, -1e6], DimensionVector::PRESSURE);
        assert!(
            eval_builtin("traction_load", &[selector, traction]).is_undef(),
            "after step-4 retirement, eval_builtin('traction_load', ...) must \
             return Undef; the structure-instance ctor path (TractionLoad) replaces \
             the builtin entirely (FEA-2 task 2881, Q-SIR-4 — rename traction_load → TractionLoad)"
        );
    }

    // ── traction_load constructor: RETIRED (FEA-2, task 2881 step-4) ─────────
    //
    // The `traction_load` name-dispatched builtin was retired in favour of the
    // `structure def TractionLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `TractionLoad(...)` calls now lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval to a `Value::StructureInstance` (end-to-end coverage:
    // `crates/reify-eval/tests/fea_loads_stdlib_smoke.rs::
    //  traction_load_in_source_lowers_to_structure_instance`).
    //
    // The Rust API contract — `eval_builtin("traction_load", ...)` returns
    // `Value::Undef` — is pinned by
    // `traction_load_eval_builtin_returns_undef_post_retirement` above. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. Selector / dimensioned-
    // vec3 validation now lives on the structure-def's field contracts and is
    // exercised by the FEA-2 boundary suite, not here.

    // ── body_force constructor: RETIRED (FEA-2, task 2881 step-8) ────────────
    //
    // The `body_force` name-dispatched builtin was retired in favour of the
    // `structure def BodyForce : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `BodyForce(...)` calls now lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval to a `Value::StructureInstance` (end-to-end coverage:
    // `crates/reify-eval/tests/fea_loads_stdlib_smoke.rs::
    //  body_force_in_source_lowers_to_structure_instance`).
    //
    // The Rust API contract — `eval_builtin("body_force", ...)` returns
    // `Value::Undef` — is pinned by
    // `body_force_eval_builtin_returns_undef_post_retirement` above. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. Selector / dimensioned-
    // vec3 validation now lives on the structure-def's field contracts and is
    // exercised by the FEA-2 boundary suite, not here.

    // ── task 2881 step-7 (RED): post-retirement contract ─────────────────────
    //
    // After step-8 (FEA-2 stdlib swap), `body_force` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def BodyForce : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `BodyForce(...)` calls lower to `CompiledExprKind::StructureInstanceCtor`
    // and eval into a `Value::StructureInstance`. The
    // `eval_builtin("body_force", ...)` Rust API path returns `Value::Undef`
    // because the dispatch arm in `eval_loads` is removed.
    //
    // RED: this test currently fails because `eval_builtin("body_force", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-8 retires
    // the arm and updates `LOAD_KINDS` so the partition guard stays green.

    #[test]
    fn body_force_eval_builtin_returns_undef_post_retirement() {
        let selector = selector_stub("body_stub");
        let force_density = make_scalar_vec3([0.0, 0.0, -77000.0], DimensionVector::FORCE_DENSITY);
        assert!(
            eval_builtin("body_force", &[selector, force_density]).is_undef(),
            "after step-8 retirement, eval_builtin('body_force', ...) must \
             return Undef; the structure-instance ctor path (BodyForce) replaces \
             the builtin entirely (FEA-2 task 2881, Q-SIR-4 — rename body_force → BodyForce)"
        );
    }

    // ── gravity constructor: 0-arg form ──────────────────────────────────────

    #[test]
    fn gravity_zero_args_returns_earth_default_acceleration() {
        use super::EARTH_GRAVITY;

        let result = eval_builtin("gravity", &[]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("gravity".to_string())),
            "kind should be 'gravity'"
        );

        let accel = map
            .get(&Value::String("acceleration".to_string()))
            .expect("acceleration field must exist");

        // Verify dimension on first component.
        let expected_dim = DimensionVector::ACCELERATION;
        assert_vector3_approx!(Vector, accel.clone(), [0.0, 0.0, -EARTH_GRAVITY]);

        // Also check dimension is correct.
        if let Value::Vector(items) = accel {
            assert_eq!(
                items[0].dimension(),
                expected_dim,
                "acceleration components should have acceleration dimension"
            );
        } else {
            panic!("acceleration should be Value::Vector");
        }
    }

    // ── gravity constructor: failure modes ────────────────────────────────────

    #[test]
    fn gravity_scalar_length_dim_returns_undef() {
        // Scalar with LENGTH dimension (not acceleration).
        let bad = Value::Scalar {
            si_value: 9.81,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("gravity", &[bad]).is_undef(),
            "Scalar<LENGTH> → Undef"
        );
    }

    #[test]
    fn gravity_scalar_force_dim_returns_undef() {
        // Scalar with FORCE dimension (not acceleration).
        let bad = Value::Scalar {
            si_value: 9.81,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            eval_builtin("gravity", &[bad]).is_undef(),
            "Scalar<FORCE> → Undef"
        );
    }

    #[test]
    fn gravity_scalar_nan_returns_undef() {
        let bad = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::ACCELERATION,
        };
        assert!(
            eval_builtin("gravity", &[bad]).is_undef(),
            "Scalar NaN → Undef"
        );
    }

    #[test]
    fn gravity_vector3_length_dim_returns_undef() {
        // Vector3 with LENGTH instead of acceleration dim.
        let bad = make_scalar_vec3([0.0, 0.0, -9.81], DimensionVector::LENGTH);
        assert!(
            eval_builtin("gravity", &[bad]).is_undef(),
            "Vector3<LENGTH> → Undef"
        );
    }

    #[test]
    fn gravity_vector2_acceleration_dim_returns_undef() {
        let dim = DimensionVector::ACCELERATION;
        let vec2 = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            },
            Value::Scalar {
                si_value: -9.81,
                dimension: dim,
            },
        ]);
        assert!(
            eval_builtin("gravity", &[vec2]).is_undef(),
            "Vector2<acceleration> → Undef"
        );
    }

    #[test]
    fn gravity_vector3_inf_component_returns_undef() {
        let bad = make_scalar_vec3([0.0, 0.0, f64::INFINITY], DimensionVector::ACCELERATION);
        assert!(
            eval_builtin("gravity", &[bad]).is_undef(),
            "Vector3 with Inf → Undef"
        );
    }

    #[test]
    fn gravity_bare_real_returns_undef() {
        // Value::Real is not a dimensioned Scalar.
        assert!(
            eval_builtin("gravity", &[Value::Real(9.81)]).is_undef(),
            "Real(9.81) → Undef"
        );
    }

    #[test]
    fn gravity_two_args_returns_undef() {
        let s = Value::Scalar {
            si_value: 9.81,
            dimension: DimensionVector::ACCELERATION,
        };
        assert!(
            eval_builtin("gravity", &[s.clone(), s]).is_undef(),
            "2 args → Undef"
        );
    }

    // ── gravity constructor: 1-arg Vector3<Acceleration> form ────────────────

    #[test]
    fn gravity_vector3_arg_round_trips_unchanged() {
        // Moon gravity in +X direction (sideways) to distinguish from the -Z sign-flip path.
        let moon_gravity = make_scalar_vec3([1.62, 0.0, 0.0], DimensionVector::ACCELERATION);

        let result = eval_builtin("gravity", std::slice::from_ref(&moon_gravity));

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("gravity".to_string())),
            "kind should be 'gravity'"
        );

        let accel = map
            .get(&Value::String("acceleration".to_string()))
            .expect("acceleration field must exist");

        // Vector3 arg round-trips unchanged (no sign flip, no Z-axis remap).
        assert_eq!(
            accel, &moon_gravity,
            "acceleration should round-trip the input Vector3 unchanged"
        );
    }

    // ── gravity constructor: 1-arg Scalar<Acceleration> form ─────────────────

    #[test]
    fn gravity_scalar_arg_returns_neg_z_vector() {
        // Positive magnitude → acceleration in -Z direction.
        let mag = Value::Scalar {
            si_value: 9.81,
            dimension: DimensionVector::ACCELERATION,
        };

        let result = eval_builtin("gravity", &[mag]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("gravity".to_string())),
            "kind should be 'gravity'"
        );

        let accel = map
            .get(&Value::String("acceleration".to_string()))
            .expect("acceleration field must exist");

        // magnitude 9.81 m/s², placed in -Z.
        assert_vector3_approx!(Vector, accel.clone(), [0.0, 0.0, -9.81]);
    }

    // ── task 3540 step-19 (RED): post-retirement contract ────────────────────
    //
    // After step-20 (SIR-α stdlib swap), `point_load` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def PointLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PointLoad(...)` calls then lower to `CompiledExprKind::StructureInstanceCtor`
    // (the precedence path landed in step-16) and eval into a
    // `Value::StructureInstance`. The `eval_builtin("point_load", ...)` Rust API
    // path (used by tests below) returns `Value::Undef` because the dispatch
    // arm in `eval_loads` is removed.
    //
    // RED: this test currently fails because `eval_builtin("point_load", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-20 retires
    // the arm and updates `LOAD_KINDS` so the partition guard stays green.

    #[test]
    fn point_load_eval_builtin_returns_undef_post_retirement() {
        let selector = selector_stub("point_stub");
        let force = make_scalar_vec3([5000.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[selector, force]).is_undef(),
            "after step-20 retirement, eval_builtin('point_load', ...) must \
             return Undef; the structure-instance ctor path replaces the \
             builtin entirely (PRD §6, Q-SIR-4 — rename point_load → PointLoad)"
        );
    }

    // ── task 3544 step-3 (RED): post-retirement contract ─────────────────────
    //
    // After step-4 (SIR-β-load stdlib swap), `pressure_load` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def PressureLoad : Load { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PressureLoad(...)` calls then lower to `CompiledExprKind::StructureInstanceCtor`
    // (the precedence path landed in SIR-α step-16) and eval into a
    // `Value::StructureInstance`. The `eval_builtin("pressure_load", ...)` Rust API
    // path returns `Value::Undef` because the dispatch arm in `eval_loads` is removed.
    //
    // RED: this test currently fails because `eval_builtin("pressure_load", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-4 retires
    // the arm and updates `LOAD_KINDS` so the partition guard stays green.

    #[test]
    fn pressure_load_eval_builtin_returns_undef_post_retirement() {
        let stub = selector_stub("face_stub");
        let pressure_mag = Value::Scalar {
            si_value: 1e6,
            dimension: DimensionVector::PRESSURE,
        };
        assert!(
            eval_builtin("pressure_load", &[stub, pressure_mag]).is_undef(),
            "after step-4 retirement, eval_builtin('pressure_load', ...) must \
             return Undef; the structure-instance ctor path replaces the \
             builtin entirely (PRD §8 Phase 2, Q-SIR-4 — rename pressure_load → PressureLoad)"
        );
    }

    // ── LOAD_KINDS partition test ─────────────────────────────────────────────

    /// Guard that every kind listed in `LOAD_KINDS` is actually dispatched by
    /// `eval_loads`.  If a kind is renamed or removed in `eval_loads` but not
    /// updated in `LOAD_KINDS` (or vice versa), this test will catch it.
    #[test]
    fn load_kinds_all_dispatched_by_eval_loads() {
        use super::{LOAD_KINDS, eval_loads, is_load_value};

        // `stub_selector` removed: was needed for `traction_load` / `body_force`
        // fixture arms, both now retired. `gravity` takes only a dimensioned
        // value, not a selector.
        //
        // `pressure_mag` removed: `pressure_load` retired in SIR-β-load (task
        // 3544 step-4) — no longer in LOAD_KINDS.
        // `pressure_vec` removed: `traction_load` retired in FEA-2 (task 2881
        // step-4) — no longer in LOAD_KINDS.
        // `fd_vec` removed: `body_force` retired in FEA-2 (task 2881 step-8)
        // — no longer in LOAD_KINDS. Only `accel_vec` (for `gravity`) remains.
        let accel_vec = make_scalar_vec3([0.0, 0.0, -9.81], DimensionVector::ACCELERATION);

        for kind in LOAD_KINDS {
            // `point_load` retired in SIR-α wave-1 (step-20) — no longer in LOAD_KINDS.
            // `pressure_load` retired in SIR-β-load (task 3544 step-4) — same.
            // `traction_load` retired in FEA-2 (task 2881 step-4) — same.
            // `body_force` retired in FEA-2 (task 2881 step-8) — same.
            let result = match *kind {
                "gravity" => eval_loads(kind, std::slice::from_ref(&accel_vec)),
                other => panic!(
                    "LOAD_KINDS contains '{}' but no fixture is defined for it — \
                     add a fixture arm to this test and an arm to eval_loads",
                    other
                ),
            };

            assert!(
                result.is_some(),
                "eval_loads('{}', ...) returned None — LOAD_KINDS is out of sync \
                 with eval_loads dispatch arms",
                kind
            );

            // Also verify the returned Map is recognized by is_load_value.
            let value = result.unwrap();
            assert!(
                is_load_value(&value),
                "is_load_value should recognize a Map produced by eval_loads('{}', ...)",
                kind
            );
        }
    }
}
