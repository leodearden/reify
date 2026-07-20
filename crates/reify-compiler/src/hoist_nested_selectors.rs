//! task 4370 AXIS-1 step-6: hoist nested selector constructors.
//!
//! A selector constructor (`faces(b)`, `face(b, "x_max")`, …) written INLINE as
//! a `StructureInstanceCtor` field value never resolves on the eval path: the
//! kernel-free symbolic selector mint
//! ([`reify_eval::geometry_ops::mint_symbolic_topology_selectors_into_values`])
//! only visits a template's top-level `value_cells` default exprs, not the
//! constructor-argument exprs nested inside them, so the field stays
//! `Value::Undef` (root-caused on the superseded Strategy-A WIP).
//!
//! This pass performs an ANF-style let-hoist: for every selector-ctor
//! `FunctionCall` sitting as a `StructureInstanceCtor` field value it mints a
//! synthetic top-level `Let` value cell (`<Template>::__sel_N`) carrying the
//! ctor verbatim, and rewrites the field to a `ValueRef(__sel_N)`. The synthetic
//! cell IS a top-level `value_cells` entry, so the symbolic mint now resolves it;
//! and because `collect_value_refs` recurses `StructureInstanceCtor` args, the
//! `ValueRef(__sel_N)` inside the ctor creates a dependency edge ordering
//! `__sel_N` BEFORE its consumer, so the R3d in-walk mint resolves it in time
//! (AXIS-2 TIMING, reused from #4900/R3d unchanged).
//!
//! ## Scope (design decision 4)
//!
//! The walk descends `StructureInstanceCtor` `ordered_args` + `defaults` and the
//! `ListLiteral`/`SetLiteral` collections between them, but NOT a ctor's `lets`
//! (struct-def template lets, which resolve in the constructed instance's child
//! scope, not the enclosing template's). A selector ctor that is itself a
//! top-level cell default (`let sel = faces(b)`) is left alone — it is already a
//! value cell the mint visits. Only selector ctors in *field* position are lifted.
//!
//! ## Hash (design decision 5)
//!
//! `compute_module_hash` folds each template's `content_hash` field into the
//! module hash, so a mutated template must have its `content_hash` refreshed or
//! the hoist would be invisible to the cache key. Each minted cell's default-expr
//! `content_hash` is combined (in deterministic walk order) into the enclosing
//! template's `content_hash`; a template that mints nothing is left byte-identical
//! (hash stability). The minted ctor exprs are the sole content-carrier of the
//! hoisted selectors, so this discriminates every source variation.

use reify_core::ContentHash;
use reify_core::identity::ValueCellId;
use reify_ir::{CompiledExpr, CompiledExprKind};

use crate::compile_builder::ctx::CompilationCtx;
use crate::types::{ValueCellDecl, ValueCellKind, Visibility};

/// Hoist nested selector-ctor field values into synthetic top-level `Let` cells.
///
/// Runs after `phase_augment_composed_captures` and before `compute_module_hash`
/// (see the driver in `lib.rs`), at which point every `StructureInstanceCtor`
/// shape is fully materialized.
pub(crate) fn phase_hoist_nested_selector_ctors(ctx: &mut CompilationCtx) {
    for template in ctx.templates.iter_mut() {
        // `template.name` is read as the synthetic cells' entity; clone it so the
        // per-cell mutable borrows below don't conflict with the field read.
        let template_name = template.name.clone();
        let mut minted: Vec<ValueCellDecl> = Vec::new();
        let mut counter: usize = 0;

        // Walk each ORIGINAL cell's default_expr. Minted cells are appended after
        // the loop (they are bare selector-ctor FunctionCalls at top level, which
        // this pass never re-hoists), so a fixed `original_len` bound is correct
        // and avoids visiting freshly-minted cells.
        let original_len = template.value_cells.len();
        for i in 0..original_len {
            if let Some(expr) = template.value_cells[i].default_expr.as_mut() {
                hoist_in_expr(expr, &template_name, &mut counter, &mut minted);
            }
        }

        if minted.is_empty() {
            continue;
        }

        // Fold the minted ctors into the enclosing template content_hash (design
        // decision 5). Deterministic: `minted` is in walk order.
        let mut new_hash = template.content_hash;
        for cell in &minted {
            let h = cell
                .default_expr
                .as_ref()
                .map(|e| e.content_hash)
                .unwrap_or(ContentHash(0));
            new_hash = new_hash.combine(h);
        }
        template.content_hash = new_hash;
        template.value_cells.extend(minted);
    }
}

/// Recursively descend `expr`, hoisting selector-ctor `FunctionCall`s that sit as
/// `StructureInstanceCtor` field values (in `ordered_args` / `defaults`, or in a
/// `ListLiteral`/`SetLiteral` nested in one).
fn hoist_in_expr(
    expr: &mut CompiledExpr,
    template_name: &str,
    counter: &mut usize,
    minted: &mut Vec<ValueCellDecl>,
) {
    match &mut expr.kind {
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, field) in ordered_args.iter_mut().chain(defaults.iter_mut()) {
                // Descend FIRST so deeper nesting (a ctor field that is itself a
                // ctor, or a list of ctors) is hoisted before we test this field.
                hoist_in_expr(field, template_name, counter, minted);
                // Then lift THIS field if it is a selector-ctor FunctionCall. A
                // nested StructureInstanceCtor field is NOT lifted (only selector
                // ctors are) — its own selector fields were handled by the recurse.
                if is_hoistable_selector_ctor(field) {
                    hoist_field(field, template_name, counter, minted);
                }
            }
            // `lets` are intentionally not descended (design decision 4).
        }
        CompiledExprKind::ListLiteral(elems) | CompiledExprKind::SetLiteral(elems) => {
            for e in elems.iter_mut() {
                hoist_in_expr(e, template_name, counter, minted);
            }
        }
        // A nested selector ctor only reaches a hoistable position through a
        // StructureInstanceCtor field (directly or via a list/set of ctors); other
        // expr shapes are out of scope for the field-only hoist (design decision 4).
        _ => {}
    }
}

/// Replace `field` (a selector-ctor `FunctionCall`) with a `ValueRef` to a newly
/// minted `<template_name>::__sel_N` `Let` cell that carries the ctor verbatim.
fn hoist_field(
    field: &mut CompiledExpr,
    template_name: &str,
    counter: &mut usize,
    minted: &mut Vec<ValueCellDecl>,
) {
    let id = ValueCellId::new(template_name, format!("__sel_{}", *counter));
    *counter += 1;
    let result_type = field.result_type.clone();
    // Move the ctor into the synthetic cell; leave a ValueRef in its place.
    let hoisted = std::mem::replace(
        field,
        CompiledExpr::value_ref(id.clone(), result_type.clone()),
    );
    minted.push(ValueCellDecl {
        id,
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: result_type,
        default_expr: Some(hoisted),
        solver_hints: Vec::new(),
        span: reify_core::SourceSpan::empty(0),
    });
}

/// A `FunctionCall` whose name types as `Type::Selector(_)` per
/// [`crate::units::topology_selector_result_type`] — i.e. one of the kernel-free
/// leaf/predicate selector ctors or the four named-leaf ctors
/// (`face`/`edge`/`vertex`/`solid_body`). Relational selectors (`adjacent_faces`,
/// `shared_edges`, `split`, …) type as `List<Geometry>` and are NOT hoisted.
fn is_hoistable_selector_ctor(expr: &CompiledExpr) -> bool {
    if let CompiledExprKind::FunctionCall { function, .. } = &expr.kind {
        matches!(
            crate::units::topology_selector_result_type(&function.name),
            Some(reify_core::Type::Selector(_))
        )
    } else {
        false
    }
}
