//! FEA support (boundary-condition) constructors for the stdlib.
//!
//! Provides `FixedSupport`, `PinnedSupport`, `DisplacementSupport`, and
//! `RollerSupport` constructors.  Each returns a `Value::Map` with a `kind`
//! discriminator field, matching the loads/joints constructor pattern.
//!
//! Selector-target validation is delegated to
//! [`crate::helpers::validate_selector_target`] (see that helper's doc-comment
//! for the rationale on why opaque pass-through is currently the right policy).
//!
//! ## BC framework (v0.4 shell-aware contract — PRD T15)
//!
//! The following contracts apply at solver time (task T8 — shell BC
//! application).  At constructor time (this module) the selector target is an
//! opaque pass-through because topology selectors are not yet distinguishable
//! by entity kind; see `helpers::validate_selector_target` and PRD task 16.
//!
//! * **`FixedSupport` on a shell entity** — the solver (T8) automatically
//!   clamps all 6 DOFs: 3 translational + 3 rotational.  This is the
//!   shell-specific extension relative to tet behaviour.
//!
//! * **`PinnedSupport` on a shell entity** — explicit-pin opt-out from the
//!   rotational auto-clamp.  Constrains translational DOFs only (3-DOF pin),
//!   leaving rotational DOFs free.  Use `PinnedSupport` when the physical BC
//!   is a pin joint rather than a rigid wall attachment.
//!
//! * **`FixedSupport` on a tet entity** — unchanged semantics (3 translational
//!   DOFs constrained; tet elements carry no rotational DOFs).
//!
//! * **`PinnedSupport` on a tet entity** — semantically equivalent to
//!   `FixedSupport` on a tet (both constrain the same 3 translational DOFs).
//!   The solver (T8) is responsible for emitting a diagnostic warning when
//!   `PinnedSupport` is applied to a tet target, because the distinction is
//!   meaningful only for shell elements.
//!
//! Selector-target type-compatibility (shell-vs-tet detection) is a
//! solver-side concern at the time of writing.  See
//! `docs/prds/v0_4/structural-analysis-shells.md` § "BC framework" for the
//! full design rationale.

use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{
    make_kind_map, validate_dimensioned_vec3, validate_dimensionless_unit_axis_vec3,
    validate_selector_target,
};

/// Canonical set of support kinds recognized by this module.
///
/// Analogous to `loads::LOAD_KINDS`.  Consumed by `is_support_value` and
/// guarded by the `support_kinds_all_dispatched_by_eval_supports` partition
/// test to prevent silent drift between this list and `eval_supports`'s
/// dispatch arms.
///
/// Not yet referenced by any external caller — the FEA solver (PRD task 16)
/// will wire this up when it lands.
///
/// task 3540 (SIR-α wave-1, step-20): `"fixed_support"` retired here — the
/// `structure def FixedSupport : Support { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_supports`'s arm
/// is removed in lockstep so this list and its partition guard stay in sync.
///
/// task 3546 (SIR-β-sup wave-2, step-4): `"pinned_support"` retired here — the
/// `structure def PinnedSupport : Support { ... }` declaration in
/// `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
/// `CompiledExprKind::StructureInstanceCtor` lowering. `eval_supports`'s arm
/// is removed in lockstep so this list and its partition guard stay in sync.
#[allow(dead_code)]
pub(crate) const SUPPORT_KINDS: &[&str] = &[
    "displacement_support",
    "roller_support",
];

/// Returns `true` if `v` is a support `Value::Map` produced by this module —
/// i.e., a Map with a `kind` field whose value is one of `SUPPORT_KINDS`.
///
/// Analogous to `loads::is_load_value`.  Used by the FEA solver (PRD
/// task 16) once it lands; not yet called from any external module.
#[allow(dead_code)]
pub(crate) fn is_support_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => m
            .get(&Value::String("kind".to_string()))
            .and_then(|k| {
                if let Value::String(s) = k {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .is_some_and(|s| SUPPORT_KINDS.contains(&s)),
        _ => false,
    }
}

/// Evaluate a supports stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_supports(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // task 3540 (SIR-α wave-1, step-20): `FixedSupport` retired. The
        // `structure def FixedSupport : Support { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `FixedSupport(...)` evals to a `Value::StructureInstance`. Returning
        // `None` here makes `eval_builtin("FixedSupport", ...)` fall through
        // to `Value::Undef` (the unknown-name contract). The `target` field
        // shape is preserved by the structure-def per Q-SIR-4.
        // task 3546 (SIR-β-sup wave-2, step-4): `PinnedSupport` retired.
        // The `structure def PinnedSupport : Support { ... }` in
        // `crates/reify-compiler/stdlib/fea_multi_case.ri` takes over via the
        // `CompiledExprKind::StructureInstanceCtor` lowering; source-level
        // `PinnedSupport(...)` evals to a `Value::StructureInstance`. Returning
        // `None` here makes `eval_builtin("PinnedSupport", ...)` fall through
        // to `Value::Undef` (the unknown-name contract). The `target` field
        // shape is preserved by the structure-def per Q-SIR-4
        // (PRD §8 Phase 2, `docs/prds/v0_3/structure-instance-runtime.md`).
        "DisplacementSupport" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensioned_vec3(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            make_kind_map(
                "displacement_support",
                vec![
                    ("displacement", args[1].clone()),
                    ("target", args[0].clone()),
                ],
            )
        }
        "RollerSupport" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_selector_target(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_dimensionless_unit_axis_vec3(&args[1]).is_none() {
                return Some(Value::Undef);
            }
            make_kind_map(
                "roller_support",
                vec![("normal", args[1].clone()), ("target", args[0].clone())],
            )
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_core::DimensionVector;
    use reify_ir::Value;
    use std::collections::BTreeMap;

    /// Build a simple opaque selector stub (Map with kind="point_stub").
    fn point_selector_stub() -> Value {
        Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("point_stub".to_string()),
            );
            m
        })
    }

    // ── FixedSupport constructor: RETIRED (SIR-α wave-1, task 3540 step-20) ──
    //
    // The `FixedSupport` name-dispatched builtin was retired in favour of the
    // `structure def FixedSupport : Support { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `FixedSupport(...)` calls now lower to
    // `CompiledExprKind::StructureInstanceCtor` and eval to a
    // `Value::StructureInstance`.
    //
    // The Rust API contract — `eval_builtin("FixedSupport", ...)` returns
    // `Value::Undef` — is pinned by
    // `fixed_support_eval_builtin_returns_undef_post_retirement` above. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. The shell-aware 6-DOF
    // clamp contract documented in this module's header now applies to the
    // structure-def evaluation path (solver task T8).

    // ── PinnedSupport constructor: RETIRED (SIR-β-sup wave-2, task 3546 step-4) ──
    //
    // The `PinnedSupport` name-dispatched builtin was retired in favour of the
    // `structure def PinnedSupport : Support { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PinnedSupport(...)` calls now lower to
    // `CompiledExprKind::StructureInstanceCtor` and eval to a
    // `Value::StructureInstance`.
    //
    // The Rust API contract — `eval_builtin("PinnedSupport", ...)` returns
    // `Value::Undef` — is pinned by
    // `pinned_support_eval_builtin_returns_undef_post_retirement` above. The
    // former happy-path + per-argument validation tests are intentionally
    // removed: with the arm gone, every input collapses to `Undef`, so those
    // assertions no longer exercise distinct behaviour. The shell-aware 3-DOF
    // translational-only constraint documented in this module's header now
    // applies to the structure-def evaluation path (solver task T8).

    // ── DisplacementSupport constructor: happy path ───────────────────────────

    #[test]
    fn displacement_support_returns_map_with_correct_fields() {
        let target = point_selector_stub();
        let displacement = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);

        let result = eval_builtin(
            "DisplacementSupport",
            &[target.clone(), displacement.clone()],
        );

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("displacement_support".to_string())),
            "kind field should be 'displacement_support'"
        );
        assert_eq!(
            map.get(&Value::String("target".to_string())),
            Some(&target),
            "target field should round-trip the input"
        );
        assert_eq!(
            map.get(&Value::String("displacement".to_string())),
            Some(&displacement),
            "displacement field should round-trip the input"
        );
    }

    // ── DisplacementSupport constructor: failure modes ────────────────────────

    #[test]
    fn displacement_support_zero_args_returns_undef() {
        assert!(eval_builtin("DisplacementSupport", &[]).is_undef());
    }

    #[test]
    fn displacement_support_one_arg_returns_undef() {
        assert!(eval_builtin("DisplacementSupport", &[point_selector_stub()]).is_undef());
    }

    #[test]
    fn displacement_support_three_args_returns_undef() {
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(
            eval_builtin(
                "DisplacementSupport",
                &[point_selector_stub(), disp.clone(), disp]
            )
            .is_undef()
        );
    }

    #[test]
    fn displacement_support_invalid_target_returns_undef() {
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("DisplacementSupport", &[Value::Real(1.0), disp]).is_undef());
    }

    #[test]
    fn displacement_support_dimensionless_displacement_returns_undef() {
        // A raw dimensionless vector (Value::Real components) — not LENGTH-dimensioned.
        let dimensionless =
            Value::Vector(vec![Value::Real(0.001), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin(
                "DisplacementSupport",
                &[point_selector_stub(), dimensionless]
            )
            .is_undef()
        );
    }

    #[test]
    fn displacement_support_force_dimensioned_displacement_returns_undef() {
        let force_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::FORCE);
        assert!(
            eval_builtin("DisplacementSupport", &[point_selector_stub(), force_vec]).is_undef()
        );
    }

    #[test]
    fn displacement_support_two_component_displacement_returns_undef() {
        // Only 2 LENGTH-dimensioned scalar components — wrong arity.
        let two_comp = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(eval_builtin("DisplacementSupport", &[point_selector_stub(), two_comp]).is_undef());
    }

    #[test]
    fn displacement_support_nan_displacement_returns_undef() {
        let nan_disp = make_scalar_vec3([f64::NAN, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("DisplacementSupport", &[point_selector_stub(), nan_disp]).is_undef());
    }

    #[test]
    fn displacement_support_inf_displacement_returns_undef() {
        let inf_disp = make_scalar_vec3([f64::INFINITY, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("DisplacementSupport", &[point_selector_stub(), inf_disp]).is_undef());
    }

    #[test]
    fn displacement_support_non_vector_displacement_returns_undef() {
        assert!(
            eval_builtin(
                "DisplacementSupport",
                &[point_selector_stub(), Value::Real(0.001)]
            )
            .is_undef()
        );
    }

    // ── RollerSupport constructor: happy path ─────────────────────────────────

    #[test]
    fn roller_support_returns_map_with_correct_fields() {
        let target = point_selector_stub();
        // Non-unit input to exercise the un-normalized contract: a regression that
        // silently normalized the normal would shrink magnitude 2.5 → 1.0 and fail
        // the round-trip assertion below.
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.5)]);

        let result = eval_builtin("RollerSupport", &[target.clone(), normal.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("roller_support".to_string())),
            "kind field should be 'roller_support'"
        );
        assert_eq!(
            map.get(&Value::String("target".to_string())),
            Some(&target),
            "target field should round-trip the input"
        );
        assert_eq!(
            map.get(&Value::String("normal".to_string())),
            Some(&normal),
            "normal field should round-trip the raw input vector un-normalized"
        );
    }

    #[test]
    fn roller_support_preserves_raw_magnitude() {
        // Explicit guard against silent normalization of the stored normal.
        // RollerSupport's contract is to preserve magnitude at consume time;
        // this test fails immediately if a future change re-normalizes input.
        let target = point_selector_stub();
        let normal = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);

        let result = eval_builtin("RollerSupport", &[target, normal]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        let stored = match map.get(&Value::String("normal".to_string())) {
            Some(Value::Vector(v)) => v.clone(),
            other => panic!("expected Value::Vector for normal, got {:?}", other),
        };

        let mag_sq: f64 = stored
            .iter()
            .map(|c| match c {
                Value::Real(r) => r * r,
                other => panic!("expected Value::Real component, got {:?}", other),
            })
            .sum();
        let mag = mag_sq.sqrt();

        assert!(
            (mag - 5.0).abs() < 1e-12,
            "stored normal magnitude should be 5.0 (raw 3-4-0 input), got {}",
            mag
        );
        assert!(
            (mag - 1.0).abs() > 1e-9,
            "stored normal must NOT be unit-length — that would indicate silent normalization"
        );
    }

    // ── RollerSupport constructor: failure modes ──────────────────────────────

    #[test]
    fn roller_support_zero_args_returns_undef() {
        assert!(eval_builtin("RollerSupport", &[]).is_undef());
    }

    #[test]
    fn roller_support_one_arg_returns_undef() {
        assert!(eval_builtin("RollerSupport", &[point_selector_stub()]).is_undef());
    }

    #[test]
    fn roller_support_three_args_returns_undef() {
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin(
                "RollerSupport",
                &[point_selector_stub(), normal.clone(), normal]
            )
            .is_undef()
        );
    }

    #[test]
    fn roller_support_invalid_target_returns_undef() {
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("RollerSupport", &[Value::Real(1.0), normal]).is_undef());
    }

    #[test]
    fn roller_support_zero_magnitude_normal_returns_undef() {
        let zero_normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), zero_normal]).is_undef());
    }

    #[test]
    fn roller_support_nan_normal_returns_undef() {
        let nan_normal = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), nan_normal]).is_undef());
    }

    #[test]
    fn roller_support_inf_normal_returns_undef() {
        let inf_normal = Value::Vector(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(1.0),
        ]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), inf_normal]).is_undef());
    }

    #[test]
    fn roller_support_length_dimensioned_normal_returns_undef() {
        // LENGTH-dimensioned rather than dimensionless — should reject.
        let length_normal = make_scalar_vec3([0.0, 0.0, 1.0], DimensionVector::LENGTH);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), length_normal]).is_undef());
    }

    #[test]
    fn roller_support_two_component_normal_returns_undef() {
        let two_comp = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("RollerSupport", &[point_selector_stub(), two_comp]).is_undef());
    }

    #[test]
    fn roller_support_non_vector_normal_returns_undef() {
        assert!(
            eval_builtin("RollerSupport", &[point_selector_stub(), Value::Real(1.0)]).is_undef()
        );
    }

    // ── Discoverability surface ───────────────────────────────────────────────

    #[test]
    fn support_kinds_lists_remaining_two_in_canonical_order() {
        use super::SUPPORT_KINDS;
        // `fixed_support` retired in SIR-α wave-1 (task 3540 step-20) — it is
        // now the `structure def FixedSupport : Support` ctor path.
        // `pinned_support` retired in SIR-β-sup wave-2 (task 3546 step-4) — it
        // is now the `structure def PinnedSupport : Support` ctor path.
        // The remaining name-dispatched support kinds keep their canonical order.
        assert_eq!(
            SUPPORT_KINDS,
            &["displacement_support", "roller_support"]
        );
    }

    // `is_support_value_recognises_pinned_support` removed in SIR-β-sup wave-2
    // (task 3546 step-4): `PinnedSupport` no longer produces a kind-tagged
    // `Value::Map` — it is a `structure def` evaluating to a
    // `Value::StructureInstance`, which `is_support_value` (a Map-shape
    // predicate) is not designed to recognise. Structure-instance recognition
    // is the SIR-β-sup boundary suite's concern
    // (`crates/reify-eval/tests/pinned_support.rs`), not this Map-kind predicate.

    // `is_support_value_recognises_fixed_support` removed in SIR-α wave-1
    // (task 3540 step-20): `FixedSupport` no longer produces a kind-tagged
    // `Value::Map` — it is a `structure def` evaluating to a
    // `Value::StructureInstance`, which `is_support_value` (a Map-shape
    // predicate) is not designed to recognise. Structure-instance recognition
    // is the SIR-α boundary suite's concern, not this Map-kind predicate.

    #[test]
    fn is_support_value_recognises_displacement_support() {
        use super::is_support_value;
        let disp = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        let v = eval_builtin("DisplacementSupport", &[point_selector_stub(), disp]);
        assert!(
            is_support_value(&v),
            "is_support_value should recognize DisplacementSupport result"
        );
    }

    #[test]
    fn is_support_value_recognises_roller_support() {
        use super::is_support_value;
        let normal = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let v = eval_builtin("RollerSupport", &[point_selector_stub(), normal]);
        assert!(
            is_support_value(&v),
            "is_support_value should recognize RollerSupport result"
        );
    }

    #[test]
    fn is_support_value_negative_cases() {
        use super::is_support_value;

        // Cross-module isolation: build a load value via eval_builtin to confirm
        // LOAD_KINDS and SUPPORT_KINDS don't cross-contaminate is_support_value.
        // Uses `traction_load` (still name-dispatched) rather than `point_load`
        // (retired in SIR-α wave-1, task 3540 step-20 — now Undef) or
        // `pressure_load` (retired in SIR-β-load, task 3544 step-4 — also Undef
        // now). `traction_load` produces an analogous kind-tagged Map with a
        // `traction` field; `is_support_value` must return false for it.
        let stub = point_selector_stub();
        let traction_vec = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::PRESSURE);
        let load_value = eval_builtin("traction_load", &[stub, traction_vec]);

        // Map without a `kind` key.
        let map_no_kind = Value::Map(BTreeMap::new());

        // Map with kind="not_a_support".
        let map_wrong_kind = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("not_a_support".to_string()),
            );
            m
        });

        // Map with kind=Value::Int(0) (non-string kind).
        let map_int_kind = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(Value::String("kind".to_string()), Value::Int(0));
            m
        });

        let negative_cases: Vec<(&str, Value)> = vec![
            ("Real(1.0)", Value::Real(1.0)),
            ("Int(0)", Value::Int(0)),
            (
                "String(\"fixed_support\")",
                Value::String("fixed_support".to_string()),
            ),
            ("Map without kind key", map_no_kind),
            ("Map with kind=\"not_a_support\"", map_wrong_kind),
            ("Map with kind=Int(0)", map_int_kind),
            ("load value from eval_builtin(traction_load)", load_value),
        ];

        for (label, v) in negative_cases {
            assert!(
                !is_support_value(&v),
                "is_support_value should return false for: {}",
                label
            );
        }
    }

    // ── task 3540 step-19 (RED): post-retirement contract ────────────────────
    //
    // After step-20 (SIR-α stdlib swap), `FixedSupport` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def FixedSupport : Support { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `FixedSupport(...)` calls then lower to
    // `CompiledExprKind::StructureInstanceCtor` (precedence path from step-16)
    // and eval into a `Value::StructureInstance`. The
    // `eval_builtin("FixedSupport", ...)` Rust API path (used by tests below)
    // returns `Value::Undef` because the dispatch arm in `eval_supports` is
    // removed.
    //
    // RED: this test currently fails because `eval_builtin("FixedSupport", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-20 retires
    // the arm and updates `SUPPORT_KINDS` so the partition guard stays green.

    #[test]
    fn fixed_support_eval_builtin_returns_undef_post_retirement() {
        let selector = point_selector_stub();
        assert!(
            eval_builtin("FixedSupport", &[selector]).is_undef(),
            "after step-20 retirement, eval_builtin('FixedSupport', ...) must \
             return Undef; the structure-instance ctor path replaces the \
             builtin entirely (PRD §6, Q-SIR-4 — FixedSupport → structure def)"
        );
    }

    // ── task 3546 step-3 (RED): PinnedSupport post-retirement contract ────────
    //
    // After step-4 (SIR-β-sup retirement), `PinnedSupport` is no longer a
    // name-dispatched builtin — its role is taken by the
    // `structure def PinnedSupport : Support { ... }` declaration in
    // `crates/reify-compiler/stdlib/fea_multi_case.ri`. Source-level
    // `PinnedSupport(...)` calls then lower to
    // `CompiledExprKind::StructureInstanceCtor` and eval into a
    // `Value::StructureInstance`. The `eval_builtin("PinnedSupport", ...)`
    // Rust API path returns `Value::Undef` because the dispatch arm in
    // `eval_supports` is removed.
    //
    // RED: this test currently fails because `eval_builtin("PinnedSupport", ...)`
    // returns a `Value::Map` (the pre-retirement happy path). Step-4 retires
    // the arm and updates `SUPPORT_KINDS` so the partition guard stays green.

    #[test]
    fn pinned_support_eval_builtin_returns_undef_post_retirement() {
        let selector = point_selector_stub();
        assert!(
            eval_builtin("PinnedSupport", &[selector]).is_undef(),
            "after step-4 SIR-β-sup retirement (task 3546), \
             eval_builtin('PinnedSupport', ...) must return Undef; the \
             structure-def ctor path replaces the builtin entirely \
             (PRD §8 Phase 2, Q-SIR-4 — PinnedSupport → structure def)"
        );
    }

    // ── SUPPORT_KINDS partition test ──────────────────────────────────────────

    /// Guard that every kind listed in `SUPPORT_KINDS` is actually dispatched
    /// by `eval_supports`.  If a kind is renamed or removed in `eval_supports`
    /// but not updated in `SUPPORT_KINDS` (or vice versa), this test will
    /// catch it.
    #[test]
    fn support_kinds_all_dispatched_by_eval_supports() {
        use super::{SUPPORT_KINDS, eval_supports, is_support_value};

        let stub_selector = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("stub".to_string()),
            );
            m
        });
        let length_vec = make_scalar_vec3([0.001, 0.0, 0.0], DimensionVector::LENGTH);
        let dimensionless_z =
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);

        for kind in SUPPORT_KINDS {
            // NOTE: SUPPORT_KINDS contains the snake_case kind-tag strings used
            // in the Map's `kind` field.  The eval_supports dispatch arms use the
            // PascalCase constructor names.  We map explicitly here so the test
            // still guards both the SUPPORT_KINDS list and the dispatch arms.
            // `fixed_support` retired in SIR-α wave-1 (step-20) — no longer in
            // SUPPORT_KINDS, so no fixture arm here.
            // `pinned_support` retired in SIR-β-sup wave-2 (task 3546 step-4) —
            // no longer in SUPPORT_KINDS, so no fixture arm here.
            let result = match *kind {
                "displacement_support" => eval_supports(
                    "DisplacementSupport",
                    &[stub_selector.clone(), length_vec.clone()],
                ),
                "roller_support" => eval_supports(
                    "RollerSupport",
                    &[stub_selector.clone(), dimensionless_z.clone()],
                ),
                other => panic!(
                    "SUPPORT_KINDS contains '{}' but no fixture is defined for it — \
                     add a fixture arm to this test and an arm to eval_supports",
                    other
                ),
            };

            assert!(
                result.is_some(),
                "eval_supports('{}', ...) returned None — SUPPORT_KINDS is out of sync \
                 with eval_supports dispatch arms",
                kind
            );

            let value = result.unwrap();
            assert!(
                is_support_value(&value),
                "is_support_value should recognize a Map produced by eval_supports('{}', ...)",
                kind
            );
        }
    }
}
