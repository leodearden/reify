// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline-level integration test for `fdm::as_printed_material_r0`
//! (task θ / 3790, step-9 RED → step-10 GREEN).
//!
//! Builds the trampoline inputs by hand — `value_inputs = [Value::String(gcode),
//! FDMProcess, AsPrintedOptions]`, NO realization handles (the R0 rung derives
//! its AABB from the parsed toolpath beads, not a body mesh) — invokes the R0
//! trampoline directly, and asserts the produced
//! `Value::Field { source: AsPrintedZones }` carries:
//!   * codomain `AnisotropicMaterial`,
//!   * the 7-element zone payload,
//!   * an AABB derived from the toolpath bead centerlines (mm→SI), and
//!   * per-zone laws that are `OrthotropicMaterial` with build-Z (`e3`) weakest.
//!
//! It then calls the R-fast trampoline on equivalent inputs and asserts the R0
//! field DIFFERS structurally: R0 is orthotropic (law `OrthotropicMaterial`,
//! `e1 ≠ e2`) with a bead-direction frame x-axis, whereas R-fast is
//! transverse-isotropic with an arbitrary build-Z complement x-axis.
//!
//! Fails until step-10 adds `as_printed_material_r0_trampoline` (the symbol is
//! absent, so the test target does not compile — the RED state).

use std::sync::Arc;

mod common;
use common::as_printed::{as_printed_options, box_mesh, fdm_process, r0_toolpath_gcode};
use reify_core::{ContentHash, RealizationNodeId, Type};
use reify_eval::compute_targets::as_printed_material::as_printed_material_r_fast_trampoline;
use reify_eval::compute_targets::as_printed_material_r0::as_printed_material_r0_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle, RealizedContent};
use reify_fdm::parse_prusaslicer_gcode;
use reify_ir::{FieldSourceKind, Value};

/// mm → SI metres (mirrors the trampoline's own conversion).
const MM_TO_M: f64 = 1.0e-3;

fn body_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::SurfaceMesh(Arc::new(box_mesh()))),
    )
}

/// Run the R0 trampoline on the multi-role gcode + default process/options.
fn run_r0(gcode: &str) -> Value {
    let value_inputs = [
        Value::String(gcode.to_string()),
        fdm_process(),
        as_printed_options(),
    ];
    let outcome = as_printed_material_r0_trampoline(
        &value_inputs,
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );
    match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    }
}

/// Destructure a `Value::Field`, returning `(codomain, source, lambda items)`.
fn field_parts(result: &Value) -> (Type, FieldSourceKind, Vec<Value>) {
    match result {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => {
            let items = match lambda.as_ref() {
                Value::List(items) => items.clone(),
                other => panic!("expected lambda Value::List, got {other:?}"),
            };
            (codomain_type.clone(), source.clone(), items)
        }
        other => panic!("expected Value::Field, got {other:?}"),
    }
}

/// The `type_name` of an `AnisotropicMaterial`'s nested `law` StructureInstance
/// (e.g. `OrthotropicMaterial` vs `TransverseIsotropicMaterial`).
fn law_type_name(aniso: &Value) -> String {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(data.type_name, "AnisotropicMaterial");
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    law.type_name.clone()
}

/// Read a dimensioned `law` constant (e.g. `e1`) from an `AnisotropicMaterial`.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    match law.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected law.{key} to be a Scalar, got {other:?}"),
    }
}

/// Read the material frame's `x_axis` (the dominant bead direction for R0, an
/// arbitrary build-Z complement for R-fast).
fn frame_x_axis(aniso: &Value) -> [f64; 3] {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    let frame = data.fields.get("frame").expect("AnisotropicMaterial.frame");
    let Value::StructureInstance(frame) = frame else {
        panic!("expected frame StructureInstance, got {frame:?}");
    };
    let x = frame.fields.get("x_axis").expect("MaterialFrame.x_axis");
    point_components(x)
}

fn point_components(v: &Value) -> [f64; 3] {
    match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => [
            c[0].as_f64().unwrap(),
            c[1].as_f64().unwrap(),
            c[2].as_f64().unwrap(),
        ],
        other => panic!("expected 3-component Point/Vector, got {other:?}"),
    }
}

/// The toolpath bead-centerline AABB (mm→SI), computed independently from the
/// parser so the test does not hardcode the trampoline's own arithmetic.
fn expected_toolpath_aabb(gcode: &str) -> ([f64; 3], [f64; 3]) {
    let tp = parse_prusaslicer_gcode(gcode).expect("gcode must parse");
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for bead in &tp.beads {
        for p in &bead.centerline {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    (
        [min[0] * MM_TO_M, min[1] * MM_TO_M, min[2] * MM_TO_M],
        [max[0] * MM_TO_M, max[1] * MM_TO_M, max[2] * MM_TO_M],
    )
}

#[test]
fn r0_trampoline_produces_orthotropic_as_printed_zones_field() {
    let gcode = r0_toolpath_gcode();
    let result = run_r0(gcode);

    // (1) Field<_, AnisotropicMaterial> sourced AsPrintedZones.
    let (codomain, source, items) = field_parts(&result);
    assert!(
        matches!(source, FieldSourceKind::AsPrintedZones),
        "field source must be AsPrintedZones, got {source:?}"
    );
    assert_eq!(
        codomain,
        Type::StructureRef("AnisotropicMaterial".to_string()),
        "field codomain must be AnisotropicMaterial"
    );

    // (2) The lambda slot is the 7-element zone payload.
    assert_eq!(items.len(), 7, "AsPrintedZones lambda must be a 7-element list");

    // (3) The stored AABB matches the toolpath bead extents (mm→SI).
    let (exp_min, exp_max) = expected_toolpath_aabb(gcode);
    let min = point_components(&items[0]);
    let max = point_components(&items[1]);
    for k in 0..3 {
        assert!(
            (min[k] - exp_min[k]).abs() < 1e-12,
            "aabb_min[{k}] {} != {} (toolpath-derived, mm→SI)",
            min[k],
            exp_min[k]
        );
        assert!(
            (max[k] - exp_max[k]).abs() < 1e-12,
            "aabb_max[{k}] {} != {} (toolpath-derived, mm→SI)",
            max[k],
            exp_max[k]
        );
    }
    // Sanity: the snippet spans 40×20 mm in XY → 0.040×0.020 m.
    assert!((max[0] - 0.040).abs() < 1e-12, "x extent 40 mm → 0.040 m");
    assert!((max[1] - 0.020).abs() < 1e-12, "y extent 20 mm → 0.020 m");

    // (4) Every zone law is OrthotropicMaterial with build-Z (e3) the weakest.
    for (name, idx) in [("wall", 4usize), ("skin", 5), ("infill", 6)] {
        let mat = &items[idx];
        assert_eq!(
            law_type_name(mat),
            "OrthotropicMaterial",
            "{name}: R0 zone law must be OrthotropicMaterial"
        );
        let e1 = law_constant(mat, "e1");
        let e2 = law_constant(mat, "e2");
        let e3 = law_constant(mat, "e3");
        assert!(e3 < e1, "{name}: build-Z e3 ({e3}) must be < e1 ({e1})");
        assert!(e3 < e2, "{name}: build-Z e3 ({e3}) must be < e2 ({e2})");
        assert!(e1 >= e2, "{name}: orthotropic e1 ({e1}) ≥ e2 ({e2})");

        // The frame x-axis is the dominant bead direction (+X), a unit vector.
        let x = frame_x_axis(mat);
        let n = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
        assert!((n - 1.0).abs() < 1e-9, "{name}: frame x-axis must be unit, got {x:?}");
    }
}

#[test]
fn r0_field_differs_structurally_from_r_fast() {
    // R0 on the toolpath; R-fast on the equivalent body + same process/options.
    let r0 = run_r0(r0_toolpath_gcode());
    let (_, _, r0_items) = field_parts(&r0);

    let rfast_outcome = as_printed_material_r_fast_trampoline(
        &[Value::Undef, fdm_process(), as_printed_options()],
        &[body_handle()],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );
    let rfast = match rfast_outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected R-fast Completed, got {other:?}"),
    };
    let (_, _, rfast_items) = field_parts(&rfast);

    let r0_wall = &r0_items[4];
    let rfast_wall = &rfast_items[4];

    // (a) Law TYPE differs: R0 orthotropic, R-fast transverse-isotropic.
    assert_eq!(law_type_name(r0_wall), "OrthotropicMaterial");
    assert_eq!(law_type_name(rfast_wall), "TransverseIsotropicMaterial");
    assert_ne!(
        law_type_name(r0_wall),
        law_type_name(rfast_wall),
        "R0 and R-fast zone laws must be different constitutive types"
    );

    // (b) R0 carries a genuine in-plane orthotropy split (e1 ≠ e2) that the
    //     R-fast transverse-isotropic law structurally cannot represent.
    let e1 = law_constant(r0_wall, "e1");
    let e2 = law_constant(r0_wall, "e2");
    assert!(
        (e1 - e2).abs() > 1e-3 * e1,
        "R0 wall must be orthotropic: e1 ({e1}) ≠ e2 ({e2})"
    );

    // (c) The frame x-axis differs: R0 tracks the real bead direction (+X),
    //     R-fast uses an arbitrary build-Z complement.
    let r0_x = frame_x_axis(r0_wall);
    let rfast_x = frame_x_axis(rfast_wall);
    let dx = (r0_x[0] - rfast_x[0]).abs()
        + (r0_x[1] - rfast_x[1]).abs()
        + (r0_x[2] - rfast_x[2]).abs();
    assert!(
        dx > 1e-6,
        "R0 frame x-axis {r0_x:?} must differ from R-fast {rfast_x:?}"
    );
}

#[test]
fn r0_trampoline_degrades_to_undef_lambda_on_malformed_gcode() {
    // A `;WIDTH:` directive with a non-numeric value is a hard toolpath parse
    // error → the trampoline must degrade honestly to an Undef-lambda field
    // (every sample falls through to Undef), never panic.
    let result = run_r0(";WIDTH:notanumber\n");
    let (codomain, source, _) = match &result {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => (codomain_type.clone(), source.clone(), lambda.clone()),
        other => panic!("expected Value::Field even on degrade, got {other:?}"),
    };
    assert!(
        matches!(source, FieldSourceKind::AsPrintedZones),
        "degraded field is still well-typed AsPrintedZones"
    );
    assert_eq!(
        codomain,
        Type::StructureRef("AnisotropicMaterial".to_string())
    );
    // The lambda is Undef — the honest-degradation signal.
    match &result {
        Value::Field { lambda, .. } => assert!(
            matches!(lambda.as_ref(), Value::Undef),
            "malformed gcode must degrade to an Undef lambda, got {lambda:?}"
        ),
        _ => unreachable!(),
    }
}
