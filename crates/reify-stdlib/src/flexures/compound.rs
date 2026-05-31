//! Compound-flexure PRB constructors: parallelogram and double-parallelogram
//! stages (Compliant-Joints PRD §6.1/§6.2).
//!
//! ## Physical model
//!
//! A parallelogram flexure stage consists of four fixed-guided blades (γ_pp = 12,
//! Howell §5 / PRD §6.1) arranged in two pairs, constraining a moving platform
//! to translate along the motion axis. Because the blades are fixed-guided
//! (both ends remain oriented), the stiffness model is identical to
//! `beam::prb_fixed_fixed_beam`.
//!
//! ### Parasitic error — Roberts approximation (PRD §6.1)
//! A translating parallelogram stage exhibits a second-order vertical (parasitic)
//! displacement modelled by the Roberts-approximation arc:
//!   δ_rot = L·(1 − cos(δ_max/L))
//!
//! ### Mirror-cancellation in the double stage (PRD §6.2)
//! Two single stages in mirror-symmetric series cancel the first-order parasitic
//! term; the residual scales as (δ/L)³ instead of (δ/L):
//!   δ_rot_double = δ_rot_single · (δ_max/L)²

use std::collections::BTreeMap;

use reify_core::DimensionVector;
use reify_ir::Value;

use super::common::{
    fixed_guided_delta_max, length_si, make_flexure_joint, material_field_si, neutral_angle_si,
    symmetric_angle_range, CANTILEVER_GAMMA, FIXED_GUIDED_GAMMA, PRB_ANGLE_LIMIT_RAD,
};

/// Shared validated inputs for compound-flexure constructors.
///
/// Argument layout: `(length, width, thickness, blade_spacing, material, motion_axis, pivot)`
/// — note that `blade_spacing` is arg[3] (differs from beam ctors where material is arg[3]).
struct CompoundInputs<'a> {
    /// Blade length L (metres).
    length: f64,
    /// Blade width b (metres) — in-plane dimension.
    width: f64,
    /// Blade thickness t (metres) — bending direction.
    thickness: f64,
    /// Young's modulus E (Pa).
    e: f64,
    /// Rectangular-section second moment of area `I = width·thickness³/12`.
    i: f64,
    /// Material yield stress σ_y (Pa), if the material carries one.
    yield_si: Option<f64>,
    /// The raw motion-axis argument (stored verbatim on the joint Map).
    axis: &'a Value,
    /// The raw pivot argument (stored verbatim on the joint Map).
    pivot: &'a Value,
}

/// Parse and validate the shared positional argument layout of both
/// compound-flexure constructors:
/// `(length, width, thickness, blade_spacing, material, motion_axis, pivot)`.
///
/// Returns `None` (⇒ `Value::Undef`) on: arity ≠ 7; non-positive or non-finite
/// geometry (including `blade_spacing`); thickness ≥ length; a material that is
/// not a `Value::StructureInstance` with a finite positive `youngs_modulus`; or a
/// motion_axis that is not a finite, non-zero, dimensionless 3-vector.
fn parse_compound_inputs(args: &[Value]) -> Option<CompoundInputs<'_>> {
    if args.len() != 7 {
        return None;
    }
    let length = length_si(&args[0])?;
    let width = length_si(&args[1])?;
    let thickness = length_si(&args[2])?;
    // `_blade_spacing`: validated (positive, finite, LENGTH) for signature fidelity
    // (PRD §6.1 prescribes the argument) but intentionally does NOT enter the
    // §6.1/§6.2 closed forms — stiffness and parasitic depend only on L,b,t,E.
    let _blade_spacing = length_si(&args[3])?;
    if length <= 0.0 || width <= 0.0 || thickness <= 0.0 || _blade_spacing <= 0.0 {
        return None;
    }
    if thickness >= length {
        return None;
    }
    let material = &args[4];
    let e = material_field_si(material, "youngs_modulus")?;
    if e <= 0.0 {
        return None;
    }
    let axis = &args[5];
    crate::helpers::validate_dimensionless_unit_axis_vec3(axis)?;
    Some(CompoundInputs {
        length,
        width,
        thickness,
        e,
        i: width * thickness.powi(3) / 12.0,
        yield_si: material_field_si(material, "yield_stress"),
        axis,
        pivot: &args[6],
    })
}

/// Shared builder for both compound-flexure constructors.
///
/// `series_divisor` is 1.0 for the single-stage parallelogram (no series
/// composition) and 2.0 for the double-parallelogram (two stages in series →
/// stiffness halves for both DOFs).
///
/// `parasitic_of(delta_rot_single, delta, length)` maps the single-stage
/// Roberts-approximation arc height to the final `parasitic_error` value:
/// - Single stage: identity — return `delta_rot_single` unchanged.
/// - Double stage: multiply by `(δ/L)²` (mirror-cancellation residual, PRD §6.2).
fn build_compound_joint<F>(c: &CompoundInputs<'_>, series_divisor: f64, parasitic_of: F) -> Value
where
    F: Fn(f64, f64, f64) -> f64,
{
    let k_blade = FIXED_GUIDED_GAMMA * c.e * c.i / c.length.powi(3);
    let k_stage = 4.0 * k_blade / series_divisor;
    let k_transverse = 4.0 * c.e * (c.width * c.thickness) / c.length / series_divisor;

    let delta = fixed_guided_delta_max(c.length, c.thickness, c.e, c.yield_si);
    let range = Value::range(
        Some(Value::length(-delta)),
        Some(Value::length(delta)),
        true,
        true,
    );

    let delta_rot_single = c.length * (1.0 - (delta / c.length).cos());
    let delta_rot = parasitic_of(delta_rot_single, delta, c.length);

    let base = make_flexure_joint(
        "prismatic",
        c.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_stage,
            dimension: DimensionVector::TRANSLATIONAL_STIFFNESS,
        },
        Value::length(0.0),
        c.pivot.clone(),
    );
    let mut m: BTreeMap<Value, Value> = match base {
        Value::Map(m) => m,
        _ => unreachable!("make_flexure_joint always returns a Map"),
    };
    m.insert(
        Value::String("transverse_stiffness".to_string()),
        Value::Scalar {
            si_value: k_transverse,
            dimension: DimensionVector::TRANSLATIONAL_STIFFNESS,
        },
    );
    m.insert(
        Value::String("parasitic_error".to_string()),
        Value::Option(Some(Box::new(Value::length(delta_rot)))),
    );
    Value::Map(m)
}

/// Evaluate a compound-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including `Some(Value::Undef)` on
/// validation failure) and `None` for any unknown name, so `eval_builtin` falls
/// through to the next module.
pub(crate) fn eval_compound(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_parallelogram_flexure" => Some(prb_parallelogram_flexure(args)),
        "prb_double_parallelogram_flexure" => Some(prb_double_parallelogram_flexure(args)),
        "prb_cartwheel_flexure" => Some(prb_cartwheel_flexure(args)),
        _ => None,
    }
}

/// Validated inputs for the cartwheel flexure constructor.
///
/// Argument layout (§6.3): `(blade_count:Int, blade_length, blade_width,
/// blade_thickness, material, pivot, axis[, neutral])` — note that blade_count
/// is arg[0], pivot is arg[5] BEFORE axis at arg[6] (matches the beam/hinge
/// family, not the compound parallelogram layout). A new dedicated parser is
/// required because the layouts differ structurally.
struct CartwheelInputs<'a> {
    /// Number of radial blades N (≥ 1), stored as f64 for arithmetic.
    blade_count: f64,
    /// Blade length L (metres).
    length: f64,
    /// Blade thickness t in the bending direction (metres).
    /// Required for the surface-yield rotation θ_yield = yield·L/(E·t/2).
    thickness: f64,
    /// Blade second moment of area `I = width·thickness³/12` (m⁴).
    i: f64,
    /// Young's modulus E (Pa).
    e: f64,
    /// Material yield stress σ_y (Pa), if the material carries one.
    yield_si: Option<f64>,
    /// The raw axis argument (stored verbatim on the joint Map).
    axis: &'a Value,
    /// The raw pivot argument (stored verbatim on the joint Map).
    pivot: &'a Value,
    /// Optional trailing `neutral` argument (present only in the 8-arg form).
    neutral_arg: Option<&'a Value>,
}

/// Parse and validate the cartwheel flexure argument layout:
/// `(blade_count:Int, blade_length, blade_width, blade_thickness,
/// material, pivot, axis[, neutral])`.
///
/// Returns `None` (⇒ `Value::Undef`) on: arity ∉ {7, 8}; blade_count not a
/// recognised numeric type; non-positive or non-finite geometry; thickness ≥
/// length; a material without a finite positive `youngs_modulus`; or an axis
/// that is not a finite, non-zero, dimensionless 3-vector.
///
/// Note: the strict blade_count ≥ 1 and integer-only guards are added in step-6;
/// this initial version accepts any finite numeric blade_count.
fn parse_cartwheel_inputs(args: &[Value]) -> Option<CartwheelInputs<'_>> {
    if args.len() != 7 && args.len() != 8 {
        return None;
    }
    // Strict blade_count: Int ≥ 1, or a whole finite Real ≥ 1.
    // Mirrors parse_let_inputs n_blades arm (hinge.rs).
    let blade_count = match &args[0] {
        Value::Int(n) if *n >= 1 => *n as f64,
        Value::Real(r) if r.is_finite() && *r >= 1.0 && r.fract() == 0.0 => *r,
        _ => return None,
    };
    let length = length_si(&args[1])?;
    let width = length_si(&args[2])?;
    let thickness = length_si(&args[3])?;
    if length <= 0.0 || width <= 0.0 || thickness <= 0.0 || thickness >= length {
        return None;
    }
    let material = &args[4];
    let e = material_field_si(material, "youngs_modulus")?;
    if e <= 0.0 {
        return None;
    }
    crate::helpers::validate_dimensionless_unit_axis_vec3(&args[6])?;
    Some(CartwheelInputs {
        blade_count,
        length,
        thickness,
        i: width * thickness.powi(3) / 12.0,
        e,
        yield_si: material_field_si(material, "yield_stress"),
        axis: &args[6],
        pivot: &args[5],
        neutral_arg: if args.len() == 8 { Some(&args[7]) } else { None },
    })
}

/// `prb_cartwheel_flexure(blade_count, blade_length, blade_width,
/// blade_thickness, material, pivot, axis[, neutral])` — PRB cartwheel flexure
/// presented as a revolute joint (Compliant-Joints PRD §6.3).
///
/// Returns a joint `Value::Map` (`kind == "revolute"`) whose rotational
/// stiffness is `k_θ = N · γ · E · I / L` (§6.3, γ = 2.65 Howell §5.1
/// cantilever coefficient, N = blade_count, I = width·thickness³/12).
///
/// The symmetric `prb_validity` rotation range is `±min(θ_yield, 5°)`, where
/// `θ_yield = yield·L/(E·t/2)` is the cantilever surface-yield rotation
/// (each radial blade is a cantilever — the cartwheel rotation equals the
/// per-blade rotation). When the material carries no `yield_stress`, only
/// the 5° PRB limit applies.
///
/// Returns `Value::Undef` on the invalid-input classes enumerated in
/// [`parse_cartwheel_inputs`].
fn prb_cartwheel_flexure(args: &[Value]) -> Value {
    let Some(c) = parse_cartwheel_inputs(args) else {
        return Value::Undef;
    };

    // PRB pivot stiffness: k_pivot = N · γ · E · I / L (§6.3).
    // Each blade contributes k_blade = γ·E·I/L (Howell §5.1 cantilever).
    let k_pivot = c.blade_count * CANTILEVER_GAMMA * c.e * c.i / c.length;

    // Symmetric prb_validity range = ±min(θ_yield, 5°). Each radial blade is a
    // cantilever; the cartwheel rotation equals the per-blade rotation, so the
    // cantilever surface-yield rotation IS the pivot's yield-limited range:
    //   θ_yield = yield·L / (E·t/2)  (Howell §5.1 cantilever surface-yield).
    // A material without yield_stress contributes only the 5° PRB cap.
    let theta_lim = match c.yield_si {
        Some(y) => (y * c.length / (c.e * c.thickness / 2.0)).min(PRB_ANGLE_LIMIT_RAD),
        None => PRB_ANGLE_LIMIT_RAD,
    };
    let range = symmetric_angle_range(theta_lim);

    // Optional trailing neutral angle (default 0 for the 7-arg form; step-8
    // wires the 8-arg form — neutral_arg is already populated by the parser).
    let neutral_si = c.neutral_arg.map(neutral_angle_si).unwrap_or(0.0);

    make_flexure_joint(
        "revolute",
        c.axis.clone(),
        range,
        Value::Scalar {
            si_value: k_pivot,
            dimension: DimensionVector::ROTATIONAL_STIFFNESS,
        },
        Value::angle(neutral_si),
        c.pivot.clone(),
    )
}

/// `prb_parallelogram_flexure(length, width, thickness, blade_spacing, material, motion_axis, pivot)`
/// — PRB parallelogram flexure stage presented as a prismatic joint.
///
/// Returns a joint `Value::Map` (`kind == "prismatic"`) with:
/// - `spring_rate` = k_stage = 48·E·I/L³ (TRANSLATIONAL_STIFFNESS)
/// - `transverse_stiffness` = 4·E·(b·t)/L (axial blade stretching)
/// - `range` = ±δ_max (symmetric LENGTH-bounded range)
/// - `parasitic_error` = Option(Some(Length(δ_rot))) where δ_rot = L·(1−cos(δ_max/L))
///
/// Returns `Value::Undef` on the invalid-input classes in [`parse_compound_inputs`].
fn prb_parallelogram_flexure(args: &[Value]) -> Value {
    let Some(c) = parse_compound_inputs(args) else {
        return Value::Undef;
    };
    build_compound_joint(&c, 1.0, |delta_rot_single, _delta, _length| delta_rot_single)
}

/// `prb_double_parallelogram_flexure(length, width, thickness, blade_spacing, material, motion_axis, pivot)`
/// — PRB double-parallelogram flexure stage: two single stages in mirror-symmetric series.
///
/// Mirror symmetry cancels the first-order Roberts-approximation parasitic error,
/// leaving a residual that scales as (δ/L)³ instead of (δ/L) (PRD §6.2).
///
/// Returns a joint `Value::Map` (`kind == "prismatic"`) with:
/// - `spring_rate` = k_stage/2 = 24·E·I/L³ (series composition halves)
/// - `transverse_stiffness` = (4·E·(b·t)/L)/2
/// - `range` = ±δ_max (same per-stage range as single, for apples-to-apples §10.1 comparison)
/// - `parasitic_error` = Option(Some(Length(δ_rot_double))) (added in step-12)
///
/// Returns `Value::Undef` on the same invalid-input classes as [`prb_parallelogram_flexure`].
fn prb_double_parallelogram_flexure(args: &[Value]) -> Value {
    let Some(c) = parse_compound_inputs(args) else {
        return Value::Undef;
    };
    // Mirror symmetry cancels the first-order parasitic; residual scales as (δ/L)³
    // instead of (δ/L) — reduction factor (δ_max/L)² (PRD §6.2).
    build_compound_joint(&c, 2.0, |delta_rot_single, delta, length| {
        delta_rot_single * (delta / length).powi(2)
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    /// Standard 7-arg argument list for compound-flexure constructors:
    /// (length, width, thickness, blade_spacing, material, motion_axis, pivot).
    /// L=20mm, b=5mm, t=0.5mm, blade_spacing=10mm, Steel_AISI_1045.
    fn compound_args() -> Vec<Value> {
        vec![
            Value::length(0.02),   // L = 20 mm
            Value::length(0.005),  // b = 5 mm
            Value::length(0.0005), // t = 0.5 mm
            Value::length(0.010),  // blade_spacing = 10 mm
            steel(),
            axis_y(),
            origin(),
        ]
    }

    // ── step-1: RED -- prb_parallelogram_flexure structure + motion stiffness ──

    #[test]
    fn prb_parallelogram_flexure_structure_and_spring_rate() {
        let result = crate::eval_builtin("prb_parallelogram_flexure", &compound_args());

        // kind == "prismatic"
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("prismatic".to_string())),
            "parallelogram flexure presents as a prismatic joint; got {result:?}"
        );

        // axis is preserved verbatim
        assert_eq!(
            map_get(&result, "axis"),
            Some(&axis_y()),
            "axis is preserved verbatim"
        );

        // pivot is preserved verbatim
        assert_eq!(
            map_get(&result, "pivot"),
            Some(&origin()),
            "pivot is preserved verbatim"
        );

        // damping == Value::Option(None)
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is None in gamma scope"
        );

        // spring_rate: TRANSLATIONAL_STIFFNESS, si_value == 4*12*E*I/L^3 (k_stage)
        let length = 0.02_f64;
        let width  = 0.005_f64;
        let thick  = 0.0005_f64;
        let e      = 205e9_f64;
        let i = width * thick.powi(3) / 12.0;
        // k_stage = 4 blades × k_blade, k_blade = 12·E·I/L³
        let k_expected = 4.0 * 12.0 * e * i / length.powi(3);
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

        // range: both-bounded, LENGTH-dimensioned, symmetric, non-zero
        let range = map_get(&result, "range").expect("range key present");
        let (lo, up) = range_lower_upper(range);
        let lo_si = match lo {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "range lower has LENGTH dimension");
                *si_value
            }
            other => panic!("range lower: expected Scalar, got {other:?}"),
        };
        let up_si = match up {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(*dimension, DimensionVector::LENGTH, "range upper has LENGTH dimension");
                *si_value
            }
            other => panic!("range upper: expected Scalar, got {other:?}"),
        };
        assert!(lo_si.is_finite() && up_si.is_finite(), "range bounds finite");
        assert!(up_si > 0.0, "range upper > 0");
        let sym = (lo_si + up_si).abs() / up_si.abs();
        assert!(sym < 1e-9, "range symmetric: lower {lo_si} == -upper {up_si}");
    }

    // ── step-3: RED -- transverse stiffness + ratio ────────────────────────────

    #[test]
    fn prb_parallelogram_flexure_transverse_stiffness_and_ratio() {
        let result = crate::eval_builtin("prb_parallelogram_flexure", &compound_args());

        let length = 0.02_f64;
        let width  = 0.005_f64;
        let thick  = 0.0005_f64;
        let e      = 205e9_f64;

        // k_transverse = 4·E·(b·t)/L  (axial stretching of 4 blades)
        let k_transverse_expected = 4.0 * e * (width * thick) / length;
        match map_get(&result, "transverse_stiffness") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::TRANSLATIONAL_STIFFNESS,
                    "transverse_stiffness carries TRANSLATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_transverse_expected).abs() / k_transverse_expected;
                assert!(
                    rel < 1e-9,
                    "transverse_stiffness {si_value} vs {k_transverse_expected} (rel {rel})"
                );
            }
            other => panic!("expected transverse_stiffness Scalar, got {other:?}"),
        }

        // ratio = k_transverse / spring_rate = (L/t)² ≥ 1000 for fixture (L/t=40 → 1600)
        let k_stage_expected = 4.0 * 12.0 * e * (width * thick.powi(3) / 12.0) / length.powi(3);
        let ratio_expected = (length / thick).powi(2); // (L/t)² = 1600
        let ratio_actual = k_transverse_expected / k_stage_expected;
        let rel_ratio = (ratio_actual - ratio_expected).abs() / ratio_expected;
        assert!(
            rel_ratio < 1e-9,
            "ratio = k_transverse/spring_rate = {ratio_actual} vs (L/t)^2 = {ratio_expected} (rel {rel_ratio})"
        );
        assert!(
            ratio_actual >= 1000.0,
            "ratio {ratio_actual} >= 1000 (§10.1 row 3)"
        );
    }

    // ── step-5: RED -- parasitic_error closed form + range magnitude ───────────

    #[test]
    fn prb_parallelogram_flexure_parasitic_error_and_range_magnitude() {
        let length = 0.02_f64;
        let width  = 0.005_f64;
        let thick  = 0.0005_f64;
        let e      = 205e9_f64;
        let yield_si = 310e6_f64;

        // ── Case 1: steel() with yield_stress ─────────────────────────────────
        let result = crate::eval_builtin("prb_parallelogram_flexure", &compound_args());

        // δ_max = yield·L²/(3·E·t)  (fixed-guided surface-yield deflection)
        let delta_max_val = yield_si * length.powi(2) / (3.0 * e * thick);
        // δ_rot = L·(1 − cos(δ_max/L))  (Roberts approximation)
        let theta = delta_max_val / length;
        let delta_rot_expected = length * (1.0 - theta.cos());

        match map_get(&result, "parasitic_error") {
            Some(Value::Option(Some(inner))) => {
                match inner.as_ref() {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "parasitic_error inner carries LENGTH dimension"
                        );
                        let rel = (si_value - delta_rot_expected).abs() / delta_rot_expected;
                        assert!(
                            rel < 1e-9,
                            "parasitic_error {si_value} vs {delta_rot_expected} (rel {rel})"
                        );
                        // §10.1 row 3: parasitic < L/1000
                        assert!(
                            *si_value < length / 1000.0,
                            "parasitic_error {si_value} < L/1000 = {} (§10.1 row 3)",
                            length / 1000.0
                        );
                    }
                    other => panic!("parasitic_error inner: expected Scalar, got {other:?}"),
                }
            }
            other => panic!("expected parasitic_error Option(Some(Scalar)), got {other:?}"),
        }

        // Range magnitude pin: upper == δ_max (yield branch)
        let range = map_get(&result, "range").expect("range key present");
        let (_, up) = range_lower_upper(range);
        match up {
            Value::Scalar { si_value, .. } => {
                let rel = (si_value - delta_max_val).abs() / delta_max_val;
                assert!(
                    rel < 1e-9,
                    "range upper {si_value} vs δ_max {delta_max_val} (rel {rel})"
                );
            }
            other => panic!("range upper: expected Scalar, got {other:?}"),
        }

        // ── Case 2: steel_no_yield() → δ_max = 0.1·L, parasitic uses that ─────
        let args_ny = vec![
            Value::length(length),
            Value::length(width),
            Value::length(thick),
            Value::length(0.010),
            steel_no_yield(),
            axis_y(),
            origin(),
        ];
        let result_ny = crate::eval_builtin("prb_parallelogram_flexure", &args_ny);
        let delta_fallback = 0.1 * length;
        let theta_ny = delta_fallback / length;
        let delta_rot_ny = length * (1.0 - theta_ny.cos());
        match map_get(&result_ny, "parasitic_error") {
            Some(Value::Option(Some(inner))) => {
                match inner.as_ref() {
                    Value::Scalar { si_value, .. } => {
                        let rel = (si_value - delta_rot_ny).abs() / delta_rot_ny;
                        assert!(
                            rel < 1e-9,
                            "no-yield parasitic {si_value} vs {delta_rot_ny} (rel {rel})"
                        );
                    }
                    other => panic!("no-yield parasitic inner: expected Scalar, got {other:?}"),
                }
            }
            other => panic!("no-yield: expected parasitic_error Option(Some(_)), got {other:?}"),
        }
    }

    // ── step-7: RED -- invalid inputs => Value::Undef ─────────────────────────

    #[test]
    fn prb_parallelogram_flexure_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_parallelogram_flexure", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let with = |idx: usize, v: Value| {
            let mut a = compound_args();
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        {
            let mut a = compound_args();
            a.truncate(6); // 6 args instead of 7
            undef(a, "6 args");
        }
        {
            let mut a = compound_args();
            a.push(Value::length(0.0)); // 8 args
            undef(a, "8 args");
        }

        // Non-positive geometry (these FAIL before step-8 guards are added):
        undef(with(0, Value::length(0.0)), "length = 0");
        undef(with(0, Value::length(-0.02)), "length < 0");
        undef(with(1, Value::length(0.0)), "width = 0");
        undef(with(3, Value::length(0.0)), "blade_spacing = 0");
        undef(with(3, Value::length(-0.01)), "blade_spacing < 0");

        // NaN (non-finite) geometry — length_si rejects NaN already:
        undef(with(2, Value::length(f64::NAN)), "thickness = NaN");

        // Degenerate beam: thickness >= length (FAILS before step-8):
        undef(with(2, Value::length(0.02)), "thickness == length");
        undef(
            {
                let mut a = compound_args();
                a[0] = Value::length(0.005); // length < thickness=0.0005 would be fine, but...
                a[2] = Value::length(0.010); // thickness > length
                a
            },
            "thickness > length",
        );

        // Bad material.
        undef(with(4, Value::Real(1.0)), "material not a StructureInstance");
        undef(with(4, material("NoModulus", &[])), "material missing youngs_modulus");

        // Bad axis (args[5]).
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

    // ── θ/step-1: RED -- prb_cartwheel_flexure structure + spring rate ──────────

    /// 7-arg cartwheel argument list:
    /// (blade_count:Int, L, b, t, material, pivot, axis)
    fn cartwheel_args(blade_count: i64) -> Vec<Value> {
        vec![
            Value::Int(blade_count), // args[0] = N
            Value::length(0.02),     // args[1] = L = 20 mm
            Value::length(0.005),    // args[2] = b = 5 mm
            Value::length(0.0005),   // args[3] = t = 0.5 mm
            steel(),                 // args[4] = material
            origin(),                // args[5] = pivot
            axis_y(),                // args[6] = axis
        ]
    }

    #[test]
    fn prb_cartwheel_flexure_structure_and_spring_rate() {
        let n = 4_i64;
        let result = crate::eval_builtin("prb_cartwheel_flexure", &cartwheel_args(n));

        // kind == "revolute"
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("revolute".to_string())),
            "cartwheel flexure presents as a revolute joint; got {result:?}"
        );

        // axis and pivot are preserved verbatim
        assert_eq!(map_get(&result, "axis"), Some(&axis_y()), "axis preserved verbatim");
        assert_eq!(map_get(&result, "pivot"), Some(&origin()), "pivot preserved verbatim");

        // damping == Value::Option(None) (γ-scope)
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is None in γ scope"
        );

        // spring_rate: ROTATIONAL_STIFFNESS, si_value == N·γ·E·I/L (§6.3 k_pivot = N·k_blade)
        let l: f64 = 0.02;
        let b: f64 = 0.005;
        let t: f64 = 0.0005;
        let e: f64 = 205e9;
        let i = b * t.powi(3) / 12.0;
        let gamma: f64 = 2.65;
        let k_blade = gamma * e * i / l;
        let k_pivot_expected = n as f64 * k_blade;

        match map_get(&result, "spring_rate") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ROTATIONAL_STIFFNESS,
                    "spring_rate carries ROTATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_pivot_expected).abs() / k_pivot_expected;
                assert!(
                    rel < 1e-9,
                    "spring_rate {si_value} vs {k_pivot_expected} (rel {rel})"
                );
                // Pin the k_pivot = N·k_blade law (§6.3)
                let rel_kb = (si_value - n as f64 * k_blade).abs() / k_pivot_expected;
                assert!(
                    rel_kb < 1e-9,
                    "spring_rate must equal N×k_blade: {si_value} vs {} (rel {rel_kb})",
                    n as f64 * k_blade
                );
            }
            other => panic!("expected spring_rate Scalar, got {other:?}"),
        }

        // Coefficient-independent scaling checks:

        // (a) Linear blade-count scaling: k(N=8) / k(N=4) == 2
        let k8 = spring_rate_si(&crate::eval_builtin("prb_cartwheel_flexure", &cartwheel_args(8)));
        let k4 = spring_rate_si(&crate::eval_builtin("prb_cartwheel_flexure", &cartwheel_args(4)));
        let ratio_n = k8 / k4;
        let rel_n = (ratio_n - 2.0).abs() / 2.0;
        assert!(rel_n < 1e-9, "k(N=8)/k(N=4) should be 2, got {ratio_n} (rel {rel_n})");

        // (b) Double thickness → ×8 (I ∝ t³)
        let thick_args: Vec<Value> = vec![
            Value::Int(4),
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.001), // 2×t
            steel(),
            origin(),
            axis_y(),
        ];
        let k_thick = spring_rate_si(&crate::eval_builtin("prb_cartwheel_flexure", &thick_args));
        let ratio_t = k_thick / k4;
        let rel_t = (ratio_t - 8.0).abs() / 8.0;
        assert!(rel_t < 1e-9, "doubling t should give ×8 stiffness, got ×{ratio_t} (rel {rel_t})");

        // (c) Double length → ×0.5 (k ∝ 1/L)
        let long_args: Vec<Value> = vec![
            Value::Int(4),
            Value::length(0.04), // 2×L
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ];
        let k_long = spring_rate_si(&crate::eval_builtin("prb_cartwheel_flexure", &long_args));
        let ratio_l = k_long / k4;
        let rel_l = (ratio_l - 0.5).abs() / 0.5;
        assert!(rel_l < 1e-9, "doubling L should give ×0.5 stiffness, got ×{ratio_l} (rel {rel_l})");

        // (d) Double E → ×2
        let e_args: Vec<Value> = vec![
            Value::Int(4),
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel_with_e(2.0 * e),
            origin(),
            axis_y(),
        ];
        let k_2e = spring_rate_si(&crate::eval_builtin("prb_cartwheel_flexure", &e_args));
        let ratio_e = k_2e / k4;
        let rel_e = (ratio_e - 2.0).abs() / 2.0;
        assert!(rel_e < 1e-9, "doubling E should give ×2 stiffness, got ×{ratio_e} (rel {rel_e})");
    }

    // ── θ/step-3: RED -- prb_cartwheel_flexure range branches ───────────────────

    #[test]
    fn prb_cartwheel_flexure_range_branches() {
        let prb_limit = 5.0_f64 * std::f64::consts::PI / 180.0;
        let e: f64 = 205e9;
        let yield_stress: f64 = 310e6;

        // (a) Yield-capped: L=5mm, t=0.5mm → θ_yield = yield·L/(E·t/2) < 5°
        let l_a: f64 = 0.005;
        let t_a: f64 = 0.0005;
        let theta_yield_a = yield_stress * l_a / (e * t_a / 2.0);
        assert!(
            theta_yield_a < prb_limit,
            "fixture (a) must have θ_yield < 5°: θ_yield={theta_yield_a:.4} rad ≈ {}°",
            theta_yield_a * 180.0 / std::f64::consts::PI
        );
        let result_a = crate::eval_builtin(
            "prb_cartwheel_flexure",
            &[
                Value::Int(4),
                Value::length(l_a),
                Value::length(0.005),
                Value::length(t_a),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        let range_a = map_get(&result_a, "range").expect("range key present (a)");
        let (lo_a, up_a) = range_lower_upper(range_a);
        assert_angle_close(lo_a, -theta_yield_a, "yield-capped lower (a)");
        assert_angle_close(up_a, theta_yield_a, "yield-capped upper (a)");

        // (b) PRB-capped: L=20mm, t=0.5mm → θ_yield ≈ 6.93° > 5° → range == ±5°
        let l_b: f64 = 0.02;
        let t_b: f64 = 0.0005;
        let theta_yield_b = yield_stress * l_b / (e * t_b / 2.0);
        assert!(
            theta_yield_b > prb_limit,
            "fixture (b) must have θ_yield > 5°: θ_yield={theta_yield_b:.4} rad ≈ {}°",
            theta_yield_b * 180.0 / std::f64::consts::PI
        );
        let result_b = crate::eval_builtin("prb_cartwheel_flexure", &cartwheel_args(4));
        let range_b = map_get(&result_b, "range").expect("range key present (b)");
        let (lo_b, up_b) = range_lower_upper(range_b);
        assert_angle_close(lo_b, -prb_limit, "prb-limited lower (b)");
        assert_angle_close(up_b, prb_limit, "prb-limited upper (b)");

        // (c) No yield_stress → range == ±5° PRB cap
        let result_c = crate::eval_builtin(
            "prb_cartwheel_flexure",
            &[
                Value::Int(4),
                Value::length(0.02),
                Value::length(0.005),
                Value::length(0.0005),
                steel_no_yield(),
                origin(),
                axis_y(),
            ],
        );
        let range_c = map_get(&result_c, "range").expect("range key present (c)");
        let (lo_c, up_c) = range_lower_upper(range_c);
        assert_angle_close(lo_c, -prb_limit, "no-yield lower (c)");
        assert_angle_close(up_c, prb_limit, "no-yield upper (c)");
    }

    // ── θ/step-5: RED -- prb_cartwheel_flexure rejects invalid inputs ────────────

    #[test]
    fn prb_cartwheel_flexure_rejects_invalid_inputs() {
        let undef = |args: Vec<Value>, label: &str| {
            let r = crate::eval_builtin("prb_cartwheel_flexure", &args);
            assert!(r.is_undef(), "{label}: expected Undef, got {r:?}");
        };
        let with = |idx: usize, v: Value| {
            let mut a = cartwheel_args(4);
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        {
            let mut a = cartwheel_args(4);
            a.truncate(6); // 6 args
            undef(a, "6 args");
        }
        {
            let mut a = cartwheel_args(4);
            a.push(Value::angle(0.0));
            a.push(Value::angle(0.0)); // 9 args
            undef(a, "9 args");
        }

        // Invalid blade_count.
        undef(with(0, Value::Int(0)), "blade_count = 0");
        undef(with(0, Value::Int(-1)), "blade_count = -1");
        undef(with(0, Value::Real(1.5)), "blade_count = 1.5 (non-integer Real)");
        undef(with(0, Value::String("4".to_string())), "blade_count not numeric");

        // Non-positive / non-finite geometry.
        undef(with(1, Value::length(0.0)), "length = 0");
        undef(with(1, Value::length(-0.02)), "length < 0");
        undef(with(2, Value::length(0.0)), "width = 0");
        undef(with(3, Value::length(f64::NAN)), "thickness = NaN");

        // Degenerate beam: thickness >= length.
        undef(with(3, Value::length(0.02)), "thickness == length");
        undef(
            {
                let mut a = cartwheel_args(4);
                a[3] = Value::length(0.03); // thickness > L=0.02
                a
            },
            "thickness > length",
        );

        // Bad material.
        undef(with(4, Value::Real(1.0)), "material not a StructureInstance");
        undef(with(4, material("NoModulus", &[])), "material missing youngs_modulus");

        // Bad axis (args[6]).
        undef(with(6, Value::Real(1.0)), "axis not a vector");
        undef(
            with(
                6,
                Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
            "axis is zero vector",
        );
        undef(
            with(
                6,
                Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(1.0),
                    Value::length(0.0),
                ]),
            ),
            "axis is length-dimensioned",
        );
    }

    // ── θ/step-7: RED -- prb_cartwheel_flexure neutral angle handling ────────────

    /// Invoke `prb_cartwheel_flexure` on the base 7-arg fixture, optionally
    /// appending an 8th `neutral` argument.
    fn cartwheel_with_neutral(neutral: Option<Value>) -> Value {
        let mut args = cartwheel_args(4);
        if let Some(n) = neutral {
            args.push(n);
        }
        crate::eval_builtin("prb_cartwheel_flexure", &args)
    }

    #[test]
    fn prb_cartwheel_flexure_neutral_angle_handling() {
        let two_deg = 2.0_f64 * std::f64::consts::PI / 180.0;

        // (a) 7-arg → neutral defaults to angle(0).
        let seven = cartwheel_with_neutral(None);
        assert_eq!(
            map_get(&seven, "neutral"),
            Some(&Value::angle(0.0)),
            "7-arg call defaults neutral to angle(0)"
        );

        // (b) 8-arg bare angle(2°) → neutral == angle(2°).
        let eight_bare = cartwheel_with_neutral(Some(Value::angle(two_deg)));
        assert_angle_close(
            map_get(&eight_bare, "neutral").expect("neutral key present (b)"),
            two_deg,
            "8-arg bare-angle neutral",
        );

        // (c) 8-arg Option(Some(angle(2°))) → unwraps to angle(2°).
        let eight_opt =
            cartwheel_with_neutral(Some(Value::Option(Some(Box::new(Value::angle(two_deg))))));
        assert_angle_close(
            map_get(&eight_opt, "neutral").expect("neutral key present (c)"),
            two_deg,
            "8-arg optional-angle neutral",
        );

        // (d) 8-arg Option(None) → neutral defaults to angle(0).
        let eight_none = cartwheel_with_neutral(Some(Value::Option(None)));
        assert_eq!(
            map_get(&eight_none, "neutral"),
            Some(&Value::angle(0.0)),
            "8-arg Option(None) neutral defaults to angle(0)"
        );
    }

    // ── step-9: RED -- prb_double_parallelogram_flexure series stiffness ───────

    #[test]
    fn prb_double_parallelogram_flexure_structure_and_series_stiffness() {
        let result = crate::eval_builtin("prb_double_parallelogram_flexure", &compound_args());

        // kind == "prismatic"
        assert_eq!(
            map_get(&result, "kind"),
            Some(&Value::String("prismatic".to_string())),
            "double parallelogram presents as a prismatic joint; got {result:?}"
        );

        // axis, pivot, damping
        assert_eq!(map_get(&result, "axis"), Some(&axis_y()), "axis preserved");
        assert_eq!(map_get(&result, "pivot"), Some(&origin()), "pivot preserved");
        assert_eq!(
            map_get(&result, "damping"),
            Some(&Value::Option(None)),
            "damping is None"
        );

        let length = 0.02_f64;
        let width  = 0.005_f64;
        let thick  = 0.0005_f64;
        let e      = 205e9_f64;
        let i = width * thick.powi(3) / 12.0;

        // Series stiffness halves: spring_rate = k_stage/2 = 24·E·I/L³
        let k_expected = 2.0 * 12.0 * e * i / length.powi(3); // = 24*E*I/L^3
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

        // Transverse stiffness also halves: (4·E·b·t/L)/2
        let k_transverse_expected = 2.0 * e * (width * thick) / length; // = (4*E*b*t/L)/2
        match map_get(&result, "transverse_stiffness") {
            Some(Value::Scalar { si_value, dimension }) => {
                assert_eq!(
                    *dimension,
                    DimensionVector::TRANSLATIONAL_STIFFNESS,
                    "transverse_stiffness carries TRANSLATIONAL_STIFFNESS"
                );
                let rel = (si_value - k_transverse_expected).abs() / k_transverse_expected;
                assert!(
                    rel < 1e-9,
                    "transverse_stiffness {si_value} vs {k_transverse_expected} (rel {rel})"
                );
            }
            other => panic!("expected transverse_stiffness Scalar, got {other:?}"),
        }

        // Smoke: invalid call (6 args) => Undef
        {
            let mut a = compound_args();
            a.truncate(6);
            let r = crate::eval_builtin("prb_double_parallelogram_flexure", &a);
            assert!(r.is_undef(), "6-arg double call => Undef");
        }
    }

    // ── step-11: RED -- double-parallelogram parasitic residual ───────────────

    #[test]
    fn prb_double_parallelogram_flexure_parasitic_residual() {
        let result = crate::eval_builtin("prb_double_parallelogram_flexure", &compound_args());

        let length   = 0.02_f64;
        let thick    = 0.0005_f64;
        let e        = 205e9_f64;
        let yield_si = 310e6_f64;

        // Single-stage: δ_max = yield·L²/(3·E·t), δ_rot_single = L·(1−cos(δ_max/L))
        let delta_max_val = yield_si * length.powi(2) / (3.0 * e * thick);
        let theta = delta_max_val / length;
        let delta_rot_single = length * (1.0 - theta.cos());

        // Double-stage residual: δ_rot_double = δ_rot_single · (δ_max/L)²
        let delta_rot_double_expected = delta_rot_single * (delta_max_val / length).powi(2);

        match map_get(&result, "parasitic_error") {
            Some(Value::Option(Some(inner))) => {
                match inner.as_ref() {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "double parasitic_error carries LENGTH dimension"
                        );
                        let rel = (si_value - delta_rot_double_expected).abs()
                            / delta_rot_double_expected;
                        assert!(
                            rel < 1e-9,
                            "double parasitic_error {si_value} vs {delta_rot_double_expected} (rel {rel})"
                        );
                        // §10.1 row 4: parasitic < L/100000
                        assert!(
                            *si_value < length / 100_000.0,
                            "double parasitic {si_value} < L/100000 = {} (§10.1 row 4)",
                            length / 100_000.0
                        );
                        // Reduction factor: δ_rot_double / δ_rot_single == (δ_max/L)²
                        // (mirror-cancellation claim made precise).
                        let reduction = *si_value / delta_rot_single;
                        let expected_reduction = (delta_max_val / length).powi(2);
                        let rel_rf = (reduction - expected_reduction).abs() / expected_reduction;
                        assert!(
                            rel_rf < 1e-9,
                            "reduction factor {reduction} vs (δ_max/L)² = {expected_reduction} (rel {rel_rf})"
                        );
                    }
                    other => panic!("double parasitic inner: expected Scalar, got {other:?}"),
                }
            }
            other => {
                panic!("expected double parasitic_error Option(Some(Scalar)), got {other:?}")
            }
        }

        // ── Case 2: steel_no_yield() → δ_max = 0.1·L fallback ───────────────
        let width = 0.005_f64;
        let args_ny = vec![
            Value::length(length),
            Value::length(width),
            Value::length(thick),
            Value::length(0.010),
            steel_no_yield(),
            axis_y(),
            origin(),
        ];
        let result_ny = crate::eval_builtin("prb_double_parallelogram_flexure", &args_ny);
        let delta_fallback = 0.1 * length;
        let delta_rot_single_ny = length * (1.0 - (delta_fallback / length).cos());
        let delta_rot_double_ny = delta_rot_single_ny * (delta_fallback / length).powi(2);
        match map_get(&result_ny, "parasitic_error") {
            Some(Value::Option(Some(inner))) => match inner.as_ref() {
                Value::Scalar { si_value, .. } => {
                    let rel = (si_value - delta_rot_double_ny).abs() / delta_rot_double_ny;
                    assert!(
                        rel < 1e-9,
                        "no-yield double parasitic {si_value} vs {delta_rot_double_ny} (rel {rel})"
                    );
                }
                other => panic!("no-yield double parasitic inner: expected Scalar, got {other:?}"),
            },
            other => {
                panic!("no-yield: expected double parasitic Option(Some(_)), got {other:?}")
            }
        }
    }
}
