//! Notch-flexure PRB constructors (Paros-Weisbord 1965, PRD §5.2):
//! circular, elliptical, and right-circular notch hinges — all → revolute.
//!
//! All three constructors share the positional argument layout
//! `(notch_radius, web_thickness, width, material, pivot, axis[, neutral[, declared_range]])`
//! and the [`parse_notch_inputs`] validation path. They differ only in the
//! dimensionless shape factors `k_factor` and `sigma_factor` passed to the
//! shared `notch_revolute` core (PRD §5.2 design decision).

use std::f64::consts::PI;

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    attach_compliance, length_si, make_compliance_record, make_flexure_joint, material_field_si,
    neutral_angle_si, parse_declared_range, symmetric_angle_range, RangeKind, PRB_ANGLE_LIMIT_RAD,
};

/// Shape factors for the standard circular notch flexure hinge
/// (Paros & Weisbord 1965, §5.2): κ = 1, k_σ = 1. All other notch variants
/// are normalised relative to this baseline.
const CIRCULAR_K: f64 = 1.0;
const CIRCULAR_SIGMA: f64 = 1.0;

/// Shape factors for the elliptical notch flexure hinge (Smith et al. 1997,
/// "Design of Elliptical Notch Flexure Hinges", Precision Engineering 20(3)).
/// For a 2:1 profile aspect ratio the elliptical hinge is more compliant than
/// the circular case; κ → 1 as the semi-axes converge to equal radii (circular
/// limit).
///
/// The stiffness shape factor (0.85) and stress shape factor (0.85) are set
/// equal as a first-order PRB approximation consistent with PRD §8.5 fidelity.
/// Physically, the same profile geometry that reduces rotational stiffness also
/// reduces peak surface stress through a comparable geometric attenuation;
/// decoupling them requires independently sourced values from the elliptic-
/// integral correction terms (Smith et al. 1997, Table 1). A future fidelity
/// pass (λ) may split them if dogfood demands greater accuracy.
const ELLIPTICAL_K: f64 = 0.85;
const ELLIPTICAL_SIGMA: f64 = 0.85;

/// Shape factors for the right-circular (toroidal / axisymmetric) notch
/// flexure hinge (Paros & Weisbord 1965, toroidal variant). The full
/// axisymmetric removal makes this the most compliant of the three profiles
/// for the same (r, t, b) geometry; κ → 1 in the planar limit.
///
/// The stiffness shape factor (0.74) and stress shape factor (0.74) are set
/// equal as a first-order PRB approximation consistent with PRD §8.5 fidelity.
/// The toroidal profile distributes bending over a wider arc, attenuating both
/// the effective rotational stiffness and the peak hinge-root stress through the
/// same geometric factor. Decoupling them would require the full axisymmetric
/// stress analysis from Paros-Weisbord's toroidal derivation; a future fidelity
/// pass (λ) may provide independently verified values.
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
    /// The optional trailing `neutral` argument (present in the 7- and 8-arg forms).
    neutral_arg: Option<&'a Value>,
    /// The optional trailing declared operating-range argument (present only in
    /// the 8-arg form). When present, its endpoint — not the auto cap — drives
    /// the joint range and the §5.3 `max_stress` stress-check (an ANGLE
    /// half-angle, since all three notch ctors are revolute).
    declared_range_arg: Option<&'a Value>,
}

/// Parse and validate the shared positional argument layout of all three
/// notch-flexure constructors: `(notch_radius, web_thickness, width,
/// material, pivot, axis[, neutral[, declared_range]])`.
///
/// Returns `None` (⇒ the caller returns `Value::Undef`) on: arity ∉ {6, 7, 8};
/// non-positive or non-finite geometry (r, t, or b ≤ 0); degenerate geometry
/// (t ≥ 2·r — web at least as thick as the notch diameter, the
/// E_FlexureGeometryInvalid regime whose diagnostic λ owns); a material that
/// is not a `Value::StructureInstance` with a finite `youngs_modulus` > 0; or
/// an axis that is not a finite, non-zero, dimensionless 3-vector.
///
/// δ emits NO diagnostics and returns Value::Undef on invalid input
/// (W_Flexure* emission is λ's responsibility).
fn parse_notch_inputs(args: &[Value]) -> Option<NotchInputs<'_>> {
    if args.len() < 6 || args.len() > 8 {
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
        neutral_arg: if args.len() >= 7 { Some(&args[6]) } else { None },
        declared_range_arg: if args.len() == 8 { Some(&args[7]) } else { None },
    })
}

/// Notch-hinge surface bending stress σ = sigma_factor·4·E·t·|θ| / (3π·(2r+t))
/// (Paros-Weisbord 1965, PRD §5.2) — the algebraic inverse of `notch_revolute`'s
/// `θ_yield = yield·3π·(2r+t) / (sigma_factor·4·E·t)`.
///
/// `theta` is the rotation (radians) at which to evaluate the stress; the
/// magnitude is used so the sign of the deflection does not matter. The
/// per-variant `sigma_factor` (κ_σ) flows through, so the elliptical (0.85) and
/// right-circular (0.74) profiles report their attenuated peak surface stress.
/// Module-local (notch-specific formula), mirroring `prismatic.rs`'s local
/// `cantilever_transverse_sigma_at`.
fn notch_sigma_at(theta: f64, radius: f64, thickness: f64, e: f64, sigma_factor: f64) -> f64 {
    sigma_factor * 4.0 * e * thickness * theta.abs() / (3.0 * PI * (2.0 * radius + thickness))
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
/// The auto symmetric prb_validity range is ±min(θ_yield, 5°) (no `yield_stress`
/// ⇒ ±5° fallback). An optional trailing `declared_range` (a half-angle)
/// OVERRIDES that cap for the joint range and the cached `max_stress` endpoint
/// (§5.3 evaluates surface stress at the declared endpoint); the auto cap is
/// retained as the SAFE/suggested `prb_validity_range` in the compliance record.
fn notch_revolute(inputs: &NotchInputs<'_>, k_factor: f64, sigma_factor: f64) -> Value {
    // Paros-Weisbord rotational stiffness (PRD §5.2).
    let k_theta = k_factor * 2.0 * inputs.e * inputs.b * inputs.t.powf(2.5)
        / (9.0 * PI * inputs.r.sqrt());

    // Auto symmetric prb_validity range = ±min(θ_yield, 5°) — the SAFE bound,
    // retained in the compliance record below regardless of any wider declared
    // range. θ_yield inverts the surface-yield stress σ(θ) (see `notch_sigma_at`).
    let theta_lim = match inputs.yield_si {
        Some(yield_si) => {
            let theta_yield = yield_si * 3.0 * PI * (2.0 * inputs.r + inputs.t)
                / (sigma_factor * 4.0 * inputs.e * inputs.t);
            theta_yield.min(PRB_ANGLE_LIMIT_RAD)
        }
        None => PRB_ANGLE_LIMIT_RAD,
    };

    // An optional user-declared operating range (±half-angle) OVERRIDES the auto
    // ±min(θ_yield, 5°) cap for the joint range and the §5.3 stress endpoint; the
    // auto θ_lim is retained as the SAFE/suggested range in the compliance record.
    let declared = parse_declared_range(inputs.declared_range_arg, RangeKind::Angle);
    let range_endpoint = declared.unwrap_or(theta_lim);
    let range = symmetric_angle_range(range_endpoint);

    // Optional trailing neutral angle (default 0 for the 6-arg form).
    let neutral_si = inputs.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    let joint = make_flexure_joint(
        "revolute",
        inputs.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_theta,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        inputs.pivot.clone(),
    );

    // Cache the FlexureCompliance record (§5.3): Paros-Weisbord surface bending
    // stress at the range endpoint (declared when present, else the auto θ_lim —
    // the worst-case operating stress) and at the neutral rest angle. The
    // sigma_factor flows into both, so each variant reports its own peak stress.
    // prb_validity_range stores the auto SAFE θ_lim regardless of any wider
    // declared range, so it always advertises the PRB-valid bound.
    let max_stress = notch_sigma_at(range_endpoint, inputs.r, inputs.t, inputs.e, sigma_factor);
    let max_stress_at_neutral =
        notch_sigma_at(neutral_si, inputs.r, inputs.t, inputs.e, sigma_factor);
    let record = make_compliance_record(
        k_theta,
        max_stress,
        max_stress_at_neutral,
        inputs.yield_si,
        None,
        theta_lim,
    );
    attach_compliance(joint, record)
}

/// `prb_notch_circular(notch_radius, web_thickness, width, material, pivot, axis[, neutral[, declared_range]])`
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

/// `prb_notch_elliptical(notch_radius, web_thickness, width, material, pivot, axis[, neutral[, declared_range]])`
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

/// `prb_notch_right_circular(notch_radius, web_thickness, width, material, pivot, axis[, neutral[, declared_range]])`
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
    use reify_ir::Value;
    use std::f64::consts::PI;
    use super::super::test_util::*;

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
        // Arity 8 (neutral + declared_range) is now VALID (step-16); 9 args
        // overflows the highest supported arity and is rejected.
        {
            let mut a = base_args();
            a.push(Value::angle(0.0)); // neutral
            a.push(Value::angle(0.0)); // declared_range
            a.push(Value::angle(0.0)); // overflow
            undef(a, "9 args");
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

    // ── steps 11 + 13: elliptical + right-circular (parametrized) ──────────

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

    /// Parametrized verification suite shared by the two non-circular variants.
    ///
    /// Covers five assertion classes:
    ///  1. Structure: revolute kind, damping=None, axis/neutral preserved, k dim.
    ///  2. Absolute spring_rate pin: k == k_factor · 2·E·b·t^2.5 / (9π·r^0.5)
    ///     to relative 1e-9 — pins the named constant AND its application site in
    ///     notch_revolute. (Swapping or mis-scaling the constant fails here.)
    ///  3. Yield-capped range with non-unit sigma_factor: asserts range bounds equal
    ///     ±theta_yield computed with the variant's sigma_factor to 1e-9, exercising
    ///     the non-trivial sigma path in notch_revolute for both variants.
    ///  4. Coefficient-independent scaling ratios (t^2.5, r^-0.5, b, E) to 1e-9.
    ///  5. Rejection spot-checks (degenerate t≥2r, 0 args → Undef).
    ///
    /// `k_factor` and `sigma_factor` are passed as hard-coded numeric literals at
    /// each call site so any change to the named constants breaks assertion 2 or 3.
    fn assert_notch_variant_full<F>(
        ctor_name: &str,
        k_factor: f64,
        sigma_factor: f64,
        call_fn: F,
    ) where
        F: Fn(f64, f64, f64, Value) -> Value,
    {
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;
        // material_with_e always carries yield_stress = 310 MPa (hardcoded).
        let yield_stress = 310e6_f64;

        let base = call_fn(r, t, b, steel_with_e(e));

        // 1. Structure assertions.
        assert_eq!(
            map_get(&base, "kind"),
            Some(&Value::String("revolute".to_string())),
            "{ctor_name}: must present as revolute"
        );
        assert_eq!(
            map_get(&base, "damping"),
            Some(&Value::Option(None)),
            "{ctor_name}: damping is None in δ scope"
        );
        assert_eq!(
            map_get(&base, "axis"),
            Some(&axis_y()),
            "{ctor_name}: axis preserved verbatim"
        );
        assert_eq!(
            map_get(&base, "neutral"),
            Some(&Value::angle(0.0)),
            "{ctor_name}: neutral defaults to angle(0)"
        );
        let k_base = spring_rate_si(&base);
        assert!(
            k_base.is_finite() && k_base > 0.0,
            "{ctor_name}: spring_rate must be finite positive"
        );
        assert_eq!(
            map_get(&base, "spring_rate").map(|v| match v {
                Value::Scalar { dimension, .. } => *dimension,
                _ => panic!("{ctor_name}: spring_rate must be a Scalar"),
            }),
            Some(DimensionVector::ROTATIONAL_STIFFNESS),
            "{ctor_name}: spring_rate carries ROTATIONAL_STIFFNESS"
        );

        // 2. Absolute spring_rate pin: k = k_factor · 2·E·b·t^2.5 / (9π·r^0.5).
        // Literal k_factor pins both the constant value and its wiring in notch_revolute:
        // any change to ELLIPTICAL_K/RIGHT_CIRCULAR_K will fail here.
        let k_circular_form = 2.0 * e * b * t.powf(2.5) / (9.0 * PI * r.sqrt());
        let k_expected = k_factor * k_circular_form;
        let rel_k = (k_base - k_expected).abs() / k_expected;
        assert!(
            rel_k < 1e-9,
            "{ctor_name}: absolute spring_rate {k_base} vs \
             k_factor({k_factor})·k_circ({k_circular_form}) = {k_expected} (rel {rel_k})"
        );

        // 3. Yield-capped range with non-unit sigma_factor.
        // theta_yield = yield · 3π · (2r+t) / (sigma_factor · 4·E·t)  — PRD §5.2.
        // For the standard fixture, theta_yield < 5° (yield-capped, not PRB-capped).
        // material_with_e carries yield_stress = 310 MPa, matching `yield_stress` above.
        let theta_yield =
            yield_stress * 3.0 * PI * (2.0 * r + t) / (sigma_factor * 4.0 * e * t);
        let prb_limit = 5.0_f64 * PI / 180.0;
        assert!(
            theta_yield < prb_limit,
            "{ctor_name}: fixture must be yield-capped \
             (θ_yield={theta_yield:.4} >= 5°={prb_limit:.4})"
        );
        let range = map_get(&base, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        assert_angle_close(
            lo,
            -theta_yield,
            &format!("{ctor_name}: yield-capped lower bound (sigma_factor={sigma_factor})"),
        );
        assert_angle_close(
            up,
            theta_yield,
            &format!("{ctor_name}: yield-capped upper bound (sigma_factor={sigma_factor})"),
        );

        // 4. Coefficient-independent functional-form scaling (κ cancels in ratios).

        // t^2.5 scaling: doubling t → ×2^2.5
        let k_2t = spring_rate_si(&call_fn(r, 2.0 * t, b, steel_with_e(e)));
        let ratio_t = k_2t / k_base;
        let expected_t = 2.0_f64.powf(2.5);
        let rel_t = (ratio_t - expected_t).abs() / expected_t;
        assert!(
            rel_t < 1e-9,
            "{ctor_name}: t^2.5 scaling: ratio {ratio_t} vs {expected_t} (rel {rel_t})"
        );

        // r^-0.5 scaling: doubling r → ×2^-0.5
        let k_2r = spring_rate_si(&call_fn(2.0 * r, t, b, steel_with_e(e)));
        let ratio_r = k_2r / k_base;
        let expected_r = 2.0_f64.powf(-0.5);
        let rel_r = (ratio_r - expected_r).abs() / expected_r;
        assert!(
            rel_r < 1e-9,
            "{ctor_name}: r^-0.5 scaling: ratio {ratio_r} vs {expected_r} (rel {rel_r})"
        );

        // b scaling: doubling b → ×2
        let k_2b = spring_rate_si(&call_fn(r, t, 2.0 * b, steel_with_e(e)));
        let ratio_b = k_2b / k_base;
        let rel_b = (ratio_b - 2.0).abs() / 2.0;
        assert!(
            rel_b < 1e-9,
            "{ctor_name}: b linear scaling: ratio {ratio_b} vs 2 (rel {rel_b})"
        );

        // E scaling: doubling E → ×2
        let k_2e = spring_rate_si(&call_fn(r, t, b, steel_with_e(2.0 * e)));
        let ratio_e = k_2e / k_base;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(
            rel_e < 1e-9,
            "{ctor_name}: E linear scaling: ratio {ratio_e} vs 2 (rel {rel_e})"
        );

        // 5. Rejection spot-checks.
        let bad_t = crate::eval_builtin(
            ctor_name,
            &[
                Value::length(r),
                Value::length(2.0 * r), // t ≥ 2r → degenerate geometry
                Value::length(b),
                steel_with_e(e),
                origin(),
                axis_y(),
            ],
        );
        assert!(bad_t.is_undef(), "{ctor_name}: degenerate t≥2r → Undef");

        let bad_arity = crate::eval_builtin(ctor_name, &[]);
        assert!(bad_arity.is_undef(), "{ctor_name}: 0 args → Undef");
    }

    #[test]
    fn prb_notch_elliptical_structure_and_scaling() {
        // k_factor=0.85 and sigma_factor=0.85 are hard-coded literals that pin
        // ELLIPTICAL_K and ELLIPTICAL_SIGMA respectively (see assertion classes 2+3).
        assert_notch_variant_full("prb_notch_elliptical", 0.85, 0.85, notch_elliptical);
    }

    // ── step-13: right-circular structure + functional-form scaling ──────────

    #[test]
    fn prb_notch_right_circular_structure_and_scaling() {
        // k_factor=0.74 and sigma_factor=0.74 are hard-coded literals that pin
        // RIGHT_CIRCULAR_K and RIGHT_CIRCULAR_SIGMA respectively (see assertion classes 2+3).
        assert_notch_variant_full("prb_notch_right_circular", 0.74, 0.74, notch_right_circular);
    }

    // ── step-15: RED — notch-family compliance population ────────────────────

    /// Read the `__flexure_compliance` FlexureCompliance record's fields from a
    /// notch flexure joint Map (panics if absent or the wrong shape). Local to
    /// this test module, mirroring beam.rs / prismatic.rs `compliance_fields`.
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

    /// Closed-form Paros-Weisbord surface bending stress at rotation `theta`
    /// (PRD §5.2): σ = sigma_factor·4·E·t·θ / (3π·(2r+t)). The algebraic inverse
    /// of the θ_yield that caps the auto prb_validity range.
    fn notch_sigma(theta: f64, r: f64, t: f64, e: f64, sigma_factor: f64) -> f64 {
        sigma_factor * 4.0 * e * t * theta / (3.0 * PI * (2.0 * r + t))
    }

    #[test]
    fn prb_notch_circular_attaches_populated_compliance() {
        // PRB-capped fixture (r=2mm, t=0.1mm): θ_yield ≈ 8.37° > 5°, so the auto
        // prb_validity endpoint is the ±5° PRB cap and σ(5°) ≈ 185 MPa < 310 MPa
        // yield ⇒ at_yield == false. (The standard base fixture r=1mm/t=0.2mm has
        // θ_yield ≈ 2.25° < 5°, whose auto endpoint sits *exactly* at yield —
        // unusable for a clean at_yield==false pin.)
        let r = 2e-3_f64;
        let t = 1e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;
        let prb_limit = 5.0_f64 * PI / 180.0;

        let result = crate::eval_builtin(
            "prb_notch_circular",
            &[
                Value::length(r),
                Value::length(t),
                Value::length(b),
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
        let spring_si = spring_rate_si(&result);
        match f("effective_stiffness") {
            Value::Real(rr) => assert!(
                (rr - spring_si).abs() / spring_si < 1e-12,
                "effective_stiffness {rr} == spring_rate {spring_si}"
            ),
            other => panic!("effective_stiffness Real, got {other:?}"),
        }

        // max_stress (PRESSURE) == σ(5°) at the auto PRB-capped endpoint.
        let expected_sigma = notch_sigma(prb_limit, r, t, e, 1.0);
        match f("max_stress") {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::PRESSURE, "max_stress is PRESSURE");
                assert!(
                    (si_value - expected_sigma).abs() / expected_sigma < 1e-9,
                    "max_stress {si_value} vs analytic σ(5°) {expected_sigma}"
                );
                assert!(*si_value < 310e6, "5° endpoint stays below the 310MPa yield ({si_value})");
            }
            other => panic!("max_stress Scalar, got {other:?}"),
        }

        // at_yield == false at the auto (safe) endpoint.
        assert_eq!(f("at_yield"), &Value::Bool(false), "auto endpoint is not at yield");

        // prb_validity_range is now Range<Angle> = [−θ_lim, +θ_lim] (task 4576).
        let (_, up) = range_lower_upper(map_get(&result, "range").expect("range present"));
        let range_half = match up {
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!("range upper Scalar, got {other:?}"),
        };
        let prb_half = angle_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half - range_half).abs() / range_half < 1e-9
                && (prb_half - prb_limit).abs() / prb_limit < 1e-9,
            "prb_validity_range half {prb_half} == range half {range_half} == 5°"
        );
    }

    #[test]
    fn prb_notch_circular_declared_range_override() {
        // Base fixture r=1mm, t=0.2mm: θ_yield ≈ 2.25° (yield-capped). A declared
        // ±10° operating range drives σ(10°) ≈ 1.38 GPa ≫ 310 MPa yield. Arg
        // layout: (r, t, width, material, pivot, axis, neutral, declared_range) —
        // declared_range is the new highest-arity (8th) slot, mirroring the beam
        // and prismatic families.
        let r = 1e-3_f64;
        let t = 2e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;
        let yield_si = 310e6_f64;
        let ten_deg = 10.0_f64 * PI / 180.0;
        // Auto SAFE bound θ_yield (sigma_factor = 1 for circular), < 5°.
        let theta_yield = yield_si * 3.0 * PI * (2.0 * r + t) / (4.0 * e * t);

        let yielding = crate::eval_builtin(
            "prb_notch_circular",
            &[
                Value::length(r),
                Value::length(t),
                Value::length(b),
                steel(),
                origin(),
                axis_y(),
                Value::angle(0.0),     // neutral
                Value::angle(ten_deg), // declared ±10° half-width
            ],
        );

        // The ctor STILL returns a valid revolute joint (not Undef) even though
        // the declared range drives surface stress past yield.
        assert!(!yielding.is_undef(), "yielding declared-range call returns a joint, not Undef");
        assert_eq!(
            map_get(&yielding, "kind"),
            Some(&Value::String("revolute".to_string())),
            "yielding notch flexure is still a revolute joint"
        );

        // (a) The joint `range` is the declared ±10°, OVERRIDING the auto cap.
        let (lo, up) = range_lower_upper(map_get(&yielding, "range").expect("range present"));
        assert_angle_close(lo, -ten_deg, "declared-range lower bound");
        assert_angle_close(up, ten_deg, "declared-range upper bound");

        let fields = compliance_fields(&yielding);
        let f = |k: &str| {
            fields
                .get(&k.to_string())
                .unwrap_or_else(|| panic!("FlexureCompliance missing `{k}`"))
        };

        // (b) max_stress is evaluated at the DECLARED 10° endpoint.
        let expected_sigma = notch_sigma(ten_deg, r, t, e, 1.0);
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
        assert_eq!(f("at_yield"), &Value::Bool(true), "declared 10° drives at_yield true");
        match f("yield_margin") {
            Value::Real(rr) => assert!(*rr < 0.0, "yielding ⇒ negative margin, got {rr}"),
            other => panic!("yield_margin Real, got {other:?}"),
        }

        // prb_validity_range still advertises the auto SAFE θ_yield (not the declared
        // 10°). Now Range<Angle> (task 4576).
        let prb_half = angle_range_half_si(f("prb_validity_range"), "prb_validity_range");
        assert!(
            (prb_half - theta_yield).abs() / theta_yield < 1e-9,
            "prb_validity_range stays the auto safe θ_yield {theta_yield}, got half {prb_half}"
        );
    }

    #[test]
    fn prb_notch_variants_attach_populated_compliance() {
        // The shared notch_revolute core wires the compliance record for all three
        // variants; the variant's sigma_factor flows into max_stress. PRB-capped
        // fixture (r=2mm, t=0.1mm) ⇒ auto endpoint = ±5° (θ_yield > 5° for both,
        // since sigma_factor < 1 only widens θ_yield), σ(5°) < yield ⇒
        // at_yield=false for every variant.
        let r = 2e-3_f64;
        let t = 1e-4_f64;
        let b = 5e-3_f64;
        let e = 205e9_f64;
        let prb_limit = 5.0_f64 * PI / 180.0;

        for (ctor, sigma_factor) in [
            ("prb_notch_elliptical", 0.85_f64),
            ("prb_notch_right_circular", 0.74_f64),
        ] {
            let result = crate::eval_builtin(
                ctor,
                &[
                    Value::length(r),
                    Value::length(t),
                    Value::length(b),
                    steel(),
                    origin(),
                    axis_y(),
                ],
            );
            let fields = compliance_fields(&result);
            let f = |k: &str| {
                fields
                    .get(&k.to_string())
                    .unwrap_or_else(|| panic!("{ctor}: FlexureCompliance missing `{k}`"))
            };

            // max_stress carries the variant's sigma_factor (σ = κ_σ·4·E·t·θ/(3π·(2r+t))).
            let expected_sigma = notch_sigma(prb_limit, r, t, e, sigma_factor);
            match f("max_stress") {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::PRESSURE,
                        "{ctor}: max_stress is PRESSURE"
                    );
                    assert!(
                        (si_value - expected_sigma).abs() / expected_sigma < 1e-9,
                        "{ctor}: max_stress {si_value} vs analytic {expected_sigma} \
                         (sigma_factor={sigma_factor})"
                    );
                    assert!(*si_value < 310e6, "{ctor}: σ(5°) stays below yield ({si_value})");
                }
                other => panic!("{ctor}: max_stress Scalar, got {other:?}"),
            }

            // at_yield false at the safe endpoint.
            assert_eq!(f("at_yield"), &Value::Bool(false), "{ctor}: safe endpoint not at yield");

            // effective_stiffness matches the joint spring_rate.
            let spring_si = spring_rate_si(&result);
            match f("effective_stiffness") {
                Value::Real(rr) => assert!(
                    (rr - spring_si).abs() / spring_si < 1e-12,
                    "{ctor}: effective_stiffness {rr} == spring_rate {spring_si}"
                ),
                other => panic!("{ctor}: effective_stiffness Real, got {other:?}"),
            }
        }
    }
}
