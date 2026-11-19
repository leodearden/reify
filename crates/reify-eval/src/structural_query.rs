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
use std::ops::ControlFlow;

use reify_compiler::{CompiledTrait, TopologyTemplate, find_template, satisfies_trait_bound};
use reify_core::{Diagnostic, DimensionVector, Type, ValueCellId};
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
/// structural-query placeholders are not re-evaluated. It admits every
/// expression `expand_structural_query` would rewrite, because both traverse
/// `CompiledExpr`'s children.
pub(crate) fn contains_structural_query(expr: &CompiledExpr) -> bool {
    is_structural_query_placeholder(expr)
        || expr
            .try_for_each_child(&mut |child| {
                if contains_structural_query(child) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .is_break()
}

/// `self.children` / `self.members` / `self.descendants`, as the compiler
/// lowers it: a MethodCall on the `__self` pseudo-cell.
fn is_structural_query_placeholder(expr: &CompiledExpr) -> bool {
    matches!(&expr.kind,
        CompiledExprKind::MethodCall { object, method, .. }
        if (method == "children" || method == "members" || method == "descendants")
            && is_self_ref(object)
    )
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
    // whole MethodCall node in-place.
    if is_structural_query_placeholder(expr) {
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

    // Recurse into the children: a MethodCall that is NOT a placeholder has
    // its object and args walked here, as `contains_structural_query` assumes.
    expr.for_each_child_mut(&mut |child| {
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
    expr.for_each_child_mut(&mut |child| {
        apply_trait_filters(child, all_templates, trait_registry);
    });

    // Detect `filter(list_literal, TraitObject-marker)` at this node.
    // Because the children (args included) were processed above, an inner filter(…) that
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

/// Apply the `cost(entity_ref_list)` subtree-cost aggregation builtin (task
/// 5015, M-WHOLE γ) to `FunctionCall { name == "cost", args: [a0] }` nodes
/// produced by the compiler intercept (`crates/reify-compiler/src/expr.rs`).
///
/// Recursively walks `expr` (same arm coverage as `expand_structural_query` /
/// `apply_trait_filters`; MUST run AFTER both, so `self.descendants` / any
/// explicit `filter(...)` has already been reduced to a `ListLiteral` of
/// entity-ref literals).
///
/// When `a0.kind` is a `ListLiteral(elems)`, each element
/// `Literal(Value::String(path), StructureRef(_))` is rewritten to
/// `ValueRef(ValueCellId(path, "line_cost"))` typed `Scalar<MONEY>` — EXACTLY
/// what `self.<sub>.line_cost` compiles to today (expr.rs:3623-3661), so it
/// resolves against the scoped cell elaboration already populates for
/// depth-1 subs and collection elements. The refs are collected into a
/// `List<Scalar<MONEY>>` list_literal and wrapped in a `MethodCall { method:
/// "sum" }` typed `Scalar<MONEY>` so the landed `.sum` reduction
/// (reify-expr/src/lib.rs:3523) does the addition — inheriting Money-
/// dimension preservation, Undef-poisoning, and empty-list -> typed-zero
/// (`Scalar{0.0, MONEY}`, not Undef / dimensionless `Real(0)`).
///
/// Elements are first filtered to `Costed`-conformers — identical
/// conformance logic to [`apply_trait_filters`] (`find_template` +
/// `satisfies_trait_bound(&t.trait_bounds, "Costed", trait_registry)`), so a
/// non-Costed structural node is dropped BEFORE any `line_cost` ValueRef is
/// emitted and can never poison the sum with Undef. `cost(...)` performs
/// this filter internally, independent of any explicit `filter(_, Costed)`
/// the user already wrote — the double Costed filter is idempotent.
pub(crate) fn apply_cost_aggregation(
    expr: &mut CompiledExpr,
    all_templates: &[TopologyTemplate],
    trait_registry: &HashMap<String, &CompiledTrait>,
) {
    // Post-order walk: recurse into children FIRST, mirroring apply_trait_filters.
    expr.for_each_child_mut(&mut |child| {
        apply_cost_aggregation(child, all_templates, trait_registry);
    });

    let is_cost_call = matches!(&expr.kind,
        CompiledExprKind::FunctionCall { function, args }
        if function.name == "cost"
            && args.len() == 1
            && matches!(&args[0].kind, CompiledExprKind::ListLiteral(_))
    );

    if !is_cost_call {
        return;
    }

    let elems = match &expr.kind {
        CompiledExprKind::FunctionCall { args, .. } => match &args[0].kind {
            CompiledExprKind::ListLiteral(e) => e.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };

    let money_scalar = Type::Scalar {
        dimension: DimensionVector::MONEY,
    };

    // Costed-conformance filter (same check as apply_trait_filters, hard-coded
    // to "Costed"): keep only descendants whose structure conforms, so a
    // non-Costed node's absent line_cost cell never poisons the sum.
    let kept: Vec<CompiledExpr> = elems
        .into_iter()
        .filter(|e| {
            if let Type::StructureRef(tn) = &e.result_type {
                find_template(all_templates, tn)
                    .map(|t| satisfies_trait_bound(&t.trait_bounds, "Costed", trait_registry))
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .collect();

    let refs: Vec<CompiledExpr> = kept
        .into_iter()
        .map(|e| match e.kind {
            CompiledExprKind::Literal(Value::String(path)) => {
                CompiledExpr::value_ref(ValueCellId::new(path, "line_cost"), money_scalar.clone())
            }
            _ => unreachable!("entity_ref_element always emits Literal(Value::String(_))"),
        })
        .collect();

    let list = CompiledExpr::list_literal(refs, Type::List(Box::new(money_scalar.clone())));
    *expr = CompiledExpr::method_call(list, "sum".to_string(), vec![], money_scalar);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn structure_list_type() -> Type {
        Type::List(Box::new(Type::StructureRef("Structure".into())))
    }

    /// `self.children` exactly as the compiler emits it.
    fn placeholder() -> CompiledExpr {
        CompiledExpr::method_call(
            CompiledExpr::value_ref(
                ValueCellId::new("S", "__self"),
                Type::StructureRef("S".into()),
            ),
            "children".into(),
            vec![],
            structure_list_type(),
        )
    }

    #[test]
    fn gate_admits_a_placeholder_nested_in_a_method_call_argument() {
        let call = CompiledExpr::method_call(
            CompiledExpr::list_literal(vec![], structure_list_type()),
            "contains".into(),
            vec![placeholder()],
            Type::Bool,
        );
        assert!(contains_structural_query(&call));
    }

    #[test]
    fn gate_admits_a_bare_placeholder_and_one_under_a_method_call_object() {
        assert!(contains_structural_query(&placeholder()));
        let len = CompiledExpr::method_call(placeholder(), "len".into(), vec![], Type::Int);
        assert!(contains_structural_query(&len));
    }

    #[test]
    fn gate_rejects_an_expression_with_no_placeholder() {
        let other = |member: &str| {
            CompiledExpr::value_ref(
                ValueCellId::new("S", member),
                Type::StructureRef("S".into()),
            )
        };
        let call = CompiledExpr::method_call(
            other("xs"),
            "m".into(),
            vec![CompiledExpr::literal(Value::Int(1), Type::Int)],
            Type::Int,
        );
        assert!(!contains_structural_query(&call));
        let children_of_non_self = CompiledExpr::method_call(
            other("xs"),
            "children".into(),
            vec![],
            structure_list_type(),
        );
        assert!(!contains_structural_query(&children_of_non_self));
    }
}
