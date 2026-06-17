//! Dispatch tests for the std.fields α/δ field-op IR variants
//! (task 4219, PRD docs/prds/v0_6/std-fields-api.md §5.2 / §5.3;
//!  task 4222, PRD §5.3 / B5).
//!
//! These tests exercise:
//!
//! 1. **Composed list-form** (`FieldSourceKind::Composed` with
//!    `lambda = Value::List[f, g]`) — step-7 RED / step-8 GREEN (task α).
//!
//! 2. **Restricted scaffold** (`FieldSourceKind::Restricted` with
//!    `lambda = Value::List[inner, region]`) — step-9 RED / step-10 GREEN (task α).
//!    The α scaffold returns `Value::Undef`; task δ revises the assertion.
//!
//! 3. **restrict() constructor** (`restrict(field, region)` FunctionCall →
//!    `Value::Field { source: Restricted, lambda: List[field, region] }`) —
//!    step-1 RED / step-2 GREEN (task δ, 4222).
//!
//! 4. **ContainmentQuery mock resolver** — step-3 RED / step-4 GREEN (task δ).
//!    Tests the `sample(restricted, pt)` dispatch arm with a mock resolver
//!    for inside/outside/indeterminate/no-resolver cases.
//!
//! Model: `field_eval_tests.rs` — same direct-Value construction +
//! `eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()))` pattern.

use std::sync::Arc;

use reify_core::{ContentHash, Type, ValueCellId};
use reify_expr::{ContainmentQuery, EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap,
};

use reify_core::DimensionVector;

// ── shared helpers ──────────────────────────────────────────────────────────

/// Build a `sample(field, at)` `CompiledExpr` for testing.
fn make_sample(field: Value, field_type: Type, at: Value, at_type: Type) -> CompiledExpr {
    let hash = ContentHash::of(b"sample");
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "sample".to_string(),
                qualified_name: "std::sample".to_string(),
            },
            args: vec![
                CompiledExpr::literal(field, field_type),
                CompiledExpr::literal(at, at_type),
            ],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: hash,
    }
}

/// Build an Analytical `Value::Field` with a single Real→Real lambda body.
///
/// The `body_fn` receives the `ValueCellId` for the parameter and must
/// return the body `CompiledExpr`.
fn make_analytical_field<F>(param_name: &str, scope: &str, body_fn: F) -> Value
where
    F: FnOnce(ValueCellId) -> CompiledExpr,
{
    let p_id = ValueCellId::new(scope, param_name);
    let body = body_fn(p_id.clone());
    Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(Value::Lambda {
            params: vec![(param_name.to_string(), p_id)],
            body: Box::new(body),
            captures: ValueMap::new(),
        }),
    }
}

// ── step-7/8: Composed list-form applies f(g(p)) ───────────────────────────

/// Sample a Composed field whose lambda slot is `Value::List[f, g]` and verify
/// the result is `f(g(3.0)) = (3.0 * 2.0) + 1.0 = 7.0`.
///
/// **RED today**: the `sample` dispatch in `reify-expr/src/lib.rs` does not
/// have a `(Value::List(_), FieldSourceKind::Composed)` arm; the composed
/// field falls through to `_ => Value::Undef`.
///
/// **GREEN after step-8**: the `sample_field_at` helper is extracted and a
/// `(Value::List(items), FieldSourceKind::Composed) if items.len() == 2`
/// arm is added that computes `sample_field_at(f, sample_field_at(g, p, ctx))`.
#[test]
fn sample_composed_list_form_applies_f_of_g() {
    // g: |x| x * 2.0  →  g(3.0) = 6.0
    let g = make_analytical_field("x", "$lambda_g.S", |x_id| {
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id, Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        )
    });

    // f: |y| y + 1.0  →  f(6.0) = 7.0
    let f = make_analytical_field("y", "$lambda_f.S", |y_id| {
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(y_id, Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        )
    });

    // composed = Field{domain:Real, codomain:Real, source:Composed, lambda:List[f, g]}
    // Convention (PRD §5.2, task 4219): items[0] = f (outer), items[1] = g (inner)
    // sample(composed, p) == f(g(p))
    let composed = Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Composed,
        lambda: Arc::new(Value::List(vec![f, g])),
    };

    let field_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let sample_expr = make_sample(composed, field_type, Value::Real(3.0), Type::dimensionless_scalar());
    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    assert_eq!(
        result,
        Value::Real(7.0),
        "sample(compose(f, g), 3.0) should be f(g(3.0)) = 7.0, got {:?}",
        result
    );
}

// ── step-1 RED (task δ 4222): restrict() constructor builds Restricted field ─

/// Build a `restrict(inner_field, region)` FunctionCall `CompiledExpr`.
///
/// `result_type` is the full `Type::Field { domain, codomain }` that
/// `field_op_result_type("restrict", ...)` would stamp; the constructor
/// arm reads `domain_type` / `codomain_type` from it.
fn make_restrict_call(
    inner_field: Value,
    inner_field_type: Type,
    region: Value,
    region_type: Type,
    result_type: Type,
) -> CompiledExpr {
    let hash = ContentHash::of(b"restrict_dispatch_test");
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "restrict".to_string(),
                qualified_name: "std::restrict".to_string(),
            },
            args: vec![
                CompiledExpr::literal(inner_field, inner_field_type),
                CompiledExpr::literal(region, region_type),
            ],
        },
        result_type,
        content_hash: hash,
    }
}

/// `restrict(field, region)` must evaluate to a `Value::Field` with
/// `source = FieldSourceKind::Restricted` and
/// `lambda = Arc(Value::List[inner_field, region])`.
/// Domain / codomain are extracted from the FunctionCall's `result_type`.
///
/// **RED before step-2 (task δ 4222)**: no "restrict" arm in eval_expr →
/// falls through to `reify_stdlib::eval_builtin` (no binding) → `Value::Undef`.
///
/// **GREEN after step-2**: `eval_restrict` is added and this test passes.
#[test]
fn restrict_constructor_builds_restricted_field() {
    // inner_field: analytical Real→Real `|x| 42.0` (constant, body ignores x).
    // The Value::Field itself carries domain/codomain = Real/Real; the constructor
    // reads its OWN domain/codomain from the FunctionCall result_type, not from
    // the arg Field's types.
    let inner_field = make_analytical_field("x", "$lambda_inner_restrict.S", |_x_id| {
        CompiledExpr::literal(Value::Real(42.0), Type::dimensionless_scalar())
    });

    // region: a placeholder `Value::Bool(false)` — must NOT be Undef, because the
    // strict-Undef short-circuit fires on any Undef arg before the restrict arm.
    // In practice the region is a Value::GeometryHandle; here any non-Undef value
    // satisfies the gate (`args[1]` is not type-checked at construction time).
    let region = Value::Bool(false);

    // result_type = Field<Point3<Length>, Real> — mirrors what field_op_result_type
    // stamps for `restrict(Field<Point3<Length>,Real>, Geometry)`.
    let point3_length = Type::Point {
        n: 3,
        quantity: Box::new(Type::Scalar { dimension: DimensionVector::LENGTH }),
    };
    let real_type = Type::dimensionless_scalar();
    let result_type = Type::Field {
        domain: Box::new(point3_length.clone()),
        codomain: Box::new(real_type.clone()),
    };

    // inner_field_type: any Field type (the gate checks args[0] is Value::Field).
    let inner_field_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };

    let restrict_expr = make_restrict_call(
        inner_field.clone(),
        inner_field_type,
        region.clone(),
        Type::Geometry,
        result_type,
    );

    let result = eval_expr(&restrict_expr, &EvalContext::simple(&ValueMap::new()));

    // Expected: Value::Field { source: Restricted, lambda: List[inner_field, region],
    //           domain: Point3<Length>, codomain: Real }
    let expected = Value::Field {
        domain_type: point3_length,
        codomain_type: real_type,
        source: FieldSourceKind::Restricted,
        lambda: Arc::new(Value::List(vec![inner_field.clone(), region.clone()])),
    };

    assert_eq!(
        result, expected,
        "restrict(field, region) should build a Restricted field, got {:?}",
        result
    );
}

// ── step-9 RED: Restricted scaffold returns Undef ──────────────────────────

/// Sample a Restricted field with **no resolver attached** and assert `Value::Undef`.
///
/// Construct a `FieldSourceKind::Restricted` field whose `lambda` slot is
/// `Value::List[inner_field, region]`.
///
/// After task δ implements the `ContainmentQuery` seam, the no-resolver case
/// (`EvalContext::simple` — no `with_containment` attached) still returns
/// `Value::Undef` unconditionally: without a live resolver, the dispatch arm
/// cannot determine containment and falls back to strict-Undef.  This test
/// pins that invariant in perpetuity.
///
/// **GREEN** (task δ step-4 and later): the `ContainmentQuery` hook exists;
/// the no-resolver arm maps `None` → `Value::Undef`.
#[test]
fn sample_restricted_scaffold_returns_undef() {
    // inner: |x| x * 3.0  (any analytical field)
    let inner = make_analytical_field("x", "$lambda_inner.S", |x_id| {
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id, Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        )
    });

    // region: placeholder — no resolver is attached so the actual value is irrelevant.
    let region = Value::Undef;

    // restricted = Field{source: Restricted, lambda: List[inner, region]}
    let restricted = Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Restricted,
        lambda: Arc::new(Value::List(vec![inner, region])),
    };

    let field_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let sample_expr = make_sample(restricted, field_type, Value::Real(1.0), Type::dimensionless_scalar());
    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    // no-resolver → Undef (containment unknowable without a resolver).
    assert_eq!(
        result,
        Value::Undef,
        "sample(restricted, 1.0) with no resolver should be Undef, got {:?}",
        result
    );
}

// ── step-3 RED (task δ 4222): ContainmentQuery mock resolver ───────────────
//
// Test the four dispatch cases for `sample(restricted, pt)`:
//   (a) resolver → Some(true)  (inside)       → inner field value
//   (b) resolver → Some(false) (outside)      → Value::Undef
//   (c) resolver → None        (indeterminate) → Value::Undef
//   (d) no resolver attached   (EvalContext::simple) → Value::Undef (already
//       covered by `sample_restricted_scaffold_returns_undef` above)
//
// RED today: `ContainmentQuery` trait and `EvalContext::with_containment` do
// not exist in reify-expr → compile-fail.
// GREEN after step-4: the trait and builder are added.

/// A minimal test double for `ContainmentQuery` — returns a pre-programmed
/// `Option<bool>` regardless of the region/point values passed.
struct MockContainmentQuery {
    result: Option<bool>,
}

impl ContainmentQuery for MockContainmentQuery {
    fn contains(&self, _region: &Value, _point: &Value) -> Option<bool> {
        self.result
    }
}

/// Build a `Value::Field { source: Restricted, lambda: List[inner, region] }`
/// suitable for mock-resolver tests.
///
/// inner: analytical `|x| 42.0` (constant).
/// region: `Value::Bool(false)` sentinel — NOT Undef (strict-Undef short-circuit
/// would fire before the restrict arm runs if args[1] were Undef).
/// The MockContainmentQuery ignores the actual region/point values.
fn make_restricted_constant_field() -> (Value, Value, Type) {
    let inner = make_analytical_field("x", "$lambda_inner_mock.S", |_x_id| {
        CompiledExpr::literal(Value::Real(42.0), Type::dimensionless_scalar())
    });
    let region = Value::Bool(false); // sentinel — MockContainmentQuery ignores it
    let field_type = Type::Field {
        domain: Box::new(Type::dimensionless_scalar()),
        codomain: Box::new(Type::dimensionless_scalar()),
    };
    let restricted = Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Restricted,
        lambda: Arc::new(Value::List(vec![inner, region])),
    };
    (restricted, Value::Real(0.0), field_type)
}

/// resolver → `Some(true)` (inside): `sample` returns the inner field value (42.0).
///
/// **RED today**: `ContainmentQuery`/`with_containment` absent → compile-fail.
/// **GREEN after step-4**: arm dispatches to `sample_field_at(inner, at, ctx)`.
#[test]
fn mock_resolver_some_true_returns_inner_value() {
    let (restricted, at, field_type) = make_restricted_constant_field();
    let sample_expr = make_sample(restricted, field_type, at, Type::dimensionless_scalar());

    let resolver = MockContainmentQuery { result: Some(true) };
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_containment(&resolver);

    let result = eval_expr(&sample_expr, &ctx);
    assert_eq!(
        result,
        Value::Real(42.0),
        "resolver→Some(true) should return inner field value 42.0, got {:?}",
        result
    );
}

/// resolver → `Some(false)` (outside): `sample` returns `Value::Undef`.
///
/// **RED today**: `ContainmentQuery`/`with_containment` absent → compile-fail.
/// **GREEN after step-4**: arm returns `Value::Undef`.
#[test]
fn mock_resolver_some_false_returns_undef() {
    let (restricted, at, field_type) = make_restricted_constant_field();
    let sample_expr = make_sample(restricted, field_type, at, Type::dimensionless_scalar());

    let resolver = MockContainmentQuery { result: Some(false) };
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_containment(&resolver);

    let result = eval_expr(&sample_expr, &ctx);
    assert_eq!(
        result,
        Value::Undef,
        "resolver→Some(false) should return Undef, got {:?}",
        result
    );
}

/// resolver → `None` (indeterminate): `sample` returns `Value::Undef`.
///
/// **RED today**: `ContainmentQuery`/`with_containment` absent → compile-fail.
/// **GREEN after step-4**: `None` arm returns `Value::Undef`.
#[test]
fn mock_resolver_none_returns_undef() {
    let (restricted, at, field_type) = make_restricted_constant_field();
    let sample_expr = make_sample(restricted, field_type, at, Type::dimensionless_scalar());

    let resolver = MockContainmentQuery { result: None };
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values).with_containment(&resolver);

    let result = eval_expr(&sample_expr, &ctx);
    assert_eq!(
        result,
        Value::Undef,
        "resolver→None should return Undef, got {:?}",
        result
    );
}
