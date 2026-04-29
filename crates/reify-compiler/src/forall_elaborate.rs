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

/// Resolve a collection sub's count cell to a literal `i64` count.
///
/// Mirrors the Literal-or-ValueRef-to-Literal chain at
/// `crates/reify-eval/src/graph.rs:171-209`. Returns `Some(n)` only when:
///   * The count cell exists in `value_cells`,
///   * Its `default_expr` is `Some(Literal(Int(n)))` directly, OR
///   * Its `default_expr` is `Some(ValueRef(other))` whose target cell's
///     `default_expr` is `Some(Literal(Int(n)))` (one indirection hop).
///
/// Returns `None` for missing cells, missing defaults, non-literal expressions,
/// non-Int literals, multi-hop indirection, or any other shape. Callers treat
/// `None` as "skip — emit zero decls, no diagnostic" per PRD criterion 7.
fn resolve_count_cell_literal(
    value_cells: &[ValueCellDecl],
    count_id: &ValueCellId,
) -> Option<i64> {
    let cell = value_cells.iter().find(|vc| vc.id == *count_id)?;
    let expr = cell.default_expr.as_ref()?;
    match &expr.kind {
        CompiledExprKind::Literal(Value::Int(n)) => Some(*n),
        CompiledExprKind::ValueRef(ref_id) => {
            let referenced = value_cells.iter().find(|vc| vc.id == *ref_id)?;
            let referenced_expr = referenced.default_expr.as_ref()?;
            if let CompiledExprKind::Literal(Value::Int(n)) = &referenced_expr.kind {
                Some(*n)
            } else {
                None
            }
        }
        _ => None,
    }
}

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
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    value_cells: &[ValueCellDecl],
    sub_components: &[SubComponentDecl],
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    constraint_inst_counts: &mut HashMap<String, usize>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut std::collections::HashSet<ValueCellId>,
    guard_index: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use reify_syntax::{Expr, ExprKind, ForallConstraintBody};

    // Resolve the collection expression to a Vec of per-element replacements.
    //   * `ListLiteral(items)` → element AST = `items[i]`.
    //   * `Ident(name)` referencing a collection sub with a literally-resolved
    //     count cell → element AST = `IndexAccess { object: Ident(name), index: NumberLiteral(i) }`.
    //   * Anything else → emit zero decls. PRD criterion 7's "no decls when
    //     count is undef" half. Non-iterable diagnostic is added in step-20;
    //     re-elaboration on count change is out of scope (future SchemaNode work).
    let elements: Vec<reify_syntax::Expr> = match &decl.collection.kind {
        ExprKind::ListLiteral(items) => items.clone(),
        ExprKind::Ident(name) => {
            // Look up the matching collection sub and resolve its count.
            let sub = sub_components
                .iter()
                .find(|s| s.name == *name && s.is_collection);
            let count = sub
                .and_then(|s| s.count_cell.as_ref())
                .and_then(|count_id| resolve_count_cell_literal(value_cells, count_id));
            let Some(count) = count else {
                // Cannot statically resolve count — emit zero decls silently.
                // (Step-20 adds the non-iterable diagnostic for genuinely
                // mistyped collection expressions; here we defer to support
                // future re-elaboration on count change.)
                return;
            };
            let coll_span = decl.collection.span;
            (0..count)
                .map(|i| Expr {
                    kind: ExprKind::IndexAccess {
                        object: Box::new(Expr {
                            kind: ExprKind::Ident(name.clone()),
                            span: coll_span,
                        }),
                        index: Box::new(Expr {
                            kind: ExprKind::NumberLiteral(i as f64),
                            span: coll_span,
                        }),
                    },
                    span: coll_span,
                })
                .collect()
        }
        // Step-20: non-iterable diagnostic. For now, silently skip.
        _ => return,
    };

    for (i, element) in elements.iter().enumerate() {
        let mut bindings: HashMap<String, reify_syntax::Expr> = HashMap::new();
        bindings.insert(decl.variable.clone(), element.clone());

        match &decl.body {
            ForallConstraintBody::Constraint(body_constraint) => {
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

                // PRD criterion 9: when the body has a `where` clause, route the
                // per-element constraint through `compile_per_decl_constraint_guard`
                // so it lives inside its own single-constraint guarded group with
                // the (per-element substituted) condition as the guard. Mirrors the
                // shape used for plain `MemberDecl::Constraint(c)` with a where
                // clause at entity.rs:951-963.
                if let Some(wc) = &body_constraint.where_clause {
                    // Substitute the bound variable inside the where condition. The
                    // condition often does not reference the bound var (e.g. a
                    // structure-level Bool flag), in which case substitution is a
                    // no-op — but in general the spec allows the condition to
                    // reference the per-element value, so we always substitute.
                    let substituted_wc = reify_syntax::WhereClause {
                        condition: substitute_expr(&wc.condition, &bindings),
                        span: wc.span,
                    };
                    compile_per_decl_constraint_guard(
                        entity_name,
                        &substituted_wc,
                        cc,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        guarded_groups,
                        structure_controlling,
                        guard_index,
                    );
                } else {
                    constraints.push(cc);
                }
            }
            ForallConstraintBody::Instantiation(ci) => {
                // Per-element substituted clone of the inst decl: each named
                // arg expr (and the where-clause condition if present) is
                // run through `substitute_expr` with `decl.variable → element`.
                // The shared `expand_constraint_inst` helper then handles def
                // lookup, arg validation, inst_idx allocation, per-predicate
                // emission, and where-clause routing exactly as for plain
                // `MemberDecl::ConstraintInst` arms — with the optional
                // `forall@<var>[<i>]` label suffix appended so per-element
                // diagnostics retain both the inst-idx provenance and the
                // forall element index.
                let substituted_args: Vec<(String, reify_syntax::Expr)> = ci
                    .args
                    .iter()
                    .map(|(n, e)| (n.clone(), substitute_expr(e, &bindings)))
                    .collect();
                let substituted_wc =
                    ci.where_clause.as_ref().map(|wc| reify_syntax::WhereClause {
                        condition: substitute_expr(&wc.condition, &bindings),
                        span: wc.span,
                    });
                let substituted_ci = reify_syntax::ConstraintInstDecl {
                    name: ci.name.clone(),
                    args: substituted_args,
                    where_clause: substituted_wc,
                    span: ci.span,
                    content_hash: ci.content_hash,
                };
                let suffix = format!("forall@{}[{}]", decl.variable, i);
                expand_constraint_inst(
                    &substituted_ci,
                    entity_name,
                    constraint_def_registry,
                    scope,
                    enum_defs,
                    functions,
                    constraints,
                    constraint_index,
                    constraint_inst_counts,
                    guarded_groups,
                    structure_controlling,
                    guard_index,
                    diagnostics,
                    Some(&suffix),
                );
            }
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
