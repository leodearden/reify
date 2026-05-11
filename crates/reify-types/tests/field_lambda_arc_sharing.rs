//! Characterization tests for `Value::Field.lambda: Arc<Value>`.
//!
//! These tests fail to compile when `lambda` is `Box<Value>` (type mismatch:
//! expected `Box<Value>`, found `Arc<Value>`). After the field is changed to
//! `Arc<Value>`, they compile and pass, pinning the O(1)-clone sharing
//! invariant at the type-definition level.

use std::sync::Arc;

use reify_types::{CompiledExpr, FieldSourceKind, Type, Value, ValueCellId, ValueMap};

/// Build a minimal non-trivial `Value::Lambda` for use in Field construction.
fn make_lambda() -> Value {
    let param_id = ValueCellId::new("test_field_lambda", "x");
    let body = CompiledExpr::value_ref(param_id.clone(), Type::Real);
    Value::Lambda {
        params: vec![("x".to_string(), param_id)],
        body: Box::new(body),
        captures: ValueMap::new(),
    }
}

/// After the type change to `Arc<Value>`, cloning a `Value::Field` must NOT
/// allocate a new backing buffer for the lambda — both the original and the
/// clone must point to the same Arc allocation (O(1) refcount bump, not a
/// deep clone of the compiled expression tree).
#[test]
fn field_clone_shares_lambda_via_arc() {
    let lambda_val = make_lambda();
    let lambda_arc: Arc<Value> = Arc::new(lambda_val);

    let domain = Type::Point {
        n: 3,
        quantity: Box::new(Type::Real),
    };
    let codomain = Type::Real;

    let original = Value::Field {
        domain_type: domain,
        codomain_type: codomain,
        source: FieldSourceKind::Analytical,
        lambda: Arc::clone(&lambda_arc),
    };

    let cloned = original.clone();

    // Destructure both to get the lambda Arcs.
    let Value::Field {
        lambda: orig_lambda,
        ..
    } = &original
    else {
        panic!("original must be a Field");
    };
    let Value::Field {
        lambda: clone_lambda,
        ..
    } = &cloned
    else {
        panic!("cloned must be a Field");
    };

    // The key invariant: clone must share the same Arc allocation, not a new one.
    assert!(
        Arc::ptr_eq(orig_lambda, clone_lambda),
        "Field clone must share the lambda Arc — cloning should bump the refcount, \
         not allocate a new heap buffer for the compiled expression tree"
    );
}

/// Guard: a cloned Field must still compare equal to the original.
/// This ensures that switching from Box to Arc does not break PartialEq semantics.
#[test]
fn field_clone_preserves_equality() {
    let lambda_val = make_lambda();
    let lambda_arc: Arc<Value> = Arc::new(lambda_val);

    let domain = Type::Point {
        n: 3,
        quantity: Box::new(Type::Real),
    };
    let codomain = Type::Real;

    let original = Value::Field {
        domain_type: domain,
        codomain_type: codomain,
        source: FieldSourceKind::Analytical,
        lambda: lambda_arc,
    };

    let cloned = original.clone();
    assert_eq!(
        original, cloned,
        "Cloned Field must equal original — PartialEq must be preserved under Arc"
    );
}
