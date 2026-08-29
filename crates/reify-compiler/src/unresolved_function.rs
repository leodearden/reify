//! Closed-world membership oracle for **builtin function names**.
//!
//! # Why this module exists
//!
//! The `NoUserFunctions` arm of the `FunctionCall` ladder in
//! [`crate::expr`] ends in a *terminal first-arg fallback*: any callee that no
//! ladder arm claims is typed as its first argument's type (or
//! `Type::dimensionless_scalar()` when zero-arg). That fallback is
//! **open-world** — a genuinely nonexistent name such as
//! `definitely_not_a_reify_builtin_xyz(2.5mm)` compiles with ZERO diagnostics
//! and silently adopts `Scalar<LENGTH>`.
//!
//! This module supplies the missing complement: a single predicate,
//! [`is_known_builtin`], that answers "is this name known to the compiler at
//! all?" by unioning **every** classification family the ladder consults, plus
//! two explicit manifests declared here:
//!
//! * `FIRST_ARG_TYPED_NAMES` — names for which the terminal fallback's
//!   first-arg typing is *verified correct*, so they are named rather than
//!   left open-world.
//! * `EVAL_DEFERRED_BUILTIN_NAMES` — names that are eval-dispatchable but not
//!   yet family-registered, whose typing is deliberately left to the fallback.
//!
//! With the union in hand, `expr.rs` can emit a
//! `DiagnosticCode::UnresolvedFunction` **warning** at the fallback when the
//! callee is unknown, closing the open world without changing any typing.
//!
//! # Warn-mode-first posture
//!
//! Typing is **unchanged** by this module. Every call that compiled before
//! still compiles to the same type; the only new observable is a diagnostic.
//! That is deliberate (fail-closed warn-first): the corpus sweep must be green
//! before the code can become an error.
//!
//! # Downstream consumers
//!
//! * **#5997** flips `UnresolvedFunction` from Warning to Error behind a
//!   break-glass env knob. It names this module's manifest, allowlist and
//!   corpus sweep as its preconditions.
//! * **#6014** (builtin-signature-registry, task omega) DELETES the terminal
//!   first-arg fallback outright, and with it this module's
//!   `FIRST_ARG_TYPED_NAMES` family — once every name in it holds a real
//!   registry row, the allowlist has no remaining job. Its family-by-family
//!   migration is seeded by this task's warn-sweep violation list
//!   (`docs/notes/unresolved-function-warn-sweep-2026-08-29.md`).

use crate::analysis_signatures::ANALYSIS_FN_NAMES;
use crate::expr::DETERMINACY_PREDICATE_NAMES;
use crate::joint_signatures::JOINT_TYPED_FN_NAMES;
use crate::list_helpers::LIST_HELPER_NAMES;
use crate::math_signatures::{
    MATH_CONSTRUCTION_NAMES, MATH_OPERATION_NAMES, MATH_TRANSCENDENTAL_NAMES,
};
use crate::orientation_signatures::ORIENTATION_TYPED_FN_NAMES;
use crate::parse_signatures::PARSE_FN_NAMES;
use crate::relation_signatures::{RELATION_FN_NAMES, is_relation_shared_verb};
use crate::units::{
    AFFINE_ALGEBRA_NAMES, AFFINE_MAP_CONSTRUCTOR_NAMES, DATUM_CONSTRUCTOR_NAMES,
    DYNAMICS_CONSTRUCTOR_NAMES, DYNAMICS_QUERY_NAMES, FEA_ENVELOPE_NAMES, FIELD_OP_NAMES,
    GEOMETRY_FUNCTION_NAMES, GEOMETRY_KINEMATIC_QUERY_NAMES, GEOMETRY_QUERY_HELPER_NAMES,
    GEOMETRY_QUERY_NAMES, GEOMETRY_TOPOLOGY_SELECTOR_NAMES, SELECTOR_COMPOSITION_NAMES,
    TOLERANCING_MARKER_NAMES,
};

/// Names for which the terminal first-arg fallback's typing is **verified
/// correct**, so they are deliberately named rather than left open-world.
///
/// Populated in step-6; forward-declared empty here so `is_known_builtin` can
/// already reference it.
pub const FIRST_ARG_TYPED_NAMES: &[&str] = &[];

/// Eval-dispatchable names that are not yet family-registered, whose typing is
/// deliberately left to the terminal fallback.
///
/// Populated in step-8; forward-declared empty here so `is_known_builtin` can
/// already reference it.
pub const EVAL_DEFERRED_BUILTIN_NAMES: &[&str] = &[];

/// Is `name` a builtin function name the compiler knows about *at all*?
///
/// Closed-world union over every classification family the `expr.rs`
/// `NoUserFunctions` ladder consults, plus the two manifests declared in this
/// module. A pure predicate: no allocation, no diagnostics.
///
/// # What this does NOT answer
///
/// Membership is a **name** fact only. A `true` answer says nothing about
/// whether a *particular call* type-checks, has the right arity, or is claimed
/// by the family that owns the name — several families are arg-aware and
/// return `None` for a mis-shaped call by design (`datum_constructor_result_type`'s
/// `offset` arity gate, `selector_composition_result_type`'s CSG fall-through,
/// `infer_list_helper_return_type`'s structural match, `field_op_result_type`).
/// Those cases are diagnosed separately by `DiagnosticCode::BuiltinArgShapeUnrecognized`.
///
/// Case-sensitive — Reify function names are snake_case.
pub fn is_known_builtin(name: &str) -> bool {
    // --- The 19 name slices the ladder consults, in ladder order. ---
    GEOMETRY_QUERY_HELPER_NAMES.contains(&name)
        || GEOMETRY_KINEMATIC_QUERY_NAMES.contains(&name)
        || GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
        || RELATION_FN_NAMES.contains(&name)
        || GEOMETRY_QUERY_NAMES.contains(&name)
        || TOLERANCING_MARKER_NAMES.contains(&name)
        || GEOMETRY_FUNCTION_NAMES.contains(&name)
        || DYNAMICS_QUERY_NAMES.contains(&name)
        || DYNAMICS_CONSTRUCTOR_NAMES.contains(&name)
        || AFFINE_MAP_CONSTRUCTOR_NAMES.contains(&name)
        || MATH_CONSTRUCTION_NAMES.contains(&name)
        || MATH_OPERATION_NAMES.contains(&name)
        || MATH_TRANSCENDENTAL_NAMES.contains(&name)
        || JOINT_TYPED_FN_NAMES.contains(&name)
        || ANALYSIS_FN_NAMES.contains(&name)
        || FEA_ENVELOPE_NAMES.contains(&name)
        || FIELD_OP_NAMES.contains(&name)
        || PARSE_FN_NAMES.contains(&name)
        || ORIENTATION_TYPED_FN_NAMES.contains(&name)
        // --- The four resolver-only families, promoted to production slices
        // --- by this task so the union can see them (they were previously
        // --- visible only as `match` arms inside their resolvers).
        || DATUM_CONSTRUCTOR_NAMES.contains(&name)
        || SELECTOR_COMPOSITION_NAMES.contains(&name)
        || LIST_HELPER_NAMES.contains(&name)
        || AFFINE_ALGEBRA_NAMES.contains(&name)
        // --- Vocabularies that live outside any slice. ---
        //
        // The arity-gated shared verbs `angle`/`distance` are deliberately
        // absent from RELATION_FN_NAMES (their arity-2 DERIVE forms are
        // geometry queries), so the slices above do not reach them.
        || is_relation_shared_verb(name)
        // The determinacy predicates are a bare `match` in the ladder; #5371
        // promoted them to a slice for exactly this reason.
        || DETERMINACY_PREDICATE_NAMES.contains(&name)
        // --- This module's two manifests. ---
        || FIRST_ARG_TYPED_NAMES.contains(&name)
        || EVAL_DEFERRED_BUILTIN_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_signatures::ANALYSIS_FN_NAMES;
    use crate::joint_signatures::JOINT_TYPED_FN_NAMES;
    use crate::math_signatures::{
        MATH_CONSTRUCTION_NAMES, MATH_OPERATION_NAMES, MATH_TRANSCENDENTAL_NAMES,
    };
    use crate::orientation_signatures::ORIENTATION_TYPED_FN_NAMES;
    use crate::parse_signatures::PARSE_FN_NAMES;
    use crate::relation_signatures::RELATION_FN_NAMES;
    use crate::units::{
        AFFINE_MAP_CONSTRUCTOR_NAMES, DYNAMICS_CONSTRUCTOR_NAMES, DYNAMICS_QUERY_NAMES,
        FEA_ENVELOPE_NAMES, FIELD_OP_NAMES, GEOMETRY_FUNCTION_NAMES,
        GEOMETRY_KINEMATIC_QUERY_NAMES, GEOMETRY_QUERY_HELPER_NAMES, GEOMETRY_QUERY_NAMES,
        GEOMETRY_TOPOLOGY_SELECTOR_NAMES, TOLERANCING_MARKER_NAMES,
    };

    /// Every name slice the `NoUserFunctions` ladder consults, paired with its
    /// identifier so a failure names the family that regressed.
    ///
    /// Nineteen families; each `*_are_disjoint_from_other_families` test in
    /// `units.rs` loops the other **18** (it excludes its own).
    const ALL_FAMILY_SLICES: &[(&str, &[&str])] = &[
        ("GEOMETRY_FUNCTION_NAMES", GEOMETRY_FUNCTION_NAMES),
        ("GEOMETRY_QUERY_HELPER_NAMES", GEOMETRY_QUERY_HELPER_NAMES),
        (
            "GEOMETRY_KINEMATIC_QUERY_NAMES",
            GEOMETRY_KINEMATIC_QUERY_NAMES,
        ),
        (
            "GEOMETRY_TOPOLOGY_SELECTOR_NAMES",
            GEOMETRY_TOPOLOGY_SELECTOR_NAMES,
        ),
        ("GEOMETRY_QUERY_NAMES", GEOMETRY_QUERY_NAMES),
        ("AFFINE_MAP_CONSTRUCTOR_NAMES", AFFINE_MAP_CONSTRUCTOR_NAMES),
        ("TOLERANCING_MARKER_NAMES", TOLERANCING_MARKER_NAMES),
        ("DYNAMICS_QUERY_NAMES", DYNAMICS_QUERY_NAMES),
        ("DYNAMICS_CONSTRUCTOR_NAMES", DYNAMICS_CONSTRUCTOR_NAMES),
        ("FEA_ENVELOPE_NAMES", FEA_ENVELOPE_NAMES),
        ("FIELD_OP_NAMES", FIELD_OP_NAMES),
        ("MATH_CONSTRUCTION_NAMES", MATH_CONSTRUCTION_NAMES),
        ("MATH_OPERATION_NAMES", MATH_OPERATION_NAMES),
        ("MATH_TRANSCENDENTAL_NAMES", MATH_TRANSCENDENTAL_NAMES),
        ("ANALYSIS_FN_NAMES", ANALYSIS_FN_NAMES),
        ("RELATION_FN_NAMES", RELATION_FN_NAMES),
        ("JOINT_TYPED_FN_NAMES", JOINT_TYPED_FN_NAMES),
        ("PARSE_FN_NAMES", PARSE_FN_NAMES),
        ("ORIENTATION_TYPED_FN_NAMES", ORIENTATION_TYPED_FN_NAMES),
    ];

    /// `is_known_builtin` must accept EVERY member of EVERY classification
    /// family the `expr.rs` ladder consults — not a spot-check per family.
    ///
    /// Iterating each slice in full is what stops the oracle rotting: a name
    /// added to any family slice is covered the moment it lands, with no
    /// parallel edit here.
    ///
    /// The four **resolver-only** families (datum-constructor, affine-map
    /// algebra, list-helper, selector-composition) have no production slice to
    /// iterate at this point in the task, so they are covered by representative
    /// names; step-2 promotes real slices for them and step-3 pins those
    /// slices against their resolvers.
    #[test]
    fn is_known_builtin_recognises_every_compiler_family() {
        for (family, slice) in ALL_FAMILY_SLICES {
            for name in *slice {
                assert!(
                    is_known_builtin(name),
                    "{name:?} is in {family} but is_known_builtin rejects it"
                );
            }
        }

        // Resolver-only families — no production name slice exists yet.
        for name in [
            // datum-constructor (units.rs `datum_constructor_result_type`)
            "frame_at",
            "midplane",
            "plane_through",
            "axis_through",
            "plane_xy",
            "axis_x",
            // affine-map algebra (units.rs `affine_map_algebra_result_type`);
            // `determinant` / `affine_apply` are omitted here because they are
            // already claimed by MATH_OPERATION_NAMES / GEOMETRY_FUNCTION_NAMES.
            "affine_compose",
            "affine_inverse",
            // list-helper (list_helpers.rs `infer_list_helper_return_type`)
            "single",
            "flat_map",
            "generate",
            // selector composition (units.rs `selector_composition_result_type`);
            // `union` / `difference` are also CSG geometry functions, `intersect`
            // is selector-only.
            "union",
            "intersect",
            "difference",
        ] {
            assert!(
                is_known_builtin(name),
                "{name:?} is claimed by a resolver-only family but \
                 is_known_builtin rejects it"
            );
        }

        // Arity-gated shared verbs — deliberately absent from RELATION_FN_NAMES
        // (their arity-2 DERIVE forms are geometry queries), so the slice loop
        // above does not reach them via that family.
        for name in ["angle", "distance"] {
            assert!(
                crate::relation_signatures::is_relation_shared_verb(name),
                "premise guard: {name:?} is no longer a relation shared verb"
            );
            assert!(
                is_known_builtin(name),
                "{name:?} is a relation shared verb but is_known_builtin rejects it"
            );
        }

        // Determinacy predicates — hard-coded in the `expr.rs` ladder as a bare
        // `match`, with no slice anywhere.
        for name in [
            "determined",
            "undetermined",
            "constrained",
            "partially_determined",
        ] {
            assert!(
                is_known_builtin(name),
                "{name:?} is a determinacy predicate but is_known_builtin rejects it"
            );
        }
    }


    /// A hand-maintained name slice is only as good as its tie to the resolver
    /// it claims to describe. Step-2 gated each of the four resolver-only
    /// families ON its slice (a name cannot be in the `match` without being in
    /// the slice); this test pins the CONVERSE direction — every slice entry is
    /// really claimed by its resolver for a well-shaped call — so a stale entry
    /// cannot linger after the resolver arm is removed.
    ///
    /// The premise-guard idiom is copied from
    /// `units::tests::datum_constructor_names_are_disjoint_from_other_families`,
    /// which asserts `datum_constructor_result_type(name, &[]).is_some()`
    /// before its absence asserts for the same reason.
    #[test]
    fn resolver_only_family_slices_match_their_resolvers() {
        use reify_core::Type;
        use reify_core::ty::SelectorKind;
        use reify_ir::{CompiledExpr, Value};

        fn arg(ty: Type) -> CompiledExpr {
            CompiledExpr::literal(Value::Undef, ty)
        }

        // ---- Construction-datum constructors -----------------------------
        //
        // Arity 2 satisfies `offset`'s arity gate (units.rs); the other ten
        // members are arity-blind, so one arg vector serves all eleven.
        let datum_args = vec![arg(Type::Plane), arg(Type::length())];
        for name in DATUM_CONSTRUCTOR_NAMES {
            assert!(
                crate::units::datum_constructor_result_type(name, &datum_args).is_some(),
                "DATUM_CONSTRUCTOR_NAMES entry {name:?} is not claimed by \
                 datum_constructor_result_type at arity 2"
            );
        }
        assert_eq!(
            crate::units::datum_constructor_result_type("not_a_datum_ctor", &datum_args),
            None,
            "converse: a non-member must not be claimed"
        );
        // `offset` really is the arity-gated member — pin the gate so the
        // arity-2 fixture above is not silently testing an arity-blind name.
        assert_eq!(
            crate::units::datum_constructor_result_type("offset", &[]),
            None,
            "offset is a construction datum at arity 2 ONLY (arity 3 is a relation)"
        );

        // ---- AffineMap algebra -------------------------------------------
        //
        // Two members are first-arg-gated, so each name needs its OWN
        // well-shaped first arg: `affine_apply` wants a Point, the rest want an
        // AffineMap. A single shared fixture would silently under-test them.
        for name in AFFINE_ALGEBRA_NAMES {
            let first_arg = if *name == "affine_apply" {
                Type::point3(Type::length())
            } else {
                Type::AffineMap(3)
            };
            assert!(
                crate::units::affine_map_algebra_result_type(name, Some(&first_arg)).is_some(),
                "AFFINE_ALGEBRA_NAMES entry {name:?} is not claimed by \
                 affine_map_algebra_result_type for a well-shaped first arg"
            );
        }
        assert_eq!(
            crate::units::affine_map_algebra_result_type(
                "not_an_affine_op",
                Some(&Type::AffineMap(3))
            ),
            None,
            "converse: a non-member must not be claimed"
        );

        // ---- List helpers -------------------------------------------------
        //
        // Each helper has a different well-shaped arg vector; `generate` is the
        // entry the older test-only fixtures omitted, so it is exactly the drift
        // this loop catches.
        let list_of_int = Type::List(Box::new(Type::Int));
        let lambda_to_list = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::List(Box::new(Type::Bool))),
        };
        let lambda_to_int = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        };
        for name in LIST_HELPER_NAMES {
            let args = match *name {
                "single" => vec![arg(list_of_int.clone())],
                "flat_map" => vec![arg(list_of_int.clone()), arg(lambda_to_list.clone())],
                "generate" => vec![arg(Type::Int), arg(lambda_to_int.clone())],
                other => panic!(
                    "LIST_HELPER_NAMES gained {other:?} with no well-shaped arg \
                     fixture here — add one so the entry is really covered"
                ),
            };
            assert!(
                crate::list_helpers::infer_list_helper_return_type(name, &args).is_some(),
                "LIST_HELPER_NAMES entry {name:?} is not claimed by \
                 infer_list_helper_return_type for a well-shaped call"
            );
        }
        assert_eq!(
            crate::list_helpers::infer_list_helper_return_type("take", &[arg(list_of_int.clone())]),
            None,
            "converse: a non-member must not be claimed"
        );

        // ---- Selector composition -----------------------------------------
        //
        // Operand-shaped, not name-shaped: two Selector-typed operands satisfy
        // both the variadic union/intersect and the strictly-binary difference.
        let selector_args = vec![
            arg(Type::Selector(SelectorKind::Face)),
            arg(Type::Selector(SelectorKind::Face)),
        ];
        for name in SELECTOR_COMPOSITION_NAMES {
            let mut diags = Vec::new();
            let resolved = crate::units::selector_composition_result_type(
                name,
                &selector_args,
                reify_core::SourceSpan::new(0, 0),
                &mut diags,
            );
            assert!(
                resolved.is_some(),
                "SELECTOR_COMPOSITION_NAMES entry {name:?} is not claimed by \
                 selector_composition_result_type for two Selector operands"
            );
            assert!(
                diags.is_empty(),
                "well-shaped {name:?} composition should emit no diagnostics, got {diags:?}"
            );
        }
        let mut diags = Vec::new();
        assert_eq!(
            crate::units::selector_composition_result_type(
                "not_a_selector_op",
                &selector_args,
                reify_core::SourceSpan::new(0, 0),
                &mut diags,
            ),
            None,
            "converse: a non-member must not be claimed"
        );
    }


    /// The allowlist is pinned against an INDEPENDENT literal, not against the
    /// slice itself, so drift in EITHER direction fails — mirroring the
    /// `EXPECTED_NAMES` idiom in `orientation_signatures.rs`. A test that read
    /// `FIRST_ARG_TYPED_NAMES` back would pass for any content at all.
    ///
    /// Each of the eight was verified against its EVAL BODY, not inferred from
    /// its name; the per-name evidence table lives on the slice's doc comment.
    #[test]
    fn first_arg_typed_names_are_exactly_the_eight_verified_names() {
        const EXPECTED_NAMES: &[&str] = &[
            "project",
            "mod",
            "to_global",
            "effective_tolerance_zone",
            "input_shape_apply",
            "complex_add",
            "complex_exp",
            "complex_sqrt",
        ];
        assert_eq!(
            FIRST_ARG_TYPED_NAMES, EXPECTED_NAMES,
            "FIRST_ARG_TYPED_NAMES drifted. Every entry asserts the terminal \
             first-arg fallback types that name CORRECTLY — a claim that must \
             be re-verified against the eval body before a name is added, and \
             the doc comment's evidence table updated with it."
        );
    }

    /// The two ways an entry could be WRONG, pinned as explicit negatives with
    /// their reasons. Both were live candidates: the task's own brief listed
    /// all eight of these names among "16 FALLBACK-CORRECT" callees.
    #[test]
    fn first_arg_typed_names_exclude_the_already_claimed_and_the_dimension_transforming() {
        // (1) Already claimed by ORIENTATION_TYPED_FN_NAMES (#5344). These are
        // not fallback-correct or fallback-incorrect — the fallback never sees
        // them, because the orientation arm claims them first. Listing one
        // here would break that family's disjointness contract AND make an
        // unfalsifiable claim about dead code.
        for name in [
            "transform_inverse",
            "transform_compose",
            "orient_inverse",
            "orient_compose",
            "orient_slerp",
        ] {
            assert!(
                ORIENTATION_TYPED_FN_NAMES.contains(&name),
                "premise guard: {name:?} is no longer claimed by \
                 ORIENTATION_TYPED_FN_NAMES, so the exclusion below has lost \
                 its reason — re-derive before editing the allowlist"
            );
            assert!(
                !FIRST_ARG_TYPED_NAMES.contains(&name),
                "{name:?} must NOT be in FIRST_ARG_TYPED_NAMES — \
                 ORIENTATION_TYPED_FN_NAMES already claims it (#5344)"
            );
        }

        // (2) Dimension-TRANSFORMING, so first-arg typing is a KNOWN-FALSE
        // claim, not merely an unverified one:
        //   complex_mul  complex.rs:138  dimension = ad.mul(bd)
        //   complex_div  complex.rs:160  dimension = ad.div(bd)
        //   complex_pow  complex.rs:191  accumulates dim^n; n=0 -> DIMENSIONLESS
        // Each is wrong whenever the second operand is dimensioned (or n != 1).
        // They belong in EVAL_DEFERRED_BUILTIN_NAMES, whose claim is only
        // "eval-dispatchable, not yet family-registered" — unconditionally
        // true, and it suppresses the warning identically. That membership is
        // pinned by `eval_deferred_manifest_contains_the_known_deferred_names`.
        for name in ["complex_mul", "complex_div", "complex_pow"] {
            assert!(
                !FIRST_ARG_TYPED_NAMES.contains(&name),
                "{name:?} must NOT be in FIRST_ARG_TYPED_NAMES — it is \
                 dimension-transforming, so first-arg typing is wrong for it \
                 whenever the second operand is dimensioned"
            );
        }
    }

    /// The allowlist is one of the unions inside `is_known_builtin`, so every
    /// member must be visible to the oracle. Cheap, but it is what makes the
    /// allowlist actually SUPPRESS the `UnresolvedFunction` warning rather
    /// than merely document an intention.
    #[test]
    fn first_arg_typed_names_are_all_known_builtins() {
        for name in FIRST_ARG_TYPED_NAMES {
            assert!(
                is_known_builtin(name),
                "FIRST_ARG_TYPED_NAMES entry {name:?} is not accepted by \
                 is_known_builtin — the family is not wired into the union"
            );
        }
    }

    /// The closed world must actually be closed: a name in no family at all is
    /// rejected. Without this the oracle could trivially satisfy the test above
    /// by returning `true` unconditionally.
    #[test]
    fn is_known_builtin_rejects_a_genuinely_nonexistent_name() {
        assert!(!is_known_builtin("definitely_not_a_reify_builtin_xyz"));
        assert!(!is_known_builtin("line_of_nonsense"));
    }
}
