//! Compiler signatures for the §13 mechanism/joint **constructor** builtins
//! (mechanism β, task 4311) — the frozen §13 constructor contract.
//!
//! Holds the single source of truth for the joint-constructor builtin name
//! family ([`JOINT_TYPED_FN_NAMES`]), the name-only classification predicate
//! ([`is_joint_typed_fn`]), and the name→nominal-type resolver
//! ([`joint_ctor_result_type`]).
//!
//! Unlike the math-linalg family, joint constructors all map to a FIXED nominal
//! `Type::StructureRef(...)` independent of their arguments — the §13 tags are
//! enforced return types consumed by γ's compile-time `DrivingJoint`-bound check
//! and by `reify-lsp` hover. PRD: docs/prds/v0_6/mechanism-completion.md task β
//! (§9, D8).
//!
//! ## StructureRef cell-typing safety (esc-3845-91)
//!
//! The joint builtins evaluate to concrete `Value::Map`/`Int`/`List` at runtime
//! (NOT `Value::Undef` like dynamics/geometry). Assigning `Type::StructureRef`
//! is nonetheless safe because:
//! - `assert_value_cell_types_representable` (engine_eval.rs:144, the debug-only
//!   invariant that runs in normal eval) explicitly PERMITS `Type::StructureRef`.
//! - `value_type_kind_matches` (lib.rs:215) is invoked ONLY on the
//!   param-override/admin-edit paths, NOT on the `Engine::eval` cold-start for
//!   let-cells.
//! - Decisive: today these joint let-cells already carry the first-arg-fallback
//!   type (e.g. `Real`) while eval stores a `Value::Map` — a mismatch that
//!   already exists yet mechanism eval tests pass — so changing `Real` →
//!   `StructureRef` only improves hover/typing.
//!
//! ## Dynamics precedent
//!
//! Mirrors the established `is_dynamics_query ⇒ Type::StructureRef("MassProperties")`
//! arm (expr.rs:1718-1730, task 3829) — this is the third stdlib constructor-
//! signature family (after math and dynamics-query).
//!
//! Wired into `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder
//! after the `is_math_typed_fn` arm (~line 1793). The family is pinned disjoint
//! from all sibling families by the `units.rs` disjointness test.

use reify_core::Type;
use reify_ir::CompiledExpr;

/// The complete set of §13 mechanism/joint constructor builtin names recognised
/// by the compiler. Single source of truth — imported into the `units.rs` test
/// module to pin disjointness from all sibling families.
///
/// **17 names** grouped by target nominal type:
/// - **Driving joint kinds** (5): `prismatic`, `revolute`, `cylindrical`,
///   `planar`, `spherical` → `Prismatic`/`Revolute`/`Cylindrical`/`Planar`/
///   `Spherical`.
/// - **Coupling** (4): `couple`, `gear`, `screw`, `rack_and_pinion` → `Coupling`.
/// - **Fixed** (1): `fixed` → `Fixed`.
/// - **Mechanism/body** (2): `mechanism`, `body` → `Mechanism`.
/// - **Snapshot** (1): `snapshot` → `Snapshot`.
/// - **BodyId** (1): `body_id_of` → `BodyId`.
/// - **SweepDim** (1): `dim` → `SweepDim`.
/// - **JointBinding** (1): `bind` → `JointBinding`.
/// - **Twist** (1): `joint_jacobian` → `Twist`.
///
/// NOTE: `sweep` is deliberately EXCLUDED — it has a geometry (arity-2 CSG)
/// overload that must keep its geometry result type; γ (task 4310) already
/// handles sweep's arity-4 kinematic conformance check separately.
///
/// Case-sensitive: Reify function names are snake_case.
///
/// Single source of truth — imported into the `units.rs` test module to pin
/// disjointness from all sibling families (mirrors `MATH_CONSTRUCTION_NAMES`).
pub const JOINT_TYPED_FN_NAMES: &[&str] = &[
    // Driving joint kind constructors (5): → Prismatic/Revolute/Cylindrical/Planar/Spherical
    "prismatic",
    "revolute",
    "cylindrical",
    "planar",
    "spherical",
    // Coupling constructors (4): couple/gear/screw/rack_and_pinion → Coupling
    "couple",
    "gear",
    "screw",
    "rack_and_pinion",
    // Fixed joint (1): → Fixed
    "fixed",
    // Mechanism/body constructors (2): mechanism/body → Mechanism
    "mechanism",
    "body",
    // Snapshot constructor (1): → Snapshot
    "snapshot",
    // Body-ID accessor (1): → BodyId
    "body_id_of",
    // Sweep dimension (1): → SweepDim
    "dim",
    // Joint binding (1): → JointBinding
    "bind",
    // Joint Jacobian / Twist (1): → Twist
    "joint_jacobian",
];

/// Is `name` a §13 joint-constructor builtin the compiler types via
/// [`joint_ctor_result_type`]? Name-only classification — a `.contains` over
/// the single-source-of-truth slice [`JOINT_TYPED_FN_NAMES`]. Case-sensitive.
pub(crate) fn is_joint_typed_fn(name: &str) -> bool {
    JOINT_TYPED_FN_NAMES.contains(&name)
}

/// Result type for a §13 joint-constructor builtin — a nominal type keyed on
/// `name` and (for the coupling family) on `args[0].result_type`.
///
/// **Coupling family is args-aware** (task #4605 ε): `couple` / `gear` / `screw`
/// / `rack_and_pinion` return `Type::applied("Coupling", [args[0].result_type])`
/// when `args` is non-empty, giving the result type the parent joint's nominal
/// type as a phantom type argument. This enables `Coupling<Prismatic>::MotionValue`
/// projection reduction via the δ reducer. When `args` is empty (e.g. in
/// name-only unit tests), the fallback `Type::StructureRef("Coupling")` is
/// returned for backward compatibility.
///
/// All other families remain argument-agnostic (name-only dispatch).
///
/// The nominal tags match the PascalCase structure definitions in
/// `crates/reify-compiler/stdlib/kinematic.ri` (task 3845 + task 4310/γ):
/// `Prismatic`/`Revolute`/`Cylindrical`/`Planar`/`Spherical`/`Coupling`/
/// `Fixed`/`Mechanism`/`Snapshot`/`BodyId`/`SweepDim`/`JointBinding`/`Twist`.
///
/// Note: runtime values stay `Value::Map`/`Int`/`List` (esc-3845-91); the
/// cell TYPE is the enforced nominal tag.
///
/// Only reached for names in [`JOINT_TYPED_FN_NAMES`] (the caller gates on
/// [`is_joint_typed_fn`]); the `_` arm is therefore unreachable in practice
/// and returns a harmless `Type::dimensionless_scalar()`.
pub(crate) fn joint_ctor_result_type(name: &str, args: &[CompiledExpr]) -> Type {
    match name {
        // ── Driving joint kind constructors (5) ──────────────────────────────
        // Each driving-joint kind maps to its own nominal structure type.
        // Runtime: joints.rs emits Value::Map; cell TYPE is the nominal tag.
        "prismatic" => Type::StructureRef("Prismatic".to_string()),
        "revolute" => Type::StructureRef("Revolute".to_string()),
        "cylindrical" => Type::StructureRef("Cylindrical".to_string()),
        "planar" => Type::StructureRef("Planar".to_string()),
        "spherical" => Type::StructureRef("Spherical".to_string()),

        // ── Coupling constructors (4) ─────────────────────────────────────────
        // couple / gear / screw / rack_and_pinion all produce a Coupling value.
        //
        // Args-aware (task #4605 ε): when args is non-empty, args[0] is the
        // parent driving joint whose nominal type parameterises Coupling. This
        // lets the δ reducer resolve `Coupling<Prismatic>::MotionValue ⇒ Length`
        // etc. for let-bound couplings (`let c = couple(j, ratio)`).
        // Empty-args fallback: `Type::StructureRef("Coupling")` — preserves
        // name-only unit tests and any call site that hasn't threaded args yet.
        //
        // B8 boundary (geometric-joints γ, task 4397): `Type::applied("Coupling",[…])`
        // / `Type::StructureRef("Coupling")` ≠ `Type::Relation` — both forms
        // make `check_relate_relations` (entity.rs) reject couplings in a
        // `relate { }` body with `DiagnosticCode::RelateExpectsRelation`.
        // Couplings are algebraic scalar-side ratios, NOT SE(3) coincidence relations.
        // Cross-reference: `crates/reify-compiler/stdlib/joints.ri` B8 section.
        "couple" | "gear" | "screw" | "rack_and_pinion" => {
            if let Some(parent) = args.first() {
                Type::applied("Coupling", vec![parent.result_type.clone()])
            } else {
                Type::StructureRef("Coupling".to_string())
            }
        }

        // ── Fixed joint (1) ──────────────────────────────────────────────────
        "fixed" => Type::StructureRef("Fixed".to_string()),

        // ── Mechanism / body constructors (2) ────────────────────────────────
        // Both `mechanism` (the top-level builder) and `body` (the body-within-
        // mechanism builder) produce a Mechanism value.
        "mechanism" | "body" => Type::StructureRef("Mechanism".to_string()),

        // ── Snapshot constructor (1) ──────────────────────────────────────────
        "snapshot" => Type::StructureRef("Snapshot".to_string()),

        // ── Body-ID accessor (1) ──────────────────────────────────────────────
        // body_id_of evaluates to Value::Int at runtime (esc-3845-91);
        // the cell TYPE is the nominal BodyId tag.
        "body_id_of" => Type::StructureRef("BodyId".to_string()),

        // ── Sweep dimension (1) ───────────────────────────────────────────────
        "dim" => Type::StructureRef("SweepDim".to_string()),

        // ── Joint binding (1) ─────────────────────────────────────────────────
        "bind" => Type::StructureRef("JointBinding".to_string()),

        // ── Joint Jacobian / Twist (1) ────────────────────────────────────────
        // joint_jacobian evaluates to Value::Map (prismatic/revolute/fixed) or
        // Value::List (planar/spherical/cylindrical) at runtime (esc-3845-91);
        // the cell TYPE is the nominal Twist tag.
        "joint_jacobian" => Type::StructureRef("Twist".to_string()),

        // Unreachable in practice — the caller gates on is_joint_typed_fn.
        _ => Type::dimensionless_scalar(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent fixture — list of all 17 expected names in the family.
    /// Deliberately does NOT reference `JOINT_TYPED_FN_NAMES` so a drift in
    /// that slice is caught against this independent list (mirrors
    /// `math_signatures::tests::EXPECTED_NAMES`).
    const EXPECTED_NAMES: [&str; 17] = [
        // Driving joint kinds (5)
        "prismatic",
        "revolute",
        "cylindrical",
        "planar",
        "spherical",
        // Coupling kinds (4)
        "couple",
        "gear",
        "screw",
        "rack_and_pinion",
        // Fixed (1)
        "fixed",
        // Mechanism/body (2)
        "mechanism",
        "body",
        // Other constructors (4)
        "snapshot",
        "body_id_of",
        "dim",
        "bind",
        "joint_jacobian",
    ];

    // ── Name-family contract (step-1 RED / step-2 GREEN) ─────────────────────

    /// `is_joint_typed_fn` recognises every expected joint-constructor name.
    #[test]
    fn is_joint_typed_fn_recognises_all_expected_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_joint_typed_fn(name),
                "is_joint_typed_fn({name:?}) must be true (§13 joint-constructor family)"
            );
        }
    }

    /// `is_joint_typed_fn` rejects names from sibling families, the empty name,
    /// and unknown names (mirrors `is_math_typed_fn_rejects_other_family_and_unknown_names`).
    #[test]
    fn is_joint_typed_fn_rejects_other_family_and_unknown_names() {
        // Geometry-query family.
        assert!(!is_joint_typed_fn("volume"), "must reject geometry-query 'volume'");
        // Dynamics-query family.
        assert!(
            !is_joint_typed_fn("body_mass_props"),
            "must reject dynamics-query 'body_mass_props'"
        );
        // Math-linalg family.
        assert!(!is_joint_typed_fn("vec"), "must reject math-linalg 'vec'");
        assert!(!is_joint_typed_fn("sqrt"), "must reject math-linalg 'sqrt'");
        // `sweep` is deliberately EXCLUDED from the family — it has a geometry overload.
        assert!(!is_joint_typed_fn("sweep"), "must reject 'sweep' (geometry overload, excluded)");
        // Empty / unknown.
        assert!(!is_joint_typed_fn(""), "must reject empty name");
        assert!(!is_joint_typed_fn("does_not_exist"), "must reject unrelated name");
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so the
    /// PascalCase forms must not match (mirrors `is_math_typed_fn_is_case_sensitive`).
    #[test]
    fn is_joint_typed_fn_is_case_sensitive() {
        assert!(!is_joint_typed_fn("Prismatic"), "PascalCase must not match");
        assert!(!is_joint_typed_fn("Couple"), "PascalCase must not match");
        assert!(!is_joint_typed_fn("Bind"), "PascalCase must not match");
        assert!(!is_joint_typed_fn("Fixed"), "PascalCase must not match");
        assert!(!is_joint_typed_fn("Mechanism"), "PascalCase must not match");
    }

    /// `JOINT_TYPED_FN_NAMES` is exactly the 17 expected names: correct count,
    /// every expected name present, and no extra entry. Mirrors
    /// `math_construction_names_are_exactly_the_four`.
    #[test]
    fn joint_typed_fn_names_are_exactly_the_17() {
        assert_eq!(
            JOINT_TYPED_FN_NAMES.len(),
            EXPECTED_NAMES.len(),
            "JOINT_TYPED_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            JOINT_TYPED_FN_NAMES
        );
        // Every expected name is in the slice.
        for name in EXPECTED_NAMES {
            assert!(
                JOINT_TYPED_FN_NAMES.contains(&name),
                "JOINT_TYPED_FN_NAMES must contain {name:?}"
            );
        }
        // No extra name beyond the expected fixture.
        for name in JOINT_TYPED_FN_NAMES {
            assert!(
                EXPECTED_NAMES.contains(name),
                "JOINT_TYPED_FN_NAMES has unexpected entry {name:?} not in the fixture"
            );
        }
    }

    // ── Result-type resolution (step-3 RED / step-4 GREEN) ───────────────────

    /// Every joint-constructor name maps to the exact nominal `Type::StructureRef`.
    /// Called with `&[]` args (name-only dispatch; args are unused).
    #[test]
    fn joint_ctor_result_type_maps_each_name_to_its_nominal_struct() {
        // Driving joint kinds → their own kind type.
        assert_eq!(
            joint_ctor_result_type("prismatic", &[]),
            Type::StructureRef("Prismatic".to_string()),
            "prismatic must map to StructureRef(Prismatic)"
        );
        assert_eq!(
            joint_ctor_result_type("revolute", &[]),
            Type::StructureRef("Revolute".to_string()),
            "revolute must map to StructureRef(Revolute)"
        );
        assert_eq!(
            joint_ctor_result_type("cylindrical", &[]),
            Type::StructureRef("Cylindrical".to_string()),
            "cylindrical must map to StructureRef(Cylindrical)"
        );
        assert_eq!(
            joint_ctor_result_type("planar", &[]),
            Type::StructureRef("Planar".to_string()),
            "planar must map to StructureRef(Planar)"
        );
        assert_eq!(
            joint_ctor_result_type("spherical", &[]),
            Type::StructureRef("Spherical".to_string()),
            "spherical must map to StructureRef(Spherical)"
        );

        // Coupling kinds: all → Coupling.
        for name in &["couple", "gear", "screw", "rack_and_pinion"] {
            assert_eq!(
                joint_ctor_result_type(name, &[]),
                Type::StructureRef("Coupling".to_string()),
                "{name} must map to StructureRef(Coupling)"
            );
        }

        // Fixed → Fixed.
        assert_eq!(
            joint_ctor_result_type("fixed", &[]),
            Type::StructureRef("Fixed".to_string()),
            "fixed must map to StructureRef(Fixed)"
        );

        // Mechanism/body → Mechanism (BOTH names map to the same type).
        assert_eq!(
            joint_ctor_result_type("mechanism", &[]),
            Type::StructureRef("Mechanism".to_string()),
            "mechanism must map to StructureRef(Mechanism)"
        );
        assert_eq!(
            joint_ctor_result_type("body", &[]),
            Type::StructureRef("Mechanism".to_string()),
            "body must map to StructureRef(Mechanism)"
        );

        // Snapshot → Snapshot.
        assert_eq!(
            joint_ctor_result_type("snapshot", &[]),
            Type::StructureRef("Snapshot".to_string()),
            "snapshot must map to StructureRef(Snapshot)"
        );

        // body_id_of → BodyId.
        assert_eq!(
            joint_ctor_result_type("body_id_of", &[]),
            Type::StructureRef("BodyId".to_string()),
            "body_id_of must map to StructureRef(BodyId)"
        );

        // dim → SweepDim.
        assert_eq!(
            joint_ctor_result_type("dim", &[]),
            Type::StructureRef("SweepDim".to_string()),
            "dim must map to StructureRef(SweepDim)"
        );

        // bind → JointBinding.
        assert_eq!(
            joint_ctor_result_type("bind", &[]),
            Type::StructureRef("JointBinding".to_string()),
            "bind must map to StructureRef(JointBinding)"
        );

        // joint_jacobian → Twist.
        assert_eq!(
            joint_ctor_result_type("joint_jacobian", &[]),
            Type::StructureRef("Twist".to_string()),
            "joint_jacobian must map to StructureRef(Twist)"
        );
    }

    /// Args-agnostic invariant: the same result is returned for non-empty args
    /// (name-only dispatch — the arg slice is currently unused).
    /// NOTE: couple/gear/screw/rack_and_pinion are intentionally EXCLUDED from
    /// this test — they are args-AWARE after step-5 (couple→Applied).
    #[test]
    fn joint_ctor_result_type_is_args_agnostic() {
        use reify_ir::Value;
        // A dummy non-empty arg slice.
        let dummy_arg =
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
        let args_slice = &[dummy_arg];

        assert_eq!(
            joint_ctor_result_type("prismatic", args_slice),
            joint_ctor_result_type("prismatic", &[]),
            "prismatic result must be the same regardless of args"
        );
        assert_eq!(
            joint_ctor_result_type("bind", args_slice),
            joint_ctor_result_type("bind", &[]),
            "bind result must be the same regardless of args"
        );
        assert_eq!(
            joint_ctor_result_type("joint_jacobian", args_slice),
            joint_ctor_result_type("joint_jacobian", &[]),
            "joint_jacobian result must be the same regardless of args"
        );
    }

    // ── couple-family args-aware (step-4 RED / step-5 GREEN) ─────────────────

    /// couple / gear / screw / rack_and_pinion with a non-empty arg slice must
    /// return `Type::applied("Coupling", [args[0].result_type])` — parameterised
    /// by the parent joint's nominal type.
    ///
    /// RED until step-5: the couple arm currently ignores `_args` and always
    /// returns `Type::StructureRef("Coupling")`.
    #[test]
    fn couple_family_with_parent_arg_returns_applied_coupling() {
        use reify_ir::Value;

        // Parent arg typed as StructureRef("Prismatic").
        let prismatic_arg = CompiledExpr::literal(
            Value::Real(1.0),
            Type::StructureRef("Prismatic".to_string()),
        );
        let args = &[prismatic_arg];

        for name in &["couple", "gear", "screw", "rack_and_pinion"] {
            assert_eq!(
                joint_ctor_result_type(name, args),
                Type::applied("Coupling", vec![Type::StructureRef("Prismatic".to_string())]),
                "{name}(Prismatic) must return Type::applied(\"Coupling\",[StructureRef(Prismatic)]); \
                 got: {:?}",
                joint_ctor_result_type(name, args)
            );
        }
    }

    /// couple / gear / screw / rack_and_pinion with an EMPTY arg slice must
    /// return `Type::StructureRef("Coupling")` — the empty-args fallback is
    /// preserved so existing &[]-based unit tests remain green.
    ///
    /// This test is GREEN both before and after step-5.
    #[test]
    fn couple_family_with_empty_args_returns_structure_ref_coupling() {
        for name in &["couple", "gear", "screw", "rack_and_pinion"] {
            assert_eq!(
                joint_ctor_result_type(name, &[]),
                Type::StructureRef("Coupling".to_string()),
                "{name}() with no args must return StructureRef(Coupling) (empty-args fallback); \
                 got: {:?}",
                joint_ctor_result_type(name, &[])
            );
        }
    }
}
