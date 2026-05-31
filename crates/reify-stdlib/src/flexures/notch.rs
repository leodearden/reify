//! Notch-flexure PRB constructors (Paros-Weisbord 1965, PRD §5.2):
//! circular, elliptical, and right-circular notch hinges — all → revolute.
//!
//! All three constructors share the positional argument layout
//! `(notch_radius, web_thickness, width, material, pivot, axis[, neutral])`
//! and the [`parse_notch_inputs`] validation path. They differ only in the
//! dimensionless shape factors `k_factor` and `sigma_factor` passed to the
//! shared `notch_revolute` core (PRD §5.2 design decision).

use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    make_flexure_joint, length_si, material_field_si, neutral_angle_si, symmetric_angle_range,
    PRB_ANGLE_LIMIT_RAD,
};

/// Shape factors for the standard circular notch flexure hinge
/// (Paros & Weisbord 1965, §5.2): κ = 1, k_σ = 1. All other notch variants
/// are normalised relative to this baseline.
const CIRCULAR_K: f64 = 1.0;
const CIRCULAR_SIGMA: f64 = 1.0;

/// Shape factors for the elliptical notch flexure hinge (Smith et al. 1997,
/// "Design of Elliptical Notch Flexure Hinges", Precision Engineering).
/// For a 2:1 profile aspect ratio the elliptical hinge is softer (more
/// compliant) than the circular case; κ → 1 as the semi-axes converge to
/// equal radii (circular limit).
const ELLIPTICAL_K: f64 = 0.85;
const ELLIPTICAL_SIGMA: f64 = 0.85;

/// Shape factors for the right-circular (toroidal / axisymmetric) notch
/// flexure hinge (Paros & Weisbord 1965, toroidal variant). The full
/// axisymmetric removal makes this the most compliant of the three profiles
/// for the same (r, t, b) geometry; κ → 1 in the planar limit.
const RIGHT_CIRCULAR_K: f64 = 0.74;
const RIGHT_CIRCULAR_SIGMA: f64 = 0.74;

/// Evaluate a notch-flexure constructor by name.
///
/// Returns `Some(Value)` for a recognised notch-flexure name (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so the caller can fall through to the next module.
pub(crate) fn eval_notch(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_notch_circular" => Some(prb_notch_circular(args)),
        "prb_notch_elliptical" => Some(prb_notch_elliptical(args)),
        "prb_notch_right_circular" => Some(prb_notch_right_circular(args)),
        _ => None,
    }
}

/// Shared, validated inputs for all three notch-flexure constructors.
struct NotchInputs<'a> {
    /// Notch radius r (metres).
    r: f64,
    /// Web thickness t (metres) — the minimum cross-section at the hinge.
    t: f64,
    /// Out-of-plane width b (metres).
    b: f64,
    /// Young's modulus E (Pa).
    e: f64,
    /// Material yield stress (Pa), if the material carries one.
    yield_si: Option<f64>,
    /// The raw axis argument, stored verbatim on the joint Map.
    axis: &'a Value,
    /// The raw pivot argument, stored verbatim on the joint Map.
    pivot: &'a Value,
    /// The optional trailing `neutral` argument (present only in the 7-arg form).
    neutral_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout of all three
/// notch-flexure constructors: `(notch_radius, web_thickness, width,
/// material, pivot, axis[, neutral])`.
///
/// Returns `None` (⇒ the caller returns `Value::Undef`) on: arity ∉ {6, 7};
/// non-positive or non-finite geometry (r, t, or b ≤ 0); degenerate geometry
/// (t ≥ 2·r — web at least as thick as the notch diameter, the
/// E_FlexureGeometryInvalid regime whose diagnostic λ owns); a material that
/// is not a `Value::StructureInstance` with a finite `youngs_modulus` > 0; or
/// an axis that is not a finite, non-zero, dimensionless 3-vector.
///
/// δ emits NO diagnostics and returns Value::Undef on invalid input
/// (W_Flexure* emission is λ's responsibility).
fn parse_notch_inputs(args: &[Value]) -> Option<NotchInputs<'_>> {
    if args.len() != 6 && args.len() != 7 {
        return None;
    }
    let r = length_si(&args[0])?;
    let t = length_si(&args[1])?;
    let b = length_si(&args[2])?;
    if r <= 0.0 || t <= 0.0 || b <= 0.0 {
        return None;
    }
    // Degenerate: web ≥ notch diameter — no hinge ligament exists.
    if t >= 2.0 * r {
        return None;
    }
    let material = &args[3];
    let e = material_field_si(material, "youngs_modulus")?;
    if e <= 0.0 {
        return None;
    }
    let axis = &args[5];
    crate::helpers::validate_dimensionless_unit_axis_vec3(axis)?;
    Some(NotchInputs {
        r,
        t,
        b,
        e,
        yield_si: material_field_si(material, "yield_stress"),
        axis,
        pivot: &args[4],
        neutral_arg: if args.len() == 7 { Some(&args[6]) } else { None },
    })
}

/// Parametrized core for all notch revolute ctors.
///
/// Computes the Paros-Weisbord closed form (PRD §5.2):
///   k_θ = k_factor · 2·E·b·t^2.5 / (9π·r^0.5)
///
/// Surface-yield rotation (PRD §5.2):
///   σ(θ) = sigma_factor · 4·E·t·θ / (3π·(2r+t))  ⇒
///   θ_yield = yield · 3π·(2r+t) / (sigma_factor · 4·E·t)
///
/// Validity range = ±min(θ_yield, 5°); no yield_stress ⇒ ±5° fallback.
fn notch_revolute(inputs: &NotchInputs<'_>, k_factor: f64, sigma_factor: f64) -> Value {
    // Paros-Weisbord rotational stiffness (PRD §5.2).
    let k_theta = k_factor * 2.0 * inputs.e * inputs.b * inputs.t.powf(2.5)
        / (9.0 * PI * inputs.r.sqrt());

    // Symmetric prb_validity range = ±min(θ_yield, 5°).
    let theta_lim = match inputs.yield_si {
        Some(yield_si) => {
            let theta_yield = yield_si * 3.0 * PI * (2.0 * inputs.r + inputs.t)
                / (sigma_factor * 4.0 * inputs.e * inputs.t);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };
    let range = symmetric_angle_range(theta_lim);

    // Optional trailing neutral angle (default 0 for the 6-arg form).
    let neutral_si = inputs.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    make_flexure_joint(
        "revolute",
        inputs.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        inputs.pivot.clone(),
    )
}

/// `prb_notch_circular(notch_radius, web_thickness, width, material, pivot, axis[, neutral])`
/// — Paros-Weisbord (1965) circular-profile notch flexure as a revolute joint.
///
/// Returns a joint `Value::Map` (`kind == "revolute"`) with rotational stiffness
/// `k_θ = 2·E·b·t^2.5 / (9π·r^0.5)` (PRD §5.2). PRD §10.1 row 2 gate:
/// r=1mm, t=0.2mm, b=5mm, Steel_AISI_1045 ⇒ k_θ ≈ 1.297 N·m/rad (within 2%).
fn prb_notch_circular(args: &[Value]) -> Value {
    match parse_notch_inputs(args) {
        Some(inputs) => notch_revolute(&inputs, CIRCULAR_K, CIRCULAR_SIGMA),
        None => Value::Undef,
    }
}

/// `prb_notch_elliptical(notch_radius, web_thickness, width, material, pivot, axis[, neutral])`
/// — Smith et al. (1997) elliptical-profile notch flexure as a revolute joint.
///
/// Same closed-form structure as `prb_notch_circular` but with a shape factor
/// κ = 0.85 (softer than circular, κ → 1 in the circular-profile limit).
fn prb_notch_elliptical(args: &[Value]) -> Value {
    match parse_notch_inputs(args) {
        Some(inputs) => notch_revolute(&inputs, ELLIPTICAL_K, ELLIPTICAL_SIGMA),
        None => Value::Undef,
    }
}

/// `prb_notch_right_circular(notch_radius, web_thickness, width, material, pivot, axis[, neutral])`
/// — Paros-Weisbord (1965) right-circular (toroidal) notch flexure as a revolute joint.
///
/// Same closed-form structure as `prb_notch_circular` but with a shape factor
/// κ = 0.74 (most compliant of the three profiles, κ → 1 in the planar limit).
fn prb_notch_right_circular(args: &[Value]) -> Value {
    match parse_notch_inputs(args) {
        Some(inputs) => notch_revolute(&inputs, RIGHT_CIRCULAR_K, RIGHT_CIRCULAR_SIGMA),
        None => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::f64::consts::PI;

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

    /// Like [`steel`] but carrying only `youngs_modulus` (no `yield_stress`),
    /// to exercise the no-yield fallback branch.
    fn steel_no_yield() -> Value {
        material(
            "Steel_NoYield",
            &[(
                "youngs_modulus",
                Value::Scalar {
                    si_value: 205e9,
                    dimension: DimensionVector::PRESSURE,
                },
            )],
        )
    }

    /// Material fixture with custom E (for functional-scaling tests).
    fn material_with_e(e: f64) -> Value {
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

    /// Extract the spring_rate si_value from a notch flexure revolute Map.
    fn spring_rate_si(v: &Value) -> f64 {
        match map_get(v, "spring_rate") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// Standard step-1 notch fixture: r=1mm, t=0.2mm, b=5mm, steel.
    fn base_args() -> Vec<Value> {
        vec![
            Value::length(1e-3),
            Value::length(2e-4),
            Value::length(5e-3),
            steel(),
            origin(),
            axis_y(),
        ]
    }

    // ── step-1: spring_rate closed-form pin ──────────────────────────────────

    #[test]
    fn prb_notch_circular_returns_revolute_with_spring_rate() {
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;

        let result = crate::eval_builtin("prb_notch_circular", &base_args());

        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "notch-circular presents as a revolute joint"
        );
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis is preserved verbatim"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is None in δ scope"
        );

        // Paros-Weisbord: k_θ = 2·E·b·t^2.5 / (9π·r^0.5)
        let k_expected = 2.0 * e * b * t.powf(2.5) / (9.0 * PI * r.sqrt());
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

    // ── step-3: yield-capped range ───────────────────────────────────────────

    #[test]
    fn prb_notch_circular_range_is_yield_capped_when_yield_dominates() {
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let e = 205e9_f64;
        let yield_stress = 310e6_f64;

        // θ_yield = yield·3π·(2r+t)/(4·E·t) — PRD §5.2
        let theta_yield = yield_stress * 3.0 * PI * (2.0 * r + t) / (4.0 * e * t);
        let prb_limit = 5.0_f64 * PI / 180.0;
        assert!(
            theta_yield < prb_limit,
            "fixture must exercise the yield-capped branch: θ_yield={theta_yield:.4} rad ≥ 5°"
        );

        let result = crate::eval_builtin("prb_notch_circular", &base_args());
        let range = map_get(&result, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        assert_angle_close(lo, -theta_yield, "yield-capped lower bound");
        assert_angle_close(up, theta_yield, "yield-capped upper bound");
    }

    // ── step-5: PRB-capped and no-yield fallback ─────────────────────────────

    #[test]
    fn prb_notch_circular_range_prb_capped_and_no_yield_fallback() {
        let prb_limit = 5.0_f64 * PI / 180.0;

        // (a) PRB-capped: thin web (r=2mm, t=0.1mm) → θ_yield ≈ 8.37° > 5°
        let thin_web = vec![
            Value::length(2e-3),   // r = 2mm
            Value::length(1e-4),   // t = 0.1mm
            Value::length(5e-3),   // b = 5mm
            steel(),
            origin(),
            axis_y(),
        ];
        {
            let r = 2e-3_f64;
            let t = 1e-4_f64;
            let e = 205e9_f64;
            let yield_stress = 310e6_f64;
            let theta_yield = yield_stress * 3.0 * PI * (2.0 * r + t) / (4.0 * e * t);
            assert!(
                theta_yield > prb_limit,
                "thin-web fixture must exercise the PRB-capped branch: θ_yield={theta_yield:.4} rad"
            );
        }
        let result_thin = crate::eval_builtin("prb_notch_circular", &thin_web);
        let range_thin = map_get(&result_thin, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range_thin);
        assert_angle_close(lo, -prb_limit, "prb-capped lower bound");
        assert_angle_close(up, prb_limit, "prb-capped upper bound");

        // (b) No-yield fallback: steel without yield_stress → ±5°
        let no_yield_args = vec![
            Value::length(1e-3),
            Value::length(2e-4),
            Value::length(5e-3),
            steel_no_yield(),
            origin(),
            axis_y(),
        ];
        let result_ny = crate::eval_builtin("prb_notch_circular", &no_yield_args);
        let range_ny = map_get(&result_ny, "range").expect("range key present");
        let (lo_ny, up_ny) = range_lower_upper(range_ny);
        assert_angle_close(lo_ny, -prb_limit, "no-yield lower bound");
        assert_angle_close(up_ny, prb_limit, "no-yield upper bound");
    }

    // ── step-7: neutral-angle handling ───────────────────────────────────────

    fn notch_circular_with_neutral(neutral: Option<Value>) -> Value {
        let mut args = base_args();
        if let Some(n) = neutral {
            args.push(n);
        }
        crate::eval_builtin("prb_notch_circular", &args)
    }

    #[test]
    fn prb_notch_circular_neutral_angle_handling() {
        let two_deg = 2.0_f64 * PI / 180.0;

        // (a) 6-arg call → neutral defaults to angle(0).
        let six = notch_circular_with_neutral(None);
        assert_eq!(
            map_get(&six, "neutral"),
            Some(&Value::angle(0.0)),
            "6-arg call defaults neutral to angle(0)"
        );

        // (b) 7-arg call with a bare angle(2°) → neutral == angle(2°).
        let seven = notch_circular_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(
            map_get(&seven, "neutral").expect("neutral key present"),
            two_deg,
            "7-arg bare-angle neutral",
        );

        // (c) 7-arg call with Option(Some(angle(2°))) → unwraps to angle(2°).
        let seven_opt = notch_circular_with_neutral(Some(Value::Option(Some(Box::new(
            Value::angle(two_deg),
        )))));
        assert_angle_close(
            map_get(&seven_opt, "neutral").expect("neutral key present"),
            two_deg,
            "7-arg optional-angle neutral",
        );

        // (d) 7-arg call with Option(None) → falls back to angle(0).
        let seven_none = notch_circular_with_neutral(Some(Value::Option(None)));
        assert_eq!(
            map_get(&seven_none, "neutral"),
            Some(&Value::angle(0.0)),
            "7-arg Option(None) neutral defaults to angle(0)"
        );
    }

    // ── step-9: invalid-input rejection ─────────────────────────────────────

    #[test]
    fn prb_notch_circular_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_notch_circular", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let with = |idx: usize, v: Value| {
            let mut a = base_args();
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        {
            let mut a = base_args();
            a.truncate(3);
            undef(a, "3 args");
        }
        {
            let mut a = base_args();
            a.push(Value::angle(0.0));
            a.push(Value::angle(0.0));
            undef(a, "8 args");
        }

        // Non-positive / non-finite geometry.
        undef(with(0, Value::length(0.0)), "r = 0");
        undef(with(0, Value::length(-1e-3)), "r < 0");
        undef(with(1, Value::length(0.0)), "t = 0");
        undef(with(2, Value::length(f64::NAN)), "b = NaN");

        // Degenerate: t ≥ 2r (web ≥ notch diameter).
        undef(with(1, Value::length(2e-3)), "t == 2r");
        undef(with(1, Value::length(3e-3)), "t > 2r");

        // Bad material.
        undef(with(3, Value::Real(1.0)), "material not a StructureInstance");
        undef(
            with(3, material("NoModulus", &[])),
            "material missing youngs_modulus",
        );

        // Bad axis.
        undef(with(5, Value::Real(1.0)), "axis not a vector");
        undef(
            with(
                5,
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
            "axis is zero vector",
        );
        undef(
            with(
                5,
                Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(1.0),
                    Value::length(0.0),
                ]),
            ),
            "axis is length-dimensioned",
        );
    }

    // ── step-11: elliptical structure + functional-form scaling ─────────────

    /// Call `prb_notch_elliptical` with explicit geometry and material.
    fn notch_elliptical(r: f64, t: f64, b: f64, mat: Value) -> Value {
        crate::eval_builtin(
            "prb_notch_elliptical",
            &[
                Value::length(r),
                Value::length(t),
                Value::length(b),
                mat,
                origin(),
                axis_y(),
            ],
        )
    }

    #[test]
    fn prb_notch_elliptical_structure_and_scaling() {
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;

        let base = notch_elliptical(r, t, b, material_with_e(e));
        assert_eq!(
            map_get(&base, "kind"),
            Some(&Value::String("revolute".to_string())),
            "elliptical presents as revolute"
        );
        assert_eq!(
            map_get(&base, "damping"),
            Some(&Value::Option(None)),
            "damping is None"
        );
        assert_eq!(
            map_get(&base, "axis"),
            Some(&axis_y()),
            "axis preserved"
        );
        assert_eq!(
            map_get(&base, "neutral"),
            Some(&Value::angle(0.0)),
            "neutral defaults to angle(0)"
        );
        let k_base = spring_rate_si(&base);
        assert!(k_base.is_finite() && k_base > 0.0, "spring_rate is finite positive");
        assert_eq!(
            map_get(&base, "spring_rate").map(|v| match v {
                Value::Scalar { dimension, .. } => *dimension,
                _ => panic!(),
            }),
            Some(DimensionVector::ROTATIONAL_STIFFNESS),
            "spring_rate carries ROTATIONAL_STIFFNESS"
        );

        // Coefficient-independent functional-form scaling (κ cancels in ratios).

        // t^2.5 scaling: doubling t → ×2^2.5
        let k_2t = spring_rate_si(&notch_elliptical(r, 2.0 * t, b, material_with_e(e)));
        let ratio_t = k_2t / k_base;
        let expected_t = 2.0_f64.powf(2.5);
        let rel_t = (ratio_t - expected_t).abs() / expected_t;
        assert!(rel_t < 1e-9, "t^2.5 scaling: ratio {ratio_t} vs {expected_t} (rel {rel_t})");

        // r^-0.5 scaling: doubling r → ×2^-0.5
        let k_2r = spring_rate_si(&notch_elliptical(2.0 * r, t, b, material_with_e(e)));
        let ratio_r = k_2r / k_base;
        let expected_r = 2.0_f64.powf(-0.5);
        let rel_r = (ratio_r - expected_r).abs() / expected_r;
        assert!(rel_r < 1e-9, "r^-0.5 scaling: ratio {ratio_r} vs {expected_r} (rel {rel_r})");

        // b scaling: doubling b → ×2
        let k_2b = spring_rate_si(&notch_elliptical(r, t, 2.0 * b, material_with_e(e)));
        let ratio_b = k_2b / k_base;
        let rel_b = (ratio_b - 2.0).abs() / 2.0;
        assert!(rel_b < 1e-9, "b linear scaling: ratio {ratio_b} vs 2 (rel {rel_b})");

        // E scaling: doubling E → ×2
        let k_2e = spring_rate_si(&notch_elliptical(r, t, b, material_with_e(2.0 * e)));
        let ratio_e = k_2e / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(rel_e < 1e-9, "E linear scaling: ratio {ratio_e} vs 2 (rel {rel_e})");

        // Invalid-input rejection: spot-check degenerate geometry.
        let bad_t = crate::eval_builtin(
            "prb_notch_elliptical",
            &[
                Value::length(r),
                Value::length(2.0 * r),  // t >= 2r
                Value::length(b),
                material_with_e(e),
                origin(),
                axis_y(),
            ],
        );
        assert!(bad_t.is_undef(), "elliptical degenerate t>=2r → Undef");

        let bad_arity = crate::eval_builtin("prb_notch_elliptical", &[]);
        assert!(bad_arity.is_undef(), "elliptical 0 args → Undef");
    }

    // ── step-13: right-circular structure + functional-form scaling ──────────

    /// Call `prb_notch_right_circular` with explicit geometry and material.
    fn notch_right_circular(r: f64, t: f64, b: f64, mat: Value) -> Value {
        crate::eval_builtin(
            "prb_notch_right_circular",
            &[
                Value::length(r),
                Value::length(t),
                Value::length(b),
                mat,
                origin(),
                axis_y(),
            ],
        )
    }

    #[test]
    fn prb_notch_right_circular_structure_and_scaling() {
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;

        let base = notch_right_circular(r, t, b, material_with_e(e));
        assert_eq!(
            map_get(&base, "kind"),
            Some(&Value::String("revolute".to_string())),
            "right-circular presents as revolute"
        );
        assert_eq!(
            map_get(&base, "damping"),
            Some(&Value::Option(None)),
            "damping is None"
        );
        assert_eq!(
            map_get(&base, "axis"),
            Some(&axis_y()),
            "axis preserved"
        );
        assert_eq!(
            map_get(&base, "neutral"),
            Some(&Value::angle(0.0)),
            "neutral defaults to angle(0)"
        );
        let k_base = spring_rate_si(&base);
        assert!(k_base.is_finite() && k_base > 0.0, "spring_rate is finite positive");
        assert_eq!(
            map_get(&base, "spring_rate").map(|v| match v {
                Value::Scalar { dimension, .. } => *dimension,
                _ => panic!(),
            }),
            Some(DimensionVector::ROTATIONAL_STIFFNESS),
            "spring_rate carries ROTATIONAL_STIFFNESS"
        );

        // Coefficient-independent functional-form scaling (κ cancels in ratios).

        // t^2.5 scaling
        let k_2t = spring_rate_si(&notch_right_circular(r, 2.0 * t, b, material_with_e(e)));
        let ratio_t = k_2t / k_base;
        let expected_t = 2.0_f64.powf(2.5);
        let rel_t = (ratio_t - expected_t).abs() / expected_t;
        assert!(rel_t < 1e-9, "t^2.5 scaling: ratio {ratio_t} vs {expected_t} (rel {rel_t})");

        // r^-0.5 scaling
        let k_2r = spring_rate_si(&notch_right_circular(2.0 * r, t, b, material_with_e(e)));
        let ratio_r = k_2r / k_base;
        let expected_r = 2.0_f64.powf(-0.5);
        let rel_r = (ratio_r - expected_r).abs() / expected_r;
        assert!(rel_r < 1e-9, "r^-0.5 scaling: ratio {ratio_r} vs {expected_r} (rel {rel_r})");

        // b linear scaling
        let k_2b = spring_rate_si(&notch_right_circular(r, t, 2.0 * b, material_with_e(e)));
        let ratio_b = k_2b / k_base;
        let rel_b = (ratio_b - 2.0).abs() / 2.0;
        assert!(rel_b < 1e-9, "b linear scaling: ratio {ratio_b} vs 2 (rel {rel_b})");

        // E linear scaling
        let k_2e = spring_rate_si(&notch_right_circular(r, t, b, material_with_e(2.0 * e)));
        let ratio_e = k_2e / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(rel_e < 1e-9, "E linear scaling: ratio {ratio_e} vs 2 (rel {rel_e})");

        // Invalid-input rejection.
        let bad_t = crate::eval_builtin(
            "prb_notch_right_circular",
            &[
                Value::length(r),
                Value::length(2.0 * r),  // t >= 2r
                Value::length(b),
                material_with_e(e),
                origin(),
                axis_y(),
            ],
        );
        assert!(bad_t.is_undef(), "right-circular degenerate t>=2r → Undef");

        let bad_arity = crate::eval_builtin("prb_notch_right_circular", &[]);
        assert!(bad_arity.is_undef(), "right-circular 0 args → Undef");
    }
}
