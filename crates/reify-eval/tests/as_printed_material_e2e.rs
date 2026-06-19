// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end δ user-observable signal (task δ / 3786, steps 11–12): the
//! trampoline → `sample()` round-trip on the as-printed material field.
//!
//! This test wires the two halves of δ together at the value layer. It runs the
//! real `fdm::as_printed_material_r_fast` trampoline to produce a
//! `Field<Point3<Length>, AnisotropicMaterial>`, then samples THAT field through
//! the public `sample(field, at)` evaluator at a wall point and a deep-interior
//! point. It asserts the field is NON-CONSTANT (wall in-plane modulus ≫ infill —
//! distinct `AnisotropicMaterial` laws per zone) and that the build (Z / axial)
//! axis is the weakest in both (PRD C4 invariant).
//!
//! It adds the assertion the sibling tests lack:
//!   * `as_printed_trampoline.rs` reads the AsPrintedZones lambda-slot list items
//!     DIRECTLY (`items[4]` / `items[6]`) — it never exercises `sample()`.
//!   * `reify-expr/tests/as_printed_field_sample.rs` samples a HAND-BUILT field
//!     carrying STRING sentinels — never a real trampoline-produced material.
//!
//! Only here does a real trampoline field round-trip through `sample_field_at`'s
//! `AsPrintedZones` dispatch (step-6) on top of the real producer (step-8).
//!
//! The full `reify eval` CLI e2e on a realized body is owned by FDM ε (#3787);
//! δ's user-observable signal is the value-layer field-production + `sample()`
//! round-trip exercised here.
//!
//! RED until the trampoline (step-8) and the `sample_field_at` AsPrintedZones arm
//! (step-6) are both wired (they are, from prior steps): this integration asserts
//! they compose into a non-constant, build-Z-weakest sampled material field.

use std::sync::Arc;

use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type};
use reify_eval::compute_targets::as_printed_material::as_printed_material_r_fast_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle, RealizedContent};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    CompiledExpr, CompiledExprKind, FieldSourceKind, Mesh, PersistentMap, ResolvedFunction,
    StructureInstanceData, StructureTypeId, Value, ValueMap,
};

/// Registry-free placeholder type id for Rust-constructed StructureInstances
/// (mirrors `reify_eval::dynamics_ops::REGISTRY_FREE_TYPE_ID`).
const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX);

// 40×40×10 mm box (SI metres); Z is the build axis.
const BOX_MIN: [f64; 3] = [0.0, 0.0, 0.0];
const BOX_MAX: [f64; 3] = [0.040, 0.040, 0.010];

// ── input fixtures (built directly, as `as_printed_trampoline.rs` does) ──────

fn structure(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
    let fields: PersistentMap<String, Value> =
        fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

fn scalar(si: f64, dim: DimensionVector) -> Value {
    Value::Scalar {
        si_value: si,
        dimension: dim,
    }
}

fn length(m: f64) -> Value {
    scalar(m, DimensionVector::LENGTH)
}

/// A `Point3<Length>` from SI-metre coordinates — the query point fed to `sample`.
fn point3(p: [f64; 3]) -> Value {
    Value::Point(vec![length(p[0]), length(p[1]), length(p[2])])
}

/// `vec3(x,y,z)` as a `Vector3<Length>` — the FDMProcess `build_direction`.
fn vec3_length(v: [f64; 3]) -> Value {
    Value::Vector(vec![length(v[0]), length(v[1]), length(v[2])])
}

/// A box surface mesh covering `[BOX_MIN, BOX_MAX]` — only the 8 corner vertices
/// matter (the trampoline derives the AABB from the vertex extent).
fn box_mesh() -> Mesh {
    let [x0, y0, z0] = BOX_MIN;
    let [x1, y1, z1] = BOX_MAX;
    let corners = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ];
    let vertices: Vec<f32> = corners
        .iter()
        .flat_map(|c| c.iter().map(|&x| x as f32))
        .collect();
    Mesh {
        vertices,
        indices: vec![],
        normals: None,
    }
}

/// An ABS-like base filament (E ≈ 2.0 GPa, ν = 0.35, ρ ≈ 1040 kg/m³).
fn abs_like_material() -> Value {
    structure(
        "ABS_Plastic",
        vec![
            ("youngs_modulus", scalar(2.0e9, DimensionVector::PRESSURE)),
            ("poisson_ratio", Value::Real(0.35)),
            ("density", scalar(1040.0, DimensionVector::MASS_DENSITY)),
        ],
    )
}

/// Default (all-`none`) coupon — no measured-property overrides.
fn coupon_default() -> Value {
    structure(
        "FDMCouponOverride",
        vec![
            ("ex", Value::Option(None)),
            ("ey", Value::Option(None)),
            ("ez", Value::Option(None)),
            ("gxy", Value::Option(None)),
            ("infill_gibson_ashby_c", Value::Option(None)),
            ("infill_gibson_ashby_n", Value::Option(None)),
        ],
    )
}

/// A walled+infilled FDM process: 3 walls, 4 top/bottom layers, 0.2mm layers,
/// 20% gyroid infill, Z build axis, ABS-like base material.
fn fdm_process() -> Value {
    structure(
        "FDMProcess",
        vec![
            ("build_direction", vec3_length([0.0, 0.0, 0.001])),
            ("layer_height", length(0.0002)),
            ("walls", Value::Int(3)),
            ("top_bottom_layers", Value::Int(4)),
            ("infill_density", Value::Real(0.2)),
            (
                "infill_pattern",
                Value::Enum {
                    type_name: "InfillPattern".to_string(),
                    variant: "Gyroid".to_string(),
                },
            ),
            ("material", abs_like_material()),
        ],
    )
}

/// Default consumer options: 0.4mm line width, no coupon, transverse-isotropic.
fn as_printed_options() -> Value {
    structure(
        "AsPrintedOptions",
        vec![
            ("line_width", length(0.0004)),
            ("coupon", coupon_default()),
            ("orthotropic", Value::Bool(false)),
            ("target_fidelity", Value::Int(0)),
        ],
    )
}

fn body_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::SurfaceMesh(Arc::new(box_mesh()))),
    )
}

// ── sample() round-trip machinery ────────────────────────────────────────────

/// Build a `sample(field, at)` FunctionCall expression — the public sampling
/// entry point (mirrors `reify-expr/tests/as_printed_field_sample.rs`).
fn sample_call(field: Value, at: Value) -> CompiledExpr {
    let field_type = Type::Field {
        domain: Box::new(Type::point3(Type::length())),
        codomain: Box::new(Type::StructureRef("AnisotropicMaterial".to_string())),
    };
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
        result_type: Type::StructureRef("AnisotropicMaterial".to_string()),
        content_hash: ContentHash::of(b"sample"),
    }
}

/// Sample `field` at SI-metre coordinates `at` through the public evaluator.
fn sample_at(field: &Value, at: [f64; 3]) -> Value {
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values);
    eval_expr(&sample_call(field.clone(), point3(at)), &ctx)
}

/// Read a `law` constant (e.g. `e_in_plane`) from a sampled `AnisotropicMaterial`
/// `Value` — its nested `law` StructureInstance's dimensioned scalar field.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(
        data.type_name, "AnisotropicMaterial",
        "sampled zone material must be an AnisotropicMaterial"
    );
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    match law.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected law.{key} to be a Scalar, got {other:?}"),
    }
}

/// Run the δ trampoline on the box fixtures and return its `Value::Field`.
fn printed_field() -> Value {
    let value_inputs = [Value::Undef, fdm_process(), as_printed_options()];
    let outcome = as_printed_material_r_fast_trampoline(
        &value_inputs,
        &[body_handle()],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );
    match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    }
}

#[test]
fn sampled_as_printed_field_is_non_constant_with_build_z_weakest() {
    let field = printed_field();

    // The δ contract: a heterogeneous Field<Point3, AnisotropicMaterial>.
    assert!(
        matches!(field, Value::Field { .. }),
        "trampoline must produce a Value::Field, got {field:?}"
    );

    // Sample the SAME field at a wall point (0.3 mm from the −X side face, well
    // inside the 1.2 mm = 3-wall × 0.4 mm band) and at the deep interior (box
    // centre — 20 mm from any side, 5 mm from top/bottom). The round-trip goes
    // through the public `sample()` evaluator → `sample_field_at` AsPrintedZones
    // dispatch → reify-fdm zone classification → precomputed zone material.
    let wall = sample_at(&field, [0.0003, 0.020, 0.005]);
    let infill = sample_at(&field, [0.020, 0.020, 0.005]);

    // (1) NON-CONSTANT field: the wall (dense perimeter) in-plane modulus must
    // greatly exceed the infill (Gibson-Ashby knocked-down at ρ = 0.2). If the
    // field collapsed to a constant, these would be equal.
    let wall_e = law_constant(&wall, "e_in_plane");
    let infill_e = law_constant(&infill, "e_in_plane");
    assert!(
        wall_e > infill_e * 2.0,
        "sampled wall in-plane modulus {wall_e} must greatly exceed infill {infill_e} \
         (non-constant material field)"
    );

    // (2) Build-Z weakest (PRD C4): in BOTH sampled zones the axial (build / Z)
    // modulus must be weaker than the in-plane modulus.
    for (name, mat) in [("wall", &wall), ("infill", &infill)] {
        let e_in_plane = law_constant(mat, "e_in_plane");
        let e_axial = law_constant(mat, "e_axial");
        assert!(
            e_axial < e_in_plane,
            "{name}: sampled build-Z (axial {e_axial}) must be weaker than in-plane ({e_in_plane})"
        );
    }
}

// ── degraded-path coverage ───────────────────────────────────────────────────
//
// The trampoline's headline robustness feature: when an input is malformed or
// the body realization is unavailable, every `?` early-return in
// `build_as_printed_field` funnels to `degraded_field()`, which still returns a
// well-typed `AsPrintedZones` field whose lambda is `Value::Undef`. The node
// degrades honestly — it must never panic. The happy-path tests above all feed
// well-formed inputs with a valid mesh handle, so this exercises the None branch.

/// Assert a trampoline `outcome` is the honest-degradation field: still
/// `Completed`, still a well-typed `AsPrintedZones` `Value::Field`, but with an
/// `Undef` lambda slot — and that sampling it yields `Undef` rather than panicking.
fn assert_degraded(outcome: ComputeOutcome) {
    let field = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("degraded path must still Complete, got {other:?}"),
    };
    match &field {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::AsPrintedZones),
                "degraded field keeps the AsPrintedZones source, got {source:?}"
            );
            assert!(
                matches!(lambda.as_ref(), Value::Undef),
                "degraded field lambda must be Undef, got {lambda:?}"
            );
        }
        other => panic!("degraded path must still produce a Value::Field, got {other:?}"),
    }
    // lambda = Undef ⇒ not a 7-element List ⇒ `sample_field_at`'s AsPrintedZones
    // arm doesn't match ⇒ it falls through to `Undef`. No panic.
    let sampled = sample_at(&field, [0.020, 0.020, 0.005]);
    assert!(
        matches!(sampled, Value::Undef),
        "sampling a degraded field must yield Undef, got {sampled:?}"
    );
}

#[test]
fn degraded_inputs_yield_undef_lambda_field_without_panicking() {
    let cancel = CancellationHandle::new();

    // (1) Realization unavailable: well-formed value_inputs but NO realization
    //     handle ⇒ body_aabb() returns None ⇒ bail at the `?` after the
    //     FDMProcess / AsPrintedOptions parse both succeed.
    assert_degraded(as_printed_material_r_fast_trampoline(
        &[Value::Undef, fdm_process(), as_printed_options()],
        &[],
        &Value::Undef,
        None,
        &cancel,
    ));

    // (2) Empty value_inputs ⇒ `value_inputs.get(1)?` is None ⇒ immediate bail,
    //     even with a valid body mesh handle present.
    assert_degraded(as_printed_material_r_fast_trampoline(
        &[],
        &[body_handle()],
        &Value::Undef,
        None,
        &cancel,
    ));

    // (3) Non-StructureInstance process ⇒ `struct_data(process)?` is None.
    assert_degraded(as_printed_material_r_fast_trampoline(
        &[Value::Undef, Value::Undef, as_printed_options()],
        &[body_handle()],
        &Value::Undef,
        None,
        &cancel,
    ));
}
