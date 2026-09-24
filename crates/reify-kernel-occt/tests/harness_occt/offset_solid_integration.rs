//! Integration tests for `GeometryOp::OffsetSolid` via the public
//! OcctKernel API: every face of a solid moves `distance` along its outward
//! normal (inward when negative) and sharp edges stay sharp, so each offset
//! volume matches its closed form. All lengths are SI metres.

#![cfg(has_occt)]

use std::f64::consts::PI;

use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

const EXACT: f64 = 1e-9;

fn execute(kernel: &mut OcctKernel, op: GeometryOp) -> GeometryHandleId {
    kernel
        .execute(&op)
        .unwrap_or_else(|e| panic!("{op:?} should succeed: {e}"))
        .id
}

fn cube(kernel: &mut OcctKernel, side: f64) -> GeometryHandleId {
    execute(
        kernel,
        GeometryOp::Box {
            width: Value::Real(side),
            height: Value::Real(side),
            depth: Value::Real(side),
        },
    )
}

fn offset_solid(
    kernel: &mut OcctKernel,
    target: GeometryHandleId,
    distance: f64,
) -> Result<GeometryHandleId, GeometryError> {
    kernel
        .execute(&GeometryOp::OffsetSolid {
            target,
            distance: Value::Real(distance),
        })
        .map(|handle| handle.id)
}

fn volume(kernel: &OcctKernel, id: GeometryHandleId) -> f64 {
    kernel
        .query(&GeometryQuery::Volume(id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric")
}

fn assert_rel(got: f64, expected: f64, tol: f64, label: &str) {
    let rel_err = (got - expected).abs() / expected.abs();
    assert!(
        rel_err <= tol,
        "{label}: got {got:.9e}, expected {expected:.9e} (rel_err={rel_err:.3e} > {tol:.0e})"
    );
}

/// Minkowski sum of a cube of side `s` with a ball of radius `r`.
fn rounded_cube_volume(s: f64, r: f64) -> f64 {
    s.powi(3) + 6.0 * s * s * r + 3.0 * PI * s * r * r + 4.0 / 3.0 * PI * r.powi(3)
}

#[test]
fn box_outward_offset_moves_every_face_out() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.010);
    let outer = offset_solid(&mut kernel, solid, 0.0005).expect("outward offset");
    assert_rel(
        volume(&kernel, outer),
        0.011_f64.powi(3),
        EXACT,
        "box +0.5mm",
    );
}

#[test]
fn box_inward_offset_moves_every_face_in() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.010);
    let inner = offset_solid(&mut kernel, solid, -0.0005).expect("inward offset");
    assert_rel(
        volume(&kernel, inner),
        0.009_f64.powi(3),
        EXACT,
        "box -0.5mm",
    );
}

#[test]
fn cylinder_offset_is_exact_both_ways() {
    let mut kernel = OcctKernel::new();
    let solid = execute(
        &mut kernel,
        GeometryOp::Cylinder {
            radius: Value::Real(0.005),
            height: Value::Real(0.010),
        },
    );
    let outer = offset_solid(&mut kernel, solid, 0.0005).expect("outward offset");
    let inner = offset_solid(&mut kernel, solid, -0.0005).expect("inward offset");
    assert_rel(
        volume(&kernel, outer),
        PI * 0.0055_f64.powi(2) * 0.011,
        EXACT,
        "cylinder +0.5mm",
    );
    assert_rel(
        volume(&kernel, inner),
        PI * 0.0045_f64.powi(2) * 0.009,
        EXACT,
        "cylinder -0.5mm",
    );
}

#[test]
fn small_solid_inward_offset_is_not_rejected_as_degenerate() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.005);
    let inner = offset_solid(&mut kernel, solid, -0.0005).expect("inward offset of a 5mm box");
    assert_rel(
        volume(&kernel, inner),
        0.004_f64.powi(3),
        EXACT,
        "5mm box -0.5mm",
    );
}

#[test]
fn zero_distance_is_rejected() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.010);
    let result = offset_solid(&mut kernel, solid, 0.0);
    assert!(
        matches!(result, Err(GeometryError::OperationFailed(_))),
        "zero-distance OffsetSolid should be OperationFailed, got {result:?}"
    );
}

#[test]
fn filleted_solid_offsets_and_zone_are_exact() {
    let mut kernel = OcctKernel::new();
    let block = cube(&mut kernel, 0.010);
    let rounded = execute(
        &mut kernel,
        GeometryOp::Fillet {
            target: block,
            edges: vec![],
            radius: Value::Real(0.001),
        },
    );
    let outer = offset_solid(&mut kernel, rounded, 0.0005).expect("outward offset");
    let inner = offset_solid(&mut kernel, rounded, -0.0005).expect("inward offset");
    let zone = execute(
        &mut kernel,
        GeometryOp::Difference {
            left: outer,
            right: inner,
        },
    );

    let core = 0.008;
    let outer_expected = rounded_cube_volume(core, 0.0015);
    let inner_expected = rounded_cube_volume(core, 0.0005);
    assert_rel(
        volume(&kernel, outer),
        outer_expected,
        EXACT,
        "filleted +0.5mm",
    );
    assert_rel(
        volume(&kernel, inner),
        inner_expected,
        EXACT,
        "filleted -0.5mm",
    );
    assert_rel(
        volume(&kernel, zone),
        outer_expected - inner_expected,
        EXACT,
        "filleted ±0.5mm zone",
    );
}

#[test]
fn inward_offset_past_the_inradius_is_an_error() {
    let mut kernel = OcctKernel::new();
    let solid = cube(&mut kernel, 0.010);
    let result = offset_solid(&mut kernel, solid, -0.006);
    assert!(
        result.is_err(),
        "offsetting a 10mm box inward by 6mm should be Err, got {result:?}"
    );
}

#[test]
fn offset_of_a_face_is_rejected() {
    let mut kernel = OcctKernel::new();
    let face = execute(
        &mut kernel,
        GeometryOp::RectangleProfile {
            width: Value::Real(0.020),
            height: Value::Real(0.010),
        },
    );
    let result = offset_solid(&mut kernel, face, 0.001);
    assert!(
        result.is_err(),
        "OffsetSolid of a face should be Err (offset_surface offsets faces), got {result:?}"
    );
}
