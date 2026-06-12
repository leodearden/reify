//! Shared test fixtures and helpers for flexure PRB constructor unit tests.
//!
//! Exported from `flexures::test_util` (declared under `#[cfg(test)]` in
//! `mod.rs`) and imported by the sibling `notch` and `hinge` test modules via
//! `use super::super::test_util::*;` — avoids duplicating `material`,
//! `steel`, `axis_y`, `origin`, `map_get`, `range_lower_upper`,
//! `assert_angle_close`, and `spring_rate_si` across multiple test suites.

use reify_core::DimensionVector;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Build a `Value::StructureInstance` material fixture from `(name, value)`
/// field pairs (mirrors the SIR-α constructor pattern in reify-ir value.rs).
pub fn material(name: &str, pairs: &[(&str, Value)]) -> Value {
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

/// `Steel_AISI_1045`-like fixture: E = 205 GPa, yield = 310 MPa (PRESSURE),
/// ν = 0.29 (bare `Value::Real`, the runtime representation for
/// `ElasticMaterial::poisson_ratio : Real ∈ [0, 0.5)`).
///
/// Notch and beam constructors ignore `poisson_ratio`; having it present does
/// not affect their outputs.
pub fn steel() -> Value {
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
            // poisson_ratio stored as bare Real (ElasticMaterial::poisson_ratio : Real).
            ("poisson_ratio", Value::Real(0.29)),
        ],
    )
}

/// Like [`steel`] but carrying only `youngs_modulus` + `poisson_ratio` (no
/// `yield_stress`), to exercise the no-yield fallback branch.
pub fn steel_no_yield() -> Value {
    material(
        "Steel_NoYield",
        &[
            (
                "youngs_modulus",
                Value::Scalar {
                    si_value: 205e9,
                    dimension: DimensionVector::PRESSURE,
                },
            ),
            ("poisson_ratio", Value::Real(0.29)),
        ],
    )
}

/// Material fixture with custom E for functional scaling-ratio tests.
///
/// Carries `yield_stress = 310 MPa` and `poisson_ratio = 0.29` so the same
/// fixture drives bending-hinge scaling tests (which only vary E) and LET
/// scaling tests (G = E/(2(1+ν)) with fixed ν).
pub fn steel_with_e(e: f64) -> Value {
    material(
        "TestMaterial",
        &[
            (
                "youngs_modulus",
                Value::Scalar {
                    si_value: e,
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
            ("poisson_ratio", Value::Real(0.29)),
        ],
    )
}

pub fn axis_y() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
}

pub fn origin() -> Value {
    Value::Point(vec![
        Value::length(0.0),
        Value::length(0.0),
        Value::length(0.0),
    ])
}

pub fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Map(m) => m.get(&Value::String(key.to_string())),
        _ => None,
    }
}

/// Destructure a bounded `Value::Range` into its inner `(lower, upper)` values.
pub fn range_lower_upper(v: &Value) -> (&Value, &Value) {
    match v {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => (lo.as_ref(), up.as_ref()),
        other => panic!("expected a both-bounded Range, got {other:?}"),
    }
}

/// Assert `v` is a symmetric ANGLE `Range` `[−h, +h]` (both-inclusive) and
/// return the half-width in radians (task 4576: `prb_validity_range` is now
/// `Range<Angle>` instead of `Value::Real`).
pub fn angle_range_half_si(v: &Value, label: &str) -> f64 {
    match v {
        Value::Range { lower, upper, lower_inclusive, upper_inclusive } => {
            assert!(*lower_inclusive, "{label}: lower_inclusive should be true");
            assert!(*upper_inclusive, "{label}: upper_inclusive should be true");
            let lo = lower.as_deref().unwrap_or_else(|| panic!("{label}: lower bound missing"));
            let hi = upper.as_deref().unwrap_or_else(|| panic!("{label}: upper bound missing"));
            let lo_si = match lo {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(*dimension, DimensionVector::ANGLE, "{label}: lower bound dimension");
                    *si_value
                }
                other => panic!("{label}: lower bound expected ANGLE Scalar, got {other:?}"),
            };
            let hi_si = match hi {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(*dimension, DimensionVector::ANGLE, "{label}: upper bound dimension");
                    *si_value
                }
                other => panic!("{label}: upper bound expected ANGLE Scalar, got {other:?}"),
            };
            // Symmetry: lo + hi == 0.0 exactly (IEEE 754 exact negation).
            assert!(
                lo_si + hi_si == 0.0,
                "{label}: range not symmetric ([{lo_si}, {hi_si}])"
            );
            hi_si
        }
        other => panic!("{label}: expected Value::Range{{ANGLE}}, got {other:?}"),
    }
}

/// Assert `actual` is an ANGLE-dimensioned Scalar whose si_value matches
/// `expected_rad` to a relative tolerance of 1e-9 (closed-form reproduction).
pub fn assert_angle_close(actual: &Value, expected_rad: f64, label: &str) {
    match actual {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "{label}: bound carries ANGLE dimension"
            );
            let rel = (si_value - expected_rad).abs() / expected_rad.abs();
            assert!(rel < 1e-9, "{label}: {si_value} vs {expected_rad} (rel {rel})");
        }
        other => panic!("{label}: expected angle Scalar, got {other:?}"),
    }
}

/// Extract the `spring_rate` si_value from a flexure revolute Map.
pub fn spring_rate_si(v: &Value) -> f64 {
    match map_get(v, "spring_rate") {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected spring_rate Scalar, got {other:?}"),
    }
}
