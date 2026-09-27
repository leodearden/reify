//! Pins INV-AD-4 for `GeometryOp::Draft.angle`: its SI magnitude is RADIANS,
//! so a draft angle of `0.1` means 0.1 rad, not 0.1 deg.
//!
//! Fixture: a centred cube whose +X wall alone is drafted about a neutral plane
//! on the cube's base (pull +Z). Two closed forms must hold: the wall tilts by
//! exactly `angle`, and |volume change| == 0.5 * side^3 * tan(angle), the prism
//! swept as the wall pivots about its base edge.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

const CUBE_SIDE: f64 = 20.0;
const DRAFT_ANGLE: f64 = 0.1;
const TILT_TOLERANCE_RAD: f64 = 1e-9;
const VOLUME_TOLERANCE: f64 = 1e-6;

fn plus_x_wall(kernel: &mut OcctKernel, cube: GeometryHandleId) -> GeometryHandleId {
    let faces = kernel.extract_faces(cube).expect("extract_faces(cube)");
    let normal_x = |face: GeometryHandleId| {
        kernel
            .face_outward_unit_normal_for_test(face)
            .expect("face_outward_unit_normal_for_test")[0]
    };
    faces
        .into_iter()
        .max_by(|a, b| normal_x(*a).total_cmp(&normal_x(*b)))
        .expect("cube has faces")
}

fn wall_tilt(
    kernel: &mut OcctKernel,
    original_wall: GeometryHandleId,
    drafted: GeometryHandleId,
) -> f64 {
    let faces = kernel
        .extract_faces(drafted)
        .expect("extract_faces(drafted)");
    faces
        .into_iter()
        .map(|face| {
            kernel
                .surface_angle(original_wall, face)
                .expect("surface_angle(original_wall, drafted face)")
        })
        .fold(f64::INFINITY, f64::min)
}

fn volume(kernel: &OcctKernel, solid: GeometryHandleId) -> f64 {
    match kernel
        .query(&GeometryQuery::Volume(solid))
        .expect("Volume query")
    {
        Value::Real(v) => v,
        other => panic!("expected Value::Real for volume, got {other:?}"),
    }
}

fn prism_volume(angle: f64) -> f64 {
    0.5 * CUBE_SIDE.powi(3) * angle.tan()
}

#[test]
fn draft_angle_magnitude_is_read_as_radians() {
    let mut kernel = OcctKernel::new();
    let cube = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(CUBE_SIDE),
            height: Value::Real(CUBE_SIDE),
            depth: Value::Real(CUBE_SIDE),
        })
        .expect("cube creation")
        .id;
    let neutral_plane = kernel.store_circle_face_for_test(CUBE_SIDE, -CUBE_SIDE / 2.0);
    let wall = plus_x_wall(&mut kernel, cube);

    let drafted = kernel
        .execute(&GeometryOp::Draft {
            target: cube,
            faces: vec![wall],
            angle: Value::angle(DRAFT_ANGLE),
            plane: neutral_plane,
        })
        .expect("single-wall draft of the cube must succeed")
        .id;

    let tilt = wall_tilt(&mut kernel, wall, drafted);
    assert!(
        (tilt - DRAFT_ANGLE).abs() < TILT_TOLERANCE_RAD,
        "drafted +X wall tilt = {tilt:.16} rad; radians reading predicts {DRAFT_ANGLE:.16}, \
         degrees reading predicts {:.16}",
        DRAFT_ANGLE.to_radians()
    );

    let volume_change = (volume(&kernel, drafted) - volume(&kernel, cube)).abs();
    assert!(
        (volume_change - prism_volume(DRAFT_ANGLE)).abs() < VOLUME_TOLERANCE,
        "|volume change| = {volume_change:.12}; radians reading predicts {:.12}, \
         degrees reading predicts {:.12}",
        prism_volume(DRAFT_ANGLE),
        prism_volume(DRAFT_ANGLE.to_radians())
    );
}
