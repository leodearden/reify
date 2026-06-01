//! Shared helpers for flexure PRB constructors.
//!
//! Hoisted from beam.rs at δ (task/3854) — the second consumer (notch.rs)
//! triggers the project's hoist-on-≥2-duplicates norm (cf. helpers.rs).

use std::collections::BTreeMap;
use std::f64::consts::PI;

use reify_core::DimensionVector;
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

/// The dimensional kind of a declared operating-range argument, selecting how
/// [`parse_declared_range`] extracts the SI half-width magnitude.
pub(super) enum RangeKind {
    /// Revolute / rotational joints: a half-angle in radians (an ANGLE
    /// `Value::Scalar`, or a bare `Value::Real` / `Value::Int` read as radians).
    Angle,
    /// Prismatic / displacement joints: a half-displacement in metres (a LENGTH
    /// `Value::Scalar`, or a bare `Value::Real` / `Value::Int` read as metres).
    ///
    /// Used by the displacement-family ctors `beam::prb_fixed_fixed_beam` and
    /// `prismatic::prb_prismatic_blade` to parse their optional trailing declared
    /// operating range.
    Length,
}

/// Parse an optional trailing declared operating-range argument into its SI
/// half-width magnitude (always non-negative — the operating range is the
/// symmetric `±h` interval the §5.3 stress-check evaluates at its endpoint).
///
/// `arg` is the raw trailing slot, or `None` when the ctor was called below the
/// declared-range arity. Accepts an ANGLE-/LENGTH-dimensioned `Value::Scalar`
/// (per `kind`), a bare `Value::Real` / `Value::Int`, or a `Value::Option`
/// wrapping any of those. A missing arg, `Value::Option(None)`, or an
/// unreadable / non-finite value all yield `None` — meaning "no user-declared
/// range, fall back to the auto-computed safe cap".
pub(super) fn parse_declared_range(arg: Option<&Value>, kind: RangeKind) -> Option<f64> {
    parse_declared_range_value(arg?, &kind)
}

fn parse_declared_range_value(v: &Value, kind: &RangeKind) -> Option<f64> {
    match v {
        Value::Option(Some(inner)) => parse_declared_range_value(inner, kind),
        Value::Option(None) => None,
        other => {
            let si = match kind {
                RangeKind::Angle => crate::helpers::trig_input(other),
                RangeKind::Length => length_si(other),
            }?;
            si.is_finite().then_some(si.abs())
        }
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
/// - `prb_validity_range` → [`Value::Real`]: the SI half-angle (revolute) or
///   half-displacement (prismatic) of the auto-computed SAFE range (the bare
///   `Real` placeholder matches the `flexures.ri` `TODO(range-angle-type)`).
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
    prb_validity_half_si: f64,
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
        (
            "prb_validity_range".to_string(),
            Value::Real(prb_validity_half_si),
        ),
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

/// A degenerate-geometry violation re-derived from a rejected PRB constructor's
/// arguments — the input class the `E_FlexureGeometryInvalid` diagnostic
/// (PRD §1) is scoped to.
///
/// The PRB ctors return `Value::Undef` for BOTH degenerate geometry AND
/// non-geometry input errors (bad material / axis / arity). To emit
/// `E_FlexureGeometryInvalid` for *only* the geometry class — mirroring how
/// `stackup::diagnose` re-classifies on the `Undef` path — `flexure_diagnose`
/// re-derives the geometry from the raw args via [`classify_geometry_invalid`].
pub(super) enum GeometryViolation {
    /// Slender-beam families `(length, width, thickness, …)`: bending thickness
    /// `t` ≥ beam length `L`.
    ThicknessExceedsLength { thickness: f64, length: f64 },
    /// Notch-hinge family `(notch_radius, web_thickness, …)`: web thickness `t`
    /// ≥ notch diameter `2r`.
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

/// Re-classify a degenerate-geometry violation from a rejected PRB constructor's
/// positional arguments, or `None` when the geometry is valid (so a
/// non-geometry Undef — bad material / axis / arity — stays silent) or the
/// relevant geometry slots cannot be read.
///
/// Dispatches by ctor name into the three positional layouts:
/// - slender-beam families `(length, width, thickness, …)` → `t ≥ L`;
/// - `prb_cartwheel_flexure` `(blade_count, length, width, thickness, …)` →
///   `t ≥ L` (length at index 1, thickness at index 3);
/// - notch family `(notch_radius, web_thickness, width, …)` → `t ≥ 2r`.
pub(super) fn classify_geometry_invalid(name: &str, args: &[Value]) -> Option<GeometryViolation> {
    match name {
        "prb_cantilever_beam"
        | "prb_fixed_fixed_beam"
        | "prb_living_hinge"
        | "prb_cross_spring_pivot"
        | "prb_let_joint"
        | "prb_prismatic_blade"
        | "prb_two_axis_pivot"
        | "prb_parallelogram_flexure"
        | "prb_double_parallelogram_flexure" => {
            let length = length_si(args.first()?)?;
            let thickness = length_si(args.get(2)?)?;
            thickness_vs_length(thickness, length)
        }
        "prb_cartwheel_flexure" => {
            let length = length_si(args.get(1)?)?;
            let thickness = length_si(args.get(3)?)?;
            thickness_vs_length(thickness, length)
        }
        "prb_notch_circular" | "prb_notch_elliptical" | "prb_notch_right_circular" => {
            let radius = length_si(args.first()?)?;
            let thickness = length_si(args.get(1)?)?;
            (radius > 0.0 && thickness > 0.0 && thickness >= 2.0 * radius)
                .then_some(GeometryViolation::WebExceedsNotchDiameter { thickness, radius })
        }
        _ => None,
    }
}

/// Shared degeneracy test for the slender-beam families: a positive, finite
/// `t ≥ L` is the `thickness ≥ length` violation those ctors reject on.
fn thickness_vs_length(thickness: f64, length: f64) -> Option<GeometryViolation> {
    (length > 0.0 && thickness > 0.0 && thickness >= length)
        .then_some(GeometryViolation::ThicknessExceedsLength { thickness, length })
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
