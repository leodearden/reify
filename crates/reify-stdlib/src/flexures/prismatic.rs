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

use reify_ir::Value;

/// Evaluate a prismatic-flexure constructor by name.
///
/// Returns `Some(Value)` for recognised names (including
/// `Some(Value::Undef)` on validation failure) and `None` for any unknown
/// name, so `eval_builtin` falls through to the next module.
pub(crate) fn eval_prismatic(_name: &str, _args: &[Value]) -> Option<Value> {
    None
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

    // ── step-1: RED — prb_prismatic_blade smoke + scaling ────────────────────

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
}
