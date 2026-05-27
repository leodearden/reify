use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value, quaternion_is_finite};

use crate::dynamics::spatial::SpatialVector6;
use crate::helpers::trig_input;
use crate::orientation::normalize_quaternion;

/// Evaluate a joints stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_joints(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "prismatic" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("prismatic", args[0].clone(), args[1].clone())
        }
        "revolute" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("revolute", args[0].clone(), args[1].clone())
        }
        // 3-DOF planar joint: two prismatic DOFs (along axis_x and axis_y) plus one
        // revolute DOF (about axis_x × axis_y). Per PRD v0_2/kinematic-constraints.md
        // §"Decomposition plan" task 6.
        //
        // Signature: planar(axis_x, axis_y, range_x, range_y, range_theta)
        // where axis_x ⊥ axis_y (both dimensionless Vector3, finite, non-zero),
        // range_x / range_y are LENGTH ranges, range_theta is an ANGLE range.
        // The raw (unnormalised) axes are stored in the Map; normalisation happens
        // at `transform_at` time — matching the prismatic/revolute precedent.
        "planar" => {
            if args.len() != 5 {
                return Some(Value::Undef);
            }
            // Validate axis_x: dimensionless Vector3, finite, non-zero.
            let comps_x = match validate_axis(&args[0]) {
                Some(c) => c,
                None => return Some(Value::Undef),
            };
            // Validate axis_y: dimensionless Vector3, finite, non-zero.
            let comps_y = match validate_axis(&args[1]) {
                Some(c) => c,
                None => return Some(Value::Undef),
            };
            // Perpendicularity check: `|dot(unit_x, unit_y)| < 1e-9`.  Normalise
            // each axis first so the dot product is in cos-angle units.  Rationale
            // and FP-tolerance discussion live on `unit_axes_xy_from_planar_map`
            // (single source of truth; see also docs/prds/v0_2/per-purpose-tolerance.md).
            let unit_x = unit_normalize(comps_x);
            let unit_y = unit_normalize(comps_y);
            let dot = unit_x[0] * unit_y[0] + unit_x[1] * unit_y[1] + unit_x[2] * unit_y[2];
            if dot.abs() >= 1e-9 {
                return Some(Value::Undef);
            }
            // Validate range_x: bounded, LENGTH-dimensioned.
            if validate_range(&args[2], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // Validate range_y: bounded, LENGTH-dimensioned.
            if validate_range(&args[3], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // Validate range_theta: bounded, ANGLE-dimensioned.
            if validate_range(&args[4], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            make_planar(
                args[0].clone(),
                args[1].clone(),
                args[2].clone(),
                args[3].clone(),
                args[4].clone(),
            )
        }
        // 3-DOF spherical joint: free rotation in SO(3), no translation.
        // Per PRD v0_2/kinematic-constraints.md §"Decomposition plan" task 4.
        //
        // Signature: spherical(range_angle: Range<Angle>) — single Range<Angle>
        // bounding the swing magnitude (axis-angle / cone half-angle). The joint
        // is axis-isotropic — there is no preferred direction — so no axis is
        // stored. The motion variable for `transform_at` is a unit quaternion
        // (`Value::Orientation`); Euler / axis-angle exposure is the user's
        // responsibility via composition of `orient_euler` / `orient_axis_angle`
        // and `orient_to_euler` / `orient_to_axis_angle`.
        "spherical" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            // Validate range_angle: bounded, ANGLE-dimensioned. Mirrors the
            // prismatic / revolute / planar precedent.
            if validate_range(&args[0], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            make_spherical(args[0].clone())
        }
        // 2-DOF cylindrical joint: composite of prismatic ⊕ revolute on a single
        // shared axis. Per PRD v0_2/kinematic-constraints.md §"Decomposition plan"
        // task 5.
        //
        // Signature: cylindrical(axis, translation_range, rotation_range)
        // where axis is a dimensionless Vector3 (finite, non-zero), translation_range
        // is a LENGTH range (bounded), and rotation_range is an ANGLE range (bounded).
        // The raw (unnormalised) axis is stored; normalisation happens at
        // `transform_at` and `joint_jacobian` time — matching the prismatic /
        // revolute / planar precedent.
        "cylindrical" => {
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            // Validate axis: dimensionless Vector3, finite, non-zero.
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            // Validate translation_range: bounded, LENGTH-dimensioned.
            if validate_range(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // Validate rotation_range: bounded, ANGLE-dimensioned.
            if validate_range(&args[2], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            make_cylindrical_joint(args[0].clone(), args[1].clone(), args[2].clone())
        }
        // 0-DOF group-only joint (sub-assembly grouping, clearance-pair filtering).
        // Per PRD v0_2/kinematic-constraints.md §"Decomposition plan" task 7.
        //
        // No axis or range: the joint has no motion variable, no 1D direction to
        // translate/rotate along, and no range to constrain. The resulting Map has
        // a single field `{ "kind": "fixed" }`, mirroring the `world` sentinel shape.
        "fixed" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("fixed".to_string()),
            );
            Value::Map(m)
        }
        // `transform_at(joint, motion_var?)` — evaluate a joint's rigid-body Transform.
        //
        // Arity:
        //   0 args           → Undef (no joint arg)
        //   1 arg (fixed)    → identity Transform (canonical 0-DOF ergonomic form, task 2688)
        //   1 arg (non-fixed) → Undef (other kinds need a real motion variable)
        //   2 args           → per-kind Transform (chain-machinery path, all kinds)
        //   3+ args          → Undef (arity error)
        "transform_at" => {
            // 1-arg form is accepted for fixed joints only (0-DOF: identity Transform,
            // no motion variable). 0-arg and 3+-arg calls always return Undef.
            // 2-arg form (the chain-machinery path) falls through unchanged.
            if args.is_empty() || args.len() > 2 {
                return Some(Value::Undef);
            }
            let map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let kind = match map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            // 1-arg form: only valid for the 0-DOF fixed joint. All other kinds need
            // a real motion variable (length, angle, orientation, list, etc.) and
            // return Undef when called with 1 arg. The 2-arg form falls through to
            // the per-kind match below unchanged (task 2688).
            if args.len() == 1 {
                return Some(if kind == "fixed" {
                    fixed_identity_transform()
                } else {
                    Value::Undef
                });
            }
            match kind {
                "prismatic" | "revolute" => transform_at_simple_joint(kind, map, &args[1]),
                // 0-DOF fixed joint: the canonical user form is now `transform_at(fixed_joint)`
                // (1-arg, task 2688); this 2-arg arm is the chain-machinery path used by
                // `chain_transform` → `value_for_joint` → `transform_at(joint, value)`.
                // Returns the identity Transform when the second arg is a
                // numeric/dimensioned scalar (Real, Int, or Scalar of any dimension).
                // Returns Undef when the second arg is Undef (Undef propagation) OR any
                // non-numeric variant (String, List, Map, Vector, Bool, etc.).
                // Type/dimension of the numeric second arg is otherwise irrelevant — a
                // 0-DOF joint has no motion variable. Mirrors the type-checking discipline
                // of every other transform_at arm (task 2687).
                //
                // NaN/Inf values are intentionally accepted (unlike sibling arms which
                // enforce is_finite() on the motion variable) because the motion variable
                // is unused for a 0-DOF joint — NaN/Inf never propagates into the identity
                // Transform output. A finiteness guard would be a no-op here and is left
                // out for clarity.
                "fixed" => {
                    if args[1].is_undef() {
                        return Some(Value::Undef);
                    }
                    // Tightened contract (task 2687): a 0-DOF joint has no motion
                    // variable, but the second arg must still be a numeric/dimensioned
                    // scalar so that upstream type errors (e.g. a String accidentally
                    // reaching this call) propagate as Undef instead of being absorbed
                    // into a well-formed identity Transform.
                    if !matches!(
                        &args[1],
                        Value::Real(_) | Value::Int(_) | Value::Scalar { .. }
                    ) {
                        return Some(Value::Undef);
                    }
                    fixed_identity_transform()
                }
                // 3-DOF planar joint: motion_vars is a Value::List of 3 elements
                // [x_length, y_length, theta_angle].
                // Composition: T_planar = T_x · T_y · T_theta (left-to-right),
                // matching the chain_transform convention in loop_closure.rs.
                // See docs/prds/v0_2/kinematic-constraints.md §"Decomposition plan"
                // task 6 for the canonical composition order (used by the
                // analytic-Jacobian follow-up task).
                "planar" => {
                    let items = match &args[1] {
                        Value::List(v) if v.len() == 3 => v.clone(),
                        _ => return Some(Value::Undef),
                    };
                    // Extract x (metres), y (metres), theta (radians).
                    let x = match length_input(&items[0]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let y = match length_input(&items[1]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let theta = match trig_input(&items[2]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    // Defense-in-depth: helpers already enforce finiteness for
                    // Scalar/Real branches; Int yields finite f64 by construction.
                    if !x.is_finite() || !y.is_finite() || !theta.is_finite() {
                        return Some(Value::Undef);
                    }
                    // Extract and unit-normalise axis_x and axis_y from the joint Map.
                    let (unit_x, unit_y) = match unit_axes_xy_from_planar_map(map) {
                        Some(axes) => axes,
                        None => return Some(Value::Undef),
                    };
                    let [ux, uy, uz] = unit_x;
                    let [vx, vy, vz] = unit_y;
                    // Plane normal n = unit_x × unit_y (cross product).
                    let (nx, ny, nz) = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx);
                    // T_x: pure translation x * unit_axis_x
                    let t_x = Value::Transform {
                        rotation: Box::new(Value::Orientation {
                            w: 1.0,
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        }),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(x * ux),
                            Value::length(x * uy),
                            Value::length(x * uz),
                        ])),
                    };
                    // T_y: pure translation y * unit_axis_y
                    let t_y = Value::Transform {
                        rotation: Box::new(Value::Orientation {
                            w: 1.0,
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                        }),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(y * vx),
                            Value::length(y * vy),
                            Value::length(y * vz),
                        ])),
                    };
                    // T_theta: pure rotation by theta about the plane normal.
                    let t_theta = Value::Transform {
                        rotation: Box::new(axis_angle_quaternion(nx, ny, nz, theta)),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(0.0),
                            Value::length(0.0),
                            Value::length(0.0),
                        ])),
                    };
                    // Compose: T_x · T_y · T_theta (left-to-right).
                    let t_xy = crate::eval_builtin("transform_compose", &[t_x, t_y]);
                    if t_xy.is_undef() {
                        return Some(Value::Undef);
                    }
                    let t_planar = crate::eval_builtin("transform_compose", &[t_xy, t_theta]);
                    if t_planar.is_undef() {
                        return Some(Value::Undef);
                    }
                    t_planar
                }
                // 3-DOF spherical joint: motion variable is `Value::Orientation`
                // (a unit quaternion). Per PRD task 4 design decision, the user
                // constructs the quaternion via `orient_axis_angle` /
                // `orient_euler` / `orient_quaternion` before calling
                // `transform_at`; the spherical arm renormalises via
                // `normalize_quaternion` as defence-in-depth against hand-built
                // `Value::Orientation` carrying non-unit components. The result
                // is `Value::Transform { rotation = normalised_q, translation = 0 }`.
                //
                // Non-`Value::Orientation` inputs (Undef, Real, Vector, List,
                // bare Map, etc.) fall through to Undef. The same Undef applies
                // when `quaternion_is_finite` rejects NaN/Inf components or
                // `normalize_quaternion` rejects a zero-norm quaternion (the
                // latter is documented at orientation.rs:560).
                "spherical" => {
                    let (w, x, y, z) = match &args[1] {
                        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                        _ => return Some(Value::Undef),
                    };
                    if !quaternion_is_finite(w, x, y, z) {
                        return Some(Value::Undef);
                    }
                    let rotation = match normalize_quaternion(w, x, y, z) {
                        Some(q) => q,
                        None => return Some(Value::Undef),
                    };
                    Value::Transform {
                        rotation: Box::new(rotation),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(0.0),
                            Value::length(0.0),
                            Value::length(0.0),
                        ])),
                    }
                }
                // 2-DOF cylindrical joint: motion variable is a 2-element
                // `Value::List` of `[Length, Angle]` (translation distance,
                // rotation angle). List-only mirrors the planar arm — Reify
                // `[a, b]` literals lower to `Value::List`.
                //
                // Composition rationale (single-step construction): translation
                // along axis n and rotation about the same axis n commute in
                // SE(3) — for any d, θ, R(n, θ)·(d·n) = d·n (rotation about an
                // axis preserves vectors along that axis). So `T_p` (translation
                // d·n, identity rotation) and `T_r` (rotation R(n, θ), zero
                // translation) compose to the same result regardless of order:
                // {rotation = R(n, θ), translation = d·n}. We build that result
                // directly, avoiding the round-trip through `transform_compose`
                // and the floating-point drift it would introduce.
                "cylindrical" => {
                    let [nax, nay, naz] = match unit_axis_from_map(map) {
                        Some(a) => a,
                        None => return Some(Value::Undef),
                    };
                    let (dist, theta) = match cylindrical_motion_vars(&args[1]) {
                        Some(pair) => pair,
                        None => return Some(Value::Undef),
                    };
                    // Defense-in-depth: cylindrical_motion_vars uses
                    // length_input/trig_input which already reject non-finite,
                    // but mirror the prismatic/revolute arms for symmetry.
                    if !dist.is_finite() || !theta.is_finite() {
                        return Some(Value::Undef);
                    }
                    let rotation = axis_angle_quaternion(nax, nay, naz, theta);
                    let translation = Value::Vector(vec![
                        Value::length(dist * nax),
                        Value::length(dist * nay),
                        Value::length(dist * naz),
                    ]);
                    Value::Transform {
                        rotation: Box::new(rotation),
                        translation: Box::new(translation),
                    }
                }
                "coupling" => {
                    // Extract the three coupling-payload fields (kind already matched
                    // above) with explicit guards. A Map built by a trusted `couple`
                    // call always has them, but hand-built Maps used in tests or future
                    // serialisation paths may not.
                    let parent_map = match map.get(&Value::String("parent".to_string())) {
                        Some(Value::Map(pm)) => pm,
                        _ => return Some(Value::Undef),
                    };
                    let ratio_f64 = match map.get(&Value::String("ratio".to_string())) {
                        Some(Value::Real(r)) => *r,
                        _ => return Some(Value::Undef),
                    };
                    // Defense-in-depth: a hand-built coupling Map could carry
                    // Value::Real(NaN) or Value::Real(Inf). `couple` rejects these at
                    // construction via `ratio_input`, but `transform_at` should not rely
                    // on the constructor. Symmetric with the parent-kind / offset / v_si
                    // guards below.
                    if !ratio_f64.is_finite() {
                        return Some(Value::Undef);
                    }
                    let offset_si = match map.get(&Value::String("offset".to_string())) {
                        Some(Value::Scalar { si_value, .. }) => *si_value,
                        _ => return Some(Value::Undef),
                    };
                    // Validate the stored parent kind — defense-in-depth against
                    // hand-built Map fixtures with invalid parent kinds.
                    // Extracting the kind as a &str (rather than a bool) means this
                    // is the single validation point; transform_at_simple_joint
                    // receives the already-validated kind and never re-reads it.
                    let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                        Some(Value::String(s))
                            if matches!(s.as_str(), "prismatic" | "revolute") =>
                        {
                            s.as_str()
                        }
                        _ => return Some(Value::Undef),
                    };
                    // Extract v_si from args[1] via dimension-appropriate helper;
                    // both helpers reject wrong-dim Scalars and non-finite values.
                    let v_si = if parent_kind == "prismatic" {
                        match length_input(&args[1]) {
                            Some(d) => d,
                            None => return Some(Value::Undef),
                        }
                    } else {
                        match trig_input(&args[1]) {
                            Some(t) => t,
                            None => return Some(Value::Undef),
                        }
                    };
                    // Defense-in-depth: length_input/trig_input already reject
                    // non-finite v; this guard mirrors the prismatic/revolute arms.
                    if !v_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    // Derive the coupled motion variable: ratio * v + offset
                    let coupled_si = ratio_f64 * v_si + offset_si;
                    if !coupled_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    let coupled_value = if parent_kind == "prismatic" {
                        Value::length(coupled_si)
                    } else {
                        Value::angle(coupled_si)
                    };
                    // Delegate to the parent joint via the private helper.
                    // Termination is guaranteed: `couple` rejects coupling parents
                    // at construction, so the recursion always reaches a
                    // prismatic/revolute arm at depth 1.
                    transform_at_simple_joint(parent_kind, parent_map, &coupled_value)
                }
                _ => Value::Undef,
            }
        }
        "couple" => {
            // Validate arg count: 2 or 3
            if args.len() != 2 && args.len() != 3 {
                return Some(Value::Undef);
            }
            // Validate parent: must be a Map with kind in {"prismatic", "revolute"}
            let parent_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            let is_prismatic = match parent_kind {
                "prismatic" => true,
                "revolute" => false,
                // Rejects "coupling" and any other kind
                _ => return Some(Value::Undef),
            };
            // Extract ratio: finite, dimensionless numeric (Real/Int/DIMENSIONLESS Scalar).
            // ratio_input rejects NaN, Inf, and dimensioned Scalars.
            let ratio_f64 = match ratio_input(&args[1]) {
                Some(r) => r,
                None => return Some(Value::Undef),
            };
            // Extract offset: use parent-dimension-keyed helper (length_input / trig_input)
            // so bare Real/Int is accepted in addition to correctly-dimensioned Scalar.
            let offset_si = if args.len() == 3 {
                if is_prismatic {
                    match length_input(&args[2]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    }
                } else {
                    match trig_input(&args[2]) {
                        Some(r) => r,
                        None => return Some(Value::Undef),
                    }
                }
            } else {
                0.0
            };
            let offset = if is_prismatic {
                Value::length(offset_si)
            } else {
                Value::angle(offset_si)
            };
            make_coupling(args[0].clone(), Value::Real(ratio_f64), offset)
        }
        // ── Coupling specialisations (PRD task 8) ──────────────────────────────
        //
        // `screw`, `gear`, and `rack_and_pinion` are thin parameterised wrappers
        // around `couple()`.  Each computes a dimensionless f64 ratio from its
        // domain parameters and delegates to `couple(parent, Value::Real(ratio))`.
        // Parent-kind validation (must be prismatic/revolute, no nested coupling,
        // etc.) is intentionally delegated to `couple()` — if `couple` returns
        // `Value::Undef`, the wrapper propagates that Undef unchanged.
        // See design decisions in the task-2676 plan.json.
        //
        // End-to-end coverage lives in crates/reify-eval/tests/kinematic_stdlib_smoke.rs.

        // `screw(parent, lead)` — wrap a prismatic driving joint as a screw.
        //
        // `lead` (metres per 2π coupling-input units) is converted to a
        // dimensionless ratio via `lead_si / (2π)` and delegated to `couple()`.
        // The canonical use passes a prismatic parent (linear input → scaled linear
        // output per turn).  Like all coupling specialisations, this wrapper is
        // dimension-agnostic — `couple()` also accepts a revolute parent, but
        // angular→angular is not a screw.  Parent-kind validation is intentionally
        // delegated to `couple()`.
        // PRD task 8: `Coupling(rotation, translation, ratio = lead / 2π)`.
        "screw" => {
            // Arity: exactly 2 args (parent, lead).
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            // Parse lead as a LENGTH value (SI metres).
            let lead_si = match length_input(&args[1]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Compute the dimensionless ratio: lead / (2π).
            let ratio = lead_si / (2.0 * std::f64::consts::PI);
            // Invariant: length_input rejects non-finite leads, so ratio is finite by
            // construction. assert! preserves this defense-in-depth in release builds.
            assert!(
                ratio.is_finite(),
                "screw: ratio must be finite — length_input rejects non-finite leads"
            );
            // Delegate to couple() — parent validation and Map layout come from there.
            crate::eval_builtin("couple", &[args[0].clone(), Value::Real(ratio)])
        }

        // `gear(parent, teeth_a, teeth_b)` — wrap a revolute driving joint as a gear pair.
        //
        // Both tooth counts must be `Value::Int` with strictly positive values.
        // `ratio = -(teeth_b as f64) / (teeth_a as f64)` (negative for external mesh).
        // f64 precision: integers are represented exactly up to 2^53 ≈ 9×10^15;
        // all real gear tooth counts are far below this limit.
        // The canonical use passes a revolute parent (angular input → scaled angular
        // output).  Like all coupling specialisations, this wrapper is
        // dimension-agnostic — `couple()` also accepts a prismatic parent, but
        // linear→linear is not a gear.  Parent-kind validation is intentionally
        // delegated to `couple()`.
        // PRD task 8: `Coupling(rotation_a, rotation_b, ratio = -teeth_b / teeth_a)`.
        "gear" => {
            // Arity: exactly 3 args (parent, teeth_a, teeth_b).
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            // Require teeth_a: strictly positive Int (prevents division by zero).
            let teeth_a = match &args[1] {
                Value::Int(i) if *i > 0 => *i,
                _ => return Some(Value::Undef),
            };
            // Require teeth_b: strictly positive Int (symmetry + non-physical 0-tooth rejected).
            let teeth_b = match &args[2] {
                Value::Int(i) if *i > 0 => *i,
                _ => return Some(Value::Undef),
            };
            // Compute ratio: negative for external mesh (reverses direction).
            let ratio = -(teeth_b as f64) / (teeth_a as f64);
            // Both teeth counts are strictly positive finite integers, so ratio is always finite
            // (i64-to-f64 is exact up to 2^53; division of two finite non-zero f64s is finite).
            assert!(
                ratio.is_finite(),
                "gear: ratio must be finite — teeth_a and teeth_b are validated positive Ints"
            );
            // Delegate to couple() — parent validation and Map layout come from there.
            crate::eval_builtin("couple", &[args[0].clone(), Value::Real(ratio)])
        }

        // `rack_and_pinion(parent, pitch_radius)` — wrap a prismatic driving joint as a rack-and-pinion.
        //
        // `pitch_radius` (metres) becomes the dimensionless coupling ratio directly:
        // `ratio = pitch_radius_si`.  The canonical use passes a prismatic parent
        // (linear input → `pitch_radius * linear` output).  Like all coupling
        // specialisations, this wrapper is dimension-agnostic — `couple()` also accepts
        // a revolute parent (angular→scaled angular), but that is not a rack-and-pinion.
        // Note: the coupling motion variable is interpreted as a length under the
        // current `couple` semantics.  Parent-kind validation is intentionally
        // delegated to `couple()`.
        // PRD task 8: `Coupling(rotation, translation, ratio = pitch_radius)`.
        // Identifier uses underscores because Reify identifiers do not allow hyphens.
        "rack_and_pinion" => {
            // Arity: exactly 2 args (parent, pitch_radius).
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            // Parse pitch_radius as a LENGTH value (SI metres).
            let pitch_radius_si = match length_input(&args[1]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // ratio = pitch_radius (dimensionless scaling factor for the coupling).
            let ratio = pitch_radius_si;
            // Invariant: length_input rejects non-finite pitch radii, so ratio is finite by
            // construction. assert! preserves this defense-in-depth in release builds.
            assert!(
                ratio.is_finite(),
                "rack_and_pinion: ratio must be finite — length_input rejects non-finite pitch_radius"
            );
            // Delegate to couple() — parent validation and Map layout come from there.
            crate::eval_builtin("couple", &[args[0].clone(), Value::Real(ratio)])
        }

        "joint_axis" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            // Returns the axis as stored at construction — the raw, potentially
            // unnormalized input vector.  `transform_at` normalizes to unit
            // length when computing the resulting Transform; this accessor
            // preserves the original value so callers can inspect what was
            // passed to `prismatic`/`revolute`.
            match &args[0] {
                Value::Map(m) => m
                    .get(&Value::String("axis".to_string()))
                    .cloned()
                    .unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }
        "joint_range" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => m
                    .get(&Value::String("range".to_string()))
                    .cloned()
                    .unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }
        "joint_ratio" => {
            // Returns the ratio stored in a coupling Map, or Undef for any other
            // input (including prismatic/revolute joints which have no ratio key).
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => m
                    .get(&Value::String("ratio".to_string()))
                    .cloned()
                    .unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }
        "joint_offset" => {
            // Returns the offset stored in a coupling Map, or Undef for any other
            // input (including prismatic/revolute joints which have no offset key).
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => m
                    .get(&Value::String("offset".to_string()))
                    .cloned()
                    .unwrap_or(Value::Undef),
                _ => Value::Undef,
            }
        }
        "joint_jacobian" => {
            // SE(3) twist column for a joint, returned as
            // `Map { "angular": Vector3<DIMENSIONLESS>, "linear": Vector3<DIMENSIONLESS> }`.
            //
            // Per-kind formula (constant w.r.t. the motion variable for v0.1
            // single-DOF joints):
            //   prismatic → angular=[0,0,0], linear=axis_unit
            //   revolute  → angular=axis_unit, linear=[0,0,0]
            //   coupling  → ratio * parent_jacobian (component-wise).
            //   fixed     → zero column placeholder (0-DOF joint).
            //   planar    → zero column placeholder (3-DOF deferred); the
            //               per-DOF wrench contributions are NOT yet computed
            //               by `chain_jacobian_fd` either: `value_for_joint`
            //               returns None for planar, so chain Jacobians for
            //               planar chains currently return None too. Deferred
            //               to PRD v0.2 kinematic task 2 (taskmaster #2670 —
            //               "FD fallback for multi-DOF kinds").
            //
            // For multi-DOF joints (fixed, planar) the returned column is zero
            // rather than Undef: callers expecting a { angular, linear } Map get a
            // well-formed result. Do NOT assume a non-zero result for planar joints
            // or that chain_jacobian_fd succeeds for chains containing planar.
            //
            // Validation mirrors `transform_at`'s coupling arm: parent kind must
            // be prismatic/revolute (no nested couplings), ratio must be a
            // finite `Value::Real`, axis must be a non-zero finite Vector3.
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            joint_jacobian_value(&args[0])
        }
        _ => return None,
    })
}

/// Compute the SE(3) twist column for a joint Map.
///
/// Returns `Value::Map` on success, `Value::Undef` on any validation failure.
///
/// The coupling arm calls this recursively on its parent; termination is
/// guaranteed because `couple` rejects coupling parents at construction and the
/// nested-coupling guard inside this helper rejects any hand-built fixture
/// that violates the invariant.
fn joint_jacobian_value(value: &Value) -> Value {
    let map = match value {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return Value::Undef,
    };
    match kind {
        "prismatic" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            make_jacobian([0.0, 0.0, 0.0], [nax, nay, naz])
        }
        "revolute" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            make_jacobian([nax, nay, naz], [0.0, 0.0, 0.0])
        }
        // 0-DOF fixed joint: zero twist column — design decision.
        // Strictly a 0-DOF joint has a 6×0 Jacobian (zero columns), but the v0.1
        // single-DOF convention returns one Map per joint. Returning a zero-magnitude
        // column preserves the uniform `{ angular, linear }` shape across all kinds,
        // is semantically valid (no motion variable contributes any twist), and keeps
        // the existing drift-guard tests simple.
        "fixed" => make_jacobian([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        // 3-DOF planar joint: returns a `Value::List` of three analytic twist
        // columns (one per DOF) in `JointValue::Planar([x, y, theta])` storage
        // order — element [0] is the ∂x DOF column (angular=0, linear=unit_x),
        // element [1] is the ∂y DOF column (angular=0, linear=unit_y), and
        // element [2] is the ∂θ DOF column (angular=plane_normal, linear=0),
        // where plane_normal = unit_x × unit_y (the joint's plane orientation).
        //
        // Mirrors the cylindrical pattern at the `"cylindrical"` arm below —
        // the per-DOF List shape (vs. a single Map) is the FD-fallback trigger
        // for `loop_closure::per_joint_jacobian_local` (which calls
        // `twist_map_to_array` expecting a single Map). KCC-γ (PRD §5.3) folds
        // in the §2.1 δ residual via this analytic-J seam; SE(3) adjoint
        // transport into the chain origin is handled by `chain_jacobian_fd`
        // until KCC-θ/ι compose the per-joint columns analytically.
        "planar" => {
            let (unit_x, unit_y) = match unit_axes_xy_from_planar_map(map) {
                Some(pair) => pair,
                None => return Value::Undef,
            };
            // plane_normal = unit_x × unit_y (cross product).
            // Perpendicularity is guaranteed by `unit_axes_xy_from_planar_map`
            // (rejects parallel axes), so plane_normal is unit-norm up to
            // floating-point precision (no renormalisation needed).
            let plane_normal = [
                unit_x[1] * unit_y[2] - unit_x[2] * unit_y[1],
                unit_x[2] * unit_y[0] - unit_x[0] * unit_y[2],
                unit_x[0] * unit_y[1] - unit_x[1] * unit_y[0],
            ];
            let col_x = make_jacobian([0.0, 0.0, 0.0], unit_x);
            let col_y = make_jacobian([0.0, 0.0, 0.0], unit_y);
            let col_theta = make_jacobian(plane_normal, [0.0, 0.0, 0.0]);
            // KCC-γ §11.1 producer-side signal: log that the analytic-J
            // columns are *available* for a multi-DOF joint.  The columns
            // are returned to the caller but the chain Jacobian is still
            // composed via FD inside `chain_jacobian_fd` — KCC-θ/ι will
            // replace that path with SE(3) adjoint transport over these
            // columns.  The `kind` field carries the joint kind so
            // downstream consumers / test captures can filter on it.
            tracing::debug!(
                target: "reify_stdlib::joints",
                kind = kind,
                "joint_jacobian analytic columns available"
            );
            Value::List(vec![col_x, col_y, col_theta])
        }
        // 3-DOF spherical joint: returns a `Value::List` of three analytic
        // angular twist columns (body-frame basis tangents) — one per manifold
        // DOF. Spherical is axis-isotropic (no preferred direction in the
        // joint Map; see `make_spherical` below), so the local-frame Jacobian
        // columns at q = identity are exactly the body-frame basis vectors:
        //   [0] ∂ω_x DOF:  { angular: [1,0,0], linear: [0,0,0] }
        //   [1] ∂ω_y DOF:  { angular: [0,1,0], linear: [0,0,0] }
        //   [2] ∂ω_z DOF:  { angular: [0,0,1], linear: [0,0,0] }
        //
        // Sign convention: the column ordering and positive signs are pinned
        // by the `spherical_analytic_jacobian_matches_transform_log_convention`
        // regression test, which composes a chain `[spherical_joint()]` at
        // q = orient_axis_angle(+Z, +π/4), takes `transform_log` of the
        // resulting Transform, and confirms the twist's angular_z is positive
        // (matching col[2].angular_z = +1.0). This is the convention used by
        // the canonical log/exp round-trip test at geometry.rs:3625; future
        // changes to the analytic Jacobian must preserve it or the Newton
        // solver's residual gradient flips and KCC-γ loops diverge.
        //
        // SE(3) adjoint transport into the chain origin is handled
        // implicitly by `chain_jacobian_fd` (FD fallback) until KCC-θ/ι
        // compose the per-joint columns analytically. The Value::List shape
        // (vs. a single Map) signals to `per_joint_jacobian_local` to fall
        // back to FD (matches the cylindrical and planar patterns).
        //
        // Field validation is intentionally minimal: the result is identity
        // (body basis) regardless of the stored range_angle, so a hand-built
        // Map with a missing range_angle still returns the correct columns.
        "spherical" => {
            let col_x = make_jacobian([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
            let col_y = make_jacobian([0.0, 1.0, 0.0], [0.0, 0.0, 0.0]);
            let col_z = make_jacobian([0.0, 0.0, 1.0], [0.0, 0.0, 0.0]);
            // KCC-γ §11.1 producer-side signal: log that the analytic-J
            // columns are *available* for a multi-DOF joint.  See the planar
            // arm above for the available-vs-consumed distinction.
            tracing::debug!(
                target: "reify_stdlib::joints",
                kind = kind,
                "joint_jacobian analytic columns available"
            );
            Value::List(vec![col_x, col_y, col_z])
        }
        // 2-DOF cylindrical joint: returns a `Value::List` of two analytic twist
        // columns (one per DOF) — element [0] is the prismatic-DOF column
        // (angular=0, linear=unit_axis) and element [1] is the revolute-DOF
        // column (angular=unit_axis, linear=0). The per-DOF ordering invariant
        // ([0]=prismatic, [1]=revolute) is documented so future analytic-Jacobian
        // composition can rely on it (PRD task 5).
        //
        // The List-of-Maps shape (vs. a single Map) is non-Undef — passes the
        // `joint_jacobian_dispatches_for_every_joint_kind` coverage test — and
        // also naturally signals to `loop_closure::per_joint_jacobian_local`
        // (which calls `twist_map_to_array` expecting a single Map) to return
        // None, triggering the documented FD-fallback path. This contrasts with
        // the planar/spherical zero-twist Map placeholder pattern: cylindrical's
        // analytic per-DOF columns are simple enough to emit cleanly.
        "cylindrical" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            let prismatic_col = make_jacobian([0.0, 0.0, 0.0], [nax, nay, naz]);
            let revolute_col = make_jacobian([nax, nay, naz], [0.0, 0.0, 0.0]);
            // KCC-γ §11.1 producer-side signal: log that the analytic-J
            // columns are *available* for a multi-DOF joint.  See the planar
            // arm above for the available-vs-consumed distinction.
            tracing::debug!(
                target: "reify_stdlib::joints",
                kind = kind,
                "joint_jacobian analytic columns available"
            );
            Value::List(vec![prismatic_col, revolute_col])
        }
        "coupling" => {
            let parent_map = match map.get(&Value::String("parent".to_string())) {
                Some(Value::Map(pm)) => pm,
                _ => return Value::Undef,
            };
            let ratio_f64 = match map.get(&Value::String("ratio".to_string())) {
                Some(Value::Real(r)) => *r,
                _ => return Value::Undef,
            };
            if !ratio_f64.is_finite() {
                return Value::Undef;
            }
            // Defense-in-depth: reject nested coupling. `couple` rejects this at
            // construction, but a hand-built Map could carry it. Mirrors
            // `transform_at`'s parent-kind validation.
            match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) if matches!(s.as_str(), "prismatic" | "revolute") => {}
                _ => return Value::Undef,
            }
            // Recurse to the parent's Jacobian (always single-DOF at depth 1).
            let parent_jac = joint_jacobian_value(&Value::Map(parent_map.clone()));
            scale_jacobian(&parent_jac, ratio_f64)
        }
        _ => Value::Undef,
    }
}

/// Motion-subspace matrix S_i for a joint (per Featherstone (2008) §5.1).
///
/// Returns `Some(cols)` where each column is a [`SpatialVector6`] in Featherstone
/// motion-vector ordering `[ω; v]` (angular first, then linear), and `cols.len()`
/// equals the joint's DOF count:
///
/// | kind        | DOF |
/// |-------------|-----|
/// | prismatic   |  1  |
/// | revolute    |  1  |
/// | cylindrical |  2  |
/// | planar      |  3  |
/// | spherical   |  3  |
/// | fixed       |  0  |
///
/// Returns `None` for:
/// - non-Map inputs (e.g. `Value::Real`, `Value::Undef`)
/// - Maps without a `"kind"` string discriminator
/// - unknown/unrecognised joint kinds (e.g. `"sliding"`)
/// - coupling joints (kind `"coupling"`) — out of scope for v0.3; no single
///   motion-subspace exists for derived joints
/// - joints with a missing or malformed axis field (propagated via `?` from
///   [`unit_axis_from_map`])
/// - planar joints with non-perpendicular axes (propagated via `?` from
///   [`unit_axes_xy_from_planar_map`])
///
/// Reference: PRD §5.1 (motion-subspace per joint kind) and §12 Q4 (cylindrical
/// column ordering).
pub(crate) fn motion_subspace_columns(joint: &Value) -> Option<Vec<SpatialVector6>> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        // 3-DOF spherical joint: PRD §4.2 — axis-isotropic (no stored axis).
        // Columns are the body-frame basis vectors (pure-angular identity):
        //   col[0] = [e_x; 0] = [1,0,0, 0,0,0]
        //   col[1] = [e_y; 0] = [0,1,0, 0,0,0]
        //   col[2] = [e_z; 0] = [0,0,1, 0,0,0]
        // World-frame transformation is RNEA's responsibility via X_{p→i}.
        // No field validation needed — result is constant w.r.t. range_angle
        // (mirrors the joint_jacobian_value spherical arm pattern).
        "spherical" => Some(vec![
            SpatialVector6::from_angular_linear([1.0, 0.0, 0.0], [0.0; 3]),
            SpatialVector6::from_angular_linear([0.0, 1.0, 0.0], [0.0; 3]),
            SpatialVector6::from_angular_linear([0.0, 0.0, 1.0], [0.0; 3]),
        ]),
        // 3-DOF planar joint: PRD §5.1 — columns are [linear_axis_x, linear_axis_y,
        // angular_normal] where normal = unit_axis_x × unit_axis_y (cross product).
        // Ordering matches the `transform_at` planar motion-var order [x, y, theta].
        // Perpendicularity is enforced by `unit_axes_xy_from_planar_map` (returns None
        // for non-perpendicular axes), so the cross product yields a unit vector.
        "planar" => {
            let (ux, uy) = unit_axes_xy_from_planar_map(map)?;
            // Plane normal = ux × uy (3-component cross product).
            // Unit because ux ⊥ uy (perpendicularity guard enforced by the helper).
            let n = [
                ux[1] * uy[2] - ux[2] * uy[1],
                ux[2] * uy[0] - ux[0] * uy[2],
                ux[0] * uy[1] - ux[1] * uy[0],
            ];
            Some(vec![
                SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], ux),
                SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], uy),
                SpatialVector6::from_angular_linear(n, [0.0, 0.0, 0.0]),
            ])
        }
        // 2-DOF cylindrical joint: PRD §12 Q4 — columns are [translation, rotation]
        // matching JointValue::Cyl ordering.
        // Column 0 (translation/prismatic-equivalent): [0; unit_axis] — linear along axis.
        // Column 1 (rotation/revolute-equivalent): [unit_axis; 0] — angular about axis.
        "cylindrical" => {
            let [ax, ay, az] = unit_axis_from_map(map)?;
            Some(vec![
                SpatialVector6::from_angular_linear([0.0, 0.0, 0.0], [ax, ay, az]),
                SpatialVector6::from_angular_linear([ax, ay, az], [0.0, 0.0, 0.0]),
            ])
        }
        // 1-DOF revolute joint: PRD §5.1 — column = [unit_axis; 0].
        // Angular component is along the (unit-normalized) joint axis;
        // linear component is zero (revolute has no translational DOF).
        "revolute" => {
            let [ax, ay, az] = unit_axis_from_map(map)?;
            Some(vec![SpatialVector6::from_angular_linear(
                [ax, ay, az],
                [0.0, 0.0, 0.0],
            )])
        }
        // 1-DOF prismatic joint: PRD §5.1 — column = [0; unit_axis].
        // Linear component is along the (unit-normalized) joint axis;
        // angular component is zero (prismatic has no rotational DOF).
        "prismatic" => {
            let [ax, ay, az] = unit_axis_from_map(map)?;
            Some(vec![SpatialVector6::from_angular_linear(
                [0.0, 0.0, 0.0],
                [ax, ay, az],
            )])
        }
        // 0-DOF fixed joint: 6×0 motion-subspace (empty Vec).
        "fixed" => Some(Vec::new()),
        _ => None,
    }
}

/// Construct the canonical identity `Transform` for a 0-DOF fixed joint.
///
/// Returns `Value::Transform { rotation: Orientation(w=1, x=0, y=0, z=0),
/// translation: Vector([length(0.0); 3]) }`.
///
/// Shared by the 1-arg ergonomic path (`transform_at(fixed_joint)`, task 2688)
/// and the 2-arg chain-machinery path (`transform_at(fixed_joint, motion_var)`,
/// task 2687). Single source of truth — avoids literal drift if the identity
/// representation ever changes. Mirrors the `make_jacobian` / `make_joint` /
/// `make_planar` / `make_spherical` helper pattern in this file.
fn fixed_identity_transform() -> Value {
    Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    }
}

/// Build a Jacobian Map with the standard `{ "angular", "linear" }` layout,
/// where each value is a `Value::Vector` of three `Value::Real` components.
fn make_jacobian(angular: [f64; 3], linear: [f64; 3]) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("angular".to_string()),
        Value::Vector(vec![
            Value::Real(angular[0]),
            Value::Real(angular[1]),
            Value::Real(angular[2]),
        ]),
    );
    m.insert(
        Value::String("linear".to_string()),
        Value::Vector(vec![
            Value::Real(linear[0]),
            Value::Real(linear[1]),
            Value::Real(linear[2]),
        ]),
    );
    Value::Map(m)
}

/// Scale a Jacobian Map's `angular` and `linear` components by `ratio`.
///
/// Returns `Value::Map` on success, `Value::Undef` if any component is
/// non-finite after scaling or the input shape is malformed.
fn scale_jacobian(jac: &Value, ratio: f64) -> Value {
    let m = match jac {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    let read_vec3 = |key: &str| -> Option<[f64; 3]> {
        match m.get(&Value::String(key.to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => {
                let a = items[0].as_f64()?;
                let b = items[1].as_f64()?;
                let c = items[2].as_f64()?;
                Some([a, b, c])
            }
            _ => None,
        }
    };
    let ang = match read_vec3("angular") {
        Some(v) => v,
        None => return Value::Undef,
    };
    let lin = match read_vec3("linear") {
        Some(v) => v,
        None => return Value::Undef,
    };
    let scaled_ang = [ratio * ang[0], ratio * ang[1], ratio * ang[2]];
    let scaled_lin = [ratio * lin[0], ratio * lin[1], ratio * lin[2]];
    for v in scaled_ang.iter().chain(scaled_lin.iter()) {
        if !v.is_finite() {
            return Value::Undef;
        }
    }
    make_jacobian(scaled_ang, scaled_lin)
}

/// Extract a dimensionless ratio from a `couple` ratio argument.
///
/// Accepts:
/// - `Value::Scalar { dimension: DIMENSIONLESS, .. }` with finite si_value.
/// - `Value::Real(r)` (finite) — treated as dimensionless ratio directly.
/// - `Value::Int(i)` — treated as dimensionless ratio directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite, NaN, Inf).
fn ratio_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if *dimension == DimensionVector::DIMENSIONLESS && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Extract metres from a `transform_at` value argument for a Prismatic joint.
///
/// Accepts:
/// - `Value::Scalar { dimension: LENGTH, .. }` — si_value is metres.
/// - `Value::Real(r)` / `Value::Int(i)` — treated as metres directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite).
fn length_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if *dimension == DimensionVector::LENGTH && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Extract `(dist, theta)` from a cylindrical motion-variable argument.
///
/// Accepts a 2-element `Value::List` whose elements are:
/// - `items[0]`: a length scalar (LENGTH-dim Scalar, or bare Real/Int as metres)
/// - `items[1]`: an angle scalar (ANGLE-dim Scalar, or bare Real/Int as radians)
///
/// Returns `None` if:
/// - the container is not a 2-element `List`,
/// - either element fails its dimension/finiteness contract
///   (`length_input` / `trig_input`, which also reject NaN/Inf and the
///   wrong-dimension Scalar).
///
/// The dim-swap case `[angle, length]` is rejected for free: `length_input`
/// rejects an angle Scalar (wrong dim) and `trig_input` rejects a length Scalar.
///
/// Container shape: List-only, mirroring the planar arm (joints.rs ~200).
/// Reify list literals `[a, b]` lower to `Value::List`, so accepting a
/// `Value::Vector` here would be reachable only from internal callers and
/// would create an asymmetry with planar that future readers would have to
/// re-derive. Keeping the surface narrow keeps the joint-zoo consistent.
fn cylindrical_motion_vars(value: &Value) -> Option<(f64, f64)> {
    let items = match value {
        Value::List(items) if items.len() == 2 => items,
        _ => return None,
    };
    let dist = length_input(&items[0])?;
    let theta = trig_input(&items[1])?;
    Some((dist, theta))
}

/// Unit-normalise a 3-component array.
///
/// The caller must have already validated the input via [`validate_axis`],
/// which guarantees non-zero, finite magnitude — so the division is safe.
/// Shared by the `"planar"` constructor (perpendicularity check), by
/// [`unit_axes_xy_from_planar_map`], and by [`unit_axis_from_map`], so the
/// normalisation formula lives in exactly one place.
fn unit_normalize(comps: [f64; 3]) -> [f64; 3] {
    let mag = (comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2]).sqrt();
    [comps[0] / mag, comps[1] / mag, comps[2] / mag]
}

/// Extract and unit-normalise both `"axis_x"` and `"axis_y"` from a planar joint Map.
///
/// Returns `Some((unit_x, unit_y))` on success, `None` if either field is
/// missing, fails [`validate_axis`] validation, or the two axes are not
/// perpendicular (`|dot(unit_x, unit_y)| >= 1e-9`).
///
/// The perpendicularity predicate uses the same tolerance (1e-9) and formula as
/// the `"planar"` constructor arm of `eval_joints` (joints.rs:67-76), so any Map
/// the constructor accepts is also accepted here, and any Map this helper rejects
/// would have been rejected by the constructor.  The check is defence-in-depth:
/// the constructor rejects non-perpendicular axes before storing a Map, but
/// hand-built Maps can bypass the constructor and reach `transform_at` directly.
///
/// ## Why 1e-9?
///
/// `|dot(unit_x, unit_y)| < 1e-9` corresponds to axes within ~1e-9 rad
/// (~2e-4 arcsec) of true perpendicularity.  The threshold is deliberately
/// *tight* so that visibly non-perpendicular axes (e.g. dot ≈ 1e-6 ≈ 0.2 arcsec)
/// are rejected and surface as `Undef` rather than silently propagating as slop.
///
/// **Hand-typed unit vectors are unaffected.**  Exact unit vectors such as
/// `(1,0,0)` and `(0,1,0)` produce a literal `0.0` dot product in IEEE 754,
/// which trivially passes the 1e-9 guard.
///
/// **Typical FP-derived axes are also unaffected.**  Standard floating-point
/// chains (one or two normalisations, a rotation or two) accumulate roughly
/// 1e-15 error per operation — six orders of magnitude below the 1e-9 bound.
/// Only pathological chains (many successive rotations without re-normalisation)
/// can approach the bound; callers in that regime should pre-normalise their axes
/// (e.g. compute `axis_y = cross(axis_z, axis_x)` and re-normalise) rather than
/// relying on the constructor or this helper to absorb accumulated slop.
///
/// **Why not tune this threshold here?**  Per-purpose tolerances are explicitly
/// deferred to v0.2 — see `docs/prds/v0_2/per-purpose-tolerance.md` ("Status:
/// deferred to v0.2 per 2026-04-26 decision").  v0.1 carries a single global
/// tolerance with no per-purpose dispatch; hard-coded thresholds in joint
/// constructors will be revisited holistically when that infrastructure ships.
fn unit_axes_xy_from_planar_map(map: &BTreeMap<Value, Value>) -> Option<([f64; 3], [f64; 3])> {
    let axis_x_val = map.get(&Value::String("axis_x".to_string()))?;
    let axis_y_val = map.get(&Value::String("axis_y".to_string()))?;
    let cx = validate_axis(axis_x_val)?;
    let cy = validate_axis(axis_y_val)?;
    let unit_x = unit_normalize(cx);
    let unit_y = unit_normalize(cy);
    // Perpendicularity guard — mirrors the constructor check at joints.rs:67-76.
    // Parallel or anti-parallel axes produce a zero cross product, which would
    // yield a degenerate (all-zero) rotation axis in the `transform_at` planar
    // arm; return None so the caller propagates Undef instead of silently
    // producing an identity rotation.
    let dot = unit_x[0] * unit_y[0] + unit_x[1] * unit_y[1] + unit_x[2] * unit_y[2];
    if dot.abs() >= 1e-9 {
        return None;
    }
    Some((unit_x, unit_y))
}

/// Look up the `"axis"` field in a joint map, validate it via [`validate_axis`],
/// and return the unit-normalized `[x, y, z]` components.
///
/// Returns `None` if the field is absent or validation fails.
/// Both the `"prismatic"` and `"revolute"` arms of `transform_at` call this
/// helper to avoid duplicating the axis lookup and normalization logic.
fn unit_axis_from_map(map: &BTreeMap<Value, Value>) -> Option<[f64; 3]> {
    let axis_val = map.get(&Value::String("axis".to_string()))?;
    let comps = validate_axis(axis_val)?;
    Some(unit_normalize(comps))
}

/// Validate an axis value: must be a Vector3 of dimensionless components,
/// all finite, with non-zero magnitude.
///
/// Returns `Some([x, y, z])` (the raw components, not normalized) on success,
/// `None` on any failure.
///
/// Delegates to [`crate::helpers::validate_dimensionless_unit_axis_vec3`], the
/// shared helper that unifies axis-validation across the joints, supports, and
/// loads modules. The wrapper is preserved (rather than replacing every call
/// site) so the existing rustdoc cross-references in this file (e.g. on
/// `unit_axes_xy_from_planar_map` and `unit_axis_from_map`) remain valid.
fn validate_axis(value: &Value) -> Option<[f64; 3]> {
    crate::helpers::validate_dimensionless_unit_axis_vec3(value)
}

/// Validate a range value: must be `Value::Range` with both lower and upper
/// bounds present, both sharing `expected_dim`.
///
/// Returns `Some(())` on success, `None` on any failure.
fn validate_range(value: &Value, expected_dim: DimensionVector) -> Option<()> {
    match value {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => {
            if lo.dimension() == expected_dim && up.dimension() == expected_dim {
                Some(())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Canonical set of joint kinds recognized by the joints module.
///
/// Used by [`is_joint_value`] as the membership set for value-level
/// joint discrimination. Per-kind dispatch arms in `transform_at` and
/// `joint_jacobian_value` (this file) hardcode the same kind strings
/// directly in `match` arms — Rust `match` patterns do not support
/// runtime slice membership, so those arms **must be kept in sync with
/// this constant** when a new kind is added.
/// `mechanism::body()` validates joint-kind membership via `is_joint_value`.
///
/// Multi-DOF kinds: `"planar"` (3-DOF), `"spherical"` (3-DOF), and
/// `"cylindrical"` (2-DOF). These have explicit `None` arms in
/// `loop_closure::value_for_joint` and `loop_closure::joint_range_midpoint`
/// because the f64-per-joint signature of `chain_transform` /
/// `chain_jacobian_fd` cannot represent multi-DOF motion variables; see
/// `loop_closure::MULTI_DOF_KINDS`.
pub(crate) const JOINT_KINDS: &[&str] = &[
    "prismatic",
    "revolute",
    "coupling",
    "fixed",
    "planar",
    "spherical",
    "cylindrical",
];

/// Returns `true` when `v` is a `Value::Map` whose `kind` field is one of
/// the strings in [`JOINT_KINDS`]. Used by `mechanism::body()` for
/// `at`-arg validation and (combined with the world-sentinel check) for
/// parent-arg validation.
///
/// Tied to `JOINT_KINDS` via `contains` so a future kind addition only
/// needs to be made in the constant — the predicate follows automatically.
pub(crate) fn is_joint_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => matches!(
            m.get(&Value::String("kind".to_string())),
            Some(Value::String(s)) if JOINT_KINDS.contains(&s.as_str())
        ),
        _ => false,
    }
}

/// Build a coupling `Value::Map` with the four-key layout:
/// `"kind"`, `"offset"`, `"parent"`, `"ratio"`.
///
/// Keys are in alphabetical order as `BTreeMap` sorts them, matching the
/// pattern of `make_joint`.  `ratio` is stored as `Value::Real` (already
/// extracted to f64 by the caller).
fn make_coupling(parent: Value, ratio: Value, offset: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("coupling".to_string()),
    );
    m.insert(Value::String("offset".to_string()), offset);
    m.insert(Value::String("parent".to_string()), parent);
    m.insert(Value::String("ratio".to_string()), ratio);
    Value::Map(m)
}

/// Build a planar joint `Value::Map` with the six-key layout:
/// `"axis_x"`, `"axis_y"`, `"kind"`, `"range_theta"`, `"range_x"`, `"range_y"`.
///
/// Keys are in BTreeMap alphabetical order.  Raw (potentially unnormalised) axes
/// are stored — normalisation happens at `transform_at` time, matching the
/// prismatic/revolute `make_joint` precedent.
fn make_planar(
    axis_x: Value,
    axis_y: Value,
    range_x: Value,
    range_y: Value,
    range_theta: Value,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("planar".to_string()),
    );
    m.insert(Value::String("axis_x".to_string()), axis_x);
    m.insert(Value::String("axis_y".to_string()), axis_y);
    m.insert(Value::String("range_x".to_string()), range_x);
    m.insert(Value::String("range_y".to_string()), range_y);
    m.insert(Value::String("range_theta".to_string()), range_theta);
    Value::Map(m)
}

/// Build a spherical joint `Value::Map` with the two-key layout:
/// `"kind"`, `"range_angle"`.
///
/// Keys are in BTreeMap alphabetical order. The spherical joint is axis-isotropic
/// (no preferred direction), so no axis is stored. The `range_angle` value is
/// a `Value::Range` of `Angle`-dimensioned bounds — see PRD task 4 design
/// decision: the range bounds the rotation magnitude (axis-angle / cone
/// half-angle) regardless of the axis direction.
fn make_spherical(range_angle: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("spherical".to_string()),
    );
    m.insert(Value::String("range_angle".to_string()), range_angle);
    Value::Map(m)
}

/// Build a cylindrical joint `Value::Map` with the four-key layout:
/// `"axis"`, `"kind"`, `"rotation_range"`, `"translation_range"`.
///
/// Keys are in `BTreeMap` alphabetical order, mirroring `make_joint` /
/// `make_planar` / `make_spherical`.  The raw (unnormalised) axis is
/// stored; normalisation happens at `transform_at` / `joint_jacobian`
/// time, matching the prismatic / revolute precedent.
///
/// Design intent (PRD task 5): the cylindrical joint is the flat composite
/// of prismatic ⊕ revolute on a single shared axis.  Storing translation_range
/// and rotation_range at the top level (rather than nesting prismatic/revolute
/// children) avoids axis duplication and keeps `joint_axis` working unchanged.
fn make_cylindrical_joint(axis: Value, translation_range: Value, rotation_range: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("cylindrical".to_string()),
    );
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(
        Value::String("translation_range".to_string()),
        translation_range,
    );
    m.insert(Value::String("rotation_range".to_string()), rotation_range);
    Value::Map(m)
}

/// Build a joint `Value::Map` with the standard three-key layout:
/// `"kind"`, `"axis"`, `"range"`.
fn make_joint(kind: &str, axis: Value, range: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String(kind.to_string()),
    );
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(Value::String("range".to_string()), range);
    Value::Map(m)
}

/// Build a quaternion `Value::Orientation` from a **pre-normalized** unit axis
/// `(nax, nay, naz)` and a rotation angle `theta` in radians.
///
/// Delegates to [`normalize_quaternion`] for a final unit-norm check to absorb
/// floating-point drift from the sin/cos computation.  Returns `Value::Undef`
/// only if inputs are non-finite or the computed norm underflows — both
/// unreachable in practice for finite, unit-magnitude axis inputs.
///
/// This mirrors the axis-angle path in `orientation::eval_orientation`
/// (`orient_axis_angle`).  A future scope expansion to `orientation.rs` can
/// lift this to `orientation::axis_angle_quaternion` and share it from both
/// call sites, eliminating the remaining duplication.
fn axis_angle_quaternion(nax: f64, nay: f64, naz: f64, theta: f64) -> Value {
    let half = theta / 2.0;
    let c = half.cos();
    let s = half.sin();
    normalize_quaternion(c, s * nax, s * nay, s * naz).unwrap_or(Value::Undef)
}

/// Evaluate `transform_at` for a simple (prismatic or revolute) joint map.
///
/// Dispatches on the pre-validated `kind` string (`"prismatic"` or `"revolute"`).
/// The caller is responsible for validating that `kind` is one of these two values;
/// passing a pre-validated `kind` keeps joint-kind validation in exactly one place
/// (the caller's match / guard) so a future new simple-joint kind only needs to be
/// added to the caller and this match — not to both separately.
/// Returns `Value::Undef` as a defence-in-depth fallback for any unrecognised kind,
/// and for any missing axis or invalid value argument.
///
/// This helper is also the terminal dispatch target for the coupling arm of
/// `transform_at` — `couple` rejects coupling parents at construction, so the
/// recursion always reaches this helper at depth 1, guaranteeing termination.
fn transform_at_simple_joint(kind: &str, map: &BTreeMap<Value, Value>, value: &Value) -> Value {
    match kind {
        "prismatic" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            // Accept Length Scalar or bare Real/Int as metres
            let dist = match length_input(value) {
                Some(d) => d,
                None => return Value::Undef,
            };
            // length_input already enforces finiteness for the Scalar/Real
            // branches; the Int branch yields finite f64 by construction.
            // This guard is defense-in-depth against future changes to
            // length_input.
            if !dist.is_finite() {
                return Value::Undef;
            }
            let translation = Value::Vector(vec![
                Value::length(dist * nax),
                Value::length(dist * nay),
                Value::length(dist * naz),
            ]);
            let rotation = Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            Value::Transform {
                rotation: Box::new(rotation),
                translation: Box::new(translation),
            }
        }
        "revolute" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            // Accept Angle Scalar or bare Real/Int as radians
            let theta = match trig_input(value) {
                Some(t) => t,
                None => return Value::Undef,
            };
            // trig_input already enforces finiteness for the Scalar/Real
            // branches; the Int branch yields finite f64 by construction.
            // This guard is defense-in-depth against future changes to
            // trig_input (parallel to the same guard in the prismatic arm).
            if !theta.is_finite() {
                return Value::Undef;
            }
            let rotation = axis_angle_quaternion(nax, nay, naz, theta);
            let translation = Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ]);
            Value::Transform {
                rotation: Box::new(rotation),
                translation: Box::new(translation),
            }
        }
        _ => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use super::{JOINT_KINDS, is_joint_value};
    use crate::eval_builtin;
    use crate::test_fixtures::{
        angle_range_0_to_pi, axis_x_unit, axis_y_unit, axis_z_unit, cylindrical_z_joint,
        length_range_0_to_1m, planar_xy_joint, spherical_joint,
    };
    use reify_types::{DimensionVector, Value};

    // ── prismatic constructor: happy path ────────────────────────────────────

    #[test]
    fn prismatic_returns_map_with_correct_fields() {
        let axis = axis_x_unit();
        let range = length_range_0_to_1m();
        let result = eval_builtin("prismatic", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("prismatic".to_string())),
            "kind field should be 'prismatic'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── prismatic constructor: wrong arg counts ──────────────────────────────

    #[test]
    fn prismatic_zero_args_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn prismatic_one_arg_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[axis_x_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── revolute constructor helpers ─────────────────────────────────────────

    // ── revolute constructor: happy path ─────────────────────────────────────

    #[test]
    fn revolute_returns_map_with_correct_fields() {
        let axis = axis_z_unit();
        let range = angle_range_0_to_pi();
        let result = eval_builtin("revolute", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("revolute".to_string())),
            "kind field should be 'revolute'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── revolute constructor: wrong arg counts ───────────────────────────────

    #[test]
    fn revolute_zero_args_returns_undef() {
        assert!(
            eval_builtin("revolute", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn revolute_one_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── prismatic validation: axis ───────────────────────────────────────────

    #[test]
    fn prismatic_non_vector_axis_returns_undef() {
        // axis is a bare Real, not a Vector3
        assert!(
            eval_builtin("prismatic", &[Value::Real(1.0), length_range_0_to_1m()]).is_undef(),
            "non-vector axis should return Undef"
        );
    }

    #[test]
    fn prismatic_vec2_axis_returns_undef() {
        // axis has 2 components, not 3
        let axis2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[axis2, length_range_0_to_1m()]).is_undef(),
            "2-component axis should return Undef"
        );
    }

    #[test]
    fn prismatic_length_dimensioned_axis_returns_undef() {
        // axis components are LENGTH-dimensioned, not dimensionless
        let axis_len = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[axis_len, length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned axis should return Undef"
        );
    }

    #[test]
    fn prismatic_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[zero_axis, length_range_0_to_1m()]).is_undef(),
            "zero-magnitude axis should return Undef"
        );
    }

    #[test]
    fn prismatic_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[nan_axis, length_range_0_to_1m()]).is_undef(),
            "NaN axis should return Undef"
        );
    }

    // ── prismatic validation: range ──────────────────────────────────────────

    #[test]
    fn prismatic_non_range_arg_returns_undef() {
        // range arg is a bare Real
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg should return Undef"
        );
    }

    #[test]
    fn prismatic_unbounded_range_returns_undef() {
        // range is missing upper bound
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), unbounded]).is_undef(),
            "unbounded range should return Undef"
        );
    }

    #[test]
    fn prismatic_angle_range_returns_undef() {
        // range dimension is Angle, not Length — dimension mismatch
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), angle_range_0_to_pi()]).is_undef(),
            "Angle-dimensioned range for Prismatic should return Undef"
        );
    }

    // ── revolute validation: axis ────────────────────────────────────────────

    #[test]
    fn revolute_non_vector_axis_returns_undef() {
        assert!(
            eval_builtin("revolute", &[Value::Real(1.0), angle_range_0_to_pi()]).is_undef(),
            "non-vector axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_vec2_axis_returns_undef() {
        let axis2 = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("revolute", &[axis2, angle_range_0_to_pi()]).is_undef(),
            "2-component axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_dimensioned_axis_returns_undef() {
        let axis_len = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(1.0),
        ]);
        assert!(
            eval_builtin("revolute", &[axis_len, angle_range_0_to_pi()]).is_undef(),
            "Length-dimensioned axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("revolute", &[zero_axis, angle_range_0_to_pi()]).is_undef(),
            "zero-magnitude axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(f64::NAN),
        ]);
        assert!(
            eval_builtin("revolute", &[nan_axis, angle_range_0_to_pi()]).is_undef(),
            "NaN axis for revolute should return Undef"
        );
    }

    // ── revolute validation: range ───────────────────────────────────────────

    #[test]
    fn revolute_non_range_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_unbounded_range_returns_undef() {
        let unbounded = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: false,
            upper_inclusive: true,
        };
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), unbounded]).is_undef(),
            "unbounded range for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_range_returns_undef() {
        // range dimension is Length, not Angle — dimension mismatch
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned range for revolute should return Undef"
        );
    }

    // ── validate_range: inverted range is intentionally permissive ───────────

    #[test]
    fn prismatic_with_inverted_range_constructs_ok() {
        // validate_range only checks that both bounds are present and
        // dimensionally consistent; ordering (lo > up) is intentionally
        // permissive.  The range field is informational metadata used by
        // callers (e.g. a sweep step), not validated for geometric sense at
        // construction time.  This test pins that permissive behaviour so
        // any future tightening is a deliberate, visible change.
        let inverted = Value::Range {
            lower: Some(Box::new(Value::length(5.0))),
            upper: Some(Box::new(Value::length(0.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let result = eval_builtin("prismatic", &[axis_x_unit(), inverted]);
        assert!(
            matches!(result, Value::Map(_)),
            "inverted-range prismatic should construct successfully, got {:?}",
            result
        );
    }

    // ── transform_at on Prismatic: helpers ───────────────────────────────────

    fn prismatic_x_joint() -> Value {
        eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()])
    }

    fn prismatic_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    fn prismatic_z_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(-5.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    /// Assert two `Value::Transform` are component-wise within tolerance.
    fn assert_transform_approx(
        result: &Value,
        exp_rot: (f64, f64, f64, f64),
        exp_trans: [f64; 3],
        tol: f64,
        label: &str,
    ) {
        let (rot, trans) = match result {
            Value::Transform {
                rotation,
                translation,
            } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("{}: expected Transform, got {:?}", label, other),
        };
        let (w, x, y, z) = match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("{}: expected Orientation, got {:?}", label, other),
        };
        assert!(
            (w - exp_rot.0).abs() < tol,
            "{}: rotation.w expected {} got {}",
            label,
            exp_rot.0,
            w
        );
        assert!(
            (x - exp_rot.1).abs() < tol,
            "{}: rotation.x expected {} got {}",
            label,
            exp_rot.1,
            x
        );
        assert!(
            (y - exp_rot.2).abs() < tol,
            "{}: rotation.y expected {} got {}",
            label,
            exp_rot.2,
            y
        );
        assert!(
            (z - exp_rot.3).abs() < tol,
            "{}: rotation.z expected {} got {}",
            label,
            exp_rot.3,
            z
        );

        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("{}: expected Vector(3), got {:?}", label, other),
        };
        for (i, (comp, &exp)) in comps.iter().zip(exp_trans.iter()).enumerate() {
            let val = comp
                .as_f64()
                .unwrap_or_else(|| panic!("{}: translation[{}] not numeric", label, i));
            assert!(
                (val - exp).abs() < tol,
                "{}: translation[{}] expected {} got {}",
                label,
                i,
                exp,
                val
            );
        }
    }

    // ── transform_at on Prismatic: analytic tests ────────────────────────────

    #[test]
    fn prismatic_transform_at_x_axis_5m() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(5.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [5.0, 0.0, 0.0],
            1e-12,
            "prismatic X, 5m",
        );
    }

    #[test]
    fn prismatic_transform_at_y_axis_3m() {
        let joint = prismatic_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(3.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 3.0, 0.0],
            1e-12,
            "prismatic Y, 3m",
        );
    }

    #[test]
    fn prismatic_transform_at_z_axis_neg2m() {
        let joint = prismatic_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(-2.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, -2.0],
            1e-12,
            "prismatic Z, -2m",
        );
    }

    #[test]
    fn prismatic_transform_at_zero_value() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(0.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "prismatic X, 0m",
        );
    }

    #[test]
    fn prismatic_transform_at_diagonal_axis() {
        // axis = [1,1,0]/√2, value = √2 m → translation = [1m, 1m, 0m]
        let sq2 = std::f64::consts::SQRT_2;
        let axis = Value::Vector(vec![
            Value::Real(1.0 / sq2),
            Value::Real(1.0 / sq2),
            Value::Real(0.0),
        ]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(10.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(sq2)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [1.0, 1.0, 0.0],
            1e-12,
            "prismatic diagonal [1,1,0]/√2, √2 m",
        );
    }

    #[test]
    fn prismatic_transform_at_unnormalized_axis() {
        // axis = [2, 0, 0] (magnitude 2), value = 1m → normalized axis [1,0,0] → translation = [1m, 0, 0]
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(1.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [1.0, 0.0, 0.0],
            1e-12,
            "prismatic unnormalized [2,0,0], 1m",
        );
    }

    #[test]
    fn prismatic_transform_at_bare_real_value() {
        // bare Value::Real(0.5) accepted as 0.5 metres
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(0.5)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "prismatic X, bare Real(0.5)",
        );
    }

    // ── transform_at on Revolute: helpers ────────────────────────────────────

    fn revolute_z_joint() -> Value {
        eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()])
    }

    fn revolute_x_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    fn revolute_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    // ── transform_at on Revolute: analytic tests ─────────────────────────────

    #[test]
    fn revolute_transform_at_z_axis_half_pi() {
        // Z axis, π/2 → rotation = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute Z, π/2",
        );
    }

    #[test]
    fn revolute_transform_at_x_axis_pi() {
        // X axis, π → rotation = (0, 1, 0, 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi)]);
        assert_transform_approx(
            &result,
            (0.0, 1.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute X, π",
        );
    }

    #[test]
    fn revolute_transform_at_y_axis_half_pi() {
        // Y axis, π/2 → rotation = (cos(π/4), 0, sin(π/4), 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, sin, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute Y, π/2",
        );
    }

    #[test]
    fn revolute_transform_at_zero_angle() {
        // angle = 0 → identity rotation
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(0.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute Z, 0",
        );
    }

    #[test]
    fn revolute_transform_at_bare_real_value() {
        // bare Real(π/2) accepted as radians
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute Z, bare Real(π/2)",
        );
    }

    #[test]
    fn revolute_transform_at_unnormalized_axis() {
        // axis [0, 0, 2] (magnitude 2) with π/2 → same rotation as [0,0,1] with π/2
        let pi = std::f64::consts::PI;
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(pi))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "revolute unnormalized [0,0,2], π/2",
        );
    }

    #[test]
    fn revolute_transform_at_translation_always_zero() {
        // translation should always be [0m, 0m, 0m] regardless of angle
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 3.0)]);
        // Check only translation components
        let trans = match &result {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Transform, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("expected Vector(3), got {:?}", other),
        };
        for (i, comp) in comps.iter().enumerate() {
            let val = comp
                .as_f64()
                .expect("translation component should be numeric");
            assert!(
                (val - 0.0).abs() < 1e-12,
                "revolute translation[{}] should be 0, got {}",
                i,
                val
            );
        }
    }

    // ── transform_at validation ──────────────────────────────────────────────

    #[test]
    fn transform_at_prismatic_with_angle_value_returns_undef() {
        // Angle Scalar passed to a Prismatic joint
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::angle(1.0)]).is_undef(),
            "Angle Scalar to Prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_length_value_returns_undef() {
        // Length Scalar passed to a Revolute joint
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::length(1.0)]).is_undef(),
            "Length Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let joint = revolute_z_joint();
        let mass = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert!(
            eval_builtin("transform_at", &[joint, mass]).is_undef(),
            "Mass Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_non_map_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[Value::Real(1.0), Value::length(1.0)]).is_undef(),
            "non-Map first arg should return Undef"
        );
    }

    #[test]
    fn transform_at_map_without_kind_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map without kind key should return Undef"
        );
    }

    #[test]
    fn transform_at_map_with_unknown_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map with unknown kind should return Undef"
        );
    }

    #[test]
    fn transform_at_prismatic_nan_value_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::NAN)]).is_undef(),
            "NaN value for prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_inf_value_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value for revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_zero_args_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[]).is_undef(),
            "0 args should return Undef"
        );
    }

    #[test]
    fn transform_at_one_arg_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn transform_at_three_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin(
                "transform_at",
                &[joint, Value::length(1.0), Value::Real(0.0)]
            )
            .is_undef(),
            "3 args should return Undef"
        );
    }

    // ── joint_ratio accessor ────────────────────────────────────────────────

    #[test]
    fn joint_ratio_prismatic_coupling_2arg_returns_ratio() {
        // 2-arg prismatic coupling: ratio stored as Value::Real(-1.0)
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(-1.0)]);
        assert_eq!(
            eval_builtin("joint_ratio", &[c]),
            Value::Real(-1.0),
            "joint_ratio should return the stored ratio Value::Real(-1.0)"
        );
    }

    #[test]
    fn joint_ratio_prismatic_coupling_3arg_returns_ratio() {
        // 3-arg prismatic coupling: ratio = 2.0
        let c = eval_builtin(
            "couple",
            &[prismatic_x_joint(), Value::Real(2.0), Value::length(0.5)],
        );
        assert_eq!(
            eval_builtin("joint_ratio", &[c]),
            Value::Real(2.0),
            "joint_ratio should return Value::Real(2.0)"
        );
    }

    #[test]
    fn joint_offset_prismatic_coupling_default_returns_length_zero() {
        // 2-arg form: default offset is Value::length(0.0)
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::length(0.0),
            "joint_offset default for prismatic should be Value::length(0.0)"
        );
    }

    #[test]
    fn joint_offset_prismatic_coupling_explicit_returns_stored_offset() {
        let c = eval_builtin(
            "couple",
            &[prismatic_x_joint(), Value::Real(1.0), Value::length(0.5)],
        );
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::length(0.5),
            "joint_offset should return Value::length(0.5)"
        );
    }

    #[test]
    fn joint_offset_revolute_coupling_returns_angle_offset() {
        let pi = std::f64::consts::PI;
        let c = eval_builtin(
            "couple",
            &[revolute_z_joint(), Value::Real(1.0), Value::angle(pi / 4.0)],
        );
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::angle(pi / 4.0),
            "joint_offset should return Value::angle(PI/4)"
        );
    }

    #[test]
    fn joint_ratio_prismatic_joint_returns_undef() {
        // Prismatic has no "ratio" key → Undef
        assert!(
            eval_builtin("joint_ratio", &[prismatic_x_joint()]).is_undef(),
            "joint_ratio of prismatic should return Undef"
        );
    }

    #[test]
    fn joint_offset_prismatic_joint_returns_undef() {
        // Prismatic has no "offset" key → Undef
        assert!(
            eval_builtin("joint_offset", &[prismatic_x_joint()]).is_undef(),
            "joint_offset of prismatic should return Undef"
        );
    }

    #[test]
    fn joint_ratio_non_map_returns_undef() {
        assert!(
            eval_builtin("joint_ratio", &[Value::Real(1.0)]).is_undef(),
            "joint_ratio of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_ratio_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_ratio", &[]).is_undef(),
            "joint_ratio with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_offset_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_offset", &[]).is_undef(),
            "joint_offset with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_ratio_two_args_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("joint_ratio", &[c, Value::Real(0.0)]).is_undef(),
            "joint_ratio with 2 args should return Undef"
        );
    }

    #[test]
    fn joint_offset_two_args_returns_undef() {
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("joint_offset", &[c, Value::Real(0.0)]).is_undef(),
            "joint_offset with 2 args should return Undef"
        );
    }

    // ── joint_axis accessor ──────────────────────────────────────────────────

    #[test]
    fn joint_axis_prismatic_returns_stored_axis() {
        let axis = axis_x_unit();
        let joint = eval_builtin("prismatic", &[axis.clone(), length_range_0_to_1m()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(prismatic) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_revolute_returns_stored_axis() {
        let axis = axis_z_unit();
        let joint = eval_builtin("revolute", &[axis.clone(), angle_range_0_to_pi()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(revolute) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[Value::Real(1.0)]).is_undef(),
            "joint_axis of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_axis_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[]).is_undef(),
            "joint_axis with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_axis_two_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("joint_axis", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_axis with 2 args should return Undef"
        );
    }

    // ── couple constructor: happy paths ─────────────────────────────────────

    #[test]
    fn couple_prismatic_2arg_returns_coupling_map() {
        // 2-arg form: couple(prismatic, ratio) → Map with kind="coupling",
        // parent=<prismatic Map>, ratio=Value::Real(-1.0),
        // offset=Value::length(0.0) (default zero in LENGTH dimension)
        let parent = prismatic_x_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(-1.0)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(-1.0)),
            "ratio should be Value::Real(-1.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(0.0)),
            "default offset for prismatic should be Value::length(0.0)"
        );
    }

    #[test]
    fn couple_prismatic_3arg_stores_explicit_offset() {
        // 3-arg form: explicit offset stored as provided
        let parent = prismatic_x_joint();
        let offset = Value::length(0.5);
        let result = eval_builtin(
            "couple",
            &[parent.clone(), Value::Real(2.0), offset.clone()],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(2.0)),
            "ratio should be Value::Real(2.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "offset should be Value::length(0.5)"
        );
    }

    #[test]
    fn couple_revolute_2arg_defaults_angle_offset() {
        // 2-arg revolute parent → default offset is Value::angle(0.0)
        let parent = revolute_z_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(0.5)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "default offset for revolute should be Value::angle(0.0)"
        );
    }

    #[test]
    fn couple_revolute_3arg_stores_explicit_angle_offset() {
        // 3-arg revolute form: explicit angle offset stored
        let pi = std::f64::consts::PI;
        let parent = revolute_z_joint();
        let offset = Value::angle(pi / 4.0);
        let result = eval_builtin(
            "couple",
            &[parent.clone(), Value::Real(0.5), offset.clone()],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "explicit angle offset should be stored"
        );
    }

    // ── couple constructor: validation rejections ────────────────────────────

    #[test]
    fn couple_zero_args_returns_undef() {
        assert!(
            eval_builtin("couple", &[]).is_undef(),
            "0 args should return Undef"
        );
    }

    #[test]
    fn couple_one_arg_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint()]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn couple_four_args_returns_undef() {
        assert!(
            eval_builtin(
                "couple",
                &[
                    prismatic_x_joint(),
                    Value::Real(1.0),
                    Value::length(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "4 args should return Undef"
        );
    }

    #[test]
    fn couple_non_map_parent_returns_undef() {
        assert!(
            eval_builtin("couple", &[Value::Real(1.0), Value::Real(1.0)]).is_undef(),
            "non-Map parent should return Undef"
        );
    }

    #[test]
    fn couple_map_missing_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "Map parent missing kind key should return Undef"
        );
    }

    #[test]
    fn couple_coupling_parent_returns_undef() {
        // nested coupling is rejected — kind="coupling" is not a DrivingJoint
        let inner = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("couple", &[inner, Value::Real(1.0)]).is_undef(),
            "coupling parent (kind='coupling') should return Undef"
        );
    }

    #[test]
    fn couple_unknown_parent_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "parent kind='sliding' should return Undef"
        );
    }

    #[test]
    fn couple_string_ratio_returns_undef() {
        assert!(
            eval_builtin(
                "couple",
                &[prismatic_x_joint(), Value::String("bad".to_string())]
            )
            .is_undef(),
            "String ratio should return Undef"
        );
    }

    #[test]
    fn couple_nan_ratio_returns_undef() {
        // NaN ratio must be rejected — only finites are valid
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::NAN)]).is_undef(),
            "NaN ratio should return Undef"
        );
    }

    #[test]
    fn couple_inf_ratio_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::INFINITY)]).is_undef(),
            "Infinite ratio should return Undef"
        );
    }

    #[test]
    fn couple_dimensioned_ratio_returns_undef() {
        // a Length Scalar as ratio is not dimensionless — must be rejected
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::length(1.0)]).is_undef(),
            "dimensioned ratio should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_wrong_offset_dim_returns_undef() {
        use reify_types::DimensionVector;
        let mass_offset = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert!(
            eval_builtin(
                "couple",
                &[prismatic_x_joint(), Value::Real(1.0), mass_offset]
            )
            .is_undef(),
            "MASS offset for prismatic parent should return Undef"
        );
    }

    #[test]
    fn couple_revolute_wrong_offset_dim_returns_undef() {
        // Length offset for a revolute parent (needs Angle or bare Real)
        assert!(
            eval_builtin(
                "couple",
                &[revolute_z_joint(), Value::Real(1.0), Value::length(1.0)]
            )
            .is_undef(),
            "Length offset for revolute parent should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_nan_offset_returns_undef() {
        assert!(
            eval_builtin(
                "couple",
                &[prismatic_x_joint(), Value::Real(1.0), Value::Real(f64::NAN)]
            )
            .is_undef(),
            "NaN offset should return Undef"
        );
    }

    // ── couple constructor: accepted ratio / offset variants ────────────────

    #[test]
    fn couple_int_ratio_accepted() {
        // Value::Int(2) is accepted by ratio_input and stored as Real(2.0)
        let result = eval_builtin("couple", &[prismatic_x_joint(), Value::Int(2)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(2.0)),
            "Int(2) ratio should be stored as Real(2.0)"
        );
    }

    #[test]
    fn couple_dimensionless_scalar_ratio_accepted() {
        use reify_types::DimensionVector;
        // DIMENSIONLESS Scalar is accepted by ratio_input and stored as Real
        let ratio = Value::Scalar {
            si_value: 0.5,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("couple", &[prismatic_x_joint(), ratio]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "DIMENSIONLESS Scalar(0.5) ratio should be stored as Real(0.5)"
        );
    }

    #[test]
    fn couple_prismatic_int_offset_accepted() {
        // Value::Int(1) is accepted by length_input for a prismatic parent
        // and stored as Value::length(1.0)
        let result = eval_builtin(
            "couple",
            &[prismatic_x_joint(), Value::Real(1.0), Value::Int(1)],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(1.0)),
            "Int(1) offset for prismatic should be stored as Value::length(1.0)"
        );
    }

    #[test]
    fn couple_prismatic_bare_real_offset_accepted() {
        // bare Value::Real(1.5) is accepted by length_input for a prismatic parent
        let result = eval_builtin(
            "couple",
            &[prismatic_x_joint(), Value::Real(1.0), Value::Real(1.5)],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(1.5)),
            "Real(1.5) offset for prismatic should be stored as Value::length(1.5)"
        );
    }

    #[test]
    fn couple_revolute_int_offset_accepted() {
        // Value::Int(0) is accepted by trig_input for a revolute parent
        let result = eval_builtin(
            "couple",
            &[revolute_z_joint(), Value::Real(1.0), Value::Int(0)],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "Int(0) offset for revolute should be stored as Value::angle(0.0)"
        );
    }

    #[test]
    fn couple_revolute_bare_real_offset_accepted() {
        // bare Value::Real(π/4) is accepted by trig_input for a revolute parent
        let pi = std::f64::consts::PI;
        let result = eval_builtin(
            "couple",
            &[revolute_z_joint(), Value::Real(1.0), Value::Real(pi / 4.0)],
        );
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(pi / 4.0)),
            "Real(π/4) offset for revolute should be stored as Value::angle(π/4)"
        );
    }

    // ── transform_at on Coupling: validation rejections ─────────────────────

    /// Build a minimal coupling Map by hand for testing defense-in-depth guards.
    fn make_coupling_fixture(parent: Value, ratio: Value, offset: Value) -> Value {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("offset".to_string()), offset);
        m.insert(Value::String("parent".to_string()), parent);
        m.insert(Value::String("ratio".to_string()), ratio);
        Value::Map(m)
    }

    #[test]
    fn transform_at_coupling_angle_to_prismatic_parent_returns_undef() {
        // Angle Scalar passed to a coupling whose parent is prismatic
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::angle(1.0)]).is_undef(),
            "Angle to prismatic coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_length_to_revolute_parent_returns_undef() {
        // Length Scalar passed to a coupling whose parent is revolute
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "Length to revolute coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let mass = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };
        assert!(
            eval_builtin("transform_at", &[c, mass]).is_undef(),
            "MASS Scalar to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_nan_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::NAN)]).is_undef(),
            "NaN value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_inf_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_sliding_parent_kind_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with parent kind="sliding"
        use std::collections::BTreeMap;
        let mut sliding = BTreeMap::new();
        sliding.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );
        sliding.insert(Value::String("axis".to_string()), axis_x_unit());
        let c = make_coupling_fixture(Value::Map(sliding), Value::Real(1.0), Value::length(0.0));
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with sliding parent should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_parent_key_returns_undef() {
        // Defense-in-depth: hand-built coupling Map without parent key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "parent" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing parent key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_ratio_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "ratio" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing ratio key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_offset_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        // no "offset" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing offset key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_int_ratio_returns_undef() {
        // Defense-in-depth: the coupling arm requires ratio to be stored as
        // Value::Real (the `make_coupling` helper always does this), but a
        // hand-built Map could carry Value::Int instead.  The guard must fire.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Int(1), // Int, not Real — guard fires
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Int ratio should return Undef (defense-in-depth)"
        );
    }

    #[test]
    fn transform_at_coupling_real_offset_returns_undef() {
        // Defense-in-depth: the coupling arm requires offset to be stored as
        // Value::Scalar (the `make_coupling` helper always does this via
        // Value::length / Value::angle), but a hand-built Map could carry a
        // bare Value::Real instead.  The guard must fire.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(1.0),
            Value::Real(0.0), // Real, not Scalar — guard fires
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Real offset should return Undef (defense-in-depth)"
        );
    }

    #[test]
    fn transform_at_coupling_nan_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with Value::Real(f64::NAN) ratio.
        // The `couple` constructor rejects NaN ratios via `ratio_input`, but a
        // hand-built fixture (or future serialisation path) could carry one. The
        // ratio guard inside the coupling arm of `transform_at` must catch it.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(f64::NAN),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with NaN ratio should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_inf_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with Value::Real(f64::INFINITY) ratio.
        // The `couple` constructor rejects Inf ratios via `ratio_input`, but a
        // hand-built fixture (or future serialisation path) could carry one.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(f64::INFINITY),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Inf ratio should return Undef"
        );
    }

    // ── transform_at on Coupling: analytic tests ────────────────────────────

    #[test]
    fn coupling_prismatic_sign_reversal() {
        // Counter-mass idiom: ratio=-1 on X-prismatic → translation negated
        // coupled_value = -1.0 * 5.0 + 0.0 = -5.0 m
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(5.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [-5.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic sign reversal, ratio=-1, v=5m → [-5,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_with_offset() {
        // ratio=2.0, offset=1.0m, v=3.0m → coupled = 2*3+1 = 7m → [7,0,0]
        let c = eval_builtin(
            "couple",
            &[prismatic_x_joint(), Value::Real(2.0), Value::length(1.0)],
        );
        let result = eval_builtin("transform_at", &[c, Value::length(3.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [7.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic ratio=2, offset=1m, v=3m → [7,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_bare_real_value() {
        // bare Real(0.5) accepted as 0.5 metres; ratio=1, offset=0 → [0.5,0,0]
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::Real(0.5)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "coupling prismatic bare Real(0.5), ratio=1 → [0.5,0,0]",
        );
    }

    #[test]
    fn coupling_revolute_sign_reversal() {
        // ratio=-1 on Z-revolute → rotation reversed: angle = -π/2
        // coupled_value = -1.0 * (π/2) + 0 = -π/2
        // rotation = (cos(-π/4), 0, 0, sin(-π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 2.0)]);
        let exp_w = (-pi / 4.0).cos();
        let exp_z = (-pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (exp_w, 0.0, 0.0, exp_z),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute sign reversal, ratio=-1, v=π/2 → -π/2",
        );
    }

    #[test]
    fn coupling_revolute_with_offset() {
        // ratio=1.0, offset=π/4, v=π/4 → coupled = 1*(π/4) + π/4 = π/2
        // rotation about Z by π/2 = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin(
            "couple",
            &[revolute_z_joint(), Value::Real(1.0), Value::angle(pi / 4.0)],
        );
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 4.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute ratio=1, offset=π/4, v=π/4 → rotation π/2",
        );
    }

    #[test]
    fn coupling_zero_ratio_gives_identity_transform() {
        // ratio=0 → coupled_value = 0*v + 0 = 0m regardless of v
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(0.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(99.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling zero ratio → identity transform",
        );
    }

    // ── joint_range accessor ─────────────────────────────────────────────────

    #[test]
    fn joint_range_prismatic_returns_stored_range() {
        let range = length_range_0_to_1m();
        let joint = eval_builtin("prismatic", &[axis_x_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(prismatic) should return stored range"
        );
    }

    #[test]
    fn joint_range_revolute_returns_stored_range() {
        let range = angle_range_0_to_pi();
        let joint = eval_builtin("revolute", &[axis_z_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(revolute) should return stored range"
        );
    }

    #[test]
    fn joint_range_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[Value::String("foo".to_string())]).is_undef(),
            "joint_range of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_range_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[]).is_undef(),
            "joint_range with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_range_two_args_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("joint_range", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_range with 2 args should return Undef"
        );
    }

    // ── joint_jacobian (step-23) ─────────────────────────────────────────────

    /// Helper: extract a 3-component f64 vector from a Map at the given key.
    fn jac_vec3_components(map: &Value, key: &str) -> [f64; 3] {
        let inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => [
                items[0].as_f64().unwrap(),
                items[1].as_f64().unwrap(),
                items[2].as_f64().unwrap(),
            ],
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// Helper: dimension of a Map's vector value at `key`.
    fn jac_vec3_dim(map: &Value, key: &str) -> reify_types::DimensionVector {
        let inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => items[0].dimension(),
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// Assert two 3-component vectors are within tolerance.
    fn assert_vec3_close(actual: [f64; 3], expected: [f64; 3], tol: f64, label: &str) {
        for i in 0..3 {
            assert!(
                (actual[i] - expected[i]).abs() < tol,
                "{}: component[{}] expected {}, got {}",
                label,
                i,
                expected[i],
                actual[i]
            );
        }
    }

    #[test]
    fn joint_jacobian_prismatic_x_axis() {
        // (a) prismatic with axis [1,0,0] → angular=[0,0,0], linear=[1,0,0].
        let joint = prismatic_x_joint();
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "prismatic X angular");
        assert_vec3_close(lin, [1.0, 0.0, 0.0], 1e-12, "prismatic X linear");
        assert_eq!(
            jac_vec3_dim(&result, "angular"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "angular should be DIMENSIONLESS"
        );
        assert_eq!(
            jac_vec3_dim(&result, "linear"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "linear should be DIMENSIONLESS"
        );
    }

    #[test]
    fn joint_jacobian_revolute_z_axis() {
        // (b) revolute with axis [0,0,1] → angular=[0,0,1], linear=[0,0,0].
        let joint = revolute_z_joint();
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 1.0], 1e-12, "revolute Z angular");
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "revolute Z linear");
        assert_eq!(
            jac_vec3_dim(&result, "angular"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "angular should be DIMENSIONLESS"
        );
        assert_eq!(
            jac_vec3_dim(&result, "linear"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "linear should be DIMENSIONLESS"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_unnormalized_axis() {
        // (c) prismatic with axis [2,0,0] (magnitude 2) is normalized to [1,0,0] in linear.
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = length_range_0_to_1m();
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(
            ang,
            [0.0, 0.0, 0.0],
            1e-12,
            "unnormalized prismatic angular",
        );
        assert_vec3_close(
            lin,
            [1.0, 0.0, 0.0],
            1e-12,
            "unnormalized prismatic linear (should be unit-normalized)",
        );
    }

    #[test]
    fn joint_jacobian_revolute_unnormalized_axis() {
        // Mirror of (c) for revolute: axis [0,0,2] → angular=[0,0,1].
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let range = angle_range_0_to_pi();
        let joint = eval_builtin("revolute", &[axis, range]);
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(
            ang,
            [0.0, 0.0, 1.0],
            1e-12,
            "unnormalized revolute angular (should be unit-normalized)",
        );
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "unnormalized revolute linear");
    }

    #[test]
    fn joint_jacobian_coupling_prismatic_ratio_2() {
        // (d) coupling of prismatic-X with ratio=2 → linear=[2,0,0], angular=0.
        let parent = prismatic_x_joint();
        let c = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let result = eval_builtin("joint_jacobian", &[c]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "coupling prismatic angular");
        assert_vec3_close(
            lin,
            [2.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic linear (ratio=2)",
        );
    }

    #[test]
    fn joint_jacobian_coupling_revolute_ratio_neg3() {
        // (e) coupling of revolute-Z with ratio=-3 → angular=[0,0,-3], linear=0.
        let parent = revolute_z_joint();
        let c = eval_builtin("couple", &[parent, Value::Real(-3.0)]);
        let result = eval_builtin("joint_jacobian", &[c]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(
            ang,
            [0.0, 0.0, -3.0],
            1e-12,
            "coupling revolute angular (ratio=-3)",
        );
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "coupling revolute linear");
    }

    #[test]
    fn joint_jacobian_zero_args_returns_undef() {
        // (f) wrong-arg count
        assert!(
            eval_builtin("joint_jacobian", &[]).is_undef(),
            "joint_jacobian with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_two_args_returns_undef() {
        // (f) wrong-arg count
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("joint_jacobian", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_jacobian with 2 args should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_non_map_arg_returns_undef() {
        // (f) non-Map arg
        assert!(
            eval_builtin("joint_jacobian", &[Value::Real(1.0)]).is_undef(),
            "joint_jacobian with non-Map arg should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_map_without_kind_returns_undef() {
        // (f) Map without "kind" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "Map without kind key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_map_with_unknown_kind_returns_undef() {
        // (f) Map with kind not in {prismatic, revolute, coupling}
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "Map with unknown kind should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_missing_axis_returns_undef() {
        // (f) joint Map missing "axis" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "prismatic Map missing axis key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_revolute_missing_axis_returns_undef() {
        // (f) joint Map missing "axis" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        m.insert(Value::String("range".to_string()), angle_range_0_to_pi());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "revolute Map missing axis key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_missing_parent_returns_undef() {
        // (f) coupling Map missing "parent" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "parent" key
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "coupling Map missing parent key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_missing_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map without ratio key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "ratio" key
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "coupling Map missing ratio key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_zero_axis_returns_undef() {
        // Defense-in-depth: hand-built prismatic Map with zero-magnitude axis
        // (`prismatic` constructor would reject this, but a hand-built Map
        // could carry it).
        use std::collections::BTreeMap;
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        m.insert(Value::String("axis".to_string()), zero_axis);
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "prismatic Map with zero-magnitude axis should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_nested_returns_undef() {
        // Nested coupling: parent has kind="coupling" — `couple` rejects this
        // at construction, but a hand-built coupling fixture could carry it.
        // joint_jacobian must reject (consistent with `couple` and transform_at).
        use std::collections::BTreeMap;
        let inner =
            make_coupling_fixture(prismatic_x_joint(), Value::Real(1.0), Value::length(0.0));
        let mut outer = BTreeMap::new();
        outer.insert(
            Value::String("kind".to_string()),
            Value::String("coupling".to_string()),
        );
        outer.insert(Value::String("parent".to_string()), inner);
        outer.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        outer.insert(Value::String("offset".to_string()), Value::length(0.0));
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(outer)]).is_undef(),
            "nested coupling should return Undef"
        );
    }

    // ── coupling specialisations: shared test helper ─────────────────────────

    /// Assert that `eval_builtin(name, args)` returns `Value::Undef`.
    ///
    /// Use this in table-driven validation tests (see `screw_validation_rejections`,
    /// `gear_validation_rejections`, `rack_and_pinion_validation_rejections`) rather
    /// than writing one `#[test]` fn per rejection case.
    fn assert_builtin_undef(name: &str, args: &[Value], label: &str) {
        assert!(
            eval_builtin(name, args).is_undef(),
            "{name}({args:?}) — {label} — should return Undef"
        );
    }

    // ── screw constructor: happy path ────────────────────────────────────────

    #[test]
    fn screw_returns_correct_coupling_and_transform_at_works() {
        // screw(prismatic-X, lead=1mm) → coupling Map with:
        //   kind="coupling", parent=prismatic-X, ratio=1e-3/(2π), offset=length(0)
        // transform_at(result, length(2π)) → translation [1e-3, 0, 0] (one lead per 2π input)
        let pi = std::f64::consts::PI;
        let parent = prismatic_x_joint();
        let lead = Value::length(1e-3); // 1 mm
        let result = eval_builtin("screw", &[parent.clone(), lead]);
        let map = match &result {
            Value::Map(m) => m.clone(),
            other => panic!("screw: expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "screw: kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "screw: parent should match the prismatic joint"
        );
        let expected_ratio = 1e-3 / (2.0 * pi);
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(expected_ratio)),
            "screw: ratio should be Value::Real(lead/(2π))"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(0.0)),
            "screw: default offset for prismatic parent should be Value::length(0.0)"
        );
        assert_eq!(
            map.len(),
            4,
            "screw coupling Map should have exactly 4 keys: kind, parent, ratio, offset"
        );
        // Verify end-to-end kinematics: transform_at(result, length(2π)) → translation [1e-3, 0, 0]
        // Math: coupled = (1e-3/(2π)) * 2π + 0 = 1e-3 m, translated along prismatic-X axis.
        let xform = eval_builtin("transform_at", &[result, Value::length(2.0 * pi)]);
        assert_transform_approx(
            &xform,
            (1.0, 0.0, 0.0, 0.0),
            [1e-3, 0.0, 0.0],
            1e-12,
            "screw transform_at: 1mm lead, 2π input → [1e-3, 0, 0]",
        );
    }

    // ── screw constructor: validation rejections ─────────────────────────────

    #[test]
    fn screw_validation_rejections() {
        use reify_types::DimensionVector;
        use std::collections::BTreeMap;
        let parent = prismatic_x_joint();
        let lead = Value::length(1e-3);
        let coupling_parent = eval_builtin("screw", &[parent.clone(), lead.clone()]);
        let mut no_kind = BTreeMap::new();
        no_kind.insert(Value::String("axis".to_string()), axis_x_unit());
        let cases: Vec<(Vec<Value>, &str)> = vec![
            (vec![], "0 args"),
            (vec![parent.clone()], "1 arg"),
            (
                vec![parent.clone(), lead.clone(), Value::Real(0.0)],
                "3 args",
            ),
            (vec![Value::Real(1.0), lead.clone()], "non-Map parent"),
            (
                vec![coupling_parent, lead.clone()],
                "coupling parent (delegated to couple())",
            ),
            (
                vec![Value::Map(no_kind), lead.clone()],
                "Map parent missing 'kind'",
            ),
            (
                vec![
                    parent.clone(),
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::MASS,
                    },
                ],
                "MASS-dimensioned lead",
            ),
            (vec![parent.clone(), Value::Real(f64::NAN)], "NaN lead"),
            (vec![parent.clone(), Value::Real(f64::INFINITY)], "Inf lead"),
            (
                vec![parent, Value::String("bad".to_string())],
                "String lead",
            ),
        ];
        for (args, label) in &cases {
            assert_builtin_undef("screw", args, label);
        }
    }

    // ── gear constructor: happy path ─────────────────────────────────────────

    #[test]
    fn gear_returns_correct_coupling_and_transform_at_works() {
        // gear(revolute-Z, teeth_a=20, teeth_b=30) → coupling Map with:
        //   kind="coupling", parent=revolute-Z, ratio=-(30/20)=-1.5, offset=angle(0)
        // transform_at(result, angle(π/3)) → coupled angle = -1.5 * π/3 = -π/2
        //   → rotation = (cos(-π/4), 0, 0, sin(-π/4))
        let pi = std::f64::consts::PI;
        let parent = revolute_z_joint();
        let result = eval_builtin("gear", &[parent.clone(), Value::Int(20), Value::Int(30)]);
        let map = match &result {
            Value::Map(m) => m.clone(),
            other => panic!("gear: expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "gear: kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "gear: parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(-30.0 / 20.0)),
            "gear: ratio should be Value::Real(-teeth_b/teeth_a) = -1.5"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "gear: default offset for revolute parent should be Value::angle(0.0)"
        );
        assert_eq!(
            map.len(),
            4,
            "gear coupling Map should have exactly 4 keys: kind, parent, ratio, offset"
        );
        // Verify end-to-end kinematics: transform_at(result, angle(π/3))
        // Math: coupled = -1.5 * (π/3) = -π/2 rad about Z-axis
        //   → rotation quaternion for -π/2 about Z = (cos(-π/4), 0, 0, sin(-π/4))
        let xform = eval_builtin("transform_at", &[result, Value::angle(pi / 3.0)]);
        let cos_q = (-pi / 4.0).cos();
        let sin_q = (-pi / 4.0).sin();
        assert_transform_approx(
            &xform,
            (cos_q, 0.0, 0.0, sin_q),
            [0.0, 0.0, 0.0],
            1e-12,
            "gear transform_at: 20:30 ratio, π/3 input → -π/2 rotation about Z",
        );
    }

    // ── gear constructor: validation rejections ──────────────────────────────

    #[test]
    fn gear_validation_rejections() {
        use reify_types::DimensionVector;
        let parent = revolute_z_joint();
        let ta = Value::Int(20);
        let tb = Value::Int(30);
        let coupling_parent = eval_builtin("gear", &[parent.clone(), ta.clone(), tb.clone()]);
        let cases: Vec<(Vec<Value>, &str)> = vec![
            (vec![], "0 args"),
            (vec![parent.clone()], "1 arg"),
            (vec![parent.clone(), ta.clone()], "2 args"),
            (
                vec![parent.clone(), ta.clone(), tb.clone(), Value::Real(0.0)],
                "4 args",
            ),
            (
                vec![Value::Real(1.0), ta.clone(), tb.clone()],
                "non-Map parent",
            ),
            (
                vec![coupling_parent, ta.clone(), tb.clone()],
                "coupling parent (delegated to couple())",
            ),
            (
                vec![parent.clone(), Value::Int(0), tb.clone()],
                "teeth_a=0 (division by zero)",
            ),
            (
                vec![parent.clone(), Value::Int(-5), tb.clone()],
                "negative teeth_a",
            ),
            (
                vec![parent.clone(), ta.clone(), Value::Int(0)],
                "teeth_b=0 (degenerate)",
            ),
            (
                vec![parent.clone(), ta.clone(), Value::Int(-3)],
                "negative teeth_b",
            ),
            (
                vec![parent.clone(), Value::Real(20.0), tb.clone()],
                "Real teeth_a (Int-only contract)",
            ),
            (
                vec![parent.clone(), ta.clone(), Value::Real(30.0)],
                "Real teeth_b (Int-only contract)",
            ),
            (
                vec![
                    parent.clone(),
                    Value::Scalar {
                        si_value: 20.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    tb.clone(),
                ],
                "Scalar teeth_a (Int-only contract)",
            ),
            (
                vec![parent, Value::String("bad".to_string()), tb],
                "String teeth_a",
            ),
        ];
        for (args, label) in &cases {
            assert_builtin_undef("gear", args, label);
        }
    }

    // ── rack_and_pinion constructor: happy path ───────────────────────────────

    #[test]
    fn rack_and_pinion_returns_correct_coupling_and_transform_at_works() {
        // rack_and_pinion(prismatic-X, pitch_radius=10mm) → coupling Map with:
        //   kind="coupling", parent=prismatic-X, ratio=0.01, offset=length(0)
        // transform_at(result, length(2π)) → coupled length = 0.01 * 2π = 0.02π m
        //   → translation [0.02π, 0, 0], identity rotation
        let pi = std::f64::consts::PI;
        let parent = prismatic_x_joint();
        let pitch_radius = Value::length(0.01); // 10 mm
        let result = eval_builtin("rack_and_pinion", &[parent.clone(), pitch_radius]);
        let map = match &result {
            Value::Map(m) => m.clone(),
            other => panic!("rack_and_pinion: expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "rack_and_pinion: kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "rack_and_pinion: parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.01)),
            "rack_and_pinion: ratio should be Value::Real(pitch_radius_si)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(0.0)),
            "rack_and_pinion: default offset for prismatic parent should be Value::length(0.0)"
        );
        assert_eq!(
            map.len(),
            4,
            "rack_and_pinion coupling Map should have exactly 4 keys: kind, parent, ratio, offset"
        );
        // Verify end-to-end kinematics: transform_at(result, length(2π))
        // Math: coupled = 0.01 * 2π + 0 = 0.02π m, translated along prismatic-X axis.
        let xform = eval_builtin("transform_at", &[result, Value::length(2.0 * pi)]);
        let expected_x = 0.01 * 2.0 * pi;
        assert_transform_approx(
            &xform,
            (1.0, 0.0, 0.0, 0.0),
            [expected_x, 0.0, 0.0],
            1e-12,
            "rack_and_pinion transform_at: 10mm pitch, 2π input → 0.02π translation",
        );
    }

    // ── rack_and_pinion constructor: validation rejections ───────────────────

    #[test]
    fn rack_and_pinion_validation_rejections() {
        use reify_types::DimensionVector;
        use std::collections::BTreeMap;
        let parent = prismatic_x_joint();
        let pr = Value::length(0.01);
        let coupling_parent = eval_builtin("rack_and_pinion", &[parent.clone(), pr.clone()]);
        let mut no_kind = BTreeMap::new();
        no_kind.insert(Value::String("axis".to_string()), axis_x_unit());
        let cases: Vec<(Vec<Value>, &str)> = vec![
            (vec![], "0 args"),
            (vec![parent.clone()], "1 arg"),
            (vec![parent.clone(), pr.clone(), Value::Real(0.0)], "3 args"),
            (vec![Value::Real(1.0), pr.clone()], "non-Map parent"),
            (
                vec![coupling_parent, pr.clone()],
                "coupling parent (delegated to couple())",
            ),
            (
                vec![Value::Map(no_kind), pr.clone()],
                "Map parent missing 'kind'",
            ),
            (
                vec![
                    parent.clone(),
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::MASS,
                    },
                ],
                "MASS-dimensioned pitch_radius",
            ),
            (
                vec![parent.clone(), Value::Real(f64::NAN)],
                "NaN pitch_radius",
            ),
            (
                vec![parent.clone(), Value::Real(f64::INFINITY)],
                "Inf pitch_radius",
            ),
            (
                vec![parent, Value::String("bad".to_string())],
                "String pitch_radius",
            ),
        ];
        for (args, label) in &cases {
            assert_builtin_undef("rack_and_pinion", args, label);
        }
    }

    // ── JOINT_KINDS / is_joint_value direct unit tests ───────────────────────

    /// All negative-case inputs for `is_joint_value` in one table-driven test.
    /// Positive cases (one per kind in `JOINT_KINDS`) are covered by
    /// `is_joint_value_aligns_with_joint_kinds` below.
    #[test]
    fn is_joint_value_negative_cases() {
        use std::collections::BTreeMap;

        let mut no_kind = BTreeMap::new();
        no_kind.insert(Value::String("axis".to_string()), axis_x_unit());

        let mut unknown_kind = BTreeMap::new();
        unknown_kind.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );

        let mut non_string_kind = BTreeMap::new();
        non_string_kind.insert(Value::String("kind".to_string()), Value::Int(0));

        let cases: Vec<(&str, Value)> = vec![
            ("Real(1.0)", Value::Real(1.0)),
            ("Int(0)", Value::Int(0)),
            (
                "bare String 'prismatic'",
                Value::String("prismatic".to_string()),
            ),
            ("Map without 'kind' key", Value::Map(no_kind)),
            (
                "Map with kind='sliding' (not in JOINT_KINDS)",
                Value::Map(unknown_kind),
            ),
            ("Map with kind=Int(0)", Value::Map(non_string_kind)),
        ];
        for (label, v) in &cases {
            assert!(!is_joint_value(v), "{label} should not be a joint value");
        }
    }

    #[test]
    fn is_joint_value_aligns_with_joint_kinds() {
        use std::collections::BTreeMap;
        // Every kind in JOINT_KINDS must be recognized as a joint value.
        for &kind in JOINT_KINDS {
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String(kind.to_string()),
            );
            assert!(
                is_joint_value(&Value::Map(m)),
                "Map with kind='{}' (in JOINT_KINDS) should be a joint value",
                kind
            );
        }
        // A kind not in JOINT_KINDS must not be recognized.
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("not_a_joint".to_string()),
        );
        assert!(
            !is_joint_value(&Value::Map(m)),
            "Map with kind='not_a_joint' should not be a joint value"
        );
    }

    /// Minimal well-formed `(joint, value_arg)` fixture for each kind in `JOINT_KINDS`.
    ///
    /// Shared by `transform_at_dispatches_for_every_joint_kind` and
    /// `joint_jacobian_dispatches_for_every_joint_kind`.
    ///
    /// Returns a `Vec` of `(joint, value_arg)` pairs for each kind so that a
    /// kind may yield multiple variants when multiple code paths must be covered.
    ///
    /// For `"coupling"` two variants are returned — one with a prismatic parent
    /// and one with a revolute parent — so that both branches in `transform_at`
    /// (`length_input` vs `trig_input`) and both paths in `joint_jacobian_value`
    /// are exercised by the dispatch-coverage tests.
    ///
    /// After building each coupling value the fixture asserts it is non-Undef.
    /// This localises the failure to `couple` itself rather than letting a
    /// regression in `couple` surface as a misleading dispatch-test failure.
    ///
    /// Consumer tests should use a nested loop:
    /// ```ignore
    /// for &kind in JOINT_KINDS {
    ///     for (joint, value_arg) in joint_kind_minimal_fixture(kind) { ... }
    /// }
    /// ```
    ///
    /// `value_arg` is the motion-variable input for `transform_at`; it is unused
    /// by `joint_jacobian` (which is a constant w.r.t. the motion variable).
    fn joint_kind_minimal_fixture(kind: &str) -> Vec<(Value, Value)> {
        match kind {
            "prismatic" => vec![(prismatic_x_joint(), Value::length(0.0))],
            "revolute" => vec![(revolute_z_joint(), Value::angle(0.0))],
            "coupling" => {
                let coupling_p = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
                assert!(
                    !coupling_p.is_undef(),
                    "couple fixture itself returned Undef — fix couple before checking dispatch"
                );
                let coupling_r = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
                assert!(
                    !coupling_r.is_undef(),
                    "couple fixture itself returned Undef — fix couple before checking dispatch"
                );
                vec![
                    (coupling_p, Value::length(0.0)),
                    (coupling_r, Value::angle(0.0)),
                ]
            }
            // 0-DOF fixed joint: any second arg is accepted (design decision: ignored).
            // Value::Real(0.0) chosen to match the minimal-value style of the other arms.
            "fixed" => vec![(eval_builtin("fixed", &[]), Value::Real(0.0))],
            // 3-DOF planar joint: motion_vars is a List of [x_length, y_length, theta_angle].
            // Using zero values keeps the fixture minimal while exercising all dispatch arms.
            "planar" => vec![(
                planar_xy_joint(),
                Value::List(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::angle(0.0),
                ]),
            )],
            // 3-DOF spherical joint: motion variable is a unit-quaternion `Value::Orientation`.
            // The identity quaternion is the minimal-rotation fixture, exercising the
            // `transform_at` and `joint_jacobian_value` spherical arms via the dispatch tests.
            "spherical" => vec![(
                spherical_joint(),
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            )],
            // 2-DOF cylindrical joint: motion variable is a 2-element
            // `Value::List` of `[length, angle]` (translation distance,
            // rotation angle). The (0, 0) pair is the minimal fixture and
            // exercises both the `transform_at` and `joint_jacobian_value`
            // cylindrical arms via the dispatch coverage tests.
            "cylindrical" => vec![(
                cylindrical_z_joint(),
                Value::List(vec![Value::length(0.0), Value::angle(0.0)]),
            )],
            _ => panic!(
                "JOINT_KINDS contains '{kind}' but the dispatch tests have no fixture; \
                 add a minimal well-formed fixture here and confirm that both \
                 `transform_at` and `joint_jacobian_value` have matching dispatch arms"
            ),
        }
    }

    /// Guard against silent drift between `JOINT_KINDS` and the per-kind `match`
    /// arms in `transform_at`.  For every kind in `JOINT_KINDS`, build a minimal
    /// well-formed joint value and assert that `transform_at` does NOT return
    /// `Value::Undef`.
    ///
    /// Two failure modes are caught:
    /// 1. A new kind is added to `JOINT_KINDS` without a fixture in
    ///    `joint_kind_minimal_fixture` → the `_` arm panics with a remediation
    ///    message.
    /// 2. A fixture exists but `transform_at` has no dispatch arm for the kind →
    ///    the `is_undef` assertion fails.
    #[test]
    fn transform_at_dispatches_for_every_joint_kind() {
        for &kind in JOINT_KINDS {
            for (joint, value_arg) in joint_kind_minimal_fixture(kind) {
                let result = eval_builtin("transform_at", &[joint, value_arg]);
                assert!(
                    !result.is_undef(),
                    "transform_at(kind='{kind}', minimal-well-formed-input) returned Undef. \
                     Either add a dispatch arm in transform_at for kind='{kind}', \
                     or remove '{kind}' from JOINT_KINDS."
                );
            }
        }
    }

    /// Guard against silent drift between `JOINT_KINDS` and the per-kind `match`
    /// arms in `joint_jacobian_value`.  Mirrors
    /// `transform_at_dispatches_for_every_joint_kind` but calls `joint_jacobian`
    /// (which takes only the joint, no motion-variable argument).
    ///
    /// Two failure modes are caught:
    /// 1. A new kind is added to `JOINT_KINDS` without a fixture in
    ///    `joint_kind_minimal_fixture` → the `_` arm panics with a remediation
    ///    message.
    /// 2. A fixture exists but `joint_jacobian_value` has no dispatch arm for
    ///    the kind → the `is_undef` assertion fails.
    #[test]
    fn joint_jacobian_dispatches_for_every_joint_kind() {
        for &kind in JOINT_KINDS {
            for (joint, _value_arg) in joint_kind_minimal_fixture(kind) {
                let result = eval_builtin("joint_jacobian", &[joint]);
                assert!(
                    !result.is_undef(),
                    "joint_jacobian(kind='{kind}', minimal-well-formed-input) returned Undef. \
                     Either add a dispatch arm in joint_jacobian_value for kind='{kind}', \
                     or remove '{kind}' from JOINT_KINDS."
                );
            }
        }
    }

    // ── fixed constructor ────────────────────────────────────────────────────

    /// `fixed()` with zero args returns a single-key Map `{ "kind" → "fixed" }`.
    /// No axis or range field — fixed joints have no motion variable.
    #[test]
    fn fixed_returns_map_with_correct_fields() {
        let result = eval_builtin("fixed", &[]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("fixed(): expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("fixed".to_string())),
            "kind field should be 'fixed'"
        );
        assert_eq!(
            map.len(),
            1,
            "fixed joint Map should have exactly 1 key (only 'kind')"
        );
    }

    /// `fixed` rejects any non-empty argument list — a 0-DOF joint has no
    /// parameters to accept.
    #[test]
    fn fixed_with_nonzero_args_returns_undef() {
        assert!(
            eval_builtin("fixed", &[Value::Real(0.0)]).is_undef(),
            "fixed(Real(0.0)) should return Undef (too many args)"
        );
        assert!(
            eval_builtin("fixed", &[Value::Real(0.0), Value::Real(1.0)]).is_undef(),
            "fixed(a, b) should return Undef (too many args)"
        );
    }

    // ── joint_jacobian for fixed ─────────────────────────────────────────────

    /// `joint_jacobian(fixed_joint)` returns a zero-twist Map.
    ///
    /// Design decision: returns `Map { angular: [0,0,0], linear: [0,0,0] }` to
    /// preserve the uniform single-column shape across all joint kinds (rather
    /// than returning a 6×0 empty-column list). Callers can read `.angular`/
    /// `.linear` keys without dispatching on shape.
    #[test]
    fn joint_jacobian_fixed_returns_zero_twist() {
        let fj = eval_builtin("fixed", &[]);
        let result = eval_builtin("joint_jacobian", &[fj]);
        let map = match &result {
            Value::Map(m) => m,
            other => panic!("joint_jacobian(fixed): expected Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("angular".to_string())),
            Some(&Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0)
            ])),
            "angular twist column should be [0, 0, 0]"
        );
        assert_eq!(
            map.get(&Value::String("linear".to_string())),
            Some(&Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0)
            ])),
            "linear twist column should be [0, 0, 0]"
        );
    }

    // ── transform_at for fixed ───────────────────────────────────────────────

    /// `transform_at(fixed_joint, motion_var)` returns the identity Transform when
    /// the second arg is a numeric/dimensioned scalar (`Real`, `Int`, or `Scalar`
    /// of any dimension). Returns `Value::Undef` when the second arg is `Undef`
    /// (Undef propagation) OR any non-numeric variant (`String`, `List`, `Map`,
    /// `Vector`, `Bool`, etc.). Type-checking mirrors the discipline of every
    /// other `transform_at` arm; dimension is unconstrained because a 0-DOF joint
    /// has no motion variable.
    #[test]
    fn transform_at_fixed_returns_identity_transform() {
        let fj = eval_builtin("fixed", &[]);

        // Primary case: second arg is a bare Real.
        let result = eval_builtin("transform_at", &[fj.clone(), Value::Real(0.0)]);
        let (rot, trans) = match &result {
            Value::Transform {
                rotation,
                translation,
            } => (rotation.as_ref(), translation.as_ref()),
            other => panic!(
                "transform_at(fixed, 0.0): expected Transform, got {:?}",
                other
            ),
        };
        assert_eq!(
            rot,
            &Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "identity rotation"
        );
        assert_eq!(
            trans,
            &Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0)
            ]),
            "zero translation"
        );

        // Undef propagation: if the second arg is Undef, the result must also be
        // Undef so that upstream evaluation errors are not swallowed.
        let undef_result = eval_builtin("transform_at", &[fj.clone(), Value::Undef]);
        assert!(
            undef_result.is_undef(),
            "transform_at(fixed, Undef): expected Undef (Undef propagation), got {:?}",
            undef_result
        );

        // Numeric/dimensioned scalar args — all should yield the identity Transform.
        // (Dimension is not validated; a 0-DOF joint has no motion variable.)
        for (label, second_arg) in [
            ("Real(1.5)", Value::Real(1.5)),
            ("length(2.5)", Value::length(2.5)),
            ("angle(1.0)", Value::angle(1.0)),
            ("Int(5)", Value::Int(5)),
        ] {
            let r2 = eval_builtin("transform_at", &[fj.clone(), second_arg]);
            assert!(
                matches!(&r2, Value::Transform { .. }),
                "transform_at(fixed, {label}): expected Transform, got {:?}",
                r2
            );
            let (r, t) = match &r2 {
                Value::Transform {
                    rotation,
                    translation,
                } => (rotation.as_ref(), translation.as_ref()),
                _ => unreachable!(),
            };
            assert_eq!(
                r,
                &Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                "identity rotation for {label}"
            );
            assert_eq!(
                t,
                &Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0)
                ]),
                "zero translation for {label}"
            );
        }
    }

    /// Non-numeric second args to `transform_at(fixed, _)` must return `Undef`.
    ///
    /// A 0-DOF joint has no motion variable, but the second arg must still be a
    /// numeric/dimensioned scalar so that upstream type errors (e.g. a `String`
    /// accidentally reaching this call) propagate as `Undef` rather than being
    /// absorbed into a well-formed identity `Transform`. Mirrors the type-checking
    /// discipline of every other `transform_at` arm.
    #[test]
    fn transform_at_fixed_with_non_numeric_second_arg_returns_undef() {
        use std::collections::BTreeMap;
        let fj = eval_builtin("fixed", &[]);

        for (label, second_arg) in [
            ("String(\"foo\")", Value::String("foo".to_string())),
            ("List(empty)", Value::List(vec![])),
            ("Map(empty)", Value::Map(BTreeMap::new())),
            ("Vector([0.0])", Value::Vector(vec![Value::Real(0.0)])),
            ("Bool(true)", Value::Bool(true)),
            (
                "Orientation(identity)",
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
        ] {
            assert!(
                eval_builtin("transform_at", &[fj.clone(), second_arg]).is_undef(),
                "transform_at(fixed, {label}): expected Undef for non-numeric second arg"
            );
        }
    }

    // ── transform_at for fixed: 1-arg ergonomic form ─────────────────────────

    /// `transform_at(fixed_joint)` (1-arg form) returns the identity Transform
    /// for a fixed joint, without requiring a redundant motion-variable argument.
    ///
    /// Also guards:
    /// - 0-arg call → Undef (arity guard preserved)
    /// - 3-arg call → Undef (arity guard preserved)
    /// - 1-arg call with prismatic joint → Undef (1-arg only valid for fixed)
    /// - 1-arg call with revolute joint → Undef (1-arg only valid for fixed)
    /// - 1-arg call with planar joint → Undef (1-arg only valid for fixed)
    /// - 1-arg call with spherical joint → Undef (1-arg only valid for fixed)
    /// - 1-arg call with cylindrical joint → Undef (1-arg only valid for fixed)
    /// - 1-arg call with non-Map first arg (Undef, Real) → Undef (Map-check preserved)
    #[test]
    fn transform_at_fixed_with_one_arg_returns_identity_transform() {
        let fj = eval_builtin("fixed", &[]);

        // (a) Primary case: 1-arg fixed joint → identity Transform
        let result = eval_builtin("transform_at", std::slice::from_ref(&fj));
        let (rot, trans) = match &result {
            Value::Transform {
                rotation,
                translation,
            } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("transform_at(fixed): expected Transform, got {:?}", other),
        };
        assert_eq!(
            rot,
            &Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            "1-arg fixed: identity rotation"
        );
        assert_eq!(
            trans,
            &Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0)
            ]),
            "1-arg fixed: zero translation"
        );

        // (b) 0-arg call → Undef (arity guard preserved)
        assert!(
            eval_builtin("transform_at", &[]).is_undef(),
            "transform_at(): expected Undef (0-arg arity guard)"
        );

        // (c) 3-arg call → Undef (arity guard preserved)
        assert!(
            eval_builtin(
                "transform_at",
                &[fj.clone(), Value::Real(0.0), Value::Real(0.0)]
            )
            .is_undef(),
            "transform_at(fixed, 0.0, 0.0): expected Undef (3-arg arity guard)"
        );

        // (d) 1-arg prismatic joint → Undef (1-arg only valid for fixed)
        let pj = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        assert!(
            eval_builtin("transform_at", &[pj]).is_undef(),
            "transform_at(prismatic): expected Undef (1-arg form is fixed-only)"
        );

        // (e) 1-arg revolute joint → Undef (1-arg only valid for fixed)
        let rj = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        assert!(
            eval_builtin("transform_at", &[rj]).is_undef(),
            "transform_at(revolute): expected Undef (1-arg form is fixed-only)"
        );

        // (f) 1-arg planar joint → Undef (locks full joint taxonomy: 1-arg is fixed-only)
        assert!(
            eval_builtin("transform_at", &[planar_xy_joint()]).is_undef(),
            "transform_at(planar): expected Undef (1-arg form is fixed-only)"
        );

        // (g) 1-arg spherical joint → Undef (locks full joint taxonomy: 1-arg is fixed-only)
        assert!(
            eval_builtin("transform_at", &[spherical_joint()]).is_undef(),
            "transform_at(spherical): expected Undef (1-arg form is fixed-only)"
        );

        // (h) 1-arg cylindrical joint → Undef (locks full joint taxonomy: 1-arg is fixed-only)
        assert!(
            eval_builtin("transform_at", &[cylindrical_z_joint()]).is_undef(),
            "transform_at(cylindrical): expected Undef (1-arg form is fixed-only)"
        );

        // (i) 1-arg non-Map first arg → Undef (Map-check fires before kind dispatch)
        assert!(
            eval_builtin("transform_at", &[Value::Undef]).is_undef(),
            "transform_at(Undef): expected Undef (non-Map first arg)"
        );
        assert!(
            eval_builtin("transform_at", &[Value::Real(0.0)]).is_undef(),
            "transform_at(Real): expected Undef (non-Map first arg)"
        );
    }

    // ── planar constructor: happy path (step-1) ───────────────────────────────

    #[test]
    fn planar_returns_map_with_correct_fields() {
        let axis_x = axis_x_unit();
        let axis_y = axis_y_unit();
        let range_x = length_range_0_to_1m();
        let range_y = length_range_0_to_1m();
        let range_theta = angle_range_0_to_pi();
        let result = eval_builtin(
            "planar",
            &[
                axis_x.clone(),
                axis_y.clone(),
                range_x.clone(),
                range_y.clone(),
                range_theta.clone(),
            ],
        );

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("planar".to_string())),
            "kind field should be 'planar'"
        );
        assert_eq!(
            map.get(&Value::String("axis_x".to_string())),
            Some(&axis_x),
            "axis_x field should match input"
        );
        assert_eq!(
            map.get(&Value::String("axis_y".to_string())),
            Some(&axis_y),
            "axis_y field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_x".to_string())),
            Some(&range_x),
            "range_x field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_y".to_string())),
            Some(&range_y),
            "range_y field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_theta".to_string())),
            Some(&range_theta),
            "range_theta field should match input"
        );
        assert_eq!(
            map.len(),
            6,
            "planar joint Map should have exactly 6 keys \
             (kind, axis_x, axis_y, range_x, range_y, range_theta), \
             got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // ── transform_at on planar: zero motion (step-5) ─────────────────────────

    #[test]
    fn transform_at_planar_zero_motion_returns_identity() {
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar zero motion → identity",
        );
    }

    // ── transform_at on planar: pure-DOF tests (step-7) ──────────────────────

    #[test]
    fn transform_at_planar_pure_x_translation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0.5m, 0m, 0rad]
        // → translation=[0.5, 0, 0], identity rotation
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.5),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "planar pure-X translation 0.5m",
        );
    }

    #[test]
    fn transform_at_planar_pure_y_translation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0m, 0.3m, 0rad]
        // → translation=[0, 0.3, 0], identity rotation
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.3),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.3, 0.0],
            1e-12,
            "planar pure-Y translation 0.3m",
        );
    }

    #[test]
    fn transform_at_planar_pure_rotation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0m, 0m, π/2 rad]
        // normal = +X × +Y = +Z
        // → translation=[0,0,0], rotation = quat(+Z, π/2) = (cos(π/4), 0, 0, sin(π/4))
        let joint = planar_xy_joint();
        let pi = std::f64::consts::PI;
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(pi / 2.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar pure-rotation π/2 about +Z",
        );
    }

    // ── transform_at on planar: combined motion (step-9) ─────────────────────

    #[test]
    fn transform_at_planar_combined_motion() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0.5m, 0.3m, π/2 rad]
        // T_x has identity rotation → translation adds; T_theta adds rotation.
        // Expected: translation=[0.5, 0.3, 0], rotation=quat(+Z, π/2).
        let joint = planar_xy_joint();
        let pi = std::f64::consts::PI;
        let motion = Value::List(vec![
            Value::length(0.5),
            Value::length(0.3),
            Value::angle(pi / 2.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.5, 0.3, 0.0],
            1e-12,
            "planar combined [0.5m, 0.3m, π/2]",
        );
    }

    #[test]
    fn transform_at_planar_combined_non_axis_aligned() {
        // axis_x = [1/√2, 1/√2, 0] (45° in XY plane)
        // axis_y = [-1/√2, 1/√2, 0] (perpendicular to axis_x in XY plane)
        // motion = [1m, 0m, 0rad]
        // → translation = 1 * unit_axis_x = [1/√2, 1/√2, 0], identity rotation
        let s2 = std::f64::consts::FRAC_1_SQRT_2; // 1/√2
        let ax = Value::Vector(vec![Value::Real(s2), Value::Real(s2), Value::Real(0.0)]);
        let ay = Value::Vector(vec![Value::Real(-s2), Value::Real(s2), Value::Real(0.0)]);
        let joint = eval_builtin(
            "planar",
            &[
                ax,
                ay,
                length_range_0_to_1m(),
                length_range_0_to_1m(),
                angle_range_0_to_pi(),
            ],
        );
        let motion = Value::List(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [s2, s2, 0.0],
            1e-12,
            "planar non-axis-aligned X translation",
        );
    }

    // ── transform_at on planar: non-Z plane normal (amendment — review suggestion 4) ──

    #[test]
    fn transform_at_planar_non_z_normal() {
        // axis_x=[1,0,0], axis_y=[0,0,1] → plane is the XZ plane.
        // Normal = axis_x × axis_y = (1,0,0)×(0,0,1):
        //   nx = ay*bz - az*by = 0*1 - 0*0 = 0
        //   ny = az*bx - ax*bz = 0*0 - 1*1 = -1
        //   nz = ax*by - ay*bx = 1*0 - 0*0 = 0
        // → normal = (0, -1, 0) = -Y axis.
        //
        // This test verifies the cross-product sign and catches axis_x/axis_y
        // transposition bugs: swapping the two axes would flip the normal to +Y
        // and produce quat.y = +sin_half (not -sin_half), which the assertion catches.
        //
        // motion = [0m, 0m, π/4 rad]
        // → translation = [0, 0, 0], rotation = quat(-Y, π/4)
        //   = (cos(π/8), 0·sin(π/8), -1·sin(π/8), 0·sin(π/8))
        //   = (cos(π/8), 0, -sin(π/8), 0)
        let pi = std::f64::consts::PI;
        let ax = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let ay = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let joint = eval_builtin(
            "planar",
            &[
                ax,
                ay,
                length_range_0_to_1m(),
                length_range_0_to_1m(),
                angle_range_0_to_pi(),
            ],
        );
        assert!(
            !joint.is_undef(),
            "planar([1,0,0],[0,0,1],...) should build OK"
        );
        let theta = pi / 4.0;
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(theta),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos_h = (theta / 2.0).cos();
        let sin_h = (theta / 2.0).sin();
        // exp_rot = (w, x, y, z); normal = (0, -1, 0) so y = -sin_h.
        assert_transform_approx(
            &result,
            (cos_h, 0.0, -sin_h, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar XZ-plane: rotation about -Y",
        );
    }

    // ── transform_at on planar: invalid motion-var validation (step-11) ─────

    #[test]
    fn transform_at_planar_invalid_motion_var_returns_undef() {
        let joint = planar_xy_joint();

        // Mass-dimensioned scalar (wrong dimension for any element)
        let mass_scalar = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };

        let cases: &[(&str, Value)] = &[
            // (a) wrong List length
            (
                "List of 2",
                Value::List(vec![Value::length(0.0), Value::length(0.0)]),
            ),
            (
                "List of 4",
                Value::List(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::angle(0.0),
                    Value::Real(0.0),
                ]),
            ),
            ("List of 0", Value::List(vec![])),
            // (b) wrong container type
            ("bare Real", Value::Real(0.0)),
            (
                "Vector(3)",
                Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::angle(0.0),
                ]),
            ),
            ("bare Map", Value::Map(Default::default())),
            // (c) wrong dimension: element 0 is Angle (should be Length)
            (
                "elem[0] is Angle",
                Value::List(vec![
                    Value::angle(0.0),
                    Value::length(0.0),
                    Value::angle(0.0),
                ]),
            ),
            // (d) wrong dimension: element 2 is Length (should be Angle)
            (
                "elem[2] is Length",
                Value::List(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]),
            ),
            // (e) wrong dimension: mass-typed element
            (
                "elem[0] mass",
                Value::List(vec![
                    mass_scalar.clone(),
                    Value::length(0.0),
                    Value::angle(0.0),
                ]),
            ),
            (
                "elem[1] mass",
                Value::List(vec![
                    Value::length(0.0),
                    mass_scalar.clone(),
                    Value::angle(0.0),
                ]),
            ),
            (
                "elem[2] mass",
                Value::List(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    mass_scalar.clone(),
                ]),
            ),
            // (f) Undef element propagates Undef result
            (
                "elem[0] Undef",
                Value::List(vec![Value::Undef, Value::length(0.0), Value::angle(0.0)]),
            ),
            (
                "elem[1] Undef",
                Value::List(vec![Value::length(0.0), Value::Undef, Value::angle(0.0)]),
            ),
            (
                "elem[2] Undef",
                Value::List(vec![Value::length(0.0), Value::length(0.0), Value::Undef]),
            ),
        ];

        for (label, motion_vars) in cases {
            assert!(
                eval_builtin("transform_at", &[joint.clone(), motion_vars.clone()]).is_undef(),
                "transform_at(planar, {label}) should return Undef but didn't"
            );
        }
    }

    // ── transform_at on planar: degenerate (parallel) axes returns Undef ─────

    /// Regression test: a hand-built planar `Value::Map` with parallel or
    /// anti-parallel axes must cause `transform_at` to return Undef.
    ///
    /// The `planar(...)` constructor rejects parallel axes at construction time
    /// (joints.rs:67-76), but `transform_at` accepts hand-built `Value::Map`
    /// fixtures.  Without the guard in `unit_axes_xy_from_planar_map`, a parallel
    /// axis pair (`axis_x = axis_y = [1,0,0]`) produces a zero cross product
    /// `(0,0,0)`, which `axis_angle_quaternion` silently promotes to an identity
    /// quaternion — a well-formed Transform that drops the requested rotation
    /// entirely.  After the fix, `unit_axes_xy_from_planar_map` rejects the pair
    /// via the perpendicularity guard and the planar arm propagates Undef.
    #[test]
    fn transform_at_planar_parallel_axes_returns_undef() {
        use std::collections::BTreeMap;

        // Build a hand-crafted planar Map that bypasses the constructor's
        // perpendicularity check.  Mirrors the 6-key layout of `make_planar`
        // (joints.rs:1079-1088): kind, axis_x, axis_y, range_x, range_y, range_theta.
        let make_map = |axis_y: Value| -> Value {
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("planar".to_string()),
            );
            m.insert(
                Value::String("axis_x".to_string()),
                Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            );
            m.insert(Value::String("axis_y".to_string()), axis_y);
            m.insert(Value::String("range_x".to_string()), length_range_0_to_1m());
            m.insert(Value::String("range_y".to_string()), length_range_0_to_1m());
            m.insert(
                Value::String("range_theta".to_string()),
                angle_range_0_to_pi(),
            );
            Value::Map(m)
        };

        // Non-zero theta forces the cross-product / quaternion path.
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(std::f64::consts::PI / 2.0),
        ]);

        let cases: &[(&str, Value)] = &[
            // axis_x = axis_y = [1,0,0]  →  dot = +1  →  zero cross product
            (
                "parallel",
                Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
            // axis_x = [1,0,0], axis_y = [-1,0,0]  →  dot = -1  →  zero cross product
            (
                "anti-parallel",
                Value::Vector(vec![Value::Real(-1.0), Value::Real(0.0), Value::Real(0.0)]),
            ),
        ];

        for (label, axis_y) in cases {
            let joint = make_map(axis_y.clone());
            assert!(
                eval_builtin("transform_at", &[joint, motion.clone()]).is_undef(),
                "transform_at(planar with {label} axes) should return Undef but didn't",
            );
        }
    }

    // ── joint_jacobian for planar (KCC-γ step-5 / step-6) ───────────────────

    /// `joint_jacobian(planar_joint)` returns a `Value::List` of length 3 with
    /// one analytic twist Map per DOF:
    ///   [0] ∂x DOF:  { angular: [0,0,0], linear: unit_x }
    ///   [1] ∂y DOF:  { angular: [0,0,0], linear: unit_y }
    ///   [2] ∂θ DOF:  { angular: plane_normal, linear: [0,0,0] }
    ///
    /// where plane_normal = unit_x × unit_y (the joint's plane orientation).
    /// For the canonical `planar_xy_joint()` fixture (axis_x = unit_x,
    /// axis_y = unit_y) the plane normal is unit_z. Mirrors the cylindrical
    /// pattern at joint_jacobian_value (joints.rs:815-823) — the `Value::List`
    /// shape (vs. a single Map) preserves analytic per-DOF information and
    /// naturally signals to `loop_closure::per_joint_jacobian_local` (which
    /// expects a single Map) to fall back to FD via its `twist_map_to_array`
    /// failure path (regression-pinned by
    /// `per_joint_jacobian_local_planar_returns_none` in loop_closure.rs).
    #[test]
    fn joint_jacobian_planar_returns_three_column_list_with_analytic_columns() {
        let pj = planar_xy_joint();
        let result = eval_builtin("joint_jacobian", &[pj]);
        assert_jacobian_list_three_columns_planar(
            &result,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            "planar xy axis-aligned",
        );
    }

    /// Non-axis-aligned planar joint exercises the cross-product plane-normal
    /// path: axis_x = [1, 0, 0] and axis_y = [0, 1/√2, 1/√2] (perpendicular to
    /// axis_x). The plane normal is unit_x × axis_y = [0, -1/√2, 1/√2].
    /// Pins that the analytic ∂θ column uses the cross product of the joint's
    /// unit_x and unit_y axes (not a hard-coded unit_z).
    #[test]
    fn joint_jacobian_planar_non_axis_aligned_uses_cross_product_plane_normal() {
        let inv_sqrt2 = 1.0 / std::f64::consts::SQRT_2;
        let axis_y = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(inv_sqrt2),
            Value::Real(inv_sqrt2),
        ]);
        let pj = eval_builtin(
            "planar",
            &[
                axis_x_unit(),
                axis_y,
                length_range_0_to_1m(),
                length_range_0_to_1m(),
                angle_range_0_to_pi(),
            ],
        );
        let result = eval_builtin("joint_jacobian", &[pj]);
        // unit_x × axis_y = ([1,0,0]) × ([0, 1/√2, 1/√2])
        //                 = [0*1/√2 - 0*1/√2, 0*0 - 1*1/√2, 1*1/√2 - 0*0]
        //                 = [0, -1/√2, 1/√2]
        assert_jacobian_list_three_columns_planar(
            &result,
            [1.0, 0.0, 0.0],
            [0.0, inv_sqrt2, inv_sqrt2],
            [0.0, -inv_sqrt2, inv_sqrt2],
            "planar non-axis-aligned (axis_y = [0, 1/√2, 1/√2])",
        );
    }

    /// Helper: assert that `result` is a `Value::List` of length 3 whose
    /// elements are `Map { angular, linear }` columns matching the planar
    /// joint_jacobian contract:
    ///   [0] ∂x DOF:  angular=[0,0,0], linear=unit_x
    ///   [1] ∂y DOF:  angular=[0,0,0], linear=unit_y
    ///   [2] ∂θ DOF:  angular=plane_normal, linear=[0,0,0]
    fn assert_jacobian_list_three_columns_planar(
        result: &Value,
        unit_x: [f64; 3],
        unit_y: [f64; 3],
        plane_normal: [f64; 3],
        label: &str,
    ) {
        let items = match result {
            Value::List(v) => v,
            other => panic!("{label}: expected List, got {:?}", other),
        };
        assert_eq!(
            items.len(),
            3,
            "{label}: List should have exactly 3 columns"
        );
        // column [0]: ∂x DOF (linear = unit_x, angular = zero)
        assert_jacobian_map_components(
            &items[0],
            [0.0, 0.0, 0.0],
            unit_x,
            &format!("{label} col[0] (∂x DOF)"),
        );
        // column [1]: ∂y DOF (linear = unit_y, angular = zero)
        assert_jacobian_map_components(
            &items[1],
            [0.0, 0.0, 0.0],
            unit_y,
            &format!("{label} col[1] (∂y DOF)"),
        );
        // column [2]: ∂θ DOF (angular = plane_normal, linear = zero)
        assert_jacobian_map_components(
            &items[2],
            plane_normal,
            [0.0, 0.0, 0.0],
            &format!("{label} col[2] (∂θ DOF)"),
        );
    }

    // ── planar constructor: validation surface (step-3) ───────────────────────

    #[test]
    fn planar_invalid_args_returns_undef() {
        // Axis helpers for validation cases
        let ax = axis_x_unit(); // [1, 0, 0]
        let ay = axis_y_unit(); // [0, 1, 0]
        let rx = length_range_0_to_1m();
        let ry = length_range_0_to_1m();
        let rt = angle_range_0_to_pi();

        // Wrong dimensioned axis (LENGTH-typed Vector3)
        let length_axis = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        // 2-component axis
        let axis2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        // Non-vector axis
        let non_vec = Value::Real(1.0);
        // Zero axis
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        // NaN axis
        let nan_axis = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        // Non-perpendicular: axis_y = [1,1,0] — dot with [1,0,0] = 1/√2 ≠ 0
        let non_perp_y = Value::Vector(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(0.0)]);
        // Parallel: axis_y = axis_x = [1,0,0]
        let parallel_y = axis_x_unit();
        // Angle-dimensioned range where LENGTH expected
        let angle_range = angle_range_0_to_pi();
        // Length-dimensioned range where ANGLE expected
        let length_range = length_range_0_to_1m();
        // Unbounded range
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };

        let cases: &[(&str, &[Value])] = &[
            // (a) wrong arg counts
            ("0 args", &[]),
            ("1 arg", std::slice::from_ref(&ax)),
            ("4 args", &[ax.clone(), ay.clone(), rx.clone(), ry.clone()]),
            (
                "6 args",
                &[
                    ax.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                    Value::Real(0.0),
                ],
            ),
            // (b) axis_x invalid variants
            (
                "axis_x: bare Real",
                &[
                    non_vec.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_x: 2-component",
                &[
                    axis2.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_x: LENGTH-dimensioned",
                &[
                    length_axis.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_x: zero",
                &[
                    zero_axis.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_x: NaN",
                &[
                    nan_axis.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            // (c) axis_y invalid variants (axis_x valid = [1,0,0])
            (
                "axis_y: bare Real",
                &[
                    ax.clone(),
                    non_vec.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_y: 2-component",
                &[
                    ax.clone(),
                    axis2.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_y: LENGTH-dimensioned",
                &[
                    ax.clone(),
                    length_axis.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_y: zero",
                &[
                    ax.clone(),
                    zero_axis.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            (
                "axis_y: NaN",
                &[
                    ax.clone(),
                    nan_axis.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            // (d) non-perpendicular axes
            (
                "non-perpendicular axes",
                &[
                    ax.clone(),
                    non_perp_y.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            // (e) parallel axes (degenerate + fails perpendicularity)
            (
                "parallel axes (axis_y = axis_x)",
                &[
                    ax.clone(),
                    parallel_y.clone(),
                    rx.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            // (f) range_x wrong dimension
            (
                "range_x: Angle-dimensioned",
                &[
                    ax.clone(),
                    ay.clone(),
                    angle_range.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
            // (g) range_y wrong dimension
            (
                "range_y: Angle-dimensioned",
                &[
                    ax.clone(),
                    ay.clone(),
                    rx.clone(),
                    angle_range.clone(),
                    rt.clone(),
                ],
            ),
            // (h) range_theta wrong dimension
            (
                "range_theta: Length-dimensioned",
                &[
                    ax.clone(),
                    ay.clone(),
                    rx.clone(),
                    ry.clone(),
                    length_range.clone(),
                ],
            ),
            // (i) unbounded range
            (
                "range_x: unbounded",
                &[
                    ax.clone(),
                    ay.clone(),
                    unbounded.clone(),
                    ry.clone(),
                    rt.clone(),
                ],
            ),
        ];

        for (label, args) in cases {
            assert!(
                eval_builtin("planar", args).is_undef(),
                "planar({label}) should return Undef but didn't"
            );
        }
    }

    // ── transform_at on spherical: invalid motion-var (step-9) ───────────────

    /// `transform_at(spherical, motion)` returns Undef whenever `motion` is
    /// not a finite, non-zero `Value::Orientation`. Covers:
    ///   (a) bare Real
    ///   (b) Vector(4) (the wrong "quaternion-ish" shape)
    ///   (c) List of three angles (Euler-tuple shape, deliberately rejected)
    ///   (d) Orientation with NaN in any component
    ///   (e) Orientation with Inf in any component
    ///   (f) Orientation with all-zero components (zero-norm quaternion)
    #[test]
    fn transform_at_spherical_invalid_motion_var_returns_undef() {
        let sj = spherical_joint();

        let cases: Vec<(&str, Value)> = vec![
            // (a) bare Real
            ("bare Real(0.5)", Value::Real(0.5)),
            // (b) Vector(4) — wrong container even though component count matches
            (
                "Vector(4)",
                Value::Vector(vec![
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(1.0),
                ]),
            ),
            // (c) Euler-tuple shape (List of three angles) — deliberately rejected
            (
                "List(3 angles)",
                Value::List(vec![
                    Value::angle(0.1),
                    Value::angle(0.2),
                    Value::angle(0.3),
                ]),
            ),
            // (d) NaN components
            (
                "Orientation w=NaN",
                Value::Orientation {
                    w: f64::NAN,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            (
                "Orientation x=NaN",
                Value::Orientation {
                    w: 1.0,
                    x: f64::NAN,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            (
                "Orientation y=NaN",
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: f64::NAN,
                    z: 0.0,
                },
            ),
            (
                "Orientation z=NaN",
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: f64::NAN,
                },
            ),
            // (e) Inf components — one Inf per axis covers the symmetric
            // `quaternion_is_finite` arms; +Inf and -Inf are mixed across
            // axes to exercise both signs.
            (
                "Orientation w=Inf",
                Value::Orientation {
                    w: f64::INFINITY,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            (
                "Orientation x=Inf",
                Value::Orientation {
                    w: 1.0,
                    x: f64::INFINITY,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            (
                "Orientation y=Inf",
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: f64::INFINITY,
                    z: 0.0,
                },
            ),
            (
                "Orientation z=-Inf",
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: f64::NEG_INFINITY,
                },
            ),
            // (f) zero-norm quaternion (normalize_quaternion returns None)
            (
                "Orientation all-zero",
                Value::Orientation {
                    w: 0.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
        ];

        for (label, motion) in &cases {
            assert!(
                eval_builtin("transform_at", &[sj.clone(), motion.clone()]).is_undef(),
                "transform_at(spherical, {label}) should return Undef but didn't"
            );
        }
    }

    // ── transform_at on spherical: identity quaternion (step-5) ──────────────

    #[test]
    fn transform_at_spherical_identity_quaternion_returns_identity() {
        let sj = spherical_joint();
        let identity_q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("transform_at", &[sj, identity_q]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "spherical identity quaternion → identity Transform",
        );
    }

    // ── transform_at on spherical: general quaternions (step-7) ──────────────

    /// `transform_at(spherical, q)` returns a Transform whose rotation matches
    /// `q` (component-wise) and whose translation is the LENGTH zero vector.
    /// Covers three non-identity cases: 90° about +Z, 180° about +X, and a
    /// general rotation built via `orient_axis_angle([1,1,0], π/3)`.
    #[test]
    fn transform_at_spherical_general_quaternion_preserves_rotation() {
        let pi = std::f64::consts::PI;

        // (a) 90° about +Z: q = (cos(π/4), 0, 0, sin(π/4))
        let cos_q4 = (pi / 4.0).cos();
        let sin_q4 = (pi / 4.0).sin();
        let q_z90 = Value::Orientation {
            w: cos_q4,
            x: 0.0,
            y: 0.0,
            z: sin_q4,
        };
        let result = eval_builtin("transform_at", &[spherical_joint(), q_z90]);
        assert_transform_approx(
            &result,
            (cos_q4, 0.0, 0.0, sin_q4),
            [0.0, 0.0, 0.0],
            1e-12,
            "spherical, 90° about +Z",
        );

        // (b) 180° about +X: q = (0, 1, 0, 0)
        let q_x180 = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("transform_at", &[spherical_joint(), q_x180]);
        assert_transform_approx(
            &result,
            (0.0, 1.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "spherical, 180° about +X",
        );

        // (c) Build a general quaternion via orient_axis_angle([1,1,0], π/3).
        // The rotation sits on a non-axis-aligned axis, so all four quaternion
        // components are non-zero.
        let axis_xy = Value::Vector(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(0.0)]);
        let q_general = eval_builtin("orient_axis_angle", &[axis_xy, Value::Real(pi / 3.0)]);
        let (gw, gx, gy, gz) = match &q_general {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!(
                "orient_axis_angle did not produce an Orientation: {:?}",
                other
            ),
        };
        let result = eval_builtin("transform_at", &[spherical_joint(), q_general.clone()]);
        assert_transform_approx(
            &result,
            (gw, gx, gy, gz),
            [0.0, 0.0, 0.0],
            1e-12,
            "spherical, general axis-angle [1,1,0]/√2 by π/3",
        );
    }

    // ── spherical constructor: validation surface (step-3) ───────────────────

    #[test]
    fn spherical_invalid_args_returns_undef() {
        let unbounded_upper = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        let unbounded_lower = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: false,
            upper_inclusive: true,
        };

        let cases: Vec<(&str, Vec<Value>)> = vec![
            // (a) wrong arg counts
            ("0 args", vec![]),
            ("2 args", vec![angle_range_0_to_pi(), angle_range_0_to_pi()]),
            (
                "3 args",
                vec![
                    angle_range_0_to_pi(),
                    angle_range_0_to_pi(),
                    angle_range_0_to_pi(),
                ],
            ),
            // (b) range_angle wrong dimension (LENGTH-typed range)
            ("LENGTH-typed range", vec![length_range_0_to_1m()]),
            // (c) range_angle unbounded
            ("unbounded upper", vec![unbounded_upper]),
            ("unbounded lower", vec![unbounded_lower]),
            // (d) range_angle non-Range types
            ("bare Real", vec![Value::Real(0.0)]),
            (
                "bare Vector",
                vec![Value::Vector(vec![
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ])],
            ),
            ("bare Map", vec![Value::Map(Default::default())]),
        ];

        for (label, args) in &cases {
            assert!(
                eval_builtin("spherical", args).is_undef(),
                "spherical({label}) should return Undef but didn't"
            );
        }
    }

    // ── spherical constructor: happy path (step-1) ────────────────────────────

    #[test]
    fn spherical_returns_map_with_correct_fields() {
        let range_angle = angle_range_0_to_pi();
        let result = eval_builtin("spherical", std::slice::from_ref(&range_angle));

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("spherical".to_string())),
            "kind field should be 'spherical'"
        );
        assert_eq!(
            map.get(&Value::String("range_angle".to_string())),
            Some(&range_angle),
            "range_angle field should match input"
        );
        assert_eq!(
            map.len(),
            2,
            "spherical joint Map should have exactly 2 keys (kind, range_angle), got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // ── joint_jacobian for spherical (KCC-γ step-7 / step-8) ────────────────

    /// `joint_jacobian(spherical_joint)` returns a `Value::List` of length 3
    /// with one analytic angular twist Map per body-frame basis DOF:
    ///   [0] ∂ω_x DOF:  { angular: [1,0,0], linear: [0,0,0] }
    ///   [1] ∂ω_y DOF:  { angular: [0,1,0], linear: [0,0,0] }
    ///   [2] ∂ω_z DOF:  { angular: [0,0,1], linear: [0,0,0] }
    ///
    /// Spherical is axis-isotropic (no preferred direction stored in the joint
    /// Map; see `make_spherical`) so the local-frame Jacobian columns are the
    /// body-frame basis vectors at q = identity. SE(3) adjoint transport via
    /// FD chain composition (`chain_jacobian_fd`) handles the q-dependent
    /// world-frame transport implicitly until KCC-θ/ι. Mirrors the
    /// cylindrical (joints.rs:815-823) and planar pattern: the `Value::List`
    /// shape signals to `loop_closure::per_joint_jacobian_local` to fall back
    /// to FD chain composition (regression-pinned by
    /// `per_joint_jacobian_local_spherical_returns_none` in loop_closure.rs).
    #[test]
    fn joint_jacobian_spherical_returns_three_column_list_with_body_basis_columns() {
        let sj = spherical_joint();
        let result = eval_builtin("joint_jacobian", &[sj]);
        let items = match &result {
            Value::List(v) => v,
            other => panic!(
                "joint_jacobian(spherical): expected List of 3 columns, got {:?}",
                other
            ),
        };
        assert_eq!(
            items.len(),
            3,
            "joint_jacobian(spherical): List should have exactly 3 columns"
        );
        assert_jacobian_map_components(
            &items[0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            "spherical col[0] (∂ω_x DOF)",
        );
        assert_jacobian_map_components(
            &items[1],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            "spherical col[1] (∂ω_y DOF)",
        );
        assert_jacobian_map_components(
            &items[2],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            "spherical col[2] (∂ω_z DOF)",
        );
    }

    /// Sign-pin regression test (KCC-γ step-7, resolves PRD §14.3): the
    /// analytic ∂ω_z column's angular_z sign must match the sign produced by
    /// `transform_log` for a positive rotation about +Z through a spherical
    /// joint. Construct a chain `[spherical_joint()]`, take `chain_transform`
    /// at q = orient_axis_angle(+Z, +π/4), take `transform_log` of the result,
    /// and confirm angular_z is positive — matching the analytic column's
    /// positive angular_z entry.
    ///
    /// This pins the column-order / sign convention against
    /// `transform_log_exp_round_trip` (geometry.rs:3625) — any future change
    /// to the spherical analytic Jacobian must keep the sign convention
    /// consistent with the existing log/exp pipeline, otherwise the
    /// Newton solver's residual gradient flips and the loop diverges.
    #[test]
    fn spherical_analytic_jacobian_matches_transform_log_convention() {
        use crate::loop_closure::chain_transform;
        use crate::loop_closure_value::JointValue;

        let theta = std::f64::consts::FRAC_PI_4;
        let q = eval_builtin(
            "orient_axis_angle",
            &[axis_z_unit(), Value::angle(theta)],
        );
        let (qw, qx, qy, qz) = match q {
            Value::Orientation { w, x, y, z } => (w, x, y, z),
            other => panic!("orient_axis_angle returned non-Orientation: {:?}", other),
        };

        // chain_transform of a single spherical joint at q = R(+Z, +π/4).
        let chain = vec![spherical_joint()];
        let vals = vec![JointValue::Sphere([qw, qx, qy, qz])];
        let t = chain_transform(&chain, &vals)
            .expect("chain_transform must return Some for a spherical-only chain");

        // transform_log gives the twist; angular_z must equal +π/4 (positive).
        let twist = eval_builtin("transform_log", &[t]);
        let twist_map = match &twist {
            Value::Map(m) => m,
            other => panic!("transform_log returned non-Map: {:?}", other),
        };
        let angular = match twist_map.get(&Value::String("angular".to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => items,
            other => panic!("transform_log: expected Vector3 angular, got {:?}", other),
        };
        let omega_z = angular[2].as_f64().expect("angular_z must be finite");
        assert!(
            (omega_z - theta).abs() < 1e-12,
            "transform_log angular_z = {omega_z}, expected +π/4 = {theta} (positive). \
             Convention has shifted from the established transform_log_exp_round_trip \
             pipeline at geometry.rs:3625 — the spherical analytic Jacobian's column \
             sign must follow."
        );

        // Cross-check: the analytic Jacobian's ∂ω_z column.angular_z must also
        // be positive (= +1.0). This is the regression pin against PRD §14.3.
        let jac = eval_builtin("joint_jacobian", &[spherical_joint()]);
        let cols = match &jac {
            Value::List(v) if v.len() == 3 => v,
            other => panic!(
                "joint_jacobian(spherical): expected List of 3 columns, got {:?}",
                other
            ),
        };
        let col_z_map = match &cols[2] {
            Value::Map(m) => m,
            other => panic!("col[2]: expected Map, got {:?}", other),
        };
        let col_z_ang = match col_z_map.get(&Value::String("angular".to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => items,
            other => panic!("col[2].angular: expected Vector3, got {:?}", other),
        };
        let col_z_angular_z = col_z_ang[2]
            .as_f64()
            .expect("col[2].angular[2] must be finite");
        assert!(
            col_z_angular_z > 0.0 && (col_z_angular_z - 1.0).abs() < 1e-12,
            "spherical analytic Jacobian col[2].angular_z = {col_z_angular_z}, \
             expected +1.0 (positive, matching transform_log sign convention). \
             If this drifted to -1.0, the Newton solver's residual gradient flips \
             and KCC-γ loops will diverge — see PRD §14.3."
        );
        // omega_z and col_z_angular_z must have the same sign (both positive).
        assert!(
            omega_z * col_z_angular_z > 0.0,
            "sign mismatch: transform_log angular_z={omega_z} vs analytic col[2].angular_z={col_z_angular_z}"
        );
    }

    // ── JOINT_KINDS membership regression pin (step-15) ──────────────────────
    //
    // Asserts that `"planar"` is a member of `JOINT_KINDS` so that:
    //  1. `is_joint_value` accepts planar joints as valid mechanism joints, and
    //  2. the existing dispatch-coverage tests (`transform_at_dispatches_for_every_joint_kind`
    //     and `joint_jacobian_dispatches_for_every_joint_kind`) iterate over the planar kind.
    //
    // This test fails until step-16 appends `"planar"` to `JOINT_KINDS`.
    #[test]
    fn joint_kinds_includes_planar() {
        assert!(
            JOINT_KINDS.contains(&"planar"),
            "\"planar\" must be in JOINT_KINDS so that is_joint_value accepts planar joints \
             and the dispatch-coverage tests exercise the planar arms in transform_at and \
             joint_jacobian_value. Add \"planar\" to the JOINT_KINDS const."
        );
    }

    // ── JOINT_KINDS membership regression pin for spherical (step-13) ────────
    //
    // Asserts that `"spherical"` is a member of `JOINT_KINDS` so that:
    //  1. `is_joint_value` accepts spherical joints as valid mechanism joints, and
    //  2. the existing dispatch-coverage tests
    //     (`transform_at_dispatches_for_every_joint_kind` and
    //     `joint_jacobian_dispatches_for_every_joint_kind`) iterate over spherical.
    //
    // This test fails until step-14 appends `"spherical"` to `JOINT_KINDS` and
    // adds the spherical arm to `joint_kind_minimal_fixture`.
    #[test]
    fn joint_kinds_includes_spherical() {
        assert!(
            JOINT_KINDS.contains(&"spherical"),
            "\"spherical\" must be in JOINT_KINDS so that is_joint_value accepts spherical \
             joints and the dispatch-coverage tests exercise the spherical arms in \
             transform_at and joint_jacobian_value. Add \"spherical\" to the JOINT_KINDS const."
        );
    }

    // ── is_joint_value recognises cylindrical (step-15) ──────────────────────

    /// `is_joint_value(map)` returns true for both a hand-built
    /// `{ "kind" → "cylindrical" }` Map and the well-formed Map produced by
    /// the `cylindrical` constructor. Pins the contract that adding
    /// `"cylindrical"` to `JOINT_KINDS` (step-16) makes the predicate accept
    /// cylindrical joints.
    #[test]
    fn is_joint_value_recognizes_cylindrical() {
        use std::collections::BTreeMap;

        // (a) hand-built Map with kind="cylindrical"
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("cylindrical".to_string()),
        );
        assert!(
            is_joint_value(&Value::Map(m)),
            "Map with kind='cylindrical' should be a joint value once step-16 adds the kind to JOINT_KINDS"
        );

        // (b) constructor output → Map shape that is_joint_value accepts
        let cyl = eval_builtin(
            "cylindrical",
            &[axis_z_unit(), length_range_0_to_1m(), angle_range_0_to_pi()],
        );
        assert!(
            is_joint_value(&cyl),
            "cylindrical(...) constructor output should be recognised as a joint value"
        );
    }

    // ── JOINT_KINDS membership regression pin for cylindrical (step-15) ──────
    //
    // Asserts that `"cylindrical"` is a member of `JOINT_KINDS` so that:
    //  1. `is_joint_value` accepts cylindrical joints as valid mechanism joints, and
    //  2. the existing dispatch-coverage tests
    //     (`transform_at_dispatches_for_every_joint_kind` and
    //     `joint_jacobian_dispatches_for_every_joint_kind`) iterate over cylindrical.
    //
    // This test fails until step-16 appends `"cylindrical"` to `JOINT_KINDS` and
    // adds the cylindrical arm to `joint_kind_minimal_fixture`.
    #[test]
    fn joint_kinds_includes_cylindrical() {
        assert!(
            JOINT_KINDS.contains(&"cylindrical"),
            "\"cylindrical\" must be in JOINT_KINDS so that is_joint_value accepts cylindrical \
             joints and the dispatch-coverage tests exercise the cylindrical arms in \
             transform_at and joint_jacobian_value. Add \"cylindrical\" to the JOINT_KINDS const."
        );
    }

    // ── cylindrical transform_at: translation-only (step-5) ──────────────────

    /// `transform_at(cyl, [length(0.5), angle(0.0)])` returns a Transform with
    /// identity rotation and translation 0.5m along the cylindrical joint's
    /// axis (here, +Z). Verifies the translation arm of the cylindrical
    /// transform_at dispatch in isolation (rotation = identity).
    #[test]
    fn cylindrical_transform_at_translation_only() {
        let cyl = cylindrical_z_joint();
        let motion = Value::List(vec![Value::length(0.5), Value::angle(0.0)]);
        let result = eval_builtin("transform_at", &[cyl, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.5],
            1e-12,
            "cyl Z, d=0.5m θ=0",
        );
    }

    // ── cylindrical transform_at: rotation-only (step-7) ─────────────────────

    /// `transform_at(cyl, [length(0.0), angle(π/2)])` returns a Transform whose
    /// rotation is the analytical revolute-Z quaternion (cos(π/4), 0, 0, sin(π/4))
    /// and zero translation. Verifies the rotation arm of the cylindrical
    /// transform_at dispatch in isolation.
    #[test]
    fn cylindrical_transform_at_rotation_only() {
        let cyl = cylindrical_z_joint();
        let pi = std::f64::consts::PI;
        let motion = Value::List(vec![Value::length(0.0), Value::angle(pi / 2.0)]);
        let result = eval_builtin("transform_at", &[cyl, motion]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "cyl Z, d=0 θ=π/2",
        );
    }

    // ── cylindrical transform_at: combined motion + unnormalised axis (step-9) ──

    /// Three sub-scenarios for `transform_at(cylindrical, [d, θ])`:
    /// (a) unit Z axis, combined translation+rotation,
    /// (b) unnormalised axis (magnitude 2 along +X) — verifies axis is
    ///     normalised before being used in both the rotation quaternion and
    ///     the translation scaling,
    /// (c) diagonal axis [1,1,0]/√2 — verifies translation along a non-axis-
    ///     aligned direction.
    #[test]
    fn cylindrical_transform_at_combined_and_unnormalized_axis() {
        let pi = std::f64::consts::PI;

        // (a) unit Z axis, [d=0.3m, θ=π/4]
        let cyl_z = cylindrical_z_joint();
        let motion_a = Value::List(vec![Value::length(0.3), Value::angle(pi / 4.0)]);
        let result_a = eval_builtin("transform_at", &[cyl_z, motion_a]);
        let cos_a = (pi / 8.0).cos();
        let sin_a = (pi / 8.0).sin();
        assert_transform_approx(
            &result_a,
            (cos_a, 0.0, 0.0, sin_a),
            [0.0, 0.0, 0.3],
            1e-12,
            "cyl Z, d=0.3m θ=π/4",
        );

        // (b) unnormalised axis [2, 0, 0] → unit X; [d=1.0m, θ=π/2] → translation [1,0,0],
        //     rotation about +X by π/2 = (cos(π/4), sin(π/4), 0, 0).
        let axis_2x = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let cyl_2x = eval_builtin(
            "cylindrical",
            &[axis_2x, length_range_0_to_1m(), angle_range_0_to_pi()],
        );
        let motion_b = Value::List(vec![Value::length(1.0), Value::angle(pi / 2.0)]);
        let result_b = eval_builtin("transform_at", &[cyl_2x, motion_b]);
        let cos_b = (pi / 4.0).cos();
        let sin_b = (pi / 4.0).sin();
        assert_transform_approx(
            &result_b,
            (cos_b, sin_b, 0.0, 0.0),
            [1.0, 0.0, 0.0],
            1e-12,
            "cyl unnormalised [2,0,0], d=1m θ=π/2",
        );

        // (c) diagonal axis [1,1,0]/√2 unit; [d=√2 m, θ=0] → translation [1,1,0], identity rotation.
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        let axis_diag = Value::Vector(vec![Value::Real(s2), Value::Real(s2), Value::Real(0.0)]);
        let cyl_diag = eval_builtin(
            "cylindrical",
            &[axis_diag, length_range_0_to_1m(), angle_range_0_to_pi()],
        );
        let motion_c = Value::List(vec![
            Value::length(std::f64::consts::SQRT_2),
            Value::angle(0.0),
        ]);
        let result_c = eval_builtin("transform_at", &[cyl_diag, motion_c]);
        assert_transform_approx(
            &result_c,
            (1.0, 0.0, 0.0, 0.0),
            [1.0, 1.0, 0.0],
            1e-12,
            "cyl diag [1,1,0]/√2, d=√2 m θ=0",
        );
    }

    // ── cylindrical joint_jacobian: two-column List (step-13) ────────────────

    /// `joint_jacobian(cyl)` returns a `Value::List` of length 2 with one twist
    /// Map per DOF:
    ///   [0] prismatic DOF: { angular: [0,0,0], linear: unit_axis }
    ///   [1] revolute  DOF: { angular: unit_axis, linear: [0,0,0] }
    ///
    /// Three sub-scenarios cover unit-Z, unit-X, and an unnormalised axis
    /// `[3, 4, 0]` (magnitude 5) that must normalise to `[0.6, 0.8, 0]` in
    /// both columns.  The List shape (vs. a single Map) is essential: it
    /// preserves analytic per-DOF information and naturally signals to
    /// `loop_closure::per_joint_jacobian_local` (which expects a single Map)
    /// to fall back to FD via its `twist_map_to_array` failure path.
    #[test]
    fn cylindrical_joint_jacobian_returns_two_columns() {
        // (a) unit Z
        let cyl_z = cylindrical_z_joint();
        let result_z = eval_builtin("joint_jacobian", &[cyl_z]);
        assert_jacobian_list_two_columns(&result_z, [0.0, 0.0, 1.0], "cyl Z unit");

        // (b) unit X
        let cyl_x = eval_builtin(
            "cylindrical",
            &[axis_x_unit(), length_range_0_to_1m(), angle_range_0_to_pi()],
        );
        let result_x = eval_builtin("joint_jacobian", &[cyl_x]);
        assert_jacobian_list_two_columns(&result_x, [1.0, 0.0, 0.0], "cyl X unit");

        // (c) unnormalised axis [3, 4, 0] (magnitude 5) → unit [0.6, 0.8, 0]
        let axis_345 = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let cyl_345 = eval_builtin(
            "cylindrical",
            &[axis_345, length_range_0_to_1m(), angle_range_0_to_pi()],
        );
        let result_345 = eval_builtin("joint_jacobian", &[cyl_345]);
        assert_jacobian_list_two_columns(&result_345, [0.6, 0.8, 0.0], "cyl unnormalised [3,4,0]");
    }

    /// Helper: assert that `result` is a `Value::List` of length 2 whose
    /// elements are `Map { angular, linear }` columns matching the cylindrical
    /// joint_jacobian contract — element [0] is the prismatic-DOF column
    /// (linear = `unit_axis`, angular = zero) and element [1] is the
    /// revolute-DOF column (angular = `unit_axis`, linear = zero).
    fn assert_jacobian_list_two_columns(result: &Value, unit_axis: [f64; 3], label: &str) {
        let items = match result {
            Value::List(v) => v,
            other => panic!("{label}: expected List, got {:?}", other),
        };
        assert_eq!(
            items.len(),
            2,
            "{label}: List should have exactly 2 columns"
        );

        // column [0]: prismatic DOF
        assert_jacobian_map_components(
            &items[0],
            [0.0, 0.0, 0.0],
            unit_axis,
            &format!("{label} col[0] (prismatic DOF)"),
        );
        // column [1]: revolute DOF
        assert_jacobian_map_components(
            &items[1],
            unit_axis,
            [0.0, 0.0, 0.0],
            &format!("{label} col[1] (revolute DOF)"),
        );
    }

    /// Helper: assert a Jacobian Map has the expected angular and linear
    /// Vector3 components (component-wise within 1e-12).
    fn assert_jacobian_map_components(
        map_val: &Value,
        exp_ang: [f64; 3],
        exp_lin: [f64; 3],
        label: &str,
    ) {
        let map = match map_val {
            Value::Map(m) => m,
            other => panic!("{label}: expected Map, got {:?}", other),
        };
        let read_vec3 = |key: &str| -> [f64; 3] {
            let v = map
                .get(&Value::String(key.to_string()))
                .unwrap_or_else(|| panic!("{label}: missing key {key:?}"));
            let items = match v {
                Value::Vector(items) if items.len() == 3 => items,
                other => panic!("{label}: {key} expected Vector3, got {:?}", other),
            };
            [
                items[0].as_f64().unwrap(),
                items[1].as_f64().unwrap(),
                items[2].as_f64().unwrap(),
            ]
        };
        let ang = read_vec3("angular");
        let lin = read_vec3("linear");
        for i in 0..3 {
            assert!(
                (ang[i] - exp_ang[i]).abs() < 1e-12,
                "{label}: angular[{i}] expected {} got {}",
                exp_ang[i],
                ang[i]
            );
            assert!(
                (lin[i] - exp_lin[i]).abs() < 1e-12,
                "{label}: linear[{i}] expected {} got {}",
                exp_lin[i],
                lin[i]
            );
        }
    }

    // ── cylindrical transform_at: invalid motion-var (step-11) ──────────────

    /// Table-driven validation surface for the cylindrical transform_at second
    /// argument. All listed shapes return Undef. Also includes two positive
    /// polarity cases: (1) a 2-element `Value::List` with the right dims that
    /// MUST return a Transform — guards against accidental over-rejection of
    /// the canonical List container shape; (2) a bare `Value::Real` pair that
    /// pins the `length_input`/`trig_input` bare-Real contract — without this,
    /// a future tightening to require dimensioned Scalars could silently shrink
    /// cylindrical's accepted input set.
    ///
    /// Container shape is List-only (mirrors the planar arm); a 2-element
    /// `Value::Vector` is rejected as a negative case to keep the joint-zoo
    /// surface consistent. Reify `[a, b]` literals lower to `Value::List`,
    /// so List-only is the canonical motion-var shape.
    #[test]
    fn cylindrical_transform_at_invalid_value_returns_undef() {
        let cyl = cylindrical_z_joint();

        let undef_cases: &[(&str, Value)] = &[
            ("bare scalar Length", Value::length(0.5)),
            ("List of 0", Value::List(vec![])),
            ("List of 1 element", Value::List(vec![Value::length(0.5)])),
            (
                "List of 3 elements",
                Value::List(vec![
                    Value::length(0.5),
                    Value::angle(0.0),
                    Value::angle(0.0),
                ]),
            ),
            (
                "dim-swapped: [angle, length]",
                Value::List(vec![Value::angle(0.5), Value::length(0.5)]),
            ),
            (
                "NaN translation, valid angle",
                Value::List(vec![Value::Real(f64::NAN), Value::angle(0.0)]),
            ),
            (
                "Inf translation, valid angle",
                Value::List(vec![Value::Real(f64::INFINITY), Value::angle(0.0)]),
            ),
            (
                "valid translation, NaN rotation",
                Value::List(vec![Value::length(0.0), Value::Real(f64::NAN)]),
            ),
            (
                "zero translation, Inf rotation",
                Value::List(vec![Value::length(0.0), Value::Real(f64::INFINITY)]),
            ),
            // Vector container is rejected (List-only contract, mirrors planar).
            (
                "Vector container (wrong shape — List required)",
                Value::Vector(vec![Value::length(0.5), Value::angle(0.0)]),
            ),
        ];

        for (label, motion) in undef_cases {
            assert!(
                eval_builtin("transform_at", &[cyl.clone(), motion.clone()]).is_undef(),
                "transform_at(cyl, {label}) should return Undef but didn't"
            );
        }

        // Positive polarity: 2-element List container is valid.
        let list_motion = Value::List(vec![Value::length(0.5), Value::angle(0.0)]);
        let result = eval_builtin("transform_at", &[cyl.clone(), list_motion]);
        assert!(
            matches!(&result, Value::Transform { .. }),
            "transform_at(cyl, List[length, angle]) must return Transform, got {:?}",
            result
        );

        // Positive polarity: bare-Real motion vars (interpreted as metres/radians).
        // Locks in that length_input/trig_input accept Value::Real, mirroring the
        // planar arm. Without this, a future tightening to require dimensioned
        // Scalars could silently shrink cylindrical's accepted input set.
        let bare_real_motion = Value::List(vec![Value::Real(0.5), Value::Real(0.0)]);
        let result = eval_builtin("transform_at", &[cyl.clone(), bare_real_motion]);
        assert!(
            matches!(&result, Value::Transform { .. }),
            "transform_at(cyl, List[Real, Real]) must return Transform, got {:?}",
            result
        );
    }

    // ── cylindrical constructor: validation surface (step-3) ─────────────────

    /// Table-driven validation surface for the cylindrical constructor.
    ///
    /// Every row should produce `Value::Undef`. Covers wrong arg counts, axis
    /// validation failures (reuses `validate_axis`'s contracts), translation_range
    /// validation failures (LENGTH-typed bounded `Range`), and rotation_range
    /// validation failures (ANGLE-typed bounded `Range`). Mirrors the structure
    /// of `planar_invalid_args_returns_undef` and `spherical_invalid_args_returns_undef`.
    #[test]
    fn cylindrical_constructor_validation_table() {
        let ax = axis_z_unit();
        let tr = length_range_0_to_1m();
        let rr = angle_range_0_to_pi();

        // Wrong-dimensioned axis (LENGTH-typed Vector3)
        let length_axis = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        // 2-component axis
        let axis2 = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0)]);
        // Non-vector axis
        let non_vec = Value::Real(1.0);
        // Zero axis
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        // NaN axis
        let nan_axis = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        // Unbounded range (lower=Some, upper=None)
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        let unbounded_angle = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };

        let cases: Vec<(&str, Vec<Value>)> = vec![
            // (a) wrong arg counts
            ("0 args", vec![]),
            ("1 arg", vec![ax.clone()]),
            ("2 args", vec![ax.clone(), tr.clone()]),
            (
                "4 args",
                vec![ax.clone(), tr.clone(), rr.clone(), Value::Real(0.0)],
            ),
            // (b) axis invalid variants
            (
                "axis: bare Real",
                vec![non_vec.clone(), tr.clone(), rr.clone()],
            ),
            (
                "axis: 2-component",
                vec![axis2.clone(), tr.clone(), rr.clone()],
            ),
            (
                "axis: LENGTH-dimensioned",
                vec![length_axis.clone(), tr.clone(), rr.clone()],
            ),
            (
                "axis: zero",
                vec![zero_axis.clone(), tr.clone(), rr.clone()],
            ),
            ("axis: NaN", vec![nan_axis.clone(), tr.clone(), rr.clone()]),
            // (c) translation_range invalid
            (
                "translation_range: bare Real",
                vec![ax.clone(), Value::Real(1.0), rr.clone()],
            ),
            (
                "translation_range: unbounded",
                vec![ax.clone(), unbounded.clone(), rr.clone()],
            ),
            (
                "translation_range: ANGLE-dim",
                vec![ax.clone(), angle_range_0_to_pi(), rr.clone()],
            ),
            // (d) rotation_range invalid
            (
                "rotation_range: bare Real",
                vec![ax.clone(), tr.clone(), Value::Real(1.0)],
            ),
            (
                "rotation_range: unbounded",
                vec![ax.clone(), tr.clone(), unbounded_angle.clone()],
            ),
            (
                "rotation_range: LENGTH-dim",
                vec![ax.clone(), tr.clone(), length_range_0_to_1m()],
            ),
        ];

        for (label, args) in &cases {
            assert_builtin_undef("cylindrical", args, label);
        }
    }

    // ── cylindrical constructor: happy path (step-1) ─────────────────────────

    /// `cylindrical(axis, translation_range, rotation_range)` returns a 4-key Map
    /// with `kind="cylindrical"` and the three input fields stored verbatim.
    ///
    /// PRD task 5: 2-DOF composite of prismatic ⊕ revolute on a single shared
    /// axis. The Map shape is flat (mirrors prismatic/revolute/planar/spherical):
    ///   { "axis", "kind", "rotation_range", "translation_range" }
    /// (alphabetically ordered via `BTreeMap`).
    #[test]
    fn cylindrical_returns_map_with_correct_fields() {
        let axis = axis_z_unit();
        let translation_range = length_range_0_to_1m();
        let rotation_range = angle_range_0_to_pi();
        let result = eval_builtin(
            "cylindrical",
            &[
                axis.clone(),
                translation_range.clone(),
                rotation_range.clone(),
            ],
        );

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("cylindrical".to_string())),
            "kind field should be 'cylindrical'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("translation_range".to_string())),
            Some(&translation_range),
            "translation_range field should match input"
        );
        assert_eq!(
            map.get(&Value::String("rotation_range".to_string())),
            Some(&rotation_range),
            "rotation_range field should match input"
        );
        assert_eq!(
            map.len(),
            4,
            "cylindrical joint Map should have exactly 4 keys \
             (kind, axis, translation_range, rotation_range), got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // ── motion_subspace_columns: fixed ────────────────────────────────────────

    // ── motion_subspace_columns: PRD regression-pin ───────────────────────────

    /// PRD §10 Phase 2 task δ "Observable signal" regression pin.
    ///
    /// Quoted verbatim: "for prismatic the column equals `[0; axis]`"
    ///
    /// Table-driven: for each (kind, joint, expected_dof_count) row, assert
    /// `Some(cols)` with `cols.len() == expected_dof_count`. Additionally for
    /// the prismatic row, assert the explicit `[0;axis]` acceptance signal.
    #[test]
    fn motion_subspace_columns_dof_counts_match_per_kind() {
        struct Row {
            kind: &'static str,
            joint: Value,
            expected_dof: usize,
        }

        let rows = vec![
            Row { kind: "prismatic",   joint: prismatic_x_joint(),    expected_dof: 1 },
            Row { kind: "revolute",    joint: revolute_z_joint(),      expected_dof: 1 },
            Row { kind: "cylindrical", joint: cylindrical_z_joint(),   expected_dof: 2 },
            Row { kind: "planar",      joint: planar_xy_joint(),       expected_dof: 3 },
            Row { kind: "spherical",   joint: spherical_joint(),       expected_dof: 3 },
            Row { kind: "fixed",       joint: eval_builtin("fixed", &[]), expected_dof: 0 },
        ];

        for Row { kind, joint, expected_dof } in &rows {
            let cols = super::motion_subspace_columns(joint).unwrap_or_else(|| {
                panic!("motion_subspace_columns({}) returned None", kind)
            });
            assert_eq!(
                cols.len(),
                *expected_dof,
                "DOF count mismatch for {}: expected {}, got {}",
                kind, expected_dof, cols.len()
            );
        }

        // PRD §10 Phase 2 task δ explicit acceptance signal:
        // "for prismatic the column equals [0; axis]"
        let prismatic_cols = super::motion_subspace_columns(&prismatic_x_joint())
            .expect("prismatic should return Some");
        let arr = prismatic_cols[0].as_array();
        let expected = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        for (i, (&got, exp)) in arr.iter().zip(expected).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "PRD §10 δ pin — prismatic column[{}]: expected {}, got {}",
                i, exp, got
            );
        }
    }

    // ── motion_subspace_columns: invalid inputs ───────────────────────────────

    /// Table-driven validation pins for `motion_subspace_columns`.
    ///
    /// Covers the contract: "Returns None for non-Map inputs, Maps without a kind
    /// discriminator, unknown kinds, coupling joints (out of scope for v0.3), and
    /// joints with malformed/missing axis or non-perpendicular planar axes."
    #[test]
    fn motion_subspace_columns_invalid_inputs_return_none() {
        // (a) Non-Map input.
        let non_map = Value::Real(1.0);
        assert!(
            super::motion_subspace_columns(&non_map).is_none(),
            "(a) non-Map input should return None"
        );

        // (b) Map missing the "kind" key.
        let empty_map = Value::Map(std::collections::BTreeMap::new());
        assert!(
            super::motion_subspace_columns(&empty_map).is_none(),
            "(b) Map missing 'kind' should return None"
        );

        // (c) Map with unknown kind "sliding".
        let mut unknown_kind_map = std::collections::BTreeMap::new();
        unknown_kind_map.insert(
            Value::String("kind".to_string()),
            Value::String("sliding".to_string()),
        );
        let unknown_kind_joint = Value::Map(unknown_kind_map);
        assert!(
            super::motion_subspace_columns(&unknown_kind_joint).is_none(),
            "(c) unknown kind 'sliding' should return None"
        );

        // (d) Coupling joint — out of scope for v0.3.
        let coupling_joint = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            super::motion_subspace_columns(&coupling_joint).is_none(),
            "(d) coupling joint should return None (out of scope for v0.3)"
        );

        // (e) Prismatic Map with missing "axis" key → None via unit_axis_from_map.
        let mut prismatic_no_axis = std::collections::BTreeMap::new();
        prismatic_no_axis.insert(
            Value::String("kind".to_string()),
            Value::String("prismatic".to_string()),
        );
        prismatic_no_axis.insert(
            Value::String("range".to_string()),
            length_range_0_to_1m(),
        );
        let prismatic_malformed = Value::Map(prismatic_no_axis);
        assert!(
            super::motion_subspace_columns(&prismatic_malformed).is_none(),
            "(e) prismatic Map missing 'axis' should return None"
        );

        // (f) Planar Map with non-perpendicular axes → None via unit_axes_xy_from_planar_map.
        // axis_x = e_x, axis_y = e_x (parallel, dot = 1.0 >> 1e-9 threshold).
        let parallel_planar = eval_builtin(
            "planar",
            &[
                axis_x_unit(),
                axis_x_unit(), // same axis — rejects at constructor
                length_range_0_to_1m(),
                length_range_0_to_1m(),
                angle_range_0_to_pi(),
            ],
        );
        // The constructor returns Undef for parallel axes; if it somehow slips through,
        // motion_subspace_columns must also return None.
        // Undef is Value::Undef, which is not a Map — case (a) handles it.
        assert!(
            super::motion_subspace_columns(&parallel_planar).is_none(),
            "(f) planar with non-perpendicular axes should return None"
        );
    }

    // ── motion_subspace_columns: spherical ───────────────────────────────────

    /// PRD §4.2 pin: spherical is axis-isotropic — no stored axis. Motion-subspace
    /// columns are the body-frame basis vectors (three pure-angular columns).
    ///
    ///   col[0] = [1,0,0, 0,0,0]  (angular about body-local +x)
    ///   col[1] = [0,1,0, 0,0,0]  (angular about body-local +y)
    ///   col[2] = [0,0,1, 0,0,0]  (angular about body-local +z)
    ///
    /// World-frame transformation is RNEA's responsibility via X_{p→i}.
    #[test]
    fn motion_subspace_columns_spherical_returns_three_pure_angular_columns() {
        let joint = spherical_joint();
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on spherical joint should return Some");
        assert_eq!(cols.len(), 3, "spherical has 3 DOFs — expected 3 columns");

        let expected: [[f64; 6]; 3] = [
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0], // col[0]: angular about body +x
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0], // col[1]: angular about body +y
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0], // col[2]: angular about body +z
        ];
        for (ci, (col, exp)) in cols.iter().zip(expected.iter()).enumerate() {
            let arr = col.as_array();
            for (i, (&got, &e)) in arr.iter().zip(exp.iter()).enumerate() {
                assert!(
                    (got - e).abs() < 1e-12,
                    "spherical col[{}][{}]: expected {}, got {}",
                    ci, i, e, got
                );
            }
        }
    }

    // ── motion_subspace_columns: planar ──────────────────────────────────────

    /// PRD §5.1 pin: planar columns = `[linear_axis_x, linear_axis_y, angular_normal]`
    /// where `normal = axis_x × axis_y`. Ordering matches the `transform_at` planar
    /// motion-var order `[x_length, y_length, theta_angle]`.
    ///
    /// For `planar_xy_joint()` (axis_x = e_x, axis_y = e_y):
    ///   col[0] = [0,0,0, 1,0,0]  (linear along +X)
    ///   col[1] = [0,0,0, 0,1,0]  (linear along +Y)
    ///   col[2] = [0,0,1, 0,0,0]  (angular about +Z = e_x × e_y)
    #[test]
    fn motion_subspace_columns_planar_returns_three_columns_with_normal_angular() {
        let joint = planar_xy_joint();
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on planar joint should return Some");
        assert_eq!(cols.len(), 3, "planar has 3 DOFs — expected 3 columns");

        let expected: [[f64; 6]; 3] = [
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0], // col[0]: linear along axis_x = +X
            [0.0, 0.0, 0.0, 0.0, 1.0, 0.0], // col[1]: linear along axis_y = +Y
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0], // col[2]: angular about normal = e_x × e_y = +Z
        ];
        for (ci, (col, exp)) in cols.iter().zip(expected.iter()).enumerate() {
            let arr = col.as_array();
            for (i, (&got, &e)) in arr.iter().zip(exp.iter()).enumerate() {
                assert!(
                    (got - e).abs() < 1e-12,
                    "planar col[{}][{}]: expected {}, got {}",
                    ci, i, e, got
                );
            }
        }
    }

    // ── motion_subspace_columns: cylindrical ─────────────────────────────────

    /// PRD §12 Q4 pin: cylindrical columns are `[translation, rotation]`.
    /// Column 0 = `[0; unit_axis]` (prismatic-equivalent, linear along +Z),
    /// column 1 = `[unit_axis; 0]` (revolute-equivalent, angular about +Z).
    #[test]
    fn motion_subspace_columns_cylindrical_returns_translation_then_rotation_columns() {
        let joint = cylindrical_z_joint();
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on cylindrical joint should return Some");
        assert_eq!(cols.len(), 2, "cylindrical has 2 DOFs — expected 2 columns");
        // Column 0: translation (prismatic-equivalent) — linear along +Z, zero angular.
        let c0 = cols[0].as_array();
        for (i, (&got, exp)) in c0.iter().zip([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "cylindrical col0 (translation)[{}]: expected {}, got {} (PRD §12 Q4: [0; axis])",
                i, exp, got
            );
        }
        // Column 1: rotation (revolute-equivalent) — angular about +Z, zero linear.
        let c1 = cols[1].as_array();
        for (i, (&got, exp)) in c1.iter().zip([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "cylindrical col1 (rotation)[{}]: expected {}, got {} (PRD §12 Q4: [axis; 0])",
                i, exp, got
            );
        }
    }

    /// Both cylindrical columns share the same unit-normalized direction.
    #[test]
    fn motion_subspace_columns_cylindrical_unnormalized_axis_normalizes_both_columns() {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let joint = eval_builtin("cylindrical", &[axis, length_range_0_to_1m(), angle_range_0_to_pi()]);
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on cylindrical joint should return Some");
        assert_eq!(cols.len(), 2, "cylindrical has 2 DOFs — expected 2 columns");
        let c0 = cols[0].as_array();
        let c1 = cols[1].as_array();
        // Column 0: translation — linear [0,0,1], angular [0,0,0].
        for (i, (&got, exp)) in c0.iter().zip([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]).enumerate() {
            assert!((got - exp).abs() < 1e-12, "col0[{}]: exp {}, got {}", i, exp, got);
        }
        // Column 1: rotation — angular [0,0,1], linear [0,0,0].
        for (i, (&got, exp)) in c1.iter().zip([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]).enumerate() {
            assert!((got - exp).abs() < 1e-12, "col1[{}]: exp {}, got {}", i, exp, got);
        }
    }

    // ── motion_subspace_columns: revolute ────────────────────────────────────

    /// PRD §5.1 pin: revolute column = `[unit_axis; 0]`.
    /// `motion_subspace_columns` on a revolute +Z joint returns a single
    /// column with angular = [0,0,1] and linear = [0,0,0].
    #[test]
    fn motion_subspace_columns_revolute_unit_axis_returns_axis_zero_column() {
        let joint = revolute_z_joint();
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on revolute joint should return Some");
        assert_eq!(cols.len(), 1, "revolute has 1 DOF — expected 1 column");
        let arr = cols[0].as_array();
        for (i, (&got, exp)) in arr.iter().zip([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "revolute +Z column[{}]: expected {}, got {} (PRD §5.1: [unit_axis; 0])",
                i, exp, got
            );
        }
    }

    /// Axis normalization: revolute with unnormalized axis `[0,0,2]` (magnitude 2)
    /// still returns column `[0,0,1, 0,0,0]`.
    #[test]
    fn motion_subspace_columns_revolute_unnormalized_axis_normalizes() {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let joint = eval_builtin("revolute", &[axis, angle_range_0_to_pi()]);
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on revolute joint should return Some");
        assert_eq!(cols.len(), 1, "revolute has 1 DOF — expected 1 column");
        let arr = cols[0].as_array();
        for (i, (&got, exp)) in arr.iter().zip([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "revolute axis-norm column[{}]: expected {}, got {}",
                i, exp, got
            );
        }
    }

    // ── motion_subspace_columns: prismatic ───────────────────────────────────

    /// PRD §5.1 pin: prismatic column = `[0; axis]`.
    /// `motion_subspace_columns` on a prismatic +X joint returns a single
    /// column with angular = [0,0,0] and linear = [1,0,0].
    #[test]
    fn motion_subspace_columns_prismatic_unit_axis_returns_zero_axis_column() {
        let joint = prismatic_x_joint();
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on prismatic joint should return Some");
        assert_eq!(cols.len(), 1, "prismatic has 1 DOF — expected 1 column");
        let arr = cols[0].as_array();
        for (i, (&got, exp)) in arr.iter().zip([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "prismatic +X column[{}]: expected {}, got {} (PRD §5.1: [0; axis])",
                i, exp, got
            );
        }
    }

    /// Axis normalization: prismatic with unnormalized axis `[2,0,0]` (magnitude 2)
    /// still returns column `[0,0,0, 1,0,0]`.
    #[test]
    fn motion_subspace_columns_prismatic_unnormalized_axis_normalizes() {
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let joint = eval_builtin("prismatic", &[axis, length_range_0_to_1m()]);
        let cols = super::motion_subspace_columns(&joint)
            .expect("motion_subspace_columns on prismatic joint should return Some");
        assert_eq!(cols.len(), 1, "prismatic has 1 DOF — expected 1 column");
        let arr = cols[0].as_array();
        for (i, (&got, exp)) in arr.iter().zip([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]).enumerate() {
            assert!(
                (got - exp).abs() < 1e-12,
                "prismatic axis-norm column[{}]: expected {}, got {}",
                i, exp, got
            );
        }
    }

    // ── motion_subspace_columns: fixed ────────────────────────────────────────

    /// `motion_subspace_columns` on a fixed joint returns `Some(vec![])` (0-DOF → 6×0
    /// motion-subspace).
    #[test]
    fn motion_subspace_columns_fixed_returns_empty_vec() {
        let fixed_joint = eval_builtin("fixed", &[]);
        let cols = super::motion_subspace_columns(&fixed_joint)
            .expect("motion_subspace_columns on fixed joint should return Some");
        assert!(
            cols.is_empty(),
            "fixed joint has 0 DOFs — motion-subspace should be 6×0 (empty Vec), got len={}",
            cols.len()
        );
    }
}
