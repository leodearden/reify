//! FEA load constructors for the stdlib.
//!
//! Provides `point_load`, `pressure_load`, `traction_load`, `body_force`, and
//! `gravity` constructors.  Each returns a `Value::Map` with a `kind`
//! discriminator field, matching the joints/coupling constructor pattern.
//!
//! ## Selector-target validation
//!
//! The topology-selector stdlib bindings (PRD `topology-selectors.md` task 5)
//! have not yet landed — there is no `Value::Face` / `Value::Edge` / `Value::Body`
//! variant today.  The `validate_selector_target` helper therefore only rejects
//! obvious primitive non-selector values (`Value::Real`, `Value::Int`,
//! `Value::Bool`, `Value::Undef`); any other shape (Map, List, String, …) is
//! accepted as an opaque pass-through.  Full topology-kind validation belongs
//! in the FEA evaluation pipeline (PRD task 16) when the engine resolves
//! selectors against the kernel and can produce diagnostics with source spans.

use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

use crate::helpers::tensor_components_f64;

/// Earth standard gravity in m/s² (CGPM 1901 definition).
pub(crate) const EARTH_GRAVITY: f64 = 9.80665;

/// Canonical set of load kinds recognized by this module.
///
/// Analogous to `joints::JOINT_KINDS`. Future FEA-solver consumers can use
/// this constant for load-kind membership checks.
pub(crate) const LOAD_KINDS: &[&str] = &[
    "point_load",
    "pressure_load",
    "traction_load",
    "body_force",
    "gravity",
];

/// Returns the acceleration dimension: m·s⁻² (LENGTH / TIME²).
///
/// Composed at runtime because `from_exps` is module-private in `dimension.rs`
/// and `mul`/`div`/`pow` are not `const fn`. Replace with a named constant
/// once `DimensionVector::ACCELERATION` is added to `reify-types`.
pub(crate) fn acceleration_dim() -> DimensionVector {
    DimensionVector::LENGTH.div(&DimensionVector::TIME.pow(2))
}

/// Returns the force-density dimension: N/m³ = kg·m⁻²·s⁻² (FORCE / VOLUME).
///
/// Composed at runtime for the same reason as `acceleration_dim`. Replace with
/// `DimensionVector::FORCE_DENSITY` once that constant is added to `reify-types`.
pub(crate) fn force_density_dim() -> DimensionVector {
    DimensionVector::FORCE.div(&DimensionVector::VOLUME)
}

/// Evaluate a loads stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_loads(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "point_load" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_vec3(&args[1], DimensionVector::FORCE).is_none() {
                return Some(Value::Undef);
            }
            make_load_map("point_load", &[
                ("force", args[1].clone()),
                ("point", args[0].clone()),
            ])
        }
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
            make_load_map("pressure_load", &[
                ("direction", direction),
                ("face", args[0].clone()),
                ("magnitude", args[1].clone()),
            ])
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
            make_load_map("traction_load", &[
                ("face", args[0].clone()),
                ("traction", args[1].clone()),
            ])
        }
        _ => return None,
    })
}

// ── Helper: Map builder ──────────────────────────────────────────────────────

/// Build a load `Value::Map` with a `kind` field plus the given extra fields.
///
/// Fields are inserted into a `BTreeMap`, which sorts them alphabetically.
/// The `kind` key is always included.  Callers pass extra `(name, value)` pairs
/// in any order — alphabetical order is guaranteed by `BTreeMap`.
fn make_load_map(kind: &str, fields: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String(kind.to_string()),
    );
    for (k, v) in fields {
        m.insert(Value::String(k.to_string()), v.clone());
    }
    Value::Map(m)
}

// ── Validators ───────────────────────────────────────────────────────────────

/// Validate that `v` is a `Value::Vector` (or Tensor/Point) of exactly 3
/// numeric components with a consistent dimension matching `expected_dim`,
/// all finite.
///
/// Returns `Some([x, y, z])` on success, `None` on any failure.
fn validate_dimensioned_vec3(v: &Value, expected_dim: DimensionVector) -> Option<[f64; 3]> {
    let (vals, dim) = tensor_components_f64(v)?;
    if vals.len() != 3 {
        return None;
    }
    if dim != expected_dim {
        return None;
    }
    if vals.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some([vals[0], vals[1], vals[2]])
}

/// Validate that `v` is a `Value::Scalar` with dimension matching `expected_dim`
/// and a finite SI value.
///
/// Returns `Some(si_value)` on success, `None` on any failure.
fn validate_dimensioned_scalar(v: &Value, expected_dim: DimensionVector) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension != expected_dim {
                return None;
            }
            if !si_value.is_finite() {
                return None;
            }
            Some(*si_value)
        }
        _ => None,
    }
}

/// Validate that `v` is a usable topology-selector target — i.e., not an
/// obvious primitive.
///
/// Rejects `Value::Real`, `Value::Int`, `Value::Bool`, and `Value::Undef`.
/// All other shapes (Map, List, String, Vector, Tensor, …) are accepted as
/// opaque pass-through values (see module-level doc for rationale).
///
/// Returns `Some(())` when the value is an acceptable selector, `None` when
/// it is a primitive that cannot be a selector.
fn validate_selector_target(v: &Value) -> Option<()> {
    match v {
        Value::Real(_) | Value::Int(_) | Value::Bool(_) | Value::Undef => None,
        _ => Some(()),
    }
}

/// Validate a pressure-load direction argument.
///
/// Accepts:
/// - `Value::String("normal")` — the outward-face-normal sentinel.
/// - `Value::Vector` (or Tensor/Point) of exactly 3 `DIMENSIONLESS` components,
///   all finite, with a non-zero magnitude.
///
/// Returns `Some(value)` (the original input) on success, `None` on failure.
/// Any other String content, dimensioned Vector, non-3-component Vector, or
/// primitive input returns `None`.
fn validate_pressure_direction(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) if s == "normal" => Some(v.clone()),
        _ => {
            let (vals, dim) = tensor_components_f64(v)?;
            if vals.len() != 3 {
                return None;
            }
            if dim != DimensionVector::DIMENSIONLESS {
                return None;
            }
            if vals.iter().any(|x| !x.is_finite()) {
                return None;
            }
            // Reject zero vector (direction has no meaning for zero magnitude).
            let mag_sq = vals[0] * vals[0] + vals[1] * vals[1] + vals[2] * vals[2];
            if mag_sq == 0.0 {
                return None;
            }
            Some(v.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_types::{DimensionVector, Value};
    use std::collections::BTreeMap;

    /// Build a simple opaque selector stub (Map with kind="point_stub").
    fn point_selector_stub() -> Value {
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("point_stub".to_string()),
            );
            m
        })
    }

    // ── point_load constructor: happy path ───────────────────────────────────

    #[test]
    fn point_load_returns_map_with_correct_fields() {
        // Opaque selector stub: a Map that is clearly not a primitive.
        let selector = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("point_stub".to_string()),
            );
            m
        });
        let force = make_scalar_vec3([5000.0, 0.0, 0.0], DimensionVector::FORCE);

        let result = eval_builtin("point_load", &[selector.clone(), force.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("point_load".to_string())),
            "kind field should be 'point_load'"
        );
        assert_eq!(
            map.get(&Value::String("point".to_string())),
            Some(&selector),
            "point field should round-trip the selector input"
        );
        assert_eq!(
            map.get(&Value::String("force".to_string())),
            Some(&force),
            "force field should round-trip the force input"
        );
    }

    // ── point_load constructor: failure modes ────────────────────────────────

    #[test]
    fn point_load_zero_args_returns_undef() {
        assert!(
            eval_builtin("point_load", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn point_load_one_arg_returns_undef() {
        assert!(
            eval_builtin("point_load", &[point_selector_stub()]).is_undef(),
            "one arg should return Undef"
        );
    }

    #[test]
    fn point_load_three_args_returns_undef() {
        let force = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[point_selector_stub(), force.clone(), force]).is_undef(),
            "three args should return Undef"
        );
    }

    #[test]
    fn point_load_force_with_length_dim_returns_undef() {
        let wrong_dim_force = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin("point_load", &[point_selector_stub(), wrong_dim_force]).is_undef(),
            "force with LENGTH dimension should return Undef"
        );
    }

    #[test]
    fn point_load_force_with_nan_component_returns_undef() {
        let nan_force = make_scalar_vec3([f64::NAN, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[point_selector_stub(), nan_force]).is_undef(),
            "force with NaN component should return Undef"
        );
    }

    #[test]
    fn point_load_force_vec2_returns_undef() {
        // Vector with only 2 components — wrong arity.
        let vec2 = Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::FORCE,
            },
        ]);
        assert!(
            eval_builtin("point_load", &[point_selector_stub(), vec2]).is_undef(),
            "force Vec2 should return Undef"
        );
    }

    #[test]
    fn point_load_force_not_a_vector_returns_undef() {
        let scalar = Value::Real(5.0);
        assert!(
            eval_builtin("point_load", &[point_selector_stub(), scalar]).is_undef(),
            "force = Real should return Undef"
        );
    }

    #[test]
    fn point_load_selector_real_returns_undef() {
        let force = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[Value::Real(0.0), force]).is_undef(),
            "selector = Real should return Undef"
        );
    }

    #[test]
    fn point_load_selector_bool_returns_undef() {
        let force = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[Value::Bool(true), force]).is_undef(),
            "selector = Bool should return Undef"
        );
    }

    #[test]
    fn point_load_selector_undef_returns_undef() {
        let force = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("point_load", &[Value::Undef, force]).is_undef(),
            "selector = Undef should return Undef"
        );
    }

    // ── Helper: face selector stub ───────────────────────────────────────────

    fn face_selector_stub() -> Value {
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("face_stub".to_string()),
            );
            m
        })
    }

    fn body_selector_stub() -> Value {
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("body_stub".to_string()),
            );
            m
        })
    }

    // ── pressure_load constructor: 3-arg happy path ──────────────────────────

    #[test]
    fn pressure_load_3arg_returns_map_with_correct_fields() {
        let face = face_selector_stub();
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        // Explicit direction: -Z unit vector (dimensionless).
        let direction = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(-1.0),
        ]);

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

    // ── pressure_load: "normal" sentinel ────────────────────────────────────

    #[test]
    fn pressure_load_normal_string_direction_accepted() {
        let face = face_selector_stub();
        let magnitude = Value::Scalar {
            si_value: 5e6,
            dimension: DimensionVector::PRESSURE,
        };
        let normal_sentinel = Value::String("normal".to_string());

        let result = eval_builtin(
            "pressure_load",
            &[face, magnitude, normal_sentinel.clone()],
        );

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
        let face = face_selector_stub();
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
            eval_builtin("pressure_load", &[face_selector_stub(), force_dim_mag]).is_undef(),
            "magnitude with FORCE dimension should return Undef"
        );
    }

    #[test]
    fn pressure_load_magnitude_not_scalar_returns_undef() {
        let not_scalar = Value::Real(5.0);
        assert!(
            eval_builtin("pressure_load", &[face_selector_stub(), not_scalar]).is_undef(),
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
            eval_builtin("pressure_load", &[face_selector_stub(), nan_mag]).is_undef(),
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
                &[face_selector_stub(), pressure_mag, bad_dir]
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
        let zero_dir = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin(
                "pressure_load",
                &[face_selector_stub(), pressure_mag, zero_dir]
            )
            .is_undef(),
            "zero direction vector should return Undef"
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
                &[face_selector_stub(), pressure_mag, bad_sentinel]
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
        assert!(eval_builtin("pressure_load", &[]).is_undef(), "0 args → Undef");
    }

    #[test]
    fn pressure_load_one_arg_returns_undef() {
        assert!(
            eval_builtin("pressure_load", &[face_selector_stub()]).is_undef(),
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
                &[face_selector_stub(), pressure_mag, dir, extra]
            )
            .is_undef(),
            "4 args → Undef"
        );
    }

    // ── traction_load constructor: happy path ────────────────────────────────

    #[test]
    fn traction_load_returns_map_with_correct_fields() {
        let face = face_selector_stub();
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
            eval_builtin("traction_load", &[face_selector_stub(), bad]).is_undef(),
            "traction with FORCE dim → Undef"
        );
    }

    #[test]
    fn traction_load_traction_dimensionless_returns_undef() {
        let bad = Value::Vector(vec![
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("traction_load", &[face_selector_stub(), bad]).is_undef(),
            "dimensionless traction → Undef"
        );
    }

    #[test]
    fn traction_load_traction_nan_returns_undef() {
        let nan_vec = make_scalar_vec3([f64::NAN, 0.0, 0.0], DimensionVector::PRESSURE);
        assert!(
            eval_builtin("traction_load", &[face_selector_stub(), nan_vec]).is_undef(),
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
            eval_builtin("traction_load", &[face_selector_stub(), vec2]).is_undef(),
            "2-component traction → Undef"
        );
    }

    #[test]
    fn traction_load_traction_real_returns_undef() {
        assert!(
            eval_builtin("traction_load", &[face_selector_stub(), Value::Real(1.0)]).is_undef(),
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
        assert!(eval_builtin("traction_load", &[]).is_undef(), "0 args → Undef");
    }

    #[test]
    fn traction_load_one_arg_returns_undef() {
        assert!(
            eval_builtin("traction_load", &[face_selector_stub()]).is_undef(),
            "1 arg → Undef"
        );
    }

    #[test]
    fn traction_load_three_args_returns_undef() {
        let traction = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::PRESSURE);
        assert!(
            eval_builtin(
                "traction_load",
                &[face_selector_stub(), traction.clone(), traction]
            )
            .is_undef(),
            "3 args → Undef"
        );
    }

    // ── body_force constructor: happy path ───────────────────────────────────

    #[test]
    fn body_force_returns_map_with_correct_fields() {
        use super::{force_density_dim};

        let body = body_selector_stub();
        // Weight-density of steel ≈ 7850 kg/m³ × 9.81 m/s² ≈ 77 kN/m³.
        let fd = make_scalar_vec3([0.0, 0.0, -77000.0], force_density_dim());

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

    #[test]
    fn force_density_dim_equals_force_div_volume() {
        use super::force_density_dim;
        assert_eq!(
            force_density_dim(),
            DimensionVector::FORCE.div(&DimensionVector::VOLUME),
            "force_density_dim() should equal FORCE / VOLUME"
        );
    }
}
