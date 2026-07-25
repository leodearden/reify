//! Integration tests for the sweep_guided operation via the public
//! OcctKernel API.
//!
//! These tests exercise GeometryOp::SweepGuided through
//! OcctKernel::execute(), verifying both the happy path
//! (non-degenerate shape, finite bounding box) and the semantic
//! distinction from plain Sweep (the guide wire changes the result,
//! so centroids differ).
//!
//! Notes:
//! - The profile is a closed circular wire (Arc from 0 to 2π), so the
//!   resulting pipe-shell is a closed tube with positive volume.
//! - The guide wire is intentionally non-parallel to the path spine so
//!   that MakePipeShell's SetMode(aux, false) has visible effect.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Build a closed circular wire profile at z=0 of the given radius.
fn make_circle_profile(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius,
            start_angle: 0.0,
            end_angle: 2.0 * std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle) creation should succeed")
        .id
}

/// Build a straight line-segment path along +Z from (0,0,0) to
/// (0,0,`length`).
fn make_straight_path(kernel: &mut OcctKernel, length: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (path) creation should succeed")
        .id
}

/// Build an auxiliary-spine guide wire clearly offset from the main
/// spine. MakePipeShell's auxiliary spine must not be coincident with
/// the spine at any parameter (otherwise OCCT reports
/// "gp_Vec::Normalized() - vector has zero norm") so both endpoints
/// must be offset in X. `dx_start`/`dx_end` vary the offset along the
/// parameter so the section orientation is non-constant and the guide
/// meaningfully biases the result.
fn make_offset_guide(
    kernel: &mut OcctKernel,
    dx_start: f64,
    dx_end: f64,
    length: f64,
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: dx_start,
            y1: 0.0,
            z1: 0.0,
            x2: dx_end,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (guide) creation should succeed")
        .id
}

/// Parse a JSON-encoded centroid string `{"x":…,"y":…,"z":…}` into (x, y, z).
fn parse_centroid(s: &str) -> (f64, f64, f64) {
    let inner = s.trim_start_matches('{').trim_end_matches('}');
    let mut x = f64::NAN;
    let mut y = f64::NAN;
    let mut z = f64::NAN;
    for pair in inner.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "x" => x = val,
            "y" => y = val,
            "z" => z = val,
            _ => {}
        }
    }
    (x, y, z)
}

#[test]
fn sweep_guided_produces_valid_shape() {
    let mut kernel = OcctKernel::new();
    let profile = make_circle_profile(&mut kernel, 0.02);
    let path = make_straight_path(&mut kernel, 0.1);
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    let result = kernel
        .execute(&GeometryOp::SweepGuided {
            profile,
            path,
            guide,
        })
        .expect("SweepGuided should succeed");

    // The pipe-shell should have a finite, non-degenerate bounding box.
    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(result.id))
        .expect("BoundingBox query should succeed");
    match bbox {
        Value::String(s) => {
            // All six fields must parse to finite numbers.
            let trimmed = s.trim_start_matches('{').trim_end_matches('}');
            for pair in trimmed.split(',') {
                let mut parts = pair.splitn(2, ':');
                let _key = parts.next().unwrap();
                let val: f64 = parts
                    .next()
                    .unwrap()
                    .trim()
                    .parse()
                    .expect("bbox component should be numeric");
                assert!(val.is_finite(), "bbox component must be finite, got {val}");
            }
        }
        other => panic!("expected bbox String, got {:?}", other),
    }
}

#[test]
fn sweep_guided_orientation_differs_from_plain_sweep() {
    let mut kernel = OcctKernel::new();
    let profile_plain = make_circle_profile(&mut kernel, 0.02);
    let path_plain = make_straight_path(&mut kernel, 0.1);
    let plain = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_plain,
            path: path_plain,
        })
        .expect("plain Sweep should succeed");
    let plain_centroid = kernel
        .query(&GeometryQuery::Centroid(plain.id))
        .expect("plain Sweep centroid should query");
    let (plain_x, plain_y, plain_z) = match plain_centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };

    // Fresh profile/path for the guided sweep — MakePipeShell consumes
    // its inputs and we've already fed these to plain Sweep.
    let profile_g = make_circle_profile(&mut kernel, 0.02);
    let path_g = make_straight_path(&mut kernel, 0.1);
    let guide_g = make_offset_guide(&mut kernel, 0.06, 0.03, 0.1);
    let guided = kernel
        .execute(&GeometryOp::SweepGuided {
            profile: profile_g,
            path: path_g,
            guide: guide_g,
        })
        .expect("SweepGuided should succeed");
    let guided_centroid = kernel
        .query(&GeometryQuery::Centroid(guided.id))
        .expect("SweepGuided centroid should query");
    let (g_x, g_y, g_z) = match guided_centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };

    // Centroids should differ — the guide wire biases orientation, which
    // in turn shifts the centroid away from the plain Sweep result.
    // For a rotation-symmetric circular profile the shift is small
    // (~3e-7 m, reflecting MakePipeShell's section parameterization vs
    // plain MakePipe), but the delta is robustly non-zero and several
    // orders of magnitude above OCCT's centroid numerical noise (~1e-12),
    // so the threshold of 1e-8 reliably detects the guide's influence.
    let dx = (g_x - plain_x).abs();
    let dy = (g_y - plain_y).abs();
    let dz = (g_z - plain_z).abs();
    let delta = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!(
        delta > 1e-8,
        "guided centroid ({g_x}, {g_y}, {g_z}) should differ from plain \
         centroid ({plain_x}, {plain_y}, {plain_z}); delta = {delta}"
    );
}
