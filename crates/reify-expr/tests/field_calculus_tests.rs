//! Field calculus tests.
//!
//! Accuracy and correctness tests for field differential operators
//! (gradient, divergence, curl, laplacian) using analytical fields
//! with known mathematical derivatives.
//!
//! Helpers are defined locally following the pattern in gradient_tests.rs
//! and field_eval_tests.rs.

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, UnOp, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a FunctionCall expression for stdlib functions.
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

/// Build a Value::Lambda with (name, id) param pairs.
fn make_value_lambda(
    params: Vec<(&str, ValueCellId)>,
    body: CompiledExpr,
    captures: ValueMap,
) -> Value {
    Value::Lambda {
        params: params
            .into_iter()
            .map(|(n, id)| (n.to_string(), id))
            .collect(),
        body: Box::new(body),
        captures,
    }
}

/// Build a `Value::Field` / `Type::Field` pair with an explicit source kind.
///
/// The general variant underlying [`make_analytical_field`]: accepts any
/// `FieldSourceKind` as an explicit parameter rather than hardcoding
/// `FieldSourceKind::Analytical`.
///
/// Parameter order `(domain, codomain, source, lambda)` preserves the existing
/// `(domain, codomain, …, lambda)` shape from `make_analytical_field` — a
/// reader translating between the two helpers need only insert one argument.
///
/// Call sites that need to retain `domain`/`codomain` after this call should
/// `.clone()` before passing, matching the existing pattern throughout this file.
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

/// Build an analytical `Value::Field` / `Type::Field` pair from typed components.
///
/// Convenience wrapper over [`make_field_with_source`] that fixes the source to
/// `FieldSourceKind::Analytical`.
///
/// Returns `(Value::Field { domain_type, codomain_type, source: Analytical, lambda },
///           Type::Field  { domain, codomain })`.
///
/// Call sites that need to retain `domain`/`codomain` after this call should
/// `.clone()` before passing, matching the existing pattern throughout this file.
fn make_analytical_field(domain: Type, codomain: Type, lambda: Value) -> (Value, Type) {
    make_field_with_source(domain, codomain, FieldSourceKind::Analytical, lambda)
}

/// Unit test for `make_field_with_source`: verifies that the returned
/// `Value::Field` and `Type::Field` carry the source kind, domain type,
/// codomain type, and lambda supplied by the caller.
///
/// The source kind is checked for multiple variants to confirm it round-trips
/// correctly rather than being silently overridden.
#[test]
fn make_field_with_source_builds_field_with_given_source() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    // Compile-time exhaustiveness guard: adding a new FieldSourceKind variant
    // will make this match non-exhaustive, forcing an update to the match arms
    // below — a visual reminder to also extend the iteration array above.
    fn _assert_all_source_kinds_covered(k: FieldSourceKind) {
        match k {
            FieldSourceKind::Analytical
            | FieldSourceKind::Sampled
            | FieldSourceKind::Composed
            | FieldSourceKind::Imported
            | FieldSourceKind::Gradient
            | FieldSourceKind::Divergence
            | FieldSourceKind::Curl
            | FieldSourceKind::Laplacian
            | FieldSourceKind::VonMises
            | FieldSourceKind::PrincipalStresses
            | FieldSourceKind::MaxShear
            | FieldSourceKind::SafetyFactor => {}
        }
    }

    for source_kind in [
        FieldSourceKind::Analytical,
        FieldSourceKind::Sampled,
        FieldSourceKind::Composed,
        FieldSourceKind::Imported,
        FieldSourceKind::Gradient,
        FieldSourceKind::Divergence,
        FieldSourceKind::Curl,
        FieldSourceKind::Laplacian,
        FieldSourceKind::VonMises,
        FieldSourceKind::PrincipalStresses,
        FieldSourceKind::MaxShear,
        FieldSourceKind::SafetyFactor,
    ] {
        let (field, field_type) =
            make_field_with_source(Type::Real, Type::Real, source_kind.clone(), lambda.clone());

        // Type::Field carries the supplied domain and codomain.
        assert_eq!(
            field_type,
            Type::Field {
                domain: Box::new(Type::Real),
                codomain: Box::new(Type::Real),
            }
        );

        // Destructure the Value::Field and assert each field.
        let Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda: boxed_lambda,
        } = field
        else {
            panic!("expected Value::Field");
        };

        assert_eq!(domain_type, Type::Real);
        assert_eq!(codomain_type, Type::Real);
        assert_eq!(source, source_kind);
        assert_eq!(*boxed_lambda, lambda);
    }
}

/// Result `Type::Field` for a `curl` operator: domain → Vec3(Real).
fn curl_result_type(domain: Type) -> Type {
    Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(Type::vec3(Type::Real)),
    }
}

/// Result `Type::Field` for operators producing a scalar field: `domain → Real`.
///
/// Used by divergence and laplacian.
fn scalar_field_result_type(domain: Type) -> Type {
    Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(Type::Real),
    }
}

/// Result `Type::Field` for a `gradient` operator: `domain → Vector_n(Real)`.
fn gradient_result_type(domain: Type, n: usize) -> Type {
    Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(Type::Vector {
            n,
            quantity: Box::new(Type::Real),
        }),
    }
}

/// Unit test for `gradient_result_type`: Vec_n(Real) result field, tested at n=2, n=3, and n=4.
#[test]
fn gradient_result_type_returns_field_vec_n_real() {
    // n=2: Vector2(Real) codomain
    let domain2 = Type::point2(Type::Real);
    let got2 = gradient_result_type(domain2.clone(), 2);
    let expected2 = Type::Field {
        domain: Box::new(domain2),
        codomain: Box::new(Type::vec2(Type::Real)),
    };
    assert_eq!(got2, expected2);

    // n=3: Vector3(Real) codomain
    let domain3 = Type::point3(Type::Real);
    let got3 = gradient_result_type(domain3.clone(), 3);
    let expected3 = Type::Field {
        domain: Box::new(domain3),
        codomain: Box::new(Type::vec3(Type::Real)),
    };
    assert_eq!(got3, expected3);

    // n=4: arbitrary n — guards the collapsed single-expression form
    let domain4 = Type::point3(Type::Real);
    let got4 = gradient_result_type(domain4.clone(), 4);
    let expected4 = Type::Field {
        domain: Box::new(domain4),
        codomain: Box::new(Type::Vector {
            n: 4,
            quantity: Box::new(Type::Real),
        }),
    };
    assert_eq!(got4, expected4);
}

/// Unit test for `scalar_field_result_type`: Real codomain result field.
#[test]
fn scalar_field_result_type_returns_field_real_codomain() {
    let domain = Type::point3(Type::Real);
    let got = scalar_field_result_type(domain.clone());
    let expected = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(Type::Real),
    };
    assert_eq!(got, expected);
}

/// Assert that a `Value::Vector` has components matching `expected` within `tol`.
fn assert_gradient_vector(result: &Value, expected: &[f64], tol: f64, label: &str) {
    match result {
        Value::Vector(components) => {
            assert_eq!(
                components.len(),
                expected.len(),
                "{label}: gradient vector has {} components, expected {}",
                components.len(),
                expected.len()
            );
            for (i, (comp, &exp)) in components.iter().zip(expected.iter()).enumerate() {
                let val = comp.as_f64().unwrap_or_else(|| {
                    panic!("{label}: component {i} should be numeric, got {:?}", comp)
                });
                assert!(
                    (val - exp).abs() < tol,
                    "{label}: component {i} = {val} differs from expected {exp} by {} (tolerance {tol})",
                    (val - exp).abs()
                );
            }
        }
        _ => panic!("{label}: expected Value::Vector, got {:?}", result),
    }
}

/// A typed sample point passed to field-sampling helpers.
///
/// Encodes both the value (as a fixed-size array of `f64`) and the static type,
/// eliminating the risk of `(Value, Type)` desynchronisation at call sites.
///
/// `into_value_and_type(self)` derives both `Value` and `Type` from the same
/// array, so callers cannot accidentally pass a mismatched type.
enum SamplePoint {
    /// A 3-component `Value::Point` with `Type::point3(Real)`.
    Point3([f64; 3]),
    /// A 3-component `Value::Vector` with `Type::vec3(Real)`.
    Vector3([f64; 3]),
    /// A 2-component `Value::Vector` with `Type::vec2(Real)`.
    Vector2([f64; 2]),
}

impl SamplePoint {
    /// Consume `self` and produce the corresponding `(Value, Type)` pair.
    fn into_value_and_type(self) -> (Value, Type) {
        match self {
            SamplePoint::Point3([a, b, c]) => (
                Value::Point(vec![Value::Real(a), Value::Real(b), Value::Real(c)]),
                Type::point3(Type::Real),
            ),
            SamplePoint::Vector3([a, b, c]) => (
                Value::Vector(vec![Value::Real(a), Value::Real(b), Value::Real(c)]),
                Type::vec3(Type::Real),
            ),
            SamplePoint::Vector2([a, b]) => (
                Value::Vector(vec![Value::Real(a), Value::Real(b)]),
                Type::vec2(Type::Real),
            ),
        }
    }
}

/// Build the identity vector field F(x,y,z)=[x,y,z], apply divergence, eval, and return
/// `(div_result, div_field_type)`.
///
/// The caller is responsible for sampling the returned field and asserting the result.
/// Shared by `run_divergence_identity_test` (happy-path) and
/// `divergence_two_element_vector_sample_point_returns_undef` (dimension-guard).
fn build_divergence_identity_field(label: &str) -> (Value, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(x, y, z)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), codomain_type, lambda);

    // divergence(field) → scalar field
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        scalar_field_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "{label}: divergence should return a Field, got {:?}",
        div_result
    );

    (div_result, scalar_field_result_type(domain_type))
}

/// Build the identity vector field F(x,y,z)=[x,y,z], compute its divergence,
/// sample at `sample_point`, and assert result ≈3.0.
///
/// Used by both `divergence_identity_vector_field` (Point sample) and
/// `divergence_accepts_vector_sample_point` (Vector sample).
fn run_divergence_identity_test(sample_point: SamplePoint, label: &str) {
    let (point, point_literal_type) = sample_point.into_value_and_type();
    let (div_result, div_field_type) = build_divergence_identity_field(label);

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "{label}: divergence sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        (val - 3.0).abs() < 1e-3,
        "{label}: expected ≈3.0, got {}",
        val
    );
}

/// Build the rotation field F(x,y,z)=[-y,x,0], apply curl, eval, and return
/// `(curl_result, curl_field_type)`.
///
/// The caller is responsible for sampling the returned field and asserting the result.
/// Shared by `run_curl_rotation_test` (happy-path) and
/// `curl_two_element_vector_sample_point_returns_undef` (dimension-guard).
fn build_curl_rotation_field(label: &str) -> (Value, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(-y, x, 0)
    let neg_y = CompiledExpr::unop(
        UnOp::Neg,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = make_function_call(
        "vec3",
        vec![
            neg_y,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::literal(Value::Real(0.0), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), codomain_type, lambda);

    // curl(field) → vector field
    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        curl_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&curl_result, Value::Field { .. }),
        "{label}: curl should return a Field, got {:?}",
        curl_result
    );

    (curl_result, curl_result_type(domain_type))
}

/// Build the rotation field F(x,y,z)=[-y,x,0], compute its curl, sample at
/// `sample_point`, and assert result ≈[0,0,2].
///
/// Used by both `curl_rotation_field` (Point sample) and
/// `curl_accepts_vector_sample_point` (Vector sample).
fn run_curl_rotation_test(sample_point: SamplePoint, label: &str) {
    let (point, point_literal_type) = sample_point.into_value_and_type();
    let (curl_result, curl_field_type) = build_curl_rotation_field(label);

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(&sample_result, &[0.0, 0.0, 2.0], 1e-3, label);
}

/// Build the quadratic scalar field f(x,y,z)=x²+y²+z², apply laplacian, eval, and return
/// `(lap_result, lap_field_type)`.
///
/// The caller is responsible for sampling the returned field and asserting the result.
/// Shared by `run_laplacian_quadratic_test` (happy-path) and
/// `laplacian_two_element_vector_sample_point_returns_undef` (dimension-guard).
fn build_laplacian_quadratic_field(label: &str) -> (Value, Type) {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Real;

    let (field, field_type) = make_analytical_field(domain_type.clone(), codomain_type, lambda);

    // laplacian(field) → scalar field
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        scalar_field_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "{label}: laplacian should return a Field, got {:?}",
        lap_result
    );

    (lap_result, scalar_field_result_type(domain_type))
}

/// Build the quadratic scalar field f(x,y,z)=x²+y²+z², compute its laplacian,
/// sample at `sample_point`, and assert result ≈6.0.
///
/// Used by both `laplacian_quadratic_accuracy` (Point sample) and
/// `laplacian_accepts_vector_sample_point` (Vector sample).
fn run_laplacian_quadratic_test(sample_point: SamplePoint, label: &str) {
    let (point, point_literal_type) = sample_point.into_value_and_type();
    let (lap_result, lap_field_type) = build_laplacian_quadratic_field(label);

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "{label}: laplacian sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        (val - 6.0).abs() < 1e-2,
        "{label}: expected ≈6.0, got {}",
        val
    );
}

/// Run a metadata-only dimensional-correctness test for a calculus operator.
///
/// Builds a dummy `Value::Field` with the given `domain_type`, `codomain_type`, and
/// `source`, creates a no-op lambda (body = `value_ref` to the first parameter),
/// applies the named operator via `make_function_call`, and asserts:
///   1. The result is `Value::Field { source: <expected from op>, .. }`.
///   2. The result's `codomain_type` equals `expected_codomain`.
///
/// The lambda body is never sampled because `compute_laplacian` / `compute_divergence`
/// derive the result codomain purely from type metadata.
///
/// `op` must be `"laplacian"` or `"divergence"`; other values cause a panic with `label`.
/// `domain_type` must have arity 1–3; `Type::Point { n, .. }` yields `n` params,
/// all other types yield 1 param (named `"x"`).
fn run_dim_metadata_test(
    op: &str,
    domain_type: Type,
    codomain_type: Type,
    source: FieldSourceKind,
    expected_codomain: Type,
    label: &str,
) {
    // Derive arity from domain type.
    let n: usize = match &domain_type {
        Type::Point { n, .. } => *n,
        _ => 1,
    };
    assert!(
        (1..=3).contains(&n),
        "{label}: arity {n} out of range 1..=3"
    );

    // Build n ValueCellIds from ["x", "y", "z"].
    let names = ["x", "y", "z"];
    let ids: Vec<ValueCellId> = names[..n]
        .iter()
        .map(|&name| ValueCellId::new("$lambda0.S", name))
        .collect();

    // Dummy body: value_ref to the first parameter (never evaluated).
    let body = CompiledExpr::value_ref(ids[0].clone(), Type::Real);

    // Build lambda.
    let params: Vec<(&str, ValueCellId)> = names[..n].iter().copied().zip(ids).collect();
    let lambda = make_value_lambda(params, body, ValueMap::new());

    // Build the (Value::Field, Type::Field) pair via the source-parameterised helper.
    let (field, field_type) =
        make_field_with_source(domain_type.clone(), codomain_type.clone(), source, lambda);

    // Build the function call.
    let expr = make_function_call(
        op,
        vec![CompiledExpr::literal(field, field_type)],
        expected_codomain.clone(),
    );

    // Evaluate with empty bindings.
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));

    // Derive expected source kind from op.
    let expected_source = match op {
        "laplacian" => FieldSourceKind::Laplacian,
        "divergence" => FieldSourceKind::Divergence,
        other => panic!("{label}: unknown op {other:?}"),
    };

    // Destructure via let-else (review S2).
    let Value::Field {
        codomain_type: actual_codomain,
        source: actual_source,
        ..
    } = &result
    else {
        panic!("{label}: {op} should return a Field, got {result:?}");
    };

    assert_eq!(
        *actual_source, expected_source,
        "{label}: expected source {:?}, got {:?}",
        expected_source, actual_source
    );
    assert_eq!(
        *actual_codomain, expected_codomain,
        "{label}: expected codomain {:?}, got {:?}",
        expected_codomain, actual_codomain
    );
}

/// Maximum number of dimensions supported by the hard-coded coordinate/name arrays
/// (`[1.0, 2.0, 3.0]` in `make_sample_point` and `["x", "y", "z"]` in `eval_field_op`).
/// Changing the cap here automatically keeps both assert messages in sync.
const MAX_POINT_ARITY: usize = 3;

/// Build a sample point for the given domain type.
///
/// Returns `(Value, Type)` where the `Value` encodes coordinates (1.0, 2.0, 3.0)
/// with component values matching the domain's quantity type:
///  - `Point{n, Real}` or `Point{n, Int}` → components are `Value::Real`
///  - `Point{n, Scalar{dim}}` → components are `Value::Scalar { si_value, dimension }`
///  - bare `Type::Real` / `Type::Int` → `(Value::Real(1.0), Type::Real)`
///  - bare `Type::Scalar{dim}` → `(Value::Scalar{1.0, dim}, domain.clone())`
///
/// Eliminates the `(Value, Type)` desynchronisation risk of constructing sample
/// points manually.
fn make_sample_point(domain: &Type) -> (Value, Type) {
    match domain {
        Type::Point { n, quantity } => {
            assert!(
                *n <= MAX_POINT_ARITY,
                "make_sample_point: Point domain only supports up to {MAX_POINT_ARITY} dimensions, got {n}"
            );
            let coords = [1.0f64, 2.0, 3.0];
            let comps: Vec<Value> = coords[..*n]
                .iter()
                .map(|&v| match quantity.as_ref() {
                    Type::Real | Type::Int => Value::Real(v),
                    Type::Scalar { dimension } => Value::Scalar {
                        si_value: v,
                        dimension: *dimension,
                    },
                    other => panic!(
                        "make_sample_point: unsupported quantity type in Point: {:?}",
                        other
                    ),
                })
                .collect();
            (Value::Point(comps), domain.clone())
        }
        Type::Real | Type::Int => (Value::Real(1.0), Type::Real),
        Type::Scalar { dimension } => (
            Value::Scalar {
                si_value: 1.0,
                dimension: *dimension,
            },
            domain.clone(),
        ),
        other => panic!("make_sample_point: unsupported domain type: {:?}", other),
    }
}

#[test]
#[should_panic(expected = "make_sample_point: Point domain only supports")]
fn make_sample_point_panics_when_point_arity_exceeds_three() {
    let domain = Type::Point {
        n: 4,
        quantity: Box::new(Type::Real),
    };
    let _ = make_sample_point(&domain);
}

/// Returns the component type for a field codomain.
///
/// For a `Type::Vector { quantity, .. }` codomain, returns the inner `*quantity`
/// (e.g., `Vec3(Scalar<Velocity>)` → `Scalar<Velocity>`).  For all other types
/// (scalar, Real, etc.) the codomain itself is returned unchanged, since non-vector
/// codomains are already their own component type.
fn codomain_component_type(codomain: &Type) -> Type {
    match codomain {
        Type::Vector { quantity, .. } => (**quantity).clone(),
        other => other.clone(),
    }
}

/// Build the standard analytical lambda body for `eval_field_op`.
///
/// Stamps every `value_ref` and intermediate node with the codomain's component
/// type (see `codomain_component_type`):
///  - Vector codomain `Vec{n}(Q)` → each arg is stamped with `Q`; the outer
///    `FunctionCall` result_type is the full codomain.
///  - Scalar / Real / other codomain → value_refs and every intermediate
///    `BinOp::Add` node are stamped with the codomain itself (since for non-Vector
///    codomains `codomain_component_type` returns the codomain unchanged).
///
/// This ensures the body's static type annotations are consistent with the declared
/// field codomain — the invariant exercised by the Case B regression guards
/// (`divergence_sample_mixed_real_to_velocity_returns_scalar` and
/// `laplacian_sample_mixed_real_to_temperature_returns_scalar`).
fn build_eval_field_op_body(ids: &[ValueCellId], codomain: &Type) -> CompiledExpr {
    let component_ty = codomain_component_type(codomain);
    match codomain {
        Type::Vector { n: vec_n, .. } => {
            // Identity: vec_n(x, y, z, ...) — each arg stamped with the inner quantity type.
            let args: Vec<CompiledExpr> = ids
                .iter()
                .map(|id| CompiledExpr::value_ref(id.clone(), component_ty.clone()))
                .collect();
            make_function_call(&format!("vec{vec_n}"), args, codomain.clone())
        }
        _ => {
            // Linear sum: x + y + z + ...
            // All value_refs and BinOp::Add intermediate nodes are stamped with
            // component_ty (== codomain for non-Vector codomains).
            let mut acc = CompiledExpr::value_ref(ids[0].clone(), component_ty.clone());
            for id in &ids[1..] {
                acc = CompiledExpr::binop(
                    BinOp::Add,
                    acc,
                    CompiledExpr::value_ref(id.clone(), component_ty.clone()),
                    component_ty.clone(),
                );
            }
            acc
        }
    }
}

/// Evaluate a calculus operator on a standard analytical test field.
///
/// Builds a `Value::Field` with the given `domain` and `codomain` types, using a
/// standard lambda body built by [`build_eval_field_op_body`]:
///  - Vector codomain → identity `vec_n(x, y, z, ...)` (passes params straight through)
///  - Scalar / Real / other codomain → linear sum `x + y + z + ...`
///
/// Value refs and intermediate nodes inside the body are stamped with the codomain's
/// component type (`codomain_component_type(codomain)`): the inner quantity for
/// Vector codomains, or the codomain itself for scalar/Real shapes.  This keeps the
/// body statically type-consistent with the declared field codomain.
///
/// Supports `"gradient"`, `"divergence"`, `"curl"`, and `"laplacian"`.
/// Returns the operator-result `Value` (a `Value::Field` for valid inputs).
fn eval_field_op(op: &str, domain: Type, codomain: Type) -> Value {
    let n: usize = match &domain {
        Type::Point { n, .. } => {
            assert!(
                *n <= MAX_POINT_ARITY,
                "eval_field_op: Point domain only supports up to {MAX_POINT_ARITY} dimensions, got {n}"
            );
            *n
        }
        _ => 1,
    };

    let names = ["x", "y", "z"];
    let ids: Vec<ValueCellId> = names[..n]
        .iter()
        .map(|&name| ValueCellId::new("$lambda0.S", name))
        .collect();

    // Build lambda body using the extracted helper.
    let body = build_eval_field_op_body(&ids, &codomain);

    let params: Vec<(&str, ValueCellId)> = names[..n].iter().copied().zip(ids).collect();
    let lambda = make_value_lambda(params, body, ValueMap::new());

    let (field, field_type) = make_analytical_field(domain, codomain, lambda);

    let op_expr = make_function_call(
        op,
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // placeholder result_type; not used by the evaluator
    );

    let values = ValueMap::new();
    eval_expr(&op_expr, &EvalContext::simple(&values))
}

#[test]
#[should_panic(expected = "eval_field_op: Point domain only supports")]
fn eval_field_op_panics_when_point_arity_exceeds_three() {
    let domain = Type::Point {
        n: 4,
        quantity: Box::new(Type::Real),
    };
    let _ = eval_field_op("gradient", domain, Type::Real);
}

/// `codomain_component_type` returns the inner quantity for Vector codomains
/// and the codomain itself for all non-Vector shapes.
///
/// Cases:
///   (a) Vec3(Scalar<Velocity>) → Scalar<Velocity>
///   (b) Vec3(Real)             → Real
///   (c) Vec2(Scalar<Length>)   → Scalar<Length>
///   (d) Scalar<Temperature>    → Scalar<Temperature>
///   (e) Real                   → Real
#[test]
fn codomain_component_type_returns_vector_quantity_or_codomain_itself() {
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);

    // (a) Vec3(Scalar<Velocity>) → Scalar<Velocity>
    let vel_scalar = Type::Scalar {
        dimension: velocity_dim,
    };
    let vec3_velocity = Type::vec3(vel_scalar.clone());
    assert_eq!(
        codomain_component_type(&vec3_velocity),
        vel_scalar,
        "Vec3(Scalar<Velocity>) should yield Scalar<Velocity>"
    );

    // (b) Vec3(Real) → Real
    let vec3_real = Type::vec3(Type::Real);
    assert_eq!(
        codomain_component_type(&vec3_real),
        Type::Real,
        "Vec3(Real) should yield Real"
    );

    // (c) Vec2(Scalar<Length>) → Scalar<Length>
    let length_scalar = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let vec2_length = Type::vec2(length_scalar.clone());
    assert_eq!(
        codomain_component_type(&vec2_length),
        length_scalar,
        "Vec2(Scalar<Length>) should yield Scalar<Length>"
    );

    // (d) Scalar<Temperature> → Scalar<Temperature>
    let temp_scalar = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };
    assert_eq!(
        codomain_component_type(&temp_scalar),
        temp_scalar.clone(),
        "Scalar<Temperature> should yield itself"
    );

    // (e) Real → Real
    assert_eq!(
        codomain_component_type(&Type::Real),
        Type::Real,
        "Real should yield itself"
    );
}

/// `build_eval_field_op_body` (Vector branch) stamps each `value_ref` with the
/// codomain's component type (the inner quantity of the Vector), not `Type::Real`.
///
/// Case 1: Vec3(Scalar<Velocity>) → component type is Scalar<Velocity>.
/// Case 2: Vec3(Real)             → component type is Real (regression check).
///
/// For each case the test asserts:
/// (a) top-level kind is FunctionCall with name "vec3" and result_type == codomain;
/// (b) exactly 3 ValueRef nodes are present in the tree;
/// (c) every ValueRef's result_type equals the expected component type.
#[test]
fn build_eval_field_op_body_vector_branch_stamps_codomain_component_type() {
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let ids: Vec<ValueCellId> = ["x", "y", "z"]
        .iter()
        .map(|&name| ValueCellId::new("$lambda0.S", name))
        .collect();

    // ── Case 1: Vec3(Scalar<Velocity>) ───────────────────────────────────────
    let vel_scalar = Type::Scalar {
        dimension: velocity_dim,
    };
    let vec3_velocity = Type::vec3(vel_scalar.clone());
    let body1 = build_eval_field_op_body(&ids, &vec3_velocity);

    // (a) top-level kind and result_type
    match &body1.kind {
        CompiledExprKind::FunctionCall { function, .. } => {
            assert_eq!(
                function.name, "vec3",
                "case 1: expected FunctionCall 'vec3', got {:?}",
                function.name
            );
        }
        other => panic!("case 1: expected FunctionCall, got {:?}", other),
    }
    assert_eq!(
        body1.result_type, vec3_velocity,
        "case 1: top-level result_type should be Vec3(Velocity), got {:?}",
        body1.result_type
    );

    // (b,c) collect value_ref result_types
    let mut value_ref_types: Vec<Type> = Vec::new();
    body1.walk(&mut |node| {
        if matches!(&node.kind, CompiledExprKind::ValueRef(_)) {
            value_ref_types.push(node.result_type.clone());
        }
    });
    assert_eq!(
        value_ref_types.len(),
        3,
        "case 1: expected 3 ValueRef nodes, got {}",
        value_ref_types.len()
    );
    for ty in &value_ref_types {
        assert_eq!(
            *ty, vel_scalar,
            "case 1: each ValueRef should have result_type Scalar<Velocity>, got {:?}",
            ty
        );
    }

    // ── Case 2: Vec3(Real) — regression check ────────────────────────────────
    let vec3_real = Type::vec3(Type::Real);
    let body2 = build_eval_field_op_body(&ids, &vec3_real);

    match &body2.kind {
        CompiledExprKind::FunctionCall { function, .. } => {
            assert_eq!(
                function.name, "vec3",
                "case 2: expected FunctionCall 'vec3', got {:?}",
                function.name
            );
        }
        other => panic!("case 2: expected FunctionCall, got {:?}", other),
    }
    assert_eq!(
        body2.result_type, vec3_real,
        "case 2: top-level result_type should be Vec3(Real), got {:?}",
        body2.result_type
    );

    let mut value_ref_types2: Vec<Type> = Vec::new();
    body2.walk(&mut |node| {
        if matches!(&node.kind, CompiledExprKind::ValueRef(_)) {
            value_ref_types2.push(node.result_type.clone());
        }
    });
    assert_eq!(
        value_ref_types2.len(),
        3,
        "case 2: expected 3 ValueRef nodes, got {}",
        value_ref_types2.len()
    );
    for ty in &value_ref_types2 {
        assert_eq!(
            *ty,
            Type::Real,
            "case 2: each ValueRef should have result_type Real, got {:?}",
            ty
        );
    }
}

/// `build_eval_field_op_body` (scalar branch) stamps every `value_ref` and every
/// intermediate `BinOp::Add` node with the codomain's component type.
///
/// Case 1: Scalar<Temperature> → all nodes stamped with Scalar<Temperature>.
/// Case 2: Real                → all nodes stamped with Real (regression check).
///
/// For each case the test asserts:
/// (a) top-level kind is BinOp(Add) with result_type == codomain;
/// (b) exactly 3 ValueRef nodes present, all with result_type == component type;
/// (c) exactly 2 BinOp(Add) nodes (the nested `(x+y)+z`), all with result_type == component type.
#[test]
fn build_eval_field_op_body_scalar_branch_stamps_codomain_into_sum_nodes() {
    let ids: Vec<ValueCellId> = ["x", "y", "z"]
        .iter()
        .map(|&name| ValueCellId::new("$lambda0.S", name))
        .collect();

    // ── Case 1: Scalar<Temperature> ─────────────────────────────────────────
    let temp_scalar = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };
    let body1 = build_eval_field_op_body(&ids, &temp_scalar);

    // (a) top-level BinOp(Add) with result_type == codomain
    match &body1.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(
                *op,
                BinOp::Add,
                "case 1: expected BinOp::Add at top level, got {:?}",
                op
            );
        }
        other => panic!("case 1: expected BinOp, got {:?}", other),
    }
    assert_eq!(
        body1.result_type, temp_scalar,
        "case 1: top-level result_type should be Scalar<Temperature>, got {:?}",
        body1.result_type
    );

    // (b,c) walk and collect ValueRef and BinOp(Add) types
    let mut value_ref_types1: Vec<Type> = Vec::new();
    let mut binop_add_types1: Vec<Type> = Vec::new();
    body1.walk(&mut |node| match &node.kind {
        CompiledExprKind::ValueRef(_) => value_ref_types1.push(node.result_type.clone()),
        CompiledExprKind::BinOp { op, .. } if *op == BinOp::Add => {
            binop_add_types1.push(node.result_type.clone());
        }
        _ => {}
    });
    assert_eq!(
        value_ref_types1.len(),
        3,
        "case 1: expected 3 ValueRef nodes, got {}",
        value_ref_types1.len()
    );
    assert_eq!(
        binop_add_types1.len(),
        2,
        "case 1: expected 2 BinOp(Add) nodes, got {}",
        binop_add_types1.len()
    );
    for ty in &value_ref_types1 {
        assert_eq!(
            *ty, temp_scalar,
            "case 1: each ValueRef should be Scalar<Temperature>, got {:?}",
            ty
        );
    }
    for ty in &binop_add_types1 {
        assert_eq!(
            *ty, temp_scalar,
            "case 1: each BinOp(Add) should be Scalar<Temperature>, got {:?}",
            ty
        );
    }

    // ── Case 2: Real — regression check ──────────────────────────────────────
    let body2 = build_eval_field_op_body(&ids, &Type::Real);

    match &body2.kind {
        CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(
                *op,
                BinOp::Add,
                "case 2: expected BinOp::Add at top level, got {:?}",
                op
            );
        }
        other => panic!("case 2: expected BinOp, got {:?}", other),
    }
    assert_eq!(
        body2.result_type,
        Type::Real,
        "case 2: top-level result_type should be Real, got {:?}",
        body2.result_type
    );

    let mut value_ref_types2: Vec<Type> = Vec::new();
    let mut binop_add_types2: Vec<Type> = Vec::new();
    body2.walk(&mut |node| match &node.kind {
        CompiledExprKind::ValueRef(_) => value_ref_types2.push(node.result_type.clone()),
        CompiledExprKind::BinOp { op, .. } if *op == BinOp::Add => {
            binop_add_types2.push(node.result_type.clone());
        }
        _ => {}
    });
    assert_eq!(
        value_ref_types2.len(),
        3,
        "case 2: expected 3 ValueRef nodes, got {}",
        value_ref_types2.len()
    );
    assert_eq!(
        binop_add_types2.len(),
        2,
        "case 2: expected 2 BinOp(Add) nodes, got {}",
        binop_add_types2.len()
    );
    for ty in &value_ref_types2 {
        assert_eq!(
            *ty,
            Type::Real,
            "case 2: each ValueRef should be Real, got {:?}",
            ty
        );
    }
    for ty in &binop_add_types2 {
        assert_eq!(
            *ty,
            Type::Real,
            "case 2: each BinOp(Add) should be Real, got {:?}",
            ty
        );
    }
}

/// Sample a field value at the standard test point for its domain type.
///
/// Extracts `codomain_type` from the `Value::Field`, calls `make_sample_point` to
/// build a domain-compatible point, constructs a `sample(field, point)` call, and
/// evaluates it.
///
/// Panics if `field` is not `Value::Field`.
fn sample_field(field: Value, domain: Type) -> Value {
    let codomain = match &field {
        Value::Field { codomain_type, .. } => codomain_type.clone(),
        other => panic!("sample_field: expected Value::Field, got {:?}", other),
    };

    let field_type = Type::Field {
        domain: Box::new(domain.clone()),
        codomain: Box::new(codomain.clone()),
    };

    let (point_val, point_type) = make_sample_point(&domain);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(point_val, point_type),
        ],
        codomain,
    );

    let values = ValueMap::new();
    eval_expr(&sample_expr, &EvalContext::simple(&values))
}

// ── Step 1: Gradient accuracy tests ──────────────────────────────────────────

/// Gradient of f(x) = x*x at x=3.0 should be ≈6.0.
///
/// Analytical derivative: df/dx = 2x. At x=3.0: 2*3=6.0.
/// Central differences with h~1e-6 gives O(h²) error, well within 1e-4.
#[test]
fn gradient_1d_quadratic_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Real;

    let (field, field_type) = make_analytical_field(domain_type, Type::Real, lambda);

    // gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 1D quadratic should return a Field, got {:?}",
        grad_result
    );

    // sample(gradient_field, 3.0)
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result
        .as_f64()
        .unwrap_or_else(|| panic!("gradient sample should be numeric, got {:?}", sample_result));
    assert!(
        (val - 6.0).abs() < 1e-4,
        "gradient of x*x at x=3.0 should be ≈6.0, got {}",
        val
    );
}

/// Gradient of f(x,y,z)=x²+y²+z² at (1,2,3) should be ≈[2,4,6].
///
/// Partial derivatives: df/dx=2x, df/dy=2y, df/dz=2z.
/// At (1,2,3): [2*1, 2*2, 2*3] = [2, 4, 6].
#[test]
fn gradient_3d_sum_of_squares_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), Type::Real, lambda);

    // gradient(field)
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        gradient_result_type(domain_type.clone(), 3),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of 3D field should return a Field, got {:?}",
        grad_result
    );

    // sample(gradient_field, Point3(1.0, 2.0, 3.0))
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(grad_result, gradient_result_type(domain_type.clone(), 3)),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Expected: Vector3(2.0, 4.0, 6.0)
    assert_gradient_vector(
        &sample_result,
        &[2.0, 4.0, 6.0],
        1e-4,
        "gradient of x²+y²+z² at (1,2,3)",
    );
}

// ── Steps 2–3: Divergence and curl tests ─────────────────────────────────────

/// Divergence of the identity vector field F(x,y,z)=[x,y,z] at (1,2,3) ≈ 3.0.
///
/// Analytical divergence: ∂x/∂x + ∂y/∂y + ∂z/∂z = 1 + 1 + 1 = 3.
/// divergence wraps around central-difference sampling; tolerance 1e-3
/// accounts for multi-component summation.
#[test]
fn divergence_identity_vector_field() {
    run_divergence_identity_test(
        SamplePoint::Point3([1.0, 2.0, 3.0]),
        "divergence of [x,y,z] at (1,2,3)",
    );
}

/// Same as `divergence_identity_vector_field` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_divergence_at_point` only matched `Value::Point`.
#[test]
fn divergence_accepts_vector_sample_point() {
    run_divergence_identity_test(
        SamplePoint::Vector3([1.0, 2.0, 3.0]),
        "divergence of [x,y,z] at Vector(1,2,3)",
    );
}

// ── Step 3: Curl test ─────────────────────────────────────────────────────────

/// Curl of the rotation field F(x,y,z)=[-y,x,0] at (1,2,3) ≈ [0,0,2].
///
/// Analytical curl: (∂Fz/∂y - ∂Fy/∂z, ∂Fx/∂z - ∂Fz/∂x, ∂Fy/∂x - ∂Fx/∂y)
///   = (0-0, 0-0, 1-(-1)) = [0, 0, 2].
/// Tolerance 1e-3 accounts for multi-component numerical differentiation.
#[test]
fn curl_rotation_field() {
    run_curl_rotation_test(
        SamplePoint::Point3([1.0, 2.0, 3.0]),
        "curl of [-y,x,0] at (1,2,3)",
    );
}

/// Same as `curl_rotation_field` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_curl_at_point` only matched `Value::Point`.
#[test]
fn curl_accepts_vector_sample_point() {
    run_curl_rotation_test(
        SamplePoint::Vector3([1.0, 2.0, 3.0]),
        "curl of [-y,x,0] at Vector(1,2,3)",
    );
}

/// Sampling a curl field with a 2-element `Value::Vector` returns `Value::Undef`.
///
/// `compute_numerical_curl_at_point` requires exactly 3 components to compute
/// the cross-product derivatives.  A 2-element input falls through the
/// `items.len() == 3` guard and must return `Value::Undef`.  This test locks in
/// that dimension guard as a regression check.
#[test]
fn curl_two_element_vector_sample_point_returns_undef() {
    let label = "curl_two_element_vector_sample_point_returns_undef";
    let (curl_result, curl_field_type) = build_curl_rotation_field(label);

    // vec2 type is intentional — compute_numerical_curl_at_point matches on Value shape at
    // runtime (items.len() == 3 guard), not the declared static type. This exercises the
    // dimension guard.
    let (point, point_literal_type) = SamplePoint::Vector2([1.0, 2.0]).into_value_and_type();

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert!(
        matches!(sample_result, Value::Undef),
        "{label}: curl with 2-element Vector sample point should return Value::Undef (dimension guard), got {:?}",
        sample_result
    );
}

/// Sampling a divergence field with a 2-element `Value::Vector` returns `Value::Undef`.
///
/// Unlike curl's explicit `items.len()==3` guard, this `Undef` arises because the
/// identity field's 3-param lambda receives only 2 coordinates, leaving the third
/// unbound; strict `Undef` propagation in `FunctionCall` then cascades.
#[test]
fn divergence_two_element_vector_sample_point_returns_undef() {
    let label = "divergence_two_element_vector_sample_point_returns_undef";
    let (div_result, div_field_type) = build_divergence_identity_field(label);

    // vec2 type is intentional — compute_numerical_divergence_at_point accepts any non-empty
    // dimension vector (no explicit len==3 guard). Undef arises because the identity field's
    // 3-param lambda receives only 2 coordinates, leaving the third unbound; strict Undef
    // propagation in FunctionCall then cascades.
    let (point, point_literal_type) = SamplePoint::Vector2([1.0, 2.0]).into_value_and_type();

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, div_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert!(
        matches!(sample_result, Value::Undef),
        "{label}: divergence with 2-element Vector sample point should return Value::Undef, got {:?}",
        sample_result
    );
}

// ── Step 4: Laplacian test ────────────────────────────────────────────────────

/// Laplacian of f(x,y,z)=x²+y²+z² at (1,2,3) ≈ 6.0.
///
/// Laplacian = divergence(gradient(f)).
/// Second partials: ∂²f/∂x² + ∂²f/∂y² + ∂²f/∂z² = 2+2+2 = 6.
/// Tolerance 1e-2 accounts for two levels of numerical differentiation.
#[test]
fn laplacian_quadratic_accuracy() {
    run_laplacian_quadratic_test(
        SamplePoint::Point3([1.0, 2.0, 3.0]),
        "laplacian of x²+y²+z² at (1,2,3)",
    );
}

/// Same as `laplacian_quadratic_accuracy` but the sample point is
/// supplied as `Value::Vector` instead of `Value::Point`.
/// Prior to the fix this returns `Value::Undef` because
/// `compute_numerical_laplacian_at_point` only matched `Value::Point`.
#[test]
fn laplacian_accepts_vector_sample_point() {
    run_laplacian_quadratic_test(
        SamplePoint::Vector3([1.0, 2.0, 3.0]),
        "laplacian of x²+y²+z² at Vector(1,2,3)",
    );
}

/// Sampling a laplacian field with a 2-element `Value::Vector` returns `Value::Undef`.
///
/// Unlike curl's explicit `items.len()==3` guard, this `Undef` arises because the
/// quadratic field's 3-param lambda receives only 2 coordinates, leaving the third
/// unbound; strict `Undef` propagation in `FunctionCall` then cascades.
#[test]
fn laplacian_two_element_vector_sample_point_returns_undef() {
    let label = "laplacian_two_element_vector_sample_point_returns_undef";
    let (lap_result, lap_field_type) = build_laplacian_quadratic_field(label);

    // vec2 type is intentional — compute_numerical_laplacian_at_point accepts any non-empty
    // dimension vector (no explicit len==3 guard). Undef arises because the quadratic field's
    // 3-param lambda receives only 2 coordinates, leaving the third unbound; strict Undef
    // propagation in FunctionCall then cascades.
    let (point, point_literal_type) = SamplePoint::Vector2([1.0, 2.0]).into_value_and_type();

    let values = ValueMap::new();
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, lap_field_type),
            CompiledExpr::literal(point, point_literal_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert!(
        matches!(sample_result, Value::Undef),
        "{label}: laplacian with 2-element Vector sample point should return Value::Undef, got {:?}",
        sample_result
    );
}

// ── Step 5: Codomain type tests ───────────────────────────────────────────────

/// Gradient of a Real→Real field produces a Field with scalar (Real) codomain.
///
/// For a 1D field, the gradient is a scalar derivative (same dimensionality
/// as the codomain). Verify the gradient Field has codomain_type=Real.
#[test]
fn gradient_1d_scalar_codomain_type() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x * x
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let (field, field_type) = make_analytical_field(Type::Real, Type::Real, lambda);

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type.clone())],
        field_type,
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    // gradient should return a Field
    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Real→Real should return a Field, got {:?}",
        grad_result
    );

    // Codomain should be scalar (Real for dimensionless domain/codomain)
    if let Value::Field { codomain_type, .. } = &grad_result {
        // 1D gradient of Real→Real produces Real codomain
        match codomain_type {
            Type::Real => {} // correct
            Type::Scalar { dimension } if *dimension == DimensionVector::DIMENSIONLESS => {} // also fine
            other => panic!(
                "gradient_1d_scalar_codomain_type: expected Real or dimensionless Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Gradient of a Point3<Real>→Real field produces a Field with Vector3 codomain.
///
/// For a 3D scalar field, the gradient is a vector of partial derivatives.
/// Verify the gradient Field has codomain_type of vector kind.
#[test]
fn gradient_3d_vector_codomain_type() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + y + z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            Type::Real,
        ),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), Type::Real, lambda);

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        gradient_result_type(domain_type, 3),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point3→Real should return a Field, got {:?}",
        grad_result
    );

    // Codomain should be a vector type (Vector { n: 3, .. } or equivalent)
    if let Value::Field { codomain_type, .. } = &grad_result {
        assert!(
            matches!(codomain_type, Type::Vector { n: 3, .. }),
            "gradient of 3D scalar field should have Vector(n=3) codomain, got {:?}",
            codomain_type
        );
    }
}

// ── Step 6: Sample identity test ─────────────────────────────────────────────

/// sample(field, 3.0) returns exactly 7.0 for f(x)=2*x+1.
///
/// Tests the sample pathway independently of differential operators.
/// Confirms field evaluation works as a baseline before testing operators
/// that depend on sampling.
#[test]
fn field_composition_sample_identity() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2*x + 1
    let two_x = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        two_x,
        CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let (field, field_type) = make_analytical_field(Type::Real, Type::Real, lambda);

    // sample(field, 3.0) → 2*3+1 = 7.0
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_eq!(
        result,
        Value::Real(7.0),
        "sample(f(x)=2*x+1, 3.0) should return exactly 7.0, got {:?}",
        result
    );
}

// ── Step 7: Dimensional correctness test ──────────────────────────────────────

/// Gradient of a Length→Temperature field has codomain dimension Temperature/Length.
///
/// For f: Scalar<Length> → Scalar<Temperature> with lambda |x| → 2*x,
/// gradient codomain_type should have dimension TEMPERATURE / LENGTH.
/// This verifies the R/Q dimensional arithmetic in compute_gradient.
#[test]
fn gradient_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| 2.0 * x (temperature field over length domain)
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let domain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let (field, field_type) = make_analytical_field(domain_type, codomain_type.clone(), lambda);

    // gradient(field) → gradient field with dimension Temperature/Length
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Length→Temperature field should return a Field, got {:?}",
        grad_result
    );

    // Verify codomain dimension: should be Temperature / Length
    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected_dim = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH);
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension, expected_dim,
                    "gradient codomain dimension should be Temperature/Length ({:?}), got {:?}",
                    expected_dim, dimension
                );
            }
            other => panic!(
                "gradient_dimensional_correctness: expected Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

// ── Step 8: Robustness tests ──────────────────────────────────────────────────

/// Divergence of constant vector field F(x,y,z)=[1,1,1] should be ≈0.
///
/// The divergence of a constant field is exactly zero (no variation in any direction).
/// Central differences should give near-zero result within 1e-6.
#[test]
fn divergence_constant_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(1.0, 1.0, 1.0) (constant vector field)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
            CompiledExpr::literal(Value::Real(1.0), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), Type::vec3(Type::Real), lambda);

    // divergence(field) → scalar field ≈ 0
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        scalar_field_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of constant vector field should return a Field, got {:?}",
        div_result
    );

    // sample at any point, expect near 0
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(div_result, scalar_field_result_type(domain_type.clone())),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "divergence sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        val.abs() < 1e-6,
        "divergence of constant [1,1,1] should be ≈0, got {}",
        val
    );
}

/// Gradient of linear f(x,y,z)=x+2*y+3*z is constant [1,2,3] everywhere.
///
/// Verify gradient at two different points both give ≈[1,2,3] within 1e-4.
#[test]
fn gradient_linear_field_constant() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let two_y = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let three_z = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(3.0), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            two_y,
            Type::Real,
        ),
        three_z,
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), Type::Real, lambda);

    // gradient(field) should give constant [1, 2, 3]
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        gradient_result_type(domain_type.clone(), 3),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient should return a Field, got {:?}",
        grad_result
    );

    // Verify at two different points
    let test_points = [
        Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]),
        Value::Point(vec![Value::Real(5.0), Value::Real(10.0), Value::Real(15.0)]),
    ];

    for (i, point) in test_points.iter().enumerate() {
        let sample_expr = make_function_call(
            "sample",
            vec![
                CompiledExpr::literal(
                    grad_result.clone(),
                    gradient_result_type(domain_type.clone(), 3),
                ),
                CompiledExpr::literal(point.clone(), domain_type.clone()),
            ],
            Type::vec3(Type::Real),
        );
        let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));
        assert_gradient_vector(
            &sample_result,
            &[1.0, 2.0, 3.0],
            1e-4,
            &format!("gradient of x+2y+3z at point {i}"),
        );
    }
}

/// Laplacian of linear f(x,y,z)=x+2*y+3*z should be ≈0.
///
/// Second partial derivatives of a linear function are all zero.
/// Tolerance 1e-4 accounts for two levels of numerical differentiation.
#[test]
fn laplacian_linear_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x + 2*y + 3*z
    let two_y = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(2.0), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let three_z = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(Value::Real(3.0), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            two_y,
            Type::Real,
        ),
        three_z,
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), Type::Real, lambda);

    // laplacian(field) → scalar field ≈ 0
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        scalar_field_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of linear field should return a Field, got {:?}",
        lap_result
    );

    // sample at (1, 2, 3)
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, scalar_field_result_type(domain_type.clone())),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "laplacian sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        val.abs() < 1e-4,
        "laplacian of linear field should be ≈0, got {}",
        val
    );
}

// ── Step 9: Dimensional correctness tests ─────────────────────────────────────

/// Divergence of a Point{3,Length} → Vector{3,Velocity} field has codomain
/// dimension Velocity/Length = (Length/Time)/Length = 1/Time.
///
/// This verifies that compute_divergence correctly derives the result codomain
/// dimension from the input field's domain and codomain component dimensions,
/// rather than unconditionally returning Type::Real.
#[test]
fn divergence_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // VELOCITY = LENGTH / TIME  (derived dimension)
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_quantity = Type::Scalar {
        dimension: velocity_dim,
    };

    let domain_type = Type::point3(domain_quantity.clone());
    let codomain_type = Type::vec3(codomain_quantity.clone());

    // Lambda: |x, y, z| vec3(x, y, z) — simple identity used only for
    // metadata test; we do not sample from this field.
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    // divergence(field) → scalar field with codomain = Velocity/Length = 1/Time
    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Length}}→Vector{{3,Velocity}} should return a Field, got {:?}",
        div_result
    );

    // Verify codomain dimension: should be Velocity / Length = 1/Time
    if let Value::Field { codomain_type, .. } = &div_result {
        let expected_dim = velocity_dim.div(&DimensionVector::LENGTH);
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension, expected_dim,
                    "divergence codomain should be Velocity/Length=1/Time ({:?}), got {:?}",
                    expected_dim, dimension
                );
            }
            other => panic!(
                "divergence_dimensional_correctness: expected Type::Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Laplacian of a Point{3,Length} → Scalar<Temperature> field has codomain
/// dimension Temperature/Length² = Temperature.div(&LENGTH.pow(2)).
///
/// This verifies that compute_laplacian correctly derives the result codomain
/// dimension by dividing the input codomain dimension by domain_dim², rather
/// than preserving the input codomain_type unchanged.
#[test]
fn laplacian_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let domain_type = Type::point3(domain_quantity.clone());

    // Lambda: |x, y, z| x + y + z — simple body used only for metadata test;
    // we do not sample from this field.
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            Type::Real,
        ),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    // laplacian(field) → scalar field with codomain = Temperature / Length²
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Scalar<Temperature> should return a Field, got {:?}",
        lap_result
    );

    // Verify codomain dimension: should be Temperature / Length²
    if let Value::Field { codomain_type, .. } = &lap_result {
        let expected_dim = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2));
        match codomain_type {
            Type::Scalar { dimension } => {
                assert_eq!(
                    *dimension, expected_dim,
                    "laplacian codomain should be Temperature/Length² ({:?}), got {:?}",
                    expected_dim, dimension
                );
            }
            other => panic!(
                "laplacian_dimensional_correctness: expected Type::Scalar codomain, got {:?}",
                other
            ),
        }
    }
}

/// Divergence of a dimensionless Point{3,Real} → Vector{3,Real} field still
/// returns Type::Real as the result codomain (regression guard).
///
/// Ensures the fallback path in compute_divergence does not break the existing
/// behaviour for dimensionless fields now that the dimensioned path is wired up.
#[test]
fn divergence_dimensionless_still_real() {
    let div_result = eval_field_op(
        "divergence",
        Type::point3(Type::Real),
        Type::vec3(Type::Real),
    );
    let Value::Field {
        codomain_type: ref actual_codomain,
        ..
    } = div_result
    else {
        panic!(
            "divergence_dimensionless_still_real: expected Field, got {:?}",
            div_result
        );
    };
    assert_eq!(
        *actual_codomain,
        Type::Real,
        "divergence of dimensionless Point{{3,Real}}→Vector{{3,Real}} should have codomain \
         Type::Real, got {:?}",
        actual_codomain
    );
}

// ── Step 10: Sample-level dimensional correctness tests ───────────────────────

/// Regression guard: sampling from the divergence of a dimensionless
/// Point{3,Real}→Vector{3,Real} field returns `Value::Real`, not `Value::Scalar`.
///
/// Locks in the dimensionless fallback path in compute_numerical_divergence_at_point
/// so the step-3 implementation change cannot regress it.
#[test]
fn divergence_sample_dimensionless_returns_real() {
    let domain = Type::point3(Type::Real);
    let div_result = eval_field_op("divergence", domain.clone(), Type::vec3(Type::Real));
    let sampled = sample_field(div_result, domain);
    match sampled {
        Value::Real(v) => {
            // The identity body `vec3(x, y, z)` has divergence ∂x/∂x + ∂y/∂y + ∂z/∂z = 3.0.
            assert!(
                (v - 3.0).abs() < 1e-4,
                "divergence_sample_dimensionless_returns_real: si_value should be ≈3.0 \
                 (identity body ∂x/∂x+∂y/∂y+∂z/∂z = 3.0), got {}",
                v
            );
        }
        Value::Scalar { .. } => panic!(
            "divergence_sample_dimensionless_returns_real: expected Value::Real but got \
             Value::Scalar — the dimensionless fallback path is broken"
        ),
        other => panic!(
            "divergence_sample_dimensionless_returns_real: expected Value::Real, got {:?}",
            other
        ),
    }
}

/// Runtime drift test: sampling from the divergence of a dimensioned
/// Point{3,Length}→Vector{3,Velocity} field should return
/// `Value::Scalar { dimension: 1/Time }`, not `Value::Real`.
///
/// Expected result dimension: Velocity/Length = (Length/Time)/Length = 1/Time.
///
/// FAILS before step-3 implementation because compute_numerical_divergence_at_point
/// returns Value::Real unconditionally.
#[test]
fn divergence_sample_dimensional_correctness_returns_scalar() {
    // VELOCITY = LENGTH / TIME
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::vec3(Type::Scalar {
        dimension: velocity_dim,
    });

    let div_result = eval_field_op("divergence", domain.clone(), codomain);
    let sampled = sample_field(div_result, domain);

    // Expected: Scalar[Velocity/Length = 1/Time], si_value ≈ 3.0.
    // The identity body `vec3(x, y, z)` from eval_field_op has divergence
    // ∂x/∂x + ∂y/∂y + ∂z/∂z = 1 + 1 + 1 = 3.0 (exact analytical value).
    let one_over_time = velocity_dim.div(&DimensionVector::LENGTH);
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, one_over_time,
                "divergence sample dimension should be 1/Time ({:?}), got {:?}",
                one_over_time, dimension,
            );
            assert!(
                (si_value - 3.0).abs() < 1e-4,
                "divergence sample si_value should be ≈3.0 (identity-field divergence), \
                 got {}",
                si_value,
            );
        }
        Value::Real(_) => panic!(
            "divergence_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar but got Value::Real — runtime drift not fixed"
        ),
        other => panic!(
            "divergence_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar, got {:?}",
            other
        ),
    }
}

/// Case A placeholder — Dimensioned domain, dimensionless codomain (divergence).
///
/// A divergence of a `Point{3,Scalar<Length>} → Vector{3,Real}` field has a physical
/// result dimension of 1/Length: the codomain (dimensionless) divided by the domain
/// unit (Length).  The DESIRED behavior is therefore `Value::Scalar { dimension: 1/Length }`.
///
/// **Current behavior (bug):** `compute_divergence` calls `dim_quotient_type` with
/// `codomain_dim = DIMENSIONLESS` and `domain_dim = LENGTH`.  Because the codomain is
/// already dimensionless, the guard `cd != DIMENSIONLESS` fails and the `_ =>` arm
/// returns the fallback `Type::Real` unchanged.  `wrap_scalar_result` then produces
/// `Value::Real` — the dimensional information is lost.
///
/// **`#[ignore]` is load-bearing:** un-ignoring this test without also fixing *both*
/// `compute_divergence`/`dim_quotient_type` (type-level) *and* the
/// `compute_numerical_divergence_at_point` / `wrap_scalar_result` path (runtime) will
/// cause it to fail with `Value::Real`.  This is the early-warning signal: a naïve
/// un-ignore serves as a concrete, executable spec for the required fix.
#[test]
#[ignore = "known bug: dim_quotient_type cd==DIMENSIONLESS branch returns Type::Real, \
            losing the 1/Length result dimension; expected Value::Scalar{1/Length}"]
fn divergence_sample_mixed_length_to_real_placeholder() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    // Dimensionless codomain: Vector{3, Real}
    let codomain = Type::vec3(Type::Real);

    let div_result = eval_field_op("divergence", domain.clone(), codomain);
    let sampled = sample_field(div_result, domain);

    // Desired: Scalar[1/Length, si_value ≈ 3.0]
    // (identity body ∂x/∂x+∂y/∂y+∂z/∂z = 3.0, result dimension = 1/Length)
    let one_over_length = DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH);
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, one_over_length,
                "divergence_sample_mixed_length_to_real_placeholder: \
                 expected dimension 1/Length ({:?}), got {:?}",
                one_over_length, dimension,
            );
            assert!(
                (si_value - 3.0).abs() < 1e-4,
                "divergence_sample_mixed_length_to_real_placeholder: \
                 si_value should be ≈3.0, got {}",
                si_value
            );
        }
        other => panic!(
            "divergence_sample_mixed_length_to_real_placeholder: \
             expected Value::Scalar{{1/Length}}, got {:?}",
            other
        ),
    }
}

/// Case B regression guard — Dimensionless domain, dimensioned codomain (divergence).
///
/// A divergence of a `Point{3,Real} → Vector{3,Scalar<Velocity>}` field has a physical
/// result dimension of Velocity/dimensionless = Velocity (a derivative per dimensionless
/// coordinate keeps the codomain dimension).
///
/// **Current behavior (correct today):** `compute_divergence` calls `dim_quotient_type`
/// with `codomain_dim = Velocity` and `domain_dim = DIMENSIONLESS`.  Because the *domain*
/// is dimensionless, the guard `dd != DIMENSIONLESS` fails and the `_ =>` arm returns
/// the fallback `dimensionless_fallback(Scalar<Velocity>) = Scalar<Velocity>` unchanged.
/// `wrap_scalar_result` then produces `Value::Scalar { dimension: Velocity }` — the
/// codomain dimension is preserved.  This is physically correct.
///
/// This test is NOT `#[ignore]` — it locks in the currently-correct behavior as a
/// regression guard.  If it starts failing, it means the `_ =>` arm or
/// `dimensionless_fallback` was changed in a way that breaks the dimensionless-domain path.
#[test]
fn divergence_sample_mixed_real_to_velocity_returns_scalar() {
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    // Dimensionless domain: Point{3, Real}
    let domain = Type::point3(Type::Real);
    // Dimensioned codomain: Vector{3, Scalar<Velocity>}
    let codomain = Type::vec3(Type::Scalar {
        dimension: velocity_dim,
    });

    let div_result = eval_field_op("divergence", domain.clone(), codomain);
    let sampled = sample_field(div_result, domain);

    // Expected: Scalar[Velocity, si_value ≈ 3.0]
    // (identity body ∂x/∂x+∂y/∂y+∂z/∂z = 3.0; Velocity / dimensionless = Velocity)
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, velocity_dim,
                "divergence_sample_mixed_real_to_velocity_returns_scalar: \
                 expected dimension Velocity ({:?}), got {:?}",
                velocity_dim, dimension,
            );
            assert!(
                (si_value - 3.0).abs() < 1e-4,
                "divergence_sample_mixed_real_to_velocity_returns_scalar: \
                 si_value should be ≈3.0, got {}",
                si_value
            );
        }
        other => panic!(
            "divergence_sample_mixed_real_to_velocity_returns_scalar: \
             expected Value::Scalar{{Velocity}}, got {:?}",
            other
        ),
    }
}

/// Laplacian of a dimensionless Point{3,Real} → Real field still returns
/// Type::Real as the result codomain (regression guard).
///
/// Ensures the fallback path in compute_laplacian does not break the existing
/// behaviour for dimensionless fields now that the dimensioned path is wired up.
#[test]
fn laplacian_dimensionless_still_real() {
    let lap_result = eval_field_op("laplacian", Type::point3(Type::Real), Type::Real);
    let Value::Field {
        codomain_type: ref actual_codomain,
        ..
    } = lap_result
    else {
        panic!(
            "laplacian_dimensionless_still_real: expected Field, got {:?}",
            lap_result
        );
    };
    assert_eq!(
        *actual_codomain,
        Type::Real,
        "laplacian of dimensionless Point{{3,Real}}→Real should have codomain Type::Real, \
         got {:?}",
        actual_codomain
    );
}

/// Regression guard: sampling from the Laplacian of a dimensionless
/// Point{3,Real}→Real field returns `Value::Real`, not `Value::Scalar`.
///
/// Locks in the dimensionless fallback path in compute_numerical_laplacian_at_point
/// so the step-6 implementation change cannot regress it.
#[test]
fn laplacian_sample_dimensionless_returns_real() {
    let domain = Type::point3(Type::Real);
    let lap_result = eval_field_op("laplacian", domain.clone(), Type::Real);
    let sampled = sample_field(lap_result, domain);
    match sampled {
        Value::Real(v) => {
            // The linear body `x + y + z` has Laplacian ∂²(linear)/∂x² + ... = 0.
            assert!(
                v.abs() < 1e-4,
                "laplacian_sample_dimensionless_returns_real: si_value should be ≈0.0 \
                 (∇²(x+y+z) = 0 for linear body), got {}",
                v
            );
        }
        Value::Scalar { .. } => panic!(
            "laplacian_sample_dimensionless_returns_real: expected Value::Real but got \
             Value::Scalar — the dimensionless fallback path is broken"
        ),
        other => panic!(
            "laplacian_sample_dimensionless_returns_real: expected Value::Real, got {:?}",
            other
        ),
    }
}

/// Runtime drift test: sampling from the Laplacian of a dimensioned
/// Point{3,Length}→Scalar<Temperature> field should return
/// `Value::Scalar { dimension: Temperature/Length² }`, not `Value::Real`.
///
/// Expected result dimension: Temperature / Length².
///
/// FAILS before step-6 implementation because compute_numerical_laplacian_at_point
/// returns Value::Real unconditionally.
#[test]
fn laplacian_sample_dimensional_correctness_returns_scalar() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let lap_result = eval_field_op("laplacian", domain.clone(), codomain);
    let sampled = sample_field(lap_result, domain);

    // Expected: Scalar[Temperature/Length²]
    let temp_per_len_sq = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2));
    match sampled {
        Value::Scalar { dimension, .. } => {
            assert_eq!(
                dimension, temp_per_len_sq,
                "laplacian sample dimension should be Temperature/Length² ({:?}), got {:?}",
                temp_per_len_sq, dimension,
            );
        }
        Value::Real(_) => panic!(
            "laplacian_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar but got Value::Real — runtime drift not fixed"
        ),
        other => panic!(
            "laplacian_sample_dimensional_correctness_returns_scalar: \
             expected Value::Scalar, got {:?}",
            other
        ),
    }
}

/// Numerical accuracy regression: sampling the Laplacian of a dimensioned
/// quadratic scalar field f(x,y,z) = x²+y²+z² on Point{3,Length}→Scalar<Temperature>
/// should return `Value::Scalar { si_value ≈ 6.0, dimension: Temperature/Length² }`.
///
/// Companion to `laplacian_sample_dimensional_correctness_returns_scalar`, which
/// only checks dimensional tagging with the linear body from `eval_field_op`.
/// This test locks in the numerical value (∇²(x²+y²+z²) = 2+2+2 = 6.0) that was
/// lost when the Task 1291 refactor replaced the quadratic body with a linear one
/// whose Laplacian is 0.0.
#[test]
fn laplacian_sample_dimensional_quadratic_returns_scalar_six() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let xx = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let yy = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let zz = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        CompiledExpr::value_ref(z_id.clone(), Type::Real),
        Type::Real,
    );
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Add, xx, yy, Type::Real),
        zz,
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let (field, field_type) = make_analytical_field(domain.clone(), codomain, lambda);

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // placeholder result_type; not used by the evaluator (matches eval_field_op)
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    let sampled = sample_field(lap_result, domain);

    // Expected: Scalar[Temperature/Length², si_value ≈ 6.0].
    let temp_per_len_sq = DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2));
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, temp_per_len_sq,
                "laplacian quadratic sample dimension should be Temperature/Length² \
                 ({:?}), got {:?}",
                temp_per_len_sq, dimension,
            );
            assert!(
                // Looser tolerance than divergence (1e-4): Laplacian uses second-order
                // numerical differentiation (finite differences applied twice),
                // which accumulates more discretization error.
                (si_value - 6.0).abs() < 1e-2,
                "laplacian quadratic sample si_value should be ≈6.0 \
                 (∇²(x²+y²+z²) = 6), got {}",
                si_value,
            );
        }
        other => panic!(
            "laplacian_sample_dimensional_quadratic_returns_scalar_six: \
             expected Value::Scalar, got {:?}",
            other
        ),
    }
}

/// Case A placeholder — Dimensioned domain, dimensionless codomain (laplacian).
///
/// A Laplacian of a `Point{3,Scalar<Length>} → Real` field has a physical result
/// dimension of 1/Length²: the codomain (dimensionless) divided by the domain unit
/// squared.  The DESIRED behavior is therefore `Value::Scalar { dimension: 1/Length² }`.
///
/// **Current behavior (bug):** `compute_laplacian` calls `dim_quotient_type` with
/// `codomain_dim = DIMENSIONLESS` and `domain_dim = LENGTH`.  Because the codomain is
/// already dimensionless, the guard `cd != DIMENSIONLESS` fails and the `_ =>` arm
/// returns the fallback `Type::Real` unchanged.  `wrap_scalar_result` then produces
/// `Value::Real` — the 1/Length² dimensional information is lost.
///
/// **`#[ignore]` is load-bearing:** un-ignoring this test without also fixing *both*
/// `compute_laplacian`/`dim_quotient_type` (type-level) *and* the
/// `compute_numerical_laplacian_at_point` / `wrap_scalar_result` path (runtime) will
/// cause it to fail with `Value::Real`.  This is the early-warning signal: a naïve
/// un-ignore serves as a concrete, executable spec for the required fix.
#[test]
#[ignore = "known bug: dim_quotient_type cd==DIMENSIONLESS branch returns Type::Real, \
            losing the 1/Length\u{00b2} result dimension; expected Value::Scalar{1/Length\u{00b2}}"]
fn laplacian_sample_mixed_length_to_real_placeholder() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    // Dimensionless codomain: Real
    let codomain = Type::Real;

    let lap_result = eval_field_op("laplacian", domain.clone(), codomain);
    let sampled = sample_field(lap_result, domain);

    // Desired: Scalar[1/Length², si_value ≈ 0.0]
    // (linear body `x+y+z`, ∇²(linear) = 0; result dimension = 1/Length²)
    let one_over_length_sq = DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH.pow(2));
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, one_over_length_sq,
                "laplacian_sample_mixed_length_to_real_placeholder: \
                 expected dimension 1/Length² ({:?}), got {:?}",
                one_over_length_sq, dimension,
            );
            assert!(
                si_value.abs() < 1e-4,
                "laplacian_sample_mixed_length_to_real_placeholder: \
                 si_value should be ≈0.0 (∇²(x+y+z) = 0), got {}",
                si_value
            );
        }
        other => panic!(
            "laplacian_sample_mixed_length_to_real_placeholder: \
             expected Value::Scalar{{1/Length²}}, got {:?}",
            other
        ),
    }
}

/// Case B regression guard — Dimensionless domain, dimensioned codomain (laplacian).
///
/// A Laplacian of a `Point{3,Real} → Scalar<Temperature>` field has a physical result
/// dimension of Temperature/dimensionless = Temperature (a second derivative per
/// dimensionless² coordinate keeps the codomain dimension).
///
/// **Current behavior (correct today):** `compute_laplacian` calls `dim_quotient_type`
/// with `codomain_dim = Temperature` and `domain_dim = DIMENSIONLESS`.  Because the
/// *domain* is dimensionless, the guard `dd != DIMENSIONLESS` fails and the `_ =>` arm
/// returns the fallback `dimensionless_fallback(Scalar<Temperature>) = Scalar<Temperature>`
/// unchanged.  `wrap_scalar_result` then produces `Value::Scalar { dimension: Temperature }`.
/// This is physically correct.
///
/// This test is NOT `#[ignore]` — it locks in the currently-correct behavior as a
/// regression guard.  If it starts failing, the `_ =>` arm or `dimensionless_fallback`
/// was changed in a way that breaks the dimensionless-domain path.
#[test]
fn laplacian_sample_mixed_real_to_temperature_returns_scalar() {
    // Dimensionless domain: Point{3, Real}
    let domain = Type::point3(Type::Real);
    // Dimensioned codomain: Scalar<Temperature>
    let codomain = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };

    let lap_result = eval_field_op("laplacian", domain.clone(), codomain);
    let sampled = sample_field(lap_result, domain);

    // Expected: Scalar[Temperature, si_value ≈ 0.0]
    // (linear body `x+y+z`, ∇²(linear) = 0; Temperature / dimensionless = Temperature)
    match sampled {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::TEMPERATURE,
                "laplacian_sample_mixed_real_to_temperature_returns_scalar: \
                 expected dimension Temperature ({:?}), got {:?}",
                DimensionVector::TEMPERATURE,
                dimension,
            );
            assert!(
                si_value.abs() < 1e-4,
                "laplacian_sample_mixed_real_to_temperature_returns_scalar: \
                 si_value should be ≈0.0 (∇²(x+y+z) = 0), got {}",
                si_value
            );
        }
        other => panic!(
            "laplacian_sample_mixed_real_to_temperature_returns_scalar: \
             expected Value::Scalar{{Temperature}}, got {:?}",
            other
        ),
    }
}

// ── Step 10: Edge-case Undef return paths ─────────────────────────────────────

/// divergence(Real) returns Undef when the argument is not a Field.
///
/// Mirrors gradient_non_field_returns_undef in lambda_eval_tests.rs. Exercises the
/// first early-return guard in compute_divergence (lib.rs:732–739).
#[test]
fn divergence_non_field_returns_undef() {
    let expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(Value::Real(1.0), Type::Real)],
        Type::Real,
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert!(
        matches!(&result, Value::Undef),
        "divergence of non-Field must return Undef, got {:?}",
        result
    );
}

/// curl(Field<Point2, Vec2>) returns Undef — curl requires a 3D domain.
///
/// compute_curl hardwires `Type::Point { n: 3, .. }` in the domain check
/// (lib.rs:852). A 2D vector field (n=2) fails that arm and returns Undef.
/// Mirrors the domain-dimension guard tests in gradient_tests.rs.
#[test]
fn curl_2d_vector_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");

    // Lambda: |x, y| vec2(-y, x)
    let neg_y = CompiledExpr::unop(
        UnOp::Neg,
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let body = make_function_call(
        "vec2",
        vec![neg_y, CompiledExpr::value_ref(x_id.clone(), Type::Real)],
        Type::vec2(Type::Real),
    );
    let lambda = make_value_lambda(vec![("x", x_id), ("y", y_id)], body, ValueMap::new());

    let (field, field_type) =
        make_analytical_field(Type::point2(Type::Real), Type::vec2(Type::Real), lambda);

    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let values = ValueMap::new();
    let result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "curl of 2D vector field must return Undef (curl requires 3D domain), got {:?}",
        result
    );
}

/// divergence(gradient(f)) returns Undef — gradient-sourced fields are not
/// accepted by compute_divergence's source-kind whitelist.
///
/// Build a 3D analytical field Point3<Real>->Real with λ(x,y,z) = x²+y²+z², then
/// take its gradient to produce Field<Point3, Vec3, source=Gradient>.  That
/// gradient field passes compute_divergence's domain guard (Point{3}), codomain
/// guard (Vector{3}), and dim-match guard (3==3).  The only remaining guard is
/// the source-kind whitelist (calculus.rs:151–156), which rejects Gradient and
/// returns Undef.  This isolates the whitelist as the sole Undef trigger and
/// mirrors gradient_of_gradient_field_returns_undef in gradient_tests.rs.
#[test]
fn divergence_gradient_field_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| x*x + y*y + z*z
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(
            BinOp::Add,
            // x*x
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                CompiledExpr::value_ref(x_id.clone(), Type::Real),
                Type::Real,
            ),
            // y*y
            CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                CompiledExpr::value_ref(y_id.clone(), Type::Real),
                Type::Real,
            ),
            Type::Real,
        ),
        // z*z
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            Type::Real,
        ),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) = make_analytical_field(domain_type.clone(), Type::Real, lambda);

    // gradient(field) — should succeed and produce a Gradient-sourced field
    // with domain=Point3 and codomain=Vec3(Real).
    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        gradient_result_type(domain_type.clone(), 3),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "first gradient should return a Field, got {:?}",
        grad_result
    );

    // divergence(gradient_field) — source=Gradient not in {Analytical, Composed},
    // so compute_divergence returns Undef (calculus.rs:151–156).
    // The domain (Point{3}), codomain (Vector{3}), and dim-match (3==3) guards
    // all pass; only the source-kind whitelist triggers Undef here.

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(
            grad_result,
            gradient_result_type(domain_type, 3),
        )],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "divergence(gradient(f)) must return Undef, got {:?}",
        result
    );
}

/// divergence(Field<Point3, Vec2>) returns Undef — domain dim 3 ≠ codomain dim 2.
///
/// compute_divergence validates that vec_n (codomain dimension) equals n (domain
/// dimension) before constructing the result field (lib.rs:795–801). A field
/// mapping R³ → Vec2 fails that guard and returns Undef.
#[test]
fn divergence_field_with_mismatched_dims_returns_undef() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec2(x, y)
    let body = make_function_call(
        "vec2",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
        ],
        Type::vec2(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    // n=3 domain, n=2 codomain — mismatch!
    let (field, field_type) =
        make_analytical_field(Type::point3(Type::Real), Type::vec2(Type::Real), lambda);

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        Type::Real, // result type doesn't matter — we expect Undef
    );

    let values = ValueMap::new();
    let result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&result, Value::Undef),
        "divergence of Field<Point3, Vec2> (mismatched dims) must return Undef, got {:?}",
        result
    );
}

// ── Step 11: Curl irrotational + 1D laplacian coverage ───────────────────────

/// Curl of the conservative field F(x,y,z)=[x,y,z] at (1,2,3) ≈ [0,0,0].
///
/// F = ∇φ where φ(x,y,z) = (x²+y²+z²)/2 — a gradient field is always
/// irrotational, so curl(F) ≡ 0 analytically. Numerical central differences
/// should give near-zero within 1e-3.
///
/// Note: divergence(F)=3 everywhere on the same field (see divergence_identity_vector_field).
/// These two tests together confirm both operators on the same lambda.
#[test]
fn curl_irrotational_field_near_zero() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // Lambda: |x, y, z| vec3(x, y, z)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let domain_type = Type::point3(Type::Real);

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), Type::vec3(Type::Real), lambda);

    // curl(field) → vector field
    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        curl_result_type(domain_type.clone()),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&curl_result, Value::Field { .. }),
        "curl of irrotational field should return a Field, got {:?}",
        curl_result
    );

    // sample(curl_field, Point3(1.0, 2.0, 3.0)) — expect ≈ [0, 0, 0]
    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_result_type(domain_type.clone())),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Real),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    assert_gradient_vector(
        &sample_result,
        &[0.0, 0.0, 0.0],
        1e-3,
        "curl of conservative field [x,y,z] at (1,2,3)",
    );
}

/// Laplacian of the 1D quadratic f(x) = x*x at x=3.0 ≈ 2.0.
///
/// d²/dx²(x²) = 2 at every x. Domain is Type::Real (bare scalar, not Point),
/// which exercises the `Type::Real` arm in compute_laplacian (lib.rs:933) and
/// the `Value::Real(r) if r.is_finite() => vec![*r]` coords-extraction arm in
/// compute_numerical_laplacian_at_point (lib.rs:1627). With n=1 and a single
/// lambda param, single_point_param = (1==1 && 1>1) = false, so the decomposed
/// per-axis path runs with n=1.
#[test]
fn laplacian_1d_quadratic_accuracy() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    // Lambda: |x| x*x
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let (field, field_type) = make_analytical_field(Type::Real, Type::Real, lambda);

    // laplacian(field) → scalar field
    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        scalar_field_result_type(Type::Real),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of 1D quadratic should return a Field, got {:?}",
        lap_result
    );

    // sample(lap_field, Value::Real(3.0)) — d²/dx²(x²) = 2 at every x
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(lap_result, scalar_field_result_type(Type::Real)),
            CompiledExpr::literal(Value::Real(3.0), Type::Real),
        ],
        Type::Real,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    let val = sample_result.as_f64().unwrap_or_else(|| {
        panic!(
            "laplacian sample should be numeric, got {:?}",
            sample_result
        )
    });
    assert!(
        (val - 2.0).abs() < 1e-2,
        "laplacian of x*x at x=3.0 should be ≈2.0, got {}",
        val
    );
}

// ── Step 9: Mixed-dim divergence fallback tests ───────────────────────────────

/// Divergence of Point{3, Real} → Vector{3, Scalar<Length>}: the domain is
/// dimensionless (Real), so the unified preserve-codomain strategy should
/// preserve the Vector's component type (Scalar<Length>) as the result codomain.
///
/// Under the old divergence fallback (`_ => Type::Real`), this test fails.
#[test]
fn divergence_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let component_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::vec3(component_type.clone());

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) = make_analytical_field(domain_type, codomain_type.clone(), lambda);

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Real}}→Vector{{3,Length}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type, component_type,
            "divergence of Point{{3,Real}}→Vector{{3,Length}} should preserve codomain Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

/// Divergence of Point{3, Scalar<Length>} → Vector{3, Real}: the codomain
/// component is dimensionless (Real), so the result codomain is Real.
///
/// Under the unified preserve-codomain strategy this coincides with the current
/// fallback, but this test documents the intended behavior explicitly.
#[test]
fn divergence_dim_domain_preserves_real_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    let codomain_type = Type::vec3(Type::Real);

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) = make_analytical_field(domain_type, codomain_type.clone(), lambda);

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Length}}→Vector{{3,Real}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "divergence of Point{{3,Length}}→Vector{{3,Real}} should preserve codomain Real, got {:?}",
            codomain_type
        );
    }
}

/// Divergence of Point{3, Int} → Vector{3, Scalar<Length>}: Int is treated as
/// dimensionless, so the unified preserve-codomain strategy preserves Scalar<Length>.
///
/// Under the old divergence fallback (`_ => Type::Real`), this test fails.
#[test]
fn divergence_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let component_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_type = Type::vec3(component_type.clone());

    // Lambda body unused (metadata-only test).
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) = make_analytical_field(domain_type, codomain_type.clone(), lambda);

    let div_expr = make_function_call(
        "divergence",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let div_result = eval_expr(&div_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&div_result, Value::Field { .. }),
        "divergence of Point{{3,Int}}→Vector{{3,Length}} should return a Field, got {:?}",
        div_result
    );

    if let Value::Field { codomain_type, .. } = &div_result {
        assert_eq!(
            *codomain_type, component_type,
            "divergence of Point{{3,Int}}→Vector{{3,Length}} should preserve codomain Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

// ── Step 10: Mixed-dim laplacian fallback tests ───────────────────────────────

/// Laplacian of Point{3, Real} → Scalar<Length>: domain is dimensionless (Real),
/// so the preserve-codomain strategy preserves Scalar<Length> unchanged.
///
/// This already coincides with the current `_ => codomain_type.clone()` fallback,
/// but documents the intended behavior under the unified strategy.
#[test]
fn laplacian_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Real}}→Scalar<Length> should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Scalar {
                dimension: DimensionVector::LENGTH
            },
            "laplacian of Point{{3,Real}}→Scalar<Length> should preserve Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Scalar<Length>} → Real: codomain is dimensionless (Real),
/// so the result codomain is Real.
///
/// This already coincides with the current fallback but documents intended behavior.
#[test]
fn laplacian_dim_domain_preserves_real_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    let codomain_type = Type::Real;

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Real should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "laplacian of Point{{3,Length}}→Real should preserve codomain Real, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Scalar<Length>} → Scalar{DIMENSIONLESS}: the codomain is
/// explicitly dimensionless, so the result should be downgraded to Type::Real.
///
/// The current fallback (`_ => codomain_type.clone()`) returns Scalar{DIMENSIONLESS}
/// instead of Real — this test exposes that bug.
#[test]
fn laplacian_explicit_dimensionless_scalar_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let domain_type = Type::point3(domain_quantity);
    // Explicitly-dimensionless Scalar (not Type::Real, but Scalar<DIMENSIONLESS>).
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Length}}→Scalar{{DIMENSIONLESS}} should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Real,
            "laplacian of Point{{3,Length}}→Scalar{{DIMENSIONLESS}} should downgrade codomain to Real, got {:?}",
            codomain_type
        );
    }
}

/// Laplacian of Point{3, Int} → Scalar<Length>: Int is treated as dimensionless,
/// so the preserve-codomain strategy preserves Scalar<Length>.
///
/// This already coincides with the current fallback but documents intended behavior.
#[test]
fn laplacian_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let lap_expr = make_function_call(
        "laplacian",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let lap_result = eval_expr(&lap_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&lap_result, Value::Field { .. }),
        "laplacian of Point{{3,Int}}→Scalar<Length> should return a Field, got {:?}",
        lap_result
    );

    if let Value::Field { codomain_type, .. } = &lap_result {
        assert_eq!(
            *codomain_type,
            Type::Scalar {
                dimension: DimensionVector::LENGTH
            },
            "laplacian of Point{{3,Int}}→Scalar<Length> should preserve Scalar<Length>, got {:?}",
            codomain_type
        );
    }
}

// ── Step 11: Mixed-dim gradient fallback tests ────────────────────────────────

/// Gradient of Point{3, Real} → Scalar<Length>: domain is dimensionless (Real),
/// so the preserve-codomain strategy returns Vector{3, Scalar<Length>}.
///
/// Gradient already handles this correctly via its fallback arm.
/// This test documents the behavior and serves as a regression guard.
#[test]
fn gradient_real_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point{{3,Real}}→Scalar<Length> should return a Field, got {:?}",
        grad_result
    );

    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        };
        assert_eq!(
            *codomain_type, expected,
            "gradient of Point{{3,Real}}→Scalar<Length> should have codomain Vector{{3,Scalar<Length>}}, got {:?}",
            codomain_type
        );
    }
}

/// Gradient of Point{3, Int} → Scalar<Length>: Int is treated as dimensionless,
/// so the preserve-codomain strategy returns Vector{3, Scalar<Length>}.
///
/// Gradient already handles this correctly via its fallback arm.
/// This test documents the behavior and serves as a regression guard.
#[test]
fn gradient_int_domain_preserves_dim_codomain() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Int);
    let codomain_type = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };

    // Lambda body unused (metadata-only test).
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let grad_expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let grad_result = eval_expr(&grad_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&grad_result, Value::Field { .. }),
        "gradient of Point{{3,Int}}→Scalar<Length> should return a Field, got {:?}",
        grad_result
    );

    if let Value::Field { codomain_type, .. } = &grad_result {
        let expected = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        };
        assert_eq!(
            *codomain_type, expected,
            "gradient of Point{{3,Int}}→Scalar<Length> should have codomain Vector{{3,Scalar<Length>}}, got {:?}",
            codomain_type
        );
    }
}

// ── Curl dimension propagation tests ────────────────────────────────────────

/// Curl of a Point{3,Length} → Vector{3,Velocity} field has codomain
/// Vector{3, Scalar{dim = Velocity/Length = 1/Time}}.
///
/// Verifies that compute_curl correctly derives the result codomain
/// dimension by dividing the input codomain component dimension by domain_dim.
#[test]
fn curl_dimensional_correctness() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // VELOCITY = LENGTH / TIME
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);

    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_quantity = Type::Scalar {
        dimension: velocity_dim,
    };

    let domain_type = Type::point3(domain_quantity.clone());
    let codomain_type = Type::vec3(codomain_quantity.clone());

    // Lambda: |x, y, z| vec3(x, y, z) — simple identity for metadata test.
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    // curl(field) → vector field with codomain Vector{3, Scalar{Velocity/Length = 1/Time}}
    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&curl_result, Value::Field { .. }),
        "curl of Point{{3,Length}}→Vector{{3,Velocity}} should return a Field, got {:?}",
        curl_result
    );

    // Verify codomain dimension: should be Vector{3, Scalar{Velocity/Length = 1/Time}}
    if let Value::Field { codomain_type, .. } = &curl_result {
        let expected_dim = velocity_dim.div(&DimensionVector::LENGTH);
        let expected = Type::vec3(Type::Scalar {
            dimension: expected_dim,
        });
        assert_eq!(
            *codomain_type, expected,
            "curl codomain should be Vector{{3, Scalar{{1/Time}}}}, got {:?}",
            codomain_type
        );
    }
}

/// Curl of a dimensionless Point{3,Real} → Vector{3,Real} field still
/// returns Vector{3,Real} codomain (regression guard).
#[test]
fn curl_dimensionless_still_vec3_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    if let Value::Field { codomain_type, .. } = &curl_result {
        let expected = Type::vec3(Type::Real);
        assert_eq!(
            *codomain_type, expected,
            "curl of dimensionless field should have Vector{{3,Real}} codomain, got {:?}",
            codomain_type
        );
    } else {
        panic!(
            "curl of dimensionless field should return a Field, got {:?}",
            curl_result
        );
    }
}

/// Sample(curl(dimensioned_field), point) returns Vector of Scalar components
/// with the correct derived dimension.
#[test]
fn curl_sample_dimensional_correctness_returns_scalar() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    // VELOCITY = LENGTH / TIME
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let domain_quantity = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let codomain_quantity = Type::Scalar {
        dimension: velocity_dim,
    };

    let domain_type = Type::point3(domain_quantity.clone());
    let codomain_type = Type::vec3(codomain_quantity.clone());

    // Lambda: |x, y, z| vec3(z, x, y)  — rotation-like, produces non-zero curl.
    // curl of (z, x, y) = (∂y/∂y - ∂x/∂z, ∂z/∂z - ∂y/∂x, ∂x/∂x - ∂z/∂y) = (1, 1, 1)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    assert!(
        matches!(&curl_result, Value::Field { .. }),
        "curl should return a Field, got {:?}",
        curl_result
    );

    // Sample at (1m, 2m, 3m)
    let point = Value::Point(vec![
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);

    let one_over_time = velocity_dim.div(&DimensionVector::LENGTH);
    let curl_codomain = Type::vec3(Type::Scalar {
        dimension: one_over_time,
    });
    let curl_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(curl_codomain),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        Type::vec3(Type::Scalar {
            dimension: one_over_time,
        }),
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // curl of (z, x, y) = (1, 1, 1) — all components should be ≈1.0
    match &sample_result {
        Value::Vector(comps) if comps.len() == 3 => {
            for (i, comp) in comps.iter().enumerate() {
                match comp {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert_eq!(
                            *dimension, one_over_time,
                            "curl sample component {i} dimension should be 1/Time ({:?}), got {:?}",
                            one_over_time, dimension,
                        );
                        assert!(
                            (si_value - 1.0).abs() < 1e-4,
                            "curl component {i} should be ≈1.0, got {}",
                            si_value
                        );
                    }
                    Value::Real(_) => panic!(
                        "curl sample component {i}: expected Value::Scalar but got Value::Real"
                    ),
                    other => panic!(
                        "curl sample component {i}: expected Value::Scalar, got {:?}",
                        other
                    ),
                }
            }
        }
        other => panic!("curl sample should return Vector(3), got {:?}", other),
    }
}

/// Sample(curl(dimensionless_field), point) returns Vector of Real components (regression guard).
#[test]
fn curl_sample_dimensionless_returns_real() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let z_id = ValueCellId::new("$lambda0.S", "z");

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::vec3(Type::Real);

    // Lambda: |x, y, z| vec3(z, x, y)
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
        ],
        codomain_type.clone(),
    );
    let lambda = make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    );

    let (field, field_type) =
        make_analytical_field(domain_type.clone(), codomain_type.clone(), lambda);

    let curl_expr = make_function_call(
        "curl",
        vec![CompiledExpr::literal(field, field_type)],
        codomain_type.clone(),
    );

    let values = ValueMap::new();
    let curl_result = eval_expr(&curl_expr, &EvalContext::simple(&values));

    let point = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);

    let curl_field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(curl_result, curl_field_type),
            CompiledExpr::literal(point, domain_type),
        ],
        codomain_type,
    );

    let sample_result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    match &sample_result {
        Value::Vector(comps) if comps.len() == 3 => {
            for (i, comp) in comps.iter().enumerate() {
                assert!(
                    matches!(comp, Value::Real(_)),
                    "curl sample of dimensionless field component {i} should be Value::Real, got {:?}",
                    comp
                );
            }
        }
        other => panic!("curl sample should return Vector(3), got {:?}", other),
    }
}

// ── Step 12: Expanded dimensional-correctness coverage (Task 1238) ────────────

/// Laplacian of a bare-scalar (1D) domain: `Type::Scalar{LENGTH}` → `Scalar<Temperature>`
/// has codomain dimension `Temperature / Length²`.
///
/// Exercises the `_ if scalar_dimension(domain_type).is_some()` first arm of
/// `compute_laplacian`'s domain match — the path where domain is a bare scalar
/// rather than a `Point{n}`.
#[test]
fn laplacian_dimensional_correctness_1d_scalar_domain() {
    run_dim_metadata_test(
        "laplacian",
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        },
        FieldSourceKind::Analytical,
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2)),
        },
        "laplacian_dimensional_correctness_1d_scalar_domain",
    );
}

/// Confirms dimension-agnostic behavior when domain is a 2D `Point{2}` —
/// `compute_laplacian`'s Point arm handles any `n>=1` via the same
/// `dim_quotient_type` derivation.
#[test]
fn laplacian_dimensional_correctness_2d_point_domain() {
    run_dim_metadata_test(
        "laplacian",
        Type::point2(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        },
        FieldSourceKind::Analytical,
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2)),
        },
        "laplacian_dimensional_correctness_2d_point_domain",
    );
}

/// Laplacian of `Point{3,Length}` → `Int` field preserves `Int` codomain.
///
/// `Int` is treated as dimensionless (`scalar_dimension(Int)=Some(DIMENSIONLESS)`),
/// so `dim_quotient_type`'s guard fails and the fallback path returns the codomain
/// unchanged (`Type::Int`). Exercises the Int-codomain fall-through branch.
#[test]
fn laplacian_dimensional_correctness_int_codomain() {
    run_dim_metadata_test(
        "laplacian",
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::Int,
        FieldSourceKind::Analytical,
        Type::Int,
        "laplacian_dimensional_correctness_int_codomain",
    );
}

/// Int-domain input-value coverage: verifies the `Int -> Int` case also returns `Type::Int`.
///
/// Not a distinct `dim_quotient_type` branch — the outer `cd != DIMENSIONLESS` guard
/// short-circuits on `cd == DIMENSIONLESS`, so the domain dimension is never consulted
/// once the codomain is `Int`. Retained as a smoke test that `Int`-quantity `Point`
/// domains are accepted by `compute_laplacian`.
#[test]
fn laplacian_dimensional_correctness_int_domain_int_codomain() {
    run_dim_metadata_test(
        "laplacian",
        Type::point3(Type::Int),
        Type::Int,
        FieldSourceKind::Analytical,
        Type::Int,
        "laplacian_dimensional_correctness_int_domain_int_codomain",
    );
}

/// This is the only laplacian test exercising the `result_dim == DIMENSIONLESS => Type::Real`
/// inner arm of `dim_quotient_type`. Codomain `Length²` divided by `Length²`
/// (domain-dim squared) collapses to `DIMENSIONLESS`, so the inner arm returns `Type::Real`.
#[test]
fn laplacian_dimensional_correctness_result_dimensionless_returns_real() {
    run_dim_metadata_test(
        "laplacian",
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::Scalar {
            dimension: DimensionVector::LENGTH.pow(2),
        },
        FieldSourceKind::Analytical,
        Type::Real,
        "laplacian_dimensional_correctness_result_dimensionless_returns_real",
    );
}

/// Exercises the `FieldSourceKind::Analytical | FieldSourceKind::Composed` source
/// whitelist in `compute_laplacian`. No other laplacian test uses a `Composed` source.
/// Mirrors `gradient_composed_field_returns_field` in `gradient_tests.rs`.
#[test]
fn laplacian_dimensional_correctness_composed_source() {
    run_dim_metadata_test(
        "laplacian",
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE,
        },
        FieldSourceKind::Composed,
        Type::Scalar {
            dimension: DimensionVector::TEMPERATURE.div(&DimensionVector::LENGTH.pow(2)),
        },
        "laplacian_dimensional_correctness_composed_source",
    );
}

/// Confirms dimension-agnostic behavior for a 1D vector field (`Point{1}` → `Vector{1}`) —
/// `compute_divergence`'s `Point{n, quantity}` arm accepts any `n>=1`.
///
/// The `point1` and `vec1` helpers do not exist in `ty.rs`, so types are
/// constructed via struct literals.
#[test]
fn divergence_dimensional_correctness_1d() {
    let velocity_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    run_dim_metadata_test(
        "divergence",
        Type::Point {
            n: 1,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        },
        Type::Vector {
            n: 1,
            quantity: Box::new(Type::Scalar {
                dimension: velocity_dim,
            }),
        },
        FieldSourceKind::Analytical,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
                .div(&DimensionVector::TIME)
                .div(&DimensionVector::LENGTH),
        },
        "divergence_dimensional_correctness_1d",
    );
}

/// Confirms dimension-agnostic behavior for a 2D vector field (`Point{2}` → `Vector{2}`) —
/// `compute_divergence`'s `Point{n, quantity}` arm accepts any `n>=1`.
#[test]
fn divergence_dimensional_correctness_2d() {
    run_dim_metadata_test(
        "divergence",
        Type::point2(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::vec2(Type::Scalar {
            dimension: DimensionVector::LENGTH.div(&DimensionVector::TIME),
        }),
        FieldSourceKind::Analytical,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
                .div(&DimensionVector::TIME)
                .div(&DimensionVector::LENGTH),
        },
        "divergence_dimensional_correctness_2d",
    );
}

/// This is the only divergence test exercising the `result_dim == DIMENSIONLESS => Type::Real`
/// inner arm of `dim_quotient_type`. Codomain component `Length` divided by domain
/// `Length` collapses to `DIMENSIONLESS`, so the inner arm returns `Type::Real`.
#[test]
fn divergence_dimensional_correctness_result_dimensionless_returns_real() {
    run_dim_metadata_test(
        "divergence",
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        FieldSourceKind::Analytical,
        Type::Real,
        "divergence_dimensional_correctness_result_dimensionless_returns_real",
    );
}

/// Exercises the `FieldSourceKind::Analytical | FieldSourceKind::Composed` source
/// whitelist in `compute_divergence`. No other divergence test uses a `Composed` source.
/// Mirrors `gradient_composed_field_returns_field` in `gradient_tests.rs`.
#[test]
fn divergence_dimensional_correctness_composed_source() {
    run_dim_metadata_test(
        "divergence",
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH.div(&DimensionVector::TIME),
        }),
        FieldSourceKind::Composed,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
                .div(&DimensionVector::TIME)
                .div(&DimensionVector::LENGTH),
        },
        "divergence_dimensional_correctness_composed_source",
    );
}

// ── Step 13: Scalar{DIMENSIONLESS} codomain downgrade coverage (Task 1291) ───────

/// Exercises the explicit `Scalar{DIMENSIONLESS} → Real` fallback arm in
/// `compute_divergence` (calculus.rs:229-231).
///
/// For a `Point{3,Scalar<Length>} → Vec3(Scalar{DIMENSIONLESS})` field, the
/// `divergence_fallback` is set to `Type::Real` because the codomain component type
/// is `Scalar{DIMENSIONLESS}`. `dim_quotient_type` then returns the fallback because
/// `cd == DIMENSIONLESS`, so `result_codomain = Type::Real`.
///
/// This arm is distinct from:
/// - The `_` wildcard arm (hit when codomain is non-Scalar-DIMENSIONLESS)
/// - The `dim_quotient_type` dimensional-quotient arm (only reached when both cd and
///   dd are non-DIMENSIONLESS)
#[test]
fn divergence_scalar_dimensionless_codomain_downgrade() {
    let domain = Type::point3(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    let codomain = Type::vec3(Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    });

    let field_result = eval_field_op("divergence", domain.clone(), codomain);

    let Value::Field {
        codomain_type: ref actual_codomain,
        ..
    } = field_result
    else {
        panic!(
            "divergence_scalar_dimensionless_codomain_downgrade: expected Field, got {:?}",
            field_result
        );
    };
    assert_eq!(
        *actual_codomain,
        Type::Real,
        "divergence of Point{{3,Length}}→Vec3(DIMENSIONLESS) should have codomain Type::Real, \
         got {:?}",
        actual_codomain
    );

    let sampled = sample_field(field_result, domain);
    assert!(
        matches!(&sampled, Value::Real(_)),
        "divergence_scalar_dimensionless_codomain_downgrade: sampled value should be \
         Value::Real, got {:?}",
        sampled
    );
}

/// Exercises the explicit `Scalar{DIMENSIONLESS} → Real` fallback arm in
/// `compute_gradient` (calculus.rs:120-122).
///
/// For a `Point{3,Real} → Scalar{DIMENSIONLESS}` field, `gradient_fallback` is set to
/// `Type::Real` because the codomain is `Scalar{DIMENSIONLESS}`. `dim_quotient_type`
/// returns the fallback because `cd == DIMENSIONLESS`, so `gradient_quantity = Type::Real`.
/// With `n = 3`, `result_codomain = Vector{3, Real}`.
///
/// This arm is distinct from:
/// - The `_` wildcard arm (hit when codomain is non-Scalar-DIMENSIONLESS)
/// - The `dim_quotient_type` dimensional-quotient arm (only reached when both cd and
///   dd are non-DIMENSIONLESS)
#[test]
fn gradient_scalar_dimensionless_codomain_downgrade() {
    let domain = Type::point3(Type::Real);
    let codomain = Type::Scalar {
        dimension: DimensionVector::DIMENSIONLESS,
    };

    let field_result = eval_field_op("gradient", domain.clone(), codomain);

    let Value::Field {
        codomain_type: ref actual_codomain,
        ..
    } = field_result
    else {
        panic!(
            "gradient_scalar_dimensionless_codomain_downgrade: expected Field, got {:?}",
            field_result
        );
    };
    assert_eq!(
        *actual_codomain,
        Type::vec3(Type::Real),
        "gradient of Point{{3,Real}}→Scalar{{DIMENSIONLESS}} should have codomain Vec3(Real), \
         got {:?}",
        actual_codomain
    );

    let sampled = sample_field(field_result, domain);
    match &sampled {
        Value::Vector(comps) => {
            assert_eq!(
                comps.len(),
                3,
                "gradient result should be a 3-vector, got {} components",
                comps.len()
            );
            for (i, comp) in comps.iter().enumerate() {
                assert!(
                    matches!(comp, Value::Real(_)),
                    "gradient_scalar_dimensionless_codomain_downgrade: component {i} should \
                     be Value::Real, got {:?}",
                    comp
                );
            }
        }
        other => panic!(
            "gradient_scalar_dimensionless_codomain_downgrade: sampled value should be \
             Value::Vector, got {:?}",
            other
        ),
    }
}

/// `SamplePoint::into_value_and_type()` produces the correct `(Value, Type)` pair
/// for each variant — Point3 → (Value::Point, Type::point3(Real)),
/// Vector3 → (Value::Vector, Type::vec3(Real)), Vector2 → (Value::Vector, Type::vec2(Real)).
#[test]
fn sample_point_enum_correctness() {
    // Point3
    let (val, ty) = SamplePoint::Point3([1.0, 2.0, 3.0]).into_value_and_type();
    assert!(
        matches!(&val, Value::Point(items) if items.len() == 3),
        "Point3 should produce Value::Point with 3 items, got {:?}",
        val
    );
    assert_eq!(
        ty,
        Type::point3(Type::Real),
        "Point3 should produce Type::point3(Real)"
    );
    if let Value::Point(items) = &val {
        assert_eq!(items[0], Value::Real(1.0));
        assert_eq!(items[1], Value::Real(2.0));
        assert_eq!(items[2], Value::Real(3.0));
    }

    // Vector3
    let (val, ty) = SamplePoint::Vector3([1.0, 2.0, 3.0]).into_value_and_type();
    assert!(
        matches!(&val, Value::Vector(items) if items.len() == 3),
        "Vector3 should produce Value::Vector with 3 items, got {:?}",
        val
    );
    assert_eq!(
        ty,
        Type::vec3(Type::Real),
        "Vector3 should produce Type::vec3(Real)"
    );
    if let Value::Vector(items) = &val {
        assert_eq!(items[0], Value::Real(1.0));
        assert_eq!(items[1], Value::Real(2.0));
        assert_eq!(items[2], Value::Real(3.0));
    }

    // Vector2
    let (val, ty) = SamplePoint::Vector2([1.0, 2.0]).into_value_and_type();
    assert!(
        matches!(&val, Value::Vector(items) if items.len() == 2),
        "Vector2 should produce Value::Vector with 2 items, got {:?}",
        val
    );
    assert_eq!(
        ty,
        Type::vec2(Type::Real),
        "Vector2 should produce Type::vec2(Real)"
    );
    if let Value::Vector(items) = &val {
        assert_eq!(items[0], Value::Real(1.0));
        assert_eq!(items[1], Value::Real(2.0));
    }
}

// ── Arc-sharing invariant tests (Task 1629) ──────────────────────────────────
//
// These four tests pin the O(1)-clone performance invariant for the differential
// operators.  Each operator stores the source field in its result's lambda slot
// via `Arc::new(field_val.clone())`.  Because `lambda: Arc<Value>` in
// `Value::Field`, `field_val.clone()` uses `Arc::clone` for the inner lambda —
// so the cloned source's lambda Arc is `ptr_eq` with the original.
//
// Tests destructure two levels:
//   result.lambda        → the stored clone of the source field (Arc<Value::Field>)
//   result.lambda.lambda → the source field's lambda (should be ptr_eq with original)

/// Build a non-trivial `Value::Lambda` with three `Real` params `(x, y, z)` and
/// body `x`.  Used as the lambda payload in gradient/laplacian arc-sharing tests.
fn make_trivial_3d_scalar_lambda() -> Value {
    let x_id = ValueCellId::new("$arc_share_test.S", "x");
    let y_id = ValueCellId::new("$arc_share_test.S", "y");
    let z_id = ValueCellId::new("$arc_share_test.S", "z");
    let body = CompiledExpr::value_ref(x_id.clone(), Type::Real);
    make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    )
}

/// Build a non-trivial `Value::Lambda` with three `Real` params `(x, y, z)` and
/// body `vec3(x, y, z)`.  Used in divergence/curl arc-sharing tests.
fn make_trivial_3d_vector_lambda() -> Value {
    let x_id = ValueCellId::new("$arc_share_test.S", "x");
    let y_id = ValueCellId::new("$arc_share_test.S", "y");
    let z_id = ValueCellId::new("$arc_share_test.S", "z");
    let body = make_function_call(
        "vec3",
        vec![
            CompiledExpr::value_ref(x_id.clone(), Type::Real),
            CompiledExpr::value_ref(y_id.clone(), Type::Real),
            CompiledExpr::value_ref(z_id.clone(), Type::Real),
        ],
        Type::vec3(Type::Real),
    );
    make_value_lambda(
        vec![("x", x_id), ("y", y_id), ("z", z_id)],
        body,
        ValueMap::new(),
    )
}

/// Evaluate a unary field operator (`op`) by name and return the result `Value`.
fn eval_unary_field_op(op: &str, source: Value, source_type: Type) -> Value {
    let op_expr = make_function_call(
        op,
        vec![CompiledExpr::literal(source, source_type)],
        Type::Real, // placeholder result_type; not inspected by the evaluator
    );
    eval_expr(&op_expr, &EvalContext::simple(&ValueMap::new()))
}

/// Encapsulates the arc-sharing O(1)-clone invariant used by all four unary
/// field operators (gradient / divergence / curl / laplacian).
///
/// Constructs a `FieldSourceKind::Analytical` source field with domain
/// `Type::point3(Type::Real)`, the given `codomain`, and a freshly wrapped
/// `Arc<Value>` around `lambda`.  Runs `op_name` via `eval_unary_field_op`,
/// then asserts that the result's nested source-field lambda `Arc::ptr_eq`s
/// with the original — proving no deep clone of the compiled expression tree
/// occurred.
///
/// Adding coverage for a fifth unary field operator is a one-liner:
/// `assert_unary_op_shares_source_lambda("new_op", codomain, make_lambda())`.
fn assert_unary_op_shares_source_lambda(op_name: &str, codomain: Type, lambda: Value) {
    let source_lambda: Arc<Value> = Arc::new(lambda);
    let domain = Type::point3(Type::Real);
    let source = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source: FieldSourceKind::Analytical,
        lambda: source_lambda.clone(),
    };
    let source_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };

    let result = eval_unary_field_op(op_name, source, source_type);

    let outer = match result {
        Value::Field { lambda, .. } => lambda,
        other => panic!("{op_name} should return Field, got {:?}", other),
    };

    let inner_lambda = match outer.as_ref() {
        Value::Field { lambda, .. } => lambda.clone(),
        other => panic!("{op_name} result lambda should be Field, got {:?}", other),
    };

    assert!(
        Arc::ptr_eq(&source_lambda, &inner_lambda),
        "{op_name}: result's nested source-field lambda should Arc::ptr_eq with original (no deep clone)"
    );
}

/// `compute_gradient` stores the source field's lambda via `Arc::clone` —
/// no deep copy of the compiled expression tree occurs.
#[test]
fn gradient_result_arc_shares_source_lambda() {
    assert_unary_op_shares_source_lambda("gradient", Type::Real, make_trivial_3d_scalar_lambda());
}

/// `compute_divergence` stores the source field's lambda via `Arc::clone` —
/// no deep copy of the compiled expression tree occurs.
#[test]
fn divergence_result_arc_shares_source_lambda() {
    assert_unary_op_shares_source_lambda(
        "divergence",
        Type::vec3(Type::Real),
        make_trivial_3d_vector_lambda(),
    );
}

/// `compute_curl` stores the source field's lambda via `Arc::clone` —
/// no deep copy of the compiled expression tree occurs.
#[test]
fn curl_result_arc_shares_source_lambda() {
    assert_unary_op_shares_source_lambda(
        "curl",
        Type::vec3(Type::Real),
        make_trivial_3d_vector_lambda(),
    );
}

/// `compute_laplacian` stores the source field's lambda via `Arc::clone` —
/// no deep copy of the compiled expression tree occurs.
#[test]
fn laplacian_result_arc_shares_source_lambda() {
    assert_unary_op_shares_source_lambda("laplacian", Type::Real, make_trivial_3d_scalar_lambda());
}

/// `sample` of an `Imported` field whose lambda slot holds a `Value::SampledField`
/// dispatches to `sampled::sample_at_point` and returns the interpolated value,
/// NOT `Value::Undef`.
///
/// Step-5 RED guard (task 3576 — OpenVDB ingest end-to-end):
/// Prior to the fix, `(Value::SampledField(_), FieldSourceKind::Imported)` falls
/// through to the `_ => Value::Undef` arm in the sample dispatch (lib.rs:319-326).
/// After step-6 adds the new arm, both `Sampled` and `Imported` fields backed by a
/// `SampledField` lambda must call `sampled::sample_at_point`.
///
/// The fixture is a 2×2×2 Regular3D grid with data = [1,2,3,4,5,6,7,8] at the
/// corners; the probe point (0.5, 0.5, 0.5) is strictly in-bounds.  The assertion
/// cross-validates the dispatch result against a direct `sampled::sample_at_point`
/// call on the same `SampledField` (exact equality — same math path), so the numeric
/// expectation is derived, not guessed.
///
/// cfg-independent: no FFI — the `SampledField` is constructed entirely in Rust.
#[test]
fn sample_imported_field_with_sampled_field_lambda_dispatches_to_interpolation() {
    use std::sync::atomic::AtomicBool;
    use reify_ir::{InterpolationKind, SampledField, SampledGridKind};

    // 2×2×2 Regular3D SampledField: axes [0.0, 1.0] on each dimension,
    // row-major data[i0*4 + i1*2 + i2] with known corner values.
    let sf = SampledField {
        name: "test_imported".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: InterpolationKind::Linear,
        data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
        oob_emitted: AtomicBool::new(false),
    };

    let domain_type = Type::point3(Type::Real);
    let codomain_type = Type::Real;

    // Probe point strictly in-bounds — trilinear interpolation at (0.5,0.5,0.5)
    // yields the average of all 8 corners = 4.5.
    let probe = Value::Point(vec![Value::Real(0.5), Value::Real(0.5), Value::Real(0.5)]);

    // Reference: direct sample_at_point call BEFORE consuming sf.
    let expected = {
        let values = ValueMap::new();
        reify_expr::sampled::sample_at_point(&sf, &probe, &codomain_type, &EvalContext::simple(&values))
    };

    // Verify the fixture is not itself broken (probe must be in-bounds).
    assert!(
        !matches!(expected, Value::Undef),
        "sample_at_point reference returned Undef — probe is out of bounds or fixture is broken"
    );

    // Construct Value::Field with source = Imported and lambda = SampledField.
    let field = Value::Field {
        domain_type: domain_type.clone(),
        codomain_type: codomain_type.clone(),
        source: FieldSourceKind::Imported,
        lambda: Arc::new(Value::SampledField(sf)),
    };
    let field_type = Type::Field {
        domain: Box::new(domain_type.clone()),
        codomain: Box::new(codomain_type.clone()),
    };

    // Evaluate sample(field, probe) via eval_expr.
    let sample_expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(field, field_type),
            CompiledExpr::literal(probe, domain_type),
        ],
        codomain_type,
    );

    let values = ValueMap::new();
    let result = eval_expr(&sample_expr, &EvalContext::simple(&values));

    // Assert dispatch result equals the direct reference (exact identity — same code path).
    assert_eq!(
        result,
        expected,
        "sample(Imported field with SampledField lambda, probe) should equal \
         sampled::sample_at_point directly; \
         got {:?} (expected {:?})",
        result,
        expected
    );
}

/// Meta-test: asserts that every `#[ignore = "..."]` attribute in this file
/// complies with the Task 1622 convention — reason strings must be
/// self-contained inline summaries beginning with `"known bug:"`.  The two
/// placeholder tests (`divergence_sample_mixed_length_to_real_placeholder` and
/// `laplacian_sample_mixed_length_to_real_placeholder`) previously had trailing
/// breadcrumbs pointing at a transient plan document step, but plan files go
/// stale.  Bare `#[ignore]` attributes (no reason string) are also forbidden.
/// (Task 1622: introduced convention and first two guards.
///  Task 1641: added bare-ignore guard and extracted `check_ignore_reasons`.)
///
/// Three guards are enforced (see `check_ignore_reasons` for implementation):
///
/// 1. **Bare-ignore rejection** — a `#[ignore]` attribute without a reason
///    string is rejected outright.
/// 2. **Positive invariant** — every reason string must begin with
///    `"known bug:"`.  This rejects wholly-replaced prefixes but does NOT
///    catch stale wordings appended inside an otherwise-compliant prefix
///    (e.g. `"known bug: see plan.md step-3"` would pass this guard and would
///    only trip guard 3 if it happened to contain the specific sentinel).
/// 3. **Belt-and-suspenders negative sentinel** — the specific historical
///    stale-pointer substring (assembled at runtime to avoid self-triggering)
///    is also checked whole-file as belt-and-suspenders.
///
/// Doc-comment lines (`///`, `//!`) are skipped so prose mentions of
/// `#[ignore]` — e.g. `` "`#[ignore]` is load-bearing" `` — do not generate
/// false positives.
///
/// All scanner constants are assembled at runtime via `.concat()` so this file
/// does not contain the guarded sequences as adjacent characters and cannot
/// accidentally self-trigger.
///
/// Note: guard 3 (stale-plan-pointer check) is also enforced workspace-wide by
/// the integration test `no_stale_plan_pointers_in_workspace_ignore_reasons` in
/// `crates/reify-test-support/tests/ignore_reason_hygiene.rs`, which walks all
/// `**/tests/**/*.rs` files via
/// `reify_test_support::ignore_hygiene::collect_workspace_stale_pointers`.
/// This narrow test is retained because it additionally enforces guards 1
/// (bare-ignore rejection) and 2 (known-bug prefix invariant) for this file
/// specifically — the workspace-wide test intentionally omits those guards
/// because existing `#[ignore]` attributes in `reify-eval` do not yet follow
/// the `"known bug:"` convention.
#[test]
fn ignore_reason_strings_have_no_stale_plan_pointers() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/field_calculus_tests.rs");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {path} for meta-inspection: {e}"));
    reify_test_support::ignore_hygiene::check_ignore_reasons(&source).unwrap_or_else(|msg| {
        panic!(
            "{msg}\nAffected tests in this file: \
             divergence_sample_mixed_length_to_real_placeholder, \
             laplacian_sample_mixed_length_to_real_placeholder."
        )
    });
}
