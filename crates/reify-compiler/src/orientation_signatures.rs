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

/// Result type for an orientation/transform/frame constructor builtin — a fixed
/// nominal type keyed on `name` alone. Mirrors [`joint_ctor_result_type`], but
/// simpler: every name in this family is arg-agnostic (fixed nominal), so the
/// `args` slice is unused (named `_args` to keep the signature parallel to the
/// sibling resolvers).
///
/// - The 10 Orientation producers → `Type::Orientation(3)`.
/// - `transform3` / `transform3_identity` / `transform_compose` →
///   `Type::Transform(3)`.
/// - `frame3` → `Type::Frame(3)`.
///
/// ## Cell-type / value-kind agreement
///
/// Unlike the joint family (which types a `Value::Map` cell as a
/// `Type::StructureRef`), here the cell TYPE matches the eval VALUE KIND
/// exactly: `Type::Orientation(3)` ⇄ `Value::Orientation`, `Type::Transform(3)`
/// ⇄ `Value::Transform`, `Type::Frame(3)` ⇄ `Value::Frame`. So this arm is
/// strictly safe under `value_type_kind_matches` — no `StructureRef` escape
/// hatch is relied upon.
///
/// Only reached for names in [`ORIENTATION_TYPED_FN_NAMES`] (the caller gates on
/// [`is_orientation_typed_fn`]); the `_` arm is therefore unreachable in
/// practice and returns a harmless `Type::dimensionless_scalar()`.
pub(crate) fn orientation_typed_fn_result_type(name: &str, _args: &[CompiledExpr]) -> Type {
    match name {
        // ── Orientation producers (10) → Orientation(3) ──────────────────────
        // Eval: reify_stdlib::orientation::eval_orientation → Value::Orientation.
        "orient_identity"
        | "orient_quaternion"
        | "orient_euler"
        | "orient_basis"
        | "orient_look_at"
        | "orient_axis_angle"
        | "orient_exp"
        | "orient_inverse"
        | "orient_compose"
        | "orient_slerp" => Type::Orientation(3),

        // ── Transform producers (3) → Transform(3) ───────────────────────────
        // Eval: reify_stdlib::geometry::eval_geometry → Value::Transform.
        "transform3" | "transform3_identity" | "transform_compose" => Type::Transform(3),

        // ── Frame producer (1) → Frame(3) ────────────────────────────────────
        // Eval: reify_stdlib::geometry::eval_geometry → Value::Frame.
        "frame3" => Type::Frame(3),

        // Unreachable in practice — the caller gates on is_orientation_typed_fn.
        _ => Type::dimensionless_scalar(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent fixture — the 18 expected names in the family. Deliberately
    /// does NOT reference `ORIENTATION_TYPED_FN_NAMES` so a drift in that slice
    /// is caught against this independent list (mirrors
    /// `joint_signatures::tests::EXPECTED_NAMES`). Re-derived from the eval
    /// dispatchers `reify_stdlib::orientation::eval_orientation` (13 arms: 10
    /// producers + 3 decomposers) and `reify_stdlib::geometry::eval_geometry`.
    const EXPECTED_NAMES: [&str; 18] = [
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
        // Frame producers (2) → Type::Frame(3)
        "frame3",
        "frame3_identity",
        // Transform producers (6) → Type::Transform(3)
        "transform3",
        "transform3_identity",
        "transform_compose",
        "transform_inverse",
        "transform_exp",
        // NOTE the `frame_` prefix — `frame_to_frame` returns Value::Transform
        // (geometry.rs:512), NOT a Frame. Prefix-based classification would put
        // it in the wrong group; see the decomposer test below.
        "frame_to_frame",
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

    /// The FOUR decomposers are deliberately EXCLUDED from the family. Each
    /// shares an `orient_`/`transform_` prefix with a genuine producer but
    /// returns a DIFFERENT value kind, so a `starts_with("orient_")` /
    /// `starts_with("transform_")` prefix rule would newly MISTYPE all four.
    /// That is precisely why [`ORIENTATION_TYPED_FN_NAMES`] must stay an
    /// explicit list and never become a prefix predicate.
    ///
    /// Kinds re-derived from the evaluators:
    /// - `orient_log` → `Value::Vector` (rotation vector / log map),
    ///   `orientation.rs:203`;
    /// - `orient_to_euler` → `Value::List` of Angles, `orientation.rs:264`;
    /// - `orient_to_axis_angle` → `Value::Map{angle, axis}`, `orientation.rs:440`;
    /// - `transform_log` → `Value::Map` (twist), `geometry.rs:677`.
    ///
    /// The two `Value::Map` cases have no clean `Type` variant and
    /// `orient_log`'s quantity slot needs a dimension ruling, so per-name typing
    /// for the decomposers is out of scope here (tracked as a follow-up).
    #[test]
    fn is_orientation_typed_fn_rejects_the_four_decomposers() {
        assert!(
            !is_orientation_typed_fn("orient_log"),
            "must reject decomposer 'orient_log' (returns Value::Vector)"
        );
        assert!(
            !is_orientation_typed_fn("orient_to_euler"),
            "must reject decomposer 'orient_to_euler' (returns Value::List of Angles)"
        );
        assert!(
            !is_orientation_typed_fn("orient_to_axis_angle"),
            "must reject decomposer 'orient_to_axis_angle' (returns Value::Map{{angle, axis}})"
        );
        assert!(
            !is_orientation_typed_fn("transform_log"),
            "must reject decomposer 'transform_log' (returns Value::Map twist)"
        );
    }

    /// `frame_at` is NOT a member: it is already typed `Type::Frame(3)` by
    /// `units.rs::datum_constructor_result_type`, which claims it as part of the
    /// DATUM constructor family. Including it here would double-classify the
    /// name across two resolvers, so the exclusion is load-bearing rather than
    /// an oversight.
    #[test]
    fn is_orientation_typed_fn_rejects_frame_at() {
        assert!(
            !is_orientation_typed_fn("frame_at"),
            "'frame_at' belongs to the datum family (datum_constructor_result_type \
             already types it Frame(3)) — claiming it here would double-classify"
        );
    }

    /// `is_orientation_typed_fn` rejects sibling-family names, the empty name,
    /// and unknown names.
    #[test]
    fn is_orientation_typed_fn_rejects_other_family_and_unknown_names() {
        // Sibling families — one representative each.
        assert!(
            !is_orientation_typed_fn("affine_identity"),
            "must reject affine-map-constructor 'affine_identity' (general affine, not rigid)"
        );
        assert!(
            !is_orientation_typed_fn("affine_from_transform"),
            "must reject 'affine_from_transform' — it CONSUMES a Transform and \
             produces an AffineMap, so it belongs to the affine family"
        );
        assert!(!is_orientation_typed_fn("vec"), "must reject math-linalg 'vec'");
        assert!(
            !is_orientation_typed_fn("prismatic"),
            "must reject joint-constructor 'prismatic'"
        );
        assert!(
            !is_orientation_typed_fn("parse_length"),
            "must reject parse-family 'parse_length'"
        );
        assert!(
            !is_orientation_typed_fn("nominal"),
            "must reject tolerancing-marker 'nominal'"
        );
        // `project` deliberately mirrors its first argument's type, so the
        // first-arg fallback is already CORRECT for it — claiming it here would
        // be a regression, not a fix.
        assert!(
            !is_orientation_typed_fn("project"),
            "must reject 'project' — its first-arg fallback is already correct"
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
            !is_orientation_typed_fn("Orient_Identity"),
            "capitalised form must not match"
        );
        assert!(
            !is_orientation_typed_fn("ORIENT_IDENTITY"),
            "upper-case form must not match"
        );
        assert!(
            !is_orientation_typed_fn("Transform3"),
            "capitalised form must not match"
        );
    }

    /// `ORIENTATION_TYPED_FN_NAMES` is exactly the 18 expected names: correct
    /// count, every expected name present, and no extra entry. Mirrors
    /// `joint_typed_fn_names_are_exactly_the_17`.
    #[test]
    fn orientation_typed_fn_names_are_exactly_the_18() {
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
