//! Compiler signatures for the orientation / transform / frame **constructor**
//! builtin family (task 5344).
//!
//! Holds the single source of truth for the orientation/transform/frame
//! constructor builtin name family ([`ORIENTATION_TYPED_FN_NAMES`]), the
//! name-only classification predicate ([`is_orientation_typed_fn`]), and the
//! name→nominal-type resolver ([`orientation_typed_fn_result_type`]).
//!
//! These constructors all map to a FIXED nominal type independent of their
//! arguments. Without this arm they fall through the `expr.rs`
//! `NoUserFunctions` ladder to the first-arg fallback, which mistypes them and
//! warns on the zero-arg members.
//!
//! [`ORIENTATION_TYPED_FN_NAMES`]'s doc comment is the SINGLE SOURCE OF TRUTH
//! for this family's membership and the rationale behind it — which names are
//! in, which are deliberately out, and the two traps encoded in the list.
//! Everything else in this crate that needs that rationale (the `expr.rs`
//! ladder arm, the tests below) points there rather than restating it.
//!
//! Mirrors the `joint_signatures.rs` module structure (task 4311 recipe).

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
/// Those four are NOT untyped — they are typed by the SIBLING slice
/// [`ORIENTATION_DECOMPOSER_FN_NAMES`] below (task 6306), which is where the
/// follow-up this doc once promised landed. They stay OUT of this slice because
/// the producer/decomposer split is the very thing that makes the no-prefix-rule
/// argument above legible: one prefix, two value kinds, therefore two lists.
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

/// The complete set of orientation/transform DECOMPOSER builtin names recognised
/// by the compiler (task 6306) — the sibling half of
/// [`ORIENTATION_TYPED_FN_NAMES`]. Single source of truth; imported into the
/// `units.rs` test module to pin disjointness from all sibling families AND from
/// the producer slice.
///
/// **4 names**, each with its eval site, runtime `Value`, and resolved static
/// type (the resolver is [`orientation_decomposer_result_type`], which carries
/// the per-arm ruling basis):
/// - `orient_log` (`orientation.rs:203`, result at `:228`) →
///   `Value::Vector` of three `Value::Real` → `Type::vec3(dimensionless)`.
/// - `orient_to_euler` (`orientation.rs:264`, result at `:438`) →
///   `Value::List` of three `Value::angle` → `Type::List(Angle)`.
/// - `orient_to_axis_angle` (`orientation.rs:440`, result at `:460-470`) →
///   `Value::Map {angle, axis}` → `Type::StructureRef("AxisAngle")`.
/// - `transform_log` (`geometry.rs:779`, constructor at `:860-873`) →
///   `Value::Map {angular, linear}` (a twist) → `Type::StructureRef("Twist")`.
///
/// # Why a SEPARATE slice rather than more entries in the producer list
///
/// These are the SAME two eval dispatchers as the producers —
/// `reify_stdlib::orientation::eval_orientation` (three of them) and
/// `reify_stdlib::geometry::eval_geometry` (one) — reached through the same
/// name-based dispatch. What differs is the VALUE KIND returned: a producer
/// hands back the nominal rigid-motion value its type names
/// (`Value::Orientation`/`Frame`/`Transform`), a decomposer hands back the
/// COORDINATES of one (`Vector`/`List`/`Map`). That split is exactly why the
/// prefix rule was rejected on [`ORIENTATION_TYPED_FN_NAMES`], and it is also why
/// the two halves cannot share one resolver: no single nominal type covers both.
///
/// Keeping them apart preserves the producer slice's doc as a true statement and
/// keeps `is_orientation_typed_fn_rejects_the_four_decomposers` meaningful; the
/// `is_orientation_decomposer_fn_rejects_the_producers` test pins the reciprocal,
/// so the two halves are disjoint by test in both directions.
///
/// Case-sensitive: Reify function names are snake_case.
pub const ORIENTATION_DECOMPOSER_FN_NAMES: &[&str] = &[
    // Orientation decomposers (3). Eval: orientation::eval_orientation.
    "orient_log",
    "orient_to_euler",
    "orient_to_axis_angle",
    // Transform decomposer (1). Eval: geometry::eval_geometry.
    "transform_log",
];

/// Is `name` an orientation/transform decomposer builtin the compiler types via
/// [`orientation_decomposer_result_type`]? Name-only classification — a
/// `.contains` over the single-source-of-truth slice
/// [`ORIENTATION_DECOMPOSER_FN_NAMES`]. Case-sensitive.
pub(crate) fn is_orientation_decomposer_fn(name: &str) -> bool {
    ORIENTATION_DECOMPOSER_FN_NAMES.contains(&name)
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
/// The per-name mapping is the grouping documented on
/// [`ORIENTATION_TYPED_FN_NAMES`], one match arm per group.
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
        // `frame_to_frame` belongs HERE, not in section (2) — see trap 1 on
        // ORIENTATION_TYPED_FN_NAMES.
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
    /// `joint_signatures::tests::EXPECTED_NAMES`).
    ///
    /// Derived BY HAND, once, from the eval dispatchers
    /// `reify_stdlib::orientation::eval_orientation` (13 arms: 10 producers + 3
    /// decomposers) and `reify_stdlib::geometry::eval_geometry`. That derivation
    /// is NOT enforced by any test: `reify-compiler` does not depend on
    /// `reify-stdlib` (not even at dev scope) and `reify-stdlib` exports no
    /// enumerable list of its dispatch arm names, so a new `orient_*` producer
    /// added there would silently fall back to the first-arg mistyping this
    /// module exists to fix. Closing that gap needs a `reify-stdlib` dev-dep and
    /// an exported name list — both outside task 5344's file scope; filed as a
    /// follow-up.
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
        // `frame_` prefix, Transform group — trap 1 on ORIENTATION_TYPED_FN_NAMES.
        "frame_to_frame",
    ];

    /// Independent fixture — the 4 expected names in the DECOMPOSER sub-family.
    /// Deliberately does NOT reference `ORIENTATION_DECOMPOSER_FN_NAMES`, for the
    /// same reason `EXPECTED_NAMES` above does not reference the producer slice:
    /// a drift in the slice is caught against an independent hand-derived list.
    ///
    /// Derived BY HAND from the SAME two eval dispatchers as `EXPECTED_NAMES`
    /// (`reify_stdlib::orientation::eval_orientation` contributes three,
    /// `reify_stdlib::geometry::eval_geometry` one) — and it carries the same
    /// un-enforced-derivation caveat recorded on `EXPECTED_NAMES`.
    const EXPECTED_DECOMPOSER_NAMES: [&str; 4] = [
        "orient_log",
        "orient_to_euler",
        "orient_to_axis_angle",
        "transform_log",
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

    /// The FOUR decomposers are deliberately EXCLUDED from the PRODUCER family —
    /// see the no-prefix-rule section of [`ORIENTATION_TYPED_FN_NAMES`] for why.
    ///
    /// EXCLUDED HERE DOES NOT MEAN UNTYPED. Since task 6306 all four are typed by
    /// the sibling predicate [`is_orientation_decomposer_fn`] over
    /// [`ORIENTATION_DECOMPOSER_FN_NAMES`]; this test asserts only that they are
    /// not claimed by the PRODUCER half, and
    /// `is_orientation_decomposer_fn_rejects_the_producers` asserts the converse.
    ///
    /// The eval sites the exclusion was derived from, for re-checking it:
    /// `orient_log` `orientation.rs:203`, `orient_to_euler`
    /// `orientation.rs:264`, `orient_to_axis_angle` `orientation.rs:440`,
    /// `transform_log` `geometry.rs:779` (was cited as `:677` before task 6306 —
    /// that line is inside `transform_exp`, which spans `:670-777`).
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

    /// `frame_at` is NOT a member — see the `frame_at` exclusion note on
    /// [`ORIENTATION_TYPED_FN_NAMES`]. Load-bearing, not an oversight.
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

    /// All 18 names, pinning the three target types. Looped over the three
    /// target groups rather than asserted per name; the assert message
    /// interpolates `{name}`, so a wrong mapping still names the exact function
    /// in the failure output.
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

    /// Guards trap 1 on [`ORIENTATION_TYPED_FN_NAMES`]: `frame_to_frame` must
    /// type as `Type::Transform(3)`, NOT the `Type::Frame(3)` a prefix-based
    /// grouping would wrongly assign.
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
            // `Type::dimensionless_scalar()` IS a `Type::Scalar`, so this single
            // negative covers the `_` arm's exact return value as well as any
            // other scalar spelling a future arm could produce.
            assert!(
                !matches!(orientation_typed_fn_result_type(name), Type::Scalar { .. }),
                "{name} fell through to the unreachable `_` scalar arm — every \
                 family member must have an explicit match arm"
            );
        }
    }

    // ── Decomposer name-family contract (this task's step-1 RED / step-2 GREEN) ─
    //
    // The mirror image of the producer contract above, for the four DECOMPOSERS.
    // Membership and every ruling live ONCE on `ORIENTATION_DECOMPOSER_FN_NAMES`.

    /// `is_orientation_decomposer_fn` recognises every expected decomposer name.
    #[test]
    fn is_orientation_decomposer_fn_recognises_all_four() {
        for name in EXPECTED_DECOMPOSER_NAMES {
            assert!(
                is_orientation_decomposer_fn(name),
                "is_orientation_decomposer_fn({name:?}) must be true \
                 (orientation/transform decomposer sub-family)"
            );
        }
    }

    /// `ORIENTATION_DECOMPOSER_FN_NAMES` is exactly the 4 expected names: correct
    /// count, every expected name present, and no extra entry. Mirrors
    /// `orientation_typed_fn_names_are_exactly_the_18`.
    #[test]
    fn orientation_decomposer_fn_names_are_exactly_the_four() {
        assert_eq!(
            ORIENTATION_DECOMPOSER_FN_NAMES.len(),
            EXPECTED_DECOMPOSER_NAMES.len(),
            "ORIENTATION_DECOMPOSER_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_DECOMPOSER_NAMES.len(),
            ORIENTATION_DECOMPOSER_FN_NAMES
        );
        // Every expected name is in the slice.
        for name in EXPECTED_DECOMPOSER_NAMES {
            assert!(
                ORIENTATION_DECOMPOSER_FN_NAMES.contains(&name),
                "ORIENTATION_DECOMPOSER_FN_NAMES must contain {name:?}"
            );
        }
        // No extra name beyond the expected fixture.
        for name in ORIENTATION_DECOMPOSER_FN_NAMES {
            assert!(
                EXPECTED_DECOMPOSER_NAMES.contains(name),
                "ORIENTATION_DECOMPOSER_FN_NAMES has unexpected entry {name:?} \
                 not in the fixture"
            );
        }
    }

    /// THE TWO-WAY LOCK that keeps the producer/decomposer split honest: no
    /// PRODUCER may be claimed by the decomposer predicate.
    ///
    /// Loops ALL of `ORIENTATION_TYPED_FN_NAMES` rather than spot-checking, so it
    /// is the exact reciprocal of `is_orientation_typed_fn_rejects_the_four_decomposers`
    /// above. Together the pair means a name added to the wrong half of this
    /// module is caught whichever half it is added to — the same regression-lock
    /// shape the `units.rs` disjointness matrix applies ACROSS families, applied
    /// here WITHIN one module.
    #[test]
    fn is_orientation_decomposer_fn_rejects_the_producers() {
        for name in ORIENTATION_TYPED_FN_NAMES {
            assert!(
                !is_orientation_decomposer_fn(name),
                "producer {name:?} must NOT be claimed by \
                 is_orientation_decomposer_fn — the two halves of this module \
                 are disjoint by construction"
            );
        }
    }

    /// `is_orientation_decomposer_fn` rejects `frame_at`, sibling-family names,
    /// the empty name, and unknown names.
    #[test]
    fn is_orientation_decomposer_fn_rejects_frame_at_and_other_families() {
        assert!(
            !is_orientation_decomposer_fn("frame_at"),
            "'frame_at' belongs to the datum family (datum_constructor_result_type \
             already types it Frame(3)) — claiming it here would double-classify"
        );
        assert!(
            !is_orientation_decomposer_fn("affine_from_transform"),
            "must reject 'affine_from_transform' — it CONSUMES a Transform and \
             produces an AffineMap, so it belongs to the affine family"
        );
        // `joint_jacobian` is the sharpest negative available: it already resolves
        // to `Type::StructureRef("Twist")` via the JOINT family
        // (joint_signatures.rs:189), the same nominal tag `transform_log` takes
        // below — so it is the one name a careless reader might file here.
        assert!(
            !is_orientation_decomposer_fn("joint_jacobian"),
            "must reject 'joint_jacobian' — it is already typed \
             StructureRef(\"Twist\") by the joint family; claiming it here would \
             double-classify the name across two resolvers"
        );
        assert!(
            !is_orientation_decomposer_fn("project"),
            "must reject 'project' — its first-arg fallback is already correct"
        );
        assert!(!is_orientation_decomposer_fn(""), "must reject empty name");
        assert!(
            !is_orientation_decomposer_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so
    /// capitalised forms must not match (mirrors
    /// `is_orientation_typed_fn_is_case_sensitive`).
    #[test]
    fn is_orientation_decomposer_fn_is_case_sensitive() {
        assert!(
            !is_orientation_decomposer_fn("Orient_Log"),
            "capitalised form must not match"
        );
        assert!(
            !is_orientation_decomposer_fn("ORIENT_LOG"),
            "upper-case form must not match"
        );
        assert!(
            !is_orientation_decomposer_fn("Transform_Log"),
            "capitalised form must not match"
        );
    }

    // ── Decomposer result-type resolution (step-3 RED / step-4 GREEN) ────────

    /// All 4 decomposer names, pinning their result types. The ruling BASIS for
    /// each lives on `orientation_decomposer_result_type`; this is the executable
    /// record of the mapping.
    #[test]
    fn orientation_decomposer_result_type_maps_each_name_to_its_result_type() {
        assert_eq!(
            orientation_decomposer_result_type("orient_log"),
            Type::vec3(Type::dimensionless_scalar()),
            "orient_log returns the rotation vector — Value::Vector of three \
             Value::Real (orientation.rs:228)"
        );
        assert_eq!(
            orientation_decomposer_result_type("orient_to_euler"),
            Type::List(Box::new(Type::angle())),
            "orient_to_euler returns Value::List of three Value::angle \
             (orientation.rs:438)"
        );
        assert_eq!(
            orientation_decomposer_result_type("orient_to_axis_angle"),
            Type::StructureRef("AxisAngle".to_string()),
            "orient_to_axis_angle returns Value::Map{{angle, axis}} \
             (orientation.rs:460-470)"
        );
        assert_eq!(
            orientation_decomposer_result_type("transform_log"),
            Type::StructureRef("Twist".to_string()),
            "transform_log returns the Value::Map{{angular, linear}} twist \
             (geometry.rs:860-873)"
        );
    }

    /// ACCEPTANCE PIN for the `orient_log` dimension ruling: the quantity slot is
    /// DIMENSIONLESS, explicitly NOT `Angle`.
    ///
    /// Two INDEPENDENT bases, either of which alone settles it:
    /// 1. **Runtime match.** `orientation.rs:228` builds
    ///    `Value::Vector(vec![Value::Real(lx), Value::Real(ly), Value::Real(lz)])`
    ///    on BOTH the general and the near-identity Taylor branch, and
    ///    `Value::Real` takes the `_ => DIMENSIONLESS` catch-all in
    ///    `Value::dimension()`. Typing this `Angle` would manufacture a fresh
    ///    static/runtime dimension DISAGREEMENT — the exact defect class the
    ///    producer family was created to remove.
    /// 2. **Precedent already ruled.** `reify_stdlib::geometry`'s
    ///    `const TWIST_ANGULAR_DIM: DimensionVector = DimensionVector::DIMENSIONLESS`
    ///    (geometry.rs:216) rules this identical ω rotation-vector quantity
    ///    dimensionless on the `transform_exp`/`transform_log` seam, and is a LIVE
    ///    eval gate — `transform_log`'s own tests assert the angular half is
    ///    DIMENSIONLESS. An `Angle` here would contradict a running gate.
    ///
    /// The components are semantically radians but are UNTAGGED at the value
    /// layer. Re-tagging them is a value-layer change (task 6080's seam), not a
    /// compiler-typing one; the compiler's job is to describe what eval actually
    /// produces.
    #[test]
    fn orient_log_quantity_slot_is_dimensionless_not_angle() {
        assert_eq!(
            orientation_decomposer_result_type("orient_log"),
            Type::vec3(Type::dimensionless_scalar()),
            "orient_log's rotation-vector components are Value::Real — DIMENSIONLESS"
        );
        assert_ne!(
            orientation_decomposer_result_type("orient_log"),
            Type::vec3(Type::angle()),
            "orient_log must NOT be typed vec3<Angle>: eval emits untagged \
             Value::Real, and TWIST_ANGULAR_DIM (geometry.rs:216) already rules \
             this same ω quantity DIMENSIONLESS on the live transform_exp gate"
        );
    }

    /// `orient_to_euler` is a `List<Angle>` — decisively NOT its first argument.
    ///
    /// The concrete defect: arg 0 is the convention SELECTOR, so under the
    /// terminal first-arg fallback the SAME call takes TWO different static types
    /// depending on how the selector is spelled — `Type::String` for
    /// `orient_to_euler("xyz", q)` and `Type::Enum("EulerConvention")` for
    /// `orient_to_euler(EulerConvention.XYZ, q)`. Resolving by NAME makes the
    /// result type independent of the spelling, which is also why this pin
    /// survives task 6082's flip to subject-first `orient_to_euler(q, convention)`
    /// unchanged: the answer never depended on the arguments.
    #[test]
    fn orient_to_euler_is_a_list_of_angles_not_its_first_arg() {
        assert_eq!(
            orientation_decomposer_result_type("orient_to_euler"),
            Type::List(Box::new(Type::angle())),
            "orient_to_euler returns three Angles as a Value::List"
        );
        assert_ne!(
            orientation_decomposer_result_type("orient_to_euler"),
            Type::String,
            "must NOT adopt the arg-0 convention SELECTOR's String type — the \
             mistyping that made one call site have two static types"
        );
        assert_ne!(
            orientation_decomposer_result_type("orient_to_euler"),
            Type::Enum("EulerConvention".to_string()),
            "nor the enum spelling of the same selector"
        );
    }

    /// The two `Value::Map` decomposers type as their NOMINAL structures, and
    /// decisively NOT as the first-argument types they silently adopt today.
    ///
    /// Those negatives are the whole point: post-5344 `orient_to_axis_angle(q)`
    /// inherits `Type::Orientation(3)` from `q` and `transform_log(t)` inherits
    /// `Type::Transform(3)` from `t` — both 1-arg, so the fallback never reaches
    /// its zero-arg warning and the mistyping is SILENT.
    #[test]
    fn map_decomposers_type_as_their_nominal_structure_refs() {
        assert_eq!(
            orientation_decomposer_result_type("orient_to_axis_angle"),
            Type::StructureRef("AxisAngle".to_string()),
        );
        assert_ne!(
            orientation_decomposer_result_type("orient_to_axis_angle"),
            Type::Orientation(3),
            "orient_to_axis_angle DECOMPOSES an Orientation into {{angle, axis}} — \
             it must not adopt its argument's Orientation(3) type"
        );
        assert_eq!(
            orientation_decomposer_result_type("transform_log"),
            Type::StructureRef("Twist".to_string()),
        );
        assert_ne!(
            orientation_decomposer_result_type("transform_log"),
            Type::Transform(3),
            "transform_log DECOMPOSES a Transform into a {{angular, linear}} \
             twist — it must not adopt its argument's Transform(3) type"
        );
    }

    /// Anti-fallthrough lock, mirroring
    /// `every_orientation_fn_name_maps_to_a_non_scalar_result_type` for the
    /// producer half: no decomposer name may resolve to a scalar. The resolver's
    /// `_` arm returns `Type::dimensionless_scalar()` and is meant to be
    /// unreachable (the caller gates on `is_orientation_decomposer_fn`), so a name
    /// that silently reached it would be INDISTINGUISHABLE from the old buggy
    /// fallback. This makes that failure loud.
    #[test]
    fn every_decomposer_name_maps_to_a_non_scalar_result_type() {
        for name in ORIENTATION_DECOMPOSER_FN_NAMES {
            assert!(
                !matches!(
                    orientation_decomposer_result_type(name),
                    Type::Scalar { .. }
                ),
                "{name} fell through to the unreachable `_` scalar arm — every \
                 family member must have an explicit match arm"
            );
        }
    }
}
