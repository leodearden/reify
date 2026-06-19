//! `sample()` dispatch for the FDM as-printed material field (task δ, step-5/6).
//!
//! Builds a `Value::Field { source: AsPrintedZones, lambda: List[...] }` by
//! hand — the lambda-slot storage contract from `reify-ir::value` — with three
//! DISTINGUISHABLE precomputed "material" values (sentinels standing in for the
//! AnisotropicMaterial values the δ ComputeNode builds; the e2e test exercises
//! real materials). Sampling at a near-side-face point must return the wall
//! material; sampling at a deep-interior point must return the infill material.
//!
//! RED until `sample_field_at` grows an `(Value::List, AsPrintedZones)` arm:
//! the new source kind otherwise falls through to the `_ => Value::Undef` arm.

use std::sync::Arc;

use reify_core::{ContentHash, DimensionVector, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap};

/// A Length-dimensioned coordinate scalar (SI metres).
fn length(m: f64) -> Value {
    Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    }
}

/// A Point3<Length> from SI-metre coordinates.
fn point3(p: [f64; 3]) -> Value {
    Value::Point(vec![length(p[0]), length(p[1]), length(p[2])])
}

/// Build a `sample(field, at)` FunctionCall expression.
fn sample_call(field: Value, field_type: Type, at: Value) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "sample".to_string(),
                qualified_name: "std::sample".to_string(),
            },
            args: vec![
                CompiledExpr::literal(field, field_type),
                CompiledExpr::literal(at, Type::point3(Type::length())),
            ],
        },
        result_type: Type::String,
        content_hash: ContentHash::of(b"sample"),
    }
}

/// The AsPrintedZones lambda-slot payload for a 40×40×10 mm box, Z build axis,
/// stdlib FDMProcess defaults + 0.4 mm line width.
fn as_printed_field() -> (Value, Type) {
    // params: [walls, top_bottom_layers, layer_height, line_width, bx, by, bz]
    let params = Value::List(vec![
        Value::Real(3.0),    // walls
        Value::Real(4.0),    // top_bottom_layers
        Value::Real(0.0002), // layer_height (m)
        Value::Real(0.0004), // line_width (m)
        Value::Real(0.0),    // build_direction x
        Value::Real(0.0),    // build_direction y
        Value::Real(1.0),    // build_direction z
    ]);
    let lambda = Value::List(vec![
        point3([0.0, 0.0, 0.0]),         // aabb_min
        point3([0.040, 0.040, 0.010]),   // aabb_max
        params,                          // FDMProcess-derived zone params
        Value::Real(std::f64::consts::FRAC_1_SQRT_2), // cos_threshold (45°)
        Value::String("WALL".to_string()),   // mat_wall
        Value::String("SKIN".to_string()),   // mat_skin
        Value::String("INFILL".to_string()), // mat_infill
    ]);
    let field = Value::Field {
        domain_type: Type::point3(Type::length()),
        // The real codomain is AnisotropicMaterial; sentinels stand in here so
        // the test isolates the zone→material selection dispatch.
        codomain_type: Type::String,
        source: FieldSourceKind::AsPrintedZones,
        lambda: Arc::new(lambda),
    };
    let field_type = Type::Field {
        domain: Box::new(Type::point3(Type::length())),
        codomain: Box::new(Type::String),
    };
    (field, field_type)
}

#[test]
fn sample_as_printed_selects_wall_at_side_and_infill_in_interior() {
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);

    // 0.3 mm from the -X side face (≤ 1.2 mm wall band) → Wall.
    let (field, field_type) = as_printed_field();
    let wall = eval_expr(
        &sample_call(field, field_type, point3([0.0003, 0.020, 0.005])),
        &ctx,
    );
    assert_eq!(
        wall,
        Value::String("WALL".to_string()),
        "near-side-face point must sample the wall material, got {:?}",
        wall
    );

    // Box centre — 20 mm from sides, 5 mm from top/bottom → Infill.
    let (field, field_type) = as_printed_field();
    let infill = eval_expr(
        &sample_call(field, field_type, point3([0.020, 0.020, 0.005])),
        &ctx,
    );
    assert_eq!(
        infill,
        Value::String("INFILL".to_string()),
        "deep-interior point must sample the infill material, got {:?}",
        infill
    );
}
