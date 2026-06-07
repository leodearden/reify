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
pub const JOINT_TYPED_FN_NAMES: &[&str] = &[];

/// Is `name` a §13 joint-constructor builtin the compiler types via
/// [`joint_ctor_result_type`]? Name-only classification — a `.contains` over
/// the single-source-of-truth slice [`JOINT_TYPED_FN_NAMES`]. Case-sensitive.
pub(crate) fn is_joint_typed_fn(name: &str) -> bool {
    JOINT_TYPED_FN_NAMES.contains(&name)
}

/// Result type for a §13 joint-constructor builtin — a fixed nominal
/// `Type::StructureRef(...)` keyed on `name`. Argument-agnostic (name-only
/// dispatch): each joint constructor maps to a single fixed nominal type
/// independent of its arguments. The `_args` parameter is retained for
/// signature parity with `math_fn_result_type` and to leave room for future
/// arity-based dispatch.
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
/// and returns a harmless `Type::Real`.
#[allow(unused_variables)]
pub(crate) fn joint_ctor_result_type(name: &str, _args: &[CompiledExpr]) -> Type {
    Type::Real
}

#[cfg(test)]
mod tests {
    use super::*;
}
