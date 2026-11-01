//! How `try_eval_topology_selector` reports a `flat_map` walk it declines.

use super::*;
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value, ValueMap};
use reify_test_support::mocks::MockGeometryKernel;

fn builtin_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: name.to_string(),
            },
            args,
        },
        result_type,
        content_hash: ContentHash::of_str(name),
    }
}

/// `flat_map(xs, |x| closest_point(x, b))` over `xs = [1]`. `closest_point`
/// rejects the non-point element with a Warning and returns `None`, so the walk
/// is not a topology walk. The cell then keeps its pure-eval value, and the
/// rejected attempt must leave no diagnostic behind.
#[test]
fn a_declined_flat_map_walk_leaves_no_diagnostics() {
    let xs = ValueCellId::new("S", "xs");
    let x = ValueCellId::new("S", "x");
    let mut values = ValueMap::new();
    values.insert(xs.clone(), Value::List(vec![Value::Int(1)]));
    let body = builtin_call(
        "closest_point",
        vec![
            CompiledExpr::value_ref(x.clone(), Type::Int),
            CompiledExpr::value_ref(ValueCellId::new("S", "b"), Type::Geometry),
        ],
        Type::Geometry,
    );
    let lambda = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x],
        body,
        Vec::new(),
        Type::Geometry,
    );
    let walk = builtin_call(
        "flat_map",
        vec![
            CompiledExpr::value_ref(xs, Type::List(Box::new(Type::Int))),
            lambda,
        ],
        Type::List(Box::new(Type::Geometry)),
    );
    let mut diagnostics = Vec::new();

    let result = try_eval_topology_selector(
        &walk,
        &HashMap::new(),
        &values,
        &mut MockGeometryKernel::new(),
        &mut diagnostics,
    );

    assert_eq!(result, None);
    assert!(
        diagnostics.is_empty(),
        "a declined flat_map walk must report nothing; diagnostics={diagnostics:#?}"
    );
}
