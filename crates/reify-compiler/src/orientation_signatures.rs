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

/// The complete set of orientation/transform/frame constructor builtin names
/// recognised by the compiler. Single source of truth — imported into the
/// `units.rs` test module to pin disjointness from all sibling families.
///
/// **18 names** grouped by target nominal type:
/// - **Orientation producers** (10) → `Type::Orientation(3)`: `orient_identity`,
///   `orient_quaternion`, `orient_euler`, `orient_basis`, `orient_look_at`,
///   `orient_axis_angle`, `orient_exp`, `orient_inverse`, `orient_compose`,
///   `orient_slerp`. Eval dispatch: `reify_stdlib::orientation::eval_orientation`.
/// - **Frame producers** (2) → `Type::Frame(3)`: `frame3`, `frame3_identity`.
///   Eval dispatch: `reify_stdlib::geometry::eval_geometry`.
/// - **Transform producers** (6) → `Type::Transform(3)`: `transform3`,
///   `transform3_identity`, `transform_compose`, `transform_inverse`,
///   `transform_exp`, `frame_to_frame`. Eval dispatch:
///   `reify_stdlib::geometry::eval_geometry`.
///
/// # The family is NOT uniform — never replace this list with a prefix rule
///
/// This MUST stay an explicit list. A `starts_with("orient_")` /
/// `starts_with("transform_")` prefix rule would newly MISTYPE all FOUR
/// DECOMPOSERS, which share a prefix with a genuine producer but return a
/// different value kind and are deliberately EXCLUDED:
/// - `orient_log` → `Value::Vector` (the rotation vector / log map),
/// - `orient_to_euler` → `Value::List` of Angles,
/// - `orient_to_axis_angle` → `Value::Map` `{angle, axis}`,
/// - `transform_log` → `Value::Map` (twist).
///
/// The two `Value::Map` cases have no clean `Type` variant and `orient_log`'s
/// quantity slot needs a dimension ruling, so per-name typing for the
/// decomposers is out of scope here (a follow-up).
///
/// `frame_at` is likewise EXCLUDED: `units.rs::datum_constructor_result_type`
/// already types it `Type::Frame(3)` as part of the DATUM family, so claiming
/// it here would double-classify the name across two resolvers.
///
/// # Two traps encoded in this list
///
/// 1. **`frame_to_frame` is a Transform, not a Frame.** Despite the `frame_`
///    prefix it returns `Value::Transform` (`geometry.rs:512`) — it computes the
///    rigid motion mapping one frame onto another.
/// 2. **The digit is meaningful.** The digit-carrying `transform3` /
///    `transform3_identity` CONSTRUCTORS are distinct from the digitless
///    `transform_compose` / `transform_inverse` / `transform_exp` OPERATIONS;
///    both groups land on `Type::Transform(3)`, but the spelling split is real
///    and must not be "tidied".
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
    // Frame producers (2): → Type::Frame(3)
    "frame3",
    "frame3_identity",
    // Transform producers (6): → Type::Transform(3)
    "transform3",
    "transform3_identity",
    "transform_compose",
    "transform_inverse",
    "transform_exp",
    // Trap: `frame_` prefix, but returns Value::Transform (geometry.rs:512).
    "frame_to_frame",
];

/// Is `name` an orientation/transform/frame constructor builtin the compiler
/// types via [`orientation_typed_fn_result_type`]? Name-only classification —
/// a `.contains` over the single-source-of-truth slice
/// [`ORIENTATION_TYPED_FN_NAMES`]. Case-sensitive.
pub(crate) fn is_orientation_typed_fn(name: &str) -> bool {
    ORIENTATION_TYPED_FN_NAMES.contains(&name)
}

/// Result type for an orientation/transform/frame constructor builtin — a fixed
/// nominal type keyed on `name` alone.
///
/// Adopts the name-only INFALLIBLE `-> Type` shape of
/// [`crate::parse_signatures::parse_fn_result_type`] rather than the args-aware
/// `-> Option<Type>` / `&[CompiledExpr]` shapes used by the arity-sensitive
/// families, because every result type here is argument-INDEPENDENT: there is
/// no argument whose type or count could change the answer.
///
/// Per-name mapping:
/// - `orient_identity`, `orient_quaternion`, `orient_euler`, `orient_basis`,
///   `orient_look_at`, `orient_axis_angle`, `orient_exp`, `orient_inverse`,
///   `orient_compose`, `orient_slerp` → `Type::Orientation(3)`
/// - `frame3`, `frame3_identity` → `Type::Frame(3)`
/// - `transform3`, `transform3_identity`, `transform_compose`,
///   `transform_inverse`, `transform_exp`, `frame_to_frame` →
///   `Type::Transform(3)`
///
/// ## Cell-type / value-kind agreement
///
/// Unlike the joint family (which types a `Value::Map` cell as a
/// `Type::StructureRef`), here the Type↔Value correspondence is EXACT:
/// `Type::Orientation(3)` ⇄ `Value::Orientation`, `Type::Frame(3)` ⇄
/// `Value::Frame`, `Type::Transform(3)` ⇄ `Value::Transform`.
///
/// This makes the change guard-SATISFYING rather than guard-breaking. Before
/// this family existed, eval produced a `Value::Orientation` into a cell
/// statically typed `Real` (via the first-arg fallback), so
/// `reify_eval::value_type_kind_matches` had a static/runtime kind DISAGREEMENT
/// at every such site. Assigning the correct static type makes that guard agree
/// where it previously did not — no `StructureRef` escape hatch is relied upon.
///
/// Only reached for names in [`ORIENTATION_TYPED_FN_NAMES`] (the caller gates on
/// [`is_orientation_typed_fn`]); the `_` arm is therefore unreachable in
/// practice and returns a harmless `Type::dimensionless_scalar()`. The
/// `every_orientation_fn_name_maps_to_a_non_scalar_result_type` test makes a
/// silent fallthrough loud.
pub(crate) fn orientation_typed_fn_result_type(name: &str) -> Type {
    match name {
        // ── Section (1): Orientation producers (10) → Orientation(3) ─────────
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

        // ── Section (2): Frame producers (2) → Frame(3) ──────────────────────
        // Eval: reify_stdlib::geometry::eval_geometry → Value::Frame.
        "frame3" | "frame3_identity" => Type::Frame(3),

        // ── Section (3): Transform producers (6) → Transform(3) ──────────────
        // Eval: reify_stdlib::geometry::eval_geometry → Value::Transform.
        // `frame_to_frame` belongs HERE, not in section (2): despite the
        // `frame_` prefix it returns the rigid motion BETWEEN two frames
        // (geometry.rs:512), i.e. a Transform.
        "transform3"
        | "transform3_identity"
        | "transform_compose"
        | "transform_inverse"
        | "transform_exp"
        | "frame_to_frame" => Type::Transform(3),

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

    /// One explicit assert per name, all 18, pinning the three target types.
    /// Written out longhand rather than looped so a wrong mapping names the
    /// exact function in the failure output.
    #[test]
    fn orientation_typed_fn_result_type_maps_each_name_to_its_nominal_type() {
        // Orientation producers (10) → Type::Orientation(3).
        for name in [
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
        ] {
            assert_eq!(
                orientation_typed_fn_result_type(name),
                Type::Orientation(3),
                "{name} must map to Type::Orientation(3)"
            );
        }
        // Frame producers (2) → Type::Frame(3).
        for name in ["frame3", "frame3_identity"] {
            assert_eq!(
                orientation_typed_fn_result_type(name),
                Type::Frame(3),
                "{name} must map to Type::Frame(3)"
            );
        }
        // Transform producers (6) → Type::Transform(3).
        for name in [
            "transform3",
            "transform3_identity",
            "transform_compose",
            "transform_inverse",
            "transform_exp",
            "frame_to_frame",
        ] {
            assert_eq!(
                orientation_typed_fn_result_type(name),
                Type::Transform(3),
                "{name} must map to Type::Transform(3)"
            );
        }
    }

    /// ACCEPTANCE PIN (the task's named test): `orient_axis_angle(axis, angle)`
    /// must resolve to `Type::Orientation(3)` and DECISIVELY NOT a Vector.
    ///
    /// Its first argument is the rotation AXIS, a `Vector{3}` — so the old
    /// first-arg fallback silently produced `Vector{3}` for every
    /// `orient_axis_angle` call site (9 of them in prj/printer_v01/printer.ri).
    /// That is the exact mistyping this family fixes.
    #[test]
    fn orient_axis_angle_result_type_is_orientation_not_vector() {
        assert_eq!(
            orientation_typed_fn_result_type("orient_axis_angle"),
            Type::Orientation(3),
            "orient_axis_angle(axis, angle) must resolve to Orientation(3)"
        );
        assert!(
            !matches!(
                orientation_typed_fn_result_type("orient_axis_angle"),
                Type::Vector { .. }
            ),
            "orient_axis_angle must NOT adopt the first-arg (axis) Vector type"
        );
    }

    /// Guards the `frame_` prefix trap: `frame_to_frame` computes the rigid
    /// motion mapping one frame onto another, so it returns `Value::Transform`
    /// (`geometry.rs:512`) and must type as `Type::Transform(3)` — NOT
    /// `Type::Frame(3)`, which a prefix-based grouping would wrongly assign.
    #[test]
    fn frame_to_frame_result_type_is_transform_not_frame() {
        assert_eq!(
            orientation_typed_fn_result_type("frame_to_frame"),
            Type::Transform(3),
            "frame_to_frame returns Value::Transform despite its frame_ prefix"
        );
        assert_ne!(
            orientation_typed_fn_result_type("frame_to_frame"),
            Type::Frame(3),
            "frame_to_frame must NOT be grouped with the Frame producers"
        );
    }

    /// The three ZERO-ARG members are the only names in the family that could
    /// trip the "cannot infer return type of zero-arg function" warning, because
    /// that warning is emitted solely from the first-arg fallback's
    /// `unwrap_or_else` branch (reached only when `compiled_args.first()` is
    /// `None`). Resolving them by name alone is what silences it.
    #[test]
    fn zero_arg_constructors_resolve_without_the_first_arg_fallback() {
        assert_eq!(
            orientation_typed_fn_result_type("orient_identity"),
            Type::Orientation(3)
        );
        assert_eq!(
            orientation_typed_fn_result_type("frame3_identity"),
            Type::Frame(3)
        );
        assert_eq!(
            orientation_typed_fn_result_type("transform3_identity"),
            Type::Transform(3)
        );
    }

    /// Anti-fallthrough lock: no name in the family may resolve to a scalar.
    /// The resolver's `_` arm returns `Type::dimensionless_scalar()` and is
    /// meant to be unreachable (the caller gates on `is_orientation_typed_fn`),
    /// so a name that silently reaches it would be indistinguishable from the
    /// old buggy fallback. This test makes that failure loud.
    #[test]
    fn every_orientation_fn_name_maps_to_a_non_scalar_result_type() {
        for name in ORIENTATION_TYPED_FN_NAMES {
            let ty = orientation_typed_fn_result_type(name);
            assert!(
                !matches!(ty, Type::Scalar { .. }),
                "{name} fell through to the unreachable `_` scalar arm — every \
                 family member must have an explicit match arm"
            );
            assert_ne!(
                ty,
                Type::dimensionless_scalar(),
                "{name} resolved to dimensionless_scalar (the `_` arm)"
            );
        }
    }
}
