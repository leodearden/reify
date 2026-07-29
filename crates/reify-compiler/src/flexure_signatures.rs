//! Compiler signatures for the PRB flexure **constructor** builtins — the
//! placeholder-ratchet α family (task #5476).
//!
//! RED SKELETON (step-7): this file currently holds ONLY the test module. The
//! `FLEXURE_CTOR_FN_NAMES` slice, the `is_flexure_typed_fn` predicate and the
//! `flexure_ctor_result_type` resolver land in step-8, together with the
//! `mod flexure_signatures;` declaration in `lib.rs` that first brings this file
//! into the compilation. Until then these tests are not compiled; the RED that
//! actually fires is the `use crate::flexure_signatures::FLEXURE_CTOR_FN_NAMES;`
//! added to the `units.rs` test module in the same step.
//!
//! PRD: docs/prds/v0_6/placeholder-type-eradication-ratchet.md §3.2 / §7.1.

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent fixture — the 13 PRB flexure constructor names.
    ///
    /// Deliberately does NOT reference `FLEXURE_CTOR_FN_NAMES`, so a drift in
    /// that slice is caught against this independent list (mirrors
    /// `joint_signatures::tests::EXPECTED_NAMES`, "Independent fixture — list of
    /// all 17 expected names in the family").
    ///
    /// Transcribed from the runtime source of truth,
    /// `reify-stdlib/src/flexures/diagnostics.rs::is_flexure_ctor`. That list is
    /// DUPLICATED here rather than imported because `reify-compiler` does not
    /// depend on `reify-stdlib` (its Cargo.toml carries reify-core + reify-ir
    /// only) — exactly as `JOINT_TYPED_FN_NAMES` duplicates the runtime joint
    /// names. Drift is caught by paired independent-fixture tests on both sides,
    /// not by a cross-crate import.
    ///
    /// Grouping matches the per-family modules under
    /// `reify-stdlib/src/flexures/`: beam(2) / notch(3) / hinge(3) /
    /// prismatic(2) / compound(3).
    const EXPECTED_NAMES: [&str; 13] = [
        // beam.rs (2)
        "prb_cantilever_beam",
        "prb_fixed_fixed_beam",
        // notch.rs (3)
        "prb_notch_circular",
        "prb_notch_elliptical",
        "prb_notch_right_circular",
        // hinge.rs (3)
        "prb_living_hinge",
        "prb_cross_spring_pivot",
        "prb_let_joint",
        // prismatic.rs (2)
        "prb_prismatic_blade",
        "prb_two_axis_pivot",
        // compound.rs (3)
        "prb_parallelogram_flexure",
        "prb_double_parallelogram_flexure",
        "prb_cartwheel_flexure",
    ];

    // ── Name-family contract ─────────────────────────────────────────────────

    /// `FLEXURE_CTOR_FN_NAMES` is exactly the 13 expected names: correct count,
    /// every expected name present, and no extra entry.
    ///
    /// This is the compiler-side half of the anti-drift guard. The count is
    /// asserted explicitly at **13**: the PRD §3.2 and task #5476's description
    /// both say "all 14 `prb_*` ctors", but the range they cite
    /// (`flexures/diagnostics.rs:163-175`) holds 13 — and that file's own doc
    /// comment reads "The 13 PRB flexure constructor names". The repo's 14th
    /// `prb_*` identifier is `prb_validity_range`, which is a
    /// `FlexureCompliance` FIELD name (it appears in all five family modules and
    /// at flexures.ri:160), not a builtin. Typing it `FlexureJoint` would be
    /// wrong, so it is excluded — and pinned as excluded below.
    #[test]
    fn flexure_ctor_fn_names_match_independent_fixture() {
        assert_eq!(
            FLEXURE_CTOR_FN_NAMES.len(),
            13,
            "FLEXURE_CTOR_FN_NAMES must hold exactly 13 names (NOT the 14 the PRD \
             §3.2 states — `prb_validity_range` is a FlexureCompliance field, not \
             a ctor); got {:?}",
            FLEXURE_CTOR_FN_NAMES
        );
        assert_eq!(
            FLEXURE_CTOR_FN_NAMES.len(),
            EXPECTED_NAMES.len(),
            "FLEXURE_CTOR_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            FLEXURE_CTOR_FN_NAMES
        );
        // Every expected name is in the slice.
        for name in EXPECTED_NAMES {
            assert!(
                FLEXURE_CTOR_FN_NAMES.contains(&name),
                "FLEXURE_CTOR_FN_NAMES must contain {name:?}"
            );
        }
        // No extra name beyond the expected fixture.
        for name in FLEXURE_CTOR_FN_NAMES {
            assert!(
                EXPECTED_NAMES.contains(name),
                "FLEXURE_CTOR_FN_NAMES has unexpected entry {name:?} not in the fixture"
            );
        }
    }

    /// `is_flexure_typed_fn` recognises every expected PRB-ctor name.
    #[test]
    fn is_flexure_typed_fn_accepts_every_family_name() {
        for name in EXPECTED_NAMES {
            assert!(
                is_flexure_typed_fn(name),
                "is_flexure_typed_fn({name:?}) must be true (PRB flexure-ctor family)"
            );
        }
    }

    /// `is_flexure_typed_fn` rejects the two near-miss names that MUST NOT be
    /// typed `FlexureJoint`, plus sibling-family names, PascalCase forms, the
    /// empty name, and unknown names.
    ///
    /// The two near-misses are the load-bearing half of this test:
    ///
    /// - **`prb_validity_range`** — carries the `prb_` prefix and so would be
    ///   swept up by any prefix-matching implementation, but it is a
    ///   `FlexureCompliance` FIELD (flexures.ri:160; emitted by all five family
    ///   modules). Typing it `FlexureJoint` would corrupt the record's own
    ///   validity-range field. The family must match by exact name, never by
    ///   prefix.
    /// - **`__flexure_compliance_get`** — the accessor intrinsic. It CONSUMES a
    ///   `FlexureJoint` and returns a `FlexureCompliance`; it does not produce a
    ///   joint. `reify-stdlib`'s `flexure_diagnose` likewise intercepts it in a
    ///   dedicated arm placed BEFORE the `is_flexure_ctor` short-circuit.
    ///   (If step-14's contingency ever adds it to this module, it must return
    ///   `StructureRef("FlexureCompliance")` — a different type — so it still
    ///   does not belong in the ctor family and this assertion stands.)
    #[test]
    fn is_flexure_typed_fn_rejects_non_family_names() {
        assert!(
            !is_flexure_typed_fn("prb_validity_range"),
            "must reject 'prb_validity_range' — it is a FlexureCompliance FIELD \
             (flexures.ri:160), not a ctor; matching it would mean the family is \
             matching on the `prb_` PREFIX instead of exact names"
        );
        assert!(
            !is_flexure_typed_fn("__flexure_compliance_get"),
            "must reject '__flexure_compliance_get' — it is the accessor intrinsic \
             that CONSUMES a FlexureJoint and returns a FlexureCompliance, not a \
             ctor that produces one"
        );
        // Sibling families.
        assert!(
            !is_flexure_typed_fn("revolute"),
            "must reject joint-ctor 'revolute'"
        );
        assert!(
            !is_flexure_typed_fn("bind"),
            "must reject joint-ctor 'bind'"
        );
        assert!(
            !is_flexure_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        assert!(!is_flexure_typed_fn("vec"), "must reject math-linalg 'vec'");
        // Case-sensitivity: Reify function names are snake_case.
        assert!(
            !is_flexure_typed_fn("PRB_NOTCH_CIRCULAR"),
            "SCREAMING_CASE must not match"
        );
        assert!(
            !is_flexure_typed_fn("FlexureJoint"),
            "PascalCase type name must not match"
        );
        // Empty / unknown / bare prefix.
        assert!(!is_flexure_typed_fn(""), "must reject empty name");
        assert!(!is_flexure_typed_fn("prb_"), "must reject bare 'prb_' prefix");
        assert!(
            !is_flexure_typed_fn("prb_does_not_exist"),
            "must reject unknown prb_-prefixed name"
        );
    }

    // ── Result-type resolution ───────────────────────────────────────────────

    /// Every one of the 13 names maps to `Type::StructureRef("FlexureJoint")`.
    ///
    /// Unlike `joint_ctor_result_type` — whose Coupling arm is args-AWARE
    /// (`Type::applied("Coupling",[parent])`, task #4605 ε) — this family is
    /// both name- and argument-agnostic: all 13 ctors produce the one marker
    /// type. Asserted with `&[]` here and with a non-empty arg slice in
    /// `flexure_ctor_result_type_is_args_agnostic` below.
    #[test]
    fn flexure_ctor_result_type_is_flexure_joint_for_every_name() {
        for name in EXPECTED_NAMES {
            assert_eq!(
                flexure_ctor_result_type(name, &[]),
                Type::StructureRef("FlexureJoint".to_string()),
                "{name} must map to StructureRef(FlexureJoint); got {:?}",
                flexure_ctor_result_type(name, &[])
            );
        }
    }

    /// Args-agnostic invariant, pinned against a NON-Length first argument.
    ///
    /// This is what makes the ladder arm provably name-dispatched rather than
    /// coincidentally agreeing with the first-arg fallback it replaces: a dummy
    /// dimensionless `Real` arg must not change the result type away from
    /// `FlexureJoint`.
    #[test]
    fn flexure_ctor_result_type_is_args_agnostic() {
        use reify_ir::Value;

        let dummy_arg = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let args_slice = &[dummy_arg];

        for name in EXPECTED_NAMES {
            assert_eq!(
                flexure_ctor_result_type(name, args_slice),
                Type::StructureRef("FlexureJoint".to_string()),
                "{name} must return StructureRef(FlexureJoint) regardless of args — \
                 the arm dispatches on NAME, never on the first argument's type \
                 (that first-arg inference is the placeholder PRD §2 eradicates)"
            );
            assert_eq!(
                flexure_ctor_result_type(name, args_slice),
                flexure_ctor_result_type(name, &[]),
                "{name} result must be identical with and without args"
            );
        }
    }
}
