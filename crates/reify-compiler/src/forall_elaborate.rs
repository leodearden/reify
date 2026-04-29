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
//!
//! ## PRD criteria 6 and 7 (first-half) contract
//!
//! **Criterion 6 — empty collection:** any collection that resolves to zero
//! elements (a `ListLiteral([])` or a count-cell with value `0`) produces
//! `Some(vec![])` from `resolve_forall_elements`. All callers iterate over
//! this empty Vec zero times, emitting no decls and no diagnostics. This
//! property is upheld by the per-element `for` loops in
//! `elaborate_forall_constraint` and `elaborate_forall_connect`; code that
//! must only run per-element MUST stay inside those loops.
//!
//! **Criterion 7 first-half — undef count:** a collection sub whose count
//! cell is missing or non-literal returns `None` from
//! `resolve_forall_elements`. All callers early-return on `None`, emitting
//! zero decls and no diagnostics. Re-elaboration when the count later
//! becomes known is future SchemaNode work (see TODO comments in tests).

use super::*;
use std::collections::HashMap;

/// Resolve the collection expression of a `forall` decl to a Vec of
/// per-element replacement ASTs.
///
/// Returns `Some(elements)` when:
///   * The collection is a `ListLiteral` (any length) — elements are the
///     literal items.
///   * The collection is an `Ident(name)` referring to a collection sub
///     whose `__count_<name>` cell resolves to a literal `Int` — elements
///     are `IndexAccess { Ident(name), NumberLiteral(i) }` for `i ∈ 0..count`.
///
/// Returns `None` (caller emits zero decls) when:
///   * The collection is an `Ident` for a collection sub whose count cell
///     is missing or non-literal (PRD criterion 7's silent-skip half — defers
///     re-elaboration to future SchemaNode work).
///   * The collection is some other expression that type-checks to
///     `Type::List(_)` or `Type::Set(_)` (a List/Set value whose count
///     isn't statically known — same defer case).
///   * The collection's compiled type is `Type::Error` (anti-cascade — the
///     upstream error has already been emitted by `compile_expr`).
///
/// Emits a diagnostic and returns `None` when:
///   * The collection's compiled type is anything else (Int, Bool, Scalar,
///     etc.) — a genuinely non-iterable collection expression.
///
/// Diagnostic wording mirrors the expression-form forall/exists at
/// `expr.rs:1791-1799` for symmetry: `cannot iterate over non-collection
/// type '<ty>' in forall: expected List<_> or Set<_>` with label
/// `not iterable` anchored at the collection expression's span.
fn resolve_forall_elements(
    collection: &reify_syntax::Expr,
    sub_components: &[SubComponentDecl],
    value_cells: &[ValueCellDecl],
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<reify_syntax::Expr>> {
    use reify_syntax::{Expr, ExprKind};

    match &collection.kind {
        ExprKind::ListLiteral(items) => Some(items.clone()),
        ExprKind::Ident(name) => {
            // Try to resolve as a collection sub first.
            if let Some(sub) = sub_components
                .iter()
                .find(|s| s.name == *name && s.is_collection)
            {
                // PRD criterion 7 first-half: if count is not yet determined
                // (cell missing, non-literal default, or multi-hop indirection),
                // the `?` returns `None` — defer silently, emit zero decls, no
                // diagnostic. Re-elaboration on count change is future SchemaNode work.
                let count = sub
                    .count_cell
                    .as_ref()
                    .and_then(|count_id| resolve_count_cell_literal(value_cells, count_id))?;
                let coll_span = collection.span;
                // PRD criterion 6 — count-cell-zero path: when `count == 0`,
                // `(0..0).map(...).collect()` produces `Some(empty Vec)`.
                // The caller then iterates zero times and emits no decls — the
                // same no-op semantics as the `ListLiteral([])` path above.
                // Both paths share this callsite's iteration, so the contract
                // holds for both criterion-6 shapes in a single place.
                return Some(
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
                        .collect(),
                );
            }
            // Ident doesn't refer to a collection sub — fall through to the
            // type-check path below so a List<T>-typed param defers silently
            // and a non-collection ident emits a non-iterable diagnostic.
            diagnose_non_iterable_or_skip(collection, scope, enum_defs, functions, diagnostics);
            None
        }
        _ => {
            diagnose_non_iterable_or_skip(collection, scope, enum_defs, functions, diagnostics);
            None
        }
    }
}

/// Type-check a non-statically-resolvable collection expression and either
/// emit a `cannot iterate over non-collection type` diagnostic or defer
/// silently.
///
/// Called from `resolve_forall_elements` for the Ident-but-not-collection-sub
/// and the catch-all branches. Anti-cascade: when `compile_expr` produces
/// `Type::Error`, suppress the new diagnostic (the upstream error already
/// surfaces the root cause). For valid `List<_>` / `Set<_>` types whose
/// count isn't statically known, defer silently — re-elaboration on count
/// change is future SchemaNode work (PRD criterion 7).
fn diagnose_non_iterable_or_skip(
    collection: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let compiled = compile_expr(collection, scope, enum_defs, functions, diagnostics);
    let ty = &compiled.result_type;
    if ty.is_error() {
        // Upstream error already emitted; do not pile on.
        return;
    }
    if matches!(ty, Type::List(_) | Type::Set(_)) {
        // Valid collection type but count not statically known — defer
        // silently. Future SchemaNode-style re-elaboration will pick this up
        // once the count becomes known at graph-build time.
        return;
    }
    diagnostics.push(
        Diagnostic::error(format!(
            "cannot iterate over non-collection type '{}' in forall: expected List<_> or Set<_>",
            ty
        ))
        .with_label(DiagnosticLabel::new(collection.span, "not iterable")),
    );
}

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
    use reify_syntax::ForallConstraintBody;

    // Resolve the collection expression to a Vec of per-element replacements.
    // The shared helper handles all four cases: ListLiteral, Ident-as-
    // collection-sub, deferred (silently skip), and non-iterable (emit a
    // diagnostic). See `resolve_forall_elements` for full semantics.
    let Some(elements) = resolve_forall_elements(
        &decl.collection,
        sub_components,
        value_cells,
        scope,
        enum_defs,
        functions,
        diagnostics,
    ) else {
        return;
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
                //
                // INTENTIONAL PLACEMENT (PRD criterion 6) — `expand_constraint_inst`
                // and the `constraint_inst_counts` mutation are both INSIDE this
                // per-element loop. For `forall v in []: constraint Inst(...)`,
                // the outer loop iterates zero times so `expand_constraint_inst`
                // is never called and no `inst_idx` is allocated. A future
                // refactor that pre-allocates `inst_idx` outside the loop would
                // break tests pinning PRD criterion 6; see tests for this module.
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
/// Currently supports:
///   * `ListLiteral` collection × any body — substitution replaces the bound
///     var with the literal element AST.
///   * `Ident(name)` collection-sub × any body — substitution replaces the
///     bound var with `IndexAccess { object: Ident(name), index: NumberLiteral(i) }`.
///   * `ConnectBody::Connect(cd)` — emits one `CompiledConnection` per element
///     via `compile_connection`, with `span = decl.span`.
///
/// `ConnectBody::Chain(cd)` is left for step-18.
///
/// Note on borrowing: `sub_components` is passed mutably because the
/// per-element `compile_connection` calls may push connector sub-components.
/// The helper takes an immutable read of collection sub-component info
/// (count_cell, etc.) up front to compute the element list, then drops that
/// borrow before entering the per-element emission loop where
/// `sub_components` is borrowed mutably via the `ConnectAccumulator`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn elaborate_forall_connect(
    decl: &reify_syntax::ForallConnectDecl,
    entity_name: &str,
    ports: &[CompiledPort],
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    value_cells: &[ValueCellDecl],
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    connections: &mut Vec<CompiledConnection>,
    sub_components: &mut Vec<SubComponentDecl>,
    connector_index: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use reify_syntax::ForallConnectBody;

    // Resolve the collection expression via the shared helper. See
    // `resolve_forall_elements` for the four-case semantics: ListLiteral,
    // Ident-as-collection-sub, deferred (silently skip), and non-iterable
    // (emit diagnostic).
    //
    // `None` means one of two things:
    //   * PRD criterion 7 first-half: collection count not yet determined —
    //     emit zero connections (no diagnostic). Re-elaboration on count
    //     change is future SchemaNode work.
    //   * Non-iterable collection expression: a diagnostic was already emitted
    //     by `resolve_forall_elements`; skip connection emission to avoid
    //     cascading errors.
    // Both cases return early here; the per-element loop below only runs when
    // a concrete, statically-known set of elements was resolved.
    let Some(elements) = resolve_forall_elements(
        &decl.collection,
        sub_components,
        value_cells,
        scope,
        enum_defs,
        functions,
        diagnostics,
    ) else {
        return; // PRD criterion 7 first-half: defer silently, or non-iterable (diagnostic already emitted).
    };

    // Build the read-only `ConnectContext` once. The accumulator is rebuilt
    // per element so we avoid retaining a long-lived mutable borrow on
    // `constraints` / `connections` / `sub_components` across loop iterations.
    let ctx = ConnectContext {
        entity_name,
        ports,
        scope,
        enum_defs,
        functions,
        trait_registry,
    };

    // PRD criterion 6 — empty-collection path: when `elements` is empty (either
    // a `ListLiteral([])` or a count-cell-zero collection sub), this loop
    // iterates zero times and emits no connections and no diagnostics. The
    // criterion-7-first-half deferred case (count not yet determined) is
    // handled by the `let Some(elements) = … else { return; }` early-return
    // above — it never reaches this loop.
    for (i, element) in elements.iter().enumerate() {
        let mut bindings: HashMap<String, reify_syntax::Expr> = HashMap::new();
        bindings.insert(decl.variable.clone(), element.clone());

        match &decl.body {
            ForallConnectBody::Connect(cd) => {
                // Substitute every expression-bearing position in the body.
                let left_substituted = substitute_expr(&cd.left.expr, &bindings);
                let right_substituted = substitute_expr(&cd.right.expr, &bindings);
                let params_substituted: Vec<(String, reify_syntax::Expr)> = cd
                    .params
                    .iter()
                    .map(|(n, e)| (n.clone(), substitute_expr(e, &bindings)))
                    .collect();

                let mut acc = ConnectAccumulator {
                    constraints,
                    constraint_index,
                    connections,
                    sub_components,
                    connector_index,
                };
                compile_connection(
                    &ctx,
                    &ConnectInput {
                        left_expr: &left_substituted,
                        operator: cd.operator,
                        right_expr: &right_substituted,
                        connector_type: cd.connector_type.as_deref(),
                        params: &params_substituted,
                        port_mappings: &cd.port_mappings,
                        // Anchor the emitted connection at the source forall
                        // declaration so per-element diagnostics cite the
                        // forall site and the element index travels in the
                        // synthetic compatibility constraint label.
                        span: decl.span,
                    },
                    diagnostics,
                    &mut acc,
                );
                let _ = i; // element index currently encoded only via the
                           // synthetic `connect_compat_<l>_<r>` label produced
                           // by `compile_connection` (the substituted port
                           // names already include `[i]`); a dedicated
                           // forall-element label is added in step-21 if
                           // needed for diagnostic provenance.
            }
            // Per-element chain desugaring: substitute every chain element,
            // then emit pairwise Forward connections via `windows(2)`. Mirror
            // the plain `MemberDecl::Chain` arm at entity.rs:1304-1342, but
            // anchor every emitted connection's span at `decl.span` so
            // per-element diagnostics cite the forall site.
            ForallConnectBody::Chain(cd) => {
                // Edge case: fewer than two elements is a malformed chain.
                // Emit the standard chain diagnostic once per element-iteration
                // (matching the plain-Chain arm's behaviour) anchored at the
                // forall span. The plain arm uses `chain_decl.span`; here the
                // forall span subsumes the chain body's span and is the
                // user-visible site.
                //
                // INTENTIONAL PLACEMENT — this guard is INSIDE the outer
                // per-element loop. For `forall v in []: chain ...` (PRD
                // criterion 6), the outer loop iterates zero times so this
                // guard is never reached and no diagnostic is emitted. For a
                // non-empty forall with a malformed chain body (e.g. only one
                // chain element), the guard fires once per outer-loop element.
                // Do NOT hoist this guard outside the loop for "efficiency" —
                // doing so would fire the diagnostic for the empty-list case
                // (breaking criterion 6) and for the undef-count deferred case.
                if cd.elements.len() < 2 {
                    diagnostics.push(
                        Diagnostic::error("chain statement requires at least two elements")
                            .with_label(DiagnosticLabel::new(decl.span, "too few elements")),
                    );
                    // Skip emission for this element; without at least two
                    // elements there is no pairwise window to desugar.
                    let _ = i;
                    continue;
                }

                let substituted_elements: Vec<reify_syntax::Expr> = cd
                    .elements
                    .iter()
                    .map(|e| substitute_expr(e, &bindings))
                    .collect();

                for pair in substituted_elements.windows(2) {
                    let mut acc = ConnectAccumulator {
                        constraints,
                        constraint_index,
                        connections,
                        sub_components,
                        connector_index,
                    };
                    compile_connection(
                        &ctx,
                        &ConnectInput {
                            left_expr: &pair[0],
                            operator: reify_syntax::ConnectOp::Forward,
                            right_expr: &pair[1],
                            connector_type: None,
                            params: &[],
                            port_mappings: &[],
                            span: decl.span,
                        },
                        diagnostics,
                        &mut acc,
                    );
                }
            }
        }
    }
}
