//! Beam-flexure PRB constructors (Howell §5): cantilever beam (revolute) and
//! fixed-fixed beam (transverse prismatic).
//!
//! Scaffold stub — the constructor arms land in the γ implementation steps.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::Value;

/// Howell pseudo-rigid-body coefficient for a cantilever beam (Howell §5.1).
const CANTILEVER_GAMMA: f64 = 2.65;

/// PRB validity limit on flexure rotation: ±5°, expressed in radians. Beyond
/// this the pseudo-rigid-body small-deflection model loses fidelity (Howell §5).
const PRB_ANGLE_LIMIT_RAD: f64 = 5.0 * PI / 180.0;

/// Evaluate a beam-flexure constructor by name.
///
/// Returns `Some(Value)` for a recognised flexure name (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so `eval_builtin` falls through to the next module.
pub(crate) fn eval_beam(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_cantilever_beam" => Some(prb_cantilever_beam(args)),
        _ => None,
    }
}

/// `prb_cantilever_beam(length, width, thickness, material, pivot, axis[, neutral])`
/// — a Howell pseudo-rigid-body cantilever flexure presented as a revolute joint.
///
/// Returns a joint `Value::Map` (`kind == "revolute"`) whose rotational
/// stiffness is the closed-form `k_θ = γ·E·I / L` (Howell §5.1, γ = 2.65), with
/// `I = width·thickness³/12` the rectangular-section second moment of area.
///
/// The symmetric `prb_validity` rotation range is `±min(θ_yield, 5°)`, where
/// `θ_yield = yield·L/(E·t/2)` is the surface-yield rotation and 5° is the PRB
/// small-deflection limit. When the material carries no `yield_stress`, only
/// the 5° PRB limit applies.
///
/// Returns `Value::Undef` on arity ≠ {6, 7} or when any geometry / material /
/// axis argument fails extraction. (Comprehensive geometry guards land in a
/// later step.)
fn prb_cantilever_beam(args: &[Value]) -> Value {
    // Uniform 6/7-arg signature (the optional 7th arg is the neutral angle,
    // wired in a later step): (length, width, thickness, material, pivot, axis).
    if args.len() != 6 && args.len() != 7 {
        return Value::Undef;
    }
    let (Some(length), Some(width), Some(thickness)) = (
        length_si(&args[0]),
        length_si(&args[1]),
        length_si(&args[2]),
    ) else {
        return Value::Undef;
    };
    let material = &args[3];
    let pivot = &args[4];
    let axis = &args[5];

    let Some(e) = material_field_si(material, "youngs_modulus") else {
        return Value::Undef;
    };
    // Axis must be a finite, non-zero, dimensionless 3-vector; stored verbatim.
    if crate::helpers::validate_dimensionless_unit_axis_vec3(axis).is_none() {
        return Value::Undef;
    }

    // Rectangular-section second moment of area and the Howell PRB rotational
    // stiffness k_θ = γ·E·I / L (Howell §5.1).
    let i = width * thickness.powi(3) / 12.0;
    let k_theta = CANTILEVER_GAMMA * e * i / length;

    // Symmetric prb_validity range = ±min(θ_yield, 5°). θ_yield is the
    // surface-yield rotation (Howell §5.1: σ(θ) = E·(t/2)·θ/L ⇒
    // θ_yield = yield·L/(E·t/2)); the 5° PRB limit bounds small-deflection
    // fidelity. A material without a yield_stress contributes only the 5° cap.
    let theta_lim = match material_field_si(material, "yield_stress") {
        Some(yield_si) => {
            let theta_yield = yield_si * length / (e * thickness / 2.0);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };
    let range = Value::range(
        Some(Value::angle(-theta_lim)),
        Some(Value::angle(theta_lim)),
        true,
        true,
    );

    make_flexure_joint(
        "revolute",
        axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(0.0),
        pivot.clone(),
    )
}

/// Extract a length in metres: a finite LENGTH-dimensioned `Value::Scalar`, or a
/// bare finite `Value::Real` / `Value::Int` interpreted as metres. Mirrors
/// `joints::length_input`. Returns `None` for any other variant.
fn length_si(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == DimensionVector::LENGTH && si_value.is_finite() => Some(*si_value),
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Extract a finite Scalar `si_value` for `key` from a material
/// `Value::StructureInstance`'s fields.
///
/// The field may be stored either as a bare `Value::Scalar` or wrapped in
/// `Value::Option(Some(Scalar))` (mirroring the `ElasticMaterial` contract,
/// where `yield_stress` is `Option<Pressure>`). Returns `None` if `material` is
/// not a `StructureInstance`, the field is absent, the option is `None`, or the
/// stored value is not a finite Scalar — so an absent or `None` field reads the
/// same as "not provided".
fn material_field_si(material: &Value, key: &str) -> Option<f64> {
    let fields = match material {
        Value::StructureInstance(data) => &data.fields,
        _ => return None,
    };
    scalar_si(fields.get(&key.to_string())?)
}

/// Unwrap a finite Scalar `si_value` from a `Value::Scalar` or a
/// `Value::Option(Some(Scalar))`.
fn scalar_si(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } if si_value.is_finite() => Some(*si_value),
        Value::Option(Some(inner)) => scalar_si(inner),
        _ => None,
    }
}

/// Assemble a flexure joint `Value::Map`: the standard `{kind, axis, range}`
/// joint layout (mirroring `joints::make_joint`) extended with the
/// flexure-specific keys `spring_rate`, `damping`, `neutral`, and `pivot`.
///
/// `damping` is always `Value::Option(None)` in γ scope (PRD §8.7). The
/// mechanism / sweep / snapshot engines dispatch on the `kind` string and
/// ignore the extra keys (PRD §8.2), so a flexure plugs into them exactly like
/// a plain revolute / prismatic joint.
fn make_flexure_joint(
    kind: &str,
    axis: Value,
    range: Value,
    spring_rate: Value,
    neutral: Value,
    pivot: Value,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String(kind.to_string()),
    );
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(Value::String("range".to_string()), range);
    m.insert(Value::String("spring_rate".to_string()), spring_rate);
    m.insert(Value::String("damping".to_string()), Value::Option(None));
    m.insert(Value::String("neutral".to_string()), neutral);
    m.insert(Value::String("pivot".to_string()), pivot);
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a `Value::StructureInstance` material fixture from `(name, value)`
    /// field pairs (mirrors the SIR-α constructor pattern in reify-ir value.rs).
    fn material(name: &str, pairs: &[(&str, Value)]) -> Value {
        let fields: PersistentMap<String, Value> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// `Steel_AISI_1045`-like fixture: E = 205 GPa, yield = 310 MPa (PRESSURE).
    fn steel() -> Value {
        material(
            "Steel_AISI_1045",
            &[
                (
                    "youngs_modulus",
                    Value::Scalar {
                        si_value: 205e9,
                        dimension: DimensionVector::PRESSURE,
                    },
                ),
                (
                    "yield_stress",
                    Value::Option(Some(Box::new(Value::Scalar {
                        si_value: 310e6,
                        dimension: DimensionVector::PRESSURE,
                    }))),
                ),
            ],
        )
    }

    fn axis_y() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    fn origin() -> Value {
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])
    }

    fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
        match v {
            Value::Map(m) => m.get(&Value::String(key.to_string())),
            _ => None,
        }
    }

    #[test]
    fn prb_cantilever_beam_returns_revolute_with_spring_rate() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let result = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "cantilever flexure presents as a revolute joint"
        );
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis is preserved verbatim"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is None in γ scope"
        );
        let i = width * thickness.powi(3) / 12.0;
        let k_expected = 2.65 * e * i / length;
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(rel < 1e-9, "spring_rate {si_value} vs {k_expected} (rel {rel})");
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// Destructure a bounded `Value::Range` into its inner `(lower, upper)`
    /// values, panicking if either bound is absent.
    fn range_lower_upper(v: &Value) -> (&Value, &Value) {
        match v {
            Value::Range {
                lower: Some(lo),
                upper: Some(up),
                ..
            } => (lo.as_ref(), up.as_ref()),
            other => panic!("expected a both-bounded Range, got {other:?}"),
        }
    }

    /// Assert `actual` is an ANGLE-dimensioned Scalar whose si_value matches
    /// `expected_rad` to a relative tolerance of 1e-9 (closed-form reproduction).
    fn assert_angle_close(actual: &Value, expected_rad: f64, label: &str) {
        match actual {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ANGLE,
                    "{label}: bound carries ANGLE dimension"
                );
                let rel = (si_value - expected_rad).abs() / expected_rad.abs();
                assert!(
                    rel < 1e-9,
                    "{label}: {si_value} vs {expected_rad} (rel {rel})"
                );
            }
            other => panic!("{label}: expected angle Scalar, got {other:?}"),
        }
    }

    #[test]
    fn prb_cantilever_beam_range_is_yield_capped_when_yield_dominates() {
        // Short/thick fixture (L/t = 10) so θ_yield < 5°, exercising the
        // yield-capped branch of the prb_validity range.
        let length = 0.005_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let yield_stress = 310e6_f64;
        let theta_yield = yield_stress * length / (e * thickness / 2.0);
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;
        assert!(
            theta_yield < prb_limit,
            "fixture must exercise the yield-capped branch: θ_yield={theta_yield} ≥ 5°"
        );
        let result = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        let range = map_get(&result, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        assert_angle_close(lo, -theta_yield, "yield-capped lower bound");
        assert_angle_close(up, theta_yield, "yield-capped upper bound");
    }

    #[test]
    fn prb_cantilever_beam_range_is_prb_limited_when_5deg_dominates() {
        // step-1 fixture (L=20mm, t=0.5mm): θ_yield ≈ 6.93° > 5°, so the ±5°
        // PRB validity cap dominates.
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;
        let result = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(0.02),
                Value::length(0.005),
                Value::length(0.0005),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        let range = map_get(&result, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        assert_angle_close(lo, -prb_limit, "prb-limited lower bound");
        assert_angle_close(up, prb_limit, "prb-limited upper bound");
    }

    /// Invoke `prb_cantilever_beam` on the step-1 geometry, optionally appending
    /// a 7th `neutral` arg.
    fn cantilever_with_neutral(neutral: Option<Value>) -> Value {
        let mut args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ];
        if let Some(n) = neutral {
            args.push(n);
        }
        crate::eval_builtin("prb_cantilever_beam", &args)
    }

    #[test]
    fn prb_cantilever_beam_neutral_angle_handling() {
        let two_deg = 2.0_f64 * std::f64::consts::PI / 180.0;

        // (a) 6-arg call → neutral defaults to angle(0).
        let six = cantilever_with_neutral(None);
        assert_eq!(
            map_get(&six, "neutral"),
            Some(&Value::angle(0.0)),
            "6-arg call defaults neutral to angle(0)"
        );

        // (b) 7-arg call with a bare angle(2°) → neutral == angle(2°).
        let seven = cantilever_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(
            map_get(&seven, "neutral").expect("neutral key present"),
            two_deg,
            "7-arg bare-angle neutral",
        );

        // (c) 7-arg call with Option(Some(angle(2°))) → unwraps to angle(2°).
        let seven_opt =
            cantilever_with_neutral(Some(Value::Option(Some(Box::new(Value::angle(two_deg))))));
        assert_angle_close(
            map_get(&seven_opt, "neutral").expect("neutral key present"),
            two_deg,
            "7-arg optional-angle neutral",
        );
    }
}
