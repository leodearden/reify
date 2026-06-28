//! Structural-query placeholder expansion for `self.children` / `self.members`
//! (task 3985, β).
//!
//! The compiler lowers `self.children`, `self.members`, and `self.descendants`
//! in a structure body to `CompiledExprKind::MethodCall { object:
//! ValueRef(ValueCellId(entity, "__self")), method: "children"|"members"|
//! "descendants", args: [], ... }` (STRUCTURAL_QUERY_ACCESSORS, α task #3982).
//!
//! There is no `__self` value cell at eval time — the generic
//! `eval_method_call` path returns `Undef` for an unknown object.  This module
//! provides a **pre-eval tree-walk rewrite** that replaces the placeholder with
//! a concrete `list_literal` BEFORE the containing cell is evaluated, mirroring
//! `expand_purpose_reflective_placeholders` in `engine_purposes.rs`.
//!
//! The rewrite is applied as a new pass AFTER the sub-component elaboration
//! loop in `Engine::eval` (`engine_eval.rs`), where every `__count_{sub}`
//! collection-count cell has already been populated.
//!
//! ## Semantics (PRD §2.1/§2.4)
//!
//! - **`children`**: one element per `sub_components` entry in declaration
//!   order.  Collection subs contribute **one slot** (not flattened).  Aux subs
//!   are included (aux is a surfacing axis, not a membership axis — PRD §3).
//!
//! - **`members`**: non-collection sub → 1 element; collection sub → N
//!   elements (N = `__count_{sub}` value; absent/Undef → 0, never panic).  Aux
//!   subs included identically.
//!
//! - **`descendants`**: OUT OF SCOPE for β (task γ).  The β dispatch matches
//!   only `"children"` and `"members"`; `"descendants"` placeholders remain
//!   unexpanded.
//!
//! ## Element representation
//!
//! Each enumerated entity is
//! `CompiledExpr::literal(Value::String(entity_path), Type::StructureRef(type_name))`.
//! `Value::String` is always determined, so `count` over the resulting
//! `Value::List` returns `Int(len)` rather than `Undef`.  Entity paths follow
//! the existing scoped scheme (`{parent}.{sub}`, `{parent}.{sub}[{idx}]`) to
//! align with elaborated sub-component entity names.
//!
//! `CompiledExpr::list_literal` is used (NOT `reflective_cell_list`, which
//! `debug_assert!`s all elements are `ValueRef`s — reify-ir/src/expr.rs:1093).

use std::collections::HashMap;

use reify_compiler::{CompiledTrait, TopologyTemplate, find_template, satisfies_trait_bound};
use reify_core::{Diagnostic, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, Value, ValueMap};

/// Build a `HashMap<String, &CompiledTrait>` from an ordered sequence of
/// trait definitions.
///
/// Callers should pass prelude-module traits first, then module-level traits,
/// so that module traits shadow same-named prelude traits (last-write wins).
/// This matches the canonical pattern in `engine_constraints.rs:1504-1511`.
///
/// NOTE: `engine_constraints.rs` also builds an equivalent registry inline.
/// Updating that site to call this helper is a cross-file refactor deferred
/// to a follow-up (that file is not in this task's scope).
pub(crate) fn build_trait_registry<'a>(
    all_trait_defs: impl Iterator<Item = &'a CompiledTrait>,
) -> HashMap<String, &'a CompiledTrait> {
    all_trait_defs.map(|t| (t.name.clone(), t)).collect()
}

/// Build a single entity-reference element for a structural-query list.
///
/// Returns `CompiledExpr::literal(Value::String(entity_path),
/// Type::StructureRef(type_name))`.  `Value::String` is always determined, so
/// `count` over a list of these elements returns `Int(len)` rather than `Undef`.
pub(crate) fn entity_ref_element(entity_path: String, type_name: &str) -> CompiledExpr {
    CompiledExpr::literal(
        Value::String(entity_path),
        Type::StructureRef(type_name.to_string()),
    )
}

/// Enumerate the `children` of a template: one element per `sub_components`
/// entry in declaration order.
///
/// Collection subs contribute **one slot** (not flattened).  Aux subs are
/// included.  Entity path: `{template.name}.{sub.name}`.
pub(crate) fn enumerate_children(template: &TopologyTemplate) -> Vec<CompiledExpr> {
    template
        .sub_components
        .iter()
        .map(|sub| {
            let entity_path = format!("{}.{}", template.name, sub.name);
            entity_ref_element(entity_path, &sub.structure_name)
        })
        .collect()
}

/// Enumerate the `members` of a template: non-collection subs → 1 element;
/// collection subs → N elements flattened.
///
/// `N` is read from `values` via `sub.count_cell` exactly as the sub-
/// elaboration loop does.  An absent count cell, `None`, or a non-`Int` value
/// yields N = 0 (no elements, no panic) — PRD §2.4 undef-determinacy rule.
/// Aux subs are included identically.
///
/// Entity paths:
/// - non-collection: `{template.name}.{sub.name}`
/// - collection instance i: `{template.name}.{sub.name}[{i}]`
pub(crate) fn enumerate_members(template: &TopologyTemplate, values: &ValueMap) -> Vec<CompiledExpr> {
    let mut result = Vec::new();
    for sub in &template.sub_components {
        if sub.is_collection {
            let n: i64 = match &sub.count_cell {
                Some(count_cell_id) => match values.get(count_cell_id) {
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                },
                None => 0,
            };
            for idx in 0..n {
                let entity_path = format!("{}.{}[{}]", template.name, sub.name, idx);
                result.push(entity_ref_element(entity_path, &sub.structure_name));
            }
        } else {
            let entity_path = format!("{}.{}", template.name, sub.name);
            result.push(entity_ref_element(entity_path, &sub.structure_name));
        }
    }
    result
}

/// Enumerate the `descendants` of a template: a pre-order DFS over the
/// containment tree.
///
/// Each node is emitted BEFORE its children (pre-order), in declaration
/// order at each level.  Aux subs are included; collection subs are not
/// flattened yet (step-4 adds that arm — task #3988 γ).
///
/// Entity paths compose an instance-path prefix: `{prefix}.{sub.name}` for
/// non-collection subs, with the node path becoming the prefix for recursion.
/// This ensures paths are unique even when two subs share the same type.
///
/// Recursion is bounded by `max_depth` and `node_budget`:
/// - If `depth >= max_depth` at entry, push a Diagnostic::error mentioning
///   "depth" and return empty (never panic).
/// - If `*node_budget == 0` before emitting a node, push an error and return.
/// - Calling with an unknown `structure_name` (no matching template) silently
///   stops that branch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn enumerate_descendants(
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
    values: &ValueMap,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    node_budget: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<CompiledExpr> {
    if depth >= max_depth {
        diagnostics.push(Diagnostic::error(format!(
            "self.descendants: max depth {} exceeded at '{}'; truncating",
            max_depth, prefix
        )));
        return Vec::new();
    }

    let mut result = Vec::new();
    for sub in &template.sub_components {
        if sub.is_collection {
            // Flatten collection sub: emit one entity-ref per instance, then
            // recurse into each instance's sub-template (mirroring
            // enumerate_members' count-cell read + undef→0 pattern).
            let n: i64 = match &sub.count_cell {
                Some(count_cell_id) => match values.get(count_cell_id) {
                    Some(Value::Int(n)) => *n,
                    _ => 0,
                },
                None => 0,
            };
            for idx in 0..n {
                if *node_budget == 0 {
                    diagnostics.push(Diagnostic::error(format!(
                        "self.descendants: node budget exhausted at depth {} ('{}'); truncating",
                        depth, prefix
                    )));
                    return result;
                }
                *node_budget -= 1;
                let node_path = format!("{}.{}[{}]", prefix, sub.name, idx);
                result.push(entity_ref_element(node_path.clone(), &sub.structure_name));
                if let Some(child_tmpl) = find_template(all_templates, &sub.structure_name) {
                    let mut child = enumerate_descendants(
                        child_tmpl,
                        all_templates,
                        values,
                        &node_path,
                        depth + 1,
                        max_depth,
                        node_budget,
                        diagnostics,
                    );
                    result.append(&mut child);
                }
            }
            continue;
        }
        if *node_budget == 0 {
            diagnostics.push(Diagnostic::error(format!(
                "self.descendants: node budget exhausted at depth {} ('{}'); truncating",
                depth, prefix
            )));
            return result;
        }
        *node_budget -= 1;
        let node_path = format!("{}.{}", prefix, sub.name);
        result.push(entity_ref_element(node_path.clone(), &sub.structure_name));
        if let Some(child_tmpl) = find_template(all_templates, &sub.structure_name) {
            let mut child = enumerate_descendants(
                child_tmpl,
                all_templates,
                values,
                &node_path,
                depth + 1,
                max_depth,
                node_budget,
                diagnostics,
            );
            result.append(&mut child);
        }
    }
    result
}

/// Returns `true` if `expr` contains any structural-query placeholder
/// (`self.children`, `self.members`, or `self.descendants` MethodCall).
///
/// Used to gate the per-cell clone+expand+eval pass so cells without
/// structural-query placeholders are not re-evaluated.
pub(crate) fn contains_structural_query(expr: &CompiledExpr) -> bool {
    match &expr.kind {
        CompiledExprKind::MethodCall { object, method, .. } => {
            if (method == "children" || method == "members" || method == "descendants") && is_self_ref(object) {
                return true;
            }
            // Recurse into object in case of chained calls (defensive).
            contains_structural_query(object)
            // args are always empty for self.children/self.members, but
            // would need recursion for a future chained-args case.
        }
        // Leaf nodes — no sub-expressions.
        CompiledExprKind::Literal(_)
        | CompiledExprKind::ValueRef(_)
        | CompiledExprKind::CrossSubGeometryRef(_)
        | CompiledExprKind::OptionNone
        | CompiledExprKind::MetaAccess { .. }
        | CompiledExprKind::DeterminacyPredicate { .. }
        | CompiledExprKind::PurposeReflectiveAggregation { .. } => false,
        // Recurse into binary and unary sub-expressions.
        CompiledExprKind::BinOp { left, right, .. } => {
            contains_structural_query(left) || contains_structural_query(right)
        }
        CompiledExprKind::UnOp { operand, .. } => contains_structural_query(operand),
        // Recurse into function call arguments.
        CompiledExprKind::FunctionCall { args, .. }
        | CompiledExprKind::UserFunctionCall { args, .. } => {
            args.iter().any(contains_structural_query)
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            contains_structural_query(condition)
                || contains_structural_query(then_branch)
                || contains_structural_query(else_branch)
        }
        CompiledExprKind::Match { discriminant, arms } => {
            contains_structural_query(discriminant)
                || arms.iter().any(|arm| contains_structural_query(&arm.body))
        }
        CompiledExprKind::Lambda { body, .. } => contains_structural_query(body),
        CompiledExprKind::ListLiteral(elements)
        | CompiledExprKind::SetLiteral(elements)
        | CompiledExprKind::ReflectiveCellList(elements) => {
            elements.iter().any(contains_structural_query)
        }
        CompiledExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(k, v)| contains_structural_query(k) || contains_structural_query(v)),
        CompiledExprKind::IndexAccess { object, index } => {
            contains_structural_query(object) || contains_structural_query(index)
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => contains_structural_query(collection) || contains_structural_query(predicate),
        CompiledExprKind::OptionSome(inner) => contains_structural_query(inner),
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            lower
                .as_ref()
                .is_some_and(|lo| contains_structural_query(lo))
                || upper
                    .as_ref()
                    .is_some_and(|hi| contains_structural_query(hi))
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            contains_structural_query(base) || args.iter().any(contains_structural_query)
        }
        // StructureInstanceCtor: recurse into supplied args and captured defaults.
        // `lets` reference template-local cells and are intentionally NOT traversed
        // (mirrors the walk/collect_value_refs_inner contract — see expr.rs).
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            ordered_args
                .iter()
                .any(|(_, e)| contains_structural_query(e))
                || defaults.iter().any(|(_, e)| contains_structural_query(e))
        }
        CompiledExprKind::ResolveSelector { selector } => contains_structural_query(selector),
    }
}

/// Check if `expr` is a `ValueRef` to the `__self` pseudo-cell.
///
/// The compiler emits `ValueRef(ValueCellId(entity, "__self"))` as the object
/// of structural-query MethodCall nodes.
fn is_self_ref(expr: &CompiledExpr) -> bool {
    matches!(
        &expr.kind,
        CompiledExprKind::ValueRef(id) if id.member == "__self"
    )
}

/// Apply `visit` to every **direct child** of `expr` in declaration order.
///
/// This is the shared traversal skeleton used by both
/// [`expand_structural_query`] and [`apply_trait_filters`].  Factoring the
/// arm-by-arm match here means both passes drive the same exhaustive tree walk
/// through a closure, so adding a new `CompiledExprKind` variant only requires
/// one update here rather than two.
///
/// Leaf nodes (`Literal`, `ValueRef`, `OptionNone`, …) have no children so
/// `visit` is not called for them.  Composite nodes call `visit` on each
/// child in the order they appear in the IR.
fn walk_children_mut(expr: &mut CompiledExpr, visit: &mut impl FnMut(&mut CompiledExpr)) {
    match &mut expr.kind {
        // Leaf nodes — nothing to visit.
        CompiledExprKind::Literal(_)
        | CompiledExprKind::ValueRef(_)
        | CompiledExprKind::CrossSubGeometryRef(_)
        | CompiledExprKind::OptionNone
        | CompiledExprKind::MetaAccess { .. }
        | CompiledExprKind::DeterminacyPredicate { .. }
        | CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        CompiledExprKind::BinOp { left, right, .. } => {
            visit(left);
            visit(right);
        }
        CompiledExprKind::UnOp { operand, .. } => visit(operand),
        CompiledExprKind::FunctionCall { args, .. }
        | CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                visit(arg);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            visit(condition);
            visit(then_branch);
            visit(else_branch);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            visit(discriminant);
            for arm in arms {
                visit(&mut arm.body);
            }
        }
        CompiledExprKind::Lambda { body, .. } => visit(body),
        CompiledExprKind::ListLiteral(elements)
        | CompiledExprKind::SetLiteral(elements)
        | CompiledExprKind::ReflectiveCellList(elements) => {
            for elem in elements {
                visit(elem);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                visit(key);
                visit(val);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            visit(object);
            visit(index);
        }
        // MethodCall: visit object and args.  Structural-query placeholders
        // (`self.children` / `self.members` / `self.descendants`) are detected
        // by the callers BEFORE walk_children_mut is invoked and replaced
        // in-place; the MethodCall case here handles non-placeholder chains.
        CompiledExprKind::MethodCall { object, args, .. } => {
            visit(object);
            for arg in args {
                visit(arg);
            }
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            visit(collection);
            visit(predicate);
        }
        CompiledExprKind::OptionSome(inner) => visit(inner),
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                visit(lo);
            }
            if let Some(hi) = upper {
                visit(hi);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            visit(base);
            for arg in args {
                visit(arg);
            }
        }
        // StructureInstanceCtor: recurse into supplied args and captured
        // defaults.  `lets` reference template-local cells and are NOT
        // traversed (mirrors the walk/collect_value_refs_inner contract).
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, arg) in ordered_args {
                visit(arg);
            }
            for (_, def) in defaults {
                visit(def);
            }
        }
        CompiledExprKind::ResolveSelector { selector } => visit(selector),
    }
}

/// Expand structural-query placeholders (`self.children`, `self.members`,
/// `self.descendants`) in-place within `expr`.
///
/// Recursively walks `expr` and replaces any
/// `MethodCall { object: ValueRef(__self), method: "children"|"members"|"descendants", args: [] }`
/// with a `list_literal` of the corresponding enumerated entities.
///
/// - `"children"` → `enumerate_children(template)` (one slot per sub, aux incl.)
/// - `"members"` → `enumerate_members(template, values)` (flat with count, aux incl.)
/// - `"descendants"` → `enumerate_descendants(...)` (pre-order DFS, depth-guarded)
///
/// The outer list type is `List(StructureRef("Structure"))` as a generic
/// placeholder (the concrete element type is embedded per element).
pub(crate) fn expand_structural_query(
    expr: &mut CompiledExpr,
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
    values: &ValueMap,
    max_depth: usize,
    node_budget: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Detect the structural-query placeholder FIRST so we can replace the
    // whole MethodCall node in-place.  We borrow `expr.kind` immutably to
    // inspect it, then take action below.
    let is_placeholder = matches!(&expr.kind,
        CompiledExprKind::MethodCall { object, method, .. }
        if (method == "children" || method == "members" || method == "descendants") && is_self_ref(object)
    );

    if is_placeholder {
        // Extract method name before we move out of expr.kind.
        let method_str = match &expr.kind {
            CompiledExprKind::MethodCall { method, .. } => method.clone(),
            _ => unreachable!(),
        };
        let elements = if method_str == "children" {
            enumerate_children(template)
        } else if method_str == "descendants" {
            enumerate_descendants(
                template,
                all_templates,
                values,
                &template.name,
                0,
                max_depth,
                node_budget,
                diagnostics,
            )
        } else {
            enumerate_members(template, values)
        };
        *expr = CompiledExpr::list_literal(
            elements,
            Type::List(Box::new(Type::StructureRef("Structure".to_string()))),
        );
        return;
    }

    // Recurse into sub-expressions using the shared walker.  MethodCall nodes
    // that are NOT structural-query placeholders are also walked here (object +
    // args), mirroring `expand_purpose_reflective_placeholders`.
    walk_children_mut(expr, &mut |child| {
        expand_structural_query(child, template, all_templates, values, max_depth, node_budget, diagnostics);
    });
}

/// Apply trait-conformance filters to `filter(list_literal, TraitObject-marker)`
/// nodes produced by the compiler intercept (task 3991, δ).
///
/// Recursively walks `expr` (same arm coverage as `expand_structural_query`).
/// When a `FunctionCall { name == "filter", args: [a0, a1] }` is found where
/// `a0.kind` is a `ListLiteral(elems)` and `a1.result_type` is
/// `Type::TraitObject(trait_name)`, rewrites the node to a
/// `list_literal(kept, a0.result_type)` where `kept` is the subset of
/// elements in source order whose `result_type == Type::StructureRef(tn)` and
/// whose structure conforms to `trait_name`.
///
/// This pass MUST run AFTER `expand_structural_query` so the `self.descendants`
/// placeholder has already been rewritten to a list_literal of entity-refs
/// (which carry per-element `Type::StructureRef` in their `result_type`).
///
/// # Conformance check (step-6: TRANSITIVE via `satisfies_trait_bound`)
///
/// Element conforms iff `satisfies_trait_bound(&template.trait_bounds,
/// trait_name, trait_registry)` returns true.  This walks refinement chains
/// through `trait_registry` (e.g. `Bolt : Fastener` means a `Bolt`-bounded
/// structure also satisfies a filter for `Fastener`).
pub(crate) fn apply_trait_filters(
    expr: &mut CompiledExpr,
    all_templates: &[TopologyTemplate],
    trait_registry: &HashMap<String, &CompiledTrait>,
) {
    // Post-order walk: recurse into children FIRST so nested
    // `filter(filter(self.descendants, Bolt), Fastener)` is resolved bottom-up.
    // After this call, any inner filter whose arg0 was a FunctionCall has been
    // rewritten to a list_literal, making the outer detection check below correct.
    walk_children_mut(expr, &mut |child| {
        apply_trait_filters(child, all_templates, trait_registry);
    });

    // Detect `filter(list_literal, TraitObject-marker)` at this node.
    // Because walk_children_mut already processed args, an inner filter(…) that
    // was the arg0 of an outer filter is now a list_literal here.
    let is_filter_call = matches!(&expr.kind,
        CompiledExprKind::FunctionCall { function, args }
        if function.name == "filter"
            && args.len() == 2
            && matches!(&args[0].kind, CompiledExprKind::ListLiteral(_))
            && matches!(&args[1].result_type, Type::TraitObject(_))
    );

    if is_filter_call {
        // Extract trait name and list elements.
        let (elems, list_type, trait_name) = match &expr.kind {
            CompiledExprKind::FunctionCall { args, .. } => {
                let list_type = args[0].result_type.clone();
                let trait_name = match &args[1].result_type {
                    Type::TraitObject(t) => t.clone(),
                    _ => unreachable!(),
                };
                let elems = match &args[0].kind {
                    CompiledExprKind::ListLiteral(e) => e.clone(),
                    _ => unreachable!(),
                };
                (elems, list_type, trait_name)
            }
            _ => unreachable!(),
        };

        // Filter elements: keep those that conform to `trait_name`.
        // TRANSITIVE conformance via satisfies_trait_bound (walks refinement
        // chains through trait_registry, e.g. Bolt : Fastener).
        let kept: Vec<CompiledExpr> = elems
            .into_iter()
            .filter(|e| {
                if let Type::StructureRef(tn) = &e.result_type {
                    find_template(all_templates, tn)
                        .map(|t| {
                            satisfies_trait_bound(&t.trait_bounds, &trait_name, trait_registry)
                        })
                        .unwrap_or(false)
                } else {
                    false
                }
            })
            .collect();

        *expr = CompiledExpr::list_literal(kept, list_type);
    }
}

/// Expand structural-query placeholders (`self.members`, `self.descendants`,
/// etc.) inside a single constraint expression, then apply trait-conformance
/// filters.
///
/// Returns `None` when `expr` contains no structural-query placeholders (fast
/// path, avoids a clone).  Returns `Some(expanded)` when at least one
/// placeholder was found and replaced.
///
/// This is the shared expansion contract used by both constraint-expansion
/// sites so that a future tweak (e.g. node-budget logic, diagnostic handling)
/// stays in one place and neither site can drift independently:
///
/// - `engine_eval.rs` (ε, task 3992): rewrites `snapshot.graph.constraints`
///   in-place after the Let-cell expansion pass.
/// - `engine_constraints.rs` (ε, task 3992): produces per-constraint expanded
///   copies for dispatch without mutating the compiled module.
///
/// Callers hold a pre-built `trait_registry` (from `build_trait_registry`) and
/// pass `max_nodes` as a per-call value (not a shared counter), so each
/// constraint gets its own fresh budget rather than inheriting exhaustion from a
/// prior iteration.
#[allow(clippy::too_many_arguments)]
pub(crate) fn expand_constraint_expr(
    expr: &CompiledExpr,
    template: &TopologyTemplate,
    all_templates: &[TopologyTemplate],
    values: &ValueMap,
    max_depth: usize,
    max_nodes: usize,
    trait_registry: &HashMap<String, &CompiledTrait>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledExpr> {
    if !contains_structural_query(expr) {
        return None;
    }
    let mut expanded = expr.clone();
    let mut node_budget = max_nodes;
    expand_structural_query(
        &mut expanded,
        template,
        all_templates,
        values,
        max_depth,
        &mut node_budget,
        diagnostics,
    );
    apply_trait_filters(&mut expanded, all_templates, trait_registry);
    Some(expanded)
}
