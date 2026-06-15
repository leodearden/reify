//! FD-primitive↔reduction/sample indistinguishability boundary contract (PRD §5).
//!
//! Task θ step-4: verifies that an eager-lowered source:Sampled field produced by
//! `sampled_differential` (via gradient/divergence FunctionCall eval) is
//! indistinguishable to BOTH `sample_at_point` AND `compute_extremum` (max/argmax),
//! agreeing with the analytic derivative on polynomial fixtures.
//!
//! Characterizes the δ/ε/ζ seam; production landed in deps, so no impl step
//! follows.  The test is the deliverable.
//!
//! # Fixtures
//!
//! (A) SCALAR/ε path — linear scalar field g(x) = 3x+2 on Regular1D (5 nodes,
//!     spacing 1.0).  gradient(g) is exactly 3.0 at every node (algebraically
//!     exact on affine functions; PROVEN by δ's gradient_1d_affine_exact).
//!     Asserts via three paths:
//!       • DIRECT: `sample_at_point` on the inner SampledField at an interior
//!         grid node → 3.0 (≤ 1e-12).
//!       • REDUCTION: `max(gradient(g))` via eval_expr → 3.0 (≤ 1e-12).
//!       • REDUCTION: `argmax(gradient(g))` via eval_expr → valid in-bounds
//!         coordinate (not Value::Undef), sampled back → 3.0 (≤ 1e-12).
//!
//! (B) VECTOR/ζ path — affine 2D vector field F(x,y) = [2x, 3y] on Regular2D
//!     (4×3 nodes, spacing 1.0).  divergence(F) = 2+3 = 5.0 everywhere
//!     (algebraically exact for affine; PROVEN by δ).
//!     Asserts via two paths:
//!       • DIRECT: `sample_at_point` on the inner scalar SampledField at an
//!         interior grid node → 5.0 (≤ 1e-12).
//!       • REDUCTION: `max(divergence(F))` via eval_expr → 5.0 (≤ 1e-12).
//!
//! G6 numeric-premise discipline: all fixtures are exact-representable
//! polynomials; no tuned tolerances.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_core::{ContentHash, Type};
use reify_expr::{EvalContext, eval_expr, sampled as sampled_mod};
use reify_ir::{
    CompiledExpr, CompiledExprKind, FieldSourceKind, InterpolationKind, ResolvedFunction,
    SampledField, SampledGridKind, Value, ValueMap,
};

// ── Helpers (mirroring field_calculus_tests.rs) ──────────────────────────────

/// Build a FunctionCall CompiledExpr for a stdlib function.
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    let hash = ContentHash::of(name.as_bytes());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: hash,
    }
}

/// Build a `Value::Field` / `Type::Field` pair with an explicit `FieldSourceKind`.
fn make_field_with_source(
    domain: Type,
    codomain: Type,
    source: FieldSourceKind,
    lambda: Value,
) -> (Value, Type) {
    let field = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source,
        lambda: Arc::new(lambda),
    };
    let field_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    (field, field_type)
}

/// Build a uniform Regular1D scalar SampledField: `n` nodes with spacing `h`,
/// `data[i] = f(i * h)`.  Mirrors δ's `make_1d_scalar`.
fn make_1d_scalar(n: usize, h: f64, f: impl Fn(f64) -> f64) -> SampledField {
    let axis: Vec<f64> = (0..n).map(|i| i as f64 * h).collect();
    let data: Vec<f64> = axis.iter().map(|&x| f(x)).collect();
    SampledField {
        name: "test-1d-scalar".to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![(n - 1) as f64 * h],
        spacing: vec![h],
        axis_grids: vec![axis],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Build a uniform Regular2D stride-2 vector SampledField: `nx × ny` nodes with
/// spacing `hx`/`hy`, `data[i*ny*2 + j*2 + c] = f(x_i, y_j)[c]` (x-major).
/// Mirrors δ's `make_2d_vector`.
fn make_2d_vector(
    nx: usize,
    ny: usize,
    hx: f64,
    hy: f64,
    f: impl Fn(f64, f64) -> [f64; 2],
) -> SampledField {
    let xs: Vec<f64> = (0..nx).map(|i| i as f64 * hx).collect();
    let ys: Vec<f64> = (0..ny).map(|j| j as f64 * hy).collect();
    let mut data = Vec::with_capacity(nx * ny * 2);
    for &x in &xs {
        for &y in &ys {
            let v = f(x, y);
            data.push(v[0]);
            data.push(v[1]);
        }
    }
    SampledField {
        name: "test-2d-vec".to_string(),
        kind: SampledGridKind::Regular2D,
        bounds_min: vec![0.0, 0.0],
        bounds_max: vec![(nx - 1) as f64 * hx, (ny - 1) as f64 * hy],
        spacing: vec![hx, hy],
        axis_grids: vec![xs, ys],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    }
}

/// Extract the inner `SampledField` from an eager-lowered
/// `Value::Field { source: Sampled, lambda: Value::SampledField(...) }`.
///
/// Panics on any other shape.
fn inner_sampled_field(field: &Value) -> &SampledField {
    match field {
        Value::Field {
            source: FieldSourceKind::Sampled,
            lambda,
            ..
        } => match lambda.as_ref() {
            Value::SampledField(sf) => sf,
            other => panic!("inner_sampled_field: expected SampledField lambda, got {:?}", other),
        },
        other => panic!("inner_sampled_field: expected Sampled Value::Field, got {:?}", other),
    }
}

/// Extract the `(domain_type, codomain_type)` from a `Value::Field`.
///
/// Panics if not a `Value::Field`.
fn field_types(field: &Value) -> (&Type, &Type) {
    match field {
        Value::Field {
            domain_type,
            codomain_type,
            ..
        } => (domain_type, codomain_type),
        other => panic!("field_types: expected Value::Field, got {:?}", other),
    }
}

// ── (A) SCALAR/ε path ────────────────────────────────────────────────────────

/// FD-primitive↔reduction/sample indistinguishability: SCALAR path (PRD §5 B1).
///
/// g(x) = 3x+2 on Regular1D (5 nodes, h=1.0).  gradient(g) eager-lowers to a
/// Sampled scalar field with data ≡ 3.0 at every node (algebraically exact for
/// affine, PROVEN by δ's gradient_1d_affine_exact at 1e-12).
///
/// Asserts:
///   (result) gradient(g) produces source:Sampled (eager-lower confirmed).
///   (a) DIRECT — sample_at_point on the inner SampledField at x=2.0 → 3.0 (≤ 1e-12).
///   (b) REDUCTION — max(gradient(g)) → 3.0 (≤ 1e-12).
///   (b) REDUCTION — argmax(gradient(g)) → valid in-bounds coord (not Undef);
///       sample_at_point at the argmax coord → 3.0 (≤ 1e-12).
///
/// Together, all three paths agree with the analytic derivative at 1e-12 —
/// proving sample and reduction are indistinguishable on the eager-lowered field.
#[test]
fn fd_primitive_sample_reduction_indistinguishable_scalar_gradient() {
    // Build g(x) = 3x+2 as a Sampled scalar field.
    let sf = make_1d_scalar(5, 1.0, |x| 3.0 * x + 2.0);
    let domain_type = Type::dimensionless_scalar();
    let codomain_type = Type::dimensionless_scalar();
    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    let (field, _field_type) = make_field_with_source(
        domain_type.clone(),
        codomain_type.clone(),
        FieldSourceKind::Sampled,
        Value::SampledField(sf),
    );

    // gradient(g) → eager-lowered Sampled field with codomain = dimensionless_scalar.
    // The gradient result codomain for a 1D scalar field with n=1 axis is a scalar
    // (not a vector): differential_codomain(Gradient, scalar, scalar) → n=1 → scalar.
    let grad_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()), // 1D gradient = scalar
    };
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        grad_field_type.clone(),
    );
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    let grad_result = eval_expr(&grad_expr, &ctx);

    // Confirm eager-lower produced source:Sampled (not source:Gradient).
    assert!(
        matches!(
            &grad_result,
            Value::Field {
                source: FieldSourceKind::Sampled,
                ..
            }
        ),
        "gradient of 1D Sampled scalar field must eager-lower to source:Sampled, \
         got {:?}",
        grad_result
    );

    // ── (a) DIRECT: sample_at_point on the inner SampledField ────────────────
    let grad_sf = inner_sampled_field(&grad_result);
    let (_, grad_cod) = field_types(&grad_result);

    // Interior grid node x=2.0 (node index 2 on axis [0,1,2,3,4]).
    let point_1d = Value::Real(2.0);
    let direct_val =
        sampled_mod::sample_at_point(grad_sf, &point_1d, grad_cod, &EvalContext::simple(&values));
    let direct_f64 = direct_val.as_f64().unwrap_or_else(|| {
        panic!(
            "sample_at_point(gradient(g), 2.0) must be numeric, got {:?}",
            direct_val
        )
    });
    assert!(
        (direct_f64 - 3.0).abs() < 1e-12,
        "DIRECT sample_at_point(gradient(g), 2.0) must be exactly 3.0, \
         got {} (error {})",
        direct_f64,
        (direct_f64 - 3.0).abs()
    );

    // ── (b) REDUCTION: max(gradient(g)) ──────────────────────────────────────
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(grad_result.clone(), grad_field_type.clone())],
        codomain_type.clone(),
    );
    let max_result = eval_expr(&max_expr, &EvalContext::simple(&values));
    let max_f64 = max_result.as_f64().unwrap_or_else(|| {
        panic!(
            "max(gradient(g)) must be numeric, got {:?}",
            max_result
        )
    });
    assert!(
        (max_f64 - 3.0).abs() < 1e-12,
        "REDUCTION max(gradient(3x+2)) must be exactly 3.0, \
         got {} (error {})",
        max_f64,
        (max_f64 - 3.0).abs()
    );

    // ── (b) REDUCTION: argmax(gradient(g)) → valid in-bounds coord ───────────
    let argmax_expr = make_function_call(
        "argmax",
        vec![CompiledExpr::literal(grad_result.clone(), grad_field_type.clone())],
        domain_type.clone(), // argmax returns a domain coord
    );
    let argmax_result = eval_expr(&argmax_expr, &EvalContext::simple(&values));
    assert_ne!(
        argmax_result,
        Value::Undef,
        "argmax(gradient(3x+2)) must return a valid in-bounds coord, got Undef"
    );

    // Sample the gradient field at the argmax coord — must agree with max to 1e-12.
    let grad_sf_again = inner_sampled_field(&grad_result);
    let argmax_sample =
        sampled_mod::sample_at_point(grad_sf_again, &argmax_result, grad_cod, &ctx);
    let argmax_sample_f64 = argmax_sample.as_f64().unwrap_or_else(|| {
        panic!(
            "sample_at_point(gradient(g), argmax_coord) must be numeric, got {:?}",
            argmax_sample
        )
    });
    assert!(
        (argmax_sample_f64 - 3.0).abs() < 1e-12,
        "sample at argmax coord must equal max (3.0), \
         got {} (error {})",
        argmax_sample_f64,
        (argmax_sample_f64 - 3.0).abs()
    );
}

// ── (B) VECTOR/ζ path ────────────────────────────────────────────────────────

/// FD-primitive↔reduction/sample indistinguishability: VECTOR path (PRD §5 B2).
///
/// F(x,y) = [2x, 3y] on Regular2D (4×3 nodes, spacing 1.0).  divergence(F) eager-
/// lowers to a Sampled scalar field with data ≡ 5.0 at every node (algebraically
/// exact for affine, PROVEN by δ's divergence 2D tests at 1e-12).
///
/// Asserts:
///   (result) divergence(F) produces source:Sampled (eager-lower confirmed).
///   (a) DIRECT — sample_at_point on the inner scalar SampledField at (1.0,1.0)
///       → 5.0 (≤ 1e-12).
///   (b) REDUCTION — max(divergence(F)) → 5.0 (≤ 1e-12).
///
/// Both paths agree with the analytic divergence (∂2x/∂x + ∂3y/∂y = 5) at 1e-12.
#[test]
fn fd_primitive_sample_reduction_indistinguishable_vector_divergence() {
    // Build F(x,y) = [2x, 3y] as a stride-2 Sampled 2D vector field.
    let sf = make_2d_vector(4, 3, 1.0, 1.0, |x, y| [2.0 * x, 3.0 * y]);
    let domain_type = Type::point2(Type::dimensionless_scalar());
    let codomain_type = Type::vec2(Type::dimensionless_scalar());
    let div_codomain = Type::dimensionless_scalar(); // divergence is scalar

    let (field, field_type) = make_field_with_source(
        domain_type.clone(),
        codomain_type,
        FieldSourceKind::Sampled,
        Value::SampledField(sf),
    );

    // divergence(F) → eager-lowered Sampled scalar field.
    let div_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(div_codomain.clone()),
    };
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        div_field_type.clone(),
    );
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    let div_result = eval_expr(&div_expr, &ctx);

    // Confirm eager-lower produced source:Sampled (not source:Divergence).
    assert!(
        matches!(
            &div_result,
            Value::Field {
                source: FieldSourceKind::Sampled,
                ..
            }
        ),
        "divergence of 2D Sampled vector field must eager-lower to source:Sampled, \
         got {:?}",
        div_result
    );

    // ── (a) DIRECT: sample_at_point on the inner scalar SampledField ─────────
    let div_sf = inner_sampled_field(&div_result);
    let (_, div_cod) = field_types(&div_result);

    // Interior point (1.0, 1.0) is a grid node on the 4×3 grid (x∈[0,3], y∈[0,2]).
    let point_2d = Value::Point(vec![Value::Real(1.0), Value::Real(1.0)]);
    let direct_val =
        sampled_mod::sample_at_point(div_sf, &point_2d, div_cod, &EvalContext::simple(&values));
    let direct_f64 = direct_val.as_f64().unwrap_or_else(|| {
        panic!(
            "sample_at_point(divergence(F), (1,1)) must be numeric, got {:?}",
            direct_val
        )
    });
    assert!(
        (direct_f64 - 5.0).abs() < 1e-12,
        "DIRECT sample_at_point(divergence([2x,3y]), (1,1)) must be exactly 5.0 (div=2+3), \
         got {} (error {})",
        direct_f64,
        (direct_f64 - 5.0).abs()
    );

    // ── (b) REDUCTION: max(divergence(F)) ────────────────────────────────────
    let max_expr = make_function_call(
        "max",
        vec![CompiledExpr::literal(div_result, div_field_type)],
        div_codomain,
    );
    let max_result = eval_expr(&max_expr, &EvalContext::simple(&values));
    let max_f64 = max_result.as_f64().unwrap_or_else(|| {
        panic!(
            "max(divergence(F)) must be numeric, got {:?}",
            max_result
        )
    });
    assert!(
        (max_f64 - 5.0).abs() < 1e-12,
        "REDUCTION max(divergence([2x,3y])) must be exactly 5.0 (div=2+3), \
         got {} (error {})",
        max_f64,
        (max_f64 - 5.0).abs()
    );
}
