//! Dispatch tests for the std.fields α field-op IR variants
//! (task 4219, PRD docs/prds/v0_6/std-fields-api.md §5.2 / §5.3).
//!
//! These tests exercise the `sample` dispatch path for:
//!
//! 1. **Composed list-form** (`FieldSourceKind::Composed` with
//!    `lambda = Value::List[f, g]`) — step-7 RED / step-8 GREEN.
//!
//! 2. **Restricted scaffold** (`FieldSourceKind::Restricted` with
//!    `lambda = Value::List[inner, region]`) — step-9 RED / step-10 GREEN.
//!    The α scaffold returns `Value::Undef`; task δ will implement OCCT
//!    point-in-region containment and revise the assertion.
//!
//! Model: `field_eval_tests.rs` — same direct-Value construction +
//! `eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()))` pattern.

use std::sync::Arc;

use reify_core::{ContentHash, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap,
};

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
        result_type: Type::Real,
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
        domain_type: Type::Real,
        codomain_type: Type::Real,
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
            CompiledExpr::value_ref(x_id, Type::Real),
            CompiledExpr::literal(Value::Real(2.0), Type::Real),
            Type::Real,
        )
    });

    // f: |y| y + 1.0  →  f(6.0) = 7.0
    let f = make_analytical_field("y", "$lambda_f.S", |y_id| {
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(y_id, Type::Real),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            Type::Real,
        )
    });

    // composed = Field{domain:Real, codomain:Real, source:Composed, lambda:List[f, g]}
    // Convention (PRD §5.2, task 4219): items[0] = f (outer), items[1] = g (inner)
    // sample(composed, p) == f(g(p))
    let composed = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Composed,
        lambda: Arc::new(Value::List(vec![f, g])),
    };

    let field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let sample_expr = make_sample(composed, field_type, Value::Real(3.0), Type::Real);
    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    assert_eq!(
        result,
        Value::Real(7.0),
        "sample(compose(f, g), 3.0) should be f(g(3.0)) = 7.0, got {:?}",
        result
    );
}

// ── step-9 RED: Restricted scaffold returns Undef ──────────────────────────

/// Sample a Restricted field and assert the α-scaffold returns `Value::Undef`.
///
/// Construct a `FieldSourceKind::Restricted` field whose `lambda` slot is
/// `Value::List[inner_field, region]`.  The α scaffold returns `Value::Undef`
/// for all points; task δ will implement OCCT point-in-region containment and
/// revise this assertion to:
///   - inside  → `sample_field_at(inner_field, at)` (the inner field value)
///   - outside → `Value::Undef`
///
/// **RED today**: `FieldSourceKind::Restricted` does not exist yet (compile-fail).
///
/// **GREEN after step-10**: the variant is added to `value.rs` and the
/// `(Value::List, Restricted)` arm in `sample_field_at` returns `Value::Undef`.
#[test]
fn sample_restricted_scaffold_returns_undef() {
    // inner: |x| x * 3.0  (any analytical field)
    let inner = make_analytical_field("x", "$lambda_inner.S", |x_id| {
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id, Type::Real),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
            Type::Real,
        )
    });

    // region: placeholder — task δ will use a real geometric region; here we
    // use Value::Undef as a sentinel (any value is accepted by the α scaffold).
    let region = Value::Undef;

    // restricted = Field{source: Restricted, lambda: List[inner, region]}
    let restricted = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Restricted,
        lambda: Arc::new(Value::List(vec![inner, region])),
    };

    let field_type = Type::Field {
        domain: Box::new(Type::Real),
        codomain: Box::new(Type::Real),
    };
    let sample_expr = make_sample(restricted, field_type, Value::Real(1.0), Type::Real);
    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    // α scaffold: always Undef (task δ revises to inside→inner-value / outside→Undef)
    assert_eq!(
        result,
        Value::Undef,
        "sample(restricted, 1.0) should be Undef in α scaffold, got {:?}",
        result
    );
}
