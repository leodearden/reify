//! Compiler signatures for the orientation / transform / frame **constructor**
//! builtin family (task 5344).
//!
//! Holds the single source of truth for the orientation/transform/frame
//! constructor builtin name family ([`ORIENTATION_TYPED_FN_NAMES`]), the
//! name-only classification predicate ([`is_orientation_typed_fn`]), and the
//! name→nominal-type resolver ([`orientation_typed_fn_result_type`]).
//!
//! These constructors all map to a FIXED nominal type independent of their
//! arguments: `orient_*` producers → `Type::Orientation(3)`, `transform*` →
//! `Type::Transform(3)`, `frame3` → `Type::Frame(3)`. Without this arm they fall
//! through the `expr.rs` `NoUserFunctions` ladder to the first-arg fallback,
//! which mistypes them (e.g. `orient_axis_angle(vec3, angle)` → `Vector{3}`,
//! `transform3(orient, vec3)` → the orient arg's type) and emits the "cannot
//! infer return type of zero-arg function" warning for `orient_identity()`.
//!
//! Mirrors the `joint_signatures.rs` module structure (task 4311 recipe).
//! Scaffolding stubs — the name slice and resolver are populated in the
//! subsequent TDD steps.

use reify_core::Type;
use reify_ir::CompiledExpr;

/// The complete set of orientation/transform/frame constructor builtin names
/// recognised by the compiler. Single source of truth — imported into the
/// `units.rs` test module to pin disjointness from all sibling families.
///
/// **14 names** grouped by target nominal type:
/// - **Orientation producers** (10) → `Type::Orientation(3)`: `orient_identity`,
///   `orient_quaternion`, `orient_euler`, `orient_basis`, `orient_look_at`,
///   `orient_axis_angle`, `orient_exp`, `orient_inverse`, `orient_compose`,
///   `orient_slerp`. Eval dispatch: `reify_stdlib::orientation::eval_orientation`.
/// - **Transform producers** (3) → `Type::Transform(3)`: `transform3`,
///   `transform3_identity`, `transform_compose`. Eval dispatch:
///   `reify_stdlib::geometry::eval_geometry`.
/// - **Frame producer** (1) → `Type::Frame(3)`: `frame3`. Eval dispatch:
///   `reify_stdlib::geometry::eval_geometry`.
///
/// **The `orient_*` family is NOT uniform** — this MUST be an explicit list, not
/// a `starts_with("orient_")` prefix. Three `orient_*` DECOMPOSERS are
/// deliberately EXCLUDED because they return other value kinds, not an
/// orientation:
/// - `orient_log` → `Value::Vector` (the rotation vector / log map),
/// - `orient_to_euler` → `Value::List` of Angles,
/// - `orient_to_axis_angle` → `Value::Map` `{angle, axis}`.
///
/// A prefix or blanket "all `orient_*` → Orientation" rule would newly MISTYPE
/// these three (and the Map case has no clean `Type` variant). Per-name typing
/// for the decomposers is out of scope here (a follow-up).
///
/// Case-sensitive: Reify function names are snake_case.
pub const ORIENTATION_TYPED_FN_NAMES: &[&str] = &[
    // Orientation producers (10): → Type::Orientation(3)
    "orient_identity",
    "orient_quaternion",
    "orient_euler",
    "orient_basis",
    "orient_look_at",
    "orient_axis_angle",
    "orient_exp",
    "orient_inverse",
    "orient_compose",
    "orient_slerp",
    // Transform producers (3): → Type::Transform(3)
    "transform3",
    "transform3_identity",
    "transform_compose",
    // Frame producer (1): → Type::Frame(3)
    "frame3",
];

/// Is `name` an orientation/transform/frame constructor builtin the compiler
/// types via [`orientation_typed_fn_result_type`]? Name-only classification —
/// a `.contains` over the single-source-of-truth slice
/// [`ORIENTATION_TYPED_FN_NAMES`]. Case-sensitive.
pub(crate) fn is_orientation_typed_fn(name: &str) -> bool {
    ORIENTATION_TYPED_FN_NAMES.contains(&name)
}

/// Result type for an orientation/transform/frame constructor builtin.
///
/// Scaffolding stub — returns `Type::dimensionless_scalar()` until the per-name
/// resolver is implemented in step-4.
pub(crate) fn orientation_typed_fn_result_type(_name: &str, _args: &[CompiledExpr]) -> Type {
    Type::dimensionless_scalar()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent fixture — the 14 expected names in the family. Deliberately
    /// does NOT reference `ORIENTATION_TYPED_FN_NAMES` so a drift in that slice
    /// is caught against this independent list (mirrors
    /// `joint_signatures::tests::EXPECTED_NAMES`).
    const EXPECTED_NAMES: [&str; 14] = [
        // Orientation producers (10) → Type::Orientation(3)
        "orient_identity",
        "orient_quaternion",
        "orient_euler",
        "orient_basis",
        "orient_look_at",
        "orient_axis_angle",
        "orient_exp",
        "orient_inverse",
        "orient_compose",
        "orient_slerp",
        // Transform producers (3) → Type::Transform(3)
        "transform3",
        "transform3_identity",
        "transform_compose",
        // Frame producer (1) → Type::Frame(3)
        "frame3",
    ];

    // ── Name-family contract (step-1 RED / step-2 GREEN) ─────────────────────

    /// `is_orientation_typed_fn` recognises every expected constructor name.
    #[test]
    fn is_orientation_typed_fn_recognises_all_expected_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_orientation_typed_fn(name),
                "is_orientation_typed_fn({name:?}) must be true \
                 (orientation/transform/frame constructor family)"
            );
        }
    }

    /// `is_orientation_typed_fn` rejects sibling-family names, the three
    /// EXCLUDED `orient_*` decomposers (they return Value::Vector/List/Map, not
    /// Orientation), the empty name, and unknown names.
    #[test]
    fn is_orientation_typed_fn_rejects_other_family_and_unknown_names() {
        // Sibling families — one representative each.
        assert!(!is_orientation_typed_fn("vec"), "must reject math-linalg 'vec'");
        assert!(!is_orientation_typed_fn("sqrt"), "must reject math 'sqrt'");
        assert!(
            !is_orientation_typed_fn("box"),
            "must reject geometry-constructor 'box'"
        );
        assert!(
            !is_orientation_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        assert!(
            !is_orientation_typed_fn("nominal"),
            "must reject tolerancing-marker 'nominal'"
        );
        assert!(
            !is_orientation_typed_fn("prismatic"),
            "must reject joint-constructor 'prismatic'"
        );
        assert!(
            !is_orientation_typed_fn("affine_scale"),
            "must reject affine-map-constructor 'affine_scale'"
        );
        // The three DECOMPOSERS deliberately EXCLUDED from the family: a naive
        // `starts_with("orient_")` prefix would wrongly claim these — they must
        // NOT match (they return Vector / List / Map, not Orientation).
        assert!(
            !is_orientation_typed_fn("orient_log"),
            "must reject decomposer 'orient_log' (returns Value::Vector)"
        );
        assert!(
            !is_orientation_typed_fn("orient_to_euler"),
            "must reject decomposer 'orient_to_euler' (returns Value::List)"
        );
        assert!(
            !is_orientation_typed_fn("orient_to_axis_angle"),
            "must reject decomposer 'orient_to_axis_angle' (returns Value::Map)"
        );
        // Empty / unknown.
        assert!(!is_orientation_typed_fn(""), "must reject empty name");
        assert!(
            !is_orientation_typed_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so
    /// capitalised forms must not match (mirrors
    /// `is_joint_typed_fn_is_case_sensitive`).
    #[test]
    fn is_orientation_typed_fn_is_case_sensitive() {
        assert!(
            !is_orientation_typed_fn("Frame3"),
            "capitalised form must not match"
        );
        assert!(
            !is_orientation_typed_fn("Transform3"),
            "capitalised form must not match"
        );
        assert!(
            !is_orientation_typed_fn("Orient_identity"),
            "capitalised form must not match"
        );
        assert!(
            !is_orientation_typed_fn("ORIENT_AXIS_ANGLE"),
            "upper-case form must not match"
        );
    }

    /// `ORIENTATION_TYPED_FN_NAMES` is exactly the 14 expected names: correct
    /// count, every expected name present, and no extra entry. Mirrors
    /// `joint_typed_fn_names_are_exactly_the_17`.
    #[test]
    fn orientation_typed_fn_names_are_exactly_the_14() {
        assert_eq!(
            ORIENTATION_TYPED_FN_NAMES.len(),
            EXPECTED_NAMES.len(),
            "ORIENTATION_TYPED_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            ORIENTATION_TYPED_FN_NAMES
        );
        // Every expected name is in the slice.
        for name in EXPECTED_NAMES {
            assert!(
                ORIENTATION_TYPED_FN_NAMES.contains(&name),
                "ORIENTATION_TYPED_FN_NAMES must contain {name:?}"
            );
        }
        // No extra name beyond the expected fixture.
        for name in ORIENTATION_TYPED_FN_NAMES {
            assert!(
                EXPECTED_NAMES.contains(name),
                "ORIENTATION_TYPED_FN_NAMES has unexpected entry {name:?} not in the fixture"
            );
        }
    }

    // ── Result-type resolution (step-3 RED / step-4 GREEN) ───────────────────

    /// The 10 Orientation producers.
    const ORIENTATION_PRODUCERS: [&str; 10] = [
        "orient_identity",
        "orient_quaternion",
        "orient_euler",
        "orient_basis",
        "orient_look_at",
        "orient_axis_angle",
        "orient_exp",
        "orient_inverse",
        "orient_compose",
        "orient_slerp",
    ];

    /// Every Orientation producer resolves to `Type::Orientation(3)`, the three
    /// Transform producers to `Type::Transform(3)`, and `frame3` to
    /// `Type::Frame(3)`. Called with `&[]` (name-only dispatch).
    ///
    /// RED until step-4: the stub resolver returns `dimensionless_scalar`.
    #[test]
    fn orientation_typed_fn_result_type_maps_each_name_to_its_nominal_type() {
        for name in ORIENTATION_PRODUCERS {
            assert_eq!(
                orientation_typed_fn_result_type(name, &[]),
                Type::Orientation(3),
                "{name} must map to Type::Orientation(3)"
            );
        }
        for name in &["transform3", "transform3_identity", "transform_compose"] {
            assert_eq!(
                orientation_typed_fn_result_type(name, &[]),
                Type::Transform(3),
                "{name} must map to Type::Transform(3)"
            );
        }
        assert_eq!(
            orientation_typed_fn_result_type("frame3", &[]),
            Type::Frame(3),
            "frame3 must map to Type::Frame(3)"
        );
    }

    /// ACCEPTANCE PIN: `orient_axis_angle(vec3, angle)` must resolve to
    /// `Type::Orientation(3)` and DECISIVELY NOT the first-arg Vector type. The
    /// dummy first arg is deliberately TYPED as a `Vector{3}` so this proves the
    /// resolver ignores the first-arg type (the old first-arg fallback would
    /// have produced `Vector{3}` here — the exact bug this task fixes).
    #[test]
    fn orient_axis_angle_result_is_orientation_not_first_arg_vector() {
        use reify_ir::Value;

        let vec3_arg = CompiledExpr::literal(
            Value::Real(1.0),
            Type::vec3(Type::dimensionless_scalar()),
        );
        let args = &[vec3_arg];

        assert_eq!(
            orientation_typed_fn_result_type("orient_axis_angle", args),
            Type::Orientation(3),
            "orient_axis_angle(vec3, angle) must resolve to Orientation(3)"
        );
        assert_ne!(
            orientation_typed_fn_result_type("orient_axis_angle", args),
            Type::vec3(Type::dimensionless_scalar()),
            "orient_axis_angle must NOT adopt the first-arg Vector type"
        );
    }

    /// Args-agnostic invariant: every producer returns the same result type for
    /// an empty arg slice and a non-empty one (the resolver is name-only).
    #[test]
    fn orientation_typed_fn_result_type_is_args_agnostic() {
        use reify_ir::Value;

        let dummy = CompiledExpr::literal(
            Value::Real(1.0),
            Type::vec3(Type::dimensionless_scalar()),
        );
        let args = &[dummy];

        for name in EXPECTED_NAMES {
            assert_eq!(
                orientation_typed_fn_result_type(name, args),
                orientation_typed_fn_result_type(name, &[]),
                "{name} result must be the same regardless of args"
            );
        }
    }
}
