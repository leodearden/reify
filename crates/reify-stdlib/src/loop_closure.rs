//! Loop-closure machinery: value-level helpers operating on joint-Map `Value`s.
//!
//! This module provides the building blocks the kinematic snapshot evaluator
//! (future task 2585) and the generic Newton solver in
//! `reify_constraints::loop_closure` use to drive closed-chain mechanisms to
//! consistency.  It is the value-side companion to `reify-constraints::loop_closure`.
//!
//! Public API surface:
//!   * [`chain_transform`] — left-fold a sequence of joint Maps + motion
//!     variables into a single composed `Value::Transform`.
//!   * [`loop_residual_twist`] — log of `inv(T_a) · T_b`, returned as a
//!     6-vector twist suitable for stacking into a Newton residual.
//!   * [`joint_range_midpoint`] — joint-range midpoint for fresh-snapshot
//!     start strategy; recurses through `coupling` joints to the parent.
//!   * [`per_joint_jacobian_local`] — wraps the existing `joint_jacobian`
//!     builtin to return an analytic per-joint twist column as `[f64; 6]`.
//!     Returns `None` for joint kinds that lack an analytic form (used as
//!     the FD-fallback signal once task 4–7's spherical/cylindrical/planar
//!     joints land).
//!   * [`chain_jacobian_fd`] — central-difference chain Jacobian, one
//!     `[f64; 6]` column per free joint index.
//!
//! Twist convention: `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear last)
//! mirroring the `Map { angular, linear }` shape emitted by `transform_log` and
//! `joint_jacobian`.  All `[f64; 6]` returns and arguments in this module follow
//! this single canonical ordering.
//!
//! Jacobian strategy (v0.2 task 2 MVP): chain Jacobians use central-difference
//! finite difference ([`chain_jacobian_fd`]).  This is correct for ALL joint
//! kinds — including future spherical/cylindrical/planar — and immediately
//! satisfies the PRD's "FD fallback" requirement.  The analytic per-joint
//! twist column is exposed via [`per_joint_jacobian_local`] for future
//! adjoint-transport composition; that optimisation is out of scope for this
//! task and tracked as a follow-up design note.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! design rationale and convergence-tolerance defaults (1 µm position, 1 µrad
//! rotation — surfaced as `NewtonConfig` knobs in `reify_constraints::loop_closure`).

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

/// Compute the SE(3) loop-closure residual twist between two chains.
///
/// Returns `transform_log(transform_inverse(T_a) ⋅ T_b)` flattened to a
/// 6-element `[f64; 6]` in `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` ordering.
///
/// Returns `None` if either chain produces a `None` from `chain_transform`,
/// or if any underlying SE(3) operation produces `Value::Undef`.
pub fn loop_residual_twist(
    chain_a: &[Value],
    vals_a: &[f64],
    chain_b: &[Value],
    vals_b: &[f64],
) -> Option<[f64; 6]> {
    let t_a = chain_transform(chain_a, vals_a)?;
    let t_b = chain_transform(chain_b, vals_b)?;
    let t_a_inv = eval_builtin("transform_inverse", &[t_a]);
    if t_a_inv.is_undef() {
        return None;
    }
    let t_rel = eval_builtin("transform_compose", &[t_a_inv, t_b]);
    if t_rel.is_undef() {
        return None;
    }
    let twist_map = eval_builtin("transform_log", &[t_rel]);
    if twist_map.is_undef() {
        return None;
    }
    twist_map_to_array(&twist_map)
}

/// Convert a twist `Value::Map { angular, linear }` into the canonical
/// `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` `[f64; 6]` layout.
///
/// Reads each Vector3 component via `Value::as_f64` (accepts `Real`, `Int`,
/// `Scalar`).  Returns `None` if either field is missing, malformed, or any
/// component is non-numeric.
fn twist_map_to_array(twist_map: &Value) -> Option<[f64; 6]> {
    let map = match twist_map {
        Value::Map(m) => m,
        _ => return None,
    };
    let read_vec3 = |key: &str| -> Option<[f64; 3]> {
        match map.get(&Value::String(key.to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => {
                let a = items[0].as_f64()?;
                let b = items[1].as_f64()?;
                let c = items[2].as_f64()?;
                if !a.is_finite() || !b.is_finite() || !c.is_finite() {
                    return None;
                }
                Some([a, b, c])
            }
            _ => None,
        }
    };
    let ang = read_vec3("angular")?;
    let lin = read_vec3("linear")?;
    Some([ang[0], ang[1], ang[2], lin[0], lin[1], lin[2]])
}

/// Return the midpoint of a joint's free-variable range, in the joint's
/// own input coordinate (metres for prismatic, radians for revolute).
///
/// Returns `None` for joints whose range is missing, unbounded on either
/// side, for fixed (0-DOF) joints whose free-variable space is empty, or
/// for non-Map / unknown-kind inputs.
///
/// **Coupling note**: returns the *parent's* range midpoint (not scaled by
/// `ratio`).  The free-variable space of a coupling joint is the parent's
/// motion variable — the coupling's `transform_at` arm applies the ratio
/// downstream when computing the parent's coupled position.  This is the
/// joint's own free-variable space, not the coupled output.
pub fn joint_range_midpoint(joint: &Value) -> Option<f64> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" | "revolute" => {
            let range = map.get(&Value::String("range".to_string()))?;
            let (lo, up) = match range {
                Value::Range {
                    lower: Some(lo),
                    upper: Some(up),
                    ..
                } => (lo.as_ref(), up.as_ref()),
                _ => return None,
            };
            let lo_si = lo.as_f64()?;
            let up_si = up.as_f64()?;
            if !lo_si.is_finite() || !up_si.is_finite() {
                return None;
            }
            Some((lo_si + up_si) / 2.0)
        }
        "coupling" => {
            let parent = map.get(&Value::String("parent".to_string()))?;
            joint_range_midpoint(parent)
        }
        "fixed" => None, // 0-DOF joint: empty free-variable space, no midpoint to compute.
        _ => None,
    }
}

/// Return the analytic per-joint twist column expressed in the joint's own
/// input frame, as `[ω_x, ω_y, ω_z, v_x, v_y, v_z]`.
///
/// Wraps the existing `joint_jacobian` builtin (analytic for prismatic /
/// revolute / coupling) and converts the resulting `Map { angular, linear }`
/// to the canonical `[f64; 6]` layout.  Returns `None` for joint kinds the
/// builtin can't analyse (the FD chain assembly in
/// [`chain_jacobian_fd`] is the fallback for those — it perturbs the chain
/// without consulting this accessor).
///
/// **Note**: this is the per-joint analytic column.  Chain-level Jacobians
/// for the v0.2 task 2 MVP are computed via finite difference in
/// [`chain_jacobian_fd`]; a future optimisation can compose these per-joint
/// columns via SE(3) adjoint transport.
pub fn per_joint_jacobian_local(joint: &Value) -> Option<[f64; 6]> {
    let result = eval_builtin("joint_jacobian", std::slice::from_ref(joint));
    if result.is_undef() {
        return None;
    }
    twist_map_to_array(&result)
}

/// Compute the chain Jacobian by finite difference: one twist column per
/// free index.
///
/// For each `i ∈ free_indices`, perturbs `values[i]` by `±eps`, evaluates
/// `chain_transform` at both perturbed values, and computes
/// `transform_log(transform_inverse(T_minus) ⋅ T_plus) / (2*eps)` to recover
/// the central-difference column.  This is symmetric-error O(ε²) and works
/// for ALL joint kinds the chain contains (the analytic per-joint accessor
/// is plumbed separately in [`per_joint_jacobian_local`]).
///
/// Returns `None` if `eps <= 0`, any free index is out of range, the
/// `chain.len() != values.len()`, or any `chain_transform` along the way
/// produces `None`.
pub fn chain_jacobian_fd(
    chain: &[Value],
    values: &[f64],
    free_indices: &[usize],
    eps: f64,
) -> Option<Vec<[f64; 6]>> {
    if eps <= 0.0 || !eps.is_finite() {
        return None;
    }
    if chain.len() != values.len() {
        return None;
    }
    let n = chain.len();
    let mut cols: Vec<[f64; 6]> = Vec::with_capacity(free_indices.len());
    for &i in free_indices {
        if i >= n {
            return None;
        }
        let mut plus = values.to_vec();
        plus[i] += eps;
        let mut minus = values.to_vec();
        minus[i] -= eps;
        let t_plus = chain_transform(chain, &plus)?;
        let t_minus = chain_transform(chain, &minus)?;
        let t_minus_inv = eval_builtin("transform_inverse", &[t_minus]);
        if t_minus_inv.is_undef() {
            return None;
        }
        let rel = eval_builtin("transform_compose", &[t_minus_inv, t_plus]);
        if rel.is_undef() {
            return None;
        }
        let twist_map = eval_builtin("transform_log", &[rel]);
        if twist_map.is_undef() {
            return None;
        }
        let twist = twist_map_to_array(&twist_map)?;
        let scale = 1.0 / (2.0 * eps);
        let mut col = [0.0; 6];
        for k in 0..6 {
            col[k] = twist[k] * scale;
        }
        cols.push(col);
    }
    Some(cols)
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
        // 0-DOF joint: no free variable; transform_at("fixed", _) ignores the
        // second argument (any non-Undef Value → identity). Real(0.0) is the
        // conventional sentinel, mirroring `transform_at(fixed_joint, 0)` in
        // the kinematic_stdlib_smoke test.
        "fixed" => Some(Value::Real(0.0)),
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

    // ── joint_range_midpoint tests ───────────────────────────────────────

    #[test]
    fn joint_range_midpoint_prismatic_0_to_1m() {
        let j = eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)]);
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        assert!((mid - 0.5).abs() < 1e-12);
    }

    #[test]
    fn joint_range_midpoint_prismatic_neg_to_pos() {
        let j = eval_builtin("prismatic", &[axis_x(), length_range(-2.0, 2.0)]);
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        assert!(mid.abs() < 1e-12);
    }

    #[test]
    fn joint_range_midpoint_revolute_0_to_pi() {
        let j = eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        );
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        assert!((mid - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn joint_range_midpoint_revolute_neg_pi_2_to_pi_2() {
        let j = eval_builtin(
            "revolute",
            &[
                axis_z(),
                angle_range(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
            ],
        );
        let mid = super::joint_range_midpoint(&j).expect("midpoint");
        assert!(mid.abs() < 1e-12);
    }

    #[test]
    fn joint_range_midpoint_coupling_delegates_to_parent() {
        let parent = eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        );
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let mid = super::joint_range_midpoint(&coupling).expect("midpoint");
        assert!(
            (mid - std::f64::consts::FRAC_PI_2).abs() < 1e-12,
            "expected π/2, got {mid}"
        );
    }

    #[test]
    fn joint_range_midpoint_missing_range_returns_none() {
        // Build a Map with a "kind" but no "range" key.
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::joint_range_midpoint(&j).is_none());
    }

    #[test]
    fn joint_range_midpoint_unbounded_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        m.insert(
            Value::String("range".to_string()),
            Value::Range {
                lower: Some(Box::new(Value::length(0.0))),
                upper: None,
                lower_inclusive: true,
                upper_inclusive: false,
            },
        );
        let j = Value::Map(m);
        assert!(super::joint_range_midpoint(&j).is_none());
    }

    // ── per_joint_jacobian_local tests ───────────────────────────────────

    #[test]
    fn per_joint_jacobian_local_prismatic_x_unit() {
        let j = eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)]);
        let col = super::per_joint_jacobian_local(&j).expect("col");
        // [ω; v]: angular zero, linear = unit X
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!(col[2].abs() < 1e-12);
        assert!((col[3] - 1.0).abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_prismatic_unnormalized_axis() {
        // axis [3, 4, 0] has magnitude 5; normalized → [0.6, 0.8, 0]
        let axis = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let j = eval_builtin("prismatic", &[axis, length_range(0.0, 1.0)]);
        let col = super::per_joint_jacobian_local(&j).expect("col");
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!(col[2].abs() < 1e-12);
        assert!((col[3] - 0.6).abs() < 1e-12);
        assert!((col[4] - 0.8).abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_revolute_z() {
        let j = eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        );
        let col = super::per_joint_jacobian_local(&j).expect("col");
        // angular = unit Z, linear zero
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!((col[2] - 1.0).abs() < 1e-12);
        assert!(col[3].abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_coupling_revolute_ratio_2() {
        let parent = eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        );
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let col = super::per_joint_jacobian_local(&coupling).expect("col");
        // ratio * parent_jac = ratio * [0,0,1, 0,0,0] = [0,0,2, 0,0,0]
        assert!(col[0].abs() < 1e-12);
        assert!(col[1].abs() < 1e-12);
        assert!((col[2] - 2.0).abs() < 1e-12);
        assert!(col[3].abs() < 1e-12);
        assert!(col[4].abs() < 1e-12);
        assert!(col[5].abs() < 1e-12);
    }

    #[test]
    fn per_joint_jacobian_local_unknown_kind_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("spherical".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::per_joint_jacobian_local(&j).is_none());
    }

    #[test]
    fn per_joint_jacobian_local_missing_axis_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        let j = Value::Map(m);
        assert!(super::per_joint_jacobian_local(&j).is_none());
    }

    #[test]
    fn per_joint_jacobian_local_non_map_returns_none() {
        assert!(super::per_joint_jacobian_local(&Value::Real(0.5)).is_none());
    }

    // ── chain_jacobian_fd tests ──────────────────────────────────────────

    fn assert_columns_close(
        actual: &[[f64; 6]],
        expected: &[[f64; 6]],
        tol: f64,
        label: &str,
    ) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{label}: column count mismatch"
        );
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            for k in 0..6 {
                assert!(
                    (a[k] - e[k]).abs() < tol,
                    "{label}: col[{i}][{k}] expected {}, got {} (diff {})",
                    e[k],
                    a[k],
                    (a[k] - e[k]).abs(),
                );
            }
        }
    }

    #[test]
    fn chain_jacobian_fd_single_prismatic_matches_analytic() {
        let chain = vec![prismatic_x()];
        let analytic = super::per_joint_jacobian_local(&chain[0]).unwrap();
        let cols = super::chain_jacobian_fd(&chain, &[0.5], &[0], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        assert_columns_close(&cols, &[analytic], 1e-7, "single_prismatic");
    }

    #[test]
    fn chain_jacobian_fd_single_revolute_matches_analytic() {
        let chain = vec![revolute_z()];
        let analytic = super::per_joint_jacobian_local(&chain[0]).unwrap();
        let cols = super::chain_jacobian_fd(&chain, &[0.0], &[0], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        assert_columns_close(&cols, &[analytic], 1e-7, "single_revolute");
    }

    #[test]
    fn chain_jacobian_fd_two_joint_returns_two_columns() {
        let chain = vec![prismatic_x(), revolute_z()];
        let cols = super::chain_jacobian_fd(
            &chain,
            &[0.5, 0.0],
            &[0, 1],
            1e-6,
        )
        .expect("cols");
        assert_eq!(cols.len(), 2);
        for col in &cols {
            for v in col.iter() {
                assert!(v.is_finite(), "expected finite, got {col:?}");
            }
        }
    }

    #[test]
    fn chain_jacobian_fd_out_of_range_returns_none() {
        let chain = vec![prismatic_x()];
        assert!(super::chain_jacobian_fd(&chain, &[0.5], &[5], 1e-6).is_none());
    }

    #[test]
    fn chain_jacobian_fd_zero_eps_returns_none() {
        let chain = vec![prismatic_x()];
        assert!(super::chain_jacobian_fd(&chain, &[0.5], &[0], 0.0).is_none());
    }

    #[test]
    fn chain_jacobian_fd_undef_chain_returns_none() {
        let mut bogus = std::collections::BTreeMap::new();
        bogus.insert(
            Value::String("kind".to_string()),
            Value::String("bogus".to_string()),
        );
        let chain = vec![Value::Map(bogus)];
        assert!(super::chain_jacobian_fd(&chain, &[0.5], &[0], 1e-6).is_none());
    }

    #[test]
    fn chain_jacobian_fd_two_joint_only_second_free_axis_aligned() {
        // chain = [prismatic_x at 0.5m, revolute_z at 0]
        // free index 1 (revolute_z) → column should be axis-aligned around Z
        // (angular ≈ [0,0,1], linear close to zero — exact form depends on
        // the SE(3) chain's left-Jacobian at the trunk; we only assert finite,
        // axis-aligned structure here).
        let chain = vec![prismatic_x(), revolute_z()];
        let cols = super::chain_jacobian_fd(&chain, &[0.5, 0.0], &[1], 1e-6).expect("cols");
        assert_eq!(cols.len(), 1);
        let col = cols[0];
        for v in col.iter() {
            assert!(v.is_finite(), "expected finite, got {col:?}");
        }
        // Angular: dominant Z component (within 1e-4 of unity at small angle).
        assert!(
            (col[2] - 1.0).abs() < 1e-4,
            "expected angular Z ≈ 1, got {}",
            col[2]
        );
    }

    #[test]
    fn joint_range_midpoint_non_map_returns_none() {
        assert!(super::joint_range_midpoint(&Value::Real(0.5)).is_none());
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

    // ── fixed-joint chain integration tests ─────────────────────────────

    fn fixed_joint() -> Value {
        eval_builtin("fixed", &[])
    }

    /// A chain containing only a fixed joint must produce the identity
    /// Transform — a fixed joint contributes no translation or rotation.
    #[test]
    fn chain_transform_single_fixed_joint_returns_identity() {
        let chain = vec![fixed_joint()];
        let result = super::chain_transform(&chain, &[0.0])
            .expect("chain_transform must return Some for a fixed joint");
        let trans = translation_xyz(&result);
        assert!(
            trans[0].abs() < 1e-12 && trans[1].abs() < 1e-12 && trans[2].abs() < 1e-12,
            "expected zero translation, got {trans:?}"
        );
        let (w, x, y, z) = rotation_wxyz(&result);
        assert!(
            (w - 1.0).abs() < 1e-12,
            "expected w ≈ 1.0 (identity rotation), got {w}"
        );
        assert!(
            x.abs() < 1e-12 && y.abs() < 1e-12 && z.abs() < 1e-12,
            "expected x,y,z ≈ 0 (identity rotation), got {x},{y},{z}"
        );
    }

    /// A fixed joint mid-chain contributes identity: the net translation must
    /// equal the sum of the two prismatic joints on either side of it.
    #[test]
    fn chain_transform_fixed_joint_in_middle_acts_as_identity_contribution() {
        // [prismatic_x @ 0.3m, fixed, prismatic_x @ 0.5m]
        // Expected: total translation_x ≈ 0.8m, no rotation.
        let chain = vec![prismatic_x(), fixed_joint(), prismatic_x()];
        let result = super::chain_transform(&chain, &[0.3, 0.0, 0.5])
            .expect("chain_transform must return Some with fixed joint in the middle");
        let trans = translation_xyz(&result);
        assert!(
            (trans[0] - 0.8).abs() < 1e-12,
            "expected translation_x = 0.8, got {}",
            trans[0]
        );
        assert!(trans[1].abs() < 1e-12);
        assert!(trans[2].abs() < 1e-12);
    }

    /// A fixed joint in the chain but NOT in free_indices must not prevent
    /// chain_jacobian_fd from assembling the other (free) columns.
    #[test]
    fn chain_jacobian_fd_with_fixed_joint_outside_free_indices() {
        // chain = [prismatic_x, fixed, revolute_z]
        // free_indices = [0, 2] (the two DOF joints — fixed at index 1 is not free)
        let chain = vec![prismatic_x(), fixed_joint(), revolute_z()];
        let cols = super::chain_jacobian_fd(&chain, &[0.5, 0.0, 0.0], &[0, 2], 1e-6)
            .expect("chain_jacobian_fd must return Some when fixed joint is not in free_indices");
        assert_eq!(cols.len(), 2, "expected 2 columns for 2 free indices");
        for col in &cols {
            for v in col.iter() {
                assert!(v.is_finite(), "expected all column entries to be finite, got {col:?}");
            }
        }
    }

    /// A fixed joint listed in free_indices must produce a zero-twist column —
    /// perturbing its (inert) motion variable never reaches transform_at and
    /// T_plus == T_minus == identity, so the central-difference quotient is 0.
    #[test]
    fn chain_jacobian_fd_with_fixed_joint_in_free_indices_yields_zero_column() {
        let chain = vec![fixed_joint()];
        let cols = super::chain_jacobian_fd(&chain, &[0.0], &[0], 1e-6)
            .expect("chain_jacobian_fd must return Some for a fixed-only chain");
        assert_eq!(cols.len(), 1, "expected 1 column");
        let col = cols[0];
        for (k, &v) in col.iter().enumerate() {
            assert!(
                v.abs() < 1e-10,
                "expected zero-twist column entry [{}], got {}",
                k,
                v
            );
        }
    }
}
