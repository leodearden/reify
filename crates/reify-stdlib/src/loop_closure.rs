//! Loop-closure machinery: value-level helpers operating on joint-Map `Value`s.
//!
//! This module provides the building blocks the kinematic snapshot evaluator
//! (future task 2585) and the generic Newton solver in
//! `reify_constraints::loop_closure` use to drive closed-chain mechanisms to
//! consistency.  It is the value-side companion to `reify-constraints::loop_closure`.
//!
//! Public API surface (filled in by the TDD steps that follow):
//!   * `chain_transform(chain, values) -> Option<Value>`
//!   * `loop_residual_twist(chain_a, vals_a, chain_b, vals_b) -> Option<[f64; 6]>`
//!   * `joint_range_midpoint(joint) -> Option<f64>`
//!   * `per_joint_jacobian_local(joint) -> Option<[f64; 6]>`
//!   * `chain_jacobian_fd(chain, values, free_indices, eps) -> Option<Vec<[f64; 6]>>`
//!
//! Twist convention: `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear last)
//! mirroring the `Map { angular, linear }` shape emitted by `transform_log` and
//! `joint_jacobian`.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! design rationale and convergence-tolerance defaults.

use reify_types::Value;

use crate::eval_builtin;

/// Fold a chain of joint Maps into a single composed Transform.
///
/// `chain[i]` is a joint `Value::Map` (kind `"prismatic"`, `"revolute"`, or
/// `"coupling"`); `values[i]` is its motion variable in SI units (metres for
/// prismatic, radians for revolute; for coupling, in the parent's input
/// coordinate — the coupling's `transform_at` arm wraps it via the parent
/// kind's helper).
///
/// Composition is left-to-right: `T_total = T_0 * T_1 * ... * T_{n-1}`,
/// matching the semantics of nesting joints from base outward.  Returns
/// `None` if any joint produces `Value::Undef` from `transform_at` (invalid
/// joint Map, dimension mismatch, etc.) or if `chain.len() != values.len()`.
pub fn chain_transform(chain: &[Value], values: &[f64]) -> Option<Value> {
    if chain.len() != values.len() {
        return None;
    }
    let mut acc = eval_builtin("transform3_identity", &[]);
    if acc.is_undef() {
        return None;
    }
    for (joint, &v) in chain.iter().zip(values.iter()) {
        let v_value = value_for_joint(joint, v)?;
        let next = eval_builtin("transform_at", &[joint.clone(), v_value]);
        if next.is_undef() {
            return None;
        }
        let composed = eval_builtin("transform_compose", &[acc, next]);
        if composed.is_undef() {
            return None;
        }
        acc = composed;
    }
    Some(acc)
}

/// Wrap a raw f64 motion variable in a dimensioned `Value` appropriate for
/// the joint kind: `Value::length` for prismatic, `Value::angle` for revolute.
/// Coupling joints delegate to their parent's kind.
///
/// Returns `None` for unknown kinds or malformed Maps.
fn value_for_joint(joint: &Value, scalar: f64) -> Option<Value> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" => Some(Value::length(scalar)),
        "revolute" => Some(Value::angle(scalar)),
        "coupling" => {
            let parent_map = match map.get(&Value::String("parent".to_string())) {
                Some(Value::Map(pm)) => pm,
                _ => return None,
            };
            let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return None,
            };
            match parent_kind {
                "prismatic" => Some(Value::length(scalar)),
                "revolute" => Some(Value::angle(scalar)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    // ── Test fixtures ────────────────────────────────────────────────────

    fn axis_x() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    fn axis_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    fn length_range(lo: f64, up: f64) -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(lo))),
            upper: Some(Box::new(Value::length(up))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn angle_range(lo: f64, up: f64) -> Value {
        Value::Range {
            lower: Some(Box::new(Value::angle(lo))),
            upper: Some(Box::new(Value::angle(up))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn prismatic_x() -> Value {
        eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)])
    }

    fn revolute_z() -> Value {
        eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        )
    }

    /// Extract the translation Vector3 from a Transform; helper for tests.
    fn translation_xyz(t: &Value) -> [f64; 3] {
        let translation = match t {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Transform, got {other:?}"),
        };
        let comps = match translation {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("expected Vector3 translation, got {other:?}"),
        };
        [
            comps[0].as_f64().unwrap(),
            comps[1].as_f64().unwrap(),
            comps[2].as_f64().unwrap(),
        ]
    }

    /// Extract orientation (w, x, y, z) from a Transform.
    fn rotation_wxyz(t: &Value) -> (f64, f64, f64, f64) {
        let rot = match t {
            Value::Transform { rotation, .. } => rotation.as_ref(),
            other => panic!("expected Transform, got {other:?}"),
        };
        match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("expected Orientation, got {other:?}"),
        }
    }

    // ── chain_transform tests ────────────────────────────────────────────

    #[test]
    fn chain_transform_empty_chain_returns_identity() {
        let result = super::chain_transform(&[], &[]).expect("identity Transform");
        let trans = translation_xyz(&result);
        assert!(trans[0].abs() < 1e-12 && trans[1].abs() < 1e-12 && trans[2].abs() < 1e-12);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    #[test]
    fn chain_transform_single_prismatic_x_at_half_metre() {
        let chain = vec![prismatic_x()];
        let result = super::chain_transform(&chain, &[0.5]).expect("Transform");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.5).abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!((w - 1.0).abs() < 1e-12);
        assert!(x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12);
    }

    #[test]
    fn chain_transform_two_prismatic_x_compose_left_to_right() {
        let chain = vec![prismatic_x(), prismatic_x()];
        let result = super::chain_transform(&chain, &[0.3, 0.5]).expect("Transform");
        let trans = translation_xyz(&result);
        assert!(
            (trans[0] - 0.8).abs() < 1e-12,
            "expected translation_x = 0.8, got {}",
            trans[0]
        );
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
    }

    #[test]
    fn chain_transform_prismatic_then_revolute() {
        // chain = [prismatic_x at 0.5m, revolute_z at π/2]
        // After prismatic: T1 has translation [0.5,0,0], rot identity.
        // After revolute composed: rotation = rot_z(π/2), translation
        // unchanged ([0.5,0,0]) because R1*t2 + t1 with t2=0 ⇒ t1 = [0.5,0,0].
        let chain = vec![prismatic_x(), revolute_z()];
        let result =
            super::chain_transform(&chain, &[0.5, std::f64::consts::FRAC_PI_2]).expect("Transform");
        let trans = translation_xyz(&result);
        assert!((trans[0] - 0.5).abs() < 1e-12);
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
        let (w, _x, _y, z) = rotation_wxyz(&result);
        let half = std::f64::consts::FRAC_PI_4;
        assert!((w - half.cos()).abs() < 1e-12 || (w + half.cos()).abs() < 1e-12);
        assert!((z.abs() - half.sin()).abs() < 1e-12);
    }

    #[test]
    fn chain_transform_length_mismatch_returns_none() {
        let chain = vec![prismatic_x(), prismatic_x()];
        assert!(super::chain_transform(&chain, &[0.3]).is_none());
        assert!(super::chain_transform(&chain, &[0.3, 0.5, 0.1]).is_none());
    }

    // ── loop_residual_twist tests ────────────────────────────────────────

    #[test]
    fn loop_residual_twist_identical_chains_zero() {
        let a = vec![prismatic_x()];
        let b = vec![prismatic_x()];
        let twist: [f64; 6] =
            super::loop_residual_twist(&a, &[0.5], &b, &[0.5]).expect("twist");
        for v in twist.iter() {
            assert!(v.abs() < 1e-12, "expected zero twist, got {twist:?}");
        }
    }

    #[test]
    fn loop_residual_twist_prismatic_diff_in_x() {
        // chain_a = prismatic_x at 0.5m, chain_b = prismatic_x at 0.3m.
        // T_a inverse * T_b = pure translation (-0.2, 0, 0). log of that is
        // a twist with angular = 0 and linear = (-0.2, 0, 0).
        let a = vec![prismatic_x()];
        let b = vec![prismatic_x()];
        let twist = super::loop_residual_twist(&a, &[0.5], &b, &[0.3]).expect("twist");
        // [ω_x, ω_y, ω_z, v_x, v_y, v_z]
        assert!(twist[0].abs() < 1e-12);
        assert!(twist[1].abs() < 1e-12);
        assert!(twist[2].abs() < 1e-12);
        assert!(
            (twist[3] + 0.2).abs() < 1e-12,
            "expected v_x ≈ -0.2, got {}",
            twist[3]
        );
        assert!(twist[4].abs() < 1e-12);
        assert!(twist[5].abs() < 1e-12);
    }

    #[test]
    fn loop_residual_twist_two_joint_identical_chains_zero() {
        let a = vec![prismatic_x(), revolute_z()];
        let b = vec![prismatic_x(), revolute_z()];
        let twist: [f64; 6] = super::loop_residual_twist(
            &a,
            &[0.5, std::f64::consts::FRAC_PI_2],
            &b,
            &[0.5, std::f64::consts::FRAC_PI_2],
        )
        .expect("twist");
        for v in twist.iter() {
            assert!(v.abs() < 1e-10, "expected ~zero twist, got {twist:?}");
        }
    }

    #[test]
    fn loop_residual_twist_length_mismatch_returns_none() {
        let a = vec![prismatic_x(), prismatic_x()];
        let b = vec![prismatic_x()];
        // chain_a length mismatches vals_a
        assert!(super::loop_residual_twist(&a, &[0.5], &b, &[0.3]).is_none());
        // chain_b length mismatches vals_b
        assert!(super::loop_residual_twist(&b, &[0.5], &b, &[0.3, 0.1]).is_none());
    }

    #[test]
    fn loop_residual_twist_undef_chain_returns_none() {
        // Hand-built joint Map with bogus kind triggers chain_transform → None.
        let mut bogus = std::collections::BTreeMap::new();
        bogus.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let a = vec![Value::Map(bogus)];
        let b = vec![prismatic_x()];
        assert!(super::loop_residual_twist(&a, &[0.0], &b, &[0.0]).is_none());
    }

    #[test]
    fn chain_transform_invalid_kind_returns_none() {
        // Hand-built joint Map with bogus kind
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let chain = vec![Value::Map(m)];
        assert!(super::chain_transform(&chain, &[0.5]).is_none());
    }
}
