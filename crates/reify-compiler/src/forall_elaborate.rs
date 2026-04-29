//! Statement-form `forall` per-element elaboration (task 2364, spec §5.4).
//!
//! Lifts the `MemberDecl::ForallConnect` / `MemberDecl::ForallConstraint`
//! stubs in `entity.rs` and emits one `CompiledConnection` /
//! `CompiledConstraint` per collection element. Each emitted decl carries a
//! span anchored at the source `forall` declaration plus a label encoding
//! the bound-variable name and element index (`forall@<var>[<idx>]`).
//!
//! The two entry points (`elaborate_forall_constraint` /
//! `elaborate_forall_connect`) are called from `compile_structure_inner`'s
//! deferred sub-pass, which runs after the main second-pass loop completes
//! so that `sub_components` and `value_cells` (count cells included) are
//! fully populated regardless of source order.
//!
//! Compile-time count resolution covers the two common shapes:
//!   * `ListLiteral` collections — count = `items.len()`, per-element
//!     replacement is the literal element AST.
//!   * `Ident(name)` referring to a `sub <name> : List<T>` with a known
//!     `__count_<name>` cell whose default resolves to a literal `Int` —
//!     count = literal value, per-element replacement is
//!     `IndexAccess { object: Ident(name), index: NumberLiteral(i) }`.
//!
//! Anything else (non-iterable, undef count, multi-hop indirection) emits
//! zero decls. Re-elaboration on count change is out of scope for this task
//! and is documented as future SchemaNode-style work.

use super::*;

/// Drive per-element constraint emission for a `forall ... : constraint ...`
/// or `forall ... : constraint Inst(...)` declaration.
///
/// Stub: implemented incrementally across steps 2, 6, 8, 12, 14, and 20.
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_forall_constraint(
    _decl: &reify_syntax::ForallConstraintDecl,
    _entity_name: &str,
    _scope: &mut CompilationScope,
    _enum_defs: &[reify_types::EnumDef],
    _functions: &[CompiledFunction],
    _constraint_def_registry: &std::collections::HashMap<String, &CompiledConstraintDef>,
    _value_cells: &[ValueCellDecl],
    _sub_components: &[SubComponentDecl],
    _constraints: &mut Vec<CompiledConstraint>,
    _constraint_index: &mut u32,
    _constraint_inst_counts: &mut std::collections::HashMap<String, u32>,
    _guarded_groups: &mut Vec<CompiledGuardedGroup>,
    _structure_controlling: &mut std::collections::HashSet<ValueCellId>,
    _guard_index: &mut u32,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Implemented in step-2.
}

/// Drive per-element connection emission for a `forall ... : connect ...`
/// or `forall ... : chain ...` declaration.
///
/// Stub: implemented incrementally across steps 16, 18, and 20.
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_forall_connect(
    _decl: &reify_syntax::ForallConnectDecl,
    _entity_name: &str,
    _ports: &[CompiledPort],
    _scope: &CompilationScope,
    _enum_defs: &[reify_types::EnumDef],
    _functions: &[CompiledFunction],
    _trait_registry: &std::collections::HashMap<String, &CompiledTrait>,
    _value_cells: &[ValueCellDecl],
    _sub_components_in: &[SubComponentDecl],
    _constraints: &mut Vec<CompiledConstraint>,
    _constraint_index: &mut u32,
    _connections: &mut Vec<CompiledConnection>,
    _sub_components_out: &mut Vec<SubComponentDecl>,
    _connector_index: &mut u32,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Implemented in step-16.
}
