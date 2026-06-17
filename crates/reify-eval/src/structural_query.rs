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

use reify_compiler::TopologyTemplate;
use reify_core::Type;
use reify_ir::{CompiledExpr, CompiledExprKind, Value, ValueMap};

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

/// Returns `true` if `expr` contains any structural-query placeholder
/// (`self.children` or `self.members` MethodCall).
///
/// Used to gate the per-cell clone+expand+eval pass so cells without
/// structural-query placeholders are not re-evaluated.
pub(crate) fn contains_structural_query(expr: &CompiledExpr) -> bool {
    match &expr.kind {
        CompiledExprKind::MethodCall { object, method, .. } => {
            if (method == "children" || method == "members") && is_self_ref(object) {
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
                .map_or(false, |lo| contains_structural_query(lo))
                || upper
                    .as_ref()
                    .map_or(false, |hi| contains_structural_query(hi))
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

/// Expand structural-query placeholders (`self.children`, `self.members`)
/// in-place within `expr`.
///
/// Recursively walks `expr` and replaces any
/// `MethodCall { object: ValueRef(__self), method: "children"|"members", args: [] }`
/// with a `list_literal` of the corresponding enumerated entities.
///
/// - `"children"` → `enumerate_children(template)` (one slot per sub, aux incl.)
/// - `"members"` → `enumerate_members(template, values)` (flat with count, aux incl.)
/// - `"descendants"` → left unexpanded (task γ scope).
///
/// The outer list type is `List(StructureRef("Structure"))` as a generic
/// placeholder (the concrete element type is embedded per element).
pub(crate) fn expand_structural_query(
    expr: &mut CompiledExpr,
    template: &TopologyTemplate,
    values: &ValueMap,
) {
    // Detect the structural-query placeholder FIRST so we can replace the
    // whole MethodCall node in-place.  We borrow `expr.kind` immutably to
    // inspect it, then take action below.
    let is_placeholder = matches!(&expr.kind,
        CompiledExprKind::MethodCall { object, method, .. }
        if (method == "children" || method == "members") && is_self_ref(object)
    );

    if is_placeholder {
        // Extract method name before we move out of expr.kind.
        let method_str = match &expr.kind {
            CompiledExprKind::MethodCall { method, .. } => method.clone(),
            _ => unreachable!(),
        };
        let elements = if method_str == "children" {
            enumerate_children(template)
        } else {
            enumerate_members(template, values)
        };
        *expr = CompiledExpr::list_literal(
            elements,
            Type::List(Box::new(Type::StructureRef("Structure".to_string()))),
        );
        return;
    }

    // Recurse into sub-expressions, mirroring `expand_purpose_reflective_placeholders`.
    match &mut expr.kind {
        // Leaf nodes — no recursion needed.
        CompiledExprKind::Literal(_)
        | CompiledExprKind::ValueRef(_)
        | CompiledExprKind::CrossSubGeometryRef(_)
        | CompiledExprKind::OptionNone
        | CompiledExprKind::MetaAccess { .. }
        | CompiledExprKind::DeterminacyPredicate { .. }
        | CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        CompiledExprKind::BinOp { left, right, .. } => {
            expand_structural_query(left, template, values);
            expand_structural_query(right, template, values);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            expand_structural_query(operand, template, values);
        }
        CompiledExprKind::FunctionCall { args, .. }
        | CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                expand_structural_query(arg, template, values);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            expand_structural_query(condition, template, values);
            expand_structural_query(then_branch, template, values);
            expand_structural_query(else_branch, template, values);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            expand_structural_query(discriminant, template, values);
            for arm in arms {
                expand_structural_query(&mut arm.body, template, values);
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            expand_structural_query(body, template, values);
        }
        CompiledExprKind::ListLiteral(elements)
        | CompiledExprKind::SetLiteral(elements)
        | CompiledExprKind::ReflectiveCellList(elements) => {
            for elem in elements {
                expand_structural_query(elem, template, values);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                expand_structural_query(key, template, values);
                expand_structural_query(val, template, values);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            expand_structural_query(object, template, values);
            expand_structural_query(index, template, values);
        }
        // MethodCall: not a structural-query placeholder (handled above).
        // Recurse into object and args for chained calls.
        CompiledExprKind::MethodCall { object, args, .. } => {
            expand_structural_query(object, template, values);
            for arg in args {
                expand_structural_query(arg, template, values);
            }
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            expand_structural_query(collection, template, values);
            expand_structural_query(predicate, template, values);
        }
        CompiledExprKind::OptionSome(inner) => {
            expand_structural_query(inner, template, values);
        }
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                expand_structural_query(lo, template, values);
            }
            if let Some(hi) = upper {
                expand_structural_query(hi, template, values);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            expand_structural_query(base, template, values);
            for arg in args {
                expand_structural_query(arg, template, values);
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
                expand_structural_query(arg, template, values);
            }
            for (_, def) in defaults {
                expand_structural_query(def, template, values);
            }
        }
        CompiledExprKind::ResolveSelector { selector } => {
            expand_structural_query(selector, template, values);
        }
    }
}
