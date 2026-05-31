//! Shared helpers for flexure PRB constructors.
//!
//! Hoisted from beam.rs at δ (task/3854) — the second consumer (notch.rs)
//! triggers the project's hoist-on-≥2-duplicates norm (cf. helpers.rs).

use std::collections::BTreeMap;
use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::Value;

/// PRB validity limit on flexure rotation: ±5°, expressed in radians. Beyond
/// this the pseudo-rigid-body small-deflection model loses fidelity (Howell §5).
pub(super) const PRB_ANGLE_LIMIT_RAD: f64 = 5.0 * PI / 180.0;

/// Return a both-inclusive symmetric angle range `[−h, +h]` centred on zero.
pub(super) fn symmetric_angle_range(half_width_rad: f64) -> Value {
    Value::range(
        Some(Value::angle(-half_width_rad)),
        Some(Value::angle(half_width_rad)),
        true,
        true,
    )
}

/// Assemble a flexure joint `Value::Map`: the standard `{kind, axis, range}`
/// joint layout (mirroring `joints::make_joint`) extended with the
/// flexure-specific keys `spring_rate`, `damping`, `neutral`, and `pivot`.
///
/// `damping` is always `Value::Option(None)` in γ scope (PRD §8.7). The
/// mechanism / sweep / snapshot engines dispatch on the `kind` string and
/// ignore the extra keys (PRD §8.2), so a flexure plugs into them exactly like
/// a plain revolute / prismatic joint.
pub(super) fn make_flexure_joint(
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

/// Extract a length in metres: a finite LENGTH-dimensioned `Value::Scalar`, or a
/// bare finite `Value::Real` / `Value::Int` interpreted as metres. Mirrors
/// `joints::length_input`. Returns `None` for any other variant.
pub(super) fn length_si(v: &Value) -> Option<f64> {
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

/// Unwrap a finite Scalar `si_value` from a `Value::Scalar` or a
/// `Value::Option(Some(Scalar))`.
pub(super) fn scalar_si(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } if si_value.is_finite() => Some(*si_value),
        Value::Option(Some(inner)) => scalar_si(inner),
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
pub(super) fn material_field_si(material: &Value, key: &str) -> Option<f64> {
    let fields = match material {
        Value::StructureInstance(data) => &data.fields,
        _ => return None,
    };
    scalar_si(fields.get(&key.to_string())?)
}

/// Extract a neutral angle in radians from a trailing constructor argument.
///
/// Accepts an ANGLE-dimensioned `Value::Scalar` (e.g. `Value::angle`), a bare
/// `Value::Real` / `Value::Int` interpreted as radians (via
/// [`crate::helpers::trig_input`]), or a `Value::Option` wrapping any of those.
/// `Option(None)` and any value that fails extraction default to `0.0` — the
/// neutral angle is an optional offset, so an absent/unreadable value is the
/// natural zero rather than a hard error.
pub(super) fn neutral_angle_si(v: &Value) -> f64 {
    match v {
        Value::Option(Some(inner)) => neutral_angle_si(inner),
        Value::Option(None) => 0.0,
        other => crate::helpers::trig_input(other).unwrap_or(0.0),
    }
}
