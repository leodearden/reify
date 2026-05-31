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

use reify_ir::Value;

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

/// `prb_living_hinge(length, width, thickness, material, pivot, axis[, neutral])`
/// — Howell §5.7 small-length flexural pivot (SLFP) as a revolute joint.
///
/// Stub — returns `Value::Undef` until step-2 implements the closed form.
fn prb_living_hinge(_args: &[Value]) -> Value {
    Value::Undef
}

/// `prb_cross_spring_pivot(length, width, thickness, material, pivot, axis[, neutral])`
/// — Haringx 1949 crossed-leaf pivot as a revolute joint.
///
/// Stub — returns `Value::Undef` until step-4 implements the closed form.
fn prb_cross_spring_pivot(_args: &[Value]) -> Value {
    Value::Undef
}

/// `prb_let_joint(length, width, thickness, n_blades, material, pivot, axis[, neutral])`
/// — Jacobsen et al. 2009 lamina-emergent torsion (multi-blade torsion) as a
/// revolute joint.
///
/// Stub — returns `Value::Undef` until step-6 implements the closed form.
fn prb_let_joint(_args: &[Value]) -> Value {
    Value::Undef
}

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};
    use std::f64::consts::PI;

    // ── Fixtures ─────────────────────────────────────────────────────────────

    /// Build a `Value::StructureInstance` material fixture from `(name, value)`
    /// field pairs (mirrors the SIR-α constructor pattern in reify-ir value.rs).
    fn material(name: &str, pairs: &[(&str, Value)]) -> Value {
        let fields: PersistentMap<String, Value> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: name.to_string(),
            version: 1,
            fields,
        }))
    }

    /// `Steel_AISI_1045`-like fixture: E = 205 GPa, yield = 310 MPa (PRESSURE),
    /// poisson_ratio = 0.29 (bare Real, for LET G derivation).
    fn steel() -> Value {
        material(
            "Steel_AISI_1045",
            &[
                (
                    "youngs_modulus",
                    Value::Scalar {
                        si_value: 205e9,
                        dimension: DimensionVector::PRESSURE,
                    },
                ),
                (
                    "yield_stress",
                    Value::Option(Some(Box::new(Value::Scalar {
                        si_value: 310e6,
                        dimension: DimensionVector::PRESSURE,
                    }))),
                ),
                // poisson_ratio stored as bare Real (the runtime representation
                // for ElasticMaterial::poisson_ratio : Real ∈ [0, 0.5)).
                ("poisson_ratio", Value::Real(0.29)),
            ],
        )
    }

    /// Like [`steel`] but carrying only `youngs_modulus` (no `yield_stress`),
    /// to exercise the no-yield fallback branch.
    fn steel_no_yield() -> Value {
        material(
            "Steel_NoYield",
            &[
                (
                    "youngs_modulus",
                    Value::Scalar {
                        si_value: 205e9,
                        dimension: DimensionVector::PRESSURE,
                    },
                ),
                ("poisson_ratio", Value::Real(0.29)),
            ],
        )
    }

    fn axis_y() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    fn origin() -> Value {
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn map_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
        match v {
            Value::Map(m) => m.get(&Value::String(key.to_string())),
            _ => None,
        }
    }

    /// Destructure a bounded `Value::Range` into its inner `(lower, upper)` values.
    fn range_lower_upper(v: &Value) -> (&Value, &Value) {
        match v {
            Value::Range {
                lower: Some(lo),
                upper: Some(up),
                ..
            } => (lo.as_ref(), up.as_ref()),
            other => panic!("expected a both-bounded Range, got {other:?}"),
        }
    }

    /// Assert `actual` is an ANGLE-dimensioned Scalar whose si_value matches
    /// `expected_rad` to a relative tolerance of 1e-9 (closed-form reproduction).
    fn assert_angle_close(actual: &Value, expected_rad: f64, label: &str) {
        match actual {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(
                    *dimension,
                    DimensionVector::ANGLE,
                    "{label}: bound carries ANGLE dimension"
                );
                let rel = (si_value - expected_rad).abs() / expected_rad.abs();
                assert!(rel < 1e-9, "{label}: {si_value} vs {expected_rad} (rel {rel})");
            }
            other => panic!("{label}: expected angle Scalar, got {other:?}"),
        }
    }

    // ── Convenience builders ─────────────────────────────────────────────────

    /// Build the standard 6-arg living-hinge argument list.
    fn lh_args_6(length: f64, width: f64, thickness: f64, mat: Value) -> Vec<Value> {
        vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            mat,
            origin(),
            axis_y(),
        ]
    }

    /// Build the standard 6-arg cross-spring argument list (same positional layout).
    fn cs_args_6(length: f64, width: f64, thickness: f64, mat: Value) -> Vec<Value> {
        vec![
            Value::length(length),
            Value::length(width),
            Value::length(thickness),
            mat,
            origin(),
            axis_y(),
        ]
    }

    // ─────────────────────────────────────────────────────────────────────────
    // step-1: RED — prb_living_hinge test suite
    // ─────────────────────────────────────────────────────────────────────────

    /// (a) Structure: kind, damping, axis, spring_rate dimension.
    #[test]
    fn prb_living_hinge_structure() {
        let args = lh_args_6(0.02, 0.005, 0.0005, steel());
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

        let args = lh_args_6(length, width, thickness, steel());
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
            &lh_args_6(l_short, 0.005, t_thick, steel()),
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
            &lh_args_6(l_long, 0.005, t_thin, steel()),
        );
        let (lo_prb, up_prb) =
            range_lower_upper(map_get(&result_prb, "range").unwrap());
        assert_angle_close(lo_prb, -prb_limit, "PRB-capped lower");
        assert_angle_close(up_prb, prb_limit, "PRB-capped upper");

        // (iii) No-yield fallback: steel without yield_stress → ±5°.
        let result_ny = crate::eval_builtin(
            "prb_living_hinge",
            &lh_args_6(l_long, 0.005, t_thin, steel_no_yield()),
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
        let base = lh_args_6(0.02, 0.005, 0.0005, steel());

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
        let base = lh_args_6(0.02, 0.005, 0.0005, steel());
        let with = |idx: usize, v: Value| {
            let mut a = base.clone();
            a[idx] = v;
            a
        };

        // Wrong arity.
        undef(vec![], "0 args");
        undef(base[..3].to_vec(), "3 args");
        {
            let mut a = base.clone();
            a.push(Value::angle(0.0));
            a.push(Value::angle(0.0));
            undef(a, "8 args");
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
        let args = cs_args_6(0.02, 0.005, 0.0005, steel());
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

        let args = cs_args_6(length, width, thickness, steel());
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
            &cs_args_6(l_short, 0.005, t_thick, steel()),
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
            &cs_args_6(l_long, 0.005, t_thin, steel()),
        );
        let (lo_prb, up_prb) = range_lower_upper(map_get(&result_prb, "range").unwrap());
        assert_angle_close(lo_prb, -prb_limit, "cs PRB-capped lower");
        assert_angle_close(up_prb, prb_limit, "cs PRB-capped upper");

        // (iii) No-yield fallback.
        let result_ny = crate::eval_builtin(
            "prb_cross_spring_pivot",
            &cs_args_6(l_long, 0.005, t_thin, steel_no_yield()),
        );
        let (lo_ny, up_ny) = range_lower_upper(map_get(&result_ny, "range").unwrap());
        assert_angle_close(lo_ny, -prb_limit, "cs no-yield lower");
        assert_angle_close(up_ny, prb_limit, "cs no-yield upper");
    }

    /// (d) Neutral handling for cross-spring.
    #[test]
    fn prb_cross_spring_pivot_neutral_angle_handling() {
        let two_deg = 2.0_f64 * PI / 180.0;
        let base = cs_args_6(0.02, 0.005, 0.0005, steel());

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
        let base = cs_args_6(0.02, 0.005, 0.0005, steel());
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

        // Wrong arity: <7 or >8.
        undef(vec![], "let 0 args");
        undef(base[..4].to_vec(), "let 4 args");
        {
            let mut a = base.clone();
            a.push(Value::angle(0.0));
            a.push(Value::angle(0.0));
            undef(a, "let 9 args");
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
}
