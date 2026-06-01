//! Hinge-flexure PRB constructors (PRD §2.2 + §11 Phase-2 task ε):
//! living hinge (Howell §5.7 SLFP), cross-spring pivot (Haringx 1949), and
//! LET joint (Jacobsen et al. 2009) — all → revolute.
//!
//! All three constructors return a 1-DOF Revolute joint `Value::Map`
//! (`kind == "revolute"`) following the same closed-form pattern as
//! γ (beam.rs) and δ (notch.rs). No FEA call — pure closed form.
//! `damping = None` for all three (PRD §5.1 / §8.7 γ-scope contract).
//! Validation failure → `Value::Undef` with NO diagnostic emission
//! (W_Flexure*/E_Flexure* emission is λ/task-3821's responsibility).

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    attach_compliance, cantilever_sigma_at, length_si, make_compliance_record, make_flexure_joint,
    material_field_si, material_numeric_field, neutral_angle_si, parse_declared_range,
    symmetric_angle_range, RangeKind, PRB_ANGLE_LIMIT_RAD,
};

/// Howell §5.7 small-length flexural pivot (SLFP) stiffness coefficient.
///
/// k_θ = γ_lh · E · I / L with γ_lh = 1.0. Unlike the cantilever PRB model
/// (γ = 2.65), the SLFP concentrates all compliance into a single torsional
/// spring with no characteristic-radius amplification — the segment IS the
/// spring (Howell §5.7, PRD §2.2 Phase-2 task ε).
const LIVING_HINGE_GAMMA: f64 = 1.0;

/// Haringx (1949) crossed-leaf pivot stiffness coefficient.
///
/// k_θ = γ_cs · E · I / L with γ_cs = 2.0. Two crossed leaves intersecting
/// at mid-length λ = 0.5, each contributing EI/L to the first-order linear
/// rotational stiffness (PRD §2.2 Phase-2 task ε).
const CROSS_SPRING_GAMMA: f64 = 2.0;

/// Evaluate a hinge-flexure constructor by name.
///
/// Returns `Some(Value)` for a recognised hinge name (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so the caller can fall through to the next module.
pub(crate) fn eval_hinge(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_living_hinge" => Some(prb_living_hinge(args)),
        "prb_cross_spring_pivot" => Some(prb_cross_spring_pivot(args)),
        "prb_let_joint" => Some(prb_let_joint(args)),
        _ => None,
    }
}

/// Shared, validated inputs for the bending-hinge constructors (living hinge
/// and cross-spring pivot). Both share the same positional argument layout
/// `(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])` and the same
/// surface-bending stress/yield formula — they differ only in the k_γ constant.
struct BendingHingeInputs<'a> {
    length: f64,
    thickness: f64,
    e: f64,
    /// Rectangular-section second moment of area `I = width·thickness³/12`.
    i: f64,
    yield_si: Option<f64>,
    axis: &'a Value,
    pivot: &'a Value,
    neutral_arg: Option<&'a Value>,
    /// The optional trailing declared operating-range argument (present only in
    /// the 8-arg form). When present, its endpoint — an ANGLE half-angle — not
    /// the auto cap, drives the joint range and the §5.3 `max_stress` endpoint.
    declared_range_arg: Option<&'a Value>,
}

/// Parse and validate the shared bending-hinge argument layout:
/// `(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])`.
///
/// Returns `None` (⇒ `Value::Undef`) on: arity ∉ {6, 7, 8}; non-positive or
/// non-finite geometry; thickness ≥ length; missing or non-positive
/// `youngs_modulus`; or an invalid axis.
fn parse_bending_hinge_inputs(args: &[Value]) -> Option<BendingHingeInputs<'_>> {
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
    Some(BendingHingeInputs {
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

/// Parametrized core for living hinge and cross-spring pivot.
///
/// Closed form (PRD §2.2):
///   k_θ = k_gamma · E · I / L
///
/// Surface-bending yield rotation:
///   σ(θ) = E · (t/2) · θ / L  ⇒  θ_yield = yield · L / (E · t/2)
///
/// The auto validity range is ±min(θ_yield, 5°) (no yield_stress ⇒ ±5°). An
/// optional trailing `declared_range` (a half-angle) OVERRIDES that cap for the
/// joint range and the cached §5.3 `max_stress` endpoint; the auto cap is
/// retained as the SAFE/suggested `prb_validity_range` in the compliance record.
fn bending_hinge_revolute(b: &BendingHingeInputs<'_>, k_gamma: f64) -> Value {
    let k_theta = k_gamma * b.e * b.i / b.length;

    // Auto SAFE bound θ_lim = ±min(θ_yield, 5°) — retained in the compliance
    // record regardless of any wider declared range.
    let theta_lim = match b.yield_si {
        Some(yield_si) => {
            let theta_yield = yield_si * b.length / (b.e * b.thickness / 2.0);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };

    // An optional user-declared operating range (±half-angle) OVERRIDES the auto
    // cap for the joint range and the §5.3 stress endpoint.
    let declared = parse_declared_range(b.declared_range_arg, RangeKind::Angle);
    let range_endpoint = declared.unwrap_or(theta_lim);
    let range = symmetric_angle_range(range_endpoint);

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

    // Cache the FlexureCompliance record (§5.3): cantilever angular surface
    // stress σ = E·(t/2)·θ/L at the range endpoint (declared when present, else
    // the auto θ_lim) and at the neutral rest angle. prb_validity_range stores
    // the auto SAFE θ_lim regardless of any wider declared range.
    let max_stress = cantilever_sigma_at(range_endpoint, b.length, b.thickness, b.e);
    let max_stress_at_neutral = cantilever_sigma_at(neutral_si, b.length, b.thickness, b.e);
    let record = make_compliance_record(
        k_theta,
        max_stress,
        max_stress_at_neutral,
        b.yield_si,
        None,
        theta_lim,
    );
    attach_compliance(joint, record)
}

/// `prb_living_hinge(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])`
/// — Howell §5.7 small-length flexural pivot (SLFP) as a revolute joint.
///
/// Returns a joint `Value::Map` (`kind == "revolute"`) with rotational
/// stiffness `k_θ = E·I/L` (γ_lh = 1.0 — no PRB characteristic-radius
/// amplification; the SLFP segment IS the torsional spring). Validity range
/// `±min(θ_yield, 5°)` where `θ_yield = yield·L/(E·t/2)`.
fn prb_living_hinge(args: &[Value]) -> Value {
    match parse_bending_hinge_inputs(args) {
        Some(b) => bending_hinge_revolute(&b, LIVING_HINGE_GAMMA),
        None => Value::Undef,
    }
}

/// `prb_cross_spring_pivot(length, width, thickness, material, pivot, axis[, neutral[, declared_range]])`
/// — Haringx 1949 crossed-leaf pivot as a revolute joint.
///
/// Same closed-form structure as `prb_living_hinge` but with `γ_cs = 2.0`
/// (two crossed leaves each contributing EI/L at the mid-length intersection).
fn prb_cross_spring_pivot(args: &[Value]) -> Value {
    match parse_bending_hinge_inputs(args) {
        Some(b) => bending_hinge_revolute(&b, CROSS_SPRING_GAMMA),
        None => Value::Undef,
    }
}

/// Validated inputs for `prb_let_joint`.
struct LetInputs<'a> {
    length: f64,
    thickness: f64,
    width: f64,
    n_blades: f64,
    /// Shear modulus G = E / (2·(1+ν)) derived from youngs_modulus + poisson_ratio.
    g: f64,
    yield_si: Option<f64>,
    axis: &'a Value,
    pivot: &'a Value,
    neutral_arg: Option<&'a Value>,
    /// The optional trailing declared operating-range argument (present only in
    /// the 9-arg form). When present, its endpoint — an ANGLE half-angle — not
    /// the auto cap, drives the joint range and the §5.3 `max_stress` endpoint.
    declared_range_arg: Option<&'a Value>,
}

/// Parse and validate LET joint arguments:
/// `(length, width, thickness, n_blades, material, pivot, axis[, neutral[, declared_range]])`.
///
/// Returns `None` on: arity ∉ {7, 8, 9}; non-positive or non-finite geometry;
/// thickness ≥ length; n_blades < 1 or non-integer; missing/non-positive
/// `youngs_modulus`; missing `poisson_ratio` or ν ∉ [0, 0.5); invalid axis.
fn parse_let_inputs(args: &[Value]) -> Option<LetInputs<'_>> {
    if args.len() < 7 || args.len() > 9 {
        return None;
    }
    let length = length_si(&args[0])?;
    let width = length_si(&args[1])?;
    let thickness = length_si(&args[2])?;
    if length <= 0.0 || width <= 0.0 || thickness <= 0.0 || thickness >= length {
        return None;
    }
    // n_blades: positive integer (Int or whole-valued finite Real).
    let n_blades = match &args[3] {
        Value::Int(n) if *n >= 1 => *n as f64,
        Value::Real(r) if r.is_finite() && *r >= 1.0 && r.fract() == 0.0 => *r,
        _ => return None,
    };
    let material = &args[4];
    let e = material_field_si(material, "youngs_modulus")?;
    if e <= 0.0 {
        return None;
    }
    let nu = material_numeric_field(material, "poisson_ratio")?;
    if !(0.0..0.5).contains(&nu) {
        return None;
    }
    let g = e / (2.0 * (1.0 + nu));
    let axis = &args[6];
    crate::helpers::validate_dimensionless_unit_axis_vec3(axis)?;
    Some(LetInputs {
        length,
        thickness,
        width,
        n_blades,
        g,
        yield_si: material_field_si(material, "yield_stress"),
        axis,
        pivot: &args[5],
        neutral_arg: if args.len() >= 8 { Some(&args[7]) } else { None },
        declared_range_arg: if args.len() == 9 { Some(&args[8]) } else { None },
    })
}

/// LET-joint torsional surface stress as a von Mises tensile-equivalent
/// σ_eq = √3·G·t·|θ| / L (Jacobsen et al. 2009) — the algebraic inverse of the
/// LET joint's `θ_yield = σy·L/(√3·G·t)`.
///
/// The LET joint is governed by SHEAR (τ = G·t·θ/L), not bending, so it cannot
/// reuse the cantilever `E·(t/2)·θ/L` formula. To let the FlexureCompliance
/// `at_yield` check compare against the material *tensile* yield σy directly
/// (as the bending families do), the cached stress is the von Mises equivalent
/// √3·τ; this makes `at_yield` trip exactly at the joint's own torsional yield
/// rotation, consistent with the auto range cap (mirroring how `prismatic.rs`'s
/// blade carries a stress formula matched to its own validity δ).
fn let_vm_sigma_at(theta: f64, length: f64, thickness: f64, g: f64) -> f64 {
    3.0_f64.sqrt() * g * thickness * theta.abs() / length
}

/// `prb_let_joint(length, width, thickness, n_blades, material, pivot, axis[, neutral[, declared_range]])`
/// — Jacobsen et al. 2009 lamina-emergent torsion (multi-blade torsion) as a
/// revolute joint.
///
/// Closed form:
///   G = E / (2·(1+ν))   (isotropic shear modulus)
///   J = width·thickness³ / 3   (thin-strip St. Venant torsion constant)
///   k_θ = n_blades · G · J / L
///
/// Torsional yield rotation:
///   τ(θ) = G·t·θ/L  ⇒  τ_y = σy/√3  ⇒  θ_yield = σy·L/(√3·G·t)
///
/// The auto validity range is ±min(θ_yield, 5°) (no yield_stress ⇒ ±5°). An
/// optional trailing `declared_range` (a half-angle) OVERRIDES that cap for the
/// joint range and the cached §5.3 `max_stress` endpoint (stored as the von
/// Mises tensile-equivalent σ_eq = √3·G·t·θ/L, see [`let_vm_sigma_at`]); the
/// auto cap is retained as the SAFE `prb_validity_range` in the compliance record.
fn prb_let_joint(args: &[Value]) -> Value {
    let Some(l) = parse_let_inputs(args) else {
        return Value::Undef;
    };

    let j = l.width * l.thickness.powi(3) / 3.0;
    let k_theta = l.n_blades * l.g * j / l.length;

    // Auto SAFE bound θ_lim = ±min(θ_yield, 5°) — retained in the compliance
    // record regardless of any wider declared range.
    let theta_lim = match l.yield_si {
        Some(yield_si) => {
            let theta_yield = yield_si * l.length / (3.0_f64.sqrt() * l.g * l.thickness);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };

    // An optional user-declared operating range (±half-angle) OVERRIDES the auto
    // cap for the joint range and the §5.3 stress endpoint.
    let declared = parse_declared_range(l.declared_range_arg, RangeKind::Angle);
    let range_endpoint = declared.unwrap_or(theta_lim);
    let range = symmetric_angle_range(range_endpoint);

    let neutral_si = l.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    let joint = make_flexure_joint(
        "revolute",
        l.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        l.pivot.clone(),
    );

    // Cache the FlexureCompliance record (§5.3): the von Mises tensile-equivalent
    // of the torsional surface stress at the range endpoint (declared when
    // present, else the auto θ_lim) and the neutral rest angle. prb_validity_range
    // stores the auto SAFE θ_lim regardless of any wider declared range.
    let max_stress = let_vm_sigma_at(range_endpoint, l.length, l.thickness, l.g);
    let max_stress_at_neutral = let_vm_sigma_at(neutral_si, l.length, l.thickness, l.g);
    let record = make_compliance_record(
        k_theta,
        max_stress,
        max_stress_at_neutral,
        l.yield_si,
        None,
        theta_lim,
    );
    attach_compliance(joint, record)
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::Value;
    use std::f64::consts::PI;
    use super::super::test_util::*;

    // ── Convenience builders ─────────────────────────────────────────────────

    /// Build the standard 6-arg bending-hinge argument list
    /// `(length, width, thickness, material, pivot, axis)` — shared by the
    /// living-hinge and cross-spring-pivot test suites.
    fn bending_hinge_args_6(length: f64, width: f64, thickness: f64, mat: Value) -> Vec<Value> {
        vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            mat,
            origin(),
            axis_y(),
        ]
    }

    // ── Scaling-ratio helpers ────────────────────────────────────────────────

    /// Coefficient-independent functional-form scaling-ratio assertions for
    /// bending-hinge ctors (living hinge and cross-spring pivot).
    ///
    /// k = γ · E · I / L  where  I = width · thickness³ / 12.
    /// Tests four independent ratios: t³, 1/L, E (linear), width (linear).
    /// γ cancels in all four ratios so the same helper covers both variants;
    /// any mis-scaling in the formula (wrong exponent on t, width vs. length
    /// swap, etc.) fails independently of the γ constant value.
    fn assert_bending_hinge_scaling(ctor_name: &str) {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;

        // Inline k-extractor to keep call sites concise.
        let k = |l: f64, w: f64, t: f64, e: f64| {
            spring_rate_si(&crate::eval_builtin(
                ctor_name,
                &bending_hinge_args_6(l, w, t, steel_with_e(e)),
            ))
        };
        let k_base = k(length, width, thickness, e);

        // t³ scaling: double t → ×2³ = 8  (I = w·t³/12 ∝ t³).
        // 2·thickness = 0.001 m < length = 0.02 m — constraint satisfied.
        let ratio_t = k(length, width, 2.0 * thickness, e) / k_base;
        let rel_t = (ratio_t - 8.0).abs() / 8.0;
        assert!(
            rel_t < 1e-9,
            "{ctor_name}: t³ scaling: ratio {ratio_t} vs 8 (rel {rel_t})"
        );

        // 1/L scaling: double L → ×1/2.
        let ratio_l = k(2.0 * length, width, thickness, e) / k_base;
        let rel_l = (ratio_l - 0.5).abs() / 0.5;
        assert!(
            rel_l < 1e-9,
            "{ctor_name}: 1/L scaling: ratio {ratio_l} vs 0.5 (rel {rel_l})"
        );

        // E linear scaling: double E → ×2  (k ∝ E·I/L).
        let ratio_e = k(length, width, thickness, 2.0 * e) / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(
            rel_e < 1e-9,
            "{ctor_name}: E scaling: ratio {ratio_e} vs 2 (rel {rel_e})"
        );

        // Width linear scaling: double width → ×2  (I = w·t³/12 ∝ w).
        let ratio_w = k(length, 2.0 * width, thickness, e) / k_base;
        let rel_w = (ratio_w - 2.0).abs() / 2.0;
        assert!(
            rel_w < 1e-9,
            "{ctor_name}: width scaling: ratio {ratio_w} vs 2 (rel {rel_w})"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // step-1: RED — prb_living_hinge test suite
    // ─────────────────────────────────────────────────────────────────────────

    /// (a) Structure: kind, damping, axis, spring_rate dimension.
    #[test]
    fn prb_living_hinge_structure() {
        let args = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());
        let result = crate::eval_builtin("prb_living_hinge", &args);
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "living hinge presents as a revolute joint"
        );
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis is preserved verbatim"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is Option(None)"
        );
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { dimension, .. }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b) k_θ closed-form: k = γ_lh · E · I / L with γ_lh = 1.0 (SLFP).
    ///
    /// γ_lh = 1.0 is pinned as a literal; any change to the constant fails here.
    #[test]
    fn prb_living_hinge_spring_rate_closed_form() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;

        let args = bending_hinge_args_6(length, width, thickness, steel());
        let result = crate::eval_builtin("prb_living_hinge", &args);

        // k_θ = γ_lh · E · I / L  with  I = width·t³/12  and  γ_lh = 1.0.
        let i = width * thickness.powi(3) / 12.0;
        let k_expected = 1.0 * e * i / length; // γ_lh = 1.0 (Howell §5.7 SLFP)
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(
                    rel < 1e-9,
                    "spring_rate {si_value} vs {k_expected} (rel {rel})"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b-scaling) Functional-form scaling-ratio assertions for living hinge:
    /// k ∝ t³, ∝ 1/L, ∝ E, ∝ width — coefficient-independent of γ_lh=1.0.
    #[test]
    fn prb_living_hinge_scaling_ratios() {
        assert_bending_hinge_scaling("prb_living_hinge");
    }

    /// (c) Range branches: yield-capped, PRB-capped, and no-yield fallback.
    ///
    /// Yield formula for living hinge: θ_yield = yield · L / (E · t/2).
    #[test]
    fn prb_living_hinge_range_branches() {
        let prb_limit = 5.0_f64 * PI / 180.0;
        let e = 205e9_f64;
        let yield_stress = 310e6_f64;

        // (i) Yield-capped branch: short/thick so θ_yield < 5°.
        //   L = 2mm, t = 0.5mm  →  θ_yield = 310e6·0.002/(205e9·0.00025) ≈ 0.012 rad < 5°
        let l_short = 0.002_f64;
        let t_thick = 0.0005_f64;
        let theta_yield_short = yield_stress * l_short / (e * t_thick / 2.0);
        assert!(
            theta_yield_short < prb_limit,
            "yield-capped fixture: θ_yield={theta_yield_short:.5} must be < 5°"
        );
        let result_yield = crate::eval_builtin(
            "prb_living_hinge",
            &bending_hinge_args_6(l_short, 0.005, t_thick, steel()),
        );
        let (lo, up) = range_lower_upper(map_get(&result_yield, "range").unwrap());
        assert_angle_close(lo, -theta_yield_short, "yield-capped lower");
        assert_angle_close(up, theta_yield_short, "yield-capped upper");

        // (ii) PRB-capped branch: long/thin so θ_yield > 5°.
        //   L = 20mm, t = 0.5mm → θ_yield ≈ 0.121 rad ≈ 6.9° > 5°
        let l_long = 0.02_f64;
        let t_thin = 0.0005_f64;
        let theta_yield_long = yield_stress * l_long / (e * t_thin / 2.0);
        assert!(
            theta_yield_long > prb_limit,
            "PRB-capped fixture: θ_yield={theta_yield_long:.5} must be > 5°"
        );
        let result_prb = crate::eval_builtin(
            "prb_living_hinge",
            &bending_hinge_args_6(l_long, 0.005, t_thin, steel()),
        );
        let (lo_prb, up_prb) =
            range_lower_upper(map_get(&result_prb, "range").unwrap());
        assert_angle_close(lo_prb, -prb_limit, "PRB-capped lower");
        assert_angle_close(up_prb, prb_limit, "PRB-capped upper");

        // (iii) No-yield fallback: steel without yield_stress → ±5°.
        let result_ny = crate::eval_builtin(
            "prb_living_hinge",
            &bending_hinge_args_6(l_long, 0.005, t_thin, steel_no_yield()),
        );
        let (lo_ny, up_ny) =
            range_lower_upper(map_get(&result_ny, "range").unwrap());
        assert_angle_close(lo_ny, -prb_limit, "no-yield lower");
        assert_angle_close(up_ny, prb_limit, "no-yield upper");
    }

    /// (d) Neutral-angle handling: 6-arg default, 7-arg bare, Option(Some), Option(None).
    #[test]
    fn prb_living_hinge_neutral_angle_handling() {
        let two_deg = 2.0_f64 * PI / 180.0;
        let base = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());

        let call_with_neutral = |n: Option<Value>| {
            let mut args = base.clone();
            if let Some(v) = n {
                args.push(v);
            }
            crate::eval_builtin("prb_living_hinge", &args)
        };

        // 6-arg: neutral defaults to angle(0).
        let six = call_with_neutral(None);
        assert_eq!(
            map_get(&six, "neutral"),
            Some(&Value::angle(0.0)),
            "6-arg defaults neutral to angle(0)"
        );

        // 7-arg bare angle.
        let seven = call_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(
            map_get(&seven, "neutral").unwrap(),
            two_deg,
            "7-arg bare-angle neutral",
        );

        // 7-arg Option(Some(angle)).
        let seven_opt = call_with_neutral(Some(Value::Option(Some(Box::new(
            Value::angle(two_deg),
        )))));
        assert_angle_close(
            map_get(&seven_opt, "neutral").unwrap(),
            two_deg,
            "7-arg Option(Some(angle)) neutral",
        );

        // 7-arg Option(None) → angle(0).
        let seven_none = call_with_neutral(Some(Value::Option(None)));
        assert_eq!(
            map_get(&seven_none, "neutral"),
            Some(&Value::angle(0.0)),
            "7-arg Option(None) neutral → angle(0)"
        );
    }

    /// (e) Invalid-input rejection → Undef.
    #[test]
    fn prb_living_hinge_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_living_hinge", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let base = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());
        let with = |idx: usize, v: Value| {
            let mut a = base.clone();
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        undef(base[..3].to_vec(), "3 args");
        // Arity 8 (neutral + declared_range) is now VALID (step-18); 9 args
        // overflows the highest supported arity and is rejected.
        {
            let mut a = base.clone();
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

        // Degenerate: thickness ≥ length.
        undef(with(2, Value::length(0.02)), "thickness == length");
        undef(with(2, Value::length(0.03)), "thickness > length");

        // Bad material.
        undef(with(3, Value::Real(1.0)), "material not StructureInstance");
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

    // ─────────────────────────────────────────────────────────────────────────
    // step-3: RED — prb_cross_spring_pivot test suite (reuses above fixtures)
    // ─────────────────────────────────────────────────────────────────────────

    /// (a) Structure for cross-spring pivot.
    #[test]
    fn prb_cross_spring_pivot_structure() {
        let args = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());
        let result = crate::eval_builtin("prb_cross_spring_pivot", &args);
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "cross-spring presents as a revolute joint"
        );
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis preserved verbatim"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is Option(None)"
        );
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { dimension, .. }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b) k_θ closed-form for cross-spring: k = γ_cs · E · I / L with γ_cs = 2.0.
    ///
    /// γ_cs = 2.0 is pinned as a literal; swapping with γ_lh = 1.0 fails here.
    #[test]
    fn prb_cross_spring_pivot_spring_rate_closed_form() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;

        let args = bending_hinge_args_6(length, width, thickness, steel());
        let result = crate::eval_builtin("prb_cross_spring_pivot", &args);

        let i = width * thickness.powi(3) / 12.0;
        let k_expected = 2.0 * e * i / length; // γ_cs = 2.0 (Haringx 1949)
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_expected).abs() / k_expected;
                assert!(
                    rel < 1e-9,
                    "spring_rate {si_value} vs {k_expected} (rel {rel})"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b-scaling) Functional-form scaling-ratio assertions for cross-spring pivot:
    /// k ∝ t³, ∝ 1/L, ∝ E, ∝ width — same formula structure as living hinge,
    /// γ_cs=2.0 cancels in all ratios so this is coefficient-independent.
    #[test]
    fn prb_cross_spring_pivot_scaling_ratios() {
        assert_bending_hinge_scaling("prb_cross_spring_pivot");
    }

    /// (c) Range branches for cross-spring: same θ_yield = yield·L/(E·t/2) formula.
    #[test]
    fn prb_cross_spring_pivot_range_branches() {
        let prb_limit = 5.0_f64 * PI / 180.0;
        let e = 205e9_f64;
        let yield_stress = 310e6_f64;

        // (i) Yield-capped.
        let l_short = 0.002_f64;
        let t_thick = 0.0005_f64;
        let theta_yield_short = yield_stress * l_short / (e * t_thick / 2.0);
        assert!(theta_yield_short < prb_limit);
        let result_yield = crate::eval_builtin(
            "prb_cross_spring_pivot",
            &bending_hinge_args_6(l_short, 0.005, t_thick, steel()),
        );
        let (lo, up) = range_lower_upper(map_get(&result_yield, "range").unwrap());
        assert_angle_close(lo, -theta_yield_short, "cs yield-capped lower");
        assert_angle_close(up, theta_yield_short, "cs yield-capped upper");

        // (ii) PRB-capped.
        let l_long = 0.02_f64;
        let t_thin = 0.0005_f64;
        let theta_yield_long = yield_stress * l_long / (e * t_thin / 2.0);
        assert!(theta_yield_long > prb_limit);
        let result_prb = crate::eval_builtin(
            "prb_cross_spring_pivot",
            &bending_hinge_args_6(l_long, 0.005, t_thin, steel()),
        );
        let (lo_prb, up_prb) = range_lower_upper(map_get(&result_prb, "range").unwrap());
        assert_angle_close(lo_prb, -prb_limit, "cs PRB-capped lower");
        assert_angle_close(up_prb, prb_limit, "cs PRB-capped upper");

        // (iii) No-yield fallback.
        let result_ny = crate::eval_builtin(
            "prb_cross_spring_pivot",
            &bending_hinge_args_6(l_long, 0.005, t_thin, steel_no_yield()),
        );
        let (lo_ny, up_ny) = range_lower_upper(map_get(&result_ny, "range").unwrap());
        assert_angle_close(lo_ny, -prb_limit, "cs no-yield lower");
        assert_angle_close(up_ny, prb_limit, "cs no-yield upper");
    }

    /// (d) Neutral handling for cross-spring.
    #[test]
    fn prb_cross_spring_pivot_neutral_angle_handling() {
        let two_deg = 2.0_f64 * PI / 180.0;
        let base = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());

        let call_with_neutral = |n: Option<Value>| {
            let mut args = base.clone();
            if let Some(v) = n {
                args.push(v);
            }
            crate::eval_builtin("prb_cross_spring_pivot", &args)
        };

        let six = call_with_neutral(None);
        assert_eq!(map_get(&six, "neutral"), Some(&Value::angle(0.0)));

        let seven = call_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(map_get(&seven, "neutral").unwrap(), two_deg, "cs 7-arg bare");

        let seven_opt = call_with_neutral(Some(Value::Option(Some(Box::new(
            Value::angle(two_deg),
        )))));
        assert_angle_close(
            map_get(&seven_opt, "neutral").unwrap(),
            two_deg,
            "cs 7-arg Option(Some)",
        );

        let seven_none = call_with_neutral(Some(Value::Option(None)));
        assert_eq!(map_get(&seven_none, "neutral"), Some(&Value::angle(0.0)));
    }

    /// (e) Invalid-input rejection for cross-spring → Undef.
    #[test]
    fn prb_cross_spring_pivot_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_cross_spring_pivot", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let base = bending_hinge_args_6(0.02, 0.005, 0.0005, steel());
        let with = |idx: usize, v: Value| {
            let mut a = base.clone();
            a[idx] = v;
            a
        };

        undef(vec![], "cs 0 args");
        undef(with(0, Value::length(0.0)), "cs length=0");
        undef(with(2, Value::length(0.02)), "cs thickness==length");
        undef(with(3, Value::Real(1.0)), "cs bad material");
        undef(with(5, Value::Real(1.0)), "cs bad axis");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // step-5: RED — prb_let_joint test suite
    // ─────────────────────────────────────────────────────────────────────────

    /// Build a 7-arg LET joint argument list (minimum arity).
    fn let_args_7(
        length: f64,
        width: f64,
        thickness: f64,
        n_blades: i64,
        mat: Value,
    ) -> Vec<Value> {
        vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            Value::Int(n_blades),
            mat,
            origin(),
            axis_y(),
        ]
    }

    /// (a) Structure for LET joint.
    #[test]
    fn prb_let_joint_structure() {
        let args = let_args_7(0.02, 0.005, 0.0005, 2, steel());
        let result = crate::eval_builtin("prb_let_joint", &args);
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "LET joint presents as a revolute joint"
        );
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis preserved verbatim"
        );
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is Option(None)"
        );
        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { dimension, .. }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }
    }

    /// (b) k_θ closed-form: G=E/(2(1+ν)), J=width·t³/3, k=n·G·J/L.
    ///
    /// Tests n=2 and n=4 to pin the linear blade-count scaling.
    #[test]
    fn prb_let_joint_spring_rate_closed_form() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let nu = 0.29_f64; // steel() poisson_ratio

        let g = e / (2.0 * (1.0 + nu));
        let j = width * thickness.powi(3) / 3.0;

        for n_blades in [2_i64, 4_i64] {
            let args = let_args_7(length, width, thickness, n_blades, steel());
            let result = crate::eval_builtin("prb_let_joint", &args);
            let k_expected = (n_blades as f64) * g * j / length;
            match map_get(&result, "spring_rate") {
                Some(Value::Scalar { si_value, dimension }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::ROTATIONAL_STIFFNESS,
                        "spring_rate carries ROTATIONAL_STIFFNESS (n={n_blades})"
                    );
                    let rel = (si_value - k_expected).abs() / k_expected;
                    assert!(
                        rel < 1e-9,
                        "n={n_blades}: spring_rate {si_value} vs {k_expected} (rel {rel})"
                    );
                }
                other => panic!("n={n_blades}: expected spring_rate Scalar, got {other:?}"),
            }
        }
    }

    /// (b-scaling) Functional-form scaling-ratio assertions for LET joint:
    /// k ∝ t³, ∝ 1/L, ∝ n_blades, ∝ G(E) — formula-structure checks independent
    /// of the absolute closed-form value.
    #[test]
    fn prb_let_joint_scaling_ratios() {
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let e = 205e9_f64;
        let n_base = 2_i64;

        let k = |l: f64, w: f64, t: f64, n: i64, e: f64| {
            spring_rate_si(&crate::eval_builtin(
                "prb_let_joint",
                &let_args_7(l, w, t, n, steel_with_e(e)),
            ))
        };
        let k_base = k(length, width, thickness, n_base, e);

        // t³ scaling: double t → ×2³ = 8  (J = w·t³/3 ∝ t³).
        // 2·thickness = 0.001 m < length = 0.02 m — constraint satisfied.
        let ratio_t = k(length, width, 2.0 * thickness, n_base, e) / k_base;
        let rel_t = (ratio_t - 8.0).abs() / 8.0;
        assert!(rel_t < 1e-9, "LET t³ scaling: ratio {ratio_t} vs 8 (rel {rel_t})");

        // 1/L scaling: double L → ×1/2.
        let ratio_l = k(2.0 * length, width, thickness, n_base, e) / k_base;
        let rel_l = (ratio_l - 0.5).abs() / 0.5;
        assert!(rel_l < 1e-9, "LET 1/L scaling: ratio {ratio_l} vs 0.5 (rel {rel_l})");

        // n_blades linear: double n → ×2  (k = n·G·J/L ∝ n).
        let ratio_n = k(length, width, thickness, 2 * n_base, e) / k_base;
        let rel_n = (ratio_n - 2.0).abs() / 2.0;
        assert!(rel_n < 1e-9, "LET n_blades scaling: ratio {ratio_n} vs 2 (rel {rel_n})");

        // G(E) scaling: double E → G = E/(2(1+ν)) doubles (ν fixed) → ×2.
        let ratio_e = k(length, width, thickness, n_base, 2.0 * e) / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(rel_e < 1e-9, "LET E→G scaling: ratio {ratio_e} vs 2 (rel {rel_e})");
    }

    /// (c) Range branches for LET: torsional yield θ = σy·L/(√3·G·t).
    #[test]
    fn prb_let_joint_range_branches() {
        let prb_limit = 5.0_f64 * PI / 180.0;
        let e = 205e9_f64;
        let nu = 0.29_f64;
        let yield_stress = 310e6_f64;
        let g = e / (2.0 * (1.0 + nu));

        // (i) Yield-capped: short/thick so θ_yield < 5°.
        //   L=2mm, t=1mm →  θ_yield = 310e6·0.002/(√3·G·0.001) ≈ ?
        let l_short = 0.002_f64;
        let t_thick = 0.001_f64;
        let theta_yield_short = yield_stress * l_short / (3.0_f64.sqrt() * g * t_thick);
        assert!(
            theta_yield_short < prb_limit,
            "LET yield-capped fixture: θ_yield={theta_yield_short:.5} must be < 5°"
        );
        let result_yield =
            crate::eval_builtin("prb_let_joint", &let_args_7(l_short, 0.005, t_thick, 1, steel()));
        let (lo, up) = range_lower_upper(map_get(&result_yield, "range").unwrap());
        assert_angle_close(lo, -theta_yield_short, "LET yield-capped lower");
        assert_angle_close(up, theta_yield_short, "LET yield-capped upper");

        // (ii) PRB-capped: long/thin so θ_yield > 5°.
        let l_long = 0.02_f64;
        let t_thin = 0.0005_f64;
        let theta_yield_long = yield_stress * l_long / (3.0_f64.sqrt() * g * t_thin);
        assert!(
            theta_yield_long > prb_limit,
            "LET PRB-capped fixture: θ_yield={theta_yield_long:.5} must be > 5°"
        );
        let result_prb = crate::eval_builtin(
            "prb_let_joint",
            &let_args_7(l_long, 0.005, t_thin, 1, steel()),
        );
        let (lo_prb, up_prb) = range_lower_upper(map_get(&result_prb, "range").unwrap());
        assert_angle_close(lo_prb, -prb_limit, "LET PRB-capped lower");
        assert_angle_close(up_prb, prb_limit, "LET PRB-capped upper");

        // (iii) No-yield fallback → ±5°.
        let result_ny = crate::eval_builtin(
            "prb_let_joint",
            &let_args_7(l_long, 0.005, t_thin, 1, steel_no_yield()),
        );
        let (lo_ny, up_ny) = range_lower_upper(map_get(&result_ny, "range").unwrap());
        assert_angle_close(lo_ny, -prb_limit, "LET no-yield lower");
        assert_angle_close(up_ny, prb_limit, "LET no-yield upper");
    }

    /// (d) Neutral handling for LET: 7-arg default, 8-arg bare/Option(Some)/Option(None).
    #[test]
    fn prb_let_joint_neutral_angle_handling() {
        let two_deg = 2.0_f64 * PI / 180.0;
        let base = let_args_7(0.02, 0.005, 0.0005, 2, steel());

        let call_with_neutral = |n: Option<Value>| {
            let mut args = base.clone();
            if let Some(v) = n {
                args.push(v);
            }
            crate::eval_builtin("prb_let_joint", &args)
        };

        // 7-arg default → angle(0).
        let seven = call_with_neutral(None);
        assert_eq!(map_get(&seven, "neutral"), Some(&Value::angle(0.0)));

        // 8-arg bare angle.
        let eight = call_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(
            map_get(&eight, "neutral").unwrap(),
            two_deg,
            "LET 8-arg bare neutral",
        );

        // 8-arg Option(Some(angle)).
        let eight_opt = call_with_neutral(Some(Value::Option(Some(Box::new(
            Value::angle(two_deg),
        )))));
        assert_angle_close(
            map_get(&eight_opt, "neutral").unwrap(),
            two_deg,
            "LET 8-arg Option(Some) neutral",
        );

        // 8-arg Option(None) → angle(0).
        let eight_none = call_with_neutral(Some(Value::Option(None)));
        assert_eq!(
            map_get(&eight_none, "neutral"),
            Some(&Value::angle(0.0)),
            "LET 8-arg Option(None) → angle(0)"
        );
    }

    /// (e) Invalid-input rejection for LET → Undef.
    #[test]
    fn prb_let_joint_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_let_joint", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let base = let_args_7(0.02, 0.005, 0.0005, 2, steel());
        let with = |idx: usize, v: Value| {
            let mut a = base.clone();
            a[idx] = v;
            a
        };

        // Wrong arity: <7 or >9.
        undef(vec![], "let 0 args");
        undef(base[..4].to_vec(), "let 4 args");
        // Arity 9 (neutral + declared_range) is now VALID (step-18); 10 args
        // overflows the highest supported arity and is rejected.
        {
            let mut a = base.clone();
            a.push(Value::angle(0.0)); // neutral
            a.push(Value::angle(0.0)); // declared_range
            a.push(Value::angle(0.0)); // overflow
            undef(a, "let 10 args");
        }

        // Non-positive geometry.
        undef(with(0, Value::length(0.0)), "let length=0");
        undef(with(1, Value::length(0.0)), "let width=0");
        undef(with(2, Value::length(f64::NAN)), "let thickness=NaN");
        undef(with(2, Value::length(0.02)), "let thickness==length");

        // Bad n_blades.
        undef(with(3, Value::Int(0)), "let n_blades=0");
        undef(with(3, Value::Int(-1)), "let n_blades<0");
        undef(with(3, Value::Real(1.5)), "let n_blades non-integer");
        undef(with(3, Value::String("2".to_string())), "let n_blades string");

        // Bad material.
        undef(with(4, Value::Real(1.0)), "let material not StructureInstance");
        undef(
            with(4, material("NoModulus", &[])),
            "let material missing youngs_modulus",
        );
        // poisson_ratio out of range.
        undef(
            with(
                4,
                material(
                    "BadPoisson",
                    &[
                        (
                            "youngs_modulus",
                            Value::Scalar {
                                si_value: 205e9,
                                dimension: DimensionVector::PRESSURE,
                            },
                        ),
                        ("poisson_ratio", Value::Real(0.5)),
                    ],
                ),
            ),
            "let poisson=0.5 (out of [0,0.5))",
        );
        undef(
            with(
                4,
                material(
                    "NegPoisson",
                    &[
                        (
                            "youngs_modulus",
                            Value::Scalar {
                                si_value: 205e9,
                                dimension: DimensionVector::PRESSURE,
                            },
                        ),
                        ("poisson_ratio", Value::Real(-0.1)),
                    ],
                ),
            ),
            "let poisson<0",
        );
        // Missing poisson_ratio.
        undef(
            with(
                4,
                material(
                    "NoPoisson",
                    &[(
                        "youngs_modulus",
                        Value::Scalar {
                            si_value: 205e9,
                            dimension: DimensionVector::PRESSURE,
                        },
                    )],
                ),
            ),
            "let missing poisson_ratio",
        );

        // Bad axis.
        undef(with(6, Value::Real(1.0)), "let axis not a vector");
        undef(
            with(
                6,
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
            "let axis zero vector",
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // step-17: RED — hinge-family compliance population
    // ─────────────────────────────────────────────────────────────────────────

    /// Read the `__flexure_compliance` FlexureCompliance record's fields from a
    /// hinge flexure joint Map (panics if absent or the wrong shape). Local to
    /// this test module, mirroring beam.rs / prismatic.rs / notch.rs.
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

    /// Auto-endpoint + declared-range compliance assertions shared by the two
    /// bending-hinge ctors (living hinge, cross-spring pivot). Both use the
    /// cantilever angular surface stress σ = E·(t/2)·θ/L (common.rs
    /// `cantilever_sigma_at`), the algebraic inverse of their
    /// θ_yield = yield·L/(E·t/2). `k_gamma` pins the effective_stiffness.
    fn assert_bending_hinge_compliance(ctor_name: &str, k_gamma: f64) {
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let prb_limit = 5.0_f64 * PI / 180.0;
        let ten_deg = 10.0_f64 * PI / 180.0;
        let sigma_at =
            |theta: f64, length: f64, thickness: f64| e * (thickness / 2.0) * theta / length;

        // ── auto endpoint (6-arg, PRB-capped fixture L=20mm/t=0.5mm) ──────────
        // θ_yield ≈ 6.93° > 5°, so endpoint = ±5° and σ(5°) ≈ 224 MPa < yield ⇒
        // at_yield=false.
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0005_f64;
        let auto =
            crate::eval_builtin(ctor_name, &bending_hinge_args_6(length, width, thickness, steel()));
        let fields = compliance_fields(&auto);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("{ctor_name}: FlexureCompliance missing `{k}`"))
        };

        // effective_stiffness (Real) == spring_rate == k_gamma·E·I/L.
        let i = width * thickness.powi(3) / 12.0;
        let k_expected = k_gamma * e * i / length;
        let spring_si = spring_rate_si(&auto);
        match f("effective_stiffness") {
            Value::Real(rr) => assert!(
                (rr - spring_si).abs() / spring_si < 1e-12
                    && (rr - k_expected).abs() / k_expected < 1e-9,
                "{ctor_name}: effective_stiffness {rr} == spring_rate {spring_si} == k·EI/L {k_expected}"
            ),
            other => panic!("{ctor_name}: effective_stiffness Real, got {other:?}"),
        }

        // max_stress (PRESSURE) == σ(5°) at the auto PRB-capped endpoint, < yield.
        let expected_auto = sigma_at(prb_limit, length, thickness);
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "{ctor_name}: max_stress PRESSURE");
                assert!(
                    (si_value - expected_auto).abs() / expected_auto < 1e-9,
                    "{ctor_name}: auto max_stress {si_value} vs σ(5°) {expected_auto}"
                );
                assert!(*si_value < yield_si, "{ctor_name}: σ(5°) below yield ({si_value})");
            }
            other => panic!("{ctor_name}: max_stress Scalar, got {other:?}"),
        }
        assert_eq!(f("at_yield"), &Value::Bool(false), "{ctor_name}: auto endpoint not at yield");
        match f("prb_validity_range") {
            Value::Real(rr) => assert!(
                (rr - prb_limit).abs() / prb_limit < 1e-9,
                "{ctor_name}: prb_validity_range {rr} == 5°"
            ),
            other => panic!("{ctor_name}: prb_validity_range Real, got {other:?}"),
        }

        // ── declared ±10° on yielding geometry (t=0.05mm, L=2mm) ─────────────
        // σ(10°) ≈ 447 MPa > 310 yield. 8-arg layout:
        //   (length, width, thickness, material, pivot, axis, neutral, declared_range).
        let yl = 0.002_f64;
        let yt = 0.00005_f64;
        let yielding = crate::eval_builtin(
            ctor_name,
            &[
                Value::length(yl),
                Value::length(width),
                Value::length(yt),
                steel(),
                origin(),
                axis_y(),
                Value::angle(0.0),     // neutral
                Value::angle(ten_deg), // declared ±10° half-width
            ],
        );
        assert!(
            !yielding.is_undef(),
            "{ctor_name}: yielding declared-range call returns a joint, not Undef"
        );
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("revolute".to_string())),
            "{ctor_name}: yielding hinge is still a revolute joint"
        );
        let (lo, up) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert_angle_close(lo, -ten_deg, &format!("{ctor_name}: declared-range lower bound"));
        assert_angle_close(up, ten_deg, &format!("{ctor_name}: declared-range upper bound"));

        let yf = compliance_fields(&yielding);
        let yg = |k: &str| {
            yf.get(&k.to_string())
                .unwrap_or_else(|| panic!("{ctor_name}: missing `{k}`"))
        };
        let expected_y = sigma_at(ten_deg, yl, yt);
        assert!(
            expected_y > yield_si,
            "{ctor_name}: fixture sanity σ(10°)={expected_y} must exceed yield {yield_si}"
        );
        match yg("max_stress") {
            Value::Scalar { si_value, .. } => assert!(
                (si_value - expected_y).abs() / expected_y < 1e-9,
                "{ctor_name}: declared max_stress {si_value} vs σ(10°) {expected_y}"
            ),
            other => panic!("{ctor_name}: max_stress Scalar, got {other:?}"),
        }
        assert_eq!(yg("at_yield"), &Value::Bool(true), "{ctor_name}: declared 10° drives at_yield true");
        match yg("yield_margin") {
            Value::Real(rr) => assert!(*rr < 0.0, "{ctor_name}: yielding ⇒ negative margin, got {rr}"),
            other => panic!("{ctor_name}: yield_margin Real, got {other:?}"),
        }

        // prb_validity_range still advertises the auto SAFE bound, NOT the
        // declared 10°. For this short geometry θ_yield = yield·L/(E·t/2) ≈ 6.93°
        // > 5°, so the SAFE bound is the ±5° PRB cap (the declared 10° must not
        // leak into prb_validity_range).
        let theta_lim_short = (yield_si * yl / (e * yt / 2.0)).min(prb_limit);
        match yg("prb_validity_range") {
            Value::Real(rr) => assert!(
                (rr - theta_lim_short).abs() / theta_lim_short < 1e-9,
                "{ctor_name}: prb_validity_range stays auto SAFE bound {theta_lim_short}, got {rr}"
            ),
            other => panic!("{ctor_name}: prb_validity_range Real, got {other:?}"),
        }
    }

    #[test]
    fn prb_living_hinge_attaches_populated_compliance() {
        // γ_lh = 1.0 (Howell §5.7 SLFP).
        assert_bending_hinge_compliance("prb_living_hinge", 1.0);
    }

    #[test]
    fn prb_cross_spring_pivot_attaches_populated_compliance() {
        // γ_cs = 2.0 (Haringx 1949).
        assert_bending_hinge_compliance("prb_cross_spring_pivot", 2.0);
    }

    #[test]
    fn prb_let_joint_attaches_populated_compliance() {
        // LET joint (Jacobsen et al. 2009): torsional, governed by shear. The
        // cached max_stress is the von Mises tensile-equivalent of the torsional
        // surface stress σ_eq = √3·G·t·θ/L (the algebraic inverse of
        // θ_yield = σy·L/(√3·G·t)), so the at_yield check — which compares against
        // the material *tensile* yield σy — trips exactly at the LET joint's own
        // torsional yield rotation, consistent with the auto range cap.
        let e = 205e9_f64;
        let nu = 0.29_f64; // steel() poisson_ratio
        let g = e / (2.0 * (1.0 + nu));
        let yield_si = 310e6_f64;
        let prb_limit = 5.0_f64 * PI / 180.0;
        let ten_deg = 10.0_f64 * PI / 180.0;
        let vm_sigma_at =
            |theta: f64, length: f64, thickness: f64| 3.0_f64.sqrt() * g * thickness * theta / length;

        // ── auto endpoint (7-arg, L=20mm/t=0.3mm): θ_yield ≈ 8.6° > 5° ⇒
        // endpoint = ±5°, σ_eq(5°) ≈ 180 MPa < yield ⇒ at_yield=false. ──
        let length = 0.02_f64;
        let width = 0.005_f64;
        let thickness = 0.0003_f64;
        let auto =
            crate::eval_builtin("prb_let_joint", &let_args_7(length, width, thickness, 2, steel()));
        let fields = compliance_fields(&auto);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("LET: FlexureCompliance missing `{k}`"))
        };

        // effective_stiffness (Real) == spring_rate (k_θ = n·G·J/L).
        let spring_si = spring_rate_si(&auto);
        match f("effective_stiffness") {
            Value::Real(rr) => assert!(
                (rr - spring_si).abs() / spring_si < 1e-12,
                "LET: effective_stiffness {rr} == spring_rate {spring_si}"
            ),
            other => panic!("LET: effective_stiffness Real, got {other:?}"),
        }

        let expected_auto = vm_sigma_at(prb_limit, length, thickness);
        assert!(expected_auto < yield_si, "LET fixture sanity: σ_eq(5°)={expected_auto} < yield");
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "LET: max_stress PRESSURE");
                assert!(
                    (si_value - expected_auto).abs() / expected_auto < 1e-9,
                    "LET: auto max_stress {si_value} vs σ_eq(5°) {expected_auto}"
                );
            }
            other => panic!("LET: max_stress Scalar, got {other:?}"),
        }
        assert_eq!(f("at_yield"), &Value::Bool(false), "LET: auto endpoint not at yield");
        match f("prb_validity_range") {
            Value::Real(rr) => assert!(
                (rr - prb_limit).abs() / prb_limit < 1e-9,
                "LET: prb_validity_range {rr} == 5°"
            ),
            other => panic!("LET: prb_validity_range Real, got {other:?}"),
        }

        // ── declared ±10° (9-arg layout): σ_eq(10°) ≈ 360 MPa > yield. The 9-arg
        // layout is (length, width, thickness, n_blades, material, pivot, axis,
        // neutral, declared_range). ──
        let yielding = crate::eval_builtin(
            "prb_let_joint",
            &[
                Value::length(length),
                Value::length(width),
                Value::length(thickness),
                Value::Int(2),
                steel(),
                origin(),
                axis_y(),
                Value::angle(0.0),     // neutral
                Value::angle(ten_deg), // declared ±10° half-width
            ],
        );
        assert!(!yielding.is_undef(), "LET: declared-range call returns a joint, not Undef");
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("revolute".to_string())),
            "LET: yielding joint is still revolute"
        );
        let (lo, up) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert_angle_close(lo, -ten_deg, "LET declared-range lower bound");
        assert_angle_close(up, ten_deg, "LET declared-range upper bound");

        let yf = compliance_fields(&yielding);
        let yg = |k: &str| {
            yf.get(&k.to_string())
                .unwrap_or_else(|| panic!("LET: missing `{k}`"))
        };
        let expected_y = vm_sigma_at(ten_deg, length, thickness);
        assert!(expected_y > yield_si, "LET fixture sanity: σ_eq(10°)={expected_y} > yield");
        match yg("max_stress") {
            Value::Scalar { si_value, .. } => assert!(
                (si_value - expected_y).abs() / expected_y < 1e-9,
                "LET: declared max_stress {si_value} vs σ_eq(10°) {expected_y}"
            ),
            other => panic!("LET: max_stress Scalar, got {other:?}"),
        }
        assert_eq!(yg("at_yield"), &Value::Bool(true), "LET: declared 10° drives at_yield true");
        match yg("yield_margin") {
            Value::Real(rr) => assert!(*rr < 0.0, "LET: yielding ⇒ negative margin, got {rr}"),
            other => panic!("LET: yield_margin Real, got {other:?}"),
        }
    }
}
