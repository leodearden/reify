//! Beam-flexure PRB constructors (Howell §5): cantilever beam (revolute) and
//! fixed-fixed beam (transverse prismatic).
//!
//! Both constructors share the positional argument layout
//! `(length, width, thickness, material, pivot, axis[, neutral])` and the
//! [`parse_beam_inputs`] validation path; they differ only in the closed-form
//! stiffness, the validity range, and the joint kind.

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    attach_compliance, cantilever_sigma_at, cantilever_theta_lim, fixed_guided_delta_max,
    fixed_guided_sigma_at, length_si, make_compliance_record, make_flexure_joint,
    material_field_si, neutral_angle_si, parse_declared_range, symmetric_angle_range, RangeKind,
    CANTILEVER_GAMMA, FIXED_GUIDED_GAMMA,
};

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
    /// The optional trailing `neutral` argument (present in the 7- and 8-arg forms).
    neutral_arg: Option<&'a Value>,
    /// The optional trailing declared operating-range argument (present only in
    /// the 8-arg form). When present, its endpoint — not the auto cap — drives
    /// the joint range and the §5.3 `max_stress` stress-check.
    declared_range_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout of both
/// beam-flexure constructors: `(length, width, thickness, material, pivot,
/// axis[, neutral[, declared_range]])`.
///
/// Returns `None` (⇒ the caller returns `Value::Undef`) on: arity ∉ {6, 7, 8};
/// non-positive or non-finite geometry; a degenerate beam (thickness ≥ length —
/// the `E_FlexureGeometryInvalid` regime, whose diagnostic task λ (3821) owns);
/// a material that is not a `Value::StructureInstance` with a finite
/// `youngs_modulus` > 0; or an axis that is not a finite, non-zero,
/// dimensionless 3-vector.
fn parse_beam_inputs(args: &[Value]) -> Option<BeamInputs<'_>> {
    if args.len() < 6 || args.len() > 8 {
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
        neutral_arg: if args.len() >= 7 { Some(&args[6]) } else { None },
        declared_range_arg: if args.len() == 8 { Some(&args[7]) } else { None },
    })
}

/// `prb_cantilever_beam(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])`
/// — a Howell pseudo-rigid-body cantilever flexure presented as a revolute joint.
///
/// Returns a joint `Value::Map` (`kind == "revolute"`) whose rotational
/// stiffness is the closed-form `k_θ = γ·E·I / L` (Howell §5.1, γ = 2.65), with
/// `I = width·thickness³/12` the rectangular-section second moment of area.
///
/// The auto-computed symmetric `prb_validity` rotation cap is `±min(θ_yield,
/// 5°)`, where `θ_yield = yield·L/(E·t/2)` is the surface-yield rotation and 5°
/// is the PRB small-deflection limit. When the material carries no
/// `yield_stress`, only the 5° PRB limit applies.
///
/// An optional trailing `declared_range` (a half-angle) OVERRIDES that cap for
/// the joint range and the cached `max_stress` endpoint (§5.3 evaluates surface
/// stress at the declared endpoint); the auto cap is retained as
/// `prb_validity_range` in the compliance record as the SAFE/suggested bound.
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
    let theta_lim = cantilever_theta_lim(b.length, b.thickness, b.e, b.yield_si);

    // An optional user-declared operating range (±half-angle) OVERRIDES the auto
    // ±min(θ_yield, 5°) cap for the joint range and the §5.3 stress endpoint; the
    // auto θ_lim is retained as the SAFE/suggested range in the compliance record.
    let declared = parse_declared_range(b.declared_range_arg, RangeKind::Angle);
    let range_endpoint = declared.unwrap_or(theta_lim);
    let range = symmetric_angle_range(range_endpoint);

    // Optional trailing neutral angle (default 0 for the 6-arg form).
    let neutral_si = b.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    let joint = make_flexure_joint(
        "revolute",
        b.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        b.pivot.clone(),
    );

    // Cache the FlexureCompliance record (§5.3): surface bending stress at the
    // range endpoint (the declared endpoint when present, else the auto θ_lim —
    // the worst-case operating stress) and at the neutral rest angle.
    // prb_validity_range stores the auto SAFE half-angle θ_lim regardless of any
    // wider declared range, so it always advertises the PRB-valid bound.
    let max_stress = cantilever_sigma_at(range_endpoint, b.length, b.thickness, b.e);
    let max_stress_at_neutral = cantilever_sigma_at(neutral_si, b.length, b.thickness, b.e);
    let record = make_compliance_record(
        k_theta,
        max_stress,
        max_stress_at_neutral,
        b.yield_si,
        None,
        symmetric_angle_range(theta_lim),
    );
    attach_compliance(joint, record)
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
    let k_trans = FIXED_GUIDED_GAMMA * b.e * b.i / b.length.powi(3);

    // Auto symmetric transverse-displacement validity δ_auto. Fixed-guided
    // bending stress σ = 3·E·t·δ / L² ⇒ δ_yield = yield·L² / (3·E·t). With no
    // material yield_stress, fall back to a documented small-deflection fraction
    // of the beam length. Retained as the SAFE prb_validity_range below.
    let delta_auto = fixed_guided_delta_max(b.length, b.thickness, b.e, b.yield_si);

    // An optional user-declared operating range (±half-displacement LENGTH)
    // OVERRIDES the auto δ_auto for the joint range and the §5.3 stress endpoint;
    // δ_auto is retained as the SAFE/suggested range in the compliance record.
    let declared = parse_declared_range(b.declared_range_arg, RangeKind::Length);
    let range_endpoint = declared.unwrap_or(delta_auto);
    let range = Value::range(
        Some(Value::length(-range_endpoint)),
        Some(Value::length(range_endpoint)),
        true,
        true,
    );

    // Optional trailing neutral transverse offset (default 0 for the 6-arg form).
    let neutral_si = b.neutral_arg.map(neutral_length_si).unwrap_or(0.0);

    let joint = make_flexure_joint(
        "prismatic",
        b.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_trans,
            dimension: DimensionVector::TRANSLATIONAL_STIFFNESS,
        },
        Value::length(neutral_si),
        b.pivot.clone(),
    );

    // Cache the FlexureCompliance record (§5.3): fixed-guided surface stress at
    // the range endpoint (declared when present, else the auto δ_auto — the
    // worst-case operating stress) and at the neutral rest offset.
    // prb_validity_range stores the auto SAFE δ_auto regardless of any wider
    // declared range, so it always advertises the PRB-valid bound.
    let max_stress = fixed_guided_sigma_at(range_endpoint, b.length, b.thickness, b.e);
    let max_stress_at_neutral = fixed_guided_sigma_at(neutral_si, b.length, b.thickness, b.e);
    let record = make_compliance_record(
        k_trans,
        max_stress,
        max_stress_at_neutral,
        b.yield_si,
        None,
        symmetric_angle_range(delta_auto),
    );
    attach_compliance(joint, record)
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
    use super::super::test_util::{angle_range_half_si, length_range_half_si};
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

    /// Read the `__flexure_compliance` FlexureCompliance record's fields from a
    /// flexure joint Map (panics if absent or the wrong shape).
    fn compliance_fields(joint: &Value) -> &PersistentMap<String, Value> {
        match map_get(joint, "__flexure_compliance") {
            Some(Value::StructureInstance(d)) => {
                assert_eq!(
                    d.type_name, "FlexureCompliance",
                    "__flexure_compliance is a FlexureCompliance record"
                );
                &d.fields
            }
            other => panic!("expected __flexure_compliance StructureInstance, got {other:?}"),
        }
    }

    #[test]
    fn prb_cantilever_beam_attaches_populated_compliance() {
        // step-1 safe geometry (L=20mm, w=5mm, t=0.5mm, steel): θ_yield ≈ 6.93° >
        // 5°, so the auto prb_validity endpoint is the ±5° PRB cap and the surface
        // stress there stays below yield ⇒ at_yield == false.
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;

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
        let fields = compliance_fields(&result);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("FlexureCompliance missing `{k}`"))
        };

        // effective_stiffness (Real) == the joint's spring_rate si (k_θ).
        let spring_si = match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        };
        match f("effective_stiffness") {
            Value::Real(r) => assert!(
                (r - spring_si).abs() / spring_si < 1e-12,
                "effective_stiffness {r} == spring_rate {spring_si}"
            ),
            other => panic!("effective_stiffness Real, got {other:?}"),
        }

        // max_stress (PRESSURE) == E·(t/2)·θ_end/L at θ_end = the ±5° cap.
        let theta_end = prb_limit;
        let expected_sigma = e * (thickness / 2.0) * theta_end / length;
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected_sigma).abs() / expected_sigma < 1e-9,
                    "max_stress {si_value} vs analytic {expected_sigma}"
                );
                assert!(
                    *si_value < 310e6,
                    "5° endpoint stays below the 310MPa yield ({si_value})"
                );
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }

        // at_yield == false at the auto (safe) endpoint.
        assert_eq!(f("at_yield"), &Value::Bool(false), "auto endpoint is not at yield");

        // prb_validity_range is now Range<Angle> = [−θ_end, +θ_end] (task 4576).
        let (_, up) = range_lower_upper(map_get(&result, "range").expect("range present"));
        let range_half = match up {
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("range upper Scalar, got {other:?}"),
        };
        let prb_half = angle_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half - range_half).abs() / range_half < 1e-9,
            "prb_validity_range half {prb_half} == joint range half-angle {range_half}"
        );
    }

    #[test]
    fn prb_cantilever_beam_declared_range_override() {
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let ten_deg = 10.0_f64 * std::f64::consts::PI / 180.0;

        // Build a cantilever with an explicit neutral (0°) plus a trailing
        // declared operating range (±10°). Arg layout:
        //   (length, width, thickness, material, pivot, axis, neutral, declared_range)
        // — the declared_range is the new highest-arity (8th) slot, mirroring
        // examples/flexures/yield_warning.ri. The arg is the symmetric
        // half-width as an Angle, so `angle(10°)` ⇒ a ±10° joint range.
        let call = |length: f64, thickness: f64| {
            crate::eval_builtin(
                "prb_cantilever_beam",
                &[
                    Value::length(length),
                    Value::length(0.005),
                    Value::length(thickness),
                    steel(),
                    origin(),
                    axis_y(),
                    Value::angle(0.0),     // neutral
                    Value::angle(ten_deg), // declared ±10° half-width
                ],
            )
        };

        // ── Yielding geometry: t=0.05 mm, L=2 mm (σ(10°) ≈ 447 MPa > 310 MPa). ──
        let length = 0.002_f64;
        let thickness = 0.00005_f64;
        let yielding = call(length, thickness);

        // (5) The ctor STILL returns a valid revolute joint (not Undef) even
        // though the declared range drives surface stress past yield.
        assert!(
            !yielding.is_undef(),
            "yielding declared-range call returns a joint, not Undef"
        );
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("revolute".to_string())),
            "yielding flexure is still a revolute joint"
        );

        // (a) The joint `range` is the declared ±10°, OVERRIDING the auto cap
        // (±min(θ_yield, 5°), far narrower for this short/thin geometry).
        let (lo, up) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert_angle_close(lo, -ten_deg, "declared-range lower bound");
        assert_angle_close(up, ten_deg, "declared-range upper bound");

        let fields = compliance_fields(&yielding);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("FlexureCompliance missing `{k}`"))
        };

        // (b) max_stress is evaluated at the DECLARED 10° endpoint:
        // σ = E·(t/2)·θ/L ≈ 447 MPa.
        let expected_sigma = e * (thickness / 2.0) * ten_deg / length;
        assert!(
            expected_sigma > yield_si,
            "fixture sanity: σ(10°)={expected_sigma} must exceed yield {yield_si}"
        );
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected_sigma).abs() / expected_sigma < 1e-9,
                    "max_stress {si_value} vs analytic σ(10°) {expected_sigma}"
                );
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }

        // (c) at_yield == true and yield_margin < 0 in the yielding regime.
        assert_eq!(
            f("at_yield"),
            &Value::Bool(true),
            "declared 10° drives at_yield true"
        );
        match f("yield_margin") {
            Value::Real(r) => assert!(*r < 0.0, "yielding ⇒ negative margin, got {r}"),
            other => panic!("yield_margin Real, got {other:?}"),
        }

        // ── Safe geometry: t=0.05 mm, L=20 mm (σ(10°) ≈ 44.7 MPa < 310 MPa). ──
        // Same ±10° declared range, but the 10× longer beam keeps σ below yield
        // (σ scales as 1/L), so at_yield stays false. NOTE: the plan's step-5
        // example "t=0.5mm,L=20mm" has t/L=0.025 — identical to the yielding
        // fixture (t=0.05mm/L=2mm) — and would itself yield at 10°. t=0.05mm/
        // L=20mm matches the plan design-decision's "σ≈44MPa at L=20mm" safe
        // reference, so the at_yield==false assertion is physically reachable.
        let safe = call(0.02, 0.00005);
        assert!(!safe.is_undef(), "safe declared-range call returns a joint, not Undef");
        let safe_fields = compliance_fields(&safe);
        assert_eq!(
            safe_fields
                .get(&"at_yield".to_string())
                .expect("at_yield present"),
            &Value::Bool(false),
            "safe geometry at ±10° stays below yield"
        );
        // The declared override still applies regardless of yield: range is ±10°.
        let (_, safe_up) = range_lower_upper(map_get(&safe, "range").expect("range present"));
        assert_angle_close(safe_up, ten_deg, "safe declared-range upper bound");
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
        // Arity 8 (neutral + declared_range) is now VALID (step-6); 9 args
        // overflows the highest supported arity and is rejected.
        {
            let mut a = valid_cantilever_args();
            a.push(Value::angle(0.0)); // neutral
            a.push(Value::angle(0.0)); // declared_range
            a.push(Value::angle(0.0)); // overflow
            undef(a, "9 args");
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

    #[test]
    fn prb_fixed_fixed_beam_attaches_populated_compliance() {
        // Fixed-guided beam (Howell §5 / PRD §6.1): transverse surface bending
        // stress σ = 3·E·t·δ / L² at the displacement endpoint — the inverse of
        // the auto validity δ = yield·L²/(3·E·t) (the surface-yield deflection),
        // so a declared displacement past it drives the cached stress past yield.
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let sigma_at = |delta: f64| 3.0 * e * thickness * delta / length.powi(2);
        let delta_auto = yield_si * length.powi(2) / (3.0 * e * thickness);

        // ── Part 1: auto endpoint (6-arg call, no declared range) ────────────
        let auto = crate::eval_builtin(
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
        let fields = compliance_fields(&auto);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("FlexureCompliance missing `{k}`"))
        };

        // effective_stiffness (Real) == the joint's transverse spring_rate (k_trans).
        let spring_si = match map_get(&auto, "spring_rate") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        };
        match f("effective_stiffness") {
            Value::Real(r) => assert!(
                (r - spring_si).abs() / spring_si < 1e-12,
                "effective_stiffness {r} == spring_rate {spring_si}"
            ),
            other => panic!("effective_stiffness Real, got {other:?}"),
        }

        // max_stress (PRESSURE) == 3·E·t·δ_auto/L² (fixed-guided surface stress
        // at the auto yield-deflection endpoint).
        let expected_auto = sigma_at(delta_auto);
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected_auto).abs() / expected_auto < 1e-9,
                    "auto max_stress {si_value} vs analytic {expected_auto}"
                );
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }

        // prb_validity_range is now Range<Length> = [−δ_auto, +δ_auto] (task 4587:
        // tightened from the Range<Angle> residual left by task 4576).
        let (_, up) = range_lower_upper(map_get(&auto, "range").expect("range present"));
        let range_half = length_scalar_si(up, "auto range upper");
        let prb_half = length_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half - delta_auto).abs() / delta_auto < 1e-9
                && (prb_half - range_half).abs() / range_half < 1e-9,
            "prb_validity_range half {prb_half} == δ_auto {delta_auto} == range half {range_half}"
        );

        // ── Part 2: declared displacement BEYOND yield deflection → at_yield ──
        // δ = 1 mm > δ_auto (≈0.40 mm): σ(1mm) ≈ 769 MPa > 310 MPa yield. Arg
        // layout (length, width, thickness, material, pivot, axis, neutral,
        // declared_range) — declared_range is the LENGTH displacement half-width.
        let big = 0.001_f64;
        let yielding = crate::eval_builtin(
            "prb_fixed_fixed_beam",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                steel(),
                origin(),
                axis_y(),
                Value::length(0.0), // neutral
                Value::length(big), // declared ±1 mm displacement
            ],
        );
        assert!(
            !yielding.is_undef(),
            "declared-displacement call returns a joint, not Undef"
        );
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("prismatic".to_string())),
            "yielding fixed-fixed beam is still a prismatic joint"
        );
        // (a) range overridden to the declared ±1 mm.
        let (ylo, yup) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert!(
            (length_scalar_si(yup, "declared range upper") - big).abs() / big < 1e-9
                && (length_scalar_si(ylo, "declared range lower") + big).abs() / big < 1e-9,
            "declared displacement overrides the joint range to ±{big}"
        );
        let yf = compliance_fields(&yielding);
        let yg = |k: &str| yf.get(&k.to_string()).unwrap_or_else(|| panic!("missing `{k}`"));
        // (b) max_stress at the declared endpoint, exceeding yield.
        let expected_big = sigma_at(big);
        assert!(
            expected_big > yield_si,
            "fixture sanity: σ(1mm)={expected_big} > yield {yield_si}"
        );
        match yg("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected_big).abs() / expected_big < 1e-9,
                    "declared max_stress {si_value} vs analytic {expected_big}"
                );
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }
        // (c) at_yield true, negative margin.
        assert_eq!(
            yg("at_yield"),
            &Value::Bool(true),
            "declared 1mm drives at_yield true"
        );
        match yg("yield_margin") {
            Value::Real(r) => assert!(*r < 0.0, "yielding ⇒ negative margin, got {r}"),
            other => panic!("yield_margin Real, got {other:?}"),
        }
        // prb_validity_range still advertises the auto SAFE δ (not the declared one),
        // now as Range<Length> (task 4587).
        let prb_half_y = length_range_half_si(yg("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half_y - delta_auto).abs() / delta_auto < 1e-9,
            "prb_validity_range stays the auto safe δ {delta_auto}, got half {prb_half_y}"
        );

        // ── Part 3: declared displacement BELOW yield deflection → safe ──────
        // δ = 0.2 mm < δ_auto: σ ≈ 154 MPa < 310 MPa ⇒ at_yield false.
        let small = 0.0002_f64;
        let safe = crate::eval_builtin(
            "prb_fixed_fixed_beam",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                steel(),
                origin(),
                axis_y(),
                Value::length(0.0),
                Value::length(small),
            ],
        );
        assert!(!safe.is_undef(), "safe declared-displacement call returns a joint");
        let sf = compliance_fields(&safe);
        assert_eq!(
            sf.get(&"at_yield".to_string()).expect("at_yield present"),
            &Value::Bool(false),
            "declared 0.2mm stays below yield"
        );
    }
}
