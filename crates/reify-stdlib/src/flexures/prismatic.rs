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

use super::common::{length_si, make_flexure_joint, material_field_si};

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
    /// Optional trailing `neutral` argument (present in the 7-arg form).
    neutral_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout:
/// `(length, width, thickness, material, pivot, axis[, neutral])`.
///
/// Returns `None` (⇒ `Value::Undef`) on: arity ∉ {6, 7}; non-positive or
/// non-finite geometry; thickness ≥ length; missing or non-positive
/// `youngs_modulus`; or an invalid axis.
fn parse_prismatic_inputs(args: &[Value]) -> Option<PrismaticInputs<'_>> {
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
    Some(PrismaticInputs {
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

    // Cantilever transverse-displacement validity range ±δ.
    // Surface stress σ = 1.5·E·t·δ / L²  ⇒  δ_yield = 2·yield·L² / (3·E·t).
    // No yield_stress ⇒ small-deflection fallback ±0.1·L.
    let delta = match b.yield_si {
        Some(yield_si) => 2.0 * yield_si * b.length.powi(2) / (3.0 * b.e * b.thickness),
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
}
