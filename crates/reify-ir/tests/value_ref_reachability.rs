//! `CompiledExpr::contains_value_ref_matching` answers "does this expression
//! depend on a cell satisfying `f`?" with exactly the dependency semantics of
//! `collect_value_refs` — which is NOT what a `walk` over `ValueRef` nodes
//! would say — while short-circuiting and allocating nothing.
//!
//! This file is its own test binary, which is what lets it install a counting
//! `#[global_allocator]` without affecting any other test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use reify_ast::QuantifierKind;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledMatchArm, CompiledPattern,
    DeterminacyPredicateKind, ResolvedFunction, SelectorKind, StructureTypeId, UnOp, Value,
};

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS_ON_THIS_THREAD: Cell<u64> = const { Cell::new(0) };
}

fn note_allocation() {
    let _ = ALLOCATIONS_ON_THIS_THREAD.try_with(|n| n.set(n.get() + 1));
}

fn allocations_on_this_thread() -> u64 {
    ALLOCATIONS_ON_THIS_THREAD.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING_ALLOCATOR: CountingAllocator = CountingAllocator;

const STRUCT_TYPE: &str = "S";
const CROSS_SUB_ENTITY: &str = "P.sub";

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("E", member)
}

fn struct_local_cell() -> ValueCellId {
    ValueCellId::new(STRUCT_TYPE, "let_local")
}

fn cross_sub_cell() -> ValueCellId {
    ValueCellId::new(CROSS_SUB_ENTITY, "geom")
}

fn r(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(cell(member), Type::dimensionless_scalar())
}

fn scalar() -> Type {
    Type::dimensionless_scalar()
}

fn node(kind: CompiledExprKind) -> CompiledExpr {
    CompiledExpr {
        kind,
        result_type: Type::Bool,
        content_hash: ContentHash::of(&[]),
    }
}

fn add(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, left, right, scalar())
}

fn list(elements: Vec<CompiledExpr>) -> CompiledExpr {
    CompiledExpr::list_literal(elements, Type::List(Box::new(scalar())))
}

fn lambda(body: CompiledExpr, captures: &[&str]) -> CompiledExpr {
    CompiledExpr::lambda(
        vec![("p".to_string(), None)],
        vec![cell("p")],
        body,
        captures.iter().map(|m| cell(m)).collect(),
        Type::Bool,
    )
}

fn forall(variable: &str, collection: CompiledExpr, predicate: CompiledExpr) -> CompiledExpr {
    CompiledExpr::quantifier(
        QuantifierKind::ForAll,
        variable.to_string(),
        cell(variable),
        collection,
        predicate,
    )
}

/// Bound `v`; the collection references `v` and `c`, the predicate `v` and `f`.
fn quantifier_binding_v() -> CompiledExpr {
    forall("v", list(vec![r("v"), r("c")]), add(r("v"), r("f")))
}

/// Outer binds `v1`, inner binds `v2`; the inner predicate references both
/// bound variables and the free cell `f`.
fn nested_quantifier() -> CompiledExpr {
    forall(
        "v1",
        r("c"),
        forall("v2", r("d"), list(vec![r("v1"), r("v2"), r("f")])),
    )
}

/// Bound `v`; the predicate is a lambda capturing the bound `v` and free `g`.
fn quantifier_over_capturing_lambda() -> CompiledExpr {
    forall("v", r("c"), lambda(r("z"), &["v", "g"]))
}

/// Captures `a` and `b`; the body references the parameter and `z` only.
fn lambda_capturing_a_b() -> CompiledExpr {
    lambda(add(r("p"), r("z")), &["a", "b"])
}

fn structure_ctor_with_lets() -> CompiledExpr {
    CompiledExpr::structure_instance_ctor(
        StructureTypeId(0),
        STRUCT_TYPE.to_string(),
        1,
        vec![("x".to_string(), r("a"))],
        vec![("y".to_string(), r("b"))],
        vec![(
            "l".to_string(),
            CompiledExpr::value_ref(struct_local_cell(), scalar()),
        )],
        None,
        Type::Bool,
    )
}

fn composite() -> CompiledExpr {
    list(vec![
        lambda_capturing_a_b(),
        nested_quantifier(),
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell("d")),
        r("a"),
    ])
}

fn corpus() -> Vec<(&'static str, CompiledExpr)> {
    vec![
        (
            "literal",
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        ),
        ("value_ref", r("a")),
        (
            "cross_sub_geometry_ref",
            CompiledExpr::cross_sub_geometry_ref(cross_sub_cell(), Type::Geometry),
        ),
        ("binop", add(r("a"), r("b"))),
        ("unop", CompiledExpr::unop(UnOp::Neg, r("a"), scalar())),
        (
            "function_call",
            node(CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "sin".to_string(),
                    qualified_name: "std::math::sin".to_string(),
                },
                args: vec![r("a"), r("b")],
            }),
        ),
        (
            "conditional",
            node(CompiledExprKind::Conditional {
                condition: Box::new(r("a")),
                then_branch: Box::new(r("b")),
                else_branch: Box::new(r("c")),
            }),
        ),
        (
            "match",
            CompiledExpr::match_expr(
                r("a"),
                vec![
                    CompiledMatchArm {
                        patterns: vec![CompiledPattern::variant("X")],
                        body: r("b"),
                    },
                    CompiledMatchArm {
                        patterns: vec![CompiledPattern::VariantBind {
                            name: "Y".to_string(),
                            binders: vec![("field".to_string(), cell("y_bind"))],
                        }],
                        body: add(r("y_bind"), r("c")),
                    },
                ],
                scalar(),
            ),
        ),
        (
            "user_function_call",
            CompiledExpr::user_function_call("f".to_string(), vec![r("a"), r("b")], scalar()),
        ),
        ("lambda", lambda_capturing_a_b()),
        ("list_literal", list(vec![r("a"), r("b")])),
        (
            "reflective_cell_list",
            CompiledExpr::reflective_cell_list(
                vec![r("a"), r("b")],
                Type::List(Box::new(scalar())),
            ),
        ),
        (
            "set_literal",
            CompiledExpr::set_literal(vec![r("a"), r("b")], Type::Set(Box::new(scalar()))),
        ),
        (
            "map_literal",
            CompiledExpr::map_literal(vec![(r("a"), r("b")), (r("c"), r("d"))], Type::Bool),
        ),
        (
            "index_access",
            CompiledExpr::index_access(r("a"), r("b"), scalar()),
        ),
        (
            "method_call",
            CompiledExpr::method_call(r("a"), "m".to_string(), vec![r("b"), r("c")], scalar()),
        ),
        ("quantifier", quantifier_binding_v()),
        ("nested_quantifier", nested_quantifier()),
        (
            "quantifier_over_capturing_lambda",
            quantifier_over_capturing_lambda(),
        ),
        (
            "option_some",
            CompiledExpr::option_some(r("a"), Type::Option(Box::new(scalar()))),
        ),
        (
            "option_none",
            CompiledExpr::option_none(Type::Option(Box::new(scalar()))),
        ),
        (
            "meta_access",
            CompiledExpr::meta_access("E".to_string(), "k".to_string()),
        ),
        (
            "determinacy_predicate",
            CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell("d")),
        ),
        (
            "range_constructor",
            CompiledExpr::range_constructor(Some(r("a")), Some(r("b")), true, false, Type::Bool),
        ),
        (
            "range_constructor_upper_only",
            CompiledExpr::range_constructor(None, Some(r("b")), false, true, Type::Bool),
        ),
        (
            "ad_hoc_selector",
            CompiledExpr::ad_hoc_selector(
                r("a"),
                SelectorKind::Point,
                vec![r("b"), r("c"), r("d")],
            ),
        ),
        (
            "purpose_reflective_aggregation",
            CompiledExpr::purpose_reflective_aggregation(
                "subject".to_string(),
                "params".to_string(),
                Type::Bool,
            ),
        ),
        ("structure_instance_ctor", structure_ctor_with_lets()),
        ("resolve_selector", CompiledExpr::resolve_selector(r("a"))),
        ("composite", composite()),
    ]
}

fn universe() -> Vec<ValueCellId> {
    let mut cells: Vec<ValueCellId> = [
        "a",
        "b",
        "c",
        "d",
        "f",
        "g",
        "z",
        "p",
        "v",
        "v1",
        "v2",
        "y_bind",
        "never_used",
    ]
    .iter()
    .map(|m| cell(m))
    .collect();
    cells.push(struct_local_cell());
    cells.push(cross_sub_cell());
    cells
}

fn reaches(expr: &CompiledExpr, target: &ValueCellId) -> bool {
    expr.contains_value_ref_matching(|id| id == target)
}

#[test]
fn the_predicate_is_consulted_on_exactly_the_refs_collect_value_refs_reports() {
    for (name, expr) in corpus() {
        let mut consulted = Vec::new();
        let matched = expr.contains_value_ref_matching(|id| {
            consulted.push(id.clone());
            false
        });
        assert!(
            !matched,
            "{name}: a never-matching predicate must answer false"
        );
        assert_eq!(
            consulted,
            expr.collect_value_refs(),
            "{name}: the predicate must see collect_value_refs' sequence, in order, duplicates kept"
        );
    }
}

#[test]
fn per_cell_membership_agrees_with_collect_value_refs() {
    let universe = universe();
    for (name, expr) in corpus() {
        let collected = expr.collect_value_refs();
        for target in &universe {
            assert_eq!(
                reaches(&expr, target),
                collected.contains(target),
                "{name}: reachability of {target} disagrees with collect_value_refs"
            );
        }
    }
}

#[test]
fn a_lambda_depends_on_its_captures_not_on_its_body() {
    let expr = lambda_capturing_a_b();
    assert!(
        reaches(&expr, &cell("b")),
        "a captured cell is a dependency"
    );
    assert!(
        !reaches(&expr, &cell("z")),
        "a ref only inside the lambda body is not a dependency — a `walk` would wrongly report it"
    );
}

#[test]
fn a_quantifier_hides_its_bound_variable_only_inside_the_predicate() {
    let predicate_only = forall("v", r("c"), add(r("v"), r("f")));
    assert!(
        !reaches(&predicate_only, &cell("v")),
        "the bound variable referenced only in the predicate is not a dependency"
    );
    assert!(
        reaches(&predicate_only, &cell("f")),
        "a free predicate ref is a dependency"
    );
    assert!(
        reaches(&predicate_only, &cell("c")),
        "a collection ref is a dependency"
    );
    assert!(
        reaches(&quantifier_binding_v(), &cell("v")),
        "the bound variable IS a dependency where it occurs in the collection"
    );
}

#[test]
fn nested_quantifiers_hide_every_enclosing_bound_variable() {
    let expr = nested_quantifier();
    assert!(
        !reaches(&expr, &cell("v1")),
        "the outer bound variable must stay hidden"
    );
    assert!(
        !reaches(&expr, &cell("v2")),
        "the inner bound variable must stay hidden"
    );
    assert!(
        reaches(&expr, &cell("f")),
        "the free ref under both binders is a dependency"
    );
}

#[test]
fn a_quantifier_hides_its_bound_variable_from_a_capturing_lambda() {
    let expr = quantifier_over_capturing_lambda();
    assert!(
        !reaches(&expr, &cell("v")),
        "a lambda capture of the bound variable is not a dependency of the quantifier"
    );
    assert!(reaches(&expr, &cell("g")), "a free capture is a dependency");
}

#[test]
fn a_determinacy_predicate_depends_on_its_cell() {
    let expr = CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, cell("d"));
    assert!(
        reaches(&expr, &cell("d")),
        "the probed cell is a dependency — a `walk` over ValueRef nodes never sees it"
    );
}

#[test]
fn stops_at_the_first_matching_ref() {
    let target = cell("a");
    let mut invocations = 0;
    let matched = list(vec![r("a"), r("b"), r("c")]).contains_value_ref_matching(|id| {
        invocations += 1;
        id == &target
    });
    assert!(matched);
    assert_eq!(
        invocations, 1,
        "nothing after the first match may be consulted"
    );

    let target = cell("f");
    let mut invocations = 0;
    let matched =
        forall("v", r("c"), list(vec![r("f"), r("g")])).contains_value_ref_matching(|id| {
            invocations += 1;
            id == &target
        });
    assert!(matched);
    assert_eq!(
        invocations, 2,
        "a match inside a quantifier predicate must stop the whole traversal (consulted c, f)"
    );
}

#[test]
fn answering_allocates_nothing() {
    let expr = composite();
    let deep_target = cell("f");
    let absent_target = cell("never_used");

    let before = allocations_on_this_thread();
    let reaches_deep = expr.contains_value_ref_matching(|id| id == &deep_target);
    let reaches_absent = expr.contains_value_ref_matching(|id| id == &absent_target);
    let allocated = allocations_on_this_thread() - before;

    assert!(reaches_deep, "f is free under the nested quantifier");
    assert!(!reaches_absent);
    assert_eq!(
        allocated, 0,
        "contains_value_ref_matching must not allocate"
    );

    let before = allocations_on_this_thread();
    let collected = expr.collect_value_refs();
    assert!(
        allocations_on_this_thread() > before,
        "the counting allocator must observe collect_value_refs' Vec ({} refs)",
        collected.len()
    );
}
