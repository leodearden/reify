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

/// Is `name` a builtin function name the compiler knows about *at all*?
///
/// Closed-world union over every classification family plus the two manifests
/// declared in this module. A pure predicate: no allocation, no diagnostics.
///
/// Case-sensitive — Reify function names are snake_case.
pub fn is_known_builtin(_name: &str) -> bool {
    false
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

    /// The closed world must actually be closed: a name in no family at all is
    /// rejected. Without this the oracle could trivially satisfy the test above
    /// by returning `true` unconditionally.
    #[test]
    fn is_known_builtin_rejects_a_genuinely_nonexistent_name() {
        assert!(!is_known_builtin("definitely_not_a_reify_builtin_xyz"));
        assert!(!is_known_builtin("line_of_nonsense"));
    }
}
