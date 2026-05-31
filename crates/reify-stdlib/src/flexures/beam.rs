//! Beam-flexure PRB constructors (Howell §5): cantilever beam (revolute) and
//! fixed-fixed beam (transverse prismatic).
//!
//! Both constructors share the positional argument layout
//! `(length, width, thickness, material, pivot, axis[, neutral])` and the
//! [`parse_beam_inputs`] validation path; they differ only in the closed-form
//! stiffness, the validity range, and the joint kind.

use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    length_si, make_flexure_joint, material_field_si, neutral_angle_si, symmetric_angle_range,
    PRB_ANGLE_LIMIT_RAD,
};

/// Howell pseudo-rigid-body coefficient for a cantilever beam (Howell §5.1).
const CANTILEVER_GAMMA: f64 = 2.65;

/// Transverse stiffness coefficient for a fixed-guided (fixed-fixed) beam:
/// `k_trans = γ_ff·E·I / L³` with γ_ff = 12. This matches the PRD §6.1
/// parallelogram-blade fixed-guided coefficient (γ_pp = 12) — the standard model
/// for a beam translating transversely while both ends remain oriented.
const FIXED_FIXED_GAMMA: f64 = 12.0;

/// Fallback transverse-displacement validity limit as a fraction of beam length,
/// used for the fixed-fixed beam when the material carries no `yield_stress`.
/// The PRB transverse small-deflection model degrades past ~0.1·L.
const SMALL_DEFLECTION_FRACTION: f64 = 0.1;

/// Evaluate a beam-flexure constructor by name.
///
/// Returns `Some(Value)` for a recognised flexure name (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so `eval_builtin` falls through to the next module.
pub(crate) fn eval_beam(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_cantilever_beam" => Some(prb_cantilever_beam(args)),
        "prb_fixed_fixed_beam" => Some(prb_fixed_fixed_beam(args)),
        _ => None,
    }
}

/// Shared, validated inputs for both beam-flexure constructors.
struct BeamInputs<'a> {
    /// Beam length L (metres).
    length: f64,
    /// Beam thickness t in the bending direction (metres).
    thickness: f64,
    /// Young's modulus E (Pa).
    e: f64,
    /// Rectangular-section second moment of area `I = width·thickness³/12`.
    i: f64,
    /// Material yield stress (Pa), if the material carries one.
    yield_si: Option<f64>,
    /// The raw axis argument, stored verbatim on the joint Map.
    axis: &'a Value,
    /// The raw pivot argument, stored verbatim on the joint Map.
    pivot: &'a Value,
    /// The optional trailing `neutral` argument (present only in the 7-arg form).
    neutral_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout of both
/// beam-flexure constructors: `(length, width, thickness, material, pivot,
/// axis[, neutral])`.
///
/// Returns `None` (⇒ the caller returns `Value::Undef`) on: arity ∉ {6, 7};
/// non-positive or non-finite geometry; a degenerate beam (thickness ≥ length —
/// the `E_FlexureGeometryInvalid` regime, whose diagnostic task λ (3821) owns);
/// a material that is not a `Value::StructureInstance` with a finite
/// `youngs_modulus` > 0; or an axis that is not a finite, non-zero,
/// dimensionless 3-vector.
fn parse_beam_inputs(args: &[Value]) -> Option<BeamInputs<'_>> {
    if args.len() != 6 && args.len() != 7 {
        return None;
    }
    let length = length_si(&args[0])?;
    let width = length_si(&args[1])?;
    let thickness = length_si(&args[2])?;
    if length <= 0.0 || width <= 0.0 || thickness <= 0.0 || thickness >= length {
        return None;
    }
    let material = &args[3];
    let e = material_field_si(material, "youngs_modulus")?;
    if e <= 0.0 {
        return None;
    }
    let axis = &args[5];
    crate::helpers::validate_dimensionless_unit_axis_vec3(axis)?;
    Some(BeamInputs {
        length,
        thickness,
        e,
        i: width * thickness.powi(3) / 12.0,
        yield_si: material_field_si(material, "yield_stress"),
        axis,
        pivot: &args[4],
        neutral_arg: if args.len() == 7 { Some(&args[6]) } else { None },
    })
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
/// Returns `Value::Undef` on the invalid-input classes enumerated in
/// [`parse_beam_inputs`].
fn prb_cantilever_beam(args: &[Value]) -> Value {
    let Some(b) = parse_beam_inputs(args) else {
        return Value::Undef;
    };

    // Howell PRB rotational stiffness k_θ = γ·E·I / L (Howell §5.1, γ = 2.65).
    let k_theta = CANTILEVER_GAMMA * b.e * b.i / b.length;

    // Symmetric prb_validity range = ±min(θ_yield, 5°). θ_yield is the
    // surface-yield rotation (Howell §5.1: σ(θ) = E·(t/2)·θ/L ⇒
    // θ_yield = yield·L/(E·t/2)); the 5° PRB limit bounds small-deflection
    // fidelity. A material without a yield_stress contributes only the 5° cap.
    let theta_lim = match b.yield_si {
        Some(yield_si) => {
            let theta_yield = yield_si * b.length / (b.e * b.thickness / 2.0);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };
    let range = symmetric_angle_range(theta_lim);

    // Optional trailing neutral angle (default 0 for the 6-arg form).
    let neutral_si = b.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    make_flexure_joint(
        "revolute",
        b.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        b.pivot.clone(),
    )
}

/// `prb_fixed_fixed_beam(length, width, thickness, material, pivot, axis[, neutral])`
/// — a Howell pseudo-rigid-body fixed-fixed (fixed-guided) beam flexure presented
/// as a transverse prismatic joint.
///
/// Returns a joint `Value::Map` (`kind == "prismatic"`) whose transverse
/// stiffness is the closed-form `k_trans = γ_ff·E·I / L³` (γ_ff = 12; Howell
/// §5 / PRD §6.1 fixed-guided bending), with `I = width·thickness³/12`.
///
/// The symmetric transverse-displacement validity range is `±δ`, where
/// `δ = yield·L²/(3·E·t)` is the fixed-guided surface-yield deflection
/// (σ = 3·E·t·δ / L²); a material without a `yield_stress` falls back to a
/// documented 10%-of-length small-deflection limit. The range is exercised for
/// shape (finite, symmetric, LENGTH-dimensioned) — its magnitude is a design
/// choice, not an externally-validated bound.
///
/// Returns `Value::Undef` on the same invalid-input classes as
/// [`prb_cantilever_beam`] (see [`parse_beam_inputs`]).
fn prb_fixed_fixed_beam(args: &[Value]) -> Value {
    let Some(b) = parse_beam_inputs(args) else {
        return Value::Undef;
    };

    // Fixed-guided transverse stiffness k_trans = γ_ff·E·I / L³ (γ_ff = 12).
    let k_trans = FIXED_FIXED_GAMMA * b.e * b.i / b.length.powi(3);

    // Symmetric transverse-displacement validity range = ±δ. Fixed-guided
    // bending stress σ = 3·E·t·δ / L² ⇒ δ_yield = yield·L² / (3·E·t). With no
    // material yield_stress, fall back to a documented small-deflection fraction
    // of the beam length.
    let delta = match b.yield_si {
        Some(yield_si) => yield_si * b.length.powi(2) / (3.0 * b.e * b.thickness),
        None => SMALL_DEFLECTION_FRACTION * b.length,
    };
    let range = Value::range(
        Some(Value::length(-delta)),
        Some(Value::length(delta)),
        true,
        true,
    );

    // Optional trailing neutral transverse offset (default 0 for the 6-arg form).
    let neutral_si = b.neutral_arg.map(neutral_length_si).unwrap_or(0.0);

    make_flexure_joint(
        "prismatic",
        b.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_trans,
            dimension: DimensionVector::TRANSLATIONAL_STIFFNESS,
        },
        Value::length(neutral_si),
        b.pivot.clone(),
    )
}

/// Extract a neutral transverse offset in metres from a trailing constructor
/// argument (the prismatic counterpart of `common::neutral_angle_si`).
///
/// Accepts a LENGTH-dimensioned `Value::Scalar` (e.g. `Value::length`), a bare
/// `Value::Real` / `Value::Int` interpreted as metres (via [`length_si`]), or a
/// `Value::Option` wrapping any of those. `Option(None)` and any value that
/// fails extraction default to `0.0`.
fn neutral_length_si(v: &Value) -> f64 {
    match v {
        Value::Option(Some(inner)) => neutral_length_si(inner),
        Value::Option(None) => 0.0,
        other => length_si(other).unwrap_or(0.0),
    }
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

    /// Like [`steel`] but carrying only `youngs_modulus` (no `yield_stress`),
    /// to exercise the no-yield fallback branches of both beam constructors.
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

    /// A valid 6-arg cantilever argument list (step-1 geometry).
    fn valid_cantilever_args() -> Vec<Value> {
        vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ]
    }

    #[test]
    fn prb_cantilever_beam_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_cantilever_beam", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        // Substitute one slot of the valid arg list.
        let with = |idx: usize, v: Value| {
            let mut a = valid_cantilever_args();
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        {
            let mut a = valid_cantilever_args();
            a.truncate(3);
            undef(a, "3 args");
        }
        {
            let mut a = valid_cantilever_args();
            a.push(Value::angle(0.0));
            a.push(Value::angle(0.0));
            undef(a, "8 args");
        }

        // Non-positive / non-finite geometry.
        undef(with(0, Value::length(0.0)), "length = 0");
        undef(with(0, Value::length(-0.02)), "length < 0");
        undef(with(1, Value::length(0.0)), "width = 0");
        undef(with(2, Value::length(f64::NAN)), "thickness = NaN");

        // Degenerate beam: thickness ≥ length (E_FlexureGeometryInvalid regime;
        // γ returns Undef without emitting the diagnostic, which λ owns).
        undef(with(2, Value::length(0.02)), "thickness == length");
        undef(
            {
                let mut a = valid_cantilever_args();
                a[0] = Value::length(0.02);
                a[2] = Value::length(0.03);
                a
            },
            "thickness > length",
        );

        // Bad material.
        undef(with(3, Value::Real(1.0)), "material not a StructureInstance");
        undef(with(3, material("NoModulus", &[])), "material missing youngs_modulus");

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

    /// Assert `v` is a LENGTH-dimensioned Scalar and return its si_value.
    fn length_scalar_si(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "{label}: bound carries LENGTH dimension"
                );
                *si_value
            }
            other => panic!("{label}: expected LENGTH Scalar, got {other:?}"),
        }
    }

    #[test]
    fn prb_fixed_fixed_beam_returns_prismatic_with_spring_rate() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let result = crate::eval_builtin(
            "prb_fixed_fixed_beam",
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
            Some(&Value::String("prismatic".to_string())),
            "fixed-fixed flexure presents as a prismatic joint"
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
        assert_eq!(
            map_get(&result, "neutral"),
            Some(&Value::length(0.0)),
            "6-arg call defaults neutral to length(0) (transverse translation)"
        );

        // Closed-form reproduction: k_trans = γ_ff·E·I / L³ with γ_ff = 12.
        let i = width * thickness.powi(3) / 12.0;
        let k_expected = 12.0 * e * i / length.powi(3);
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar {
                si_value,
                dimension,
            }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::TRANSLATIONAL_STIFFNESS,
                    "spring_rate carries TRANSLATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(
                    rel < 1e-9,
                    "spring_rate {si_value} vs {k_expected} (rel {rel})"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }

        // Range shape only (no magnitude pin): finite, LENGTH-dimensioned,
        // symmetric (lower == -upper), and non-zero.
        let range = map_get(&result, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        let lo_si = length_scalar_si(lo, "range lower");
        let up_si = length_scalar_si(up, "range upper");
        assert!(
            lo_si.is_finite() && up_si.is_finite(),
            "range bounds are finite (lo={lo_si}, up={up_si})"
        );
        assert!(up_si != 0.0, "range is non-zero (up={up_si})");
        let sym = (lo_si + up_si).abs() / up_si.abs();
        assert!(sym < 1e-9, "range is symmetric: lower {lo_si} == -upper {up_si}");

        // Pin the yield-branch δ magnitude (closed-form reproduction, matching
        // the rigor of the cantilever range tests): the fixed-guided
        // surface-yield deflection δ = yield·L²/(3·E·t), from σ = 3·E·t·δ / L².
        let yield_stress = 310e6_f64; // steel() fixture
        let expected_delta = yield_stress * length.powi(2) / (3.0 * e * thickness);
        let rel_delta = (up_si - expected_delta).abs() / expected_delta;
        assert!(
            rel_delta < 1e-9,
            "fixed-fixed δ {up_si} vs analytic {expected_delta} (rel {rel_delta})"
        );
    }

    #[test]
    fn prb_fixed_fixed_beam_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_fixed_fixed_beam", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let with = |idx: usize, v: Value| {
            let mut a = valid_cantilever_args();
            a[idx] = v;
            a
        };

        undef(vec![], "0 args");
        {
            let mut a = valid_cantilever_args();
            a.truncate(3);
            undef(a, "3 args");
        }
        undef(with(0, Value::length(0.0)), "length = 0");
        undef(with(0, Value::length(-0.02)), "length < 0");
        undef(with(1, Value::length(0.0)), "width = 0");
        undef(with(2, Value::length(f64::NAN)), "thickness = NaN");
        undef(with(2, Value::length(0.02)), "thickness == length");
        undef(with(3, Value::Real(1.0)), "material not a StructureInstance");
        undef(
            with(3, material("NoModulus", &[])),
            "material missing youngs_modulus",
        );
        undef(with(5, Value::Real(1.0)), "axis not a vector");
        undef(
            with(
                5,
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
            "axis is zero vector",
        );
    }

    #[test]
    fn prb_beams_fall_back_to_default_limits_without_yield_stress() {
        // A material with no yield_stress drives both ctors onto their fallback
        // validity-range branches (the `None` arms of theta_lim / delta), which
        // the yield-carrying `steel()` fixture never reaches.
        let length = 0.02_f64;
        let call = |name: &str| {
            crate::eval_builtin(
                name,
                &[
                    Value::length(length),
                    Value::length(0.005),
                    Value::length(0.0005),
                    steel_no_yield(),
                    origin(),
                    axis_y(),
                ],
            )
        };

        // (a) Cantilever: no yield ⇒ range is the ±5° PRB small-deflection cap.
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;
        let cantilever = call("prb_cantilever_beam");
        let (lo, up) =
            range_lower_upper(map_get(&cantilever, "range").expect("range key present"));
        assert_angle_close(lo, -prb_limit, "no-yield cantilever lower bound");
        assert_angle_close(up, prb_limit, "no-yield cantilever upper bound");

        // (b) Fixed-fixed: no yield ⇒ range falls back to ±(0.1·L).
        let expected_delta = 0.1 * length;
        let fixed_fixed = call("prb_fixed_fixed_beam");
        let (lo, up) =
            range_lower_upper(map_get(&fixed_fixed, "range").expect("range key present"));
        let lo_si = length_scalar_si(lo, "no-yield fixed-fixed lower bound");
        let up_si = length_scalar_si(up, "no-yield fixed-fixed upper bound");
        let rel_up = (up_si - expected_delta).abs() / expected_delta;
        assert!(
            rel_up < 1e-9,
            "no-yield fixed-fixed upper {up_si} vs {expected_delta} (rel {rel_up})"
        );
        let rel_lo = (lo_si + expected_delta).abs() / expected_delta;
        assert!(
            rel_lo < 1e-9,
            "no-yield fixed-fixed lower {lo_si} vs -{expected_delta} (rel {rel_lo})"
        );
    }

    /// Invoke `prb_fixed_fixed_beam` on the step-9 geometry, optionally appending
    /// a 7th `neutral` (transverse offset) arg — the prismatic counterpart of
    /// [`cantilever_with_neutral`].
    fn fixed_fixed_with_neutral(neutral: Option<Value>) -> Value {
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
        crate::eval_builtin("prb_fixed_fixed_beam", &args)
    }

    #[test]
    fn prb_fixed_fixed_beam_neutral_length_handling() {
        let offset = 0.001_f64; // 1 mm transverse neutral offset

        // (a) 6-arg call → neutral defaults to length(0).
        let six = fixed_fixed_with_neutral(None);
        assert_eq!(
            map_get(&six, "neutral"),
            Some(&Value::length(0.0)),
            "6-arg call defaults neutral to length(0)"
        );

        // (b) 7-arg call with a bare length → neutral == length(offset).
        let seven = fixed_fixed_with_neutral(Some(Value::length(offset)));
        assert_eq!(
            map_get(&seven, "neutral"),
            Some(&Value::length(offset)),
            "7-arg bare-length neutral"
        );

        // (c) 7-arg call with Option(Some(length)) → unwraps to length(offset).
        let seven_opt =
            fixed_fixed_with_neutral(Some(Value::Option(Some(Box::new(Value::length(offset))))));
        assert_eq!(
            map_get(&seven_opt, "neutral"),
            Some(&Value::length(offset)),
            "7-arg optional-length neutral unwraps"
        );

        // (d) 7-arg call with Option(None) → falls back to length(0).
        let seven_none = fixed_fixed_with_neutral(Some(Value::Option(None)));
        assert_eq!(
            map_get(&seven_none, "neutral"),
            Some(&Value::length(0.0)),
            "7-arg Option(None) neutral defaults to length(0)"
        );
    }
}
