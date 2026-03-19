//! Tests for ConstraintClassifier — classifying constraint expressions
//! into their appropriate ConstraintDomain.

use reify_constraints::ConstraintClassifier;
use reify_test_support::*;
use reify_types::{
    CompiledExpr, CompiledExprKind, ConstraintDomain, ContentHash, ResolvedFunction, Type, Value,
};

// --- Helper: build a geometry function call expression ---
fn geometry_function_call(name: &str) -> CompiledExpr {
    let arg = literal(mm(10.0));
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::geo::{}", name),
            },
            args: vec![arg],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: ContentHash::of(b"geo_test"),
    }
}

/// Scalar comparisons (thickness > 2mm) should classify as Dimensional.
#[test]
fn classify_scalar_comparison_as_dimensional() {
    // thickness > 2mm
    let thickness = value_ref("Bracket", "thickness");
    let two_mm = literal(mm(2.0));
    let expr = gt(thickness, two_mm);

    let domain = ConstraintClassifier::classify(&expr);
    assert_eq!(domain, ConstraintDomain::Dimensional);
}

/// Bool literal equality (x == true) should classify as Logical.
#[test]
fn classify_bool_eq_as_logical() {
    let x = value_ref_typed("Part", "flag", Type::Bool);
    let t = literal(Value::Bool(true));
    let expr = eq(x, t);

    let domain = ConstraintClassifier::classify(&expr);
    assert_eq!(domain, ConstraintDomain::Logical);
}

/// Mixed expression (x > 2mm && y == true) should classify as CrossDomain.
#[test]
fn classify_mixed_as_cross_domain() {
    // x > 2mm (Dimensional)
    let x = value_ref("Part", "x");
    let dim_part = gt(x, literal(mm(2.0)));

    // y == true (Logical)
    let y = value_ref_typed("Part", "flag", Type::Bool);
    let logic_part = eq(y, literal(Value::Bool(true)));

    // Combined: dim AND logic → CrossDomain
    let expr = and(dim_part, logic_part);

    let domain = ConstraintClassifier::classify(&expr);
    assert_eq!(domain, ConstraintDomain::CrossDomain);
}

/// Pure arithmetic on dimensionless Reals should classify as Dimensional.
#[test]
fn classify_dimensionless_real_as_dimensional() {
    let x = value_ref_typed("Part", "ratio", Type::Real);
    let threshold = literal(Value::Real(0.5));
    let expr = gt(x, threshold);

    let domain = ConstraintClassifier::classify(&expr);
    assert_eq!(domain, ConstraintDomain::Dimensional);
}

/// Expression with geometry function call should classify as Geometric.
#[test]
fn classify_geometry_function_as_geometric() {
    let geo_call = geometry_function_call("distance");
    let threshold = literal(mm(5.0));
    let expr = gt(geo_call, threshold);

    let domain = ConstraintClassifier::classify(&expr);
    assert_eq!(domain, ConstraintDomain::Geometric);
}
