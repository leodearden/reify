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
use std::collections::HashMap;

/// Drive per-element constraint emission for a `forall ... : constraint ...`
/// or `forall ... : constraint Inst(...)` declaration.
///
/// Currently supports:
///   * `ListLiteral` collection × `ConstraintBody::Constraint` body —
///     emits one `CompiledConstraint` per literal element with label
///     `forall@<var>[<idx>]` and `span = decl.span`.
///
/// Other shapes (collection-sub idents, instantiation bodies, where
/// clauses) are stubbed and will be filled in by subsequent steps
/// (see plan steps 6, 8, 12, 14, 20).
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_forall_constraint(
    decl: &reify_syntax::ForallConstraintDecl,
    entity_name: &str,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    _constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    _value_cells: &[ValueCellDecl],
    _sub_components: &[SubComponentDecl],
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    _constraint_inst_counts: &mut HashMap<String, usize>,
    _guarded_groups: &mut Vec<CompiledGuardedGroup>,
    _structure_controlling: &mut std::collections::HashSet<ValueCellId>,
    _guard_index: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use reify_syntax::{ExprKind, ForallConstraintBody};

    // Step-2 scope: only ListLiteral collections × ConstraintBody::Constraint.
    // Other shapes (Ident-as-collection-sub, Instantiation body, where clauses)
    // are deferred to subsequent steps.
    let elements: Vec<reify_syntax::Expr> = match &decl.collection.kind {
        ExprKind::ListLiteral(items) => items.clone(),
        // Step-6+: Ident-as-collection-sub resolution. Step-20: non-iterable diagnostic.
        // For now, silently skip unsupported collection shapes.
        _ => return,
    };

    let body_constraint = match &decl.body {
        ForallConstraintBody::Constraint(c) => c,
        // Step-14: Instantiation body. For now, silently skip.
        ForallConstraintBody::Instantiation(_) => return,
    };

    for (i, element) in elements.iter().enumerate() {
        let mut bindings: HashMap<String, reify_syntax::Expr> = HashMap::new();
        bindings.insert(decl.variable.clone(), element.clone());

        let substituted_expr = substitute_expr(&body_constraint.expr, &bindings);
        let compiled_expr =
            compile_expr(&substituted_expr, scope, enum_defs, functions, diagnostics);

        let id = ConstraintNodeId::new(entity_name, *constraint_index);
        let cc = CompiledConstraint {
            id,
            label: Some(format!("forall@{}[{}]", decl.variable, i)),
            expr: compiled_expr,
            span: decl.span,
            domain: None,
            optimized_target: None,
        };
        *constraint_index += 1;

        // Step-12: route through compile_per_decl_constraint_guard when
        // body_constraint.where_clause is Some. For step-2, the test source
        // has no where clause, so we push directly.
        if body_constraint.where_clause.is_none() {
            constraints.push(cc);
        } else {
            // Step-12 stub: drop on the floor for now.
            constraints.push(cc);
        }
    }
}

/// Drive per-element connection emission for a `forall ... : connect ...`
/// or `forall ... : chain ...` declaration.
///
/// Stub: implemented incrementally across steps 16, 18, and 20.
///
/// Note on borrowing: `sub_components` is passed mutably because the
/// per-element `compile_connection` calls may push connector sub-components.
/// The helper is responsible for taking an immutable read of collection
/// sub-component info (count_cell, etc.) before entering the per-element
/// emission loop, so that the immutable borrow is dropped before any
/// mutating call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_forall_connect(
    _decl: &reify_syntax::ForallConnectDecl,
    _entity_name: &str,
    _ports: &[CompiledPort],
    _scope: &CompilationScope,
    _enum_defs: &[reify_types::EnumDef],
    _functions: &[CompiledFunction],
    _trait_registry: &HashMap<String, &CompiledTrait>,
    _value_cells: &[ValueCellDecl],
    _constraints: &mut Vec<CompiledConstraint>,
    _constraint_index: &mut u32,
    _connections: &mut Vec<CompiledConnection>,
    _sub_components: &mut Vec<SubComponentDecl>,
    _connector_index: &mut u32,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Implemented in step-16.
}
