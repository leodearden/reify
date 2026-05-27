//! Tests for ConstraintClassifier — classifying constraint expressions
//! into their appropriate ConstraintDomain.

use reify_constraints::ConstraintClassifier;
use reify_test_support::*;
use reify_core::{ContentHash, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, ConstraintDomain, ResolvedFunction, Value};

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

/// Conditional with mixed branches exercises all arms of collect_flags.
///
/// condition: ratio > 0.5 (numeric/Dimensional)
/// then_branch: std::geo::distance(...) (Geometric)
/// else_branch: flag == true (Logical)
///
/// Expected: CrossDomain (has all three domain flags).
/// This locks in behavior before refactoring collect_flags to use walk().
#[test]
fn classify_conditional_with_mixed_branches() {
    // Condition: ratio > 0.5 (numeric → Dimensional)
    let ratio = value_ref_typed("Part", "ratio", Type::Real);
    let half = literal(Value::Real(0.5));
    let condition = gt(ratio, half);

    // Then branch: geometry function call → Geometric
    let then_branch = geometry_function_call("distance");

    // Else branch: flag == true → Logical
    let flag = value_ref_typed("Part", "flag", Type::Bool);
    let t = literal(Value::Bool(true));
    let else_branch = eq(flag, t);

    let expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type: Type::Bool,
        content_hash: ContentHash::of(b"test_cond"),
    };

    let domain = ConstraintClassifier::classify(&expr);
    // Has numeric (from ratio > 0.5), geometric (from geo::distance), and logical (from flag == true)
    // → CrossDomain
    assert_eq!(
        domain,
        ConstraintDomain::CrossDomain,
        "Conditional with numeric condition, geometric then, and logical else should be CrossDomain"
    );
}

// --- Regression tests for type_system_inconsistency (String misclassification) ---

/// String-typed ValueRefs must NOT set the numeric domain flag.
///
/// Type::is_numeric() explicitly excludes String (only Int|Real|Scalar are numeric),
/// but the classifier's ValueRef arm incorrectly includes Type::String in the numeric
/// match arm. A constraint with only String-typed ValueRefs should NOT set has_numeric.
///
/// With no domain flags set, into_domain() returns Dimensional by default, so a
/// String-only expression still returns Dimensional. The observable bug manifests when
/// String is mixed with Bool (see classify_string_does_not_contribute_numeric_flag).
/// This test verifies String-only expressions produce Dimensional (the no-flags default)
/// rather than CrossDomain or any other domain.
#[test]
fn classify_string_valueref_is_not_numeric() {
    // Two String-typed ValueRefs compared via eq — no numeric or logical content.
    let name_a = value_ref_typed("Part", "name", Type::String);
    let name_b = value_ref_typed("Part", "label", Type::String);
    let expr = eq(name_a, name_b);

    let domain = ConstraintClassifier::classify(&expr);
    // Both before and after the fix, String-only returns Dimensional (the default).
    // The bug is that String incorrectly sets has_numeric; after fix, no flags are set
    // but both paths yield Dimensional. The observable difference is tested below.
    assert_eq!(domain, ConstraintDomain::Dimensional);
}

/// String-typed ValueRefs must NOT contribute the numeric flag.
///
/// When a String-typed ValueRef is combined with a Bool-typed ValueRef, the result
/// should be Logical (only Bool contributes a domain flag). The current buggy code
/// sets has_numeric=true for String, producing CrossDomain instead.
#[test]
fn classify_string_does_not_contribute_numeric_flag() {
    // String-typed ValueRef — should NOT set has_numeric
    let name = value_ref_typed("Part", "name", Type::String);
    // Bool-typed ValueRef — sets has_logical
    let flag = value_ref_typed("Part", "active", Type::Bool);
    // Combine via and() — only Bool should contribute a domain flag
    let expr = and(name, flag);

    let domain = ConstraintClassifier::classify(&expr);
    // Expected: Logical (only has_logical set)
    // Buggy:   CrossDomain (has_numeric from String + has_logical from Bool)
    assert_eq!(
        domain,
        ConstraintDomain::Logical,
        "String ValueRef should not contribute numeric flag; only Bool's logical flag should be set"
    );
}
