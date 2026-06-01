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

/// Howell pseudo-rigid-body coefficient for a cantilever beam (Howell §5.1).
/// Used by `beam::prb_cantilever_beam` (revolute joint) and
/// `compound::prb_cartwheel_flexure` (cartwheel blade — same cantilever
/// boundary condition, N blades contributing k_pivot = N·k_blade).
pub(super) const CANTILEVER_GAMMA: f64 = 2.65;

/// Fixed-guided (fixed-fixed) stiffness coefficient γ_ff = 12 (Howell §5 /
/// PRD §6.1). Used by `beam::prb_fixed_fixed_beam` (transverse prismatic joint)
/// and `compound::{prb_parallelogram_flexure, prb_double_parallelogram_flexure}`
/// (parallelogram blade — same fixed-guided boundary condition).
pub(super) const FIXED_GUIDED_GAMMA: f64 = 12.0;

/// Fallback transverse-displacement validity limit as a fraction of beam length,
/// used when the material carries no `yield_stress`. The PRB transverse
/// small-deflection model degrades past ~0.1·L.
pub(super) const SMALL_DEFLECTION_FRACTION: f64 = 0.1;

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

/// Extract a numeric `f64` from a material `Value::StructureInstance` field,
/// accepting `Value::Scalar{si_value}`, `Value::Option(Some(inner))`,
/// `Value::Real`, or `Value::Int` — the `read_scalar_si` pattern
/// (reify-eval/src/modal_ops.rs:839).
///
/// Unlike [`material_field_si`] (which only matches `Scalar` / `Option<Scalar>`),
/// this also accepts bare `Value::Real` and `Value::Int`, making it suitable
/// for fields such as `poisson_ratio` that land as a bare `Value::Real` at
/// runtime. `Option(Some(_))` is unwrapped recursively for parity with
/// [`material_field_si`]'s option-handling — so if `poisson_ratio` is ever
/// stored as `Option<Real>`, it still reads correctly rather than returning
/// `None`.
pub(super) fn material_numeric_field(material: &Value, key: &str) -> Option<f64> {
    let fields = match material {
        Value::StructureInstance(data) => &data.fields,
        _ => return None,
    };
    numeric_from_value(fields.get(&key.to_string())?)
}

/// Unwrap a numeric `f64` from any scalar-like `Value` variant, recursing into
/// `Option(Some(_))` wrappers.
fn numeric_from_value(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, .. } if si_value.is_finite() => Some(*si_value),
        Value::Option(Some(inner)) => numeric_from_value(inner),
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Compute the cantilever surface-yield rotation limit θ_lim, capped at the
/// PRB small-deflection limit.
///
/// Cantilever bending stress σ = E·(t/2)·θ/L (Howell §5.1)
/// ⇒ θ_yield = yield·L/(E·t/2), capped at [`PRB_ANGLE_LIMIT_RAD`] (5°).
/// No `yield_stress` ⇒ only the PRB cap applies.
///
/// Shared by `beam::prb_cantilever_beam` (revolute joint) and
/// `compound::prb_cartwheel_flexure` (each radial blade is a cantilever) —
/// a single definition prevents the two modules drifting on the surface-yield model.
pub(super) fn cantilever_theta_lim(length: f64, thickness: f64, e: f64, yield_si: Option<f64>) -> f64 {
    match yield_si {
        Some(y) => (y * length / (e * thickness / 2.0)).min(PRB_ANGLE_LIMIT_RAD),
        None => PRB_ANGLE_LIMIT_RAD,
    }
}

/// Compute the fixed-guided surface-yield deflection half-width δ_max.
///
/// Fixed-guided bending stress σ = 3·E·t·δ / L²
/// ⇒ δ_yield = yield·L²/(3·E·t).
/// No `yield_stress` ⇒ small-deflection fallback δ = [`SMALL_DEFLECTION_FRACTION`]·L.
///
/// Shared by `beam::prb_fixed_fixed_beam` and `compound::{prb_parallelogram_flexure,
/// prb_double_parallelogram_flexure}` — a single definition prevents the two
/// modules drifting on the bending-stress model.
pub(super) fn fixed_guided_delta_max(
    length: f64,
    thickness: f64,
    e: f64,
    yield_si: Option<f64>,
) -> f64 {
    match yield_si {
        Some(y) => y * length.powi(2) / (3.0 * e * thickness),
        None => SMALL_DEFLECTION_FRACTION * length,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    /// Read a field `Value` by name from a `FlexureCompliance` StructureInstance.
    fn field<'a>(rec: &'a Value, key: &str) -> &'a Value {
        match rec {
            Value::StructureInstance(data) => data
                .fields
                .get(&key.to_string())
                .unwrap_or_else(|| panic!("FlexureCompliance missing field `{key}`")),
            other => panic!("expected FlexureCompliance StructureInstance, got {other:?}"),
        }
    }

    /// Assert `v` is a PRESSURE-dimensioned Scalar and return its si_value.
    fn pressure_si(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::PRESSURE,
                    "{label}: carries PRESSURE dimension"
                );
                *si_value
            }
            other => panic!("{label}: expected PRESSURE Scalar, got {other:?}"),
        }
    }

    /// Assert `v` is a bare `Value::Real` and return it.
    fn real_of(v: &Value, label: &str) -> f64 {
        match v {
            Value::Real(r) => *r,
            other => panic!("{label}: expected Real, got {other:?}"),
        }
    }

    #[test]
    fn make_compliance_record_is_flexure_compliance_with_seven_fields() {
        let rec = make_compliance_record(1.42, 100e6, 0.0, Some(310e6), None, 0.0872664626);
        match &rec {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "FlexureCompliance", "type_name");
            }
            other => panic!("expected StructureInstance, got {other:?}"),
        }
        // All 7 FlexureCompliance fields present.
        for key in [
            "effective_stiffness",
            "max_stress",
            "max_stress_at_neutral",
            "yield_margin",
            "parasitic_error",
            "prb_validity_range",
            "at_yield",
        ] {
            let _ = field(&rec, key);
        }
    }

    #[test]
    fn make_compliance_record_safe_input_positive_margin_not_yielding() {
        // max_stress (100 MPa) < yield (310 MPa) ⇒ at_yield false, positive margin.
        let yield_si = 310e6_f64;
        let max_stress = 100e6_f64;
        let rec = make_compliance_record(1.42, max_stress, 0.0, Some(yield_si), None, 0.0872664626);

        // effective_stiffness stored as a bare Real (family-agnostic: revolute
        // rotational vs prismatic translational stiffness share this slot).
        assert_eq!(
            real_of(field(&rec, "effective_stiffness"), "effective_stiffness"),
            1.42
        );

        // Stresses are PRESSURE-dimensioned Scalars.
        assert_eq!(pressure_si(field(&rec, "max_stress"), "max_stress"), max_stress);
        assert_eq!(
            pressure_si(field(&rec, "max_stress_at_neutral"), "max_stress_at_neutral"),
            0.0
        );

        // yield_margin == (yield - max_stress) / yield, and positive for safe input.
        let expected_margin = (yield_si - max_stress) / yield_si;
        let m = real_of(field(&rec, "yield_margin"), "yield_margin");
        assert!(
            (m - expected_margin).abs() < 1e-12,
            "margin {m} vs expected {expected_margin}"
        );
        assert!(m > 0.0, "safe input ⇒ positive margin, got {m}");

        // at_yield == false.
        assert_eq!(field(&rec, "at_yield"), &Value::Bool(false), "not at yield");

        // parasitic_error None ⇒ Option(None).
        assert_eq!(
            field(&rec, "parasitic_error"),
            &Value::Option(None),
            "absent parasitic ⇒ Option(None)"
        );

        // prb_validity_range stored as a Real (the SI half-angle/half-displacement).
        assert!(
            (real_of(field(&rec, "prb_validity_range"), "prb_validity_range") - 0.0872664626).abs()
                < 1e-9
        );
    }

    #[test]
    fn make_compliance_record_yielding_input_negative_margin_at_yield() {
        // max_stress (447 MPa) > yield (310 MPa) ⇒ at_yield true, negative margin.
        let yield_si = 310e6_f64;
        let max_stress = 447e6_f64;
        let rec =
            make_compliance_record(0.01, max_stress, 50e6, Some(yield_si), Some(1e-6), 0.17453293);

        assert_eq!(field(&rec, "at_yield"), &Value::Bool(true), "at yield");

        let m = real_of(field(&rec, "yield_margin"), "yield_margin");
        let expected = (yield_si - max_stress) / yield_si;
        assert!((m - expected).abs() < 1e-9, "margin {m} vs {expected}");
        assert!(m < 0.0, "yielding input ⇒ negative margin, got {m}");

        // parasitic Some(1µm) ⇒ Option(Some(Length)).
        match field(&rec, "parasitic_error") {
            Value::Option(Some(inner)) => match inner.as_ref() {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(*dimension, DimensionVector::LENGTH, "parasitic is a LENGTH");
                    assert!((si_value - 1e-6).abs() < 1e-15, "parasitic si {si_value}");
                }
                other => panic!("parasitic inner: expected Length Scalar, got {other:?}"),
            },
            other => panic!("expected Option(Some(Length)), got {other:?}"),
        }
    }

    #[test]
    fn make_compliance_record_no_yield_input_uses_safe_sentinel() {
        // None yield ⇒ at_yield false, margin sentinel = 1.0 (maximally safe:
        // no yield datum places no stress limit, clamped to the margin upper
        // bound). Pairs naturally with at_yield=false (0.0 would falsely read as
        // "exactly at the yield boundary").
        let rec = make_compliance_record(1.0, 100e6, 0.0, None, None, 0.0872664626);
        assert_eq!(
            field(&rec, "at_yield"),
            &Value::Bool(false),
            "no-yield material ⇒ not at yield"
        );
        let m = real_of(field(&rec, "yield_margin"), "yield_margin");
        assert_eq!(m, 1.0, "no-yield margin sentinel is 1.0 (maximally safe)");
    }
}
