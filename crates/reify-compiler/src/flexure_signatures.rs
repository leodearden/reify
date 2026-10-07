//! Compiler signatures for the PRB flexure **constructor** builtins — the
//! placeholder-ratchet α family (task #5476).
//!
//! Holds the compiler-side source of truth for the PRB flexure-constructor name
//! family ([`FLEXURE_CTOR_FN_NAMES`]), the name-only classification predicate
//! ([`is_flexure_typed_fn`]), and the family's single nominal result type
//! ([`flexure_joint_type`]).
//!
//! All 13 ctors map to the single nominal marker `StructureRef("FlexureJoint")`
//! (declared `structure def FlexureJoint : DrivingJoint { }` in
//! `stdlib/flexures.ri`). That nominal result is what lets
//! `flexure_compliance(joint: FlexureJoint)` match a real ctor result while
//! REJECTING every bare literal through the compiler's exact-equality overload
//! filter (`type_compat.rs::resolve_function_overload`).
//!
//! PRD: docs/prds/v0_6/placeholder-type-eradication-ratchet.md §3.2 / §7.1.
//!
//! ## What this replaces
//!
//! Without a signature arm, a `prb_*` call falls through the `NoUserFunctions`
//! ladder to the FIRST-ARG FALLBACK, which types the call as its first
//! geometric argument's type — e.g. `prb_notch_circular(1mm, …)` types as
//! `Scalar[LENGTH]` from `notch_radius`. That is precisely the placeholder PRD
//! §2 names as the root cause: it made a bare `5mm` statically
//! indistinguishable from a real flexure joint.
//!
//! ## StructureRef cell-typing safety (esc-3845-91)
//!
//! The PRB ctors evaluate to a concrete `Value::Map` at runtime, exactly like
//! the joint builtins, and α does NOT change that — PRD §7.1 keeps the
//! joints→typed-StructureInstance migration rejected. Assigning
//! `Type::StructureRef` to the cell is nonetheless safe, by the same argument
//! `joint_signatures` records:
//! - `assert_value_cell_types_representable` (the debug-only invariant that runs
//!   in normal eval) explicitly PERMITS `Type::StructureRef`.
//! - `value_type_kind_matches` is invoked ONLY on the param-override/admin-edit
//!   paths, NOT on the `Engine::eval` cold-start for let-cells.
//! - Decisive: these let-cells ALREADY carry a first-arg-fallback type
//!   (`Scalar[LENGTH]`) while eval stores a `Value::Map` — a mismatch that
//!   exists today and that flexure eval tests pass with — so `Scalar[LENGTH]` →
//!   `StructureRef` is strictly more correct, not a new class of divergence.
//!
//! ## Name-list duplication (deliberate)
//!
//! [`FLEXURE_CTOR_FN_NAMES`] duplicates
//! `reify-stdlib/src/flexures/diagnostics.rs::is_flexure_ctor`. `reify-compiler`
//! depends only on reify-core + reify-ir — NOT on reify-stdlib — so sharing the
//! list would require a cross-crate dependency in the wrong direction or a
//! widening of this crate's public API. `JOINT_TYPED_FN_NAMES` sets the
//! precedent by restating the runtime joint names the same way.
//!
//! Drift is caught by PAIRED independent-fixture tests — one here
//! (`flexure_ctor_fn_names_match_independent_fixture`), one in reify-stdlib
//! (`is_flexure_ctor_matches_independent_fixture`) — and BOTH assert
//! SET-EQUALITY against their fixture: every fixture name present AND no entry
//! beyond it. The no-extras half on each side is what closes the drift loop.
//! Without it a list could GROW silently, and a compiler-side list missing a
//! runtime ctor is exactly how a legitimate `flexure_compliance(prb_new(...))`
//! would start failing with `no matching overload`. (The reify-stdlib side is
//! only able to assert that direction because its `is_flexure_ctor` is backed
//! by an enumerable `PRB_CTOR_NAMES` slice rather than a `matches!` arm.)
//!
//! Wired into `expr.rs`'s `NoUserFunctions` ladder after the `is_joint_typed_fn`
//! arm. The family is pinned disjoint from all sibling families by the `units.rs`
//! disjointness test, which is what makes that arm position unobservable.
//!
//! Registry τ4 (#6006, pending) migrates this family into `reify-builtins` rows
//! and DELETES this slice and its `is_known_builtin` arm, per
//! `unresolved_function.rs`'s "registry arm is where the rest of this union is
//! going" contract — it must not leave both the rows and this slice.

use reify_core::Type;

/// The complete set of PRB flexure-constructor builtin names recognised by the
/// compiler. Single source of truth compiler-side — imported into the `units.rs`
/// test module to pin disjointness from all sibling families.
///
/// **13 names**, grouped by the per-family modules under
/// `reify-stdlib/src/flexures/`. Every one maps to the same nominal type,
/// `StructureRef("FlexureJoint")`.
///
/// NOTE: `prb_validity_range` is deliberately EXCLUDED despite the `prb_`
/// prefix — it is a `FlexureCompliance` FIELD (emitted by all five family
/// modules; declared as the `prb_validity_range` param of `structure def
/// FlexureCompliance` in flexures.ri), not a constructor. This is also why
/// the family is 13 and not the 14 stated in PRD §3.2. `__flexure_compliance_get`
/// is likewise excluded: it CONSUMES a joint and returns a `FlexureCompliance`.
/// Both exclusions are pinned by `is_flexure_typed_fn_rejects_non_family_names`.
///
/// Case-sensitive: Reify function names are snake_case.
pub(crate) const FLEXURE_CTOR_FN_NAMES: &[&str] = &[
    // Beam flexures (2) — beam.rs
    "prb_cantilever_beam",
    "prb_fixed_fixed_beam",
    // Notch hinges (3) — notch.rs
    "prb_notch_circular",
    "prb_notch_elliptical",
    "prb_notch_right_circular",
    // Hinge / pivot flexures (3) — hinge.rs
    "prb_living_hinge",
    "prb_cross_spring_pivot",
    "prb_let_joint",
    // Prismatic flexures (2) — prismatic.rs
    "prb_prismatic_blade",
    "prb_two_axis_pivot",
    // Compound flexures (3) — compound.rs
    "prb_parallelogram_flexure",
    "prb_double_parallelogram_flexure",
    "prb_cartwheel_flexure",
];

/// Is `name` a PRB flexure-constructor builtin the compiler types as
/// [`flexure_joint_type`]? Name-only classification — a `.contains` over the
/// single-source-of-truth slice [`FLEXURE_CTOR_FN_NAMES`].
///
/// Matches on the EXACT name, never on the `prb_` prefix: `prb_validity_range`
/// shares that prefix but is a record field, not a ctor.
pub(crate) fn is_flexure_typed_fn(name: &str) -> bool {
    FLEXURE_CTOR_FN_NAMES.contains(&name)
}

/// The nominal marker type every PRB flexure constructor resolves to:
/// `Type::StructureRef("FlexureJoint")`.
///
/// **Zero-arg on purpose.** Unlike the sibling [`crate::joint_ctor_result_type`],
/// whose Coupling arm is args-AWARE (`Type::applied("Coupling",[parent])`, task
/// #4605 ε), this family has nothing to dispatch on: all 13 ctors produce the
/// one marker type. Taking `(name, args)` for shape-uniformity with the joint
/// family would only mislead — a reader of the `expr.rs` ladder arm would have
/// to come here to learn that neither parameter is consulted. Dropping them
/// makes "the arm dispatches by NAME (via [`is_flexure_typed_fn`]), never by
/// first-argument type" structurally true instead of test-asserted.
///
/// Runtime values stay `Value::Map` (esc-3845-91); the cell TYPE is the nominal
/// tag. See the module doc for the full safety argument.
pub(crate) fn flexure_joint_type() -> Type {
    Type::StructureRef("FlexureJoint".to_string())
}

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
    /// (`flexures/diagnostics.rs::PRB_CTOR_NAMES`) holds 13 — and that file's own doc
    /// comment reads "The 13 PRB flexure constructor names". The repo's 14th
    /// `prb_*` identifier is `prb_validity_range`, which is a
    /// `FlexureCompliance` FIELD name (it appears in all five family modules and
    /// as the `prb_validity_range` param of `structure def FlexureCompliance`
    /// in flexures.ri), not a builtin. Typing it `FlexureJoint` would be
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
    ///   `FlexureCompliance` FIELD (the `prb_validity_range` param of
    ///   `structure def FlexureCompliance` in flexures.ri; emitted by all five family
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
             (the `prb_validity_range` param of `structure def \
             FlexureCompliance`), not a ctor; matching it would mean the family is \
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
        assert!(
            !is_flexure_typed_fn("prb_"),
            "must reject bare 'prb_' prefix"
        );
        assert!(
            !is_flexure_typed_fn("prb_does_not_exist"),
            "must reject unknown prb_-prefixed name"
        );
    }

    // ── Result-type resolution ───────────────────────────────────────────────

    /// `flexure_joint_type()` is exactly `Type::StructureRef("FlexureJoint")`.
    ///
    /// The load-bearing content is the SPELLING: the string must match the
    /// `structure def FlexureJoint` declared in `stdlib/flexures.ri` verbatim,
    /// because the compiler's overload filter
    /// (`type_compat.rs::resolve_function_overload`) compares nominal types by
    /// exact equality. A typo here would not fail to compile — it would type
    /// every `prb_*` call as a reference to a structure that does not exist,
    /// and `flexure_compliance` would reject every real flexure joint.
    ///
    /// The former "args-agnostic" and "same for every name" invariants are no
    /// longer asserted here because they are no longer assertable: the function
    /// takes neither a name nor an arg slice, so name-/arg-independence is a
    /// property of the signature rather than of the body.
    #[test]
    fn flexure_joint_type_is_the_nominal_marker() {
        assert_eq!(
            flexure_joint_type(),
            Type::StructureRef("FlexureJoint".to_string()),
            "the PRB ctor family's result type must be the nominal marker declared \
             as `structure def FlexureJoint : DrivingJoint {{ }}` in \
             stdlib/flexures.ri, spelled identically; got {:?}",
            flexure_joint_type()
        );
    }
}
