//! FEA load constructors for the stdlib.
//!
//! Provides `point_load`, `pressure_load`, `traction_load`, `body_force`, and
//! `gravity` constructors.  Each returns a `Value::Map` with a `kind`
//! discriminator field, matching the joints/coupling constructor pattern.
//!
//! Selector-target validation is delegated to
//! [`crate::helpers::validate_selector_target`].  Selector targets are
//! validated as a narrow placeholder set (`Value::Map` or `Value::String`)
//! until the topology-selector variants land — see that helper's doc-comment
//! for the full narrowed contract and PRD task 16 deadline reference.

use reify_types::{DimensionVector, Value};

use crate::helpers::{
    make_kind_map, validate_dimensioned_scalar, validate_dimensioned_vec3,
    validate_dimensionless_unit_axis_vec3, validate_selector_target,
};

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
#[allow(dead_code)]
pub(crate) const LOAD_KINDS: &[&str] = &[
    "pressure_load",
    "traction_load",
    "body_force",
    "gravity",
];

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
        "pressure_load" => {
            // Accept arity 2 (direction defaults to "normal") or 3 (explicit direction).
            if args.len() != 2 && args.len() != 3 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_scalar(&args[1], DimensionVector::PRESSURE).is_none() {
                return Some(Value::Undef);
            }
            let direction = if args.len() == 2 {
                Value::String("normal".to_string())
            } else {
                match validate_pressure_direction(&args[2]) {
                    Some(d) => d,
                    None => return Some(Value::Undef),
                }
            };
            make_kind_map(
                "pressure_load",
                vec![
                    ("direction", direction),
                    ("face", args[0].clone()),
                    ("magnitude", args[1].clone()),
                ],
            )
        }
        "traction_load" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_vec3(&args[1], DimensionVector::PRESSURE).is_none() {
                return Some(Value::Undef);
            }
            make_kind_map(
                "traction_load",
                vec![("face", args[0].clone()), ("traction", args[1].clone())],
            )
        }
        "body_force" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_vec3(&args[1], DimensionVector::FORCE_DENSITY).is_none() {
                return Some(Value::Undef);
            }
            make_kind_map(
                "body_force",
                vec![
                    ("body", args[0].clone()),
                    ("force_density", args[1].clone()),
                ],
            )
        }
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

/// Validate a pressure-load direction argument.
///
/// Accepts:
/// - `Value::String("normal")` — the outward-face-normal sentinel.
/// - `Value::Vector` (or Tensor/Point) of exactly 3 `DIMENSIONLESS` components,
///   all finite, with a non-zero, finite squared magnitude.
///
/// Returns `Some(value)` (the original input, un-normalized) on success,
/// `None` on failure. Any other String content, dimensioned Vector,
/// non-3-component Vector, or primitive input returns `None`.
///
/// The non-sentinel branch delegates to
/// [`helpers::validate_dimensionless_unit_axis_vec3`] so the
/// `mag_sq.is_finite()` overflow guard (e.g. for `[f64::MAX, 0, 0]`) is
/// applied uniformly with `supports::validate_unit_axis_vec3` and
/// `joints::validate_axis`.
fn validate_pressure_direction(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) if s == "normal" => Some(v.clone()),
        _ => validate_dimensionless_unit_axis_vec3(v).map(|_| v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_types::{DimensionVector, Value};
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

    // ── pressure_load constructor: 3-arg happy path ──────────────────────────

    #[test]
    fn pressure_load_3arg_returns_map_with_correct_fields() {
        let face = selector_stub("face_stub");
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        // Explicit direction: -Z unit vector (dimensionless).
        let direction = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(-1.0)]);

        let result = eval_builtin(
            "pressure_load",
            &[face.clone(), magnitude.clone(), direction.clone()],
        );

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("pressure_load".to_string())),
            "kind should be 'pressure_load'"
        );
        assert_eq!(
            map.get(&Value::String("face".to_string())),
            Some(&face),
            "face should round-trip the selector input"
        );
        assert_eq!(
            map.get(&Value::String("magnitude".to_string())),
            Some(&magnitude),
            "magnitude should round-trip"
        );
        assert_eq!(
            map.get(&Value::String("direction".to_string())),
            Some(&direction),
            "direction should round-trip"
        );
    }

    #[test]
    fn pressure_load_direction_non_unit_vector_accepted() {
        let face = selector_stub("face_stub");
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        // Non-unit dimensionless direction — magnitude 5.0, not 1.0.
        let direction = Value::Vector(vec![Value::Real(5.0), Value::Real(0.0), Value::Real(0.0)]);

        let result = eval_builtin(
            "pressure_load",
            &[face.clone(), magnitude.clone(), direction.clone()],
        );

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("pressure_load".to_string())),
            "kind should be 'pressure_load'"
        );
        assert_eq!(
            map.get(&Value::String("direction".to_string())),
            Some(&direction),
            "direction should round-trip the un-normalized (5,0,0) input — \
             normalization happens downstream"
        );
    }

    // ── pressure_load: "normal" sentinel ────────────────────────────────────

    #[test]
    fn pressure_load_normal_string_direction_accepted() {
        let face = selector_stub("face_stub");
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        let normal_sentinel = Value::String("normal".to_string());

        let result = eval_builtin("pressure_load", &[face, magnitude, normal_sentinel.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("direction".to_string())),
            Some(&normal_sentinel),
            "direction should be Value::String(\"normal\") round-tripped"
        );
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("pressure_load".to_string())),
        );
    }

    // ── pressure_load: 2-arg form defaults to "normal" ──────────────────────

    #[test]
    fn pressure_load_2arg_defaults_direction_to_normal() {
        let face = selector_stub("face_stub");
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };

        let result = eval_builtin("pressure_load", &[face.clone(), magnitude.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("direction".to_string())),
            Some(&Value::String("normal".to_string())),
            "2-arg form should default direction to \"normal\""
        );
        assert_eq!(
            map.get(&Value::String("face".to_string())),
            Some(&face),
            "face should round-trip"
        );
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("pressure_load".to_string())),
        );
        assert_eq!(
            map.get(&Value::String("magnitude".to_string())),
            Some(&magnitude),
            "magnitude should round-trip"
        );
    }

    // ── pressure_load: failure modes ─────────────────────────────────────────

    #[test]
    fn pressure_load_magnitude_with_force_dim_returns_undef() {
        let force_dim_mag = Value::Scalar {
            si_value: 5000.0,
            dimension: DimensionVector::FORCE, // wrong: should be PRESSURE
        };
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), force_dim_mag]
            )
            .is_undef(),
            "magnitude with FORCE dimension should return Undef"
        );
    }

    #[test]
    fn pressure_load_magnitude_not_scalar_returns_undef() {
        let not_scalar = Value::Real(5.0);
        assert!(
            eval_builtin("pressure_load", &[selector_stub("face_stub"), not_scalar]).is_undef(),
            "magnitude = Real should return Undef"
        );
    }

    #[test]
    fn pressure_load_magnitude_nan_returns_undef() {
        let nan_mag = Value::Scalar {
            si_value: f64::NAN,
            dimension: DimensionVector::PRESSURE,
        };
        assert!(
            eval_builtin("pressure_load", &[selector_stub("face_stub"), nan_mag]).is_undef(),
            "magnitude NaN should return Undef"
        );
    }

    #[test]
    fn pressure_load_direction_length_dim_returns_undef() {
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        // Direction has LENGTH dimension — should be dimensionless.
        let bad_dir = make_scalar_vec3([0.0, 0.0, -1.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), pressure_mag, bad_dir]
            )
            .is_undef(),
            "direction with LENGTH dimension should return Undef"
        );
    }

    #[test]
    fn pressure_load_direction_zero_vector_returns_undef() {
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        // Dimensionless zero vector — invalid direction.
        let zero_dir = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), pressure_mag, zero_dir]
            )
            .is_undef(),
            "zero direction vector should return Undef"
        );
    }

    #[test]
    fn pressure_load_direction_overflow_vector_returns_undef() {
        // Regression: `[f64::MAX, 0.0, 0.0]` has squared magnitude
        // f64::MAX² → +inf. The pre-hoist `validate_pressure_direction`
        // only checked `mag_sq == 0.0` and silently accepted this input.
        // Routing through `helpers::validate_dimensionless_unit_axis_vec3`
        // applies the `mag_sq.is_finite()` guard uniformly with
        // `validate_axis` (joints) and `validate_unit_axis_vec3` (supports).
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        let overflow_dir = Value::Vector(vec![
            Value::Real(f64::MAX),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), pressure_mag, overflow_dir]
            )
            .is_undef(),
            "direction with squared-magnitude overflow (+inf) should return Undef"
        );
    }

    #[test]
    fn pressure_load_direction_wrong_string_returns_undef() {
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        let bad_sentinel = Value::String("up".to_string());
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), pressure_mag, bad_sentinel]
            )
            .is_undef(),
            "direction string other than \"normal\" should return Undef"
        );
    }

    #[test]
    fn pressure_load_selector_real_returns_undef() {
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        assert!(
            eval_builtin("pressure_load", &[Value::Real(0.0), pressure_mag]).is_undef(),
            "selector = Real should return Undef"
        );
    }

    #[test]
    fn pressure_load_zero_args_returns_undef() {
        assert!(
            eval_builtin("pressure_load", &[]).is_undef(),
            "0 args → Undef"
        );
    }

    #[test]
    fn pressure_load_one_arg_returns_undef() {
        assert!(
            eval_builtin("pressure_load", &[selector_stub("face_stub")]).is_undef(),
            "1 arg → Undef"
        );
    }

    #[test]
    fn pressure_load_four_args_returns_undef() {
        let pressure_mag = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        let dir = Value::String("normal".to_string());
        let extra = Value::Real(0.0);
        assert!(
            eval_builtin(
                "pressure_load",
                &[selector_stub("face_stub"), pressure_mag, dir, extra]
            )
            .is_undef(),
            "4 args → Undef"
        );
    }

    // ── traction_load constructor: happy path ────────────────────────────────

    #[test]
    fn traction_load_returns_map_with_correct_fields() {
        let face = selector_stub("face_stub");
        // Shear traction with normal+tangential components (Pa).
        let traction = make_scalar_vec3([2e6, 0.0, -1e6], DimensionVector::PRESSURE);

        let result = eval_builtin("traction_load", &[face.clone(), traction.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("traction_load".to_string())),
            "kind should be 'traction_load'"
        );
        assert_eq!(
            map.get(&Value::String("face".to_string())),
            Some(&face),
            "face should round-trip"
        );
        assert_eq!(
            map.get(&Value::String("traction".to_string())),
            Some(&traction),
            "traction should round-trip"
        );
    }

    // ── traction_load: failure modes ─────────────────────────────────────────

    #[test]
    fn traction_load_traction_force_dim_returns_undef() {
        let bad = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("traction_load", &[selector_stub("face_stub"), bad]).is_undef(),
            "traction with FORCE dim → Undef"
        );
    }

    #[test]
    fn traction_load_traction_dimensionless_returns_undef() {
        let bad = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("traction_load", &[selector_stub("face_stub"), bad]).is_undef(),
            "dimensionless traction → Undef"
        );
    }

    #[test]
    fn traction_load_traction_nan_returns_undef() {
        let nan_vec = make_scalar_vec3([f64::NAN, 0.0, 0.0], DimensionVector::PRESSURE);
        assert!(
            eval_builtin("traction_load", &[selector_stub("face_stub"), nan_vec]).is_undef(),
            "NaN traction component → Undef"
        );
    }

    #[test]
    fn traction_load_traction_vec2_returns_undef() {
        let vec2 = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::PRESSURE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::PRESSURE,
            },
        ]);
        assert!(
            eval_builtin("traction_load", &[selector_stub("face_stub"), vec2]).is_undef(),
            "2-component traction → Undef"
        );
    }

    #[test]
    fn traction_load_traction_real_returns_undef() {
        assert!(
            eval_builtin(
                "traction_load",
                &[selector_stub("face_stub"), Value::Real(1.0)]
            )
            .is_undef(),
            "traction = Real → Undef"
        );
    }

    #[test]
    fn traction_load_selector_int_returns_undef() {
        let traction = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::PRESSURE);
        assert!(
            eval_builtin("traction_load", &[Value::Int(7), traction]).is_undef(),
            "selector = Int → Undef"
        );
    }

    #[test]
    fn traction_load_zero_args_returns_undef() {
        assert!(
            eval_builtin("traction_load", &[]).is_undef(),
            "0 args → Undef"
        );
    }

    #[test]
    fn traction_load_one_arg_returns_undef() {
        assert!(
            eval_builtin("traction_load", &[selector_stub("face_stub")]).is_undef(),
            "1 arg → Undef"
        );
    }

    #[test]
    fn traction_load_three_args_returns_undef() {
        let traction = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::PRESSURE);
        assert!(
            eval_builtin(
                "traction_load",
                &[selector_stub("face_stub"), traction.clone(), traction]
            )
            .is_undef(),
            "3 args → Undef"
        );
    }

    // ── body_force constructor: happy path ───────────────────────────────────

    #[test]
    fn body_force_returns_map_with_correct_fields() {
        let body = selector_stub("body_stub");
        // Weight-density of steel ≈ 7850 kg/m³ × 9.81 m/s² ≈ 77 kN/m³.
        let fd = make_scalar_vec3([0.0, 0.0, -77000.0], DimensionVector::FORCE_DENSITY);

        let result = eval_builtin("body_force", &[body.clone(), fd.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("body_force".to_string())),
            "kind should be 'body_force'"
        );
        assert_eq!(
            map.get(&Value::String("body".to_string())),
            Some(&body),
            "body should round-trip"
        );
        assert_eq!(
            map.get(&Value::String("force_density".to_string())),
            Some(&fd),
            "force_density should round-trip"
        );
    }

    // ── body_force: failure modes ─────────────────────────────────────────────

    #[test]
    fn body_force_force_dim_returns_undef() {
        // FORCE instead of ForceDensity.
        let bad = make_scalar_vec3([0.0, 0.0, -9.81], DimensionVector::FORCE);
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub"), bad]).is_undef(),
            "FORCE dim → Undef"
        );
    }

    #[test]
    fn body_force_pressure_dim_returns_undef() {
        let bad = make_scalar_vec3([0.0, 0.0, -9.81], DimensionVector::PRESSURE);
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub"), bad]).is_undef(),
            "PRESSURE dim → Undef"
        );
    }

    #[test]
    fn body_force_inf_component_returns_undef() {
        let inf_vec = make_scalar_vec3([f64::INFINITY, 0.0, 0.0], DimensionVector::FORCE_DENSITY);
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub"), inf_vec]).is_undef(),
            "Inf component → Undef"
        );
    }

    #[test]
    fn body_force_vec4_returns_undef() {
        let dim = DimensionVector::FORCE_DENSITY;
        let vec4 = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            },
            Value::Scalar {
                si_value: -77000.0,
                dimension: dim,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            },
        ]);
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub"), vec4]).is_undef(),
            "4-component vector → Undef"
        );
    }

    #[test]
    fn body_force_selector_bool_returns_undef() {
        let fd = make_scalar_vec3([0.0, 0.0, -77000.0], DimensionVector::FORCE_DENSITY);
        assert!(
            eval_builtin("body_force", &[Value::Bool(false), fd]).is_undef(),
            "selector = Bool → Undef"
        );
    }

    #[test]
    fn body_force_zero_args_returns_undef() {
        assert!(eval_builtin("body_force", &[]).is_undef(), "0 args → Undef");
    }

    #[test]
    fn body_force_one_arg_returns_undef() {
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub")]).is_undef(),
            "1 arg → Undef"
        );
    }

    #[test]
    fn body_force_three_args_returns_undef() {
        let fd = make_scalar_vec3([0.0, 0.0, -77000.0], DimensionVector::FORCE_DENSITY);
        assert!(
            eval_builtin("body_force", &[selector_stub("body_stub"), fd.clone(), fd]).is_undef(),
            "3 args → Undef"
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

        let stub_selector = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("stub".to_string()),
            );
            m
        });
        let pressure_mag = Value::Scalar {
            si_value: 1e6,
            dimension: DimensionVector::PRESSURE,
        };
        let pressure_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::PRESSURE);
        let fd_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE_DENSITY);
        let accel_vec = make_scalar_vec3([0.0, 0.0, -9.81], DimensionVector::ACCELERATION);

        for kind in LOAD_KINDS {
            // `point_load` retired in SIR-α wave-1 (step-20) — no longer in
            // LOAD_KINDS, so no fixture arm here.
            let result = match *kind {
                "pressure_load" => eval_loads(kind, &[stub_selector.clone(), pressure_mag.clone()]),
                "traction_load" => eval_loads(kind, &[stub_selector.clone(), pressure_vec.clone()]),
                "body_force" => eval_loads(kind, &[stub_selector.clone(), fd_vec.clone()]),
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
