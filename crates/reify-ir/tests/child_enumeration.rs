//! `CompiledExpr::try_for_each_child` is the one statement of each
//! `CompiledExprKind` variant's direct child expressions; `for_each_child`
//! derives from it and `for_each_child_mut` mirrors it arm for arm.
//!
//! Every corpus exemplar's direct children are `ValueRef` leaves labelled by
//! their cell member (`c0`, `c1`, …), and the expected child sequence is
//! written by hand, so the accessors are pinned by something other than
//! themselves. The expected variant set is `CompiledExprKindDiscriminants::iter()`,
//! which strum derives from `CompiledExprKind`, so a new variant fails
//! `corpus_has_an_exemplar_for_every_variant` until the corpus gains an
//! exemplar for it, and that exemplar pins both `for_each_child` and
//! `for_each_child_mut`.

use std::collections::HashSet;
use std::ops::ControlFlow;

use reify_ast::QuantifierKind;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledMatchArm, CompiledPattern,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind, StructureTypeId, UnOp, Value,
};
use reify_ir::expr::CompiledExprKindDiscriminants;
use strum::IntoEnumIterator;

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

fn value_ref_leaf() -> CompiledExpr {
    r("leaf")
}

fn cross_sub_leaf() -> CompiledExpr {
    CompiledExpr::cross_sub_geometry_ref(cross_sub_cell(), Type::Geometry)
}

fn meta_access_on_e() -> CompiledExpr {
    CompiledExpr::meta_access("E".to_string(), "k".to_string())
}

/// Probes cell `d`.
fn determinacy_on_d() -> CompiledExpr {
    CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell("d"))
}

/// Param id `p`, capture `k`, body `c0`.
fn lambda_capturing_k() -> CompiledExpr {
    CompiledExpr::lambda(
        vec![("p".to_string(), None)],
        vec![cell("p")],
        r("c0"),
        vec![cell("k")],
        Type::Bool,
    )
}

/// Variable id `v`, collection `c0`, predicate `c1`.
fn quantifier_binding_v() -> CompiledExpr {
    CompiledExpr::quantifier(
        QuantifierKind::ForAll,
        "v".to_string(),
        cell("v"),
        r("c0"),
        r("c1"),
    )
}

fn lets_of(expr: &CompiledExpr) -> &[(String, CompiledExpr)] {
    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor { lets, .. } => lets,
        other => panic!(
            "expected a StructureInstanceCtor, got {:?}",
            variant_of(other)
        ),
    }
}

fn let_cell(expr: &CompiledExpr) -> &ValueCellId {
    match &lets_of(expr)[0].1.kind {
        CompiledExprKind::ValueRef(id) => id,
        other => panic!(
            "expected the let to be a ValueRef, got {:?}",
            variant_of(other)
        ),
    }
}

fn variant_of(kind: &CompiledExprKind) -> CompiledExprKindDiscriminants {
    CompiledExprKindDiscriminants::from(kind)
}

/// A `ValueRef` child is labelled by its cell member; any other node by its
/// variant name in angle brackets.
fn label(expr: &CompiledExpr) -> String {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => id.member.clone(),
        other => format!("<{:?}>", variant_of(other)),
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
        case("value_ref", value_ref_leaf(), &[]),
        case("cross_sub_geometry_ref", cross_sub_leaf(), &[]),
        case(
            "option_none",
            CompiledExpr::option_none(Type::Option(Box::new(scalar()))),
            &[],
        ),
        case("meta_access", meta_access_on_e(), &[]),
        case(
            "determinacy_predicate_probed_cell_is_not_a_child",
            determinacy_on_d(),
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
            lambda_capturing_k(),
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
            CompiledExpr::map_literal(vec![(r("c0"), r("c1")), (r("c2"), r("c3"))], Type::Bool),
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
            quantifier_binding_v(),
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
    let covered: HashSet<CompiledExprKindDiscriminants> = corpus()
        .iter()
        .map(|case| variant_of(&case.expr.kind))
        .collect();
    let missing: Vec<_> = CompiledExprKindDiscriminants::iter()
        .filter(|d| !covered.contains(d))
        .collect();
    assert!(
        missing.is_empty(),
        "child_enumeration corpus has no exemplar for CompiledExprKind variant(s) {missing:?}; \
         add a case whose direct children are labelled ValueRefs"
    );
}

#[test]
fn for_each_child_yields_each_variants_direct_children_in_order() {
    for case in corpus() {
        assert_eq!(
            child_labels(&case.expr),
            case.children,
            "case `{}`",
            case.name
        );
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

fn child_cells(expr: &CompiledExpr) -> Vec<ValueCellId> {
    let mut cells = Vec::new();
    expr.for_each_child(&mut |child| match &child.kind {
        CompiledExprKind::ValueRef(id) => cells.push(id.clone()),
        other => panic!("corpus children are ValueRefs, got {:?}", variant_of(other)),
    });
    cells
}

fn ref_cell(expr: &CompiledExpr) -> &ValueCellId {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => id,
        other => panic!("expected a ref leaf, got {:?}", variant_of(other)),
    }
}

/// `(captures, param_ids)`.
fn lambda_cells(expr: &CompiledExpr) -> (&[ValueCellId], &[ValueCellId]) {
    match &expr.kind {
        CompiledExprKind::Lambda {
            captures,
            param_ids,
            ..
        } => (captures, param_ids),
        other => panic!("expected a Lambda, got {:?}", variant_of(other)),
    }
}

fn quantifier_variable(expr: &CompiledExpr) -> &ValueCellId {
    match &expr.kind {
        CompiledExprKind::Quantifier { variable_id, .. } => variable_id,
        other => panic!("expected a Quantifier, got {:?}", variant_of(other)),
    }
}

fn determinacy_cell(expr: &CompiledExpr) -> &ValueCellId {
    match &expr.kind {
        CompiledExprKind::DeterminacyPredicate { cell, .. } => cell,
        other => panic!(
            "expected a DeterminacyPredicate, got {:?}",
            variant_of(other)
        ),
    }
}

fn meta_entity(expr: &CompiledExpr) -> &str {
    match &expr.kind {
        CompiledExprKind::MetaAccess { entity, .. } => entity,
        other => panic!("expected a MetaAccess, got {:?}", variant_of(other)),
    }
}

fn remapped_cell(mut expr: CompiledExpr, from: &ValueCellId, to: &ValueCellId) -> CompiledExpr {
    expr.remap_cell(from, to);
    expr
}

fn remapped_entity(mut expr: CompiledExpr, from: &str, to: &str) -> CompiledExpr {
    expr.remap_entity(from, to);
    expr
}

#[test]
fn walk_visits_each_node_then_its_children_pre_order() {
    for case in corpus() {
        let mut visited = Vec::new();
        case.expr.walk(&mut |n| visited.push(label(n)));
        let mut expected = vec![label(&case.expr)];
        expected.extend(case.children.iter().map(|c| c.to_string()));
        assert_eq!(visited, expected, "case `{}`", case.name);
    }
}

#[test]
fn remap_cell_rewrites_a_matching_cell_in_every_child_slot() {
    for case in corpus() {
        let expr = remapped_cell(case.expr, &cell("c0"), &cell("z"));
        let expected: Vec<&str> = case
            .children
            .iter()
            .map(|&c| if c == "c0" { "z" } else { c })
            .collect();
        assert_eq!(child_labels(&expr), expected, "case `{}`", case.name);
    }

    let ctor = remapped_cell(structure_ctor_with_lets(), &struct_local_cell(), &cell("z"));
    assert_eq!(let_cell(&ctor), &struct_local_cell());
}

#[test]
fn remap_cell_rewrites_each_variants_own_cells() {
    let z = cell("z");

    let lambda = remapped_cell(lambda_capturing_k(), &cell("k"), &z);
    assert_eq!(lambda_cells(&lambda), (&[z.clone()][..], &[cell("p")][..]));
    let lambda = remapped_cell(lambda_capturing_k(), &cell("p"), &z);
    assert_eq!(lambda_cells(&lambda), (&[cell("k")][..], &[z.clone()][..]));

    let quantifier = remapped_cell(quantifier_binding_v(), &cell("v"), &z);
    assert_eq!(quantifier_variable(&quantifier), &z);

    let predicate = remapped_cell(determinacy_on_d(), &cell("d"), &z);
    assert_eq!(determinacy_cell(&predicate), &z);

    let leaf = remapped_cell(value_ref_leaf(), &cell("leaf"), &z);
    assert_eq!(ref_cell(&leaf), &z);

    let geom2 = ValueCellId::new("P.sub", "geom2");
    let cross_sub = remapped_cell(cross_sub_leaf(), &cross_sub_cell(), &geom2);
    assert_eq!(ref_cell(&cross_sub), &geom2);
}

#[test]
fn remap_entity_rewrites_child_cells_and_own_cells() {
    for case in corpus() {
        let expr = remapped_entity(case.expr, "E", "F");
        let cells = child_cells(&expr);
        let members: Vec<&str> = cells.iter().map(|id| id.member.as_str()).collect();
        assert_eq!(members, case.children, "case `{}`", case.name);
        assert!(
            cells.iter().all(|id| id.entity == "F"),
            "case `{}`: {cells:?}",
            case.name
        );
    }

    let f = |member: &str| ValueCellId::new("F", member);

    let lambda = remapped_entity(lambda_capturing_k(), "E", "F");
    assert_eq!(lambda_cells(&lambda), (&[f("k")][..], &[f("p")][..]));

    let quantifier = remapped_entity(quantifier_binding_v(), "E", "F");
    assert_eq!(quantifier_variable(&quantifier), &f("v"));

    let predicate = remapped_entity(determinacy_on_d(), "E", "F");
    assert_eq!(determinacy_cell(&predicate), &f("d"));

    let meta = remapped_entity(meta_access_on_e(), "E", "F");
    assert_eq!(meta_entity(&meta), "F");

    let leaf = remapped_entity(value_ref_leaf(), "E", "F");
    assert_eq!(ref_cell(&leaf), &f("leaf"));

    let cross_sub = remapped_entity(cross_sub_leaf(), "P.sub", "Q.sub");
    assert_eq!(ref_cell(&cross_sub), &ValueCellId::new("Q.sub", "geom"));

    let ctor = remapped_entity(structure_ctor_with_lets(), STRUCT_TYPE, "F");
    assert_eq!(let_cell(&ctor), &struct_local_cell());
}
