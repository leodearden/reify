//! FEA support (boundary-condition) constructors for the stdlib.
//!
//! Provides `FixedSupport`, `DisplacementSupport`, and `RollerSupport`
//! constructors.  Each returns a `Value::Map` with a `kind` discriminator
//! field, matching the loads/joints constructor pattern.
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

/// Canonical set of support kinds recognized by this module.
///
/// Analogous to `loads::LOAD_KINDS`.  Consumed by `is_support_value` and
/// guarded by the `support_kinds_all_dispatched_by_eval_supports` partition
/// test to prevent silent drift between this list and `eval_supports`'s
/// dispatch arms.
///
/// Not yet referenced by any external caller — the FEA solver (PRD task 16)
/// will wire this up when it lands.
#[allow(dead_code)]
pub(crate) const SUPPORT_KINDS: &[&str] =
    &["fixed_support", "displacement_support", "roller_support"];

/// Returns `true` if `v` is a support `Value::Map` produced by this module —
/// i.e., a Map with a `kind` field whose value is one of `SUPPORT_KINDS`.
///
/// Analogous to `loads::is_load_value`.  Used by the FEA solver (PRD
/// task 16) once it lands; not yet called from any external module.
#[allow(dead_code)]
pub(crate) fn is_support_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => m
            .get(&Value::String("kind".to_string()))
            .and_then(|k| if let Value::String(s) = k { Some(s.as_str()) } else { None })
            .is_some_and(|s| SUPPORT_KINDS.contains(&s)),
        _ => false,
    }
}

/// Evaluate a supports stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_supports(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "FixedSupport" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            make_support_map("fixed_support", vec![("target", args[0].clone())])
        }
        "DisplacementSupport" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_vec3(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            make_support_map("displacement_support", vec![
                ("displacement", args[1].clone()),
                ("target", args[0].clone()),
            ])
        }
        "RollerSupport" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_unit_axis_vec3(&args[1]).is_none() {
                return Some(Value::Undef);
            }
            make_support_map("roller_support", vec![
                ("normal", args[1].clone()),
                ("target", args[0].clone()),
            ])
        }
        _ => return None,
    })
}

// ── Builder ───────────────────────────────────────────────────────────────────

fn make_support_map(kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String(kind.to_string()),
    );
    for (k, v) in fields {
        m.insert(Value::String(k.to_string()), v);
    }
    Value::Map(m)
}

// ── Validators ────────────────────────────────────────────────────────────────

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

/// Validate that `v` is a `Value::Vector` (or Tensor/Point) of exactly 3
/// numeric components with a consistent dimension matching `expected_dim`,
/// all finite.
///
/// Returns `Some(())` on success, `None` on any failure.
fn validate_dimensioned_vec3(v: &Value, expected_dim: DimensionVector) -> Option<()> {
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
    Some(())
}

/// Validate that `v` is a `Value::Vector` (or Tensor/Point) of exactly 3
/// dimensionless components, all finite, with a non-zero squared magnitude.
///
/// Mirrors `joints::validate_axis` semantics but returns `Option<()>` (unit)
/// instead of `Option<[f64; 3]>` since the RollerSupport arm round-trips the
/// original `Value` rather than reconstructing from extracted components.
///
/// The normal is stored un-normalized — magnitude is preserved at consume time
/// (joints precedent; see design decisions).
fn validate_unit_axis_vec3(v: &Value) -> Option<()> {
    let (comps, dim) = tensor_components_f64(v)?;
    if comps.len() != 3 {
        return None;
    }
    if dim != DimensionVector::DIMENSIONLESS {
        return None;
    }
    if comps.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let mag_sq = comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2];
    if mag_sq == 0.0 || !mag_sq.is_finite() {
        return None;
    }
    Some(())
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

    // ── FixedSupport constructor: happy path ──────────────────────────────────

    #[test]
    fn fixed_support_returns_map_with_correct_fields() {
        let selector = point_selector_stub();

        let result = eval_builtin("FixedSupport", &[selector.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("fixed_support".to_string())),
            "kind field should be 'fixed_support'"
        );
        assert_eq!(
            map.get(&Value::String("target".to_string())),
            Some(&selector),
            "target field should round-trip the selector input"
        );
    }

    // ── FixedSupport constructor: failure modes ───────────────────────────────

    #[test]
    fn fixed_support_zero_args_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn fixed_support_two_args_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[point_selector_stub(), point_selector_stub()])
                .is_undef(),
            "two args should return Undef"
        );
    }

    #[test]
    fn fixed_support_real_target_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[Value::Real(1.0)]).is_undef(),
            "Real target should return Undef"
        );
    }

    #[test]
    fn fixed_support_int_target_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[Value::Int(7)]).is_undef(),
            "Int target should return Undef"
        );
    }

    #[test]
    fn fixed_support_bool_target_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[Value::Bool(true)]).is_undef(),
            "Bool target should return Undef"
        );
    }

    #[test]
    fn fixed_support_undef_target_returns_undef() {
        assert!(
            eval_builtin("FixedSupport", &[Value::Undef]).is_undef(),
            "Undef target should return Undef"
        );
    }

    // ── DisplacementSupport constructor: happy path ───────────────────────────

    #[test]
    fn displacement_support_returns_map_with_correct_fields() {
        let target = point_selector_stub();
        let displacement = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);

        let result = eval_builtin("DisplacementSupport", &[target.clone(), displacement.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("displacement_support".to_string())),
            "kind field should be 'displacement_support'"
        );
        assert_eq!(
            map.get(&Value::String("target".to_string())),
            Some(&target),
            "target field should round-trip the input"
        );
        assert_eq!(
            map.get(&Value::String("displacement".to_string())),
            Some(&displacement),
            "displacement field should round-trip the input"
        );
    }

    // ── DisplacementSupport constructor: failure modes ────────────────────────

    #[test]
    fn displacement_support_zero_args_returns_undef() {
        assert!(eval_builtin("DisplacementSupport", &[]).is_undef());
    }

    #[test]
    fn displacement_support_one_arg_returns_undef() {
        assert!(eval_builtin("DisplacementSupport", &[point_selector_stub()]).is_undef());
    }

    #[test]
    fn displacement_support_three_args_returns_undef() {
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin(
                "DisplacementSupport",
                &[point_selector_stub(), disp.clone(), disp]
            )
            .is_undef()
        );
    }

    #[test]
    fn displacement_support_invalid_target_returns_undef() {
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("DisplacementSupport", &[Value::Real(1.0), disp]).is_undef());
    }

    #[test]
    fn displacement_support_dimensionless_displacement_returns_undef() {
        // A raw dimensionless vector (Value::Real components) — not LENGTH-dimensioned.
        let dimensionless = Value::Vector(vec![
            Value::Real(0.001),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), dimensionless]).is_undef()
        );
    }

    #[test]
    fn displacement_support_force_dimensioned_displacement_returns_undef() {
        let force_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), force_vec]).is_undef()
        );
    }

    #[test]
    fn displacement_support_two_component_displacement_returns_undef() {
        // Only 2 LENGTH-dimensioned scalar components — wrong arity.
        let two_comp = Value::Vector(vec![
            Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
        ]);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), two_comp]).is_undef()
        );
    }

    #[test]
    fn displacement_support_nan_displacement_returns_undef() {
        let nan_disp = make_scalar_vec3([f64::NAN, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), nan_disp]).is_undef()
        );
    }

    #[test]
    fn displacement_support_inf_displacement_returns_undef() {
        let inf_disp = make_scalar_vec3([f64::INFINITY, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), inf_disp]).is_undef()
        );
    }

    #[test]
    fn displacement_support_non_vector_displacement_returns_undef() {
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), Value::Real(0.001)])
                .is_undef()
        );
    }

    // ── RollerSupport constructor: happy path ─────────────────────────────────

    #[test]
    fn roller_support_returns_map_with_correct_fields() {
        let target = point_selector_stub();
        // Raw dimensionless z-unit vector (un-normalized intentionally — joints precedent).
        // The magnitude is preserved at consume time.
        let normal = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);

        let result = eval_builtin("RollerSupport", &[target.clone(), normal.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("roller_support".to_string())),
            "kind field should be 'roller_support'"
        );
        assert_eq!(
            map.get(&Value::String("target".to_string())),
            Some(&target),
            "target field should round-trip the input"
        );
        assert_eq!(
            map.get(&Value::String("normal".to_string())),
            Some(&normal),
            "normal field should round-trip the raw input vector"
        );
    }

    // ── RollerSupport constructor: failure modes ──────────────────────────────

    #[test]
    fn roller_support_zero_args_returns_undef() {
        assert!(eval_builtin("RollerSupport", &[]).is_undef());
    }

    #[test]
    fn roller_support_one_arg_returns_undef() {
        assert!(eval_builtin("RollerSupport", &[point_selector_stub()]).is_undef());
    }

    #[test]
    fn roller_support_three_args_returns_undef() {
        let normal = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(
            eval_builtin(
                "RollerSupport",
                &[point_selector_stub(), normal.clone(), normal]
            )
            .is_undef()
        );
    }

    #[test]
    fn roller_support_invalid_target_returns_undef() {
        let normal = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(eval_builtin("RollerSupport", &[Value::Real(1.0), normal]).is_undef());
    }

    #[test]
    fn roller_support_zero_magnitude_normal_returns_undef() {
        let zero_normal = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("RollerSupport", &[point_selector_stub(), zero_normal]).is_undef()
        );
    }

    #[test]
    fn roller_support_nan_normal_returns_undef() {
        let nan_normal = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), nan_normal]).is_undef());
    }

    #[test]
    fn roller_support_inf_normal_returns_undef() {
        let inf_normal = Value::Vector(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), inf_normal]).is_undef());
    }

    #[test]
    fn roller_support_length_dimensioned_normal_returns_undef() {
        // LENGTH-dimensioned rather than dimensionless — should reject.
        let length_normal = make_scalar_vec3([0.0, 0.0, 1.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin("RollerSupport", &[point_selector_stub(), length_normal]).is_undef()
        );
    }

    #[test]
    fn roller_support_two_component_normal_returns_undef() {
        let two_comp = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("RollerSupport", &[point_selector_stub(), two_comp]).is_undef()
        );
    }

    #[test]
    fn roller_support_non_vector_normal_returns_undef() {
        assert!(
            eval_builtin("RollerSupport", &[point_selector_stub(), Value::Real(1.0)]).is_undef()
        );
    }

    // ── Discoverability surface ───────────────────────────────────────────────

    #[test]
    fn support_kinds_lists_all_three_in_canonical_order() {
        use super::SUPPORT_KINDS;
        assert_eq!(
            SUPPORT_KINDS,
            &["fixed_support", "displacement_support", "roller_support"]
        );
    }

    #[test]
    fn is_support_value_recognises_fixed_support() {
        use super::is_support_value;
        let v = eval_builtin("FixedSupport", &[point_selector_stub()]);
        assert!(is_support_value(&v), "is_support_value should recognize FixedSupport result");
    }

    #[test]
    fn is_support_value_recognises_displacement_support() {
        use super::is_support_value;
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        let v = eval_builtin("DisplacementSupport", &[point_selector_stub(), disp]);
        assert!(
            is_support_value(&v),
            "is_support_value should recognize DisplacementSupport result"
        );
    }

    #[test]
    fn is_support_value_recognises_roller_support() {
        use super::is_support_value;
        let normal =
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let v = eval_builtin("RollerSupport", &[point_selector_stub(), normal]);
        assert!(
            is_support_value(&v),
            "is_support_value should recognize RollerSupport result"
        );
    }

    #[test]
    fn is_support_value_negative_cases() {
        use super::is_support_value;

        // Cross-module isolation: build a load value via eval_builtin to confirm
        // LOAD_KINDS and SUPPORT_KINDS don't cross-contaminate is_support_value.
        let stub = point_selector_stub();
        let force_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        let load_value = eval_builtin("point_load", &[stub, force_vec]);

        // Map without a `kind` key.
        let map_no_kind = Value::Map(BTreeMap::new());

        // Map with kind="not_a_support".
        let map_wrong_kind = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("not_a_support".to_string()),
            );
            m
        });

        // Map with kind=Value::Int(0) (non-string kind).
        let map_int_kind = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(Value::String("kind".to_string()), Value::Int(0));
            m
        });

        let negative_cases: Vec<(&str, Value)> = vec![
            ("Real(1.0)", Value::Real(1.0)),
            ("Int(0)", Value::Int(0)),
            ("String(\"fixed_support\")", Value::String("fixed_support".to_string())),
            ("Map without kind key", map_no_kind),
            ("Map with kind=\"not_a_support\"", map_wrong_kind),
            ("Map with kind=Int(0)", map_int_kind),
            ("load value from eval_builtin(point_load)", load_value),
        ];

        for (label, v) in negative_cases {
            assert!(
                !is_support_value(&v),
                "is_support_value should return false for: {}",
                label
            );
        }
    }

    // ── SUPPORT_KINDS partition test ──────────────────────────────────────────

    /// Guard that every kind listed in `SUPPORT_KINDS` is actually dispatched
    /// by `eval_supports`.  If a kind is renamed or removed in `eval_supports`
    /// but not updated in `SUPPORT_KINDS` (or vice versa), this test will
    /// catch it.
    #[test]
    fn support_kinds_all_dispatched_by_eval_supports() {
        use super::{eval_supports, is_support_value, SUPPORT_KINDS};

        let stub_selector = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("stub".to_string()),
            );
            m
        });
        let length_vec = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        let dimensionless_z = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);

        for kind in SUPPORT_KINDS {
            // NOTE: SUPPORT_KINDS contains the snake_case kind-tag strings used
            // in the Map's `kind` field.  The eval_supports dispatch arms use the
            // PascalCase constructor names.  We map explicitly here so the test
            // still guards both the SUPPORT_KINDS list and the dispatch arms.
            let result = match *kind {
                "fixed_support" => eval_supports("FixedSupport", &[stub_selector.clone()]),
                "displacement_support" => {
                    eval_supports("DisplacementSupport", &[stub_selector.clone(), length_vec.clone()])
                }
                "roller_support" => {
                    eval_supports("RollerSupport", &[stub_selector.clone(), dimensionless_z.clone()])
                }
                other => panic!(
                    "SUPPORT_KINDS contains '{}' but no fixture is defined for it — \
                     add a fixture arm to this test and an arm to eval_supports",
                    other
                ),
            };

            assert!(
                result.is_some(),
                "eval_supports('{}', ...) returned None — SUPPORT_KINDS is out of sync \
                 with eval_supports dispatch arms",
                kind
            );

            let value = result.unwrap();
            assert!(
                is_support_value(&value),
                "is_support_value should recognize a Map produced by eval_supports('{}', ...)",
                kind
            );
        }
    }
}
