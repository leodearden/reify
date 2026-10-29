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

use crate::common::as_printed::{
    BOX_MAX, BOX_MIN, as_printed_options, assert_degraded_field, box_mesh, coupon_default,
    fdm_process, length, scalar, with_field,
};
use reify_core::{ContentHash, DiagnosticCode, DimensionVector, RealizationNodeId, Severity, Type};
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

// ── task 6922 η: the as-printed readers reject a wrong dimension ─────────────

fn run_r_fast(options: Value) -> ComputeOutcome {
    run_r_fast_with(fdm_process(), options)
}

fn run_r_fast_with(process: Value, options: Value) -> ComputeOutcome {
    as_printed_material_r_fast_trampoline(
        &[Value::Undef, process, options],
        &[body_handle()],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    )
}

/// `outcome` is `Failed` with exactly one `DimensionedArgRejected` Error
/// naming `path` (the code is the contract; the path is a naming check).
fn assert_rejects(outcome: ComputeOutcome, path: &str) {
    let ComputeOutcome::Failed { diagnostics, .. } = &outcome else {
        panic!("a refused {path} must yield Failed, got {outcome:?}");
    };
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one diagnostic, got {diagnostics:?}"
    );
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::DimensionedArgRejected)
    );
    assert!(
        diagnostic.message.contains(path),
        "the rejection must name {path:?}, got {:?}",
        diagnostic.message
    );
}

/// The infill zone's in-plane modulus of a `Completed` run with no diagnostics.
fn infill_e_in_plane(outcome: ComputeOutcome) -> f64 {
    match outcome {
        ComputeOutcome::Completed {
            result: Value::Field { lambda, .. },
            diagnostics,
            ..
        } => {
            assert!(
                diagnostics.is_empty(),
                "no diagnostics expected, got {diagnostics:?}"
            );
            let Value::List(items) = lambda.as_ref() else {
                panic!("expected the 7-element zone payload, got {lambda:?}");
            };
            law_constant(&items[6], "e_in_plane")
        }
        other => panic!("expected Completed with a Field, got {other:?}"),
    }
}

/// task 6940: the zone-params slot (lambda items[2]) types its two band widths
/// — `layer_height`, `line_width` — as `Length`; the counts and the unit build
/// direction stay dimensionless `Value::Real`.
#[test]
fn as_printed_trampoline_types_the_zone_param_lengths() {
    let ComputeOutcome::Completed {
        result: Value::Field { lambda, .. },
        ..
    } = run_r_fast(as_printed_options())
    else {
        panic!("expected Completed with a Field");
    };
    let Value::List(items) = lambda.as_ref() else {
        panic!("expected the 7-element zone payload, got {lambda:?}");
    };
    let Value::List(p) = &items[2] else {
        panic!("expected the params list at items[2], got {:?}", items[2]);
    };
    assert_eq!(p.len(), 7, "params must be a 7-element list");
    assert_eq!(
        p[2],
        scalar(0.0002, DimensionVector::LENGTH),
        "layer_height"
    );
    assert_eq!(p[3], scalar(0.0004, DimensionVector::LENGTH), "line_width");
    for idx in [0, 1, 4, 5, 6] {
        assert!(
            matches!(p[idx], Value::Real(_)),
            "params[{idx}] must stay a dimensionless Value::Real, got {:?}",
            p[idx]
        );
    }
}

/// The result of a `Completed` run.
fn completed_result(outcome: ComputeOutcome) -> Value {
    match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    }
}

/// task 6940: a non-finite `line_width` has no meaningful wall band, so the
/// field degrades to its Undef lambda rather than carrying it into the params.
#[test]
fn as_printed_trampoline_degrades_on_a_non_finite_line_width() {
    let options = with_field(as_printed_options(), "line_width", length(f64::NAN));
    assert_degraded_field(&completed_result(run_r_fast(options)));
}

/// task 6940: likewise for a non-finite `layer_height` (the skin band).
#[test]
fn as_printed_trampoline_degrades_on_a_non_finite_layer_height() {
    let process = with_field(fdm_process(), "layer_height", length(f64::INFINITY));
    assert_degraded_field(&completed_result(run_r_fast_with(
        process,
        as_printed_options(),
    )));
}

fn coupon_with_ex(ex: Value) -> Value {
    with_field(coupon_default(), "ex", Value::Option(Some(Box::new(ex))))
}

#[test]
fn as_printed_trampoline_rejects_a_bare_line_width() {
    let options = with_field(as_printed_options(), "line_width", Value::Real(0.4));
    assert_rejects(run_r_fast(options), "AsPrintedOptions.line_width");
}

#[test]
fn as_printed_trampoline_rejects_a_length_coupon_modulus() {
    let coupon = coupon_with_ex(scalar(0.002, DimensionVector::LENGTH));
    let options = with_field(as_printed_options(), "coupon", coupon);
    assert_rejects(run_r_fast(options), "FDMCouponOverride.ex");
}

#[test]
fn as_printed_trampoline_rejects_a_non_numeric_line_width() {
    let options = with_field(as_printed_options(), "line_width", Value::Bool(true));
    assert_rejects(run_r_fast(options), "AsPrintedOptions.line_width");
}

#[test]
fn as_printed_trampoline_rejects_a_length_infill_density() {
    let process = with_field(
        fdm_process(),
        "infill_density",
        scalar(0.2, DimensionVector::LENGTH),
    );
    assert_rejects(
        run_r_fast_with(process, as_printed_options()),
        "FDMProcess.infill_density",
    );
}

#[test]
fn as_printed_trampoline_reads_a_bare_gibson_ashby_coefficient() {
    let baseline = infill_e_in_plane(run_r_fast(as_printed_options()));
    let coupon = with_field(
        coupon_default(),
        "infill_gibson_ashby_c",
        Value::Option(Some(Box::new(Value::Real(2.0)))),
    );
    let overridden = infill_e_in_plane(run_r_fast(with_field(
        as_printed_options(),
        "coupon",
        coupon,
    )));
    assert_ne!(
        overridden, baseline,
        "a bare dimensionless C must be read, not refused or ignored"
    );
}

#[test]
fn as_printed_trampoline_applies_a_pressure_coupon_modulus_without_diagnostics() {
    let baseline = infill_e_in_plane(run_r_fast(as_printed_options()));
    let coupon = coupon_with_ex(scalar(2.0e9, DimensionVector::PRESSURE));
    let overridden = infill_e_in_plane(run_r_fast(with_field(
        as_printed_options(),
        "coupon",
        coupon,
    )));
    assert_eq!(
        overridden, 2.0e9,
        "the ex override sets the in-plane modulus"
    );
    assert_ne!(
        overridden, baseline,
        "the override must change the infill law"
    );
}
