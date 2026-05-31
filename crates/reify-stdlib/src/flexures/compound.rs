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

use super::common::{length_si, make_flexure_joint, material_field_si};

/// Fixed-guided stiffness coefficient γ_pp = 12 (Howell §5 / PRD §6.1 parallelogram
/// blade). Identical to `beam::FIXED_FIXED_GAMMA` — the same boundary condition.
const FIXED_GUIDED_GAMMA: f64 = 12.0;

/// Fallback transverse-displacement validity limit as a fraction of beam length,
/// used when the material carries no `yield_stress`.
const SMALL_DEFLECTION_FRACTION: f64 = 0.1;

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
/// Returns `None` (⇒ `Value::Undef`) on: arity ≠ 7; non-finite geometry or
/// blade_spacing; a material that is not a `Value::StructureInstance` with a
/// finite `youngs_modulus`; or a motion_axis that is not a finite, non-zero,
/// dimensionless 3-vector.
///
/// **Positivity / degeneracy guards are added in step-8** — this early version
/// intentionally accepts non-positive geometry to keep the RED→GREEN steps
/// incremental (step-7 tests that those guards are absent, step-8 adds them).
fn parse_compound_inputs(args: &[Value]) -> Option<CompoundInputs<'_>> {
    if args.len() != 7 {
        return None;
    }
    let length = length_si(&args[0])?;
    let width = length_si(&args[1])?;
    let thickness = length_si(&args[2])?;
    // blade_spacing validated for presence/finitude but does NOT enter §6.1/§6.2
    // closed forms — accepted for signature fidelity (PRD §6.1 prescribes it).
    let _blade_spacing = length_si(&args[3])?;
    let material = &args[4];
    let e = material_field_si(material, "youngs_modulus")?;
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

/// Compute the fixed-guided transverse displacement validity half-width δ_max.
///
/// Fixed-guided bending stress σ = 3·E·t·δ / L²
/// ⇒ δ_yield = yield·L²/(3·E·t).
/// No `yield_stress` ⇒ small-deflection fallback δ = 0.1·L.
fn delta_max(c: &CompoundInputs<'_>) -> f64 {
    match c.yield_si {
        Some(yield_si) => yield_si * c.length.powi(2) / (3.0 * c.e * c.thickness),
        None => SMALL_DEFLECTION_FRACTION * c.length,
    }
}

/// Evaluate a compound-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including `Some(Value::Undef)` on
/// validation failure) and `None` for any unknown name, so `eval_builtin` falls
/// through to the next module.
pub(crate) fn eval_compound(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "prb_parallelogram_flexure" => Some(prb_parallelogram_flexure(args)),
        _ => None,
    }
}

/// `prb_parallelogram_flexure(length, width, thickness, blade_spacing, material, motion_axis, pivot)`
/// — PRB parallelogram flexure stage presented as a prismatic joint.
///
/// Returns a joint `Value::Map` (`kind == "prismatic"`) with:
/// - `spring_rate` = k_stage = 48·E·I/L³ (TRANSLATIONAL_STIFFNESS)
/// - `transverse_stiffness` = 4·E·(b·t)/L (axial blade stretching)
/// - `range` = ±δ_max (symmetric LENGTH-bounded range)
///
/// Returns `Value::Undef` on the invalid-input classes in [`parse_compound_inputs`].
fn prb_parallelogram_flexure(args: &[Value]) -> Value {
    let Some(c) = parse_compound_inputs(args) else {
        return Value::Undef;
    };

    // Motion stiffness: k_blade = 12·E·I/L³ (fixed-guided, γ_pp=12), k_stage = 4 blades.
    let k_blade = FIXED_GUIDED_GAMMA * c.e * c.i / c.length.powi(3);
    let k_stage = 4.0 * k_blade;

    // Transverse (orthogonal DOF) stiffness: axial stretching of 4 blades.
    // k_transverse = 4·E·(b·t)/L;  ratio = k_transverse/k_stage = (L/t)².
    let k_transverse = 4.0 * c.e * (c.width * c.thickness) / c.length;

    // Validity range ±δ_max (symmetric LENGTH-bounded range).
    let delta = delta_max(&c);
    let range = Value::range(
        Some(Value::length(-delta)),
        Some(Value::length(delta)),
        true,
        true,
    );

    // Build the standard joint base then add the compound-specific extra keys.
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
    Value::Map(m)
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
}
