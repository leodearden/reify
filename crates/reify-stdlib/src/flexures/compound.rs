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

use reify_ir::Value;

/// Evaluate a compound-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including `Some(Value::Undef)` on
/// validation failure) and `None` for any unknown name, so `eval_builtin` falls
/// through to the next module.
pub(crate) fn eval_compound(_name: &str, _args: &[Value]) -> Option<Value> {
    None
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
}
