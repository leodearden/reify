//! Tests for connected-component decomposition of constraint problems.

use reify_constraints::decompose_into_components;
use reify_test_support::*;
use reify_types::{AutoParam, Type, ValueMap};

/// 3 constraints each referencing a unique auto param → 3 components.
#[test]
fn three_independent_constraints_three_components() {
    let a = vcid("Part", "a");
    let b = vcid("Part", "b");
    let c = vcid("Part", "c");

    let auto_params = vec![
        AutoParam { id: a.clone(), param_type: Type::length(), bounds: None },
        AutoParam { id: b.clone(), param_type: Type::length(), bounds: None },
        AutoParam { id: c.clone(), param_type: Type::length(), bounds: None },
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

    let components = decompose_into_components(&auto_params, &constraints);
    assert_eq!(components.len(), 3, "3 independent constraints should yield 3 components");

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

    let auto_params = vec![
        AutoParam { id: a.clone(), param_type: Type::length(), bounds: None },
    ];

    // Both constraints reference param 'a'
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    let c2 = lt(value_ref("Part", "a"), literal(mm(10.0)));

    let constraints = vec![
        (cnid("Part", 0), c1),
        (cnid("Part", 1), c2),
    ];

    let components = decompose_into_components(&auto_params, &constraints);
    assert_eq!(components.len(), 1, "2 constraints on same param should yield 1 component");
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
        AutoParam { id: a.clone(), param_type: Type::length(), bounds: None },
        AutoParam { id: b.clone(), param_type: Type::length(), bounds: None },
        AutoParam { id: c.clone(), param_type: Type::length(), bounds: None },
    ];

    // C1: a > b (refs {a, b})
    let c1 = gt(value_ref("Part", "a"), value_ref("Part", "b"));
    // C2: b > c (refs {b, c})
    let c2 = gt(value_ref("Part", "b"), value_ref("Part", "c"));

    let constraints = vec![
        (cnid("Part", 0), c1),
        (cnid("Part", 1), c2),
    ];

    let components = decompose_into_components(&auto_params, &constraints);
    assert_eq!(components.len(), 1, "chained constraints should merge into 1 component");
    assert_eq!(components[0].auto_params.len(), 3, "all 3 params should be in the component");
    assert_eq!(components[0].constraints.len(), 2);
}

/// Empty constraints → 0 components.
#[test]
fn empty_constraints_zero_components() {
    let auto_params = vec![
        AutoParam {
            id: vcid("Part", "a"),
            param_type: Type::length(),
            bounds: None,
        },
    ];

    let constraints: Vec<(_, _)> = vec![];
    let components = decompose_into_components(&auto_params, &constraints);
    assert_eq!(components.len(), 0, "no constraints should yield 0 components");
}

/// Constraint referencing no auto params → excluded from components.
#[test]
fn constraint_without_auto_params_excluded() {
    let a = vcid("Part", "a");

    let auto_params = vec![
        AutoParam { id: a.clone(), param_type: Type::length(), bounds: None },
    ];

    // C1 references 'a' (an auto param)
    let c1 = gt(value_ref("Part", "a"), literal(mm(1.0)));
    // C2 references 'x' (NOT an auto param — a regular param or unknown)
    let c2 = gt(value_ref("Part", "x"), literal(mm(5.0)));

    let constraints = vec![
        (cnid("Part", 0), c1),
        (cnid("Part", 1), c2),
    ];

    let components = decompose_into_components(&auto_params, &constraints);

    // C2 references no auto params, so it should be excluded
    // Only C1 should appear in a component
    assert_eq!(components.len(), 1, "only constraint with auto params should form a component");
    assert_eq!(components[0].constraints.len(), 1);
    assert_eq!(components[0].auto_params.len(), 1);
    assert!(components[0].auto_params.contains(&a));
}
