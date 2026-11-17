//! `CompiledExpr::try_for_each_child` is the one statement of each
//! `CompiledExprKind` variant's direct child expressions; `for_each_child`
//! derives from it and `for_each_child_mut` mirrors it arm for arm.
//!
//! Every corpus exemplar's direct children are `ValueRef` leaves labelled by
//! their cell member (`c0`, `c1`, …), and the expected child sequence is
//! written by hand, so the accessors are pinned by something other than
//! themselves. The variant-coverage guard makes a new variant fail to compile
//! here until it is named, and fail `corpus_has_an_exemplar_for_every_variant`
//! until it gains an exemplar.

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use reify_ast::QuantifierKind;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledMatchArm, CompiledPattern,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind, StructureTypeId, UnOp, Value,
};

const STRUCT_TYPE: &str = "S";

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("E", member)
}

fn struct_local_cell() -> ValueCellId {
    ValueCellId::new(STRUCT_TYPE, "let_local")
}

fn cross_sub_cell() -> ValueCellId {
    ValueCellId::new("P.sub", "geom")
}

fn scalar() -> Type {
    Type::dimensionless_scalar()
}

fn r(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(cell(member), scalar())
}

fn node(kind: CompiledExprKind) -> CompiledExpr {
    CompiledExpr {
        kind,
        result_type: Type::Bool,
        content_hash: ContentHash::of(&[]),
    }
}

fn function_call(args: Vec<CompiledExpr>) -> CompiledExpr {
    node(CompiledExprKind::FunctionCall {
        function: ResolvedFunction {
            name: "max".to_string(),
            qualified_name: "std::math::max".to_string(),
        },
        args,
    })
}

fn add(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, left, right, scalar())
}

fn list_type() -> Type {
    Type::List(Box::new(scalar()))
}

fn structure_ctor_with_lets() -> CompiledExpr {
    CompiledExpr::structure_instance_ctor(
        StructureTypeId(0),
        STRUCT_TYPE.to_string(),
        1,
        vec![("x".to_string(), r("c0"))],
        vec![("y".to_string(), r("c1"))],
        vec![(
            "l".to_string(),
            CompiledExpr::value_ref(struct_local_cell(), scalar()),
        )],
        None,
        Type::Bool,
    )
}

fn lets_of(expr: &CompiledExpr) -> &[(String, CompiledExpr)] {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor { lets, .. } => lets,
        other => panic!("expected a StructureInstanceCtor, got {}", variant_name(other)),
    }
}

fn let_cell(expr: &CompiledExpr) -> &ValueCellId {
    match &lets_of(expr)[0].1.kind {
        CompiledExprKind::ValueRef(id) => id,
        other => panic!("expected the let to be a ValueRef, got {}", variant_name(other)),
    }
}

fn variant_name(kind: &CompiledExprKind) -> &'static str {
    match kind {
        CompiledExprKind::Literal(_) => "Literal",
        CompiledExprKind::ValueRef(_) => "ValueRef",
        CompiledExprKind::BinOp { .. } => "BinOp",
        CompiledExprKind::UnOp { .. } => "UnOp",
        CompiledExprKind::FunctionCall { .. } => "FunctionCall",
        CompiledExprKind::Conditional { .. } => "Conditional",
        CompiledExprKind::Match { .. } => "Match",
        CompiledExprKind::UserFunctionCall { .. } => "UserFunctionCall",
        CompiledExprKind::Lambda { .. } => "Lambda",
        CompiledExprKind::ListLiteral(_) => "ListLiteral",
        CompiledExprKind::SetLiteral(_) => "SetLiteral",
        CompiledExprKind::MapLiteral(_) => "MapLiteral",
        CompiledExprKind::IndexAccess { .. } => "IndexAccess",
        CompiledExprKind::MethodCall { .. } => "MethodCall",
        CompiledExprKind::Quantifier { .. } => "Quantifier",
        CompiledExprKind::OptionSome(_) => "OptionSome",
        CompiledExprKind::OptionNone => "OptionNone",
        CompiledExprKind::MetaAccess { .. } => "MetaAccess",
        CompiledExprKind::DeterminacyPredicate { .. } => "DeterminacyPredicate",
        CompiledExprKind::RangeConstructor { .. } => "RangeConstructor",
        CompiledExprKind::AdHocSelector { .. } => "AdHocSelector",
        CompiledExprKind::PurposeReflectiveAggregation { .. } => "PurposeReflectiveAggregation",
        CompiledExprKind::ReflectiveCellList(_) => "ReflectiveCellList",
        CompiledExprKind::CrossSubGeometryRef(_) => "CrossSubGeometryRef",
        CompiledExprKind::StructureInstanceCtor { .. } => "StructureInstanceCtor",
        CompiledExprKind::VariantCtor { .. } => "VariantCtor",
        CompiledExprKind::ResolveSelector { .. } => "ResolveSelector",
    }
}

const EVERY_VARIANT: [&str; 27] = [
    "Literal",
    "ValueRef",
    "BinOp",
    "UnOp",
    "FunctionCall",
    "Conditional",
    "Match",
    "UserFunctionCall",
    "Lambda",
    "ListLiteral",
    "SetLiteral",
    "MapLiteral",
    "IndexAccess",
    "MethodCall",
    "Quantifier",
    "OptionSome",
    "OptionNone",
    "MetaAccess",
    "DeterminacyPredicate",
    "RangeConstructor",
    "AdHocSelector",
    "PurposeReflectiveAggregation",
    "ReflectiveCellList",
    "CrossSubGeometryRef",
    "StructureInstanceCtor",
    "VariantCtor",
    "ResolveSelector",
];

/// A `ValueRef` child is labelled by its cell member; any other node by its
/// variant name in angle brackets.
fn label(expr: &CompiledExpr) -> String {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => id.member.clone(),
        other => format!("<{}>", variant_name(other)),
    }
}

fn child_labels(expr: &CompiledExpr) -> Vec<String> {
    let mut labels = Vec::new();
    expr.for_each_child(&mut |child| labels.push(label(child)));
    labels
}

/// One exemplar and its direct children in evaluation order, written by hand.
struct Case {
    name: &'static str,
    expr: CompiledExpr,
    children: Vec<&'static str>,
}

fn case(name: &'static str, expr: CompiledExpr, children: &[&'static str]) -> Case {
    Case {
        name,
        expr,
        children: children.to_vec(),
    }
}

fn corpus() -> Vec<Case> {
    vec![
        case(
            "literal",
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
            &[],
        ),
        case("value_ref", r("leaf"), &[]),
        case(
            "cross_sub_geometry_ref",
            CompiledExpr::cross_sub_geometry_ref(cross_sub_cell(), Type::Geometry),
            &[],
        ),
        case(
            "option_none",
            CompiledExpr::option_none(Type::Option(Box::new(scalar()))),
            &[],
        ),
        case(
            "meta_access",
            CompiledExpr::meta_access("E".to_string(), "k".to_string()),
            &[],
        ),
        case(
            "determinacy_predicate_probed_cell_is_not_a_child",
            CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell("d")),
            &[],
        ),
        case(
            "purpose_reflective_aggregation",
            CompiledExpr::purpose_reflective_aggregation(
                "subject".to_string(),
                "params".to_string(),
                Type::Bool,
            ),
            &[],
        ),
        case("binop", add(r("c0"), r("c1")), &["c0", "c1"]),
        case(
            "unop",
            CompiledExpr::unop(UnOp::Neg, r("c0"), scalar()),
            &["c0"],
        ),
        case(
            "function_call",
            function_call(vec![r("c0"), r("c1")]),
            &["c0", "c1"],
        ),
        case(
            "user_function_call",
            CompiledExpr::user_function_call("f".to_string(), vec![r("c0"), r("c1")], scalar()),
            &["c0", "c1"],
        ),
        case(
            "conditional",
            node(CompiledExprKind::Conditional {
                condition: Box::new(r("c0")),
                then_branch: Box::new(r("c1")),
                else_branch: Box::new(r("c2")),
            }),
            &["c0", "c1", "c2"],
        ),
        case(
            "match_discriminant_then_arm_bodies_not_binders",
            CompiledExpr::match_expr(
                r("c0"),
                vec![
                    CompiledMatchArm {
                        patterns: vec![CompiledPattern::variant("X")],
                        body: r("c1"),
                    },
                    CompiledMatchArm {
                        patterns: vec![CompiledPattern::VariantBind {
                            name: "Y".to_string(),
                            binders: vec![("field".to_string(), cell("y_bind"))],
                        }],
                        body: r("c2"),
                    },
                ],
                scalar(),
            ),
            &["c0", "c1", "c2"],
        ),
        case(
            "lambda_body_only_not_captures_or_params",
            CompiledExpr::lambda(
                vec![("p".to_string(), None)],
                vec![cell("p")],
                r("c0"),
                vec![cell("k")],
                Type::Bool,
            ),
            &["c0"],
        ),
        case(
            "list_literal",
            CompiledExpr::list_literal(vec![r("c0"), r("c1")], list_type()),
            &["c0", "c1"],
        ),
        case(
            "set_literal",
            CompiledExpr::set_literal(vec![r("c0"), r("c1")], Type::Set(Box::new(scalar()))),
            &["c0", "c1"],
        ),
        case(
            "reflective_cell_list",
            CompiledExpr::reflective_cell_list(vec![r("c0"), r("c1")], list_type()),
            &["c0", "c1"],
        ),
        case(
            "map_literal_key_then_value_per_entry",
            CompiledExpr::map_literal(
                vec![(r("c0"), r("c1")), (r("c2"), r("c3"))],
                Type::Bool,
            ),
            &["c0", "c1", "c2", "c3"],
        ),
        case(
            "index_access",
            CompiledExpr::index_access(r("c0"), r("c1"), scalar()),
            &["c0", "c1"],
        ),
        case(
            "method_call_object_then_args",
            CompiledExpr::method_call(r("c0"), "m".to_string(), vec![r("c1"), r("c2")], scalar()),
            &["c0", "c1", "c2"],
        ),
        case(
            "quantifier_collection_then_predicate",
            CompiledExpr::quantifier(
                QuantifierKind::ForAll,
                "v".to_string(),
                cell("v"),
                r("c0"),
                r("c1"),
            ),
            &["c0", "c1"],
        ),
        case(
            "option_some",
            CompiledExpr::option_some(r("c0"), Type::Option(Box::new(scalar()))),
            &["c0"],
        ),
        case(
            "range_constructor_both_bounds",
            CompiledExpr::range_constructor(Some(r("c0")), Some(r("c1")), true, false, Type::Bool),
            &["c0", "c1"],
        ),
        case(
            "range_constructor_lower_only",
            CompiledExpr::range_constructor(Some(r("c0")), None, true, false, Type::Bool),
            &["c0"],
        ),
        case(
            "range_constructor_upper_only",
            CompiledExpr::range_constructor(None, Some(r("c1")), false, true, Type::Bool),
            &["c1"],
        ),
        case(
            "ad_hoc_selector_base_then_args",
            CompiledExpr::ad_hoc_selector(r("c0"), SelectorKind::Point, vec![r("c1"), r("c2")]),
            &["c0", "c1", "c2"],
        ),
        case(
            "structure_instance_ctor_excludes_its_lets",
            structure_ctor_with_lets(),
            &["c0", "c1"],
        ),
        case(
            "variant_ctor",
            CompiledExpr::variant_ctor(
                "Shape".to_string(),
                "Rect".to_string(),
                vec![("x".to_string(), r("c0")), ("y".to_string(), r("c1"))],
                Type::Enum("Shape".to_string()),
            ),
            &["c0", "c1"],
        ),
        case(
            "resolve_selector",
            CompiledExpr::resolve_selector(r("c0")),
            &["c0"],
        ),
    ]
}

#[test]
fn corpus_has_an_exemplar_for_every_variant() {
    let covered: BTreeSet<&str> = corpus()
        .iter()
        .map(|case| variant_name(&case.expr.kind))
        .collect();
    let every: BTreeSet<&str> = EVERY_VARIANT.into_iter().collect();
    assert_eq!(covered, every);
}

#[test]
fn for_each_child_yields_each_variants_direct_children_in_order() {
    for case in corpus() {
        assert_eq!(child_labels(&case.expr), case.children, "case `{}`", case.name);
    }
}

#[test]
fn try_for_each_child_matches_for_each_child_and_stops_at_first_break() {
    for case in corpus() {
        let mut labels = Vec::new();
        let flow = case.expr.try_for_each_child::<()>(&mut |child| {
            labels.push(label(child));
            ControlFlow::Continue(())
        });
        assert_eq!(flow, ControlFlow::Continue(()), "case `{}`", case.name);
        assert_eq!(labels, case.children, "case `{}`", case.name);
    }

    let call = function_call(vec![r("c0"), r("c1"), r("c2")]);
    let mut consulted = Vec::new();
    let flow = call.try_for_each_child(&mut |child| {
        let child_label = label(child);
        consulted.push(child_label.clone());
        if child_label == "c1" {
            ControlFlow::Break(child_label)
        } else {
            ControlFlow::Continue(())
        }
    });
    assert_eq!(flow, ControlFlow::Break("c1".to_string()));
    assert_eq!(consulted, ["c0", "c1"]);
}

#[test]
fn for_each_child_mut_reaches_the_same_child_slots() {
    for mut case in corpus() {
        case.expr.for_each_child_mut(&mut |child| {
            if let CompiledExprKind::ValueRef(id) = &mut child.kind {
                id.member.push('\'');
            }
        });
        let primed: Vec<String> = case.children.iter().map(|c| format!("{c}'")).collect();
        assert_eq!(child_labels(&case.expr), primed, "case `{}`", case.name);
    }

    let mut ctor = structure_ctor_with_lets();
    ctor.for_each_child_mut(&mut |child| {
        if let CompiledExprKind::ValueRef(id) = &mut child.kind {
            id.member.push('\'');
        }
    });
    assert_eq!(let_cell(&ctor), &struct_local_cell());
}

#[test]
fn children_are_direct_only() {
    let nested = add(add(r("c0"), r("c1")), r("c2"));
    assert_eq!(child_labels(&nested), ["<BinOp>", "c2"]);
}
