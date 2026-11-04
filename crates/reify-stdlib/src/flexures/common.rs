//! Shared helpers for flexure PRB constructors.
//!
//! Hoisted from beam.rs at δ (task/3854) — the second consumer (notch.rs)
//! triggers the project's hoist-on-≥2-duplicates norm (cf. helpers.rs).

use std::collections::BTreeMap;
use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::arg_acceptance::{
    Acceptance, ArgSpec, FieldAcceptance, FieldRejection, accept_arg, accept_field, angle_spec,
    dimensionless_spec, length_spec, pressure_spec,
};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

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

/// Relative tolerance for the `FlexureCompliance.at_yield` stress check
/// ([`make_compliance_record`]). A flexure operating at its auto
/// yield-deflection endpoint sits exactly at yield by construction; the
/// closed-form θ_yield→σ round-trip accrues ~1e-16 relative FP noise that can
/// nudge `max_stress` a few ulps above yield. `at_yield` therefore flags only
/// stresses past `yield·(1 + this)` — orders of magnitude below any genuine
/// (≥10%) overshoot, orders of magnitude above the round-trip noise floor.
const AT_YIELD_REL_TOL: f64 = 1e-9;

/// Return a both-inclusive symmetric angle range `[−h, +h]` centred on zero.
pub(super) fn symmetric_angle_range(half_width_rad: f64) -> Value {
    Value::range(
        Some(Value::angle(-half_width_rad)),
        Some(Value::angle(half_width_rad)),
        true,
        true,
    )
}

/// Return a both-inclusive symmetric length range `[−h, +h]` centred on zero.
pub(super) fn symmetric_length_range(half_width_m: f64) -> Value {
    Value::range(
        Some(Value::length(-half_width_m)),
        Some(Value::length(half_width_m)),
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

/// Why a PRB constructor refused its arguments: the one verdict both the ctor
/// (⇒ `Value::Undef`) and its classifier ([`super::input_fault`]) read, so the
/// acceptance and the diagnostic come from one expectation (PRD 5 §7 I1).
#[derive(Debug)]
pub(super) enum InputFault {
    /// A present argument or material field of the wrong dimension or kind,
    /// named by its argument path (`length`, `material.youngs_modulus`, …).
    Rejected(FieldRejection),
    /// Positive geometry that admits no pseudo-rigid-body joint — the
    /// `E_FlexureGeometryInvalid` regime ([`slender_beam`], [`notch_ligament`]).
    Degenerate(GeometryViolation),
    /// Any other refusal: arity, non-positive / non-finite geometry, a bad axis
    /// or count, a material that is not a structure or has no positive
    /// `youngs_modulus`.
    Invalid,
}

/// The geometry gate of every slender-beam layout: a positive `thickness ≥
/// length` is [`InputFault::Degenerate`] — whatever the width — and any other
/// non-positive dimension is [`InputFault::Invalid`].
pub(super) fn slender_beam(length: f64, width: f64, thickness: f64) -> Result<(), InputFault> {
    if length > 0.0 && thickness > 0.0 && thickness >= length {
        return Err(InputFault::Degenerate(
            GeometryViolation::ThicknessExceedsLength { thickness, length },
        ));
    }
    if length <= 0.0 || width <= 0.0 || thickness <= 0.0 {
        return Err(InputFault::Invalid);
    }
    Ok(())
}

/// The geometry gate of the notch layout: a positive web `thickness ≥
/// 2·radius` leaves no hinge ligament ([`InputFault::Degenerate`], whatever the
/// width), and any other non-positive dimension is [`InputFault::Invalid`].
pub(super) fn notch_ligament(radius: f64, thickness: f64, width: f64) -> Result<(), InputFault> {
    if radius > 0.0 && thickness > 0.0 && thickness >= 2.0 * radius {
        return Err(InputFault::Degenerate(
            GeometryViolation::WebExceedsNotchDiameter { thickness, radius },
        ));
    }
    if radius <= 0.0 || thickness <= 0.0 || width <= 0.0 {
        return Err(InputFault::Invalid);
    }
    Ok(())
}

/// `accept_arg(v, spec)` as a reader of the positional argument `arg`:
/// `Ok(Some(si))` accepted, `Ok(None)` unresolved, a refusal named `arg`.
fn positional(v: &Value, spec: &ArgSpec, arg: &str) -> Result<Option<f64>, InputFault> {
    match accept_arg(v, spec) {
        Acceptance::Accepted(si) => Ok(Some(si)),
        Acceptance::Undefined => Ok(None),
        Acceptance::Rejected(rejection) => Err(InputFault::Rejected(FieldRejection {
            path: arg.to_string(),
            rejection,
        })),
    }
}

/// A required LENGTH argument in metres. A bare number is refused — it would
/// otherwise read as metres, the 10-vs-10mm hazard — and an unresolved or
/// non-finite one is [`InputFault::Invalid`].
pub(super) fn length_arg(v: &Value, arg: &str) -> Result<f64, InputFault> {
    positional(v, &length_spec(), arg)?
        .filter(|si| si.is_finite())
        .ok_or(InputFault::Invalid)
}

/// An ANGLE argument in radians with [`crate::helpers::trig_input`]'s accept
/// set: a bare number still reads as radians (retiring that is PRD 3's), and
/// any other present value must be an ANGLE.
fn angle_arg(v: &Value, arg: &str) -> Result<Option<f64>, InputFault> {
    match v {
        Value::Real(_) | Value::Int(_) => Ok(crate::helpers::trig_input(v)),
        other => positional(other, &angle_spec(), arg),
    }
}

/// The material field `key` in SI, read against `spec`: absent, `none` or
/// unresolved ⇒ `Ok(None)`; a refusal is named `material.<key>`.
fn material_field(material: &Value, key: &str, spec: &ArgSpec) -> Result<Option<f64>, InputFault> {
    let Value::StructureInstance(data) = material else {
        return Err(InputFault::Invalid);
    };
    match accept_field(data, key, spec) {
        FieldAcceptance::Accepted(si) => Ok(si.is_finite().then_some(si)),
        FieldAcceptance::Absent | FieldAcceptance::Undefined => Ok(None),
        FieldAcceptance::Rejected(rejection) => Err(InputFault::Rejected(FieldRejection::new(
            "material", key, rejection,
        ))),
    }
}

/// The material's Young's modulus E (Pa): a PRESSURE that must be present,
/// finite and positive.
pub(super) fn youngs_modulus(material: &Value) -> Result<f64, InputFault> {
    material_field(material, "youngs_modulus", &pressure_spec())?
        .filter(|e| *e > 0.0)
        .ok_or(InputFault::Invalid)
}

/// The material's yield stress (Pa), or `None` when the material carries none
/// — the documented small-deflection / 5° fallback.
pub(super) fn yield_stress(material: &Value) -> Result<Option<f64>, InputFault> {
    material_field(material, "yield_stress", &pressure_spec())
}

/// The material's Poisson ratio ν: dimensionless (a bare number is correct
/// here), present and in `[0, 0.5)`.
pub(super) fn poisson_ratio(material: &Value) -> Result<f64, InputFault> {
    material_field(material, "poisson_ratio", &dimensionless_spec())?
        .filter(|nu| (0.0..0.5).contains(nu))
        .ok_or(InputFault::Invalid)
}

/// The dimensional kind of a flexure's motion coordinate, selecting how its
/// optional trailing `neutral` and `declared_range` arguments are read.
pub(super) enum RangeKind {
    /// Revolute / rotational joints: radians (an ANGLE `Value::Scalar`, or a
    /// bare `Value::Real` / `Value::Int` read as radians).
    Angle,
    /// Prismatic / displacement joints: metres (a LENGTH `Value::Scalar`; a
    /// bare number is refused).
    Length,
}

/// An optional trailing motion argument `arg` of `kind`: absent, `none`,
/// unresolved or non-finite ⇒ `Ok(None)` (the ctor's documented default); a
/// present value of the wrong dimension or kind is refused rather than read as
/// that default.
fn optional_motion(v: Option<&Value>, kind: &RangeKind, arg: &str) -> Result<Option<f64>, InputFault> {
    let si = match v {
        None | Some(Value::Option(None)) => return Ok(None),
        Some(Value::Option(Some(inner))) => return optional_motion(Some(inner), kind, arg),
        Some(other) => match kind {
            RangeKind::Angle => angle_arg(other, arg)?,
            RangeKind::Length => positional(other, &length_spec(), arg)?,
        },
    };
    Ok(si.filter(|si| si.is_finite()))
}

/// The optional trailing `neutral` offset of `kind` in SI (radians or metres),
/// defaulting to `0.0`.
pub(super) fn neutral(v: Option<&Value>, kind: &RangeKind) -> Result<f64, InputFault> {
    Ok(optional_motion(v, kind, "neutral")?.unwrap_or(0.0))
}

/// The optional trailing declared operating range of `kind` as its SI
/// half-width magnitude (always non-negative — the symmetric `±h` interval the
/// §5.3 stress-check evaluates at its endpoint). `None` means "no user-declared
/// range, fall back to the auto-computed safe cap".
pub(super) fn declared_range(v: Option<&Value>, kind: &RangeKind) -> Result<Option<f64>, InputFault> {
    Ok(optional_motion(v, kind, "declared_range")?.map(f64::abs))
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

/// Build the cached `FlexureCompliance` record as a `Value::StructureInstance`.
///
/// Mirrors the SIR-α `StructureInstanceData` construction (beam.rs test
/// `material()` helper / reify-ir value.rs): a placeholder `type_id`
/// (`StructureTypeId(0)` — the record is built Rust-side, bypassing the
/// `flexures.ri` ctor and its registered type id), `type_name =
/// "FlexureCompliance"`, `version = 1`, and the 7-field map matching the
/// `flexures.ri` `structure def FlexureCompliance`.
///
/// Field representations:
/// - `effective_stiffness` → bare [`Value::Real`] (family-agnostic: revolute
///   flexures carry rotational stiffness, prismatic carry translational; storing
///   the bare SI magnitude sidesteps committing the cache to one dimension).
/// - `max_stress` / `max_stress_at_neutral` → PRESSURE-dimensioned [`Value::Scalar`].
/// - `yield_margin` → [`Value::Real`]: `(yield − max_stress) / yield` when a yield
///   stress is known (negative in the at-yield regime; ≤ 1 by construction so the
///   `flexures.ri` `yield_margin <= 1` constraint holds), or the sentinel `1.0`
///   (maximally safe — no yield datum places no stress limit) when `yield_si` is
///   `None`.
/// - `parasitic_error` → [`Value::Option`] of a LENGTH Scalar (`None` ⇒ `Option(None)`).
/// - `prb_validity_range` → the caller-provided `Value` stored verbatim —
///   `Range<Angle>` (both-inclusive `[−h, +h]`) for revolute/cantilever families
///   (via [`symmetric_angle_range`]), `Range<Length>` for prismatic/displacement
///   families (via [`symmetric_length_range`]). Each caller wraps the
///   dimensionally-correct half-width before calling this function.
/// - `at_yield` → [`Value::Bool`]: `max_stress > yield·(1 + AT_YIELD_REL_TOL)`
///   (strict, with a tiny relative tolerance so operating exactly at the
///   yield-deflection endpoint — the SAFE-envelope boundary — is not flagged by
///   FP round-trip noise). Always `false` when no yield stress is known.
pub(super) fn make_compliance_record(
    effective_stiffness: f64,
    max_stress_si: f64,
    max_stress_at_neutral_si: f64,
    yield_si: Option<f64>,
    parasitic: Option<f64>,
    prb_validity_range: Value,
) -> Value {
    let pressure = |si: f64| Value::Scalar {
        si_value: si,
        dimension: DimensionVector::PRESSURE,
    };
    let (yield_margin, at_yield) = match yield_si {
        // `at_yield` is STRICT and carries a small relative tolerance: a flexure
        // operating AT its yield-deflection endpoint is at the SAFE-envelope
        // boundary (max_stress == yield by construction), not over it. The
        // yield-capped families (notch) and the fixed-guided compound stages sit
        // exactly there at their auto endpoint, and the closed-form round-trip
        // (θ_yield → σ(θ_yield)) accrues ~1e-16 relative FP noise that can nudge
        // max_stress a few ulps ABOVE yield. The `(1 + AT_YIELD_REL_TOL)` band
        // absorbs that noise so safe defaults are never flagged, while a genuine
        // overshoot (a declared range past the safe bound — ≥10% over in every
        // fixture) clears it by orders of magnitude. yield_margin stays the exact
        // signed `(yield − max_stress)/yield` (≈0 at the boundary).
        Some(y) => (
            (y - max_stress_si) / y,
            max_stress_si > y * (1.0 + AT_YIELD_REL_TOL),
        ),
        // No yield datum: maximally-safe sentinel margin, never "at yield".
        None => (1.0, false),
    };
    let parasitic_error = match parasitic {
        Some(p) => Value::Option(Some(Box::new(Value::length(p)))),
        None => Value::Option(None),
    };
    let fields: PersistentMap<String, Value> = [
        (
            "effective_stiffness".to_string(),
            Value::Real(effective_stiffness),
        ),
        ("max_stress".to_string(), pressure(max_stress_si)),
        (
            "max_stress_at_neutral".to_string(),
            pressure(max_stress_at_neutral_si),
        ),
        ("yield_margin".to_string(), Value::Real(yield_margin)),
        ("parasitic_error".to_string(), parasitic_error),
        ("prb_validity_range".to_string(), prb_validity_range),
        ("at_yield".to_string(), Value::Bool(at_yield)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "FlexureCompliance".to_string(),
        version: 1,
        fields,
    }))
}

/// Cantilever surface bending stress σ = E·(t/2)·|θ|/L (Howell §5.1) — the
/// algebraic inverse of [`cantilever_theta_lim`]'s `θ_yield = yield·L/(E·t/2)`.
///
/// `theta` is the rotation (radians) at which to evaluate the stress; the
/// magnitude is used so the sign of the deflection does not matter. Shared by
/// the cantilever/blade families (`beam::prb_cantilever_beam`, the hinge ctors,
/// and `compound::prb_cartwheel_flexure`) that wire the compliance record.
pub(super) fn cantilever_sigma_at(theta: f64, length: f64, thickness: f64, e: f64) -> f64 {
    e * (thickness / 2.0) * theta.abs() / length
}

/// Fixed-guided transverse surface bending stress σ = 3·E·t·|δ| / L² (Howell §5
/// / PRD §6.1) — the algebraic inverse of [`fixed_guided_delta_max`]'s
/// `δ_yield = yield·L²/(3·E·t)`.
///
/// `delta` is the transverse displacement (metres) at which to evaluate the
/// stress; the magnitude is used so the sign of the deflection does not matter.
/// Shared by the fixed-guided family (`beam::prb_fixed_fixed_beam`) and the
/// compound parallelogram stages (`compound::*`, wired at step-20), all of which
/// share the fixed-guided boundary condition. Note the single-cantilever
/// prismatic blade (`prismatic::prb_prismatic_blade`) uses HALF this coefficient
/// (1.5) — its one free end is not guided — so it carries its own local helper.
pub(super) fn fixed_guided_sigma_at(delta: f64, length: f64, thickness: f64, e: f64) -> f64 {
    3.0 * e * thickness * delta.abs() / length.powi(2)
}

/// Insert the cached `FlexureCompliance` record under the reserved hidden joint
/// key `__flexure_compliance` and return the augmented joint.
///
/// The β-established `__flexure_compliance` reserved-name convention: the
/// mechanism / sweep / snapshot engines dispatch on the `kind` string and
/// ignore unknown keys (PRD §8.2), so the cache rides along invisibly. A
/// non-`Map` input (e.g. `Value::Undef` from a rejected ctor) passes through
/// unchanged — there is no joint to annotate.
pub(super) fn attach_compliance(joint: Value, record: Value) -> Value {
    match joint {
        Value::Map(mut m) => {
            m.insert(
                Value::String("__flexure_compliance".to_string()),
                record,
            );
            Value::Map(m)
        }
        other => other,
    }
}

/// A degenerate-geometry violation of a PRB constructor's arguments — the
/// input class the `E_FlexureGeometryInvalid` diagnostic (PRD §1) is scoped to,
/// carried by [`InputFault::Degenerate`].
#[derive(Debug)]
pub(super) enum GeometryViolation {
    /// Slender-beam families: bending thickness `t` ≥ beam length `L`.
    ThicknessExceedsLength { thickness: f64, length: f64 },
    /// Notch-hinge family: web thickness `t` ≥ notch diameter `2r`.
    WebExceedsNotchDiameter { thickness: f64, radius: f64 },
}

impl GeometryViolation {
    /// A human-readable description of the degeneracy (always mentions
    /// "geometry"), suffixed onto the `E_FLEXURE_GEOMETRY_INVALID` message.
    pub(super) fn describe(&self) -> String {
        match self {
            GeometryViolation::ThicknessExceedsLength { thickness, length } => format!(
                "degenerate flexure geometry: bending thickness {:.4} mm ≥ beam length \
                 {:.4} mm — the pseudo-rigid-body model requires a slender beam \
                 (thickness < length)",
                thickness * 1e3,
                length * 1e3,
            ),
            GeometryViolation::WebExceedsNotchDiameter { thickness, radius } => format!(
                "degenerate flexure geometry: web thickness {:.4} mm ≥ notch diameter \
                 {:.4} mm — the notch-hinge model requires web thickness < 2·radius",
                thickness * 1e3,
                2.0 * radius * 1e3,
            ),
        }
    }
}

#[cfg(test)]
mod read_family_tests {
    use super::super::test_util::{material, steel, steel_no_yield};
    use super::*;

    fn pressure(si: f64) -> Value {
        Value::Scalar {
            si_value: si,
            dimension: DimensionVector::PRESSURE,
        }
    }

    /// The argument path a refusal names; panics on any other outcome.
    fn rejected_path<T: std::fmt::Debug>(read: Result<T, InputFault>) -> String {
        match read {
            Err(InputFault::Rejected(rejection)) => rejection.path,
            other => panic!("expected a Rejected fault, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_length_is_refused_by_name_and_a_dimensioned_one_read_in_metres() {
        assert_eq!(rejected_path(length_arg(&Value::Real(0.02), "thickness")), "thickness");
        assert_eq!(length_arg(&Value::length(0.02), "thickness").unwrap(), 0.02);
        assert!(matches!(length_arg(&Value::Undef, "thickness"), Err(InputFault::Invalid)));
    }

    #[test]
    fn a_nested_optional_motion_argument_unwraps_to_its_value() {
        let nested = Value::Option(Some(Box::new(Value::Option(Some(Box::new(
            Value::angle(0.1),
        ))))));
        assert_eq!(neutral(Some(&nested), &RangeKind::Angle).unwrap(), 0.1);
        assert_eq!(neutral(None, &RangeKind::Angle).unwrap(), 0.0);
        assert_eq!(neutral(Some(&Value::Option(None)), &RangeKind::Length).unwrap(), 0.0);
    }

    #[test]
    fn a_bare_neutral_is_radians_for_an_angle_but_refused_for_a_length() {
        assert_eq!(neutral(Some(&Value::Real(0.1)), &RangeKind::Angle).unwrap(), 0.1);
        assert_eq!(rejected_path(neutral(Some(&Value::Real(0.001)), &RangeKind::Length)), "neutral");
        assert_eq!(
            rejected_path(declared_range(Some(&Value::length(0.001)), &RangeKind::Angle)),
            "declared_range"
        );
    }

    #[test]
    fn an_absent_or_none_yield_stress_is_the_documented_fallback() {
        assert_eq!(yield_stress(&steel_no_yield()).unwrap(), None);
        let none_yield = material(
            "NoneYield",
            &[("youngs_modulus", pressure(205e9)), ("yield_stress", Value::Option(None))],
        );
        assert_eq!(yield_stress(&none_yield).unwrap(), None);
        assert_eq!(yield_stress(&steel()).unwrap(), Some(310e6));
    }

    #[test]
    fn a_material_field_of_the_wrong_dimension_is_refused_by_its_path() {
        let length_modulus = material("FakeMat", &[("youngs_modulus", Value::length(0.2))]);
        assert_eq!(rejected_path(youngs_modulus(&length_modulus)), "material.youngs_modulus");
        let pressure_ratio = material(
            "FakeMat",
            &[("youngs_modulus", pressure(205e9)), ("poisson_ratio", pressure(0.29))],
        );
        assert_eq!(rejected_path(poisson_ratio(&pressure_ratio)), "material.poisson_ratio");
        assert_eq!(poisson_ratio(&steel()).unwrap(), 0.29);
        assert!(matches!(youngs_modulus(&Value::Real(1.0)), Err(InputFault::Invalid)));
    }

    #[test]
    fn degenerate_geometry_is_named_whatever_the_width_and_non_positive_is_invalid() {
        assert!(matches!(
            slender_beam(0.002, -1.0, 0.003),
            Err(InputFault::Degenerate(GeometryViolation::ThicknessExceedsLength { .. }))
        ));
        assert!(matches!(slender_beam(0.02, 0.0, 0.0005), Err(InputFault::Invalid)));
        assert!(slender_beam(0.02, 0.005, 0.0005).is_ok());
        assert!(matches!(
            notch_ligament(0.001, 0.002, -1.0),
            Err(InputFault::Degenerate(GeometryViolation::WebExceedsNotchDiameter { .. }))
        ));
        assert!(matches!(notch_ligament(0.0, 0.002, 0.005), Err(InputFault::Invalid)));
        assert!(notch_ligament(0.002, 0.001, 0.005).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::angle_range_half_si;
    use super::*;

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
        match accept_arg(v, &pressure_spec()) {
            Acceptance::Accepted(si) => si,
            other => panic!("{label}: expected PRESSURE Scalar, got {v:?} ({other:?})"),
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
    fn make_compliance_record_stores_caller_provided_range() {
        use super::super::test_util::{angle_range_half_si, length_range_half_si};
        // LENGTH range: symmetric_length_range(δ) stored verbatim → Range<Length>.
        let delta = 0.003_f64;
        let rec_len = make_compliance_record(
            1.0,
            50e6,
            0.0,
            None,
            None,
            symmetric_length_range(delta),
        );
        let half_len = length_range_half_si(field(&rec_len, "prb_validity_range"), "prb_validity_range/length");
        assert!(
            (half_len - delta).abs() < 1e-12,
            "length half-width {half_len} vs expected {delta}"
        );
        // ANGLE range: symmetric_angle_range(θ) stored verbatim → Range<Angle>.
        let theta = 0.0872664626_f64;
        let rec_ang = make_compliance_record(
            1.0,
            50e6,
            0.0,
            None,
            None,
            symmetric_angle_range(theta),
        );
        let half_ang = angle_range_half_si(field(&rec_ang, "prb_validity_range"), "prb_validity_range/angle");
        assert!(
            (half_ang - theta).abs() < 1e-9,
            "angle half-width {half_ang} vs expected {theta}"
        );
    }

    #[test]
    fn make_compliance_record_is_flexure_compliance_with_seven_fields() {
        let rec = make_compliance_record(1.42, 100e6, 0.0, Some(310e6), None, symmetric_angle_range(0.0872664626));
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
        let rec = make_compliance_record(1.42, max_stress, 0.0, Some(yield_si), None, symmetric_angle_range(0.0872664626));

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

        // prb_validity_range is a symmetric ANGLE Range<Angle> = [−h, +h].
        let half = angle_range_half_si(field(&rec, "prb_validity_range"), "prb_validity_range");
        assert!(
            (half - 0.0872664626).abs() < 1e-9,
            "prb_validity_range half-width {half} vs expected 0.0872664626"
        );
    }

    #[test]
    fn make_compliance_record_yielding_input_negative_margin_at_yield() {
        // max_stress (447 MPa) > yield (310 MPa) ⇒ at_yield true, negative margin.
        let yield_si = 310e6_f64;
        let max_stress = 447e6_f64;
        let rec =
            make_compliance_record(0.01, max_stress, 50e6, Some(yield_si), Some(1e-6), symmetric_angle_range(0.17453293));

        assert_eq!(field(&rec, "at_yield"), &Value::Bool(true), "at yield");

        let m = real_of(field(&rec, "yield_margin"), "yield_margin");
        let expected = (yield_si - max_stress) / yield_si;
        assert!((m - expected).abs() < 1e-9, "margin {m} vs {expected}");
        assert!(m < 0.0, "yielding input ⇒ negative margin, got {m}");

        // parasitic Some(1µm) ⇒ Option(Some(Length)).
        match field(&rec, "parasitic_error") {
            Value::Option(Some(inner)) => match accept_arg(inner, &length_spec()) {
                Acceptance::Accepted(si) => {
                    assert!((si - 1e-6).abs() < 1e-15, "parasitic si {si}");
                }
                other => panic!("parasitic inner: expected Length Scalar, got {inner:?} ({other:?})"),
            },
            other => panic!("expected Option(Some(Length)), got {other:?}"),
        }

        // prb_validity_range is a symmetric ANGLE Range<Angle> = [−h, +h].
        let half = angle_range_half_si(field(&rec, "prb_validity_range"), "prb_validity_range");
        assert!(
            (half - 0.17453293).abs() < 1e-9,
            "prb_validity_range half-width {half} vs expected 0.17453293"
        );
    }

    #[test]
    fn make_compliance_record_no_yield_input_uses_safe_sentinel() {
        // None yield ⇒ at_yield false, margin sentinel = 1.0 (maximally safe:
        // no yield datum places no stress limit, clamped to the margin upper
        // bound). Pairs naturally with at_yield=false (0.0 would falsely read as
        // "exactly at the yield boundary").
        let rec = make_compliance_record(1.0, 100e6, 0.0, None, None, symmetric_angle_range(0.0872664626));
        assert_eq!(
            field(&rec, "at_yield"),
            &Value::Bool(false),
            "no-yield material ⇒ not at yield"
        );
        let m = real_of(field(&rec, "yield_margin"), "yield_margin");
        assert_eq!(m, 1.0, "no-yield margin sentinel is 1.0 (maximally safe)");

        // prb_validity_range is a symmetric ANGLE Range<Angle> = [−h, +h].
        let half = angle_range_half_si(field(&rec, "prb_validity_range"), "prb_validity_range");
        assert!(
            (half - 0.0872664626).abs() < 1e-9,
            "prb_validity_range half-width {half} vs expected 0.0872664626"
        );
    }
}
