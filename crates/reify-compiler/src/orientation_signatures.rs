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
/// Populated in step-2 (currently an empty scaffold).
pub const ORIENTATION_TYPED_FN_NAMES: &[&str] = &[];

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
