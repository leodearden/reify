//! Batch-sweep stdlib for forward kinematics (task 2529).
//!
//! Implements the v0.1 `dim()` / `sweep()` / `sweep_grid()` builtins per
//! `docs/prds/kinematic-constraints.md` task 5 and `docs/reify-stdlib-reference.md` §13.4.
//!
//! Both `sweep` and `sweep_grid` delegate to the existing `snapshot()` builtin
//! (task 2535) — they construct interpolated bindings lists from per-joint
//! ranges and steps, then call `eval_builtin("snapshot", ...)` once per
//! result element.  Joints absent from the bindings list automatically fall
//! back to range midpoint via `snapshot()`'s existing fallback chain.
//!
//! Surface:
//!   - `dim(joint, range, steps)`             → SweepDim Map
//!   - `sweep(m, joint, range, steps)`        → List<Snapshot>
//!   - `sweep_grid(m, dims_list)`             → List<Snapshot>

use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

use crate::eval_builtin;

/// Evaluate a sweep stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_sweep(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "dim" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE constructing the SweepDim Map):
            //   args.len() == 3                        → arity guard
            //   is_driving_joint(args[0])              → joint kind
            //                                            (coupling/fixed/
            //                                            world/non-Map all
            //                                            rejected per §13.4)
            //   args[1] is Value::Range with both
            //     lower/upper bounds present, both
            //     SI-finite, and dimension == joint
            //     kind's expected (LENGTH for
            //     prismatic, ANGLE for revolute)        → range guard
            //   args[2] is Value::Int(n) with n >= 0   → steps guard
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            let expected_dim = match driving_joint_kind(&args[0]) {
                Some(d) => d,
                None => return Some(Value::Undef),
            };
            if validate_range_with_dimension(&args[1], expected_dim).is_none() {
                return Some(Value::Undef);
            }
            match &args[2] {
                Value::Int(n) if *n >= 0 => {}
                _ => return Some(Value::Undef),
            }
            make_sweep_dim(args[0].clone(), args[1].clone(), args[2].clone())
        }
        "sweep" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE any `eval_builtin("snapshot", ...)`
            // delegation; mirrors snapshot.rs's snapshot arm validation):
            //   args.len() == 4                                → arity guard
            //   args[0] is Map with kind="mechanism"           → mechanism guard
            //   is_driving_joint(args[1])                      → joint kind guard
            //   args[2] is Value::Range matching the joint's   → range guard
            //     dimension and SI-finite
            //   args[3] is Value::Int(n) with n >= 0           → steps guard
            //
            // After validation, `sweep` is just the 1-D specialisation of
            // `sweep_grid`: build a single-element `metas` vector and
            // delegate to `build_snapshot_list`, which centralises the
            // steps==0 → empty / steps==1 → lower / steps>=2 → linear-
            // interpolation cascade and the eval_builtin error
            // propagation.
            if args.len() != 4 {
                return Some(Value::Undef);
            }
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }
            // Errored-mechanism short-circuit (mirrors snapshot.rs's
            // snapshot arm and body_id_of in mechanism.rs). Layered
            // AFTER the kind-guard so an unrelated error-bearing Map
            // without kind="mechanism" still hits the kind-mismatch
            // guard, not this short-circuit.
            if mech_map.contains_key(&Value::String("error".to_string())) {
                return Some(Value::Undef);
            }
            let expected_dim = match driving_joint_kind(&args[1]) {
                Some(d) => d,
                None => return Some(Value::Undef),
            };
            if validate_range_with_dimension(&args[2], expected_dim).is_none() {
                return Some(Value::Undef);
            }
            let steps = match &args[3] {
                Value::Int(n) if *n >= 0 => *n,
                _ => return Some(Value::Undef),
            };
            // Range bounds in SI units; validation above guarantees
            // both are present and finite.
            let (lo_si, up_si) = match &args[2] {
                Value::Range {
                    lower: Some(lo),
                    upper: Some(up),
                    ..
                } => match (lo.as_f64(), up.as_f64()) {
                    (Some(a), Some(b)) => (a, b),
                    _ => return Some(Value::Undef),
                },
                _ => return Some(Value::Undef),
            };
            let metas = vec![DimMeta {
                joint: args[1].clone(),
                lo_si,
                up_si,
                steps,
            }];
            build_snapshot_list(&args[0], &metas)
        }
        "sweep_grid" => {
            // Validation surface (each guard short-circuits to
            // Value::Undef BEFORE any iteration / delegation):
            //   args.len() == 2                                → arity guard
            //   args[0] is Map with kind="mechanism"           → mechanism guard
            //   no `error` key on the mechanism Map            → errored-mech guard
            //   args[1] is Value::List                         → dims-list shape
            //   each entry is Value::Map with kind="sweep_dim"
            //     and present `joint`/`range`/`steps` fields    → per-entry shape
            // After validation, cross-product iteration is delegated to
            // `build_snapshot_list` (shared with `sweep` arm).
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let mech_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            if mech_map.get(&Value::String("kind".to_string()))
                != Some(&Value::String("mechanism".to_string()))
            {
                return Some(Value::Undef);
            }
            // Errored-mechanism short-circuit (mirrors `sweep` and
            // `snapshot`). Layered AFTER the kind-guard so an
            // unrelated error-bearing Map without kind="mechanism"
            // still hits the kind-mismatch guard above.
            if mech_map.contains_key(&Value::String("error".to_string())) {
                return Some(Value::Undef);
            }
            let dims = match &args[1] {
                Value::List(d) => d,
                _ => return Some(Value::Undef),
            };
            // Per-entry shape + content validation. Each entry must be
            // a SweepDim Map with the canonical four-field layout
            // (kind="sweep_dim", joint, range, steps), and the inner
            // joint / range / steps must satisfy the same constraints
            // that `dim()` enforces — so a hand-constructed SweepDim
            // can't bypass dim()'s validation. Whole-call rejection on
            // any malformed entry, mirroring snapshot.rs's bindings
            // validation.
            let mut metas: Vec<DimMeta> = Vec::with_capacity(dims.len());
            for entry in dims {
                let emap = match entry {
                    Value::Map(m) => m,
                    _ => return Some(Value::Undef),
                };
                if emap.get(&Value::String("kind".to_string()))
                    != Some(&Value::String("sweep_dim".to_string()))
                {
                    return Some(Value::Undef);
                }
                let joint = match emap.get(&Value::String("joint".to_string())) {
                    Some(j) => j.clone(),
                    None => return Some(Value::Undef),
                };
                let range = match emap.get(&Value::String("range".to_string())) {
                    Some(r) => r,
                    None => return Some(Value::Undef),
                };
                let steps = match emap.get(&Value::String("steps".to_string())) {
                    Some(Value::Int(n)) if *n >= 0 => *n,
                    _ => return Some(Value::Undef),
                };
                let expected_dim = match driving_joint_kind(&joint) {
                    Some(d) => d,
                    None => return Some(Value::Undef),
                };
                if validate_range_with_dimension(range, expected_dim).is_none() {
                    return Some(Value::Undef);
                }
                let (lo_si, up_si) = match range {
                    Value::Range {
                        lower: Some(lo),
                        upper: Some(up),
                        ..
                    } => match (lo.as_f64(), up.as_f64()) {
                        (Some(a), Some(b)) => (a, b),
                        _ => return Some(Value::Undef),
                    },
                    _ => return Some(Value::Undef),
                };
                metas.push(DimMeta {
                    joint,
                    lo_si,
                    up_si,
                    steps,
                });
            }
            build_snapshot_list(&args[0], &metas)
        }
        _ => return None,
    })
}

/// Map a driving joint Value to its expected motion-variable dimension.
///
/// Returns:
/// - `Some(LENGTH)` for prismatic joints
/// - `Some(ANGLE)`  for revolute  joints
/// - `None` for any other shape: coupling joints, fixed joints, the
///   world sentinel, non-joint Maps, and non-Map values.
///
/// Couplings are rejected per §13.4 ("Couplings cannot appear in sweep
/// dims — their motion is derived from the driving joint that is
/// already being swept").  Fixed joints (0-DOF) have no motion variable
/// to sweep over.  The combined predicate-plus-dimension is the
/// uniform driving-joint test for both `dim` and `sweep`.
fn driving_joint_kind(v: &Value) -> Option<DimensionVector> {
    let map = match v {
        Value::Map(m) => m,
        _ => return None,
    };
    match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => match s.as_str() {
            "prismatic" => Some(DimensionVector::LENGTH),
            "revolute" => Some(DimensionVector::ANGLE),
            _ => None,
        },
        _ => None,
    }
}

/// Wrap an interpolated f64 (in SI units) back into a dimensioned
/// `Value` based on the driving joint's kind.
///
/// - `prismatic` → `Value::length(v_si)` (metres)
/// - `revolute`  → `Value::angle(v_si)`  (radians)
///
/// Returns `None` for any other shape (coupling, fixed, world, non-Map).
/// Couplings and fixed joints never reach this helper because
/// `driving_joint_kind` rejects them upstream — the `None` branch is
/// pure defense-in-depth.
///
/// Mirrors `wrap_midpoint_for_joint` in snapshot.rs (the `prismatic` and
/// `revolute` arms specifically); the coupling arm is omitted because
/// couplings are rejected at the `dim`/`sweep` boundary by
/// `driving_joint_kind`.  A future refactor could promote both helpers
/// to a shared `stdlib/helpers.rs` utility — see snapshot.rs:597-599.
fn wrap_value_for_driving_joint(joint: &Value, v_si: f64) -> Option<Value> {
    let map = match joint {
        Value::Map(m) => m,
        _ => return None,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    match kind {
        "prismatic" => Some(Value::length(v_si)),
        "revolute" => Some(Value::angle(v_si)),
        _ => None,
    }
}

/// Validate a range value: must be `Value::Range` with both bounds
/// present, both sharing `expected_dim`, both SI-finite. Mirrors
/// `validate_range` in joints.rs (which checks dimension only); this
/// variant additionally enforces finite SI values, since sweep
/// interpolation relies on `as_f64()` returning finite numbers.
fn validate_range_with_dimension(value: &Value, expected_dim: DimensionVector) -> Option<()> {
    match value {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => {
            if lo.dimension() != expected_dim || up.dimension() != expected_dim {
                return None;
            }
            let lo_si = lo.as_f64()?;
            let up_si = up.as_f64()?;
            if !lo_si.is_finite() || !up_si.is_finite() {
                return None;
            }
            Some(())
        }
        _ => None,
    }
}

/// Cross-product iteration metadata for a single sweep_grid dim.
///
/// Cached upfront so the per-tuple inner loop doesn't re-walk the
/// SweepDim Map's keys, re-validate the joint kind, or re-extract SI
/// bounds from `Value::Range` for every grid index.
struct DimMeta {
    joint: Value,
    lo_si: f64,
    up_si: f64,
    steps: i64,
}

/// Cross-product builder shared by `sweep` and `sweep_grid`.
///
/// Given a pre-validated list of `DimMeta`s, produce the lexicographic
/// (last-dim-varies-fastest) list of snapshots by iterating over every
/// tuple of per-dim indices, interpolating each dim's motion value via
/// [`interpolate_dim`], wrapping it back into a dimensioned binding, and
/// delegating to `eval_builtin("snapshot", ...)` once per tuple.
///
/// Centralises:
/// - **Total-count overflow guard** — `try_fold(checked_mul)` returns
///   `Value::Undef` if the product of step counts overflows `i64`,
///   preventing a wrapped-positive `total` from triggering an
///   unbounded `Vec::with_capacity` allocation.
/// - **Empty-result short-circuit** — `total == 0` (any dim with
///   steps=0 collapses total to 0) yields `Value::List(vec![])`.
/// - **Empty-grid all-midpoints path** — empty `metas` (the empty
///   product = 1) yields one snapshot built from an empty bindings
///   list, so every joint falls back to its range midpoint via
///   `snapshot()`'s existing fallback chain.
/// - **steps==1 single-sample / steps>=2 linear-interpolation
///   cascade** — both delegated to `interpolate_dim`, so the design
///   decision pinned by step-13 (n=1 → lower endpoint) lives in one
///   place.
/// - **eval_builtin error propagation** — any `Undef` from `bind` /
///   `snapshot` short-circuits the whole call to `Undef`, mirroring
///   `snapshot()`'s semantics from snapshot.rs.
fn build_snapshot_list(mechanism: &Value, metas: &[DimMeta]) -> Value {
    // Total snapshot count = product of all step counts. Use
    // `checked_mul` so an over-large grid (e.g. many dims, or
    // pathologically large `steps` per dim) falls back to Undef
    // rather than wrapping silently and then allocating wrong-sized
    // buffers from `Vec::with_capacity(total as usize)`.
    let total: i64 = match metas
        .iter()
        .try_fold(1i64, |acc, m| acc.checked_mul(m.steps))
    {
        Some(t) => t,
        None => return Value::Undef,
    };
    if total == 0 {
        return Value::List(vec![]);
    }
    // Per-dim strides for lexicographic-order index decomposition
    // — last dim varies fastest. For dims = [d0, d1, ..., dk]:
    //   stride[k]   = 1
    //   stride[k-1] = steps[k]
    //   stride[k-2] = steps[k-1] * steps[k]
    //   ...
    // Then per-tuple indices[d] = (idx / stride[d]) % steps[d].
    let n = metas.len();
    let mut strides: Vec<i64> = vec![1; n];
    for d in (0..n.saturating_sub(1)).rev() {
        strides[d] = strides[d + 1] * metas[d + 1].steps;
    }
    let mut snapshots: Vec<Value> = Vec::with_capacity(total as usize);
    for idx in 0..total {
        let mut bindings: Vec<Value> = Vec::with_capacity(n);
        for d in 0..n {
            let i_d = (idx / strides[d]) % metas[d].steps;
            let v_si = interpolate_dim(&metas[d], i_d);
            let wrapped = match wrap_value_for_driving_joint(&metas[d].joint, v_si) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let binding = eval_builtin("bind", &[metas[d].joint.clone(), wrapped]);
            if binding.is_undef() {
                return Value::Undef;
            }
            bindings.push(binding);
        }
        let snap = eval_builtin("snapshot", &[mechanism.clone(), Value::List(bindings)]);
        if snap.is_undef() {
            return Value::Undef;
        }
        snapshots.push(snap);
    }
    Value::List(snapshots)
}

/// Interpolate the `i`-th motion-variable value (in SI units) for a
/// single sweep_grid dim.
///
/// - `steps == 1`  → returns `lo_si` (matches the steps==1 branch in
///   `sweep`; the spec is silent for n=1, lower is the canonical
///   single-sample choice — design decision pinned in step-13).
/// - `steps >= 2`  → linearly interpolated:
///   `lo + (i / (steps - 1)) * (up - lo)`.
///
/// Caller must guarantee `i ∈ 0..steps`.  `steps == 0` is unreachable
/// because the cross-product loop short-circuits when the total
/// product is 0 (any dim with steps=0 collapses total to 0).
fn interpolate_dim(meta: &DimMeta, i: i64) -> f64 {
    if meta.steps == 1 {
        meta.lo_si
    } else {
        let t = (i as f64) / ((meta.steps - 1) as f64);
        meta.lo_si + t * (meta.up_si - meta.lo_si)
    }
}

/// Build a SweepDim `Value::Map` with the standard four-key layout:
/// `kind`, `joint`, `range`, `steps` (alphabetical, matching `BTreeMap`
/// iteration). Mirrors `make_binding` in snapshot.rs and `make_joint` in
/// joints.rs — the kind-discriminated Map convention used across the
/// stdlib value types.
fn make_sweep_dim(joint: Value, range: Value, steps: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("kind".to_string()),
        Value::String("sweep_dim".to_string()),
    );
    m.insert(Value::String("joint".to_string()), joint);
    m.insert(Value::String("range".to_string()), range);
    m.insert(Value::String("steps".to_string()), steps);
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::Value;

    // ── Joint / range fixtures (mirror the per-module duplication
    //    convention noted in snapshot.rs:597-599). ────────────────────────

    fn axis_x_unit() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    fn axis_z_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    fn length_range_0_to_1m() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn angle_range_0_to_pi() -> Value {
        Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    // ── dim(joint, range, steps): happy path ─────────────────────────────

    /// `dim(joint, range, steps)` returns a `Value::Map` with shape
    /// `{kind="sweep_dim", joint=<input joint>, range=<input range>, steps=<input steps>}`.
    /// Pins the SweepDim shape so subsequent `sweep_grid` steps can rely on
    /// these four canonical fields existing.
    #[test]
    fn dim_returns_sweep_dim_map_with_kind_joint_range_steps() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        let n = Value::Int(11);
        let result = eval_builtin("dim", &[j.clone(), r.clone(), n.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map sweep_dim record, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("sweep_dim".to_string())),
            "kind field should be 'sweep_dim'"
        );
        assert_eq!(
            map.get(&Value::String("joint".to_string())),
            Some(&j),
            "joint field should be the input joint verbatim"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&r),
            "range field should be the input range verbatim"
        );
        assert_eq!(
            map.get(&Value::String("steps".to_string())),
            Some(&n),
            "steps field should be the input Int verbatim"
        );
    }

    // ── dim() input validation: full surface returns Undef ────────────────
    //
    // Validation allow-list (matches `eval_sweep::dim` arm — wired in step-4):
    //   args.len() == 3                        → arity guard
    //   is_driving_joint(args[0])              → joint kind guard (only
    //                                            prismatic/revolute; rejects
    //                                            coupling, fixed, world,
    //                                            non-Map). Couplings are
    //                                            rejected per §13.4 — their
    //                                            motion is derived from the
    //                                            driving joint already swept.
    //                                            Fixed joints have no motion
    //                                            variable to sweep.
    //   args[1] is Value::Range with both
    //     lower/upper bounds present and
    //     dimension == joint kind's expected   → range/dimension guard
    //   args[2] is Value::Int(n) with n >= 0   → steps guard
    // Any guard failure returns `Value::Undef` BEFORE Map construction.

    /// `dim()` with an arity outside {3} returns Undef.
    #[test]
    fn dim_wrong_arity_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // 0, 1, 2, 4 args
        assert!(eval_builtin("dim", &[]).is_undef());
        assert!(eval_builtin("dim", std::slice::from_ref(&j)).is_undef());
        assert!(eval_builtin("dim", &[j.clone(), r.clone()]).is_undef());
        assert!(eval_builtin("dim", &[j, r, n.clone(), n]).is_undef());
    }

    /// `dim(non_driving_joint, range, steps)` returns Undef when args[0]
    /// is not a prismatic/revolute joint. Covers Real, String, world
    /// sentinel, fixed joint, and coupling joint — couplings are
    /// rejected per §13.4 ("Couplings cannot appear in sweep dims —
    /// their motion is derived from the driving joint that is already
    /// being swept"). Fixed joints have no motion variable.
    #[test]
    fn dim_non_driving_joint_returns_undef() {
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // Real (not a Map at all)
        assert!(eval_builtin("dim", &[Value::Real(1.0), r.clone(), n.clone()]).is_undef());

        // String (not a Map at all)
        assert!(
            eval_builtin("dim", &[Value::String("not a joint".to_string()), r.clone(), n.clone()])
                .is_undef()
        );

        // world sentinel — Map with kind="world", not a joint
        assert!(eval_builtin("dim", &[eval_builtin("world", &[]), r.clone(), n.clone()]).is_undef());

        // fixed joint — kind="fixed" has no motion variable
        let fixed = eval_builtin("fixed", &[]);
        assert!(eval_builtin("dim", &[fixed, r.clone(), n.clone()]).is_undef());

        // coupling joint — derived from a driving joint, can't be swept
        let parent = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        assert!(eval_builtin("dim", &[coupling, r, n]).is_undef());
    }

    /// `dim(joint, non_range, steps)` returns Undef when args[1] is not a
    /// Value::Range with both bounds present.
    #[test]
    fn dim_non_range_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let n = Value::Int(11);

        // Real
        assert!(eval_builtin("dim", &[j.clone(), Value::Real(1.0), n.clone()]).is_undef());

        // String
        assert!(
            eval_builtin("dim", &[j.clone(), Value::String("nope".to_string()), n.clone()])
                .is_undef()
        );

        // Map (not a Range)
        let mech = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("dim", &[j, mech, n]).is_undef());
    }

    /// `dim(joint, range, non_int_steps)` returns Undef when args[2] is
    /// not a Value::Int.
    #[test]
    fn dim_non_int_steps_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();

        // Real
        assert!(eval_builtin("dim", &[j.clone(), r.clone(), Value::Real(11.0)]).is_undef());

        // String
        assert!(
            eval_builtin("dim", &[j, r, Value::String("eleven".to_string())]).is_undef()
        );
    }

    /// `dim(joint, range, Int(-1))` returns Undef — negative step counts
    /// are rejected. (Int(0) is valid: `sweep` returns the empty list.)
    #[test]
    fn dim_negative_steps_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        assert!(eval_builtin("dim", &[j, r, Value::Int(-1)]).is_undef());
    }

    /// `dim(joint, range, steps)` returns Undef when the range's
    /// dimension does not match the joint kind:
    /// - prismatic joint requires LENGTH range
    /// - revolute  joint requires ANGLE  range
    /// Mirrors `validate_range`'s pattern from joints.rs.
    #[test]
    fn dim_dimension_mismatch_returns_undef() {
        // Prismatic joint + angle range → Undef.
        let j_pris = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        assert!(
            eval_builtin("dim", &[j_pris, angle_range_0_to_pi(), Value::Int(11)]).is_undef()
        );

        // Revolute joint + length range → Undef.
        let j_rev = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        assert!(
            eval_builtin("dim", &[j_rev, length_range_0_to_1m(), Value::Int(11)]).is_undef()
        );
    }

    // ── sweep(m, joint, range, steps): degenerate inputs ──────────────────

    /// Helper: build a 1-body mechanism with a prismatic +X joint
    /// (range 0..1m), parent=world, identity pose. Returns the
    /// mechanism Map and the joint so tests can use both.
    fn make_one_body_prismatic_mechanism() -> (Value, Value) {
        let m0 = eval_builtin("mechanism", &[]);
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let solid = Value::String("a".to_string());
        let m1 = eval_builtin("body", &[m0, solid, j.clone()]);
        (m1, j)
    }

    /// `sweep(m, j, range, Int(0))` returns the empty `Value::List`.
    /// Pins the degenerate-input semantic so callers can pipe
    /// `sweep(...).map(...)` without a defensive `if steps > 0`
    /// guard.
    #[test]
    fn sweep_steps_zero_returns_empty_list() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let result = eval_builtin(
            "sweep",
            &[m, j, length_range_0_to_1m(), Value::Int(0)],
        );
        assert_eq!(
            result,
            Value::List(vec![]),
            "sweep with steps==0 should yield an empty List"
        );
    }

    // ── sweep(): headline acceptance test (PRD task 5) ────────────────────

    /// Decompose a `Value::Transform` into its translation `[tx, ty, tz]`
    /// (SI metres). Mirrors the pattern in
    /// `snapshot.rs::tests::decompose_transform_for_assert`.
    fn translation_of_transform(t: &Value) -> [f64; 3] {
        let trans = match t {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Value::Transform, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(c) if c.len() == 3 => c,
            other => panic!("expected Value::Vector len=3, got {:?}", other),
        };
        let read = |v: &Value| -> f64 {
            match v {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                other => panic!("expected numeric component, got {:?}", other),
            }
        };
        [read(&comps[0]), read(&comps[1]), read(&comps[2])]
    }

    /// Extract a snapshot's body 0 world translation (assumes id=0 sits
    /// at index 0 of the bodies list).
    fn body_0_translation(snapshot: &Value) -> [f64; 3] {
        let smap = match snapshot {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        let body = match &bodies[0] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body
            .get(&Value::String("world_transform".to_string()))
            .expect("body record must carry world_transform");
        translation_of_transform(wt)
    }

    /// Headline acceptance test (PRD task 5): 11 evenly-spaced
    /// snapshots over a 0..1m prismatic +X joint. The i-th snapshot's
    /// body sits at world translation (i/10, 0, 0). Endpoints match
    /// `snapshot(m, [bind(j, range.lower)])` and `snapshot(m, [bind(j,
    /// range.upper)])` per spec §13.4.
    #[test]
    fn sweep_eleven_steps_evenly_spaced_first_last_match_snapshot() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();
        let result = eval_builtin(
            "sweep",
            &[m.clone(), j.clone(), r.clone(), Value::Int(11)],
        );
        let list = match result {
            Value::List(l) => l,
            other => panic!("expected Value::List, got {:?}", other),
        };
        assert_eq!(list.len(), 11, "sweep with steps=11 should produce 11 snapshots");

        // Each snapshot is a Snapshot Map with kind="snapshot"; body 0
        // sits at world translation (i/10, 0, 0).
        for i in 0..=10 {
            let snap = &list[i];
            let smap = match snap {
                Value::Map(m) => m,
                other => panic!("snap[{}] should be a Map, got {:?}", i, other),
            };
            assert_eq!(
                smap.get(&Value::String("kind".to_string())),
                Some(&Value::String("snapshot".to_string())),
                "snap[{}].kind should be 'snapshot'",
                i
            );
            let [tx, ty, tz] = body_0_translation(snap);
            let expected_x = (i as f64) / 10.0;
            assert!(
                (tx - expected_x).abs() < 1e-12,
                "snap[{}] body translation x should be {}, got {}",
                i,
                expected_x,
                tx
            );
            assert!(ty.abs() < 1e-12, "snap[{}] body translation y should be 0, got {}", i, ty);
            assert!(tz.abs() < 1e-12, "snap[{}] body translation z should be 0, got {}", i, tz);
        }

        // First snapshot equals snapshot(m, [bind(j, length(0.0))]).
        let bind_lo = eval_builtin("bind", &[j.clone(), Value::length(0.0)]);
        let snap_lo = eval_builtin(
            "snapshot",
            &[m.clone(), Value::List(vec![bind_lo])],
        );
        assert_eq!(
            list[0], snap_lo,
            "first sweep element should equal snapshot(m, [bind(j, length(0))])"
        );

        // Last snapshot equals snapshot(m, [bind(j, length(1.0))]).
        let bind_hi = eval_builtin("bind", &[j.clone(), Value::length(1.0)]);
        let snap_hi = eval_builtin("snapshot", &[m, Value::List(vec![bind_hi])]);
        assert_eq!(
            list[10], snap_hi,
            "last sweep element should equal snapshot(m, [bind(j, length(1))])"
        );
    }

    // ── Errored-mechanism short-circuit ───────────────────────────────────

    fn axis_y_unit() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    /// `sweep()` on an errored Mechanism returns `Value::Undef` — not a
    /// partial List of pre-error snapshots. Mirrors
    /// `snapshot_on_errored_mechanism_returns_undef` in snapshot.rs:
    /// chained sweeps must reckon with the upstream error before
    /// getting a plausible-looking List back.
    #[test]
    fn sweep_on_errored_mechanism_returns_undef() {
        // Build an errored mechanism via parent-conflict — same recipe as
        // snapshot.rs::snapshot_on_errored_mechanism_returns_undef.
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b]);
        // Sanity: the setup actually produced an errored mechanism.
        match &errored {
            Value::Map(m) => assert_eq!(
                m.get(&Value::String("error".to_string())),
                Some(&Value::String("closed_chain".to_string())),
                "setup precondition: errored mechanism has error='closed_chain'"
            ),
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        // sweep() on the errored mechanism must yield Undef even
        // though the pre-error bodies list contains a fully-formed
        // body record.
        assert!(
            eval_builtin(
                "sweep",
                &[errored, j_x, angle_range_0_to_pi(), Value::Int(11)]
            )
            .is_undef(),
            "sweep() on errored mechanism must yield Undef"
        );
    }

    // ── sweep() input validation: full surface returns Undef ──────────────
    //
    // Validation allow-list (matches `eval_sweep::sweep` arm):
    //   args.len() == 4                                → arity guard
    //   args[0] is Map with kind="mechanism"           → mechanism guard
    //   no `error` key on the mechanism Map            → errored-mech guard
    //   is_driving_joint(args[1])                      → joint kind guard
    //   args[2] is Value::Range with both bounds and
    //     dimension matching the joint kind            → range/dim guard
    //   args[3] is Value::Int(n) with n >= 0           → steps guard
    // Any guard failure returns `Value::Undef`.

    /// `sweep()` with an arity outside {4} returns Undef.
    #[test]
    fn sweep_wrong_arity_returns_undef() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // 0, 1, 2, 3, 5 args
        assert!(eval_builtin("sweep", &[]).is_undef());
        assert!(eval_builtin("sweep", std::slice::from_ref(&m)).is_undef());
        assert!(eval_builtin("sweep", &[m.clone(), j.clone()]).is_undef());
        assert!(eval_builtin("sweep", &[m.clone(), j.clone(), r.clone()]).is_undef());
        assert!(
            eval_builtin("sweep", &[m, j, r, n.clone(), n]).is_undef()
        );
    }

    /// `sweep(non_mechanism, ...)` returns Undef when args[0] is not a
    /// Mechanism Map.
    #[test]
    fn sweep_non_mechanism_returns_undef() {
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // Real
        assert!(
            eval_builtin("sweep", &[Value::Real(1.0), j.clone(), r.clone(), n.clone()])
                .is_undef()
        );

        // world sentinel — Map with kind="world"
        assert!(
            eval_builtin(
                "sweep",
                &[eval_builtin("world", &[]), j.clone(), r.clone(), n.clone()]
            )
            .is_undef()
        );

        // Map with a different kind discriminator
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("other".to_string()),
        );
        assert!(eval_builtin("sweep", &[Value::Map(m), j, r, n]).is_undef());
    }

    /// `sweep(m, non_driving_joint, range, steps)` returns Undef.
    #[test]
    fn sweep_non_driving_joint_returns_undef() {
        let (m, _) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();
        let n = Value::Int(11);

        // Real
        assert!(
            eval_builtin("sweep", &[m.clone(), Value::Real(1.0), r.clone(), n.clone()])
                .is_undef()
        );

        // world sentinel
        assert!(
            eval_builtin(
                "sweep",
                &[m.clone(), eval_builtin("world", &[]), r.clone(), n.clone()]
            )
            .is_undef()
        );

        // fixed joint
        let fixed = eval_builtin("fixed", &[]);
        assert!(eval_builtin("sweep", &[m.clone(), fixed, r.clone(), n.clone()]).is_undef());

        // coupling joint
        let parent = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let coupling = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        assert!(eval_builtin("sweep", &[m, coupling, r, n]).is_undef());
    }

    /// `sweep(m, joint, non_range, steps)` returns Undef.
    #[test]
    fn sweep_non_range_returns_undef() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let n = Value::Int(11);

        // Real
        assert!(
            eval_builtin("sweep", &[m.clone(), j.clone(), Value::Real(1.0), n.clone()])
                .is_undef()
        );

        // Map (not a Range)
        let other = eval_builtin("mechanism", &[]);
        assert!(eval_builtin("sweep", &[m.clone(), j.clone(), other, n.clone()]).is_undef());

        // List
        assert!(
            eval_builtin("sweep", &[m, j, Value::List(vec![]), n]).is_undef()
        );
    }

    /// `sweep(m, joint, range, non_int_steps)` returns Undef.
    #[test]
    fn sweep_non_int_steps_returns_undef() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();

        // Real
        assert!(
            eval_builtin("sweep", &[m.clone(), j.clone(), r.clone(), Value::Real(11.0)])
                .is_undef()
        );

        // String
        assert!(
            eval_builtin("sweep", &[m, j, r, Value::String("eleven".to_string())])
                .is_undef()
        );
    }

    /// `sweep(m, joint, range, Int(<0))` returns Undef.
    #[test]
    fn sweep_negative_steps_returns_undef() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();

        assert!(
            eval_builtin("sweep", &[m.clone(), j.clone(), r.clone(), Value::Int(-1)])
                .is_undef()
        );
        assert!(eval_builtin("sweep", &[m, j, r, Value::Int(-3)]).is_undef());
    }

    /// `sweep(m, joint, range, steps)` returns Undef when range
    /// dimension does not match joint kind.
    #[test]
    fn sweep_dimension_mismatch_returns_undef() {
        let (m, _) = make_one_body_prismatic_mechanism();
        let n = Value::Int(11);

        // Prismatic joint + angle range → Undef.
        let j_pris = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        assert!(
            eval_builtin(
                "sweep",
                &[m.clone(), j_pris, angle_range_0_to_pi(), n.clone()]
            )
            .is_undef()
        );

        // Revolute joint + length range → Undef.
        let j_rev = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        // Need a mechanism whose body uses j_rev for the parent-chain
        // walk to even start; build a dedicated one so the mismatch
        // surfaces from the sweep arm's range guard, not from snapshot.
        let m0 = eval_builtin("mechanism", &[]);
        let m_rev = eval_builtin(
            "body",
            &[m0, Value::String("a".to_string()), j_rev.clone()],
        );
        assert!(
            eval_builtin("sweep", &[m_rev, j_rev, length_range_0_to_1m(), n]).is_undef()
        );
    }

    // ── sweep() steps == 1 single-snapshot-at-lower semantic ─────────────

    /// `sweep(m, j, range, Int(1))` returns a single snapshot at
    /// `range.lower`. Pins the design choice (the spec is silent for
    /// n=1; lower is consistent with how a 0-step→1-step extension
    /// would naturally extend the sequence — a single sample of any
    /// sequence canonically takes the start).
    #[test]
    fn sweep_steps_one_returns_single_snapshot_at_lower() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let r = length_range_0_to_1m();
        let result = eval_builtin(
            "sweep",
            &[m.clone(), j.clone(), r, Value::Int(1)],
        );
        let list = match result {
            Value::List(l) => l,
            other => panic!("expected Value::List, got {:?}", other),
        };
        assert_eq!(list.len(), 1, "sweep with steps=1 should yield exactly 1 snapshot");

        // Single element equals snapshot(m, [bind(j, length(0.0))]).
        let bind_lo = eval_builtin("bind", &[j, Value::length(0.0)]);
        let snap_lo = eval_builtin("snapshot", &[m, Value::List(vec![bind_lo])]);
        assert_eq!(
            list[0], snap_lo,
            "sweep with steps=1 single element should equal snapshot(m, [bind(j, lower)])"
        );
    }

    /// `sweep(m, joint, unbounded_range, steps)` returns Undef when
    /// either range bound is `None`.
    #[test]
    fn sweep_unbounded_range_returns_undef() {
        let (m, j) = make_one_body_prismatic_mechanism();
        let n = Value::Int(11);

        // No lower bound
        let no_lower = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::length(1.0))),
            lower_inclusive: false,
            upper_inclusive: true,
        };
        assert!(
            eval_builtin("sweep", &[m.clone(), j.clone(), no_lower, n.clone()]).is_undef()
        );

        // No upper bound
        let no_upper = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        assert!(eval_builtin("sweep", &[m, j, no_upper, n]).is_undef());
    }

    // ── sweep_grid(m, []): empty-dims semantic ────────────────────────────

    /// Extract the i-th body's world translation from a snapshot.
    fn body_n_translation(snapshot: &Value, n: usize) -> [f64; 3] {
        let smap = match snapshot {
            Value::Map(m) => m,
            other => panic!("expected Snapshot Map, got {:?}", other),
        };
        let bodies = match smap.get(&Value::String("bodies".to_string())) {
            Some(Value::List(b)) => b,
            other => panic!("expected snapshot bodies List, got {:?}", other),
        };
        let body = match &bodies[n] {
            Value::Map(b) => b,
            other => panic!("expected snapshot body record Map, got {:?}", other),
        };
        let wt = body
            .get(&Value::String("world_transform".to_string()))
            .expect("body record must carry world_transform");
        translation_of_transform(wt)
    }

    /// `sweep_grid(m, [])` returns a `Value::List` containing a single
    /// snapshot — the all-midpoints snapshot — because the product of an
    /// empty set of dim cardinalities is 1, and the implicit binding
    /// list is empty so every joint falls back to its range midpoint via
    /// `snapshot()`'s existing fallback. Pins the empty-grid semantic
    /// per design notes in plan.json.
    #[test]
    fn sweep_grid_empty_dims_returns_single_all_midpoint_snapshot() {
        let (m, _j) = make_one_body_prismatic_mechanism();
        let result = eval_builtin("sweep_grid", &[m, Value::List(vec![])]);
        let list = match result {
            Value::List(l) => l,
            other => panic!("expected Value::List, got {:?}", other),
        };
        assert_eq!(
            list.len(),
            1,
            "sweep_grid(m, []) should yield a single all-midpoints snapshot"
        );

        // The single snapshot has body 0 at the joint's range midpoint:
        // range 0..1m → midpoint 0.5m on +X axis.
        let [tx, ty, tz] = body_0_translation(&list[0]);
        assert!(
            (tx - 0.5).abs() < 1e-12,
            "single all-midpoints snapshot body translation x should be 0.5 (midpoint of 0..1m), got {}",
            tx
        );
        assert!(ty.abs() < 1e-12, "y should be 0, got {}", ty);
        assert!(tz.abs() < 1e-12, "z should be 0, got {}", tz);
    }

    // ── sweep_grid(): headline acceptance test (PRD task 5) ───────────────

    fn axis_y_unit_grid() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    /// Headline acceptance test (PRD task 5, grid case): 2×3 grid sweep
    /// over a 2-body chain. Body A at `j_x = prismatic(+X, 0..1m)`
    /// (parent=world), body B at `j_y = prismatic(+Y, 0..1m)`
    /// (parent=j_x). With dims = [dim(j_x, 2 steps), dim(j_y, 3 steps)],
    /// the result is 6 snapshots in lexicographic order — last dim
    /// varies fastest:
    ///   idx 0: (j_x=0,   j_y=0)   → body B at (0,   0,   0)
    ///   idx 1: (j_x=0,   j_y=0.5) → body B at (0,   0.5, 0)
    ///   idx 2: (j_x=0,   j_y=1)   → body B at (0,   1,   0)
    ///   idx 3: (j_x=1,   j_y=0)   → body B at (1,   0,   0)
    ///   idx 4: (j_x=1,   j_y=0.5) → body B at (1,   0.5, 0)
    ///   idx 5: (j_x=1,   j_y=1)   → body B at (1,   1,   0)
    /// First snapshot equals `snapshot(m, [bind(j_x, 0), bind(j_y, 0)])`,
    /// last equals `snapshot(m, [bind(j_x, 1m), bind(j_y, 1m)])`.
    /// Pins lexicographic ordering from §13.4.
    #[test]
    fn sweep_grid_two_by_three_lexicographic_order() {
        let j_x = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_y = eval_builtin("prismatic", &[axis_y_unit_grid(), length_range_0_to_1m()]);
        let solid_a = Value::String("a".to_string());
        let solid_b = Value::String("b".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        // Body A: at j_x, parent=world.
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone()]);
        // Body B: at j_y, parent=j_x.
        let m2 = eval_builtin(
            "body",
            &[m1, solid_b, j_y.clone(), j_x.clone()],
        );

        let dim_x = eval_builtin(
            "dim",
            &[j_x.clone(), length_range_0_to_1m(), Value::Int(2)],
        );
        let dim_y = eval_builtin(
            "dim",
            &[j_y.clone(), length_range_0_to_1m(), Value::Int(3)],
        );
        let result = eval_builtin(
            "sweep_grid",
            &[m2.clone(), Value::List(vec![dim_x, dim_y])],
        );
        let list = match result {
            Value::List(l) => l,
            other => panic!("expected Value::List, got {:?}", other),
        };
        assert_eq!(list.len(), 6, "sweep_grid 2×3 should produce 6 snapshots");

        // Expected (j_x, j_y) values in lexicographic order — last dim
        // varies fastest.
        let expected: [(f64, f64); 6] = [
            (0.0, 0.0),
            (0.0, 0.5),
            (0.0, 1.0),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
        ];
        for (i, (vx, vy)) in expected.iter().enumerate() {
            // Body B world translation = (j_x_value, j_y_value, 0).
            let [tx, ty, tz] = body_n_translation(&list[i], 1);
            assert!(
                (tx - vx).abs() < 1e-12,
                "snap[{}] body B tx should be {}, got {}",
                i,
                vx,
                tx
            );
            assert!(
                (ty - vy).abs() < 1e-12,
                "snap[{}] body B ty should be {}, got {}",
                i,
                vy,
                ty
            );
            assert!(
                tz.abs() < 1e-12,
                "snap[{}] body B tz should be 0, got {}",
                i,
                tz
            );
        }

        // First snapshot equals snapshot(m, [bind(j_x, 0), bind(j_y, 0)]).
        let bind_x_lo = eval_builtin("bind", &[j_x.clone(), Value::length(0.0)]);
        let bind_y_lo = eval_builtin("bind", &[j_y.clone(), Value::length(0.0)]);
        let snap_first = eval_builtin(
            "snapshot",
            &[m2.clone(), Value::List(vec![bind_x_lo, bind_y_lo])],
        );
        assert_eq!(
            list[0], snap_first,
            "first sweep_grid element should equal snapshot at (lo, lo)"
        );

        // Last snapshot equals snapshot(m, [bind(j_x, 1m), bind(j_y, 1m)]).
        let bind_x_hi = eval_builtin("bind", &[j_x, Value::length(1.0)]);
        let bind_y_hi = eval_builtin("bind", &[j_y, Value::length(1.0)]);
        let snap_last = eval_builtin(
            "snapshot",
            &[m2, Value::List(vec![bind_x_hi, bind_y_hi])],
        );
        assert_eq!(
            list[5], snap_last,
            "last sweep_grid element should equal snapshot at (hi, hi)"
        );
    }

    // ── sweep_grid() input validation: full surface returns Undef ────────
    //
    // Validation allow-list (matches `eval_sweep::sweep_grid` arm):
    //   args.len() == 2                             → arity guard
    //   args[0] is Map with kind="mechanism"        → mechanism guard
    //   no `error` key on the mechanism Map         → errored-mech guard
    //   args[1] is Value::List                      → dims-list shape
    //   each entry is Value::Map with kind="sweep_dim",
    //     present joint/range/steps fields, and the
    //     same joint-kind / range-dim / steps>=0
    //     constraints that dim() enforces           → per-entry shape
    // Any guard failure returns `Value::Undef`.

    /// `sweep_grid()` with an arity outside {2} returns Undef.
    #[test]
    fn sweep_grid_wrong_arity_returns_undef() {
        let (m, _) = make_one_body_prismatic_mechanism();
        let dims_empty = Value::List(vec![]);

        // 0, 1, 3 args
        assert!(eval_builtin("sweep_grid", &[]).is_undef());
        assert!(eval_builtin("sweep_grid", std::slice::from_ref(&m)).is_undef());
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m, dims_empty.clone(), dims_empty]
            )
            .is_undef()
        );
    }

    /// `sweep_grid(non_mechanism, dims)` returns Undef when args[0] is
    /// not a Mechanism Map.
    #[test]
    fn sweep_grid_non_mechanism_returns_undef() {
        let dims_empty = Value::List(vec![]);

        // Real
        assert!(
            eval_builtin("sweep_grid", &[Value::Real(1.0), dims_empty.clone()]).is_undef()
        );

        // world sentinel — Map with kind="world"
        assert!(
            eval_builtin(
                "sweep_grid",
                &[eval_builtin("world", &[]), dims_empty.clone()]
            )
            .is_undef()
        );

        // Map with a different kind discriminator
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("other".to_string()),
        );
        assert!(eval_builtin("sweep_grid", &[Value::Map(m), dims_empty]).is_undef());
    }

    /// `sweep_grid(m, non_list)` returns Undef when args[1] is not a
    /// `Value::List`.
    #[test]
    fn sweep_grid_non_list_dims_returns_undef() {
        let (m, _) = make_one_body_prismatic_mechanism();

        // Real
        assert!(
            eval_builtin("sweep_grid", &[m.clone(), Value::Real(1.0)]).is_undef()
        );

        // Map (a SweepDim by itself, not wrapped in a List)
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let single_dim = eval_builtin(
            "dim",
            &[j, length_range_0_to_1m(), Value::Int(2)],
        );
        assert!(eval_builtin("sweep_grid", &[m.clone(), single_dim]).is_undef());

        // Undef
        assert!(eval_builtin("sweep_grid", &[m, Value::Undef]).is_undef());
    }

    /// `sweep_grid(m, [bad_entry])` returns Undef when any dim entry is
    /// invalid — whole-call rejection. Covers: non-Map entries, Maps
    /// without kind="sweep_dim", and SweepDim Maps missing required
    /// fields.
    #[test]
    fn sweep_grid_invalid_dim_entry_returns_undef() {
        let (m, _) = make_one_body_prismatic_mechanism();

        // Real entry
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m.clone(), Value::List(vec![Value::Real(1.0)])]
            )
            .is_undef()
        );

        // Map without kind="sweep_dim" (a Mechanism, for instance)
        let mech_other = eval_builtin("mechanism", &[]);
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m.clone(), Value::List(vec![mech_other])]
            )
            .is_undef()
        );

        // Map with kind="sweep_dim" but missing the `joint` field
        let mut bad_map = std::collections::BTreeMap::new();
        bad_map.insert(
            Value::String("kind".to_string()),
            Value::String("sweep_dim".to_string()),
        );
        bad_map.insert(
            Value::String("range".to_string()),
            length_range_0_to_1m(),
        );
        bad_map.insert(Value::String("steps".to_string()), Value::Int(2));
        // No `joint` key
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m.clone(), Value::List(vec![Value::Map(bad_map)])]
            )
            .is_undef()
        );

        // Map with kind="sweep_dim" but missing the `range` field
        let mut bad_map2 = std::collections::BTreeMap::new();
        bad_map2.insert(
            Value::String("kind".to_string()),
            Value::String("sweep_dim".to_string()),
        );
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        bad_map2.insert(Value::String("joint".to_string()), j);
        bad_map2.insert(Value::String("steps".to_string()), Value::Int(2));
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m.clone(), Value::List(vec![Value::Map(bad_map2)])]
            )
            .is_undef()
        );

        // Map with kind="sweep_dim" but missing the `steps` field
        let mut bad_map3 = std::collections::BTreeMap::new();
        bad_map3.insert(
            Value::String("kind".to_string()),
            Value::String("sweep_dim".to_string()),
        );
        let j = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        bad_map3.insert(Value::String("joint".to_string()), j);
        bad_map3.insert(
            Value::String("range".to_string()),
            length_range_0_to_1m(),
        );
        assert!(
            eval_builtin(
                "sweep_grid",
                &[m, Value::List(vec![Value::Map(bad_map3)])]
            )
            .is_undef()
        );
    }

    /// `sweep_grid()` on an errored Mechanism returns `Value::Undef` —
    /// not a partial result. Mirrors the `sweep()` short-circuit.
    #[test]
    fn sweep_grid_on_errored_mechanism_returns_undef() {
        // Build an errored mechanism via parent-conflict (same recipe
        // as snapshot.rs::snapshot_on_errored_mechanism_returns_undef).
        let j_a = eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()]);
        let j_b = eval_builtin("prismatic", &[axis_y_unit(), length_range_0_to_1m()]);
        let j_x = eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()]);
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a]);
        let errored = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b]);
        match &errored {
            Value::Map(m) => assert_eq!(
                m.get(&Value::String("error".to_string())),
                Some(&Value::String("closed_chain".to_string())),
                "setup precondition: errored mechanism has error='closed_chain'"
            ),
            other => panic!("expected errored Mechanism Map, got {:?}", other),
        }

        let dim_x = eval_builtin(
            "dim",
            &[j_x, angle_range_0_to_pi(), Value::Int(3)],
        );
        assert!(
            eval_builtin(
                "sweep_grid",
                &[errored, Value::List(vec![dim_x])]
            )
            .is_undef(),
            "sweep_grid() on errored mechanism must yield Undef"
        );
    }
}
