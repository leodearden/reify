//! Tests for connected-component decomposition of constraint problems.

use reify_constraints::decompose_into_components;
use reify_test_support::*;
use reify_core::Type;
use reify_ir::BinOp;

/// 3 constraints each referencing a unique auto param → 3 components.
#[test]
fn three_independent_constraints_three_components() {
    let a = vcid("Part", "a");
    let b = vcid("Part", "b");
    let c = vcid("Part", "c");

    let auto_params = vec![
        single_auto_param(a.clone()),
        single_auto_param(b.clone()),
        single_auto_param(c.clone()),
    ];

    // Each constraint references exactly one param
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    let c2 = gt(value_ref("Part", "b"), literal(mm(2.0)));
    let c3 = gt(value_ref("Part", "c"), literal(mm(3.0)));

    let constraints = vec![
        (cnid("Part", 0), c1),
        (cnid("Part", 1), c2),
        (cnid("Part", 2), c3),
    ];

    let components = decompose_into_components(&auto_params, &constraints, None);
    assert_eq!(
        components.len(),
        3,
        "3 independent constraints should yield 3 components"
    );

    // Each component should have exactly 1 auto param and 1 constraint
    for comp in &components {
        assert_eq!(comp.auto_params.len(), 1);
        assert_eq!(comp.constraints.len(), 1);
    }
}

/// 2 constraints sharing one auto param → 1 component.
#[test]
fn shared_param_merges_into_one_component() {
    let a = vcid("Part", "a");

    let auto_params = vec![single_auto_param(a.clone())];

    // Both constraints reference param 'a'
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    let c2 = lt(value_ref("Part", "a"), literal(mm(10.0)));

    let constraints = vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)];

    let components = decompose_into_components(&auto_params, &constraints, None);
    assert_eq!(
        components.len(),
        1,
        "2 constraints on same param should yield 1 component"
    );
    assert_eq!(components[0].auto_params.len(), 1);
    assert_eq!(components[0].constraints.len(), 2);
}

/// Chain: C1 refs {a,b}, C2 refs {b,c} → 1 component with all 3 params.
#[test]
fn chain_constraints_single_component() {
    let a = vcid("Part", "a");
    let b = vcid("Part", "b");
    let c = vcid("Part", "c");

    let auto_params = vec![
        single_auto_param(a.clone()),
        single_auto_param(b.clone()),
        single_auto_param(c.clone()),
    ];

    // C1: a > b (refs {a, b})
    let c1 = gt(value_ref("Part", "a"), value_ref("Part", "b"));
    // C2: b > c (refs {b, c})
    let c2 = gt(value_ref("Part", "b"), value_ref("Part", "c"));

    let constraints = vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)];

    let components = decompose_into_components(&auto_params, &constraints, None);
    assert_eq!(
        components.len(),
        1,
        "chained constraints should merge into 1 component"
    );
    assert_eq!(
        components[0].auto_params.len(),
        3,
        "all 3 params should be in the component"
    );
    assert_eq!(components[0].constraints.len(), 2);
}

/// Empty constraints → 0 components.
#[test]
fn empty_constraints_zero_components() {
    let auto_params = vec![single_auto_param(vcid("Part", "a"))];

    let constraints: Vec<(_, _)> = vec![];
    let components = decompose_into_components(&auto_params, &constraints, None);
    assert_eq!(
        components.len(),
        0,
        "no constraints should yield 0 components"
    );
}

/// Constraint referencing no auto params → excluded from components.
#[test]
fn constraint_without_auto_params_excluded() {
    let a = vcid("Part", "a");

    let auto_params = vec![single_auto_param(a.clone())];

    // C1 references 'a' (an auto param)
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    // C2 references 'x' (NOT an auto param — a regular param or unknown)
    let c2 = gt(value_ref("Part", "x"), literal(mm(5.0)));

    let constraints = vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)];

    let components = decompose_into_components(&auto_params, &constraints, None);

    // C2 references no auto params, so it should be excluded
    // Only C1 should appear in a component
    assert_eq!(
        components.len(),
        1,
        "only constraint with auto params should form a component"
    );
    assert_eq!(components[0].constraints.len(), 1);
    assert_eq!(components[0].auto_params.len(), 1);
    assert!(components[0].auto_params.contains(&a));
}

/// Nested Conditional expression: all ValueRefs across condition/then/else
/// branches are collected into one component because a single constraint
/// references all of them.
///
/// This locks in behavior before refactoring collect_value_refs to use walk().
#[test]
fn collect_value_refs_handles_nested_conditional() {
    let a = vcid("Part", "a");
    let b = vcid("Part", "b");
    let c = vcid("Part", "c");
    let d = vcid("Part", "d");

    let auto_params = vec![
        single_auto_param(a.clone()),
        single_auto_param(b.clone()),
        single_auto_param(c.clone()),
        single_auto_param(d.clone()),
    ];

    // Build a Conditional expression:
    //   if (a > b) then (c > 1mm) else (d > 2mm)
    // This references {a, b, c, d} across all branches.
    let condition = gt(value_ref("Part", "a"), value_ref("Part", "b"));
    let then_branch = gt(value_ref("Part", "c"), literal(mm(1.0)));
    let else_branch = gt(value_ref("Part", "d"), literal(mm(2.0)));

    let conditional = reify_ir::CompiledExpr {
        kind: reify_ir::CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type: Type::Bool,
        content_hash: reify_core::ContentHash::of(b"test_cond"),
    };

    // Single constraint using the conditional expression
    let constraints = vec![(cnid("Part", 0), conditional)];

    let components = decompose_into_components(&auto_params, &constraints, None);

    // All 4 params referenced by one constraint → 1 component with all 4 params
    assert_eq!(
        components.len(),
        1,
        "single constraint should yield 1 component"
    );
    assert_eq!(
        components[0].auto_params.len(),
        4,
        "all 4 params should be in the component"
    );
    assert!(components[0].auto_params.contains(&a));
    assert!(components[0].auto_params.contains(&b));
    assert!(components[0].auto_params.contains(&c));
    assert!(components[0].auto_params.contains(&d));
}

/// Objective expression merges independent params into one component.
///
/// When the objective references params from multiple independent components,
/// those components must be merged via the union-find. This test calls
/// `decompose_into_components` with the new third argument (objective expression).
///
#[test]
fn objective_merges_independent_params_into_one_component() {
    let a = vcid("Part", "a");
    let b = vcid("Part", "b");

    let auto_params = vec![single_auto_param(a.clone()), single_auto_param(b.clone())];

    // Independent constraints (different params)
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    let c2 = gt(value_ref("Part", "b"), literal(mm(1.0)));

    let constraints = vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)];

    // Objective: a + b (references both params)
    let objective_expr = binop(BinOp::Add, value_ref("Part", "a"), value_ref("Part", "b"));

    // decompose_into_components with objective should merge both into 1 component
    let components = decompose_into_components(&auto_params, &constraints, Some(&objective_expr));
    assert_eq!(
        components.len(),
        1,
        "objective referencing both params should merge into 1 component"
    );
    assert_eq!(components[0].auto_params.len(), 2);
}
