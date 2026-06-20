// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline-level integration test for `fdm::as_printed_material_r_fast`
//! (task δ / 3786, step-7 RED → step-8 GREEN).
//!
//! Constructs the trampoline inputs by hand — a [`RealizationReadHandle`]
//! wrapping a small box surface mesh plus
//! `value_inputs = [body, FDMProcess, AsPrintedOptions]` — invokes the δ
//! trampoline directly, and asserts the produced
//! `Value::Field { source: AsPrintedZones }` carries:
//!   * codomain `AnisotropicMaterial`,
//!   * the body's AABB (derived from the realization mesh vertices), and
//!   * DISTINCT wall (dense) vs infill (Gibson-Ashby knocked-down) materials,
//!     with the build (Z / axial) axis the weakest in both.
//!
//! Fails until step-8 adds `as_printed_material_r_fast_trampoline` (the symbol
//! is absent, so the test target does not compile — the RED state).

use std::sync::Arc;

mod common;
use common::as_printed::{as_printed_options, box_mesh, fdm_process, BOX_MAX, BOX_MIN};
use reify_core::{ContentHash, RealizationNodeId, Type};
use reify_eval::compute_targets::as_printed_material::as_printed_material_r_fast_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle, RealizedContent};
use reify_ir::{FieldSourceKind, Value};

fn body_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::SurfaceMesh(Arc::new(box_mesh()))),
    )
}

/// Read a `law` constant (e.g. `e_in_plane`) from an `AnisotropicMaterial`
/// `Value` — its nested `law` StructureInstance's dimensioned scalar field.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(
        data.type_name, "AnisotropicMaterial",
        "zone material must be an AnisotropicMaterial"
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

#[test]
fn as_printed_trampoline_produces_as_printed_zones_field_with_distinct_zone_materials() {
    let value_inputs = [Value::Undef, fdm_process(), as_printed_options()];
    let outcome = as_printed_material_r_fast_trampoline(
        &value_inputs,
        &[body_handle()],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    };

    // (1) The result is a Field<_, AnisotropicMaterial> sourced AsPrintedZones.
    let (codomain_type, source, lambda) = match &result {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => (codomain_type, source, lambda.clone()),
        other => panic!("expected Value::Field, got {other:?}"),
    };
    assert!(
        matches!(source, FieldSourceKind::AsPrintedZones),
        "field source must be AsPrintedZones, got {source:?}"
    );
    assert_eq!(
        *codomain_type,
        Type::StructureRef("AnisotropicMaterial".to_string()),
        "field codomain must be AnisotropicMaterial"
    );

    // (2) The lambda slot is the 7-element zone payload.
    let items = match lambda.as_ref() {
        Value::List(items) => items,
        other => panic!("expected lambda Value::List, got {other:?}"),
    };
    assert_eq!(
        items.len(),
        7,
        "AsPrintedZones lambda must be a 7-element list"
    );

    // (3) The stored AABB matches the box extents (derived from the mesh).
    let min = point_components(&items[0]);
    let max = point_components(&items[1]);
    for k in 0..3 {
        assert!(
            (min[k] - BOX_MIN[k]).abs() < 1e-9,
            "aabb_min[{k}] {} != {}",
            min[k],
            BOX_MIN[k]
        );
        assert!(
            (max[k] - BOX_MAX[k]).abs() < 1e-9,
            "aabb_max[{k}] {} != {}",
            max[k],
            BOX_MAX[k]
        );
    }

    // (4) Wall (dense) vs infill (Gibson-Ashby knocked-down) materials are
    // DISTINCT — the non-constant-field signal — and the build (Z / axial) axis
    // is the weakest in BOTH (PRD C4 invariant).
    let mat_wall = &items[4]; // Zone::Wall slot
    let mat_infill = &items[6]; // Zone::Infill slot

    let wall_e = law_constant(mat_wall, "e_in_plane");
    let infill_e = law_constant(mat_infill, "e_in_plane");
    assert!(
        wall_e > infill_e * 2.0,
        "wall in-plane modulus {wall_e} must greatly exceed infill {infill_e}"
    );

    for (name, mat) in [("wall", mat_wall), ("infill", mat_infill)] {
        let e_in_plane = law_constant(mat, "e_in_plane");
        let e_axial = law_constant(mat, "e_axial");
        assert!(
            e_axial < e_in_plane,
            "{name}: build-Z (axial {e_axial}) must be weaker than in-plane ({e_in_plane})"
        );
    }
}
