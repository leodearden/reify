//! Prismatic-blade and two-axis-pivot PRB constructors (Howell §6.2; Henein 2010).
//!
//! Two constructors, sharing the positional argument layout
//! `(length, width, thickness, material, pivot, axis[, neutral])` for parser
//! symmetry:
//!
//!  - `prb_prismatic_blade` → `kind = "prismatic"` (Howell §6.2 single-
//!    cantilever-blade). Transverse stiffness `k_trans = 3·E·I/L³` (γ = 3,
//!    intentionally distinct from `beam::prb_fixed_fixed_beam`'s γ = 12).
//!
//!  - `prb_two_axis_pivot` → `kind = "spherical"` (Henein 2010 two-axis pivot).
//!    Per-axis rotational stiffness `k_axis = E·I/L` (γ = 1, slender-blade).
//!    `spring_rate = None` (PRD §8.6/§13.1 multi-DOF invariant); stiffness is
//!    surfaced only via `effective_stiffness`. Axis is validated for signature
//!    symmetry but NOT stored — the spherical joint is axis-isotropic.
//!
//! Dispatch via [`eval_prismatic`], mirroring the sibling modules.

use reify_core::DimensionVector;
use reify_ir::Value;

use std::collections::BTreeMap;

use super::common::{
    attach_compliance, cantilever_sigma_at, cantilever_theta_lim, length_si, make_compliance_record,
    make_flexure_joint, material_field_si, parse_declared_range, symmetric_angle_range,
    symmetric_length_range, RangeKind,
};

/// Single-cantilever-blade PRB transverse stiffness coefficient (Howell §6.2).
///
/// `k_trans = γ_pb · E · I / L³` with `γ_pb = 3.0`. Intentionally distinct
/// from `beam::FIXED_FIXED_GAMMA = 12.0` (fixed-guided boundary condition, where
/// both ends are oriented). The cantilever has one free end that both translates
/// and rotates, yielding the smaller coefficient.
const PRISMATIC_BLADE_GAMMA: f64 = 3.0;

/// Fallback cantilever transverse-displacement validity limit as a fraction of
/// beam length. Used when the material carries no `yield_stress`. The PRB
/// small-deflection model degrades past ~0.1·L for a cantilever.
const SMALL_DEFLECTION_FRACTION: f64 = 0.1;

/// Evaluate a prismatic-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so `eval_builtin` falls through to the next module.
pub(crate) fn eval_prismatic(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_prismatic_blade" => Some(prb_prismatic_blade(args)),
        "prb_two_axis_pivot" => Some(prb_two_axis_pivot(args)),
        _ => None,
    }
}

/// Shared validated inputs for prismatic-flexure constructors.
struct PrismaticInputs<'a> {
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
    /// The raw axis argument, stored verbatim on 1-DOF joint Maps.
    axis: &'a Value,
    /// The raw pivot argument.
    pivot: &'a Value,
    /// Optional trailing `neutral` argument (present in the 7- and 8-arg forms).
    neutral_arg: Option<&'a Value>,
    /// The optional trailing declared operating-range argument (present only in
    /// the 8-arg form). When present, its endpoint — not the auto cap — drives
    /// the joint range and the §5.3 `max_stress` stress-check: a LENGTH
    /// half-displacement for `prb_prismatic_blade`, an ANGLE half-angle for
    /// `prb_two_axis_pivot` (each ctor selects the `RangeKind`).
    declared_range_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout:
/// `(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])`.
///
/// Returns `None` (⇒ `Value::Undef`) on: arity ∉ {6, 7, 8}; non-positive or
/// non-finite geometry; thickness ≥ length; missing or non-positive
/// `youngs_modulus`; or an invalid axis.
fn parse_prismatic_inputs(args: &[Value]) -> Option<PrismaticInputs<'_>> {
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
    Some(PrismaticInputs {
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

/// Single-cantilever-blade transverse surface bending stress σ = 1.5·E·t·|δ|/L²
/// (Howell §6.2) — the algebraic inverse of the blade's
/// `δ_yield = 2·yield·L²/(3·E·t)`.
///
/// Intentionally HALF of `common::fixed_guided_sigma_at` (3·E·t·δ/L²): the blade
/// is a cantilever with one free end, not a guided pair, so its peak surface
/// stress at a given tip displacement is half the fixed-guided beam's. Using the
/// fixed-guided coefficient here would report `at_yield = true` at the blade's
/// own documented safe validity δ.
fn cantilever_transverse_sigma_at(delta: f64, length: f64, thickness: f64, e: f64) -> f64 {
    1.5 * e * thickness * delta.abs() / length.powi(2)
}

/// Extract a neutral transverse offset in metres from a trailing constructor
/// argument (the prismatic counterpart of `common::neutral_angle_si`).
///
/// Accepts a LENGTH-dimensioned `Value::Scalar` (e.g. `Value::length`), a bare
/// `Value::Real` / `Value::Int` interpreted as metres, or a `Value::Option`
/// wrapping any of those. `Option(None)` and any value that fails extraction
/// default to `0.0`.
fn neutral_length_si(v: &Value) -> f64 {
    match v {
        Value::Option(Some(inner)) => neutral_length_si(inner),
        Value::Option(None) => 0.0,
        other => length_si(other).unwrap_or(0.0),
    }
}

/// `prb_prismatic_blade(length, width, thickness, material, pivot, axis[, neutral])`
/// — Howell §6.2 single-cantilever-blade flexure presented as a prismatic joint.
///
/// Returns a joint `Value::Map` (`kind == "prismatic"`) whose transverse
/// stiffness is `k_trans = 3·E·I/L³` (γ = 3), with `I = width·thickness³/12`.
///
/// Transverse validity range `±δ`:
/// - With `yield_stress`: cantilever surface stress `σ = 1.5·E·t·δ/L²` ⇒
///   `δ_yield = 2·yield·L²/(3·E·t)`.
/// - No `yield_stress`: fallback `±0.1·L` small-deflection limit.
///
/// The range shape (finite, LENGTH-dimensioned, symmetric, non-zero) is tested;
/// its magnitude is a design choice, not an externally-validated bound.
///
/// Returns `Value::Undef` on the invalid-input classes in [`parse_prismatic_inputs`].
fn prb_prismatic_blade(args: &[Value]) -> Value {
    let Some(b) = parse_prismatic_inputs(args) else {
        return Value::Undef;
    };

    // Cantilever transverse stiffness k_trans = γ_pb · E · I / L³ (γ = 3, Howell §6.2).
    let k_trans = PRISMATIC_BLADE_GAMMA * b.e * b.i / b.length.powi(3);

    // Auto cantilever transverse-displacement validity δ_auto.
    // Surface stress σ = 1.5·E·t·δ / L²  ⇒  δ_yield = 2·yield·L² / (3·E·t).
    // No yield_stress ⇒ small-deflection fallback 0.1·L. Retained as the SAFE
    // prb_validity_range in the compliance record below.
    let delta_auto = match b.yield_si {
        Some(yield_si) => 2.0 * yield_si * b.length.powi(2) / (3.0 * b.e * b.thickness),
        None => SMALL_DEFLECTION_FRACTION * b.length,
    };

    // An optional user-declared operating range (±half-displacement LENGTH)
    // OVERRIDES δ_auto for the joint range and the §5.3 stress endpoint.
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

    // Cache the FlexureCompliance record (§5.3): cantilever-transverse surface
    // stress at the range endpoint and the neutral rest offset; prb_validity_range
    // advertises the auto SAFE δ_auto regardless of any wider declared range.
    let max_stress = cantilever_transverse_sigma_at(range_endpoint, b.length, b.thickness, b.e);
    let max_stress_at_neutral = cantilever_transverse_sigma_at(neutral_si, b.length, b.thickness, b.e);
    let record = make_compliance_record(
        k_trans,
        max_stress,
        max_stress_at_neutral,
        b.yield_si,
        None,
        symmetric_length_range(delta_auto),
    );
    attach_compliance(joint, record)
}

/// `prb_two_axis_pivot(length, width, thickness, material, pivot, axis[, neutral])`
/// — Henein 2010 two-axis pivot presented as a spherical joint.
///
/// Returns a spherical-joint `Value::Map` (`kind == "spherical"`) with the
/// isotropic per-axis rotational stiffness `k_axis = E·I/L` (γ = 1, slender
/// blade; Henein 2010 §4). A topology-specific Henein coefficient is a future
/// refinement — the symmetric slender-blade γ = 1 convention is shared with
/// `prb_living_hinge` (hinge.rs).
///
/// Per PRD §8.6 / §13.1 the multi-DOF scalar spring tensor is deferred:
/// `spring_rate = Value::Option(None)`. Per-axis stiffness is surfaced only via
/// `effective_stiffness` (FlexureCompliance, populated in task λ).
///
/// **2-DOF vs 3-DOF representation.** A two-axis pivot is physically a 2-DOF
/// (universal-style) joint. It is mapped to `kind = "spherical"` (conventionally
/// 3-DOF) because the PRD §8.6 / §13.1 joint taxonomy recognises only
/// `"prismatic"`, `"revolute"`, and `"spherical"` as scalar-stiffness kinds in
/// γ scope; no `"universal"` kind exists in the taxonomy and no downstream
/// dispatcher branches on it. This is an intentional design decision, not an
/// oversight.
///
/// The `axis` argument is validated (finite, non-zero, dimensionless 3-vector)
/// for parser symmetry with the 1-DOF constructors but is NOT stored in the map
/// — the spherical joint is axis-isotropic and the canonical spherical Map has
/// no `axis` key.
///
/// Returns `Value::Undef` on the same invalid-input classes as
/// [`prb_prismatic_blade`] (see [`parse_prismatic_inputs`]).
fn prb_two_axis_pivot(args: &[Value]) -> Value {
    let Some(b) = parse_prismatic_inputs(args) else {
        return Value::Undef;
    };

    // Symmetric per-axis rotational stiffness k_axis = E·I/L (γ = 1, Henein 2010).
    let k_axis = b.e * b.i / b.length;

    // Auto angular validity θ_lim = min(θ_yield, 5°) (the surface-yield rotation
    // capped at the PRB small-deflection bound; 5° only without a yield_stress) —
    // reuse common::cantilever_theta_lim, the same model the cantilever beam uses.
    let theta_lim = cantilever_theta_lim(b.length, b.thickness, b.e, b.yield_si);

    // An optional user-declared angular operating range (±half-angle) OVERRIDES
    // θ_lim for the joint range and the §5.3 stress endpoint; θ_lim is retained
    // as the SAFE/suggested range in the compliance record.
    let declared = parse_declared_range(b.declared_range_arg, RangeKind::Angle);
    let range_endpoint = declared.unwrap_or(theta_lim);
    let range_angle = symmetric_angle_range(range_endpoint);

    // Build the spherical joint Map directly (NOT make_flexure_joint, which
    // emits `axis`/`range`/`neutral` keys appropriate for 1-DOF joints only).
    // The canonical spherical Map uses `range_angle` (axis-isotropic), has no
    // `axis` or `neutral` key, and carries `spring_rate = None` per §8.6.
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("spherical".to_string()),
    );
    m.insert(Value::String("range_angle".to_string()), range_angle);
    m.insert(
        Value::String("effective_stiffness".to_string()),
        Value::Scalar {
            si_value: k_axis,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
    );
    m.insert(
        Value::String("spring_rate".to_string()),
        Value::Option(None),
    );
    m.insert(
        Value::String("damping".to_string()),
        Value::Option(None),
    );
    m.insert(Value::String("pivot".to_string()), b.pivot.clone());
    let joint = Value::Map(m);

    // Cache the FlexureCompliance record (§5.3): cantilever angular surface stress
    // σ = E·(t/2)·θ/L at the (declared|auto) endpoint. The spherical pivot has no
    // neutral offset, so max_stress_at_neutral is the rest stress (0). The record
    // rides on the spherical Map via the reserved `__flexure_compliance` key.
    let max_stress = cantilever_sigma_at(range_endpoint, b.length, b.thickness, b.e);
    let record = make_compliance_record(
        k_axis,
        max_stress,
        0.0,
        b.yield_si,
        None,
        symmetric_angle_range(theta_lim),
    );
    attach_compliance(joint, record)
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::Value;
    use super::super::test_util::*;

    /// Standard 6-arg argument list for both prismatic-flexure constructors:
    /// L = 20 mm, w = 5 mm, t = 0.5 mm, steel (E = 205 GPa, yield = 310 MPa).
    fn prismatic_args() -> Vec<Value> {
        vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ]
    }

    // ── step-1/2: prb_prismatic_blade smoke + scaling ────────────────────────

    /// (a) kind, damping, spring_rate dimension + closed-form value.
    #[test]
    fn prb_prismatic_blade_structure_and_spring_rate() {
        let result = crate::eval_builtin("prb_prismatic_blade", &prismatic_args());

        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("prismatic".to_string())),
            "prismatic blade presents as a prismatic joint"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is Option(None)"
        );

        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let i = width * thickness.powi(3) / 12.0;
        // γ = 3: single cantilever blade (Howell §6.2).
        let k_expected = 3.0 * e * i / length.powi(3);

        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::TRANSLATIONAL_STIFFNESS,
                    "spring_rate carries TRANSLATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(rel < 1e-9, "spring_rate {si_value} vs {k_expected} (rel {rel})");
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b) γ-independent functional scaling ratios for prb_prismatic_blade.
    ///
    /// k_trans = γ · E · I / L³  where  I = w·t³/12.
    /// Four ratios checked: t³ (×8), 1/L³ (×1/8), E (×2), width (×2).
    /// γ cancels in all ratios so this is coefficient-independent of γ = 3.
    #[test]
    fn prb_prismatic_blade_scaling_ratios() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;

        let k = |l: f64, w: f64, t: f64, e_val: f64| {
            spring_rate_si(&crate::eval_builtin(
                "prb_prismatic_blade",
                &[
                    Value::length(l),
                    Value::length(w),
                    Value::length(t),
                    steel_with_e(e_val),
                    origin(),
                    axis_y(),
                ],
            ))
        };
        let k_base = k(length, width, thickness, e);

        // t³: double t → ×8  (I ∝ t³; 2t = 0.001 < L = 0.02 — constraint satisfied).
        let ratio_t = k(length, width, 2.0 * thickness, e) / k_base;
        let rel_t = (ratio_t - 8.0).abs() / 8.0;
        assert!(rel_t < 1e-9, "t³ scaling: ratio {ratio_t} vs 8 (rel {rel_t})");

        // 1/L³: double L → ×1/8  (k ∝ 1/L³).
        let ratio_l = k(2.0 * length, width, thickness, e) / k_base;
        let rel_l = (ratio_l - 0.125).abs() / 0.125;
        assert!(rel_l < 1e-9, "1/L³ scaling: ratio {ratio_l} vs 0.125 (rel {rel_l})");

        // E linear: double E → ×2  (k ∝ E).
        let ratio_e = k(length, width, thickness, 2.0 * e) / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(rel_e < 1e-9, "E scaling: ratio {ratio_e} vs 2 (rel {rel_e})");

        // Width linear: double width → ×2  (I = w·t³/12 ∝ w).
        let ratio_w = k(length, 2.0 * width, thickness, e) / k_base;
        let rel_w = (ratio_w - 2.0).abs() / 2.0;
        assert!(rel_w < 1e-9, "width scaling: ratio {ratio_w} vs 2 (rel {rel_w})");
    }

    // ── suggestion-1: transverse validity range ±δ ───────────────────────────

    /// Asserts that `prb_prismatic_blade` emits a symmetric, LENGTH-dimensioned
    /// `range` key with the correct closed-form ±δ:
    ///  - Steel (with yield_stress): δ_yield = 2·yield·L²/(3·E·t)
    ///  - No-yield fallback: δ = 0.1·L
    #[test]
    fn prb_prismatic_blade_validity_range() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let yield_si = 310e6_f64;

        // ── Case 1: steel() with yield_stress ────────────────────────────────
        let result = crate::eval_builtin("prb_prismatic_blade", &prismatic_args());
        let range = map_get(&result, "range").expect("range key must exist");
        let (lower, upper) = range_lower_upper(range);

        let delta_yield = 2.0 * yield_si * length.powi(2) / (3.0 * e * thickness);

        match lower {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "yield lower: LENGTH dimension");
                let rel = (si_value + delta_yield).abs() / delta_yield;
                assert!(rel < 1e-9, "yield lower = {si_value} vs {} (rel {rel})", -delta_yield);
            }
            other => panic!("yield lower: expected Length Scalar, got {other:?}"),
        }
        match upper {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "yield upper: LENGTH dimension");
                let rel = (si_value - delta_yield).abs() / delta_yield;
                assert!(rel < 1e-9, "yield upper = {si_value} vs {delta_yield} (rel {rel})");
            }
            other => panic!("yield upper: expected Length Scalar, got {other:?}"),
        }

        // ── Case 2: steel_no_yield() → δ = 0.1·L fallback ───────────────────
        let args_no_yield = vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            steel_no_yield(),
            origin(),
            axis_y(),
        ];
        let result_ny = crate::eval_builtin("prb_prismatic_blade", &args_no_yield);
        let range_ny = map_get(&result_ny, "range").expect("no-yield range key must exist");
        let (lower_ny, upper_ny) = range_lower_upper(range_ny);

        let delta_fallback = 0.1 * length;

        match lower_ny {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "no-yield lower: LENGTH dimension");
                let rel = (si_value + delta_fallback).abs() / delta_fallback;
                assert!(
                    rel < 1e-9,
                    "no-yield lower = {si_value} vs {} (rel {rel})",
                    -delta_fallback
                );
            }
            other => panic!("no-yield lower: expected Length Scalar, got {other:?}"),
        }
        match upper_ny {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "no-yield upper: LENGTH dimension");
                let rel = (si_value - delta_fallback).abs() / delta_fallback;
                assert!(rel < 1e-9, "no-yield upper = {si_value} vs {delta_fallback} (rel {rel})");
            }
            other => panic!("no-yield upper: expected Length Scalar, got {other:?}"),
        }
    }

    // ── suggestion-4: neutral (7th) argument ─────────────────────────────────

    /// Verifies the optional 7th `neutral` argument of `prb_prismatic_blade`:
    ///  - plain `Value::length(x)` → neutral SI = x
    ///  - `Value::Option(Some(Value::length(x)))` → neutral SI = x
    ///  - `Value::Option(None)` → neutral SI = 0.0 (default)
    #[test]
    fn prb_prismatic_blade_neutral_argument() {
        let neutral_m = 0.001_f64;
        let base = [
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ];

        // ── Case 1: plain Value::length ──────────────────────────────────────
        let mut args1 = base.to_vec();
        args1.push(Value::length(neutral_m));
        let result1 = crate::eval_builtin("prb_prismatic_blade", &args1);
        match map_get(&result1, "neutral") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "plain neutral: LENGTH dimension");
                let rel = (si_value - neutral_m).abs() / neutral_m;
                assert!(rel < 1e-9, "plain neutral = {si_value} vs {neutral_m} (rel {rel})");
            }
            other => panic!("plain neutral: expected Scalar, got {other:?}"),
        }

        // ── Case 2: Value::Option(Some(Value::length(x))) ────────────────────
        let mut args2 = base.to_vec();
        args2.push(Value::Option(Some(Box::new(Value::length(neutral_m)))));
        let result2 = crate::eval_builtin("prb_prismatic_blade", &args2);
        match map_get(&result2, "neutral") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "Option(Some) neutral: LENGTH dimension"
                );
                let rel = (si_value - neutral_m).abs() / neutral_m;
                assert!(
                    rel < 1e-9,
                    "Option(Some) neutral = {si_value} vs {neutral_m} (rel {rel})"
                );
            }
            other => panic!("Option(Some) neutral: expected Scalar, got {other:?}"),
        }

        // ── Case 3: Value::Option(None) → 0.0 ────────────────────────────────
        let mut args3 = base.to_vec();
        args3.push(Value::Option(None));
        let result3 = crate::eval_builtin("prb_prismatic_blade", &args3);
        match map_get(&result3, "neutral") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::LENGTH,
                    "Option(None) neutral: LENGTH dimension"
                );
                assert!(
                    si_value.abs() < 1e-12,
                    "Option(None) neutral = {si_value}, expected 0.0"
                );
            }
            other => panic!("Option(None) neutral: expected Scalar(0.0), got {other:?}"),
        }
    }

    // ── suggestion-2: invalid-input rejection ────────────────────────────────

    /// Verifies that `parse_prismatic_inputs` returns `None` (→ `Value::Undef`)
    /// for representative invalid-input classes, for both constructors.
    #[test]
    fn prb_prismatic_inputs_invalid_yield_undef() {
        // 5-arg call (arity ∉ {6, 7})
        let five_args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
        ];
        assert_eq!(
            crate::eval_builtin("prb_prismatic_blade", &five_args),
            Value::Undef,
            "blade: 5-arg call → Undef"
        );
        assert_eq!(
            crate::eval_builtin("prb_two_axis_pivot", &five_args),
            Value::Undef,
            "pivot: 5-arg call → Undef"
        );

        // thickness ≥ length (thickness = 0.001 ≥ length = 0.0005)
        let thick_args = vec![
            Value::length(0.0005),
            Value::length(0.005),
            Value::length(0.001),
            steel(),
            origin(),
            axis_y(),
        ];
        assert_eq!(
            crate::eval_builtin("prb_prismatic_blade", &thick_args),
            Value::Undef,
            "blade: thickness ≥ length → Undef"
        );

        // material without youngs_modulus
        let no_e = material("NoE", &[]);
        let no_e_args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            no_e,
            origin(),
            axis_y(),
        ];
        assert_eq!(
            crate::eval_builtin("prb_prismatic_blade", &no_e_args),
            Value::Undef,
            "blade: no youngs_modulus → Undef"
        );

        // dimensioned axis (LENGTH-dimensioned vector fails DIMENSIONLESS check)
        let bad_axis = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let bad_axis_args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            bad_axis,
        ];
        assert_eq!(
            crate::eval_builtin("prb_prismatic_blade", &bad_axis_args),
            Value::Undef,
            "blade: dimensioned axis → Undef"
        );
    }

    // ── step-3: RED — prb_two_axis_pivot smoke + multi-DOF invariant ─────────

    /// (a) kind, spring_rate invariant, damping, effective_stiffness closed-form.
    ///
    /// The §8.6/§13.1 multi-DOF invariant: `spring_rate == Option(None)` for any
    /// spherical (multi-DOF) joint — stiffness is surfaced only via
    /// `effective_stiffness`.
    #[test]
    fn prb_two_axis_pivot_structure_and_stiffness() {
        let result = crate::eval_builtin("prb_two_axis_pivot", &prismatic_args());

        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("spherical".to_string())),
            "two-axis pivot presents as a spherical joint"
        );
        // §8.6/§13.1 multi-DOF invariant: spring_rate is None for spherical joints.
        assert_eq!(
            map_get(&result, "spring_rate"),
            Some(&Value::Option(None)),
            "spring_rate is Option(None) [§8.6 multi-DOF invariant]"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is Option(None)"
        );

        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let i = width * thickness.powi(3) / 12.0;
        // γ = 1: symmetric per-axis slender blade (Henein 2010).
        let k_expected = e * i / length;

        match map_get(&result, "effective_stiffness") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "effective_stiffness carries ROTATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(
                    rel < 1e-9,
                    "effective_stiffness {si_value} vs {k_expected} (rel {rel})"
                );
            }
            other => panic!("expected effective_stiffness Scalar, got {other:?}"),
        }
    }

    // ── suggestion-3: spherical map structural invariants + θ_lim clamp ──────

    /// Asserts the spherical-specific invariants of `prb_two_axis_pivot`:
    ///  - no `axis` key (isotropic joint, not stored)
    ///  - no `neutral` key (spherical map layout)
    ///  - `pivot` is propagated
    ///  - `range_angle` is a symmetric ANGLE range
    ///  - θ_lim is clamped at PRB_ANGLE_LIMIT_RAD (5°) when θ_yield > 5°
    ///  - θ_lim equals θ_yield when θ_yield < 5° (soft material)
    #[test]
    fn prb_two_axis_pivot_spherical_invariants() {
        // PRB_ANGLE_LIMIT_RAD = 5° (reproduced inline — pub(super) not visible here)
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;

        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;

        // ── Structural invariants (steel() → θ_yield ≈ 0.121 rad > 5°) ───────
        let result = crate::eval_builtin("prb_two_axis_pivot", &prismatic_args());

        assert!(
            map_get(&result, "axis").is_none(),
            "spherical map must NOT carry an `axis` key (axis-isotropic)"
        );
        assert!(
            map_get(&result, "neutral").is_none(),
            "spherical map must NOT carry a `neutral` key"
        );
        assert!(
            map_get(&result, "pivot").is_some(),
            "`pivot` must be propagated"
        );

        // range_angle: symmetric ANGLE range clamped at PRB_ANGLE_LIMIT_RAD
        let range_angle = map_get(&result, "range_angle").expect("`range_angle` key must exist");
        let (lower, upper) = range_lower_upper(range_angle);
        assert_angle_close(lower, -prb_limit, "steel θ_lim lower (capped at 5°)");
        assert_angle_close(upper, prb_limit, "steel θ_lim upper (capped at 5°)");

        // ── θ_lim below cap: soft material → θ_yield < 5° ────────────────────
        // θ_yield = yield·L / (E·t/2);  with L=0.02, E=205e9, t=0.0005:
        // θ_yield = 100e6 * 0.02 / (205e9 * 0.00025) ≈ 0.039 rad < 5°.
        let small_yield = 100e6_f64;
        let theta_yield = small_yield * length / (e * thickness / 2.0);
        assert!(
            theta_yield < prb_limit,
            "pre-condition: theta_yield {theta_yield} must be < PRB limit {prb_limit}"
        );

        let soft = material(
            "SoftMaterial",
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
                        si_value: small_yield,
                        dimension: DimensionVector::PRESSURE,
                    }))),
                ),
            ],
        );
        let soft_args = vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            soft,
            origin(),
            axis_y(),
        ];
        let result_soft = crate::eval_builtin("prb_two_axis_pivot", &soft_args);
        let range_soft =
            map_get(&result_soft, "range_angle").expect("soft: `range_angle` key must exist");
        let (lower_s, upper_s) = range_lower_upper(range_soft);
        assert_angle_close(lower_s, -theta_yield, "soft θ_lim lower (uncapped = θ_yield)");
        assert_angle_close(upper_s, theta_yield, "soft θ_lim upper (uncapped = θ_yield)");
    }

    // ── step-13: RED — displacement-family compliance population ─────────────

    /// Read the `__flexure_compliance` FlexureCompliance record's fields from a
    /// flexure joint Map (panics if absent or the wrong shape). Local to this
    /// test module — `test_util` carries the shared fixtures, while the
    /// compliance reader is wired here as the prismatic family gains its cached
    /// record (step-14), mirroring beam.rs's local `compliance_fields`.
    fn compliance_fields(joint: &Value) -> &reify_ir::PersistentMap<String, Value> {
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

    /// Assert `v` is a LENGTH-dimensioned Scalar and return its si_value.
    fn length_scalar_si(v: &Value, label: &str) -> f64 {
        match v {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "{label}: LENGTH dimension");
                *si_value
            }
            other => panic!("{label}: expected LENGTH Scalar, got {other:?}"),
        }
    }

    #[test]
    fn prb_prismatic_blade_attaches_populated_compliance() {
        // Single-cantilever-blade (Howell §6.2): transverse surface stress
        // σ = 1.5·E·t·δ / L² at the displacement endpoint — consistent with the
        // blade's documented auto validity δ = 2·yield·L²/(3·E·t) (at which σ ==
        // yield). NOTE: distinct from the fixed-guided beam's σ = 3·E·t·δ/L²;
        // the blade is a cantilever (one free end), so its surface-stress
        // coefficient is half the fixed-guided one — using 3·E·t·δ/L² here would
        // report at_yield=true at the blade's own documented safe range.
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let sigma_at = |delta: f64| 1.5 * e * thickness * delta / length.powi(2);
        let delta_auto = 2.0 * yield_si * length.powi(2) / (3.0 * e * thickness);

        // ── auto endpoint (6-arg) ────────────────────────────────────────────
        let auto = crate::eval_builtin("prb_prismatic_blade", &prismatic_args());
        let fields = compliance_fields(&auto);
        let f = |k: &str| fields.get(&k.to_string()).unwrap_or_else(|| panic!("missing `{k}`"));

        let spring_si = spring_rate_si(&auto);
        match f("effective_stiffness") {
            Value::Real(r) => assert!(
                (r - spring_si).abs() / spring_si < 1e-12,
                "effective_stiffness {r} == spring_rate {spring_si}"
            ),
            other => panic!("effective_stiffness Real, got {other:?}"),
        }
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
        let (_, up) = range_lower_upper(map_get(&auto, "range").expect("range present"));
        let range_half = length_scalar_si(up, "auto range upper");
        // prb_validity_range is now Range<Length> = [−δ_auto, +δ_auto] (task 4587:
        // tightened from the Range<Angle> residual left by task 4576).
        let prb_half = length_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half - delta_auto).abs() / delta_auto < 1e-9
                && (prb_half - range_half).abs() / range_half < 1e-9,
            "prb_validity_range half {prb_half} == δ_auto {delta_auto} == range half {range_half}"
        );

        // ── declared displacement beyond yield deflection → at_yield ─────────
        // δ = 2 mm > δ_auto (≈0.81 mm): σ(2mm) ≈ 769 MPa > 310 MPa.
        let big = 0.002_f64;
        let yielding = crate::eval_builtin(
            "prb_prismatic_blade",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                steel(),
                origin(),
                axis_y(),
                Value::length(0.0), // neutral
                Value::length(big), // declared ±2 mm displacement
            ],
        );
        assert!(
            !yielding.is_undef(),
            "declared-displacement blade returns a joint, not Undef"
        );
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("prismatic".to_string())),
            "yielding blade is still a prismatic joint"
        );
        let (ylo, yup) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert!(
            (length_scalar_si(yup, "declared upper") - big).abs() / big < 1e-9
                && (length_scalar_si(ylo, "declared lower") + big).abs() / big < 1e-9,
            "declared displacement overrides the range to ±{big}"
        );
        let yf = compliance_fields(&yielding);
        let yg = |k: &str| yf.get(&k.to_string()).unwrap_or_else(|| panic!("missing `{k}`"));
        let expected_big = sigma_at(big);
        assert!(
            expected_big > yield_si,
            "fixture sanity: σ(2mm)={expected_big} > yield {yield_si}"
        );
        match yg("max_stress") {
            Value::Scalar { si_value, .. } => assert!(
                (si_value - expected_big).abs() / expected_big < 1e-9,
                "declared max_stress {si_value} vs analytic {expected_big}"
            ),
            other => panic!("max_stress Scalar, got {other:?}"),
        }
        assert_eq!(
            yg("at_yield"),
            &Value::Bool(true),
            "declared 2mm drives at_yield true"
        );
        match yg("yield_margin") {
            Value::Real(r) => assert!(*r < 0.0, "yielding ⇒ negative margin, got {r}"),
            other => panic!("yield_margin Real, got {other:?}"),
        }

        // ── declared displacement below yield deflection → safe ──────────────
        // δ = 0.3 mm < δ_auto: σ ≈ 115 MPa < 310 MPa ⇒ at_yield false.
        let small = 0.0003_f64;
        let safe = crate::eval_builtin(
            "prb_prismatic_blade",
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
        assert!(!safe.is_undef(), "safe declared-displacement blade returns a joint");
        let sf = compliance_fields(&safe);
        assert_eq!(
            sf.get(&"at_yield".to_string()).expect("at_yield present"),
            &Value::Bool(false),
            "declared 0.3mm stays below yield"
        );
    }

    #[test]
    fn prb_two_axis_pivot_attaches_populated_compliance() {
        // Spherical two-axis pivot (Henein 2010): cantilever angular surface
        // stress σ = E·(t/2)·θ / L at the auto θ_lim = min(θ_yield, 5°). For
        // steel / L=20mm / t=0.5mm, θ_yield ≈ 6.93° > 5°, so θ_lim = 5° and σ <
        // yield ⇒ at_yield false. The record rides on the spherical Map exactly
        // like the 1-DOF joints (attach_compliance works on any Value::Map).
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;

        let result = crate::eval_builtin("prb_two_axis_pivot", &prismatic_args());
        let fields = compliance_fields(&result);
        let f = |k: &str| fields.get(&k.to_string()).unwrap_or_else(|| panic!("missing `{k}`"));

        // effective_stiffness (Real) == per-axis k_axis = E·I/L (γ = 1).
        let i = width * thickness.powi(3) / 12.0;
        let k_axis = e * i / length;
        match f("effective_stiffness") {
            Value::Real(r) => assert!(
                (r - k_axis).abs() / k_axis < 1e-9,
                "effective_stiffness {r} == k_axis {k_axis}"
            ),
            other => panic!("effective_stiffness Real, got {other:?}"),
        }

        // max_stress (PRESSURE) == E·(t/2)·θ_lim/L at θ_lim = 5°, below yield.
        let expected = e * (thickness / 2.0) * prb_limit / length;
        assert!(
            expected < yield_si,
            "fixture sanity: σ(5°)={expected} < yield {yield_si}"
        );
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected).abs() / expected < 1e-9,
                    "pivot max_stress {si_value} vs analytic {expected}"
                );
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }

        // at_yield false (the 5° cap keeps σ below yield).
        assert_eq!(f("at_yield"), &Value::Bool(false), "5°-capped pivot is not at yield");

        // prb_validity_range is now Range<Angle> = [−θ_lim, +θ_lim] (task 4576).
        let prb_half_p = angle_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half_p - prb_limit).abs() / prb_limit < 1e-9,
            "prb_validity_range half {prb_half_p} == θ_lim {prb_limit}"
        );
    }
}
